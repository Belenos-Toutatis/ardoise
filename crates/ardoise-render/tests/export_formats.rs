//! The M5 export proof: a synthetic reference note (every tool, several colours, a
//! translucent stroke, a Seyès template with texture, two layers of which one is hidden) is
//! exported to PNG (paper and transparent), WebP (both), JPG, SVG, PDF (two pages, actual size and A4) and
//! the clipboard payload, and each file is checked by an independent reader (the `png`
//! decoder, `jpeg-decoder`, `roxmltree`, `lopdf`, and the DIB header read by hand).
//!
//! `cargo test -p ardoise-render --test export_formats -- --nocapture` prints a table; the
//! files stay in `target/tmp/export_formats/` to be opened by hand.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::f32::consts::TAU;
use std::fs;
use std::path::PathBuf;

use ardoise_core::note::{
    A4_LANDSCAPE, A4_PORTRAIT, Background, Layer, Note, Origin, Paper, Template, Timing,
};
use ardoise_ink::{Point, Rgba, Stroke, Tool, outline};
use ardoise_render::{
    LayerMode, PdfOptions, PdfPage, RenderOptions, SvgOptions, clipboard_image_at, encode_jpeg_at,
    encode_png_at, encode_webp, note_to_svg, notes_to_pdf, render_note, render_note_transparent,
    scale_for_dpi,
};

const DPI: u32 = 150;
const JPEG_QUALITY: u8 = 90;

fn trace(n: usize, t0: u32, f: impl Fn(f32) -> (f32, f32, f32)) -> Vec<Point> {
    (0..=n)
        .map(|i| {
            let (x, y, pressure) = f(i as f32 / n as f32);
            Point {
                x,
                y,
                pressure,
                t: t0 + i as u32 * 5,
            }
        })
        .collect()
}

fn stroke(tool: Tool, color: Rgba, layer: u32, points: Vec<Point>) -> Stroke {
    Stroke {
        tool,
        color,
        width: tool.default_width(),
        layer,
        points,
    }
}

/// A wave along the line `y`, from `x0` to `x1`, with pressure rising then falling.
fn wave(x0: f32, x1: f32, y: f32, amplitude: f32, t0: u32) -> Vec<Point> {
    trace(80, t0, |u| {
        (
            x0 + (x1 - x0) * u,
            y + amplitude * (u * TAU * 3.0).sin(),
            0.3 + 0.7 * (u * std::f32::consts::PI).sin(),
        )
    })
}

