//! Shapes to strokes: vertices in millimetres, then samples along them.

use ardoise_ink::shapes::SAMPLE_SPACING;
use ardoise_ink::{Point, Rgba, Stroke, Tool};

use crate::geom::UNITS;
use crate::orders::Shape;

/// A point in millimetres.
pub(crate) type P = [f32; 2];

/// Distance between the samples of a shape, millimetres: the editor's straightening's.
const STEP_MM: f32 = (SAMPLE_SPACING / 100.0) as f32;
/// Length of an arrow's barbs when the order does not say, millimetres (the trial's; supposé).
const DEFAULT_HEAD_MM: f32 = 3.5;
/// Angle between an arrow's shaft and each barb (the trial's; supposé).
const HEAD_SPREAD_DEG: f32 = 24.0;
/// Longest straight piece of an ellipse before sampling, millimetres.
const ELLIPSE_CHORD_MM: f32 = 0.8;
const ELLIPSE_MIN_VERTICES: usize = 16;
/// How a curve is cut between two of its points: about one piece per millimetre, within these.
const CURVE_MIN_PIECES: usize = 8;
const CURVE_MAX_PIECES: usize = 400;

/// The pen of the strokes made from shapes.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Style {
    pub tool: Tool,
    pub color: Rgba,
    /// Device units.
    pub width: f32,
    pub layer: u32,
    pub pressure: f32,
}

fn distance(a: P, b: P) -> f32 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

pub(crate) fn length(path: &[P]) -> f32 {
    path.windows(2).map(|w| distance(w[0], w[1])).sum()
}

/// The two barbs of an arrow head at `tip`, coming from `tail`.
fn barbs(tail: P, tip: P, head: f32) -> (P, P) {
    let angle = (tail[1] - tip[1]).atan2(tail[0] - tip[0]);
    let spread = HEAD_SPREAD_DEG.to_radians();
    let barb = |sign: f32| {
        let a = angle + sign * spread;
        [tip[0] + head * a.cos(), tip[1] + head * a.sin()]
    };
    (barb(1.0), barb(-1.0))
}

fn ellipse(c: P, rx: f32, ry: f32, degrees: f32) -> Vec<P> {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let vertices = ((rx.max(ry) * std::f32::consts::TAU / ELLIPSE_CHORD_MM).ceil() as usize)
        .max(ELLIPSE_MIN_VERTICES);
    (0..=vertices)
        .map(|i| {
            let t = i as f32 / vertices as f32 * std::f32::consts::TAU;
            let (x, y) = (rx * t.cos(), ry * t.sin());
            [c[0] + x * cos - y * sin, c[1] + x * sin + y * cos]
        })
        .collect()
}

/// How many pieces a curve is cut into between two of its points.
fn curve_pieces(a: P, b: P) -> usize {
    (distance(a, b).ceil() as usize).clamp(CURVE_MIN_PIECES, CURVE_MAX_PIECES)
}

/// Catmull-Rom through the points (end points counted twice).
fn curve(points: &[P]) -> Vec<P> {
    let mut out = Vec::new();
    let at = |i: isize| {
        let last = points.len() as isize - 1;
        points
            .get(i.clamp(0, last.max(0)) as usize)
            .copied()
            .unwrap_or_default()
    };
    for i in 0..points.len() as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let pieces = curve_pieces(p1, p2);
        for piece in 0..pieces {
            let t = piece as f32 / pieces as f32;
            let blend = |a: f32, b: f32, c: f32, d: f32| {
                0.5 * (2.0 * b
                    + (c - a) * t
                    + (2.0 * a - 5.0 * b + 4.0 * c - d) * t * t
                    + (3.0 * b - a - 3.0 * c + d) * t * t * t)
            };
            out.push([
                blend(p0[0], p1[0], p2[0], p3[0]),
                blend(p0[1], p1[1], p2[1], p3[1]),
            ]);
        }
    }
    if let Some(&last) = points.last() {
        out.push(last);
    }
    out
}

