//! Page backgrounds: paper colour, printed templates (in millimetres, at any scale) and the
//! optional paper texture.
//!
//! The template geometry ([`draw_template`]) is written once over [`TemplateCanvas`]:
//!
//! - the raster canvas snaps lines to whole pixels so that they stay crisp; a line thinner
//!   than a pixel is drawn one pixel wide and proportionally lighter, and a family of lines
//!   fades out when its spacing gets down to a few pixels (thumbnails), instead of turning
//!   grey;
//! - the vector collector keeps the exact positions and widths, without fading, as
//!   rectangles and discs for the SVG and PDF writers ([`vector_background`]); the texture,
//!   when on, becomes an image of the paper at a moderate resolution.

use ardoise_core::note::{Background, PageSize, Paper, Template};
use ardoise_ink::Rgba;
use tiny_skia::{Color, Paint, PathBuilder, Pixmap, Rect, Transform};

use crate::look::UNITS_PER_INCH;
use crate::noise::value_noise;
use crate::{Image, MAX_SIDE, image_from};

/// Device units per millimetre.
const UNITS_PER_MM: f32 = 100.0;
/// Seed of the paper texture: the same paper looks the same in every note.
const PAPER_SEED: u32 = 0x5eed_9a9e;
/// Side of the paper texture tile, in millimetres. The grain is computed once per tile and
/// repeated every 10 cm, which a fine irregular grain does not show.
const TEXTURE_TILE_MM: f32 = 100.0;

/// Paper colours (SPEC §4.4).
const WHITE: Rgba = Rgba::rgb(255, 255, 255);
const CREAM: Rgba = Rgba::rgb(250, 244, 226);
const KRAFT: Rgba = Rgba::rgb(206, 174, 128);
const SLATE: Rgba = Rgba::rgb(42, 50, 48);

/// The opaque colour of `paper` (a custom colour with alpha is composited over white).
pub fn paper_color(paper: Paper) -> Rgba {
    match paper {
        Paper::White => WHITE,
        Paper::Cream => CREAM,
        Paper::Kraft => KRAFT,
        Paper::Slate => SLATE,
        Paper::Custom(c) => {
            let a = u16::from(c.a);
            let over_white = |v: u8| ((u16::from(v) * a + 255 * (255 - a) + 127) / 255) as u8;
            Rgba::rgb(over_white(c.r), over_white(c.g), over_white(c.b))
        }
    }
}

/// True for papers on which light ink shows best (slate, dark custom colours).
pub(crate) fn is_dark(paper: Paper) -> bool {
    let c = paper_color(paper);
    let luminance = 0.2126 * f32::from(c.r) + 0.7152 * f32::from(c.g) + 0.0722 * f32::from(c.b);
    luminance < 110.0
}

/// One family of printed lines: colour, opacity and thickness in millimetres.
#[derive(Debug, Clone, Copy)]
struct Line {
    rgb: [u8; 3],
    alpha: f32,
    width_mm: f32,
}

/// Template colours for light papers and for dark ones.
struct Palette {
    rule: Line,
    seyes_thin: Line,
    seyes_strong: Line,
    margin: Line,
    grid: Line,
    mm_fine: Line,
    mm_mid: Line,
    mm_bold: Line,
    dot: Line,
    staff: Line,
}

