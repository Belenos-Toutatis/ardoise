//! Rendering and export of notes: ink tools, page backgrounds, templates and textures; PNG,
//! JPG, SVG, PDF and the clipboard image (SPEC §4.6, `docs/design/exports-and-operations.md`).
//!
//! The public surface below is the contract used by the Inkspace import comparison, the
//! exports and the thumbnails. Every format draws the same thing (SPEC §6.2):
//!
//! 1. the background ([`background`]): paper colour, template in millimetres, optional paper
//!    texture; or plain white (transparent for [`render_note_transparent`], SVG and PDF) when
//!    the background is off;
//! 2. the strokes of visible layers, bottom layer first and in note order within a layer
//!    ([`drawing_order`]), each filled from [`ardoise_ink::outline`] with its tool's look
//!    ([`look`]): with tiny-skia and anti-aliasing for images ([`render_note`], [`ink`]), as
//!    paths for [`note_to_svg`] and [`notes_to_pdf`].
//!
//! [`PageCanvas`] is the raster drawing itself, one stroke at a time: [`render_note`] is built
//! on it, and the replay frames (`ardoise-media`) draw a page incrementally with it.
//!
//! Rendering is deterministic: every texture comes from a fixed or content-derived seed, so
//! the same note and options always give the same bytes (the reference images rely on it).

mod background;
mod canvas;
mod clipboard;
mod ink;
mod jpeg;
pub mod look;
mod noise;
mod pdf;
mod svg;
mod vector;

use ardoise_core::note::{Note, PageSize};
use ardoise_ink::Stroke;
use tiny_skia::Pixmap;

pub use background::paper_color;
pub use canvas::{Backdrop, PageCanvas, PixelRect, PreparedStroke, image_size};
pub use clipboard::{ClipboardImage, clipboard_image, clipboard_image_at};
pub use look::{Blend, Fill, Grain, Look};
pub use pdf::{PdfOptions, PdfPage, notes_to_pdf};
pub use svg::{SvgOptions, note_to_svg};

/// Smallest and largest resolution of the image exports, in dots per inch.
pub const MIN_DPI: u32 = 72;
pub const MAX_DPI: u32 = 600;

/// Pixels per device unit (10 µm) at `dpi` dots per inch.
pub fn scale_for_dpi(dpi: u32) -> f32 {
    dpi as f32 / look::UNITS_PER_INCH
}

/// Largest image side, in pixels.
const MAX_SIDE: u32 = 16_384;

/// How to render a note.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RenderOptions {
    /// Pixels per device unit (10 µm). 0.04 gives 1 px = 0.25 mm (Inkspace's previews).
    pub scale: f32,
    /// Draw the paper colour, template and texture; otherwise a plain white page.
    pub background: bool,
}

impl RenderOptions {
    /// Scale so that the page is `width_px` pixels wide.
    pub fn fit_width(page: PageSize, width_px: u32) -> Self {
        Self {
            scale: width_px as f32 / page.width.max(1) as f32,
            background: true,
        }
    }

    /// Scale for a resolution of `dpi` dots per inch.
    pub fn at_dpi(dpi: u32, background: bool) -> Self {
        Self {
            scale: scale_for_dpi(dpi),
            background,
        }
    }
}

/// An RGBA image, 8 bits per channel, straight alpha, rows top to bottom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl Image {
    /// The RGBA value at `(x, y)`, or `None` outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let i = (y as usize * self.width as usize + x as usize) * 4;
        let p = self.pixels.get(i..i + 4)?;
        Some([p[0], p[1], p[2], p[3]])
    }
}

/// Errors while rendering or encoding.
#[derive(Debug, thiserror::Error)]
pub enum RenderError {
    #[error("the image would be empty or too large ({width} x {height} px)")]
    Size { width: u32, height: u32 },
    #[error("cannot encode the image: {0}")]
    Encode(String),
    #[error("cannot decode the image: {0}")]
    Decode(String),
    /// A page written into a frame buffer that cannot hold it ([`PageCanvas::write_rgba`]).
    #[error("cannot place the page in the frame: {0}")]
    Frame(String),
}

