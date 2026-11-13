//! Text frames (SPEC §4.4, « Texte tapé au clavier ») in every output: the raster images (their
//! glyphs where the frame's placement puts them, in their colour, in drawing order), the SVG
//! (curved paths carrying their words, in their layer) and the PDF (curved paths, optional
//! content groups, invisible text that makes the words searchable), each read back by an
//! independent reader (`roxmltree`, `resvg`, `lopdf`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ardoise_core::note::{
    Background, Layer, Note, ObjectBody, Origin, PageObject, PageSize, Paper, Template, TextBody,
    Timing,
};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_render::{
    Backdrop, DrawItem, Image, LayerMode, PageCanvas, PdfOptions, RenderOptions, SvgOptions,
    draw_items, note_to_svg, notes_to_pdf, render_note, render_note_transparent, scale_for_dpi,
};
use ardoise_text::{FontFamily, TextStyle, layout};

const PAGE: PageSize = PageSize {
    width: 8000,
    height: 5000,
};
/// 400 × 250 px.
const SCALE: f32 = 0.05;
const RED: Rgba = Rgba::rgb(220, 20, 20);

fn note(layers: &[(&str, bool)], strokes: Vec<Stroke>, objects: Vec<PageObject>) -> Note {
    Note {
        id: "00000000-0000-4000-8000-0000000000aa".into(),
        title: "texte".into(),
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
        strokes,
        objects,
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

fn body(text: &str, font: FontFamily, bold: bool, size: f32, color: Rgba) -> TextBody {
    TextBody {
        text: text.into(),
        font,
        bold,
        size,
        color,
    }
}

fn frame(id: &str, layer: u32, x: f32, y: f32, body: TextBody) -> PageObject {
    PageObject::text(id, layer, 0, x, y, body)
}

/// A straight ballpoint stroke.
fn line(layer: u32, from: (f32, f32), to: (f32, f32), width: f32, color: Rgba) -> Stroke {
    Stroke {
        tool: Tool::Ballpoint,
        color,
        width,
        layer,
        points: (0..=20)
            .map(|i| {
                let u = i as f32 / 20.0;
                Point {
                    x: from.0 + (to.0 - from.0) * u,
                    y: from.1 + (to.1 - from.1) * u,
                    pressure: 1.0,
                    t: i,
                }
            })
            .collect(),
    }
}

fn plain(scale: f32) -> RenderOptions {
    RenderOptions {
        scale,
        background: false,
    }
}

/// The frame's box on the page in pixels `[x0, y0, x1, y1]`, grown by `margin` pixels.
fn pixel_box(object: &PageObject, scale: f32, margin: f32) -> [f32; 4] {
    let [x0, y0, x1, y1] = object.page_bounds();
    [
        x0 * scale - margin,
        y0 * scale - margin,
        x1 * scale + margin,
        y1 * scale + margin,
    ]
}

fn inside([x0, y0, x1, y1]: [f32; 4], x: u32, y: u32) -> bool {
    let (x, y) = (x as f32 + 0.5, y as f32 + 0.5);
    x >= x0 && x <= x1 && y >= y0 && y <= y1
}

/// Pixels that are not white, with their position.
fn inked(image: &Image) -> Vec<(u32, u32, [u8; 4])> {
    let mut out = Vec::new();
    for y in 0..image.height {
        for x in 0..image.width {
            let p = image.pixel(x, y).unwrap();
            if p != [255, 255, 255, 255] {
                out.push((x, y, p));
            }
        }
    }
    out
}

/// Bounds `[x0, y0, x1, y1]` of the inked pixels (their edges).
fn ink_bounds(ink: &[(u32, u32, [u8; 4])]) -> [f32; 4] {
    ink.iter().fold(
        [f32::INFINITY, f32::INFINITY, 0.0, 0.0],
        |[x0, y0, x1, y1], &(x, y, _)| {
            let (x, y) = (x as f32, y as f32);
            [x0.min(x), y0.min(y), x1.max(x + 1.0), y1.max(y + 1.0)]
        },
    )
}

// --- Raster ---

#[test]
fn a_frame_is_drawn_in_its_colour_inside_its_box_and_nowhere_else() {
    let object = frame(
        "a",
        0,
        1000.0,
        1000.0,
        body("Bonjour", FontFamily::Andika, true, 900.0, RED),
    );
    let n = note(&[("1", true)], vec![], vec![object.clone()]);
    let image = render_note(&n, &plain(SCALE)).unwrap();
    let ink = inked(&image);
    let area = pixel_box(&object, SCALE, 1.0);
    for &(x, y, p) in &ink {
        assert!(
            inside(area, x, y),
            "ink at ({x}, {y}) {p:?} outside {area:?}"
        );
        // The frame's colour, anti-aliased over white: red stays the strongest channel.
        assert!(p[0] >= p[1] && p[1] == p[2], "({x}, {y}) {p:?}");
    }
    // Fully covered pixels hold exactly the colour; a good part of the box is ink.
    let solid = ink
        .iter()
        .filter(|(.., p)| *p == [220, 20, 20, 255])
        .count();
    assert!(solid > 200, "{solid} pixels of the exact colour");
    let [x0, y0, x1, y1] = pixel_box(&object, SCALE, 0.0);
    assert!(
        ink.len() as f32 > 0.15 * (x1 - x0) * (y1 - y0),
        "{}",
        ink.len()
    );
    // The ink spans the box's width.
    let [ix0, _, ix1, _] = ink_bounds(&ink);
    assert!(ix1 - ix0 > 0.9 * (x1 - x0), "{ix0} {ix1} for {x0} {x1}");
    // With the background, the same pixels on the paper.
    let n_with_paper = render_note(
        &n,
        &RenderOptions {
            scale: SCALE,
            background: true,
        },
    )
    .unwrap();
    assert_eq!(n_with_paper, image);
}

#[test]
fn a_turned_and_scaled_frame_lands_where_its_page_bounds_say() {
    let mut coverages = Vec::new();
    for quarter_turns in 0..4u8 {
        let mut object = frame(
            "t",
            0,
            4000.0,
            2500.0,
            body("Turn", FontFamily::LatinModern, false, 600.0, RED),
        );
        object.scale = 1.6;
        object.quarter_turns = quarter_turns;
        let n = note(&[("1", true)], vec![], vec![object.clone()]);
        let image = render_note(&n, &plain(SCALE)).unwrap();
        let ink = inked(&image);
        assert!(!ink.is_empty(), "{quarter_turns}");
        let area = pixel_box(&object, SCALE, 1.0);
        for &(x, y, _) in &ink {
            assert!(
                inside(area, x, y),
                "{quarter_turns}: ({x}, {y}) outside {area:?}"
            );
        }
        // The text runs along the box's long side, from end to end.
        let [x0, y0, x1, y1] = pixel_box(&object, SCALE, 0.0);
        let [ix0, iy0, ix1, iy1] = ink_bounds(&ink);
        let (along, box_along) = if quarter_turns % 2 == 0 {
            (ix1 - ix0, x1 - x0)
        } else {
            (iy1 - iy0, y1 - y0)
        };
        assert!(
            along > 0.9 * box_along,
            "{quarter_turns}: {along} of {box_along}"
        );
        // Turned, the frame keeps its size: as much ink in every direction.
        let coverage: f32 = ink
            .iter()
            .map(|(.., p)| f32::from(255 - p[1]) / 235.0)
            .sum();
        coverages.push(coverage);
    }
    for c in &coverages {
        assert!((c / coverages[0] - 1.0).abs() < 0.02, "{coverages:?}");
    }
    // Twice the scale, four times the ink.
    let mut small = frame(
        "s",
        0,
        1000.0,
        1000.0,
        body("Turn", FontFamily::LatinModern, false, 600.0, RED),
    );
    small.scale = 0.8;
    let image = render_note(&note(&[("1", true)], vec![], vec![small]), &plain(SCALE)).unwrap();
    let coverage: f32 = inked(&image)
        .iter()
        .map(|(.., p)| f32::from(255 - p[1]) / 235.0)
        .sum();
    assert!(
        (coverages[0] / coverage - 4.0).abs() < 0.2,
        "{} {coverage}",
        coverages[0]
    );
}

#[test]
fn hidden_layers_and_unknown_kinds_draw_nothing() {
    let hidden = frame(
        "h",
        1,
        1000.0,
        1000.0,
        body("Caché", FontFamily::Andika, false, 900.0, RED),
    );
    let mut unknown = frame(
        "u",
        0,
        1000.0,
        3000.0,
        body("Inconnu", FontFamily::Andika, false, 900.0, RED),
    );
    unknown.body = ObjectBody::Unknown {
        kind: "equation".into(),
        fields: Default::default(),
    };
    // A text frame this version cannot read is an unknown body too.
    let mut unreadable = unknown.clone();
    unreadable.body = ObjectBody::Unknown {
        kind: "text".into(),
        fields: Default::default(),
    };
    let n = note(
        &[("A", true), ("B", false)],
        vec![],
        vec![hidden, unknown.clone(), unreadable],
    );
    assert!(draw_items(&n).is_empty());
    let blank = note(&[("A", true), ("B", false)], vec![], vec![]);
    for options in [plain(SCALE), RenderOptions::at_dpi(72, true)] {
        assert_eq!(
            render_note(&n, &options).unwrap(),
            render_note(&blank, &options).unwrap()
        );
    }
    let mut canvas = PageCanvas::new(&n, SCALE, Backdrop::White).unwrap();
    let before = canvas.to_image();
    assert_eq!(canvas.draw_object(&unknown), None);
    assert_eq!(canvas.draw_item(&n, DrawItem::Object(1)), None);
    assert_eq!(canvas.draw_item(&n, DrawItem::Object(99)), None);
    assert_eq!(canvas.to_image(), before);
    // Nor in the vector formats.
    let svg = note_to_svg(&n, &SvgOptions::default());
    assert!(!svg.contains("<path") && !svg.contains("Caché") && !svg.contains("Inconnu"));
    let pdf = notes_to_pdf(&[&n], &PdfOptions::default()).unwrap();
    let document = lopdf::Document::load_mem(&pdf).unwrap();
    let text = document.extract_text(&[1]).unwrap_or_default();
    assert!(
        !text.contains("Cach") && !text.contains("Incon"),
        "{text:?}"
    );
    assert!(!page_content(&document).iter().any(|op| op.operator == "BT"));
}

/// The objects of a layer go over its strokes (whatever their order in the note) and under the
/// layers above; an object whose layer has no entry goes on top.
#[test]
fn objects_go_over_their_layers_strokes_and_under_the_layers_above() {
    let style = TextStyle {
        family: FontFamily::Andika,
        bold: true,
        size: 1800.0,
    };
    let text = |id, layer, color| {
        frame(
            id,
            layer,
            1500.0,
            1200.0,
            body("IIIII", style.family, style.bold, style.size, color),
        )
    };
    // Two rows across the letters' stems.
    let [_, top, _, bottom] = ardoise_text::text_outline("IIIII", style)
        .1
        .bounds()
        .unwrap();
    let black_y = 1200.0 + top + 0.35 * (bottom - top);
    let blue_y = 1200.0 + top + 0.7 * (bottom - top);
    let strokes = vec![
        // Top layer, across the text.
        line(1, (500.0, black_y), (7500.0, black_y), 300.0, Rgba::BLACK),
        // Bottom layer, across the text.
        line(
            0,
            (500.0, blue_y),
            (7500.0, blue_y),
            300.0,
            Rgba::rgb(20, 40, 200),
        ),
    ];
    let objects = vec![
        text("bottom", 0, RED),
        // No layer entry: drawn on top.
        text("orphan", 9, Rgba::rgb(0, 160, 0)),
    ];
    let mut n = note(&[("Bas", true), ("Haut", true)], strokes, objects);
    // Shifted so that the two frames' stems do not coincide.
    n.objects[1].x += 150.0;
    assert_eq!(
        draw_items(&n),
        [
            DrawItem::Stroke(1),
            DrawItem::Object(0),
            DrawItem::Stroke(0),
            DrawItem::Object(1)
        ]
    );
    let image = render_note(&n, &plain(SCALE)).unwrap();
    let draw = |order: &[DrawItem]| {
        let mut canvas = PageCanvas::new(&n, SCALE, Backdrop::White).unwrap();
        for &item in order {
            canvas.draw_item(&n, item);
        }
        canvas.to_image()
    };
    assert_eq!(draw(&draw_items(&n)), image);
    // Another order shows: the bottom stroke over the text would not be the same picture.
    let other = [
        DrawItem::Object(0),
        DrawItem::Stroke(1),
        DrawItem::Stroke(0),
        DrawItem::Object(1),
    ];
    assert_ne!(draw(&other), image);
    // Where the bottom layer's blue stroke crosses a red stem, red; where the top layer's black
    // stroke crosses it, black; the orphan's green is over everything.
    let count = |rgb: [u8; 3], row: u32| {
        (0..image.width)
            .filter(|&x| image.pixel(x, row).unwrap()[..3] == rgb)
            .count()
    };
    let (black_row, blue_row) = ((black_y * SCALE) as u32, (blue_y * SCALE) as u32);
    assert!(count([220, 20, 20], blue_row) > 10, "red over blue");
    assert_eq!(count([220, 20, 20], black_row), 0, "black over red");
    assert!(count([0, 160, 0], black_row) > 10, "green over black");
}

#[test]
fn the_colour_alpha_is_honoured() {
    let translucent = Rgba {
        a: 128,
        ..Rgba::rgb(0, 0, 200)
    };
    let object = frame(
        "a",
        0,
        1000.0,
        1000.0,
        body("HHH", FontFamily::Andika, true, 1500.0, translucent),
    );
    let n = note(&[("1", true)], vec![], vec![object]);
    let image = render_note(&n, &plain(SCALE)).unwrap();
    let ink = inked(&image);
    // Fully covered: half the colour over white, never darker.
    assert!(
        ink.iter().all(|(.., p)| p[0] >= 126),
        "darker than the colour at half"
    );
    let full = ink
        .iter()
        .filter(|(.., p)| p[0].abs_diff(127) <= 1 && p[2].abs_diff(227) <= 1)
        .count();
    assert!(full > 100, "{full}");
    // On a transparent page, the colour itself at half opacity.
    let clear = render_note_transparent(&n, SCALE).unwrap();
    let alphas: Vec<[u8; 4]> = clear
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .copied()
        .filter(|p| p[3] > 0)
        .collect();
    assert!(alphas.iter().all(|p| p[3] <= 128));
    let solid = alphas
        .iter()
        .filter(|p| p[3] == 128 && p[2].abs_diff(200) <= 1 && p[0] == 0)
        .count();
    assert!(solid > 100, "{solid}");
}

#[test]
fn a_canvas_draws_an_object_only_within_its_bounds() {
    let mut object = frame(
        "a",
        0,
        3000.0,
        2000.0,
        body("Élève ça\nœuvre", FontFamily::LatinModern, true, 900.0, RED),
    );
    object.quarter_turns = 3;
    let n = note(&[("1", true)], vec![], vec![object.clone()]);
    let base = PageCanvas::new(&n, 0.06, Backdrop::Paper).unwrap();
    let before = base.to_image();
    let prepared = base.prepare_object(&object);
    let bounds = prepared.bounds().unwrap();
    let mut drawn = base.clone();
    assert_eq!(drawn.draw_object(&object), Some(bounds));
    let after = drawn.to_image();
    let mut changed = 0;
    for y in 0..after.height {
        for x in 0..after.width {
            if after.pixel(x, y) != before.pixel(x, y) {
                changed += 1;
                let pixel = ardoise_render::PixelRect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                };
                assert!(bounds.contains(pixel), "({x}, {y}) outside {bounds:?}");
            }
        }
    }
    assert!(changed > 100);
    let mut again = base.clone();
    again.draw_prepared_object(&prepared);
    assert_eq!(again.to_image(), after);
    // Off the page: nothing to draw.
    object.x = -50_000.0;
    assert_eq!(base.prepare_object(&object).bounds(), None);
}

// --- SVG ---

/// Bottom layer: a stroke and an Andika frame (TrueType: quadratic curves); a hidden layer with
/// a frame; a top layer holding only a translucent Latin Modern frame (CFF: cubic curves).
fn layered() -> Note {
    let strokes = vec![line(
        0,
        (500.0, 4500.0),
        (7500.0, 4500.0),
        80.0,
        Rgba::BLACK,
    )];
    let objects = vec![
        frame(
            "bonjour",
            0,
            500.0,
            500.0,
            body(
                "Bonjour <tout> le\nmonde & \"vous\"",
                FontFamily::Andika,
                false,
                600.0,
                RED,
            ),
        ),
        frame(
            "secret",
            1,
            500.0,
            2500.0,
            body("mots secrets", FontFamily::Andika, false, 600.0, RED),
        ),
        frame(
            "eleve",
            2,
            4000.0,
            2500.0,
            body(
                "Élève",
                FontFamily::LatinModern,
                false,
                900.0,
                Rgba {
                    a: 128,
                    ..Rgba::rgb(0, 0, 200)
                },
            ),
        ),
    ];
    note(
        &[("Bas", true), ("Secret", false), ("Haut & co", true)],
        strokes,
        objects,
    )
}

fn attr<'a>(node: roxmltree::Node<'a, '_>, name: &str) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == name)
        .map(|a| a.value())
}