impl Palette {
    fn for_paper(dark: bool) -> Palette {
        let line = |rgb, alpha, width_mm| Line {
            rgb,
            alpha,
            width_mm,
        };
        if dark {
            let chalk = [235, 238, 230];
            Palette {
                rule: line(chalk, 0.30, 0.20),
                seyes_thin: line(chalk, 0.18, 0.12),
                seyes_strong: line(chalk, 0.34, 0.20),
                margin: line([255, 140, 150], 0.50, 0.25),
                grid: line(chalk, 0.24, 0.15),
                mm_fine: line([240, 200, 150], 0.14, 0.08),
                mm_mid: line([240, 200, 150], 0.26, 0.15),
                mm_bold: line([240, 200, 150], 0.40, 0.25),
                dot: line(chalk, 0.45, 0.60),
                staff: line(chalk, 0.45, 0.15),
            }
        } else {
            let blue = [60, 110, 190];
            let orange = [225, 115, 40];
            Palette {
                rule: line(blue, 0.45, 0.20),
                seyes_thin: line([95, 140, 215], 0.42, 0.12),
                seyes_strong: line([80, 80, 185], 0.62, 0.20),
                margin: line([220, 40, 70], 0.75, 0.25),
                grid: line(blue, 0.36, 0.15),
                mm_fine: line(orange, 0.32, 0.08),
                mm_mid: line(orange, 0.50, 0.15),
                mm_bold: line(orange, 0.75, 0.25),
                dot: line([70, 80, 100], 0.60, 0.60),
                staff: line([40, 40, 50], 0.65, 0.15),
            }
        }
    }
}

/// Where template lines are drawn, in millimetres from the top-left corner of the page.
trait TemplateCanvas {
    /// Width and height of the page in millimetres.
    fn size_mm(&self) -> (f32, f32);
    /// Opacity factor for a family of lines `spacing_mm` apart (1 = fully drawn).
    fn density(&self, spacing_mm: f32) -> f32;
    /// A horizontal line centred on `y_mm`, from `x0_mm` to `x1_mm`.
    fn hline(&mut self, y_mm: f32, x0_mm: f32, x1_mm: f32, line: Line, fade: f32);
    /// A vertical line centred on `x_mm`, from `y0_mm` to `y1_mm`.
    fn vline(&mut self, x_mm: f32, y0_mm: f32, y1_mm: f32, line: Line, fade: f32);
    /// A dot of diameter `line.width_mm` centred on `(x_mm, y_mm)`.
    fn dot(&mut self, x_mm: f32, y_mm: f32, line: Line, fade: f32);
}

/// Draws template lines on a pixmap, in millimetres.
struct Canvas<'a> {
    pixmap: &'a mut Pixmap,
    px_per_mm: f32,
    width_mm: f32,
    height_mm: f32,
}

impl Canvas<'_> {
    fn fill(&mut self, x: f32, y: f32, w: f32, h: f32, rgb: [u8; 3], alpha: f32) {
        let Some(rect) = Rect::from_xywh(x, y, w, h) else {
            return;
        };
        let mut paint = Paint::default();
        let mut color = Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255);
        color.set_alpha(alpha.clamp(0.0, 1.0));
        paint.set_color(color);
        self.pixmap
            .fill_rect(rect, &paint, Transform::identity(), None);
    }

    /// Pixel span `(start, size)` of a line centred at `pos_mm`, and the opacity factor that
    /// keeps a sub-pixel line as dark on average as its true width.
    fn span(&self, pos_mm: f32, width_mm: f32) -> (f32, f32, f32) {
        let pos = pos_mm * self.px_per_mm;
        let width = width_mm * self.px_per_mm;
        if width < 1.0 {
            (pos.floor(), 1.0, width)
        } else {
            let size = width.round();
            ((pos - size / 2.0).round(), size, 1.0)
        }
    }
}

