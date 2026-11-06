//! Where strokes go in time. The check against the real replay (`Timeline`) is in
//! `crates/ardoise-media/tests/revise_replay.rs`.

use super::*;

/// Stroke 0 from 0 to 500 ms, stroke 1 from `next` to `next + 500`, stroke 2 500 ms later.
fn two_and_a_later(next: u32) -> Vec<Stroke> {
    vec![
        line_stroke([10.0, 20.0], [15.0, 20.0], 0, 500),
        line_stroke([20.0, 20.0], [25.0, 20.0], next, next + 500),
        line_stroke([30.0, 20.0], [35.0, 20.0], next + 1000, next + 1500),
    ]
}

/// A letter to copy that lasts 100 ms, written long after the rest.
fn with_letter(mut strokes: Vec<Stroke>) -> Vec<Stroke> {
    strokes.push(line_stroke([50.0, 20.0], [52.0, 20.0], 900_000, 900_100));
    strokes
}

fn times(stroke: &Stroke) -> (u32, u32) {
    (stroke.points[0].t, stroke.points.last().unwrap().t)
}

fn narrated(strokes: &[Stroke]) -> Input<'_> {
    Input {
        narration: true,
        ..input(strokes)
    }
}

const COPY_AFTER_0: &str = r#"[{"op":"copy","select":{"ids":[3]},"to_x":17,"after":0}]"#;

#[test]
fn a_letter_that_fits_moves_nothing_else() {
    // 40 + 100 + 40 = 180 ms are needed; there are 1000 between the strokes.
    let strokes = with_letter(two_and_a_later(1500));
    let revised = run(&strokes, COPY_AFTER_0);
    assert_eq!(times(revised.stroke(4)), (540, 640));
    for (number, stroke) in strokes.iter().enumerate() {
        assert_eq!(revised.stroke(number), stroke, "stroke {number}");
    }
    let time = revised.report.orders[0].time.as_ref().unwrap();
    assert_eq!(time.after, Some(0));
    assert_eq!((time.start_ms, time.end_ms), (540, 640));
    assert_eq!((time.pushed_ms, time.pushed_strokes), (0, 0));
    assert_eq!(time.pace, 1.0);
    assert_eq!(revised.report.pushed_ms, 0);
    assert_eq!(revised.report.strokes_faster, 0);
    assert_monotonic(revised.stroke(4));
}

#[test]
fn a_letter_that_just_fits_moves_nothing_else() {
    let strokes = with_letter(two_and_a_later(680));
    let revised = run(&strokes, COPY_AFTER_0);
    assert_eq!(times(revised.stroke(1)), (680, 1180));
    assert_eq!(revised.report.strokes_pushed, 0);
    assert_eq!(times(revised.stroke(4)), (540, 640));
}

#[test]
fn a_letter_that_does_not_fit_pushes_what_follows_by_what_is_missing() {
    // 180 ms are needed, 100 are there: what follows moves on by 80. The letter's source, written
    // later, moves on too.
    let strokes = with_letter(two_and_a_later(600));
    let revised = run(&strokes, COPY_AFTER_0);
    assert_eq!(times(revised.stroke(0)), (0, 500));
    assert_eq!(times(revised.stroke(4)), (540, 640));
    assert_eq!(times(revised.stroke(1)), (680, 1180));
    assert_eq!(times(revised.stroke(2)), (1680, 2180));
    assert_eq!(times(revised.stroke(3)), (900_080, 900_180));
    let time = revised.report.orders[0].time.as_ref().unwrap();
    assert_eq!((time.pushed_ms, time.pushed_strokes), (80, 3));
    assert_eq!(time.pace, 1.0);
    assert_eq!(revised.report.pushed_ms, 80);
    assert_eq!(revised.report.strokes_pushed, 3);
    for stroke in &revised.strokes {
        assert_monotonic(stroke);
    }
}

#[test]
fn with_a_narration_nothing_moves_and_the_letter_is_faster() {
    let strokes = with_letter(two_and_a_later(590));
    let revised = run_on(&narrated(&strokes), COPY_AFTER_0);
    for (number, stroke) in strokes.iter().enumerate() {
        assert_eq!(revised.stroke(number), stroke, "stroke {number}");
    }
    let letter = revised.stroke(4);
    assert_monotonic(letter);
    let (start, end) = times(letter);
    assert!(start > 500 && end < 590, "{start}..{end}");
    // 88 ms for 180: about half as fast.
    let pace = 88.0 / 180.0;
    assert!((end - start) as f32 <= 100.0 * pace + 1.0, "{start}..{end}");
    let time = revised.report.orders[0].time.as_ref().unwrap();
    assert_near(time.pace, pace, 0.01);
    assert_eq!((time.pushed_ms, time.pushed_strokes), (0, 0));
    assert_eq!(revised.report.strokes_faster, 1);
    assert_eq!(revised.report.strokes_pushed, 0);
}

