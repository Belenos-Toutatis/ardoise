//! Strokes as the assistant sees them: a summary per stroke, and polylines in millimetres
//! simplified (Douglas–Peucker) to a budget of points.

use ardoise_ink::Stroke;
use serde::Serialize;
use serde_json::Value;

use crate::notes::{RectMm, hex, mm};

/// Default and largest budget of points of `get_note` in `points` mode, over the whole note.
pub const DEFAULT_MAX_POINTS: usize = 5_000;
pub const MAX_POINTS: usize = 50_000;

/// One stroke of a note.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StrokeInfo {
    /// Position in the note's list of strokes (what `get_note_text` gives for drawings).
    pub index: usize,
    pub tool: Value,
    /// `#rrggbb`, or `#rrggbbaa` when translucent.
    pub color: String,
    /// Nominal width at full pressure.
    pub width_mm: f64,
    /// Index of the layer.
    pub layer: u32,
    pub point_count: usize,
    /// The rectangle the sample points cover (the line's own width not included).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds_mm: Option<RectMm>,
    /// Milliseconds after the creation of the note.
    pub start_ms: u32,
    pub end_ms: u32,
    /// With `points` detail: the simplified polyline `[x, y]` in millimetres (absent for a
    /// stroke left out when the budget of points ran out).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub points_mm: Option<Vec<[f64; 2]>>,
}

impl StrokeInfo {
    pub fn of(index: usize, stroke: &Stroke) -> StrokeInfo {
        let bounds = stroke
            .points
            .iter()
            .fold(None, |bounds, point| match bounds {
                None => Some((point.x, point.y, point.x, point.y)),
                Some((x0, y0, x1, y1)) => Some((
                    x0.min(point.x),
                    y0.min(point.y),
                    x1.max(point.x),
                    y1.max(point.y),
                )),
            });
        StrokeInfo {
            index,
            tool: serde_json::to_value(stroke.tool).unwrap_or(Value::Null),
            color: hex(stroke.color),
            width_mm: mm(stroke.width),
            layer: stroke.layer,
            point_count: stroke.points.len(),
            bounds_mm: bounds.map(|(x0, y0, x1, y1)| RectMm {
                x: mm(x0),
                y: mm(y0),
                width: mm(x1 - x0),
                height: mm(y1 - y0),
            }),
            start_ms: stroke.points.first().map_or(0, |p| p.t),
            end_ms: stroke.points.last().map_or(0, |p| p.t),
            points_mm: None,
        }
    }
}

/// Distance from `p` to the segment `a`–`b`.
fn distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let length2 = dx * dx + dy * dy;
    let t = if length2 == 0.0 {
        0.0
    } else {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / length2).clamp(0.0, 1.0)
    };
    let (x, y) = (a[0] + t * dx, a[1] + t * dy);
    ((p[0] - x).powi(2) + (p[1] - y).powi(2)).sqrt()
}