/// The reference note: one row per tool on a cream Seyès page with texture, a highlighter
/// over ink, a translucent stroke, and a hidden layer of strokes that must not be exported.
fn reference_note() -> Note {
    let colours = [
        Rgba::BLACK,
        Rgba::rgb(0x1a, 0x2a, 0x80),
        Rgba::rgb(0xd8, 0x00, 0x00),
        Rgba::rgb(0x3a, 0x3a, 0x3c),
        Rgba::rgb(0x4b, 0x2a, 0x9c),
        Rgba::rgb(0xff, 0xe6, 0x2e),
        Rgba::rgb(0x84, 0xbd, 0x00),
    ];
    let mut strokes: Vec<Stroke> = Tool::ALL
        .iter()
        .zip(colours)
        .enumerate()
        .map(|(row, (&tool, colour))| {
            let y = 4_000.0 + row as f32 * 2_400.0;
            stroke(
                tool,
                colour,
                0,
                wave(5_000.0, 26_000.0, y, 500.0, row as u32 * 1000),
            )
        })
        .collect();
    // A highlighter across the ballpoint row, and a translucent felt tip.
    strokes.push(stroke(
        Tool::Highlighter,
        Rgba::rgb(0x9c, 0xf0, 0x7a),
        0,
        trace(20, 9_000, |u| {
            (12_000.0 + 1_000.0 * u, 2_500.0 + 4_000.0 * u, 1.0)
        }),
    ));
    strokes.push(stroke(
        Tool::FeltTip,
        Rgba {
            a: 128,
            ..Rgba::rgb(0xbb, 0x3e, 0x9a)
        },
        0,
        trace(40, 10_000, |u| {
            let a = u * TAU;
            (
                8_000.0 + 1_500.0 * a.cos(),
                12_000.0 + 1_500.0 * a.sin(),
                0.8,
            )
        }),
    ));
    // The hidden layer.
    for i in 0..3 {
        strokes.push(stroke(
            Tool::Ballpoint,
            Rgba::rgb(0xd8, 0x00, 0x00),
            1,
            wave(
                4_000.0,
                25_000.0,
                6_000.0 + i as f32 * 5_000.0,
                900.0,
                20_000,
            ),
        ));
    }
    Note {
        id: "7d1f0c3e-5b2a-4c8e-9f61-0a5e3c2b1d40".into(),
        title: "Référence & exports".into(),
        created: 1_792_843_200_000,
        modified: 1_792_843_200_000,
        page: A4_LANDSCAPE,
        background: Background {
            paper: Paper::Cream,
            template: Template::Seyes,
            texture: true,
        },
        background_image: None,
        tags: vec![],
        favorite: false,
        layers: vec![
            Layer {
                name: "Calque 1".into(),
                visible: true,
            },
            Layer {
                name: "Brouillon".into(),
                visible: false,
            },
        ],
        strokes,
        objects: Vec::new(),
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

/// A second, portrait note on dotted white paper for the second PDF page.
fn second_note() -> Note {
    Note {
        id: "7d1f0c3e-5b2a-4c8e-9f61-0a5e3c2b1d41".into(),
        title: "Page 2".into(),
        page: A4_PORTRAIT,
        background: Background {
            paper: Paper::White,
            template: Template::Dotted,
            texture: false,
        },
        layers: vec![Layer {
            name: "Calque 1".into(),
            visible: true,
        }],
        strokes: vec![
            stroke(
                Tool::FountainPen,
                Rgba::BLACK,
                0,
                wave(3_000.0, 18_000.0, 5_000.0, 400.0, 0),
            ),
            stroke(
                Tool::Brush,
                Rgba::rgb(0x51, 0xa7, 0xf9),
                0,
                wave(3_000.0, 18_000.0, 9_000.0, 800.0, 900),
            ),
        ],
        ..reference_note()
    }
}

/// Fills a vector format must hold: the felt tip has a halo and a core, every other tool
/// one fill; hidden layers and empty outlines give none.
fn expected_fills(note: &Note) -> usize {
    note.strokes
        .iter()
        .filter(|s| note.layers.get(s.layer as usize).is_none_or(|l| l.visible))
        .filter(|s| !outline(s).is_empty())
        .map(|s| if s.tool == Tool::FeltTip { 2 } else { 1 })
        .sum()
}

fn visible_highlighters(note: &Note) -> usize {
    note.strokes
        .iter()
        .filter(|s| s.tool == Tool::Highlighter)
        .filter(|s| note.layers.get(s.layer as usize).is_none_or(|l| l.visible))
        .count()
}

/// One row of the report.
struct Row {
    format: &'static str,
    bytes: usize,
    valid: bool,
    dimensions: String,
    pages: String,
    file: PathBuf,
}

fn print_table(rows: &[Row]) {
    println!(
        "\n{:<24} {:>10}  {:<5}  {:<40} {:<5}  file",
        "format", "bytes", "valid", "dimensions", "pages"
    );
    for row in rows {
        println!(
            "{:<24} {:>10}  {:<5}  {:<40} {:<5}  {}",
            row.format,
            row.bytes,
            if row.valid { "yes" } else { "NO" },
            row.dimensions,
            row.pages,
            row.file.file_name().unwrap().to_string_lossy()
        );
    }
}

fn png_info(
    bytes: &[u8],
) -> (
    u32,
    u32,
    png::ColorType,
    Option<png::PixelDimensions>,
    Vec<u8>,
) {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::IDENTITY);
    let mut reader = decoder.read_info().expect("PNG header");
    let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
    let frame = reader.next_frame(&mut buffer).expect("PNG data");
    buffer.truncate(frame.buffer_size());
    let dims = reader.info().pixel_dims;
    (frame.width, frame.height, frame.color_type, dims, buffer)
}

fn number(object: &lopdf::Object) -> f32 {
    match object {
        lopdf::Object::Integer(i) => *i as f32,
        lopdf::Object::Real(r) => *r,
        other => panic!("not a number: {other:?}"),
    }
}

#[test]
fn a_reference_note_exports_to_every_format() {
    let note = reference_note();
    let second = second_note();
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("export_formats");
    fs::create_dir_all(&dir).unwrap();
    let mut rows = Vec::new();
    let fills = expected_fills(&note);
    assert_eq!(
        fills, 11,
        "7 tools (the felt tip has 2 fills), a highlighter, a translucent felt tip"
    );

    // The expected image size at 150 dpi: 297 × 216 mm.
    let raster = render_note(&note, &RenderOptions::at_dpi(DPI, true)).unwrap();
    let (w, h) = (raster.width, raster.height);
    assert_eq!((w, h), (1754, 1276));
    assert_eq!(scale_for_dpi(DPI), DPI as f32 / 2540.0);

    // PNG with the paper: opaque RGB, 150 dpi recorded.
    let png = encode_png_at(&raster, Some(DPI)).unwrap();
    let file = dir.join("reference.png");
    fs::write(&file, &png).unwrap();
    let (pw, ph, colour, dims, data) = png_info(&png);
    let ppm = dims.expect("pHYs chunk");
    let valid = (pw, ph) == (w, h)
        && colour == png::ColorType::Rgb
        && ppm.unit == png::Unit::Meter
        && ppm.xppu == 5906
        && data.len() == (w * h * 3) as usize;
    rows.push(Row {
        format: "PNG (paper)",
        bytes: png.len(),
        valid,
        dimensions: format!(
            "{pw} x {ph} px, {} dpi",
            (f64::from(ppm.xppu) * 0.0254).round()
        ),
        pages: "-".into(),
        file,
    });

    // Transparent PNG: RGBA, transparent where there is no ink, opaque ink somewhere.
    let clear = render_note_transparent(&note, scale_for_dpi(DPI)).unwrap();
    let png = encode_png_at(&clear, Some(DPI)).unwrap();
    let file = dir.join("reference-transparent.png");
    fs::write(&file, &png).unwrap();
    let (pw, ph, colour, _, data) = png_info(&png);
    let alphas: Vec<u8> = data.as_chunks::<4>().0.iter().map(|p| p[3]).collect();
    let transparent = alphas.iter().filter(|&&a| a == 0).count();
    let opaque = alphas.iter().filter(|&&a| a == 255).count();
    let valid = (pw, ph) == (w, h)
        && colour == png::ColorType::Rgba
        && transparent > alphas.len() / 2
        && opaque > 1000
        && data[..4] == [0, 0, 0, 0];
    rows.push(Row {
        format: "PNG (transparent)",
        bytes: png.len(),
        valid,
        dimensions: format!(
            "{pw} x {ph} px, {:.0} % clear",
            100.0 * transparent as f64 / alphas.len() as f64
        ),
        pages: "-".into(),
        file,
    });

    // WebP: lossless like the PNG (every pixel back, read by `image-webp`'s decoder), and
    // lighter; the transparent one keeps its alpha.
    let paper_png_size = encode_png_at(&raster, Some(DPI)).unwrap().len();
    let webp = encode_webp(&raster).unwrap();
    let file = dir.join("reference.webp");
    fs::write(&file, &webp).unwrap();
    let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(&webp)).unwrap();
    let (ww, wh) = decoder.dimensions();
    let mut pixels = vec![0; decoder.output_buffer_size().unwrap()];
    decoder.read_image(&mut pixels).unwrap();
    let identical = pixels
        .chunks(3)
        .zip(raster.pixels.as_chunks::<4>().0)
        .all(|(d, r)| d == &r[..3]);
    rows.push(Row {
        format: "WebP (paper)",
        bytes: webp.len(),
        valid: (ww, wh) == (w, h) && !decoder.has_alpha() && !decoder.is_lossy() && identical,
        dimensions: format!(
            "{ww} x {wh} px, {:.0} % of the PNG",
            100.0 * webp.len() as f64 / paper_png_size as f64
        ),
        pages: "-".into(),
        file,
    });
    let webp = encode_webp(&clear).unwrap();
    let file = dir.join("reference-transparent.webp");
    fs::write(&file, &webp).unwrap();
    let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(&webp)).unwrap();
    rows.push(Row {
        format: "WebP (transparent)",
        bytes: webp.len(),
        valid: decoder.dimensions() == (w, h) && decoder.has_alpha() && !decoder.is_lossy(),
        dimensions: format!("{} x {} px", decoder.dimensions().0, decoder.dimensions().1),
        pages: "-".into(),
        file,
    });

    // JPG: baseline YCbCr, close to the rendered image (the error, measured 1.9 / 255, is
    // mostly the paper's fine grain, which JPEG smooths away).
    let jpg = encode_jpeg_at(&raster, JPEG_QUALITY, Some(DPI)).unwrap();
    let file = dir.join("reference.jpg");
    fs::write(&file, &jpg).unwrap();
    let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(&jpg));
    let decoded = decoder.decode().expect("JPEG decodes");
    let info = decoder.info().unwrap();
    let mean_error = decoded
        .as_chunks::<3>()
        .0
        .iter()
        .zip(raster.pixels.as_chunks::<4>().0)
        .map(|(d, r)| {
            (0..3)
                .map(|c| (i32::from(d[c]) - i32::from(r[c])).abs())
                .sum::<i32>()
        })
        .map(f64::from)
        .sum::<f64>()
        / (f64::from(w) * f64::from(h) * 3.0);
    let valid = (u32::from(info.width), u32::from(info.height)) == (w, h)
        && info.pixel_format == jpeg_decoder::PixelFormat::RGB24
        && decoded.len() == (w * h * 3) as usize
        && mean_error < 3.0;
    rows.push(Row {
        format: "JPG (quality 90)",
        bytes: jpg.len(),
        valid,
        dimensions: format!(
            "{} x {} px, mean error {mean_error:.2}",
            info.width, info.height
        ),
        pages: "-".into(),
        file,
    });

    // SVG: an SVG root in millimetres over device units, one path per fill.
    let svg = note_to_svg(&note, &SvgOptions::default());
    let file = dir.join("reference.svg");
    fs::write(&file, &svg).unwrap();
    let document = roxmltree::Document::parse(&svg).expect("well-formed XML");
    let root = document.root_element();
    let paths: Vec<_> = root
        .descendants()
        .filter(|n| n.has_tag_name(("http://www.w3.org/2000/svg", "path")))
        .collect();
    let blended = paths
        .iter()
        .filter(|p| p.attribute("style") == Some("mix-blend-mode:multiply"))
        .count();
    let images = root
        .descendants()
        .filter(|n| n.has_tag_name("image"))
        .count();
    let valid = root.has_tag_name(("http://www.w3.org/2000/svg", "svg"))
        && root.attribute("width") == Some("297mm")
        && root.attribute("height") == Some("216mm")
        && root.attribute("viewBox") == Some("0 0 29700 21600")
        && paths.len() == fills
        && paths
            .iter()
            .all(|p| p.attribute("fill-rule") == Some("nonzero"))
        && paths
            .iter()
            .all(|p| p.attribute("d").is_some_and(|d| d.starts_with('M')))
        && blended == visible_highlighters(&note)
        && images == 1;
    rows.push(Row {
        format: "SVG",
        bytes: svg.len(),
        valid,
        dimensions: format!(
            "{} x {}, {} paths",
            root.attribute("width").unwrap_or("?"),
            root.attribute("height").unwrap_or("?"),
            paths.len()
        ),
        pages: "-".into(),
        file,
    });

    // PDF, actual size: two pages, each the size of its note.
    for (format, page, name, sizes) in [
        (
            "PDF (actual size)",
            PdfPage::Actual,
            "reference.pdf",
            [(841.89, 612.28), (612.28, 841.89)],
        ),
        (
            "PDF (A4)",
            PdfPage::A4,
            "reference-a4.pdf",
            [(841.89, 595.28), (595.28, 841.89)],
        ),
    ] {
        let options = PdfOptions {
            page,
            background: true,
            ..PdfOptions::default()
        };
        let pdf = notes_to_pdf(&[&note, &second], &options).unwrap();
        let file = dir.join(name);
        fs::write(&file, &pdf).unwrap();
        let document = lopdf::Document::load_mem(&pdf).expect("PDF parses");
        let pages = document.get_pages();
        let mut boxes = Vec::new();
        let mut fill_ops = Vec::new();
        for &id in pages.values() {
            let page = document.get_dictionary(id).unwrap();
            let media: Vec<f32> = page
                .get(b"MediaBox")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(number)
                .collect();
            boxes.push(media);
            let content = document
                .get_page_content_with_limit(id, 256 << 20)
                .expect("content stream decompresses");
            let ops = lopdf::content::Content::decode(&content)
                .unwrap()
                .operations;
            fill_ops.push(ops.iter().filter(|op| op.operator == "f").count());
        }
        let boxes_ok = boxes.iter().zip(sizes).all(|(b, (bw, bh))| {
            b.len() == 4
                && b[0] == 0.0
                && b[1] == 0.0
                && (b[2] - bw).abs() < 0.01
                && (b[3] - bh).abs() < 0.01
        });
        // Page 1: paper + template shapes + the strokes' fills (at least).
        let valid = pdf.starts_with(b"%PDF-1.")
            && pages.len() == 2
            && boxes_ok
            && fill_ops[0] > fills
            && fill_ops[1] > expected_fills(&second);
        let describe = |b: &Vec<f32>| format!("{:.2} x {:.2}", b[2], b[3]);
        rows.push(Row {
            format,
            bytes: pdf.len(),
            valid,
            dimensions: format!(
                "{} / {} pt",
                boxes.first().map(describe).unwrap_or_default(),
                boxes.get(1).map(describe).unwrap_or_default()
            ),
            pages: pages.len().to_string(),
            file,
        });
    }

    // Clipboard: PNG for the "PNG" format, a bottom-up 24-bit DIB for CF_DIB.
    let clip = clipboard_image_at(&raster, Some(DPI)).unwrap();
    let file = dir.join("reference-clipboard.dib");
    fs::write(&file, &clip.dib).unwrap();
    let field = |at: usize| u32::from_le_bytes(clip.dib[at..at + 4].try_into().unwrap());
    let half = |at: usize| u16::from_le_bytes(clip.dib[at..at + 2].try_into().unwrap());
    let stride = (w * 3).div_ceil(4) * 4;
    // The bottom-left pixel of the image is the first one of the DIB, in BGR order.
    let bottom_left = raster.pixel(0, h - 1).unwrap();
    let valid = field(0) == 40
        && field(4) == w
        && field(8) == h
        && half(12) == 1
        && half(14) == 24
        && field(16) == 0
        && field(20) == stride * h
        && field(24) == 5906
        && clip.dib.len() == 40 + (stride * h) as usize
        && clip.dib[40..43] == [bottom_left[2], bottom_left[1], bottom_left[0]]
        && png_info(&clip.png).0 == w;
    rows.push(Row {
        format: "Clipboard (CF_DIB+PNG)",
        bytes: clip.dib.len() + clip.png.len(),
        valid,
        dimensions: format!("{} x {} px, 24-bit, rows of {stride} B", field(4), field(8)),
        pages: "-".into(),
        file,
    });

    print_table(&rows);
    println!("files in {}", dir.display());
    let failed: Vec<&str> = rows.iter().filter(|r| !r.valid).map(|r| r.format).collect();
    assert!(failed.is_empty(), "invalid exports: {failed:?}");
}