#[test]
fn with_a_narration_a_letter_that_fits_keeps_its_speed() {
    let strokes = with_letter(two_and_a_later(1500));
    let revised = run_on(&narrated(&strokes), COPY_AFTER_0);
    assert_eq!(times(revised.stroke(4)), (540, 640));
    assert_eq!(revised.report.strokes_faster, 0);
}

#[test]
fn with_a_narration_no_room_at_all_still_moves_nothing() {
    // The next stroke starts as the other ends.
    let strokes = with_letter(two_and_a_later(500));
    let revised = run_on(&narrated(&strokes), COPY_AFTER_0);
    assert_eq!(times(revised.stroke(1)), (500, 1000));
    let (start, end) = times(revised.stroke(4));
    assert_eq!((start, end), (500, 500));
    assert_eq!(revised.report.orders[0].time.as_ref().unwrap().pace, 0.0);
    assert_monotonic(revised.stroke(4));
}

#[test]
fn after_the_last_stroke_there_is_all_the_room() {
    let strokes = with_letter(two_and_a_later(600));
    for input in [input(&strokes), narrated(&strokes)] {
        let revised = run_on(
            &input,
            r#"[{"op":"copy","select":{"ids":[3]},"dy":10,"after":3}]"#,
        );
        assert_eq!(times(revised.stroke(4)), (900_140, 900_240));
        assert_eq!(revised.report.strokes_pushed, 0);
    }
}

#[test]
fn several_strokes_come_one_after_the_other_as_written() {
    // Selected by number, written the other way round: they keep the order they were written in.
    let mut strokes = with_letter(two_and_a_later(5000));
    strokes.push(line_stroke([60.0, 20.0], [62.0, 20.0], 800_000, 800_200));
    let revised = run(
        &strokes,
        r#"[{"op":"copy","select":{"ids":[3,4]},"dy":10,"after":0}]"#,
    );
    assert_eq!(revised.report.orders[0].new_strokes, [5, 6]);
    // The copy of stroke 4 (200 ms) was written first.
    assert_eq!(times(revised.stroke(6)), (540, 740));
    assert_eq!(times(revised.stroke(5)), (780, 880));
}

#[test]
fn without_after_new_strokes_come_at_the_end_of_the_note() {
    let strokes = with_letter(two_and_a_later(1500));
    let revised = run(
        &strokes,
        r##"[{"op":"copy","select":{"ids":[0]},"dy":5},
             {"op":"add","shapes":[{"line":{"from":[0,0],"to":[150,0]}}],"color":"#000000"},
             {"op":"glyph","parts":[{"stroke":3,"x":0,"y":0}]}]"##,
    );
    assert_eq!(times(revised.stroke(4)), (900_140, 900_640));
    // At 150 mm/s (supposé), 1000 ms.
    assert_eq!(times(revised.stroke(5)), (900_680, 901_680));
    assert_monotonic(revised.stroke(5));
    assert_eq!(times(revised.stroke(6)), (901_720, 901_820));
    assert_eq!(revised.stroke(0), &strokes[0]);
    assert_eq!(revised.report.strokes_pushed, 0);
    // Not with a narration either: nothing moves, the end is free.
    let revised = run_on(
        &narrated(&strokes),
        r#"[{"op":"copy","select":{"ids":[0]},"dy":5}]"#,
    );
    assert_eq!(times(revised.stroke(4)), (900_140, 900_640));
    assert_eq!(revised.report.strokes_faster, 0);
}

#[test]
fn add_and_glyph_can_come_after_a_stroke() {
    let strokes = with_letter(two_and_a_later(2000));
    let revised = run(
        &strokes,
        r##"[{"op":"add","shapes":[{"line":{"from":[0,0],"to":[1,0]}}],"color":"#000000","after":0},
             {"op":"glyph","parts":[{"stroke":3,"x":0,"y":0},{"stroke":0,"x":10,"y":0}],"after":4}]"##,
    );
    // A shape takes at least 60 ms.
    assert_eq!(times(revised.stroke(4)), (540, 600));
    // The pieces come in the order given, each with its own duration.
    assert_eq!(times(revised.stroke(5)), (640, 740));
    assert_eq!(times(revised.stroke(6)), (780, 1280));
    assert_eq!(times(revised.stroke(1)), (2000, 2500));
}

