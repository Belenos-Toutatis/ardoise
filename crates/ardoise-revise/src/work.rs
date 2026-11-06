//! Runs the orders: the strokes under their numbers, what selections read, the drawing order.

use std::collections::BTreeSet;

use ardoise_ink::{Point, Rgba, Stroke, Tool};

use crate::geom::{Area, UNITS, area_of, median, smooth_stroke};
use crate::orders::{Flip, Order, Part, Select, Shape};
use crate::report::{Ctx, OrderReport, Report, ReviseError, SlopeFrom};
use crate::shapes::{P, Style, length, paths, sample_count, stroke_along};
use crate::tidy::TidySpec;
use crate::validate::{Color, too_many_points};
use crate::{Input, MAX_COORD_MM, MAX_POINTS, MAX_SMOOTHED_POINTS, Revised};

/// A replaced stroke's pressure is at least this: a line drawn lightly stays visible once it is a
/// clean line (the trial's; supposé).
const MIN_REPLACE_PRESSURE: f32 = 0.25;
/// Pressure of new shapes (the trial's; supposé).
const ADD_PRESSURE: f32 = 0.6;
/// A new shape is written at this speed, millimetres per millisecond (supposé: about a pen's)...
const SHAPE_MM_PER_MS: f32 = 0.15;
/// ...and takes at least this long, milliseconds.
const SHAPE_MIN_MS: u32 = 60;
/// A stroke further than this beyond the page counts as off the page, millimetres: a pen can
/// touch the very edge.
const OFF_PAGE_SLACK_MM: f32 = 0.5;
/// How many stroke numbers a warning writes out: on a large note it stays a sentence.
const LISTED_NUMBERS: usize = 20;

/// The numbers, or the first of them and how many others.
fn listed(numbers: &[usize]) -> String {
    match numbers.len().checked_sub(LISTED_NUMBERS) {
        Some(more) if more > 0 => format!("{:?} and {more} more", &numbers[..LISTED_NUMBERS]),
        _ => format!("{numbers:?}"),
    }
}

/// What a selection reads: a stroke as given, or as made.
#[derive(Debug, Clone, Copy)]
struct Facts {
    area: Option<Area>,
    color: Rgba,
    layer: u32,
}

