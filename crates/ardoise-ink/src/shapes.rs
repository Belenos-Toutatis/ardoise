//! Straightening hand-drawn shapes ("Redresser les formes", SPEC §4.4).
//!
//! [`straighten`] looks at strokes (the editor's selection) and replaces the ones that are a
//! clear shape by a clean version of it:
//!
//! - a **straight line**;
//! - a **circle or ellipse**, possibly turned;
//! - a **rectangle**, possibly turned;
//! - an **arrow**: a shaft and a two-sided head, drawn in one stroke (tail → tip → barb → back
//!   to the tip → other barb, or the other way round) or in several strokes (a line and a
//!   "V" near one of its ends, a line and two separate barbs, a line ending with one barb and
//!   the other barb on its own…).
//!
//! Anything else is left exactly as it is. Handwriting must never be straightened, so the
//! recognition is conservative: a shape must be large ([`MIN_SIZE`]), fit its ideal closely,
//! and a closed shape must go round exactly once. Arrows need exactly one long shaft and two
//! barbs pointing back along it on both sides, which letters such as V, Y, K, N, T, A or 4 never
//! give.
//!
//! # The straightened strokes
//!
//! Each new stroke comes from one original stroke and keeps its tool, colour, width and layer.
//! Its pressure is the median of the original pressures, the same at every point, so that the
//! line keeps its look. Its points are spread over the original's duration in proportion to
//! the distance along the shape, so that the replay still draws it. Points are dense (every
//! [`SAMPLE_SPACING`] device units at most) so that the replay is smooth and the outline round;
//! the corners of rectangles and arrows are samples where the line turns by more than 60°,
//! which the outline geometry keeps sharp (see the `geometry` module). Closed shapes end
//! exactly where they start, at the corner (or the angle round the ellipse) nearest to where
//! the pen landed, and go round in the drawing direction.
//!
//! # Thresholds
//!
//! Sizes are in device units (10 µm); relative tolerances are fractions of the shape's size.
//!
//! | What | Value | Meaning |
//! |---|---|---|
//! | [`MIN_SIZE`] | 1000 (10 mm) | smallest line, closed shape (largest side) or arrow shaft |
//! | `CLOSE_GAP` | 0.2 | largest gap between the ends of a closed shape, × its bounding-box diagonal (a circle open by about 30° still counts) |
//! | `MIN_ASPECT` | 0.1 | thinnest closed shape (short side ÷ long side) |
//! | `FIT_MEAN` / `FIT_MAX` | 0.06 / 0.2 | mean and largest distance of the stroke (overshoot included) to the ideal ellipse or rectangle, × its mean radius (√(a·b), √(w·h)/2) |
//! | `RECT_COHERENCE` | 0.55 | how consistently the edges of a rectangle run along two perpendicular directions (1 = perfectly, 0 for a circle) |
//! | `SNAP_RATIO` | 0.9 | an ellipse or rectangle this close to round or square becomes a circle or a square |
//! | `SNAP_ANGLE` | 4° | a line, shaft, rectangle or ellipse axis this close to horizontal or vertical becomes exactly so |
//! | `LINE_MAX_DEV` / `LINE_RMS_DEV` | 0.06 / 0.025 | largest and root-mean-square distance to the straight line, × its length |
//! | `SEG_EPS` | 0.02 (≥ 25) | tolerance of the split of a stroke into straight pieces, × its bounding-box diagonal |
//! | `HOOK_SHARE` / `HOOK_MAX` | 0.08 / 200 | a piece at an end shorter than both (× the longest piece / units) is a pen hook, ignored |
//! | barbs | 0.04–0.55 × shaft, ≥ 250 | length of an arrow's barbs; at 12°–70° from the shaft, on both sides, starting within 0.35 × their length (≥ 120) of the tip |
//!
//! Everything is plain `f64` arithmetic: the result is deterministic.

use std::f64::consts::{PI, TAU};
use std::ops::{Add, Mul, Sub};

use crate::geometry::MAX_COORD;
use crate::model::{Point, Stroke};

/// Smallest shape that is straightened, in device units (10 mm): the length of a line or of an
/// arrow's shaft, the long side of a closed shape. Handwriting is smaller.
pub const MIN_SIZE: f64 = 1000.0;
/// Largest distance between two points of a straightened stroke, in device units (0.5 mm).
pub const SAMPLE_SPACING: f64 = 50.0;
/// Most points of one straightened stroke (the spacing grows for huge shapes).
const MAX_POINTS: usize = 4000;

/// Closing gap allowed, × the bounding-box diagonal.
const CLOSE_GAP: f64 = 0.2;
/// The closing point is looked for once this share of the path is drawn.
const CLOSE_SEARCH: f64 = 0.5;
/// Thinnest closed shape, short side ÷ long side.
const MIN_ASPECT: f64 = 0.1;
/// Mean and largest distance to the ideal closed shape, × its mean radius.
const FIT_MEAN: f64 = 0.06;
const FIT_MAX: f64 = 0.2;
/// Smallest edge-direction coherence of a rectangle.
const RECT_COHERENCE: f64 = 0.55;
/// Each side of a rectangle holds at least this share of the points its length would give.
const RECT_SIDE_SHARE: f64 = 0.4;
/// Ratio above which an ellipse becomes a circle and a rectangle a square.
const SNAP_RATIO: f64 = 0.9;
/// Angle within which directions snap to horizontal or vertical, in radians (4°).
const SNAP_ANGLE: f64 = 4.0 * PI / 180.0;
/// Points the closed-shape fit resamples the path to (fit, then check).
const FIT_SAMPLES: usize = 96;
const CHECK_SAMPLES: usize = 128;

/// Largest and root-mean-square distance to a straight line, × its length.
const LINE_MAX_DEV: f64 = 0.06;
const LINE_RMS_DEV: f64 = 0.025;
/// Longest path along a line, × its length (a line drawn back and forth is no line).
const LINE_MAX_PATH: f64 = 1.3;

