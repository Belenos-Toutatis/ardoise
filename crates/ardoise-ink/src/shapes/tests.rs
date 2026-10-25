use super::*;
use crate::geometry::outline;
use crate::model::{Rgba, Tool};

/// Deterministic pseudo-random numbers (64-bit LCG, Knuth's constants).
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    /// Uniform in 0..1.
    fn unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }

    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
}

/// Distance between the samples of the synthetic pen, in device units (about 100 mm/s at the
/// tablet's 200 samples per second).
const PEN_STEP: f64 = 40.0;

fn v(x: f64, y: f64) -> V {
    V::new(x, y)
}

/// Points along straight edges through `vertices`, `PEN_STEP` apart.
fn polyline(vertices: &[V]) -> Vec<V> {
    let mut out = vec![vertices[0]];
    for w in vertices.windows(2) {
        let steps = ((w[0].dist(w[1]) / PEN_STEP).ceil() as usize).max(1);
        for i in 1..=steps {
            out.push(w[0] + (w[1] - w[0]) * (i as f64 / steps as f64));
        }
    }
    out
}

/// Points along an ellipse arc (centre, semi-axes, axis angle) from `start` over `sweep`
/// radians (negative: the other way round).
fn arc(c: V, a: f64, b: f64, theta: f64, start: f64, sweep: f64) -> Vec<V> {
    let length = sweep.abs() * (a + b) / 2.0;
    let steps = ((length / PEN_STEP).ceil() as usize).max(8);
    (0..=steps)
        .map(|i| {
            let phi = start + sweep * i as f64 / steps as f64;
            c + V::new(a * phi.cos(), b * phi.sin()).rot(theta)
        })
        .collect()
}

/// A smooth curve through `controls` (Catmull-Rom), points about `PEN_STEP` apart.
fn smooth(controls: &[V]) -> Vec<V> {
    let n = controls.len();
    let at = |i: isize| controls[i.clamp(0, n as isize - 1) as usize];
    let mut out = vec![controls[0]];
    for i in 0..n as isize - 1 {
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let steps = ((p1.dist(p2) / PEN_STEP).ceil() as usize).max(2);
        for k in 1..=steps {
            let t = k as f64 / steps as f64;
            let (t2, t3) = (t * t, t * t * t);
            let point = (p1 * 2.0
                + (p2 - p0) * t
                + (p0 * 2.0 - p1 * 5.0 + p2 * 4.0 - p3) * t2
                + (p3 - p0 + p1 * 3.0 - p2 * 3.0) * t3)
                * 0.5;
            out.push(point);
        }
    }
    out
}

/// A hand-drawn stroke along `ideal`: a slow wobble of `wobble` units and a jitter of
/// `jitter` units, varying pressure, a sample every 5 ms from `t0`.
fn hand(ideal: &[V], rng: &mut Lcg, wobble: f64, jitter: f64, t0: u32) -> Stroke {
    let total = path_length(ideal).max(1.0);
    let (f1, f2, f3, f4) = (
        rng.range(1.0, 2.5),
        rng.range(3.0, 5.0),
        rng.range(1.0, 2.5),
        rng.range(3.0, 5.0),
    );
    let phases: Vec<f64> = (0..4).map(|_| rng.range(0.0, TAU)).collect();
    let mut travelled = 0.0;
    let points = ideal
        .iter()
        .enumerate()
        .map(|(i, &p)| {
            if i > 0 {
                travelled += ideal[i - 1].dist(p);
            }
            let s = travelled / total;
            let dx = wobble
                * (0.7 * (TAU * f1 * s + phases[0]).sin() + 0.3 * (TAU * f2 * s + phases[1]).sin());
            let dy = wobble
                * (0.7 * (TAU * f3 * s + phases[2]).sin() + 0.3 * (TAU * f4 * s + phases[3]).sin());
            let q = p + V::new(
                dx + rng.range(-jitter, jitter),
                dy + rng.range(-jitter, jitter),
            );
            Point {
                x: q.x as f32,
                y: q.y as f32,
                pressure: (0.55 + 0.25 * (TAU * s * 1.5).sin() + rng.range(-0.05, 0.05)) as f32,
                t: t0 + 5 * i as u32,
            }
        })
        .collect();
    Stroke {
        tool: Tool::FeltTip,
        color: Rgba::rgb(0x1e, 0x6f, 0xd9),
        width: 80.0,
        layer: 1,
        points,
    }
}

fn pts(stroke: &Stroke) -> Vec<V> {
    stroke
        .points
        .iter()
        .map(|p| V::new(f64::from(p.x), f64::from(p.y)))
        .collect()
}

fn signed_area(poly: &[V]) -> f64 {
    (0..poly.len())
        .map(|i| poly[i].cross(poly[(i + 1) % poly.len()]))
        .sum::<f64>()
        / 2.0
}

