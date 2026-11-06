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

/// A note's times are an order of writing while at most one stroke in this many overlaps the next
/// in time. One pen writes one stroke at a time: real notes have a few (2.4 % at most in a library
/// of 28), while strokes with a clock each overlap all the time.
const STROKES_PER_OVERLAP: usize = 20;

/// The positions in `note.strokes` of the strokes that are handwriting candidates: on a visible
/// layer (hidden layers are not shown, so they are not searched either), not highlighter marks
/// (wide translucent marks over the text, never letters), with at least one point.
///
/// They come in the order they were written, which is not always the file's: the engine reads
/// strokes in the order it is given them, and a retouched copy keeps a letter put back in its word
/// at the end of the file, with the right time. Strokes of the same time stay in the file's order,
/// and so does the whole note when its times are no order of writing ([`STROKES_PER_OVERLAP`]):
/// sorted, its strokes would be shuffled and the engine would read nothing.
pub fn recognizable_indices(note: &Note) -> Vec<usize> {
    let listed: Vec<usize> = note
        .strokes
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
        .collect();
    let start = |index: usize| {
        note.strokes[index]
            .points
            .first()
            .map_or(0, |point| point.t)
    };
    let end = |index: usize| note.strokes[index].points.last().map_or(0, |point| point.t);
    let mut written = listed.clone();
    written.sort_by_key(|&index| start(index));
    let overlaps = written
        .windows(2)
        .filter(|pair| start(pair[1]) < end(pair[0]))
        .count();
    if overlaps * STROKES_PER_OVERLAP > listed.len() {
        listed
    } else {
        written
    }
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

    /// A stroke of two points whose first one is at `t`.
    fn at(t: u32) -> Stroke {
        let mut stroke = stroke(Tool::Ballpoint, 0, 2);
        for point in &mut stroke.points {
            point.t += t;
        }
        stroke
    }

    fn note_of(strokes: Vec<Stroke>) -> Note {
        Note {
            id: "n".into(),
            title: String::new(),
            created: 0,
            modified: 0,
            page: A4_LANDSCAPE,
            background: Background::default(),
            background_image: None,
            tags: Vec::new(),
            favorite: false,
            layers: vec![Layer {
                name: "1".into(),
                visible: true,
            }],
            strokes,
            timing: Timing::Recorded,
            origin: Origin::Tablet,
        }
    }

    #[test]
    fn strokes_are_read_in_the_order_they_were_written() {
        // As a retouched copy has it: the letter put back in the word is last in the file.
        let note = note_of(vec![at(0), at(1000), at(2000), at(500)]);
        assert_eq!(recognizable_indices(&note), [0, 3, 1, 2]);
        let kept = recognizable_strokes(&note);
        let starts: Vec<u32> = kept.iter().map(|s| s.points[0].t).collect();
        assert_eq!(starts, [0, 500, 1000, 2000]);
    }

    #[test]
    fn a_note_written_in_order_keeps_its_order_and_strokes_of_one_time_stay_as_they_are() {
        // So its strokes hash, and what the cache holds for it, do not change.
        let note = note_of(vec![at(0), at(1000), at(2000)]);
        assert_eq!(recognizable_indices(&note), [0, 1, 2]);
        let same = note_of(vec![at(7), at(7), at(7)]);
        assert_eq!(recognizable_indices(&same), [0, 1, 2]);
    }

    #[test]
    fn strokes_with_a_clock_each_stay_in_the_file() {
        // Each line restarts at 0, as a page put together from pieces: sorted, it would be shuffled.
        let note = note_of(vec![at(0), at(10), at(20), at(0), at(10), at(20)]);
        assert_eq!(recognizable_indices(&note), [0, 1, 2, 3, 4, 5]);
    }

    #[test]
    fn a_rare_overlap_does_not_stop_a_long_note_being_read_in_order() {
        // 40 strokes, one overlapping the next (real notes have some), one put back at the end.
        let mut strokes: Vec<Stroke> = (0..39).map(|i| at(1000 * i)).collect();
        strokes[5].points[1].t += 5000;
        strokes.push(at(10_500));
        let note = note_of(strokes);
        let read = recognizable_indices(&note);
        assert_eq!(read[10..13], [10, 39, 11]);
        assert_eq!(read[..3], [0, 1, 2]);
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
