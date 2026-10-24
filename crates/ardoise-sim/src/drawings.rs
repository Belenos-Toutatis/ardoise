//! Sample drawings for the simulated tablet, in device units (10 µm), as a pen moving at
//! about 100 mm/s would produce them (one sample every 5 ms, 0.5 mm apart).
//!
//! - Three reference pages, the ones the guided test asks the user to draw:
//!   [`square_and_circle`], [`scribble`] (handwriting standing for "Ardoise 2026") and
//!   [`pressure_lines`] (light, normal and heavy pressure).
//! - [`live_script`]: a few seconds of live-mode handwriting with hovering, pauses and varying
//!   pressure.
//!
//! Everything is deterministic and stays inside the drawing area it is given.

use std::f64::consts::{PI, TAU};
use std::time::Duration;

use ardoise_proto::SLATE_MAX_PRESSURE;
use ardoise_proto::stroke::{Point, Stroke, StrokeTime};

use crate::pen::{PenScript, PenSegment};

/// Distance the pen travels between two samples, in device units.
const STEP: f64 = 50.0;
/// Samples over which the pressure rises at the start of a stroke and falls at its end.
const PRESSURE_RAMP: f64 = 6.0;

/// The reference pages, in the order the guided test asks for them.
pub fn reference_pages(width: u32, height: u32) -> Vec<Vec<Stroke>> {
    vec![
        square_and_circle(width, height),
        scribble(width, height),
        pressure_lines(width, height),
    ]
}

/// Reference page 1: a big square around a circle, centred, each in one stroke.
pub fn square_and_circle(width: u32, height: u32) -> Vec<Stroke> {
    let area = Area::page(width, height);
    let (cx, cy) = (area.width / 2.0, area.height / 2.0);
    let half = area.height * 0.3;
    let square = [
        (cx - half, cy - half),
        (cx + half, cy - half),
        (cx + half, cy + half),
        (cx - half, cy + half),
        (cx - half, cy - half),
    ];
    let radius = area.height * 0.25;
    let circle = curve(
        |t| (cx + radius * t.cos(), cy - radius * t.sin()),
        0.0,
        TAU,
        720,
    );
    vec![
        area.stroke(&resample(&square, STEP), 1100.0),
        area.stroke(&resample(&circle, STEP), 1100.0),
    ]
}

/// Reference page 2: handwriting standing for "Ardoise 2026": a cursive word of seven loops
/// with the dot of the i, then four digits.
pub fn scribble(width: u32, height: u32) -> Vec<Stroke> {
    let area = Area::page(width, height);
    let mut strokes = Vec::new();

    // "Ardoise": one loop per letter (a trochoid whose loop height changes with the letter).
    let x0 = area.width * 0.12;
    let y0 = area.height * 0.38;
    let advance = area.width * 0.012;
    let loops = [700.0, 330.0, 650.0, 340.0, 300.0, 320.0, 340.0];
    let word = curve(
        |t| {
            let letter = ((t / TAU) as usize).min(loops.len() - 1);
            let b = loops[letter];
            let slope = -t * 6.0;
            (
                x0 + advance * t - b * t.sin(),
                y0 + slope - b * (1.0 - t.cos()),
            )
        },
        0.0,
        TAU * loops.len() as f64,
        2400,
    );
    strokes.push(area.stroke(&resample(&word, STEP), 950.0));
    // The dot of the i (fifth letter).
    let dot_x = x0 + advance * (TAU * 4.5);
    let dot_y = y0 - 1000.0;
    strokes.push(area.stroke(
        &resample(&[(dot_x, dot_y), (dot_x + 120.0, dot_y + 60.0)], STEP),
        800.0,
    ));

    // "2026".
    let digit_height = area.height * 0.11;
    let digit_width = digit_height * 0.6;
    let baseline = area.height * 0.66;
    let mut x = area.width * 0.22;
    for digit in ['2', '0', '2', '6'] {
        let path = digit_path(digit, x, baseline, digit_width, digit_height);
        strokes.push(area.stroke(&resample(&path, STEP), 1200.0));
        x += digit_width * 1.45;
    }
    strokes
}

/// Reference page 3: three horizontal lines drawn with light, normal and heavy pressure.
pub fn pressure_lines(width: u32, height: u32) -> Vec<Stroke> {
    let area = Area::page(width, height);
    [(0.3, 300.0), (0.5, 1000.0), (0.7, 1800.0)]
        .into_iter()
        .map(|(row, pressure)| {
            let y = area.height * row;
            let line = [(area.width * 0.12, y), (area.width * 0.88, y)];
            area.stroke(&resample(&line, STEP), pressure)
        })
        .collect()
}

