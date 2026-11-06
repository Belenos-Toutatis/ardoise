//! What the editor needs to say that a note is a revision, a retouched copy made by an assistant
//! (SPEC §4.9, `docs/design/revision.md`): the account that comes with the copy
//! (`revision.json`), where its source is now (found by id: the title and the folder of a note
//! change, its id does not), and the two pictures of "Comparer".

use std::path::Path;

use ardoise_core::format::{RevisionEntry, RevisionInfo};
use ardoise_core::library::Library;
use ardoise_render::{NoteView, RenderError, RenderOptions, encode_png, render_note};
use serde::Serialize;
use tauri::State;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::library::{library_error, library_of};
use crate::thumbnails::png_data_url;

/// Largest side of a picture of "Comparer", in pixels.
pub const MAX_PICTURE_PX: u32 = 2400;
/// Smallest: a dialog measured before it is laid out must not ask for a speck.
pub const MIN_PICTURE_PX: u32 = 120;

/// Where the source of a revision is now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceLink {
    /// The file of the source in the library.
    pub path: String,
    /// Its title now (it may have been renamed since the copy).
    pub title: String,
}

/// `RevisionOfNote` in `note.ts`: the content of `revision.json`, and the source found by id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevisionView {
    /// Id of the source.
    pub of: String,
    /// Its title when the copy was made.
    pub of_title: String,
    /// When the copy was made (Unix milliseconds).
    pub created: i64,
    /// The account, oldest first. Plain text.
    pub summary: Vec<RevisionEntry>,
    /// The source in the library; `None` when it is gone (trashed, deleted, not synced yet).
    pub source: Option<SourceLink>,
}

/// The note of the library with this id. A sync client's conflict copy has the id of the note it
/// copies: the original file is preferred.
pub fn find_source(library: &Library, id: &str) -> Option<SourceLink> {
    let listing = crate::library::list(library);
    let mut found = listing.notes.iter().filter(|note| note.id == id);
    let first = found.next()?;
    let best = if first.conflict.is_none() {
        first
    } else {
        found.find(|note| note.conflict.is_none()).unwrap_or(first)
    };
    Some(SourceLink {
        path: best.path.clone(),
        title: best.title.clone(),
    })
}

/// The revision as the editor gets it.
pub fn view_of(library: &Library, info: &RevisionInfo) -> RevisionView {
    RevisionView {
        of: info.of.clone(),
        of_title: info.of_title.clone(),
        created: info.created,
        summary: info.summary.clone(),
        source: find_source(library, &info.of),
    }
}

/// The source of a revision, looked for again (the editor asks when the user opens it: the note
/// may have moved, come back from the trash or been deleted since the revision was opened).
#[tauri::command]
pub async fn revision_source(
    context: State<'_, AppContext>,
    id: String,
) -> CommandResult<Option<SourceLink>> {
    let library = library_of(&context)?;
    crate::notes::blocking(move || Ok(find_source(&library, &id))).await
}

/// A PNG of the note at `path` as last saved, the whole page, drawn with its paper and
/// background image, as large as fits in `max_width` x `max_height` pixels (each clamped to
/// [`MIN_PICTURE_PX`]..=[`MAX_PICTURE_PX`]).
pub fn picture_in(
    library: &Library,
    path: &Path,
    max_width: u32,
    max_height: u32,
) -> CommandResult<Vec<u8>> {
    let file = library.read(path).map_err(library_error)?.file;
    let page = file.note.page;
    // A page of size zero is refused when a note is read (`format::validate`).
    let clamp = |side: u32| side.clamp(MIN_PICTURE_PX, MAX_PICTURE_PX) as f32;
    let scale = (clamp(max_width) / page.width as f32).min(clamp(max_height) / page.height as f32);
    let picture = crate::background::picture_of(&file.note, file.background.as_ref());
    let render_error = |error: RenderError| match error {
        RenderError::Size { .. } => CommandError::new("noteInvalid", error),
        _ => CommandError::new("render", error),
    };
    let image = render_note(
        NoteView::new(&file.note, picture.as_ref()),
        &RenderOptions {
            scale,
            background: true,
        },
    )
    .map_err(render_error)?;
    encode_png(&image).map_err(render_error)
}

/// `picture_in` as a data URL, for an `<img>`.
#[tauri::command]
pub async fn note_picture(
    context: State<'_, AppContext>,
    path: String,
    max_width: u32,
    max_height: u32,
) -> CommandResult<String> {
    let library = library_of(&context)?;
    crate::notes::blocking(move || {
        picture_in(&library, Path::new(&path), max_width, max_height).map(|png| png_data_url(&png))
    })
    .await
}

#[cfg(test)]
mod tests;