#[test]
fn the_svg_without_background_is_transparent_and_hidden_strokes_never_appear() {
    let mut note = reference_note();
    let svg = note_to_svg(
        &note,
        &SvgOptions {
            background: false,
            ..SvgOptions::default()
        },
    );
    let document = roxmltree::Document::parse(&svg).unwrap();
    let elements: Vec<&str> = document
        .root_element()
        .descendants()
        .filter(|n| n.is_element())
        .map(|n| n.tag_name().name())
        .collect();
    assert!(!elements.contains(&"rect") && !elements.contains(&"image"));
    assert_eq!(
        elements.iter().filter(|&&e| e == "path").count(),
        expected_fills(&note)
    );

    // Showing the hidden layer adds exactly its three strokes.
    note.layers[1].visible = true;
    let svg = note_to_svg(
        &note,
        &SvgOptions {
            background: false,
            ..SvgOptions::default()
        },
    );
    assert_eq!(svg.matches("<path ").count(), expected_fills(&note));
    assert_eq!(expected_fills(&note), 14);
}

/// `svg` of a page `page_width` device units wide, drawn by resvg (its own parser, and
/// another tiny-skia version) at `dpi` into `width × height` pixels (the raster's size),
/// composited over white, as RGB.
fn draw_svg(svg: &str, page_width: u32, dpi: u32, width: u32, height: u32) -> Vec<[u8; 3]> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("usvg parses");
    // usvg sizes the page in CSS pixels (96 per inch); the raster has `dpi / 2540` pixels per
    // device unit and crops the last partial pixel.
    let scale = scale_for_dpi(dpi) * page_width as f32 / tree.size().width();
    let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    pixmap
        .pixels()
        .iter()
        .map(|p| {
            // Premultiplied over white.
            let white = 255 - p.alpha();
            [p.red() + white, p.green() + white, p.blue() + white]
        })
        .collect()
}