/// What every straightened stroke keeps of its source.
fn assert_like_source(new: &Stroke, source: &Stroke) {
    assert_eq!(new.tool, source.tool);
    assert_eq!(new.color, source.color);
    assert_eq!(new.width, source.width);
    assert_eq!(new.layer, source.layer);
    let pressure = median_pressure(source);
    assert!(new.points.iter().all(|p| p.pressure == pressure));
    assert!(
        new.points.windows(2).all(|w| w[0].t <= w[1].t),
        "times go forward"
    );
    let points = pts(new);
    assert!(
        points
            .windows(2)
            .all(|w| w[0].dist(w[1]) <= SAMPLE_SPACING + 1e-3),
        "dense points"
    );
}

fn single(result: &[Straightened], shape: Shape) -> &Straightened {
    assert_eq!(result.len(), 1, "{result:?}");
    assert_eq!(result[0].shape, shape);
    &result[0]
}

// --- Lines ---------------------------------------------------------------------------------

#[test]
fn wobbly_lines_become_straight_and_keep_their_ends_and_times() {
    let mut rng = Lcg::new(1);
    for (k, &(length, degrees)) in [
        (8000.0, 17.0),
        (1500.0, 63.0),
        (20_000.0, -35.0),
        (4000.0, 125.0),
    ]
    .iter()
    .enumerate()
    {
        let angle = f64::to_radians(degrees);
        let a = v(3000.0, 9000.0);
        let b = a + V::new(length, 0.0).rot(angle);
        let source = hand(&polyline(&[a, b]), &mut rng, 0.012 * length, 10.0, 1000);
        let result = straighten(std::slice::from_ref(&source));
        let line = single(&result, Shape::Line);
        assert_eq!(line.sources, [0]);
        let [new] = &line.strokes[..] else {
            panic!("one stroke")
        };
        assert_like_source(new, &source);
        let p = pts(new);
        let (first, last) = (p[0], p[p.len() - 1]);
        let raw = pts(&source);
        assert!(first.dist(raw[0]) < 0.04 * length, "case {k}: start kept");
        assert!(
            last.dist(raw[raw.len() - 1]) < 0.04 * length,
            "case {k}: end kept"
        );
        let u = (last - first).unit().unwrap();
        assert!(
            p.iter().all(|&q| (q - first).cross(u).abs() < 0.5),
            "case {k}: straight"
        );
        assert_eq!(new.points[0].t, source.points[0].t);
        assert_eq!(
            new.points.last().unwrap().t,
            source.points.last().unwrap().t
        );
    }
}

#[test]
fn nearly_horizontal_and_vertical_lines_snap_to_the_axes() {
    let mut rng = Lcg::new(2);
    for (degrees, horizontal) in [(2.5, true), (-3.0, true), (88.0, false), (-91.5, false)] {
        let a = v(5000.0, 5000.0);
        let b = a + V::new(9000.0, 0.0).rot(f64::to_radians(degrees));
        let source = hand(&polyline(&[a, b]), &mut rng, 60.0, 8.0, 0);
        let result = straighten(&[source]);
        let p = pts(&single(&result, Shape::Line).strokes[0]);
        if horizontal {
            assert!(p.iter().all(|q| (q.y - p[0].y).abs() < 0.01), "{degrees}°");
        } else {
            assert!(p.iter().all(|q| (q.x - p[0].x).abs() < 0.01), "{degrees}°");
        }
    }
    // 10° is a slanted line on purpose: kept slanted.
    let a = v(5000.0, 5000.0);
    let b = a + V::new(9000.0, 0.0).rot(f64::to_radians(10.0));
    let result = straighten(&[hand(&polyline(&[a, b]), &mut rng, 60.0, 8.0, 0)]);
    let p = pts(&single(&result, Shape::Line).strokes[0]);
    let angle = (p[p.len() - 1] - p[0]).y.atan2((p[p.len() - 1] - p[0]).x);
    assert!((angle.to_degrees() - 10.0).abs() < 1.5, "{angle}");
}

#[test]
fn pen_hooks_at_the_ends_of_a_line_are_dropped() {
    let mut rng = Lcg::new(3);
    let a = v(2000.0, 4000.0);
    let b = v(12_000.0, 5800.0);
    let ideal = polyline(&[a + V::new(-60.0, 120.0), a, b, b + V::new(90.0, 110.0)]);
    let result = straighten(&[hand(&ideal, &mut rng, 40.0, 6.0, 0)]);
    let p = pts(&single(&result, Shape::Line).strokes[0]);
    assert!(
        p[0].dist(a) < 120.0 && p[p.len() - 1].dist(b) < 120.0,
        "{:?}",
        (p[0], p[p.len() - 1])
    );
}

#[test]
fn curves_and_short_strokes_are_no_lines() {
    let mut rng = Lcg::new(4);
    // A clear arc (40° of a circle) and a line of 8 mm.
    let curve = hand(
        &arc(v(9000.0, 9000.0), 6000.0, 6000.0, 0.0, -1.9, 0.7),
        &mut rng,
        20.0,
        5.0,
        0,
    );
    let short = hand(
        &polyline(&[v(1000.0, 1000.0), v(1800.0, 1000.0)]),
        &mut rng,
        10.0,
        4.0,
        0,
    );
    assert!(straighten(&[curve, short]).is_empty());
}

