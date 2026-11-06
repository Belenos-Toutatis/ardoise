//! The first pass over a list of orders, before any work: every number and every name that can be
//! judged without knowing what earlier orders do. Stroke numbers and point ranges are checked when
//! the order runs, as numbers of made strokes depend on what earlier orders found.

use crate::orders::{Order, Part, Select, Shape};
use crate::report::{Ctx, ReviseError};
use crate::shapes::{P, paths, sample_count};
use crate::{
    Input, MAX_COORD_MM, MAX_FACTOR, MAX_ORDERS, MAX_POINTS, MAX_SHAPE_MM, MAX_SIGMA_MM,
    MAX_SLOPE_DEG, MAX_WIDTH_MM, MIN_DASH_MM, MIN_FACTOR,
};

/// A colour as an order writes it: `#rrggbb` or `#rrggbbaa`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Color {
    pub rgb: [u8; 3],
    pub alpha: Option<u8>,
}

impl Color {
    pub fn parse(text: &str) -> Option<Self> {
        let digits = text.strip_prefix('#')?;
        if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
        let rgb = [byte(0)?, byte(2)?, byte(4)?];
        match digits.len() {
            6 => Some(Self { rgb, alpha: None }),
            8 => Some(Self {
                rgb,
                alpha: Some(byte(6)?),
            }),
            _ => None,
        }
    }
}

fn parse_color(ctx: Ctx, name: &str, text: &str) -> Result<(), ReviseError> {
    Color::parse(text).map(|_| ()).ok_or_else(|| {
        ctx.err(format!(
            "`{name}` must be written #rrggbb or #rrggbbaa, not \"{text}\""
        ))
    })
}

fn finite(ctx: Ctx, name: &str, value: f32) -> Result<(), ReviseError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(ctx.err(format!("`{name}` is not a finite number")))
    }
}

/// A place or a distance on the page.
fn millimetres(ctx: Ctx, name: &str, value: f32) -> Result<(), ReviseError> {
    finite(ctx, name, value)?;
    if value.abs() > MAX_COORD_MM {
        return Err(ctx.err(format!(
            "`{name}` is {value} mm: nothing is further than {MAX_COORD_MM} mm"
        )));
    }
    Ok(())
}

fn point(ctx: Ctx, name: &str, value: P) -> Result<(), ReviseError> {
    millimetres(ctx, name, value[0])?;
    millimetres(ctx, name, value[1])
}

fn optional(ctx: Ctx, name: &str, value: Option<f32>) -> Result<(), ReviseError> {
    value.map_or(Ok(()), |value| millimetres(ctx, name, value))
}

fn factor(ctx: Ctx, name: &str, value: Option<f32>) -> Result<(), ReviseError> {
    let Some(value) = value else {
        return Ok(());
    };
    finite(ctx, name, value)?;
    if !(MIN_FACTOR..=MAX_FACTOR).contains(&value) {
        return Err(ctx.err(format!(
            "`{name}` is {value}: it must be between {MIN_FACTOR} and {MAX_FACTOR}"
        )));
    }
    Ok(())
}

fn sigma(ctx: Ctx, name: &str, value: f32) -> Result<(), ReviseError> {
    finite(ctx, name, value)?;
    if value <= 0.0 || value > MAX_SIGMA_MM {
        return Err(ctx.err(format!(
            "`{name}` is {value} mm: it must be above 0 and at most {MAX_SIGMA_MM}"
        )));
    }
    Ok(())
}

fn pen_width(ctx: Ctx, value: Option<f32>) -> Result<(), ReviseError> {
    let Some(value) = value else {
        return Ok(());
    };
    finite(ctx, "width", value)?;
    if value <= 0.0 || value > MAX_WIDTH_MM {
        return Err(ctx.err(format!(
            "`width` is {value} mm: it must be above 0 and at most {MAX_WIDTH_MM}"
        )));
    }
    Ok(())
}

fn layer(ctx: Ctx, input: &Input, name: &str, value: Option<u32>) -> Result<(), ReviseError> {
    match value {
        Some(value) if value >= input.layers => Err(ctx.err(format!(
            "`{name}` is {value}: this note has {} layer(s), numbered from 0",
            input.layers
        ))),
        _ => Ok(()),
    }
}

fn select(ctx: Ctx, input: &Input, select: &Select) -> Result<(), ReviseError> {
    if let Some(rect) = select.rect {
        for value in rect {
            millimetres(ctx, "rect", value)?;
        }
        if rect[2] < 0.0 || rect[3] < 0.0 {
            return Err(ctx.err("`rect` is [x, y, w, h]: its width and height cannot be negative"));
        }
    }
    if let Some(text) = &select.color {
        parse_color(ctx, "select.color", text)?;
    }
    layer(ctx, input, "select.layer", select.layer)?;
    for (name, value) in [("min_size", select.min_size), ("max_size", select.max_size)] {
        optional(ctx, name, value)?;
        if value.is_some_and(|value| value < 0.0) {
            return Err(ctx.err(format!("`{name}` cannot be negative")));
        }
    }
    Ok(())
}

