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

// ---------------------------------------------------------------------------------------------
// Hidden layers: the picture does not show them

/// Layer 0 shown, layer 1 hidden.
fn layer_1_hidden(strokes: &[Stroke]) -> Input<'_> {
    Input {
        visible_layers: &[true, false],
        ..input(strokes)
    }
}

/// Strokes 0, 1 and 3 on layer 0, stroke 2 on layer 1.
fn one_hidden() -> Vec<Stroke> {
    let mut strokes = row(4);
    strokes[2].layer = 1;
    strokes
}

/// What a `recolor` selects, and its warnings.
fn recolored(input: &Input, select: &str) -> (Vec<usize>, Vec<String>) {
    let json = format!(r##"[{{"op":"recolor","select":{select},"color":"#123456"}}]"##);
    let revised = run_on(input, &json);
    let found = revised
        .numbers
        .iter()
        .copied()
        .filter(|&n| revised.stroke(n).color.r == 0x12)
        .collect();
    (found, revised.report.orders[0].warnings.clone())
}

#[test]
fn a_selection_without_a_place_or_by_a_rectangle_takes_drawn_strokes_only() {
    let strokes = one_hidden();
    let input = layer_1_hidden(&strokes);
    let (found, warnings) = recolored(&input, "{}");
    assert_eq!(found, [0, 1, 3]);
    assert!(warnings.is_empty(), "{warnings:?}");
    let (found, warnings) = recolored(&input, r#"{"rect":[0,0,300,200]}"#);
    assert_eq!(found, [0, 1, 3]);
    assert!(warnings.is_empty(), "{warnings:?}");
    // A filter does not bring them back.
    let (found, _) = recolored(&input, r##"{"color":"#000000"}"##);
    assert_eq!(found, [0, 1, 3]);
}

#[test]
fn naming_a_layer_or_strokes_takes_hidden_strokes_and_the_report_says_so() {
    let strokes = one_hidden();
    let input = layer_1_hidden(&strokes);
    for select in [r#"{"layer":1}"#, r#"{"ids":[2]}"#] {
        let (found, warnings) = recolored(&input, select);
        assert_eq!(found, [2], "{select}");
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("[2]") && w.contains("hidden")),
            "{select}: {warnings:?}"
        );
    }
    let (found, _) = recolored(&input, r#"{"rect":[0,0,300,200],"layer":1}"#);
    assert_eq!(found, [2]);
    // `ids` add up with a rectangle: the rectangle still only takes what is drawn.
    let (found, warnings) = recolored(&input, r#"{"ids":[2],"rect":[15,10,10,20]}"#);
    assert_eq!(found, [1, 2]);
    assert!(warnings.iter().any(|w| w.contains("[2]")), "{warnings:?}");
}

#[test]
fn a_selection_that_finds_only_hidden_strokes_says_why_it_is_empty() {
    let strokes = one_hidden();
    let input = layer_1_hidden(&strokes);
    // The rectangle holds stroke 2 only.
    let (found, warnings) = recolored(&input, r#"{"rect":[29,10,5,20]}"#);
    assert!(found.is_empty());
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("nothing selected") && w.contains("`layer`")),
        "{warnings:?}"
    );
    // An empty selection with nothing hidden says nothing of layers.
    let (_, warnings) = recolored(&input, r#"{"rect":[200,10,5,20]}"#);
    assert!(!warnings[0].contains("`layer`"), "{warnings:?}");
}

#[test]
fn a_long_list_of_hidden_strokes_stays_a_sentence() {
    let mut strokes = row(100);
    for stroke in &mut strokes {
        stroke.layer = 1;
    }
    let input = layer_1_hidden(&strokes);
    let (found, warnings) = recolored(&input, r#"{"layer":1}"#);
    assert_eq!(found.len(), 100);
    assert!(warnings[0].len() < 200, "{}", warnings[0]);
    assert!(warnings[0].contains("and 80 more"), "{}", warnings[0]);
}

#[test]
fn replace_goes_on_a_visible_layer_when_it_can() {
    // Layer 0 is the hidden one: the drawing order alone would choose it.
    let mut strokes = row(2);
    strokes[1].layer = 1;
    strokes[1].color = Rgba::rgb(1, 2, 3);
    let input = Input {
        visible_layers: &[false, true],
        ..input(&strokes)
    };
    let revised = run_on(
        &input,
        r#"[{"op":"replace","select":{"ids":[0,1]},"with":[{"line":{"from":[1,1],"to":[9,1]}}]}]"#,
    );
    let made = revised.stroke(2);
    assert_eq!(made.layer, 1);
    assert_eq!(
        made.color,
        Rgba::rgb(1, 2, 3),
        "it looks like the drawn one"
    );
    assert!(
        revised.report.orders[0].warnings[0].contains("[0]"),
        "{:?}",
        revised.report.orders[0].warnings
    );

    // Only hidden strokes are named: what replaces them stays with them.
    let revised = run_on(
        &input,
        r#"[{"op":"replace","select":{"ids":[0]},"with":[{"line":{"from":[1,1],"to":[9,1]}}]}]"#,
    );
    assert_eq!(revised.stroke(2).layer, 0);
    assert!(revised.report.orders[0].warnings[0].contains("hidden"));
}

const ADD_LINE: &str =
    r##"[{"op":"add","shapes":[{"line":{"from":[1,1],"to":[9,1]}}],"color":"#000000"}]"##;

#[test]
fn add_goes_on_a_visible_layer_by_default() {
    // The first writing of the note is on a hidden layer: it gives neither its layer nor its pen.
    let mut strokes = row(2);
    strokes[0].tool = Tool::FeltTip;
    strokes[1].layer = 1;
    let input_ = Input {
        visible_layers: &[false, true],
        ..input(&strokes)
    };
    let revised = run_on(&input_, ADD_LINE);
    let made = revised.stroke(2);
    assert_eq!(made.layer, 1);
    assert_eq!(made.tool, Tool::Ballpoint);
    assert!(revised.report.orders[0].warnings.is_empty());

    // No writing on what is shown: the first layer shown, with the pen of the note's writing.
    let mut hidden_only = row(1);
    hidden_only[0].tool = Tool::FeltTip;
    let input_ = Input {
        visible_layers: &[false, true],
        ..input(&hidden_only)
    };
    let revised = run_on(&input_, ADD_LINE);
    assert_eq!(revised.stroke(1).layer, 1);
    assert_eq!(revised.stroke(1).tool, Tool::FeltTip);

    // Every layer hidden: the report says the new strokes will not show.
    let input_ = Input {
        visible_layers: &[false, false],
        ..input(&hidden_only)
    };
    let revised = run_on(&input_, ADD_LINE);
    assert_eq!(revised.stroke(1).layer, 0);
    assert!(revised.report.orders[0].warnings[0].contains("hidden"));
}

#[test]
fn add_on_a_hidden_layer_it_names_is_warned() {
    let strokes = row(1);
    let input = layer_1_hidden(&strokes);
    let revised = run_on(
        &input,
        r##"[{"op":"add","shapes":[{"line":{"from":[1,1],"to":[9,1]}}],"color":"#000000","layer":1}]"##,
    );
    assert_eq!(revised.stroke(1).layer, 1);
    assert!(revised.report.orders[0].warnings[0].contains("layer 1"));
}

#[test]
fn trim_and_glyph_warn_about_hidden_strokes() {
    let strokes = one_hidden();
    let input = layer_1_hidden(&strokes);
    let revised = run_on(&input, r#"[{"op":"trim","stroke":2,"from":1}]"#);
    assert!(revised.report.orders[0].warnings[0].contains("[2]"));
    let revised = run_on(&input, r#"[{"op":"trim","stroke":0,"from":1}]"#);
    assert!(revised.report.orders[0].warnings.is_empty());
    // A piece of a hidden stroke is on its layer: it would not show.
    let revised = run_on(
        &input,
        r#"[{"op":"glyph","parts":[{"stroke":2,"x":50,"y":50}]}]"#,
    );
    assert_eq!(revised.stroke(4).layer, 1);
    assert!(revised.report.orders[0].warnings[0].contains("[4]"));
    let revised = run_on(
        &input,
        r#"[{"op":"glyph","parts":[{"stroke":0,"x":50,"y":50}]}]"#,
    );
    assert!(revised.report.orders[0].warnings.is_empty());
}

#[test]
fn a_layer_the_list_does_not_reach_is_shown() {
    let strokes = one_hidden();
    let (found, _) = recolored(&input(&strokes), "{}");
    assert_eq!(found, [0, 1, 2, 3]);
}
