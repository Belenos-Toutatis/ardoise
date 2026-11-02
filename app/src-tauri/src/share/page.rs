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

/// What the viewer draws of a note (`SharedPage` in `share.ts`): no title, tags, file or dates.
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
        SharedPage {
            id: note.id,
            page: note.page,
            background: note.background,
            layers: note.layers,
            strokes: note.strokes,
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

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum Message<'a> {
    Note {
        page: &'a SharedPage,
        background: Option<&'a str>,
    },
    NoteStrokes {
        strokes: &'a [Stroke],
    },
}

/// The note last shared, its paper and the paper's picture.
#[derive(Default)]
pub struct NoteSharer {
    last: Option<SharedPage>,
    paper: Option<(Paper, Option<String>)>,
}

impl NoteSharer {
    /// Forgets what was sent (a new share: the next note goes whole).
    pub fn reset(&mut self) {
        self.last = None;
    }

    /// The messages for `note`: `None` when the viewers already show exactly this. `render`
    /// makes the paper's picture (a PNG data URL) when the paper is not plain and changed;
    /// `None` from it leaves the paper colour.
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
            self.paper = Some((paper, picture));
        }
        let picture = self.paper.as_ref().and_then(|(_, p)| p.as_deref());

        let added = match &self.last {
            Some(last) if !paper_changed && last.same_sheet(&page) => {
                let known = last.strokes.len();
                if page.strokes.len() >= known && page.strokes[..known] == last.strokes[..] {
                    Some(known)
                } else {
                    None
                }
            }
            _ => None,
        };
        if added == Some(page.strokes.len()) {
            return None;
        }
        let full = serde_json::to_string(&Message::Note {
            page: &page,
            background: picture,
        })
        .ok()?;
        let append = added.and_then(|known| {
            serde_json::to_string(&Message::NoteStrokes {
                strokes: &page.strokes[known..],
            })
            .ok()
        });
        self.last = Some(page);
        Some(NoteUpdate { full, append })
    }
}
