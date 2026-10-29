//! Test of the page structure with Windows' ink analysis (`InkAnalyzer`, ROADMAP M8, text
//! export in Markdown and Word): a synthetic page (never anyone's notes) with a title, a
//! two-line paragraph, a list of three dashed items and two drawings (a circle and a square)
//! is analysed by Windows; the structure built from the analysis and the recognised words must
//! hold the paragraphs, the three list items and the two drawings. A PC without a French
//! recognizer skips the test with a message.
//!
//! `cargo test -p ardoise-hwr --test structure -- --nocapture` prints what was found.

#![cfg(windows)]

use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Note, Origin, Timing};
use ardoise_hwr::ink::{recognizable_indices, recognizable_strokes};
use ardoise_hwr::structure::{Item, from_recognition, from_regions, ink_boxes, read_regions};
use ardoise_hwr::synthetic::Hand;
use ardoise_hwr::text::fold;
use ardoise_hwr::{Language, Recognizer, SystemRecognizer, recognizer_for};
use ardoise_ink::{Point, Rgba, Stroke, Tool};

fn polyline(points: &[(f32, f32)], t0: u32) -> Stroke {
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 40.0,
        layer: 0,
        points: points
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| Point {
                x,
                y,
                pressure: 0.5,
                t: t0 + i as u32 * 5,
            })
            .collect(),
    }
}

/// A straight segment sampled every few units.
fn segment(from: (f32, f32), to: (f32, f32)) -> Vec<(f32, f32)> {
    (0..=20)
        .map(|i| {
            let f = i as f32 / 20.0;
            (from.0 + (to.0 - from.0) * f, from.1 + (to.1 - from.1) * f)
        })
        .collect()
}

fn circle(cx: f32, cy: f32, r: f32) -> Vec<(f32, f32)> {
    (0..=60)
        .map(|i| {
            let a = i as f32 / 60.0 * std::f32::consts::TAU;
            (cx + r * a.cos(), cy + r * a.sin())
        })
        .collect()
}

fn page() -> Note {
    let hand = |left: f32, baseline: f32, x_height: f32, seed: u64| Hand {
        left,
        baseline,
        x_height,
        width: 14_000.0,
        line_gap: x_height * 3.4,
        seed,
        jitter: 0.4,
        ..Hand::default()
    };
    let mut strokes: Vec<Stroke> = Vec::new();
    strokes.extend(hand(2000.0, 2000.0, 520.0, 1).write("journal de bord"));
    strokes.extend(
        hand(2000.0, 4200.0, 320.0, 2)
            .write("le chat dort sur le canape et la pluie tombe sur la ville"),
    );
    strokes.extend(
        hand(2000.0, 8000.0, 320.0, 3).write("demain nous irons au marche acheter des pommes"),
    );
    for (i, item) in ["pommes", "poires rouges", "du pain"].iter().enumerate() {
        let y = 11_500.0 + i as f32 * 1200.0;
        strokes.push(polyline(
            &segment((2000.0, y - 150.0), (2400.0, y - 150.0)),
            100_000,
        ));
        strokes.extend(hand(3000.0, y, 320.0, 10 + i as u64).write(item));
    }
    strokes.push(polyline(&circle(21_000.0, 5000.0, 1500.0), 200_000));
    let square: Vec<(f32, f32)> = [
        ((19_500.0, 8000.0), (22_500.0, 8000.0)),
        ((22_500.0, 8000.0), (22_500.0, 10_500.0)),
        ((22_500.0, 10_500.0), (19_500.0, 10_500.0)),
        ((19_500.0, 10_500.0), (19_500.0, 8000.0)),
    ]
    .iter()
    .flat_map(|&(a, b)| segment(a, b))
    .collect();
    strokes.push(polyline(&square, 300_000));
    Note {
        id: "structure".into(),
        title: String::new(),
        created: 0,
        modified: 0,
        page: A4_LANDSCAPE,
        background: Background::default(),
        background_image: None,
        tags: Vec::new(),
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

fn describe(items: &[Item]) -> Vec<String> {
    items
        .iter()
        .map(|item| match item {
            Item::Paragraph(t) => format!("paragraph: {t}"),
            Item::ListItem(t) => format!("list item: {t}"),
            Item::Drawing(d) => format!("drawing: {} strokes", d.strokes.len()),
        })
        .collect()
}

#[test]
fn windows_ink_analysis_finds_paragraphs_list_items_and_drawings() {
    let engine = SystemRecognizer::new();
    let installed = engine
        .recognizers()
        .expect("the engine lists its recognizers");
    if recognizer_for(&installed, Language::Fr).is_none() {
        println!("skipped: no French handwriting recognizer on this PC");
        return;
    }
    let note = page();
    let strokes = recognizable_strokes(&note);
    let analyzed = recognizable_indices(&note);
    let recognition = engine.recognize(&strokes, Language::Fr).unwrap();
    let ink = ink_boxes(&note);

    let mut regions = engine.analyze(&strokes).unwrap();
    read_regions(&engine, &strokes, &mut regions, Language::Fr);
    println!("regions: {}", regions.len());
    for region in &regions {
        println!("  {:?} {:?} {:?}", region.kind, region.bounds, region.text);
    }
    let items = from_regions(&regions, &analyzed, &recognition, &ink);
    println!("structure from the analysis:");
    for line in describe(&items) {
        println!("  {line}");
    }
    let drawings: Vec<_> = items
        .iter()
        .filter_map(|item| match item {
            Item::Drawing(d) => Some(d),
            _ => None,
        })
        .collect();
    assert_eq!(drawings.len(), 2, "{:?}", describe(&items));
    // A drawing holds its one stroke.
    assert!(drawings.iter().all(|d| d.strokes.len() == 1));
    let list: Vec<String> = items
        .iter()
        .filter_map(|item| match item {
            Item::ListItem(t) => Some(fold(t)),
            _ => None,
        })
        .collect();
    assert_eq!(list.len(), 3, "{:?}", describe(&items));
    // Words may be misread by the engine; the first letters are enough to tell the items.
    for (found, expected) in list.iter().zip(["pomm", "poire", "du"]) {
        assert!(found.starts_with(expected), "{list:?}");
    }
    let paragraphs = items
        .iter()
        .filter(|item| matches!(item, Item::Paragraph(_)))
        .count();
    assert!(
        (2..=4).contains(&paragraphs),
        "{paragraphs} paragraphs: {:?}",
        describe(&items)
    );

    // The heuristics alone, on the same page: printed for comparison.
    println!("structure from the heuristics:");
    for line in describe(&from_recognition(&recognition, &ink)) {
        println!("  {line}");
    }
}
