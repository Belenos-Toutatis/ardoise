//! `ardoise.exe --create-demo-library DIR`: makes DIR a library holding a few synthetic notes,
//! for demonstrations and end-to-end tests. Everything is generated (deterministic
//! handwriting-like curves): nothing comes from a real note.

use std::collections::HashSet;
use std::f32::consts::TAU;
use std::fs;
use std::path::Path;

use ardoise_core::format::NoteFile;
use ardoise_core::library::{self, Library};
use ardoise_core::note::{
    A4_LANDSCAPE, A4_PORTRAIT, Background, Layer, Note, Origin, PageSize, Paper, Template, Timing,
};
use ardoise_ink::{Point, Rgba, Stroke, Tool};

use crate::library::{binder_path, slash_path};
use crate::thumbnails::thumbnail_png;

/// 2026-09-07 07:30 UTC: the first demo note; the others follow over about four weeks.
const FIRST_DAY_MS: i64 = 1_788_766_200_000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

const BLACK: Rgba = Rgba::BLACK;
const BLUE: Rgba = Rgba::rgb(0x51, 0xa7, 0xf9);
const GREEN: Rgba = Rgba::rgb(0x84, 0xbd, 0x00);
const PURPLE: Rgba = Rgba::rgb(0xbb, 0x3e, 0x9a);
const RED: Rgba = Rgba::rgb(0xd8, 0x00, 0x00);
const ORANGE: Rgba = Rgba::rgb(0xff, 0x6e, 0x03);
const GREY: Rgba = Rgba::rgb(0x6b, 0x6b, 0x6b);
const YELLOW: Rgba = Rgba::rgb(0xff, 0xe0, 0x3a);
const WHITE: Rgba = Rgba::WHITE;

/// Small deterministic generator (xorshift64*).
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }

    /// Uniform in `[0, 1)`.
    fn next(&mut self) -> f32 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        let value = self.0.wrapping_mul(0x2545_f491_4f6c_dd1d);
        (value >> 40) as f32 / (1u64 << 24) as f32
    }

    fn range(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * self.next()
    }
}

#[derive(Clone, Copy)]
struct Pen {
    tool: Tool,
    color: Rgba,
    width: f32,
    layer: u32,
}

const fn pen(tool: Tool, color: Rgba, width: f32) -> Pen {
    Pen {
        tool,
        color,
        width,
        layer: 0,
    }
}

/// Writes strokes as a hand would: time runs on from stroke to stroke.
struct Hand {
    rng: Rng,
    /// Milliseconds since the note's creation.
    t: u32,
    strokes: Vec<Stroke>,
}

impl Hand {
    fn new(seed: u64) -> Self {
        Hand {
            rng: Rng::new(seed),
            t: 400,
            strokes: Vec::new(),
        }
    }

    /// Adds a stroke through `points` (x, y), pressure rising then falling, one sample every
    /// 5 ms as the tablet sends them.
    fn stroke(&mut self, pen: Pen, points: &[(f32, f32)]) {
        let n = points.len().max(2) as f32;
        let mut out = Vec::with_capacity(points.len());
        for (i, &(x, y)) in points.iter().enumerate() {
            let s = i as f32 / (n - 1.0);
            let pressure =
                (0.3 + 0.55 * (s * std::f32::consts::PI).sin() + self.rng.range(-0.04, 0.04))
                    .clamp(0.05, 1.0);
            out.push(Point {
                x,
                y,
                pressure,
                t: self.t,
            });
            self.t += 5;
        }
        self.strokes.push(Stroke {
            tool: pen.tool,
            color: pen.color,
            width: pen.width,
            layer: pen.layer,
            points: out,
        });
        // Lifting the pen and moving to the next stroke.
        self.t += 120 + (self.rng.next() * 250.0) as u32;
    }

