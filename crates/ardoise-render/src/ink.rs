//! Ink: fills each stroke's outline (from `ardoise_ink::outline`) with the look of its tool.
//!
//! | Tool          | Look                                                                    |
//! |---------------|-------------------------------------------------------------------------|
//! | `Ballpoint`   | solid                                                                   |
//! | `FountainPen` | solid (the width already depends on the direction)                      |
//! | `FeltTip`     | slightly translucent core over a faint, wider halo: a soft edge         |
//! | `Pencil`      | graphite grain (seeded per stroke), darker with more pressure           |
//! | `Brush`       | solid (the outline is tapered)                                          |
//! | `Highlighter` | multiply blending: colours the paper, leaves dark ink dark (screen on dark paper) |
//! | `Chalk`       | coarse grain with gaps, like chalk on a blackboard                      |
//!
//! The stroke colour's alpha multiplies every tool's own opacity. A stroke is filled as one
//! path, so where it crosses itself it is not painted twice.

use std::collections::HashMap;

use ardoise_ink::{Outline, Stroke, Tool, outline};
use tiny_skia::{
    BlendMode, Color, ColorU8, FillRule, FilterQuality, Paint, Path, PathBuilder, Pattern, Pixmap,
    Shader, SpreadMode, Transform,
};

use crate::noise::{hash, hash2, unit, value_noise};

/// Felt tip: opacity of the core and of the halo, and how much wider the halo is.
const FELT_CORE: f32 = 0.9;
const FELT_HALO: f32 = 0.3;
const FELT_HALO_WIDTH: f32 = 1.06;
/// Extra halo diameter in pixels, so that the soft edge stays visible when zoomed out.
const FELT_HALO_PX: f32 = 1.0;
/// Pencil: opacity at the lightest and at full (mean) pressure.
const PENCIL_LIGHT: f32 = 0.55;
const PENCIL_FULL: f32 = 1.0;
/// Chalk opacity.
const CHALK_OPACITY: f32 = 0.95;
/// Size of the repeating grain tiles, in pixels.
const GRAIN_TILE: u32 = 128;
/// Size of one grain cell, in millimetres (never smaller than a pixel).
const GRAIN_MM: f32 = 0.18;

/// Which grain texture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Grain {
    Pencil,
    Chalk,
}

/// Draws strokes on one page. Keeps the grain tiles made so far (one per grain and colour).
pub(crate) struct InkPainter {
    /// Pixels per device unit.
    scale: f32,
    /// The page is dark: the highlighter lightens instead of darkening.
    dark_paper: bool,
    width: f32,
    height: f32,
    /// `None` if a tile could not be allocated (the grain then falls back to solid ink).
    tiles: HashMap<(Grain, [u8; 3]), Option<Pixmap>>,
}

impl InkPainter {
    pub(crate) fn new(pixmap: &Pixmap, scale: f32, dark_paper: bool) -> Self {
        Self {
            scale,
            dark_paper,
            width: pixmap.width() as f32,
            height: pixmap.height() as f32,
            tiles: HashMap::new(),
        }
    }

    /// `outline` as a path in pixels, or `None` if nothing of it falls on the page.
    fn path(&self, outline: &Outline) -> Option<Path> {
        let b = outline.bounds()?;
        let s = self.scale;
        let visible = b.max_x * s >= -1.0
            && b.max_y * s >= -1.0
            && b.min_x * s <= self.width + 1.0
            && b.min_y * s <= self.height + 1.0;
        if !visible {
            return None;
        }
        let mut builder = PathBuilder::new();
        for contour in &outline.contours {
            let mut points = contour.iter();
            let Some(&[x, y]) = points.next() else {
                continue;
            };
            builder.move_to(x * s, y * s);
            for &[x, y] in points {
                builder.line_to(x * s, y * s);
            }
            builder.close();
        }
        builder.finish()
    }