/// The positions in `points` that Douglas–Peucker keeps for `tolerance`: the ends, and every
/// point that is farther than that from the line between the points kept around it.
pub fn simplify(points: &[[f64; 2]], tolerance: f64) -> Vec<usize> {
    let n = points.len();
    if n <= 2 {
        return (0..n).collect();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut pending = vec![(0, n - 1)];
    while let Some((first, last)) = pending.pop() {
        let (mut far, mut far_distance) = (first, 0.0);
        for (i, point) in points.iter().enumerate().take(last).skip(first + 1) {
            let d = distance(*point, points[first], points[last]);
            if d > far_distance {
                (far, far_distance) = (i, d);
            }
        }
        if far_distance > tolerance {
            keep[far] = true;
            pending.push((first, far));
            pending.push((far, last));
        }
    }
    keep.iter()
        .enumerate()
        .filter_map(|(i, &kept)| kept.then_some(i))
        .collect()
}

/// The polylines of `strokes` in millimetres, simplified with the smallest tolerance for which
/// the whole holds at most `budget` points (a stroke keeps its two ends at least). When even
/// the coarsest simplification is over budget, the strokes that come last are left out (`None`).
pub fn polylines(strokes: &[Stroke], budget: usize) -> Vec<Option<Vec<[f64; 2]>>> {
    let lines: Vec<Vec<[f64; 2]>> = strokes
        .iter()
        .map(|stroke| {
            stroke
                .points
                .iter()
                .map(|p| [f64::from(p.x) * 0.01, f64::from(p.y) * 0.01])
                .collect()
        })
        .collect();
    let round = |value: f64| (value * 10.0).round() / 10.0;
    let kept_at = |tolerance: f64| -> Vec<Vec<usize>> {
        lines.iter().map(|line| simplify(line, tolerance)).collect()
    };
    let mut tolerance = 0.05;
    let mut kept = kept_at(tolerance);
    // Doubling reaches the page's diagonal in a dozen rounds.
    while kept.iter().map(Vec::len).sum::<usize>() > budget && tolerance < 1_000.0 {
        tolerance *= 2.0;
        kept = kept_at(tolerance);
    }
    let mut left = budget;
    lines
        .iter()
        .zip(kept)
        .map(|(line, kept)| {
            if kept.len() > left {
                left = 0;
                return None;
            }
            left -= kept.len();
            Some(
                kept.into_iter()
                    .map(|i| [round(line[i][0]), round(line[i][1])])
                    .collect(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ardoise_ink::{Point, Rgba, Tool};

    use super::*;

    fn stroke(points: &[(f32, f32)]) -> Stroke {
        Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 40.0,
            layer: 0,
            points: points
                .iter()
                .enumerate()
                .map(|(i, &(x, y))| Point {
                    x,
                    y,
                    pressure: 1.0,
                    t: i as u32 * 10,
                })
                .collect(),
        }
    }

    #[test]
    fn a_straight_line_keeps_its_two_ends() {
        let line: Vec<[f64; 2]> = (0..50)
            .map(|i| [f64::from(i), 2.0 * f64::from(i)])
            .collect();
        assert_eq!(simplify(&line, 0.01), vec![0, 49]);
    }

    #[test]
    fn a_corner_is_kept() {
        let line = [
            [0.0, 0.0],
            [5.0, 0.0],
            [10.0, 0.0],
            [10.0, 5.0],
            [10.0, 10.0],
        ];
        assert_eq!(simplify(&line, 0.1), vec![0, 2, 4]);
    }

    #[test]
    fn the_budget_of_points_is_respected() {
        // 40 wavy strokes of 200 points each.
        let strokes: Vec<Stroke> = (0..40)
            .map(|s| {
                let points: Vec<(f32, f32)> = (0..200)
                    .map(|i| {
                        (
                            i as f32 * 20.0,
                            (i as f32 * 0.3).sin() * 300.0 + s as f32 * 100.0,
                        )
                    })
                    .collect();
                stroke(&points)
            })
            .collect();
        for budget in [100, 1_000, 5_000] {
            let lines = polylines(&strokes, budget);
            let total: usize = lines.iter().flatten().map(Vec::len).sum();
            assert!(total <= budget, "{total} > {budget}");
            assert!(lines.iter().flatten().all(|l| l.len() >= 2));
        }
        // 40 strokes need 80 points at least: with 50, the last ones are left out.
        let lines = polylines(&strokes, 50);
        assert!(lines.iter().any(Option::is_none));
        assert!(lines[0].is_some());
    }

    #[test]
    fn a_stroke_is_summarised_in_millimetres() {
        let info = StrokeInfo::of(3, &stroke(&[(100.0, 200.0), (1_100.0, 700.0)]));
        assert_eq!(info.index, 3);
        assert_eq!((info.width_mm, info.point_count, info.end_ms), (0.4, 2, 10));
        assert_eq!(
            info.bounds_mm,
            Some(RectMm {
                x: 1.0,
                y: 2.0,
                width: 10.0,
                height: 5.0
            })
        );
        assert_eq!(info.tool, "ballpoint");
    }
}
