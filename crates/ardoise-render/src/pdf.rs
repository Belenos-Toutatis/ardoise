//! PDF export (SPEC §4.6): one page per note, strokes as vector paths filled with the
//! non-zero rule ([`crate::look`]), opacity and blending through ExtGState (`/ca`, `/BM`), the
//! background as for SVG (paper, template shapes, texture as a JPEG image) and the background
//! image as an image XObject: a JPEG passed through as it is (`/DCTDecode`), other pixels
//! deflated, with a soft mask (`/SMask`) for their transparency.
//!
//! Page sizes: `Actual` is the note's own size (1 device unit = 72 / 2540 pt); `A4` is
//! 210 × 297 mm in the note's orientation, the note scaled to fit and centred (the margins
//! stay white). Pages larger than PDF's 14 400 units a side get a `/UserUnit`.

use std::collections::HashMap;

use pdf_writer::types::BlendMode;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, TextStr};

use crate::RenderError;
use crate::background::{Shape, VectorBackground};
use crate::look::{Blend, UNITS_PER_INCH};
use crate::picture::{DctColor, NoteView, PlacedPicture};
use crate::svg::TEXTURE_QUALITY;
use crate::vector::Scene;

/// Points per device unit.
const PT_PER_UNIT: f32 = 72.0 / UNITS_PER_INCH;
/// A4 portrait in points (210 × 297 mm).
const A4_PT: (f32, f32) = (210.0 / 25.4 * 72.0, 297.0 / 25.4 * 72.0);
/// Largest page side in user space units (PDF 32000-1, annex C.2).
const MAX_PAGE_SIDE: f32 = 14_400.0;
/// Bézier handle length of a quarter circle, relative to the radius.
const KAPPA: f32 = 0.552_284_8;
/// zlib level of the content streams.
const DEFLATE_LEVEL: u8 = 6;

/// Page size of a PDF export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PdfPage {
    /// The note's own size.
    #[default]
    Actual,
    /// 210 × 297 mm in the note's orientation, the note fitted and centred.
    A4,
}

/// How to write notes as PDF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfOptions {
    pub page: PdfPage,
    /// Draw the paper colour, template and texture; otherwise the page stays white.
    pub background: bool,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            page: PdfPage::Actual,
            background: true,
        }
    }
}

/// A PDF document with one page per note, in order (`&Note`s, or [`NoteView`]s for notes
/// with a background image).
pub fn notes_to_pdf<'a, V>(notes: &[V], options: &PdfOptions) -> Result<Vec<u8>, RenderError>
where
    V: Into<NoteView<'a>> + Copy,
{
    if notes.is_empty() {
        return Err(RenderError::Encode("no note to put in the PDF".into()));
    }
    let count = i32::try_from(notes.len())
        .map_err(|_| RenderError::Encode("too many notes for one PDF".into()))?;
    let mut pdf = Pdf::new();
    let mut next = Ref::new(1);
    let catalog = next.bump();
    let tree = next.bump();
    let info = next.bump();
    let pages: Vec<Ref> = notes.iter().map(|_| next.bump()).collect();
    pdf.catalog(catalog).pages(tree);
    pdf.pages(tree).kids(pages.iter().copied()).count(count);
    for (&view, &page) in notes.iter().zip(&pages) {
        write_page(&mut pdf, &mut next, tree, page, view.into(), options)?;
    }
    let mut document = pdf.document_info(info);
    document.producer(TextStr("Ardoise"));
    if let [view] = notes {
        let view: NoteView<'_> = (*view).into();
        let title = view.note.title.trim();
        if !title.is_empty() {
            document.title(TextStr(title));
        }
    }
    document.finish();
    Ok(pdf.finish())
}

/// Where the note goes on its page.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Placement {
    /// Page size in user space units.
    width: f32,
    height: f32,
    /// Size of a user space unit in points (`/UserUnit`), 1 normally.
    user_unit: f32,
    /// User space units per device unit.
    scale: f32,
    /// Offset of the note's top-left corner from the page's top-left corner.
    left: f32,
    top: f32,
}

