//! `tidy_line` on synthetic writing.

use super::*;

/// A little wave `w` wide and `h` tall sitting on the line through `base` at `x`, `slope` being
/// dy/dx, at this time.
pub(in crate::tests) fn letter(x: f32, w: f32, base: f32, slope: f32, h: f32, t: u32) -> Stroke {
    let n = 12;
    let points = (0..n)
        .map(|i| {
            let u = i as f32 / (n - 1) as f32;
            let px = x + w * u;
            let wave = 0.5 - 0.5 * (u * std::f32::consts::TAU * 1.5).cos();
            pt(px, base + slope * px - h * wave, t + i as u32 * 5)
        })
        .collect();
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 50.0,
        layer: 0,
        points,
    }
}

/// `words` words of `letters` letters each, from `x`: letters 2.5 mm wide with 1 mm between them,
/// words 6 mm apart.
pub(in crate::tests) fn writing(
    x: f32,
    base: f32,
    slope: f32,
    words: usize,
    letters: usize,
) -> Vec<Stroke> {
    let mut out = Vec::new();
    let mut cursor = x;
    let mut t = 0;
    for _ in 0..words {
        for _ in 0..letters {
            out.push(letter(cursor, 2.5, base, slope, 3.0, t));
            cursor += 3.5;
            t += 100;
        }
        cursor += 5.0;
    }
    out
}

/// The lowest y of each stroke of a line, millimetres.
fn bottoms(revised: &Revised, numbers: std::ops::Range<usize>) -> Vec<f32> {
    numbers.map(|n| box_of(revised.stroke(n))[3]).collect()
}

const TILT: f32 = 0.05; // About 2.9 degrees.