    pub(crate) fn draw(&mut self, pixmap: &mut Pixmap, stroke: &Stroke) {
        let Some(path) = self.path(&outline(stroke)) else {
            return;
        };
        let c = stroke.color;
        let alpha = f32::from(c.a) / 255.0;
        let rgb = [c.r, c.g, c.b];
        match stroke.tool {
            Tool::Ballpoint | Tool::FountainPen | Tool::Brush => {
                fill_solid(pixmap, &path, rgb, alpha, BlendMode::SourceOver);
            }
            Tool::FeltTip => {
                let halo = Stroke {
                    width: stroke.width * FELT_HALO_WIDTH + FELT_HALO_PX / self.scale,
                    ..stroke.clone()
                };
                if let Some(halo_path) = self.path(&outline(&halo)) {
                    fill_solid(
                        pixmap,
                        &halo_path,
                        rgb,
                        alpha * FELT_HALO,
                        BlendMode::SourceOver,
                    );
                }
                fill_solid(pixmap, &path, rgb, alpha * FELT_CORE, BlendMode::SourceOver);
            }
            Tool::Highlighter => {
                let blend = if self.dark_paper {
                    BlendMode::Screen
                } else {
                    BlendMode::Multiply
                };
                fill_solid(pixmap, &path, rgb, alpha, blend);
            }
            Tool::Pencil => {
                let pressure = mean_pressure(stroke);
                let opacity = alpha * (PENCIL_LIGHT + (PENCIL_FULL - PENCIL_LIGHT) * pressure);
                self.fill_grain(pixmap, &path, Grain::Pencil, rgb, opacity, stroke);
            }
            Tool::Chalk => {
                self.fill_grain(
                    pixmap,
                    &path,
                    Grain::Chalk,
                    rgb,
                    alpha * CHALK_OPACITY,
                    stroke,
                );
            }
        }
    }