    /// A cursive-looking word: a looping curve (prolate cycloid), taller loops for
    /// "ascenders". Returns where it ends.
    fn word(&mut self, pen: Pen, x: f32, baseline: f32, letters: usize, size: f32) -> f32 {
        let mut points = Vec::new();
        let mut left = x;
        for _ in 0..letters {
            let tall = self.rng.next() < 0.25;
            let height = size * if tall { 1.9 } else { 1.0 } * self.rng.range(0.85, 1.1);
            let width = size * self.rng.range(0.75, 1.05);
            let lean = size * 0.15;
            let loop_size = width * 0.3;
            for k in 0..16 {
                let theta = TAU * k as f32 / 16.0;
                let rise = (1.0 - theta.cos()) / 2.0;
                points.push((
                    left + width * theta / TAU - loop_size * theta.sin() + lean * rise,
                    baseline - height * rise,
                ));
            }
            left += width;
        }
        points.push((left, baseline));
        self.stroke(pen, &points);
        left
    }

    /// Words from `left` to about `right` on one line; returns where the line ends.
    fn line(&mut self, pen: Pen, left: f32, right: f32, baseline: f32, size: f32) -> f32 {
        let mut x = left;
        loop {
            let letters = 2 + (self.rng.next() * 6.0) as usize;
            if x + letters as f32 * size * 0.9 > right {
                return x;
            }
            let drift = self.rng.range(-20.0, 20.0);
            x = self.word(pen, x, baseline + drift, letters, size);
            x += size * self.rng.range(0.9, 1.4);
        }
    }

    /// `count` lines of text of varied lengths.
    fn paragraph(&mut self, pen: Pen, area: Area, count: usize, gap: f32, size: f32) -> f32 {
        let mut baseline = area.top;
        for _ in 0..count {
            let right = area.left + (area.right - area.left) * self.rng.range(0.55, 1.0);
            self.line(pen, area.left, right, baseline, size);
            baseline += gap;
        }
        baseline
    }

    /// A straight line, slightly wavy as drawn by hand.
    fn segment(&mut self, pen: Pen, from: (f32, f32), to: (f32, f32)) {
        let steps = 24;
        let wobble = self.rng.range(-25.0, 25.0);
        let points: Vec<(f32, f32)> = (0..=steps)
            .map(|i| {
                let s = i as f32 / steps as f32;
                let bump = wobble * (s * std::f32::consts::PI).sin();
                (
                    from.0 + (to.0 - from.0) * s,
                    from.1 + (to.1 - from.1) * s + bump,
                )
            })
            .collect();
        self.stroke(pen, &points);
    }

    fn polyline(&mut self, pen: Pen, corners: &[(f32, f32)]) {
        let mut points = Vec::new();
        for pair in corners.windows(2) {
            for i in 0..12 {
                let s = i as f32 / 12.0;
                points.push((
                    pair[0].0 + (pair[1].0 - pair[0].0) * s,
                    pair[0].1 + (pair[1].1 - pair[0].1) * s,
                ));
            }
        }
        if let Some(&last) = corners.last() {
            points.push(last);
        }
        self.stroke(pen, &points);
    }

    /// An ellipse drawn in one go, overlapping a little where it closes.
    fn ellipse(&mut self, pen: Pen, center: (f32, f32), rx: f32, ry: f32) {
        let start = self.rng.range(0.0, TAU);
        let points: Vec<(f32, f32)> = (0..=52)
            .map(|i| {
                let a = start + TAU * 1.05 * i as f32 / 52.0;
                (center.0 + rx * a.cos(), center.1 + ry * a.sin())
            })
            .collect();
        self.stroke(pen, &points);
    }

    fn arrow(&mut self, pen: Pen, from: (f32, f32), to: (f32, f32)) {
        self.segment(pen, from, to);
        let angle = (to.1 - from.1).atan2(to.0 - from.0);
        let head = 450.0;
        for side in [-0.5f32, 0.5] {
            let a = angle + std::f32::consts::PI + side;
            self.segment(pen, to, (to.0 + head * a.cos(), to.1 + head * a.sin()));
        }
    }
}