/// Tolerance of the split into straight pieces, × the bounding-box diagonal, at least
/// `SEG_EPS_MIN` units.
const SEG_EPS: f64 = 0.02;
const SEG_EPS_MIN: f64 = 25.0;
/// Two pieces turning by less than this (radians, 25°) are one piece if the joined piece stays
/// within `MERGE_DEV` × its length (or 1.5 × the split tolerance) of its chord.
const MERGE_TURN: f64 = 25.0 * PI / 180.0;
const MERGE_DEV: f64 = 0.05;
/// A piece at an end shorter than `HOOK_SHARE` × the longest piece and than `HOOK_MAX` units
/// is the hook the pen makes when it lands or lifts.
const HOOK_SHARE: f64 = 0.08;
const HOOK_MAX: f64 = 200.0;
/// Samples at the end of a straight piece farther than this from it (or 2.5 × the piece's
/// root-mean-square distance) are a hook too, in units.
const HOOK_DEV_MIN: f64 = 15.0;
/// A stroke needing more straight pieces is not part of an arrow.
const MAX_PIECES: usize = 6;

/// Barbs: length relative to the shaft and in units, angle to the shaft, reach to the tip.
const BARB_MIN_SHARE: f64 = 0.04;
const BARB_MIN: f64 = 250.0;
const BARB_MAX_SHARE: f64 = 0.55;
const BARB_ANGLE_MIN: f64 = 12.0 * PI / 180.0;
const BARB_ANGLE_MAX: f64 = 70.0 * PI / 180.0;
const BARB_REACH: f64 = 0.35;
const BARB_REACH_MIN: f64 = 120.0;
/// A piece going straight back over the previous one (turn above 160°, length ratio below
/// 1.67) retraces it: the way back from a barb to the tip.
const RETRACE_TURN: f64 = 160.0 * PI / 180.0;
const RETRACE_RATIO: f64 = 1.67;
/// Strokes completing an arrow: one of their ends within `PARTNER_REACH` × the shaft (at least
/// `PARTNER_REACH_MIN` units) of the shaft's end, all their points within `PARTNER_SPAN` × the
/// shaft; at most `PARTNER_CANDIDATES` of them are tried, the nearest first.
const PARTNER_REACH: f64 = 0.25;
const PARTNER_REACH_MIN: f64 = 300.0;
const PARTNER_SPAN: f64 = 0.7;
const PARTNER_CANDIDATES: usize = 4;

/// What a group of strokes was recognised as.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shape {
    Line,
    /// A circle or an ellipse.
    Ellipse,
    /// A rectangle or a square.
    Rectangle,
    Arrow,
}

impl Shape {
    pub const ALL: [Shape; 4] = [Shape::Line, Shape::Ellipse, Shape::Rectangle, Shape::Arrow];
}

/// Strokes recognised as one shape, and their straightened replacement.
#[derive(Debug, Clone, PartialEq)]
pub struct Straightened {
    /// Indices of the original strokes this replaces, ascending.
    pub sources: Vec<usize>,
    pub shape: Shape,
    /// The new strokes, in drawing order.
    pub strokes: Vec<Stroke>,
}

/// Recognises the shapes among `strokes` and returns their straightened versions, ordered by
/// their first source. Strokes that are no clear shape appear in no result. Arrows may combine
/// several strokes of the same layer; every other shape is one stroke.
pub fn straighten(strokes: &[Stroke]) -> Vec<Straightened> {
    let paths: Vec<Vec<V>> = strokes.iter().map(clean).collect();
    let mut used = vec![false; strokes.len()];
    let mut out = Vec::new();

    // Closed shapes first: they are a single stroke and never part of an arrow.
    for (i, path) in paths.iter().enumerate() {
        if let Some(closed) = closed_shape(path) {
            used[i] = true;
            let (shape, ideal) = match closed.fit {
                ClosedFit::Ellipse(e) => (Shape::Ellipse, ellipse_points(&e, &closed)),
                ClosedFit::Rect(r) => (Shape::Rectangle, densify(&rect_corners(&r, &closed))),
            };
            out.push(Straightened {
                sources: vec![i],
                shape,
                strokes: vec![make_stroke(&strokes[i], &ideal)],
            });
        }
    }

    // Arrows, then lines, from the straight pieces of the other strokes.
    let pieces: Vec<Option<Vec<Piece>>> = paths
        .iter()
        .enumerate()
        .map(|(i, path)| if used[i] { None } else { split(path) })
        .collect();
    let longest = |i: usize| -> f64 {
        pieces[i]
            .as_ref()
            .map_or(0.0, |p| p.iter().map(Piece::len).fold(0.0, f64::max))
    };
    let mut order: Vec<usize> = (0..strokes.len())
        .filter(|&i| longest(i) >= MIN_SIZE)
        .collect();
    order.sort_by(|&a, &b| longest(b).total_cmp(&longest(a)).then(a.cmp(&b)));
    for &s in &order {
        if used[s] {
            continue;
        }
        if let Some(group) = find_arrow(s, strokes, &paths, &pieces, &used) {
            for &i in &group.sources {
                used[i] = true;
            }
            out.push(group);
        }
    }
    for (i, piece) in pieces.iter().enumerate() {
        if used[i] {
            continue;
        }
        let Some([piece]) = piece.as_deref() else {
            continue;
        };
        if let Some((a, b)) = straight_line(&paths[i][piece.from..=piece.to], piece) {
            out.push(Straightened {
                sources: vec![i],
                shape: Shape::Line,
                strokes: vec![make_stroke(&strokes[i], &densify(&[a, b]))],
            });
        }
    }
    out.sort_by_key(|group| group.sources.first().copied().unwrap_or(usize::MAX));
    out
}

// --- Vectors -------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
struct V {
    x: f64,
    y: f64,
}

