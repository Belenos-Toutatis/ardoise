//! The trash: `_ardoise/trash/` in the library, so that it follows the notes to every
//! computer.
//!
//! A trashed note keeps its bytes untouched and is renamed `<deleted ms> <file name>`
//! (unique). Next to it, `<that name>.json` records where it came from:
//! `{"original": "Classe/2026-10-24 Titre.ardoise", "deleted": 1792843200000, "title": …,
//! "id": …}` (`original` uses `/` whatever the system). The record is written before the move,
//! so a note in the trash always knows where to go back; a record left alone by an
//! interruption is ignored and purged.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

use super::names::{MAX_SUFFIX, exists, is_note_file_name, note_stem, with_suffix};
use super::{Library, LibraryError, META_DIR, format_error, io_error, now_ms};
use crate::format::{self, NoteFile};
use crate::fsutil::{remove_file_with_retry, rename_no_replace, write_atomic_new};
use crate::note::Note;

/// Folder of the trash inside `_ardoise/`.
pub const TRASH_DIR: &str = "trash";
/// How long trashed notes are kept before [`Library::purge_expired`] removes them.
pub const TRASH_RETENTION: Duration = Duration::from_secs(30 * 24 * 60 * 60);

const RECORD_SUFFIX: &str = ".json";

/// Why a note could not be put at a name of the trash.
enum Placed {
    /// The name was taken meanwhile: the next one is tried.
    Taken,
    Failed(LibraryError),
}

/// The `.json` record next to a trashed note.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Record {
    /// Path relative to the library root, `/`-separated.
    original: String,
    /// Unix milliseconds.
    deleted: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    id: Option<String>,
}

/// A note in the trash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashItem {
    /// Identifier for [`Library::restore`] and [`Library::purge`]: the file name in the trash.
    pub id: String,
    /// Where the note is now.
    pub path: PathBuf,
    /// Where it came from, relative to the library root.
    pub original: PathBuf,
    /// When it was trashed (Unix milliseconds).
    pub deleted: i64,
    pub title: Option<String>,
    pub note_id: Option<String>,
}

fn record_path(note: &Path) -> PathBuf {
    let mut name = note.as_os_str().to_owned();
    name.push(RECORD_SUFFIX);
    PathBuf::from(name)
}

/// `original` of a record, accepted only if it is a plain relative note path.
fn relative_note_path(original: &str) -> Option<PathBuf> {
    let path = PathBuf::from(original.replace('\\', "/"));
    let mut parts = 0;
    for component in path.components() {
        match component {
            // A colon is never part of a valid name (Windows reserves it for drive letters and
            // streams): rejecting it keeps `C:/x` out on every system, not only on Windows.
            Component::Normal(part)
                if !super::walk::is_skipped_name(part.to_str()?)
                    && !part.to_str()?.contains(':') =>
            {
                parts += 1
            }
            _ => return None,
        }
    }
    let name = path.file_name()?.to_str()?;
    (parts > 0 && is_note_file_name(name)).then_some(path)
}

/// For a trash file without a valid record: `<ms> <name>` gives both back.
fn guess_from_name(id: &str) -> (Option<i64>, String) {
    match id.split_once(' ') {
        Some((ms, rest)) if !rest.is_empty() && ms.bytes().all(|b| b.is_ascii_digit()) => {
            (ms.parse().ok(), rest.to_owned())
        }
        _ => (None, id.to_owned()),
    }
}

impl Library {
    /// The trash folder (may not exist yet).
    pub fn trash_dir(&self) -> PathBuf {
        self.root.join(META_DIR).join(TRASH_DIR)
    }

