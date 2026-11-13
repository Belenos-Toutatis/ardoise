//! Tests of the replay: timeline arithmetic, frames against `render_note`, performance.

use std::time::{Duration, Instant};

use ardoise_core::note::{
    A4_LANDSCAPE, Background, Layer, Note, ObjectBody, Origin, PageObject, PageSize, Paper,
    Template, TextBody, Timing,
};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_render::{
    Backdrop, DrawItem, Image, RenderOptions, paper_color, render_note, render_note_transparent,
};
use ardoise_text::FontFamily;

use super::*;
use crate::MediaError;

const PAGE: PageSize = PageSize {
    width: 6000,
    height: 4000,
};

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn in_ms(duration: Duration) -> f64 {
    duration.as_nanos() as f64 / 1e6
}

/// `n` samples (at least 2) from `from` to `to`, evenly spread from `t0` to `t1` ms.
fn line(tool: Tool, layer: u32, from: (f32, f32), to: (f32, f32), t0: u32, t1: u32) -> Stroke {
    let n = 20u32;
    let points = (0..n)
        .map(|i| {
            let u = i as f32 / (n - 1) as f32;
            Point {
                x: from.0 + (to.0 - from.0) * u,
                y: from.1 + (to.1 - from.1) * u,
                pressure: 0.4 + 0.5 * u,
                t: t0 + (u64::from(t1 - t0) * u64::from(i) / u64::from(n - 1)) as u32,
            }
        })
        .collect();
    Stroke {
        tool,
        color: Rgba::rgb(30, 50, 140),
        width: if tool == Tool::Highlighter {
            300.0
        } else {
            tool.default_width().max(60.0)
        },
        layer,
        points,
    }
}

/// A stroke made of samples at these times, all at one place.
fn timed(times: &[u32]) -> Stroke {
    Stroke {
        points: times
            .iter()
            .enumerate()
            .map(|(i, &t)| Point {
                x: 1000.0 + 20.0 * i as f32,
                y: 1000.0,
                pressure: 0.5,
                t,
            })
            .collect(),
        ..line(Tool::Ballpoint, 0, (0.0, 0.0), (1.0, 1.0), 0, 1)
    }
}

fn note(strokes: Vec<Stroke>, visible: &[bool]) -> Note {
    Note {
        id: "00000000-0000-4000-8000-000000000000".into(),
        title: "replay".into(),
        created: 1_700_000_000_000,
        modified: 1_700_000_000_000,
        page: PAGE,
        background: Background {
            paper: Paper::Cream,
            template: Template::Seyes,
            texture: true,
        },
        background_image: None,
        tags: vec![],
        favorite: false,
        layers: visible
            .iter()
            .enumerate()
            .map(|(i, &visible)| Layer {
                name: format!("{}", i + 1),
                visible,
            })
            .collect(),
        strokes,
        objects: Vec::new(),
        timing: Timing::Recorded,
        origin: Origin::Tablet,
    }
}

fn options(speed: f64, max_pause: Option<u64>) -> ReplayOptions {
    ReplayOptions {
        speed,
        max_pause: max_pause.map(ms),
    }
}

/// Strokes over [0, 1000], [3000, 4000] and [4200, 5000] ms.
fn three_strokes() -> Note {
    note(
        vec![
            line(Tool::Ballpoint, 0, (500.0, 500.0), (5500.0, 600.0), 0, 1000),
            line(
                Tool::Ballpoint,
                0,
                (500.0, 1500.0),
                (5500.0, 1600.0),
                3000,
                4000,
            ),
            line(
                Tool::Ballpoint,
                0,
                (500.0, 2500.0),
                (5500.0, 2600.0),
                4200,
                5000,
            ),
        ],
        &[true],
    )
}

fn assert_close(actual: f64, expected: f64, what: &str) {
    assert!(
        (actual - expected).abs() < 1e-3,
        "{what}: {actual} instead of {expected}"
    );
}

#[test]
fn speed_divides_the_duration() {
    let n = three_strokes();
    let normal = Timeline::new(&n, &ReplayOptions::default()).unwrap();
    assert_eq!(normal.duration(), ms(5000));
    assert_eq!(normal.note_end(), 5000.0);
    // At speed 1 with every pause kept, replay time is note time.
    for t in [0u32, 1, 999, 2000, 3500, 5000] {
        assert_eq!(normal.to_replay(f64::from(t)), ms(u64::from(t)));
        assert_close(normal.to_note(ms(u64::from(t))), f64::from(t), "speed 1");
    }
    let fast = Timeline::new(&n, &options(2.0, None)).unwrap();
    assert_eq!(fast.duration(), ms(2500));
    assert_eq!(fast.to_replay(3000.0), ms(1500));
    assert_close(fast.to_note(ms(1500)), 3000.0, "speed 2");
    let slow = Timeline::new(&n, &options(MIN_SPEED, None)).unwrap();
    assert_eq!(slow.duration(), ms(20_000));
    let fastest = Timeline::new(&n, &options(MAX_SPEED, None)).unwrap();
    assert_eq!(fastest.duration(), Duration::from_micros(312_500));
    // Past the end, or before the start: clamped.
    assert_eq!(fast.to_replay(1e12), fast.duration());
    assert_eq!(fast.to_replay(-5.0), Duration::ZERO);
    assert_eq!(fast.to_replay(f64::NAN), Duration::ZERO);
    assert_eq!(fast.to_note(ms(99_999)), 5000.0);
}

