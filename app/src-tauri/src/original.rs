//! "Back to the original" (SPEC §4.4, docs/design/extras.md): the ink of a note as it was before
//! it was edited, which the editor can restore.
//!
//! - A tablet note keeps the tablet's bytes (`original.bin`, never modified): its original is
//!   decoded again with [`note_from_page`], exactly as the synchronisation decoded it, with this
//!   PC's tablet size and orientation (the registered tablet's drawing area, else the Bamboo Slate's;
//!   the orientation of the settings). A page of the Xiaomi blackboard is recognised from its bytes
//!   ([`page_kind`]) and comes back on the blackboard's own page, turned for the position the note
//!   records it was made in ([`board_position_of`]: upright for a note made before the positions),
//!   whatever the position set now.
//! - Any other note (live mode, Inkspace import, made in Ardoise) gets `original.ardoise` when
//!   the editor first saves a change of its ink over it ([`keep_original`], called by
//!   `notes::save_in`), or when live mode first writes new ink on it ([`keep_resumed_original`],
//!   called by `live::hub`): the version on disk, note and strokes only (its page objects are in
//!   the note). It is written once and never changed afterwards. "The ink" is everything drawn on
//!   the page: the page size, the layers, the strokes and the page objects (text frames, format
//!   1.4): moving or retyping a text frame is a change of the ink, and a page holding only text
//!   frames has something to go back to.
//!
//! Only the page size, the layers, the strokes and the page objects come back (a tablet's page has
//! no page object: going back to it removes the text frames added since); the title, tags,
//! favourite, paper and narration stay as they are now. The first version also gives the
//! placement of its background image (the `backgroundImage` of the `note.json` in
//! `original.ardoise`, whose pixels are not in it): the editor puts the current image back where
//! it was when it is the same image (`lib/editor/original.ts`). The tablet's pages have no image.
//! It also gives its paper, for one case: a sheet whose slate comes from being written on under
//! live mode's blackboard gets back the paper it had (same file).
//!
//! The tablet's bytes are decoded with today's settings (the orientation, the registered tablet's
//! area): a note synced with the other orientation, or from another tablet model, comes back in the
//! geometry of today's settings (`docs/design/extras.md`).

use std::path::Path;

use ardoise_core::format::{FormatError, NoteFile, read_from_bytes, to_bytes};
use ardoise_core::library::Library;
use ardoise_core::note::{BackgroundImage, Layer, Note, PageObject, PageSize, Paper};
use ardoise_core::settings::{Orientation, Settings};
use ardoise_device::identity;
use ardoise_device::pages::{board_position_of, note_from_page, page_kind};
use ardoise_device::session::DownloadedPage;
use ardoise_ink::Stroke;
use serde::Serialize;
use tauri::State;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::library::{first_layer_name, library_error, library_of, now_ms};
use crate::tablet::interface_language;

/// The ink a note can go back to (`NoteOriginal` in `lib/original.ts`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "source", rename_all = "camelCase")]
pub enum NoteOriginal {
    /// Nothing to go back to: the note was never edited, and does not come from the tablet.
    None,
    /// As synced from the tablet (`original.bin`).
    Tablet {
        page: PageSize,
        layers: Vec<Layer>,
        strokes: Vec<Stroke>,
        /// Always empty (a tablet's page has none), given all the same: the note's page objects
        /// are replaced by these, as its strokes are.
        objects: Vec<PageObject>,
    },
    /// As it was before its first edit in Ardoise (`original.ardoise`), with the placement of
    /// the background image it had then (its pixels are the note file's, if it still has them).
    FirstVersion {
        page: PageSize,
        layers: Vec<Layer>,
        strokes: Vec<Stroke>,
        /// Its page objects (text frames), as `note.json` writes them; empty when it had none.
        objects: Vec<PageObject>,
        /// The paper it had.
        paper: Paper,
        #[serde(rename = "backgroundImage", skip_serializing_if = "Option::is_none")]
        background_image: Option<BackgroundImage>,
    },
}

/// Before `edited` is saved over `file` (the version on disk): keeps that version as
/// `original.ardoise` if the file has no original yet and the ink changes. Nothing is kept for a
/// tablet note (it has `original.bin`), for an empty page (a note just created, or the blank
/// file a "save as a copy" fills: nothing drawn, neither a stroke with a sample nor a page
/// object, so nothing to go back to), or when only the title, tags, background or narration
/// change (the original is taken at the first change of the ink, so it holds the ink before any
/// edit). A page that holds only text frames has something to go back to.
pub fn keep_original(file: &mut NoteFile, edited: &Note) -> Result<(), FormatError> {
    if !file.note.has_content() {
        return Ok(());
    }
    keep_first_version(file, edited)
}

/// [`keep_original`] for live mode writing on a note it resumed: `file` is the note as it was read,
/// and there a blank page is something to go back to. It is a worksheet (a page of a PDF, an image,
/// a template) that a teacher takes up to write on, and the sheet before the lesson is what must
/// stay recoverable (`docs/questions.md` Q9). Nothing is kept for what leaves the ink as it was.
pub fn keep_resumed_original(file: &mut NoteFile, written: &Note) -> Result<(), FormatError> {
    keep_first_version(file, written)
}

