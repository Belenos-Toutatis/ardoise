//! Each order, and stroke numbers across orders.

use super::*;

#[test]
fn recolor_changes_the_colour_and_keeps_the_opacity() {
    let mut strokes = row(3);
    strokes[1].color.a = 0x80;
    let revised = run(
        &strokes,
        r##"[{"op":"recolor","select":{"ids":[0,1]},"color":"#1f4e9e"}]"##,
    );
    assert_eq!(revised.stroke(0).color, Rgba::rgb(0x1f, 0x4e, 0x9e));
    assert_eq!(revised.stroke(1).color.a, 0x80);
    assert_eq!(revised.stroke(2).color, Rgba::BLACK);
    let revised = run(
        &strokes,
        r##"[{"op":"recolor","select":{"ids":[1]},"color":"#1f4e9eff"}]"##,
    );
    assert_eq!(revised.stroke(1).color.a, 0xff);
}

#[test]
fn restyle_changes_the_pen() {
    let revised = run(
        &row(3),
        r#"[{"op":"restyle","select":{"ids":[0]},"width":1.5},
            {"op":"restyle","select":{"ids":[1]},"tool":"highlighter"},
            {"op":"restyle","select":{"ids":[2]},"tool":"fountainPen","width":0.25}]"#,
    );
    assert_near(revised.stroke(0).width, 150.0, 0.001);
    assert_eq!(revised.stroke(0).tool, Tool::Ballpoint);
    assert_eq!(revised.stroke(1).tool, Tool::Highlighter);
    assert_near(revised.stroke(1).width, 50.0, 0.001);
    assert_eq!(revised.stroke(2).tool, Tool::FountainPen);
    assert_near(revised.stroke(2).width, 25.0, 0.001);
}

#[test]
fn restyle_needs_something_to_change() {
    let message = refused(&row(1), r#"[{"op":"restyle","select":{}}]"#, 0);
    assert!(message.contains("`width`, `tool`"), "{message}");
}

#[test]
fn delete_removes_and_the_other_numbers_stay() {
    let revised = run(
        &row(4),
        r##"[{"op":"delete","select":{"ids":[1]}},
             {"op":"recolor","select":{"ids":[2]},"color":"#ff0000"}]"##,
    );
    assert_eq!(revised.numbers, [0, 2, 3]);
    assert_eq!(revised.stroke(2).color, Rgba::rgb(255, 0, 0));
    assert_eq!(revised.report.strokes_before, 4);
    assert_eq!(revised.report.strokes_after, 3);
}