/// Renders `note` to an image of `page × scale` pixels (each side rounded, at most
/// 16 384 px). Strokes on hidden layers are not drawn; a stroke whose layer index has no
/// layer entry is drawn (above the others) rather than lost.
pub fn render_note(note: &Note, options: &RenderOptions) -> Result<Image, RenderError> {
    let backdrop = if options.background {
        Backdrop::Paper
    } else {
        Backdrop::White
    };
    render(note, options.scale, backdrop)
}

/// Renders `note` like [`render_note`] without any paper: the strokes on a transparent page,
/// alpha kept (PNG with a transparent background).
pub fn render_note_transparent(note: &Note, scale: f32) -> Result<Image, RenderError> {
    render(note, scale, Backdrop::Transparent)
}

fn render(note: &Note, scale: f32, backdrop: Backdrop) -> Result<Image, RenderError> {
    let mut canvas = PageCanvas::new(note, scale, backdrop)?;
    for stroke in visible_strokes(note) {
        canvas.draw_stroke(stroke);
    }
    Ok(canvas.to_image())
}

/// Indices into `note.strokes` of the strokes every format draws, in drawing order: visible
/// layers only (a stroke whose layer index has no layer entry counts as visible), bottom
/// layer first, in note order within a layer.
pub fn drawing_order(note: &Note) -> Vec<usize> {
    let mut order: Vec<(u32, usize)> = note
        .strokes
        .iter()
        .enumerate()
        .filter(|(_, s)| {
            note.layers
                .get(s.layer as usize)
                .is_none_or(|layer| layer.visible)
        })
        .map(|(i, s)| (s.layer, i))
        .collect();
    // Stable: note order within a layer.
    order.sort_by_key(|&(layer, _)| layer);
    order.into_iter().map(|(_, i)| i).collect()
}

/// Strokes to draw, in [`drawing_order`].
fn visible_strokes(note: &Note) -> Vec<&Stroke> {
    drawing_order(note)
        .into_iter()
        .filter_map(|i| note.strokes.get(i))
        .collect()
}

/// Straight-alpha RGBA copy of a (premultiplied) pixmap.
fn image_from(pixmap: &Pixmap) -> Image {
    let mut pixels = Vec::with_capacity(pixmap.data().len());
    for p in pixmap.pixels() {
        let c = p.demultiply();
        pixels.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Image {
        width: pixmap.width(),
        height: pixmap.height(),
        pixels,
    }
}

/// Checks that `image` is not empty and holds exactly `width × height` RGBA pixels.
pub(crate) fn check_pixels(image: &Image) -> Result<(), RenderError> {
    let (width, height) = (image.width, image.height);
    if width == 0 || height == 0 {
        return Err(RenderError::Size { width, height });
    }
    let expected = width as usize * height as usize * 4;
    if image.pixels.len() != expected {
        return Err(RenderError::Encode(format!(
            "{} bytes of pixels for {width} x {height} px (expected {expected})",
            image.pixels.len()
        )));
    }
    Ok(())
}

/// A straight-alpha RGBA pixel composited over white.
pub(crate) fn over_white([r, g, b, a]: [u8; 4]) -> [u8; 3] {
    let a = u16::from(a);
    let mix = |v: u8| ((u16::from(v) * a + 255 * (255 - a) + 127) / 255) as u8;
    [mix(r), mix(g), mix(b)]
}

/// `dpi` in pixels per metre (PNG `pHYs`, BMP headers).
pub(crate) fn pixels_per_metre(dpi: u32) -> u32 {
    (f64::from(dpi) / 0.0254).round() as u32
}

/// PNG bytes of `image` (8-bit RGB when every pixel is opaque, which rendered notes are;
/// RGBA otherwise).
pub fn encode_png(image: &Image) -> Result<Vec<u8>, RenderError> {
    encode_png_at(image, None)
}

/// Like [`encode_png`], recording a resolution of `dpi` (a `pHYs` chunk), so that programs
/// that honour it show the note at its real size.
pub fn encode_png_at(image: &Image, dpi: Option<u32>) -> Result<Vec<u8>, RenderError> {
    check_pixels(image)?;
    let (width, height) = (image.width, image.height);
    let opaque = image.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    let encode_error = |e: png::EncodingError| RenderError::Encode(e.to_string());
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Balanced);
    if let Some(dpi) = dpi.filter(|&d| d > 0) {
        let ppm = pixels_per_metre(dpi);
        encoder.set_pixel_dims(Some(png::PixelDimensions {
            xppu: ppm,
            yppu: ppm,
            unit: png::Unit::Meter,
        }));
    }
    let rgb: Vec<u8>;
    let data = if opaque {
        encoder.set_color(png::ColorType::Rgb);
        rgb = image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        &rgb
    } else {
        encoder.set_color(png::ColorType::Rgba);
        &image.pixels
    };
    let mut writer = encoder.write_header().map_err(encode_error)?;
    writer.write_image_data(data).map_err(encode_error)?;
    writer.finish().map_err(encode_error)?;
    Ok(out)
}