#[derive(Clone, Copy)]
struct Area {
    left: f32,
    right: f32,
    top: f32,
}

/// One demo note before it is written.
struct DemoNote {
    binder: &'static str,
    note: Note,
}

fn demo_id(index: u64) -> String {
    format!("5e1f0a3c-d3e0-4a11-8b00-{index:012x}")
}

#[allow(clippy::too_many_arguments)]
fn base_note(
    index: u64,
    title: &str,
    day: i64,
    page: PageSize,
    background: Background,
    tags: &[&str],
    layers: &[&str],
    origin: Origin,
) -> Note {
    let created = FIRST_DAY_MS + day * DAY_MS + (index as i64) * 37 * 60 * 1000;
    Note {
        id: demo_id(index),
        title: title.to_owned(),
        created,
        modified: created + (index as i64 + 1) * 3 * 60 * 60 * 1000,
        page,
        background,
        background_image: None,
        tags: tags.iter().map(|t| (*t).to_owned()).collect(),
        favorite: false,
        layers: layers
            .iter()
            .map(|name| Layer {
                name: (*name).to_owned(),
                visible: true,
            })
            .collect(),
        strokes: Vec::new(),
        timing: Timing::Recorded,
        origin,
    }
}

fn background(paper: Paper, template: Template, texture: bool) -> Background {
    Background {
        paper,
        template,
        texture,
    }
}