fn paths<'a, 'input>(g: roxmltree::Node<'a, 'input>) -> Vec<roxmltree::Node<'a, 'input>> {
    g.children().filter(|c| c.has_tag_name("path")).collect()
}

fn title(node: roxmltree::Node<'_, '_>) -> Option<String> {
    node.children()
        .find(|c| c.has_tag_name("title"))
        .and_then(|t| t.text().map(str::to_owned))
}

#[test]
fn the_svg_writes_each_frame_as_a_curved_path_with_its_words_in_its_layer() {
    let n = layered();
    let svg = note_to_svg(
        &n,
        &SvgOptions {
            background: false,
            layers: LayerMode::Separate,
        },
    );
    let document = roxmltree::Document::parse(&svg).expect("valid XML");
    let layers: Vec<roxmltree::Node> = document
        .descendants()
        .filter(|g| g.has_tag_name("g") && attr(*g, "groupmode") == Some("layer"))
        .collect();
    let ids: Vec<&str> = layers.iter().filter_map(|g| attr(*g, "id")).collect();
    // The text-only top layer has its group; the hidden one has none.
    assert_eq!(ids, ["layer1", "layer3"]);
    assert_eq!(attr(layers[1], "label"), Some("Haut & co"));
    assert!(!svg.contains("secrets"));

    // Bottom layer: the stroke first, then the frame over it.
    let bottom = paths(layers[0]);
    assert_eq!(bottom.len(), 2);
    assert_eq!(title(bottom[0]), None);
    let text = bottom[1];
    assert_eq!(
        title(text).as_deref(),
        Some("Bonjour <tout> le\nmonde & \"vous\"")
    );
    let d = attr(text, "d").unwrap();
    assert!(
        d.contains('Q') && d.starts_with('M') && d.contains('Z'),
        "{d:.80}"
    );
    assert_eq!(attr(text, "fill"), Some("#dc1414"));
    assert_eq!(attr(text, "fill-rule"), Some("nonzero"));
    assert_eq!(attr(text, "fill-opacity"), None);
    // Top layer: the translucent Latin Modern frame, cubic curves.
    let top = paths(layers[1]);
    assert_eq!(top.len(), 1);
    assert_eq!(title(top[0]).as_deref(), Some("Élève"));
    assert!(attr(top[0], "d").unwrap().contains('C'));
    assert_eq!(attr(top[0], "fill"), Some("#0000c8"));
    assert_eq!(attr(top[0], "fill-opacity"), Some("0.502"));

    // Merged: one group, each layer's stroke then its frames, bottom layer first.
    let merged = note_to_svg(
        &n,
        &SvgOptions {
            background: true,
            layers: LayerMode::Merged,
        },
    );
    let document = roxmltree::Document::parse(&merged).expect("valid XML");
    let ink = document
        .descendants()
        .find(|g| g.has_tag_name("g") && attr(*g, "id") == Some("ink"))
        .unwrap();
    let titles: Vec<Option<String>> = paths(ink).into_iter().map(title).collect();
    assert_eq!(
        titles,
        [
            None,
            Some("Bonjour <tout> le\nmonde & \"vous\"".to_owned()),
            Some("Élève".to_owned())
        ]
    );
    assert_eq!(attr(ink, "label"), Some("Bas + Haut & co"));
}

