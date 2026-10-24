//! Stroke geometry: the width each tool gives a point, and the outline that every renderer
//! fills. The screen, the PNG/SVG/PDF exports and the video all fill these same contours, so
//! they all show the same strokes (SPEC §6.2).
//!
//! # Widths
//!
//! [`Tool::width_at`] turns the stroke's nominal width and a point's pressure into the
//! diameter of the line at that point:
//!
//! `diameter = width × (min + (1 − min) × pressure^gamma)`
//!
//! | Tool          | min  | gamma | pressure 0 / 0.5 / 1   | Shape                                   |
//! |---------------|------|-------|------------------------|-----------------------------------------|
//! | `Ballpoint`   | 0.45 | 1     | 45 % / 72.5 % / 100 %  | round                                   |
//! | `FountainPen` | 0.60 | 1     | 60 % / 80 % / 100 %    | slanted elliptic nib (40°, 1 : 4)       |
//! | `FeltTip`     | 0.85 | 1     | 85 % / 92.5 % / 100 %  | round                                   |
//! | `Pencil`      | 0.55 | 1     | 55 % / 77.5 % / 100 %  | round (thin by default width)           |
//! | `Brush`       | 0.12 | 1.6   | 12 % / 41 % / 100 %    | round, tapered over 3 widths at the ends|
//! | `Highlighter` | 0.90 | 1     | 90 % / 95 % / 100 %    | round joins, square ends                |
//! | `Chalk`       | 0.70 | 1     | 70 % / 85 % / 100 %    | round, irregular width (±12 %)          |
//!
//! At full pressure every tool draws exactly the nominal width. The ballpoint's 45–100 % range
//! covers a light touch to full pressure and matches Inkspace's pen, whose stored widths span
//! about 47–100 % of their maximum. [`Tool::pressure_for_width`] is the exact inverse
//! (clamped to 0..=1): the Inkspace import uses it to store Inkspace's per-point widths as
//! pressures. For the ballpoint the outline has exactly `width_at` as diameter at every
//! sample, with no smoothing or scaling of the width, so imported widths render faithfully.
//!
//! # Outlines
//!
//! [`outline`] builds closed contours in page coordinates:
//!
//! 1. Samples with a non-finite value or a coordinate beyond ±[`MAX_COORD`] are skipped;
//!    consecutive samples at the same place are merged (the widest wins).
//! 2. The centreline is a Catmull-Rom spline through the samples (end points duplicated, as
//!    Inkspace does), flattened into short segments. Samples are vertices of the flattened
//!    line, so they never move. A sample where the line turns by more than 60° is a corner
//!    and stays sharp (cusps; shapes made of few points, which plain Catmull-Rom would
//!    inflate). Tangents are limited to twice the length of their segment, which only
//!    matters where a short segment sits between long ones and avoids the small loops plain
//!    Catmull-Rom makes there. The radius varies linearly between samples.
//! 3. Tool shaping: brush taper near both ends, chalk roughness.
//! 4. The line is swept with the tool's nib (a disc, or a slanted ellipse for the fountain
//!    pen). A run of segments becomes one polygon: left side forward, end cap, right side
//!    back, start cap. Where a turn is too sharp for the two sides to stay apart, the run is
//!    cut with flat ends and the sector of the nib on the outer side of the turn fills the
//!    gap: a round join, with no spike and no hole. (The runs cover the inner side: every
//!    point near the line is beside a segment or in the outer wedge of a vertex.)
//!
//! All contours wind the same way (counter-clockwise as seen on the page, y pointing down;
//! negative shoelace area), so the union of the pieces is what the **non-zero** fill rule
//! paints: tiny-skia `FillRule::Winding`, SVG `fill-rule="nonzero"`, canvas
//! `fill("nonzero")`. The even-odd rule would punch holes where pieces overlap.

use std::f64::consts::{PI, TAU};
use std::fmt::Write as _;
use std::ops::{Add, Mul, Neg, Sub};

use crate::model::{Stroke, Tool};

/// Samples farther than this from the origin, in device units (10 km), are skipped.
pub const MAX_COORD: f32 = 1.0e9;

/// Smallest outline radius, in device units, so that a zero-width stroke stays a valid shape.
const MIN_RADIUS: f64 = 0.5;
/// Largest outline radius, in device units (100 m), to keep the arithmetic finite.
const MAX_RADIUS: f64 = 1.0e7;
/// Points closer than this, in device units, are the same point.
const SAME_POINT: f64 = 1.0e-3;
/// Largest distance between the Catmull-Rom curve and its flattened segments, in device units.
const FLATNESS: f64 = 1.0;
/// Most segments one curve segment between two samples is cut into.
const MAX_SUBDIVISIONS: usize = 32;
/// Cosine of the sharpest turn (60°) between the chords on both sides of a sample that the
/// spline rounds off. A sharper turn is a corner: the tangent there is zero, so the curve
/// keeps the corner (cusps, shapes drawn with few points such as a straightened rectangle).
const COS_CORNER: f64 = 0.5;
/// Largest distance between a true arc and its polygon, in device units.
const ARC_TOLERANCE: f64 = 1.0;
/// Cosine of the sharpest turn (0.4 rad) joined smoothly inside one run; sharper turns cut the
/// run and get a round join. At that angle the line is at most 2 % thinner beside the vertex.
const COS_MAX_SMOOTH_TURN: f64 = 0.921_060_994;
/// Fountain pen nib: angle of its long axis above the horizontal, and minor/major axis ratio.
/// Down-strokes come out thicker than horizontal ones, diagonals rising to the right thinnest.
const NIB_ANGLE_DEGREES: f64 = 40.0;
const NIB_RATIO: f64 = 0.25;
/// Highlighter: how far a square end extends past the end sample, in radii.
const SQUARE_CAP_EXTENSION: f64 = 0.5;
/// Brush: length of the taper at each end, in nominal widths, and the radius factor at the tip.
const BRUSH_TAPER_WIDTHS: f64 = 3.0;
const BRUSH_TIP: f64 = 0.08;
/// Chalk: relative width variation, and its period along the stroke in nominal widths.
const CHALK_ROUGHNESS: f64 = 0.12;
const CHALK_PERIOD_WIDTHS: f64 = 0.8;

/// Pressure response of a tool: `diameter = width × (min + (1 − min) × pressure^gamma)`.
#[derive(Debug, Clone, Copy)]
struct Response {
    min: f64,
    gamma: f64,
}