impl Placement {
    fn of(width: f32, height: f32, page: PdfPage) -> Placement {
        let (page_w, page_h, scale) = match page {
            PdfPage::Actual => (width * PT_PER_UNIT, height * PT_PER_UNIT, PT_PER_UNIT),
            PdfPage::A4 => {
                let (w, h) = if width > height {
                    (A4_PT.1, A4_PT.0)
                } else {
                    A4_PT
                };
                (w, h, (w / width).min(h / height))
            }
        };
        let user_unit = (page_w.max(page_h) / MAX_PAGE_SIDE).ceil().max(1.0);
        let (page_w, page_h, scale) = (page_w / user_unit, page_h / user_unit, scale / user_unit);
        Placement {
            width: page_w,
            height: page_h,
            user_unit,
            scale,
            left: (page_w - width * scale) / 2.0,
            top: (page_h - height * scale) / 2.0,
        }
    }
}

/// Rounds page coordinates to 0.01 device unit (0.1 µm): shorter numbers, same drawing.
fn round(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}

/// The content stream of one page, keeping track of the graphics state it set.
struct Painter {
    content: Content,
    /// ExtGState names by (opacity bits, blend).
    states: HashMap<(u32, Blend), usize>,
    state: (u32, Blend),
    rgb: Option<[u8; 3]>,
}

impl Painter {
    fn new() -> Self {
        Self {
            content: Content::new(),
            states: HashMap::new(),
            state: (1.0f32.to_bits(), Blend::Normal),
            rgb: None,
        }
    }

    /// Sets the fill colour, opacity and blending for the next fill.
    fn style(&mut self, rgb: [u8; 3], opacity: f32, blend: Blend) {
        self.alpha(opacity, blend);
        if self.rgb != Some(rgb) {
            let [r, g, b] = rgb.map(|c| f32::from(c) / 255.0);
            self.content.set_fill_rgb(r, g, b);
            self.rgb = Some(rgb);
        }
    }

    /// Sets the opacity and blending (of fills and images) for what is painted next.
    fn alpha(&mut self, opacity: f32, blend: Blend) {
        let opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let key = (opacity.to_bits(), blend);
        if key != self.state {
            let next = self.states.len();
            let index = *self.states.entry(key).or_insert(next);
            self.content
                .set_parameters(Name(format!("G{index}").as_bytes()));
            self.state = key;
        }
    }

    /// The background image (the XObject `/Im1`), with its opacity, at its place. The page is
    /// already clipped to the note.
    fn picture(&mut self, placed: &PlacedPicture<'_>) {
        // Outside the saved state, so that the state this painter tracks stays the real one.
        self.alpha(placed.opacity, Blend::Normal);
        let (w, h) = (
            placed.picture.width() as f32,
            placed.picture.height() as f32,
        );
        let [a, b, c, d, e, f] = placed.matrix;
        // An image fills the unit square with its first row at the top: (s, t) is the pixel
        // (s w, (1 - t) h), which the image's own map then puts on the page.
        self.content.save_state();
        self.content
            .transform([a * w, b * w, -c * h, -d * h, c * h + e, d * h + f]);
        self.content.x_object(Name(b"Im1"));
        self.content.restore_state();
    }

    fn shape(&mut self, shape: Shape) {
        match shape {
            Shape::Rect {
                x,
                y,
                width,
                height,
            } => {
                self.content
                    .rect(round(x), round(y), round(width), round(height));
            }
            Shape::Disc { cx, cy, r } => {
                let k = KAPPA * r;
                let c = &mut self.content;
                c.move_to(round(cx + r), round(cy));
                c.cubic_to(
                    round(cx + r),
                    round(cy + k),
                    round(cx + k),
                    round(cy + r),
                    round(cx),
                    round(cy + r),
                );
                c.cubic_to(
                    round(cx - k),
                    round(cy + r),
                    round(cx - r),
                    round(cy + k),
                    round(cx - r),
                    round(cy),
                );
                c.cubic_to(
                    round(cx - r),
                    round(cy - k),
                    round(cx - k),
                    round(cy - r),
                    round(cx),
                    round(cy - r),
                );
                c.cubic_to(
                    round(cx + k),
                    round(cy - r),
                    round(cx + r),
                    round(cy - k),
                    round(cx + r),
                    round(cy),
                );
                c.close_path();
            }
        }
        self.content.fill_nonzero();
    }

    fn background(&mut self, background: &VectorBackground, width: f32, height: f32) {
        let paper = background.paper;
        self.style([paper.r, paper.g, paper.b], 1.0, Blend::Normal);
        self.shape(Shape::Rect {
            x: 0.0,
            y: 0.0,
            width,
            height,
        });
        if background.texture.is_some() {
            // The image's first row is at the top: map the unit square upside down.
            self.content.save_state();
            self.content
                .transform([width, 0.0, 0.0, -height, 0.0, height]);
            self.content.x_object(Name(b"Im0"));
            self.content.restore_state();
        }
        for mark in &background.marks {
            self.style(mark.rgb, mark.alpha, Blend::Normal);
            self.shape(mark.shape);
        }
    }
}