impl Facts {
    fn of(stroke: &Stroke) -> Self {
        Self {
            area: area_of(&stroke.points),
            color: stroke.color,
            layer: stroke.layer,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Slot {
    given: Facts,
    pub(crate) stroke: Option<Stroke>,
    /// The order that took it away.
    removed_by: Option<usize>,
    /// The last order that gave it a place in time.
    pub(crate) placed_by: Option<usize>,
}

impl Slot {
    fn new(stroke: Stroke) -> Self {
        Self {
            given: Facts::of(&stroke),
            stroke: Some(stroke),
            removed_by: None,
            placed_by: None,
        }
    }
}

pub(crate) struct Work<'a> {
    pub(crate) input: &'a Input<'a>,
    /// Indexed by stroke number.
    pub(crate) slots: Vec<Slot>,
    /// The numbers in the order of the note's list (removed ones stay here, and are skipped at the
    /// end): the drawing order within a layer.
    list: Vec<usize>,
    /// How many of `list` are `behind` strokes, at its start.
    behind: usize,
    /// Points made so far (the note's own do not count).
    made_points: usize,
    /// Samples smoothed so far, each time it is smoothed.
    smoothed_points: usize,
    pub(crate) pushed: BTreeSet<usize>,
    pub(crate) pushed_ms: u64,
    pub(crate) faster: BTreeSet<usize>,
}

fn color_of(spec: Color, old: Rgba) -> Rgba {
    let [r, g, b] = spec.rgb;
    Rgba {
        r,
        g,
        b,
        a: spec.alpha.unwrap_or(old.a),
    }
}

fn parse(ctx: Ctx, text: &str) -> Result<Color, ReviseError> {
    Color::parse(text).ok_or_else(|| ctx.err(format!("the colour \"{text}\" cannot be read")))
}

impl<'a> Work<'a> {
    pub(crate) fn new(input: &'a Input<'a>) -> Self {
        Self {
            input,
            slots: input.strokes.iter().cloned().map(Slot::new).collect(),
            list: (0..input.strokes.len()).collect(),
            behind: 0,
            made_points: 0,
            smoothed_points: 0,
            pushed: BTreeSet::new(),
            pushed_ms: 0,
            faster: BTreeSet::new(),
        }
    }

    pub(crate) fn run(&mut self, ctx: Ctx, order: &Order) -> Result<OrderReport, ReviseError> {
        let mut report = OrderReport::new(ctx.op);
        match order {
            Order::Recolor { select, color } => {
                let color = parse(ctx, color)?;
                let ids = self.select(ctx, select, &mut report)?;
                self.each_stroke(&ids, |stroke| stroke.color = color_of(color, stroke.color));
            }
            Order::Restyle {
                select,
                width,
                tool,
            } => {
                let ids = self.select(ctx, select, &mut report)?;
                let tool = tool.map(Tool::from);
                self.each_stroke(&ids, |stroke| {
                    if let Some(width) = width {
                        stroke.width = width * UNITS;
                    }
                    if let Some(tool) = tool {
                        stroke.tool = tool;
                    }
                });
            }
            Order::Delete { select } => {
                let ids = self.select(ctx, select, &mut report)?;
                self.remove(ctx, &ids);
            }
            Order::Move { select, dx, dy } => {
                let ids = self.select(ctx, select, &mut report)?;
                let (dx, dy) = (dx * UNITS, dy * UNITS);
                self.each_point(&ids, |p| {
                    p.x += dx;
                    p.y += dy;
                });
                self.check_reach(ctx, &ids)?;
            }
            Order::Scale {
                select,
                factor,
                sx,
                sy,
                origin,
            } => {
                let ids = self.select(ctx, select, &mut report)?;
                let sx = factor.or(*sx).unwrap_or(1.0);
                let sy = factor.or(*sy).unwrap_or(1.0);
                let (ox, oy) = (origin[0] * UNITS, origin[1] * UNITS);
                self.each_point(&ids, |p| {
                    p.x = ox + (p.x - ox) * sx;
                    p.y = oy + (p.y - oy) * sy;
                });
                self.check_reach(ctx, &ids)?;
            }
            Order::Rotate {
                select,
                degrees,
                origin,
            } => {
                let ids = self.select(ctx, select, &mut report)?;
                let (sin, cos) = f64::from(*degrees).to_radians().sin_cos();
                let (ox, oy) = (f64::from(origin[0] * UNITS), f64::from(origin[1] * UNITS));
                self.each_point(&ids, |p| {
                    let (x, y) = (f64::from(p.x) - ox, f64::from(p.y) - oy);
                    p.x = (ox + x * cos - y * sin) as f32;
                    p.y = (oy + x * sin + y * cos) as f32;
                });
                self.check_reach(ctx, &ids)?;
            }
            Order::Smooth { select, sigma } => {
                let ids = self.select(ctx, select, &mut report)?;
                self.charge_smoothing(ctx, &ids)?;
                self.each_stroke(&ids, |stroke| smooth_stroke(stroke, *sigma));
            }
            Order::Trim { stroke, from, to } => {
                report.selected = 1;
                self.trim(ctx, *stroke, *from, *to)?;
                self.warn_hidden(&[*stroke], &mut report);
            }
            Order::Replace {
                select,
                with,
                color,
                width,
            } => {
                let ids = self.select(ctx, select, &mut report)?;
                if ids.is_empty() {
                    return Err(ctx.err("nothing selected: there is nothing to replace"));
                }
                let color = color.as_deref().map(|text| parse(ctx, text)).transpose()?;
                report.new_strokes = self.replace(ctx, &ids, with, color, *width)?;
            }
            Order::Add {
                shapes,
                color,
                width,
                tool,
                layer,
                behind,
                after,
            } => {
                let color = parse(ctx, color)?;
                if let Some(after) = after {
                    self.alive(ctx, *after)?;
                }
                let style = self.add_style(color, *width, tool.map(Tool::from), *layer);
                if !self.input.shows(style.layer) {
                    report.warnings.push(format!(
                        "the new strokes are on layer {}, which is hidden: the picture does not show them",
                        style.layer
                    ));
                }
                let numbers = self.add(ctx, shapes, &style, *behind)?;
                report.time = self.place(ctx, &numbers, *after)?;
                report.new_strokes = numbers;
            }
            Order::Copy {
                select,
                to_x,
                dx,
                dy,
                factor,
                color,
                after,
            } => {
                let color = color.as_deref().map(|text| parse(ctx, text)).transpose()?;
                if let Some(after) = after {
                    self.alive(ctx, *after)?;
                }
                let ids = self.select(ctx, select, &mut report)?;
                let placement = Placement {
                    to_x: *to_x,
                    dx: dx.unwrap_or(0.0),
                    dy: dy.unwrap_or(0.0),
                    factor: factor.unwrap_or(1.0),
                };
                let numbers = self.copy(ctx, &ids, &placement, color)?;
                // Written as they were: by the time they were written, not by their number.
                let mut in_time = numbers.clone();
                in_time.sort_by_key(|&n| self.stroke(n).and_then(start_of));
                report.time = self.place(ctx, &in_time, *after)?;
                report.new_strokes = numbers;
            }
            Order::Glyph {
                parts,
                color,
                after,
            } => {
                let color = color.as_deref().map(|text| parse(ctx, text)).transpose()?;
                if let Some(after) = after {
                    self.alive(ctx, *after)?;
                }
                report.selected = parts.len();
                let numbers = self.glyph(ctx, parts, color)?;
                self.warn_hidden(&numbers, &mut report);
                report.time = self.place(ctx, &numbers, *after)?;
                report.new_strokes = numbers;
            }
            Order::Retime { select, after } => {
                self.alive(ctx, *after)?;
                let ids = self.select(ctx, select, &mut report)?;
                if ids.is_empty() {
                    return Err(ctx.err("nothing selected: there is nothing to place in time"));
                }
                if ids.contains(after) {
                    return Err(ctx.err(format!(
                        "stroke {after} is in the selection: it cannot come after itself"
                    )));
                }
                let mut in_time = ids.clone();
                in_time.sort_by_key(|&n| self.stroke(n).and_then(start_of));
                report.time = self.place(ctx, &in_time, Some(*after))?;
            }
            Order::TidyLine {
                select,
                baseline,
                left,
                mode,
                scale,
                slope,
                gaps,
                word_gap,
                smooth,
            } => {
                let ids = self.select(ctx, select, &mut report)?;
                let spec = TidySpec {
                    baseline: *baseline,
                    left: *left,
                    mode: mode.unwrap_or_default(),
                    scale: scale.unwrap_or(1.0),
                    slope: *slope,
                    gaps: gaps.unwrap_or_default(),
                    word_gap: *word_gap,
                    smooth: *smooth,
                };
                report.line = self.tidy(ctx, &ids, &spec)?;
                if report
                    .line
                    .as_ref()
                    .is_some_and(|line| line.slope_from == SlopeFrom::TooSteep)
                {
                    report.warnings.push(
                        "the fit found a line steeper than 45 degrees, so it was taken as level: check the selection, or give the `slope` you see".to_owned(),
                    );
                }
            }
        }
        Ok(report)
    }

    /// A warning when strokes of these numbers are on hidden layers.
    fn warn_hidden(&self, numbers: &[usize], report: &mut OrderReport) {
        let hidden: Vec<usize> = numbers
            .iter()
            .copied()
            .filter(|&number| {
                self.stroke(number)
                    .is_some_and(|stroke| !self.input.shows(stroke.layer))
            })
            .collect();
        if !hidden.is_empty() {
            report.warnings.push(format!(
                "stroke(s) {} are on a hidden layer: the picture does not show them",
                listed(&hidden)
            ));
        }
    }

    /// The numbers a selection finds, sorted. An unknown number is an error; finding nothing is a
    /// warning here (the orders for which it is an error say so). What the note does not show is
    /// only taken when the selection asks for it: by `ids`, or by naming a `layer`.
    fn select(
        &self,
        ctx: Ctx,
        select: &Select,
        report: &mut OrderReport,
    ) -> Result<Vec<usize>, ReviseError> {
        let count = self.slots.len();
        for &number in select
            .ids
            .iter()
            .flatten()
            .chain(select.not.iter().flatten())
        {
            if number >= count {
                return Err(self.unknown(ctx, number));
            }
        }
        let drawn_only = select.layer.is_none();
        let mut hidden_left_out = 0usize;
        let mut chosen: Vec<usize> = select.ids.clone().unwrap_or_default();
        let mut candidate = |number: usize, slot: &Slot| {
            if drawn_only && !self.input.shows(slot.given.layer) {
                hidden_left_out += 1;
            } else {
                chosen.push(number);
            }
        };
        if let Some([x, y, w, h]) = select.rect {
            for (number, slot) in self.slots.iter().enumerate() {
                let Some(area) = slot.given.area else {
                    continue;
                };
                let [cx, cy] = area.centre();
                if cx >= x && cx <= x + w && cy >= y && cy <= y + h {
                    candidate(number, slot);
                }
            }
        }
        if select.ids.is_none() && select.rect.is_none() {
            for (number, slot) in self.slots.iter().enumerate() {
                candidate(number, slot);
            }
        }
        chosen.sort_unstable();
        chosen.dedup();
        let mut left_out = select.not.clone().unwrap_or_default();
        left_out.sort_unstable();
        let color = select
            .color
            .as_deref()
            .map(|text| parse(ctx, text))
            .transpose()?;
        chosen.retain(|&number| {
            let given = self.slots[number].given;
            let size = given.area.map(|area| area.size());
            color.is_none_or(|c| {
                [given.color.r, given.color.g, given.color.b] == c.rgb
                    && c.alpha.is_none_or(|alpha| alpha == given.color.a)
            }) && select.layer.is_none_or(|layer| layer == given.layer)
                && select
                    .min_size
                    .is_none_or(|m| size.is_some_and(|size| size >= m))
                && select
                    .max_size
                    .is_none_or(|m| size.is_some_and(|size| size <= m))
                && left_out.binary_search(&number).is_err()
        });
        let named_gone: Vec<usize> = select
            .ids
            .iter()
            .flatten()
            .copied()
            .filter(|number| {
                chosen.binary_search(number).is_ok() && self.slots[*number].stroke.is_none()
            })
            .collect();
        if !named_gone.is_empty() {
            report.warnings.push(format!(
                "stroke(s) {named_gone:?} were already removed by an earlier order"
            ));
        }
        chosen.retain(|&number| {
            self.slots[number]
                .stroke
                .as_ref()
                .is_some_and(|stroke| !stroke.points.is_empty())
        });
        report.selected = chosen.len();
        if chosen.is_empty() {
            report.warnings.push(if hidden_left_out > 0 {
                format!(
                    "nothing selected: this order did nothing ({hidden_left_out} stroke(s) on hidden layers were left out: name their `layer` or their `ids` to touch them)"
                )
            } else {
                "nothing selected: this order did nothing".to_owned()
            });
        }
        self.warn_hidden(&chosen, report);
        Ok(chosen)
    }

    fn unknown(&self, ctx: Ctx, number: usize) -> ReviseError {
        ctx.err(format!(
            "no stroke {number}: so far the strokes are numbered 0 to {}",
            self.slots.len().saturating_sub(1)
        ))
    }

    /// A stroke an order names, which must be there and have points.
    fn alive(&self, ctx: Ctx, number: usize) -> Result<&Stroke, ReviseError> {
        let Some(slot) = self.slots.get(number) else {
            return Err(self.unknown(ctx, number));
        };
        let Some(stroke) = slot.stroke.as_ref() else {
            let by = slot
                .removed_by
                .map_or_else(String::new, |order| format!(" by order {order}"));
            return Err(ctx.err(format!("stroke {number} was removed{by}: name another one")));
        };
        if stroke.points.is_empty() {
            return Err(ctx.err(format!("stroke {number} has no points")));
        }
        Ok(stroke)
    }

    pub(crate) fn stroke(&self, number: usize) -> Option<&Stroke> {
        self.slots.get(number).and_then(|slot| slot.stroke.as_ref())
    }

    pub(crate) fn stroke_mut(&mut self, number: usize) -> Option<&mut Stroke> {
        self.slots
            .get_mut(number)
            .and_then(|slot| slot.stroke.as_mut())
    }

    fn each_stroke(&mut self, ids: &[usize], mut change: impl FnMut(&mut Stroke)) {
        for &number in ids {
            if let Some(stroke) = self.stroke_mut(number) {
                change(stroke);
            }
        }
    }

    fn each_point(&mut self, ids: &[usize], mut change: impl FnMut(&mut Point)) {
        self.each_stroke(ids, |stroke| {
            for point in &mut stroke.points {
                change(point);
            }
        });
    }

    /// Refuses what would put a stroke absurdly far (repeated scaling can): the file format has
    /// limits, and the arithmetic must stay finite. Samples that cannot be drawn are not looked at.
    pub(crate) fn check_reach(&self, ctx: Ctx, ids: &[usize]) -> Result<(), ReviseError> {
        let limit = MAX_COORD_MM * UNITS;
        for &number in ids {
            let Some(stroke) = self.stroke(number) else {
                continue;
            };
            let far = stroke.points.iter().any(|p| {
                (p.x.is_finite() && p.x.abs() > limit) || (p.y.is_finite() && p.y.abs() > limit)
            });
            if far {
                return Err(ctx.err(format!(
                    "stroke {number} would end up further than {MAX_COORD_MM} mm from the page"
                )));
            }
        }
        Ok(())
    }

    fn charge(&mut self, ctx: Ctx, points: usize) -> Result<(), ReviseError> {
        self.made_points = self.made_points.saturating_add(points);
        if self.made_points > MAX_POINTS {
            return Err(too_many_points(ctx));
        }
        Ok(())
    }

    /// Counts the samples of these strokes as smoothed once more, before the work is done.
    pub(crate) fn charge_smoothing(&mut self, ctx: Ctx, ids: &[usize]) -> Result<(), ReviseError> {
        let samples: usize = ids
            .iter()
            .filter_map(|&number| self.stroke(number))
            .map(|stroke| stroke.points.len())
            .sum();
        self.smoothed_points = self.smoothed_points.saturating_add(samples);
        if self.smoothed_points > MAX_SMOOTHED_POINTS {
            return Err(ctx.err(format!(
                "the orders would smooth more than {MAX_SMOOTHED_POINTS} samples in total: smooth each stroke once"
            )));
        }
        Ok(())
    }

    fn remove(&mut self, ctx: Ctx, ids: &[usize]) {
        for &number in ids {
            if let Some(slot) = self.slots.get_mut(number) {
                slot.stroke = None;
                slot.removed_by = Some(ctx.index);
            }
        }
    }

    /// Gives a new stroke the next free number.
    fn make(&mut self, stroke: Stroke) -> usize {
        let number = self.slots.len();
        self.slots.push(Slot::new(stroke));
        number
    }

    fn trim(
        &mut self,
        ctx: Ctx,
        number: usize,
        from: usize,
        to: Option<usize>,
    ) -> Result<(), ReviseError> {
        let count = self.alive(ctx, number)?.points.len();
        // Its place in time was made for the whole stroke: what follows moved for it (or it was
        // drawn faster, with a narration), and a piece would leave a hole. `glyph` times a piece.
        if let Some(order) = self.slots.get(number).and_then(|slot| slot.placed_by) {
            return Err(ctx.err(format!(
                "stroke {number} was given its place in time by order {order}, for its whole length: use `glyph` with `from` and `to` instead, which times the piece only (or `trim` a stroke before placing it)"
            )));
        }
        let to = to.unwrap_or(count - 1);
        if to >= count || from > to {
            return Err(ctx.err(format!(
                "points {from} to {to} are out of range: stroke {number} has {count}, numbered from 0"
            )));
        }
        if let Some(stroke) = self.stroke_mut(number) {
            stroke.points.truncate(to + 1);
            stroke.points.drain(..from);
        }
        Ok(())
    }

    /// The shapes take the place of `ids` (see [`Order::Replace`]): the first in the drawing order
    /// (among those the note shows, when it shows any) gives its place, layer, tool, colour and
    /// width; they share the time span of all.
    fn replace(
        &mut self,
        ctx: Ctx,
        ids: &[usize],
        with: &[Shape],
        color: Option<Color>,
        width: Option<f32>,
    ) -> Result<Vec<usize>, ReviseError> {
        // In the drawing order: by layer, then along the list (the first of equal layers wins).
        let first = self
            .list
            .iter()
            .copied()
            .filter(|number| ids.binary_search(number).is_ok())
            .min_by_key(|&number| {
                self.stroke(number)
                    .map(|stroke| (!self.input.shows(stroke.layer), stroke.layer))
            });
        let like = first.and_then(|number| self.stroke(number));
        let (Some(first), Some(like)) = (first, like) else {
            return Err(ctx.err("nothing selected: there is nothing to replace"));
        };
        let mut pressures: Vec<f32> = ids
            .iter()
            .filter_map(|&number| self.stroke(number))
            .flat_map(|stroke| stroke.points.iter().map(|p| p.pressure))
            .filter(|pressure| pressure.is_finite())
            .collect();
        let style = Style {
            tool: like.tool,
            color: color.map_or(like.color, |c| color_of(c, like.color)),
            width: width.map_or(like.width, |width| width * UNITS),
            layer: like.layer,
            pressure: median(&mut pressures)
                .unwrap_or(0.5)
                .clamp(MIN_REPLACE_PRESSURE, 1.0),
        };
        let spans: Vec<(u32, u32)> = ids
            .iter()
            .filter_map(|&number| self.stroke(number))
            .filter_map(span_of)
            .collect();
        let start = spans.iter().map(|span| span.0).min().unwrap_or(0);
        let end = spans.iter().map(|span| span.1).max().unwrap_or(start);

        let all: Vec<Vec<P>> = with.iter().flat_map(paths).collect();
        let total: f64 = all.iter().map(|path| f64::from(length(path))).sum();
        let span = f64::from(end.saturating_sub(start));
        let at = |travelled: f64| {
            let k = if total > 0.0 { travelled / total } else { 1.0 };
            start.saturating_add((span * k.min(1.0)).round() as u32)
        };
        let mut travelled = 0.0;
        let mut made = Vec::with_capacity(all.len());
        for path in &all {
            self.charge(ctx, sample_count(path))?;
            let from = at(travelled);
            travelled += f64::from(length(path));
            made.push(stroke_along(path, &style, from, at(travelled)));
        }
        let numbers: Vec<usize> = made.into_iter().map(|stroke| self.make(stroke)).collect();
        let place = self
            .list
            .iter()
            .position(|&number| number == first)
            .unwrap_or(self.list.len());
        self.list.splice(place..place, numbers.iter().copied());
        // A stroke behind the others keeps what replaces it there, under what comes later.
        if place < self.behind {
            self.behind += numbers.len();
        }
        self.remove(ctx, ids);
        Ok(numbers)
    }

    /// The stroke new shapes look like when the order does not say: the note's writing that it
    /// shows (any of its writing when it shows none), not a highlight. They go on its layer, or on
    /// the first layer the note shows.
    fn add_style(
        &self,
        color: Color,
        width: Option<f32>,
        tool: Option<Tool>,
        layer: Option<u32>,
    ) -> Style {
        let writing = |shown_only: bool| {
            self.slots.iter().find_map(|slot| {
                slot.stroke.as_ref().filter(|stroke| {
                    stroke.tool != Tool::Highlighter
                        && !stroke.points.is_empty()
                        && (!shown_only || self.input.shows(stroke.layer))
                })
            })
        };
        let shown = writing(true);
        let like = shown.or_else(|| writing(false));
        let default_layer = shown
            .map(|stroke| stroke.layer)
            .or_else(|| (0..self.input.layers).find(|&layer| self.input.shows(layer)))
            .unwrap_or(0);
        let like_tool = like.map_or(Tool::Ballpoint, |stroke| stroke.tool);
        let like_width = like.map_or(Tool::Ballpoint.default_width(), |stroke| stroke.width);
        let chosen_tool = tool.unwrap_or(like_tool);
        let usual_width = if chosen_tool == like_tool {
            like_width
        } else {
            chosen_tool.default_width()
        };
        Style {
            tool: chosen_tool,
            color: color_of(color, Rgba::BLACK),
            width: width.map_or(usual_width, |width| width * UNITS),
            layer: layer.unwrap_or(default_layer),
            pressure: ADD_PRESSURE,
        }
    }

    fn add(
        &mut self,
        ctx: Ctx,
        shapes: &[Shape],
        style: &Style,
        behind: bool,
    ) -> Result<Vec<usize>, ReviseError> {
        let mut numbers = Vec::new();
        for path in shapes.iter().flat_map(paths) {
            self.charge(ctx, sample_count(&path))?;
            let duration = ((length(&path) / SHAPE_MM_PER_MS).round() as u32).max(SHAPE_MIN_MS);
            numbers.push(self.make(stroke_along(&path, style, 0, duration)));
        }
        if behind {
            self.list
                .splice(self.behind..self.behind, numbers.iter().copied());
            self.behind += numbers.len();
        } else {
            self.list.extend(&numbers);
        }
        Ok(numbers)
    }

    fn copy(
        &mut self,
        ctx: Ctx,
        ids: &[usize],
        placement: &Placement,
        color: Option<Color>,
    ) -> Result<Vec<usize>, ReviseError> {
        let area = ids
            .iter()
            .filter_map(|&number| self.stroke(number))
            .filter_map(|stroke| area_of(&stroke.points))
            .reduce(Area::union);
        let (left, bottom) = area.map_or((0.0, 0.0), |area| (area.x0, area.y1));
        let shift = placement.to_x.map_or(placement.dx, |x| x - left);
        let (ox, oy) = (left * UNITS, bottom * UNITS);
        let (dx, dy) = (shift * UNITS, placement.dy * UNITS);
        let k = placement.factor;
        let mut numbers = Vec::with_capacity(ids.len());
        for &number in ids {
            let Some(source) = self.stroke(number) else {
                continue;
            };
            let mut stroke = source.clone();
            self.charge(ctx, stroke.points.len())?;
            for p in &mut stroke.points {
                p.x = ox + (p.x - ox) * k + dx;
                p.y = oy + (p.y - oy) * k + dy;
            }
            if let Some(color) = color {
                stroke.color = color_of(color, stroke.color);
            }
            numbers.push(self.make(stroke));
        }
        self.list.extend(&numbers);
        self.check_reach(ctx, &numbers)?;
        Ok(numbers)
    }

    fn glyph(
        &mut self,
        ctx: Ctx,
        parts: &[Part],
        color: Option<Color>,
    ) -> Result<Vec<usize>, ReviseError> {
        let mut numbers = Vec::with_capacity(parts.len());
        for part in parts {
            let source = self.alive(ctx, part.stroke)?;
            let count = source.points.len();
            let from = part.from.unwrap_or(0);
            let to = part.to.unwrap_or(count - 1);
            let points = if from <= to {
                source.points.get(from..=to)
            } else {
                None
            };
            let Some(points) = points else {
                return Err(ctx.err(format!(
                    "points {from} to {to} are out of range: stroke {} has {count}, numbered from 0",
                    part.stroke
                )));
            };
            let Some(area) = area_of(points) else {
                return Err(ctx.err(format!(
                    "points {from} to {to} of stroke {} cannot be drawn",
                    part.stroke
                )));
            };
            let mut stroke = Stroke {
                points: points.to_vec(),
                ..source.clone()
            };
            let (width, height) = (area.width(), area.height());
            let (sx, sy) = (part.sx.unwrap_or(1.0), part.sy.unwrap_or(1.0));
            for p in &mut stroke.points {
                let (mut x, mut y) = (p.x / UNITS - area.x0, p.y / UNITS - area.y1);
                if matches!(part.flip, Some(Flip::H | Flip::Rot)) {
                    x = width - x;
                }
                if matches!(part.flip, Some(Flip::V | Flip::Rot)) {
                    y = -height - y;
                }
                p.x = (part.x + x * sx) * UNITS;
                p.y = (part.y + y * sy) * UNITS;
            }
            if let Some(color) = color {
                stroke.color = color_of(color, stroke.color);
            }
            self.charge(ctx, stroke.points.len())?;
            numbers.push(self.make(stroke));
        }
        self.list.extend(&numbers);
        self.check_reach(ctx, &numbers)?;
        Ok(numbers)
    }

    /// The result: the list without what was removed or left without points, and the report.
    pub(crate) fn finish(self, orders: Vec<OrderReport>) -> Revised {
        let page = self.input.page_mm;
        let given_count = self.input.strokes.len();
        let page_known = page.iter().all(|side| side.is_finite() && *side > 0.0);
        let beyond = |area: &Area| {
            area.x0 < -OFF_PAGE_SLACK_MM
                || area.y0 < -OFF_PAGE_SLACK_MM
                || area.x1 > page[0] + OFF_PAGE_SLACK_MM
                || area.y1 > page[1] + OFF_PAGE_SLACK_MM
        };
        let mut slots = self.slots;
        let mut strokes = Vec::with_capacity(self.list.len());
        let mut numbers = Vec::with_capacity(self.list.len());
        let mut off_page = Vec::new();
        let mut empty_removed = 0;
        for number in self.list {
            let Some(slot) = slots.get_mut(number) else {
                continue;
            };
            let Some(stroke) = slot.stroke.take() else {
                continue;
            };
            if stroke.points.is_empty() {
                empty_removed += 1;
                continue;
            }
            let area = area_of(&stroke.points);
            let touched = number >= given_count || area != slot.given.area;
            if page_known && touched && area.as_ref().is_some_and(beyond) {
                off_page.push(number);
            }
            strokes.push(stroke);
            numbers.push(number);
        }
        off_page.sort_unstable();
        let report = Report {
            strokes_before: given_count,
            strokes_after: strokes.len(),
            empty_removed,
            off_page,
            strokes_pushed: self.pushed.len(),
            pushed_ms: self.pushed_ms,
            strokes_faster: self.faster.len(),
            orders,
        };
        Revised {
            strokes,
            numbers,
            report,
        }
    }
}

/// Where a `copy` goes.
struct Placement {
    to_x: Option<f32>,
    dx: f32,
    dy: f32,
    factor: f32,
}

/// When a stroke starts: its first sample, the replay's first appearance.
pub(crate) fn start_of(stroke: &Stroke) -> Option<u32> {
    stroke.points.first().map(|p| p.t)
}

/// When a stroke starts and ends: times out of order are not trusted to go backwards, as in the
/// replay.
pub(crate) fn span_of(stroke: &Stroke) -> Option<(u32, u32)> {
    let start = start_of(stroke)?;
    let end = stroke.points.iter().map(|p| p.t).max()?.max(start);
    Some((start, end))
}
