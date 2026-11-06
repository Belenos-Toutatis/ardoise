//! `ink_lines`: finds the written lines from the ink. The handwriting recogniser merges lines, so
//! the assistant cannot lean on its regions to say which strokes make a line.
//!
//! First the scales of the page are read: the height of its strokes, the direction its lines go,
//! the distance from one line to the next. Then the strokes are followed from left to right along
//! each line (which stands tilted lines), each stroke goes to the line nearest to it, and lines
//! that are one are joined. Last come what cannot lead a line: marks (dots, accents, commas), and
//! letters much taller than the others. A stroke's height on the page is its median sample: the
//! mass of a letter sits in the body, whatever its ascender or descender.
//!
//! The constants below come from the four notes of the trial (`tests/real_notes.rs`) and synthetic
//! pages: other handwriting may ask for others (supposé).

use ardoise_ink::{Stroke, Tool};
use serde::Serialize;

use crate::geom::{
    Area, FIT_MIN_DX_MM, Span, area_of, cut_words, default_word_gap, fit_line, median, round2,
    samples_of, theil_sen,
};

/// A stroke taller than this many usual heights is a drawing, left out...
const TALL: f32 = 3.0;
/// ...except a letter on a line (an l, a p, the usual height being that of the letters' bodies):
/// narrower than this many usual heights...
const TALL_LETTER_WIDTH: f32 = 4.0;
/// ...not taller than this many...
const TALL_LETTER: f32 = 5.0;
/// ...and not taller than this many line pitches.
const TALL_LETTER_PITCH: f32 = 1.2;
/// A stroke wider than this many usual heights and lower than [`RULE_HEIGHT`] of one is a rule or
/// an underline, left out.
const RULE_WIDTH: f32 = 12.0;
const RULE_HEIGHT: f32 = 0.5;
/// Strokes lower than this don't count in the usual height (dots, accents), millimetres.
const MIN_LETTER_MM: f32 = 1.0;
/// A mark (dot, accent, comma) is lower than this many usual heights...
const MARK_HEIGHT: f32 = 0.45;
/// ...and narrower than this many.
const MARK_WIDTH: f32 = 1.5;
/// A stroke continues a line within this share of the line pitch of where the line is expected...
const FOLLOW_PITCH: f32 = 0.45;
/// ...at least this many usual heights...
const FOLLOW_MIN: f32 = 0.7;
/// ...and at most this many (a pitch read wrongly on a page of drawings must not join everything).
const FOLLOW_MAX: f32 = 2.5;
/// Without a pitch to read (a single line), a stroke continues within this many usual heights.
const FOLLOW_ALONE: f32 = 1.5;
/// Slopes tried for the direction of the page, degrees either way, and the step.
const PAGE_SLOPE_MAX_DEG: f32 = 8.0;
const PAGE_SLOPE_STEP_DEG: f32 = 0.25;
/// A slope must beat a more level one by this much: on few strokes, any slope lines up some.
const PAGE_SLOPE_MARGIN: f32 = 1.05;
/// Bins of the profile of the levels across the page, millimetres.
const PROFILE_BIN_MM: f32 = 0.5;
/// A page is not longer than this in bins of [`PROFILE_BIN_MM`] (a safeguard: 5 m).
const MAX_PROFILE_BINS: usize = 10_000;
/// The largest pitch looked for, millimetres.
const MAX_PITCH_MM: f32 = 40.0;
/// A peak of the profile's autocorrelation must stand this much above the valley before it...
const PITCH_PROMINENCE: f32 = 1.2;
/// ...and the pitch is the first peak of at least this share of the highest (the others are its
/// multiples).
const PITCH_PEAK_SHARE: f32 = 0.7;
/// Blank beyond which a line does not continue: this many usual heights, or
/// [`MIN_LINE_BREAK_MM`] (two blocks side by side are not one line).
const LINE_BREAK: f32 = 8.0;
const MIN_LINE_BREAK_MM: f32 = 30.0;
/// While following a line, its direction comes from its last strokes over this many millimetres,
/// at most [`RECENT_STROKES`] of them (a scribble must not make it slow)...
const RECENT_MM: f32 = 60.0;
const RECENT_STROKES: usize = 40;
/// ...once they span this much; before that, from the direction of the page.
const MIN_SPAN_MM: f32 = 30.0;
/// How many times strokes go to their nearest line.
const REASSIGN_ROUNDS: usize = 3;
/// Two lines that are one are this close, as a share of the distance a stroke can be from its line.
const JOIN: f32 = 0.5;
/// A few strokes on their own (a comma set low, an exponent, a piece of a letter) join a line around
/// them when there are at most this many...
const SMALL_LINE_STROKES: usize = 3;
/// ...over at most this many usual heights across...
const SMALL_LINE_WIDTH: f32 = 4.0;
/// ...and this share of the line pitch from it at most (or of a mark's reach, without a pitch).
const SMALL_LINE_PITCH: f32 = 0.6;
const SMALL_LINE_REACH: f32 = 0.9;
/// A mark can sit this many usual heights above the middle of its line (an accent, the dot of an
/// i)...
const MARK_ABOVE: f32 = 1.5;
/// ...this many below (a comma)...
const MARK_BELOW: f32 = 1.0;
/// ...and beyond the ends of the line by this many (a dash that opens the line, a full stop).
const MARK_BESIDE: f32 = 4.0;
/// A note that would make more lines than this is no written page (333 pages of 30 lines merged
/// into one): the work grows with the lines times the strokes, so its lines are not looked for.
const MAX_LINES: usize = 10_000;

