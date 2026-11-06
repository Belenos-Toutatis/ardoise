//! Boxes, medians, the lower outline and the line through it, smoothing. Millimetres everywhere
//! except in [`smooth_stroke`], which works on device units.

use std::collections::BTreeMap;

use ardoise_ink::{Point, Stroke};

use crate::report::SlopeFrom;

/// Device units (10 µm) per millimetre.
pub(crate) const UNITS: f32 = 100.0;
/// Width of the bins of the outline of a line, millimetres.
const BIN_MM: f32 = 1.0;
/// Two samples of the outline must be this far apart to give a slope, millimetres.
pub(crate) const FIT_MIN_DX_MM: f32 = 12.0;
/// A line narrower than this takes slope 0 (the fit is wrong on short lines), millimetres.
const SHORT_LINE_MM: f32 = 40.0;
/// A line of fewer bins than this takes slope 0.
const SHORT_LINE_BINS: usize = 8;
/// More samples than this are thinned before a fit: the cost grows with the square.
const MAX_FIT_POINTS: usize = 1000;
/// A sample is averaged with at most this many neighbours on each side; beyond, it takes them
/// evenly (each standing for those around it). Writing at the tablet's sampling rate never has so
/// many within a usual smoothing, so its result does not change; a very dense stroke costs little.
const SMOOTH_NEIGHBOURS: usize = 64;
/// Default word gap: this many body heights...
const WORD_GAP_BODIES: f32 = 1.2;
/// ...and at least this many millimetres.
const WORD_GAP_MIN_MM: f32 = 2.5;

pub(crate) fn round2(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}

/// A box on the page, millimetres.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Area {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl Area {
    pub fn around(mut points: impl Iterator<Item = [f32; 2]>) -> Option<Self> {
        let [x, y] = points.next()?;
        let mut area = Area {
            x0: x,
            y0: y,
            x1: x,
            y1: y,
        };
        for [x, y] in points {
            area.x0 = area.x0.min(x);
            area.y0 = area.y0.min(y);
            area.x1 = area.x1.max(x);
            area.y1 = area.y1.max(y);
        }
        Some(area)
    }

    pub fn width(&self) -> f32 {
        self.x1 - self.x0
    }

    pub fn height(&self) -> f32 {
        self.y1 - self.y0
    }

    /// The larger side.
    pub fn size(&self) -> f32 {
        self.width().max(self.height())
    }

    pub fn centre(&self) -> [f32; 2] {
        [(self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0]
    }

    pub fn union(self, other: Area) -> Area {
        Area {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }
}

/// The samples of a point list that can be drawn, millimetres (the drawing skips the others).
pub(crate) fn samples_of(points: &[Point]) -> impl Iterator<Item = [f32; 2]> + '_ {
    points
        .iter()
        .filter(|p| p.x.is_finite() && p.y.is_finite())
        .map(|p| [p.x / UNITS, p.y / UNITS])
}

pub(crate) fn area_of(points: &[Point]) -> Option<Area> {
    Area::around(samples_of(points))
}

/// The middle value (the upper one of an even count), `None` without values.
pub(crate) fn median(values: &mut [f32]) -> Option<f32> {
    values.sort_unstable_by(f32::total_cmp);
    values.get(values.len() / 2).copied()
}

/// Lower outline (largest y, the page's y pointing down) or upper outline of these points, one
/// value per bin, left to right.
pub(crate) fn outline(points: impl Iterator<Item = [f32; 2]>, lower: bool) -> Vec<[f32; 2]> {
    let mut bins: BTreeMap<i32, f32> = BTreeMap::new();
    for [x, y] in points {
        let key = (x / BIN_MM).floor() as i32;
        let slot = bins.entry(key).or_insert(y);
        *slot = if lower { slot.max(y) } else { slot.min(y) };
    }
    bins.into_iter()
        .map(|(key, y)| [(key as f32 + 0.5) * BIN_MM, y])
        .collect()
}

/// Theil-Sen: the median slope of the pairs at least `min_dx` apart. `None` without such a pair.
pub(crate) fn theil_sen(points: &[[f32; 2]], min_dx: f32) -> Option<f32> {
    let step = points.len() / MAX_FIT_POINTS + 1;
    let thinned: Vec<[f32; 2]> = points.iter().step_by(step).copied().collect();
    let mut slopes = Vec::new();
    for (i, a) in thinned.iter().enumerate() {
        for b in thinned.iter().skip(i + 1) {
            let dx = b[0] - a[0];
            if dx.abs() >= min_dx {
                slopes.push((b[1] - a[1]) / dx);
            }
        }
    }
    median(&mut slopes)
}

/// The line through a written line's strokes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LineFit {
    /// dy/dx.
    pub slope: f32,
    /// y at x = 0.
    pub intercept: f32,
    /// Height of the body of the letters: from the middle of the upper outline to the middle of
    /// the lower one.
    pub body: f32,
    pub x0: f32,
    pub x1: f32,
    pub slope_from: SlopeFrom,
}

impl LineFit {
    pub fn at(&self, x: f32) -> f32 {
        self.intercept + self.slope * x
    }

    pub fn slope_degrees(&self) -> f32 {
        self.slope.atan().to_degrees()
    }
}