/// `seconds` of live-mode handwriting (see [`live_script_seeded`]) with a fixed seed.
pub fn live_script(seconds: u32, width: u32, height: u32) -> PenScript {
    live_script_seeded(seconds, width, height, 0x5eed_a2d0_1520_2026)
}

/// `seconds` of live-mode handwriting: the pen comes near for 2 to 5 s at a time, writes
/// cursive fragments of 0.3 to 1.2 s with varying pressure, hovers (pressure 0) between them,
/// then leaves for 80 to 200 ms. The script lasts exactly `seconds` and ends with the pen
/// away for at least 100 ms, so a host streaming it for `seconds` receives every sample.
///
/// Positions stay in the live area of protocol.md §7.2 (x from 2500 to width − 1000, y from
/// 500 to height − 500).
pub fn live_script_seeded(seconds: u32, width: u32, height: u32, seed: u64) -> PenScript {
    const MIN_NEAR_MS: u64 = 400;
    const MIN_TAIL_MS: u64 = 100;
    let area = Area::live(width, height);
    let mut rng = Rng::new(seed);
    let mut hand = Hand::new(&area);
    let total = u64::from(seconds) * 1000;
    let mut used = 0;
    let mut script = PenScript::new();
    while total - used >= MIN_NEAR_MS + MIN_TAIL_MS {
        let budget = rng.range(2000, 5000).min(total - used - MIN_TAIL_MS) / 5 * 5;
        // The last 5 ms slot is the "pen left" record.
        let samples = usize::try_from(budget / 5 - 1).unwrap_or(0);
        script.push(PenSegment::Near(hand.session(&mut rng, samples, &area)));
        used += budget;
        let remaining = total - used;
        let pause = rng.range(80, 200) / 5 * 5;
        let pause = if remaining < pause + MIN_NEAR_MS + MIN_TAIL_MS {
            remaining
        } else {
            pause
        };
        script.push(PenSegment::Away(Duration::from_millis(pause)));
        used += pause;
    }
    if used < total {
        script.push(PenSegment::Away(Duration::from_millis(total - used)));
    }
    script
}

/// Where points may go, as floating-point device units.
struct Area {
    width: f64,
    height: f64,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
}

impl Area {
    fn page(width: u32, height: u32) -> Self {
        let (width, height) = (f64::from(width), f64::from(height));
        Self {
            width,
            height,
            x_min: 0.0,
            x_max: (width - 1.0).max(0.0),
            y_min: 0.0,
            y_max: (height - 1.0).max(0.0),
        }
    }

    fn live(width: u32, height: u32) -> Self {
        let page = Self::page(width, height);
        Self {
            x_min: 2500.0_f64.min(page.x_max),
            x_max: (page.width - 1000.0).max(2500.0_f64.min(page.x_max)),
            y_min: 500.0_f64.min(page.y_max),
            y_max: (page.height - 500.0).max(500.0_f64.min(page.y_max)),
            ..page
        }
    }

    fn point(&self, x: f64, y: f64, pressure: f64) -> Point {
        Point {
            x: x.clamp(self.x_min, self.x_max).round() as u16,
            y: y.clamp(self.y_min, self.y_max).round() as u16,
            pressure: pressure.clamp(0.0, f64::from(SLATE_MAX_PRESSURE)).round() as u16,
        }
    }

    /// A pen-down stroke along `path` around `pressure`, rising at the start and falling at
    /// the end, with a slight wobble.
    fn stroke(&self, path: &[(f64, f64)], pressure: f64) -> Stroke {
        let n = path.len() as f64;
        let points = path
            .iter()
            .enumerate()
            .map(|(i, &(x, y))| {
                let i = i as f64;
                let ramp = ((i + 1.0) / PRESSURE_RAMP)
                    .min((n - i) / PRESSURE_RAMP)
                    .clamp(0.25, 1.0);
                let wobble = 1.0 + 0.08 * (i * 0.11).sin();
                self.point(x, y, (pressure * ramp * wobble).max(1.0))
            })
            .collect();
        Stroke {
            time: StrokeTime::Unknown,
            pen: None,
            points,
            lost_points: 0,
        }
    }
}

/// `steps + 1` points of the parametric curve `f` for `t` from `t0` to `t1`.
fn curve(f: impl Fn(f64) -> (f64, f64), t0: f64, t1: f64, steps: usize) -> Vec<(f64, f64)> {
    let steps = steps.max(1);
    (0..=steps)
        .map(|i| f(t0 + (t1 - t0) * i as f64 / steps as f64))
        .collect()
}