/// A word of an [`InkLine`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InkWord {
    /// The word's strokes, by number.
    pub strokes: Vec<usize>,
    /// Left end, millimetres.
    pub x0: f32,
    /// Right end, millimetres.
    pub x1: f32,
}

/// A written line. Give its `strokes` to `tidy_line`, which then finds the same slope and words.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InkLine {
    /// The line's strokes, by number.
    pub strokes: Vec<usize>,
    /// Left end, millimetres.
    pub x0: f32,
    /// Right end, millimetres.
    pub x1: f32,
    /// y of the baseline at the left end, millimetres.
    pub baseline_left: f32,
    /// y of the baseline at the right end, millimetres.
    pub baseline_right: f32,
    /// Slope of the line in degrees, positive going down to the right. Level on a short line, where a fit goes wrong, and when the fit is steeper than 45 degrees.
    pub slope: f32,
    /// Height of the body of the letters, millimetres.
    pub body: f32,
    /// The words, left to right.
    pub words: Vec<InkWord>,
}

/// The written lines of a note, top to bottom, and the strokes that are in none.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct InkLines {
    pub lines: Vec<InkLine>,
    /// Strokes left out, by number: drawings (over 3 × the usual height, unless a letter on a line), rules, highlights, marks too far from any line. Every stroke, on a note that would make more than 10,000 lines: no written page does, and its lines are not looked for.
    pub apart: Vec<usize>,
}

#[derive(Debug, Clone, Copy)]
struct Item {
    number: usize,
    area: Area,
    /// Median height of its samples.
    level: f32,
}

impl Item {
    fn cx(&self) -> f32 {
        self.area.centre()[0]
    }
}

/// What near means on this page.
#[derive(Debug, Clone, Copy)]
struct Scales {
    /// The usual height of a stroke.
    usual: f32,
    /// The direction of the page, dy/dx: the slope of a line too short to have its own.
    prior: f32,
    /// From one line to the next, when it can be read.
    pitch: Option<f32>,
    /// How far from the middle of its line a stroke can be.
    reach: f32,
    /// Blank beyond which a line does not continue.
    line_break: f32,
}

/// A straight direction through the middle of a line.
#[derive(Debug, Clone, Copy)]
struct Guide {
    slope: f32,
    /// y at x = 0.
    level: f32,
    x0: f32,
    x1: f32,
}

impl Guide {
    fn at(&self, x: f32) -> f32 {
        self.level + self.slope * x
    }

    fn through(anchors: &[[f32; 2]], prior: f32) -> Option<Self> {
        let x0 = anchors.iter().map(|a| a[0]).fold(f32::INFINITY, f32::min);
        let x1 = anchors
            .iter()
            .map(|a| a[0])
            .fold(f32::NEG_INFINITY, f32::max);
        let slope = if x1 - x0 >= MIN_SPAN_MM {
            theil_sen(anchors, FIT_MIN_DX_MM).unwrap_or(prior)
        } else {
            prior
        };
        let mut rest: Vec<f32> = anchors.iter().map(|a| a[1] - slope * a[0]).collect();
        Some(Self {
            slope,
            level: median(&mut rest)?,
            x0,
            x1,
        })
    }
}

