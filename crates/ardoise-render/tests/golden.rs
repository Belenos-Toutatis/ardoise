//! Reference images: synthetic notes covering every tool, colour and pressure range, every
//! paper and template, texture on and off, a landscape and a portrait page, a hidden layer, a
//! background image (turned, translucent, reaching beyond the page) and text frames (both
//! families, turned, scaled, translucent, between strokes of their layers), rendered and
//! compared with `testdata/golden/*.png`.
//!
//! `cargo xtask golden` regenerates the images (it runs the ignored test
//! `regenerate_golden_images`); look at them before committing.

#![allow(clippy::expect_used)]

use std::f32::consts::TAU;
use std::fs;
use std::path::PathBuf;

use ardoise_core::note::{
    A4_LANDSCAPE, A4_PORTRAIT, Background, BackgroundImage, ImageRect, Layer, Note, Origin,
    PageObject, PageSize, Paper, Template, TextBody, Timing,
};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_render::{
    Image, NoteView, Picture, RenderOptions, decode_png, encode_png, render_note,
};
use ardoise_text::FontFamily;

/// Largest accepted mean absolute difference per channel (0..255) between a fresh render and
/// its reference image.
const MAX_MEAN_DIFFERENCE: f64 = 0.5;
/// Largest accepted share of pixels with a channel differing by more than
/// [`PIXEL_THRESHOLD`].
const MAX_DIFFERING_SHARE: f64 = 0.002;
const PIXEL_THRESHOLD: u8 = 16;
/// Largest accepted share of differing pixels within a single row or column.
const MAX_LINE_SHARE: f64 = 0.25;

// Inkspace's colours (research §5.2) and a few more.
const BLACK: Rgba = Rgba::BLACK;
const WHITE: Rgba = Rgba::WHITE;
const BLUE: Rgba = Rgba::rgb(0x51, 0xa7, 0xf9);
const GREEN: Rgba = Rgba::rgb(0x84, 0xbd, 0x00);
const MAGENTA: Rgba = Rgba::rgb(0xbb, 0x3e, 0x9a);
const RED: Rgba = Rgba::rgb(0xd8, 0x00, 0x00);
const GREY: Rgba = Rgba::rgb(0x6b, 0x6b, 0x6b);
const NAVY: Rgba = Rgba::rgb(0x1a, 0x2a, 0x80);
const VIOLET: Rgba = Rgba::rgb(0x4b, 0x2a, 0x9c);
const GRAPHITE: Rgba = Rgba::rgb(0x3a, 0x3a, 0x3c);
const YELLOW_MARKER: Rgba = Rgba::rgb(0xff, 0xe6, 0x2e);
const GREEN_MARKER: Rgba = Rgba::rgb(0x9c, 0xf0, 0x7a);
const PINK_MARKER: Rgba = Rgba::rgb(0xff, 0x8f, 0xc8);
const CHALK_YELLOW: Rgba = Rgba::rgb(0xff, 0xf0, 0x9a);
const CHALK_PINK: Rgba = Rgba::rgb(0xff, 0xb0, 0xc8);

/// Deterministic pseudo-random numbers (64-bit LCG, Knuth's constants).
struct Lcg(u64);

impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) as u32
    }

    fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
    }
}

fn pt(x: f32, y: f32, pressure: f32, t: u32) -> Point {
    Point { x, y, pressure, t }
}

fn stroke(tool: Tool, color: Rgba, width: f32, points: Vec<Point>) -> Stroke {
    Stroke {
        tool,
        color,
        width,
        layer: 0,
        points,
    }
}

fn on_layer(mut s: Stroke, layer: u32) -> Stroke {
    s.layer = layer;
    s
}

fn translucent(c: Rgba, a: u8) -> Rgba {
    Rgba { a, ..c }
}

/// Points along `f(u)` for `u` in 0..=1, `n + 1` samples, 5 ms apart.
fn trace(n: usize, f: impl Fn(f32) -> (f32, f32, f32)) -> Vec<Point> {
    (0..=n)
        .map(|i| {
            let (x, y, p) = f(i as f32 / n as f32);
            pt(x, y, p, i as u32 * 5)
        })
        .collect()
}

/// A wave from `(x, y)` over `length`, pressure going from `p0` to `p1`.
fn wave(x: f32, y: f32, length: f32, amplitude: f32, p0: f32, p1: f32) -> Vec<Point> {
    let n = (length / 40.0) as usize;
    trace(n, |u| {
        (
            x + u * length,
            y + amplitude * (u * TAU * 2.0).sin(),
            p0 + (p1 - p0) * u,
        )
    })
}