impl TemplateCanvas for Canvas<'_> {
    fn size_mm(&self) -> (f32, f32) {
        (self.width_mm, self.height_mm)
    }

    /// Fades out below 4 px.
    fn density(&self, spacing_mm: f32) -> f32 {
        ((spacing_mm * self.px_per_mm - 1.5) / 2.5).clamp(0.0, 1.0)
    }

    fn hline(&mut self, y_mm: f32, x0_mm: f32, x1_mm: f32, line: Line, fade: f32) {
        let (y, h, k) = self.span(y_mm, line.width_mm);
        let (x0, x1) = (x0_mm * self.px_per_mm, x1_mm * self.px_per_mm);
        self.fill(x0, y, x1 - x0, h, line.rgb, line.alpha * k * fade);
    }

    fn vline(&mut self, x_mm: f32, y0_mm: f32, y1_mm: f32, line: Line, fade: f32) {
        let (x, w, k) = self.span(x_mm, line.width_mm);
        let (y0, y1) = (y0_mm * self.px_per_mm, y1_mm * self.px_per_mm);
        self.fill(x, y0, w, y1 - y0, line.rgb, line.alpha * k * fade);
    }

    fn dot(&mut self, x_mm: f32, y_mm: f32, line: Line, fade: f32) {
        let d = line.width_mm * self.px_per_mm;
        let (x, y) = (x_mm * self.px_per_mm, y_mm * self.px_per_mm);
        if d < 2.5 {
            // Small dots: a crisp square of whole pixels, lighter if smaller than that.
            let size = d.round().max(1.0);
            let k = ((d * d) / (size * size)).min(1.0);
            let (x0, y0) = ((x - size / 2.0).round(), (y - size / 2.0).round());
            self.fill(x0, y0, size, size, line.rgb, line.alpha * k * fade);
        } else if let Some(path) = PathBuilder::from_circle(x, y, d / 2.0) {
            let mut paint = Paint::default();
            let mut color = Color::from_rgba8(line.rgb[0], line.rgb[1], line.rgb[2], 255);
            color.set_alpha((line.alpha * fade).clamp(0.0, 1.0));
            paint.set_color(color);
            self.pixmap.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }
}

/// Positions `start, start + step, …` up to `end` (inclusive).
fn steps(start: f32, end: f32, step: f32) -> impl Iterator<Item = f32> {
    let count = if step > 0.0 && end >= start {
        ((end - start) / step).floor() as usize + 1
    } else {
        0
    };
    (0..count).map(move |i| start + i as f32 * step)
}

/// Offset that centres a grid of `step` on a length `size` (equal margins at both ends).
fn centred(size: f32, step: f32) -> f32 {
    (size - (size / step).floor() * step) / 2.0
}

/// Ruled paper: the first rule's distance from the top and the distance between two rules.
const LINED_FIRST_MM: f32 = 16.0;
const LINED_PITCH_MM: f32 = 8.0;
/// Seyès: its strong horizontal lines start there and are this far apart (the thin ones between
/// them are a quarter of it).
const SEYES_TOP_MM: f32 = 16.0;
const SEYES_PITCH_MM: f32 = 8.0;
/// Squared paper.
const GRID_PITCH_MM: f32 = 5.0;

/// The horizontal rules a person writes on: where the first one is, and the distance between two.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Ruling {
    /// Distance of the first rule from the top of the page, millimetres.
    pub first_mm: f32,
    /// Distance between two rules, millimetres.
    pub pitch_mm: f32,
}

/// The rules of `template` on a page `page_height_mm` high, `None` for the templates without
/// horizontal rules to write on (blank, dotted, millimetric, music staves). For Seyès these are the
/// strong lines (the thin ones between them are a quarter of the pitch); for squared paper, its
/// horizontal lines. Same numbers as [`draw_template`] draws.
pub fn ruling(template: Template, page_height_mm: f32) -> Option<Ruling> {
    match template {
        Template::Lined => Some(Ruling {
            first_mm: LINED_FIRST_MM,
            pitch_mm: LINED_PITCH_MM,
        }),
        Template::Seyes => Some(Ruling {
            first_mm: SEYES_TOP_MM,
            pitch_mm: SEYES_PITCH_MM,
        }),
        Template::Grid5mm => Some(Ruling {
            first_mm: centred(page_height_mm, GRID_PITCH_MM),
            pitch_mm: GRID_PITCH_MM,
        }),
        Template::Blank | Template::Millimetric | Template::Dotted | Template::Music => None,
    }
}