/// Mean absolute difference per channel, and the share of pixels with a channel off by more
/// than 64, between a raster render and an SVG drawn by resvg.
fn difference(raster: &ardoise_render::Image, svg: &[[u8; 3]]) -> (f64, f64) {
    let mut total = 0u64;
    let mut gross = 0usize;
    for (r, s) in raster.pixels.as_chunks::<4>().0.iter().zip(svg) {
        let d: Vec<u8> = (0..3).map(|c| r[c].abs_diff(s[c])).collect();
        total += d.iter().map(|&v| u64::from(v)).sum::<u64>();
        if d.iter().any(|&v| v > 64) {
            gross += 1;
        }
    }
    let pixels = svg.len() as f64;
    (total as f64 / (pixels * 3.0), gross as f64 / pixels)
}

/// SPEC §6.2, "un seul dessin des traits": the SVG, drawn by an independent renderer, shows
/// what the PNG shows. The grain tools are left out of the pixel-by-pixel check (vector
/// formats fill them with the grain's mean coverage), as is the paper texture (an embedded
/// JPEG image, which resvg draws only with its raster-image feature).
#[test]
fn the_svg_draws_what_the_raster_draws() {
    let mut note = reference_note();
    note.strokes
        .retain(|s| !matches!(s.tool, Tool::Pencil | Tool::Chalk));
    note.background.texture = false;
    let dpi = 100;
    for background in [false, true] {
        let raster = render_note(&note, &RenderOptions::at_dpi(dpi, background)).unwrap();
        let svg = note_to_svg(
            &note,
            &SvgOptions {
                background,
                ..SvgOptions::default()
            },
        );
        let drawn = draw_svg(&svg, note.page.width, dpi, raster.width, raster.height);
        let (mean, gross) = difference(&raster, &drawn);
        println!(
            "SVG drawn by resvg vs PNG at {dpi} dpi, background {background}: mean difference \
             {mean:.3} / 255, {:.3} % of pixels off by more than 64",
            gross * 100.0
        );
        // Strokes alone: only anti-aliasing differs (and the felt-tip halo, sized for
        // 300 dpi in vector formats); measured 0.04 and no pixel. With the paper, the raster
        // snaps template lines to whole pixels where the vector spreads their exact width
        // over two (measured 1.8 and 0.16 %); white paper instead of cream would give 15.
        let (max_mean, max_gross) = if background {
            (2.5, 0.005)
        } else {
            (0.2, 0.000_1)
        };
        assert!(mean < max_mean, "mean difference {mean}");
        assert!(gross < max_gross, "share of very different pixels {gross}");
    }

    // A missing stroke would be caught: drop the felt-tip row from the SVG only.
    let raster = render_note(&note, &RenderOptions::at_dpi(dpi, false)).unwrap();
    let mut fewer = note.clone();
    fewer.strokes.retain(|s| s.tool != Tool::FeltTip);
    let drawn = draw_svg(
        &note_to_svg(
            &fewer,
            &SvgOptions {
                background: false,
                ..SvgOptions::default()
            },
        ),
        note.page.width,
        dpi,
        raster.width,
        raster.height,
    );
    let (_, gross) = difference(&raster, &drawn);
    assert!(gross > 0.000_5, "{gross}");
}

