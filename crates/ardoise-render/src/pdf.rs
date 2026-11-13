//! PDF export (SPEC §4.6): one page per note, strokes as vector paths filled with the
//! non-zero rule ([`crate::look`]), opacity and blending through ExtGState (`/ca`, `/BM`), the
//! background as for SVG (paper, template shapes, texture as a JPEG image) and the background
//! image as an image XObject: a JPEG passed through as it is (`/DCTDecode`), other pixels
//! deflated, with a soft mask (`/SMask`) for their transparency.
//!
//! Page sizes: `Actual` is the note's own size (1 device unit = 72 / 2540 pt); `A4` is
//! 210 × 297 mm in the note's orientation, the note scaled to fit and centred (the margins
//! stay white). Pages larger than PDF's 14 400 units a side get a `/UserUnit`.
//!
//! Layers: merged (the default) paints every fill in one stream; separate puts each note
//! layer's fills in marked content tied to an optional content group (PDF 1.5), one group per
//! layer name over the whole document, so that a viewer can show or hide it.
//!
//! Outline (bookmarks): [`notes_to_pdf_with_outline`] writes the document outline of the viewer's
//! bookmarks panel from a flat list of [`OutlineEntry`] (title, depth, page), all entries open.

use std::collections::HashMap;

use pdf_writer::types::{BlendMode, PageMode};
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref, TextStr};

use crate::background::{Shape, VectorBackground};
use crate::look::{Blend, UNITS_PER_INCH};
use crate::picture::{DctColor, NoteView, PlacedPicture};
use crate::svg::TEXTURE_QUALITY;
use crate::vector::{LayerFills, Scene};
use crate::{LayerMode, RenderError};

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
    /// One optional content group per note layer, or one drawing.
    pub layers: LayerMode,
}

impl Default for PdfOptions {
    fn default() -> Self {
        Self {
            page: PdfPage::Actual,
            background: true,
            layers: LayerMode::Merged,
        }
    }
}

/// One bookmark of a PDF's outline: a title, how deep it is (0 = top level; an entry one level
/// deeper than the one before it is its child), and the 0-based page it goes to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutlineEntry {
    pub title: String,
    pub level: usize,
    pub page: usize,
}

/// A PDF document with one page per note, in order (`&Note`s, or [`NoteView`]s for notes
/// with a background image).
pub fn notes_to_pdf<'a, V>(notes: &[V], options: &PdfOptions) -> Result<Vec<u8>, RenderError>
where
    V: Into<NoteView<'a>> + Copy,
{
    notes_to_pdf_with_outline(notes, options, &[])
}