/// `svg` drawn by resvg over white at the scale of a raster of `width × height` pixels, as RGB.
fn draw_svg(svg: &str, dpi: u32, width: u32, height: u32) -> Vec<[u8; 3]> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("usvg parses");
    let scale = scale_for_dpi(dpi) * PAGE.width as f32 / tree.size().width();
    let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    pixmap
        .pixels()
        .iter()
        .map(|p| {
            let white = 255 - p.alpha();
            [p.red() + white, p.green() + white, p.blue() + white]
        })
        .collect()
}

/// SPEC §6.2: the SVG, drawn by an independent renderer, shows the frames where the PNG shows
/// them (turned, scaled, translucent).
#[test]
fn the_svg_draws_the_frames_the_raster_draws() {
    let mut n = layered();
    for (i, (turns, scale)) in [(1u8, 1.4f32), (2, 0.7), (3, 1.0)].into_iter().enumerate() {
        let mut object = frame(
            &format!("turned{i}"),
            2,
            2000.0 + 1800.0 * i as f32,
            2500.0,
            body("Tourné àé", FontFamily::Andika, i == 1, 500.0, RED),
        );
        object.quarter_turns = turns;
        object.scale = scale;
        n.objects.push(object);
    }
    let dpi = 100;
    let raster = render_note(&n, &RenderOptions::at_dpi(dpi, false)).unwrap();
    let svg = note_to_svg(
        &n,
        &SvgOptions {
            background: false,
            ..SvgOptions::default()
        },
    );
    let drawn = draw_svg(&svg, dpi, raster.width, raster.height);
    let mut total = 0u64;
    let mut gross = 0usize;
    for (r, s) in raster.pixels.as_chunks::<4>().0.iter().zip(&drawn) {
        let d: Vec<u8> = (0..3).map(|c| r[c].abs_diff(s[c])).collect();
        total += d.iter().map(|&v| u64::from(v)).sum::<u64>();
        gross += usize::from(d.iter().any(|&v| v > 64));
    }
    let pixels = drawn.len() as f64;
    let (mean, gross) = (total as f64 / (pixels * 3.0), gross as f64 / pixels);
    println!(
        "text frames, SVG drawn by resvg vs PNG: mean difference {mean:.4} / 255, {:.4} % of \
         pixels off by more than 64",
        gross * 100.0
    );
    // Only anti-aliasing differs (resvg flattens the curves its own way): measured 0.014 and
    // no pixel.
    assert!(mean < 0.1, "mean difference {mean}");
    assert!(gross < 0.000_1, "share of very different pixels {gross}");
    // A frame missing from the SVG would be caught.
    let mut fewer = n.clone();
    fewer.objects.pop();
    let drawn = draw_svg(
        &note_to_svg(
            &fewer,
            &SvgOptions {
                background: false,
                ..SvgOptions::default()
            },
        ),
        dpi,
        raster.width,
        raster.height,
    );
    let off = raster
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(&drawn)
        .filter(|(r, s)| (0..3).any(|c| r[c].abs_diff(s[c]) > 64))
        .count();
    assert!(off > 50, "{off}");
}