// --- Layers at export (M14, SPEC §4.6, docs/design/exports-and-operations.md) -------------

/// FNV-1a 64 of `bytes`, with their length: a short fingerprint of a file.
fn fingerprint(bytes: &[u8]) -> (usize, u64) {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    (bytes.len(), hash)
}

/// The exports of the reference note in the default modes (SVG separate, PDF merged) are the
/// files written before the layers option existed: fingerprints taken then, on the same notes.
#[test]
fn the_default_modes_write_todays_files_byte_for_byte() {
    let note = reference_note();
    let second = second_note();
    assert_eq!(
        fingerprint(note_to_svg(&note, &SvgOptions::default()).as_bytes()),
        (142_826, 15_857_894_918_227_345_026)
    );
    for (page, expected) in [
        (PdfPage::Actual, (172_161, 7_469_838_746_515_188_696)),
        (PdfPage::A4, (172_166, 12_133_712_646_582_357_706)),
    ] {
        let options = PdfOptions {
            page,
            background: true,
            layers: LayerMode::Merged,
        };
        let pdf = notes_to_pdf(&[&note, &second], &options).unwrap();
        assert_eq!(fingerprint(&pdf), expected, "{page:?}");
    }
    assert_eq!(SvgOptions::default().layers, LayerMode::Separate);
    assert_eq!(PdfOptions::default().layers, LayerMode::Merged);
}