/// The demo notes, always the same.
fn demo_notes() -> Vec<DemoNote> {
    let mut notes = Vec::new();
    let full = Area {
        left: 2500.0,
        right: 27_500.0,
        top: 3200.0,
    };

    // 1. A shopping list, straight from the tablet.
    let mut note = base_note(
        1,
        "Liste de courses",
        0,
        A4_LANDSCAPE,
        background(Paper::White, Template::Lined, false),
        &["perso"],
        &["Calque 1"],
        Origin::Tablet,
    );
    let mut hand = Hand::new(1);
    let ballpoint = pen(Tool::Ballpoint, BLACK, 40.0);
    for i in 0..7 {
        let y = 3200.0 + 1600.0 * i as f32;
        hand.segment(ballpoint, (2500.0, y - 200.0), (2900.0, y - 200.0));
        let length = hand.rng.range(4000.0, 9000.0);
        hand.line(ballpoint, 3300.0, 3300.0 + length, y, 380.0);
    }
    note.strokes = hand.strokes;
    notes.push(DemoNote { binder: "", note });

    // 2. A physics lesson, favourite: title, underline, highlight, a circuit.
    let mut note = base_note(
        2,
        "Électricité – loi d'Ohm",
        3,
        A4_LANDSCAPE,
        background(Paper::Cream, Template::Seyes, false),
        &["physique", "3e B"],
        &["Calque 1"],
        Origin::Live,
    );
    note.favorite = true;
    let mut hand = Hand::new(2);
    let title_pen = pen(Tool::FountainPen, BLUE, 80.0);
    let end = hand.line(title_pen, 3000.0, 16_000.0, 3000.0, 650.0);
    hand.segment(
        pen(Tool::FeltTip, RED, 90.0),
        (2900.0, 3300.0),
        (end, 3300.0),
    );
    let text = pen(Tool::FountainPen, BLACK, 60.0);
    hand.paragraph(
        text,
        Area {
            top: 5400.0,
            right: 17_000.0,
            ..full
        },
        6,
        1600.0,
        380.0,
    );
    hand.segment(
        pen(Tool::Highlighter, YELLOW, 600.0),
        (2900.0, 8400.0),
        (14_000.0, 8400.0),
    );
    hand.polyline(
        text,
        &[
            (19_000.0, 6000.0),
            (26_000.0, 6000.0),
            (26_000.0, 12_000.0),
            (19_000.0, 12_000.0),
            (19_000.0, 6000.0),
        ],
    );
    hand.ellipse(
        pen(Tool::FountainPen, RED, 60.0),
        (22_500.0, 6000.0),
        700.0,
        700.0,
    );
    hand.line(text, 21_500.0, 24_000.0, 6250.0, 300.0);
    hand.arrow(title_pen, (19_500.0, 13_500.0), (25_500.0, 13_500.0));
    note.strokes = hand.strokes;
    notes.push(DemoNote {
        binder: "Classe/3e B",
        note,
    });

    // 3. Chemistry on squared paper: molecules as circles.
    let mut note = base_note(
        3,
        "Réaction chimique : combustion",
        10,
        A4_LANDSCAPE,
        background(Paper::White, Template::Grid5mm, false),
        &["chimie", "3e B"],
        &["Calque 1"],
        Origin::Live,
    );
    let mut hand = Hand::new(3);
    let green = pen(Tool::FeltTip, GREEN, 110.0);
    let purple = pen(Tool::FeltTip, PURPLE, 110.0);
    hand.line(purple, 2500.0, 18_000.0, 3000.0, 600.0);
    hand.paragraph(
        green,
        Area {
            top: 5000.0,
            ..full
        },
        4,
        1500.0,
        400.0,
    );
    for (i, color) in [BLACK, RED, RED, BLACK].into_iter().enumerate() {
        let x = 5000.0 + 2600.0 * i as f32;
        hand.ellipse(
            pen(Tool::FeltTip, color, 110.0),
            (x, 15_000.0),
            900.0,
            900.0,
        );
    }
    hand.arrow(purple, (15_000.0, 15_000.0), (19_000.0, 15_000.0));
    hand.ellipse(purple, (22_500.0, 15_000.0), 1500.0, 1000.0);
    note.strokes = hand.strokes;
    notes.push(DemoNote {
        binder: "Classe/3e B",
        note,
    });

    // 4. Chalk on the blackboard.
    let mut note = base_note(
        4,
        "Énergie cinétique",
        17,
        A4_LANDSCAPE,
        background(Paper::Slate, Template::Blank, true),
        &["physique"],
        &["Calque 1"],
        Origin::Ardoise,
    );
    let mut hand = Hand::new(4);
    let chalk = pen(Tool::Chalk, WHITE, 170.0);
    hand.line(
        pen(Tool::Chalk, YELLOW, 190.0),
        3000.0,
        17_000.0,
        3500.0,
        750.0,
    );
    hand.paragraph(
        chalk,
        Area {
            top: 6500.0,
            right: 18_000.0,
            ..full
        },
        5,
        1900.0,
        480.0,
    );
    hand.ellipse(chalk, (22_500.0, 12_000.0), 1400.0, 1400.0);
    hand.arrow(
        pen(Tool::Chalk, YELLOW, 170.0),
        (22_500.0, 12_000.0),
        (27_000.0, 9000.0),
    );
    note.strokes = hand.strokes;
    notes.push(DemoNote {
        binder: "Classe/3e B",
        note,
    });

    // 5. Personal ideas on kraft paper, pencil and brush.
    let mut note = base_note(
        5,
        "Idées pour Noël",
        21,
        A4_LANDSCAPE,
        background(Paper::Kraft, Template::Dotted, false),
        &["perso", "idées"],
        &["Calque 1"],
        Origin::Ardoise,
    );
    let mut hand = Hand::new(5);
    let pencil = pen(Tool::Pencil, GREY, 55.0);
    hand.line(
        pen(Tool::Brush, ORANGE, 260.0),
        2500.0,
        15_000.0,
        3200.0,
        700.0,
    );
    hand.paragraph(
        pencil,
        Area {
            top: 5600.0,
            right: 17_000.0,
            ..full
        },
        7,
        1500.0,
        360.0,
    );
    let star: Vec<(f32, f32)> = (0..=5)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_2 + TAU * 2.0 * i as f32 / 5.0;
            (22_000.0 + 2500.0 * a.cos(), 11_000.0 + 2500.0 * a.sin())
        })
        .collect();
    hand.polyline(pen(Tool::Brush, ORANGE, 240.0), &star);
    note.strokes = hand.strokes;
    notes.push(DemoNote {
        binder: "Perso",
        note,
    });

    // 6. A portrait sketch on graph paper, with an annotation layer.
    let mut note = base_note(
        6,
        "Croquis du jardin",
        24,
        A4_PORTRAIT,
        background(Paper::White, Template::Millimetric, false),
        &[],
        &["Calque 1", "Annotations"],
        Origin::Tablet,
    );
    let mut hand = Hand::new(6);
    let leaf = pen(Tool::Brush, GREEN, 200.0);
    for (x, y) in [(6000.0, 8000.0), (11_000.0, 7000.0), (15_500.0, 9000.0)] {
        hand.segment(leaf, (x, y + 600.0), (x + 300.0, 22_000.0));
        for petal in 0..5 {
            let a = TAU * petal as f32 / 5.0;
            hand.ellipse(
                pen(Tool::Ballpoint, PURPLE, 45.0),
                (x + 700.0 * a.cos(), y + 700.0 * a.sin()),
                500.0,
                350.0,
            );
        }
    }
    hand.segment(
        pen(Tool::Ballpoint, BLACK, 45.0),
        (1500.0, 22_500.0),
        (20_000.0, 22_300.0),
    );
    let annotation = Pen {
        layer: 1,
        ..pen(Tool::Ballpoint, RED, 45.0)
    };
    hand.paragraph(
        annotation,
        Area {
            left: 2000.0,
            right: 19_000.0,
            top: 25_000.0,
        },
        2,
        1400.0,
        350.0,
    );
    hand.arrow(annotation, (8000.0, 24_300.0), (10_500.0, 18_500.0));
    note.strokes = hand.strokes;
    notes.push(DemoNote {
        binder: "Perso",
        note,
    });

    // 7. Untitled: the library shows its date instead.
    let mut note = base_note(
        7,
        "",
        27,
        A4_LANDSCAPE,
        background(
            Paper::Custom(Rgba::rgb(0xe8, 0xf1, 0xfb)),
            Template::Blank,
            false,
        ),
        &[],
        &["Calque 1"],
        Origin::Tablet,
    );
    let mut hand = Hand::new(7);
    hand.paragraph(
        pen(Tool::Ballpoint, BLUE, 40.0),
        Area {
            top: 4000.0,
            right: 20_000.0,
            ..full
        },
        3,
        1500.0,
        400.0,
    );
    note.strokes = hand.strokes;
    notes.push(DemoNote { binder: "", note });

    notes
}