#[test]
fn move_scale_and_rotate_change_the_place() {
    let strokes = vec![line_stroke([10.0, 20.0], [20.0, 20.0], 0, 100)];
    let moved = run(&strokes, r#"[{"op":"move","select":{},"dx":5,"dy":-2.5}]"#);
    assert_box(moved.stroke(0), [15.0, 17.5, 25.0, 17.5]);

    let scaled = run(
        &strokes,
        r#"[{"op":"scale","select":{},"factor":2,"origin":[10,0]}]"#,
    );
    assert_box(scaled.stroke(0), [10.0, 40.0, 30.0, 40.0]);

    let stretched = run(
        &strokes,
        r#"[{"op":"scale","select":{},"sx":0.5,"sy":3,"origin":[10,20]}]"#,
    );
    assert_box(stretched.stroke(0), [10.0, 20.0, 15.0, 20.0]);

    // A quarter turn clockwise on the page: what went right now goes down.
    let turned = run(
        &strokes,
        r#"[{"op":"rotate","select":{},"degrees":90,"origin":[10,20]}]"#,
    );
    assert_box(turned.stroke(0), [10.0, 20.0, 10.0, 30.0]);
    assert_near(turned.stroke(0).points[10].y, 3000.0, 0.01);
}

#[test]
fn scale_wants_factor_or_both_sides() {
    for fields in ["", r#""sx":2,"#, r#""factor":2,"sx":2,"sy":2,"#] {
        let json = format!(r#"[{{"op":"scale","select":{{}},{fields}"origin":[0,0]}}]"#);
        let message = refused(&row(1), &json, 0);
        assert!(message.contains("`factor`, or `sx` and `sy`"), "{message}");
    }
}

#[test]
fn smooth_takes_the_tremble_off_and_the_ends_stay() {
    let mut stroke = line_stroke([10.0, 20.0], [40.0, 20.0], 0, 1000);
    stroke.points = (0..=300)
        .map(|i| {
            let wobble = if i % 2 == 0 { 0.05 } else { -0.05 };
            pt(10.0 + i as f32 * 0.1, 20.0 + wobble, i * 3)
        })
        .collect();
    let last = stroke.points.len() - 1;
    let revised = run(
        std::slice::from_ref(&stroke),
        r#"[{"op":"smooth","select":{},"sigma":0.2}]"#,
    );
    let smooth = revised.stroke(0);
    assert_eq!(smooth.points.len(), stroke.points.len());
    assert_eq!(smooth.points[0], stroke.points[0]);
    assert_eq!(smooth.points[last], stroke.points[last]);
    let middle = &smooth.points[100..200];
    assert!(middle.iter().all(|p| (p.y - 2000.0).abs() < 1.0));
    // Pressure and time do not change.
    assert_eq!(smooth.points[150].t, stroke.points[150].t);
    assert_eq!(smooth.points[150].pressure, stroke.points[150].pressure);
}

#[test]
fn trim_keeps_these_points() {
    let revised = run(&row(1), r#"[{"op":"trim","stroke":0,"from":2,"to":4}]"#);
    let stroke = revised.stroke(0);
    assert_eq!(stroke.points, row(1)[0].points[2..=4].to_vec());
    let revised = run(&row(1), r#"[{"op":"trim","stroke":0,"from":9}]"#);
    assert_eq!(revised.stroke(0).points.len(), 2);
    // A second trim counts from what is left.
    let revised = run(
        &row(1),
        r#"[{"op":"trim","stroke":0,"from":2,"to":8},{"op":"trim","stroke":0,"from":0,"to":1}]"#,
    );
    assert_eq!(revised.stroke(0).points, row(1)[0].points[2..=3].to_vec());
}

#[test]
fn trim_out_of_range_is_refused() {
    for order in [
        r#"{"op":"trim","stroke":0,"from":0,"to":11}"#,
        r#"{"op":"trim","stroke":0,"from":11}"#,
        r#"{"op":"trim","stroke":0,"from":4,"to":2}"#,
    ] {
        let message = refused(&row(1), &format!("[{order}]"), 0);
        assert!(message.contains("trim"), "{message}");
    }
    let message = refused(&row(1), r#"[{"op":"trim","stroke":3,"from":0}]"#, 0);
    assert!(message.contains("no stroke 3"), "{message}");
    let message = refused(
        &row(1),
        r#"[{"op":"delete","select":{}},{"op":"trim","stroke":0,"from":0}]"#,
        1,
    );
    assert!(message.contains("removed by order 0"), "{message}");
}

#[test]
fn replace_takes_the_place_and_the_look_of_what_it_replaces() {
    let mut strokes = row(4);
    strokes[1].tool = Tool::FeltTip;
    strokes[1].color = Rgba::rgb(10, 20, 30);
    strokes[1].width = 80.0;
    strokes[1].layer = 1;
    strokes[2].layer = 1;
    for p in &mut strokes[1].points {
        p.pressure = 0.8;
    }
    for p in &mut strokes[2].points {
        p.pressure = 0.8;
    }
    let revised = run(
        &strokes,
        r#"[{"op":"replace","select":{"ids":[1,2]},
             "with":[{"line":{"from":[5,5],"to":[6,5]}},{"line":{"from":[5,6],"to":[8,6]}}]}]"#,
    );
    assert_eq!(revised.report.orders[0].new_strokes, [4, 5]);
    // At the place of the first in the list.
    assert_eq!(revised.numbers, [0, 4, 5, 3]);
    let (first, second) = (revised.stroke(4), revised.stroke(5));
    for made in [first, second] {
        assert_eq!(made.tool, Tool::FeltTip);
        assert_eq!(made.color, Rgba::rgb(10, 20, 30));
        assert_near(made.width, 80.0, 0.001);
        assert_eq!(made.layer, 1);
        assert!(made.points.iter().all(|p| (p.pressure - 0.8).abs() < 1e-5));
        assert_monotonic(made);
    }
    // They share the time span of what they replace, in proportion to their lengths.
    assert_eq!(first.points[0].t, 1000);
    assert_eq!(second.points.last().unwrap().t, 2500);
    assert_eq!(first.points.last().unwrap().t, second.points[0].t);
    assert_eq!(first.points.last().unwrap().t, 1000 + 1500 / 4);
}

#[test]
fn replace_starts_at_the_first_in_the_drawing_order() {
    // Stroke 0 is on the upper layer: the drawing starts with stroke 1.
    let mut strokes = row(3);
    strokes[0].layer = 1;
    strokes[0].color = Rgba::rgb(9, 9, 9);
    strokes[1].color = Rgba::rgb(1, 1, 1);
    let revised = run(
        &strokes,
        r#"[{"op":"replace","select":{"ids":[0,1]},"with":[{"line":{"from":[5,5],"to":[6,5]}}]}]"#,
    );
    assert_eq!(revised.numbers, [3, 2]);
    assert_eq!(revised.stroke(3).color, Rgba::rgb(1, 1, 1));
    assert_eq!(revised.stroke(3).layer, 0);
}

#[test]
fn replace_can_change_colour_and_width() {
    let revised = run(
        &row(1),
        r##"[{"op":"replace","select":{"ids":[0]},"color":"#ff0000","width":1,
              "with":[{"line":{"from":[5,5],"to":[6,5]}}]}]"##,
    );
    assert_eq!(revised.stroke(1).color, Rgba::rgb(255, 0, 0));
    assert_near(revised.stroke(1).width, 100.0, 0.001);
}

#[test]
fn add_looks_like_the_writing_by_default() {
    let mut strokes = row(1);
    strokes[0].tool = Tool::FountainPen;
    strokes[0].width = 70.0;
    strokes[0].layer = 1;
    let revised = run(
        &strokes,
        r##"[{"op":"add","shapes":[{"line":{"from":[5,5],"to":[6,5]}}],"color":"#00ff00"}]"##,
    );
    let made = revised.stroke(1);
    assert_eq!(made.tool, Tool::FountainPen);
    assert_near(made.width, 70.0, 0.001);
    assert_eq!(made.layer, 1);
    assert_eq!(made.color, Rgba::rgb(0, 255, 0));
    assert_eq!(revised.numbers, [0, 1]);
}

#[test]
fn add_with_another_tool_takes_its_width() {
    let revised = run(
        &row(1),
        r##"[{"op":"add","tool":"highlighter","layer":1,"behind":true,"color":"#ffee00",
              "shapes":[{"rect":{"x":5,"y":5,"w":20,"h":4}}]}]"##,
    );
    let made = revised.stroke(1);
    assert_eq!(made.tool, Tool::Highlighter);
    assert_near(made.width, Tool::Highlighter.default_width(), 0.001);
    assert_eq!(made.layer, 1);
    // Under the others.
    assert_eq!(revised.numbers, [1, 0]);
}

