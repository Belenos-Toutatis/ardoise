//! Rendering timings: an A4 landscape page of handwriting, 300 strokes / about 20 000
//! points, rendered at 0.04 px/unit (Inkspace's preview size) and at 1080p width.
//!
//! `cargo run --release -p ardoise-render --example render_bench`

use std::f32::consts::TAU;
use std::hint::black_box;
use std::time::{Duration, Instant};

use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Note, Origin, Paper, Template, Timing};
use ardoise_ink::{Point, Rgba, Stroke, Tool, outline};
use ardoise_render::{RenderOptions, encode_png, render_note};

const LINES: usize = 15;
const WORDS_PER_LINE: usize = 20;
const POINTS_PER_WORD: usize = 67;

/// One cursive word: loops along a baseline, sampled like the tablet (every 5 ms).
fn word(x: f32, y: f32, letters: usize, seed: usize) -> Vec<Point> {
    let h = 500.0;
    let advance = 0.5 * h / TAU;
    (0..POINTS_PER_WORD)
        .map(|i| {
            let u = i as f32 / (POINTS_PER_WORD - 1) as f32;
            let t = u * letters as f32 * TAU;
            let tall = (seed + (t / TAU) as usize).is_multiple_of(3);
            let height = if tall { h } else { 0.55 * h };
            Point {
                x: x + advance * t + 1.6 * advance * t.sin(),
                y: y - height * 0.5 * (1.0 - t.cos()),
                pressure: 0.5 + 0.4 * (t * 0.37).sin(),
                t: i as u32 * 5,
            }
        })
        .collect()
}

/// A page of handwriting; with `tools`, each line uses another tool.
fn page(tools: bool, background: Background) -> Note {
    let mut strokes = Vec::with_capacity(LINES * WORDS_PER_LINE);
    for line in 0..LINES {
        let tool = if tools {
            Tool::ALL[line % Tool::ALL.len()]
        } else {
            Tool::Ballpoint
        };
        let width = if tool == Tool::Highlighter {
            150.0
        } else {
            tool.default_width().min(120.0)
        };
        let y = 1800.0 + line as f32 * 1300.0;
        let mut x = 800.0;
        for w in 0..WORDS_PER_LINE {
            let letters = 3 + (line + w) % 3;
            strokes.push(Stroke {
                tool,
                color: Rgba::rgb(20, 40, 120),
                width,
                layer: 0,
                points: word(x, y, letters, line * 31 + w),
            });
            x += letters as f32 * 250.0 + 150.0;
        }
    }
    Note {
        id: "bench".into(),
        title: "Bench".into(),
        created: 0,
        modified: 0,
        page: A4_LANDSCAPE,
        background,
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

/// Runs `f` several times; prints the fastest and the median duration.
fn time(label: &str, mut f: impl FnMut()) {
    let mut runs: Vec<Duration> = (0..7)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed()
        })
        .collect();
    runs.sort();
    println!(
        "{label:<48} min {:>7.1} ms   median {:>7.1} ms",
        runs[0].as_secs_f64() * 1000.0,
        runs[runs.len() / 2].as_secs_f64() * 1000.0
    );
}

fn main() {
    let plain = Background::default();
    let ruled = Background {
        paper: Paper::Cream,
        template: Template::Seyes,
        texture: true,
    };
    for (name, note) in [
        ("ballpoint, white page", page(false, plain)),
        ("all tools, Seyès + texture", page(true, ruled)),
    ] {
        let points: usize = note.strokes.iter().map(|s| s.points.len()).sum();
        let outlines: Vec<_> = note.strokes.iter().map(outline).collect();
        let contours: usize = outlines.iter().map(|o| o.contours.len()).sum();
        let vertices: usize = outlines
            .iter()
            .flat_map(|o| &o.contours)
            .map(Vec::len)
            .sum();
        println!(
            "\n{name}: {} strokes, {points} points -> {contours} contours, {vertices} vertices",
            note.strokes.len()
        );
        time("outlines only", || {
            for stroke in &note.strokes {
                black_box(outline(stroke));
            }
        });
        let preview = RenderOptions {
            scale: 0.04,
            background: true,
        };
        let full_hd = RenderOptions::fit_width(note.page, 1920);
        let empty = Note {
            strokes: vec![],
            ..note.clone()
        };
        for (label, options) in [
            ("0.04 px/unit (1188 x 864)", preview),
            ("1080p width (1920 x 1396)", full_hd),
        ] {
            time(&format!("background only at {label}"), || {
                black_box(render_note(&empty, &options).ok());
            });
            time(&format!("render_note at {label}"), || {
                black_box(render_note(&note, &options).ok());
            });
        }
        if let Ok(image) = render_note(&note, &full_hd) {
            time("encode_png at 1080p width", || {
                black_box(encode_png(&image).ok());
            });
        }
    }
}
