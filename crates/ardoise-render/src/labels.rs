//! Stroke numbers written on a picture of a page, so that someone who looks at the picture (an
//! assistant that retouches a note) can say which stroke it means.
//!
//! The digits are drawn as lines of a hand-made shape (no font, nothing to install), small and red,
//! with a pale halo so that they read over ink and over rules. Their size is the same whatever the
//! scale of the picture: zoomed on a region, the strokes grow and the numbers stay legible. Each
//! number is put next to its stroke, on the first free place around the stroke's box (above,
//! below, right, left, inside), so that numbers rarely cover each other.

use tiny_skia::{
    Color, FillRule, LineCap, LineJoin, Paint, PathBuilder, Pixmap, Rect, Stroke, Transform,
};

use crate::Image;

/// Height of a digit, pixels.
pub const LABEL_HEIGHT_PX: f32 = 11.0;
/// Thickness of a digit's lines, pixels.
const LINE_PX: f32 = 1.4;
/// Thickness of the pale halo behind a digit's lines.
const HALO_PX: f32 = 3.6;
/// Space between a number and its stroke's box, and between two numbers, pixels.
const GAP_PX: f32 = 2.0;
/// Digits are drawn in a box of this many units wide and 5 high.
const GLYPH_UNITS_WIDE: f32 = 3.0;
const GLYPH_UNITS_HIGH: f32 = 5.0;
/// Distance from the start of one digit to the next, in units.
const ADVANCE_UNITS: f32 = 4.4;
/// The red of the numbers.
const RED: (u8, u8, u8) = (214, 24, 24);

/// The digits as polylines in a box of 3 x 5 units, y downwards.
fn glyph(digit: u8) -> &'static [&'static [(f32, f32)]] {
    match digit {
        0 => &[&[(0.0, 0.0), (3.0, 0.0), (3.0, 5.0), (0.0, 5.0), (0.0, 0.0)]],
        1 => &[
            &[(0.4, 1.2), (2.0, 0.0), (2.0, 5.0)],
            &[(0.6, 5.0), (3.0, 5.0)],
        ],
        2 => &[&[
            (0.0, 0.0),
            (3.0, 0.0),
            (3.0, 2.5),
            (0.0, 2.5),
            (0.0, 5.0),
            (3.0, 5.0),
        ]],
        3 => &[
            &[(0.0, 0.0), (3.0, 0.0), (3.0, 5.0), (0.0, 5.0)],
            &[(0.8, 2.5), (3.0, 2.5)],
        ],
        4 => &[
            &[(0.0, 0.0), (0.0, 2.5), (3.0, 2.5)],
            &[(3.0, 0.0), (3.0, 5.0)],
        ],
        5 => &[&[
            (3.0, 0.0),
            (0.0, 0.0),
            (0.0, 2.5),
            (3.0, 2.5),
            (3.0, 5.0),
            (0.0, 5.0),
        ]],
        6 => &[&[
            (3.0, 0.0),
            (0.0, 0.0),
            (0.0, 5.0),
            (3.0, 5.0),
            (3.0, 2.5),
            (0.0, 2.5),
        ]],
        7 => &[&[(0.0, 0.0), (3.0, 0.0), (1.2, 5.0)]],
        8 => &[
            &[(0.0, 0.0), (3.0, 0.0), (3.0, 5.0), (0.0, 5.0), (0.0, 0.0)],
            &[(0.0, 2.5), (3.0, 2.5)],
        ],
        _ => &[&[
            (3.0, 2.5),
            (0.0, 2.5),
            (0.0, 0.0),
            (3.0, 0.0),
            (3.0, 5.0),
            (0.0, 5.0),
        ]],
    }
}

/// A stroke to number: its box on the page, in device units (10 µm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LabelSpot {
    pub number: usize,
    /// `[x0, y0, x1, y1]`.
    pub bounds: [f32; 4],
}

/// A rectangle of the picture, pixels.
#[derive(Debug, Clone, Copy)]
struct Area {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
}