// --- Closed shapes -------------------------------------------------------------------------

#[test]
fn hand_drawn_circles_and_ellipses_become_clean_and_closed() {
    let mut rng = Lcg::new(5);
    // centre, a, b, axis angle, start angle, sweep (gap or overshoot, both directions).
    let cases = [
        (v(10_000.0, 8000.0), 3000.0, 3000.0, 0.0, 0.3, TAU * 0.93),
        (v(10_000.0, 8000.0), 3000.0, 2900.0, 0.0, 2.0, -TAU * 1.12),
        (v(15_000.0, 9000.0), 5000.0, 2200.0, 0.52, -1.0, TAU * 1.05),
        (v(8000.0, 12_000.0), 2500.0, 1100.0, 1.9, 4.0, -TAU * 0.95),
        (v(6000.0, 6000.0), 700.0, 600.0, 0.0, 0.0, TAU * 1.02),
    ];
    for (k, &(c, a, b, theta, start, sweep)) in cases.iter().enumerate() {
        let source = hand(
            &arc(c, a, b, theta, start, sweep),
            &mut rng,
            0.025 * b,
            0.006 * b,
            500,
        );
        let result = straighten(std::slice::from_ref(&source));
        let ellipse = single(&result, Shape::Ellipse);
        let new = &ellipse.strokes[0];
        assert_like_source(new, &source);
        let p = pts(new);
        assert_eq!(p[0], p[p.len() - 1], "case {k}: closed");
        assert!(p.len() >= 73, "case {k}: round");
        let (w, h) = extent(&p);
        let centre = V::new(
            p.iter().map(|q| q.x).sum::<f64>() / p.len() as f64,
            p.iter().map(|q| q.y).sum::<f64>() / p.len() as f64,
        );
        assert!(centre.dist(c) < 0.06 * a, "case {k}: centre {centre:?}");
        let (ew, eh) = extent(&arc(c, a, b, theta, 0.0, TAU));
        assert!(
            (w - ew).abs() < 0.1 * a && (h - eh).abs() < 0.1 * a,
            "case {k}: size"
        );
        // Same direction as drawn, starting where the pen landed.
        assert_eq!(
            signed_area(&p).signum(),
            sweep.signum(),
            "case {k}: direction"
        );
        assert!(p[0].dist(pts(&source)[0]) < 0.1 * a, "case {k}: start");
        assert_eq!(new.points[0].t, 500);
        assert_eq!(
            new.points.last().unwrap().t,
            source.points.last().unwrap().t
        );
    }
}

#[test]
fn nearly_round_ellipses_become_circles() {
    let mut rng = Lcg::new(6);
    let source = hand(
        &arc(v(9000.0, 9000.0), 3000.0, 2800.0, 0.4, 0.0, TAU),
        &mut rng,
        50.0,
        10.0,
        0,
    );
    let result = straighten(&[source]);
    let p = pts(&single(&result, Shape::Ellipse).strokes[0]);
    let centre = V::new(
        p[..p.len() - 1].iter().map(|q| q.x).sum::<f64>() / (p.len() - 1) as f64,
        p[..p.len() - 1].iter().map(|q| q.y).sum::<f64>() / (p.len() - 1) as f64,
    );
    let radii: Vec<f64> = p.iter().map(|q| q.dist(centre)).collect();
    let (min, max) = radii
        .iter()
        .fold((f64::MAX, 0.0f64), |(lo, hi), &r| (lo.min(r), hi.max(r)));
    assert!(max - min < 1.0, "{min} {max}");
}

/// A hand-drawn rectangle: corners rounded with `radius`, from the middle of the top edge,
/// going round `turns` times (below 1: left open), turned by `theta` about its centre.
fn rectangle(c: V, w: f64, h: f64, theta: f64, radius: f64, turns: f64, reverse: bool) -> Vec<V> {
    let (hw, hh) = (w / 2.0, h / 2.0);
    let r = radius;
    let mut path = vec![v(0.0, -hh)];
    let corners = [
        (v(hw - r, -hh + r), -PI / 2.0),
        (v(hw - r, hh - r), 0.0),
        (v(-hw + r, hh - r), PI / 2.0),
        (v(-hw + r, -hh + r), PI),
    ];
    for (centre, start) in corners {
        path.extend(polyline(&[
            *path.last().unwrap(),
            centre + V::new(r, 0.0).rot(start),
        ]));
        if r > 0.0 {
            path.extend(arc(centre, r, r, 0.0, start, PI / 2.0).into_iter().skip(1));
        }
    }
    path.extend(polyline(&[*path.last().unwrap(), v(0.0, -hh)]));
    // Cut or extend along the path.
    let lap = path.clone();
    let total = path_length(&lap);
    while path_length(&path) < turns * total - 1.0 {
        path.extend(lap.iter().skip(1));
    }
    let mut cut = vec![path[0]];
    let mut travelled = 0.0;
    for w2 in path.windows(2) {
        travelled += w2[0].dist(w2[1]);
        if travelled > turns * total {
            break;
        }
        cut.push(w2[1]);
    }
    if reverse {
        for q in &mut cut {
            q.y = -q.y;
        }
    }
    cut.into_iter().map(|q| c + q.rot(theta)).collect()
}

