//! On real notes, when they are there: `testdata/private/revise/*.ardoise` in this checkout or one
//! it is a worktree of (git-ignored, never committed), or the folder `ARDOISE_REVISE_NOTES` names.
//! Skipped otherwise. It prints counts only. Set `ARDOISE_SAMPLES_DIR` to get pictures: each line in
//! its own colour with its baseline, and the page once its lines are tidied. Nothing else is
//! written, and these pictures are private: keep them out of the repository.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};

use ardoise_core::format::read_note;
use ardoise_core::note::Note;
use ardoise_render::{RenderOptions, encode_png, render_note};
use ardoise_revise::{InkLines, Input, Order, Revised, Select, Shape, apply, ink_lines};

const COLORS: [&str; 6] = [
    "#c0392b", "#1f4e9e", "#1a7f37", "#8e44ad", "#d35400", "#117a8b",
];

fn notes_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ARDOISE_REVISE_NOTES") {
        return Some(PathBuf::from(dir)).filter(|dir| dir.is_dir());
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|dir| dir.join("testdata/private/revise"))
        .find(|dir| dir.is_dir())
}

/// The notes of the folder, by file stem. Empty, with a message, when there are none.
fn notes() -> Vec<(String, Note)> {
    let Some(dir) = notes_dir() else {
        eprintln!("skipped: no testdata/private/revise here");
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|e| e == "ardoise"))
        .collect();
    found.sort();
    found
        .iter()
        .map(|path| {
            let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
            (stem, read_note(path).unwrap().note)
        })
        .collect()
}

fn visible(note: &Note) -> Vec<usize> {
    note.strokes
        .iter()
        .enumerate()
        .filter(|(_, stroke)| {
            note.layers
                .get(stroke.layer as usize)
                .is_some_and(|layer| layer.visible)
        })
        .map(|(number, _)| number)
        .collect()
}

fn input(note: &Note) -> Input<'_> {
    Input {
        strokes: &note.strokes,
        page_mm: [
            note.page.width as f32 / 100.0,
            note.page.height as f32 / 100.0,
        ],
        layers: note.layers.len() as u32,
        narration: false,
    }
}

fn picture(note: &Note, revised: &Revised, name: &str) {
    let Some(dir) = std::env::var_os("ARDOISE_SAMPLES_DIR") else {
        return;
    };
    let mut shown = note.clone();
    shown.strokes.clone_from(&revised.strokes);
    let image = render_note(&shown, &RenderOptions::at_dpi(110, true)).unwrap();
    std::fs::write(
        Path::new(&dir).join(format!("{name}.png")),
        encode_png(&image).unwrap(),
    )
    .unwrap();
}

/// Each line in its colour, with its baseline; what is in no line in light grey.
fn colour_orders(found: &InkLines) -> Vec<Order> {
    let mut orders = Vec::new();
    for (index, line) in found.lines.iter().enumerate() {
        let color = COLORS[index % COLORS.len()].to_owned();
        orders.push(Order::Recolor {
            select: Select {
                ids: Some(line.strokes.clone()),
                ..Select::default()
            },
            color: color.clone(),
        });
        orders.push(Order::Add {
            shapes: vec![Shape::Line {
                from: [line.x0, line.baseline_left],
                to: [line.x1, line.baseline_right],
            }],
            color,
            width: Some(0.15),
            tool: None,
            layer: None,
            behind: false,
            after: None,
        });
    }
    orders.push(Order::Recolor {
        select: Select {
            ids: Some(found.apart.clone()),
            ..Select::default()
        },
        color: "#b0b0b0".to_owned(),
    });
    orders
}

fn tidy_orders(found: &InkLines) -> Vec<Order> {
    found
        .lines
        .iter()
        .map(|line| Order::TidyLine {
            select: Select {
                ids: Some(line.strokes.clone()),
                ..Select::default()
            },
            baseline: None,
            left: None,
            mode: None,
            scale: None,
            slope: None,
            gaps: None,
            word_gap: None,
            smooth: Some(0.2),
        })
        .collect()
}

#[test]
fn the_written_lines_of_real_notes() {
    for (name, note) in notes() {
        let visible = visible(&note);
        let found = ink_lines(&note.strokes, &visible);
        let in_lines: usize = found.lines.iter().map(|line| line.strokes.len()).sum();
        let words: usize = found.lines.iter().map(|line| line.words.len()).sum();
        eprintln!(
            "{name}: {} strokes, {} lines holding {in_lines}, {} apart, {words} words",
            visible.len(),
            found.lines.len(),
            found.apart.len(),
        );
        for (index, line) in found.lines.iter().enumerate() {
            eprintln!(
                "  line {index}: {} strokes, {} words, x {:.0}..{:.0}, y {:.1}..{:.1}, slope {:.2}, body {:.1}",
                line.strokes.len(),
                line.words.len(),
                line.x0,
                line.x1,
                line.baseline_left,
                line.baseline_right,
                line.slope,
                line.body,
            );
        }
        assert_eq!(in_lines + found.apart.len(), visible.len(), "{name}");
        picture(
            &note,
            &apply(&input(&note), &colour_orders(&found)).unwrap(),
            &format!("{name}-lines"),
        );
        if name == "incident" {
            assert_eq!(found.lines.len(), 15, "the 15 written lines of {name}");
        }
    }
}

#[test]
fn tidying_real_notes_line_by_line() {
    for (name, note) in notes() {
        let found = ink_lines(&note.strokes, &visible(&note));
        let revised = apply(&input(&note), &tidy_orders(&found)).unwrap();
        assert_eq!(revised.strokes.len(), note.strokes.len(), "{name}");
        // The same words as `ink_lines` said, and the lines level afterwards.
        for (order, line) in revised.report.orders.iter().zip(&found.lines) {
            let report = order.line.as_ref().unwrap();
            assert_eq!(report.words, line.words.len(), "{name}");
            assert_eq!(report.strokes, line.strokes.len(), "{name}");
        }
        let after = ink_lines(&revised.strokes, &visible_of(&note, &revised));
        let steep = after
            .lines
            .iter()
            .filter(|line| line.slope.abs() > 0.5)
            .count();
        eprintln!(
            "{name}: tidied {} lines, {} after, {steep} still sloping over 0.5 degree",
            found.lines.len(),
            after.lines.len(),
        );
        picture(&note, &revised, &format!("{name}-tidy"));
        if name == "incident" {
            assert_eq!(after.lines.len(), found.lines.len(), "{name}");
            assert_eq!(steep, 0, "{name}");
        }
    }
}

/// `visible` on the revised strokes.
fn visible_of(note: &Note, revised: &Revised) -> Vec<usize> {
    revised
        .strokes
        .iter()
        .enumerate()
        .filter(|(_, stroke)| {
            note.layers
                .get(stroke.layer as usize)
                .is_some_and(|layer| layer.visible)
        })
        .map(|(number, _)| number)
        .collect()
}
