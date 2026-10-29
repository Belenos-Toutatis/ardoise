//! Import of Wacom Inkspace notes into the library (SPEC §4.3, §6.3): one implementation for
//! the interface (the commands below, `app/ui/src/lib/api.ts`, section "Inkspace import") and
//! for `ardoise.exe --import-inkspace` ([`cli`]).
//!
//! - **Where**: [`ardoise_inkspace::locate_local_state`] looks where Windows keeps Inkspace's
//!   data (nothing is found on other systems); a folder chosen by hand goes through
//!   [`ardoise_inkspace::local_state_in`] (`inkspaceNotFound` when it holds no Inkspace notes).
//! - **Reading** ([`read_notes`]): Inkspace's note store is read into memory through a
//!   `TrackedReader` and converted there. Nothing is ever written into Inkspace's folder, and
//!   its `db\` settings store (whose `profile` key holds an access token and an e-mail address)
//!   is never opened.
//! - **Saving** ([`save_into_library`]): each note is written atomically (`Library::create`)
//!   into the binder (`Inkspace` by default, [`DEFAULT_BINDER`]) with its thumbnail and its
//!   original dates. Notes already imported are skipped: an imported note's id is derived from
//!   Inkspace's note id (UUID v5, `ardoise_inkspace::convert::note_id`), so a note of the
//!   library or of its trash with that id is that Inkspace note, imported before (edits,
//!   renames, moves and the kept part of a split keep the id). The trash counts: a note the
//!   user deleted is not brought back.
//! - **Cancelling** ([`inkspace_cancel`]): checked before each note. The notes written stay,
//!   each complete; the next import skips them.
//! - **Errors** (`errorCodes.*`): `inkspaceNotFound`, `inkspaceUnreadable` (a file cannot be
//!   read, e.g. while Inkspace runs), `inkspaceDamaged`, `inkspaceTooLarge`, `inkspaceBusy`,
//!   `inkspaceTarget` (the binder would be inside Inkspace's folder); a note that cannot be
//!   decoded is reported as `inkspaceNoteDamaged`, one that cannot be written with the
//!   library's code.

pub mod cli;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ardoise_core::format::NoteFile;
use ardoise_core::library::Library;
use ardoise_inkspace::{ImportError, ImportReport, TrackedReader, import_local_state};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

pub use cli::{ImportArgs, run_cli};

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::library::{binder_path, library_error, library_of, path_text, slash_path};

/// Binder receiving imported notes unless the user chooses another one.
pub const DEFAULT_BINDER: &str = "Inkspace";

/// Error code of a note Inkspace stores but that cannot be decoded.
pub const NOTE_DAMAGED: &str = "inkspaceNoteDamaged";

/// What a folder of Inkspace data holds (`InkspaceSource` in `inkspace.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InkspaceSource {
    /// Inkspace's `LocalState` folder.
    pub path: String,
    /// Notes found (one per Inkspace page), readable or not.
    pub notes: usize,
    /// Notes among `notes` already in the library or its trash.
    pub already_imported: usize,
    /// Notes among `notes` that cannot be decoded.
    pub unreadable: usize,
}

/// How far the saving is (`InkspaceProgress` in `inkspace.ts`): notes handled (written, skipped
/// or failed) out of the readable notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ImportProgress {
    pub done: usize,
    pub total: usize,
}

/// A note that was not imported (`InkspaceFailure` in `inkspace.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportFailure {
    /// Inkspace's note id (UUID).
    pub inkspace_id: String,
    /// When the note was written (Unix ms), if it could be decoded.
    pub created: Option<i64>,
    /// `errorCodes.*`.
    pub code: &'static str,
    /// The technical detail.
    pub message: String,
}

/// The final report of an import into the library (`InkspaceReport` in `inkspace.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryImport {
    /// The binder, `/`-separated; empty for the library root.
    pub binder: String,
    /// Notes found.
    pub total: usize,
    pub imported: usize,
    /// Already in the library or its trash.
    pub skipped: usize,
    /// Notes that cannot be decoded, then notes that could not be written.
    pub failed: Vec<ImportFailure>,
    /// Stopped by the user: the notes after the last one handled were not looked at.
    pub cancelled: bool,
}

