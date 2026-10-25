//! Background images (SPEC §4.4, P2) in every format: drawn over the paper and the template,
//! under the strokes, turned, translucent and clipped to the page in the raster formats; placed
//! exactly in SVG (drawn back by resvg) and embedded as an image XObject in PDF (read back by
//! lopdf).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ardoise_core::note::{
    Background, BackgroundImage, ImageRect, Layer, Note, Origin, PageSize, Paper, Template, Timing,
};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_render::{
    Backdrop, Image, NoteView, PageCanvas, PdfOptions, Picture, RenderOptions, SvgOptions,
    drawing_order, encode_jpeg, encode_png, note_to_svg, notes_to_pdf, paper_color, render_note,
    scale_for_dpi,
};

const PAGE: PageSize = PageSize {
    width: 5000,
    height: 4000,
};
/// 0.05 px per unit: the page is 250 × 200 px.
const SCALE: f32 = 0.05;
const RED: [u8; 3] = [230, 20, 20];
const GREEN: [u8; 3] = [20, 200, 40];
const BLUE: [u8; 3] = [20, 40, 220];
const YELLOW: [u8; 3] = [240, 220, 20];

fn image(width: u32, height: u32, f: impl Fn(u32, u32) -> [u8; 4]) -> Image {
    let pixels = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| f(x, y))
        .collect();
    Image {
        width,
        height,
        pixels,
    }
}

fn rgba([r, g, b]: [u8; 3]) -> [u8; 4] {
    [r, g, b, 255]
}

/// 40 × 20 pixels: red, green on top; blue, yellow below.
fn quadrants() -> Image {
    image(40, 20, |x, y| match (x < 20, y < 10) {
        (true, true) => rgba(RED),
        (false, true) => rgba(GREEN),
        (true, false) => rgba(BLUE),
        (false, false) => rgba(YELLOW),
    })
}

fn png_picture(image: &Image) -> Picture {
    Picture::decode(encode_png(image).unwrap()).unwrap()
}

fn note(background: Background, image: Option<BackgroundImage>, strokes: Vec<Stroke>) -> Note {
    Note {
        id: "00000000-0000-4000-8000-000000000001".into(),
        title: "fond".into(),
        created: 0,
        modified: 0,
        page: PAGE,
        background,
        background_image: image,
        tags: vec![],
        favorite: false,
        layers: vec![Layer {
            name: "1".into(),
            visible: true,
        }],
        strokes,
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

fn white() -> Background {
    Background {
        paper: Paper::White,
        template: Template::Blank,
        texture: false,
    }
}

/// The image over `rect`, `quarter_turns` clockwise, at `opacity`.
fn placed(rect: ImageRect, quarter_turns: u8, opacity: f32) -> BackgroundImage {
    BackgroundImage {
        name: "fiche.png".into(),
        pdf_page: None,
        rect,
        quarter_turns,
        opacity,
    }
}

/// 2000 × 1000 units at (1000, 1000): pixels 50..150 × 50..100 of the page.
const RECT: ImageRect = ImageRect {
    x: 1000.0,
    y: 1000.0,
    width: 2000.0,
    height: 1000.0,
};

/// The four quadrant centres of [`RECT`], in pixels: top left, top right, bottom left, bottom
/// right.
const CENTRES: [(u32, u32); 4] = [(75, 62), (125, 62), (75, 87), (125, 87)];

fn rgb_at(image: &Image, (x, y): (u32, u32)) -> [u8; 3] {
    let [r, g, b, _] = image.pixel(x, y).unwrap();
    [r, g, b]
}

fn close(a: [u8; 3], b: [u8; 3], tolerance: u8) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tolerance)
}

fn line(color: Rgba, from: (f32, f32), to: (f32, f32)) -> Stroke {
    Stroke {
        tool: Tool::Ballpoint,
        color,
        width: 120.0,
        layer: 0,
        points: (0..=20)
            .map(|i| {
                let t = i as f32 / 20.0;
                Point {
                    x: from.0 + (to.0 - from.0) * t,
                    y: from.1 + (to.1 - from.1) * t,
                    pressure: 1.0,
                    t: i,
                }
            })
            .collect(),
    }
}