impl V {
    const fn new(x: f64, y: f64) -> Self {
        V { x, y }
    }

    fn dot(self, o: V) -> f64 {
        self.x * o.x + self.y * o.y
    }

    fn cross(self, o: V) -> f64 {
        self.x * o.y - self.y * o.x
    }

    fn len(self) -> f64 {
        self.x.hypot(self.y)
    }

    fn dist(self, o: V) -> f64 {
        (self - o).len()
    }

    fn unit(self) -> Option<V> {
        let len = self.len();
        (len > 1e-9).then(|| self * (1.0 / len))
    }

    /// Turned by `angle` radians (from x towards y).
    fn rot(self, angle: f64) -> V {
        let (s, c) = angle.sin_cos();
        V::new(c * self.x - s * self.y, s * self.x + c * self.y)
    }
}

impl Add for V {
    type Output = V;
    fn add(self, o: V) -> V {
        V::new(self.x + o.x, self.y + o.y)
    }
}

impl Sub for V {
    type Output = V;
    fn sub(self, o: V) -> V {
        V::new(self.x - o.x, self.y - o.y)
    }
}

impl Mul<f64> for V {
    type Output = V;
    fn mul(self, k: f64) -> V {
        V::new(self.x * k, self.y * k)
    }
}

/// Unsigned angle between two vectors, 0..=π.
fn angle_between(a: V, b: V) -> f64 {
    a.cross(b).abs().atan2(a.dot(b))
}

/// The usable samples of a stroke: finite, on the page's scale, without repeats.
fn clean(stroke: &Stroke) -> Vec<V> {
    let limit = f64::from(MAX_COORD);
    let mut out: Vec<V> = Vec::with_capacity(stroke.points.len());
    for p in &stroke.points {
        let v = V::new(f64::from(p.x), f64::from(p.y));
        if !v.x.is_finite() || !v.y.is_finite() || v.x.abs() > limit || v.y.abs() > limit {
            continue;
        }
        if out.last().is_some_and(|last| last.dist(v) < 1.0) {
            continue;
        }
        out.push(v);
    }
    out
}

fn path_length(points: &[V]) -> f64 {
    points.windows(2).map(|w| w[0].dist(w[1])).sum()
}

/// Width and height of the bounding box.
fn extent(points: &[V]) -> (f64, f64) {
    let mut min = V::new(f64::INFINITY, f64::INFINITY);
    let mut max = V::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
    for p in points {
        min = V::new(min.x.min(p.x), min.y.min(p.y));
        max = V::new(max.x.max(p.x), max.y.max(p.y));
    }
    if points.is_empty() {
        (0.0, 0.0)
    } else {
        (max.x - min.x, max.y - min.y)
    }
}

/// `n` points equally spaced along the path (`n` ≥ 2, the path at least 2 points long).
fn resample(points: &[V], n: usize) -> Vec<V> {
    let total = path_length(points);
    if points.len() < 2 || total <= 0.0 || n < 2 {
        return points.to_vec();
    }
    let step = total / (n - 1) as f64;
    let mut out = Vec::with_capacity(n);
    out.push(points[0]);
    let mut travelled = 0.0;
    let mut next = step;
    for w in points.windows(2) {
        let len = w[0].dist(w[1]);
        while next <= travelled + len && out.len() < n - 1 {
            let k = if len > 0.0 {
                (next - travelled) / len
            } else {
                0.0
            };
            out.push(w[0] + (w[1] - w[0]) * k);
            next += step;
        }
        travelled += len;
    }
    out.push(points[points.len() - 1]);
    out
}

/// Total least-squares line through `points`: a point on it and its unit direction.
fn fit_line(points: &[V]) -> Option<(V, V)> {
    let n = points.len() as f64;
    if points.len() < 2 {
        return None;
    }
    let c = points.iter().fold(V::new(0.0, 0.0), |s, &p| s + p) * (1.0 / n);
    let (mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0);
    for p in points {
        let d = *p - c;
        sxx += d.x * d.x;
        syy += d.y * d.y;
        sxy += d.x * d.y;
    }
    if sxx + syy <= 1e-12 {
        return None;
    }
    let angle = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    Some((c, V::new(angle.cos(), angle.sin())))
}

/// `direction` turned to exactly horizontal or vertical when it is within [`SNAP_ANGLE`].
fn snap_direction(direction: V) -> V {
    let angle = direction.y.atan2(direction.x);
    let quarter = (angle / (PI / 2.0)).round();
    if (angle - quarter * PI / 2.0).abs() <= SNAP_ANGLE {
        let snapped = quarter * PI / 2.0;
        V::new(snapped.cos().round(), snapped.sin().round())
    } else {
        direction
    }
}

// --- Closed shapes -------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
struct Ellipse {
    c: V,
    a: f64,
    b: f64,
    theta: f64,
}

#[derive(Debug, Clone, Copy)]
struct Rect {
    c: V,
    /// Half width and half height, along the turned axes.
    hw: f64,
    hh: f64,
    theta: f64,
}

#[derive(Debug, Clone, Copy)]
enum ClosedFit {
    Ellipse(Ellipse),
    Rect(Rect),
}

#[derive(Debug, Clone, Copy)]
struct Closed {
    fit: ClosedFit,
    /// +1 when drawn with increasing angle (x towards y), −1 otherwise.
    dir: f64,
    /// Where the pen landed.
    start: V,
}