    /// Moves a note to the trash. Its bytes are not changed.
    pub fn trash(&self, path: &Path) -> Result<TrashItem, LibraryError> {
        let path = self.note_path(path)?;
        if !path.is_file() {
            return Err(LibraryError::NotFound { path });
        }
        // Best effort: a damaged note can be trashed too.
        let header = format::read_header(&path).ok();
        self.put_in_trash(
            &path,
            header.as_ref().map(|h| &h.note),
            |target| match rename_no_replace(&path, target) {
                Err(e) if e.kind() == io::ErrorKind::NotFound && !exists(&path) => {
                    Err(Placed::Failed(LibraryError::NotFound {
                        path: path.clone(),
                    }))
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(Placed::Taken),
                Err(e) => Err(Placed::Failed(io_error(&path)(e))),
                Ok(()) => Ok(()),
            },
        )
    }

    /// Puts `file`, a note that is not in the library, into the trash as if it had been trashed
    /// from `original` (a note path of this library, absolute or relative, that need not exist):
    /// the trash view shows it with its title, and restoring it creates that note (" (n)" added
    /// when the name is taken). For a version of a note kept before a change the app cannot undo
    /// (`straighten_shapes` of the MCP server): give it its own id, so that it never stands for
    /// the note itself once restored.
    pub fn trash_copy(&self, file: &NoteFile, original: &Path) -> Result<TrashItem, LibraryError> {
        let original = self.note_path(original)?;
        let bytes = format::to_bytes(file).map_err(format_error(&original))?;
        self.put_in_trash(
            &original,
            Some(&file.note),
            |target| match write_atomic_new(target, &bytes) {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(Placed::Taken),
                Err(e) => Err(Placed::Failed(io_error(target)(e))),
                Ok(_) => Ok(()),
            },
        )
    }

    /// Writes the record of a note trashed from `path` (absolute, in the library) under the
    /// first free name of the trash, then has `place` put the note at that name.
    fn put_in_trash(
        &self,
        path: &Path,
        note: Option<&Note>,
        mut place: impl FnMut(&Path) -> Result<(), Placed>,
    ) -> Result<TrashItem, LibraryError> {
        let relative = path.strip_prefix(&self.root).unwrap_or(path);
        let original = relative
            .components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect::<Vec<_>>()
            .join("/");
        let deleted = now_ms();
        let record = Record {
            original: original.clone(),
            deleted,
            title: note.map(|n| n.title.clone()),
            id: note.map(|n| n.id.clone()),
        };
        let record_bytes = serde_json::to_vec_pretty(&record).map_err(|e| LibraryError::Io {
            path: path.to_path_buf(),
            source: io::Error::other(e),
        })?;
        let dir = self.trash_dir();
        fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let stem = format!("{deleted} {}", note_stem(name));
        for n in 1..=MAX_SUFFIX {
            let id = format!("{}.{}", with_suffix(&stem, n), super::NOTE_EXTENSION);
            let target = dir.join(&id);
            let record_file = record_path(&target);
            if exists(&target) || exists(&record_file) {
                continue;
            }
            match write_atomic_new(&record_file, &record_bytes) {
                Ok(_) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_error(&record_file)(e)),
            }
            match place(&target) {
                Ok(()) => {
                    return Ok(TrashItem {
                        id,
                        path: target,
                        original: PathBuf::from(original),
                        deleted,
                        title: record.title,
                        note_id: record.id,
                    });
                }
                Err(failure) => {
                    let _ = remove_file_with_retry(&record_file);
                    match failure {
                        Placed::Taken => continue,
                        Placed::Failed(error) => return Err(error),
                    }
                }
            }
        }
        Err(LibraryError::NoFreeName {
            path: dir.join(stem),
        })
    }

    fn trash_item(&self, id: &str, path: PathBuf) -> TrashItem {
        let record: Option<Record> = fs::read(record_path(&path))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let (guessed_time, guessed_name) = guess_from_name(id);
        let original = record
            .as_ref()
            .and_then(|r| relative_note_path(&r.original))
            .or_else(|| relative_note_path(&guessed_name))
            .unwrap_or_else(|| PathBuf::from(id));
        let deleted = record
            .as_ref()
            .map(|r| r.deleted)
            .or(guessed_time)
            .or_else(|| {
                let modified = fs::metadata(&path).and_then(|m| m.modified()).ok()?;
                let since = modified.duration_since(std::time::UNIX_EPOCH).ok()?;
                i64::try_from(since.as_millis()).ok()
            })
            .unwrap_or(0);
        let (title, note_id) = record.map_or((None, None), |r| (r.title, r.id));
        TrashItem {
            id: id.to_owned(),
            path,
            original,
            deleted,
            title,
            note_id,
        }
    }