/// Points every `step` units along the polyline `path`, from its first point to its last.
fn resample(path: &[(f64, f64)], step: f64) -> Vec<(f64, f64)> {
    let Some(&first) = path.first() else {
        return Vec::new();
    };
    let mut out = vec![first];
    // Distance travelled since the last emitted point.
    let mut carry = 0.0;
    for pair in path.windows(2) {
        let [(ax, ay), (bx, by)] = pair else {
            continue;
        };
        let length = (bx - ax).hypot(by - ay);
        if length <= f64::EPSILON {
            continue;
        }
        let mut at = step - carry;
        while at <= length {
            let t = at / length;
            out.push((ax + (bx - ax) * t, ay + (by - ay) * t));
            at += step;
        }
        carry = length - (at - step);
    }
    if let (Some(&(lx, ly)), Some(&(px, py))) = (path.last(), out.last())
        && (lx - px).hypot(ly - py) > step * 0.25
    {
        out.push((lx, ly));
    }
    out
}

/// Outline of a handwritten digit whose bounding box starts at `x`, sits on `baseline`.
fn digit_path(digit: char, x: f64, baseline: f64, width: f64, height: f64) -> Vec<(f64, f64)> {
    let r = width / 2.0;
    let cx = x + r;
    match digit {
        '0' => {
            let cy = baseline - height / 2.0;
            curve(
                |t| (cx + r * t.cos(), cy - height / 2.0 * t.sin()),
                PI / 2.0,
                PI / 2.0 + TAU * 1.03,
                180,
            )
        }
        '6' => {
            let cy = baseline - height / 2.0;
            let mut path = curve(
                |t| (cx + r * t.cos(), cy - height / 2.0 * t.sin()),
                PI * 0.4,
                PI * 1.5,
                120,
            );
            let loop_r = r * 0.9;
            let loop_cy = baseline - loop_r;
            path.extend(curve(
                |t| (cx + loop_r * t.cos(), loop_cy - loop_r * t.sin()),
                PI * 1.5,
                PI * 3.5,
                120,
            ));
            path
        }
        // '2' and anything else.
        _ => {
            let cy = baseline - height + r;
            let mut path = curve(|t| (cx + r * t.cos(), cy - r * t.sin()), PI, -PI / 5.0, 90);
            path.push((x, baseline));
            path.push((x + width, baseline));
            path
        }
    }
}

/// The simulated hand of [`live_script_seeded`].
struct Hand {
    x: f64,
    /// Baseline of the line being written.
    y: f64,
    phase: f64,
}

impl Hand {
    fn new(area: &Area) -> Self {
        Self {
            x: area.x_min + 500.0,
            y: area.y_min + 1500.0,
            phase: 0.0,
        }
    }

    /// `samples` samples near the tablet: cursive fragments separated by hovering.
    fn session(&mut self, rng: &mut Rng, samples: usize, area: &Area) -> Vec<Point> {
        let mut out = Vec::with_capacity(samples);
        while out.len() < samples {
            let length = usize::try_from(rng.range(60, 240)).unwrap_or(60);
            let peak = rng.range(700, 1800) as f64;
            let loop_height = rng.range(220, 420) as f64;
            self.write(&mut out, samples, length, peak, loop_height, area);
            if out.len() >= samples {
                break;
            }
            let gap = rng.range(150, 500) as f64;
            let hover = usize::try_from(rng.range(10, 40)).unwrap_or(10);
            if self.x + gap > area.x_max - 2000.0 {
                // Next line.
                let mut y = self.y + 1600.0;
                if y > area.y_max - 500.0 {
                    y = area.y_min + 1500.0;
                }
                let samples_to_move = usize::try_from(rng.range(60, 100)).unwrap_or(60);
                self.hover(
                    &mut out,
                    samples,
                    samples_to_move,
                    area.x_min + 500.0,
                    y,
                    area,
                );
            } else {
                self.hover(&mut out, samples, hover, self.x + gap, self.y, area);
            }
        }
        out
    }

    /// A cursive fragment of up to `length` samples, pen down.
    fn write(
        &mut self,
        out: &mut Vec<Point>,
        limit: usize,
        length: usize,
        peak: f64,
        loop_height: f64,
        area: &Area,
    ) {
        let advance = 170.0;
        let (base_x, base_y) = (self.x, self.y);
        let n = length.min(limit - out.len());
        for i in 0..n {
            let t = (i + 1) as f64 * 0.16;
            let x = base_x + advance * t - loop_height * t.sin();
            let y = base_y - loop_height * (1.0 - t.cos());
            let i = i as f64;
            let ramp = ((i + 1.0) / 8.0).min((n as f64 - i) / 8.0).clamp(0.2, 1.0);
            let pressure = peak * ramp * (0.75 + 0.25 * (i * 0.07 + self.phase).sin());
            out.push(area.point(x, y, pressure.max(1.0)));
            self.x = x.clamp(area.x_min, area.x_max);
        }
        self.phase += 1.3;
    }