/// The ellipse or rectangle `points` draws, if it is clearly one.
fn closed_shape(points: &[V]) -> Option<Closed> {
    if points.len() < 8 {
        return None;
    }
    let (w, h) = extent(points);
    if w.max(h) < MIN_SIZE {
        return None;
    }
    let diag = w.hypot(h);

    // Where the stroke comes back to its start: the rest is overshoot (it must still lie on
    // the shape, which the check below includes).
    let total = path_length(points);
    let mut travelled = 0.0;
    let mut close: Option<(usize, f64)> = None;
    for i in 1..points.len() {
        travelled += points[i - 1].dist(points[i]);
        if travelled < CLOSE_SEARCH * total {
            continue;
        }
        let gap = points[i].dist(points[0]);
        if close.is_none_or(|(_, best)| gap < best) {
            close = Some((i, gap));
        }
    }
    let (end, gap) = close?;
    if gap > CLOSE_GAP * diag {
        return None;
    }
    let trimmed = &points[..=end];
    let (tw, th) = extent(trimmed);
    if tw.max(th) < MIN_SIZE || tw.min(th) < MIN_ASPECT * tw.max(th) {
        return None;
    }

    let fit = resample(trimmed, FIT_SAMPLES);
    let dir = winding(&fit)?;
    let check = resample(points, CHECK_SAMPLES);

    let ellipse = fit_ellipse(&fit).and_then(|e| {
        [snap_ellipse(e), e]
            .into_iter()
            .map(|e| {
                (
                    e,
                    closed_error(&check, |p| ellipse_distance(&e, p), e.a * e.b),
                )
            })
            .find(|(_, (mean, max))| *mean <= FIT_MEAN && *max <= FIT_MAX)
    });
    let rect = [true, false].into_iter().find_map(|snap| {
        let r = fit_rect(&fit, snap)?;
        let error = closed_error(&check, |p| rect_distance(&r, p), r.hw * r.hh);
        (error.0 <= FIT_MEAN && error.1 <= FIT_MAX).then_some((r, error))
    });
    let best = match (ellipse, rect) {
        (Some((e, (em, _))), Some((r, (rm, _)))) => {
            if rm < em {
                ClosedFit::Rect(r)
            } else {
                ClosedFit::Ellipse(e)
            }
        }
        (Some((e, _)), None) => ClosedFit::Ellipse(e),
        (None, Some((r, _))) => ClosedFit::Rect(r),
        (None, None) => return None,
    };
    Some(Closed {
        fit: best,
        dir,
        start: points[0],
    })
}

/// +1 or −1 when the closed polygon goes round exactly once (in that direction), else `None`
/// (a figure of eight, a double loop, a scribble).
fn winding(poly: &[V]) -> Option<f64> {
    let edges: Vec<V> = (0..poly.len())
        .map(|i| poly[(i + 1) % poly.len()] - poly[i])
        .filter(|e| e.len() > 1e-9)
        .collect();
    if edges.len() < 3 {
        return None;
    }
    let turning: f64 = (0..edges.len())
        .map(|i| {
            let (a, b) = (edges[i], edges[(i + 1) % edges.len()]);
            a.cross(b).atan2(a.dot(b))
        })
        .sum();
    let turns = (turning / TAU).round();
    (turns.abs() == 1.0).then_some(turns)
}

/// Mean and largest of `distance` over `points`, relative to the shape's mean radius
/// (`√area_term`).
fn closed_error(points: &[V], distance: impl Fn(V) -> f64, area_term: f64) -> (f64, f64) {
    let scale = area_term.sqrt();
    if points.is_empty() || scale <= 0.0 {
        return (f64::INFINITY, f64::INFINITY);
    }
    let (sum, max) = points.iter().fold((0.0, 0.0f64), |(sum, max), &p| {
        let e = distance(p) / scale;
        (sum + e, max.max(e))
    });
    (sum / points.len() as f64, max)
}

/// The ellipse with the area moments of the closed polygon (exact for a filled ellipse: the
/// variance along an axis of semi-length `a` is a²/4).
fn fit_ellipse(poly: &[V]) -> Option<Ellipse> {
    let n = poly.len();
    let o = poly.iter().fold(V::new(0.0, 0.0), |s, &p| s + p) * (1.0 / n as f64);
    let (mut a2, mut cx, mut cy, mut sxx, mut syy, mut sxy) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
    for i in 0..n {
        let p = poly[i] - o;
        let q = poly[(i + 1) % n] - o;
        let c = p.cross(q);
        a2 += c;
        cx += (p.x + q.x) * c;
        cy += (p.y + q.y) * c;
        sxx += (p.x * p.x + p.x * q.x + q.x * q.x) * c;
        syy += (p.y * p.y + p.y * q.y + q.y * q.y) * c;
        sxy += (p.x * q.y + 2.0 * p.x * p.y + 2.0 * q.x * q.y + q.x * p.y) * c;
    }
    let area = a2 / 2.0;
    if area.abs() < 1.0 {
        return None;
    }
    let (gx, gy) = (cx / (6.0 * area), cy / (6.0 * area));
    let mxx = sxx / (12.0 * area) - gx * gx;
    let myy = syy / (12.0 * area) - gy * gy;
    let mxy = sxy / (24.0 * area) - gx * gy;
    let half = (mxx + myy) / 2.0;
    let d = (((mxx - myy) / 2.0).powi(2) + mxy * mxy).sqrt();
    let (l1, l2) = (half + d, half - d);
    if l2 <= 0.0 || !l1.is_finite() {
        return None;
    }
    Some(Ellipse {
        c: o + V::new(gx, gy),
        a: 2.0 * l1.sqrt(),
        b: 2.0 * l2.sqrt(),
        theta: 0.5 * (2.0 * mxy).atan2(mxx - myy),
    })
}

/// Nearly round becomes a circle, a nearly horizontal or vertical axis becomes exactly so.
fn snap_ellipse(mut e: Ellipse) -> Ellipse {
    if e.b >= SNAP_RATIO * e.a {
        let r = (e.a * e.b).sqrt();
        e.a = r;
        e.b = r;
        e.theta = 0.0;
    } else {
        let axis = snap_direction(V::new(e.theta.cos(), e.theta.sin()));
        e.theta = axis.y.atan2(axis.x);
    }
    e
}

