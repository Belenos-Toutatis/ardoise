//! What each tool paints: the fills of a stroke, described once and consumed by the raster
//! painter (PNG, JPG, clipboard), the SVG writer and the PDF writer (SPEC §6.2, "un seul
//! dessin des traits"). Every fill's geometry is [`ardoise_ink::outline`] of the stroke (or of
//! the same stroke made wider, for the felt-tip halo), filled with the non-zero rule.
//!
//! | Tool          | Fills                                                                   |
//! |---------------|-------------------------------------------------------------------------|
//! | `Ballpoint`   | solid                                                                   |
//! | `FountainPen` | solid (the width already depends on the direction)                      |
//! | `FeltTip`     | a faint, wider halo, then a slightly translucent core: a soft edge      |
//! | `Pencil`      | graphite grain (seeded per stroke), darker with more pressure           |
//! | `Brush`       | solid (the outline is tapered)                                          |
//! | `Highlighter` | multiply blending: colours the paper, leaves dark ink dark (screen on dark paper) |
//! | `Chalk`       | coarse grain with gaps, like chalk on a blackboard                      |
//!
//! The stroke colour's alpha multiplies every tool's own opacity. A stroke is filled as one
//! path, so where it crosses itself it is not painted twice. Vector formats cannot reproduce
//! the grain: they fill with its mean coverage folded into the opacity
//! ([`Fill::flat_opacity`]).

use std::sync::OnceLock;

use ardoise_ink::{Outline, Stroke, Tool, outline};

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
/// Side of the repeating grain tiles, in pixels.
pub(crate) const GRAIN_TILE: u32 = 128;
/// Vector formats have no pixels: the felt-tip halo is sized as for a print at this
/// resolution (its extra pixel is then 0.08 mm).
const VECTOR_DPI: f32 = 300.0;
/// Device units (10 µm) per inch.
pub(crate) const UNITS_PER_INCH: f32 = 2540.0;

/// How a fill combines with what is below it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Blend {
    /// Paint over (source-over).
    Normal,
    /// Darkens: the highlighter on light paper.
    Multiply,
    /// Lightens: the highlighter on dark paper.
    Screen,
}

/// A paper grain the fill is textured with (raster formats).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Grain {
    Pencil,
    Chalk,
}

impl Grain {
    /// Mean coverage (0..1) of the grain tile as the raster painter uses it: the factor that
    /// vector formats apply to the opacity instead of the grain.
    pub fn mean_coverage(self) -> f32 {
        static MEANS: OnceLock<[f32; 2]> = OnceLock::new();
        let means = MEANS.get_or_init(|| [Grain::Pencil, Grain::Chalk].map(tile_mean));
        match self {
            Grain::Pencil => means[0],
            Grain::Chalk => means[1],
        }
    }
}

/// One filled shape of a stroke.
#[derive(Debug, Clone, PartialEq)]
pub struct Fill {
    /// Closed contours in page coordinates (device units), to fill with the non-zero rule.
    pub outline: Outline,
    pub rgb: [u8; 3],
    /// 0..1, the stroke colour's alpha included.
    pub opacity: f32,
    pub blend: Blend,
    /// Grain texture (raster formats); `None` for a flat fill.
    pub grain: Option<Grain>,
    /// Offset of the grain, from the stroke's content (a stroke keeps its grain when other
    /// strokes are added, removed or reordered).
    pub seed: u32,
}

impl Fill {
    /// Opacity of the fill drawn flat (vector formats): the grain's mean coverage folded in.
    pub fn flat_opacity(&self) -> f32 {
        let coverage = self.grain.map_or(1.0, Grain::mean_coverage);
        (self.opacity * coverage).clamp(0.0, 1.0)
    }
}

/// The context that changes what a stroke looks like: the output resolution (for the
/// felt-tip halo) and the paper (for the highlighter).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Pixels per device unit.
    px_per_unit: f32,
    /// The page is dark: the highlighter lightens instead of darkening.
    dark_paper: bool,
}

impl Look {
    /// For a raster image of `px_per_unit` pixels per device unit.
    pub fn raster(px_per_unit: f32, dark_paper: bool) -> Self {
        Self {
            px_per_unit,
            dark_paper,
        }
    }

    /// For vector formats (SVG, PDF).
    pub fn vector(dark_paper: bool) -> Self {
        Self::raster(VECTOR_DPI / UNITS_PER_INCH, dark_paper)
    }