impl Tool {
    fn response(self) -> Response {
        let (min, gamma) = match self {
            Tool::Ballpoint => (0.45, 1.0),
            Tool::FountainPen => (0.60, 1.0),
            Tool::FeltTip => (0.85, 1.0),
            Tool::Pencil => (0.55, 1.0),
            Tool::Brush => (0.12, 1.6),
            Tool::Highlighter => (0.90, 1.0),
            Tool::Chalk => (0.70, 1.0),
        };
        Response { min, gamma }
    }

    /// Diameter of the line, in device units, for a stroke of nominal width `stroke_width` at
    /// `pressure` (0..=1; values outside are clamped, NaN counts as 0). Strictly increasing in
    /// pressure; equal to `stroke_width` at full pressure. A negative or non-finite width
    /// counts as 0. See the module documentation for each tool's response.
    pub fn width_at(self, stroke_width: f32, pressure: f32) -> f32 {
        let width = sanitize_width(stroke_width);
        let p = if pressure.is_nan() {
            0.0
        } else {
            f64::from(pressure).clamp(0.0, 1.0)
        };
        let Response { min, gamma } = self.response();
        let k = if gamma == 1.0 { p } else { p.powf(gamma) };
        (width * (min + (1.0 - min) * k)) as f32
    }

    /// Exact inverse of [`Tool::width_at`]: the pressure that gives a line of `diameter`
    /// device units, clamped to 0..=1 (a diameter below the tool's lightest line gives 0,
    /// above the nominal width gives 1). Returns 0 for a zero or invalid width or a NaN
    /// diameter.
    pub fn pressure_for_width(self, stroke_width: f32, diameter: f32) -> f32 {
        let width = sanitize_width(stroke_width);
        if width <= 0.0 || diameter.is_nan() {
            return 0.0;
        }
        let Response { min, gamma } = self.response();
        let k = ((f64::from(diameter) / width - min) / (1.0 - min)).clamp(0.0, 1.0);
        let p = if gamma == 1.0 { k } else { k.powf(1.0 / gamma) };
        p as f32
    }

    /// A comfortable nominal width for a new stroke of this tool, in device units: 0.5 mm
    /// for the ballpoint (Inkspace's pen), wide for the highlighter, thin for the pencil.
    pub fn default_width(self) -> f32 {
        match self {
            Tool::Ballpoint => 50.0,
            Tool::FountainPen => 90.0,
            Tool::FeltTip => 110.0,
            Tool::Pencil => 35.0,
            Tool::Brush => 220.0,
            Tool::Highlighter => 500.0,
            Tool::Chalk => 180.0,
        }
    }
}

fn sanitize_width(width: f32) -> f64 {
    if width.is_finite() && width > 0.0 {
        f64::from(width)
    } else {
        0.0
    }
}

/// Axis-aligned bounding box in page coordinates.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

/// The filled shape of a stroke: closed contours in page coordinates (device units).
///
/// Each contour is implicitly closed (the last vertex joins the first; the first vertex is
/// not repeated) and has at least 3 vertices. Fill them all together with the non-zero rule.
/// Serialises as `{"contours": [[[x, y], …], …]}`.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Outline {
    pub contours: Vec<Vec<[f32; 2]>>,
}

impl Outline {
    /// True if there is nothing to fill (a stroke without a usable sample).
    pub fn is_empty(&self) -> bool {
        self.contours.is_empty()
    }

    /// Bounding box of all contours, or `None` if empty.
    pub fn bounds(&self) -> Option<Bounds> {
        let mut points = self.contours.iter().flatten();
        let &[x, y] = points.next()?;
        let mut b = Bounds {
            min_x: x,
            min_y: y,
            max_x: x,
            max_y: y,
        };
        for &[x, y] in points {
            b.min_x = b.min_x.min(x);
            b.min_y = b.min_y.min(y);
            b.max_x = b.max_x.max(x);
            b.max_y = b.max_y.max(y);
        }
        Some(b)
    }

    /// True if `(x, y)` is inside the filled shape (non-zero winding), e.g. for hit tests.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let (x, y) = (f64::from(x), f64::from(y));
        let mut winding = 0i64;
        for contour in &self.contours {
            for (i, a) in contour.iter().enumerate() {
                let b = contour[(i + 1) % contour.len()];
                let (x0, y0) = (f64::from(a[0]), f64::from(a[1]));
                let (x1, y1) = (f64::from(b[0]), f64::from(b[1]));
                let side = (x1 - x0) * (y - y0) - (x - x0) * (y1 - y0);
                if y0 <= y {
                    if y1 > y && side > 0.0 {
                        winding += 1;
                    }
                } else if y1 <= y && side < 0.0 {
                    winding -= 1;
                }
            }
        }
        winding != 0
    }

    /// SVG path data in page coordinates (`M x y L x y … Z` per contour, two decimals at
    /// most). Use it with `fill-rule="nonzero"`, the SVG default.
    pub fn to_svg_path(&self) -> String {
        let mut path = String::new();
        for contour in &self.contours {
            for (i, &[x, y]) in contour.iter().enumerate() {
                path.push(if i == 0 { 'M' } else { 'L' });
                push_number(&mut path, x);
                path.push(' ');
                push_number(&mut path, y);
            }
            path.push('Z');
        }
        path
    }

    /// Contours as flat `[x0, y0, x1, y1, …]` arrays, for a canvas or a Tauri channel.
    pub fn to_flat(&self) -> Vec<Vec<f32>> {
        self.contours
            .iter()
            .map(|c| c.iter().flatten().copied().collect())
            .collect()
    }
}

/// Appends `value` with at most two decimals and no trailing zeros.
fn push_number(out: &mut String, value: f32) {
    let start = out.len();
    let _ = write!(out, "{value:.2}");
    if out[start..].contains('.') {
        let trimmed = out.trim_end_matches('0').trim_end_matches('.').len();
        out.truncate(trimmed);
    }
    if &out[start..] == "-0" {
        out.truncate(start);
        out.push('0');
    }
}

/// The outline of `stroke` (see the module documentation). Never panics; a stroke without
/// any usable sample gives an empty outline.
pub fn outline(stroke: &Stroke) -> Outline {
    let width = sanitize_width(stroke.width);
    let samples = samples(stroke);
    let Some(first) = samples.first() else {
        return Outline::default();
    };
    let mut line = centreline(&samples);
    match stroke.tool {
        Tool::Brush => taper(&mut line, BRUSH_TAPER_WIDTHS * width),
        Tool::Chalk => {
            let seed = hash_f64(first.p.x) ^ hash_f64(first.p.y).rotate_left(16);
            roughen(&mut line, (CHALK_PERIOD_WIDTHS * width).max(1.0), seed);
        }
        _ => {}
    }
    let style = Style::of(stroke.tool);
    let mut contours = Vec::new();
    sweep(&line, &style, &mut contours);
    contours.retain(|c| c.len() >= 3 && c.iter().flatten().all(|v| v.is_finite()));
    Outline { contours }
}

