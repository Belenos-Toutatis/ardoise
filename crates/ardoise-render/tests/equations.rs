//! Equations (SPEC §4.4 « Équation », `docs/design/equations.md`) in every output, drawn from the
//! outlines they store (MathJax runs only in the editor): the raster images and their encodings
//! (PNG, JPEG, WebP, the transparent PNG), the SVG (one path carrying the LaTeX source as its
//! title, drawn by an independent renderer where the raster draws it) and the PDF (vector fills,
//! no invisible text). An equation this version cannot read, or of a hidden layer, draws nothing.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ardoise_core::note::{
    Background, EquationBody, Layer, Note, ObjectBody, Origin, PageObject, PageSize, Paper,
    Template, Timing,
};
use ardoise_ink::Rgba;
use ardoise_render::{
    Image, LayerMode, PdfOptions, RenderOptions, SvgOptions, decode_png, encode_jpeg, encode_png,
    encode_webp, note_to_svg, notes_to_pdf, render_note, render_note_transparent,
};

const PAGE: PageSize = PageSize {
    width: 8000,
    height: 5000,
};
/// 400 × 250 px.
const SCALE: f32 = 0.05;
const BLUE: Rgba = Rgba::rgb(20, 40, 200);

/// A bar (a fraction's rule) and a round shape of cubic curves, 400 × 250 units.
const DRAWING: &str =
    "M0 0L400 0L400 100L0 100ZM100 200C100 150 300 150 300 200C300 250 100 250 100 200Z";