#[test]
fn replace_keeps_the_time_span_and_moves_nothing() {
    let strokes = two_and_a_later(600);
    let revised = run(
        &strokes,
        r#"[{"op":"replace","select":{"ids":[1]},"with":[{"line":{"from":[0,0],"to":[300,0]}}]}]"#,
    );
    assert_eq!(times(revised.stroke(3)), (600, 1100));
    assert_eq!(times(revised.stroke(2)), (1600, 2100));
    assert_eq!(revised.report.strokes_pushed, 0);
}

#[test]
fn retime_leaves_its_old_place_as_a_pause() {
    // Stroke 2 comes right after stroke 0: room is made, its old place stays empty.
    let strokes = two_and_a_later(600);
    let revised = run(
        &strokes,
        r#"[{"op":"retime","select":{"ids":[2]},"after":0}]"#,
    );
    assert_eq!(times(revised.stroke(0)), (0, 500));
    assert_eq!(times(revised.stroke(2)), (540, 1040));
    // 580 needed, 100 there: stroke 1 moves on by 480 and nothing else is after it.
    assert_eq!(times(revised.stroke(1)), (1080, 1580));
    let time = revised.report.orders[0].time.as_ref().unwrap();
    assert_eq!((time.pushed_ms, time.pushed_strokes), (480, 1));
    assert_eq!(revised.report.strokes_after, 3);
    // Nothing moved on the page.
    assert_eq!(box_of(revised.stroke(2)), box_of(&strokes[2]));
}

#[test]
fn retime_keeps_the_order_they_were_written_in() {
    let strokes = row(5);
    let revised = run(
        &strokes,
        r#"[{"op":"retime","select":{"ids":[4,3]},"after":0}]"#,
    );
    assert!(times(revised.stroke(3)).1 < times(revised.stroke(4)).0);
    assert_eq!(times(revised.stroke(3)).0, 540);
}

#[test]
fn retime_refuses_to_come_after_itself() {
    let message = refused(
        &row(3),
        r#"[{"op":"retime","select":{"ids":[0,1]},"after":1}]"#,
        0,
    );
    assert!(message.contains("after itself"), "{message}");
}

#[test]
fn after_cannot_name_a_removed_stroke() {
    let message = refused(
        &row(3),
        r#"[{"op":"delete","select":{"ids":[1]}},{"op":"copy","select":{"ids":[0]},"after":1}]"#,
        1,
    );
    assert!(message.contains("removed by order 0"), "{message}");
}

#[test]
fn the_time_of_a_stroke_made_can_be_named() {
    let strokes = row(2);
    let revised = run(
        &strokes,
        r#"[{"op":"copy","select":{"ids":[0]},"dy":5,"after":0},
            {"op":"copy","select":{"ids":[1]},"dy":5,"after":2}]"#,
    );
    assert_eq!(times(revised.stroke(2)), (540, 1040));
    assert_eq!(times(revised.stroke(3)), (1080, 1580));
    // Each order pushes what is after it: the second pushes the first copy's follower only.
    assert!(times(revised.stroke(1)).0 >= 1580);
}

#[test]
fn hours_of_clock_stay_exact_to_the_millisecond() {
    // At 10 hours a float of 32 bits only counts every 2 or 4 ms.
    let base = 36_000_000;
    let strokes = vec![
        line_stroke([10.0, 20.0], [15.0, 20.0], base, base + 500),
        line_stroke([20.0, 20.0], [25.0, 20.0], base + 601, base + 1101),
        line_stroke([30.0, 20.0], [35.0, 20.0], base + 2001, base + 2101),
    ];
    let revised = run(
        &strokes,
        r#"[{"op":"copy","select":{"ids":[2]},"dy":5,"after":0}]"#,
    );
    assert_eq!(times(revised.stroke(3)), (base + 540, base + 640));
    assert_eq!(times(revised.stroke(1)), (base + 680, base + 1180));
    let letter: Vec<u32> = revised.stroke(3).points.iter().map(|p| p.t).collect();
    let expected: Vec<u32> = strokes[2]
        .points
        .iter()
        .map(|p| p.t - (base + 2001) + base + 540)
        .collect();
    assert_eq!(letter, expected);
}

#[test]
fn the_end_of_the_clock_does_no_harm() {
    let strokes = vec![
        line_stroke([10.0, 20.0], [15.0, 20.0], u32::MAX - 600, u32::MAX - 500),
        line_stroke([20.0, 20.0], [25.0, 20.0], u32::MAX - 450, u32::MAX),
    ];
    let revised = run(
        &strokes,
        r#"[{"op":"retime","select":{"ids":[1]},"after":0}]"#,
    );
    assert_monotonic(revised.stroke(1));
    let revised = run(
        &strokes,
        r#"[{"op":"copy","select":{"ids":[0]},"after":1}]"#,
    );
    assert_monotonic(revised.stroke(2));
}