fn render(note: &Note, picture: &Picture) -> Image {
    render_note(
        NoteView::new(note, Some(picture)),
        &RenderOptions {
            scale: SCALE,
            background: true,
        },
    )
    .unwrap()
}

#[test]
fn the_image_is_drawn_in_its_rectangle_over_the_paper() {
    let picture = png_picture(&quadrants());
    let n = note(white(), Some(placed(RECT, 0, 1.0)), vec![]);
    let drawn = render(&n, &picture);
    assert_eq!((drawn.width, drawn.height), (250, 200));
    for (centre, colour) in CENTRES.iter().zip([RED, GREEN, BLUE, YELLOW]) {
        assert!(
            close(rgb_at(&drawn, *centre), colour, 2),
            "{centre:?}: {:?}",
            rgb_at(&drawn, *centre)
        );
    }
    // Paper around it.
    for outside in [(40, 75), (160, 75), (100, 40), (100, 110)] {
        assert_eq!(rgb_at(&drawn, outside), [255, 255, 255], "{outside:?}");
    }
    // Without its pixels, or with the background off, no image.
    let plain = render_note(&n, &RenderOptions::fit_width(PAGE, 250)).unwrap();
    assert!(plain.pixels.iter().all(|&v| v == 255));
    let off = render_note(
        NoteView::new(&n, Some(&picture)),
        &RenderOptions {
            scale: SCALE,
            background: false,
        },
    )
    .unwrap();
    assert!(off.pixels.iter().all(|&v| v == 255));
}

#[test]
fn a_quarter_turn_turns_the_pixels_clockwise() {
    let picture = png_picture(&quadrants());
    let n = note(white(), Some(placed(RECT, 1, 1.0)), vec![]);
    let drawn = render(&n, &picture);
    // The red top-left quadrant is now top right; blue went top left.
    for (centre, colour) in CENTRES.iter().zip([BLUE, RED, YELLOW, GREEN]) {
        assert!(
            close(rgb_at(&drawn, *centre), colour, 2),
            "{centre:?}: {:?}",
            rgb_at(&drawn, *centre)
        );
    }
    let n = note(white(), Some(placed(RECT, 2, 1.0)), vec![]);
    let drawn = render(&n, &picture);
    for (centre, colour) in CENTRES.iter().zip([YELLOW, BLUE, GREEN, RED]) {
        assert!(close(rgb_at(&drawn, *centre), colour, 2), "{centre:?}");
    }
}

#[test]
fn opacity_mixes_the_image_with_the_paper_and_template() {
    let picture = png_picture(&quadrants());
    let kraft = Background {
        paper: Paper::Kraft,
        template: Template::Blank,
        texture: false,
    };
    let n = note(kraft, Some(placed(RECT, 0, 0.5)), vec![]);
    let drawn = render(&n, &picture);
    let paper = paper_color(Paper::Kraft);
    let mix = |c: u8, p: u8| ((u16::from(c) + u16::from(p)) / 2) as u8;
    let expected = [
        mix(RED[0], paper.r),
        mix(RED[1], paper.g),
        mix(RED[2], paper.b),
    ];
    assert!(
        close(rgb_at(&drawn, CENTRES[0]), expected, 2),
        "{:?} vs {expected:?}",
        rgb_at(&drawn, CENTRES[0])
    );
    // The template shows through a translucent image, not through an opaque one.
    let grid = |opacity| {
        let background = Background {
            template: Template::Grid5mm,
            ..white()
        };
        let whole = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 5000.0,
            height: 4000.0,
        };
        let flat = png_picture(&image(4, 4, |_, _| rgba([255, 255, 255])));
        let n = note(background, Some(placed(whole, 0, opacity)), vec![]);
        let drawn = render(&n, &flat);
        drawn
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|p| p[..3] != [255, 255, 255])
            .count()
    };
    assert_eq!(grid(1.0), 0);
    assert!(grid(0.5) > 1000);
}

