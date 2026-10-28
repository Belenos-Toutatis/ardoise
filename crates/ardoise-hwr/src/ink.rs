//! Which strokes of a note are recognised, and the scale handed to the engine.
//!
//! The engine works in device-independent pixels (DIPs, 1/96 inch) as a pen on a screen gives
//! them. Notes are in device units (10 µm). [`DIPS_PER_UNIT`] gives the physical size; the
//! strokes are then enlarged by [`RECOGNITION_SCALE`], chosen by measuring the recognition rate
//! of the reference notes on Windows (see the `reference` test): handwriting on paper is
//! smaller than handwriting on a screen, which the engine is tuned for.

use ardoise_core::note::Note;
use ardoise_ink::{Stroke, Tool};

use crate::model::Bounds;

/// DIPs per device unit at the physical size: 96 DIPs per inch, 2540 device units per inch.
pub const DIPS_PER_UNIT: f32 = 96.0 / 2540.0;

/// How much the strokes are enlarged beyond their physical size for the engine.
pub const RECOGNITION_SCALE: f32 = 2.0;

/// The positions in `note.strokes` of the strokes that are handwriting candidates: on a visible
/// layer (hidden layers are not shown, so they are not searched either), not highlighter marks
/// (wide translucent marks over the text, never letters), with at least one point.
pub fn recognizable_indices(note: &Note) -> Vec<usize> {
    note.strokes
        .iter()
        .enumerate()
        .filter(|(_, stroke)| {
            note.layers
                .get(stroke.layer as usize)
                .is_some_and(|layer| layer.visible)
                && stroke.tool != Tool::Highlighter
                && !stroke.points.is_empty()
        })
        .map(|(index, _)| index)
        .collect()
}

/// The strokes of `note` that are handwriting candidates ([`recognizable_indices`]).
pub fn recognizable_strokes(note: &Note) -> Vec<Stroke> {
    recognizable_indices(note)
        .into_iter()
        .map(|index| note.strokes[index].clone())
        .collect()
}

/// Scale from page units to engine coordinates, and back.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    /// Engine coordinates per page unit.
    pub factor: f32,
}

impl Default for Scale {
    fn default() -> Self {
        Scale::enlarged(RECOGNITION_SCALE)
    }
}

impl Scale {
    /// The physical size enlarged `enlargement` times.
    pub fn enlarged(enlargement: f32) -> Scale {
        Scale {
            factor: DIPS_PER_UNIT * enlargement,
        }
    }

    /// A point in page units → engine coordinates.
    pub fn to_engine(&self, x: f32, y: f32) -> (f32, f32) {
        (x * self.factor, y * self.factor)
    }

    /// A rectangle in engine coordinates → page units.
    pub fn to_page(&self, x: f32, y: f32, width: f32, height: f32) -> Bounds {
        let f = if self.factor > 0.0 { self.factor } else { 1.0 };
        Bounds {
            x: x / f,
            y: y / f,
            width: width / f,
            height: height / f,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Origin, Timing};
    use ardoise_ink::{Point, Rgba};

    fn stroke(tool: Tool, layer: u32, points: usize) -> Stroke {
        Stroke {
            tool,
            color: Rgba::BLACK,
            width: 40.0,
            layer,
            points: (0..points)
                .map(|i| Point {
                    x: i as f32,
                    y: 0.0,
                    pressure: 0.5,
                    t: i as u32,
                })
                .collect(),
        }
    }

    #[test]
    fn only_visible_handwriting_is_recognised() {
        let note = Note {
            id: "n".into(),
            title: String::new(),
            created: 0,
            modified: 0,
            page: A4_LANDSCAPE,
            background: Background::default(),
            background_image: None,
            tags: Vec::new(),
            favorite: false,
            layers: vec![
                Layer {
                    name: "1".into(),
                    visible: true,
                },
                Layer {
                    name: "2".into(),
                    visible: false,
                },
            ],
            strokes: vec![
                stroke(Tool::Ballpoint, 0, 3),
                stroke(Tool::Ballpoint, 1, 3),
                stroke(Tool::Highlighter, 0, 3),
                stroke(Tool::Pencil, 0, 0),
                stroke(Tool::Chalk, 0, 1),
                // A layer that does not exist is not drawn either.
                stroke(Tool::Ballpoint, 7, 3),
            ],
            timing: Timing::Recorded,
            origin: Origin::Tablet,
        };
        assert_eq!(recognizable_indices(&note), [0, 4]);
        let kept = recognizable_strokes(&note);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].tool, Tool::Ballpoint);
        assert_eq!(kept[1].tool, Tool::Chalk);
    }

    #[test]
    fn the_scale_goes_both_ways() {
        let physical = Scale::enlarged(1.0);
        // One inch is 96 DIPs.
        let (x, y) = physical.to_engine(2540.0, 1270.0);
        assert!((x - 96.0).abs() < 1e-3 && (y - 48.0).abs() < 1e-3);
        let scale = Scale::default();
        let (x, y) = scale.to_engine(1000.0, 500.0);
        let back = scale.to_page(x, y, x, y);
        assert!((back.x - 1000.0).abs() < 0.01 && (back.height - 500.0).abs() < 0.01);
    }
}