#[test]
fn add_can_give_the_width() {
    let revised = run(
        &[],
        r##"[{"op":"add","width":2,"color":"#ffee00","shapes":[{"line":{"from":[0,0],"to":[1,1]}}]}]"##,
    );
    assert_near(revised.stroke(0).width, 200.0, 0.001);
    assert_eq!(revised.stroke(0).tool, Tool::Ballpoint);
    assert_eq!(revised.stroke(0).layer, 0);
}

#[test]
fn several_things_behind_stay_in_their_order_under_everything() {
    let revised = run(
        &row(1),
        r##"[{"op":"add","behind":true,"color":"#ffee00","shapes":[{"line":{"from":[0,0],"to":[1,1]}}]},
             {"op":"add","behind":true,"color":"#ffee00","shapes":[{"line":{"from":[0,0],"to":[1,1]}}]},
             {"op":"add","color":"#ffee00","shapes":[{"line":{"from":[0,0],"to":[1,1]}}]}]"##,
    );
    assert_eq!(revised.numbers, [1, 2, 0, 3]);
}

#[test]
fn what_replaces_a_stroke_behind_stays_behind() {
    let revised = run(
        &row(1),
        r##"[{"op":"add","behind":true,"color":"#ffee00","shapes":[{"line":{"from":[0,0],"to":[1,1]}}]},
             {"op":"replace","select":{"ids":[1]},"with":[{"line":{"from":[0,0],"to":[2,2]}}]},
             {"op":"add","behind":true,"color":"#ffee00","shapes":[{"line":{"from":[0,0],"to":[1,1]}}]}]"##,
    );
    assert_eq!(revised.numbers, [2, 3, 0]);
}