/// Distance from `p` to the ellipse along the ray from its centre (never less than the true
/// distance, so the check errs on the safe side).
fn ellipse_distance(e: &Ellipse, p: V) -> f64 {
    let q = (p - e.c).rot(-e.theta);
    let r = ((q.x / e.a).powi(2) + (q.y / e.b).powi(2)).sqrt();
    if r < 1e-9 {
        e.b
    } else {
        q.len() * (1.0 - 1.0 / r).abs()
    }
}

/// The rectangle the closed polygon draws: its edges' common direction (the mean of 4 × their
/// angles), then each side at the median of the points nearest to it.
fn fit_rect(poly: &[V], snap: bool) -> Option<Rect> {
    let n = poly.len();
    let (mut mx, mut my, mut total) = (0.0, 0.0, 0.0);
    for i in 0..n {
        let e = poly[(i + 1) % n] - poly[i];
        let len = e.len();
        let angle = 4.0 * e.y.atan2(e.x);
        mx += len * angle.cos();
        my += len * angle.sin();
        total += len;
    }
    if total <= 0.0 || mx.hypot(my) / total < RECT_COHERENCE {
        return None;
    }
    let mut theta = my.atan2(mx) / 4.0;
    if snap && theta.abs() <= SNAP_ANGLE {
        theta = 0.0;
    }
    let o = poly.iter().fold(V::new(0.0, 0.0), |s, &p| s + p) * (1.0 / n as f64);
    let q: Vec<V> = poly.iter().map(|&p| (p - o).rot(-theta)).collect();
    let (mut left, mut right, mut top, mut bottom) = q.iter().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(l, r, t, b), p| (l.min(p.x), r.max(p.x), t.min(p.y), b.max(p.y)),
    );
    let mut counts = [0usize; 4];
    for _ in 0..4 {
        let mut sides: [Vec<f64>; 4] = Default::default();
        for p in &q {
            let d = [
                (p.x - left).abs(),
                (p.x - right).abs(),
                (p.y - top).abs(),
                (p.y - bottom).abs(),
            ];
            let side = (0..4).min_by(|&i, &j| d[i].total_cmp(&d[j])).unwrap_or(0);
            sides[side].push(if side < 2 { p.x } else { p.y });
        }
        let medians: Vec<f64> = sides.iter_mut().filter_map(|s| median(s)).collect();
        let [l, r, t, b] = medians[..] else {
            return None;
        };
        (left, right, top, bottom) = (l, r, t, b);
        counts = [
            sides[0].len(),
            sides[1].len(),
            sides[2].len(),
            sides[3].len(),
        ];
    }
    let (w, h) = (right - left, bottom - top);
    if w <= 0.0 || h <= 0.0 {
        return None;
    }
    let lengths = [h, h, w, w];
    let perimeter = 2.0 * (w + h);
    if (0..4).any(|i| (counts[i] as f64) < RECT_SIDE_SHARE * n as f64 * lengths[i] / perimeter) {
        return None;
    }
    let mut rect = Rect {
        c: o + V::new((left + right) / 2.0, (top + bottom) / 2.0).rot(theta),
        hw: w / 2.0,
        hh: h / 2.0,
        theta,
    };
    if snap && rect.hw.min(rect.hh) >= SNAP_RATIO * rect.hw.max(rect.hh) {
        let half = (rect.hw * rect.hh).sqrt();
        rect.hw = half;
        rect.hh = half;
    }
    Some(rect)
}

fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[values.len() / 2])
}

fn rect_distance(r: &Rect, p: V) -> f64 {
    let q = (p - r.c).rot(-r.theta);
    let dx = q.x.abs() - r.hw;
    let dy = q.y.abs() - r.hh;
    if dx > 0.0 || dy > 0.0 {
        dx.max(0.0).hypot(dy.max(0.0))
    } else {
        -dx.max(dy)
    }
}

/// The ellipse as dense points, from the angle where the pen landed, in the drawing direction,
/// ending where it starts.
fn ellipse_points(e: &Ellipse, closed: &Closed) -> Vec<V> {
    let q = (closed.start - e.c).rot(-e.theta);
    let start = (q.y / e.b).atan2(q.x / e.a);
    // Equal angles: the points are farthest apart at the ends of the long axis.
    let n = ((TAU * e.a.max(e.b) / SAMPLE_SPACING).ceil() as usize).clamp(72, MAX_POINTS);
    let mut points: Vec<V> = (0..n)
        .map(|i| {
            let phi = start + closed.dir * TAU * i as f64 / n as f64;
            e.c + V::new(e.a * phi.cos(), e.b * phi.sin()).rot(e.theta)
        })
        .collect();
    points.push(points[0]);
    points
}

/// The rectangle's corners from the one nearest to where the pen landed, in the drawing
/// direction, ending at the first.
fn rect_corners(r: &Rect, closed: &Closed) -> Vec<V> {
    // Increasing angle (x towards y) in the rectangle's frame.
    let mut corners = vec![
        V::new(-r.hw, -r.hh),
        V::new(r.hw, -r.hh),
        V::new(r.hw, r.hh),
        V::new(-r.hw, r.hh),
    ];
    if closed.dir < 0.0 {
        corners.reverse();
    }
    let q = (closed.start - r.c).rot(-r.theta);
    let first = (0..4)
        .min_by(|&i, &j| corners[i].dist(q).total_cmp(&corners[j].dist(q)))
        .unwrap_or(0);
    corners.rotate_left(first);
    corners.push(corners[0]);
    corners.into_iter().map(|c| r.c + c.rot(r.theta)).collect()
}

// --- Straight pieces, lines and arrows -----------------------------------------------------

/// A straight piece of a stroke: its fitted ends, in drawing order, and the samples it covers.
#[derive(Debug, Clone, Copy)]
struct Piece {
    a: V,
    b: V,
    from: usize,
    to: usize,
}