/// The height strokes usually have: writing, not its dots and accents.
fn usual_height(items: &[Item]) -> f32 {
    let mut heights: Vec<f32> = items
        .iter()
        .map(|item| item.area.height())
        .filter(|&height| height >= MIN_LETTER_MM)
        .collect();
    if heights.is_empty() {
        heights = items.iter().map(|item| item.area.height()).collect();
    }
    median(&mut heights).unwrap_or(0.0).max(MIN_LETTER_MM)
}

/// The levels of the strokes across the page, once this slope is taken away, in small bins. Each
/// stroke also counts in the bins next to its own: a line's strokes are never quite level.
fn profile(items: &[Item], slope: f32) -> Vec<f32> {
    let bin = |item: &Item| ((item.level - slope * item.cx()) / PROFILE_BIN_MM).floor() as i64;
    let (Some(low), Some(high)) = (items.iter().map(bin).min(), items.iter().map(bin).max()) else {
        return Vec::new();
    };
    let len = usize::try_from(high - low + 5)
        .unwrap_or(0)
        .min(MAX_PROFILE_BINS);
    let mut out = vec![0.0f32; len];
    for item in items {
        let at = usize::try_from(bin(item) - low + 2).unwrap_or(0);
        for (near, weight) in [(at.wrapping_sub(1), 0.5), (at, 1.0), (at + 1, 0.5)] {
            if let Some(slot) = out.get_mut(near) {
                *slot += weight;
            }
        }
    }
    out
}

/// The direction of the page: the slope that lines the strokes up best, the most level one among
/// those alike. Its lines are never quite parallel, but near enough to tell them apart.
fn page_slope(items: &[Item]) -> f32 {
    let sharpness = |slope: f32| -> f32 { profile(items, slope).iter().map(|v| v * v).sum() };
    let mut best = (0.0, sharpness(0.0));
    let steps = (PAGE_SLOPE_MAX_DEG / PAGE_SLOPE_STEP_DEG) as i32;
    for step in 1..=steps {
        for sign in [1.0, -1.0] {
            let slope = (sign * step as f32 * PAGE_SLOPE_STEP_DEG)
                .to_radians()
                .tan();
            let score = sharpness(slope);
            if score > best.1 * PAGE_SLOPE_MARGIN {
                best = (slope, score);
            }
        }
    }
    best.0
}

/// The distance from one written line to the next, read on the page: the first strong peak of the
/// autocorrelation of the levels. `None` on a single line.
fn line_pitch(items: &[Item], slope: f32) -> Option<f32> {
    let levels = profile(items, slope);
    let lags = ((MAX_PITCH_MM / PROFILE_BIN_MM) as usize).min(levels.len());
    let correlation: Vec<f32> = (0..lags)
        .map(|lag| {
            levels
                .iter()
                .zip(levels.iter().skip(lag))
                .map(|(a, b)| a * b)
                .sum()
        })
        .collect();
    // Past the bump of the line itself: the lowest point before the correlation rises again.
    let mut valley = 1;
    while let (Some(here), Some(next)) = (correlation.get(valley), correlation.get(valley + 1)) {
        if next > here {
            break;
        }
        valley += 1;
    }
    let floor = *correlation.get(valley)?;
    let peaks: Vec<(usize, f32)> = correlation
        .windows(3)
        .enumerate()
        .map(|(before, w)| (before + 1, w))
        .filter(|(lag, w)| *lag > valley && w[1] >= w[0] && w[1] >= w[2])
        .map(|(lag, w)| (lag, w[1]))
        .filter(|&(_, value)| value > floor * PITCH_PROMINENCE)
        .collect();
    let highest = peaks.iter().map(|p| p.1).fold(0.0, f32::max);
    peaks
        .iter()
        .find(|p| p.1 >= highest * PITCH_PEAK_SHARE)
        .map(|p| p.0 as f32 * PROFILE_BIN_MM)
}

/// A line being followed, left to right.
struct Track {
    members: Vec<usize>,
    anchors: Vec<[f32; 2]>,
    reach: f32,
    guide: Guide,
}