#[test]
fn times_out_of_order_inside_a_stroke_come_out_in_order() {
    let mut strokes = with_letter(two_and_a_later(1500));
    // As the replay reads them: a sample never appears before the one it follows.
    strokes[3].points[4].t = 899_000;
    let revised = run(&strokes, COPY_AFTER_0);
    assert_monotonic(revised.stroke(4));
    assert_eq!(times(revised.stroke(4)), (540, 640));
}

#[test]
fn a_stroke_starting_where_the_other_ends_is_what_follows() {
    let strokes = with_letter(two_and_a_later(500));
    let revised = run(&strokes, COPY_AFTER_0);
    assert_eq!(times(revised.stroke(4)), (540, 640));
    assert_eq!(times(revised.stroke(1)), (680, 1180));
}

#[test]
fn a_stroke_overlapping_in_time_is_left_alone() {
    // Merged notes: stroke 1 started before stroke 0 ended.
    let strokes = with_letter(vec![
        line_stroke([10.0, 20.0], [15.0, 20.0], 0, 500),
        line_stroke([20.0, 20.0], [25.0, 20.0], 400, 800),
        line_stroke([30.0, 20.0], [35.0, 20.0], 600, 900),
    ]);
    let revised = run(&strokes, COPY_AFTER_0);
    assert_eq!(times(revised.stroke(1)), (400, 800));
    // What starts after: 180 ms are needed, 100 are there.
    assert_eq!(times(revised.stroke(2)), (680, 980));
}

// ---------------------------------------------------------------------------------------------
// A piece of a stroke in time. The check against the replay is in `revise_replay.rs`.

#[test]
fn a_stroke_placed_in_time_cannot_be_trimmed() {
    let strokes = with_letter(two_and_a_later(1500));
    for (place, number) in [
        (
            r#"{"op":"copy","select":{"ids":[3]},"to_x":17,"after":0}"#,
            4,
        ),
        (r#"{"op":"copy","select":{"ids":[3]},"to_x":17}"#, 4),
        (
            r#"{"op":"glyph","parts":[{"stroke":3,"x":17,"y":20}],"after":0}"#,
            4,
        ),
        (r#"{"op":"retime","select":{"ids":[3]},"after":0}"#, 3),
    ] {
        let json = format!(r#"[{place},{{"op":"trim","stroke":{number},"from":2,"to":4}}]"#);
        let message = refused(&strokes, &json, 1);
        assert!(
            message.contains("order 0") && message.contains("`glyph`"),
            "{place}: {message}"
        );
    }
    // Moved by another stroke's placing is not placed: it stays in its own time.
    run(
        &strokes,
        r#"[{"op":"copy","select":{"ids":[3]},"to_x":17,"after":0},{"op":"trim","stroke":1,"from":2}]"#,
    );
}

#[test]
fn a_piece_of_a_stroke_takes_its_own_time_only() {
    // Stroke 3 lasts 100 ms: 10 ms for each of its 10 steps. A piece of 2 steps needs 40 + 20 + 40.
    let strokes = with_letter(two_and_a_later(600));
    let glyph =
        r#"[{"op":"glyph","parts":[{"stroke":3,"from":2,"to":4,"x":17,"y":20}],"after":0}]"#;
    let revised = run(&strokes, glyph);
    assert_eq!(times(revised.stroke(4)), (540, 560));
    // It fits (100 ms are there, 100 are needed): nothing else moves.
    assert_eq!(revised.report.strokes_pushed, 0);
    assert_eq!(times(revised.stroke(1)), (600, 1100));

    // Trimmed before it is placed, the same.
    let revised = run(
        &strokes,
        r#"[{"op":"trim","stroke":3,"from":2,"to":4},{"op":"retime","select":{"ids":[3]},"after":0}]"#,
    );
    assert_eq!(times(revised.stroke(3)), (540, 560));
    assert_eq!(revised.report.strokes_pushed, 0);
}

#[test]
fn a_stroke_not_placed_stays_in_its_own_time_when_it_is_trimmed() {
    let strokes = two_and_a_later(600);
    let revised = run(&strokes, r#"[{"op":"trim","stroke":0,"from":2,"to":4}]"#);
    assert_eq!(times(revised.stroke(0)), (100, 200));
    for (number, stroke) in strokes.iter().enumerate().skip(1) {
        assert_eq!(revised.stroke(number), stroke);
    }
}