// --- PDF ---

fn page_content(document: &lopdf::Document) -> Vec<lopdf::content::Operation> {
    let (_, &page) = document.get_pages().iter().next().unwrap();
    let content = document.get_page_content(page);
    lopdf::content::Content::decode(&content)
        .unwrap()
        .operations
}

fn number(object: &lopdf::Object) -> f32 {
    match object {
        lopdf::Object::Integer(i) => *i as f32,
        lopdf::Object::Real(r) => *r,
        other => panic!("not a number: {other:?}"),
    }
}

/// The page's fonts: resource name → (base font, encoding, first char, widths).
fn page_fonts(document: &lopdf::Document) -> Vec<(String, String, String, i64, Vec<f32>)> {
    let (_, &page) = document.get_pages().iter().next().unwrap();
    let mut fonts: Vec<_> = document
        .get_page_fonts(page)
        .unwrap()
        .into_iter()
        .map(|(name, font)| {
            let name_of = |key: &[u8]| {
                String::from_utf8_lossy(font.get(key).unwrap().as_name().unwrap()).into_owned()
            };
            let widths = font
                .get(b"Widths")
                .unwrap()
                .as_array()
                .unwrap()
                .iter()
                .map(number)
                .collect();
            (
                String::from_utf8_lossy(&name).into_owned(),
                name_of(b"BaseFont"),
                name_of(b"Encoding"),
                font.get(b"FirstChar").unwrap().as_i64().unwrap(),
                widths,
            )
        })
        .collect();
    fonts.sort_by(|a, b| a.0.cmp(&b.0));
    fonts
}

