//! The time orders of `ardoise-revise` against the real replay (`docs/design/revision.md`, "Time"):
//! not the numbers in the strokes, but what `replay::Timeline` shows, and when. The test lives here
//! because `ardoise-media` needs `nasm` to build, which `ardoise-revise` must not.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::time::Duration;

use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Note, Origin, Paper, Template, Timing};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_media::replay::{ReplayOptions, Timeline};
use ardoise_revise::{Input, Order, Revised, Select, apply};

/// A stroke in device units from `x` to `x + 500`, from `t0` to `t1` ms.
fn stroke(x: f32, t0: u32, t1: u32) -> Stroke {
    let n = 21u32;
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 50.0,
        layer: 0,
        points: (0..n)
            .map(|i| Point {
                x: x + 500.0 * i as f32 / (n - 1) as f32,
                y: 2000.0,
                pressure: 0.5,
                t: t0 + (t1 - t0) * i / (n - 1),
            })
            .collect(),
    }
}

fn note(strokes: Vec<Stroke>) -> Note {
    Note {
        id: "00000000-0000-4000-8000-000000000000".into(),
        title: "revise".into(),
        created: 1_700_000_000_000,
        modified: 1_700_000_000_000,
        page: A4_LANDSCAPE,
        background: Background {
            paper: Paper::White,
            template: Template::Blank,
            texture: false,
        },
        background_image: None,
        tags: vec![],
        favorite: false,
        layers: vec![Layer {
            name: "1".into(),
            visible: true,
        }],
        strokes,
        timing: Timing::Recorded,
        origin: Origin::Tablet,
    }
}

/// Strokes 0, 1 and 2 of a sentence, with `gap` ms between 0 and 1, and a letter (stroke 3) written
/// long after, 100 ms long, to copy.
fn sentence(gap: u32) -> Note {
    let next = 500 + gap;
    note(vec![
        stroke(1000.0, 0, 500),
        stroke(2000.0, next, next + 500),
        stroke(3000.0, next + 1000, next + 1500),
        stroke(4000.0, next + 5000, next + 5100),
    ])
}

fn ids(list: &[usize]) -> Select {
    Select {
        ids: Some(list.to_vec()),
        ..Select::default()
    }
}

fn copy_of(number: usize, after: usize) -> Order {
    Order::Copy {
        select: ids(&[number]),
        to_x: Some(16.0),
        dx: None,
        dy: None,
        factor: None,
        color: None,
        after: Some(after),
    }
}

fn revise(source: &Note, narration: bool, orders: &[Order]) -> (Note, Revised) {
    let input = Input {
        strokes: &source.strokes,
        page_mm: [297.0, 216.0],
        layers: 1,
        visible_layers: &[],
        narration,
    };
    let revised = apply(&input, orders).unwrap();
    let mut out = source.clone();
    out.strokes.clone_from(&revised.strokes);
    (out, revised)
}

fn timeline(note: &Note) -> Timeline<'_> {
    Timeline::new(note, &ReplayOptions::default()).unwrap()
}

/// The strokes' numbers, in the order the replay starts them; each with the instants it starts and
/// is finished.
fn appearances(
    note: &Note,
    revised: &Revised,
    options: &ReplayOptions,
) -> Vec<(usize, Duration, Duration)> {
    let line = Timeline::new(note, options).unwrap();
    let mut out: Vec<(usize, Duration, Duration)> = revised
        .numbers
        .iter()
        .enumerate()
        .map(|(index, &number)| {
            let points = &note.strokes[index].points;
            let start = line.to_replay(f64::from(points[0].t));
            let end = line.to_replay(f64::from(points[points.len() - 1].t));
            (number, start, end)
        })
        .collect();
    out.sort_by_key(|entry| entry.1);
    out
}

/// True when the replay shows the strokes one after the other, in this order, none overlapping.
#[track_caller]
fn assert_written_in_order(note: &Note, revised: &Revised, order: &[usize]) {
    let seen = appearances(note, revised, &ReplayOptions::default());
    let numbers: Vec<usize> = seen.iter().map(|entry| entry.0).collect();
    assert_eq!(numbers, order);
    for pair in seen.windows(2) {
        assert!(
            pair[0].2 < pair[1].1,
            "stroke {} is not finished when stroke {} starts",
            pair[0].0,
            pair[1].0
        );
    }
}