impl Area {
    fn grown(self, by: f32) -> Area {
        Area {
            x0: self.x0 - by,
            y0: self.y0 - by,
            x1: self.x1 + by,
            y1: self.y1 + by,
        }
    }

    fn meets(self, other: Area) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.y0 < other.y1 && other.y0 < self.y1
    }

    fn inside(self, width: f32, height: f32) -> bool {
        self.x0 >= 0.0 && self.y0 >= 0.0 && self.x1 <= width && self.y1 <= height
    }
}

fn digits_of(number: usize) -> Vec<u8> {
    number
        .to_string()
        .bytes()
        .map(|digit| digit - b'0')
        .collect()
}

/// Width of the number's written form, pixels.
fn label_width(digits: usize) -> f32 {
    let unit = LABEL_HEIGHT_PX / GLYPH_UNITS_HIGH;
    ((digits.saturating_sub(1)) as f32 * ADVANCE_UNITS + GLYPH_UNITS_WIDE) * unit
}

/// The place of a number of `width` next to `stroke` (its box in pixels): the first of the
/// places around it that is on the picture and free, else the first that is on the picture, else
/// the first.
fn place(stroke: Area, width: f32, taken: &[Area], picture: (f32, f32)) -> Area {
    let h = LABEL_HEIGHT_PX;
    let (cx, cy) = ((stroke.x0 + stroke.x1) / 2.0, (stroke.y0 + stroke.y1) / 2.0);
    let at = |x: f32, y: f32| Area {
        x0: x,
        y0: y,
        x1: x + width,
        y1: y + h,
    };
    let candidates = [
        at(cx - width / 2.0, stroke.y0 - h - GAP_PX),
        at(cx - width / 2.0, stroke.y1 + GAP_PX),
        at(stroke.x1 + GAP_PX, cy - h / 2.0),
        at(stroke.x0 - width - GAP_PX, cy - h / 2.0),
        at(stroke.x0 + GAP_PX, stroke.y0 + GAP_PX),
    ];
    candidates
        .iter()
        .find(|c| {
            c.inside(picture.0, picture.1) && !taken.iter().any(|t| t.meets(c.grown(GAP_PX / 2.0)))
        })
        .or_else(|| candidates.iter().find(|c| c.inside(picture.0, picture.1)))
        .copied()
        .unwrap_or(candidates[0])
}

fn paint(red: bool, alpha: f32) -> Paint<'static> {
    let mut paint = Paint::default();
    let (r, g, b) = if red { RED } else { (255, 255, 255) };
    paint.set_color(Color::from_rgba8(r, g, b, (alpha * 255.0) as u8));
    paint.anti_alias = true;
    paint
}