fn draw_template(c: &mut impl TemplateCanvas, template: Template, p: &Palette) {
    let (w, h) = c.size_mm();
    match template {
        Template::Blank => {}
        Template::Lined => {
            // Ruled lines 8 mm apart, below a two-line header.
            let fade = c.density(LINED_PITCH_MM);
            for y in steps(LINED_FIRST_MM, h - 4.0, LINED_PITCH_MM) {
                c.hline(y, 0.0, w, p.rule, fade);
            }
        }
        Template::Seyes => {
            // 8 mm squares (strong lines), 3 thin lines between the horizontal ones, vertical
            // lines right of a margin at 32 mm.
            const MARGIN: f32 = 32.0;
            const TOP: f32 = SEYES_TOP_MM;
            let thin_fade = c.density(2.0);
            let strong_fade = c.density(SEYES_PITCH_MM);
            for (i, y) in steps(TOP, h, SEYES_PITCH_MM / 4.0).enumerate() {
                if i % 4 != 0 {
                    c.hline(y, 0.0, w, p.seyes_thin, thin_fade);
                }
            }
            for y in steps(TOP, h, SEYES_PITCH_MM) {
                c.hline(y, 0.0, w, p.seyes_strong, strong_fade);
            }
            for x in steps(MARGIN + 8.0, w, 8.0) {
                c.vline(x, 0.0, h, p.seyes_strong, strong_fade);
            }
            c.vline(MARGIN, 0.0, h, p.margin, 1.0);
        }
        Template::Grid5mm => {
            let fade = c.density(GRID_PITCH_MM);
            for y in steps(centred(h, GRID_PITCH_MM), h, GRID_PITCH_MM) {
                c.hline(y, 0.0, w, p.grid, fade);
            }
            for x in steps(centred(w, GRID_PITCH_MM), w, GRID_PITCH_MM) {
                c.vline(x, 0.0, h, p.grid, fade);
            }
        }
        Template::Millimetric => {
            // 1 mm lines, bolder every 5 mm and 10 mm; the 10 mm grid is centred.
            let (ox, oy) = (centred(w, 10.0), centred(h, 10.0));
            let families = [
                (p.mm_fine, c.density(1.0)),
                (p.mm_mid, c.density(5.0)),
                (p.mm_bold, c.density(10.0)),
            ];
            let family = |i: i32| -> usize {
                if i.rem_euclid(10) == 0 {
                    2
                } else if i.rem_euclid(5) == 0 {
                    1
                } else {
                    0
                }
            };
            // Finest first, so that bolder lines sit on top.
            for (level, &(line, fade)) in families.iter().enumerate() {
                for i in -(oy.floor() as i32)..=((h - oy).floor() as i32) {
                    if family(i) == level {
                        c.hline(oy + i as f32, 0.0, w, line, fade);
                    }
                }
                for i in -(ox.floor() as i32)..=((w - ox).floor() as i32) {
                    if family(i) == level {
                        c.vline(ox + i as f32, 0.0, h, line, fade);
                    }
                }
            }
        }
        Template::Dotted => {
            let fade = c.density(5.0);
            for y in steps(centred(h, 5.0), h, 5.0) {
                for x in steps(centred(w, 5.0), w, 5.0) {
                    c.dot(x, y, p.dot, fade);
                }
            }
        }
        Template::Music => {
            // Five-line staves: 2 mm between lines, 12 mm between staves, centred, with
            // 12 mm side margins and a thin line closing each end of a staff.
            const LINE_GAP: f32 = 2.0;
            const STAFF: f32 = 4.0 * LINE_GAP;
            const PITCH: f32 = STAFF + 12.0;
            const SIDE: f32 = 12.0;
            const MIN_TOP: f32 = 12.0;
            let count = ((h - 2.0 * MIN_TOP + (PITCH - STAFF)) / PITCH)
                .floor()
                .max(0.0);
            let top = (h - (count * PITCH - (PITCH - STAFF))) / 2.0;
            let fade = c.density(LINE_GAP);
            for s in 0..count as usize {
                let y0 = top + s as f32 * PITCH;
                for l in 0..5 {
                    c.hline(y0 + l as f32 * LINE_GAP, SIDE, w - SIDE, p.staff, fade);
                }
                c.vline(SIDE, y0, y0 + STAFF, p.staff, fade);
                c.vline(w - SIDE, y0, y0 + STAFF, p.staff, fade);
            }
        }
    }
}