/// A 2D vector in page coordinates. The geometry runs in `f64` so that huge coordinates and
/// tiny segments stay well-behaved; outlines are stored in `f32`.
#[derive(Debug, Clone, Copy, PartialEq)]
struct V {
    x: f64,
    y: f64,
}

impl V {
    const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    fn dot(self, o: V) -> f64 {
        self.x * o.x + self.y * o.y
    }

    fn cross(self, o: V) -> f64 {
        self.x * o.y - self.y * o.x
    }

    /// Length. Coordinates are at most 1e9 and radii 1e7, so the squares cannot overflow
    /// an `f64` (and `sqrt` is one instruction, unlike `hypot`).
    fn len(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// The vector turned a quarter turn (to the left of `self` in a y-up frame).
    fn perp(self) -> V {
        V::new(-self.y, self.x)
    }

    fn normalized(self) -> Option<V> {
        let len = self.len();
        (len > 0.0 && len.is_finite()).then(|| self * (1.0 / len))
    }

    /// `self` shortened to at most `max` long.
    fn limited(self, max: f64) -> V {
        let len = self.len();
        if len > max && len > 0.0 {
            self * (max / len)
        } else {
            self
        }
    }

    fn to_f32(self) -> [f32; 2] {
        [self.x as f32, self.y as f32]
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

impl Neg for V {
    type Output = V;
    fn neg(self) -> V {
        V::new(-self.x, -self.y)
    }
}

/// A point of the centreline with the outline radius there.
#[derive(Debug, Clone, Copy)]
struct Vertex {
    p: V,
    r: f64,
}

/// Usable samples with their radius, consecutive duplicates merged.
fn samples(stroke: &Stroke) -> Vec<Vertex> {
    let mut out: Vec<Vertex> = Vec::with_capacity(stroke.points.len());
    for point in &stroke.points {
        let finite = point.x.is_finite() && point.y.is_finite() && point.pressure.is_finite();
        if !finite || point.x.abs() > MAX_COORD || point.y.abs() > MAX_COORD {
            continue;
        }
        let diameter = stroke.tool.width_at(stroke.width, point.pressure);
        let vertex = Vertex {
            p: V::new(point.x.into(), point.y.into()),
            r: (f64::from(diameter) / 2.0).clamp(MIN_RADIUS, MAX_RADIUS),
        };
        match out.last_mut() {
            Some(last) if (last.p - vertex.p).len() <= SAME_POINT => last.r = last.r.max(vertex.r),
            _ => out.push(vertex),
        }
    }
    out
}

/// The flattened Catmull-Rom centreline through `samples` (which contains every sample).
fn centreline(samples: &[Vertex]) -> Vec<Vertex> {
    let n = samples.len();
    let mut line = Vec::with_capacity(n * 2);
    let Some(&first) = samples.first() else {
        return line;
    };
    line.push(first);
    // Catmull-Rom tangents, (next − previous) / 2, zero at corners.
    let tangents: Vec<V> = (0..n)
        .map(|i| {
            let before = samples[i.saturating_sub(1)].p;
            let after = samples[(i + 1).min(n - 1)].p;
            let p = samples[i].p;
            let turn = (p - before)
                .normalized()
                .zip((after - p).normalized())
                .map(|(d_in, d_out)| d_in.dot(d_out));
            if turn.is_some_and(|cos| cos < COS_CORNER) {
                V::new(0.0, 0.0)
            } else {
                (after - before) * 0.5
            }
        })
        .collect();
    for i in 0..n.saturating_sub(1) {
        let (a, b) = (samples[i], samples[i + 1]);
        let chord = (b.p - a.p).len();
        // Bézier form of the Catmull-Rom segment.
        let c1 = a.p + tangents[i].limited(2.0 * chord) * (1.0 / 3.0);
        let c2 = b.p - tangents[i + 1].limited(2.0 * chord) * (1.0 / 3.0);
        // The flattening error with k segments is at most 0.75 × bend / k².
        let bend = (a.p - c1 * 2.0 + c2).len().max((c1 - c2 * 2.0 + b.p).len());
        let steps = ((0.75 * bend / FLATNESS).sqrt().ceil() as usize).clamp(1, MAX_SUBDIVISIONS);
        for s in 1..steps {
            let t = s as f64 / steps as f64;
            let vertex = Vertex {
                p: bezier(a.p, c1, c2, b.p, t),
                r: a.r + (b.r - a.r) * t,
            };
            push_vertex(&mut line, vertex, false);
        }
        push_vertex(&mut line, b, true);
    }
    line
}

fn bezier(p0: V, c1: V, c2: V, p1: V, t: f64) -> V {
    let u = 1.0 - t;
    p0 * (u * u * u) + c1 * (3.0 * u * u * t) + c2 * (3.0 * u * t * t) + p1 * (t * t * t)
}

/// Appends `vertex` unless it lands on the previous vertex, in which case the two merge
/// (widest radius). A sample takes the place of a generated point, so samples never move.
fn push_vertex(line: &mut Vec<Vertex>, vertex: Vertex, is_sample: bool) {
    if let Some(last) = line.last_mut()
        && (last.p - vertex.p).len() <= SAME_POINT
    {
        if is_sample {
            last.p = vertex.p;
        }
        last.r = last.r.max(vertex.r);
        return;
    }
    line.push(vertex);
}

/// Cumulative length along `line` at each vertex.
fn arc_lengths(line: &[Vertex]) -> Vec<f64> {
    let mut total = 0.0;
    let mut previous = line.first().map(|v| v.p);
    line.iter()
        .map(|v| {
            if let Some(p) = previous {
                total += (v.p - p).len();
            }
            previous = Some(v.p);
            total
        })
        .collect()
}

/// Brush: the radius shrinks smoothly to [`BRUSH_TIP`] over `length` at both ends.
fn taper(line: &mut [Vertex], length: f64) {
    let lengths = arc_lengths(line);
    let total = lengths.last().copied().unwrap_or(0.0);
    let ramp = length.min(total / 2.0);
    if ramp.is_nan() || ramp <= 0.0 {
        return;
    }
    for (vertex, s) in line.iter_mut().zip(lengths) {
        let x = (s.min(total - s) / ramp).clamp(0.0, 1.0);
        let k = BRUSH_TIP + (1.0 - BRUSH_TIP) * x * x * (3.0 - 2.0 * x);
        vertex.r = (vertex.r * k).max(MIN_RADIUS);
    }
}

/// Chalk: the radius wanders by ±[`CHALK_ROUGHNESS`] along the stroke (deterministic noise).
fn roughen(line: &mut [Vertex], period: f64, seed: u32) {
    let lengths = arc_lengths(line);
    for (vertex, s) in line.iter_mut().zip(lengths) {
        let n = value_noise(s / period, seed);
        vertex.r = (vertex.r * (1.0 + CHALK_ROUGHNESS * (2.0 * n - 1.0))).max(MIN_RADIUS);
    }
}

/// Integer hash (good avalanche, no state), the basis of every deterministic noise here.
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

fn hash_f64(value: f64) -> u32 {
    let bits = value.to_bits();
    hash(bits as u32 ^ hash((bits >> 32) as u32))
}

/// Smooth 1D value noise in 0..=1.
fn value_noise(t: f64, seed: u32) -> f64 {
    let t = if t.is_finite() { t } else { 0.0 };
    let i = t.floor();
    let f = t - i;
    let cell = i as i64 as u32;
    let at = |c: u32| f64::from(hash(c ^ seed) >> 8) / f64::from(1u32 << 24);
    let (a, b) = (at(cell), at(cell.wrapping_add(1)));
    a + (b - a) * f * f * (3.0 - 2.0 * f)
}

/// How a stroke ends.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Cap {
    Round,
    /// Flat, extended past the end sample by [`SQUARE_CAP_EXTENSION`] radii.
    Square,
}

/// The pen tip swept along the centreline: an ellipse of major semi-axis `r` along `u` and
/// minor semi-axis `r × ratio` (a disc when `ratio` is 1). Points on it are parametrised by
/// an angle `θ`: `u·r·cos θ + v·r·ratio·sin θ`; decreasing `θ` runs the same way round as
/// the outline contours.
#[derive(Debug, Clone, Copy)]
struct Nib {
    u: V,
    ratio: f64,
}

impl Nib {
    const ROUND: Nib = Nib {
        u: V::new(1.0, 0.0),
        ratio: 1.0,
    };

    fn fountain_pen() -> Nib {
        let angle = NIB_ANGLE_DEGREES.to_radians();
        // y points down on the page: "above the horizontal" is negative y.
        Nib {
            u: V::new(angle.cos(), -angle.sin()),
            ratio: NIB_RATIO,
        }
    }

    fn v(self) -> V {
        self.u.perp()
    }

    /// Angle of the nib point farthest in direction `n` (its support point).
    fn theta(self, n: V) -> f64 {
        (self.ratio * n.dot(self.v())).atan2(n.dot(self.u))
    }

    fn point(self, theta: f64, r: f64) -> V {
        self.u * (r * theta.cos()) + self.v() * (r * self.ratio * theta.sin())
    }

    /// The nib point farthest in the unit direction `n`: `point(theta(n), r)`, without
    /// trigonometry for a round nib (where it is simply `n × r`).
    fn side(self, n: V, r: f64) -> V {
        if self.ratio == 1.0 {
            n * r
        } else {
            self.point(self.theta(n), r)
        }
    }

    /// Number of polygon sides for the whole nib (even, so that halves are exact).
    fn sides(r: f64) -> usize {
        let cos_half = (1.0 - ARC_TOLERANCE / r).clamp(-1.0, 1.0);
        let step = 2.0 * cos_half.acos();
        let sides = if step > 0.0 {
            (TAU / step).ceil() as usize
        } else {
            96
        };
        sides.clamp(12, 96).next_multiple_of(2)
    }

    /// The whole nib centred on `vertex`, as an outline contour.
    fn polygon(self, vertex: Vertex) -> Vec<V> {
        let sides = Self::sides(vertex.r);
        (0..sides)
            .map(|j| vertex.p + self.point(-TAU * j as f64 / sides as f64, vertex.r))
            .collect()
    }

    /// The round join where a run is cut at `vertex`, turning from `d_in` to `d_out`.
    ///
    /// The run that ends there covers everything behind its flat end, the run that starts
    /// there everything ahead of its own, so the only gap is the wedge between the two flat
    /// ends on the outer side of the turn: this is that sector of the nib. Its apex is moved
    /// slightly inwards so that the vertex itself is strictly inside (for hit tests); the
    /// sector stays inside the nib, so it adds nothing outside the true stroke.
    fn join(self, vertex: Vertex, d_in: V, d_out: V) -> Vec<V> {
        let (theta_in, theta_out) = (self.theta(d_in.perp()), self.theta(d_out.perp()));
        // Turning towards the left side puts the right side outside.
        let (start, end) = if d_in.cross(d_out) > 0.0 {
            (theta_out + PI, theta_in + PI)
        } else {
            (theta_in, theta_out)
        };
        // The outline's way round is decreasing θ; the outer wedge spans at most a half turn
        // (a sweep just under a full turn is a rounding artefact of a straight line).
        let sweep = (start - end).rem_euclid(TAU);
        let sweep = if sweep > PI + 0.5 { 0.0 } else { sweep.min(PI) };
        if sweep < 1e-9 {
            return Vec::new();
        }
        let steps = ((Self::sides(vertex.r) as f64 * sweep / TAU).ceil() as usize).max(1);
        let rim: Vec<V> = (0..=steps)
            .map(|j| vertex.p + self.point(start - sweep * j as f64 / steps as f64, vertex.r))
            .collect();
        let (first, last) = (rim[0] - vertex.p, rim[steps] - vertex.p);
        let outward = (first + last).normalized().unwrap_or(d_in);
        let apex = vertex.p - outward * (0.2 * self.ratio * vertex.r);
        std::iter::once(apex).chain(rim).collect()
    }
}

/// Tool-specific outline shape.
#[derive(Debug, Clone, Copy)]
struct Style {
    nib: Nib,
    cap: Cap,
}

impl Style {
    fn of(tool: Tool) -> Style {
        match tool {
            Tool::FountainPen => Style {
                nib: Nib::fountain_pen(),
                cap: Cap::Round,
            },
            Tool::Highlighter => Style {
                nib: Nib::ROUND,
                cap: Cap::Square,
            },
            Tool::Ballpoint | Tool::FeltTip | Tool::Pencil | Tool::Brush | Tool::Chalk => Style {
                nib: Nib::ROUND,
                cap: Cap::Round,
            },
        }
    }

    /// Shape of a stroke reduced to one point.
    fn tip(&self, vertex: Vertex) -> Vec<V> {
        match self.cap {
            Cap::Round => self.nib.polygon(vertex),
            Cap::Square => {
                let r = vertex.r;
                [(r, r), (r, -r), (-r, -r), (-r, r)]
                    .into_iter()
                    .map(|(x, y)| vertex.p + V::new(x, y))
                    .collect()
            }
        }
    }

    /// Cap points strictly between the side points at `theta_from` and `theta_from − π`,
    /// bulging towards `outward`.
    fn cap(&self, vertex: Vertex, theta_from: f64, outward: V) -> Vec<V> {
        match self.cap {
            Cap::Round => {
                let half = Nib::sides(vertex.r) / 2;
                (1..half)
                    .map(|j| {
                        let theta = theta_from - PI * j as f64 / half as f64;
                        vertex.p + self.nib.point(theta, vertex.r)
                    })
                    .collect()
            }
            Cap::Square => {
                let side = self.nib.point(theta_from, vertex.r);
                let extension = outward * (SQUARE_CAP_EXTENSION * vertex.r);
                vec![vertex.p + side + extension, vertex.p - side + extension]
            }
        }
    }
}

/// True if the quadrilateral `a b c d` is convex and runs the outline's way round (or is
/// degenerate), so that it adds coverage everywhere inside it and nowhere else.
fn quad_ok(a: V, b: V, c: V, d: V) -> bool {
    let pts = [a, b, c, d];
    (0..4).all(|i| {
        let e1 = pts[(i + 1) % 4] - pts[i];
        let e2 = pts[(i + 2) % 4] - pts[(i + 1) % 4];
        e1.cross(e2) <= 1e-9 * e1.len() * e2.len()
    })
}

/// One polygon of the outline being built: both sides of a run of segments.
#[derive(Default)]
struct Run {
    left: Vec<V>,
    right: Vec<V>,
    start_cap: Vec<V>,
}

impl Run {
    /// Starts a run at `vertex` going in direction `d`; with a cap if the stroke starts here.
    fn start(&mut self, vertex: Vertex, d: V, style: &Style, stroke_start: bool) {
        self.left.clear();
        self.right.clear();
        self.start_cap.clear();
        let side = style.nib.side(d.perp(), vertex.r);
        self.left.push(vertex.p + side);
        self.right.push(vertex.p - side);
        if stroke_start {
            self.start_cap = style.cap(vertex, style.nib.theta(d.perp()) + PI, -d);
        }
    }

    /// Adds the side points of `vertex` for the side direction `n`.
    fn push(&mut self, vertex: Vertex, n: V, nib: Nib) {
        let side = nib.side(n, vertex.r);
        self.left.push(vertex.p + side);
        self.right.push(vertex.p - side);
    }

    /// Joins smoothly at `vertex` with the bisecting side direction `bisector`, if the
    /// quadrilaterals before it and after it (up to `next`, direction `d_next`) stay valid.
    fn try_join(&mut self, vertex: Vertex, bisector: V, next: Vertex, d_next: V, nib: Nib) -> bool {
        let (Some(&left_prev), Some(&right_prev)) = (self.left.last(), self.right.last()) else {
            return false;
        };
        let side = nib.side(bisector, vertex.r);
        let (left, right) = (vertex.p + side, vertex.p - side);
        let side_next = nib.side(d_next.perp(), next.r);
        let (left_next, right_next) = (next.p + side_next, next.p - side_next);
        if quad_ok(left_prev, left, right, right_prev)
            && quad_ok(left, left_next, right_next, right)
        {
            self.left.push(left);
            self.right.push(right);
            true
        } else {
            false
        }
    }

    /// The closed contour: left side, `end_cap`, right side backwards, start cap.
    fn contour(&self, end_cap: Vec<V>) -> Vec<[f32; 2]> {
        self.left
            .iter()
            .copied()
            .chain(end_cap)
            .chain(self.right.iter().rev().copied())
            .chain(self.start_cap.iter().copied())
            .map(V::to_f32)
            .collect()
    }
}

/// Sweeps the nib along `line`, adding the contours to `contours`.
fn sweep(line: &[Vertex], style: &Style, contours: &mut Vec<Vec<[f32; 2]>>) {
    let to_f32 = |points: Vec<V>| points.into_iter().map(V::to_f32).collect::<Vec<_>>();
    let (Some(&first), Some(&last)) = (line.first(), line.last()) else {
        return;
    };
    if line.len() == 1 {
        contours.push(to_f32(style.tip(first)));
        return;
    }
    let dirs: Vec<V> = line
        .windows(2)
        .map(|w| (w[1].p - w[0].p).normalized().unwrap_or(V::new(1.0, 0.0)))
        .collect();
    let mut run = Run::default();
    run.start(first, dirs[0], style, true);
    for k in 1..line.len() - 1 {
        let (d_in, d_out, vertex) = (dirs[k - 1], dirs[k], line[k]);
        let joined = d_in.dot(d_out) >= COS_MAX_SMOOTH_TURN
            && (d_in.perp() + d_out.perp())
                .normalized()
                .is_some_and(|b| run.try_join(vertex, b, line[k + 1], d_out, style.nib));
        if !joined {
            // Close this run flat at the vertex, fill the outer wedge with a round join, and
            // start the next run there.
            run.push(vertex, d_in.perp(), style.nib);
            contours.push(run.contour(Vec::new()));
            let join = style.nib.join(vertex, d_in, d_out);
            if join.len() >= 3 {
                contours.push(to_f32(join));
            }
            run.start(vertex, d_out, style, false);
        }
    }
    let d_end = dirs[dirs.len() - 1];
    run.push(last, d_end.perp(), style.nib);
    let end_cap = style.cap(last, style.nib.theta(d_end.perp()), d_end);
    contours.push(run.contour(end_cap));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Point, Rgba};

    fn pt(x: f32, y: f32, pressure: f32) -> Point {
        Point {
            x,
            y,
            pressure,
            t: 0,
        }
    }

    fn stroke(tool: Tool, width: f32, points: Vec<Point>) -> Stroke {
        Stroke {
            tool,
            color: Rgba::BLACK,
            width,
            layer: 0,
            points,
        }
    }

    /// Deterministic pseudo-random numbers (64-bit LCG, Knuth's constants).
    struct Lcg(u64);

    impl Lcg {
        fn next_u32(&mut self) -> u32 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (self.0 >> 33) as u32
        }

        /// Uniform in 0..1.
        fn unit(&mut self) -> f32 {
            (self.next_u32() >> 8) as f32 / (1u32 << 24) as f32
        }

        fn range(&mut self, lo: f32, hi: f32) -> f32 {
            lo + (hi - lo) * self.unit()
        }
    }