#[test]
fn the_pdf_paints_frames_as_curves_and_carries_their_words() {
    let n = layered();
    let options = PdfOptions {
        background: false,
        ..PdfOptions::default()
    };
    let pdf = notes_to_pdf(&[&n], &options).unwrap();
    let document = lopdf::Document::load_mem(&pdf).unwrap();
    let ops = page_content(&document);
    let count = |name: &str| ops.iter().filter(|op| op.operator == name).count();
    // The stroke and the two visible frames: three fills, the frames' curves as `c`.
    assert_eq!(count("f"), 3);
    assert!(count("c") > 50, "{} curves", count("c"));
    // One invisible text object per line: two for « Bonjour… », one for « Élève ».
    assert_eq!(count("BT"), 3);
    assert_eq!(count("ET"), 3);
    let modes: Vec<f32> = ops
        .iter()
        .filter(|op| op.operator == "Tr")
        .map(|op| number(&op.operands[0]))
        .collect();
    assert_eq!(modes, [3.0; 3]);
    // Text extraction finds every word of the visible frames, line by line.
    let text = document.extract_text(&[1]).unwrap();
    for words in ["Bonjour <tout> le", "monde & \"vous\"", "Élève"] {
        assert!(text.contains(words), "{words:?} not in {text:?}");
    }
    assert!(!text.contains("secrets"), "{text:?}");
    // The fonts: never embedded, WinAnsiEncoding, the advances of our glyphs.
    let fonts = page_fonts(&document);
    let names: Vec<(&str, &str, &str, i64)> = fonts
        .iter()
        .map(|(n, b, e, first, _)| (n.as_str(), b.as_str(), e.as_str(), *first))
        .collect();
    assert_eq!(
        names,
        [
            ("T0", "Helvetica", "WinAnsiEncoding", 32),
            ("T1", "Times-Roman", "WinAnsiEncoding", 32)
        ]
    );
    let advance = |family, c: char| {
        layout(
            &c.to_string(),
            TextStyle {
                family,
                bold: false,
                size: 1000.0,
            },
        )
        .width
    };
    for (font, family) in [
        (&fonts[0], FontFamily::Andika),
        (&fonts[1], FontFamily::LatinModern),
    ] {
        let widths = &font.4;
        assert_eq!(widths.len(), 224);
        // Latin-1 characters have their own code.
        for c in ['B', ' ', 'é', 'É'] {
            let code = c as usize;
            assert!((widths[code - 32] - advance(family, c)).abs() < 0.01, "{c}");
        }
        // '€' is 0x80 in WinAnsiEncoding.
        assert!((widths[0x80 - 32] - advance(family, '€')).abs() < 0.01);
        assert_eq!(widths[0x81 - 32], 0.0);
    }
    // The first line starts on the frame's first baseline, upright, at the text's size.
    let first = &n.objects[0];
    let baseline = first.as_text().unwrap().layout().lines[0].baseline;
    let tm: Vec<f32> = ops
        .iter()
        .find(|op| op.operator == "Tm")
        .unwrap()
        .operands
        .iter()
        .map(number)
        .collect();
    let expected = [600.0, 0.0, 0.0, -600.0, first.x, first.y + baseline];
    for (got, want) in tm.iter().zip(expected) {
        assert!((got - want).abs() < 0.02, "{tm:?} instead of {expected:?}");
    }
}