/// Indices of the samples where the line turns by more than 60°.
fn corners_of(p: &[V]) -> Vec<usize> {
    (1..p.len() - 1)
        .filter(|&i| angle_between(p[i] - p[i - 1], p[i + 1] - p[i]) > PI / 3.0)
        .collect()
}

#[test]
fn hand_drawn_rectangles_get_sharp_corners() {
    let mut rng = Lcg::new(7);
    // centre, w, h, angle, corner radius, turns, reversed.
    let cases = [
        (v(12_000.0, 9000.0), 8000.0, 5000.0, 0.0, 250.0, 1.08, false),
        (v(12_000.0, 9000.0), 6000.0, 6200.0, 0.03, 150.0, 0.94, true),
        (
            v(14_000.0, 10_000.0),
            9000.0,
            3000.0,
            0.45,
            300.0,
            1.0,
            false,
        ),
        (v(9000.0, 9000.0), 4000.0, 2500.0, -1.2, 100.0, 1.15, true),
        (v(5000.0, 5000.0), 1200.0, 700.0, 0.2, 50.0, 1.0, false),
    ];
    for (k, &(c, w, h, theta, radius, turns, reverse)) in cases.iter().enumerate() {
        let ideal = rectangle(c, w, h, theta, radius, turns, reverse);
        let source = hand(&ideal, &mut rng, 0.02 * h, 0.004 * h, 0);
        let result = straighten(std::slice::from_ref(&source));
        let rect = single(&result, Shape::Rectangle);
        let new = &rect.strokes[0];
        assert_like_source(new, &source);
        let p = pts(new);
        assert_eq!(p[0], p[p.len() - 1], "case {k}: closed");
        let corners = corners_of(&p);
        assert_eq!(
            corners.len(),
            3,
            "case {k}: three corners between the start and the end"
        );
        for &i in &corners {
            let turn = angle_between(p[i] - p[i - 1], p[i + 1] - p[i]);
            assert!((turn - PI / 2.0).abs() < 1e-3, "case {k}: right angles");
        }
        let side = |i: usize, j: usize| p[i].dist(p[j]);
        let (s1, s2) = (side(0, corners[0]), side(corners[0], corners[1]));
        let (long, short) = (s1.max(s2), s1.min(s2));
        if h / w > SNAP_RATIO && w / h > SNAP_RATIO {
            assert!((long - short).abs() < 1.0, "case {k}: square");
        } else {
            assert!(
                (long - w.max(h)).abs() < 0.08 * w.max(h),
                "case {k}: long side {long}"
            );
            assert!(
                (short - w.min(h)).abs() < 0.12 * w.min(h),
                "case {k}: short side {short}"
            );
        }
        // The sides keep the drawing's angle (snapped when nearly axis-aligned).
        let edge = p[corners[0]] - p[0];
        let angle = edge.y.atan2(edge.x).rem_euclid(PI / 2.0);
        let expected = if theta.abs() <= SNAP_ANGLE {
            0.0
        } else {
            theta.rem_euclid(PI / 2.0)
        };
        let diff = (angle - expected).abs();
        assert!(
            diff.min(PI / 2.0 - diff) < 0.06,
            "case {k}: angle {angle} vs {expected}"
        );
        let drawn = if reverse { -1.0 } else { 1.0 };
        assert_eq!(signed_area(&p).signum(), drawn, "case {k}: direction");
    }
}

#[test]
fn the_outline_keeps_the_corners_of_a_straightened_rectangle() {
    let mut rng = Lcg::new(8);
    let source = hand(
        &rectangle(v(10_000.0, 8000.0), 6000.0, 4000.0, 0.0, 200.0, 1.05, false),
        &mut rng,
        60.0,
        15.0,
        0,
    );
    let result = straighten(&[source]);
    let new = &single(&result, Shape::Rectangle).strokes[0];
    let p = pts(new);
    let outline = outline(new);
    let radius = new.tool.width_at(new.width, new.points[0].pressure) as f64 / 2.0;
    for &i in &corners_of(&p) {
        // Just inside the round join at the outer side of the corner: a rounded centreline
        // would leave it empty.
        let centre = p.iter().fold(V::new(0.0, 0.0), |s, &q| s + q) * (1.0 / p.len() as f64);
        let outward = (p[i] - centre).unit().unwrap();
        let probe = p[i] + outward * (0.9 * radius);
        assert!(
            outline.contains(probe.x as f32, probe.y as f32),
            "corner {i} is sharp"
        );
    }
}