    fn signed_area(contour: &[[f32; 2]]) -> f64 {
        let n = contour.len();
        (0..n)
            .map(|i| {
                let [x0, y0] = contour[i];
                let [x1, y1] = contour[(i + 1) % n];
                f64::from(x0) * f64::from(y1) - f64::from(x1) * f64::from(y0)
            })
            .sum::<f64>()
            / 2.0
    }

    /// Width of the filled shape through `(x, y)` along direction `(nx, ny)` (unit), found by
    /// walking out from the point until it leaves the shape on both sides.
    fn width_through(outline: &Outline, x: f32, y: f32, nx: f32, ny: f32, max: f32) -> f32 {
        let reach = |sign: f32| {
            let (mut inside, mut outside) = (0.0f32, max);
            for _ in 0..40 {
                let mid = (inside + outside) / 2.0;
                if outline.contains(x + sign * nx * mid, y + sign * ny * mid) {
                    inside = mid;
                } else {
                    outside = mid;
                }
            }
            inside
        };
        reach(1.0) + reach(-1.0)
    }

    #[test]
    fn widths_round_trip_for_every_tool() {
        for tool in Tool::ALL {
            for width in [1.0f32, 50.0, 333.3, 5000.0] {
                for i in 0..=100 {
                    let p = i as f32 / 100.0;
                    let d = tool.width_at(width, p);
                    let back = tool.pressure_for_width(width, d);
                    assert!(
                        (back - p).abs() < 1e-4,
                        "{tool:?} w={width} p={p} back={back}"
                    );
                    let again = tool.width_at(width, back);
                    assert!((again - d).abs() <= d * 1e-5, "{tool:?} w={width} d={d}");
                }
            }
        }
    }