impl Piece {
    fn len(&self) -> f64 {
        self.a.dist(self.b)
    }

    fn dir(&self) -> V {
        self.b - self.a
    }
}

/// Largest distance of `points` to the chord from the first to the last.
fn chord_deviation(points: &[V]) -> f64 {
    let (Some(&a), Some(&b)) = (points.first(), points.last()) else {
        return 0.0;
    };
    match (b - a).unit() {
        Some(u) => points
            .iter()
            .map(|&p| (p - a).cross(u).abs())
            .fold(0.0, f64::max),
        None => points.iter().map(|&p| p.dist(a)).fold(0.0, f64::max),
    }
}

/// Indices of the Douglas-Peucker simplification of `points` with tolerance `eps`.
fn douglas_peucker(points: &[V], eps: f64) -> Vec<usize> {
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    let mut stack = vec![(0, points.len() - 1)];
    while let Some((i, j)) = stack.pop() {
        if j <= i + 1 {
            continue;
        }
        let (a, b) = (points[i], points[j]);
        let u = (b - a).unit();
        let (mut far, mut far_d) = (i, -1.0);
        for (k, &p) in points.iter().enumerate().take(j).skip(i + 1) {
            let d = match u {
                Some(u) => (p - a).cross(u).abs(),
                None => p.dist(a),
            };
            if d > far_d {
                (far, far_d) = (k, d);
            }
        }
        if far_d > eps {
            keep[far] = true;
            stack.push((i, far));
            stack.push((far, j));
        }
    }
    (0..points.len()).filter(|&k| keep[k]).collect()
}

/// The stroke as straight pieces (pen hooks at its ends dropped), or `None` when it needs more
/// than [`MAX_PIECES`] (a word, a curve, a scribble).
fn split(points: &[V]) -> Option<Vec<Piece>> {
    if points.len() < 2 {
        return None;
    }
    let (w, h) = extent(points);
    let eps = (SEG_EPS * w.hypot(h)).max(SEG_EPS_MIN);
    let mut idx = douglas_peucker(points, eps);
    if idx.len() > 4 * MAX_PIECES {
        return None;
    }
    // Join pieces that are really one (a slightly bowed line cut by the simplification).
    loop {
        let joined = (1..idx.len().saturating_sub(1)).find(|&j| {
            let (i0, i1, i2) = (idx[j - 1], idx[j], idx[j + 1]);
            let turn = angle_between(points[i1] - points[i0], points[i2] - points[i1]);
            let tolerance = (MERGE_DEV * points[i0].dist(points[i2])).max(1.5 * eps);
            turn < MERGE_TURN && chord_deviation(&points[i0..=i2]) <= tolerance
        });
        match joined {
            Some(j) => {
                idx.remove(j);
            }
            None => break,
        }
    }
    let chord = |idx: &[usize], k: usize| points[idx[k]].dist(points[idx[k + 1]]);
    if idx.len() > 2 {
        let longest = (0..idx.len() - 1)
            .map(|k| chord(&idx, k))
            .fold(0.0, f64::max);
        let hook = (HOOK_SHARE * longest).min(HOOK_MAX);
        if chord(&idx, 0) < hook {
            idx.remove(0);
        }
        if idx.len() > 2 && chord(&idx, idx.len() - 2) < hook {
            idx.pop();
        }
    }
    if idx.len() < 2 || idx.len() - 1 > MAX_PIECES {
        return None;
    }
    idx.windows(2)
        .map(|k| fit_piece(points, k[0], k[1]))
        .collect()
}

/// The straight piece through `points[from..=to]`, without the samples at its ends that stray
/// from it (a pen hook smaller than the split tolerance, the rounding of a corner): at most
/// [`HOOK_MAX`] units of path at each end, farther than 2.5 × the root-mean-square distance
/// (at least `HOOK_DEV_MIN` units).
fn fit_piece(points: &[V], from: usize, to: usize) -> Option<Piece> {
    let (c, u) = fit_line(&points[from..=to])?;
    let dev = |p: V| (p - c).cross(u).abs();
    let count = (to - from + 1) as f64;
    let rms = (points[from..=to]
        .iter()
        .map(|&p| dev(p).powi(2))
        .sum::<f64>()
        / count)
        .sqrt();
    let tolerance = (2.5 * rms).max(HOOK_DEV_MIN);
    let (mut start, mut end) = (from, to);
    let mut walked = 0.0;
    while start + 1 < end && dev(points[start]) > tolerance && walked < HOOK_MAX {
        walked += points[start].dist(points[start + 1]);
        start += 1;
    }
    walked = 0.0;
    while end > start + 1 && dev(points[end]) > tolerance && walked < HOOK_MAX {
        walked += points[end].dist(points[end - 1]);
        end -= 1;
    }
    let (c, u) = if (start, end) == (from, to) {
        (c, u)
    } else {
        fit_line(&points[start..=end])?
    };
    let project = |p: V| c + u * (p - c).dot(u);
    Some(Piece {
        a: project(points[start]),
        b: project(points[end]),
        from: start,
        to: end,
    })
}

/// The ends of the straight line drawn by `points` (one piece of a stroke), if it is one.
fn straight_line(points: &[V], piece: &Piece) -> Option<(V, V)> {
    let len = piece.len();
    if len < MIN_SIZE {
        return None;
    }
    let (c, u) = fit_line(points)?;
    let deviations: Vec<f64> = points.iter().map(|&p| (p - c).cross(u).abs()).collect();
    let max = deviations.iter().copied().fold(0.0, f64::max);
    let rms = (deviations.iter().map(|d| d * d).sum::<f64>() / deviations.len() as f64).sqrt();
    if max > LINE_MAX_DEV * len || rms > LINE_RMS_DEV * len {
        return None;
    }
    if path_length(points) > LINE_MAX_PATH * len {
        return None;
    }
    Some(snap_segment(piece.a, piece.b))
}

