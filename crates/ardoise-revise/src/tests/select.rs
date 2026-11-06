//! What a selection finds, and that it reads the note as given.

use super::*;

/// How many strokes a `recolor` finds on these strokes, and which ones it recoloured.
fn selected(strokes: &[Stroke], select: &str) -> Vec<usize> {
    let json = format!(r##"[{{"op":"recolor","select":{select},"color":"#123456"}}]"##);
    let revised = run(strokes, &json);
    let found: Vec<usize> = revised
        .numbers
        .iter()
        .copied()
        .filter(|&n| {
            let color = revised.stroke(n).color;
            [color.r, color.g, color.b] == [0x12, 0x34, 0x56]
        })
        .collect();
    assert_eq!(revised.report.orders[0].selected, found.len());
    found
}

fn varied() -> Vec<Stroke> {
    let mut strokes = row(5);
    strokes[1].color = Rgba::rgb(0xc0, 0x39, 0x2b);
    strokes[2].layer = 1;
    // A long one and a short one.
    strokes[3] = line_stroke([40.0, 20.0], [90.0, 20.0], 3000, 3500);
    strokes[4] = line_stroke([50.0, 50.0], [50.5, 50.0], 4000, 4500);
    strokes
}

#[test]
fn nothing_named_means_every_stroke() {
    assert_eq!(selected(&row(3), "{}"), [0, 1, 2]);
}

#[test]
fn an_empty_list_of_ids_means_none() {
    assert!(selected(&row(3), r#"{"ids":[]}"#).is_empty());
}

#[test]
fn ids_and_rect_add_up() {
    // Stroke 1 by number, stroke 2's centre (32.5, 20) in the rectangle.
    assert_eq!(
        selected(&row(4), r#"{"ids":[1],"rect":[30,15,5,10]}"#),
        [1, 2]
    );
}

#[test]
fn a_rect_looks_at_the_centre_of_the_stroke() {
    // Strokes go from 20 to 25 and from 30 to 35: only the first has its centre inside.
    assert_eq!(selected(&row(3), r#"{"rect":[19,10,4,20]}"#), [1]);
    assert!(selected(&row(3), r#"{"rect":[19,10,3,20]}"#).is_empty());
}

#[test]
fn the_filters_narrow() {
    let strokes = varied();
    assert_eq!(selected(&strokes, r##"{"color":"#C0392B"}"##), [1]);
    assert_eq!(selected(&strokes, r#"{"layer":1}"#), [2]);
    assert_eq!(selected(&strokes, r#"{"min_size":10}"#), [3]);
    assert_eq!(selected(&strokes, r#"{"max_size":1}"#), [4]);
    assert_eq!(selected(&strokes, r#"{"not":[0,3]}"#), [1, 2, 4]);
    assert_eq!(
        selected(&strokes, r#"{"ids":[0,1,2],"layer":0,"not":[0]}"#),
        [1]
    );
}

#[test]
fn a_colour_with_opacity_must_match_it() {
    let mut strokes = row(2);
    strokes[1].color = Rgba {
        r: 0xff,
        g: 0xee,
        b: 0x00,
        a: 0x80,
    };
    assert_eq!(selected(&strokes, r##"{"color":"#ffee00"}"##), [1]);
    assert_eq!(selected(&strokes, r##"{"color":"#ffee0080"}"##), [1]);
    assert!(selected(&strokes, r##"{"color":"#ffee00ff"}"##).is_empty());
}

#[test]
fn a_selection_reads_the_place_the_stroke_was_given_at() {
    // Stroke 0 moves far away; a rectangle at its old place still finds it, one at its new place
    // does not: the assistant reasons on the picture it saw.
    let revised = run(
        &row(2),
        r##"[{"op":"move","select":{"ids":[0]},"dx":100,"dy":100},
             {"op":"recolor","select":{"rect":[9,15,8,10]},"color":"#ff0000"},
             {"op":"recolor","select":{"rect":[105,115,20,10]},"color":"#00ff00"}]"##,
    );
    assert_eq!(revised.stroke(0).color, Rgba::rgb(255, 0, 0));
    assert_eq!(revised.report.orders[1].selected, 1);
    assert_eq!(revised.report.orders[2].selected, 0);
}

#[test]
fn a_selection_reads_the_colour_and_size_the_stroke_was_given_with() {
    let revised = run(
        &row(2),
        r##"[{"op":"recolor","select":{"ids":[0]},"color":"#ff0000"},
             {"op":"scale","select":{"ids":[0]},"factor":10,"origin":[0,0]},
             {"op":"move","select":{"color":"#000000","max_size":6},"dx":1,"dy":0},
             {"op":"move","select":{"color":"#ff0000"},"dx":1,"dy":0}]"##,
    );
    assert_eq!(revised.report.orders[2].selected, 2);
    assert_eq!(revised.report.orders[3].selected, 0);
}

#[test]
fn a_made_stroke_is_selected_on_what_it_was_when_made() {
    let revised = run(
        &row(1),
        r##"[{"op":"add","shapes":[{"line":{"from":[100,100],"to":[110,100]}}],"color":"#0000ff"},
             {"op":"move","select":{"ids":[1]},"dx":50,"dy":0},
             {"op":"recolor","select":{"rect":[95,95,20,10],"color":"#0000ff"},"color":"#00ff00"}]"##,
    );
    assert_eq!(revised.stroke(1).color, Rgba::rgb(0, 255, 0));
    assert_box(revised.stroke(1), [150.0, 100.0, 160.0, 100.0]);
}

#[test]
fn every_stroke_includes_made_ones() {
    let revised = run(
        &row(1),
        r##"[{"op":"copy","select":{"ids":[0]},"dy":10},
             {"op":"recolor","select":{},"color":"#00ff00"}]"##,
    );
    assert_eq!(revised.report.orders[1].selected, 2);
}

#[test]
fn an_empty_selection_is_a_warning_for_most_orders() {
    for order in [
        r##"{"op":"recolor","select":{"ids":[]},"color":"#00ff00"}"##,
        r#"{"op":"restyle","select":{"ids":[]},"width":1}"#,
        r#"{"op":"delete","select":{"ids":[]}}"#,
        r#"{"op":"move","select":{"ids":[]},"dx":1,"dy":1}"#,
        r#"{"op":"scale","select":{"ids":[]},"factor":2,"origin":[0,0]}"#,
        r#"{"op":"rotate","select":{"ids":[]},"degrees":2,"origin":[0,0]}"#,
        r#"{"op":"smooth","select":{"ids":[]},"sigma":0.2}"#,
        r#"{"op":"copy","select":{"ids":[]},"dx":1}"#,
        r#"{"op":"tidy_line","select":{"ids":[]}}"#,
    ] {
        let revised = run(&row(2), &format!("[{order}]"));
        let report = &revised.report.orders[0];
        assert_eq!(report.selected, 0, "{order}");
        assert!(report.warnings[0].contains("nothing selected"), "{order}");
        assert_eq!(revised.strokes, row(2), "{order}");
    }
}

#[test]
fn an_empty_selection_is_an_error_for_replace_and_retime() {
    let message = refused(
        &row(2),
        r#"[{"op":"replace","select":{"ids":[]},"with":[{"line":{"from":[0,0],"to":[1,1]}}]}]"#,
        0,
    );
    assert!(message.contains("nothing selected"), "{message}");
    let message = refused(
        &row(2),
        r#"[{"op":"retime","select":{"rect":[200,200,1,1]},"after":0}]"#,
        0,
    );
    assert!(message.contains("nothing selected"), "{message}");
}

#[test]
fn a_number_that_is_not_a_stroke_is_refused() {
    let message = refused(
        &row(2),
        r#"[{"op":"delete","select":{"ids":[1]}},{"op":"delete","select":{"ids":[2]}}]"#,
        1,
    );
    assert!(message.contains("no stroke 2"), "{message}");
    assert!(message.starts_with("order 1 (delete)"), "{message}");
    let message = refused(&row(2), r#"[{"op":"delete","select":{"not":[7]}}]"#, 0);
    assert!(message.contains("no stroke 7"), "{message}");
}

#[test]
fn naming_a_removed_stroke_shows() {
    let revised = run(
        &row(2),
        r#"[{"op":"delete","select":{"ids":[0]}},{"op":"move","select":{"ids":[0,1]},"dx":1,"dy":0}]"#,
    );
    let report = &revised.report.orders[1];
    assert_eq!(report.selected, 1);
    assert!(report.warnings[0].contains("[0]"), "{:?}", report.warnings);
    assert!(!revised.has(0));
}

#[test]
fn a_selection_is_refused_with_bad_values() {
    for (select, expected) in [
        (r#"{"rect":[0,0,-1,5]}"#, "negative"),
        (r#"{"layer":2}"#, "layer"),
        (r##"{"color":"red"}"##, "#rrggbb"),
        (r##"{"color":"#12345"}"##, "#rrggbb"),
        (r##"{"color":"#12345g"}"##, "#rrggbb"),
        (r#"{"min_size":-1}"#, "negative"),
        (r#"{"max_size":-1}"#, "negative"),
    ] {
        let json = format!(r#"[{{"op":"delete","select":{select}}}]"#);
        let message = refused(&row(1), &json, 0);
        assert!(message.contains(expected), "{select}: {message}");
    }
}