#[test]
fn long_pauses_are_cut_after_max_pause() {
    let n = three_strokes();
    // 1000 + 500 (of the 2000 ms pause) + 1000 + 200 (short pause, kept) + 800.
    let trimmed = Timeline::new(&n, &options(1.0, Some(500))).unwrap();
    assert_eq!(trimmed.duration(), ms(3500));
    assert_eq!(trimmed.to_replay(1200.0), ms(1200));
    // The rest of the pause is cut: it all maps to the instant of the cut...
    assert_eq!(trimmed.to_replay(1500.0), ms(1500));
    assert_eq!(trimmed.to_replay(2000.0), ms(1500));
    assert_eq!(trimmed.to_replay(3000.0), ms(1500));
    // ...where the next stroke starts.
    assert_close(trimmed.to_note(ms(1500)), 3000.0, "after the cut");
    assert_close(trimmed.to_note(ms(1499)), 1499.0, "before the cut");
    assert_eq!(trimmed.to_replay(4100.0), ms(2600));
    assert_eq!(trimmed.to_replay(4300.0), ms(2800));
    // The speed applies after the trimming.
    let fast = Timeline::new(&n, &options(2.0, Some(500))).unwrap();
    assert_eq!(fast.duration(), ms(1750));
    assert_eq!(fast.to_replay(4300.0), ms(1400));
    // No pause at all; pauses all shorter than the limit.
    let none = Timeline::new(&n, &options(1.0, Some(0))).unwrap();
    assert_eq!(none.duration(), ms(2800));
    assert_close(none.to_note(ms(1000)), 3000.0, "no pause");
    let loose = Timeline::new(&n, &options(1.0, Some(2000))).unwrap();
    assert_eq!(loose.duration(), ms(5000));

    // The time before the first stroke is a pause too.
    let late = note(
        vec![line(
            Tool::Ballpoint,
            0,
            (0.0, 0.0),
            (900.0, 0.0),
            10_000,
            11_000,
        )],
        &[true],
    );
    let kept = Timeline::new(&late, &ReplayOptions::default()).unwrap();
    assert_eq!(kept.duration(), ms(11_000));
    assert!(kept.note_at(ms(9_999)).strokes.is_empty());
    let cut = Timeline::new(&late, &options(1.0, Some(500))).unwrap();
    assert_eq!(cut.duration(), ms(1_500));
    assert!(cut.note_at(ms(499)).strokes.is_empty());
    assert_eq!(cut.note_at(ms(500)).strokes[0].points.len(), 1);
}

#[test]
fn the_pen_standing_still_inside_a_stroke_is_not_a_pause() {
    let n = note(vec![timed(&[0, 100, 5100, 5200])], &[true]);
    let t = Timeline::new(&n, &options(1.0, Some(500))).unwrap();
    assert_eq!(t.duration(), ms(5200));
    assert_eq!(t.note_at(ms(5099)).strokes[0].points.len(), 2);
}

#[test]
fn overlapping_strokes_pause_only_when_none_is_being_drawn() {
    // Unsorted in the note, on two layers, B written "during" A (merged or edited notes).
    let a = line(Tool::Ballpoint, 0, (500.0, 500.0), (5500.0, 500.0), 0, 3000);
    let b = line(
        Tool::Highlighter,
        1,
        (500.0, 900.0),
        (5500.0, 900.0),
        1000,
        2000,
    );
    let c = line(
        Tool::FeltTip,
        0,
        (500.0, 1500.0),
        (5500.0, 1500.0),
        5000,
        6000,
    );
    let n = note(vec![c, b, a], &[true, true]);
    let t = Timeline::new(&n, &options(1.0, Some(1000))).unwrap();
    // Busy over [0, 3000] and [5000, 6000]; the 2000 ms pause is cut to 1000.
    assert_eq!(t.duration(), ms(5000));
    // Strokes appear by time, not by note order.
    let at = |r| -> Vec<usize> {
        t.note_at(ms(r))
            .strokes
            .iter()
            .map(|s| s.points.len())
            .collect()
    };
    assert_eq!(at(0), [1]);
    assert_eq!(at(1500), [10, 10]);
    assert_eq!(at(3999), [20, 20]);
    assert_eq!(at(4000), [1, 20, 20]);
    // The strokes keep their note order and layers.
    let full = t.note_at(t.duration());
    assert_eq!(full.strokes, n.strokes);
    assert_eq!(full.layers, n.layers);
}

#[test]
fn the_mapping_round_trips() {
    let n = three_strokes();
    for (speed, max_pause) in [
        (1.0, None),
        (1.5, Some(500)),
        (0.25, Some(0)),
        (16.0, Some(150)),
    ] {
        let t = Timeline::new(&n, &options(speed, max_pause)).unwrap();
        let what = format!("speed {speed}, max pause {max_pause:?}");
        // Note time inside a cut maps to the instant of the cut, shown as the time after it.
        let cut = |tau: f64| match max_pause {
            Some(cap) if tau >= 1000.0 + cap as f64 && tau < 3000.0 => Some(3000.0),
            Some(cap) if tau >= 4000.0 + cap as f64 && tau < 4200.0 => Some(4200.0),
            _ => None,
        };
        let mut previous = Duration::ZERO;
        for tau in (0..=5000).step_by(7).map(f64::from) {
            let r = t.to_replay(tau);
            assert!(r >= previous, "{what}: not monotonic at {tau}");
            previous = r;
            let back = t.to_note(r);
            assert_close(back, cut(tau).unwrap_or(tau), &format!("{what} at {tau}"));
        }
        let mut previous = 0.0;
        let step = (t.duration().as_micros() / 997).max(1) as u64;
        for r in (0..=t.duration().as_micros() as u64).step_by(step as usize) {
            let r = Duration::from_micros(r);
            let tau = t.to_note(r);
            assert!(tau >= previous, "{what}: not monotonic at {r:?}");
            previous = tau;
            assert_close(
                in_ms(t.to_replay(tau)),
                in_ms(r),
                &format!("{what} at {r:?}"),
            );
        }
        assert_eq!(t.to_replay(t.to_note(t.duration())), t.duration());
    }
}

