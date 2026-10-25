//! "Back to the original" (SPEC §4.4, docs/design/extras.md): the ink of a note as it was before
//! it was edited, which the editor can restore.
//!
//! - A tablet note keeps the tablet's bytes (`original.bin`, never modified): its original is
//!   decoded again with [`note_from_page`], exactly as the synchronisation decoded it, with this
//!   PC's tablet size and hand (the registered tablet's drawing area, else the Bamboo Slate's;
//!   the hand of the settings).
//! - Any other note (live mode, Inkspace import, made in Ardoise) gets `original.ardoise` when
//!   the editor first saves a change of its ink over it ([`keep_original`], called by
//!   `notes::save_in`): the version on disk, note and strokes only. It is written once and never
//!   changed afterwards.
//!
//! Only the page size, the layers and the strokes come back; the title, tags, favourite,
//! paper and narration stay as they are now. The first version also gives the placement of its
//! background image (the `backgroundImage` of the `note.json` in `original.ardoise`, whose
//! pixels are not in it): the editor puts the current image back where it was when it is the
//! same image (`lib/editor/original.ts`). The tablet's pages have no image.
//!
//! The tablet's bytes are decoded with today's settings (the hand, the registered tablet's
//! area): a note synced with the other hand, or from another tablet model, comes back in the
//! geometry of today's settings (`docs/design/extras.md`).

use std::path::Path;

use ardoise_core::format::{FormatError, NoteFile, read_from_bytes, to_bytes};
use ardoise_core::library::Library;
use ardoise_core::note::{BackgroundImage, Layer, Note, PageSize};
use ardoise_core::settings::{Hand, Settings};
use ardoise_device::identity;
use ardoise_device::pages::note_from_page;
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
    },
    /// As it was before its first edit in Ardoise (`original.ardoise`), with the placement of
    /// the background image it had then (its pixels are the note file's, if it still has them).
    FirstVersion {
        page: PageSize,
        layers: Vec<Layer>,
        strokes: Vec<Stroke>,
        #[serde(rename = "backgroundImage", skip_serializing_if = "Option::is_none")]
        background_image: Option<BackgroundImage>,
    },
}

/// Before `edited` is saved over `file` (the version on disk): keeps that version as
/// `original.ardoise` if the file has no original yet and the ink changes. Nothing is kept for a
/// tablet note (it has `original.bin`), for an empty page (a note just created, or the blank
/// file a "save as a copy" fills: there is nothing to go back to), or when only the title,
/// tags, background or narration change (the original is taken at the first change of the
/// ink, so it holds the ink before any edit).
pub fn keep_original(file: &mut NoteFile, edited: &Note) -> Result<(), FormatError> {
    if file.original.is_some() || file.original_note.is_some() {
        return Ok(());
    }
    let on_disk = &file.note;
    if on_disk.strokes.is_empty() || same_ink(on_disk, edited) {
        return Ok(());
    }
    file.original_note = Some(to_bytes(&NoteFile::new(on_disk.clone()))?);
    Ok(())
}

fn same_ink(a: &Note, b: &Note) -> bool {
    a.page == b.page && a.layers == b.layers && a.strokes == b.strokes
}

/// How this PC turns the tablet's pages into notes, as the synchronisation does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TabletContext {
    pub size: PageSize,
    pub hand: Hand,
    pub layer_name: &'static str,
}

impl TabletContext {
    /// The tablet registered in the library at `root` (its drawing area; the Bamboo Slate's
    /// when none is registered), the hand and the language of `settings`.
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
            hand: settings.tablet.hand,
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
        };
        let decoded = note_from_page(
            &page,
            tablet.size,
            tablet.hand,
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
        });
    }
    if let Some(bytes) = &file.original_note {
        let note = read_from_bytes(bytes).map_err(|e| damaged(&e))?.note;
        return Ok(NoteOriginal::FirstVersion {
            page: note.page,
            layers: note.layers,
            strokes: note.strokes,
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
