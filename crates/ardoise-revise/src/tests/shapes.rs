//! Shapes to strokes.

use super::*;

fn made(shapes: &str) -> Revised {
    run(
        &[],
        &format!(r##"[{{"op":"add","color":"#000000","shapes":[{shapes}]}}]"##),
    )
}

fn mm(point: &Point) -> [f32; 2] {
    [point.x / 100.0, point.y / 100.0]
}

/// True when `path` (samples) goes through this point.
fn goes_through(stroke: &Stroke, x: f32, y: f32) -> bool {
    stroke
        .points
        .iter()
        .any(|p| (p.x / 100.0 - x).abs() < 0.02 && (p.y / 100.0 - y).abs() < 0.02)
}

/// The samples are never much further apart than the editor's straightening's.
#[track_caller]
fn assert_dense(stroke: &Stroke) {
    for pair in stroke.points.windows(2) {
        let distance = (pair[1].x - pair[0].x).hypot(pair[1].y - pair[0].y);
        assert!(distance <= 51.0, "{distance}");
    }
}

#[test]
fn a_line_is_one_dense_stroke() {
    let revised = made(r#"{"line":{"from":[10,10],"to":[40,50]}}"#);
    let stroke = revised.stroke(0);
    assert_eq!(revised.strokes.len(), 1);
    assert_eq!(mm(&stroke.points[0]), [10.0, 10.0]);
    assert_eq!(mm(stroke.points.last().unwrap()), [40.0, 50.0]);
    assert_dense(stroke);
}

#[test]
fn a_polyline_keeps_its_corners_and_can_close() {
    let revised = made(r#"{"polyline":{"points":[[10,10],[40,10],[40,50]]}}"#);
    let stroke = revised.stroke(0);
    assert!(goes_through(stroke, 40.0, 10.0));
    assert_eq!(mm(stroke.points.last().unwrap()), [40.0, 50.0]);

    let revised = made(r#"{"polyline":{"points":[[10,10],[40,10],[40,50]],"closed":true}}"#);
    assert_eq!(mm(revised.stroke(0).points.last().unwrap()), [10.0, 10.0]);
}

#[test]
fn a_rect_goes_round() {
    let revised = made(r#"{"rect":{"x":10,"y":20,"w":30,"h":40}}"#);
    let stroke = revised.stroke(0);
    assert_box(stroke, [10.0, 20.0, 40.0, 60.0]);
    for [x, y] in [[10.0, 20.0], [40.0, 20.0], [40.0, 60.0], [10.0, 60.0]] {
        assert!(goes_through(stroke, x, y), "{x} {y}");
    }
    assert_eq!(mm(stroke.points.last().unwrap()), [10.0, 20.0]);
}

#[test]
fn circles_and_ellipses_stay_on_their_curve() {
    let revised = made(r#"{"circle":{"c":[50,50],"r":10}}"#);
    let stroke = revised.stroke(0);
    assert_box(stroke, [40.0, 40.0, 60.0, 60.0]);
    for p in &stroke.points {
        let [x, y] = mm(p);
        assert_near((x - 50.0).hypot(y - 50.0), 10.0, 0.02);
    }

    let revised = made(r#"{"ellipse":{"c":[50,50],"rx":20,"ry":5}}"#);
    assert_box(revised.stroke(0), [30.0, 45.0, 70.0, 55.0]);

    // Turned by a quarter, clockwise: what went across goes up and down.
    let revised = made(r#"{"ellipse":{"c":[50,50],"rx":20,"ry":5,"angle":90}}"#);
    assert_box(revised.stroke(0), [45.0, 30.0, 55.0, 70.0]);
}

#[test]
fn an_arrow_is_one_stroke_tail_tip_barb_tip_barb() {
    let revised = made(r#"{"arrow":{"from":[10,50],"to":[60,50],"head":5}}"#);
    assert_eq!(revised.strokes.len(), 1);
    let stroke = revised.stroke(0);
    assert_eq!(mm(&stroke.points[0]), [10.0, 50.0]);
    let tip_visits = stroke
        .points
        .iter()
        .filter(|p| (p.x / 100.0 - 60.0).abs() < 0.001 && (p.y / 100.0 - 50.0).abs() < 0.001)
        .count();
    assert_eq!(
        tip_visits, 2,
        "the pen comes back to the tip for the other barb"
    );
    let last = mm(stroke.points.last().unwrap());
    let reach = (last[0] - 60.0).hypot(last[1] - 50.0);
    assert_near(reach, 5.0, 0.01);
    assert!(last[0] < 60.0, "barbs point back");
    // Both barbs, one on each side.
    let area = box_of(stroke);
    assert_near(area[1], 50.0 - area[3] + 50.0, 0.01);
    assert!(area[3] > 51.0);
}

#[test]
fn an_arrow_with_two_heads_is_one_stroke_too() {
    let revised = made(r#"{"arrow":{"from":[10,50],"to":[60,50],"both":true}}"#);
    assert_eq!(revised.strokes.len(), 1);
    let area = box_of(revised.stroke(0));
    assert_near(area[0], 10.0, 0.01);
    assert_near(area[2], 60.0, 0.01);
    assert!(goes_through(revised.stroke(0), 10.0, 50.0));
    assert!(goes_through(revised.stroke(0), 60.0, 50.0));
}

#[test]
fn a_dashed_line_is_one_stroke_per_dash() {
    let revised = made(r#"{"dashed":{"from":[10,50],"to":[20,50],"dash":2,"gap":1}}"#);
    // 10-12, 13-15, 16-18, 19-20.
    assert_eq!(revised.strokes.len(), 4);
    assert_box(revised.stroke(1), [13.0, 50.0, 15.0, 50.0]);
    assert_box(revised.stroke(3), [19.0, 50.0, 20.0, 50.0]);
    assert_eq!(revised.report.orders[0].new_strokes, [0, 1, 2, 3]);

    let revised = made(r#"{"dashed":{"from":[10,50],"to":[20,50],"dash":2,"gap":1,"head":2}}"#);
    assert_eq!(revised.strokes.len(), 6, "and one per head");
    assert!(goes_through(revised.stroke(4), 20.0, 50.0));
    assert!(goes_through(revised.stroke(5), 10.0, 50.0));
}

#[test]
fn a_curve_goes_through_its_points() {
    let revised = made(r#"{"curve":{"points":[[10,10],[30,40],[60,20]]}}"#);
    let stroke = revised.stroke(0);
    assert_eq!(revised.strokes.len(), 1);
    assert!(goes_through(stroke, 30.0, 40.0));
    assert_eq!(mm(stroke.points.last().unwrap()), [60.0, 20.0]);
    assert_dense(stroke);
}

#[test]
fn degenerate_shapes_are_refused() {
    for (shape, expected) in [
        (r#"{"polyline":{"points":[[1,1]]}}"#, "at least 2"),
        (r#"{"curve":{"points":[]}}"#, "at least 2"),
        (r#"{"rect":{"x":1,"y":1,"w":0,"h":0}}"#, "no size"),
        (r#"{"circle":{"c":[1,1],"r":0}}"#, "above 0"),
        (r#"{"ellipse":{"c":[1,1],"rx":1,"ry":-1}}"#, "above 0"),
        (r#"{"arrow":{"from":[1,1],"to":[1,1]}}"#, "itself"),
        (r#"{"arrow":{"from":[1,1],"to":[2,1],"head":0}}"#, "head"),
        (
            r#"{"dashed":{"from":[1,1],"to":[1,1],"dash":1,"gap":1}}"#,
            "itself",
        ),
    ] {
        let json = format!(r##"[{{"op":"add","color":"#000000","shapes":[{shape}]}}]"##);
        let message = refused(&[], &json, 0);
        assert!(message.contains(expected), "{shape}: {message}");
    }
    let message = refused(&[], r##"[{"op":"add","color":"#000000","shapes":[]}]"##, 0);
    assert!(message.contains("no shape"), "{message}");
    let message = refused(&row(1), r#"[{"op":"replace","select":{},"with":[]}]"#, 0);
    assert!(message.contains("no shape"), "{message}");
}