/// Fits the baseline of a written line: lower outline (1 mm bins), Theil-Sen through it. `slope`
/// (degrees) overrides the fit; without it, a short line takes slope 0. `None` without any sample.
pub(crate) fn fit_line<'s>(
    strokes: impl Iterator<Item = &'s Stroke>,
    slope: Option<f32>,
) -> Option<LineFit> {
    let all: Vec<[f32; 2]> = strokes.flat_map(|s| samples_of(&s.points)).collect();
    let area = Area::around(all.iter().copied())?;
    let bottoms = outline(all.iter().copied(), true);
    let tops = outline(all.iter().copied(), false);
    let (slope, slope_from) = match slope {
        Some(degrees) => (degrees.to_radians().tan(), SlopeFrom::Given),
        None if area.width() < SHORT_LINE_MM || bottoms.len() < SHORT_LINE_BINS => {
            (0.0, SlopeFrom::ShortLine)
        }
        None => (
            theil_sen(&bottoms, FIT_MIN_DX_MM).unwrap_or(0.0),
            SlopeFrom::Fitted,
        ),
    };
    let mut rest: Vec<f32> = bottoms.iter().map(|p| p[1] - slope * p[0]).collect();
    let intercept = median(&mut rest)?;
    let mut low: Vec<f32> = bottoms.iter().map(|p| p[1] - slope * p[0]).collect();
    let mut high: Vec<f32> = tops.iter().map(|p| p[1] - slope * p[0]).collect();
    let body = (median(&mut low)? - median(&mut high)?).max(0.0);
    Some(LineFit {
        slope,
        intercept,
        body,
        x0: area.x0,
        x1: area.x1,
        slope_from,
    })
}

/// The blank that separates two words when the order does not say: `max(1.2 × body, 2.5 mm)`.
pub(crate) fn default_word_gap(body: f32) -> f32 {
    (WORD_GAP_BODIES * body).max(WORD_GAP_MIN_MM)
}

/// A stroke seen from above, to cut a line into words.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Span {
    pub number: usize,
    pub x0: f32,
    pub x1: f32,
}

/// Words of a line: the strokes in x order, cut where the blank is wider than `threshold`. Gives
/// the words (each with its extent), and the blanks between them.
pub(crate) fn cut_words(mut spans: Vec<Span>, threshold: f32) -> (Vec<Word>, Vec<f32>) {
    spans.sort_by(|a, b| a.x0.total_cmp(&b.x0).then(a.number.cmp(&b.number)));
    let mut words: Vec<Word> = Vec::new();
    let mut gaps = Vec::new();
    for span in spans {
        match words.last_mut() {
            Some(word) if span.x0 - word.x1 <= threshold => {
                word.strokes.push(span.number);
                word.x1 = word.x1.max(span.x1);
            }
            last => {
                if let Some(word) = last {
                    gaps.push(span.x0 - word.x1);
                }
                words.push(Word {
                    strokes: vec![span.number],
                    x0: span.x0,
                    x1: span.x1,
                });
            }
        }
    }
    (words, gaps)
}

/// Strokes that make a word, left to right.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Word {
    pub strokes: Vec<usize>,
    pub x0: f32,
    pub x1: f32,
}

/// At most [`SMOOTH_NEIGHBOURS`] of these indices, evenly spread over them, each with the number of
/// indices it stands for.
fn spread(range: std::ops::Range<usize>) -> impl Iterator<Item = (usize, f64)> {
    let end = range.end;
    let group = range.len().div_ceil(SMOOTH_NEIGHBOURS).max(1);
    range.step_by(group).map(move |start| {
        let members = group.min(end - start);
        (start + (members - 1) / 2, members as f64)
    })
}

/// Removes the tremble: each sample becomes a Gaussian average of its neighbours along the
/// stroke, the window shrinking near the ends so that the ends stay where they are. A stroke with a
/// sample that cannot be drawn is left alone.
pub(crate) fn smooth_stroke(stroke: &mut Stroke, sigma_mm: f32) {
    let count = stroke.points.len();
    if count < 5
        || stroke
            .points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite())
    {
        return;
    }
    let sigma = f64::from(sigma_mm * UNITS);
    let old: Vec<[f64; 2]> = stroke
        .points
        .iter()
        .map(|p| [f64::from(p.x), f64::from(p.y)])
        .collect();
    let mut along = Vec::with_capacity(count);
    let mut travelled = 0.0f64;
    let mut previous = old.first().copied().unwrap_or_default();
    for &point in &old {
        travelled += (point[0] - previous[0]).hypot(point[1] - previous[1]);
        along.push(travelled);
        previous = point;
    }
    let weight = |d: f64| (-(d * d) / (2.0 * sigma * sigma)).exp();
    for (i, point) in stroke.points.iter_mut().enumerate().take(count - 1).skip(1) {
        let here = along[i];
        let reach = (3.0 * sigma).min(here).min(travelled - here);
        // `along` only grows: the neighbours within reach are found without walking through them.
        let first = along[..i].partition_point(|&there| here - there > reach);
        let last = i + 1 + along[i + 1..].partition_point(|&there| there - here <= reach);
        let (mut sum_x, mut sum_y, mut sum_w) = (old[i][0], old[i][1], 1.0);
        for (j, share) in spread(first..i).chain(spread(i + 1..last)) {
            let w = weight((along[j] - here).abs()) * share;
            sum_x += old[j][0] * w;
            sum_y += old[j][1] * w;
            sum_w += w;
        }
        point.x = (sum_x / sum_w) as f32;
        point.y = (sum_y / sum_w) as f32;
    }
}