#[test]
fn empty_and_degenerate_notes() {
    let empty_stroke = Stroke {
        points: vec![],
        ..timed(&[0])
    };
    let hidden = Stroke {
        layer: 1,
        ..timed(&[0, 100])
    };
    for n in [
        note(vec![], &[true]),
        note(vec![empty_stroke], &[true]),
        note(vec![hidden], &[true, false]),
    ] {
        for max_pause in [None, Some(0), Some(500)] {
            let t = Timeline::new(&n, &options(2.0, max_pause)).unwrap();
            assert_eq!(t.duration(), Duration::ZERO);
            assert_eq!(t.frame_count(30), 1);
            assert_eq!(t.to_note(ms(123)), 0.0);
            assert_eq!(t.to_replay(123.0), Duration::ZERO);
            assert!(t.note_at(Duration::ZERO).strokes.is_empty());
            assert!(t.drawing_order().is_empty());
        }
        let frames = FrameRenderer::new(
            Timeline::new(&n, &ReplayOptions::default()).unwrap(),
            &settings(60, 40),
        );
        let mut frames = frames.unwrap();
        assert_eq!(frames.frame_count(), 1);
        let blank = render_note(&n, &opts(0.01)).unwrap();
        assert_eq!(frames.render_frame(0).unwrap(), blank.pixels);
    }

    // Single-point strokes, identical timestamps: they appear together.
    let n = note(vec![timed(&[500]), timed(&[500]), timed(&[2000])], &[true]);
    let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
    assert_eq!(t.duration(), ms(2000));
    assert!(t.note_at(Duration::from_micros(499_999)).strokes.is_empty());
    assert_eq!(t.note_at(ms(500)).strokes.len(), 2);
    assert_eq!(t.note_at(ms(2000)).strokes.len(), 3);
    // Every sample at the same instant: the whole stroke at once.
    let n = note(vec![timed(&[100, 100, 100])], &[true]);
    let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
    assert!(t.note_at(ms(99)).strokes.is_empty());
    assert_eq!(t.note_at(ms(100)).strokes[0].points.len(), 3);
    // All at time zero: a replay of one frame showing everything.
    let n = note(vec![timed(&[0, 0]), timed(&[0])], &[true]);
    let t = Timeline::new(&n, &options(1.0, Some(0))).unwrap();
    assert_eq!((t.duration(), t.frame_count(25)), (Duration::ZERO, 1));
    assert_eq!(t.note_at(Duration::ZERO).strokes, n.strokes);
    // Times going backwards inside a stroke: a sample never comes before the one it follows.
    let n = note(vec![timed(&[0, 50, 30, 80, 60])], &[true]);
    let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
    assert_eq!(t.duration(), ms(80));
    let shown = |r| t.note_at(ms(r)).strokes[0].points.len();
    assert_eq!(
        [shown(29), shown(49), shown(50), shown(79), shown(80)],
        [1, 1, 3, 3, 5]
    );
}

#[test]
fn speeds_outside_the_limits_are_refused() {
    let n = three_strokes();
    for speed in [MIN_SPEED, 1.0, 3.7, MAX_SPEED] {
        assert!(Timeline::new(&n, &options(speed, None)).is_ok(), "{speed}");
    }
    for speed in [0.0, 0.2499, 16.01, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            matches!(
                Timeline::new(&n, &options(speed, None)),
                Err(MediaError::InvalidSettings(_))
            ),
            "{speed}"
        );
    }
    // A target duration picks the speed, within the limits.
    let speed_for = |max_pause: Option<u64>, target: u64| {
        ReplayOptions::for_duration(&n, max_pause.map(ms), ms(target)).speed
    };
    assert_eq!(speed_for(None, 2500), 2.0);
    assert_eq!(speed_for(Some(500), 7000), 0.5);
    assert_eq!(speed_for(None, 10), MAX_SPEED);
    assert_eq!(speed_for(None, 0), MAX_SPEED);
    assert_eq!(speed_for(None, 3_600_000), MIN_SPEED);
    let picked = ReplayOptions::for_duration(&n, Some(ms(500)), ms(1750));
    assert_eq!(picked, options(2.0, Some(500)));
    let t = Timeline::new(&n, &picked).unwrap();
    assert_eq!(t.duration(), ms(1750));
    let empty = note(vec![], &[true]);
    assert_eq!(ReplayOptions::for_duration(&empty, None, ms(10)).speed, 1.0);
}

#[test]
fn hidden_layers_are_neither_drawn_nor_waited_for() {
    let shown = line(Tool::Ballpoint, 0, (0.0, 0.0), (900.0, 0.0), 0, 1000);
    let hidden = line(Tool::Ballpoint, 1, (0.0, 900.0), (900.0, 900.0), 2000, 9000);
    // A layer index without a layer entry is drawn, as by every export.
    let orphan = line(
        Tool::Ballpoint,
        5,
        (0.0, 1900.0),
        (900.0, 1900.0),
        1500,
        1800,
    );
    let n = note(vec![shown.clone(), hidden, orphan.clone()], &[true, false]);
    let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
    assert_eq!(t.duration(), ms(1800));
    assert_eq!(t.drawing_order(), [0, 2]);
    assert_eq!(t.note_at(ms(10_000)).strokes, [shown, orphan]);
}

#[test]
fn synthesized_timing_follows_the_same_rules() {
    let recorded = three_strokes();
    let synthesized = Note {
        timing: Timing::Synthesized,
        origin: Origin::Inkspace { id: "a".repeat(36) },
        ..recorded.clone()
    };
    let o = options(1.5, Some(400));
    let a = Timeline::new(&recorded, &o).unwrap();
    let b = Timeline::new(&synthesized, &o).unwrap();
    assert_eq!(a.duration(), b.duration());
    for r in (0..a.duration().as_millis() as u64).step_by(37) {
        assert_eq!(a.to_note(ms(r)), b.to_note(ms(r)));
        assert_eq!(a.note_at(ms(r)).strokes, b.note_at(ms(r)).strokes);
    }
}

#[test]
fn frame_times_and_counts() {
    let n = three_strokes();
    let two_seconds = Timeline::new(&n, &options(2.5, None)).unwrap();
    assert_eq!(two_seconds.duration(), ms(2000));
    // Frames 0..=60: the last one at exactly 2 s shows the whole note.
    assert_eq!(two_seconds.frame_count(30), 61);
    assert_eq!(frame_time(60, 30), ms(2000));
    assert_eq!(frame_time(1, 30), Duration::from_nanos(33_333_333));
    assert_eq!(frame_time(2, 30), Duration::from_nanos(66_666_666));
    // 3.125 s at 24 fps: 75 frames cover 3.083 s, frame 75 (3.125 s) ends it.
    let odd = Timeline::new(&n, &options(1.6, None)).unwrap();
    assert_eq!(odd.duration(), Duration::from_millis(3125));
    assert_eq!(odd.frame_count(24), 76);
    assert!(frame_time(75, 24) >= odd.duration());
    assert!(frame_time(74, 24) < odd.duration());
    assert_eq!(odd.frame_count(0), 1);
    assert_eq!(frame_time(5, 0), Duration::ZERO);
}