#[test]
fn a_turned_frame_s_invisible_text_turns_with_it_and_other_scripts_are_left_out() {
    let mut object = frame(
        "g",
        0,
        6000.0,
        1000.0,
        body("α = 2·π r\ts", FontFamily::Andika, true, 400.0, RED),
    );
    object.quarter_turns = 1;
    object.scale = 2.0;
    let n = note(&[("1", true)], vec![], vec![object.clone()]);
    let pdf = notes_to_pdf(&[&n], &PdfOptions::default()).unwrap();
    let document = lopdf::Document::load_mem(&pdf).unwrap();
    let ops = page_content(&document);
    let tm: Vec<f32> = ops
        .iter()
        .find(|op| op.operator == "Tm")
        .unwrap()
        .operands
        .iter()
        .map(number)
        .collect();
    // Clockwise: the line runs down the page, the glyphs' tops face right.
    let laid = object.as_text().unwrap().layout();
    let line = &laid.lines[0];
    // The first character shown is the space after « α ».
    let x0 = line.glyphs[1].x;
    let [ex, ey] = object.page_point([x0, line.baseline]);
    let expected = [0.0, 800.0, 800.0, 0.0, ex, ey];
    for (got, want) in tm.iter().zip(expected) {
        assert!((got - want).abs() < 0.02, "{tm:?} instead of {expected:?}");
    }
    assert_eq!(page_fonts(&document)[0].1, "Helvetica-Bold");
    let text = document.extract_text(&[1]).unwrap();
    assert!(
        text.contains("= 2·") && text.contains('r') && text.contains('s'),
        "{text:?}"
    );
    assert!(!text.contains('α') && !text.contains('π'), "{text:?}");
    // The first Greek letter is skipped by starting after it; the second one and the tab are
    // steps of the positioning array.
    let tj = ops.iter().find(|op| op.operator == "TJ").unwrap();
    let steps = tj.operands[0]
        .as_array()
        .unwrap()
        .iter()
        .filter(|o| !matches!(o, lopdf::Object::String(..)))
        .count();
    assert_eq!(steps, 2, "{:?}", tj.operands);
}