    #[test]
    fn widths_are_strictly_increasing_and_nominal_at_full_pressure() {
        for tool in Tool::ALL {
            let mut previous = -1.0;
            for i in 0..=1000 {
                let d = tool.width_at(100.0, i as f32 / 1000.0);
                assert!(d > previous, "{tool:?} not increasing at {i}");
                previous = d;
            }
            assert_eq!(tool.width_at(100.0, 1.0), 100.0, "{tool:?}");
            assert!(tool.width_at(100.0, 0.0) > 0.0, "{tool:?}");
        }
        assert_eq!(Tool::Ballpoint.width_at(100.0, 0.0), 45.0);
        assert_eq!(Tool::Ballpoint.width_at(100.0, 0.5), 72.5);
    }

    #[test]
    fn widths_clamp_and_survive_bad_input() {
        for tool in Tool::ALL {
            assert_eq!(tool.width_at(80.0, -3.0), tool.width_at(80.0, 0.0));
            assert_eq!(tool.width_at(80.0, 7.0), 80.0);
            assert_eq!(tool.width_at(80.0, f32::NAN), tool.width_at(80.0, 0.0));
            assert_eq!(tool.width_at(f32::NAN, 0.5), 0.0);
            assert_eq!(tool.width_at(-5.0, 0.5), 0.0);
            assert_eq!(tool.width_at(f32::INFINITY, 0.5), 0.0);
            assert_eq!(tool.pressure_for_width(80.0, 0.0), 0.0);
            assert_eq!(tool.pressure_for_width(80.0, 1000.0), 1.0);
            assert_eq!(tool.pressure_for_width(80.0, f32::NAN), 0.0);
            assert_eq!(tool.pressure_for_width(0.0, 10.0), 0.0);
            assert_eq!(tool.pressure_for_width(80.0, f32::INFINITY), 1.0);
            assert!(tool.default_width() > 0.0);
        }
    }