// ---------------------------------------------------------------------------------------
// Frames
// ---------------------------------------------------------------------------------------

fn settings(width: u32, height: u32) -> FrameSettings {
    FrameSettings {
        width,
        height,
        frame_rate: 24,
        backdrop: Backdrop::Paper,
    }
}

fn opts(scale: f32) -> RenderOptions {
    RenderOptions {
        scale,
        background: true,
    }
}

/// A note with every non-grain tool, on two layers, written out of drawing order: a stroke
/// of the top layer is written before the strokes of the bottom layer it crosses, and the
/// note's order is not the order of time.
fn smooth_tools_note() -> Note {
    let strokes = vec![
        line(
            Tool::Ballpoint,
            0,
            (300.0, 600.0),
            (5700.0, 900.0),
            900,
            1500,
        ),
        // Top layer, written first, crossing everything.
        line(
            Tool::Highlighter,
            1,
            (3000.0, 200.0),
            (3200.0, 3800.0),
            0,
            800,
        ),
        line(
            Tool::FountainPen,
            0,
            (300.0, 1500.0),
            (5700.0, 1300.0),
            1600,
            2200,
        ),
        line(
            Tool::FeltTip,
            0,
            (300.0, 2300.0),
            (5700.0, 2500.0),
            2300,
            2600,
        ),
        line(
            Tool::Brush,
            0,
            (300.0, 3200.0),
            (5700.0, 3000.0),
            3300,
            3900,
        ),
        // Bottom layer, written last, under the highlighter.
        line(
            Tool::Highlighter,
            0,
            (2000.0, 3700.0),
            (4000.0, 300.0),
            4000,
            4400,
        ),
        // Overlapping in time with the brush.
        line(
            Tool::Ballpoint,
            1,
            (500.0, 300.0),
            (5500.0, 3700.0),
            3500,
            3700,
        ),
    ];
    note(strokes, &[true, true])
}

fn assert_same_image(frame: &[u8], expected: &Image, what: &str) {
    assert_eq!(frame.len(), expected.pixels.len(), "{what}: size");
    let differing = frame
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.pixels.as_chunks::<4>().0)
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differing, 0, "{what}: {differing} pixels differ");
}

#[test]
fn every_frame_is_the_note_written_so_far() {
    let n = smooth_tools_note();
    let t = Timeline::new(&n, &options(1.0, Some(300))).unwrap();
    let mut frames = FrameRenderer::new(t, &settings(240, 160)).unwrap();
    let count = frames.frame_count();
    assert!(count > 80, "{count} frames");
    for k in 0..count {
        let at = frames.frame_time(k);
        let expected = render_note(&frames.timeline().note_at(at), &opts(0.04)).unwrap();
        let frame = frames.render_frame(k).unwrap();
        assert_same_image(frame, &expected, &format!("frame {k} at {at:?}"));
        assert!(frame.as_chunks::<4>().0.iter().all(|p| p[3] == 255));
    }
    // The last frame is the exported image.
    let full = render_note(&n, &opts(0.04)).unwrap();
    assert_same_image(frames.render_frame(count - 1).unwrap(), &full, "last frame");
}

#[test]
fn the_last_frame_is_the_exported_image_with_bands_of_paper() {
    let mut strokes = smooth_tools_note().strokes;
    strokes.push(line(
        Tool::Pencil,
        0,
        (400.0, 400.0),
        (5000.0, 3600.0),
        4500,
        5000,
    ));
    strokes.push(line(
        Tool::Chalk,
        1,
        (400.0, 3600.0),
        (5000.0, 400.0),
        5100,
        5600,
    ));
    let n = note(strokes, &[true, true]);
    let paper = paper_color(n.background.paper);
    let band = [paper.r, paper.g, paper.b, 255];
    // Wider than the page, then taller: centred, the rest plain paper.
    for (width, height, x, y) in [(330, 200, 15, 0), (300, 251, 0, 25), (300, 200, 0, 0)] {
        let t = Timeline::new(&n, &options(4.0, None)).unwrap();
        let mut frames = FrameRenderer::new(t, &settings(width, height)).unwrap();
        let place = frames.placement();
        assert_eq!(
            (place.x, place.y, place.width, place.height),
            (x, y, 300, 200)
        );
        assert_eq!(place.scale, 0.05);
        let expected = render_note(&n, &opts(place.scale)).unwrap();
        let last = frames.frame_count() - 1;
        let frame = frames.render_frame(last).unwrap();
        for py in 0..height {
            for px in 0..width {
                let i = ((py * width + px) * 4) as usize;
                let inside = (x..x + 300).contains(&px) && (y..y + 200).contains(&py);
                let want = if inside {
                    expected.pixel(px - x, py - y).unwrap()
                } else {
                    band
                };
                assert_eq!(frame[i..i + 4], want, "{width} x {height} at ({px}, {py})");
            }
        }
    }
}

/// Bounds in pixels (x0, y0, x1, y1) of `stroke` at `scale`, with a margin.
fn pixel_bounds(stroke: &Stroke, scale: f32, margin: f32) -> (f32, f32, f32, f32) {
    let b = ardoise_ink::outline(stroke).bounds().unwrap();
    (
        b.min_x * scale - margin,
        b.min_y * scale - margin,
        b.max_x * scale + margin,
        b.max_y * scale + margin,
    )
}