/// Checks a shape's numbers and size. A shape is checked before its vertices are computed, which
/// their number depends on.
fn shape(ctx: Ctx, shape: &Shape) -> Result<(), ReviseError> {
    let too_large = |size: f32| {
        Err(ctx.err(format!(
            "a shape of {size} mm: shapes larger than {MAX_SHAPE_MM} mm are refused"
        )))
    };
    let heads = |head: Option<f32>| -> Result<(), ReviseError> {
        let Some(head) = head else {
            return Ok(());
        };
        finite(ctx, "head", head)?;
        if head <= 0.0 || head > MAX_SHAPE_MM {
            return Err(ctx.err(format!(
                "`head` is {head} mm: it must be above 0 and at most {MAX_SHAPE_MM}"
            )));
        }
        Ok(())
    };
    let mut vertices: Vec<P> = Vec::new();
    match shape {
        Shape::Line { from, to } => vertices.extend([*from, *to]),
        Shape::Polyline { points, .. } | Shape::Curve { points } => {
            if points.len() < 2 {
                return Err(ctx.err("a polyline or a curve needs at least 2 points"));
            }
            vertices.extend(points);
        }
        Shape::Rect { x, y, w, h } => {
            for (name, value) in [("x", x), ("y", y), ("w", w), ("h", h)] {
                millimetres(ctx, name, *value)?;
            }
            if *w == 0.0 && *h == 0.0 {
                return Err(ctx.err("a rectangle of no size"));
            }
            vertices.extend([[*x, *y], [x + w, y + h]]);
        }
        Shape::Circle { c, r } => {
            point(ctx, "c", *c)?;
            finite(ctx, "r", *r)?;
            if *r <= 0.0 {
                return Err(ctx.err("the radius of a circle must be above 0"));
            }
            if r * 2.0 > MAX_SHAPE_MM {
                return too_large(r * 2.0);
            }
        }
        Shape::Ellipse { c, rx, ry, angle } => {
            point(ctx, "c", *c)?;
            finite(ctx, "rx", *rx)?;
            finite(ctx, "ry", *ry)?;
            finite(ctx, "angle", *angle)?;
            if *rx <= 0.0 || *ry <= 0.0 {
                return Err(ctx.err("the half-axes of an ellipse must be above 0"));
            }
            if rx.max(*ry) * 2.0 > MAX_SHAPE_MM {
                return too_large(rx.max(*ry) * 2.0);
            }
        }
        Shape::Arrow { from, to, head, .. } => {
            heads(*head)?;
            if from == to {
                return Err(ctx.err("an arrow from a point to itself"));
            }
            vertices.extend([*from, *to]);
        }
        Shape::Dashed {
            from,
            to,
            dash,
            gap,
            head,
        } => {
            heads(*head)?;
            finite(ctx, "dash", *dash)?;
            finite(ctx, "gap", *gap)?;
            if *dash < MIN_DASH_MM || *gap < MIN_DASH_MM {
                return Err(ctx.err(format!(
                    "`dash` is {dash} and `gap` {gap}: each must be at least {MIN_DASH_MM} mm"
                )));
            }
            if from == to {
                return Err(ctx.err("a dashed line from a point to itself"));
            }
            vertices.extend([*from, *to]);
        }
    }
    for vertex in &vertices {
        point(ctx, "point", *vertex)?;
    }
    if let Some(area) = crate::geom::Area::around(vertices.iter().copied())
        && area.size() > MAX_SHAPE_MM
    {
        return too_large(area.size());
    }
    Ok(())
}

fn shapes(ctx: Ctx, list: &[Shape], made: &mut usize) -> Result<(), ReviseError> {
    if list.is_empty() {
        return Err(ctx.err("no shape given"));
    }
    for item in list {
        shape(ctx, item)?;
        for path in paths(item) {
            *made = made.saturating_add(sample_count(&path));
        }
        if *made > MAX_POINTS {
            return Err(too_many_points(ctx));
        }
    }
    Ok(())
}

pub(crate) fn too_many_points(ctx: Ctx) -> ReviseError {
    ctx.err(format!(
        "the orders would make more than {MAX_POINTS} points in total"
    ))
}

fn part(ctx: Ctx, part: &Part) -> Result<(), ReviseError> {
    factor(ctx, "sx", part.sx)?;
    factor(ctx, "sy", part.sy)?;
    millimetres(ctx, "x", part.x)?;
    millimetres(ctx, "y", part.y)?;
    if let (Some(from), Some(to)) = (part.from, part.to)
        && from > to
    {
        return Err(ctx.err(format!("`from` ({from}) is after `to` ({to})")));
    }
    Ok(())
}