#[test]
fn transparent_pixels_show_the_paper_and_strokes_stay_on_top() {
    let half_clear = png_picture(&image(40, 20, |x, _| {
        if x < 20 { [0, 0, 0, 0] } else { rgba(GREEN) }
    }));
    let stroke = line(Rgba::BLACK, (500.0, 1700.0), (4500.0, 1700.0));
    let slate = Background {
        paper: Paper::Slate,
        template: Template::Blank,
        texture: false,
    };
    let n = note(slate, Some(placed(RECT, 0, 1.0)), vec![stroke]);
    let drawn = render(&n, &half_clear);
    let paper = paper_color(Paper::Slate);
    assert_eq!(rgb_at(&drawn, CENTRES[0]), [paper.r, paper.g, paper.b]);
    assert!(close(rgb_at(&drawn, CENTRES[1]), GREEN, 2));
    // The stroke at y = 1700 (85 px) crosses the image: ink on top.
    assert_eq!(rgb_at(&drawn, (125, 85)), [0, 0, 0]);
}

#[test]
fn a_cover_image_is_clipped_to_the_page() {
    let picture = png_picture(&quadrants());
    // 7000 × 6000 units centred on the page: it reaches 1000 units beyond each side.
    let cover = ImageRect {
        x: -1000.0,
        y: -1000.0,
        width: 7000.0,
        height: 6000.0,
    };
    let n = note(white(), Some(placed(cover, 0, 1.0)), vec![]);
    let drawn = render(&n, &picture);
    assert_eq!((drawn.width, drawn.height), (250, 200));
    let corners = [(1, 1), (248, 1), (1, 198), (248, 198)];
    for (corner, colour) in corners.iter().zip([RED, GREEN, BLUE, YELLOW]) {
        assert!(close(rgb_at(&drawn, *corner), colour, 2), "{corner:?}");
    }
    // Nothing of the paper is left.
    assert!(
        drawn
            .pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[..3] != [255, 255, 255])
    );
    // Entirely off the page: nothing drawn, no error.
    let away = ImageRect { x: 6000.0, ..RECT };
    let n = note(white(), Some(placed(away, 0, 1.0)), vec![]);
    assert!(render(&n, &picture).pixels.iter().all(|&v| v == 255));
}

#[test]
fn a_much_smaller_render_averages_the_pixels() {
    // One-pixel black and white stripes, drawn 20 times smaller: an even grey, not stripes.
    let stripes = png_picture(&image(400, 400, |x, _| {
        if x % 2 == 0 {
            rgba([0, 0, 0])
        } else {
            rgba([255, 255, 255])
        }
    }));
    let square = ImageRect {
        x: 1000.0,
        y: 1000.0,
        width: 400.0,
        height: 400.0,
    };
    let n = note(white(), Some(placed(square, 0, 1.0)), vec![]);
    let drawn = render(&n, &stripes);
    for x in 52..69 {
        let [r, ..] = rgb_at(&drawn, (x, 60));
        assert!(r.abs_diff(128) <= 6, "column {x}: {r}");
    }
}

#[test]
fn the_canvas_draws_what_render_note_draws() {
    let picture = Picture::decode(encode_jpeg(&quadrants(), 95).unwrap()).unwrap();
    let strokes = vec![
        line(Rgba::rgb(10, 10, 200), (500.0, 900.0), (4500.0, 2500.0)),
        line(Rgba::BLACK, (500.0, 3000.0), (4500.0, 1200.0)),
    ];
    let n = note(white(), Some(placed(RECT, 3, 0.8)), strokes);
    let view = NoteView::new(&n, Some(&picture));
    let mut canvas = PageCanvas::new(view, SCALE, Backdrop::Paper).unwrap();
    for i in drawing_order(&n) {
        canvas.draw_stroke(&n.strokes[i]);
    }
    assert_eq!(canvas.to_image(), render(&n, &picture));
    // The transparent backdrop never shows the image.
    let clear = PageCanvas::new(view, SCALE, Backdrop::Transparent).unwrap();
    assert!(clear.to_image().pixels.iter().all(|&v| v == 0));
}