fn import_error(error: ImportError) -> CommandError {
    let code = match &error {
        ImportError::Io { .. } => "inkspaceUnreadable",
        ImportError::TooLarge { .. } => "inkspaceTooLarge",
        ImportError::Database(_) | ImportError::Malformed(_) => "inkspaceDamaged",
    };
    CommandError::new(code, error)
}

/// The `LocalState` folder meant by `folder`, chosen by hand (`inkspaceNotFound` otherwise).
pub fn local_state_of(reader: &TrackedReader, folder: &Path) -> CommandResult<PathBuf> {
    ardoise_inkspace::local_state_in(reader, folder)
        .ok_or_else(|| CommandError::new("inkspaceNotFound", folder.display()))
}

/// Every note of Inkspace's `local_state`, converted in memory.
pub fn read_notes(local_state: &Path, reader: &TrackedReader) -> CommandResult<ImportReport> {
    import_local_state(local_state, reader).map_err(import_error)
}

/// Ids of the notes of the library and of its trash. A note that cannot be listed (damaged,
/// from a newer version) is not known: its Inkspace note would be imported again.
fn known_ids(library: &Library) -> CommandResult<HashSet<String>> {
    let mut known: HashSet<String> = library
        .list()
        .notes
        .into_iter()
        .map(|entry| entry.id)
        .collect();
    known.extend(
        library
            .trash_items()
            .map_err(library_error)?
            .into_iter()
            .filter_map(|item| item.note_id),
    );
    Ok(known)
}

/// What `local_state` holds, and how much of it `library` already has.
pub fn survey(
    local_state: &Path,
    library: Option<&Library>,
    reader: &TrackedReader,
) -> CommandResult<InkspaceSource> {
    let report = read_notes(local_state, reader)?;
    // Without a library (the first-run wizard) nothing is imported yet; a trash that cannot be
    // read only makes the count less precise.
    let known = library
        .and_then(|library| known_ids(library).ok())
        .unwrap_or_default();
    Ok(InkspaceSource {
        path: path_text(local_state),
        notes: report.total(),
        already_imported: report
            .notes
            .iter()
            .filter(|imported| known.contains(&imported.note.id))
            .count(),
        unreadable: report.failures.len(),
    })
}

/// `path` with its symbolic links and `..` resolved, as far as it exists (the rest appended).
fn resolved(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(real) = existing.canonicalize() {
            return rest.iter().rev().fold(real, |acc, part| acc.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_owned());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// `inkspaceTarget` when notes written into `target` would land inside Inkspace's
/// `local_state` (a library chosen inside it, or a binder leading into a copy of it).
fn check_target(local_state: &Path, target: &Path) -> CommandResult<()> {
    if resolved(target).starts_with(resolved(local_state)) {
        Err(CommandError::new("inkspaceTarget", target.display()))
    } else {
        Ok(())
    }
}

/// Writes every note of `report` into `binder` of `library`, skipping the notes already
/// imported, and telling `progress` before the first note and after each one. Checks `cancel`
/// before each note. A note that cannot be written is reported and does not stop the others.
pub fn save_into_library(
    library: &Library,
    binder: &Path,
    report: &ImportReport,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(ImportProgress),
) -> CommandResult<LibraryImport> {
    let target = library.binder_dir(binder).map_err(library_error)?;
    check_target(&report.local_state, &target)?;
    let mut known = known_ids(library)?;
    let mut result = LibraryImport {
        binder: slash_path(binder),
        total: report.total(),
        imported: 0,
        skipped: 0,
        failed: report
            .failures
            .iter()
            .map(|failure| ImportFailure {
                inkspace_id: failure.inkspace_id.clone(),
                created: None,
                code: NOTE_DAMAGED,
                message: failure.error.to_string(),
            })
            .collect(),
        cancelled: false,
    };
    let total = report.notes.len();
    progress(ImportProgress { done: 0, total });
    for (i, imported) in report.notes.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            result.cancelled = true;
            break;
        }
        let note = &imported.note;
        if known.contains(&note.id) {
            result.skipped += 1;
        } else {
            let mut file = NoteFile::new(note.clone());
            file.thumbnail = crate::thumbnails::thumbnail_png(note);
            match library.create(binder, &file) {
                Ok(_) => {
                    known.insert(note.id.clone());
                    result.imported += 1;
                }
                Err(error) => {
                    let error = library_error(error);
                    let (code, message) = error.parts();
                    result.failed.push(ImportFailure {
                        inkspace_id: imported.inkspace_id.clone(),
                        created: Some(note.created),
                        code,
                        message: message.to_owned(),
                    });
                }
            }
        }
        progress(ImportProgress { done: i + 1, total });
    }
    Ok(result)
}