#[test]
fn grain_strokes_keep_their_grain_while_they_grow() {
    for tool in [Tool::Pencil, Tool::Chalk] {
        let mut stroke = line(tool, 0, (500.0, 2000.0), (5500.0, 2000.0), 0, 2000);
        stroke.width = 150.0;
        for p in &mut stroke.points {
            p.pressure = 0.7;
        }
        let other = line(
            Tool::Ballpoint,
            0,
            (500.0, 3300.0),
            (5500.0, 3300.0),
            2100,
            2500,
        );
        let n = note(vec![stroke.clone(), other], &[true]);
        let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
        let mut frames = FrameRenderer::new(t, &settings(600, 400)).unwrap();
        let scale = frames.placement().scale;
        let final_frame = frames
            .render_frame(frames.frame_count() - 1)
            .unwrap()
            .to_vec();
        // Halfway through the stroke.
        let at = ms(1000);
        let written = frames.timeline().note_at(at);
        let cut = render_note(&written, &opts(scale)).unwrap();
        let frame = frames.render_at(at).unwrap().to_vec();
        // The outline only differs from the complete stroke's after the second last sample
        // shown (the spline's last piece and the end cap).
        let settled = written.strokes[0].points.iter().rev().nth(1).unwrap().x * scale - 8.0;
        let (x0, y0, x1, y1) = pixel_bounds(&stroke, scale, 3.0);
        let (mut kept, mut moved) = (0, 0);
        for py in 0..400u32 {
            for px in 0..600u32 {
                let i = ((py * 600 + px) * 4) as usize;
                let (fx, fy) = (px as f32, py as f32);
                let in_stroke = fx >= x0 && fx <= x1 && fy >= y0 && fy <= y1;
                if !in_stroke {
                    // Away from the growing stroke: exactly the note written so far.
                    assert_eq!(
                        frame[i..i + 4],
                        cut.pixels[i..i + 4],
                        "{tool:?} ({px}, {py})"
                    );
                } else if fx < settled {
                    // What is written of the stroke already has its final grain...
                    assert_eq!(
                        frame[i..i + 4],
                        final_frame[i..i + 4],
                        "{tool:?} ({px}, {py})"
                    );
                    kept += 1;
                    // ...where the cut stroke on its own would show another one.
                    if frame[i..i + 4] != cut.pixels[i..i + 4] {
                        moved += 1;
                    }
                }
            }
        }
        assert!(kept > 1000, "{tool:?}: {kept} pixels compared");
        assert!(
            moved > 100,
            "{tool:?}: only {moved} pixels show another grain"
        );
    }
}

#[test]
fn strokes_that_waited_join_the_cache_without_changing_the_frames() {
    // 40 strokes on the top layer, then one on the bottom layer: they all wait for it outside
    // the cache, and join it a few per frame once it is finished; more strokes follow.
    let mut strokes: Vec<Stroke> = (0..40)
        .map(|i| {
            let y = 200.0 + 90.0 * i as f32;
            let t0 = 100 * i;
            line(
                Tool::Ballpoint,
                1,
                (300.0, y),
                (5700.0, y + 50.0),
                t0,
                t0 + 90,
            )
        })
        .collect();
    strokes.push(line(
        Tool::Highlighter,
        0,
        (3000.0, 100.0),
        (3000.0, 3900.0),
        4000,
        4100,
    ));
    strokes.extend((0..10).map(|i| {
        let x = 400.0 + 500.0 * i as f32;
        let t0 = 4150 + 60 * i;
        line(
            Tool::FeltTip,
            1,
            (x, 300.0),
            (x + 200.0, 3700.0),
            t0,
            t0 + 50,
        )
    }));
    let n = note(strokes, &[true, true]);
    let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
    let mut frames = FrameRenderer::new(
        t,
        &FrameSettings {
            frame_rate: 60,
            ..settings(150, 100)
        },
    )
    .unwrap();
    for k in 0..frames.frame_count() {
        let at = frames.frame_time(k);
        let expected = render_note(&frames.timeline().note_at(at), &opts(0.025)).unwrap();
        let frame = frames.render_frame(k).unwrap();
        assert_same_image(frame, &expected, &format!("frame {k} at {at:?}"));
    }
}

/// Deterministic pseudo-random numbers (64-bit LCG, Knuth's constants).
struct Lcg(u64);

impl Lcg {
    fn below(&mut self, n: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % u64::from(n)) as u32
    }
}

#[test]
fn a_busy_note_replays_exactly() {
    // 60 strokes of every tool, criss-crossing, on three layers (one hidden), unsorted and
    // overlapping in time, some translucent, some single points.
    let mut random = Lcg(7);
    let strokes: Vec<Stroke> = (0..60)
        .map(|i| {
            let tool = Tool::ALL[random.below(7) as usize];
            let from = (random.below(6000) as f32, random.below(4000) as f32);
            let to = (random.below(6000) as f32, random.below(4000) as f32);
            let t0 = random.below(20_000);
            let mut stroke = line(tool, random.below(3), from, to, t0, t0 + random.below(900));
            stroke.color.a = if i % 5 == 0 { 140 } else { 255 };
            if i % 11 == 0 {
                stroke.points.truncate(1);
            }
            stroke
        })
        .collect();
    let n = note(strokes, &[true, true, false]);
    let t = Timeline::new(&n, &options(4.0, Some(400))).unwrap();
    let mut frames = FrameRenderer::new(t, &settings(200, 140)).unwrap();
    let scale = frames.placement().scale;
    let (y, h) = (
        frames.placement().y as usize,
        frames.placement().height as usize,
    );
    let mut compared = 0;
    for k in 0..frames.frame_count() {
        let at = frames.frame_time(k);
        let written = frames.timeline().note_at(at);
        let frame = frames.render_frame(k).unwrap().to_vec();
        // A grain stroke in progress has the grain of the complete stroke (tested above).
        let grain_in_progress = written.strokes.iter().any(|s| {
            matches!(s.tool, Tool::Pencil | Tool::Chalk)
                && n.strokes.iter().any(|full| {
                    full.points.len() > s.points.len() && full.points[..s.points.len()] == s.points
                })
        });
        if grain_in_progress {
            continue;
        }
        let expected = render_note(&written, &opts(scale)).unwrap();
        let page = &frame[y * 200 * 4..(y + h) * 200 * 4];
        assert_same_image(page, &expected, &format!("frame {k} at {at:?}"));
        compared += 1;
    }
    assert!(compared > 40, "{compared} frames compared");
}

#[test]
fn frames_can_be_asked_in_any_order() {
    let n = smooth_tools_note();
    let new = || {
        let t = Timeline::new(&n, &options(2.0, None)).unwrap();
        FrameRenderer::new(t, &settings(150, 100)).unwrap()
    };
    let mut forward = new();
    let in_order: Vec<Vec<u8>> = (0..forward.frame_count())
        .map(|k| forward.render_frame(k).unwrap().to_vec())
        .collect();
    let mut jumping = new();
    let last = in_order.len() as u64 - 1;
    for k in [last, 3, 40, 41, 0, 7, 7, last, 20] {
        assert_eq!(
            jumping.render_frame(k).unwrap(),
            in_order[k as usize],
            "frame {k}"
        );
    }
}