fn index_of(revised: &Revised, number: usize) -> usize {
    revised.numbers.iter().position(|&n| n == number).unwrap()
}

/// Walks through the replay: what it shows of each stroke only grows, a stroke in progress is cut
/// after its last sample shown, and at the end everything is there.
#[track_caller]
fn assert_replay_is_sound(note: &Note) {
    let line = timeline(note);
    let mut shown = vec![0usize; note.strokes.len()];
    let steps = 400u32;
    for step in 0..=steps {
        let at = line.duration().mul_f64(f64::from(step) / f64::from(steps));
        let tau = line.to_note(at);
        for (index, slot) in shown.iter_mut().enumerate() {
            let count = line.samples_shown(index, tau);
            assert!(count >= *slot, "a stroke lost samples at {at:?}");
            *slot = count;
        }
        assert_eq!(
            line.note_at(at).strokes.len(),
            shown.iter().filter(|&&count| count > 0).count()
        );
    }
    let last = line.note_at(line.duration());
    assert_eq!(
        last.strokes, note.strokes,
        "the last frame is the whole note"
    );
}

#[test]
fn a_letter_that_fits_is_written_in_its_place_and_nothing_else_moves() {
    let source = sentence(1000);
    let (out, revised) = revise(&source, false, &[copy_of(3, 0)]);
    assert_written_in_order(&out, &revised, &[0, 4, 1, 2, 3]);
    assert_eq!(timeline(&out).duration(), timeline(&source).duration());
    for number in 0..4 {
        assert_eq!(
            out.strokes[index_of(&revised, number)],
            source.strokes[number]
        );
    }
    let line = timeline(&out);
    // When the stroke before it is finished it has not begun, and it is finished before what
    // follows begins.
    let copy = index_of(&revised, 4);
    assert_eq!(line.samples_shown(copy, 500.0), 0);
    assert!(line.is_complete(copy, f64::from(source.strokes[1].points[0].t) - 1.0));
    assert_replay_is_sound(&out);
}

#[test]
fn a_letter_that_does_not_fit_pushes_what_follows_in_the_replay() {
    // 40 + 100 + 40 ms are needed, 100 are there: the replay is 80 ms longer.
    let source = sentence(100);
    let (out, revised) = revise(&source, false, &[copy_of(3, 0)]);
    assert_written_in_order(&out, &revised, &[0, 4, 1, 2, 3]);
    assert_eq!(
        timeline(&out).duration(),
        timeline(&source).duration() + Duration::from_millis(80)
    );
    assert_eq!(revised.report.pushed_ms, 80);
    let before = appearances(
        &source,
        &Revised {
            numbers: (0..4).collect(),
            ..revised.clone()
        },
        &ReplayOptions::default(),
    );
    let after = appearances(&out, &revised, &ReplayOptions::default());
    let at = |list: &[(usize, Duration, Duration)], number: usize| {
        list.iter().find(|entry| entry.0 == number).unwrap().1
    };
    assert_eq!(at(&after, 0), at(&before, 0));
    for number in 1..4 {
        assert_eq!(
            at(&after, number),
            at(&before, number) + Duration::from_millis(80)
        );
    }
    assert_replay_is_sound(&out);
}