fn keep_first_version(file: &mut NoteFile, edited: &Note) -> Result<(), FormatError> {
    if file.original.is_some() || file.original_note.is_some() || same_ink(&file.note, edited) {
        return Ok(());
    }
    // Bookmarks are marks on the page, not ink: the original does not hold them (and going back to it
    // keeps the ones the note has now, `lib/editor/original.ts`).
    let mut first = file.note.clone();
    first.objects.retain(|object| !object.is_bookmark());
    file.original_note = Some(to_bytes(&NoteFile::new(first))?);
    Ok(())
}

/// Whether `a` and `b` draw the same page: page size, layers, strokes and page objects, bookmarks
/// left out (adding, renaming or moving one is not a change of the ink). The objects are compared
/// with their boxes as this version measures them, as the file stores them (the editor may send a
/// box measured otherwise for a frame it did not change).
fn same_ink(a: &Note, b: &Note) -> bool {
    let drawn = |note: &'_ Note| -> Vec<PageObject> {
        note.objects
            .iter()
            .filter(|object| !object.is_bookmark())
            .map(measured)
            .collect()
    };
    a.page == b.page && a.layers == b.layers && a.strokes == b.strokes && drawn(a) == drawn(b)
}

/// `object` with its box as this version measures it (`PageObject::measure`).
fn measured(object: &PageObject) -> PageObject {
    let mut object = object.clone();
    object.measure();
    object
}

/// How this PC turns the tablet's pages into notes, as the synchronisation does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabletContext {
    pub size: PageSize,
    pub orientation: Orientation,
    pub layer_name: &'static str,
}

impl TabletContext {
    /// The tablet registered in the library at `root` (its drawing area; the Bamboo Slate's
    /// when none is registered), the orientation and the language of `settings`.
    pub fn of(root: &Path, settings: &Settings, system_locale: Option<&str>) -> Self {
        let size = match identity::load(root) {
            Ok(Some(record)) if record.width > 0 && record.height > 0 => PageSize {
                width: record.width,
                height: record.height,
            },
            _ => PageSize {
                width: ardoise_proto::SLATE_LARGE_WIDTH,
                height: ardoise_proto::SLATE_LARGE_HEIGHT,
            },
        };
        TabletContext {
            size,
            orientation: settings.tablet.orientation,
            layer_name: first_layer_name(interface_language(settings, system_locale)),
        }
    }
}

/// The original of a note file.
pub fn original_of(file: &NoteFile, tablet: &TabletContext) -> CommandResult<NoteOriginal> {
    let damaged = |error: &dyn std::fmt::Display| CommandError::new("originalDamaged", error);
    if let Some(bytes) = &file.original {
        let page = DownloadedPage {
            bytes: bytes.clone(),
            timestamp: None,
            announced_size: u32::try_from(bytes.len()).unwrap_or(u32::MAX),
            kind: page_kind(bytes),
            previous: None,
        };
        // A page of the blackboard comes back the way up it was made in (its note says how the
        // blackboard was held; a position this version does not know: upright, the tablet's
        // own frame).
        let board_position = board_position_of(file).unwrap_or_default();
        let decoded = note_from_page(
            &page,
            tablet.size,
            tablet.orientation,
            board_position,
            file.note.id.clone(),
            tablet.layer_name,
            now_ms(),
        )
        .map_err(|e| damaged(&e))?;
        // As the synced note was stored: positions and pressures as the format keeps them.
        let stored = to_bytes(&NoteFile::new(decoded.note))
            .and_then(|bytes| read_from_bytes(&bytes))
            .map_err(|e| damaged(&e))?;
        let note = stored.note;
        return Ok(NoteOriginal::Tablet {
            page: note.page,
            layers: note.layers,
            strokes: note.strokes,
            objects: note.objects,
        });
    }
    if let Some(bytes) = &file.original_note {
        let note = read_from_bytes(bytes).map_err(|e| damaged(&e))?.note;
        return Ok(NoteOriginal::FirstVersion {
            page: note.page,
            layers: note.layers,
            strokes: note.strokes,
            objects: note.objects,
            paper: note.background.paper,
            background_image: note.background_image,
        });
    }
    Ok(NoteOriginal::None)
}

/// The original of the note at `path`, which must be a note of the library.
pub fn original_in(
    library: &Library,
    path: &Path,
    tablet: &TabletContext,
) -> CommandResult<NoteOriginal> {
    let loaded = library.read(path).map_err(library_error)?;
    original_of(&loaded.file, tablet)
}

#[tauri::command]
pub async fn note_original(
    context: State<'_, AppContext>,
    path: String,
) -> CommandResult<NoteOriginal> {
    let library = library_of(&context)?;
    let store = context.store.clone().ok();
    let locale = context.system_locale.clone();
    // Reading the settings and the note, decoding the tablet's bytes: off the async runtime's
    // threads.
    crate::notes::blocking(move || {
        let settings = store
            .and_then(|store| store.load().ok())
            .unwrap_or_default();
        let tablet = TabletContext::of(library.root(), &settings, locale.as_deref());
        original_in(&library, Path::new(&path), &tablet)
    })
    .await
}

#[cfg(test)]
mod tests;
