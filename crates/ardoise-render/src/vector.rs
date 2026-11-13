//! What the vector formats (SVG, PDF) draw for a note, computed once for both: the page
//! background as shapes (with the background image, embedded as a raster), and the visible
//! strokes' fills and text frames, grouped by layer ([`crate::draw_items`]).

use crate::background::{VectorBackground, vector_background};
use crate::look::{Fill, Look};
use crate::picture::{NoteView, PlacedPicture};
use crate::text::TextFill;
use crate::{DrawItem, draw_items};

/// What one layer draws: its strokes' fills, then its text frames.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LayerFills {
    /// Index of the layer in the note.
    pub index: u32,
    /// Its name; `None` for strokes and objects whose layer has no entry (drawn on top).
    pub name: Option<String>,
    /// Non-empty fills of the layer's strokes, bottom first.
    pub fills: Vec<Fill>,
    /// The layer's text frames, in note order, drawn over its strokes.
    pub texts: Vec<TextFill>,
}

impl LayerFills {
    fn is_empty(&self) -> bool {
        self.fills.is_empty() && self.texts.is_empty()
    }
}

/// A note ready for a vector writer.
#[derive(Debug, Clone)]
pub(crate) struct Scene<'a> {
    /// Page size in device units.
    pub width: f32,
    pub height: f32,
    /// `None` when the background is off (transparent page).
    pub background: Option<VectorBackground>,
    /// The background image, over the background and under the strokes; only with the
    /// background.
    pub picture: Option<PlacedPicture<'a>>,
    /// Visible layers holding strokes or text frames, bottom first.
    pub layers: Vec<LayerFills>,
}

impl<'a> Scene<'a> {
    pub(crate) fn of(view: NoteView<'a>, background: bool) -> Scene<'a> {
        let note = view.note;
        let picture = if background { view.placed() } else { None };
        let background = background.then(|| vector_background(&note.background, note.page));
        let look = Look::vector(background.as_ref().is_some_and(|b| b.dark));
        let mut layers: Vec<LayerFills> = Vec::new();
        // Sorted by layer, strokes before objects within one: each layer's items follow each
        // other.
        for item in draw_items(note) {
            let (index, fills, text) = match item {
                DrawItem::Stroke(i) => {
                    let Some(stroke) = note.strokes.get(i) else {
                        continue;
                    };
                    let fills: Vec<Fill> = look
                        .fills(stroke)
                        .into_iter()
                        .filter(|f| !f.outline.is_empty())
                        .collect();
                    (stroke.layer, fills, None)
                }
                DrawItem::Object(i) => {
                    let Some(object) = note.objects.get(i) else {
                        continue;
                    };
                    (object.layer, Vec::new(), TextFill::of(object))
                }
            };
            if layers.last().is_none_or(|layer| layer.index != index) {
                layers.push(LayerFills {
                    index,
                    name: note.layers.get(index as usize).map(|l| l.name.clone()),
                    fills: Vec::new(),
                    texts: Vec::new(),
                });
            }
            if let Some(layer) = layers.last_mut() {
                layer.fills.extend(fills);
                layer.texts.extend(text);
            }
        }
        layers.retain(|layer| !layer.is_empty());
        Scene {
            width: note.page.width as f32,
            height: note.page.height as f32,
            background,
            picture,
            layers,
        }
    }

    /// How many fills and text frames there are.
    pub(crate) fn len(&self) -> usize {
        self.layers
            .iter()
            .map(|layer| layer.fills.len() + layer.texts.len())
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::look::Blend;
    use ardoise_core::note::{Background, Layer, Note, Origin, PageSize, Paper, Template, Timing};
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
            background_image: None,
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
            objects: Vec::new(),
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
        let scene = Scene::of((&n).into(), true);
        let summary: Vec<(u32, Option<&str>, usize)> = scene
            .layers
            .iter()
            .map(|l| (l.index, l.name.as_deref(), l.fills.len()))
            .collect();
        assert_eq!(
            summary,
            [(0, Some("A"), 2), (2, Some("C"), 2), (9, None, 1)]
        );
        assert_eq!(scene.len(), 5);
        assert!(
            scene
                .background
                .as_ref()
                .is_some_and(|b| !b.marks.is_empty())
        );
        assert!(Scene::of((&n).into(), false).background.is_none());
    }

    #[test]
    fn text_frames_go_over_their_layers_strokes_and_a_text_only_layer_has_its_entry() {
        use ardoise_core::note::{ObjectBody, PageObject, TextBody};
        let text = |layer, words: &str| {
            PageObject::text(
                words,
                layer,
                0,
                100.0,
                100.0,
                TextBody {
                    text: words.into(),
                    font: ardoise_text::FontFamily::Andika,
                    bold: false,
                    size: 400.0,
                    color: Rgba::BLACK,
                },
            )
        };
        let mut n = note(
            Paper::White,
            vec![stroke(Tool::Ballpoint, 0, 5), stroke(Tool::FeltTip, 0, 5)],
        );
        let mut unknown = text(0, "x");
        unknown.body = ObjectBody::Unknown {
            kind: "equation".into(),
            fields: Default::default(),
        };
        n.objects = vec![
            text(2, "haut"),
            text(0, "bas"),
            text(1, "caché"),
            unknown,
            text(9, "orphelin"),
            // Blank: nothing to draw, no layer for it.
            text(7, "  "),
            text(0, "bas 2"),
        ];
        let scene = Scene::of((&n).into(), false);
        let summary: Vec<(u32, usize, Vec<&str>)> = scene
            .layers
            .iter()
            .map(|l| {
                let texts = l.texts.iter().map(|t| t.text.as_str()).collect();
                (l.index, l.fills.len(), texts)
            })
            .collect();
        assert_eq!(
            summary,
            [
                (0, 3, vec!["bas", "bas 2"]),
                (2, 0, vec!["haut"]),
                (9, 0, vec!["orphelin"])
            ]
        );
        assert_eq!(scene.layers[1].name.as_deref(), Some("C"));
        assert_eq!(scene.layers[2].name, None);
        assert_eq!(scene.len(), 7);
    }

    #[test]
    fn dark_paper_turns_the_highlighter_to_screen() {
        let marker = |paper, background| {
            let n = note(paper, vec![stroke(Tool::Highlighter, 0, 5)]);
            Scene::of((&n).into(), background).layers[0].fills[0].blend
        };
        assert_eq!(marker(Paper::White, true), Blend::Multiply);
        assert_eq!(marker(Paper::Slate, true), Blend::Screen);
        // Without the paper, the page is transparent (white once flattened).
        assert_eq!(marker(Paper::Slate, false), Blend::Multiply);
    }
}