    #[test]
    fn empty_and_unusable_strokes_give_empty_outlines() {
        for tool in Tool::ALL {
            assert!(outline(&stroke(tool, 50.0, vec![])).is_empty());
            let bad = vec![
                pt(f32::NAN, 0.0, 0.5),
                pt(0.0, f32::INFINITY, 0.5),
                pt(1.0, 1.0, f32::NAN),
                pt(2.0e9, 0.0, 0.5),
                pt(0.0, -f32::MAX, 0.5),
            ];
            let o = outline(&stroke(tool, 50.0, bad));
            assert!(o.is_empty(), "{tool:?}");
            assert_eq!(o.bounds(), None);
            assert_eq!(o.to_svg_path(), "");
        }
    }

    #[test]
    fn single_point_is_a_disc_of_the_right_diameter() {
        for pressure in [0.0, 0.3, 1.0] {
            let s = stroke(Tool::Ballpoint, 60.0, vec![pt(1000.0, 2000.0, pressure)]);
            let o = outline(&s);
            assert_eq!(o.contours.len(), 1);
            let r = f64::from(Tool::Ballpoint.width_at(60.0, pressure)) / 2.0;
            for &[x, y] in &o.contours[0] {
                let d = (f64::from(x) - 1000.0).hypot(f64::from(y) - 2000.0);
                assert!((d - r).abs() < 1e-3, "vertex at {d}, radius {r}");
            }
            let area = signed_area(&o.contours[0]).abs();
            let disc = PI * r * r;
            assert!((area - disc).abs() / disc < 0.05, "area {area} vs {disc}");
        }
        // Duplicates of one point are one point.
        let s = stroke(Tool::Ballpoint, 60.0, vec![pt(5.0, 5.0, 1.0); 4]);
        assert_eq!(outline(&s).contours.len(), 1);
    }

    #[test]
    fn single_point_of_each_tool() {
        for tool in Tool::ALL {
            let o = outline(&stroke(tool, 100.0, vec![pt(0.0, 0.0, 1.0)]));
            assert_eq!(o.contours.len(), 1, "{tool:?}");
            assert!(o.contains(0.0, 0.0), "{tool:?}");
            let b = o.bounds().unwrap();
            let extent = b.max_x - b.min_x;
            // Chalk wanders up to 12 % wider.
            assert!(extent > 20.0 && extent <= 112.5, "{tool:?}: {extent}");
        }
        // The highlighter ends square: a single point is a square.
        let o = outline(&stroke(Tool::Highlighter, 100.0, vec![pt(0.0, 0.0, 1.0)]));
        assert_eq!(o.contours[0].len(), 4);
    }

    #[test]
    fn two_points_make_a_capsule() {
        let s = stroke(
            Tool::Ballpoint,
            40.0,
            vec![pt(0.0, 0.0, 1.0), pt(1000.0, 0.0, 1.0)],
        );
        let o = outline(&s);
        assert_eq!(o.contours.len(), 1);
        let b = o.bounds().unwrap();
        assert!((b.min_x + 20.0).abs() < 0.01 && (b.max_x - 1020.0).abs() < 0.01);
        assert!((b.min_y + 20.0).abs() < 0.01 && (b.max_y - 20.0).abs() < 0.01);
        assert!(o.contains(500.0, 19.0) && !o.contains(500.0, 21.0));
        assert!(o.contains(-19.0, 0.0) && !o.contains(-21.0, 0.0));
        // A highlighter of the same points ends square, half a radius past the samples.
        let mut h = s.clone();
        h.tool = Tool::Highlighter;
        let b = outline(&h).bounds().unwrap();
        assert!((b.min_x + 10.0).abs() < 0.01 && (b.max_x - 1010.0).abs() < 0.01);
        assert!(outline(&h).contains(1008.0, 19.0));
    }