    /// Fills `path` with a grain tile of `rgb`, shifted by a per-stroke seed.
    fn fill_grain(
        &mut self,
        pixmap: &mut Pixmap,
        path: &Path,
        grain: Grain,
        rgb: [u8; 3],
        opacity: f32,
        stroke: &Stroke,
    ) {
        let cell = (GRAIN_MM * 100.0 * self.scale).max(1.0);
        let seed = stroke_seed(stroke);
        let (dx, dy) = (
            (seed % GRAIN_TILE) as f32,
            ((seed >> 8) % GRAIN_TILE) as f32,
        );
        let Some(tile) = self
            .tiles
            .entry((grain, rgb))
            .or_insert_with(|| grain_tile(grain, rgb))
            .as_ref()
            .map(Pixmap::as_ref)
        else {
            fill_solid(pixmap, path, rgb, opacity, BlendMode::SourceOver);
            return;
        };
        let quality = if cell > 1.0 {
            FilterQuality::Bilinear
        } else {
            FilterQuality::Nearest
        };
        let transform = Transform::from_row(cell, 0.0, 0.0, cell, dx * cell, dy * cell);
        let paint = Paint {
            shader: Pattern::new(tile, SpreadMode::Repeat, quality, opacity, transform),
            ..Paint::default()
        };
        pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

fn fill_solid(pixmap: &mut Pixmap, path: &Path, rgb: [u8; 3], alpha: f32, blend: BlendMode) {
    let mut color = Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255);
    color.set_alpha(alpha.clamp(0.0, 1.0));
    let paint = Paint {
        shader: Shader::SolidColor(color),
        blend_mode: blend,
        ..Paint::default()
    };
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

/// Mean pressure of the stroke's usable points (1 if there is none).
fn mean_pressure(stroke: &Stroke) -> f32 {
    let (sum, count) = stroke
        .points
        .iter()
        .filter(|p| p.pressure.is_finite())
        .fold((0.0f64, 0u32), |(s, n), p| {
            (s + f64::from(p.pressure.clamp(0.0, 1.0)), n + 1)
        });
    if count == 0 {
        1.0
    } else {
        (sum / f64::from(count)) as f32
    }
}

/// A seed that depends on the stroke's content only (first point and length), so a stroke
/// keeps its grain when other strokes are added, removed or reordered.
fn stroke_seed(stroke: &Stroke) -> u32 {
    let first = stroke.points.first();
    let x = first.map_or(0, |p| p.x.to_bits());
    let y = first.map_or(0, |p| p.y.to_bits());
    let t = first.map_or(0, |p| p.t);
    hash(x ^ hash(y ^ hash(t ^ hash(stroke.points.len() as u32))))
}

/// Coverage (0..1) of a grain at tile pixel `(x, y)`. Seamless over the tile.
fn grain_coverage(grain: Grain, x: i32, y: i32) -> f32 {
    let white = unit(hash2(x, y, 0x9e4c_11a5));
    match grain {
        Grain::Pencil => {
            // Fine graphite speckle on clumps of paper tooth.
            const CELL: i32 = 4;
            let clump = value_noise(
                x as f32 / CELL as f32,
                y as f32 / CELL as f32,
                0x9e4c_11a6,
                Some((GRAIN_TILE as i32 / CELL, GRAIN_TILE as i32 / CELL)),
            );
            (0.12 + 1.05 * (0.55 * white + 0.45 * clump)).clamp(0.0, 1.0)
        }
        Grain::Chalk => {
            // Mostly covered, with gaps where the chalk skipped over the board's grain.
            const CELL: i32 = 8;
            let clump = value_noise(
                x as f32 / CELL as f32,
                y as f32 / CELL as f32,
                0xc4a1_7001,
                Some((GRAIN_TILE as i32 / CELL, GRAIN_TILE as i32 / CELL)),
            );
            let v = 0.6 * clump + 0.4 * white;
            let t = ((v - 0.28) / 0.30).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        }
    }
}

/// The repeating grain tile of `grain` in colour `rgb` (premultiplied, alpha = coverage).
fn grain_tile(grain: Grain, rgb: [u8; 3]) -> Option<Pixmap> {
    let mut tile = Pixmap::new(GRAIN_TILE, GRAIN_TILE)?;
    let size = GRAIN_TILE as usize;
    for (i, pixel) in tile.pixels_mut().iter_mut().enumerate() {
        let (x, y) = ((i % size) as i32, (i / size) as i32);
        let coverage = grain_coverage(grain, x, y);
        let a = (coverage * 255.0).round() as u8;
        *pixel = ColorU8::from_rgba(rgb[0], rgb[1], rgb[2], a).premultiply();
    }
    Some(tile)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_ink::Point;

    #[test]
    fn grain_tiles_are_seamless_and_mixed() {
        for grain in [Grain::Pencil, Grain::Chalk] {
            let n = GRAIN_TILE as i32;
            let mut sum = 0.0;
            for y in 0..n {
                for x in 0..n {
                    let c = grain_coverage(grain, x, y);
                    assert!((0.0..=1.0).contains(&c));
                    sum += c;
                }
            }
            let mean = sum / (n * n) as f32;
            assert!(
                (0.35..0.9).contains(&mean),
                "{grain:?}: mean coverage {mean}"
            );
            let tile = grain_tile(grain, [10, 20, 30]).unwrap();
            assert_eq!(tile.width(), GRAIN_TILE);
        }
    }

    #[test]
    fn stroke_seed_depends_on_content() {
        let stroke = |x: f32| Stroke {
            tool: Tool::Pencil,
            color: ardoise_ink::Rgba::BLACK,
            width: 30.0,
            layer: 0,
            points: vec![Point {
                x,
                y: 0.0,
                pressure: 0.5,
                t: 0,
            }],
        };
        assert_eq!(stroke_seed(&stroke(1.0)), stroke_seed(&stroke(1.0)));
        assert_ne!(stroke_seed(&stroke(1.0)), stroke_seed(&stroke(2.0)));
        assert_eq!(mean_pressure(&stroke(1.0)), 0.5);
    }
}