#[test]
fn a_tilted_line_comes_out_level() {
    let strokes = writing(20.0, 40.0, TILT, 8, 4);
    let last = strokes.len();
    let revised = run(&strokes, r#"[{"op":"tidy_line","select":{}}]"#);
    let report = revised.report.orders[0].line.as_ref().unwrap();
    assert_eq!(report.strokes, last);
    assert_eq!(report.words, 8);
    assert_near(report.slope, TILT.atan().to_degrees(), 0.2);
    assert_eq!(report.slope_from, SlopeFrom::Fitted);
    assert_near(report.body, 3.0, 0.5);
    // Sits where it did at its left end.
    assert_near(report.baseline, 40.0 + TILT * 20.0, 0.3);
    assert_near(report.left, 20.0, 0.01);
    for bottom in bottoms(&revised, 0..last) {
        assert_near(bottom, report.baseline, 0.3);
    }
}

#[test]
fn baseline_and_left_say_where_it_goes() {
    let strokes = writing(20.0, 40.0, TILT, 4, 4);
    let revised = run(
        &strokes,
        r#"[{"op":"tidy_line","select":{},"baseline":60,"left":100}]"#,
    );
    assert_near(box_of(revised.stroke(0))[0], 100.0, 0.01);
    for bottom in bottoms(&revised, 0..strokes.len()) {
        assert_near(bottom, 60.0, 0.3);
    }
}

#[test]
fn letters_keep_their_place_in_a_word() {
    let strokes = writing(20.0, 40.0, 0.0, 2, 4);
    let revised = run(&strokes, r#"[{"op":"tidy_line","select":{},"left":50}]"#);
    // Across, the first word only moves as a whole.
    for (n, stroke) in strokes.iter().enumerate().take(4) {
        let (before, after) = (box_of(stroke), box_of(revised.stroke(n)));
        assert_near(after[0] - before[0], 30.0, 0.01);
        assert_near(after[2] - before[2], 30.0, 0.01);
    }
}

#[test]
fn gaps_even_brings_them_closer_to_the_usual_one_and_keep_does_not() {
    // Five words 6 mm apart (the blank), one 26 mm further.
    let mut strokes = writing(20.0, 40.0, 0.0, 5, 3);
    let wide = strokes.len() - 3;
    for stroke in &mut strokes[wide..] {
        for p in &mut stroke.points {
            p.x += 2000.0;
        }
    }
    let gap =
        |revised: &Revised| box_of(revised.stroke(wide))[0] - box_of(revised.stroke(wide - 1))[2];

    let kept = run(
        &strokes,
        r#"[{"op":"tidy_line","select":{},"gaps":"keep"}]"#,
    );
    assert_near(gap(&kept), 26.0, 0.01);

    let even = run(
        &strokes,
        r#"[{"op":"tidy_line","select":{},"gaps":"even"}]"#,
    );
    // Clamped at 1.4 times the usual blank.
    assert_near(gap(&even), 1.4 * 6.0, 0.01);
    // The default.
    assert_eq!(
        run(&strokes, r#"[{"op":"tidy_line","select":{}}]"#).strokes,
        even.strokes
    );
}

#[test]
fn word_gap_says_where_words_end() {
    let strokes = writing(20.0, 40.0, 0.0, 3, 3);
    // Default: the blank between words (6 mm) is over max(1.2 × 3, 2.5).
    let report = |json: &str| run(&strokes, json).report.orders[0].line.clone().unwrap();
    let words = report(r#"[{"op":"tidy_line","select":{}}]"#);
    assert_eq!(words.words, 3);
    assert_near(words.word_gap, 1.2 * words.body, 0.01);
    assert_eq!(
        report(r#"[{"op":"tidy_line","select":{},"word_gap":7}]"#).words,
        1
    );
    assert_eq!(
        report(r#"[{"op":"tidy_line","select":{},"word_gap":0.5}]"#).words,
        9
    );
}

#[test]
fn the_default_word_gap_is_at_least_2_5_mm() {
    let low: Vec<Stroke> = (0..6)
        .map(|i| {
            let x = 20.0 + i as f32 * 5.0;
            letter(x, 2.8, 40.0, 0.0, 1.0, i * 100)
        })
        .collect();
    let revised = run(&low, r#"[{"op":"tidy_line","select":{}}]"#);
    let report = revised.report.orders[0].line.as_ref().unwrap();
    assert_near(report.word_gap, 2.5, 0.01);
    assert_eq!(report.words, 1, "2.2 mm between the letters");
}

#[test]
fn a_short_line_takes_slope_0() {
    // Under 40 mm: the fit is wrong on short lines.
    let strokes = writing(20.0, 40.0, TILT, 2, 3);
    let revised = run(&strokes, r#"[{"op":"tidy_line","select":{}}]"#);
    let report = revised.report.orders[0].line.as_ref().unwrap();
    assert_eq!(report.slope, 0.0);
    assert_eq!(report.slope_from, SlopeFrom::ShortLine);
}

#[test]
fn a_line_of_few_bins_takes_slope_0() {
    // Two dots far apart: long, but only a few bins of ink.
    let strokes = vec![
        line_stroke([20.0, 40.0], [20.5, 40.0], 0, 10),
        line_stroke([40.0, 45.0], [40.5, 45.0], 20, 30),
        line_stroke([80.0, 50.0], [80.5, 50.0], 40, 50),
    ];
    let revised = run(&strokes, r#"[{"op":"tidy_line","select":{}}]"#);
    let report = revised.report.orders[0].line.as_ref().unwrap();
    assert_eq!(report.slope_from, SlopeFrom::ShortLine);
}

#[test]
fn slope_overrides_the_fit() {
    let strokes = writing(20.0, 40.0, TILT, 8, 4);
    let revised = run(&strokes, r#"[{"op":"tidy_line","select":{},"slope":0}]"#);
    let report = revised.report.orders[0].line.as_ref().unwrap();
    assert_eq!(report.slope, 0.0);
    assert_eq!(report.slope_from, SlopeFrom::Given);
    // A short line too.
    let strokes = writing(20.0, 40.0, TILT, 2, 3);
    let degrees = TILT.atan().to_degrees();
    let revised = run(
        &strokes,
        &format!(r#"[{{"op":"tidy_line","select":{{}},"slope":{degrees},"mode":"rigid"}}]"#),
    );
    for bottom in bottoms(&revised, 0..strokes.len()) {
        assert_near(bottom, 40.0 + TILT * 20.0, 0.2);
    }
}

#[test]
fn rigid_mode_keeps_the_whole_block() {
    // An exponent above a line: it must stay above, and the words keep their gaps.
    let mut strokes = writing(20.0, 40.0, 0.0, 3, 3);
    strokes.push(letter(30.0, 2.0, 35.0, 0.0, 2.0, 2000));
    let revised = run(
        &strokes,
        r#"[{"op":"tidy_line","select":{},"mode":"rigid","slope":0,"left":50,"baseline":80}]"#,
    );
    let report = revised.report.orders[0].line.as_ref().unwrap();
    assert_eq!(report.words, 1);
    let dx = box_of(revised.stroke(0))[0] - box_of(&strokes[0])[0];
    let dy = box_of(revised.stroke(0))[3] - box_of(&strokes[0])[3];
    assert_near(dx, 30.0, 0.01);
    assert_near(box_of(revised.stroke(0))[3], 80.0, 0.3);
    for (number, stroke) in strokes.iter().enumerate() {
        let (before, after) = (box_of(stroke), box_of(revised.stroke(number)));
        assert_near(after[0] - before[0], dx, 0.01);
        assert_near(after[3] - before[3], dy, 0.01);
    }
}

#[test]
fn words_mode_takes_each_word_to_the_baseline() {
    // The second word floats 1 mm above the line: it comes down.
    let mut strokes = writing(20.0, 40.0, 0.0, 3, 4);
    for stroke in &mut strokes[4..8] {
        for p in &mut stroke.points {
            p.y -= 100.0;
        }
    }
    let revised = run(
        &strokes,
        r#"[{"op":"tidy_line","select":{},"baseline":50}]"#,
    );
    for bottom in bottoms(&revised, 0..strokes.len()) {
        assert_near(bottom, 50.0, 0.3);
    }
}

#[test]
fn a_word_floating_far_only_comes_back_part_of_the_way() {
    // At most 0.4 body from the line: an exponent written apart keeps its height.
    let mut strokes = writing(20.0, 40.0, 0.0, 5, 4);
    for stroke in &mut strokes[4..8] {
        for p in &mut stroke.points {
            p.y -= 500.0;
        }
    }
    let revised = run(
        &strokes,
        r#"[{"op":"tidy_line","select":{},"baseline":50}]"#,
    );
    let raised = box_of(revised.stroke(0))[3] - box_of(revised.stroke(4))[3];
    let report = revised.report.orders[0].line.as_ref().unwrap();
    assert!(
        raised < 5.0 && raised >= 5.0 - 0.4 * report.body - 1.0,
        "{raised}"
    );
    assert!(raised > 2.5, "{raised}");
}

#[test]
fn punctuation_only_follows_the_line() {
    // A low mark, lower than 0.45 body, stays at its height from the line: a comma stays low.
    let mut strokes = writing(20.0, 40.0, 0.0, 3, 4);
    let last = box_of(strokes.last().unwrap())[2];
    strokes.push(line_stroke(
        [last + 4.0, 41.0],
        [last + 4.3, 41.3],
        5000,
        5100,
    ));
    let revised = run(
        &strokes,
        r#"[{"op":"tidy_line","select":{},"baseline":50,"gaps":"keep"}]"#,
    );
    assert_near(box_of(revised.stroke(strokes.len() - 1))[3], 51.3, 0.3);
}

#[test]
fn scale_works_around_the_start_of_each_word_and_the_baseline() {
    let strokes = writing(20.0, 40.0, 0.0, 3, 3);
    let revised = run(
        &strokes,
        r#"[{"op":"tidy_line","select":{},"scale":0.5,"baseline":50,"left":20,"gaps":"keep"}]"#,
    );
    let area = box_of(revised.stroke(0));
    assert_near(area[0], 20.0, 0.01);
    assert_near(area[2] - area[0], 1.25, 0.01);
    assert_near(area[3], 50.0, 0.3);
    assert_near(area[3] - area[1], 1.5, 0.05);
    // Blanks too.
    let gap = box_of(revised.stroke(3))[0] - box_of(revised.stroke(2))[2];
    assert_near(gap, 3.0, 0.01);
}

#[test]
fn smooth_takes_the_tremble_off_while_tidying() {
    let strokes = writing(20.0, 40.0, 0.0, 2, 3);
    let plain = run(&strokes, r#"[{"op":"tidy_line","select":{}}]"#);
    let smoothed = run(&strokes, r#"[{"op":"tidy_line","select":{},"smooth":1}]"#);
    assert_ne!(plain.strokes, smoothed.strokes);
    // The ends stay.
    let (a, b) = (&plain.stroke(0).points, &smoothed.stroke(0).points);
    assert_eq!(a[0], b[0]);
    assert_eq!(a[a.len() - 1], b[b.len() - 1]);
}

#[test]
fn a_move_then_a_tidy_composes() {
    // The fit is made where the strokes are now, while the selection reads the note as given.
    let strokes = writing(20.0, 40.0, 0.0, 3, 3);
    let revised = run(
        &strokes,
        r#"[{"op":"move","select":{},"dx":10,"dy":30},
            {"op":"tidy_line","select":{"rect":[10,30,100,20]}}]"#,
    );
    let report = revised.report.orders[1].line.as_ref().unwrap();
    assert_eq!(report.strokes, strokes.len());
    assert_near(report.baseline, 70.0, 0.3);
    assert_near(report.left, 30.0, 0.01);
    assert_near(box_of(revised.stroke(0))[0], 30.0, 0.01);
}

#[test]
fn a_tidy_then_a_copy_takes_the_tidied_letter() {
    let strokes = writing(20.0, 40.0, TILT, 8, 4);
    let revised = run(
        &strokes,
        r#"[{"op":"tidy_line","select":{},"baseline":60},
            {"op":"copy","select":{"ids":[0]},"to_x":200}]"#,
    );
    let number = strokes.len();
    assert_near(box_of(revised.stroke(number))[3], 60.0, 0.3);
    assert_near(box_of(revised.stroke(number))[0], 200.0, 0.01);
}

#[test]
fn time_and_points_do_not_change() {
    let strokes = writing(20.0, 40.0, TILT, 8, 4);
    let revised = run(&strokes, r#"[{"op":"tidy_line","select":{},"smooth":0.2}]"#);
    for (number, stroke) in strokes.iter().enumerate() {
        let after = revised.stroke(number);
        assert_eq!(after.points.len(), stroke.points.len());
        for (a, b) in stroke.points.iter().zip(&after.points) {
            assert_eq!((a.t, a.pressure), (b.t, b.pressure));
        }
    }
}