/// Follows the strokes, sorted by their left end, into lines: each continues the nearest line
/// expected near it, or starts one. `None` beyond [`MAX_LINES`].
fn follow(items: &[Item], scales: &Scales) -> Option<Vec<Vec<usize>>> {
    let mut tracks: Vec<Track> = Vec::new();
    for (index, item) in items.iter().enumerate() {
        let anchor = [item.cx(), item.level];
        let nearest = tracks
            .iter_mut()
            .filter(|track| item.area.x0 - track.reach <= scales.line_break)
            .map(|track| ((item.level - track.guide.at(anchor[0])).abs(), track))
            .filter(|(distance, _)| *distance <= scales.reach)
            .min_by(|a, b| a.0.total_cmp(&b.0));
        if let Some((_, track)) = nearest {
            track.members.push(index);
            track.anchors.push(anchor);
            track.reach = track.reach.max(item.area.x1);
            let recent: Vec<[f32; 2]> = track
                .anchors
                .iter()
                .rev()
                .take(RECENT_STROKES)
                .copied()
                .filter(|a| a[0] >= anchor[0] - RECENT_MM)
                .collect();
            if let Some(guide) = Guide::through(&recent, scales.prior) {
                track.guide = guide;
            }
        } else if let Some(guide) = Guide::through(&[anchor], scales.prior) {
            if tracks.len() == MAX_LINES {
                return None;
            }
            tracks.push(Track {
                members: vec![index],
                anchors: vec![anchor],
                reach: item.area.x1,
                guide,
            });
        }
    }
    Some(tracks.into_iter().map(|track| track.members).collect())
}

fn guides_of(items: &[Item], lines: &[Vec<usize>], prior: f32) -> Vec<Option<Guide>> {
    lines
        .iter()
        .map(|members| {
            let anchors: Vec<[f32; 2]> = members
                .iter()
                .filter_map(|&index| items.get(index))
                .map(|item| [item.cx(), item.level])
                .collect();
            Guide::through(&anchors, prior)
        })
        .collect()
}

fn extent(items: &[Item], members: &[usize]) -> Option<Area> {
    members
        .iter()
        .filter_map(|&index| items.get(index))
        .map(|item| item.area)
        .reduce(Area::union)
}

/// Each stroke goes to the line nearest to it, among those near it across.
fn reassign(items: &[Item], lines: Vec<Vec<usize>>, scales: &Scales) -> Vec<Vec<usize>> {
    let guides = guides_of(items, &lines, scales.prior);
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); lines.len()];
    for (own, members) in lines.iter().enumerate() {
        for &index in members {
            let Some(item) = items.get(index) else {
                continue;
            };
            let (x, y) = (item.cx(), item.level);
            let mut best = (own, f32::INFINITY);
            if let Some(Some(guide)) = guides.get(own) {
                best.1 = (y - guide.at(x)).abs();
            }
            for (other, guide) in guides.iter().enumerate() {
                let Some(guide) = guide else {
                    continue;
                };
                let distance = (y - guide.at(x)).abs();
                if distance < best.1
                    && x >= guide.x0 - scales.line_break
                    && x <= guide.x1 + scales.line_break
                {
                    best = (other, distance);
                }
            }
            if let Some(line) = out.get_mut(best.0) {
                line.push(index);
            }
        }
    }
    out.retain(|members| !members.is_empty());
    out
}

/// Puts the strokes of line `from` into line `into`.
fn merge(lines: &mut Vec<Vec<usize>>, from: usize, into: usize) {
    if from == into || from >= lines.len() || into >= lines.len() {
        return;
    }
    let moved = lines.remove(from);
    let into = if into > from { into - 1 } else { into };
    if let Some(line) = lines.get_mut(into) {
        line.extend(moved);
    }
}

/// Joins the lines that are one: level with each other, near each other across (their strokes may
/// well alternate).
fn join(items: &[Item], lines: &mut Vec<Vec<usize>>, scales: &Scales) {
    loop {
        let guides = guides_of(items, lines, scales.prior);
        let boxes: Vec<Option<Area>> = lines.iter().map(|m| extent(items, m)).collect();
        let found = boxes.iter().enumerate().find_map(|(a, first)| {
            boxes
                .iter()
                .enumerate()
                .skip(a + 1)
                .find_map(|(b, second)| {
                    let first = (*first)?;
                    let second = (*second)?;
                    let guide_a = (*guides.get(a)?)?;
                    let guide_b = (*guides.get(b)?)?;
                    if second.x0 - first.x1 > scales.line_break
                        || first.x0 - second.x1 > scales.line_break
                    {
                        return None;
                    }
                    // Where they are nearest to each other across.
                    let middle = (first.x0.max(second.x0) + first.x1.min(second.x1)) / 2.0;
                    ((guide_a.at(middle) - guide_b.at(middle)).abs() <= JOIN * scales.reach)
                        .then_some((b, a))
                })
        });
        let Some((from, into)) = found else {
            return;
        };
        merge(lines, from, into);
    }
}

