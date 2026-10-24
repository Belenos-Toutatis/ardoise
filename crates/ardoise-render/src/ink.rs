//! Raster ink: paints each stroke's fills (from [`crate::look`]) with tiny-skia, anti-aliased.
//! Grain fills use a repeating tile of the grain in the stroke's colour, shifted by the
//! stroke's seed.

use std::collections::HashMap;

use ardoise_ink::Outline;
use tiny_skia::{
    BlendMode, Color, ColorU8, FillRule, FilterQuality, Paint, Path, PathBuilder, Pattern, Pixmap,
    Rect, Shader, SpreadMode, Transform,
};

use crate::look::{Blend, Fill, GRAIN_TILE, Grain, Look, grain_alpha};

/// Size of one grain cell, in millimetres (never smaller than a pixel).
const GRAIN_MM: f32 = 0.18;

/// The fills of a stroke with their paths in pixels (`None` for a fill off the page).
#[derive(Debug, Clone)]
pub(crate) struct Prepared {
    pub(crate) fills: Vec<Fill>,
    pub(crate) paths: Vec<Option<Path>>,
}

impl Prepared {
    /// A stroke whose own outline (the last fill) is off the page is skipped entirely, halo
    /// included.
    pub(crate) fn is_drawn(&self) -> bool {
        matches!(self.paths.last(), Some(Some(_)))
    }

    /// Bounds of what painting it touches, in pixels (`None` when nothing is drawn).
    pub(crate) fn bounds(&self) -> Option<Rect> {
        if !self.is_drawn() {
            return None;
        }
        self.paths
            .iter()
            .flatten()
            .map(Path::bounds)
            .reduce(|a, b| {
                Rect::from_ltrb(
                    a.left().min(b.left()),
                    a.top().min(b.top()),
                    a.right().max(b.right()),
                    a.bottom().max(b.bottom()),
                )
                .unwrap_or(a)
            })
    }
}

/// Draws strokes on one page. Keeps the grain tiles made so far (one per grain and colour).
#[derive(Clone)]
pub(crate) struct InkPainter {
    look: Look,
    /// Pixels per device unit.
    scale: f32,
    width: f32,
    height: f32,
    /// `None` if a tile could not be allocated (the grain then falls back to solid ink).
    tiles: HashMap<(Grain, [u8; 3]), Option<Pixmap>>,
}

impl InkPainter {
    pub(crate) fn new(pixmap: &Pixmap, scale: f32, dark_paper: bool) -> Self {
        Self {
            look: Look::raster(scale, dark_paper),
            scale,
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

    /// The look strokes are drawn with (scale and paper).
    pub(crate) fn look(&self) -> Look {
        self.look
    }

    /// The fills with their paths in pixels, ready to paint (possibly several times).
    pub(crate) fn prepare(&self, fills: Vec<Fill>) -> Prepared {
        let paths = fills.iter().map(|f| self.path(&f.outline)).collect();
        Prepared { fills, paths }
    }

    pub(crate) fn paint(&mut self, pixmap: &mut Pixmap, prepared: &Prepared) {
        if !prepared.is_drawn() {
            return;
        }
        for (fill, path) in prepared.fills.iter().zip(&prepared.paths) {
            let Some(path) = path else {
                continue;
            };
            match fill.grain {
                None => fill_solid(pixmap, path, fill.rgb, fill.opacity, fill.blend),
                Some(grain) => self.fill_grain(pixmap, path, grain, fill),
            }
        }
    }

    /// Fills `path` with a grain tile of the fill's colour, shifted by its seed.
    fn fill_grain(&mut self, pixmap: &mut Pixmap, path: &Path, grain: Grain, fill: &Fill) {
        let cell = (GRAIN_MM * 100.0 * self.scale).max(1.0);
        let seed = fill.seed;
        let (dx, dy) = (
            (seed % GRAIN_TILE) as f32,
            ((seed >> 8) % GRAIN_TILE) as f32,
        );
        let Some(tile) = self
            .tiles
            .entry((grain, fill.rgb))
            .or_insert_with(|| grain_tile(grain, fill.rgb))
            .as_ref()
            .map(Pixmap::as_ref)
        else {
            fill_solid(pixmap, path, fill.rgb, fill.opacity, Blend::Normal);
            return;
        };
        let quality = if cell > 1.0 {
            FilterQuality::Bilinear
        } else {
            FilterQuality::Nearest
        };
        let transform = Transform::from_row(cell, 0.0, 0.0, cell, dx * cell, dy * cell);
        let paint = Paint {
            shader: Pattern::new(tile, SpreadMode::Repeat, quality, fill.opacity, transform),
            ..Paint::default()
        };
        pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
    }
}

fn blend_mode(blend: Blend) -> BlendMode {
    match blend {
        Blend::Normal => BlendMode::SourceOver,
        Blend::Multiply => BlendMode::Multiply,
        Blend::Screen => BlendMode::Screen,
    }
}

fn fill_solid(pixmap: &mut Pixmap, path: &Path, rgb: [u8; 3], alpha: f32, blend: Blend) {
    let mut color = Color::from_rgba8(rgb[0], rgb[1], rgb[2], 255);
    color.set_alpha(alpha.clamp(0.0, 1.0));
    let paint = Paint {
        shader: Shader::SolidColor(color),
        blend_mode: blend_mode(blend),
        ..Paint::default()
    };
    pixmap.fill_path(path, &paint, FillRule::Winding, Transform::identity(), None);
}

/// The repeating grain tile of `grain` in colour `rgb` (premultiplied, alpha = coverage).
fn grain_tile(grain: Grain, rgb: [u8; 3]) -> Option<Pixmap> {
    let mut tile = Pixmap::new(GRAIN_TILE, GRAIN_TILE)?;
    let size = GRAIN_TILE as usize;
    for (i, pixel) in tile.pixels_mut().iter_mut().enumerate() {
        let (x, y) = ((i % size) as i32, (i / size) as i32);
        let a = grain_alpha(grain, x, y);
        *pixel = ColorU8::from_rgba(rgb[0], rgb[1], rgb[2], a).premultiply();
    }
    Some(tile)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grain_tiles_have_the_tile_size() {
        for grain in [Grain::Pencil, Grain::Chalk] {
            let tile = grain_tile(grain, [10, 20, 30]).unwrap();
            assert_eq!((tile.width(), tile.height()), (GRAIN_TILE, GRAIN_TILE));
            let opaque = tile.pixels().iter().filter(|p| p.alpha() == 255).count();
            assert!(opaque > 0, "{grain:?}");
        }
    }
}
