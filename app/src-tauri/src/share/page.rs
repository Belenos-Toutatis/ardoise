//! The note open in the editor, for the share (docs/design/lan-share.md, "Note of the editor"):
//! the page as the viewer draws it, sent whole or as the strokes added since the last time, and
//! its paper rendered only when it changed.

use ardoise_core::note::{Background, BackgroundImage, Layer, Note, PageSize, Template};
use ardoise_ink::Stroke;
use ardoise_share::NoteUpdate;
use serde::Serialize;

/// Longer side of the paper sent to the viewers, pixels (a 4K screen showing the whole page
/// needs about this much).
pub const PAPER_PX: u32 = 2560;

/// Whether the viewer draws `stroke`: its layer is not hidden (a stroke of a layer that does not
/// exist is drawn, as `drawingOrder` in `live-render.ts` does). Strokes of a hidden layer never
/// leave the PC: anyone with the code could read them in the stream, and a teacher hides an
/// "answers" layer precisely to keep it to themselves.
pub fn is_drawn(layers: &[Layer], stroke: &Stroke) -> bool {
    usize::try_from(stroke.layer)
        .ok()
        .and_then(|index| layers.get(index))
        .is_none_or(|layer| layer.visible)
}

/// What the viewer draws of a note (`SharedPage` in `share.ts`): no title, tags, file, dates,
/// layer names or stroke of a hidden layer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SharedPage {
    pub id: String,
    pub page: PageSize,
    pub background: Background,
    pub layers: Vec<Layer>,
    pub strokes: Vec<Stroke>,
}

impl SharedPage {
    pub fn of(note: Note) -> SharedPage {
        // Strokes refer to their layer by index: the layer list stays whole.
        let mut strokes = note.strokes;
        strokes.retain(|stroke| is_drawn(&note.layers, stroke));
        SharedPage {
            id: note.id,
            page: note.page,
            background: note.background,
            // Their names are typed by the user; the viewer only needs whether they show.
            layers: note
                .layers
                .into_iter()
                .map(|layer| Layer {
                    name: String::new(),
                    ..layer
                })
                .collect(),
            strokes,
        }
    }

    /// Same sheet: only strokes may differ.
    fn same_sheet(&self, other: &SharedPage) -> bool {
        self.id == other.id
            && self.page == other.page
            && self.background == other.background
            && self.layers == other.layers
    }
}

/// What the paper's picture depends on.
#[derive(Debug, Clone, PartialEq)]
pub struct Paper {
    pub page: PageSize,
    pub background: Background,
    /// The background image and the key of its staged pixels.
    pub image: Option<(BackgroundImage, String)>,
}

impl Paper {
    /// Plain paper: the viewer fills the page with the paper colour, nothing to render.
    pub fn is_plain(&self) -> bool {
        self.background.template == Template::Blank
            && !self.background.texture
            && self.image.is_none()
    }

    /// Width in pixels at which to render it ([`PAPER_PX`] on the longer side).
    pub fn width_px(&self) -> u32 {
        let PageSize { width, height } = self.page;
        if width >= height || height == 0 {
            PAPER_PX
        } else {
            let scaled = u64::from(PAPER_PX) * u64::from(width) / u64::from(height);
            u32::try_from(scaled).unwrap_or(PAPER_PX).max(1)
        }
    }
}

/// The strokes added at the end of the page last sent.
#[derive(Serialize)]
#[serde(tag = "type", rename = "noteStrokes")]
struct NoteStrokes<'a> {
    strokes: &'a [Stroke],
}

/// The note last shared, its paper and the paper's picture.
#[derive(Default)]
pub struct NoteSharer {
    last: Option<SharedPage>,
    /// The paper last seen and its picture as JSON (`"data:image/png;base64,…"` or `null`),
    /// rendered and encoded once.
    paper: Option<(Paper, String)>,
}

impl NoteSharer {
    /// Forgets what the viewers show (a new share, or they left the note): the next note goes
    /// whole. The paper's picture is kept: it does not need rendering again.
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// The messages for `note`: `None` when the viewers already show exactly this. `render`
    /// makes the paper's picture (a PNG data URL) when the paper is not plain and changed;
    /// `None` from it leaves the paper colour.
    ///
    /// `full` is the whole page with its paper (for a viewer that joins, or that showed something
    /// else). For the viewers already showing the previous note, `append` is the strokes added
    /// when that is all that changed, else the whole page *without* its paper when the paper did
    /// not change (an erased stroke, an undo, a colour: the picture, which may weigh megabytes,
    /// is not sent again), else nothing (`full` goes).
    pub fn update(
        &mut self,
        note: Note,
        image_key: Option<String>,
        render: impl FnOnce(&Paper) -> Option<String>,
    ) -> Option<NoteUpdate> {
        let paper = Paper {
            page: note.page,
            background: note.background,
            image: note.background_image.clone().zip(image_key),
        };
        let page = SharedPage::of(note);
        let paper_changed = self.paper.as_ref().is_none_or(|(known, _)| *known != paper);
        if paper_changed {
            let picture = if paper.is_plain() {
                None
            } else {
                render(&paper)
            };
            let json = serde_json::to_string(&picture).ok()?;
            self.paper = Some((paper, json));
        }
        let picture = self
            .paper
            .as_ref()
            .map_or("null", |(_, json)| json.as_str());

        let previous = self.last.as_ref().filter(|_| !paper_changed);
        let added = previous.and_then(|last| {
            let known = last.strokes.len();
            (last.same_sheet(&page)
                && page.strokes.len() >= known
                && page.strokes[..known] == last.strokes[..])
                .then_some(known)
        });
        if added == Some(page.strokes.len()) {
            return None;
        }
        let page_json = serde_json::to_string(&page).ok()?;
        let full = format!(r#"{{"type":"note","page":{page_json},"background":{picture}}}"#);
        let append = match added {
            Some(known) => Some(
                serde_json::to_string(&NoteStrokes {
                    strokes: &page.strokes[known..],
                })
                .ok()?,
            ),
            None if previous.is_some() => Some(format!(r#"{{"type":"note","page":{page_json}}}"#)),
            None => None,
        };
        self.last = Some(page);
        Some(NoteUpdate { full, append })
    }
}