// --- Arrows --------------------------------------------------------------------------------

/// The ideal arrow from `tail` to `tip` with barbs of `barb` units at `angle` radians.
fn arrow_points(tail: V, tip: V, barb: f64, angle: f64) -> (V, V, V, V) {
    let back = (tail - tip).unit().unwrap();
    (
        tail,
        tip,
        tip + back.rot(angle) * barb,
        tip + back.rot(-angle) * barb,
    )
}

fn assert_arrow(result: &[Straightened], tail: V, tip: V, sources: &[usize]) {
    let arrow = single(result, Shape::Arrow);
    assert_eq!(arrow.sources, sources);
    let length = tail.dist(tip);
    // The vertices of the new strokes: their ends and corners.
    let mut vertices: Vec<V> = Vec::new();
    for stroke in &arrow.strokes {
        let p = pts(stroke);
        let mut ends = vec![p[0], p[p.len() - 1]];
        ends.extend(corners_of(&p).iter().map(|&i| p[i]));
        for q in ends {
            if vertices.iter().all(|w| w.dist(q) > 1.0) {
                vertices.push(q);
            }
        }
    }
    assert_eq!(vertices.len(), 4, "tail, tip and two barbs: {vertices:?}");
    let nearest = |to: V| {
        *vertices
            .iter()
            .min_by(|a, b| a.dist(to).total_cmp(&b.dist(to)))
            .unwrap()
    };
    let (found_tip, found_tail) = (nearest(tip), nearest(tail));
    assert!(found_tip.dist(tip) < 0.05 * length, "tip {found_tip:?}");
    assert!(found_tail.dist(tail) < 0.05 * length, "tail {found_tail:?}");
    let barbs: Vec<V> = vertices
        .iter()
        .copied()
        .filter(|&q| q != found_tip && q != found_tail)
        .collect();
    // A symmetric head: both barbs as long, at the same angle on both sides of the shaft.
    let back = (found_tail - found_tip).unit().unwrap();
    let (d0, d1) = (barbs[0].dist(found_tip), barbs[1].dist(found_tip));
    assert!((d0 - d1).abs() < 1.0, "barbs {d0} {d1}");
    let side = |q: V| back.cross(q - found_tip);
    assert!((side(barbs[0]) + side(barbs[1])).abs() < 1.0, "symmetric");
    assert!(back.dot(barbs[0] - found_tip) > 0.0, "barbs point back");
}

#[test]
fn an_arrow_drawn_in_one_stroke_is_straightened() {
    let mut rng = Lcg::new(9);
    for (k, &(tail, tip, angle)) in [
        (v(3000.0, 8000.0), v(13_000.0, 8300.0), 0.5),
        (v(9000.0, 3000.0), v(7000.0, 12_000.0), 0.4),
        (v(12_000.0, 12_000.0), v(4000.0, 6000.0), 0.7),
    ]
    .iter()
    .enumerate()
    {
        let barb = 0.15 * tail.dist(tip);
        let (t, p, b1, b2) = arrow_points(tail, tip, barb, angle);
        let ideal = if k == 2 {
            // Head first, then the shaft.
            polyline(&[b1, p, b2, p, t])
        } else {
            polyline(&[t, p, b1, p, b2])
        };
        let source = hand(&ideal, &mut rng, 40.0, 8.0, 0);
        let result = straighten(std::slice::from_ref(&source));
        assert_arrow(&result, tail, tip, &[0]);
        let new = &result[0].strokes;
        assert_eq!(new.len(), 1, "case {k}: still one stroke");
        assert_like_source(&new[0], &source);
        // Drawn in the same order: the replay starts where the pen did.
        let first = pts(&new[0])[0];
        let start = if k == 2 { b1 } else { t };
        assert!(
            first.dist(start) < 0.06 * tail.dist(tip),
            "case {k}: start {first:?}"
        );
    }
}