    /// Hovering (pressure 0) in a straight line to (`x`, `y`) over `length` samples.
    fn hover(
        &mut self,
        out: &mut Vec<Point>,
        limit: usize,
        length: usize,
        x: f64,
        y: f64,
        area: &Area,
    ) {
        let (from_x, from_y) = (self.x, self.y);
        let n = length.min(limit - out.len());
        for i in 0..n {
            let t = (i + 1) as f64 / length as f64;
            out.push(area.point(from_x + (x - from_x) * t, from_y + (y - from_y) * t, 0.0));
        }
        if n == length {
            self.x = x.clamp(area.x_min, area.x_max);
            self.y = y.clamp(area.y_min, area.y_max);
        }
    }
}

/// A small deterministic generator (xorshift64*), so drawings are the same at every run.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// A number from `lo` to `hi` inclusive.
    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        let (lo, hi) = (lo.min(hi), lo.max(hi));
        lo + self.next() % (hi - lo + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_proto::{SLATE_LARGE_HEIGHT as H, SLATE_LARGE_WIDTH as W};

    fn inside(strokes: &[Stroke], width: u32, height: u32) -> bool {
        strokes.iter().flat_map(|s| &s.points).all(|p| {
            u32::from(p.x) < width && u32::from(p.y) < height && p.pressure <= SLATE_MAX_PRESSURE
        })
    }

    #[test]
    fn reference_pages_fit_both_slates_and_touch_the_paper() {
        for (width, height) in [(W, H), (21_600, 14_800)] {
            let pages = reference_pages(width, height);
            assert_eq!(pages.len(), 3);
            for page in &pages {
                assert!(!page.is_empty());
                assert!(inside(page, width, height));
                for stroke in page {
                    assert!(stroke.points.len() > 1);
                    assert!(stroke.points.iter().all(|p| p.pressure > 0));
                }
            }
        }
    }

    #[test]
    fn reference_pages_have_the_expected_shapes() {
        assert_eq!(square_and_circle(W, H).len(), 2);
        // Word, dot of the i, four digits.
        assert_eq!(scribble(W, H).len(), 6);
        let lines = pressure_lines(W, H);
        assert_eq!(lines.len(), 3);
        let middle = |s: &Stroke| s.points[s.points.len() / 2].pressure;
        assert!(middle(&lines[0]) < middle(&lines[1]));
        assert!(middle(&lines[1]) < middle(&lines[2]));
        // About 0.5 mm between samples.
        let square = &square_and_circle(W, H)[0];
        let (a, b) = (square.points[10], square.points[11]);
        let distance = (f64::from(a.x) - f64::from(b.x)).hypot(f64::from(a.y) - f64::from(b.y));
        assert!((45.0..=55.0).contains(&distance), "{distance}");
    }

    #[test]
    fn live_script_lasts_exactly_and_ends_away() {
        for seconds in [1, 3, 30] {
            let script = live_script(seconds, W, H);
            assert_eq!(script.duration(), Duration::from_secs(u64::from(seconds)));
            match script.segments().last() {
                Some(PenSegment::Away(pause)) => assert!(*pause >= Duration::from_millis(100)),
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(live_script(0, W, H).sample_count(), 0);
    }

    #[test]
    fn live_script_stays_in_the_live_area_with_hover_and_pressure() {
        let script = live_script(30, W, H);
        let samples = script.sample_count();
        assert!((5600..6000).contains(&samples), "{samples}");
        assert!(script.approach_count() >= 5);
        let points: Vec<_> = script.points().collect();
        assert!(
            points
                .iter()
                .all(|p| (2500..=W - 1000).contains(&u32::from(p.x)))
        );
        assert!(
            points
                .iter()
                .all(|p| (500..=H - 500).contains(&u32::from(p.y)))
        );
        assert!(points.iter().any(|p| p.pressure == 0), "hovering");
        assert!(points.iter().any(|p| p.pressure > 1500), "heavy");
        assert!(points.iter().all(|p| p.pressure <= SLATE_MAX_PRESSURE));
        assert_eq!(script, live_script(30, W, H), "deterministic");
    }

    #[test]
    fn resample_keeps_a_constant_step() {
        let points = resample(&[(0.0, 0.0), (100.0, 0.0), (100.0, 100.0)], 25.0);
        assert_eq!(points.len(), 9);
        assert_eq!(points[4], (100.0, 0.0));
        assert_eq!(points[8], (100.0, 100.0));
    }
}