/// Paper grain: per-pixel multiplicative darkening on light papers (or brightening on dark
/// ones, like chalk dust), from value noise in millimetres so that it looks the same at
/// every scale.
fn apply_texture(pixmap: &mut Pixmap, paper: Paper, px_per_mm: f32) {
    let dark = is_dark(paper);
    let (amount, weights, fibres) = match paper {
        Paper::White => (0.035, [0.55, 0.30, 0.15], 0.0),
        Paper::Cream => (0.045, [0.55, 0.30, 0.15], 0.0),
        Paper::Kraft => (0.10, [0.40, 0.25, 0.15], 0.20),
        Paper::Slate => (0.075, [0.30, 0.25, 0.45], 0.0),
        Paper::Custom(_) if dark => (0.06, [0.30, 0.25, 0.45], 0.0),
        Paper::Custom(_) => (0.045, [0.55, 0.30, 0.15], 0.0),
    };
    let (width, height) = (pixmap.width() as usize, pixmap.height() as usize);
    if width == 0 || height == 0 || px_per_mm.is_nan() || px_per_mm <= 0.0 {
        return;
    }
    // The grain is computed once over a seamless tile and repeated. The tile is a whole
    // number of pixels, so its millimetre scale is adjusted by less than half a pixel.
    let tile = (TEXTURE_TILE_MM * px_per_mm).round().max(1.0) as usize;
    let mm_per_px = TEXTURE_TILE_MM / tile as f32;
    let (tile_w, tile_h) = (tile.min(width), tile.min(height));
    // Lattice frequencies (cells per millimetre) fit a whole number of times in the tile.
    let octave = |x: f32, y: f32, fx: f32, fy: f32, seed: u32| {
        let period = |f: f32| ((TEXTURE_TILE_MM * f).round() as i32).max(1);
        value_noise(x * fx, y * fy, seed, Some((period(fx), period(fy))))
    };
    // Per tile pixel: a factor on light papers, an offset on dark ones.
    let mut effect = Vec::with_capacity(tile_w * tile_h);
    for ty in 0..tile_h {
        for tx in 0..tile_w {
            let (x, y) = ((tx as f32 + 0.5) * mm_per_px, (ty as f32 + 0.5) * mm_per_px);
            let mut n = weights[0] * octave(x, y, 4.0, 4.0, PAPER_SEED)
                + weights[1] * octave(x, y, 1.0, 1.0, PAPER_SEED ^ 1)
                + weights[2] * octave(x, y, 0.2, 0.2, PAPER_SEED ^ 2);
            if fibres > 0.0 {
                // Fibres: cells stretched along x (2.5 mm long, 0.25 mm thick).
                n += fibres * octave(x, y, 0.4, 4.0, PAPER_SEED ^ 3);
            }
            effect.push(if dark {
                amount * 255.0 * n
            } else {
                1.0 - amount * n
            });
        }
    }
    for (y, row) in pixmap.data_mut().chunks_exact_mut(width * 4).enumerate() {
        let tile_row = &effect[(y % tile) * tile_w..][..tile_w];
        for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let e = tile_row[x % tile];
            for c in &mut px[..3] {
                let v = f32::from(*c);
                let v = if dark { v + e } else { v * e };
                // `as` saturates to 0..=255; adding 0.5 rounds.
                *c = (v + 0.5) as u8;
            }
        }
    }
}

