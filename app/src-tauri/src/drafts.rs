//! Drafts of the notes being edited (M10; `docs/design/interface.md`, "M10: drafts and session
//! end"). While a note has unsaved changes, the editor hands its present state here a few
//! seconds after the last change (and at once when Windows ends the session); a crash, a
//! restart for Windows Update, a logoff or an empty battery then loses at most those seconds.
//!
//! - **Where**: `%LOCALAPPDATA%\Ardoise\drafts\<copy>\` (the copy is the hash of its settings
//!   location, as in the bridge pipe's name: `ardoise_mcp::bridge::pipe::settings_hash`). Never
//!   in the library: drafts are not synced, and another PC never sees them.
//! - **What**: one JSON file per note (`draft-<hash of library and note id>.json`): the note
//!   as edited, the path and the stamp of the file the edits started from, the time, the
//!   process that wrote it; beside it, the background image the edited note shows when it is
//!   only staged in memory (an image imported and not saved yet). Every write is atomic.
//! - **When it goes**: on save (the note is no longer dirty), on an explicit discard, or once
//!   restored as a copy. A draft is never deleted on its own.
//! - **Restoring** is the interface's choice (`lib/editor/drafts.ts`): over the note when the
//!   note on disk is still the version the edits started from, else only as a new note
//!   ([`restore_copy_in`]), never over a newer version.

use std::io;
use std::path::{Path, PathBuf};

use ardoise_core::format::{BackgroundFile, NoteFile};
use ardoise_core::fsutil;
use ardoise_core::library::{FileStamp, Library, LibraryError};
use ardoise_core::note::{Note, Origin};
use ardoise_mcp::bridge::pipe::{fnv1a64, settings_hash, settings_key, settings_key_of};
use ardoise_render::NoteView;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppContext;
use crate::background::{self, Staged, StagedBackground};
use crate::commands::{CommandError, CommandResult, current_state};
use crate::library::{NoteSummary, library_error, library_of, now_ms, slash_path, summary_of};
use crate::notes::blocking;
use crate::thumbnails::thumbnail_view;

/// Version of the draft files; a file of another version is ignored (left in place).
pub const DRAFT_VERSION: u32 = 1;
/// A draft could not be written, read or removed.
pub const DRAFT_FAILED: &str = "draftFailed";
/// No draft is kept for that note.
pub const DRAFT_NOT_FOUND: &str = "draftNotFound";

const DRAFT_PREFIX: &str = "draft-";
const DRAFT_EXTENSION: &str = "json";

/// The background image kept with a draft (the member it would be stored as).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftImage {
    /// The staging key the editor gave (a hash of the bytes): the file is written once per key.
    pub key: String,
    pub extension: String,
    pub width: u32,
    pub height: u32,
}

/// One draft file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftRecord {
    pub version: u32,
    /// The library, as configured when the draft was written.
    pub library: String,
    /// The note file the edits belong to, when the draft was written.
    pub path: String,
    /// The id of the note in that file (the edited note may carry another one after "save as a
    /// copy": this is the file's).
    pub id: String,
    /// Unix ms.
    pub saved_at: i64,
    /// The stamp of the file the edits started from; `None` when unknown.
    pub base_stamp: Option<FileStamp>,
    /// The process that wrote it ([`DraftRuntime::session`]).
    pub session: String,
    #[serde(default)]
    pub image: Option<DraftImage>,
    pub note: Note,
}

/// What the library screen lists (`DraftSummary` in `lib/editor/drafts.ts`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftSummary {
    pub id: String,
    pub path: String,
    pub title: String,
    pub saved_at: i64,
}

/// A draft with its note (`Draft` in `lib/editor/drafts.ts`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Draft {
    pub id: String,
    pub path: String,
    pub title: String,
    pub saved_at: i64,
    pub base_stamp: Option<FileStamp>,
    pub has_image: bool,
    pub note: Note,
}

impl From<&DraftRecord> for DraftSummary {
    fn from(record: &DraftRecord) -> Self {
        DraftSummary {
            id: record.id.clone(),
            path: record.path.clone(),
            title: record.note.title.clone(),
            saved_at: record.saved_at,
        }
    }
}

impl From<DraftRecord> for Draft {
    fn from(record: DraftRecord) -> Self {
        Draft {
            has_image: record.image.is_some(),
            id: record.id,
            path: record.path,
            title: record.note.title.clone(),
            saved_at: record.saved_at,
            base_stamp: record.base_stamp,
            note: record.note,
        }
    }
}