#[test]
fn copy_places_what_it_copies() {
    let strokes = row(3);
    // Its left side at 100, 5 lower; both strokes keep their distance.
    let revised = run(
        &strokes,
        r#"[{"op":"copy","select":{"ids":[0,1]},"to_x":100,"dy":5}]"#,
    );
    assert_eq!(revised.report.orders[0].new_strokes, [3, 4]);
    assert_eq!(revised.numbers, [0, 1, 2, 3, 4]);
    assert_box(revised.stroke(3), [100.0, 25.0, 105.0, 25.0]);
    assert_box(revised.stroke(4), [110.0, 25.0, 115.0, 25.0]);
    // The originals stay.
    assert_eq!(revised.stroke(0), &strokes[0]);

    let revised = run(&strokes, r#"[{"op":"copy","select":{"ids":[0]},"dx":-3}]"#);
    assert_box(revised.stroke(3), [7.0, 20.0, 12.0, 20.0]);
}

#[test]
fn copy_resizes_around_its_bottom_left() {
    let strokes = vec![line_stroke([10.0, 10.0], [20.0, 20.0], 0, 100)];
    let revised = run(
        &strokes,
        r##"[{"op":"copy","select":{},"factor":2,"dx":100,"dy":1,"color":"#ff0000"}]"##,
    );
    assert_box(revised.stroke(1), [110.0, 1.0, 130.0, 21.0]);
    assert_eq!(revised.stroke(1).color, Rgba::rgb(255, 0, 0));
}

#[test]
fn copy_takes_strokes_as_they_are_now() {
    let revised = run(
        &row(1),
        r#"[{"op":"move","select":{"ids":[0]},"dx":30,"dy":0},
            {"op":"copy","select":{"ids":[0]},"dy":5}]"#,
    );
    assert_box(revised.stroke(1), [40.0, 25.0, 45.0, 25.0]);
}

#[test]
fn copy_refuses_two_ways_of_saying_where() {
    let message = refused(&row(1), r#"[{"op":"copy","select":{},"to_x":1,"dx":2}]"#, 0);
    assert!(message.contains("`to_x` or `dx`"), "{message}");
}

#[test]
fn a_glyph_places_its_pieces_by_their_left_and_bottom() {
    let strokes = vec![line_stroke([10.0, 10.0], [20.0, 30.0], 0, 100)];
    let revised = run(
        &strokes,
        r##"[{"op":"glyph","color":"#ff0000","parts":[
               {"stroke":0,"x":100,"y":50},
               {"stroke":0,"sx":2,"sy":0.5,"x":200,"y":50}]}]"##,
    );
    assert_eq!(revised.report.orders[0].new_strokes, [1, 2]);
    assert_eq!(revised.report.orders[0].selected, 2);
    assert_box(revised.stroke(1), [100.0, 30.0, 110.0, 50.0]);
    assert_box(revised.stroke(2), [200.0, 40.0, 220.0, 50.0]);
    assert_eq!(revised.stroke(1).color, Rgba::rgb(255, 0, 0));
    // The source stays.
    assert_box(revised.stroke(0), [10.0, 10.0, 20.0, 30.0]);
}