/// The segment turned about its middle to exactly horizontal or vertical when it is nearly so.
fn snap_segment(a: V, b: V) -> (V, V) {
    let Some(u) = (b - a).unit() else {
        return (a, b);
    };
    let snapped = snap_direction(u);
    let middle = (a + b) * 0.5;
    let half = a.dist(b) / 2.0;
    (middle - snapped * half, middle + snapped * half)
}

/// What a piece of an arrow is.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Role {
    Shaft,
    Barb(usize),
    /// Goes back over the previous piece of the same stroke.
    Retrace,
}

/// The ideal arrow: tail, tip and barb ends.
#[derive(Debug, Clone, Copy)]
struct ArrowFit {
    tail: V,
    tip: V,
    barbs: [V; 2],
    /// The raw tip, to orient each piece.
    raw_tip: V,
}

/// An arrow whose shaft is the longest piece of stroke `s`, alone or with up to two other
/// strokes of its layer near one end of the shaft.
fn find_arrow(
    s: usize,
    strokes: &[Stroke],
    paths: &[Vec<V>],
    pieces: &[Option<Vec<Piece>>],
    used: &[bool],
) -> Option<Straightened> {
    let own = pieces[s].as_ref()?;
    if let Some(found) = arrow_of(&[s], strokes, pieces) {
        return Some(found);
    }
    let shaft = own.iter().max_by(|a, b| a.len().total_cmp(&b.len()))?;
    let length = shaft.len();
    for end in [shaft.b, shaft.a] {
        let mut near: Vec<(f64, usize)> = (0..strokes.len())
            .filter(|&j| j != s && !used[j] && strokes[j].layer == strokes[s].layer)
            .filter_map(|j| {
                let other = pieces[j].as_ref()?;
                let reach = other
                    .iter()
                    .flat_map(|p| [p.a.dist(end), p.b.dist(end)])
                    .fold(f64::INFINITY, f64::min);
                let span = paths[j].iter().map(|p| p.dist(end)).fold(0.0, f64::max);
                (reach <= (PARTNER_REACH * length).max(PARTNER_REACH_MIN)
                    && span <= PARTNER_SPAN * length)
                    .then_some((reach, j))
            })
            .collect();
        near.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        near.truncate(PARTNER_CANDIDATES);
        for (k, &(_, j)) in near.iter().enumerate() {
            if let Some(found) = arrow_of(&[s, j], strokes, pieces) {
                return Some(found);
            }
            for &(_, l) in &near[k + 1..] {
                if let Some(found) = arrow_of(&[s, j, l], strokes, pieces) {
                    return Some(found);
                }
            }
        }
    }
    None
}

/// The arrow drawn by the pieces of `group` (stroke indices), if they make exactly one.
fn arrow_of(
    group: &[usize],
    strokes: &[Stroke],
    pieces: &[Option<Vec<Piece>>],
) -> Option<Straightened> {
    // Each stroke's pieces, with the returns over a barb marked.
    let mut chains: Vec<(usize, Vec<(Piece, bool)>)> = Vec::new();
    for &s in group {
        let list = pieces[s].as_ref()?;
        let mut chain: Vec<(Piece, bool)> = Vec::with_capacity(list.len());
        for (k, piece) in list.iter().enumerate() {
            let retrace = k > 0 && !chain[k - 1].1 && {
                let previous = &list[k - 1];
                let ratio = piece.len() / previous.len().max(1e-9);
                angle_between(previous.dir(), piece.dir()) >= RETRACE_TURN
                    && (1.0 / RETRACE_RATIO..=RETRACE_RATIO).contains(&ratio)
            };
            chain.push((*piece, retrace));
        }
        chains.push((s, chain));
    }
    let main: Vec<(usize, usize)> = chains
        .iter()
        .enumerate()
        .flat_map(|(c, (_, chain))| {
            chain
                .iter()
                .enumerate()
                .filter(|(_, (_, retrace))| !retrace)
                .map(move |(k, _)| (c, k))
        })
        .collect();
    if main.len() != 3 {
        return None;
    }
    let piece = |(c, k): (usize, usize)| chains[c].1[k].0;
    let shaft_at = *main
        .iter()
        .max_by(|&&a, &&b| piece(a).len().total_cmp(&piece(b).len()))?;
    let shaft = piece(shaft_at);
    if shaft.len() < MIN_SIZE {
        return None;
    }
    let barb_at: Vec<(usize, usize)> = main.iter().copied().filter(|&m| m != shaft_at).collect();
    let fit = [(shaft.b, shaft.a), (shaft.a, shaft.b)]
        .into_iter()
        .find_map(|(tip, tail)| arrow_fit(tip, tail, [piece(barb_at[0]), piece(barb_at[1])]))?;

    // Each stroke becomes its ideal pieces, in its own drawing order.
    let mut result = Straightened {
        sources: Vec::new(),
        shape: Shape::Arrow,
        strokes: Vec::new(),
    };
    for (c, (s, chain)) in chains.iter().enumerate() {
        let mut lines: Vec<Vec<V>> = Vec::new();
        let mut previous: Option<(V, V)> = None;
        for (k, (p, retrace)) in chain.iter().enumerate() {
            let role = if *retrace {
                Role::Retrace
            } else if (c, k) == shaft_at {
                Role::Shaft
            } else if (c, k) == barb_at[0] {
                Role::Barb(0)
            } else {
                Role::Barb(1)
            };
            let towards_tip = p.a.dist(fit.raw_tip) > p.b.dist(fit.raw_tip);
            let (from, to) = match role {
                Role::Retrace => match previous {
                    Some((from, to)) => (to, from),
                    None => continue,
                },
                Role::Shaft if towards_tip => (fit.tail, fit.tip),
                Role::Shaft => (fit.tip, fit.tail),
                Role::Barb(i) if towards_tip => (fit.barbs[i], fit.tip),
                Role::Barb(i) => (fit.tip, fit.barbs[i]),
            };
            match lines.last_mut() {
                Some(line) if line.last().is_some_and(|last| last.dist(from) < 1.0) => {
                    line.push(to);
                }
                _ => lines.push(vec![from, to]),
            }
            previous = Some((from, to));
        }
        result.sources.push(*s);
        result.strokes.extend(make_strokes(
            &strokes[*s],
            &lines.iter().map(|l| densify(l)).collect::<Vec<_>>(),
        ));
    }
    result.sources.sort_unstable();
    Some(result)
}