/// Draws the number at `area`: a pale halo, then the red lines.
fn draw_number(pixmap: &mut Pixmap, number: usize, area: Area) {
    let unit = LABEL_HEIGHT_PX / GLYPH_UNITS_HIGH;
    // A pale backing, so that the digits read over ink.
    let pad = HALO_PX / 2.0;
    if let Some(rect) = Rect::from_ltrb(area.x0 - pad, area.y0 - pad, area.x1 + pad, area.y1 + pad)
    {
        let mut backing = PathBuilder::new();
        backing.push_rect(rect);
        if let Some(path) = backing.finish() {
            pixmap.fill_path(
                &path,
                &paint(false, 0.72),
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
    let line = Stroke {
        width: LINE_PX,
        line_cap: LineCap::Round,
        line_join: LineJoin::Round,
        ..Stroke::default()
    };
    for (position, digit) in digits_of(number).into_iter().enumerate() {
        let x0 = area.x0 + position as f32 * ADVANCE_UNITS * unit;
        for polyline in glyph(digit) {
            let mut builder = PathBuilder::new();
            for (i, &(x, y)) in polyline.iter().enumerate() {
                let point = (x0 + x * unit, area.y0 + y * unit);
                if i == 0 {
                    builder.move_to(point.0, point.1);
                } else {
                    builder.line_to(point.0, point.1);
                }
            }
            if let Some(path) = builder.finish() {
                pixmap.stroke_path(&path, &paint(true, 1.0), &line, Transform::identity(), None);
            }
        }
    }
}

/// Writes the number of each of `spots` next to its stroke on `image`, which shows the page at
/// `scale` pixels per device unit, its top left corner being the pixel `origin` of that raster (a
/// cut-out of a larger picture has an origin; a whole page has `(0, 0)`). Numbers that fall
/// outside the picture are left out. The picture is meant to be opaque, as a rendered page is.
pub fn draw_labels(image: &mut Image, spots: &[LabelSpot], scale: f32, origin: (i64, i64)) {
    let (width, height) = (image.width as f32, image.height as f32);
    let Some(size) = tiny_skia::IntSize::from_wh(image.width, image.height) else {
        return;
    };
    let Some(mut pixmap) = Pixmap::from_vec(std::mem::take(&mut image.pixels), size) else {
        return;
    };
    let mut spots: Vec<LabelSpot> = spots.to_vec();
    spots.sort_by_key(|spot| spot.number);
    let margin = LABEL_HEIGHT_PX + GAP_PX;
    let mut taken: Vec<Area> = Vec::new();
    for spot in spots {
        let [x0, y0, x1, y1] = spot.bounds;
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            continue;
        }
        let stroke = Area {
            x0: x0 * scale - origin.0 as f32,
            y0: y0 * scale - origin.1 as f32,
            x1: x1 * scale - origin.0 as f32,
            y1: y1 * scale - origin.1 as f32,
        };
        if stroke.x1 < -margin
            || stroke.y1 < -margin
            || stroke.x0 > width + margin
            || stroke.y0 > height + margin
        {
            continue;
        }
        let digits = digits_of(spot.number).len();
        let area = place(stroke, label_width(digits), &taken, (width, height));
        draw_number(&mut pixmap, spot.number, area);
        taken.push(area);
    }
    image.pixels = pixmap.take();
}

#[cfg(test)]
mod tests {
    use super::*;

    fn white(width: u32, height: u32) -> Image {
        Image {
            width,
            height,
            pixels: vec![255; width as usize * height as usize * 4],
        }
    }

    fn red_pixels(image: &Image, area: (u32, u32, u32, u32)) -> usize {
        let mut count = 0;
        for y in area.1..area.3.min(image.height) {
            for x in area.0..area.2.min(image.width) {
                let [r, g, b, _] = image.pixel(x, y).unwrap();
                if r > 150 && g < 120 && b < 120 {
                    count += 1;
                }
            }
        }
        count
    }

    fn spot(number: usize, bounds: [f32; 4]) -> LabelSpot {
        LabelSpot { number, bounds }
    }

    #[test]
    fn a_number_is_written_above_its_stroke() {
        let mut image = white(200, 100);
        // A stroke box at x 80..120, y 50..70 pixels, at a scale of 1 pixel per unit.
        draw_labels(
            &mut image,
            &[spot(7, [80.0, 50.0, 120.0, 70.0])],
            1.0,
            (0, 0),
        );
        // Above the box (the label is 11 px high, 2 px above): red there, nowhere else.
        assert!(red_pixels(&image, (80, 35, 120, 50)) > 10);
        assert_eq!(red_pixels(&image, (0, 0, 200, 30)), 0);
        assert_eq!(red_pixels(&image, (0, 55, 200, 100)), 0);
        assert_eq!(red_pixels(&image, (0, 0, 70, 100)), 0);
    }

    #[test]
    fn every_digit_has_a_shape_of_its_own() {
        let mut shapes = Vec::new();
        for digit in 0..10 {
            let mut image = white(40, 30);
            draw_labels(
                &mut image,
                &[spot(digit, [10.0, 15.0, 30.0, 25.0])],
                1.0,
                (0, 0),
            );
            assert!(red_pixels(&image, (0, 0, 40, 30)) > 8, "digit {digit}");
            shapes.push(image.pixels);
        }
        for a in 0..10 {
            for b in a + 1..10 {
                assert_ne!(shapes[a], shapes[b], "{a} and {b} look the same");
            }
        }
    }

    #[test]
    fn the_size_does_not_depend_on_the_scale_and_a_cut_out_shifts_the_place() {
        // The same stroke, seen at two scales: the number has the same size.
        let at = |scale: f32| {
            let mut image = white(400, 200);
            draw_labels(
                &mut image,
                &[spot(
                    12,
                    [100.0 / scale, 100.0 / scale, 160.0 / scale, 130.0 / scale],
                )],
                scale,
                (0, 0),
            );
            red_pixels(&image, (0, 0, 400, 200))
        };
        let (small, large) = (at(0.5), at(2.0));
        assert!(
            small.abs_diff(large) <= small / 10,
            "{small} against {large}"
        );

        // A cut-out whose corner is at pixel (50, 40) of the page: the number is where the stroke
        // is on the cut-out.
        let mut whole = white(300, 200);
        draw_labels(
            &mut whole,
            &[spot(5, [100.0, 100.0, 140.0, 120.0])],
            1.0,
            (0, 0),
        );
        let mut cut = white(100, 100);
        draw_labels(
            &mut cut,
            &[spot(5, [100.0, 100.0, 140.0, 120.0])],
            1.0,
            (50, 40),
        );
        for y in 0..100 {
            for x in 0..100 {
                assert_eq!(cut.pixel(x, y), whole.pixel(x + 50, y + 40), "({x}, {y})");
            }
        }
    }

    #[test]
    fn two_close_strokes_do_not_get_numbers_on_top_of_each_other() {
        let mut image = white(300, 120);
        // Two boxes side by side, 4 px apart: the second number cannot sit above the first.
        draw_labels(
            &mut image,
            &[
                spot(10, [100.0, 60.0, 112.0, 80.0]),
                spot(11, [116.0, 60.0, 128.0, 80.0]),
            ],
            1.0,
            (0, 0),
        );
        let mut alone = white(300, 120);
        draw_labels(
            &mut alone,
            &[spot(10, [100.0, 60.0, 112.0, 80.0])],
            1.0,
            (0, 0),
        );
        let mut other = white(300, 120);
        draw_labels(
            &mut other,
            &[spot(11, [116.0, 60.0, 128.0, 80.0])],
            1.0,
            (0, 0),
        );
        // Together there are as many red pixels as the two alone: nothing was covered.
        let together = red_pixels(&image, (0, 0, 300, 120));
        let apart = red_pixels(&alone, (0, 0, 300, 120)) + red_pixels(&other, (0, 0, 300, 120));
        assert!(together >= apart * 9 / 10, "{together} < {apart}");
    }

    #[test]
    fn strokes_off_the_picture_and_odd_numbers_are_harmless() {
        let mut image = white(60, 40);
        let before = image.clone();
        draw_labels(
            &mut image,
            &[
                spot(1, [1_000.0, 1_000.0, 1_010.0, 1_010.0]),
                spot(2, [f32::NAN, 0.0, 1.0, 1.0]),
            ],
            1.0,
            (0, 0),
        );
        assert_eq!(image, before);
        // At the edge: still drawn, inside the picture.
        draw_labels(
            &mut image,
            &[spot(123_456, [0.0, 0.0, 5.0, 5.0])],
            1.0,
            (0, 0),
        );
        assert!(red_pixels(&image, (0, 0, 60, 40)) > 0);
        // An empty picture is left as it is.
        let mut empty = Image {
            width: 0,
            height: 0,
            pixels: Vec::new(),
        };
        draw_labels(&mut empty, &[spot(1, [0.0, 0.0, 1.0, 1.0])], 1.0, (0, 0));
    }
}