    /// Every note in the trash, most recently trashed first.
    pub fn trash_items(&self) -> Result<Vec<TrashItem>, LibraryError> {
        let dir = self.trash_dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(io_error(&dir)(e)),
        };
        let mut items = Vec::new();
        for entry in entries {
            let entry = entry.map_err(io_error(&dir))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if is_note_file_name(&name) && entry.file_type().is_ok_and(|t| t.is_file()) {
                items.push(self.trash_item(&name, entry.path()));
            }
        }
        items.sort_by(|a, b| b.deleted.cmp(&a.deleted).then_with(|| a.id.cmp(&b.id)));
        Ok(items)
    }

    /// Path of a trash item, checking that `id` is a plain name of a note in the trash.
    fn trash_path(&self, id: &str) -> Result<PathBuf, LibraryError> {
        let plain = !id.contains(['/', '\\', ':']) && is_note_file_name(id);
        let path = self.trash_dir().join(id);
        if plain && path.is_file() {
            Ok(path)
        } else {
            Err(LibraryError::NotInTrash { id: id.to_owned() })
        }
    }

    /// Puts a trashed note back where it was (its binder is recreated if needed; a ` (n)`
    /// suffix is added if the name has been taken since). Returns its new path.
    pub fn restore(&self, id: &str) -> Result<PathBuf, LibraryError> {
        let path = self.trash_path(id)?;
        let item = self.trash_item(id, path.clone());
        let parent = item.original.parent().unwrap_or(Path::new(""));
        let dir = self
            .binder_dir(parent)
            .unwrap_or_else(|_| self.root.clone());
        fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        let name = item
            .original
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(id);
        let restored = self.move_unique(&path, &dir, note_stem(name))?;
        let _ = remove_file_with_retry(&record_path(&path));
        Ok(restored)
    }

    /// Deletes one trashed note for good.
    pub fn purge(&self, id: &str) -> Result<(), LibraryError> {
        let path = self.trash_path(id)?;
        remove_file_with_retry(&path).map_err(io_error(&path))?;
        let _ = remove_file_with_retry(&record_path(&path));
        Ok(())
    }

    /// Deletes for good the notes trashed more than [`TRASH_RETENTION`] before `now_ms`, and
    /// records left without a note. Returns how many notes were deleted.
    ///
    /// The date in the record is the clock of the computer that trashed the note, and this
    /// computer's clock may be far ahead of it: a note is also kept until its file has been
    /// here for the whole retention, by this computer's own clock (`created` time of the file
    /// in the trash: the moment Nextcloud brought it, or the note's own creation for a note
    /// trashed here, which the record dates the same way). A wrong clock then deletes nothing
    /// that other computers trashed recently.
    pub fn purge_expired_at(&self, now_ms: i64) -> Result<usize, LibraryError> {
        let retention = i64::try_from(TRASH_RETENTION.as_millis()).unwrap_or(i64::MAX);
        let limit = now_ms.saturating_sub(retention);
        self.purge_where(
            |item| {
                item.deleted <= limit
                    && local_file_time(&item.path).is_some_and(|here| here <= limit)
            },
            |deleted| deleted <= limit,
        )
    }

    /// [`purge_expired_at`](Self::purge_expired_at) now.
    pub fn purge_expired(&self) -> Result<usize, LibraryError> {
        self.purge_expired_at(now_ms())
    }

    /// Empties the trash. Returns how many notes were deleted.
    pub fn empty_trash(&self) -> Result<usize, LibraryError> {
        self.purge_where(|_| true, |_| true)
    }

    fn purge_where(
        &self,
        expired: impl Fn(&TrashItem) -> bool,
        expired_record: impl Fn(i64) -> bool,
    ) -> Result<usize, LibraryError> {
        let mut purged = 0;
        for item in self.trash_items()? {
            if expired(&item) {
                remove_file_with_retry(&item.path).map_err(io_error(&item.path))?;
                let _ = remove_file_with_retry(&record_path(&item.path));
                purged += 1;
            }
        }
        self.purge_orphan_records(&expired_record)?;
        Ok(purged)
    }

    /// Removes records whose note is gone (an interrupted trash or restore).
    fn purge_orphan_records(&self, expired: &impl Fn(i64) -> bool) -> Result<(), LibraryError> {
        let dir = self.trash_dir();
        let Ok(entries) = fs::read_dir(&dir) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Some(note_name) = name.strip_suffix(RECORD_SUFFIX) else {
                continue;
            };
            if !is_note_file_name(note_name) || dir.join(note_name).exists() {
                continue;
            }
            let deleted = fs::read(&path)
                .ok()
                .and_then(|b| serde_json::from_slice::<Record>(&b).ok())
                .map_or(0, |r| r.deleted);
            if expired(deleted) {
                let _ = remove_file_with_retry(&path);
            }
        }
        Ok(())
    }
}

/// When this computer got the file, by its clock: the creation time of the file (a download
/// creates it), else its modification time; Unix milliseconds. `None` if neither is known.
fn local_file_time(path: &Path) -> Option<i64> {
    let metadata = fs::metadata(path).ok()?;
    let time = metadata.created().or_else(|_| metadata.modified()).ok()?;
    let since = time.duration_since(std::time::UNIX_EPOCH).ok()?;
    i64::try_from(since.as_millis()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_paths_are_validated() {
        assert_eq!(
            relative_note_path("Classe/2026-10-24 Titre.ardoise"),
            Some(PathBuf::from("Classe").join("2026-10-24 Titre.ardoise"))
        );
        assert_eq!(
            relative_note_path(r"Classe\a.ardoise"),
            Some(PathBuf::from("Classe").join("a.ardoise"))
        );
        for bad in [
            "../evil.ardoise",
            "/abs.ardoise",
            "C:/abs.ardoise",
            "_ardoise/trash/x.ardoise",
            ".ardoise/trash/x.ardoise",
            "Classe/_Ardoise/x.ardoise",
            "x.exe",
            "",
            "a/../b.ardoise",
        ] {
            assert_eq!(relative_note_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn names_without_record() {
        assert_eq!(
            guess_from_name("1792843200000 2026-10-24 Titre.ardoise"),
            (
                Some(1_792_843_200_000),
                "2026-10-24 Titre.ardoise".to_owned()
            )
        );
        assert_eq!(
            guess_from_name("Titre.ardoise"),
            (None, "Titre.ardoise".to_owned())
        );
    }
}