/// What [`create_demo_library`] did.
#[derive(Debug, Default)]
pub struct DemoReport {
    /// Files written, relative to the library, with a description.
    pub created: Vec<String>,
    /// Demo notes already present (same id), left alone.
    pub skipped: usize,
}

/// Makes `dir` (created if needed) a library holding the demo notes. Notes already there
/// (same id) are kept as they are, so running it twice adds nothing.
pub fn create_demo_library(dir: &Path) -> Result<DemoReport, String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    library::init(dir).map_err(|e| e.to_string())?;
    let library = Library::open(dir).map_err(|e| e.to_string())?;
    let existing: HashSet<String> = library.list().notes.into_iter().map(|n| n.id).collect();
    let mut report = DemoReport::default();
    for demo in demo_notes() {
        if existing.contains(&demo.note.id) {
            report.skipped += 1;
            continue;
        }
        let mut file = NoteFile::new(demo.note);
        file.thumbnail = thumbnail_png(&file.note);
        let saved = library
            .create(&binder_path(demo.binder), &file)
            .map_err(|e| e.to_string())?;
        let relative = saved.path.strip_prefix(dir).unwrap_or(&saved.path);
        let mut line = format!(
            "{} ({} traits",
            slash_path(relative),
            file.note.strokes.len()
        );
        if !file.note.tags.is_empty() {
            line.push_str(&format!(", étiquettes : {}", file.note.tags.join(", ")));
        }
        if file.note.favorite {
            line.push_str("; favori");
        }
        line.push(')');
        report.created.push(line);
    }
    Ok(report)
}