/// A library path as compared between drafts (`\\?\` dropped, `\` separators, lower case).
pub fn library_key(library: &str) -> String {
    settings_key_of(library.trim_end_matches(['\\', '/']))
}

/// The file stem of the draft of note `id` in `library`: a stable hash, so that any id (even
/// one with characters a file name cannot hold) gives a plain name.
pub fn draft_stem(library: &str, id: &str) -> String {
    let key = format!("{}\n{id}", library_key(library));
    format!("{DRAFT_PREFIX}{:016x}", fnv1a64(key.as_bytes()))
}

/// A staging key or an extension as part of a file name: ASCII letters, digits and `-` only.
fn plain(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The drafts of one copy of Ardoise, in one folder.
#[derive(Debug, Clone)]
pub struct DraftStore {
    dir: PathBuf,
}

impl DraftStore {
    pub fn new(dir: impl Into<PathBuf>) -> DraftStore {
        DraftStore { dir: dir.into() }
    }

    /// `<local>/Ardoise/drafts/<copy>`; `shared` when the copy is unknown (no settings
    /// location).
    pub fn for_copy(local: &Path, copy: Option<&str>) -> DraftStore {
        DraftStore::new(
            local
                .join("Ardoise")
                .join("drafts")
                .join(copy.unwrap_or("shared")),
        )
    }

    #[cfg(test)]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn json_path(&self, stem: &str) -> PathBuf {
        self.dir.join(format!("{stem}.{DRAFT_EXTENSION}"))
    }

    fn image_path(&self, stem: &str, image: &DraftImage) -> Option<PathBuf> {
        (plain(&image.key, 64) && plain(&image.extension, 10)).then(|| {
            self.dir
                .join(format!("{stem}.{}.{}", image.key, image.extension))
        })
    }

    /// Writes `record` (and `image`, the member its note shows, when the record names one and
    /// it is not on disk yet), replacing the note's previous draft. The image goes first: a
    /// draft file never names an image that is not there.
    pub fn write(&self, record: &DraftRecord, image: Option<&BackgroundFile>) -> io::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        let stem = draft_stem(&record.library, &record.id);
        let mut record = record.clone();
        let kept = match (&record.image, image) {
            (Some(described), Some(member)) => match self.image_path(&stem, described) {
                Some(path) => {
                    if !path.is_file() {
                        fsutil::write_atomic(&path, &member.bytes)?;
                    }
                    Some(path)
                }
                None => None,
            },
            _ => None,
        };
        if kept.is_none() {
            record.image = None;
        }
        let bytes = serde_json::to_vec(&record).map_err(io::Error::other)?;
        fsutil::write_atomic(&self.json_path(&stem), &bytes)?;
        self.remove_images(&stem, kept.as_deref());
        Ok(())
    }

    /// The draft of note `id` in `library`, if a readable one of this version exists.
    pub fn read(&self, library: &str, id: &str) -> Option<DraftRecord> {
        let record = read_record(&self.json_path(&draft_stem(library, id)))?;
        (record.id == id && library_key(&record.library) == library_key(library)).then_some(record)
    }

    /// Every readable draft of `library`, the most recent first.
    pub fn list(&self, library: &str) -> Vec<DraftRecord> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let wanted = library_key(library);
        let mut records: Vec<DraftRecord> = entries
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|e| e == DRAFT_EXTENSION)
                    && path
                        .file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with(DRAFT_PREFIX))
            })
            .filter_map(|path| read_record(&path))
            .filter(|record| library_key(&record.library) == wanted)
            .collect();
        records.sort_by(|a, b| b.saved_at.cmp(&a.saved_at).then(a.id.cmp(&b.id)));
        records
    }

    /// Removes the draft of note `id` in `library` and its image (nothing to do when there is
    /// none).
    pub fn remove(&self, library: &str, id: &str) -> io::Result<()> {
        let stem = draft_stem(library, id);
        match std::fs::remove_file(self.json_path(&stem)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.remove_images(&stem, None);
        Ok(())
    }

    /// The background image kept with `record`, if any.
    pub fn image(&self, record: &DraftRecord) -> Option<BackgroundFile> {
        let image = record.image.as_ref()?;
        let path = self.image_path(&draft_stem(&record.library, &record.id), image)?;
        Some(BackgroundFile {
            extension: image.extension.clone(),
            bytes: std::fs::read(path).ok()?,
        })
    }

    /// Removes the images of draft `stem` but `keep` (best effort).
    fn remove_images(&self, stem: &str, keep: Option<&Path>) {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return;
        };
        let prefix = format!("{stem}.");
        let json = format!("{stem}.{DRAFT_EXTENSION}");
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            if name.starts_with(&prefix) && name != json && Some(path.as_path()) != keep {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

fn read_record(path: &Path) -> Option<DraftRecord> {
    let record: DraftRecord = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    (record.version == DRAFT_VERSION).then_some(record)
}

/// Makes a new note of `record`'s edits in `library`, titled `title`: in the binder of the note
/// it came from when that folder still exists, else at the root, with a new id and the image
/// kept with the draft (an image whose pixels were not kept is left out). The note it came from
/// is not touched.
pub fn restore_copy_in(
    library: &Library,
    record: &DraftRecord,
    image: Option<BackgroundFile>,
    title: &str,
) -> Result<NoteSummary, LibraryError> {
    let now = now_ms();
    let mut note = record.note.clone();
    note.id = uuid::Uuid::new_v4().to_string();
    note.title = title.trim().to_owned();
    note.created = now;
    note.modified = now;
    note.origin = Origin::Ardoise;
    let background = if note.background_image.is_some() {
        image
    } else {
        None
    };
    if background.is_none() {
        note.background_image = None;
    }
    let picture = background::picture_of(&note, background.as_ref());
    let mut file = NoteFile::new(note);
    file.thumbnail = thumbnail_view(NoteView::new(&file.note, picture.as_ref()));
    file.background = background;
    let saved = library.create(&binder_of(library, &record.path), &file)?;
    Ok(summary_of(library, &saved.path, &file.note, saved.stamp))
}

/// The binder of the note file `path` when its folder is inside `library` and still exists,
/// else the root.
fn binder_of(library: &Library, path: &str) -> PathBuf {
    let Some(dir) = Path::new(path).parent() else {
        return PathBuf::new();
    };
    match dir.strip_prefix(library.root()) {
        Ok(relative) if dir.is_dir() => PathBuf::from(slash_path(relative)),
        _ => PathBuf::new(),
    }
}

// --- App state and commands -------------------------------------------------------------------

/// The drafts of the running copy, and the token of this process (drafts it wrote are not
/// "left over from a previous session").
pub struct DraftRuntime {
    store: DraftStore,
    session: String,
}

impl DraftRuntime {
    /// The store of the copy whose settings are `context.store`, under `%LOCALAPPDATA%` (the
    /// temporary folder when it is not set).
    pub fn new(context: &AppContext) -> DraftRuntime {
        let local = std::env::var_os("LOCALAPPDATA")
            .filter(|dir| !dir.is_empty())
            .map_or_else(std::env::temp_dir, PathBuf::from);
        let copy = context
            .store
            .as_ref()
            .ok()
            .map(|store| settings_hash(&settings_key(store.path())));
        DraftRuntime {
            store: DraftStore::for_copy(&local, copy.as_deref()),
            session: uuid::Uuid::new_v4().to_string(),
        }
    }
}

fn failed(error: impl ToString) -> CommandError {
    CommandError::new(DRAFT_FAILED, error)
}

/// The configured library, as text (`noLibrary` when none).
fn library_text(context: &AppContext) -> CommandResult<String> {
    current_state(context)
        .library()
        .map(str::to_owned)
        .ok_or_else(|| CommandError::new("noLibrary", "no library is chosen on this computer"))
}

/// The draft of `note`, edited from the file at `path` whose id is `note_id`.
pub fn draft_of(
    library: &str,
    path: &str,
    note_id: &str,
    note: Note,
    base_stamp: Option<FileStamp>,
    session: &str,
    image: Option<(&str, &Staged)>,
) -> DraftRecord {
    DraftRecord {
        version: DRAFT_VERSION,
        library: library.to_owned(),
        path: path.to_owned(),
        id: note_id.to_owned(),
        saved_at: now_ms(),
        base_stamp,
        session: session.to_owned(),
        image: image
            .filter(|_| note.background_image.is_some())
            .map(|(key, staged)| DraftImage {
                key: key.to_owned(),
                extension: staged.file.extension.clone(),
                width: staged.width,
                height: staged.height,
            }),
        note,
    }
}

/// Keeps the editor's unsaved note (`imageKey`: its staged background pixels).
#[tauri::command]
pub async fn draft_write(
    context: State<'_, AppContext>,
    runtime: State<'_, DraftRuntime>,
    path: String,
    note_id: String,
    note: Note,
    base_stamp: Option<FileStamp>,
    image_key: Option<String>,
) -> CommandResult<()> {
    let library = library_text(&context)?;
    let store = runtime.store.clone();
    let session = runtime.session.clone();
    blocking(move || {
        let staged = image_key
            .as_deref()
            .and_then(|key| background::staging().get(key).map(|staged| (key, staged)));
        let record = draft_of(
            &library,
            &path,
            &note_id,
            note,
            base_stamp,
            &session,
            staged.as_ref().map(|(key, staged)| (*key, staged.as_ref())),
        );
        let member = staged.as_ref().map(|(_, staged)| &staged.file);
        store.write(&record, member).map_err(failed)
    })
    .await
}

/// The draft kept for note `noteId` of the library, if any.
#[tauri::command]
pub async fn draft_find(
    context: State<'_, AppContext>,
    runtime: State<'_, DraftRuntime>,
    note_id: String,
) -> CommandResult<Option<Draft>> {
    let library = library_text(&context)?;
    let store = runtime.store.clone();
    blocking(move || Ok(store.read(&library, &note_id).map(Draft::from))).await
}

/// The drafts of the library left by an earlier run of Ardoise (closed without saving).
#[tauri::command]
pub async fn draft_list(
    context: State<'_, AppContext>,
    runtime: State<'_, DraftRuntime>,
) -> CommandResult<Vec<DraftSummary>> {
    let library = library_text(&context)?;
    let store = runtime.store.clone();
    let session = runtime.session.clone();
    blocking(move || {
        Ok(store
            .list(&library)
            .iter()
            .filter(|record| record.session != session)
            .map(DraftSummary::from)
            .collect())
    })
    .await
}

/// Forgets the draft of note `noteId` (saved, or its changes discarded).
#[tauri::command]
pub async fn draft_discard(
    context: State<'_, AppContext>,
    runtime: State<'_, DraftRuntime>,
    note_id: String,
) -> CommandResult<()> {
    let library = library_text(&context)?;
    let store = runtime.store.clone();
    blocking(move || store.remove(&library, &note_id).map_err(failed)).await
}

/// Stages the background image kept with the draft of `noteId` for the editor; `None` when it
/// kept none.
#[tauri::command]
pub async fn draft_stage_image(
    context: State<'_, AppContext>,
    runtime: State<'_, DraftRuntime>,
    note_id: String,
) -> CommandResult<Option<StagedBackground>> {
    let library = library_text(&context)?;
    let store = runtime.store.clone();
    blocking(move || {
        let record = store
            .read(&library, &note_id)
            .ok_or_else(|| CommandError::new(DRAFT_NOT_FOUND, "no draft for this note"))?;
        let (Some(image), Some(described), Some(member)) = (
            record.note.background_image.as_ref(),
            record.image.as_ref(),
            store.image(&record),
        ) else {
            return Ok(None);
        };
        let (width, height) = (described.width, described.height);
        let key = background::staging().put(Staged {
            file: member,
            width,
            height,
        });
        Ok(Some(StagedBackground {
            key,
            name: image.name.clone(),
            pdf_page: image.pdf_page,
            width,
            height,
        }))
    })
    .await
}

/// Makes a new note titled `title` of the draft of `noteId`, then forgets the draft.
#[tauri::command]
pub async fn draft_restore_copy(
    context: State<'_, AppContext>,
    runtime: State<'_, DraftRuntime>,
    note_id: String,
    title: String,
) -> CommandResult<NoteSummary> {
    let library_path = library_text(&context)?;
    let library = library_of(&context)?;
    let store = runtime.store.clone();
    blocking(move || {
        let record = store
            .read(&library_path, &note_id)
            .ok_or_else(|| CommandError::new(DRAFT_NOT_FOUND, "no draft for this note"))?;
        let image = store.image(&record);
        let summary = restore_copy_in(&library, &record, image, &title).map_err(library_error)?;
        // The copy holds the edits now; a draft that cannot be removed is offered again later.
        let _ = store.remove(&library_path, &note_id);
        Ok(summary)
    })
    .await
}

#[cfg(test)]
mod tests;