/// A white dotted page with one ballpoint stroke per entry of `rows` (the layer it is on).
fn layered_note(title: &str, layers: &[(&str, bool)], rows: &[u32]) -> Note {
    Note {
        title: title.into(),
        background: Background {
            paper: Paper::White,
            template: Template::Dotted,
            texture: false,
        },
        layers: layers
            .iter()
            .map(|&(name, visible)| Layer {
                name: name.into(),
                visible,
            })
            .collect(),
        strokes: rows
            .iter()
            .enumerate()
            .map(|(i, &layer)| {
                stroke(
                    Tool::Ballpoint,
                    Rgba::BLACK,
                    layer,
                    wave(3_000.0, 20_000.0, 3_000.0 + i as f32 * 2_000.0, 300.0, 0),
                )
            })
            .collect(),
        ..reference_note()
    }
}

/// Two layers shown, one hidden, and a stroke on a layer the note has no entry for.
fn first_layered() -> Note {
    layered_note(
        "Un",
        &[
            ("Calque 1", true),
            ("Correction", true),
            ("Reponses", false),
        ],
        &[0, 0, 1, 2, 2, 5],
    )
}

/// Shares « Calque 1 » with the first.
fn second_layered() -> Note {
    layered_note("Deux", &[("Calque 1", true), ("Notes", true)], &[0, 1, 1])
}

/// A note whose only layer is hidden.
fn all_hidden() -> Note {
    layered_note("Trois", &[("Cachee", false)], &[0, 0])
}

/// The SVG of `note` and its layers of ink (not the background's): label and path count.
fn svg_layers(note: &Note, layers: LayerMode) -> (String, Vec<(String, usize)>) {
    let svg = note_to_svg(
        note,
        &SvgOptions {
            background: true,
            layers,
        },
    );
    let document = roxmltree::Document::parse(&svg).expect("well-formed XML");
    let inkscape = "http://www.inkscape.org/namespaces/inkscape";
    let found = document
        .root_element()
        .children()
        .filter(|n| n.is_element() && n.attribute((inkscape, "groupmode")) == Some("layer"))
        .filter(|n| n.attribute("id") != Some("background"))
        .map(|n| {
            (
                n.attribute((inkscape, "label")).unwrap_or("").to_owned(),
                n.descendants().filter(|d| d.has_tag_name("path")).count(),
            )
        })
        .collect();
    (svg, found)
}