/// A zigzag with sharp turns, sampled densely along each tooth like a pen would.
fn zigzag(x: f32, y: f32, length: f32, height: f32, teeth: usize, p: f32) -> Vec<Point> {
    let mut points = Vec::new();
    let tooth = length / teeth as f32;
    for k in 0..teeth {
        for j in 0..8 {
            let u = j as f32 / 8.0;
            let (from, to) = if k % 2 == 0 {
                (y + height / 2.0, y - height / 2.0)
            } else {
                (y - height / 2.0, y + height / 2.0)
            };
            let t = (points.len() * 5) as u32;
            points.push(pt(x + (k as f32 + u) * tooth, from + (to - from) * u, p, t));
        }
    }
    let t = (points.len() * 5) as u32;
    let end = if teeth.is_multiple_of(2) {
        y + height / 2.0
    } else {
        y - height / 2.0
    };
    points.push(pt(x + length, end, p, t));
    points
}

/// Advance along the baseline of one cursive letter of height `h`.
fn letter_width(h: f32) -> f32 {
    0.5 * h
}

/// Cursive "handwriting": loops of letter height `h` (tall or short letters, picked by
/// `seed`) along the baseline `y` from `x`, with a light start and end.
fn cursive(x: f32, y: f32, h: f32, letters: usize, seed: u64, pressure: f32) -> Vec<Point> {
    let mut rng = Lcg(seed);
    let heights: Vec<f32> = (0..letters)
        .map(|_| if rng.unit() < 0.35 { h } else { 0.55 * h })
        .collect();
    // x = a·t + b·sin t loops back near the top of each letter when b > a.
    let advance = letter_width(h) / TAU;
    let swing = 1.6 * advance;
    let per_letter = 22;
    trace(letters * per_letter, |u| {
        let t = u * letters as f32 * TAU;
        let k = ((t / TAU) as usize).min(letters - 1);
        let ramp = (u * 8.0).min((1.0 - u) * 8.0).clamp(0.0, 1.0);
        (
            x + advance * t + swing * t.sin(),
            y - heights[k] * 0.5 * (1.0 - t.cos()),
            (pressure * (0.35 + 0.65 * ramp) + 0.12 * (t * 0.37).sin()).clamp(0.0, 1.0),
        )
    })
}

fn circle(cx: f32, cy: f32, r: f32, p: f32) -> Vec<Point> {
    trace(64, |u| {
        let a = u * TAU * 1.04;
        (cx + r * a.cos(), cy + r * a.sin(), p)
    })
}

fn segment(from: (f32, f32), to: (f32, f32), p: f32) -> Vec<Point> {
    let n = ((to.0 - from.0).hypot(to.1 - from.1) / 40.0).max(1.0) as usize;
    trace(n, |u| {
        (
            from.0 + (to.0 - from.0) * u,
            from.1 + (to.1 - from.1) * u,
            p,
        )
    })
}

