//! Tests of the orders, in JSON as an assistant writes them. All on synthetic strokes.

mod limits;
mod lines;
mod orders;
mod schema;
mod select;
mod shapes;
mod tidy;
mod time;

use ardoise_ink::{Point, Rgba, Stroke, Tool};

use super::*;

pub(super) const PAGE: [f32; 2] = [300.0, 200.0];

/// A point in millimetres.
pub(super) fn pt(x: f32, y: f32, t: u32) -> Point {
    Point {
        x: x * 100.0,
        y: y * 100.0,
        pressure: 0.5,
        t,
    }
}

/// A black ballpoint stroke of `n` samples from `from` to `to`, evenly spread from `t0` to `t1` ms.
pub(super) fn line_stroke(from: [f32; 2], to: [f32; 2], t0: u32, t1: u32) -> Stroke {
    let n = 11u32;
    let points = (0..n)
        .map(|i| {
            let u = i as f32 / (n - 1) as f32;
            pt(
                from[0] + (to[0] - from[0]) * u,
                from[1] + (to[1] - from[1]) * u,
                t0 + (t1 - t0) * i / (n - 1),
            )
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

/// `count` short strokes side by side, one every second: strokes 0, 1, 2... at x = 10, 20, 30...
pub(super) fn row(count: usize) -> Vec<Stroke> {
    (0..count)
        .map(|i| {
            let x = 10.0 * (i as f32 + 1.0);
            let t = 1000 * i as u32;
            line_stroke([x, 20.0], [x + 5.0, 20.0], t, t + 500)
        })
        .collect()
}

pub(super) fn input(strokes: &[Stroke]) -> Input<'_> {
    Input {
        strokes,
        page_mm: PAGE,
        layers: 2,
        narration: false,
    }
}

pub(super) fn parse(json: &str) -> Vec<Order> {
    serde_json::from_str(json).expect("orders in JSON")
}

pub(super) fn run(strokes: &[Stroke], json: &str) -> Revised {
    run_on(&input(strokes), json)
}

pub(super) fn run_on(input: &Input, json: &str) -> Revised {
    let before = input.strokes.to_vec();
    let revised = apply(input, &parse(json)).expect("orders accepted");
    assert_eq!(before, input.strokes, "the source is never changed");
    assert_eq!(revised.strokes.len(), revised.numbers.len());
    assert_eq!(revised.strokes.len(), revised.report.strokes_after);
    revised
}

/// The message of the error, which names order `order`.
pub(super) fn refused(strokes: &[Stroke], json: &str, order: usize) -> String {
    let error = apply(&input(strokes), &parse(json)).expect_err("orders refused");
    assert!(
        matches!(&error, ReviseError::Order { order: found, .. } if *found == order),
        "{error}"
    );
    error.to_string()
}

impl Revised {
    /// The stroke of this number in the result.
    pub(super) fn stroke(&self, number: usize) -> &Stroke {
        let at = self
            .numbers
            .iter()
            .position(|&n| n == number)
            .unwrap_or_else(|| panic!("no stroke {number} in {:?}", self.numbers));
        &self.strokes[at]
    }

    pub(super) fn has(&self, number: usize) -> bool {
        self.numbers.contains(&number)
    }
}

/// `[x0, y0, x1, y1]` of a stroke, millimetres.
pub(super) fn box_of(stroke: &Stroke) -> [f32; 4] {
    let area = crate::geom::area_of(&stroke.points).expect("samples");
    [area.x0, area.y0, area.x1, area.y1]
}

#[track_caller]
pub(super) fn assert_near(found: f32, expected: f32, tolerance: f32) {
    assert!(
        (found - expected).abs() <= tolerance,
        "{found} is not within {tolerance} of {expected}"
    );
}

#[track_caller]
pub(super) fn assert_box(stroke: &Stroke, expected: [f32; 4]) {
    let found = box_of(stroke);
    for (a, b) in found.iter().zip(expected) {
        assert!((a - b).abs() < 0.01, "{found:?} is not {expected:?}");
    }
}

/// True when no point of a stroke comes before the one it follows.
#[track_caller]
pub(super) fn assert_monotonic(stroke: &Stroke) {
    for pair in stroke.points.windows(2) {
        assert!(pair[0].t <= pair[1].t, "{} then {}", pair[0].t, pair[1].t);
    }
}
