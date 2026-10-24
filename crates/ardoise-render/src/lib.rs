//! Raster rendering of notes: ink tools, page backgrounds, templates and textures (PNG export).
//!
//! The public surface below is the contract used by the Inkspace import comparison, the
//! exports and the thumbnails. [`render_note`] draws, with tiny-skia and anti-aliasing:
//!
//! 1. the background ([`background`]): paper colour, template in millimetres, optional paper
//!    texture; or plain white when [`RenderOptions::background`] is off;
//! 2. the strokes of visible layers, bottom layer first and in note order within a layer,
//!    each filled from [`ardoise_ink::outline`] with its tool's look ([`ink`]).
//!
//! Rendering is deterministic: every texture comes from a fixed or content-derived seed, so
//! the same note and options always give the same bytes (the reference images rely on it).

mod background;
mod ink;
mod noise;

use ardoise_core::note::{Note, PageSize};
use ardoise_ink::Stroke;
use tiny_skia::{Color, Pixmap};

pub use background::paper_color;

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
}

/// Renders `note` to an image of `page × scale` pixels (each side rounded, at most
/// 16 384 px). Strokes on hidden layers are not drawn; a stroke whose layer index has no
/// layer entry is drawn (above the others) rather than lost.
pub fn render_note(note: &Note, options: &RenderOptions) -> Result<Image, RenderError> {
    let scale = options.scale;
    let width = (note.page.width as f32 * scale).round() as u32;
    let height = (note.page.height as f32 * scale).round() as u32;
    let size_error = RenderError::Size { width, height };
    if width == 0 || height == 0 || width > MAX_SIDE || height > MAX_SIDE {
        return Err(size_error);
    }
    let mut pixmap = Pixmap::new(width, height).ok_or(size_error)?;
    let dark = if options.background {
        background::paint(&mut pixmap, &note.background, scale);
        background::is_dark(note.background.paper)
    } else {
        pixmap.fill(Color::WHITE);
        false
    };
    let mut painter = ink::InkPainter::new(&pixmap, scale, dark);
    for stroke in visible_strokes(note) {
        painter.draw(&mut pixmap, stroke);
    }
    Ok(image_from(&pixmap))
}

/// Strokes to draw, bottom layer first, in note order within a layer.
fn visible_strokes(note: &Note) -> Vec<&Stroke> {
    let mut strokes: Vec<&Stroke> = note
        .strokes
        .iter()
        .filter(|s| {
            note.layers
                .get(s.layer as usize)
                .is_none_or(|layer| layer.visible)
        })
        .collect();
    strokes.sort_by_key(|s| s.layer);
    strokes
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

/// PNG bytes of `image` (8-bit RGB when every pixel is opaque, which rendered notes are;
/// RGBA otherwise).
pub fn encode_png(image: &Image) -> Result<Vec<u8>, RenderError> {
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
    let opaque = image.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255);
    let encode_error = |e: png::EncodingError| RenderError::Encode(e.to_string());
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Balanced);
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
            tags: vec![],
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
}