#[test]
fn a_glyph_mirrors_and_turns_within_the_box() {
    let strokes = vec![line_stroke([10.0, 10.0], [20.0, 30.0], 0, 100)];
    let revised = run(
        &strokes,
        r#"[{"op":"glyph","parts":[
              {"stroke":0,"flip":"h","x":100,"y":50},
              {"stroke":0,"flip":"v","x":100,"y":50},
              {"stroke":0,"flip":"rot","x":100,"y":50}]}]"#,
    );
    let first_and_last = |number: usize| {
        let points = &revised.stroke(number).points;
        let (a, b) = (points[0], points[points.len() - 1]);
        [a.x / 100.0, a.y / 100.0, b.x / 100.0, b.y / 100.0]
    };
    // The original starts at the top left and ends at the bottom right of its box.
    let close = |found: [f32; 4], expected: [f32; 4]| {
        for (a, b) in found.iter().zip(expected) {
            assert_near(*a, b, 0.01);
        }
    };
    close(first_and_last(1), [110.0, 30.0, 100.0, 50.0]);
    close(first_and_last(2), [100.0, 50.0, 110.0, 30.0]);
    close(first_and_last(3), [110.0, 50.0, 100.0, 30.0]);
}

#[test]
fn a_glyph_can_take_some_points_of_a_stroke() {
    let revised = run(
        &row(1),
        r#"[{"op":"glyph","parts":[{"stroke":0,"from":2,"to":4,"x":100,"y":50}]}]"#,
    );
    assert_eq!(revised.stroke(1).points.len(), 3);
    assert_box(revised.stroke(1), [100.0, 50.0, 101.0, 50.0]);
}