#[test]
fn svg_layers_are_separate_or_merged_and_a_hidden_one_is_never_written() {
    let note = first_layered();
    let (separate, layers) = svg_layers(&note, LayerMode::Separate);
    assert_eq!(
        layers,
        [
            ("Calque 1".into(), 2),
            ("Correction".into(), 1),
            ("6".into(), 1)
        ]
    );
    let (merged, layers) = svg_layers(&note, LayerMode::Merged);
    assert_eq!(layers, [("Calque 1 + Correction + 6".into(), 4)]);
    assert_eq!(
        separate.matches("<path ").count(),
        merged.matches("<path ").count()
    );
    for svg in [&separate, &merged] {
        assert!(!svg.contains("Reponses"));
    }
    // The reference note's hidden layer, in either mode.
    let reference = reference_note();
    for mode in [LayerMode::Separate, LayerMode::Merged] {
        let (svg, layers) = svg_layers(&reference, mode);
        assert!(!svg.contains("Brouillon"));
        let paths: usize = layers.iter().map(|(_, paths)| paths).sum();
        assert_eq!(paths, expected_fills(&reference));
    }
    // A note with nothing visible has no layer of ink.
    for mode in [LayerMode::Separate, LayerMode::Merged] {
        let (svg, layers) = svg_layers(&all_hidden(), mode);
        assert!(layers.is_empty() && !svg.contains("Cachee"));
    }
}

/// What the tests read back of a PDF made with separate layers.
#[derive(Debug, Default)]
struct PdfLayers {
    /// `/OCProperties /OCGs`, as names.
    groups: Vec<String>,
    /// `/OCProperties /D /Order`, as names.
    order: Vec<String>,
    /// Per page, the `/Properties` resource: tag, group name, group object.
    properties: Vec<Vec<(String, String, lopdf::ObjectId)>>,
    /// Per page, the marked-content sequences in painting order: group name, fills inside.
    sequences: Vec<Vec<(String, usize)>>,
    /// Per page, every `f` of the content stream.
    fills: Vec<usize>,
    /// Per page, the `BDC` and `EMC` operators.
    marks: Vec<(usize, usize)>,
}

fn group_name(document: &lopdf::Document, object: &lopdf::Object) -> String {
    let (_, object) = document.dereference(object).unwrap();
    let group = object.as_dict().unwrap();
    assert_eq!(group.get(b"Type").unwrap().as_name().unwrap(), b"OCG");
    String::from_utf8(group.get(b"Name").unwrap().as_str().unwrap().to_vec()).unwrap()
}