/// Paints the page background: paper colour, template, texture.
pub(crate) fn paint(pixmap: &mut Pixmap, background: &Background, scale: f32) {
    let paper = paper_color(background.paper);
    pixmap.fill(Color::from_rgba8(paper.r, paper.g, paper.b, 255));
    let px_per_mm = scale * UNITS_PER_MM;
    let palette = Palette::for_paper(is_dark(background.paper));
    let (width_mm, height_mm) = (
        pixmap.width() as f32 / px_per_mm,
        pixmap.height() as f32 / px_per_mm,
    );
    let mut canvas = Canvas {
        pixmap: &mut *pixmap,
        px_per_mm,
        width_mm,
        height_mm,
    };
    draw_template(&mut canvas, background.template, &palette);
    if background.texture {
        apply_texture(pixmap, background.paper, px_per_mm);
    }
}

/// Resolution of the paper texture image in vector exports: its finest grain is 0.25 mm.
const TEXTURE_DPI: f32 = 100.0;

/// A printed shape of the template, in device units (10 µm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Shape {
    Rect {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    },
    Disc {
        cx: f32,
        cy: f32,
        r: f32,
    },
}

/// A shape with its colour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Mark {
    pub shape: Shape,
    pub rgb: [u8; 3],
    /// 0..1.
    pub alpha: f32,
}

/// The page background for the vector formats.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct VectorBackground {
    pub paper: Rgba,
    /// Light ink shows best on it (the highlighter screens instead of multiplying).
    pub dark: bool,
    /// The template, in drawing order.
    pub marks: Vec<Mark>,
    /// The paper with its texture (no template), to stretch over the whole page; `None` when
    /// the texture is off (or the image could not be made: the plain paper colour remains).
    pub texture: Option<Image>,
}

/// Collects the template as exact shapes: no pixel snapping, no fading.
struct Collector {
    width_mm: f32,
    height_mm: f32,
    marks: Vec<Mark>,
}

impl Collector {
    fn push(&mut self, shape: Shape, line: Line, fade: f32) {
        let alpha = (line.alpha * fade).clamp(0.0, 1.0);
        let visible = match shape {
            Shape::Rect { width, height, .. } => width > 0.0 && height > 0.0,
            Shape::Disc { r, .. } => r > 0.0,
        };
        if visible && alpha > 0.0 {
            self.marks.push(Mark {
                shape,
                rgb: line.rgb,
                alpha,
            });
        }
    }
}

impl TemplateCanvas for Collector {
    fn size_mm(&self) -> (f32, f32) {
        (self.width_mm, self.height_mm)
    }

    fn density(&self, _spacing_mm: f32) -> f32 {
        1.0
    }

    fn hline(&mut self, y_mm: f32, x0_mm: f32, x1_mm: f32, line: Line, fade: f32) {
        let shape = Shape::Rect {
            x: x0_mm * UNITS_PER_MM,
            y: (y_mm - line.width_mm / 2.0) * UNITS_PER_MM,
            width: (x1_mm - x0_mm) * UNITS_PER_MM,
            height: line.width_mm * UNITS_PER_MM,
        };
        self.push(shape, line, fade);
    }

    fn vline(&mut self, x_mm: f32, y0_mm: f32, y1_mm: f32, line: Line, fade: f32) {
        let shape = Shape::Rect {
            x: (x_mm - line.width_mm / 2.0) * UNITS_PER_MM,
            y: y0_mm * UNITS_PER_MM,
            width: line.width_mm * UNITS_PER_MM,
            height: (y1_mm - y0_mm) * UNITS_PER_MM,
        };
        self.push(shape, line, fade);
    }

    fn dot(&mut self, x_mm: f32, y_mm: f32, line: Line, fade: f32) {
        let shape = Shape::Disc {
            cx: x_mm * UNITS_PER_MM,
            cy: y_mm * UNITS_PER_MM,
            r: line.width_mm / 2.0 * UNITS_PER_MM,
        };
        self.push(shape, line, fade);
    }
}