#[test]
fn a_glyph_refuses_wrong_pieces() {
    for (part, expected) in [
        (r#"{"stroke":5,"x":0,"y":0}"#, "no stroke 5"),
        (
            r#"{"stroke":0,"from":0,"to":11,"x":0,"y":0}"#,
            "out of range",
        ),
        (r#"{"stroke":0,"from":11,"x":0,"y":0}"#, "out of range"),
        (r#"{"stroke":0,"from":3,"to":2,"x":0,"y":0}"#, "after `to`"),
        (r#"{"stroke":0,"sx":0,"x":0,"y":0}"#, "between"),
    ] {
        let json = format!(r#"[{{"op":"glyph","parts":[{part}]}}]"#);
        let message = refused(&row(1), &json, 0);
        assert!(message.contains(expected), "{part}: {message}");
    }
    let message = refused(&row(1), r#"[{"op":"glyph","parts":[]}]"#, 0);
    assert!(message.contains("no part"), "{message}");
}

#[test]
fn numbers_go_on_across_orders() {
    // 2 strokes: the copy of one is 2, the shape is 3, the two copies are 4 and 5, the glyph 6,
    // and later orders can name them all, whatever happened to the others.
    let revised = run(
        &row(2),
        r##"[{"op":"copy","select":{"ids":[0]},"dy":10},
             {"op":"delete","select":{"ids":[0]}},
             {"op":"add","shapes":[{"line":{"from":[0,0],"to":[1,0]}}],"color":"#ff0000"},
             {"op":"copy","select":{"ids":[1,2]},"dy":20},
             {"op":"glyph","parts":[{"stroke":5,"x":200,"y":100}]},
             {"op":"replace","select":{"ids":[3]},"with":[{"circle":{"c":[50,50],"r":5}}]},
             {"op":"move","select":{"ids":[6,7]},"dx":1,"dy":0}]"##,
    );
    let news: Vec<&[usize]> = revised
        .report
        .orders
        .iter()
        .map(|order| order.new_strokes.as_slice())
        .collect();
    assert_eq!(news, [&[2][..], &[], &[3], &[4, 5], &[6], &[7], &[]]);
    assert_box(revised.stroke(6), [201.0, 100.0, 206.0, 100.0]);
    assert!(!revised.has(0));
    assert!(!revised.has(3));
    assert_eq!(revised.report.orders[6].selected, 2);
    // Copy 2 came from stroke 0 and 4 is its copy, 5 lower... each at its own place.
    assert_box(revised.stroke(2), [10.0, 30.0, 15.0, 30.0]);
    assert_box(revised.stroke(4), [20.0, 40.0, 25.0, 40.0]);
    assert_box(revised.stroke(5), [10.0, 50.0, 15.0, 50.0]);
}

#[test]
fn a_replaced_stroke_can_be_named_no_more() {
    let message = refused(
        &row(2),
        r#"[{"op":"replace","select":{"ids":[0]},"with":[{"line":{"from":[0,0],"to":[1,1]}}]},
            {"op":"retime","select":{"ids":[1]},"after":0}]"#,
        1,
    );
    assert!(
        message.contains("stroke 0 was removed by order 0"),
        "{message}"
    );
}

#[test]
fn geometry_changes_accumulate() {
    let revised = run(
        &row(1),
        r#"[{"op":"move","select":{"ids":[0]},"dx":10,"dy":0},
            {"op":"scale","select":{"ids":[0]},"factor":2,"origin":[0,0]},
            {"op":"move","select":{"ids":[0]},"dx":1,"dy":1}]"#,
    );
    assert_box(revised.stroke(0), [41.0, 41.0, 51.0, 41.0]);
}

#[test]
fn a_stroke_without_points_is_removed_and_counted() {
    let mut strokes = row(2);
    strokes[0].points.clear();
    let revised = run(&strokes, r#"[{"op":"move","select":{},"dx":1,"dy":0}]"#);
    assert_eq!(revised.numbers, [1]);
    assert_eq!(revised.report.empty_removed, 1);
    assert_eq!(revised.report.orders[0].selected, 1);
}

#[test]
fn what_ends_up_off_the_page_shows() {
    let mut strokes = row(3);
    // Already beyond the page, and not touched: not this list's doing.
    strokes[2] = line_stroke([295.0, 20.0], [305.0, 20.0], 3000, 3100);
    let revised = run(
        &strokes,
        r##"[{"op":"move","select":{"ids":[0]},"dx":290,"dy":0},
             {"op":"add","shapes":[{"line":{"from":[10,190],"to":[10,210]}}],"color":"#ff0000"},
             {"op":"move","select":{"ids":[1]},"dx":1,"dy":0}]"##,
    );
    assert_eq!(revised.report.off_page, [0, 3]);
}

#[test]
fn samples_that_cannot_be_drawn_do_no_harm() {
    let mut strokes = row(2);
    strokes[0].points[3].x = f32::NAN;
    strokes[0].points[4].y = f32::INFINITY;
    let orders = parse(
        r#"[{"op":"move","select":{},"dx":1,"dy":0},
            {"op":"smooth","select":{},"sigma":0.2},
            {"op":"rotate","select":{},"degrees":1,"origin":[0,0]},
            {"op":"copy","select":{},"dy":10},
            {"op":"tidy_line","select":{"ids":[0,1]},"smooth":0.2}]"#,
    );
    let revised = apply(&input(&strokes), &orders).unwrap();
    assert_eq!(revised.strokes.len(), 4);
    let found = ink_lines(&revised.strokes, &[0, 1, 2, 3]);
    assert!(!found.lines.is_empty());
}

#[test]
fn the_report_goes_out_in_json() {
    let revised = run(
        &row(2),
        r##"[{"op":"tidy_line","select":{}},{"op":"copy","select":{"ids":[0]},"after":0}]"##,
    );
    let json = serde_json::to_value(&revised.report).unwrap();
    assert_eq!(json["strokes_before"], 2);
    assert_eq!(json["orders"][0]["op"], "tidy_line");
    assert_eq!(json["orders"][0]["line"]["slope_from"], "short_line");
    assert_eq!(json["orders"][1]["new_strokes"][0], 2);
    assert_eq!(json["orders"][1]["time"]["after"], 0);
    // What did not happen is not said.
    assert!(json["orders"][0].get("new_strokes").is_none());
    assert!(json["orders"][0].get("warnings").is_none());
}
