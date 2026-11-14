//! Bookmarks (SPEC §4.4 « Signets », format 1.6) in the outputs: they are the editor's only, so every
//! image, SVG and PDF of a page is the same with and without them (`draw_items` leaves them out), and the
//! PDF carries them in its outline (`bookmark_outline`), each going to its spot on its page, read back
//! with an independent reader (`lopdf`).

#![allow(clippy::unwrap_used, clippy::expect_used)]

use ardoise_core::note::{
    Background, Layer, Note, Origin, PageObject, PageSize, Paper, Template, TextBody, Timing,
};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_render::{
    DrawItem, LayerMode, OutlineEntry, PdfOptions, PdfPage, RenderOptions, SvgOptions,
    bookmark_outline, draw_items, note_to_svg, notes_to_pdf, notes_to_pdf_with_outline,
    render_note,
};
use ardoise_text::FontFamily;
use lopdf::{Document, Object, ObjectId};

const PAGE: PageSize = PageSize {
    width: 8000,
    height: 5000,
};

fn note(objects: Vec<PageObject>, hidden_layer: bool) -> Note {
    Note {
        id: "00000000-0000-4000-8000-0000000000bb".into(),
        title: "signets".into(),
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
        layers: vec![
            Layer {
                name: "Calque 1".into(),
                visible: true,
            },
            Layer {
                name: "Cachée".into(),
                visible: !hidden_layer,
            },
        ],
        strokes: vec![Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 60.0,
            layer: 0,
            points: (0..=10)
                .map(|i| Point {
                    x: 500.0 + i as f32 * 300.0,
                    y: 800.0,
                    pressure: 1.0,
                    t: i,
                })
                .collect(),
        }],
        objects,
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

fn mark(id: &str, layer: u32, x: f32, y: f32, label: &str) -> PageObject {
    PageObject::bookmark(id, layer, 0, x, y, label)
}

fn frame() -> PageObject {
    PageObject::text(
        "t",
        0,
        0,
        600.0,
        2000.0,
        TextBody {
            text: "Texte".into(),
            font: FontFamily::Andika,
            bold: false,
            size: 600.0,
            color: Rgba::BLACK,
        },
    )
}

fn marks() -> Vec<PageObject> {
    vec![
        mark("b", 0, 4000.0, 3000.0, "Second"),
        mark("a", 1, 1000.0, 1500.0, "Premier"),
        mark("blank", 0, 100.0, 100.0, "  "),
    ]
}

#[test]
fn a_bookmark_is_drawn_nowhere() {
    let with = note([vec![frame()], marks()].concat(), false);
    let without = note(vec![frame()], false);
    // The draw order of every format holds the strokes and the text frame, no bookmark.
    assert_eq!(draw_items(&with), draw_items(&without));
    assert!(draw_items(&with).contains(&DrawItem::Object(0)));
    assert_eq!(draw_items(&with).len(), 2);

    // The raster images are the same, with the layer of the bookmarks hidden or shown.
    let options = RenderOptions {
        scale: 0.05,
        background: true,
    };
    let reference = render_note(&without, &options).unwrap();
    assert_eq!(render_note(&with, &options).unwrap(), reference);
    let hidden = note([vec![frame()], marks()].concat(), true);
    assert_eq!(render_note(&hidden, &options).unwrap(), reference);

    // The SVG neither: not their text, nor a path, nor a layer group.
    let svg = note_to_svg(&with, &SvgOptions::default());
    assert_eq!(svg, note_to_svg(&without, &SvgOptions::default()));
    assert!(!svg.contains("Premier") && !svg.contains("Second"));

    // The PDF (no outline asked): the same bytes, the labels nowhere.
    for layers in [LayerMode::Merged, LayerMode::Separate] {
        let options = PdfOptions {
            layers,
            ..PdfOptions::default()
        };
        let pdf = notes_to_pdf(&[&with], &options).unwrap();
        assert_eq!(pdf, notes_to_pdf(&[&without], &options).unwrap());
        assert!(
            !pdf.windows(b"Premier".len())
                .any(|window| window == b"Premier")
        );
    }
}

/// The title of an outline entry, written as PDFDocEncoding or as UTF-16 with a byte-order mark.
fn pdf_text(bytes: &[u8]) -> String {
    match bytes {
        [0xfe, 0xff, rest @ ..] => String::from_utf16(
            &rest
                .chunks(2)
                .map(|c| u16::from_be_bytes([c[0], c[1]]))
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        plain => String::from_utf8_lossy(plain).into_owned(),
    }
}

/// Every entry of the outline in reading order: its depth, its title, its page (0-based), and its
/// destination (`Fit`, or `XYZ` with its left and top).
type Seen = Vec<(usize, String, usize, Option<[f32; 2]>)>;

fn outline_of(pdf: &[u8]) -> Seen {
    fn walk(
        document: &Document,
        first: ObjectId,
        depth: usize,
        pages: &[ObjectId],
        out: &mut Seen,
    ) {
        let mut current = Some(first);
        while let Some(id) = current {
            let item = document.get_dictionary(id).unwrap();
            let Object::String(title, _) = item.get(b"Title").unwrap() else {
                panic!("title")
            };
            let Object::Array(dest) = item.get(b"Dest").unwrap() else {
                panic!("dest")
            };
            let page = pages
                .iter()
                .position(|p| *p == dest[0].as_reference().unwrap())
                .unwrap();
            let at = match dest[1].as_name().unwrap() {
                b"Fit" => None,
                b"XYZ" => Some([dest[2].as_float().unwrap(), dest[3].as_float().unwrap()]),
                other => panic!("destination {other:?}"),
            };
            out.push((depth, pdf_text(title), page, at));
            if let Ok(child) = item.get(b"First") {
                walk(
                    document,
                    child.as_reference().unwrap(),
                    depth + 1,
                    pages,
                    out,
                );
            }
            current = item.get(b"Next").ok().map(|n| n.as_reference().unwrap());
        }
    }
    let document = Document::load_mem(pdf).unwrap();
    let pages: Vec<ObjectId> = document.get_pages().values().copied().collect();
    let root = document
        .catalog()
        .unwrap()
        .get(b"Outlines")
        .unwrap()
        .as_reference()
        .unwrap();
    let first = document
        .get_dictionary(root)
        .unwrap()
        .get(b"First")
        .unwrap()
        .as_reference()
        .unwrap();
    let mut seen = Vec::new();
    walk(&document, first, 0, &pages, &mut seen);
    seen
}

#[test]
fn the_bookmarks_of_a_note_make_its_pdf_outline_in_reading_order() {
    // Two bookmarks, one on a hidden layer (a bookmark is not hidden with its layer), one blank (not listed).
    let with = note(marks(), true);
    let outline = bookmark_outline(&with, 0, 0);
    assert_eq!(
        outline.iter().map(|e| e.title.as_str()).collect::<Vec<_>>(),
        ["Premier", "Second"]
    );
    let pdf = notes_to_pdf_with_outline(&[&with], &PdfOptions::default(), &outline).unwrap();
    let seen = outline_of(&pdf);
    assert_eq!(seen.len(), 2);
    assert_eq!(
        (seen[0].0, seen[0].1.as_str(), seen[0].2),
        (0, "Premier", 0)
    );
    assert_eq!((seen[1].0, seen[1].1.as_str(), seen[1].2), (0, "Second", 0));

    // Each goes to its spot, a little above its box, in the page's user space (y up): 1 device unit =
    // 72 / 2540 pt on the actual-size page.
    let unit = 72.0 / 2540.0;
    let [left, top, _, _] = with.objects[1].page_bounds();
    let [want_x, want_y] = [left * unit, (PAGE.height as f32 - (top - 500.0)) * unit];
    let [got_x, got_y] = seen[0].3.expect("a spot");
    assert!((got_x - want_x).abs() < 0.05, "{got_x} {want_x}");
    assert!((got_y - want_y).abs() < 0.05, "{got_y} {want_y}");
    // The page without a bookmark has no outline.
    assert!(bookmark_outline(&note(vec![frame()], false), 0, 0).is_empty());
}

#[test]
fn a_bookmark_goes_to_its_spot_on_the_a4_page_the_note_is_fitted_into() {
    let with = note(vec![mark("a", 0, 2000.0, 1000.0, "Ici")], false);
    let options = PdfOptions {
        page: PdfPage::A4,
        ..PdfOptions::default()
    };
    let pdf =
        notes_to_pdf_with_outline(&[&with], &options, &bookmark_outline(&with, 0, 0)).unwrap();
    let seen = outline_of(&pdf);
    // A4 landscape (842 × 595 pt); the 80 × 50 mm note scaled up to fit it and centred.
    let [left, top, _, _] = with.objects[0].page_bounds();
    let scale = (841.89_f32 / 8000.0).min(595.28 / 5000.0);
    let x = (841.89 - 8000.0 * scale) / 2.0 + left * scale;
    let y = 595.28 - ((595.28 - 5000.0 * scale) / 2.0 + (top - 500.0) * scale);
    let [got_x, got_y] = seen[0].3.expect("a spot");
    assert!((got_x - x).abs() < 0.1, "{got_x} {x}");
    assert!((got_y - y).abs() < 0.1, "{got_y} {y}");
}

#[test]
fn the_bookmarks_come_under_their_notes_entry_in_a_binder_pdf() {
    let a = note(vec![mark("a", 0, 500.0, 500.0, "Intro")], false);
    let b = note(vec![mark("b", 0, 500.0, 2500.0, "Bilan")], false);
    let mut outline = vec![OutlineEntry {
        title: "Cours".into(),
        level: 0,
        page: 0,
        at: None,
    }];
    outline.extend(bookmark_outline(&a, 0, 1));
    outline.push(OutlineEntry {
        title: "TP".into(),
        level: 0,
        page: 1,
        at: None,
    });
    outline.extend(bookmark_outline(&b, 1, 1));
    let pdf = notes_to_pdf_with_outline(&[&a, &b], &PdfOptions::default(), &outline).unwrap();
    let seen = outline_of(&pdf);
    let summary: Vec<_> = seen
        .iter()
        .map(|(d, t, p, at)| (*d, t.as_str(), *p, at.is_some()))
        .collect();
    assert_eq!(
        summary,
        [
            (0, "Cours", 0, false),
            (1, "Intro", 0, true),
            (0, "TP", 1, false),
            (1, "Bilan", 1, true),
        ]
    );
}