/// The background of a `page` for the vector formats: paper colour, the template as exact
/// shapes (same geometry as the raster template) and, when on, the paper texture as an image.
pub(crate) fn vector_background(background: &Background, page: PageSize) -> VectorBackground {
    let dark = is_dark(background.paper);
    let mut collector = Collector {
        width_mm: page.width as f32 / UNITS_PER_MM,
        height_mm: page.height as f32 / UNITS_PER_MM,
        marks: Vec::new(),
    };
    draw_template(
        &mut collector,
        background.template,
        &Palette::for_paper(dark),
    );
    let texture = if background.texture {
        texture_image(background.paper, page)
    } else {
        None
    };
    VectorBackground {
        paper: paper_color(background.paper),
        dark,
        marks: collector.marks,
        texture,
    }
}

/// The paper with its texture and no template, at [`TEXTURE_DPI`] (less if a side would
/// exceed the largest image side).
fn texture_image(paper: Paper, page: PageSize) -> Option<Image> {
    let longest = page.width.max(page.height).max(1) as f32;
    let scale = (TEXTURE_DPI / UNITS_PER_INCH).min(MAX_SIDE as f32 / longest);
    let side = |units: u32| (units as f32 * scale).round().clamp(1.0, MAX_SIDE as f32) as u32;
    let mut pixmap = Pixmap::new(side(page.width), side(page.height))?;
    let plain = Background {
        paper,
        template: Template::Blank,
        texture: true,
    };
    paint(&mut pixmap, &plain, scale);
    Some(image_from(&pixmap))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_rules_to_write_on_are_those_the_template_draws() {
        let lined = ruling(Template::Lined, 297.0).unwrap();
        assert_eq!((lined.first_mm, lined.pitch_mm), (16.0, 8.0));
        let seyes = ruling(Template::Seyes, 297.0).unwrap();
        assert_eq!((seyes.first_mm, seyes.pitch_mm), (16.0, 8.0));
        // Squared paper: its grid is centred, like the drawing.
        let grid = ruling(Template::Grid5mm, 297.0).unwrap();
        assert_eq!(grid.pitch_mm, 5.0);
        assert!((grid.first_mm - 1.0).abs() < 1e-4, "{}", grid.first_mm);
        for template in [
            Template::Blank,
            Template::Dotted,
            Template::Millimetric,
            Template::Music,
        ] {
            assert_eq!(ruling(template, 297.0), None);
        }
        // The first rule is really drawn there: a pixel row of the rule is not the paper's.
        let drawn = |template: Template| {
            let background = Background {
                template,
                ..Background::default()
            };
            let mut pixmap = Pixmap::new(210, 297).unwrap();
            paint(&mut pixmap, &background, 0.01);
            pixmap
        };
        let lined_page = drawn(Template::Lined);
        let paper_row = |y: u32| lined_page.pixel(100, y).unwrap().red();
        let darker = |rows: std::ops::RangeInclusive<u32>, plain: u32| {
            rows.into_iter().any(|y| paper_row(y) < paper_row(plain))
        };
        assert!(darker(15..=17, 12), "no rule at 16 mm");
        assert!(darker(23..=25, 20), "no rule at 24 mm");
    }

    #[test]
    fn custom_paper_is_composited_over_white() {
        assert_eq!(
            paper_color(Paper::Custom(Rgba::rgb(10, 20, 30))),
            Rgba::rgb(10, 20, 30)
        );
        let half = paper_color(Paper::Custom(Rgba {
            r: 0,
            g: 0,
            b: 0,
            a: 128,
        }));
        assert_eq!((half.r, half.a), (127, 255));
        assert!(is_dark(Paper::Slate) && !is_dark(Paper::Kraft) && !is_dark(Paper::Cream));
    }

    #[test]
    fn steps_and_centring() {
        assert_eq!(steps(1.0, 3.0, 1.0).collect::<Vec<_>>(), [1.0, 2.0, 3.0]);
        assert_eq!(steps(1.0, 0.0, 1.0).count(), 0);
        assert_eq!(steps(0.0, 1.0, 0.0).count(), 0);
        assert!((centred(297.0, 5.0) - 1.0).abs() < 1e-4);
    }
}