#[test]
fn an_arrow_drawn_in_several_strokes_is_straightened() {
    let mut rng = Lcg::new(10);
    let tail = v(4000.0, 15_000.0);
    let tip = v(14_000.0, 9000.0);
    let (t, p, b1, b2) = arrow_points(tail, tip, 1800.0, 0.45);
    let off = V::new(60.0, -40.0);
    // Line + "V" whose apex misses the tip a little.
    let shaft = hand(&polyline(&[t, p]), &mut rng, 50.0, 8.0, 0);
    let head = hand(
        &polyline(&[b1 + off, p + off, b2 + off]),
        &mut rng,
        20.0,
        6.0,
        2000,
    );
    let result = straighten(&[shaft.clone(), head.clone()]);
    assert_arrow(&result, tail, tip, &[0, 1]);
    assert_eq!(result[0].strokes.len(), 2);
    assert_like_source(&result[0].strokes[0], &shaft);
    assert_like_source(&result[0].strokes[1], &head);
    assert_eq!(result[0].strokes[1].points[0].t, 2000);

    // Line + two separate barbs, the barbs first, with unrelated strokes around.
    let barb1 = hand(&polyline(&[p, b1]), &mut rng, 15.0, 5.0, 0);
    let barb2 = hand(&polyline(&[b2, p]), &mut rng, 15.0, 5.0, 500);
    let far_line = hand(
        &polyline(&[v(2000.0, 2000.0), v(2100.0, 7000.0)]),
        &mut rng,
        30.0,
        5.0,
        900,
    );
    let shaft = hand(&polyline(&[t, p]), &mut rng, 50.0, 8.0, 1000);
    let result = straighten(&[barb1, barb2, far_line, shaft]);
    assert_eq!(result.len(), 2, "{result:?}");
    assert_eq!(result[0].shape, Shape::Arrow);
    assert_eq!(result[0].sources, [0, 1, 3]);
    assert_eq!(result[1].shape, Shape::Line);
    assert_eq!(result[1].sources, [2]);

    // The shaft and one barb in one stroke, the other barb on its own.
    let first = hand(&polyline(&[t, p, b1]), &mut rng, 40.0, 8.0, 0);
    let second = hand(&polyline(&[p, b2]), &mut rng, 15.0, 5.0, 3000);
    assert_arrow(&straighten(&[first, second]), tail, tip, &[0, 1]);
}

#[test]
fn a_line_with_a_head_on_another_layer_is_no_arrow() {
    let mut rng = Lcg::new(11);
    let (t, p, b1, b2) = arrow_points(v(4000.0, 9000.0), v(14_000.0, 9000.0), 1800.0, 0.45);
    let shaft = hand(&polyline(&[t, p]), &mut rng, 50.0, 8.0, 0);
    let mut head = hand(&polyline(&[b1, p, b2]), &mut rng, 20.0, 6.0, 2000);
    head.layer = 0;
    let result = straighten(&[shaft, head]);
    // The shaft alone is a line; the "V" is left alone.
    assert_eq!(result.len(), 1);
    assert_eq!(result[0].shape, Shape::Line);
}

// --- Things that must stay as they are -----------------------------------------------------

/// A glyph: strokes in a box of height 1 (y down), each with whether it is a straight line.
type Glyph = Vec<(Vec<V>, bool)>;

fn curve(points: &[(f64, f64)]) -> (Vec<V>, bool) {
    (points.iter().map(|&(x, y)| v(x, y)).collect(), false)
}

fn straight(points: &[(f64, f64)]) -> (Vec<V>, bool) {
    (points.iter().map(|&(x, y)| v(x, y)).collect(), true)
}