#[test]
fn transparent_and_white_frames() {
    let n = smooth_tools_note();
    for (backdrop, band) in [(Backdrop::Transparent, [0; 4]), (Backdrop::White, [255; 4])] {
        let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
        let settings = FrameSettings {
            backdrop,
            ..settings(160, 80)
        };
        let mut frames = FrameRenderer::new(t, &settings).unwrap();
        let place = frames.placement();
        assert_eq!((place.x, place.width, place.height), (20, 120, 80));
        let expected = match backdrop {
            Backdrop::Transparent => render_note_transparent(&n, place.scale),
            _ => render_note(
                &n,
                &RenderOptions {
                    scale: place.scale,
                    background: false,
                },
            ),
        }
        .unwrap();
        let last = frames.frame_count() - 1;
        let frame = frames.render_frame(last).unwrap();
        for row in 0..80usize {
            let line = &frame[row * 160 * 4..(row + 1) * 160 * 4];
            assert_eq!(
                line[20 * 4..140 * 4],
                expected.pixels[row * 120 * 4..(row + 1) * 120 * 4],
                "{backdrop:?} row {row}"
            );
            assert!(line[..20 * 4].chunks(4).all(|p| p == band), "{backdrop:?}");
            assert!(line[140 * 4..].chunks(4).all(|p| p == band), "{backdrop:?}");
        }
    }
}

#[test]
fn bad_frame_settings_are_refused() {
    let n = three_strokes();
    let new = |settings: FrameSettings| {
        let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
        FrameRenderer::new(t, &settings).map(|_| ())
    };
    for (width, height, frame_rate) in
        [(0, 100, 30), (100, 0, 30), (20_000, 100, 30), (100, 100, 0)]
    {
        let s = FrameSettings {
            frame_rate,
            ..settings(width, height)
        };
        assert!(
            matches!(new(s), Err(MediaError::InvalidSettings(_))),
            "{width} x {height} at {frame_rate}"
        );
    }
    // One pixel is a frame; an empty page is not drawable.
    assert!(new(settings(1, 1)).is_ok());
    let flat = Note {
        page: PageSize {
            width: 0,
            height: 4000,
        },
        ..three_strokes()
    };
    let t = Timeline::new(&flat, &ReplayOptions::default()).unwrap();
    assert!(matches!(
        FrameRenderer::new(t, &settings(100, 100)),
        Err(MediaError::Render(_))
    ));
    let video = crate::VideoSettings {
        width: 1920,
        height: 1080,
        frame_rate: 30,
        audio_channels: Some(1),
    };
    assert_eq!(
        FrameSettings::for_video(&video),
        FrameSettings {
            width: 1920,
            height: 1080,
            frame_rate: 30,
            backdrop: Backdrop::Paper,
        }
    );
}

// ---------------------------------------------------------------------------------------
// Text frames
// ---------------------------------------------------------------------------------------

const RED: Rgba = Rgba::rgb(220, 20, 20);

/// A bold Andika text frame shown at `t` ms.
fn text_at(layer: u32, t: u32, (x, y): (f32, f32), text: &str, color: Rgba) -> PageObject {
    PageObject::text(
        format!("{text}-{t}"),
        layer,
        t,
        x,
        y,
        TextBody {
            text: text.into(),
            font: FontFamily::Andika,
            bold: true,
            size: 900.0,
            color,
        },
    )
}

/// Pixels of a frame of the red of these tests' text frames (not the Seyès margin's pink).
fn reds(frame: &[u8]) -> usize {
    frame
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] > 150 && p[1] < 60 && p[2] < 60)
        .count()
}

#[test]
fn text_frames_appear_whole_at_their_time_in_drawing_order() {
    let strokes = vec![
        line(Tool::Ballpoint, 0, (300.0, 600.0), (5700.0, 700.0), 0, 1000),
        // Same layer, written after the red frame appears, across it: drawn under it.
        line(
            Tool::FeltTip,
            0,
            (300.0, 1800.0),
            (5700.0, 2000.0),
            2000,
            3000,
        ),
        // The layer above, across the red frame: over it.
        line(
            Tool::Highlighter,
            1,
            (1500.0, 300.0),
            (1700.0, 3700.0),
            1200,
            1800,
        ),
    ];
    let mut unknown = text_at(0, 9500, (800.0, 3000.0), "Inconnu", RED);
    unknown.body = ObjectBody::Unknown {
        kind: "equation".into(),
        fields: Default::default(),
    };
    let mut n = note(strokes, &[true, true, false]);
    n.objects = vec![
        text_at(0, 1500, (800.0, 1300.0), "Texte", RED),
        text_at(1, 2500, (3200.0, 2600.0), "Dessus", Rgba::rgb(20, 140, 20)),
        // A hidden layer and a kind this version does not draw: never drawn, never waited for.
        text_at(2, 9000, (800.0, 3000.0), "Caché", RED),
        unknown,
    ];
    let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
    assert_eq!(t.duration(), ms(3000));
    assert_eq!(
        t.items(),
        [
            DrawItem::Stroke(0),
            DrawItem::Stroke(1),
            DrawItem::Object(0),
            DrawItem::Stroke(2),
            DrawItem::Object(1)
        ]
    );
    assert_eq!(t.drawing_order(), [0, 1, 2]);
    assert!(!t.object_shown(0, 1499.0) && t.object_shown(0, 1500.0));
    assert!(!t.object_shown(2, 1e9) && !t.object_shown(3, 1e9) && !t.object_shown(9, 1e9));
    assert_eq!(t.item_shown(DrawItem::Object(1), 2500.0), 1);
    assert!(t.note_at(ms(1499)).objects.is_empty());
    assert_eq!(t.note_at(ms(1500)).objects, n.objects[..1]);
    assert_eq!(t.note_at(t.duration()).objects, n.objects[..2]);

    let mut frames = FrameRenderer::new(t, &settings(240, 160)).unwrap();
    let mut in_order = Vec::new();
    for k in 0..frames.frame_count() {
        let at = frames.frame_time(k);
        let expected = render_note(&frames.timeline().note_at(at), &opts(0.04)).unwrap();
        let frame = frames.render_frame(k).unwrap();
        assert_same_image(frame, &expected, &format!("frame {k} at {at:?}"));
        if at < ms(1500) {
            assert_eq!(reds(frame), 0, "frame {k} at {at:?}");
        } else {
            assert!(reds(frame) > 50, "frame {k} at {at:?}");
        }
        in_order.push(frame.to_vec());
    }
    let full = render_note(&n, &opts(0.04)).unwrap();
    assert_same_image(in_order.last().unwrap(), &full, "last frame");
    // Going back and forth gives the same frames.
    let last = in_order.len() as u64 - 1;
    for k in [last, 36, 35, 60, 2, 47, last] {
        assert_eq!(
            frames.render_frame(k).unwrap(),
            in_order[k as usize],
            "frame {k}"
        );
    }
}