fn read_layers(pdf: &[u8]) -> (lopdf::Document, PdfLayers) {
    let document = lopdf::Document::load_mem(pdf).expect("PDF parses");
    let mut read = PdfLayers::default();
    if let Ok(properties) = document.catalog().unwrap().get(b"OCProperties") {
        let properties = document.dereference(properties).unwrap().1;
        let properties = properties.as_dict().unwrap();
        let names = |object: &lopdf::Object| -> Vec<String> {
            object
                .as_array()
                .unwrap()
                .iter()
                .map(|o| group_name(&document, o))
                .collect()
        };
        read.groups = names(properties.get(b"OCGs").unwrap());
        let config = properties.get(b"D").unwrap().as_dict().unwrap();
        assert_eq!(config.get(b"BaseState").unwrap().as_name().unwrap(), b"ON");
        read.order = names(config.get(b"Order").unwrap());
    }
    for &id in document.get_pages().values() {
        let (resources, _) = document.get_page_resources(id).unwrap();
        let tags: Vec<(String, String, lopdf::ObjectId)> = resources
            .and_then(|r| r.get(b"Properties").ok())
            .map(|p| {
                let p = document.dereference(p).unwrap().1;
                p.as_dict()
                    .unwrap()
                    .iter()
                    .map(|(tag, group)| {
                        (
                            String::from_utf8(tag.clone()).unwrap(),
                            group_name(&document, group),
                            group.as_reference().unwrap(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let content = document.get_page_content_with_limit(id, 256 << 20).unwrap();
        let operations = lopdf::content::Content::decode(&content)
            .unwrap()
            .operations;
        let (mut begins, mut ends, mut fills) = (0, 0, 0);
        let mut sequences = Vec::new();
        let mut open: Option<(String, usize)> = None;
        for op in &operations {
            match op.operator.as_str() {
                "BDC" => {
                    begins += 1;
                    assert_eq!(op.operands[0].as_name().unwrap(), b"OC");
                    let tag =
                        String::from_utf8(op.operands[1].as_name().unwrap().to_vec()).unwrap();
                    let group = tags
                        .iter()
                        .find(|(t, _, _)| *t == tag)
                        .expect("the tag is in /Properties");
                    assert!(open.replace((group.1.clone(), 0)).is_none(), "nested BDC");
                }
                "EMC" => {
                    ends += 1;
                    sequences.push(open.take().expect("EMC closes a BDC"));
                }
                "f" => {
                    fills += 1;
                    if let Some((_, count)) = open.as_mut() {
                        *count += 1;
                    }
                }
                _ => {}
            }
        }
        assert!(open.is_none(), "unclosed BDC");
        read.properties.push(tags);
        read.sequences.push(sequences);
        read.fills.push(fills);
        read.marks.push((begins, ends));
    }
    (document, read)
}

fn pdf_of(notes: &[&Note], layers: LayerMode) -> Vec<u8> {
    let options = PdfOptions {
        layers,
        ..PdfOptions::default()
    };
    notes_to_pdf(notes, &options).unwrap()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn a_pdf_with_separate_layers_has_one_group_per_layer_name_over_all_its_pages() {
    let (first, second) = (first_layered(), second_layered());
    let pdf = pdf_of(&[&first, &second], LayerMode::Separate);
    let (document, read) = read_layers(&pdf);

    // Two layers on each page, one name in common: 3 groups, not 4, the top layer of the
    // first page first.
    let expected = ["Correction", "Calque 1", "Notes"];
    assert_eq!(read.groups, expected);
    assert_eq!(read.order, expected);
    let catalog = document.catalog().unwrap();
    assert_eq!(
        catalog.get(b"PageMode").unwrap().as_name().unwrap(),
        b"UseOC"
    );
    // Each page names its own groups; « Calque 1 » is the same object on both.
    let names = |page: usize| {
        let mut names: Vec<&str> = read.properties[page]
            .iter()
            .map(|(_, name, _)| name.as_str())
            .collect();
        names.sort_unstable();
        names
    };
    assert_eq!(names(0), ["Calque 1", "Correction"]);
    assert_eq!(names(1), ["Calque 1", "Notes"]);
    let shared = |page: usize| {
        read.properties[page]
            .iter()
            .find(|(_, name, _)| name == "Calque 1")
            .unwrap()
            .2
    };
    assert_eq!(shared(0), shared(1));

    // One sequence per layer of the page, bottom first, around that layer's fills; the stroke
    // of a layer the note has no entry for stays outside the groups.
    assert_eq!(
        read.sequences[0],
        [("Calque 1".to_owned(), 2), ("Correction".to_owned(), 1)]
    );
    assert_eq!(
        read.sequences[1],
        [("Calque 1".to_owned(), 1), ("Notes".to_owned(), 2)]
    );
    assert_eq!(read.marks, [(2, 2), (2, 2)]);

    // The drawing is the same as merged: as many fills, the hidden layer's not among them.
    let (_, merged) = read_layers(&pdf_of(&[&first, &second], LayerMode::Merged));
    assert_eq!(read.fills, merged.fills);
    // Two strokes of the first layer, one of the second, the one without a layer entry.
    assert_eq!(expected_fills(&first), 4);

    // A hidden layer's name is in no object, and not in the page streams either.
    assert!(!contains(&pdf, b"Reponses"));
    for &id in document.get_pages().values() {
        let content = document.get_page_content_with_limit(id, 256 << 20).unwrap();
        assert!(!contains(&content, b"Reponses"));
    }
}

#[test]
fn a_merged_pdf_has_no_optional_content_and_a_hidden_layer_is_in_no_pdf() {
    let (first, second) = (first_layered(), second_layered());
    let pdf = pdf_of(&[&first, &second], LayerMode::Merged);
    let (_, read) = read_layers(&pdf);
    assert!(read.groups.is_empty() && read.properties.iter().all(Vec::is_empty));
    assert_eq!(read.marks, [(0, 0), (0, 0)]);
    for word in [
        &b"OCProperties"[..],
        b"/OCG",
        b"/OC ",
        b"UseOC",
        b"Reponses",
    ] {
        assert!(!contains(&pdf, word), "{}", String::from_utf8_lossy(word));
    }
    // The reference note's hidden layer, in both modes, one note or two.
    let (note, other) = (reference_note(), second_note());
    for mode in [LayerMode::Merged, LayerMode::Separate] {
        for notes in [vec![&note], vec![&note, &other]] {
            let pdf = pdf_of(&notes, mode);
            assert!(!contains(&pdf, b"Brouillon"), "{mode:?}");
            let (_, read) = read_layers(&pdf);
            if mode == LayerMode::Separate {
                assert_eq!(read.groups, ["Calque 1"]);
            }
        }
    }
}

#[test]
fn a_page_whose_layers_are_all_hidden_has_no_group() {
    let (first, hidden) = (first_layered(), all_hidden());
    // Alone: no optional content at all, as for a merged PDF.
    let alone = pdf_of(&[&hidden], LayerMode::Separate);
    let (document, read) = read_layers(&alone);
    assert!(read.groups.is_empty() && read.properties == [Vec::new()]);
    assert_eq!(read.marks, [(0, 0)]);
    assert!(document.catalog().unwrap().get(b"PageMode").is_err());
    assert!(!contains(&alone, b"Cachee") && !contains(&alone, b"OCProperties"));
    // After a note with layers: that page has none, the document's groups are the first's.
    let pdf = pdf_of(&[&first, &hidden], LayerMode::Separate);
    let (_, read) = read_layers(&pdf);
    assert_eq!(read.groups, ["Correction", "Calque 1"]);
    assert!(read.properties[1].is_empty() && read.sequences[1].is_empty());
    assert!(!contains(&pdf, b"Cachee"));
}