/// How far a mark at `y` is from the line at `level`, in its reach: 1 at the limit, above or below.
fn mark_distance(y: f32, level: f32, usual: f32) -> f32 {
    let offset = (y - level) / usual;
    if offset < 0.0 {
        -offset / MARK_ABOVE
    } else {
        offset / MARK_BELOW
    }
}

/// A few strokes on their own, inside the span of a longer line and near it, are part of it.
fn absorb_small(items: &[Item], lines: &mut Vec<Vec<usize>>, scales: &Scales) {
    let usual = scales.usual;
    loop {
        let guides = guides_of(items, lines, scales.prior);
        let boxes: Vec<Option<Area>> = lines.iter().map(|m| extent(items, m)).collect();
        let mut found = None;
        for (small, members) in lines.iter().enumerate() {
            let Some(Some(area)) = boxes.get(small) else {
                continue;
            };
            if members.len() > SMALL_LINE_STROKES || area.width() > SMALL_LINE_WIDTH * usual {
                continue;
            }
            let mut levels: Vec<f32> = members
                .iter()
                .filter_map(|&index| items.get(index))
                .map(|item| item.level)
                .collect();
            let y = median(&mut levels).unwrap_or(area.centre()[1]);
            let x = area.centre()[0];
            let host = guides
                .iter()
                .zip(&boxes)
                .enumerate()
                .filter_map(|(other, (guide, around))| {
                    let (guide, around) = ((*guide)?, (*around)?);
                    let longer = lines.get(other).is_some_and(|m| m.len() > members.len());
                    let inside = around.x0 - usual <= area.x0 && area.x1 <= around.x1 + usual;
                    let level = guide.at(x);
                    let near = match scales.pitch {
                        Some(pitch) => (y - level).abs() <= SMALL_LINE_PITCH * pitch,
                        None => mark_distance(y, level, usual) <= SMALL_LINE_REACH,
                    };
                    (other != small && longer && inside && near)
                        .then_some((other, (y - level).abs()))
                })
                .min_by(|a, b| a.1.total_cmp(&b.1));
            if let Some((other, _)) = host {
                found = Some((small, other));
                break;
            }
        }
        let Some((small, host)) = found else {
            return;
        };
        merge(lines, small, host);
    }
}

/// The marks and the tall letters go to the line they belong to, as numbers. Those too far from any
/// line are left out.
fn attach(
    marks: &[Item],
    tall: &[Item],
    items: &[Item],
    lines: &[Vec<usize>],
    scales: &Scales,
) -> (Vec<Vec<usize>>, Vec<usize>) {
    let usual = scales.usual;
    let guides = guides_of(items, lines, scales.prior);
    let boxes: Vec<Option<Area>> = lines.iter().map(|m| extent(items, m)).collect();
    let around = |x: f32, beside: f32| {
        guides
            .iter()
            .zip(&boxes)
            .enumerate()
            .filter_map(move |(line, (guide, area))| {
                let (guide, area) = ((*guide)?, (*area)?);
                (x >= area.x0 - beside && x <= area.x1 + beside).then_some((line, guide))
            })
    };
    let mut attached: Vec<Vec<usize>> = vec![Vec::new(); lines.len()];
    let mut apart = Vec::new();
    for mark in marks {
        let [x, y] = mark.area.centre();
        let nearest = around(x, MARK_BESIDE * usual)
            .map(|(line, guide)| (line, mark_distance(y, guide.at(x), usual)))
            .filter(|(_, distance)| *distance <= 1.0)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match nearest.and_then(|(line, _)| attached.get_mut(line)) {
            Some(line) => line.push(mark.number),
            None => apart.push(mark.number),
        }
    }
    for letter in tall {
        let x = letter.cx();
        let height = letter.area.height();
        let fits = letter.area.width() <= TALL_LETTER_WIDTH * usual
            && height <= TALL_LETTER * usual
            && scales
                .pitch
                .is_none_or(|pitch| height <= TALL_LETTER_PITCH * pitch);
        let nearest = around(x, MARK_BESIDE * usual)
            .map(|(line, guide)| (line, (letter.level - guide.at(x)).abs()))
            .filter(|(_, distance)| fits && *distance <= scales.reach)
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match nearest.and_then(|(line, _)| attached.get_mut(line)) {
            Some(line) => line.push(letter.number),
            None => apart.push(letter.number),
        }
    }
    (attached, apart)
}