/// JPEG bytes of `image`: baseline, 8-bit, YCbCr (JFIF), `quality` 1..=100 (clamped), alpha
/// flattened over white.
pub fn encode_jpeg(image: &Image, quality: u8) -> Result<Vec<u8>, RenderError> {
    jpeg::encode_image(image, quality, None)
}

/// Like [`encode_jpeg`], recording a resolution of `dpi` in the JFIF header.
pub fn encode_jpeg_at(
    image: &Image,
    quality: u8,
    dpi: Option<u32>,
) -> Result<Vec<u8>, RenderError> {
    jpeg::encode_image(image, quality, dpi)
}

/// Decodes a PNG file into an RGBA image (any colour type and bit depth is converted to
/// 8-bit RGBA). For reference images and Inkspace's previews.
pub fn decode_png(bytes: &[u8]) -> Result<Image, RenderError> {
    let decode_error = |e: png::DecodingError| RenderError::Decode(e.to_string());
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(decode_error)?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| RenderError::Decode("image too large".into()))?;
    let mut buffer = vec![0; size];
    let info = reader.next_frame(&mut buffer).map_err(decode_error)?;
    let data = buffer.get(..info.buffer_size()).unwrap_or_default();
    if info.bit_depth != png::BitDepth::Eight {
        return Err(RenderError::Decode(format!(
            "unexpected bit depth {:?}",
            info.bit_depth
        )));
    }
    let pixels: Vec<u8> = match info.color_type {
        png::ColorType::Rgba => data.to_vec(),
        png::ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => data
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        png::ColorType::Indexed => {
            return Err(RenderError::Decode("unexpanded palette".into()));
        }
    };
    let (width, height) = (info.width, info.height);
    if pixels.len() != width as usize * height as usize * 4 {
        return Err(RenderError::Decode("truncated image data".into()));
    }
    Ok(Image {
        width,
        height,
        pixels,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Origin, Paper, Template, Timing};
    use ardoise_ink::{Point, Rgba, Tool};

    fn note(page: PageSize, background: Background, strokes: Vec<Stroke>) -> Note {
        Note {
            id: "00000000-0000-4000-8000-000000000000".into(),
            title: "test".into(),
            created: 0,
            modified: 0,
            page,
            background,
            background_image: None,
            tags: vec![],
            favorite: false,
            layers: vec![
                Layer {
                    name: "1".into(),
                    visible: true,
                },
                Layer {
                    name: "2".into(),
                    visible: false,
                },
            ],
            strokes,
            timing: Timing::Recorded,
            origin: Origin::Ardoise,
        }
    }

    fn line(
        tool: Tool,
        color: Rgba,
        width: f32,
        layer: u32,
        from: (f32, f32),
        to: (f32, f32),
    ) -> Stroke {
        let points = (0..=20)
            .map(|i| {
                let t = i as f32 / 20.0;
                Point {
                    x: from.0 + (to.0 - from.0) * t,
                    y: from.1 + (to.1 - from.1) * t,
                    pressure: 1.0,
                    t: i,
                }
            })
            .collect();
        Stroke {
            tool,
            color,
            width,
            layer,
            points,
        }
    }

    const SMALL: PageSize = PageSize {
        width: 5000,
        height: 4000,
    };

    fn opts(scale: f32) -> RenderOptions {
        RenderOptions {
            scale,
            background: true,
        }
    }

    fn bg(paper: Paper, template: Template, texture: bool) -> Background {
        Background {
            paper,
            template,
            texture,
        }
    }

    #[test]
    fn size_is_page_times_scale_and_bad_sizes_are_refused() {
        let n = note(A4_LANDSCAPE, Background::default(), vec![]);
        let image = render_note(&n, &opts(0.04)).unwrap();
        assert_eq!((image.width, image.height), (1188, 864));
        assert_eq!(image.pixels.len(), 1188 * 864 * 4);
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, 1.0] {
            assert!(
                matches!(render_note(&n, &opts(scale)), Err(RenderError::Size { .. })),
                "{scale}"
            );
        }
        let fit = RenderOptions::fit_width(A4_LANDSCAPE, 1920);
        assert_eq!(render_note(&n, &fit).unwrap().width, 1920);
    }

    #[test]
    fn papers_fill_the_page_and_background_off_is_white() {
        for paper in [
            Paper::White,
            Paper::Cream,
            Paper::Kraft,
            Paper::Slate,
            Paper::Custom(Rgba::rgb(200, 220, 240)),
        ] {
            let n = note(SMALL, bg(paper, Template::Blank, false), vec![]);
            let image = render_note(&n, &opts(0.05)).unwrap();
            let c = paper_color(paper);
            assert_eq!(image.pixel(10, 10), Some([c.r, c.g, c.b, 255]), "{paper:?}");
            let plain = RenderOptions {
                background: false,
                ..opts(0.05)
            };
            let image = render_note(&n, &plain).unwrap();
            assert!(image.pixels.iter().all(|&v| v == 255), "{paper:?}");
        }
    }

    #[test]
    fn templates_draw_lines_and_blank_draws_none() {
        let paper = paper_color(Paper::White);
        let count_marked = |template| {
            let n = note(SMALL, bg(Paper::White, template, false), vec![]);
            let image = render_note(&n, &opts(0.05)).unwrap();
            image
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[..3] != [paper.r, paper.g, paper.b])
                .count()
        };
        assert_eq!(count_marked(Template::Blank), 0);
        for template in [
            Template::Lined,
            Template::Seyes,
            Template::Grid5mm,
            Template::Millimetric,
            Template::Dotted,
            Template::Music,
        ] {
            assert!(count_marked(template) > 100, "{template:?}");
        }
    }

    #[test]
    fn hidden_layers_are_not_drawn() {
        let visible = line(
            Tool::Ballpoint,
            Rgba::BLACK,
            60.0,
            0,
            (500.0, 500.0),
            (4500.0, 500.0),
        );
        let hidden = line(
            Tool::Ballpoint,
            Rgba::BLACK,
            60.0,
            1,
            (500.0, 2000.0),
            (4500.0, 2000.0),
        );
        let with = note(SMALL, Background::default(), vec![visible.clone(), hidden]);
        let without = note(SMALL, Background::default(), vec![visible]);
        assert_eq!(
            render_note(&with, &opts(0.05)).unwrap(),
            render_note(&without, &opts(0.05)).unwrap()
        );
        // A stroke on a layer that does not exist is still drawn.
        let orphan = line(
            Tool::Ballpoint,
            Rgba::BLACK,
            60.0,
            7,
            (500.0, 2000.0),
            (4500.0, 2000.0),
        );
        let n = note(SMALL, Background::default(), vec![orphan]);
        let image = render_note(&n, &opts(0.05)).unwrap();
        assert_eq!(image.pixel(125, 100), Some([0, 0, 0, 255]));
    }

    #[test]
    fn ballpoint_line_is_as_wide_as_width_at() {
        // A horizontal line 100 units wide at 0.1 px/unit: 10 px of ink across.
        let s = line(
            Tool::Ballpoint,
            Rgba::BLACK,
            100.0,
            0,
            (500.0, 2000.0),
            (4500.0, 2000.0),
        );
        let n = note(SMALL, Background::default(), vec![s]);
        let image = render_note(&n, &opts(0.1)).unwrap();
        let coverage: f32 = (0..image.height)
            .map(|y| 1.0 - f32::from(image.pixel(250, y).unwrap()[0]) / 255.0)
            .sum();
        assert!((coverage - 10.0).abs() < 0.3, "{coverage} px");
    }

    #[test]
    fn highlighter_colours_paper_and_keeps_ink_dark() {
        let ink = line(
            Tool::Ballpoint,
            Rgba::BLACK,
            80.0,
            0,
            (500.0, 2000.0),
            (4500.0, 2000.0),
        );
        let marker = line(
            Tool::Highlighter,
            Rgba::rgb(255, 230, 0),
            500.0,
            0,
            (2500.0, 1000.0),
            (2500.0, 3000.0),
        );
        let n = note(SMALL, Background::default(), vec![ink, marker]);
        let image = render_note(&n, &opts(0.05)).unwrap();
        assert_eq!(
            image.pixel(125, 100),
            Some([0, 0, 0, 255]),
            "ink under the marker"
        );
        assert_eq!(
            image.pixel(125, 70),
            Some([255, 230, 0, 255]),
            "paper under the marker"
        );
        assert_eq!(
            image.pixel(50, 70),
            Some([255, 255, 255, 255]),
            "paper elsewhere"
        );
    }

    #[test]
    fn rendering_is_deterministic() {
        let strokes: Vec<Stroke> = Tool::ALL
            .iter()
            .enumerate()
            .map(|(i, &tool)| {
                let y = 400.0 + i as f32 * 500.0;
                line(
                    tool,
                    Rgba::rgb(40, 60, 160),
                    tool.default_width(),
                    0,
                    (300.0, y),
                    (4700.0, y + 200.0),
                )
            })
            .collect();
        let n = note(SMALL, bg(Paper::Kraft, Template::Seyes, true), strokes);
        let a = render_note(&n, &opts(0.08)).unwrap();
        let b = render_note(&n, &opts(0.08)).unwrap();
        assert_eq!(a, b);
        assert_eq!(encode_png(&a).unwrap(), encode_png(&b).unwrap());
    }

    #[test]
    fn degenerate_strokes_do_not_break_rendering() {
        let mut strokes = vec![
            line(
                Tool::Ballpoint,
                Rgba::BLACK,
                50.0,
                0,
                (-9.0e8, -9.0e8),
                (9.0e8, 9.0e8),
            ),
            line(
                Tool::Chalk,
                Rgba::WHITE,
                f32::NAN,
                0,
                (100.0, 100.0),
                (100.0, 100.0),
            ),
            line(
                Tool::Pencil,
                Rgba::BLACK,
                1e30,
                0,
                (2000.0, 2000.0),
                (2100.0, 2000.0),
            ),
        ];
        let mut nan = line(
            Tool::FeltTip,
            Rgba::BLACK,
            50.0,
            0,
            (0.0, 0.0),
            (10.0, 10.0),
        );
        nan.points[3].x = f32::NAN;
        strokes.push(nan);
        strokes.push(Stroke {
            points: vec![],
            ..strokes[0].clone()
        });
        let n = note(SMALL, Background::default(), strokes);
        let image = render_note(&n, &opts(0.05)).unwrap();
        assert_eq!(image.width, 250);
    }

    #[test]
    fn png_round_trip() {
        let s = line(
            Tool::FeltTip,
            Rgba::rgb(200, 30, 60),
            150.0,
            0,
            (500.0, 500.0),
            (4500.0, 3500.0),
        );
        let n = note(SMALL, bg(Paper::Cream, Template::Grid5mm, true), vec![s]);
        let image = render_note(&n, &opts(0.05)).unwrap();
        let bytes = encode_png(&image).unwrap();
        assert_eq!(&bytes[1..4], b"PNG");
        assert_eq!(decode_png(&bytes).unwrap(), image);
        // Translucent images keep their alpha.
        let translucent = Image {
            width: 2,
            height: 1,
            pixels: vec![1, 2, 3, 4, 5, 6, 7, 255],
        };
        let back = decode_png(&encode_png(&translucent).unwrap()).unwrap();
        assert_eq!(back, translucent);
        // Inconsistent or empty images are refused.
        let bad = Image {
            width: 2,
            height: 2,
            pixels: vec![0; 3],
        };
        assert!(matches!(encode_png(&bad), Err(RenderError::Encode(_))));
        let empty = Image {
            width: 0,
            height: 5,
            pixels: vec![],
        };
        assert!(matches!(encode_png(&empty), Err(RenderError::Size { .. })));
        assert!(matches!(
            decode_png(b"not a png"),
            Err(RenderError::Decode(_))
        ));
    }

    #[test]
    fn png_records_the_resolution() {
        let image = Image {
            width: 1,
            height: 1,
            pixels: vec![1, 2, 3, 255],
        };
        let bytes = encode_png_at(&image, Some(300)).unwrap();
        let reader = png::Decoder::new(std::io::Cursor::new(&bytes))
            .read_info()
            .unwrap();
        let dims = reader.info().pixel_dims.unwrap();
        assert_eq!((dims.xppu, dims.yppu), (11_811, 11_811));
        assert_eq!(dims.unit, png::Unit::Meter);
        let plain = encode_png(&image).unwrap();
        let reader = png::Decoder::new(std::io::Cursor::new(&plain))
            .read_info()
            .unwrap();
        assert!(reader.info().pixel_dims.is_none());
        assert_eq!(scale_for_dpi(254), 0.1);
        assert_eq!(RenderOptions::at_dpi(254, false).scale, 0.1);
    }

    #[test]
    fn the_transparent_render_keeps_only_the_ink() {
        let ink = line(
            Tool::Ballpoint,
            Rgba::BLACK,
            100.0,
            0,
            (500.0, 2000.0),
            (4500.0, 2000.0),
        );
        let marker = line(
            Tool::Highlighter,
            Rgba::rgb(255, 230, 0),
            500.0,
            0,
            (2500.0, 1000.0),
            (2500.0, 3000.0),
        );
        let n = note(
            SMALL,
            bg(Paper::Slate, Template::Seyes, true),
            vec![ink, marker],
        );
        let image = render_note_transparent(&n, 0.05).unwrap();
        assert_eq!((image.width, image.height), (250, 200));
        // No paper anywhere: away from the ink, fully transparent.
        assert_eq!(image.pixel(10, 10), Some([0, 0, 0, 0]));
        assert_eq!(image.pixel(50, 100), Some([0, 0, 0, 255]));
        // The highlighter alone shows its colour (it multiplies nothing).
        assert_eq!(image.pixel(125, 70), Some([255, 230, 0, 255]));
        let opaque = image
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[3] == 255);
        assert!(opaque.count() < 250 * 200 / 4);
        assert!(render_note_transparent(&n, 0.0).is_err());
    }

    /// Mean absolute difference per channel between a JPEG and the image it encodes.
    fn jpeg_error(image: &Image, quality: u8) -> f64 {
        let bytes = encode_jpeg(image, quality).unwrap();
        let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(&bytes));
        let decoded = decoder.decode().unwrap();
        let info = decoder.info().unwrap();
        assert_eq!(
            (u32::from(info.width), u32::from(info.height)),
            (image.width, image.height)
        );
        assert_eq!(info.pixel_format, jpeg_decoder::PixelFormat::RGB24);
        let total: u64 = decoded
            .as_chunks::<3>()
            .0
            .iter()
            .zip(image.pixels.as_chunks::<4>().0)
            .map(|(d, p)| {
                let o = over_white(*p);
                (0..3).map(|c| u64::from(d[c].abs_diff(o[c]))).sum::<u64>()
            })
            .sum();
        total as f64 / (f64::from(image.width) * f64::from(image.height) * 3.0)
    }

    #[test]
    fn jpeg_decodes_close_to_the_image_at_every_size_and_quality() {
        let strokes = Tool::ALL
            .iter()
            .enumerate()
            .map(|(i, &tool)| {
                let y = 400.0 + i as f32 * 500.0;
                line(
                    tool,
                    Rgba::rgb(40, 60, 160),
                    tool.default_width(),
                    0,
                    (300.0, y),
                    (4700.0, y + 200.0),
                )
            })
            .collect();
        let n = note(SMALL, bg(Paper::Cream, Template::Grid5mm, false), strokes);
        let image = render_note(&n, &opts(0.08)).unwrap();
        // Full chroma from 90, subsampled below; lower qualities lose more. Measured: 0.97,
        // 3.6 and 9.2, the errors of libjpeg (ImageMagick) at the same qualities on this
        // line art within 1 %.
        let fine = jpeg_error(&image, 95);
        let usual = jpeg_error(&image, 75);
        let rough = jpeg_error(&image, 10);
        assert!(fine < 1.2, "{fine}");
        assert!(usual < 4.5 && usual > fine, "{usual}");
        assert!(rough < 11.0 && rough > usual, "{rough}");
        // Sizes that are not multiples of the block size, and a flat colour.
        for (width, height) in [(1, 1), (7, 9), (17, 15), (33, 2)] {
            let flat = Image {
                width,
                height,
                pixels: [200u8, 30, 60, 255].repeat((width * height) as usize),
            };
            for quality in [50, 95] {
                assert!(jpeg_error(&flat, quality) < 2.0, "{width} x {height}");
            }
        }
        // Transparent pixels become white.
        let clear = Image {
            width: 8,
            height: 8,
            pixels: vec![0; 256],
        };
        assert!(jpeg_error(&clear, 90) < 1.0);
        let bytes = encode_jpeg(&clear, 90).unwrap();
        let white = jpeg_decoder::Decoder::new(std::io::Cursor::new(&bytes))
            .decode()
            .unwrap();
        assert!(white.iter().all(|&v| v >= 254), "{white:?}");
        // Inconsistent images are refused.
        let bad = Image {
            width: 2,
            height: 2,
            pixels: vec![0; 3],
        };
        assert!(encode_jpeg(&bad, 90).is_err());
        assert!(encode_jpeg_at(&image, 90, Some(300)).unwrap().len() > 1000);
    }

    #[test]
    fn vector_templates_keep_exact_widths_and_positions() {
        let page = PageSize {
            width: 10_000,
            height: 10_000,
        };
        let seyes = background::vector_background(&bg(Paper::White, Template::Seyes, false), page);
        assert!(!seyes.dark && seyes.texture.is_none());
        // The red margin: 0.25 mm wide, centred 32 mm from the left, full height.
        let margin = seyes.marks.iter().find(|m| m.rgb == [220, 40, 70]).unwrap();
        assert_eq!(
            margin.shape,
            background::Shape::Rect {
                x: 3187.5,
                y: 0.0,
                width: 25.0,
                height: 10_000.0
            }
        );
        assert_eq!(margin.alpha, 0.75);
        // No fading: every 2 mm line is there, even those a thumbnail would hide.
        let horizontal = seyes
            .marks
            .iter()
            .filter(
                |m| matches!(m.shape, background::Shape::Rect { width, .. } if width == 10_000.0),
            )
            .count();
        // From 16 mm to 100 mm every 2 mm.
        assert_eq!(horizontal, 43);

        let dots = background::vector_background(&bg(Paper::Slate, Template::Dotted, true), page);
        assert!(dots.dark);
        // Every 5 mm from 0 to 100 mm, both edges included (as in the raster).
        assert_eq!(dots.marks.len(), 21 * 21);
        assert!(dots.marks.iter().all(
            |m| matches!(m.shape, background::Shape::Disc { r, .. } if (r - 30.0).abs() < 1e-3)
        ));
        // The texture image: 100 dpi, the page's proportions.
        let texture = dots.texture.unwrap();
        assert_eq!((texture.width, texture.height), (394, 394));
        let blank = background::vector_background(&bg(Paper::Kraft, Template::Blank, false), page);
        assert!(blank.marks.is_empty());
        assert_eq!(blank.paper, paper_color(Paper::Kraft));
    }

    #[test]
    fn pdf_pages_use_the_states_of_the_look() {
        let strokes = vec![
            line(
                Tool::Highlighter,
                Rgba::rgb(255, 230, 0),
                300.0,
                0,
                (500.0, 500.0),
                (4500.0, 500.0),
            ),
            line(
                Tool::FeltTip,
                Rgba::rgb(10, 10, 200),
                100.0,
                0,
                (500.0, 1500.0),
                (4500.0, 1500.0),
            ),
            line(
                Tool::Ballpoint,
                Rgba::BLACK,
                60.0,
                1,
                (500.0, 2500.0),
                (4500.0, 2500.0),
            ),
        ];
        let n = note(SMALL, bg(Paper::White, Template::Blank, false), strokes);
        let pdf = notes_to_pdf(&[&n, &n, &n], &PdfOptions::default()).unwrap();
        let document = lopdf::Document::load_mem(&pdf).unwrap();
        assert_eq!(document.get_pages().len(), 3);
        let text = String::from_utf8_lossy(&pdf);
        assert!(text.contains("/BM /Multiply"), "highlighter blending");
        assert!(text.contains("/ca 0.3"), "felt-tip halo");
        assert!(text.contains("/ca 0.9"), "felt-tip core");
        assert!(text.contains("/Producer (Ardoise)"));
        // Three notes: no document title.
        assert!(!text.contains("/Title"));
        let (_, &first) = document.get_pages().iter().next().unwrap();
        let content = document
            .get_page_content_with_limit(first, 1 << 24)
            .unwrap();
        let ops = lopdf::content::Content::decode(&content)
            .unwrap()
            .operations;
        // Paper + highlighter + halo + core; the hidden layer's stroke is not there.
        assert_eq!(ops.iter().filter(|op| op.operator == "f").count(), 4);
        // Deterministic.
        assert_eq!(
            pdf,
            notes_to_pdf(&[&n, &n, &n], &PdfOptions::default()).unwrap()
        );

        let single = notes_to_pdf(&[&n], &PdfOptions::default()).unwrap();
        assert!(String::from_utf8_lossy(&single).contains("/Title (test)"));
        let mut empty = n.clone();
        empty.page.width = 0;
        assert!(matches!(
            notes_to_pdf(&[&empty], &PdfOptions::default()),
            Err(RenderError::Size { .. })
        ));
    }
}