/// The vertices of a checked shape (see `validate`): one list per stroke.
pub(crate) fn paths(shape: &Shape) -> Vec<Vec<P>> {
    match shape {
        Shape::Line { from, to } => vec![vec![*from, *to]],
        Shape::Polyline { points, closed } => {
            let mut path = points.clone();
            if let (true, Some(&first)) = (*closed, points.first()) {
                path.push(first);
            }
            vec![path]
        }
        Shape::Rect { x, y, w, h } => vec![vec![
            [*x, *y],
            [x + w, *y],
            [x + w, y + h],
            [*x, y + h],
            [*x, *y],
        ]],
        Shape::Circle { c, r } => vec![ellipse(*c, *r, *r, 0.0)],
        Shape::Ellipse { c, rx, ry, angle } => vec![ellipse(*c, *rx, *ry, *angle)],
        Shape::Arrow {
            from,
            to,
            head,
            both,
        } => {
            let size = head.unwrap_or(DEFAULT_HEAD_MM);
            let (a, b) = barbs(*from, *to, size);
            let mut path = Vec::with_capacity(9);
            if *both {
                let (c, d) = barbs(*to, *from, size);
                path.extend([c, *from, d]);
            }
            path.extend([*from, *to, a, *to, b]);
            vec![path]
        }
        Shape::Dashed {
            from,
            to,
            dash,
            gap,
            head,
        } => {
            let len = distance(*from, *to);
            let (ux, uy) = ((to[0] - from[0]) / len, (to[1] - from[1]) / len);
            let along = |d: f32| [from[0] + ux * d, from[1] + uy * d];
            let mut out = Vec::new();
            let mut d = 0.0;
            while d < len {
                let end = (d + dash).min(len);
                out.push(vec![along(d), along(end)]);
                d = end + gap;
            }
            if let Some(size) = head {
                let (a, b) = barbs(*from, *to, *size);
                out.push(vec![a, *to, b]);
                let (c, e) = barbs(*to, *from, *size);
                out.push(vec![c, *from, e]);
            }
            out
        }
        Shape::Curve { points } => vec![curve(points)],
    }
}

/// The fewest samples a checked shape gives, known without building it (each vertex is a sample):
/// the shapes with an input array are the ones whose vertices can be many, whatever their size.
pub(crate) fn least_samples(shape: &Shape) -> usize {
    match shape {
        Shape::Polyline { points, .. } => points.len(),
        Shape::Curve { points } => {
            1 + points
                .windows(2)
                .map(|w| curve_pieces(w[0], w[1]))
                .sum::<usize>()
        }
        _ => 0,
    }
}

/// How many samples [`sampled`] gives for `path`.
pub(crate) fn sample_count(path: &[P]) -> usize {
    1 + path
        .windows(2)
        .map(|w| ((distance(w[0], w[1]) / STEP_MM).ceil() as usize).max(1))
        .sum::<usize>()
}

/// Samples along the path, about [`STEP_MM`] apart; its vertices are among them, so corners stay.
fn sampled(path: &[P]) -> Vec<P> {
    let mut out = Vec::with_capacity(sample_count(path));
    out.extend(path.first().copied());
    for w in path.windows(2) {
        let pieces = ((distance(w[0], w[1]) / STEP_MM).ceil() as usize).max(1);
        for i in 1..=pieces {
            let k = i as f32 / pieces as f32;
            out.push([
                w[0][0] + (w[1][0] - w[0][0]) * k,
                w[0][1] + (w[1][1] - w[0][1]) * k,
            ]);
        }
    }
    out
}

/// A stroke along `path` from `start` to `end` ms, its times following the distance travelled.
pub(crate) fn stroke_along(path: &[P], style: &Style, start: u32, end: u32) -> Stroke {
    let samples = sampled(path);
    let total: f64 = samples
        .windows(2)
        .map(|w| f64::from(distance(w[0], w[1])))
        .sum();
    let span = f64::from(end.saturating_sub(start));
    let mut travelled = 0.0f64;
    let mut previous: Option<P> = None;
    let points = samples
        .iter()
        .map(|&p| {
            if let Some(q) = previous {
                travelled += f64::from(distance(q, p));
            }
            previous = Some(p);
            let k = if total > 0.0 {
                (travelled / total).min(1.0)
            } else {
                0.0
            };
            Point {
                x: p[0] * UNITS,
                y: p[1] * UNITS,
                pressure: style.pressure,
                t: start.saturating_add((span * k).round() as u32),
            }
        })
        .collect();
    Stroke {
        tool: style.tool,
        color: style.color,
        width: style.width,
        layer: style.layer,
        points,
    }
}