#[test]
fn with_a_narration_the_replay_keeps_its_clock_and_the_letter_is_faster() {
    let source = sentence(90);
    let (out, revised) = revise(&source, true, &[copy_of(3, 0)]);
    assert_written_in_order(&out, &revised, &[0, 4, 1, 2, 3]);
    assert_eq!(timeline(&out).duration(), timeline(&source).duration());
    assert_eq!(revised.report.pushed_ms, 0);
    assert_eq!(revised.report.strokes_faster, 1);
    let line = timeline(&out);
    let before = timeline(&source);
    for number in 0..4 {
        // Each stroke shows the same samples at the same instant as in the source.
        let (kept, was) = (index_of(&revised, number), number);
        for step in 0..100u32 {
            let at = line.duration().mul_f64(f64::from(step) / 100.0);
            assert_eq!(
                line.samples_shown(kept, line.to_note(at)),
                before.samples_shown(was, before.to_note(at))
            );
        }
    }
    // Faster than it was written.
    let copy = &out.strokes[index_of(&revised, 4)];
    let taken = copy.points[copy.points.len() - 1].t - copy.points[0].t;
    assert!(taken < 100, "{taken} ms");
    assert!(copy.points.windows(2).all(|pair| pair[0].t <= pair[1].t));
    assert_replay_is_sound(&out);
}

#[test]
fn a_word_retimed_into_its_sentence_comes_there_and_leaves_a_pause() {
    // Strokes 3 comes long after: moved after stroke 0, its old place is a pause.
    let source = sentence(100);
    let order = Order::Retime {
        select: ids(&[3]),
        after: 0,
    };
    let (out, revised) = revise(&source, false, &[order]);
    assert_written_in_order(&out, &revised, &[0, 3, 1, 2]);
    assert_replay_is_sound(&out);

    // Trimming the pauses of the replay takes the old place out, as the replay can.
    let whole = timeline(&out).duration();
    let trimmed = Timeline::new(
        &out,
        &ReplayOptions {
            speed: 1.0,
            max_pause: Some(Duration::from_millis(60)),
        },
    )
    .unwrap()
    .duration();
    assert!(trimmed < whole, "{trimmed:?} {whole:?}");
}

#[test]
fn new_strokes_without_after_end_the_replay() {
    let source = sentence(100);
    let order = Order::Copy {
        select: ids(&[0]),
        to_x: None,
        dx: None,
        dy: Some(10.0),
        factor: None,
        color: None,
        after: None,
    };
    let (out, revised) = revise(&source, true, &[order]);
    assert_written_in_order(&out, &revised, &[0, 1, 2, 3, 4]);
    assert_eq!(
        timeline(&out).duration(),
        timeline(&source).duration() + Duration::from_millis(540)
    );
}

#[test]
fn a_redrawn_stroke_takes_the_same_time_in_the_replay() {
    let source = sentence(100);
    let order = Order::Replace {
        select: ids(&[1]),
        with: vec![ardoise_revise::Shape::Line {
            from: [20.0, 20.0],
            to: [40.0, 20.0],
        }],
        color: None,
        width: None,
    };
    let (out, revised) = revise(&source, false, &[order]);
    assert_written_in_order(&out, &revised, &[0, 4, 2, 3]);
    assert_eq!(timeline(&out).duration(), timeline(&source).duration());
    let line = timeline(&out);
    let redrawn = index_of(&revised, 4);
    assert_eq!(line.samples_shown(redrawn, 599.0), 0);
    assert!(line.is_complete(redrawn, 1100.0));
}

// -------------------------------------------------------------------------------------------------
// A letter taken out of a word written in one stroke

/// Strokes 0, 1 and 2 of a sentence, with `gap` ms between 0 and 1, and a word written in one stroke
/// long after (stroke 3): 2 s, a sample every 100 ms.
fn sentence_and_a_word(gap: u32) -> Note {
    let next = 500 + gap;
    note(vec![
        stroke(1000.0, 0, 500),
        stroke(2000.0, next, next + 500),
        stroke(3000.0, next + 1000, next + 1500),
        stroke(4000.0, next + 5000, next + 7000),
    ])
}

/// Samples 5 and 6 of the word, 100 ms, as a letter right after stroke 0.
fn letter_of_the_word() -> Order {
    Order::Glyph {
        parts: vec![ardoise_revise::Part {
            stroke: 3,
            from: Some(5),
            to: Some(6),
            flip: None,
            sx: None,
            sy: None,
            x: 16.0,
            y: 20.0,
        }],
        color: None,
        after: Some(0),
    }
}