fn write_page(
    pdf: &mut Pdf,
    next: &mut Ref,
    tree: Ref,
    page_id: Ref,
    view: NoteView<'_>,
    options: &PdfOptions,
) -> Result<(), RenderError> {
    let (width, height) = (view.note.page.width, view.note.page.height);
    if width == 0 || height == 0 {
        return Err(RenderError::Size { width, height });
    }
    let scene = Scene::of(view, options.background);
    let place = Placement::of(scene.width, scene.height, options.page);

    let mut painter = Painter::new();
    painter.content.save_state();
    // Device units, origin at the note's top-left corner, y down.
    painter.content.transform([
        place.scale,
        0.0,
        0.0,
        -place.scale,
        place.left,
        place.height - place.top,
    ]);
    painter.content.rect(0.0, 0.0, scene.width, scene.height);
    painter.content.clip_nonzero();
    painter.content.end_path();
    if let Some(background) = &scene.background {
        painter.background(background, scene.width, scene.height);
    }
    if let Some(placed) = &scene.picture {
        painter.picture(placed);
    }
    for fill in scene.fills() {
        painter.style(fill.rgb, fill.flat_opacity(), fill.blend);
        for contour in &fill.outline.contours {
            let mut points = contour.iter();
            let Some(&[x, y]) = points.next() else {
                continue;
            };
            painter.content.move_to(round(x), round(y));
            for &[x, y] in points {
                painter.content.line_to(round(x), round(y));
            }
            painter.content.close_path();
        }
        painter.content.fill_nonzero();
    }
    painter.content.restore_state();

    let content_id = next.bump();
    let stream = miniz_oxide::deflate::compress_to_vec_zlib(
        painter.content.finish().as_slice(),
        DEFLATE_LEVEL,
    );
    pdf.stream(content_id, &stream).filter(Filter::FlateDecode);

    let mut states: Vec<((u32, Blend), usize)> = painter.states.into_iter().collect();
    states.sort_by_key(|&(_, index)| index);
    let mut state_ids = Vec::with_capacity(states.len());
    for ((opacity, blend), index) in states {
        let id = next.bump();
        let mut state = pdf.ext_graphics(id);
        state.non_stroking_alpha(f32::from_bits(opacity));
        state.blend_mode(match blend {
            Blend::Normal => BlendMode::Normal,
            Blend::Multiply => BlendMode::Multiply,
            Blend::Screen => BlendMode::Screen,
        });
        state.finish();
        state_ids.push((format!("G{index}"), id));
    }

    let texture = match scene.background.as_ref().and_then(|b| b.texture.as_ref()) {
        Some(texture) => {
            let jpeg = crate::jpeg::encode_image(texture, TEXTURE_QUALITY, None)?;
            let id = next.bump();
            let side = |v: u32| {
                i32::try_from(v).map_err(|_| RenderError::Size {
                    width: texture.width,
                    height: texture.height,
                })
            };
            let mut image = pdf.image_xobject(id, &jpeg);
            image.filter(Filter::DctDecode);
            image.width(side(texture.width)?);
            image.height(side(texture.height)?);
            image.color_space().device_rgb();
            image.bits_per_component(8);
            image.finish();
            Some(id)
        }
        None => None,
    };
    let picture = match &scene.picture {
        Some(placed) => Some(write_picture(pdf, next, placed)?),
        None => None,
    };

    let mut page = pdf.page(page_id);
    page.parent(tree)
        .media_box(Rect::new(0.0, 0.0, place.width, place.height))
        .contents(content_id);
    if place.user_unit > 1.0 {
        page.user_unit(place.user_unit);
    }
    page.group().transparency().color_space().device_rgb();
    let mut resources = page.resources();
    if !state_ids.is_empty() {
        let mut dict = resources.ext_g_states();
        for (name, id) in &state_ids {
            dict.pair(Name(name.as_bytes()), *id);
        }
        dict.finish();
    }
    if texture.is_some() || picture.is_some() {
        let mut objects = resources.x_objects();
        if let Some(id) = texture {
            objects.pair(Name(b"Im0"), id);
        }
        if let Some(id) = picture {
            objects.pair(Name(b"Im1"), id);
        }
        objects.finish();
    }
    resources.finish();
    page.finish();
    Ok(())
}