/// `svg` drawn by resvg at the raster's size, composited over white.
fn draw_svg(svg: &str, width: u32, height: u32) -> Image {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_str(svg, &usvg::Options::default()).expect("usvg parses");
    let scale = width as f32 / tree.size().width();
    let mut pixmap = tiny_skia::Pixmap::new(width, height).unwrap();
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let pixels = pixmap
        .pixels()
        .iter()
        .flat_map(|p| {
            let white = 255 - p.alpha();
            [p.red() + white, p.green() + white, p.blue() + white, 255]
        })
        .collect();
    Image {
        width,
        height,
        pixels,
    }
}

/// Mean absolute difference per channel, and the share of pixels off by more than 48.
fn difference(a: &Image, b: &Image) -> (f64, f64) {
    let mut total = 0u64;
    let mut gross = 0usize;
    for (p, q) in a
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0)
    {
        let d: Vec<u8> = (0..3).map(|c| p[c].abs_diff(q[c])).collect();
        total += d.iter().map(|&v| u64::from(v)).sum::<u64>();
        if d.iter().any(|&v| v > 48) {
            gross += 1;
        }
    }
    let pixels = (a.pixels.len() / 4) as f64;
    (total as f64 / (pixels * 3.0), gross as f64 / pixels)
}

#[test]
fn the_svg_places_the_image_where_the_raster_draws_it() {
    // A smooth image, so that both resamplings agree away from the edges.
    let gradient = image(64, 32, |x, y| [(x * 4) as u8, (y * 8) as u8, 120, 255]);
    let cover = ImageRect {
        x: -600.0,
        y: 700.0,
        width: 3200.0,
        height: 6000.0,
    };
    for (picture, quarter_turns) in [
        (png_picture(&gradient), 1),
        (
            Picture::decode(encode_jpeg(&gradient, 95).unwrap()).unwrap(),
            2,
        ),
    ] {
        let n = note(
            Background {
                paper: Paper::Cream,
                template: Template::Blank,
                texture: false,
            },
            Some(placed(cover, quarter_turns, 0.7)),
            vec![],
        );
        let view = NoteView::new(&n, Some(&picture));
        let svg = note_to_svg(view, &SvgOptions::default());
        assert_eq!(svg.matches("<image ").count(), 1);
        assert!(svg.contains("clip-path=\"url(#page-clip)\""));
        assert!(svg.contains("opacity=\"0.7\""));
        let mime = picture.format().mime();
        assert!(svg.contains(&format!("data:{mime};base64,")), "{mime}");
        let raster = render_note(view, &RenderOptions::at_dpi(100, true)).unwrap();
        let drawn = draw_svg(&svg, raster.width, raster.height);
        let (mean, gross) = difference(&raster, &drawn);
        println!(
            "{mime}, {quarter_turns} quarter turns: mean difference {mean:.3}, {:.3} % off by \
             more than 48",
            gross * 100.0
        );
        assert!(mean < 2.0 && gross < 0.005, "{mean} {gross}");
        // Without the background, no image.
        let bare = note_to_svg(view, &SvgOptions { background: false });
        assert!(!bare.contains("<image"));
    }
    assert!((scale_for_dpi(100) - 100.0 / 2540.0).abs() < 1e-6);
}

fn number(object: &lopdf::Object) -> f32 {
    match object {
        lopdf::Object::Integer(i) => *i as f32,
        lopdf::Object::Real(r) => *r,
        other => panic!("not a number: {other:?}"),
    }
}

/// The stream of the page's XObject `name`.
fn xobject<'d>(
    document: &'d lopdf::Document,
    page: lopdf::ObjectId,
    name: &[u8],
) -> &'d lopdf::Stream {
    let page = document.get_dictionary(page).unwrap();
    let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
    let objects = resources.get(b"XObject").unwrap().as_dict().unwrap();
    let id = objects.get(name).unwrap().as_reference().unwrap();
    document.get_object(id).unwrap().as_stream().unwrap()
}