    /// A wavy stroke with varying pressure, sampled like the tablet (every few units).
    fn wave(tool: Tool, width: f32) -> Stroke {
        let points = (0..200)
            .map(|i| {
                let t = i as f32;
                pt(
                    1000.0 + t * 12.0,
                    2000.0 + 150.0 * (t / 18.0).sin(),
                    0.5 + 0.5 * (t / 23.0).sin(),
                )
            })
            .collect();
        stroke(tool, width, points)
    }

    #[test]
    fn ballpoint_width_at_each_sample_is_width_at() {
        let s = wave(Tool::Ballpoint, 50.0);
        let o = outline(&s);
        let mut worst = 0.0f32;
        for i in 1..s.points.len() - 1 {
            let (a, p, b) = (s.points[i - 1], s.points[i], s.points[i + 1]);
            // Local direction from the neighbours; the width is measured across it.
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len = dx.hypot(dy);
            let (nx, ny) = (-dy / len, dx / len);
            let expected = Tool::Ballpoint.width_at(50.0, p.pressure);
            let measured = width_through(&o, p.x, p.y, nx, ny, 200.0);
            let error = (measured - expected).abs() / expected;
            worst = worst.max(error);
            assert!(error < 0.01, "sample {i}: {measured} vs {expected}");
        }
        println!("largest relative width error at a sample: {worst:.5}");
    }

    #[test]
    fn outlines_contain_their_samples_and_wind_one_way() {
        for tool in Tool::ALL {
            let s = wave(tool, tool.default_width());
            let o = outline(&s);
            assert!(!o.is_empty());
            for p in &s.points {
                assert!(o.contains(p.x, p.y), "{tool:?} misses ({}, {})", p.x, p.y);
            }
            for c in &o.contours {
                assert!(c.len() >= 3);
                assert!(
                    signed_area(c) < 0.0,
                    "{tool:?}: contour winds the other way"
                );
            }
        }
    }

    #[test]
    fn sharp_turns_have_round_joins_without_spikes() {
        // A zigzag with 170° turns.
        let points: Vec<Point> = (0..9)
            .map(|i| pt(i as f32 * 60.0, if i % 2 == 0 { 0.0 } else { 600.0 }, 1.0))
            .collect();
        let s = stroke(Tool::Ballpoint, 50.0, points.clone());
        let o = outline(&s);
        let b = o.bounds().unwrap();
        // No vertex sticks out farther than a radius beyond the samples.
        assert!(b.min_y >= -25.01 && b.max_y <= 625.01, "{b:?}");
        assert!(b.min_x >= -25.01 && b.max_x <= 505.01, "{b:?}");
        // Round join: the tip of each turn is covered up to the radius.
        assert!(o.contains(60.0, 624.0) && o.contains(120.0, -24.0));
        for p in &points {
            assert!(o.contains(p.x, p.y));
        }
        // A U-turn back over itself.
        let u = stroke(
            Tool::Ballpoint,
            50.0,
            vec![pt(0.0, 0.0, 1.0), pt(500.0, 0.0, 1.0), pt(0.0, 0.0, 1.0)],
        );
        let o = outline(&u);
        assert!(o.contains(524.0, 0.0) && !o.contains(526.0, 0.0));
        assert!(o.contains(250.0, 24.0) && !o.contains(250.0, 26.0));
    }

    #[test]
    fn corners_of_sparse_shapes_stay_sharp() {
        // A square drawn with its four corners only: straight sides, no bulge.
        let corners = [
            (0.0, 0.0),
            (3000.0, 0.0),
            (3000.0, 3000.0),
            (0.0, 3000.0),
            (0.0, 0.0),
        ];
        let points = corners.iter().map(|&(x, y)| pt(x, y, 1.0)).collect();
        let o = outline(&stroke(Tool::Ballpoint, 100.0, points));
        let b = o.bounds().unwrap();
        assert!(
            (b.min_x + 50.0).abs() < 0.01 && (b.max_x - 3050.0).abs() < 0.01,
            "{b:?}"
        );
        assert!(
            (b.min_y + 50.0).abs() < 0.01 && (b.max_y - 3050.0).abs() < 0.01,
            "{b:?}"
        );
        assert!(o.contains(1500.0, 49.0) && !o.contains(1500.0, 51.0));
        assert!(o.contains(1500.0, -49.0) && !o.contains(1500.0, -51.0));
        assert!(!o.contains(1500.0, 1500.0));
        // A smooth curve sampled sparsely is still rounded through its samples.
        let arc: Vec<Point> = (0..=8)
            .map(|i| {
                let a = i as f32 * std::f32::consts::FRAC_PI_8;
                pt(1000.0 * a.cos(), 1000.0 * a.sin(), 1.0)
            })
            .collect();
        let o = outline(&stroke(Tool::Ballpoint, 20.0, arc));
        let mid = std::f32::consts::FRAC_PI_8 / 2.0;
        assert!(o.contains(1000.0 * mid.cos(), 1000.0 * mid.sin()));
    }