/// Finds the written lines among `visible`, the numbers of the strokes on visible layers (a number
/// that is not a stroke is ignored).
pub fn ink_lines(strokes: &[Stroke], visible: &[usize]) -> InkLines {
    let mut numbers = visible.to_vec();
    numbers.sort_unstable();
    numbers.dedup();
    let mut apart = Vec::new();
    let mut items = Vec::with_capacity(numbers.len());
    for number in numbers {
        let Some(stroke) = strokes.get(number) else {
            continue;
        };
        let Some(area) = area_of(&stroke.points) else {
            continue;
        };
        if stroke.tool == Tool::Highlighter {
            apart.push(number);
            continue;
        }
        let mut heights: Vec<f32> = samples_of(&stroke.points).map(|p| p[1]).collect();
        let level = median(&mut heights).unwrap_or(area.centre()[1]);
        items.push(Item {
            number,
            area,
            level,
        });
    }
    let usual = usual_height(&items);

    let mut mains = Vec::with_capacity(items.len());
    let mut marks = Vec::new();
    let mut tall = Vec::new();
    for item in items {
        let (width, height) = (item.area.width(), item.area.height());
        if height > TALL * usual {
            tall.push(item);
        } else if width > RULE_WIDTH * usual && height < RULE_HEIGHT * usual {
            apart.push(item.number);
        } else if height < MARK_HEIGHT * usual && width < MARK_WIDTH * usual {
            marks.push(item);
        } else {
            mains.push(item);
        }
    }
    mains.sort_by(|a, b| {
        a.area
            .x0
            .total_cmp(&b.area.x0)
            .then(a.number.cmp(&b.number))
    });

    let prior = page_slope(&mains);
    let pitch = line_pitch(&mains, prior);
    let scales = Scales {
        usual,
        prior,
        pitch,
        reach: pitch.map_or(FOLLOW_ALONE * usual, |pitch| {
            (FOLLOW_PITCH * pitch).clamp(FOLLOW_MIN * usual, FOLLOW_MAX * usual)
        }),
        line_break: (LINE_BREAK * usual).max(MIN_LINE_BREAK_MM),
    };
    let Some(mut lines) = follow(&mains, &scales) else {
        let everything = mains.iter().chain(&marks).chain(&tall);
        apart.extend(everything.map(|item| item.number));
        apart.sort_unstable();
        return InkLines {
            lines: Vec::new(),
            apart,
        };
    };
    for _ in 0..REASSIGN_ROUNDS {
        lines = reassign(&mains, lines, &scales);
    }
    join(&mains, &mut lines, &scales);
    absorb_small(&mains, &mut lines, &scales);
    let (attached, left_out) = attach(&marks, &tall, &mains, &lines, &scales);
    apart.extend(left_out);
    apart.sort_unstable();

    let mut found: Vec<InkLine> = lines
        .iter()
        .zip(attached)
        .filter_map(|(members, joined)| {
            let mut numbers: Vec<usize> = members
                .iter()
                .filter_map(|&index| mains.get(index))
                .map(|item| item.number)
                .chain(joined)
                .collect();
            numbers.sort_unstable();
            ink_line(strokes, numbers)
        })
        .collect();
    found.sort_by(|a, b| {
        (a.baseline_left + a.baseline_right)
            .total_cmp(&(b.baseline_left + b.baseline_right))
            .then(a.x0.total_cmp(&b.x0))
    });
    InkLines {
        lines: found,
        apart,
    }
}

/// What the assistant is told about a line: the very fit and words `tidy_line` will find.
fn ink_line(strokes: &[Stroke], numbers: Vec<usize>) -> Option<InkLine> {
    let fit = fit_line(
        numbers.iter().filter_map(|&number| strokes.get(number)),
        None,
    )?;
    let spans: Vec<Span> = numbers
        .iter()
        .filter_map(|&number| {
            let area = area_of(&strokes.get(number)?.points)?;
            Some(Span {
                number,
                x0: area.x0,
                x1: area.x1,
            })
        })
        .collect();
    let (words, _) = cut_words(spans, default_word_gap(fit.body));
    let words = words
        .into_iter()
        .map(|mut word| {
            word.strokes.sort_unstable();
            InkWord {
                strokes: word.strokes,
                x0: round2(word.x0),
                x1: round2(word.x1),
            }
        })
        .collect();
    Some(InkLine {
        strokes: numbers,
        x0: round2(fit.x0),
        x1: round2(fit.x1),
        baseline_left: round2(fit.at(fit.x0)),
        baseline_right: round2(fit.at(fit.x1)),
        slope: round2(fit.slope_degrees()),
        body: round2(fit.body),
        words,
    })
}