/// Runs the mode and returns the exit code.
pub fn run_cli(dir: &Path) -> i32 {
    if dir.as_os_str().is_empty() {
        println!("Usage : ardoise.exe --create-demo-library DOSSIER");
        return 2;
    }
    match create_demo_library(dir) {
        Ok(report) => {
            for line in &report.created {
                println!("Créée : {line}");
            }
            println!(
                "Bibliothèque de démonstration {} : {} note(s) créée(s), {} déjà présente(s)",
                dir.display(),
                report.created.len(),
                report.skipped
            );
            0
        }
        Err(error) => {
            println!("Échec de la création de la bibliothèque de démonstration : {error}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_core::format::read_note;

    #[test]
    fn demo_library_content() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Démo");
        let report = create_demo_library(&root).unwrap();
        assert_eq!(report.created.len(), 7);
        assert_eq!(report.skipped, 0);

        let library = Library::open(&root).unwrap();
        let listing = crate::library::list(&library);
        assert_eq!(listing.notes.len(), 7);
        assert!(listing.problems.is_empty());
        assert_eq!(listing.binders, ["Classe", "Classe/3e B", "Perso"]);
        assert_eq!(
            listing.notes.iter().filter(|n| n.binder.is_empty()).count(),
            2
        );
        assert_eq!(listing.notes.iter().filter(|n| n.favorite).count(), 1);
        assert!(listing.notes.iter().any(|n| n.title == "Énergie cinétique"));
        assert!(listing.notes.iter().any(|n| n.title.is_empty()));
        assert!(
            listing
                .tags
                .iter()
                .any(|t| t.tag == "physique" && t.count == 2)
        );

        let mut tools = HashSet::new();
        let mut papers = HashSet::new();
        let mut templates = HashSet::new();
        let mut days = HashSet::new();
        for entry in &listing.notes {
            let file = read_note(Path::new(&entry.path)).unwrap();
            assert!(file.thumbnail.is_some());
            assert!(file.note.strokes.len() >= 5, "{}", entry.title);
            // Every point on the page, times increasing.
            for stroke in &file.note.strokes {
                for pair in stroke.points.windows(2) {
                    assert!(pair[1].t > pair[0].t);
                }
                for p in &stroke.points {
                    assert!(p.x >= 0.0 && p.x <= file.note.page.width as f32, "{}", p.x);
                    assert!(p.y >= 0.0 && p.y <= file.note.page.height as f32, "{}", p.y);
                }
            }
            tools.extend(file.note.strokes.iter().map(|s| format!("{:?}", s.tool)));
            papers.insert(format!("{:?}", file.note.background.paper));
            templates.insert(format!("{:?}", file.note.background.template));
            days.insert(file.note.created / DAY_MS);
        }
        assert_eq!(tools.len(), Tool::ALL.len(), "{tools:?}");
        assert!(papers.len() >= 5 && templates.len() >= 6);
        assert_eq!(days.len(), 7);

        // Again: nothing added.
        let again = create_demo_library(&root).unwrap();
        assert!(again.created.is_empty());
        assert_eq!(again.skipped, 7);
    }

    #[test]
    fn demo_notes_are_deterministic() {
        let a: Vec<Note> = demo_notes().into_iter().map(|d| d.note).collect();
        let b: Vec<Note> = demo_notes().into_iter().map(|d| d.note).collect();
        assert_eq!(a, b);
    }

    #[test]
    fn missing_folder_argument_is_a_usage_error() {
        assert_eq!(run_cli(Path::new("")), 2);
    }
}