#[test]
fn a_layer_holding_only_text_has_its_optional_content_group() {
    let n = layered();
    let options = PdfOptions {
        layers: LayerMode::Separate,
        ..PdfOptions::default()
    };
    let pdf = notes_to_pdf(&[&n, &n], &options).unwrap();
    let document = lopdf::Document::load_mem(&pdf).unwrap();
    let catalog = document.catalog().unwrap();
    let properties = catalog.get(b"OCProperties").unwrap().as_dict().unwrap();
    let names: Vec<String> = properties
        .get(b"OCGs")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|r| {
            let group = document.get_dictionary(r.as_reference().unwrap()).unwrap();
            match group.get(b"Name").unwrap() {
                lopdf::Object::String(bytes, _) => String::from_utf8_lossy(bytes).into_owned(),
                other => panic!("{other:?}"),
            }
        })
        .collect();
    // Numbered top layer first; the hidden layer has none.
    assert_eq!(names, ["Haut & co", "Bas"]);
    // The top layer's frame, path and invisible text, inside its marked content.
    let ops = page_content(&document);
    let start = ops
        .iter()
        .rposition(|op| op.operator == "BDC")
        .expect("marked content");
    let end = start
        + ops[start..]
            .iter()
            .position(|op| op.operator == "EMC")
            .unwrap();
    let inside: Vec<&str> = ops[start..end]
        .iter()
        .map(|op| op.operator.as_str())
        .collect();
    assert!(inside.contains(&"f") && inside.contains(&"c") && inside.contains(&"TJ"));
    assert_eq!(document.get_pages().len(), 2);
    for page in [1, 2] {
        assert!(document.extract_text(&[page]).unwrap().contains("Élève"));
    }
}