/// Refuses a list whose numbers cannot be right. Nothing has been done yet.
pub(crate) fn check(input: &Input, orders: &[Order]) -> Result<(), ReviseError> {
    if orders.len() > MAX_ORDERS {
        return Err(ReviseError::TooManyOrders {
            count: orders.len(),
        });
    }
    let mut made = 0;
    for (index, order) in orders.iter().enumerate() {
        let ctx = Ctx {
            index,
            op: order.op(),
        };
        check_order(ctx, input, order, &mut made)?;
    }
    Ok(())
}

fn check_order(
    ctx: Ctx,
    input: &Input,
    order: &Order,
    made: &mut usize,
) -> Result<(), ReviseError> {
    match order {
        Order::Recolor {
            select: chosen,
            color,
        } => {
            select(ctx, input, chosen)?;
            parse_color(ctx, "color", color)?;
        }
        Order::Restyle {
            select: chosen,
            width,
            tool,
        } => {
            select(ctx, input, chosen)?;
            pen_width(ctx, *width)?;
            if width.is_none() && tool.is_none() {
                return Err(ctx.err("give `width`, `tool` or both"));
            }
        }
        Order::Delete { select: chosen } => select(ctx, input, chosen)?,
        Order::Move {
            select: chosen,
            dx,
            dy,
        } => {
            select(ctx, input, chosen)?;
            millimetres(ctx, "dx", *dx)?;
            millimetres(ctx, "dy", *dy)?;
        }
        Order::Scale {
            select: chosen,
            factor: both,
            sx,
            sy,
            origin,
        } => {
            select(ctx, input, chosen)?;
            point(ctx, "origin", *origin)?;
            factor(ctx, "factor", *both)?;
            factor(ctx, "sx", *sx)?;
            factor(ctx, "sy", *sy)?;
            match (both, sx, sy) {
                (Some(_), None, None) | (None, Some(_), Some(_)) => {}
                _ => return Err(ctx.err("give `factor`, or `sx` and `sy`")),
            }
        }
        Order::Rotate {
            select: chosen,
            degrees,
            origin,
        } => {
            select(ctx, input, chosen)?;
            finite(ctx, "degrees", *degrees)?;
            point(ctx, "origin", *origin)?;
        }
        Order::Smooth {
            select: chosen,
            sigma: value,
        } => {
            select(ctx, input, chosen)?;
            sigma(ctx, "sigma", *value)?;
        }
        Order::Trim { from, to, .. } => {
            if let Some(to) = to
                && from > to
            {
                return Err(ctx.err(format!("`from` ({from}) is after `to` ({to})")));
            }
        }
        Order::Replace {
            select: chosen,
            with,
            color,
            width,
        } => {
            select(ctx, input, chosen)?;
            shapes(ctx, with, made)?;
            if let Some(text) = color {
                parse_color(ctx, "color", text)?;
            }
            pen_width(ctx, *width)?;
        }
        Order::Add {
            shapes: list,
            color,
            width,
            layer: on,
            ..
        } => {
            shapes(ctx, list, made)?;
            parse_color(ctx, "color", color)?;
            pen_width(ctx, *width)?;
            layer(ctx, input, "layer", *on)?;
        }
        Order::Copy {
            select: chosen,
            to_x,
            dx,
            dy,
            factor: size,
            color,
            ..
        } => {
            select(ctx, input, chosen)?;
            optional(ctx, "to_x", *to_x)?;
            optional(ctx, "dx", *dx)?;
            optional(ctx, "dy", *dy)?;
            factor(ctx, "factor", *size)?;
            if to_x.is_some() && dx.is_some() {
                return Err(ctx.err("give `to_x` or `dx`, not both"));
            }
            if let Some(text) = color {
                parse_color(ctx, "color", text)?;
            }
        }
        Order::Glyph { parts, color, .. } => {
            if parts.is_empty() {
                return Err(ctx.err("no part given"));
            }
            for item in parts {
                part(ctx, item)?;
            }
            if let Some(text) = color {
                parse_color(ctx, "color", text)?;
            }
        }
        Order::Retime { select: chosen, .. } => select(ctx, input, chosen)?,
        Order::TidyLine {
            select: chosen,
            baseline,
            left,
            scale,
            slope,
            word_gap,
            smooth,
            ..
        } => {
            select(ctx, input, chosen)?;
            optional(ctx, "baseline", *baseline)?;
            optional(ctx, "left", *left)?;
            factor(ctx, "scale", *scale)?;
            if let Some(slope) = slope {
                finite(ctx, "slope", *slope)?;
                if slope.abs() > MAX_SLOPE_DEG {
                    return Err(ctx.err(format!(
                        "`slope` is {slope} degrees: a written line is not steeper than {MAX_SLOPE_DEG}"
                    )));
                }
            }
            optional(ctx, "word_gap", *word_gap)?;
            if word_gap.is_some_and(|gap| gap < 0.0) {
                return Err(ctx.err("`word_gap` cannot be negative"));
            }
            if let Some(value) = smooth {
                sigma(ctx, "smooth", *value)?;
            }
        }
    }
    Ok(())
}