    /// The fills of `stroke`, bottom first; never empty. The last one is always the stroke's
    /// own outline (the felt-tip halo lies under it). An outline can be empty (a stroke
    /// without a usable sample): writers skip those ([`Outline::is_empty`]).
    pub fn fills(&self, stroke: &Stroke) -> Vec<Fill> {
        let c = stroke.color;
        let alpha = f32::from(c.a) / 255.0;
        let rgb = [c.r, c.g, c.b];
        let fill = |outline, opacity: f32, blend, grain| Fill {
            outline,
            rgb,
            opacity: opacity.clamp(0.0, 1.0),
            blend,
            grain,
            seed: stroke_seed(stroke),
        };
        match stroke.tool {
            Tool::Ballpoint | Tool::FountainPen | Tool::Brush => {
                vec![fill(outline(stroke), alpha, Blend::Normal, None)]
            }
            Tool::FeltTip => {
                let halo = Stroke {
                    width: stroke.width * FELT_HALO_WIDTH + FELT_HALO_PX / self.px_per_unit,
                    ..stroke.clone()
                };
                vec![
                    fill(outline(&halo), alpha * FELT_HALO, Blend::Normal, None),
                    fill(outline(stroke), alpha * FELT_CORE, Blend::Normal, None),
                ]
            }
            Tool::Highlighter => {
                let blend = if self.dark_paper {
                    Blend::Screen
                } else {
                    Blend::Multiply
                };
                vec![fill(outline(stroke), alpha, blend, None)]
            }
            Tool::Pencil => {
                let pressure = mean_pressure(stroke);
                let opacity = alpha * (PENCIL_LIGHT + (PENCIL_FULL - PENCIL_LIGHT) * pressure);
                vec![fill(
                    outline(stroke),
                    opacity,
                    Blend::Normal,
                    Some(Grain::Pencil),
                )]
            }
            Tool::Chalk => vec![fill(
                outline(stroke),
                alpha * CHALK_OPACITY,
                Blend::Normal,
                Some(Grain::Chalk),
            )],
        }
    }
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

/// A seed that depends on the stroke's content only (first point and length).
fn stroke_seed(stroke: &Stroke) -> u32 {
    let first = stroke.points.first();
    let x = first.map_or(0, |p| p.x.to_bits());
    let y = first.map_or(0, |p| p.y.to_bits());
    let t = first.map_or(0, |p| p.t);
    hash(x ^ hash(y ^ hash(t ^ hash(stroke.points.len() as u32))))
}

/// Coverage (0..1) of a grain at tile pixel `(x, y)`. Seamless over the tile.
pub(crate) fn grain_coverage(grain: Grain, x: i32, y: i32) -> f32 {
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

/// The tile's alpha at `(x, y)`, quantised as the raster tile stores it.
pub(crate) fn grain_alpha(grain: Grain, x: i32, y: i32) -> u8 {
    (grain_coverage(grain, x, y) * 255.0).round() as u8
}

/// Mean of the quantised tile alpha, 0..1.
fn tile_mean(grain: Grain) -> f32 {
    let n = GRAIN_TILE as i32;
    let sum: u64 = (0..n)
        .flat_map(|y| (0..n).map(move |x| u64::from(grain_alpha(grain, x, y))))
        .sum();
    (sum as f64 / (f64::from(n * n) * 255.0)) as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_ink::{Point, Rgba};

    fn stroke(tool: Tool, color: Rgba) -> Stroke {
        Stroke {
            tool,
            color,
            width: 80.0,
            layer: 0,
            points: (0..10)
                .map(|i| Point {
                    x: 100.0 * i as f32,
                    y: 500.0,
                    pressure: 0.5,
                    t: 5 * i,
                })
                .collect(),
        }
    }

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
            assert!((grain.mean_coverage() - mean).abs() < 0.01, "{grain:?}");
        }
    }

    #[test]
    fn stroke_seed_depends_on_content() {
        let at = |x: f32| Stroke {
            points: vec![Point {
                x,
                y: 0.0,
                pressure: 0.5,
                t: 0,
            }],
            ..stroke(Tool::Pencil, Rgba::BLACK)
        };
        assert_eq!(stroke_seed(&at(1.0)), stroke_seed(&at(1.0)));
        assert_ne!(stroke_seed(&at(1.0)), stroke_seed(&at(2.0)));
        assert_eq!(mean_pressure(&at(1.0)), 0.5);
    }

    #[test]
    fn every_tool_ends_with_its_own_outline() {
        let look = Look::vector(false);
        for tool in Tool::ALL {
            let s = stroke(tool, Rgba::rgb(10, 20, 30));
            let fills = look.fills(&s);
            let last = fills.last().unwrap();
            assert_eq!(last.outline, outline(&s), "{tool:?}");
            assert_eq!(last.rgb, [10, 20, 30]);
            assert_eq!(fills.len(), if tool == Tool::FeltTip { 2 } else { 1 });
            for fill in &fills {
                assert!((0.0..=1.0).contains(&fill.opacity));
                assert!((0.0..=1.0).contains(&fill.flat_opacity()));
            }
        }
    }

    #[test]
    fn opacities_blends_and_grains() {
        let look = Look::raster(0.1, false);
        let half = Rgba {
            a: 128,
            ..Rgba::BLACK
        };
        let felt = look.fills(&stroke(Tool::FeltTip, half));
        let a = 128.0 / 255.0;
        assert!((felt[0].opacity - a * FELT_HALO).abs() < 1e-6);
        assert!((felt[1].opacity - a * FELT_CORE).abs() < 1e-6);
        // The halo is wider than the core: 6 % and one pixel (10 units at 0.1 px/unit).
        let width = |fill: &Fill| {
            let b = fill.outline.bounds().unwrap();
            b.max_y - b.min_y
        };
        assert!((width(&felt[0]) - (width(&felt[1]) * 1.06 + 10.0)).abs() < 2.0);

        let marker = |dark| Look::raster(0.1, dark).fills(&stroke(Tool::Highlighter, half));
        assert_eq!(marker(false)[0].blend, Blend::Multiply);
        assert_eq!(marker(true)[0].blend, Blend::Screen);

        let pencil = look.fills(&stroke(Tool::Pencil, Rgba::BLACK));
        assert_eq!(pencil[0].grain, Some(Grain::Pencil));
        // Pressure 0.5: halfway between the lightest and the full opacity.
        assert!((pencil[0].opacity - 0.775).abs() < 1e-6);
        let flat = pencil[0].flat_opacity();
        assert!((flat - 0.775 * Grain::Pencil.mean_coverage()).abs() < 1e-6);

        let chalk = look.fills(&stroke(Tool::Chalk, Rgba::WHITE));
        assert_eq!(chalk[0].grain, Some(Grain::Chalk));
        assert!(chalk[0].flat_opacity() < CHALK_OPACITY);
    }
}
