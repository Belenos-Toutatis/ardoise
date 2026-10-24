//! What the vector formats (SVG, PDF) draw for a note, computed once for both: the page
//! background as shapes and the visible strokes' fills, grouped by layer.

use ardoise_core::note::Note;

use crate::background::{VectorBackground, vector_background};
use crate::look::{Fill, Look};
use crate::visible_strokes;

/// The strokes of one layer, in drawing order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayerFills {
    /// Index of the layer in the note.
    pub index: u32,
    /// Its name; `None` for strokes whose layer has no entry (drawn on top).
    pub name: Option<String>,
    /// Non-empty fills, bottom first.
    pub fills: Vec<Fill>,
}

/// A note ready for a vector writer.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Scene {
    /// Page size in device units.
    pub width: f32,
    pub height: f32,
    /// `None` when the background is off (transparent page).
    pub background: Option<VectorBackground>,
    /// Visible layers holding strokes, bottom first.
    pub layers: Vec<LayerFills>,
}

impl Scene {
    pub(crate) fn of(note: &Note, background: bool) -> Scene {
        let background = background.then(|| vector_background(&note.background, note.page));
        let look = Look::vector(background.as_ref().is_some_and(|b| b.dark));
        let mut layers: Vec<LayerFills> = Vec::new();
        for stroke in visible_strokes(note) {
            let fills = look
                .fills(stroke)
                .into_iter()
                .filter(|f| !f.outline.is_empty());
            match layers.last_mut() {
                Some(layer) if layer.index == stroke.layer => layer.fills.extend(fills),
                _ => layers.push(LayerFills {
                    index: stroke.layer,
                    name: note
                        .layers
                        .get(stroke.layer as usize)
                        .map(|l| l.name.clone()),
                    fills: fills.collect(),
                }),
            }
        }
        layers.retain(|layer| !layer.fills.is_empty());
        Scene {
            width: note.page.width as f32,
            height: note.page.height as f32,
            background,
            layers,
        }
    }

    /// Every fill, in drawing order.
    pub(crate) fn fills(&self) -> impl Iterator<Item = &Fill> {
        self.layers.iter().flat_map(|layer| &layer.fills)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look::Blend;
    use ardoise_core::note::{Background, Layer, Origin, PageSize, Paper, Template, Timing};
    use ardoise_ink::{Point, Rgba, Stroke, Tool};

    fn stroke(tool: Tool, layer: u32, points: usize) -> Stroke {
        Stroke {
            tool,
            color: Rgba::BLACK,
            width: 50.0,
            layer,
            points: (0..points)
                .map(|i| Point {
                    x: 100.0 + 50.0 * i as f32,
                    y: 300.0,
                    pressure: 1.0,
                    t: i as u32,
                })
                .collect(),
        }
    }

    fn note(paper: Paper, strokes: Vec<Stroke>) -> Note {
        Note {
            id: "id".into(),
            title: "t".into(),
            created: 0,
            modified: 0,
            page: PageSize {
                width: 2000,
                height: 1000,
            },
            background: Background {
                paper,
                template: Template::Grid5mm,
                texture: false,
            },
            tags: vec![],
            favorite: false,
            layers: vec![
                Layer {
                    name: "A".into(),
                    visible: true,
                },
                Layer {
                    name: "B".into(),
                    visible: false,
                },
                Layer {
                    name: "C".into(),
                    visible: true,
                },
            ],
            strokes,
            timing: Timing::Recorded,
            origin: Origin::Ardoise,
        }
    }

    #[test]
    fn layers_are_grouped_bottom_first_without_hidden_or_empty_strokes() {
        let n = note(
            Paper::White,
            vec![
                stroke(Tool::Ballpoint, 2, 5),
                stroke(Tool::FeltTip, 0, 5),
                stroke(Tool::Ballpoint, 1, 5),
                stroke(Tool::Pencil, 9, 5),
                stroke(Tool::Ballpoint, 0, 0),
                stroke(Tool::Highlighter, 2, 5),
            ],
        );
        let scene = Scene::of(&n, true);
        let summary: Vec<(u32, Option<&str>, usize)> = scene
            .layers
            .iter()
            .map(|l| (l.index, l.name.as_deref(), l.fills.len()))
            .collect();
        assert_eq!(
            summary,
            [(0, Some("A"), 2), (2, Some("C"), 2), (9, None, 1)]
        );
        assert_eq!(scene.fills().count(), 5);
        assert!(
            scene
                .background
                .as_ref()
                .is_some_and(|b| !b.marks.is_empty())
        );
        assert!(Scene::of(&n, false).background.is_none());
    }

    #[test]
    fn dark_paper_turns_the_highlighter_to_screen() {
        let marker = |paper, background| {
            let n = note(paper, vec![stroke(Tool::Highlighter, 0, 5)]);
            Scene::of(&n, background).layers[0].fills[0].blend
        };
        assert_eq!(marker(Paper::White, true), Blend::Multiply);
        assert_eq!(marker(Paper::Slate, true), Blend::Screen);
        // Without the paper, the page is transparent (white once flattened).
        assert_eq!(marker(Paper::Slate, false), Blend::Multiply);
    }
}