#[test]
fn the_pdf_embeds_the_image_with_its_transparency() {
    let jpeg = encode_jpeg(&quadrants(), 92).unwrap();
    let photo = Picture::decode(jpeg.clone()).unwrap();
    let clear = png_picture(&image(40, 20, |x, _| [20, 40, 220, (x * 6) as u8]));
    let first = note(white(), Some(placed(RECT, 1, 0.6)), vec![]);
    let second = note(
        white(),
        Some(placed(RECT, 0, 1.0)),
        vec![line(Rgba::BLACK, (500.0, 500.0), (4500.0, 500.0))],
    );
    let pdf = notes_to_pdf(
        &[
            NoteView::new(&first, Some(&photo)),
            NoteView::new(&second, Some(&clear)),
            NoteView::new(&second, None),
        ],
        &PdfOptions::default(),
    )
    .unwrap();
    let document = lopdf::Document::load_mem(&pdf).expect("PDF parses");
    let pages: Vec<_> = document.get_pages().values().copied().collect();
    assert_eq!(pages.len(), 3);

    // The JPEG goes in as it is.
    let image = xobject(&document, pages[0], b"Im1");
    let dict = &image.dict;
    assert_eq!(dict.get(b"Subtype").unwrap().as_name().unwrap(), b"Image");
    assert_eq!(
        dict.get(b"Filter").unwrap().as_name().unwrap(),
        b"DCTDecode"
    );
    assert_eq!(number(dict.get(b"Width").unwrap()), 40.0);
    assert_eq!(number(dict.get(b"Height").unwrap()), 20.0);
    assert_eq!(image.content, jpeg);
    // Drawn after the paper, with its opacity, by a matrix that turns it.
    let content = document
        .get_page_content_with_limit(pages[0], 1 << 24)
        .unwrap();
    let ops = lopdf::content::Content::decode(&content)
        .unwrap()
        .operations;
    let draw = ops
        .iter()
        .position(|op| op.operator == "Do")
        .expect("the image is drawn");
    let paper = ops.iter().position(|op| op.operator == "f").unwrap();
    assert!(paper < draw);
    let cm = ops[..draw]
        .iter()
        .rev()
        .find(|op| op.operator == "cm")
        .unwrap();
    let m: Vec<f32> = cm.operands.iter().map(number).collect();
    // Turned clockwise: the image's rows (the unit square's x) run down the page, and its
    // first row (the square's top) is on the right.
    let expected = [0.0, 1000.0, 2000.0, 0.0, 1000.0, 1000.0];
    assert!(
        m.iter().zip(expected).all(|(a, b)| (a - b).abs() < 0.01),
        "{m:?}"
    );
    let text = String::from_utf8_lossy(&pdf);
    assert!(text.contains("/ca 0.6"), "the image's opacity");

    // Pixels with transparency: deflated colour and a soft mask.
    let image = xobject(&document, pages[1], b"Im1");
    assert_eq!(
        image.dict.get(b"Filter").unwrap().as_name().unwrap(),
        b"FlateDecode"
    );
    let mask_id = image.dict.get(b"SMask").unwrap().as_reference().unwrap();
    let mask = document.get_object(mask_id).unwrap().as_stream().unwrap();
    let alpha = mask.decompressed_content().unwrap();
    assert_eq!(alpha.len(), 40 * 20);
    assert_eq!(&alpha[..3], &[0, 6, 12]);
    assert_eq!(image.decompressed_content().unwrap().len(), 40 * 20 * 3);
    // The stroke is drawn after the image.
    let content = document
        .get_page_content_with_limit(pages[1], 1 << 24)
        .unwrap();
    let ops = lopdf::content::Content::decode(&content)
        .unwrap()
        .operations;
    let draw = ops.iter().position(|op| op.operator == "Do").unwrap();
    let last_fill = ops.iter().rposition(|op| op.operator == "f").unwrap();
    assert!(draw < last_fill);

    // No pixels: no image.
    let page = document.get_dictionary(pages[2]).unwrap();
    let resources = page.get(b"Resources").unwrap().as_dict().unwrap();
    assert!(resources.get(b"XObject").is_err());
}