    #[test]
    fn loops_have_no_holes() {
        // A cursive loop crossing itself (trochoid), with a fat stroke.
        let points: Vec<Point> = (0..120)
            .map(|i| {
                let t = i as f32 * 0.1;
                pt(100.0 * t - 220.0 * t.sin(), 220.0 * t.cos(), 1.0)
            })
            .collect();
        let s = stroke(Tool::Ballpoint, 120.0, points.clone());
        let o = outline(&s);
        // Every point within 0.9 radius of a sample is inside.
        for p in &points {
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                assert!(o.contains(p.x + 54.0 * a.cos(), p.y + 54.0 * a.sin()));
            }
        }
    }

    #[test]
    fn fountain_pen_width_depends_on_direction() {
        let width = 100.0;
        let along = |dx: f32, dy: f32| {
            let s = stroke(
                Tool::FountainPen,
                width,
                vec![pt(0.0, 0.0, 1.0), pt(1000.0 * dx, 1000.0 * dy, 1.0)],
            );
            width_through(&outline(&s), 500.0 * dx, 500.0 * dy, -dy, dx, 200.0)
        };
        let a = NIB_ANGLE_DEGREES.to_radians() as f32;
        // Along the nib's long axis (rising to the right) the line is thinnest, across it
        // the line is the full nominal width.
        let thin = along(a.cos(), -a.sin());
        let thick = along(a.sin(), a.cos());
        assert!((thin - width * NIB_RATIO as f32).abs() < 1.0, "thin {thin}");
        assert!((thick - width).abs() < 1.0, "thick {thick}");
        let vertical = along(0.0, 1.0);
        let horizontal = along(1.0, 0.0);
        assert!(vertical > horizontal && horizontal > thin && vertical < thick);
    }

    #[test]
    fn brush_tapers_at_both_ends() {
        let points = (0..=100).map(|i| pt(i as f32 * 20.0, 0.0, 1.0)).collect();
        let o = outline(&stroke(Tool::Brush, 200.0, points));
        let width_at_x = |x: f32| width_through(&o, x, 0.0, 0.0, 1.0, 400.0);
        assert!(width_at_x(1000.0) > 199.0);
        assert!(width_at_x(10.0) < 40.0, "{}", width_at_x(10.0));
        assert!(width_at_x(1990.0) < 40.0);
        assert!(width_at_x(300.0) > width_at_x(100.0));
    }

    #[test]
    fn huge_coordinates_and_widths_stay_finite() {
        let cases = [
            vec![pt(9.9e8, -9.9e8, 0.5), pt(-9.9e8, 9.9e8, 0.5)],
            vec![
                pt(1.0e8, 1.0e8, 1.0),
                pt(1.0e8 + 0.5, 1.0e8, 1.0),
                pt(1.0e8, 1.0e8 + 3.0, 0.0),
            ],
            vec![pt(0.0, 0.0, 1.0), pt(1e-30, 0.0, 1.0), pt(0.0, 1e-30, 1.0)],
        ];
        for tool in Tool::ALL {
            for points in &cases {
                for width in [0.0, 1e-20, 50.0, 1e30, f32::MAX] {
                    let o = outline(&stroke(tool, width, points.clone()));
                    assert!(!o.is_empty(), "{tool:?} {width}");
                    assert!(o.contours.iter().flatten().flatten().all(|v| v.is_finite()));
                }
            }
        }
    }

    #[test]
    fn svg_path_and_flat_contours() {
        let s = stroke(
            Tool::Ballpoint,
            40.0,
            vec![pt(0.0, 0.0, 1.0), pt(100.5, 0.25, 1.0)],
        );
        let o = outline(&s);
        let svg = o.to_svg_path();
        assert!(svg.starts_with('M'));
        assert_eq!(svg.matches('Z').count(), o.contours.len());
        assert_eq!(svg.matches('M').count(), o.contours.len());
        assert!(!svg.contains("NaN") && !svg.contains("-0 ") && !svg.contains(".00"));
        let flat = o.to_flat();
        assert_eq!(flat.len(), o.contours.len());
        assert_eq!(flat[0].len(), o.contours[0].len() * 2);
        assert_eq!(flat[0][0], o.contours[0][0][0]);
        let mut n = String::new();
        push_number(&mut n, -0.001);
        push_number(&mut n, 1.5);
        push_number(&mut n, 20.0);
        push_number(&mut n, -3.456);
        assert_eq!(n, "01.520-3.46");
    }

    #[test]
    fn fuzz_outlines_never_break() {
        let mut rng = Lcg(0x5eed_1234);
        for case in 0..400 {
            let tool = Tool::ALL[case % Tool::ALL.len()];
            let width = match rng.next_u32() % 5 {
                0 => 0.0,
                1 => rng.range(0.0, 5.0),
                _ => rng.range(5.0, 600.0),
            };
            let n = (rng.next_u32() % 60) as usize;
            let (mut x, mut y) = (rng.range(-1e4, 4e4), rng.range(-1e4, 4e4));
            let mut points = Vec::with_capacity(n);
            for _ in 0..n {
                match rng.next_u32() % 12 {
                    0 => {} // duplicate point
                    1 => {
                        // sharp jump
                        x += rng.range(-2000.0, 2000.0);
                        y += rng.range(-2000.0, 2000.0);
                    }
                    2 => {
                        // tiny step
                        x += rng.range(-0.01, 0.01);
                        y += rng.range(-0.01, 0.01);
                    }
                    _ => {
                        x += rng.range(-40.0, 40.0);
                        y += rng.range(-40.0, 40.0);
                    }
                }
                let mut p = pt(x, y, rng.range(-0.2, 1.2));
                match rng.next_u32() % 40 {
                    0 => p.x = f32::NAN,
                    1 => p.pressure = f32::INFINITY,
                    2 => p.y = 3e9,
                    _ => {}
                }
                points.push(p);
            }
            let s = stroke(tool, width, points);
            let o = outline(&s);
            let usable: Vec<&Point> = s
                .points
                .iter()
                .filter(|p| {
                    p.x.is_finite()
                        && p.y.is_finite()
                        && p.pressure.is_finite()
                        && p.x.abs() <= MAX_COORD
                        && p.y.abs() <= MAX_COORD
                })
                .collect();
            assert_eq!(o.is_empty(), usable.is_empty(), "case {case}");
            for c in &o.contours {
                assert!(c.len() >= 3, "case {case}");
                assert!(c.iter().flatten().all(|v| v.is_finite()), "case {case}");
                assert!(
                    signed_area(c) <= 0.0,
                    "case {case}: contour winds the other way"
                );
            }
            for p in &usable {
                assert!(
                    o.contains(p.x, p.y),
                    "case {case}: sample ({}, {}) outside",
                    p.x,
                    p.y
                );
            }
            // Nothing reaches farther than the widest radius (plus a square end) from the
            // samples' box, and plain Catmull-Rom loops cannot run away.
            if let Some(b) = o.bounds() {
                let margin = width.max(1.0) * 1.2 + 1.0;
                let min_x = usable.iter().map(|p| p.x).fold(f32::MAX, f32::min);
                let max_x = usable.iter().map(|p| p.x).fold(f32::MIN, f32::max);
                let min_y = usable.iter().map(|p| p.y).fold(f32::MAX, f32::min);
                let max_y = usable.iter().map(|p| p.y).fold(f32::MIN, f32::max);
                let spread = (max_x - min_x).max(max_y - min_y);
                let slack = margin + spread * 0.75;
                assert!(
                    b.min_x >= min_x - slack && b.max_x <= max_x + slack,
                    "case {case}"
                );
                assert!(
                    b.min_y >= min_y - slack && b.max_y <= max_y + slack,
                    "case {case}"
                );
            }
        }
    }
}