#[test]
fn a_note_ending_with_a_text_frame_replays_until_it_appears() {
    let mut n = note(
        vec![line(
            Tool::Ballpoint,
            0,
            (300.0, 600.0),
            (5700.0, 700.0),
            0,
            1000,
        )],
        &[true],
    );
    n.objects = vec![text_at(0, 4000, (800.0, 1500.0), "Fin", RED)];
    let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
    assert_eq!((t.duration(), t.note_end()), (ms(4000), 4000.0));
    assert!(t.note_at(ms(3999)).objects.is_empty());
    assert_eq!(t.note_at(ms(4000)).objects, n.objects);
    // The pause before it is a pause like any other; a target duration counts it.
    let cut = Timeline::new(&n, &options(1.0, Some(500))).unwrap();
    assert_eq!(cut.duration(), ms(1500));
    assert_eq!(cut.to_replay(4000.0), ms(1500));
    assert_eq!(ReplayOptions::for_duration(&n, None, ms(2000)).speed, 2.0);
    // The last frame holds it, the one before does not.
    let mut frames = FrameRenderer::new(cut, &settings(150, 100)).unwrap();
    let last = frames.frame_count() - 1;
    assert_eq!(frames.frame_time(last), ms(1500));
    let before = frames.render_frame(last - 1).unwrap().to_vec();
    assert_eq!(reds(&before), 0);
    let full = render_note(&n, &opts(0.025)).unwrap();
    let frame = frames.render_frame(last).unwrap();
    assert_same_image(frame, &full, "last frame");
    assert!(reds(frame) > 20);

    // A page holding only a text frame: a replay of its time; at time zero, one frame.
    for (time, duration, count) in [(700, ms(700), 18), (0, Duration::ZERO, 1)] {
        let mut only = note(vec![], &[true]);
        only.objects = vec![text_at(0, time, (800.0, 1500.0), "Seul", RED)];
        let t = Timeline::new(&only, &options(1.0, None)).unwrap();
        assert_eq!((t.duration(), t.frame_count(24)), (duration, count));
        let mut frames = FrameRenderer::new(t, &settings(150, 100)).unwrap();
        let frame = frames.render_frame(count - 1).unwrap();
        assert_same_image(frame, &render_note(&only, &opts(0.025)).unwrap(), "only");
        assert!(reds(frame) > 20);
    }
}

#[test]
fn a_busy_note_with_text_frames_replays_exactly() {
    // 40 strokes and 12 text frames (turned, scaled, translucent), criss-crossing on three
    // layers (one hidden), unsorted and overlapping in time.
    let smooth = [
        Tool::Ballpoint,
        Tool::FountainPen,
        Tool::FeltTip,
        Tool::Brush,
        Tool::Highlighter,
    ];
    let mut random = Lcg(11);
    let strokes: Vec<Stroke> = (0..40)
        .map(|_| {
            let tool = smooth[random.below(5) as usize];
            let from = (random.below(6000) as f32, random.below(4000) as f32);
            let to = (random.below(6000) as f32, random.below(4000) as f32);
            let t0 = random.below(12_000);
            line(tool, random.below(3), from, to, t0, t0 + random.below(900))
        })
        .collect();
    let objects: Vec<PageObject> = (0..12)
        .map(|i| {
            let color = Rgba {
                a: if i % 4 == 0 { 120 } else { 255 },
                ..Rgba::rgb(
                    random.below(200) as u8,
                    random.below(200) as u8,
                    random.below(200) as u8,
                )
            };
            let mut object = text_at(
                random.below(3),
                random.below(12_500),
                (random.below(5000) as f32, random.below(3500) as f32),
                ["Mot", "Deux mots", "Ligne\nautre"][i % 3],
                color,
            );
            object.quarter_turns = (random.below(4)) as u8;
            object.scale = 0.5 + random.below(100) as f32 / 100.0;
            object
        })
        .collect();
    let mut n = note(strokes, &[true, true, false]);
    n.objects = objects;
    let t = Timeline::new(&n, &options(4.0, Some(400))).unwrap();
    assert!(t.items().iter().any(|i| matches!(i, DrawItem::Object(_))));
    let mut frames = FrameRenderer::new(t, &settings(200, 140)).unwrap();
    let scale = frames.placement().scale;
    let (y, h) = (
        frames.placement().y as usize,
        frames.placement().height as usize,
    );
    let mut with_text = 0;
    for k in 0..frames.frame_count() {
        let at = frames.frame_time(k);
        let written = frames.timeline().note_at(at);
        with_text += usize::from(!written.objects.is_empty());
        let expected = render_note(&written, &opts(scale)).unwrap();
        let frame = frames.render_frame(k).unwrap();
        let page = &frame[y * 200 * 4..(y + h) * 200 * 4];
        assert_same_image(page, &expected, &format!("frame {k} at {at:?}"));
    }
    assert!(with_text > 20, "{with_text} frames with text");
}

// ---------------------------------------------------------------------------------------
// Performance (SPEC §8)
// ---------------------------------------------------------------------------------------