/// The ideal arrow with its tip at `tip`, if the two `barbs` start near it, point back along
/// the shaft on both sides and have a barb's length.
fn arrow_fit(tip: V, tail: V, barbs: [Piece; 2]) -> Option<ArrowFit> {
    let shaft = tail - tip;
    let length = shaft.len();
    let back = shaft.unit()?;
    let mut lengths = [0.0; 2];
    let mut angles = [0.0; 2];
    let mut sides = [0.0; 2];
    for (i, barb) in barbs.iter().enumerate() {
        let (near, far) = if barb.a.dist(tip) <= barb.b.dist(tip) {
            (barb.a, barb.b)
        } else {
            (barb.b, barb.a)
        };
        let len = barb.len();
        if len < (BARB_MIN_SHARE * length).max(BARB_MIN) || len > BARB_MAX_SHARE * length {
            return None;
        }
        if near.dist(tip) > (BARB_REACH * len).max(BARB_REACH_MIN) {
            return None;
        }
        let angle = angle_between(back, far - near);
        if !(BARB_ANGLE_MIN..=BARB_ANGLE_MAX).contains(&angle) {
            return None;
        }
        lengths[i] = len;
        angles[i] = angle;
        sides[i] = back.cross(far - near).signum();
    }
    if sides[0] == sides[1] {
        return None;
    }
    // A clean, symmetric head on a shaft that snaps to the axes.
    let (snapped_tail, snapped_tip) = snap_segment(tail, tip);
    let back = (snapped_tail - snapped_tip).unit()?;
    let len = (lengths[0] + lengths[1]) / 2.0;
    let angle = (angles[0] + angles[1]) / 2.0;
    Some(ArrowFit {
        tail: snapped_tail,
        tip: snapped_tip,
        barbs: [
            snapped_tip + back.rot(sides[0] * angle) * len,
            snapped_tip + back.rot(sides[1] * angle) * len,
        ],
        raw_tip: tip,
    })
}

// --- New strokes ---------------------------------------------------------------------------

/// The polyline through `vertices` with points at most [`SAMPLE_SPACING`] apart (more for
/// huge shapes); the vertices themselves are samples, so corners stay corners.
fn densify(vertices: &[V]) -> Vec<V> {
    let total = path_length(vertices);
    let spacing = SAMPLE_SPACING.max(total / MAX_POINTS as f64);
    let mut out = Vec::new();
    for (k, w) in vertices.windows(2).enumerate() {
        if k == 0 {
            out.push(w[0]);
        }
        let steps = ((w[0].dist(w[1]) / spacing).ceil() as usize).max(1);
        for i in 1..=steps {
            out.push(w[0] + (w[1] - w[0]) * (i as f64 / steps as f64));
        }
    }
    if out.is_empty() {
        out.extend(vertices.first());
    }
    out
}

/// Median pressure of a stroke (0.5 without a usable one).
fn median_pressure(stroke: &Stroke) -> f32 {
    let mut values: Vec<f64> = stroke
        .points
        .iter()
        .map(|p| f64::from(p.pressure))
        .filter(|p| p.is_finite())
        .collect();
    median(&mut values).map_or(0.5, |p| p.clamp(0.0, 1.0) as f32)
}

/// Start and end time of a stroke, ms.
fn time_span(stroke: &Stroke) -> (u32, u32) {
    let start = stroke.points.first().map_or(0, |p| p.t);
    let end = stroke.points.last().map_or(start, |p| p.t).max(start);
    (start, end)
}

/// A stroke like `source` (tool, colour, width, layer) along `points`, its pressure the
/// source's median, its times spread over the source's duration.
fn make_stroke(source: &Stroke, points: &[V]) -> Stroke {
    let (start, end) = time_span(source);
    timed(source, points, start, end)
}

/// Several strokes sharing the duration of `source`, in proportion to their lengths.
fn make_strokes(source: &Stroke, lines: &[Vec<V>]) -> Vec<Stroke> {
    let (start, end) = time_span(source);
    let total: f64 = lines.iter().map(|l| path_length(l)).sum();
    let mut travelled = 0.0;
    lines
        .iter()
        .map(|line| {
            let at = |d: f64| {
                let k = if total > 0.0 { d / total } else { 0.0 };
                start + (f64::from(end - start) * k).round() as u32
            };
            let from = at(travelled);
            travelled += path_length(line);
            timed(source, line, from, at(travelled))
        })
        .collect()
}

fn timed(source: &Stroke, points: &[V], start: u32, end: u32) -> Stroke {
    let pressure = median_pressure(source);
    let total = path_length(points);
    let span = f64::from(end.saturating_sub(start));
    let mut travelled = 0.0;
    let mut previous: Option<V> = None;
    let points = points
        .iter()
        .map(|&p| {
            if let Some(q) = previous {
                travelled += q.dist(p);
            }
            previous = Some(p);
            let k = if total > 0.0 { travelled / total } else { 0.0 };
            Point {
                x: p.x as f32,
                y: p.y as f32,
                pressure,
                t: start + (span * k).round() as u32,
            }
        })
        .collect();
    Stroke {
        tool: source.tool,
        color: source.color,
        width: source.width,
        layer: source.layer,
        points,
    }
}

#[cfg(test)]
mod tests;