/// [`notes_to_pdf`] with an outline (the viewer's bookmarks): `outline` in reading order, each
/// entry's `level` at most one more than the entry before it (a deeper one is brought up to
/// that), the first at level 0. An entry whose page does not exist is left out. With no entry
/// the document has no outline.
pub fn notes_to_pdf_with_outline<'a, V>(
    notes: &[V],
    options: &PdfOptions,
    outline: &[OutlineEntry],
) -> Result<Vec<u8>, RenderError>
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
    let bookmarks = Bookmarks::of(outline, notes.len(), &mut next);
    let separate = options.layers == LayerMode::Separate;
    if !separate {
        let mut root = pdf.catalog(catalog);
        root.pages(tree);
        if let Some(bookmarks) = &bookmarks {
            root.outlines(bookmarks.root);
            root.page_mode(PageMode::UseOutlines);
        }
        root.finish();
    }
    if let Some(bookmarks) = &bookmarks {
        bookmarks.write(&mut pdf, &pages);
    }
    pdf.pages(tree).kids(pages.iter().copied()).count(count);
    let mut groups = Groups::default();
    for (&view, &page) in notes.iter().zip(&pages) {
        let groups = separate.then_some(&mut groups);
        write_page(
            &mut pdf,
            &mut next,
            tree,
            page,
            view.into(),
            options,
            groups,
        )?;
    }
    if separate {
        // The groups are known once every page is written, so the catalog comes last.
        write_groups(&mut pdf, &groups);
        let mut root = pdf.catalog(catalog);
        root.pages(tree);
        if let Some(bookmarks) = &bookmarks {
            root.outlines(bookmarks.root);
        }
        if !groups.ids.is_empty() {
            root.page_mode(PageMode::UseOC);
            let mut properties = root.insert(Name(b"OCProperties")).dict();
            properties
                .insert(Name(b"OCGs"))
                .array()
                .items(groups.ids.iter().copied());
            let mut config = properties.insert(Name(b"D")).dict();
            config.pair(Name(b"BaseState"), Name(b"ON"));
            config
                .insert(Name(b"Order"))
                .array()
                .items(groups.ids.iter().copied());
            config.finish();
            properties.finish();
        }
        root.finish();
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

/// The outline of a document, laid out as the tree the PDF wants (parents, siblings, children).
struct Bookmarks {
    root: Ref,
    /// Per entry, in order: its object, its title, its page, its parent (an entry's index; `None`
    /// for the top level), and its children's indexes.
    items: Vec<Bookmark>,
    top: Vec<usize>,
}

struct Bookmark {
    id: Ref,
    title: String,
    page: usize,
    parent: Option<usize>,
    children: Vec<usize>,
}

impl Bookmarks {
    /// The tree of `outline` over `pages` pages, with its object numbers taken from `next`;
    /// `None` when there is no entry to show.
    fn of(outline: &[OutlineEntry], pages: usize, next: &mut Ref) -> Option<Bookmarks> {
        let mut items: Vec<Bookmark> = Vec::new();
        let mut top = Vec::new();
        // The entries of the branch being built, from the top level down to the last one.
        let mut path: Vec<usize> = Vec::new();
        for entry in outline.iter().filter(|entry| entry.page < pages) {
            let depth = entry.level.min(path.len());
            path.truncate(depth);
            let parent = path.last().copied();
            let index = items.len();
            items.push(Bookmark {
                id: next.bump(),
                title: entry.title.clone(),
                page: entry.page,
                parent,
                children: Vec::new(),
            });
            match parent {
                Some(parent) => items[parent].children.push(index),
                None => top.push(index),
            }
            path.push(index);
        }
        (!items.is_empty()).then(|| Bookmarks {
            root: next.bump(),
            items,
            top,
        })
    }

    /// How many entries lie below `index`, at any depth (every entry is open).
    fn below(&self, index: usize) -> i32 {
        self.items[index]
            .children
            .iter()
            .map(|child| 1 + self.below(*child))
            .sum()
    }

    fn write(&self, pdf: &mut Pdf, pages: &[Ref]) {
        let (first, last) = (self.top[0], self.top[self.top.len() - 1]);
        let mut root = pdf.outline(self.root);
        root.first(self.items[first].id)
            .last(self.items[last].id)
            .count(i32::try_from(self.items.len()).unwrap_or(i32::MAX));
        root.finish();
        for (index, item) in self.items.iter().enumerate() {
            let siblings = match item.parent {
                Some(parent) => &self.items[parent].children,
                None => &self.top,
            };
            let at = siblings.iter().position(|s| *s == index).unwrap_or(0);
            let mut dict = pdf.outline_item(item.id);
            dict.title(TextStr(&item.title));
            dict.parent(item.parent.map_or(self.root, |p| self.items[p].id));
            if at > 0 {
                dict.prev(self.items[siblings[at - 1]].id);
            }
            if let Some(next) = siblings.get(at + 1) {
                dict.next(self.items[*next].id);
            }
            if let (Some(first), Some(last)) = (item.children.first(), item.children.last()) {
                dict.first(self.items[*first].id)
                    .last(self.items[*last].id)
                    .count(self.below(index));
            }
            dict.dest().page(pages[item.page]).fit();
            dict.finish();
        }
    }
}

/// The optional content groups of a document: one per distinct layer name, over all its pages,
/// in order of first appearance.
#[derive(Default)]
struct Groups {
    names: Vec<String>,
    ids: Vec<Ref>,
}

impl Groups {
    /// The number of the group named `name`, made when there is none yet.
    fn number(&mut self, name: &str, next: &mut Ref) -> usize {
        if let Some(k) = self.names.iter().position(|n| n == name) {
            return k;
        }
        self.names.push(name.to_owned());
        self.ids.push(next.bump());
        self.names.len() - 1
    }
}

/// The name a layer's group shows in a viewer's layers panel: the layer's name (its number when
/// the name is blank). `None` for strokes whose layer has no entry: they stay outside the groups.
fn group_name(layer: &LayerFills) -> Option<String> {
    let name = layer.name.as_deref()?;
    Some(if name.trim().is_empty() {
        (layer.index + 1).to_string()
    } else {
        name.to_owned()
    })
}

fn write_groups(pdf: &mut Pdf, groups: &Groups) {
    for (name, &id) in groups.names.iter().zip(&groups.ids) {
        let mut group = pdf.indirect(id).dict();
        group.pair(Name(b"Type"), Name(b"OCG"));
        group.pair(Name(b"Name"), TextStr(name));
        group.finish();
    }
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
    mut groups: Option<&mut Groups>,
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
    // The group of each layer of the page (separate layers). Numbered top layer first, the order
    // a viewer lists them in; the fills are still painted bottom first.
    let mut layer_groups: Vec<Option<usize>> = vec![None; scene.layers.len()];
    if let Some(groups) = groups.as_deref_mut() {
        for (layer, slot) in scene.layers.iter().zip(&mut layer_groups).rev() {
            *slot = group_name(layer).map(|name| groups.number(&name, next));
        }
    }
    for (layer, group) in scene.layers.iter().zip(&layer_groups) {
        // Marked content is not a saved state: what the painter tracks stays true.
        let tag = group.map(|k| format!("L{k}"));
        if let Some(tag) = &tag {
            painter
                .content
                .begin_marked_content_with_properties(Name(b"OC"))
                .properties_named(Name(tag.as_bytes()));
        }
        for fill in &layer.fills {
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
        if tag.is_some() {
            painter.content.end_marked_content();
        }
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
    if let Some(groups) = groups {
        let mut used: Vec<usize> = layer_groups.iter().flatten().copied().collect();
        used.sort_unstable();
        used.dedup();
        if !used.is_empty() {
            let mut properties = resources.insert(Name(b"Properties")).dict();
            for k in used {
                properties.pair(Name(format!("L{k}").as_bytes()), groups.ids[k]);
            }
            properties.finish();
        }
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

    fn entry(title: &str, level: usize, page: usize) -> OutlineEntry {
        OutlineEntry {
            title: title.to_owned(),
            level,
            page,
        }
    }

    #[test]
    fn the_outline_is_a_tree_of_the_levels_in_order() {
        let outline = [
            entry("A", 0, 0),
            entry("S", 0, 1),
            entry("S1", 1, 1),
            entry("S2", 1, 2),
            // Too deep (two levels below the entry before it): brought up to its child.
            entry("S2x", 3, 3),
            entry("B", 0, 4),
            // Past the last page: left out.
            entry("lost", 0, 9),
        ];
        let mut next = Ref::new(10);
        let tree = Bookmarks::of(&outline, 5, &mut next).unwrap();
        assert_eq!(tree.items.len(), 6);
        assert_eq!(tree.top, [0, 1, 5]);
        assert_eq!(tree.items[1].children, [2, 3]);
        assert_eq!(tree.items[3].children, [4]);
        assert_eq!(tree.items[4].parent, Some(3));
        assert_eq!(tree.below(1), 3);
        assert_eq!(tree.below(0), 0);
        // Seven objects were taken: six entries and the root.
        assert_eq!(next, Ref::new(17));
        assert!(Bookmarks::of(&[], 5, &mut Ref::new(1)).is_none());
        assert!(Bookmarks::of(&[entry("x", 0, 5)], 5, &mut Ref::new(1)).is_none());
    }

    #[test]
    fn groups_are_numbered_by_name_in_order_of_first_appearance() {
        let mut next = Ref::new(10);
        let mut groups = Groups::default();
        assert_eq!(groups.number("B", &mut next), 0);
        assert_eq!(groups.number("A", &mut next), 1);
        assert_eq!(groups.number("B", &mut next), 0);
        assert_eq!(groups.ids, [Ref::new(10), Ref::new(11)]);
        assert_eq!(next, Ref::new(12));
    }

    #[test]
    fn a_group_takes_the_layer_name_or_its_number_and_a_layer_without_entry_has_none() {
        let layer = |index, name: Option<&str>| LayerFills {
            index,
            name: name.map(str::to_owned),
            fills: vec![],
        };
        assert_eq!(
            group_name(&layer(0, Some("Calque 1"))).as_deref(),
            Some("Calque 1")
        );
        assert_eq!(group_name(&layer(2, Some(" "))).as_deref(), Some("3"));
        assert_eq!(group_name(&layer(9, None)), None);
    }
}