fn glyphs(rng: &mut Lcg) -> Vec<(&'static str, Glyph)> {
    let mut scribble = vec![(0.5, 0.5)];
    for _ in 0..30 {
        scribble.push((rng.range(0.0, 1.0), rng.range(0.0, 1.0)));
    }
    let spiral: Vec<(f64, f64)> = (0..=60)
        .map(|i| {
            let s = f64::from(i) / 60.0;
            let r = 0.05 + 0.45 * s;
            (
                0.5 + r * (3.0 * TAU * s).cos(),
                0.5 + r * (3.0 * TAU * s).sin(),
            )
        })
        .collect();
    vec![
        (
            "a",
            vec![curve(&[
                (0.6, 0.35),
                (0.45, 0.25),
                (0.25, 0.3),
                (0.15, 0.55),
                (0.25, 0.8),
                (0.45, 0.85),
                (0.6, 0.6),
                (0.62, 0.35),
                (0.6, 0.7),
                (0.65, 0.95),
                (0.75, 0.9),
            ])],
        ),
        (
            "e",
            vec![curve(&[
                (0.1, 0.6),
                (0.7, 0.55),
                (0.65, 0.3),
                (0.4, 0.2),
                (0.15, 0.4),
                (0.15, 0.75),
                (0.4, 0.95),
                (0.75, 0.85),
            ])],
        ),
        (
            "l",
            vec![curve(&[
                (0.0, 1.0),
                (0.25, 0.6),
                (0.35, 0.15),
                (0.25, 0.0),
                (0.15, 0.15),
                (0.2, 0.6),
                (0.3, 1.0),
                (0.45, 0.9),
            ])],
        ),
        (
            "m",
            vec![curve(&[
                (0.0, 0.5),
                (0.05, 1.0),
                (0.1, 0.6),
                (0.25, 0.5),
                (0.35, 0.7),
                (0.35, 1.0),
                (0.4, 0.6),
                (0.55, 0.5),
                (0.65, 0.7),
                (0.65, 1.0),
                (0.75, 0.95),
            ])],
        ),
        (
            "le",
            vec![curve(&[
                (0.0, 1.0),
                (0.25, 0.6),
                (0.35, 0.15),
                (0.25, 0.0),
                (0.15, 0.15),
                (0.2, 0.6),
                (0.3, 1.0),
                (0.5, 0.75),
                (0.85, 0.7),
                (0.8, 0.5),
                (0.6, 0.5),
                (0.5, 0.75),
                (0.6, 0.98),
                (0.9, 0.9),
            ])],
        ),
        (
            "U",
            vec![curve(&[
                (0.0, 0.0),
                (0.0, 0.7),
                (0.15, 0.95),
                (0.35, 1.0),
                (0.55, 0.95),
                (0.7, 0.7),
                (0.7, 0.0),
            ])],
        ),
        (
            "C",
            vec![curve(&[
                (0.7, 0.15),
                (0.45, 0.0),
                (0.15, 0.1),
                (0.0, 0.5),
                (0.15, 0.9),
                (0.45, 1.0),
                (0.7, 0.85),
            ])],
        ),
        (
            "D",
            vec![curve(&[
                (0.0, 0.0),
                (0.0, 0.5),
                (0.0, 1.0),
                (0.4, 0.95),
                (0.65, 0.5),
                (0.4, 0.05),
                (0.02, 0.02),
            ])],
        ),
        (
            "S",
            vec![curve(&[
                (0.7, 0.1),
                (0.4, 0.0),
                (0.1, 0.15),
                (0.2, 0.4),
                (0.55, 0.55),
                (0.7, 0.8),
                (0.4, 1.0),
                (0.05, 0.9),
            ])],
        ),
        (
            "2",
            vec![curve(&[
                (0.05, 0.25),
                (0.3, 0.0),
                (0.6, 0.1),
                (0.55, 0.45),
                (0.0, 1.0),
                (0.4, 0.98),
                (0.7, 1.0),
            ])],
        ),
        (
            "8",
            vec![curve(&[
                (0.5, 0.45),
                (0.75, 0.25),
                (0.5, 0.0),
                (0.25, 0.25),
                (0.5, 0.45),
                (0.8, 0.75),
                (0.5, 1.0),
                (0.2, 0.75),
                (0.5, 0.45),
            ])],
        ),
        (
            "6",
            vec![curve(&[
                (0.6, 0.0),
                (0.25, 0.3),
                (0.1, 0.7),
                (0.35, 1.0),
                (0.6, 0.8),
                (0.5, 0.5),
                (0.2, 0.6),
            ])],
        ),
        ("V", vec![straight(&[(0.0, 0.0), (0.35, 1.0), (0.7, 0.0)])]),
        (
            "N",
            vec![straight(&[
                (0.0, 1.0),
                (0.0, 0.0),
                (0.55, 1.0),
                (0.55, 0.0),
            ])],
        ),
        (
            "Z",
            vec![straight(&[(0.0, 0.0), (0.6, 0.0), (0.0, 1.0), (0.6, 1.0)])],
        ),
        (
            "Y",
            vec![
                straight(&[(0.0, 0.0), (0.3, 0.45), (0.6, 0.0)]),
                straight(&[(0.3, 0.45), (0.3, 1.0)]),
            ],
        ),
        (
            "K",
            vec![
                straight(&[(0.0, 0.0), (0.0, 1.0)]),
                straight(&[(0.55, 0.0), (0.02, 0.55), (0.6, 1.0)]),
            ],
        ),
        (
            "4",
            vec![
                straight(&[(0.45, 0.0), (0.0, 0.65), (0.65, 0.65)]),
                straight(&[(0.45, 0.05), (0.45, 1.0)]),
            ],
        ),
        (
            "A",
            vec![
                straight(&[(0.0, 1.0), (0.3, 0.0), (0.6, 1.0)]),
                straight(&[(0.15, 0.6), (0.45, 0.6)]),
            ],
        ),
        (
            "1",
            vec![straight(&[(0.15, 0.2), (0.45, 0.0), (0.45, 1.0)])],
        ),
        (
            "F",
            vec![
                straight(&[(0.55, 0.0), (0.0, 0.0), (0.0, 1.0)]),
                straight(&[(0.0, 0.5), (0.4, 0.5)]),
            ],
        ),
        (
            "x",
            vec![
                straight(&[(0.0, 0.3), (0.6, 1.0)]),
                straight(&[(0.6, 0.3), (0.0, 1.0)]),
            ],
        ),
        ("scribble", vec![curve(&scribble)]),
        ("spiral", vec![curve(&spiral)]),
    ]
}

/// The glyph's strokes, `height` units high at `origin`, hand-drawn.
fn write(glyph: &Glyph, origin: V, height: f64, rng: &mut Lcg) -> Vec<(Stroke, bool)> {
    glyph
        .iter()
        .enumerate()
        .map(|(i, (controls, is_straight))| {
            let placed: Vec<V> = controls.iter().map(|&q| origin + q * height).collect();
            let ideal = if *is_straight {
                polyline(&placed)
            } else {
                smooth(&placed)
            };
            (
                hand(&ideal, rng, 0.012 * height, 0.004 * height, 1000 * i as u32),
                *is_straight,
            )
        })
        .collect()
}

