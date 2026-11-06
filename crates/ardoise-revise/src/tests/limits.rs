//! What is refused, with a message, before any work.

use super::*;

const LINE: &str = r#"{"line":{"from":[1,1],"to":[2,1]}}"#;

fn add(shape: &str) -> String {
    format!(r##"[{{"op":"add","color":"#000000","shapes":[{shape}]}}]"##)
}

#[test]
fn more_than_5000_orders_are_refused() {
    let orders = vec![
        Order::Delete {
            select: Select::default(),
        };
        MAX_ORDERS + 1
    ];
    let error = apply(&input(&row(1)), &orders).unwrap_err();
    assert_eq!(error, ReviseError::TooManyOrders { count: 5001 });
    assert!(error.to_string().contains("5000"), "{error}");

    let orders = vec![
        Order::Delete {
            select: Select {
                ids: Some(Vec::new()),
                ..Select::default()
            },
        };
        MAX_ORDERS
    ];
    assert!(apply(&input(&row(1)), &orders).is_ok());
}

#[test]
fn numbers_too_large_for_json_become_infinite_and_are_refused() {
    // serde gives f32::INFINITY for 1e39, as JSON has no such number: `apply` must catch it.
    for (order, name) in [
        (r#"{"op":"move","select":{},"dx":1e39,"dy":0}"#, "dx"),
        (r#"{"op":"move","select":{},"dx":0,"dy":-1e39}"#, "dy"),
        (
            r#"{"op":"move","select":{"rect":[1e39,0,1,1]},"dx":0,"dy":0}"#,
            "rect",
        ),
        (
            r#"{"op":"move","select":{"min_size":1e39},"dx":0,"dy":0}"#,
            "min_size",
        ),
        (
            r#"{"op":"move","select":{"max_size":1e39},"dx":0,"dy":0}"#,
            "max_size",
        ),
        (
            r#"{"op":"scale","select":{},"factor":1e39,"origin":[0,0]}"#,
            "factor",
        ),
        (
            r#"{"op":"scale","select":{},"factor":2,"origin":[1e39,0]}"#,
            "origin",
        ),
        (
            r#"{"op":"scale","select":{},"sx":1e39,"sy":2,"origin":[0,0]}"#,
            "sx",
        ),
        (
            r#"{"op":"rotate","select":{},"degrees":1e39,"origin":[0,0]}"#,
            "degrees",
        ),
        (
            r#"{"op":"rotate","select":{},"degrees":1,"origin":[0,1e39]}"#,
            "origin",
        ),
        (r#"{"op":"smooth","select":{},"sigma":1e39}"#, "sigma"),
        (r#"{"op":"restyle","select":{},"width":1e39}"#, "width"),
        (r#"{"op":"copy","select":{},"to_x":1e39}"#, "to_x"),
        (r#"{"op":"copy","select":{},"dx":1e39}"#, "dx"),
        (r#"{"op":"copy","select":{},"dy":1e39}"#, "dy"),
        (r#"{"op":"copy","select":{},"factor":1e39}"#, "factor"),
        (
            r#"{"op":"glyph","parts":[{"stroke":0,"x":1e39,"y":0}]}"#,
            "x",
        ),
        (
            r#"{"op":"glyph","parts":[{"stroke":0,"x":0,"y":1e39}]}"#,
            "y",
        ),
        (
            r#"{"op":"glyph","parts":[{"stroke":0,"sy":1e39,"x":0,"y":0}]}"#,
            "sy",
        ),
        (
            r#"{"op":"tidy_line","select":{},"baseline":1e39}"#,
            "baseline",
        ),
        (r#"{"op":"tidy_line","select":{},"left":1e39}"#, "left"),
        (r#"{"op":"tidy_line","select":{},"scale":1e39}"#, "scale"),
        (r#"{"op":"tidy_line","select":{},"slope":1e39}"#, "slope"),
        (
            r#"{"op":"tidy_line","select":{},"word_gap":1e39}"#,
            "word_gap",
        ),
        (r#"{"op":"tidy_line","select":{},"smooth":1e39}"#, "smooth"),
    ] {
        let message = refused(&row(1), &format!("[{order}]"), 0);
        assert!(message.contains(&format!("`{name}`")), "{order}: {message}");
        assert!(message.contains("finite"), "{order}: {message}");
    }
}

#[test]
fn not_a_number_is_refused_too() {
    let nan = f32::NAN;
    let orders = [
        Order::Move {
            select: Select::default(),
            dx: nan,
            dy: 0.0,
        },
        Order::Add {
            shapes: vec![Shape::Line {
                from: [nan, 0.0],
                to: [1.0, 1.0],
            }],
            color: "#000000".to_owned(),
            width: None,
            tool: None,
            layer: None,
            behind: false,
            after: None,
        },
        Order::Add {
            shapes: vec![Shape::Polyline {
                points: vec![[0.0, 0.0], [f32::INFINITY, 0.0]],
                closed: false,
            }],
            color: "#000000".to_owned(),
            width: None,
            tool: None,
            layer: None,
            behind: false,
            after: None,
        },
        Order::Add {
            shapes: vec![Shape::Circle {
                c: [0.0, 0.0],
                r: nan,
            }],
            color: "#000000".to_owned(),
            width: None,
            tool: None,
            layer: None,
            behind: false,
            after: None,
        },
    ];
    for order in orders {
        let error = apply(&input(&row(1)), std::slice::from_ref(&order)).unwrap_err();
        assert!(error.to_string().contains("finite"), "{order:?}: {error}");
    }
}

#[test]
fn places_absurdly_far_are_refused() {
    let message = refused(&row(1), r#"[{"op":"move","select":{},"dx":1e6,"dy":0}]"#, 0);
    assert!(message.contains("further than"), "{message}");
    // Repeated scaling gets there by itself.
    let many = [r#"{"op":"scale","select":{},"factor":20,"origin":[0,0]}"#; 10].join(",");
    let error = apply(&input(&row(1)), &parse(&format!("[{many}]"))).unwrap_err();
    assert!(error.to_string().contains("further than"), "{error}");
}

#[test]
fn unknown_strokes_are_refused_wherever_they_are_named() {
    for order in [
        r#"{"op":"retime","select":{"ids":[0]},"after":9}"#,
        r#"{"op":"retime","select":{"ids":[9]},"after":0}"#,
        r#"{"op":"copy","select":{"ids":[0]},"after":9}"#,
        r#"{"op":"glyph","parts":[{"stroke":0,"x":0,"y":0}],"after":9}"#,
        r##"{"op":"add","shapes":[{"line":{"from":[0,0],"to":[1,1]}}],"color":"#000000","after":9}"##,
        r#"{"op":"trim","stroke":9,"from":0}"#,
        r#"{"op":"glyph","parts":[{"stroke":9,"x":0,"y":0}]}"#,
    ] {
        let message = refused(&row(1), &format!("[{order}]"), 0);
        assert!(message.contains("no stroke 9"), "{order}: {message}");
    }
}

#[test]
fn a_layer_that_does_not_exist_is_refused() {
    let message = refused(
        &row(1),
        r##"[{"op":"add","shapes":[{"line":{"from":[0,0],"to":[1,1]}}],"color":"#000000","layer":2}]"##,
        0,
    );
    assert!(message.contains("2 layer(s)"), "{message}");
    let message = refused(&row(1), r#"[{"op":"delete","select":{"layer":2}}]"#, 0);
    assert!(message.contains("2 layer(s)"), "{message}");
}

#[test]
fn more_than_a_million_points_made_are_refused() {
    // Each 5 m line makes 10,000 samples: 101 of them are too many.
    let long = r#"{"line":{"from":[0,0],"to":[4990,0]}}"#;
    let shapes = [long; 100].join(",");
    assert!(apply(&input(&[]), &parse(&add(&shapes))).is_ok());
    let shapes = [long; 101].join(",");
    let message = refused(&[], &add(&shapes), 0);
    assert!(message.contains("1000000 points"), "{message}");
}

#[test]
fn copies_count_in_the_points_made() {
    // A stroke of 100,000 points can be copied 10 times, not 11.
    let mut stroke = row(1).remove(0);
    let point = stroke.points[0];
    stroke.points = vec![point; 100_000];
    let strokes = [stroke];
    let copy = r#"{"op":"copy","select":{"ids":[0]},"dy":1}"#;
    let list = [copy; 10].join(",");
    assert!(apply(&input(&strokes), &parse(&format!("[{list}]"))).is_ok());
    let list = [copy; 11].join(",");
    let error = apply(&input(&strokes), &parse(&format!("[{list}]"))).unwrap_err();
    assert!(
        matches!(&error, ReviseError::Order { order: 10, .. }),
        "{error}"
    );
    assert!(error.to_string().contains("1000000 points"), "{error}");
}

#[test]
fn a_dense_note_can_still_be_retouched() {
    // Its own points do not count.
    let mut stroke = row(1).remove(0);
    let point = stroke.points[0];
    stroke.points = vec![point; 1_000_001];
    let strokes = [stroke];
    let revised = apply(
        &input(&strokes),
        &parse(r#"[{"op":"move","select":{},"dx":1,"dy":0}]"#),
    )
    .unwrap();
    assert_eq!(revised.strokes[0].points.len(), 1_000_001);
}

#[test]
fn shapes_larger_than_5_m_are_refused() {
    for shape in [
        r#"{"line":{"from":[0,0],"to":[5001,0]}}"#,
        r#"{"polyline":{"points":[[0,0],[0,5001]]}}"#,
        r#"{"curve":{"points":[[0,0],[0,5001]]}}"#,
        r#"{"rect":{"x":0,"y":0,"w":5001,"h":1}}"#,
        r#"{"circle":{"c":[0,0],"r":2501}}"#,
        r#"{"ellipse":{"c":[0,0],"rx":1,"ry":2501}}"#,
        r#"{"arrow":{"from":[0,0],"to":[5001,0]}}"#,
        r#"{"dashed":{"from":[0,0],"to":[5001,0],"dash":1,"gap":1}}"#,
    ] {
        let message = refused(&[], &add(shape), 0);
        assert!(message.contains("larger than 5000"), "{shape}: {message}");
    }
    assert!(
        apply(
            &input(&[]),
            &parse(&add(r#"{"line":{"from":[0,0],"to":[4999,0]}}"#))
        )
        .is_ok()
    );
}

#[test]
fn a_dash_or_a_gap_under_0_2_mm_is_refused() {
    for (dash, gap) in [(0.19, 1.0), (1.0, 0.19), (0.0, 1.0), (1.0, -1.0)] {
        let shape =
            format!(r#"{{"dashed":{{"from":[0,0],"to":[50,0],"dash":{dash},"gap":{gap}}}}}"#);
        let message = refused(&[], &add(&shape), 0);
        assert!(message.contains("at least 0.2"), "{shape}: {message}");
    }
    let shape = r#"{"dashed":{"from":[0,0],"to":[50,0],"dash":0.2,"gap":0.2}}"#;
    assert!(apply(&input(&[]), &parse(&add(shape))).is_ok());
}

#[test]
fn a_sigma_over_5_mm_is_refused() {
    for order in [
        r#"{"op":"smooth","select":{},"sigma":5.01}"#,
        r#"{"op":"smooth","select":{},"sigma":0}"#,
        r#"{"op":"tidy_line","select":{},"smooth":5.01}"#,
    ] {
        let message = refused(&row(1), &format!("[{order}]"), 0);
        assert!(message.contains("at most 5"), "{order}: {message}");
    }
    assert!(
        apply(
            &input(&row(1)),
            &parse(r#"[{"op":"smooth","select":{},"sigma":5}]"#)
        )
        .is_ok()
    );
}

#[test]
fn a_factor_outside_0_05_to_20_is_refused() {
    for order in [
        r#"{"op":"scale","select":{},"factor":0.04,"origin":[0,0]}"#,
        r#"{"op":"scale","select":{},"factor":21,"origin":[0,0]}"#,
        r#"{"op":"scale","select":{},"factor":-1,"origin":[0,0]}"#,
        r#"{"op":"scale","select":{},"sx":0.04,"sy":1,"origin":[0,0]}"#,
        r#"{"op":"scale","select":{},"sx":1,"sy":21,"origin":[0,0]}"#,
        r#"{"op":"copy","select":{},"factor":21}"#,
        r#"{"op":"glyph","parts":[{"stroke":0,"sx":21,"x":0,"y":0}]}"#,
        r#"{"op":"glyph","parts":[{"stroke":0,"sy":0.04,"x":0,"y":0}]}"#,
        r#"{"op":"tidy_line","select":{},"scale":21}"#,
    ] {
        let message = refused(&row(1), &format!("[{order}]"), 0);
        assert!(
            message.contains("between 0.05 and 20"),
            "{order}: {message}"
        );
    }
    assert!(
        apply(
            &input(&row(1)),
            &parse(r#"[{"op":"scale","select":{},"factor":0.05,"origin":[0,0]}]"#)
        )
        .is_ok()
    );
}

#[test]
fn a_pen_width_and_a_slope_out_of_reason_are_refused() {
    for (order, expected) in [
        (r#"{"op":"restyle","select":{},"width":0}"#, "above 0"),
        (r#"{"op":"restyle","select":{},"width":101}"#, "at most 100"),
        (r#"{"op":"tidy_line","select":{},"slope":46}"#, "steeper"),
        (
            r#"{"op":"tidy_line","select":{},"word_gap":-1}"#,
            "negative",
        ),
    ] {
        let message = refused(&row(1), &format!("[{order}]"), 0);
        assert!(message.contains(expected), "{order}: {message}");
    }
    let message = refused(
        &row(1),
        &format!(r##"[{{"op":"add","color":"#000000","width":101,"shapes":[{LINE}]}}]"##),
        0,
    );
    assert!(message.contains("at most 100"), "{message}");
}

#[test]
fn colours_that_cannot_be_read_are_refused() {
    for order in [
        r#"{"op":"recolor","select":{},"color":"black"}"#,
        r##"{"op":"replace","select":{},"color":"#000","with":[{"line":{"from":[0,0],"to":[1,1]}}]}"##,
        r#"{"op":"copy","select":{},"color":"000000"}"#,
        r##"{"op":"glyph","color":"#00000","parts":[{"stroke":0,"x":0,"y":0}]}"##,
        r##"{"op":"add","color":"#gggggg","shapes":[{"line":{"from":[0,0],"to":[1,1]}}]}"##,
    ] {
        let message = refused(&row(1), &format!("[{order}]"), 0);
        assert!(message.contains("#rrggbb"), "{order}: {message}");
    }
}

#[test]
fn the_error_comes_from_the_order_that_is_wrong_and_nothing_comes_out() {
    let message = refused(
        &row(2),
        r#"[{"op":"delete","select":{"ids":[0]}},{"op":"move","select":{},"dx":1e39,"dy":0}]"#,
        1,
    );
    assert!(message.starts_with("order 1 (move): "), "{message}");
}

#[test]
fn unknown_fields_and_orders_are_refused_when_parsing() {
    for json in [
        r#"{"op":"delete","select":{},"extra":1}"#,
        r#"{"op":"delete","select":{"extra":1}}"#,
        r#"{"op":"fly","select":{}}"#,
        r#"{"select":{}}"#,
        r#"{"op":"move","select":{},"dx":1}"#,
        r##"{"op":"add","color":"#000000","shapes":[{"line":{"from":[0,0],"to":[1,1],"extra":1}}]}"##,
        r##"{"op":"add","color":"#000000","shapes":[{"triangle":{}}]}"##,
        r#"{"op":"glyph","parts":[{"stroke":0,"x":0,"y":0,"id":3}]}"#,
        r#"{"op":"glyph","parts":[{"stroke":0,"flip":"x","x":0,"y":0}]}"#,
        r#"{"op":"tidy_line","select":{},"mode":"line"}"#,
        r#"{"op":"tidy_line","select":{},"gaps":"wide"}"#,
        r#"{"op":"restyle","select":{},"tool":"crayon"}"#,
    ] {
        assert!(serde_json::from_str::<Order>(json).is_err(), "{json}");
    }
}

#[test]
fn a_polyline_or_a_curve_has_a_limited_number_of_points() {
    for name in ["polyline", "curve"] {
        let points = vec![[1.0f32, 1.0]; MAX_SHAPE_VERTICES + 1];
        let shape = match name {
            "polyline" => Shape::Polyline {
                points,
                closed: false,
            },
            _ => Shape::Curve { points },
        };
        let order = Order::Add {
            shapes: vec![shape],
            color: "#000000".to_owned(),
            width: None,
            tool: None,
            layer: None,
            behind: false,
            after: None,
        };
        let message = apply(&input(&row(1)), &[order]).unwrap_err().to_string();
        assert!(message.contains("at most 100000"), "{name}: {message}");
    }
}

#[test]
fn a_curve_is_counted_before_it_is_built() {
    // Within the limit on its points, each stretch long enough for 400 pieces: 16,000,000 vertices.
    let points: Vec<[f32; 2]> = (0..40_000)
        .map(|i| [if i % 2 == 0 { 0.0 } else { 500.0 }, 0.0])
        .collect();
    let order = Order::Add {
        shapes: vec![Shape::Curve { points }],
        color: "#000000".to_owned(),
        width: None,
        tool: None,
        layer: None,
        behind: false,
        after: None,
    };
    let message = apply(&input(&row(1)), &[order]).unwrap_err().to_string();
    assert!(message.contains("more than 1000000 points"), "{message}");
}