fn note(layers: &[(&str, bool)], objects: Vec<PageObject>) -> Note {
    Note {
        id: "00000000-0000-4000-8000-0000000000ee".into(),
        title: "équations".into(),
        created: 0,
        modified: 0,
        page: PAGE,
        background: Background {
            paper: Paper::White,
            template: Template::Blank,
            texture: false,
        },
        background_image: None,
        tags: vec![],
        favorite: false,
        layers: layers
            .iter()
            .map(|&(name, visible)| Layer {
                name: name.into(),
                visible,
            })
            .collect(),
        strokes: vec![],
        objects,
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

fn equation(id: &str, layer: u32, at: [f32; 2], scale: f32, color: Rgba) -> PageObject {
    PageObject::equation(
        id,
        layer,
        0,
        at,
        scale,
        EquationBody {
            latex: r"\frac{1}{2} & \ce{H2O}".into(),
            path: DRAWING.into(),
            color,
        },
    )
}

/// Pixels that are not white, with their position.
fn inked(image: &Image) -> Vec<(u32, u32, [u8; 4])> {
    let mut out = Vec::new();
    for y in 0..image.height {
        for x in 0..image.width {
            let p = image.pixel(x, y).unwrap();
            if p[..3] != [255, 255, 255] && p[3] != 0 {
                out.push((x, y, p));
            }
        }
    }
    out
}

/// The box `[x0, y0, x1, y1]` of an object on the page, in pixels, grown by `margin`.
fn pixel_box(object: &PageObject, margin: f32) -> [f32; 4] {
    let [x0, y0, x1, y1] = object.page_bounds();
    [
        x0 * SCALE - margin,
        y0 * SCALE - margin,
        x1 * SCALE + margin,
        y1 * SCALE + margin,
    ]
}

fn inside([x0, y0, x1, y1]: [f32; 4], x: u32, y: u32) -> bool {
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    x >= x0 && x <= x1 && y >= y0 && y <= y1
}

fn plain() -> RenderOptions {
    RenderOptions {
        scale: SCALE,
        background: false,
    }
}

#[test]
fn the_raster_draws_an_equation_in_its_colour_inside_its_box() {
    let object = equation("e", 0, [1000.0, 1000.0], 2.0, BLUE);
    assert_eq!((object.width, object.height), (400.0, 250.0));
    let n = note(&[("1", true)], vec![object.clone()]);
    let image = render_note(&n, &plain()).unwrap();
    let ink = inked(&image);
    // The bar: 800 × 200 page units, 40 × 10 px of solid colour.
    assert!(ink.len() > 400, "{} px", ink.len());
    let area = pixel_box(&object, 1.0);
    assert!(ink.iter().all(|&(x, y, _)| inside(area, x, y)));
    assert_eq!(image.pixel(70, 52), Some([20, 40, 200, 255]), "the bar");
    assert_eq!(
        image.pixel(70, 70),
        Some([20, 40, 200, 255]),
        "the round shape"
    );
    assert_eq!(
        image.pixel(70, 60),
        Some([255, 255, 255, 255]),
        "between them"
    );

    // Every raster encoding carries it.
    let png = decode_png(&encode_png(&image).unwrap()).unwrap();
    assert_eq!(png, image);
    let jpeg = encode_jpeg(&image, 95).unwrap();
    let decoded = jpeg_decoder::Decoder::new(std::io::Cursor::new(&jpeg))
        .decode()
        .unwrap();
    let at = (52 * image.width as usize + 70) * 3;
    assert!(
        decoded[at + 2] > 150 && decoded[at] < 80,
        "{:?}",
        &decoded[at..at + 3]
    );
    let webp = encode_webp(&image).unwrap();
    let mut reader = image_webp::WebPDecoder::new(std::io::Cursor::new(&webp)).unwrap();
    let mut rgb = vec![0; reader.output_buffer_size().unwrap()];
    reader.read_image(&mut rgb).unwrap();
    assert_eq!(&rgb[at..at + 3], &[20, 40, 200]);
    // The transparent image: the equation alone.
    let clear = render_note_transparent(&n, SCALE).unwrap();
    assert_eq!(clear.pixel(70, 52), Some([20, 40, 200, 255]));
    assert_eq!(clear.pixel(10, 10), Some([0, 0, 0, 0]));
}

#[test]
fn a_turned_and_scaled_equation_lands_where_its_page_bounds_say() {
    for turns in 0..4u8 {
        let mut object = equation("e", 0, [4000.0, 2500.0], 1.5, BLUE);
        object.quarter_turns = turns;
        let n = note(&[("1", true)], vec![object.clone()]);
        let image = render_note(&n, &plain()).unwrap();
        let ink = inked(&image);
        assert!(!ink.is_empty(), "{turns}");
        let area = pixel_box(&object, 1.0);
        assert!(
            ink.iter().all(|&(x, y, _)| inside(area, x, y)),
            "{turns}: {area:?}"
        );
    }
}

#[test]
fn hidden_layers_and_unreadable_equations_draw_nothing() {
    let hidden = equation("h", 1, [1000.0, 1000.0], 2.0, BLUE);
    // What a reader makes of an equation whose drawing it cannot read (`ardoise-core`'s tests read
    // the hostile files): kept, of no kind it draws.
    let mut unreadable = equation("u", 0, [1000.0, 3000.0], 2.0, BLUE);
    unreadable.body = ObjectBody::Unknown {
        kind: "equation".into(),
        fields: Default::default(),
    };
    let n = note(&[("1", true), ("2", false)], vec![hidden, unreadable]);
    let image = render_note(&n, &plain()).unwrap();
    assert!(inked(&image).is_empty());
    let svg = note_to_svg(
        &n,
        &SvgOptions {
            background: false,
            layers: LayerMode::Separate,
        },
    );
    assert!(!svg.contains("<path"), "{svg}");
    // An equation whose stored drawing was replaced in memory by data outside the grammar draws
    // nothing either (and never panics).
    let mut broken = equation("b", 0, [1000.0, 1000.0], 2.0, BLUE);
    if let ObjectBody::Equation(body) = &mut broken.body {
        body.path = "M0 0 L".into();
    }
    let n = note(&[("1", true)], vec![broken]);
    assert!(inked(&render_note(&n, &plain()).unwrap()).is_empty());
}

#[test]
fn the_svg_writes_the_equation_as_one_path_titled_with_its_source_and_draws_it_where_the_raster_does()
 {
    let object = equation("e", 0, [1000.0, 1000.0], 2.0, BLUE);
    let n = note(&[("Calque 1", true)], vec![object]);
    let svg = note_to_svg(
        &n,
        &SvgOptions {
            background: false,
            layers: LayerMode::Separate,
        },
    );
    let doc = roxmltree::Document::parse(&svg).expect("valid XML");
    let path = doc
        .descendants()
        .find(|n| n.has_tag_name("path"))
        .expect("a path");
    assert_eq!(path.attribute("fill"), Some("#1428c8"));
    let d = path.attribute("d").unwrap();
    assert!(d.starts_with("M1000 1000L1800 1000"), "{d}");
    assert!(d.contains('C'), "the curves are kept: {d}");
    let title = path
        .children()
        .find(|c| c.has_tag_name("title"))
        .and_then(|t| t.text());
    assert_eq!(title, Some(r"\frac{1}{2} & \ce{H2O}"));
    // An independent renderer draws it where the raster does.
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(&svg, &usvg::Options::default()).unwrap();
    let scale = 400.0 / tree.size().width();
    let mut pixmap = tiny_skia::Pixmap::new(400, 250).unwrap();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let at = |x: u32, y: u32| {
        let p = pixmap.pixel(x, y).unwrap();
        [p.red(), p.green(), p.blue(), p.alpha()]
    };
    assert_eq!(at(70, 52), [20, 40, 200, 255]);
    assert_eq!(at(70, 60)[3], 0);
}

#[test]
fn the_pdf_fills_the_equation_with_curves_and_no_invisible_text() {
    let object = equation("e", 0, [1000.0, 1000.0], 2.0, BLUE);
    let n = note(&[("1", true)], vec![object]);
    let pdf = notes_to_pdf(
        &[&n],
        &PdfOptions {
            background: false,
            ..PdfOptions::default()
        },
    )
    .unwrap();
    let document = lopdf::Document::load_mem(&pdf).unwrap();
    let (_, &page) = document.get_pages().iter().next().unwrap();
    let content = document.get_page_content(page);
    let ops = lopdf::content::Content::decode(&content)
        .unwrap()
        .operations;
    let count = |name: &str| ops.iter().filter(|op| op.operator == name).count();
    assert_eq!(count("f"), 1);
    assert_eq!(count("c"), 2, "the round shape's two cubic curves");
    assert_eq!(count("BT"), 0, "an equation's LaTeX is not written as text");
    assert!(document.get_page_fonts(page).unwrap().is_empty());
    // The colour, as a fill colour (0..1).
    let colour = ops
        .iter()
        .find(|op| op.operator == "rg")
        .map(|op| op.operands.clone())
        .unwrap();
    let rgb: Vec<f32> = colour
        .iter()
        .map(|o| match o {
            lopdf::Object::Real(r) => *r,
            lopdf::Object::Integer(i) => *i as f32,
            other => panic!("{other:?}"),
        })
        .collect();
    assert!((rgb[2] - 200.0 / 255.0).abs() < 0.01, "{rgb:?}");
}