#[test]
fn a_letter_out_of_a_word_pushes_what_follows_by_what_it_needs_only() {
    // 40 + 100 + 40 ms are needed, 100 are there. Placed for the whole word, it would be 2 s.
    let source = sentence_and_a_word(100);
    let (out, revised) = revise(&source, false, &[letter_of_the_word()]);
    assert_written_in_order(&out, &revised, &[0, 4, 1, 2, 3]);
    assert_eq!(revised.report.pushed_ms, 80);
    assert_eq!(
        timeline(&out).duration(),
        timeline(&source).duration() + Duration::from_millis(80)
    );
    // No hole: it starts a pause after the stroke before it, and takes its own time.
    let letter = &out.strokes[index_of(&revised, 4)];
    assert_eq!(letter.points.len(), 2);
    assert_eq!(letter.points[0].t, 540);
    assert_eq!(letter.points[1].t, 640);
    assert_replay_is_sound(&out);
}

#[test]
fn with_a_narration_a_letter_out_of_a_word_is_drawn_at_its_own_pace() {
    // 88 ms are there for the 180 it needs. Placed for the whole word, it would get about 4 ms.
    let source = sentence_and_a_word(90);
    let (out, revised) = revise(&source, true, &[letter_of_the_word()]);
    assert_written_in_order(&out, &revised, &[0, 4, 1, 2, 3]);
    assert_eq!(timeline(&out).duration(), timeline(&source).duration());
    assert_eq!(revised.report.pushed_ms, 0);
    assert_eq!(revised.report.strokes_faster, 1);
    let letter = &out.strokes[index_of(&revised, 4)];
    let taken = letter.points[1].t - letter.points[0].t;
    assert!((40..100).contains(&taken), "{taken} ms");
    // It starts right after a pause at that pace, and everything else is where it was.
    assert!(letter.points[0].t - 500 < 40);
    for number in 0..4 {
        assert_eq!(
            out.strokes[index_of(&revised, number)],
            source.strokes[number]
        );
    }
    assert_replay_is_sound(&out);
}

#[test]
fn a_copy_placed_in_time_cannot_be_trimmed_afterwards() {
    let source = sentence_and_a_word(100);
    for narration in [false, true] {
        let input = Input {
            strokes: &source.strokes,
            page_mm: [297.0, 216.0],
            layers: 1,
            visible_layers: &[],
            narration,
        };
        let orders = [
            copy_of(3, 0),
            Order::Trim {
                stroke: 4,
                from: 5,
                to: Some(6),
            },
        ];
        let message = apply(&input, &orders).unwrap_err().to_string();
        assert!(message.contains("`glyph`"), "{message}");
    }
}

#[test]
fn a_stroke_cut_before_it_is_placed_takes_its_own_time() {
    let source = sentence_and_a_word(100);
    let orders = [
        Order::Trim {
            stroke: 3,
            from: 5,
            to: Some(6),
        },
        Order::Retime {
            select: ids(&[3]),
            after: 0,
        },
    ];
    let (out, revised) = revise(&source, false, &orders);
    assert_written_in_order(&out, &revised, &[0, 3, 1, 2]);
    assert_eq!(revised.report.pushed_ms, 80);
    let cut = &out.strokes[index_of(&revised, 3)];
    assert_eq!((cut.points[0].t, cut.points[1].t), (540, 640));
    assert_replay_is_sound(&out);

    // With a narration, nothing else moves.
    let (out, revised) = revise(&source, true, &orders);
    assert_written_in_order(&out, &revised, &[0, 3, 1, 2]);
    assert_eq!(revised.report.pushed_ms, 0);
    assert_replay_is_sound(&out);
}

#[test]
fn a_stroke_cut_where_it_is_stays_in_its_own_time() {
    let source = sentence(100);
    let order = Order::Trim {
        stroke: 1,
        from: 5,
        to: Some(15),
    };
    let (out, revised) = revise(&source, true, &[order]);
    assert_written_in_order(&out, &revised, &[0, 1, 2, 3]);
    assert_eq!(timeline(&out).duration(), timeline(&source).duration());
    for number in [0, 2, 3] {
        assert_eq!(
            out.strokes[index_of(&revised, number)],
            source.strokes[number]
        );
    }
    assert_replay_is_sound(&out);
}