/// Reads Inkspace's `local_state` and saves its notes into `binder` of `library`
/// ([`read_notes`], then [`save_into_library`]).
pub fn import_into_library(
    local_state: &Path,
    library: &Library,
    binder: &Path,
    reader: &TrackedReader,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(ImportProgress),
) -> CommandResult<LibraryImport> {
    // A binder that cannot be used is refused before Inkspace's data is read.
    let target = library.binder_dir(binder).map_err(library_error)?;
    check_target(local_state, &target)?;
    let report = read_notes(local_state, reader)?;
    save_into_library(library, binder, &report, cancel, progress)
}

// --- Commands -----------------------------------------------------------------------------

/// The import under way: one at a time, and the flag that stops it.
#[derive(Debug, Default)]
pub struct InkspaceImport {
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

/// Marks an import as running until dropped.
#[derive(Debug)]
struct Running(Arc<AtomicBool>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

impl InkspaceImport {
    /// Starts an import (`inkspaceBusy` while another one runs) with a fresh cancel flag.
    fn begin(&self) -> CommandResult<(Running, Arc<AtomicBool>)> {
        if self
            .running
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return Err(CommandError::new(
                "inkspaceBusy",
                "an Inkspace import is running",
            ));
        }
        self.cancel.store(false, Ordering::Relaxed);
        Ok((Running(Arc::clone(&self.running)), Arc::clone(&self.cancel)))
    }
}

/// The work of a command, on a blocking thread (reading and writing take seconds).
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| CommandError::new("unexpected", error))?
}

/// Inkspace's `LocalState` folder on this computer, if any (always `None` outside Windows).
#[tauri::command]
pub async fn inkspace_detect() -> Option<String> {
    tauri::async_runtime::spawn_blocking(|| {
        ardoise_inkspace::locate_local_state(&TrackedReader::new()).map(|dir| path_text(&dir))
    })
    .await
    .ok()
    .flatten()
}

/// What the Inkspace folder `path` (detected, or chosen by hand) holds.
#[tauri::command]
pub async fn inkspace_inspect(
    context: State<'_, AppContext>,
    path: String,
) -> CommandResult<InkspaceSource> {
    let library = library_of(&context).ok();
    blocking(move || {
        let reader = TrackedReader::new();
        let local_state = local_state_of(&reader, Path::new(&path))?;
        survey(&local_state, library.as_ref(), &reader)
    })
    .await
}

/// Imports the notes of the Inkspace folder `path` into `binder` of the library.
#[tauri::command]
pub async fn inkspace_import(
    context: State<'_, AppContext>,
    runtime: State<'_, InkspaceImport>,
    path: String,
    binder: String,
    on_progress: Channel<ImportProgress>,
) -> CommandResult<LibraryImport> {
    let library = library_of(&context)?;
    let (running, cancel) = runtime.begin()?;
    blocking(move || {
        let _running = running;
        let reader = TrackedReader::new();
        let local_state = local_state_of(&reader, Path::new(&path))?;
        import_into_library(
            &local_state,
            &library,
            &binder_path(binder.trim()),
            &reader,
            &cancel,
            &mut |progress| {
                // The interface may have gone: the import goes on.
                let _ = on_progress.send(progress);
            },
        )
    })
    .await
}

/// Asks the import under way to stop before its next note.
#[tauri::command]
pub fn inkspace_cancel(runtime: State<'_, InkspaceImport>) {
    runtime.cancel.store(true, Ordering::Relaxed);
}

#[cfg(test)]
mod tests;