#[test]
fn handwriting_is_never_straightened() {
    let mut rng = Lcg::new(12);
    for (name, glyph) in glyphs(&mut rng) {
        // At handwriting size nothing changes at all.
        for height in [400.0, 650.0, 900.0] {
            let strokes: Vec<Stroke> = write(&glyph, v(3000.0, 3000.0), height, &mut rng)
                .into_iter()
                .map(|(s, _)| s)
                .collect();
            let result = straighten(&strokes);
            assert!(result.is_empty(), "{name} at {height}: {result:?}");
        }
        // Even very large, a curved letter stays as it is, and the strokes of a letter never
        // make an arrow or a closed shape: at most a stroke that is itself a straight line is
        // made straight.
        for height in [1800.0, 3000.0] {
            let written = write(&glyph, v(3000.0, 3000.0), height, &mut rng);
            let strokes: Vec<Stroke> = written.iter().map(|(s, _)| s.clone()).collect();
            for group in straighten(&strokes) {
                assert_eq!(group.shape, Shape::Line, "{name} at {height}: {group:?}");
                assert!(
                    group.sources.iter().all(|&i| written[i].1),
                    "{name} at {height}: {group:?}"
                );
                let (glyph_stroke, _) = &glyph[group.sources[0]];
                assert_eq!(
                    glyph_stroke.len(),
                    2,
                    "{name}: only a single straight segment"
                );
            }
        }
    }
}

#[test]
fn a_selection_mixing_shapes_and_writing_changes_only_the_shapes() {
    let mut rng = Lcg::new(13);
    let mut strokes: Vec<Stroke> = Vec::new();
    for (k, (_, glyph)) in glyphs(&mut rng).into_iter().enumerate() {
        let origin = v(
            1000.0 + 700.0 * (k % 12) as f64,
            2000.0 + 1200.0 * (k / 12) as f64,
        );
        strokes.extend(
            write(&glyph, origin, 600.0, &mut rng)
                .into_iter()
                .map(|(s, _)| s),
        );
    }
    let writing = strokes.len();
    strokes.push(hand(
        &arc(v(12_000.0, 12_000.0), 2500.0, 2500.0, 0.0, 0.0, TAU),
        &mut rng,
        50.0,
        10.0,
        0,
    ));
    strokes.push(hand(
        &polyline(&[v(2000.0, 16_000.0), v(20_000.0, 16_300.0)]),
        &mut rng,
        80.0,
        10.0,
        0,
    ));
    let result = straighten(&strokes);
    let shapes: Vec<(Shape, Vec<usize>)> = result
        .iter()
        .map(|g| (g.shape, g.sources.clone()))
        .collect();
    assert_eq!(
        shapes,
        [
            (Shape::Ellipse, vec![writing]),
            (Shape::Line, vec![writing + 1])
        ]
    );
}

#[test]
fn degenerate_input_is_harmless() {
    let empty = Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 50.0,
        layer: 0,
        points: Vec::new(),
    };
    let mut dot = empty.clone();
    dot.points = vec![
        Point {
            x: 10.0,
            y: 10.0,
            pressure: 0.5,
            t: 0
        };
        5
    ];
    let mut wild = empty.clone();
    wild.points = (0..50)
        .map(|i| Point {
            x: if i % 7 == 0 { f32::NAN } else { i as f32 * 1e8 },
            y: if i % 5 == 0 { f32::INFINITY } else { 3.0 },
            pressure: f32::NAN,
            t: i,
        })
        .collect();
    assert!(straighten(&[]).is_empty());
    assert!(straighten(&[empty, dot]).is_empty());
    // Whatever the odd samples give, the result is finite.
    for group in straighten(&[wild]) {
        for p in group.strokes.iter().flat_map(|s| &s.points) {
            assert!(p.x.is_finite() && p.y.is_finite() && p.pressure.is_finite());
        }
    }
}

#[test]
fn the_same_input_gives_the_same_output() {
    let mut rng = Lcg::new(14);
    let strokes = vec![
        hand(
            &rectangle(v(9000.0, 9000.0), 5000.0, 3000.0, 0.3, 200.0, 1.05, false),
            &mut rng,
            60.0,
            12.0,
            0,
        ),
        hand(
            &arc(v(20_000.0, 9000.0), 3000.0, 1800.0, 0.2, 0.0, TAU),
            &mut rng,
            50.0,
            10.0,
            0,
        ),
    ];
    assert_eq!(straighten(&strokes), straighten(&strokes));
}

#[test]
fn a_gentle_wave_is_no_line() {
    // ±3 mm over 9 cm, a period and a half: visibly curved.
    let ideal: Vec<V> = (0..60)
        .map(|i| v(3000.0 + 9000.0 * f64::from(i) / 59.0, 4000.0 + 300.0 * (f64::from(i) / 6.0).sin()))
        .collect();
    let mut rng = Lcg::new(15);
    assert!(straighten(&[hand(&ideal, &mut rng, 0.0, 0.0, 0)]).is_empty());
}