/// A page of handwriting written in 2 minutes: 15 lines of 20 words, one stroke per word,
/// 67 samples 5 ms apart, a new word every 400 ms. `tools`: each line with another tool;
/// `layers`: every other line on the top layer, so that finished strokes wait outside the
/// cache (the drawing order is not the order of time).
fn handwriting(tools: bool, layers: bool) -> Note {
    use std::f32::consts::TAU;
    let mut strokes = Vec::with_capacity(300);
    for line in 0..15usize {
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
        for w in 0..20usize {
            let letters = 3 + (line + w) % 3;
            let start = ((line * 20 + w) * 400) as u32;
            let points = (0..67)
                .map(|i| {
                    let u = i as f32 / 66.0;
                    let a = u * letters as f32 * TAU;
                    let tall = (line * 31 + w + (a / TAU) as usize).is_multiple_of(3);
                    let height = if tall { 500.0 } else { 275.0 };
                    let advance = 250.0 / TAU;
                    Point {
                        x: x + advance * a + 1.6 * advance * a.sin(),
                        y: y - height * 0.5 * (1.0 - a.cos()),
                        pressure: 0.5 + 0.4 * (a * 0.37).sin(),
                        t: start + i * 5,
                    }
                })
                .collect();
            strokes.push(Stroke {
                tool,
                color: Rgba::rgb(20, 40, 120),
                width,
                layer: u32::from(layers && line % 2 == 1),
                points,
            });
            x += letters as f32 * 250.0 + 150.0;
        }
    }
    Note {
        page: A4_LANDSCAPE,
        strokes,
        background: if tools {
            Background {
                paper: Paper::Cream,
                template: Template::Seyes,
                texture: true,
            }
        } else {
            Background::default()
        },
        ..note(vec![], &[true, true])
    }
}

/// Run with `cargo test --release -p ardoise-media replay_performance -- --ignored
/// --nocapture`: milliseconds per frame of a 2-minute replay at 30 fps, 1080p (and 4K once).
#[test]
#[ignore = "timing; run in release"]
fn replay_performance() {
    for (name, n, width, height) in [
        (
            "ballpoint, white page",
            handwriting(false, false),
            1920,
            1080,
        ),
        (
            "all tools, Seyès + texture",
            handwriting(true, false),
            1920,
            1080,
        ),
        (
            "all tools, two layers out of time order",
            handwriting(true, true),
            1920,
            1080,
        ),
        (
            "all tools, two layers out of time order",
            handwriting(true, true),
            3840,
            2160,
        ),
    ] {
        let video = FrameSettings {
            width,
            height,
            frame_rate: 30,
            backdrop: Backdrop::Paper,
        };
        let started = Instant::now();
        let t = Timeline::new(&n, &ReplayOptions::default()).unwrap();
        let mut frames = FrameRenderer::new(t, &video).unwrap();
        let setup = started.elapsed();
        let count = frames.frame_count();
        let mut times: Vec<f64> = Vec::with_capacity(count as usize);
        let started = Instant::now();
        for k in 0..count {
            let frame_start = Instant::now();
            let frame = frames.render_frame(k).unwrap();
            std::hint::black_box(frame);
            times.push(frame_start.elapsed().as_secs_f64() * 1000.0);
        }
        let total = started.elapsed();
        let last = frames.render_frame(count - 1).unwrap().to_vec();
        times.sort_by(f64::total_cmp);
        let quantile = |q: f64| times[((times.len() - 1) as f64 * q).round() as usize];
        let place = frames.placement();
        println!(
            "{name}: {} strokes, {count} frames of {:.2} s at {width} x {height} (page {} x {}): \
             setup {:.0} ms, median {:.2} ms, 95 % {:.2} ms, worst {:.2} ms, total {:.2} s",
            n.strokes.len(),
            frames.timeline().duration().as_secs_f64(),
            place.width,
            place.height,
            setup.as_secs_f64() * 1000.0,
            quantile(0.5),
            quantile(0.95),
            quantile(1.0),
            total.as_secs_f64(),
        );
        // The last frame is still the exported image.
        let expected = render_note(&n, &opts(place.scale)).unwrap();
        let row_bytes = place.width as usize * 4;
        for row in 0..place.height as usize {
            let start = ((row + place.y as usize) * width as usize + place.x as usize) * 4;
            assert!(
                last[start..start + row_bytes]
                    == expected.pixels[row * row_bytes..(row + 1) * row_bytes],
                "{name}: row {row} of the last frame"
            );
        }
    }
}

#[test]
fn every_frame_shows_the_background_image_under_the_strokes() {
    use ardoise_core::note::{BackgroundImage, ImageRect};
    use ardoise_render::{NoteView, Picture, encode_png};

    let pixels = Image {
        width: 8,
        height: 4,
        pixels: (0..32u32)
            .flat_map(|i| [200, (i * 7) as u8, 40, 255])
            .collect(),
    };
    let picture = Picture::decode(encode_png(&pixels).unwrap()).unwrap();
    let mut n = three_strokes();
    n.background_image = Some(BackgroundImage {
        name: "fiche.png".into(),
        pdf_page: None,
        rect: ImageRect {
            x: 1000.0,
            y: -500.0,
            width: 3000.0,
            height: 3000.0,
        },
        quarter_turns: 3,
        opacity: 0.75,
    });
    let view = NoteView::new(&n, Some(&picture));
    let t = Timeline::new(&n, &options(4.0, None)).unwrap();
    let mut frames = FrameRenderer::with_picture(t, &settings(300, 200), Some(&picture)).unwrap();
    let scale = frames.placement().scale;
    // The first frame: the page with its image and the first sample; the last: everything.
    let begun = frames.timeline().note_at(frames.frame_time(0));
    let first = frames.render_frame(0).unwrap().to_vec();
    let expected = render_note(NoteView::new(&begun, Some(&picture)), &opts(scale)).unwrap();
    assert_eq!(first, expected.pixels);
    let last = frames.frame_count() - 1;
    let frame = frames.render_frame(last).unwrap();
    assert_eq!(
        frame,
        render_note(view, &opts(scale)).unwrap().pixels.as_slice()
    );
    // Without the pixels, the frames are those of the plain page.
    let t = Timeline::new(&n, &options(4.0, None)).unwrap();
    let mut plain = FrameRenderer::new(t, &settings(300, 200)).unwrap();
    assert_ne!(plain.render_frame(0).unwrap(), first.as_slice());
}