/// Writes the background image's XObject (and its soft mask); returns its reference. A JPEG
/// in grey or colour goes in as it is; anything else as deflated RGB samples, with an 8-bit
/// soft mask when some pixels are not opaque.
fn write_picture(
    pdf: &mut Pdf,
    next: &mut Ref,
    placed: &PlacedPicture<'_>,
) -> Result<Ref, RenderError> {
    let picture = placed.picture;
    let (width, height) = (picture.width(), picture.height());
    let side = |v: u32| i32::try_from(v).map_err(|_| RenderError::Size { width, height });
    let (w, h) = (side(width)?, side(height)?);
    let id = next.bump();
    if let Some(color) = picture.dct() {
        let mut image = pdf.image_xobject(id, picture.bytes());
        image.filter(Filter::DctDecode);
        image.width(w);
        image.height(h);
        match color {
            DctColor::Gray => image.color_space().device_gray(),
            DctColor::Rgb => image.color_space().device_rgb(),
        }
        image.bits_per_component(8);
        image.interpolate(true);
        image.finish();
        return Ok(id);
    }
    let pixels = picture.image().pixels.as_chunks::<4>().0;
    let rgb: Vec<u8> = pixels.iter().flat_map(|p| [p[0], p[1], p[2]]).collect();
    let mask = if pixels.iter().any(|p| p[3] != 255) {
        let alpha: Vec<u8> = pixels.iter().map(|p| p[3]).collect();
        let mask_id = next.bump();
        let data = miniz_oxide::deflate::compress_to_vec_zlib(&alpha, DEFLATE_LEVEL);
        let mut mask = pdf.image_xobject(mask_id, &data);
        mask.filter(Filter::FlateDecode);
        mask.width(w);
        mask.height(h);
        mask.color_space().device_gray();
        mask.bits_per_component(8);
        mask.interpolate(true);
        mask.finish();
        Some(mask_id)
    } else {
        None
    };
    let data = miniz_oxide::deflate::compress_to_vec_zlib(&rgb, DEFLATE_LEVEL);
    let mut image = pdf.image_xobject(id, &data);
    image.filter(Filter::FlateDecode);
    image.width(w);
    image.height(h);
    image.color_space().device_rgb();
    image.bits_per_component(8);
    image.interpolate(true);
    if let Some(mask_id) = mask {
        image.s_mask(mask_id);
    }
    image.finish();
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actual_pages_have_the_note_size() {
        let place = Placement::of(29_700.0, 21_600.0, PdfPage::Actual);
        // 297 × 216 mm.
        assert!((place.width - 841.89).abs() < 0.01, "{place:?}");
        assert!((place.height - 612.28).abs() < 0.01, "{place:?}");
        assert_eq!(place.user_unit, 1.0);
        assert_eq!((place.left, place.top), (0.0, 0.0));
    }

    #[test]
    fn a4_pages_follow_the_note_orientation_and_centre_it() {
        let landscape = Placement::of(29_700.0, 21_600.0, PdfPage::A4);
        assert!((landscape.width - 841.89).abs() < 0.01);
        assert!((landscape.height - 595.28).abs() < 0.01);
        // 216 mm high in a 210 mm page: scaled down, centred horizontally.
        assert!((landscape.scale * 21_600.0 - landscape.height).abs() < 0.01);
        assert!(landscape.left > 0.0 && landscape.top.abs() < 0.01);

        let portrait = Placement::of(14_800.0, 21_000.0, PdfPage::A4);
        assert!((portrait.width - 595.28).abs() < 0.01);
        assert!((portrait.height - 841.89).abs() < 0.01);
        assert!(portrait.scale * 14_800.0 <= portrait.width + 0.01);
    }

    #[test]
    fn huge_pages_get_a_user_unit() {
        // 10 m long: 28 346 pt, above 14 400.
        let place = Placement::of(29_700.0, 1_000_000.0, PdfPage::Actual);
        assert_eq!(place.user_unit, 2.0);
        assert!(place.height <= MAX_PAGE_SIDE);
        assert!((place.height * place.user_unit - 1_000_000.0 * PT_PER_UNIT).abs() < 1.0);
    }

    #[test]
    fn no_note_no_pdf() {
        assert!(matches!(
            notes_to_pdf::<NoteView<'_>>(&[], &PdfOptions::default()),
            Err(RenderError::Encode(_))
        ));
    }
}