fn note(page: PageSize, background: Background, strokes: Vec<Stroke>) -> Note {
    Note {
        id: "6f0e4b1c-0000-4000-8000-00000000901d".into(),
        title: "Golden".into(),
        created: 1_761_300_000_000,
        modified: 1_761_300_000_000,
        page,
        background,
        background_image: None,
        tags: vec![],
        favorite: false,
        layers: vec![Layer {
            name: "Calque 1".into(),
            visible: true,
        }],
        strokes,
        objects: Vec::new(),
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

fn bg(paper: Paper, template: Template, texture: bool) -> Background {
    Background {
        paper,
        template,
        texture,
    }
}

fn at(scale: f32) -> RenderOptions {
    RenderOptions {
        scale,
        background: true,
    }
}

/// Typical colours for each tool's row on the tools sheet.
fn tool_colours(tool: Tool) -> [Rgba; 4] {
    match tool {
        Tool::Ballpoint => [BLACK, BLUE, MAGENTA, RED],
        Tool::FountainPen => [NAVY, VIOLET, BLACK, RED],
        Tool::FeltTip => [GREEN, BLUE, MAGENTA, RED],
        Tool::Pencil => [GRAPHITE, GREY, BLUE, RED],
        Tool::Brush => [BLACK, RED, NAVY, GREEN],
        Tool::Highlighter => [YELLOW_MARKER, GREEN_MARKER, PINK_MARKER, BLUE],
        Tool::Chalk => [GREY, BLUE, MAGENTA, RED],
    }
}

/// Every tool (one row each), four colours, pressure ramps, sharp turns, loops and dots,
/// plus a row of translucent colours.
fn tools_sheet() -> Note {
    let mut strokes = Vec::new();
    for (row, tool) in Tool::ALL.into_iter().enumerate() {
        let y = 1000.0 + row as f32 * 2000.0;
        let w = tool.default_width();
        let [c0, c1, c2, c3] = tool_colours(tool);
        if tool == Tool::Highlighter {
            // Something to highlight: black handwriting under the marker.
            strokes.push(stroke(
                Tool::Ballpoint,
                BLACK,
                50.0,
                cursive(700.0, y + 250.0, 600.0, 17, 1, 0.8),
            ));
            strokes.push(stroke(
                Tool::Ballpoint,
                BLACK,
                50.0,
                cursive(10900.0, y + 250.0, 600.0, 13, 2, 0.8),
            ));
        }
        strokes.push(stroke(tool, c0, w, wave(600.0, y, 5400.0, 350.0, 0.0, 1.0)));
        strokes.push(stroke(
            tool,
            c1,
            w,
            zigzag(6800.0, y, 3700.0, 900.0, 6, 0.7),
        ));
        strokes.push(stroke(
            tool,
            c2,
            w,
            cursive(10900.0, y + 350.0, 800.0, 10, row as u64 + 10, 0.8),
        ));
        for (i, p) in [0.0, 0.5, 1.0].into_iter().enumerate() {
            let x = 16500.0 + i as f32 * 1200.0;
            strokes.push(stroke(tool, c3, w, vec![pt(x, y, p, 0)]));
        }
    }
    // Translucent colours: overlapping strokes show through, a stroke crossing itself does
    // not darken.
    let y = 15000.0;
    strokes.push(stroke(
        Tool::Ballpoint,
        translucent(BLUE, 140),
        120.0,
        cursive(600.0, y + 400.0, 900.0, 16, 7, 1.0),
    ));
    strokes.push(stroke(
        Tool::Ballpoint,
        translucent(RED, 140),
        120.0,
        wave(600.0, y, 7000.0, 500.0, 1.0, 1.0),
    ));
    strokes.push(stroke(
        Tool::FeltTip,
        translucent(GREEN, 160),
        200.0,
        zigzag(9000.0, y, 4000.0, 1000.0, 5, 1.0),
    ));
    strokes.push(stroke(
        Tool::Brush,
        translucent(MAGENTA, 120),
        300.0,
        circle(15500.0, y, 600.0, 1.0),
    ));
    strokes.push(stroke(
        Tool::Pencil,
        translucent(BLACK, 128),
        60.0,
        circle(18200.0, y, 600.0, 1.0),
    ));
    let page = PageSize {
        width: 20_000,
        height: 16_000,
    };
    note(page, Background::default(), strokes)
}

/// Each tool close up, to see textures, halos, tapers and nib shapes.
fn tools_close_up() -> Note {
    let mut strokes = Vec::new();
    for (row, tool) in Tool::ALL.into_iter().enumerate() {
        let y = 450.0 + row as f32 * 600.0;
        let w = tool.default_width();
        let [c0, ..] = tool_colours(tool);
        if tool == Tool::Highlighter {
            strokes.push(stroke(
                Tool::Ballpoint,
                BLACK,
                50.0,
                cursive(400.0, y + 150.0, 300.0, 34, 3, 0.9),
            ));
        }
        strokes.push(stroke(tool, c0, w, wave(300.0, y, 3300.0, 150.0, 0.0, 1.0)));
        strokes.push(stroke(
            tool,
            c0,
            w,
            cursive(3900.0, y + 200.0, 400.0, 8, 5, 0.9),
        ));
    }
    let page = PageSize {
        width: 6000,
        height: 4500,
    };
    note(page, Background::default(), strokes)
}

/// The blackboard: chalk and light inks on slate with a Seyès ruling and texture.
fn slate() -> Note {
    let strokes = vec![
        stroke(
            Tool::Chalk,
            WHITE,
            180.0,
            cursive(800.0, 2000.0, 1000.0, 14, 21, 0.9),
        ),
        stroke(
            Tool::Chalk,
            CHALK_YELLOW,
            220.0,
            wave(800.0, 3300.0, 8000.0, 400.0, 0.2, 1.0),
        ),
        stroke(
            Tool::Chalk,
            CHALK_PINK,
            160.0,
            zigzag(800.0, 4800.0, 5000.0, 800.0, 7, 0.8),
        ),
        stroke(
            Tool::Highlighter,
            YELLOW_MARKER,
            500.0,
            segment((6500.0, 4800.0), (11000.0, 4800.0), 1.0),
        ),
        stroke(
            Tool::Ballpoint,
            WHITE,
            50.0,
            cursive(6600.0, 4950.0, 500.0, 16, 22, 0.8),
        ),
        stroke(
            Tool::FeltTip,
            Rgba::rgb(0x8f, 0xd3, 0xff),
            110.0,
            circle(9800.0, 2000.0, 900.0, 0.9),
        ),
        stroke(
            Tool::Brush,
            WHITE,
            220.0,
            wave(800.0, 6600.0, 10000.0, 500.0, 1.0, 1.0),
        ),
    ];
    let page = PageSize {
        width: 12_000,
        height: 8_000,
    };
    note(page, bg(Paper::Slate, Template::Seyes, true), strokes)
}

/// A small page with handwriting on lines.
/// A text frame (SPEC §4.4) at `(x, y)`, turned and scaled.
#[allow(clippy::too_many_arguments)]
fn text_frame(
    layer: u32,
    (x, y): (f32, f32),
    text: &str,
    (font, bold, size): (FontFamily, bool, f32),
    color: Rgba,
    quarter_turns: u8,
    scale: f32,
) -> PageObject {
    let mut object = PageObject::text(
        format!("{x}-{y}"),
        layer,
        0,
        x,
        y,
        TextBody {
            text: text.into(),
            font,
            bold,
            size,
            color,
        },
    );
    object.quarter_turns = quarter_turns;
    object.scale = scale;
    object
}

/// Text frames in both families, bold or not, the three sizes, several colours (one
/// translucent), turned and scaled, several lines with a tab, accents and Greek letters (taken
/// from Andika); over a highlighter stroke of their layer, under a stroke of the layer above; one
/// on a hidden layer (not drawn), one on a layer without entry (on top).
fn text_frames() -> Note {
    const LM: FontFamily = FontFamily::LatinModern;
    const ANDIKA: FontFamily = FontFamily::Andika;
    let strokes = vec![
        stroke(
            Tool::Highlighter,
            YELLOW_MARKER,
            500.0,
            segment((900.0, 1450.0), (6200.0, 1450.0), 0.8),
        ),
        on_layer(
            stroke(
                Tool::Ballpoint,
                RED,
                60.0,
                cursive(1000.0, 1700.0, 600.0, 9, 5, 0.8),
            ),
            1,
        ),
    ];
    let objects = vec![
        text_frame(
            0,
            (1000.0, 900.0),
            "Leçon 3 : l'énergie",
            (LM, true, 900.0),
            NAVY,
            0,
            1.0,
        ),
        text_frame(
            0,
            (1000.0, 2400.0),
            "1.\tLa masse se conserve.\n2.\tÉlèves, œuvre « 12 € »\n\tα + β = γ",
            (ANDIKA, false, 600.0),
            BLACK,
            0,
            1.0,
        ),
        text_frame(
            0,
            (1000.0, 5000.0),
            "Petit texte translucide, gras",
            (LM, true, 400.0),
            translucent(MAGENTA, 150),
            0,
            1.0,
        ),
        text_frame(
            0,
            (1000.0, 5800.0),
            "Andika gras ×1,5",
            (ANDIKA, true, 400.0),
            GREEN,
            0,
            1.5,
        ),
        text_frame(
            1,
            (10_000.0, 800.0),
            "Tourné d'un quart",
            (ANDIKA, false, 600.0),
            BLUE,
            1,
            1.0,
        ),
        text_frame(
            1,
            (9200.0, 7000.0),
            "À l'envers",
            (LM, false, 600.0),
            RED,
            2,
            0.8,
        ),
        text_frame(
            1,
            (6400.0, 6900.0),
            "Trois quarts",
            (LM, false, 400.0),
            VIOLET,
            3,
            1.0,
        ),
        text_frame(
            2,
            (1000.0, 4400.0),
            "jamais vu",
            (ANDIKA, true, 900.0),
            RED,
            0,
            1.0,
        ),
        text_frame(
            5,
            (4200.0, 1100.0),
            "au-dessus",
            (ANDIKA, false, 400.0),
            GREY,
            0,
            1.0,
        ),
    ];
    let mut n = small_page(bg(Paper::Cream, Template::Seyes, true), strokes);
    n.layers = ["Calque 1", "Calque 2", "Masqué"]
        .iter()
        .enumerate()
        .map(|(i, name)| Layer {
            name: (*name).into(),
            visible: i != 2,
        })
        .collect();
    n.objects = objects;
    n
}

fn small_page(background: Background, strokes: Vec<Stroke>) -> Note {
    let page = PageSize {
        width: 10_500,
        height: 7_400,
    };
    note(page, background, strokes)
}

/// Lines of cursive words (3 to 7 letters, 5.5 mm tall) on baselines `gap` apart.
fn writing_lines(
    pen: (Tool, Rgba, f32),
    first: f32,
    gap: f32,
    lines: usize,
    words: usize,
) -> Vec<Stroke> {
    let (tool, color, width) = pen;
    let h = 550.0;
    (0..lines)
        .flat_map(|l| {
            let y = first + l as f32 * gap;
            let mut x = 1200.0;
            (0..words)
                .map(|w| {
                    let letters = 3 + (l * 7 + w * 3) % 5;
                    let seed = (l * 100 + w) as u64;
                    let s = stroke(tool, color, width, cursive(x, y, h, letters, seed, 0.75));
                    x += letters as f32 * letter_width(h) + 450.0;
                    s
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

fn lined_white() -> Note {
    let mut strokes = writing_lines((Tool::Ballpoint, BLACK, 50.0), 2400.0, 800.0, 5, 5);
    strokes.extend(writing_lines(
        (Tool::Ballpoint, BLUE, 50.0),
        6400.0,
        800.0,
        1,
        5,
    ));
    small_page(bg(Paper::White, Template::Lined, false), strokes)
}

fn seyes_cream() -> Note {
    let mut strokes = writing_lines((Tool::FountainPen, VIOLET, 90.0), 2400.0, 800.0, 4, 5);
    strokes.push(stroke(
        Tool::Ballpoint,
        RED,
        50.0,
        segment((3200.0, 5900.0), (9800.0, 5900.0), 0.8),
    ));
    strokes.push(stroke(
        Tool::Ballpoint,
        RED,
        50.0,
        cursive(3400.0, 6600.0, 500.0, 16, 77, 0.8),
    ));
    small_page(bg(Paper::Cream, Template::Seyes, true), strokes)
}

fn grid_kraft() -> Note {
    let strokes = vec![
        stroke(
            Tool::FeltTip,
            Rgba::rgb(0x20, 0x20, 0x20),
            110.0,
            circle(2500.0, 3700.0, 1500.0, 0.9),
        ),
        stroke(
            Tool::FeltTip,
            RED,
            110.0,
            vec![
                pt(5000.0, 2200.0, 1.0, 0),
                pt(8500.0, 2200.0, 1.0, 5),
                pt(8500.0, 5200.0, 1.0, 10),
                pt(5000.0, 5200.0, 1.0, 15),
                pt(5000.0, 2200.0, 1.0, 20),
            ],
        ),
        stroke(
            Tool::Pencil,
            GRAPHITE,
            35.0,
            segment((500.0, 6500.0), (10000.0, 1000.0), 0.6),
        ),
        stroke(
            Tool::Pencil,
            GRAPHITE,
            35.0,
            wave(500.0, 6200.0, 9500.0, 300.0, 0.2, 1.0),
        ),
    ];
    small_page(bg(Paper::Kraft, Template::Grid5mm, true), strokes)
}

fn millimetric_white() -> Note {
    let mut strokes = vec![
        stroke(
            Tool::Ballpoint,
            BLACK,
            50.0,
            segment((1000.0, 6500.0), (10000.0, 6500.0), 1.0),
        ),
        stroke(
            Tool::Ballpoint,
            BLACK,
            50.0,
            segment((1000.0, 6500.0), (1000.0, 500.0), 1.0),
        ),
    ];
    let curve = trace(120, |u| {
        let x = 1000.0 + u * 8500.0;
        (x, 6500.0 - 5500.0 * u * u, 0.7)
    });
    strokes.push(stroke(Tool::Pencil, BLUE, 35.0, curve));
    let points: Vec<Stroke> = (0..8)
        .map(|i| {
            let u = i as f32 / 7.0;
            let (x, y) = (1000.0 + u * 8500.0, 6500.0 - 5500.0 * u * u);
            stroke(Tool::Ballpoint, RED, 90.0, vec![pt(x, y, 1.0, 0)])
        })
        .collect();
    strokes.extend(points);
    small_page(bg(Paper::White, Template::Millimetric, false), strokes)
}

fn dotted_custom() -> Note {
    let strokes = vec![
        stroke(
            Tool::Brush,
            NAVY,
            220.0,
            wave(800.0, 2500.0, 9000.0, 600.0, 0.3, 1.0),
        ),
        stroke(
            Tool::Brush,
            RED,
            300.0,
            segment((1500.0, 4500.0), (4500.0, 6500.0), 1.0),
        ),
        stroke(
            Tool::Highlighter,
            GREEN_MARKER,
            500.0,
            segment((5000.0, 5500.0), (9800.0, 5500.0), 1.0),
        ),
        stroke(
            Tool::Ballpoint,
            BLACK,
            50.0,
            cursive(5200.0, 5650.0, 450.0, 18, 4, 0.8),
        ),
    ];
    small_page(
        bg(
            Paper::Custom(Rgba::rgb(222, 236, 248)),
            Template::Dotted,
            false,
        ),
        strokes,
    )
}

fn music_cream() -> Note {
    let mut strokes = Vec::new();
    // Note heads on the first staff and stems; a slur with the fountain pen.
    let top = (7400.0 - (3.0 * 2000.0 - 1200.0)) / 2.0;
    for i in 0..10 {
        let x = 2000.0 + i as f32 * 750.0;
        let y = top + 800.0 - (i % 5) as f32 * 100.0;
        strokes.push(stroke(
            Tool::Ballpoint,
            BLACK,
            180.0,
            vec![pt(x, y, 1.0, 0)],
        ));
        strokes.push(stroke(
            Tool::Ballpoint,
            BLACK,
            30.0,
            segment((x + 85.0, y), (x + 85.0, y - 700.0), 1.0),
        ));
    }
    strokes.push(stroke(
        Tool::FountainPen,
        BLACK,
        90.0,
        trace(40, |u| {
            (
                2000.0 + u * 6750.0,
                top + 2000.0 + 900.0 - 500.0 * (u * std::f32::consts::PI).sin(),
                1.0,
            )
        }),
    ));
    small_page(bg(Paper::Cream, Template::Music, false), strokes)
}

/// A full A4 landscape page of notes on lined paper with texture: handwriting, a drawing,
/// a highlighted line.
fn a4_landscape() -> Note {
    let mut strokes = writing_lines((Tool::Ballpoint, BLACK, 50.0), 2400.0, 800.0, 8, 11);
    strokes.extend(writing_lines(
        (Tool::Ballpoint, BLUE, 50.0),
        9600.0,
        800.0,
        4,
        11,
    ));
    strokes.push(stroke(
        Tool::Highlighter,
        YELLOW_MARKER,
        500.0,
        segment((1100.0, 3020.0), (9000.0, 3020.0), 1.0),
    ));
    strokes.push(stroke(
        Tool::FeltTip,
        RED,
        110.0,
        circle(22000.0, 8000.0, 3500.0, 0.9),
    ));
    strokes.push(stroke(
        Tool::FeltTip,
        RED,
        110.0,
        vec![
            pt(17000.0, 15000.0, 1.0, 0),
            pt(20500.0, 11000.0, 1.0, 5),
            pt(19300.0, 11200.0, 1.0, 10),
            pt(20500.0, 11000.0, 1.0, 15),
            pt(20400.0, 12200.0, 1.0, 20),
        ],
    ));
    strokes.push(stroke(
        Tool::Pencil,
        GRAPHITE,
        35.0,
        zigzag(16000.0, 18000.0, 11000.0, 2000.0, 9, 0.6),
    ));
    note(
        A4_LANDSCAPE,
        bg(Paper::White, Template::Lined, true),
        strokes,
    )
}

/// A4 portrait with three layers; the middle one (a big red cross) is hidden.
fn a4_portrait_layers() -> Note {
    let mut strokes = writing_lines((Tool::Ballpoint, NAVY, 50.0), 3000.0, 1000.0, 10, 8);
    strokes.push(on_layer(
        stroke(
            Tool::FeltTip,
            RED,
            400.0,
            segment((1000.0, 1000.0), (20600.0, 28700.0), 1.0),
        ),
        1,
    ));
    strokes.push(on_layer(
        stroke(
            Tool::FeltTip,
            RED,
            400.0,
            segment((20600.0, 1000.0), (1000.0, 28700.0), 1.0),
        ),
        1,
    ));
    strokes.push(on_layer(
        stroke(
            Tool::Highlighter,
            GREEN_MARKER,
            500.0,
            segment((1100.0, 5000.0), (12000.0, 5000.0), 1.0),
        ),
        2,
    ));
    strokes.push(on_layer(
        stroke(
            Tool::Brush,
            GREEN,
            300.0,
            wave(2000.0, 22000.0, 17000.0, 1500.0, 0.4, 1.0),
        ),
        2,
    ));
    let mut n = note(
        A4_PORTRAIT,
        bg(Paper::Cream, Template::Grid5mm, false),
        strokes,
    );
    n.layers = vec![
        Layer {
            name: "Texte".into(),
            visible: true,
        },
        Layer {
            name: "Brouillon".into(),
            visible: false,
        },
        Layer {
            name: "Annotations".into(),
            visible: true,
        },
    ];
    n
}

/// `background: false`: a textured Seyès kraft page renders as plain white.
fn background_off() -> Note {
    let strokes = writing_lines((Tool::FountainPen, BLACK, 90.0), 2400.0, 800.0, 3, 5);
    small_page(bg(Paper::Kraft, Template::Seyes, true), strokes)
}

/// Every reference image: file name (without `.png`), note and options.
/// A synthetic picture for the background image case: a 300 × 200 sheet with a soft colour
/// gradient, a dark grid every 20 pixels, a disc and a transparent band.
fn worksheet_picture() -> Picture {
    let (width, height) = (300u32, 200u32);
    let pixels = (0..height)
        .flat_map(|y| (0..width).map(move |x| (x, y)))
        .flat_map(|(x, y)| {
            let (dx, dy) = (x as f32 - 210.0, y as f32 - 90.0);
            if x % 20 == 0 || y % 20 == 0 {
                [40, 50, 90, 255]
            } else if dx * dx + dy * dy < 45.0 * 45.0 {
                [220, 60, 50, 255]
            } else if (120..150).contains(&y) {
                [30, 160, 90, 90]
            } else {
                [
                    (200 + x * 50 / width) as u8,
                    (215 + y * 30 / height) as u8,
                    235,
                    255,
                ]
            }
        })
        .collect();
    let image = Image {
        width,
        height,
        pixels,
    };
    Picture::decode(encode_png(&image).expect("encode")).expect("decode")
}

/// Seyès paper under a turned, translucent worksheet reaching beyond the left and bottom
/// edges, with writing over it.
fn background_image() -> (Note, Picture) {
    let mut n = small_page(
        bg(Paper::Cream, Template::Seyes, false),
        vec![
            stroke(
                Tool::Ballpoint,
                NAVY,
                60.0,
                cursive(2600.0, 3600.0, 600.0, 7, 41, 0.7),
            ),
            stroke(
                Tool::Highlighter,
                YELLOW_MARKER,
                500.0,
                segment((900.0, 2400.0), (6500.0, 2400.0), 1.0),
            ),
        ],
    );
    n.background_image = Some(BackgroundImage {
        name: "fiche.png".into(),
        pdf_page: None,
        rect: ImageRect {
            x: -800.0,
            y: 1200.0,
            width: 6000.0,
            height: 9000.0,
        },
        quarter_turns: 1,
        opacity: 0.85,
    });
    (n, worksheet_picture())
}

fn cases() -> Vec<(&'static str, Note, RenderOptions, Option<Picture>)> {
    let plain = RenderOptions {
        scale: 0.04,
        background: false,
    };
    let (with_image, picture) = background_image();
    vec![
        ("tools", tools_sheet(), at(0.05), None),
        ("tools_close_up", tools_close_up(), at(0.2), None),
        ("slate_seyes_texture", slate(), at(0.06), None),
        ("lined_white", lined_white(), at(0.06), None),
        ("seyes_cream_texture", seyes_cream(), at(0.06), None),
        ("grid_kraft_texture", grid_kraft(), at(0.06), None),
        ("millimetric_white", millimetric_white(), at(0.06), None),
        ("dotted_custom", dotted_custom(), at(0.06), None),
        ("music_cream", music_cream(), at(0.06), None),
        (
            "a4_landscape_lined_texture",
            a4_landscape(),
            at(0.025),
            None,
        ),
        (
            "a4_portrait_hidden_layer",
            a4_portrait_layers(),
            at(0.025),
            None,
        ),
        ("background_off", background_off(), plain, None),
        ("background_image", with_image, at(0.06), Some(picture)),
        ("text_frames", text_frames(), at(0.06), None),
    ]
}

fn render_case(note: &Note, options: &RenderOptions, picture: Option<&Picture>) -> Image {
    render_note(NoteView::new(note, picture), options).expect("render")
}

fn golden_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../testdata/golden")
}

/// Differences between a fresh render and its reference image.
struct Difference {
    /// Mean absolute difference per channel (0..255).
    mean: f64,
    /// Share of pixels with a channel differing by more than [`PIXEL_THRESHOLD`].
    share: f64,
    /// Largest such share within one row or one column: a lost 1-px template line hardly
    /// moves `share` but fills its whole row or column.
    line_share: f64,
}

impl Difference {
    fn ok(&self) -> bool {
        self.mean <= MAX_MEAN_DIFFERENCE
            && self.share <= MAX_DIFFERING_SHARE
            && self.line_share <= MAX_LINE_SHARE
    }
}

fn compare(a: &Image, b: &Image) -> Difference {
    let (width, height) = (a.width as usize, a.height as usize);
    let mut total = 0u64;
    let mut differing = 0u64;
    let mut per_row = vec![0u32; height];
    let mut per_column = vec![0u32; width];
    for (i, (pa, pb)) in a
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.pixels.as_chunks::<4>().0.iter())
        .enumerate()
    {
        let mut worst = 0u8;
        for (&x, &y) in pa.iter().zip(pb) {
            let d = x.abs_diff(y);
            total += u64::from(d);
            worst = worst.max(d);
        }
        if worst > PIXEL_THRESHOLD {
            differing += 1;
            per_row[i / width.max(1)] += 1;
            per_column[i % width.max(1)] += 1;
        }
    }
    let pixels = (a.pixels.len() / 4).max(1) as f64;
    let worst_row = per_row.iter().max().copied().unwrap_or(0) as f64 / width.max(1) as f64;
    let worst_column = per_column.iter().max().copied().unwrap_or(0) as f64 / height.max(1) as f64;
    Difference {
        mean: total as f64 / (pixels * 4.0),
        share: differing as f64 / pixels,
        line_share: worst_row.max(worst_column),
    }
}

#[test]
fn golden_images_match() {
    let dir = golden_dir();
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("golden");
    let mut failures = Vec::new();
    println!(
        "{:<28} {:>11} {:>10} {:>10} {:>10}  (limits: mean {MAX_MEAN_DIFFERENCE}, share \
         {MAX_DIFFERING_SHARE}, line {MAX_LINE_SHARE})",
        "image", "size", "mean diff", "share >16", "worst line"
    );
    for (name, note, options, picture) in cases() {
        let image = render_case(&note, &options, picture.as_ref());
        let path = dir.join(format!("{name}.png"));
        let reference = match fs::read(&path).map(|bytes| decode_png(&bytes)) {
            Ok(Ok(reference)) => reference,
            Ok(Err(e)) => {
                failures.push(format!("{name}: cannot decode {}: {e}", path.display()));
                continue;
            }
            Err(e) => {
                failures.push(format!("{name}: cannot read {}: {e}", path.display()));
                continue;
            }
        };
        if (reference.width, reference.height) != (image.width, image.height) {
            failures.push(format!(
                "{name}: size {}x{} instead of {}x{}",
                image.width, image.height, reference.width, reference.height
            ));
            continue;
        }
        let diff = compare(&image, &reference);
        let ok = diff.ok();
        println!(
            "{name:<28} {:>11} {:>10.4} {:>10.5} {:>10.4}  {}",
            format!("{}x{}", image.width, image.height),
            diff.mean,
            diff.share,
            diff.line_share,
            if ok { "ok" } else { "DIFFERENT" }
        );
        if !ok {
            fs::create_dir_all(&out).expect("output dir");
            let fresh = out.join(format!("{name}.png"));
            fs::write(&fresh, encode_png(&image).expect("encode")).expect("write");
            failures.push(format!(
                "{name}: mean difference {:.4}, {:.3} % of pixels differ, worst line {:.1} % \
                 (new render: {})",
                diff.mean,
                diff.share * 100.0,
                diff.line_share * 100.0,
                fresh.display()
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "rendering differs from the reference images in testdata/golden:\n  {}\n\
         If the change is intended, run `cargo xtask golden`, look at the new images and \
         commit them.",
        failures.join("\n  ")
    );
}

/// Writes every reference image. Run through `cargo xtask golden`.
#[test]
#[ignore = "regenerates testdata/golden; run `cargo xtask golden`"]
fn regenerate_golden_images() {
    let dir = golden_dir();
    fs::create_dir_all(&dir).expect("golden dir");
    let mut total = 0;
    for (name, note, options, picture) in cases() {
        let image = render_case(&note, &options, picture.as_ref());
        let bytes = encode_png(&image).expect("encode");
        total += bytes.len();
        fs::write(dir.join(format!("{name}.png")), &bytes).expect("write");
        println!(
            "{name:<28} {}x{} {:>7.1} KB",
            image.width,
            image.height,
            bytes.len() as f64 / 1024.0
        );
    }
    println!("total {:.1} KB in {}", total as f64 / 1024.0, dir.display());
}

/// The comparison must catch a template line that disappears although it is a tiny share of
/// the image: here the vertical margin of the Seyès ruling (about 0.16 % of the pixels).
#[test]
fn a_lost_one_pixel_line_is_caught() {
    let (_, note, options, _) = cases()
        .into_iter()
        .find(|(name, ..)| *name == "seyes_cream_texture")
        .expect("seyes_cream_texture case");
    let image = render_note(&note, &options).expect("render");
    let (width, height) = (image.width as usize, image.height as usize);
    let pixel = |img: &Image, x: usize, y: usize| {
        let i = (y * width + x) * 4;
        [img.pixels[i], img.pixels[i + 1], img.pixels[i + 2]]
    };
    // The margin is the column where most pixels are clearly red.
    let reddish = |x: usize| {
        (0..height)
            .filter(|&y| {
                let [r, g, _] = pixel(&image, x, y);
                i32::from(r) - i32::from(g) > 40
            })
            .count()
    };
    let margin = (1..width).max_by_key(|&x| reddish(x)).expect("a column");
    assert!(reddish(margin) > height / 2, "no margin line found");
    let mut damaged = image.clone();
    for y in 0..height {
        let left = pixel(&image, margin - 4, y);
        let i = (y * width + margin) * 4;
        damaged.pixels[i..i + 3].copy_from_slice(&left);
    }

    let diff = compare(&damaged, &image);
    println!(
        "margin column {margin} erased: mean {:.4}, share {:.5}, worst line {:.3}",
        diff.mean, diff.share, diff.line_share
    );
    assert!(
        diff.share <= MAX_DIFFERING_SHARE,
        "the global share alone would miss it"
    );
    assert!(!diff.ok(), "the per-line check must catch it");
}
