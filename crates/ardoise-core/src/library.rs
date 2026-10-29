//! The user's library folder: an ordinary folder of notes plus an `_ardoise/` folder
//! (SPEC §3.2, §3.4).
//!
//! - One note = one `YYYY-MM-DD Title.ardoise` file; sub-folders are binders.
//! - Every write is atomic ([`crate::fsutil`]); renames and moves never replace a file.
//! - Saving checks the file's modification time and size against what was loaded, before
//!   writing and again right before the new version replaces the file: a note changed
//!   meanwhile on another computer (and synced by Nextcloud) is never overwritten silently
//!   ([`LibraryError::Conflict`]).
//! - No lock file, no database: the folder can be synced by any tool.

mod conflicts;
mod names;
mod trash;
mod walk;
mod watch;

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::format::{self, FormatError, NoteFile};
use crate::fsutil::{CheckedWriteError, rename_no_replace, write_atomic_checked, write_atomic_new};

pub use conflicts::{ConflictKind, ConflictName, parse_conflict_name};
pub use names::{
    MAX_NAME_CHARS, NOTE_EXTENSION, invalid_name_reason, local_date, note_file_stem, sanitize_name,
};
pub use trash::{TRASH_DIR, TRASH_RETENTION, TrashItem};
pub use watch::{LibraryChange, LibraryWatcher};

use names::{MAX_SUFFIX, exists, note_path, note_stem, stem_matches, with_suffix};

/// Folder that marks a library and holds its shared data (tablet registration, diagnostics,
/// trash). Not hidden: Nextcloud does not sync hidden folders unless told to.
pub const META_DIR: &str = "_ardoise";

/// The first name of [`META_DIR`]. Being hidden (leading dot), Nextcloud's default settings
/// left it out of the sync; libraries that still have it are migrated by
/// [`migrate_legacy_meta_dir`].
pub const LEGACY_META_DIR: &str = ".ardoise";

/// Whether `folder` already holds an Ardoise library (under the current or the old name).
pub fn is_library(folder: &Path) -> bool {
    folder.join(META_DIR).is_dir() || folder.join(LEGACY_META_DIR).is_dir()
}

/// Whether `dir` or a folder below it holds a note file. Walks like the library does (links
/// and junctions are not followed, so a loop cannot run away; hidden and temporary names are
/// skipped).
pub fn holds_notes(dir: &Path) -> bool {
    let mut found = false;
    walk::walk(dir, |item| {
        if matches!(item, walk::Found::Note { .. }) {
            found = true;
        }
    });
    found
}

/// Makes `folder` a library, keeping anything already there. The folder itself must exist,
/// so that a typo in a path never silently creates a new tree.
pub fn init(folder: &Path) -> io::Result<()> {
    if !folder.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("{} is not an existing folder", folder.display()),
        ));
    }
    migrate_legacy_meta_dir(folder)?;
    fs::create_dir_all(folder.join(META_DIR))
}

/// Moves the content of an old `.ardoise/` folder into `_ardoise/`: a plain rename when
/// `_ardoise/` does not exist yet, otherwise a merge in which nothing is overwritten (on a
/// name clash the `_ardoise/` file wins and the old one stays in `.ardoise/`, which is then
/// kept). Returns whether anything was moved. Safe to call at every start.
pub fn migrate_legacy_meta_dir(folder: &Path) -> io::Result<bool> {
    let legacy = folder.join(LEGACY_META_DIR);
    if !legacy.is_dir() {
        return Ok(false);
    }
    let current = folder.join(META_DIR);
    if !current.exists() {
        fs::rename(&legacy, &current)?;
        return Ok(true);
    }
    let moved = merge_dir(&legacy, &current)?;
    // Only an emptied folder goes; anything left (clashes) is kept for the user.
    let _ = fs::remove_dir(&legacy);
    Ok(moved)
}

/// Moves every entry of `from` into `to` without replacing anything; returns whether
/// something moved. Folders present on both sides are merged recursively.
fn merge_dir(from: &Path, to: &Path) -> io::Result<bool> {
    let mut moved = false;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let source = entry.path();
        let target = to.join(entry.file_name());
        if !target.exists() {
            rename_no_replace(&source, &target)?;
            moved = true;
        } else if source.is_dir() && target.is_dir() {
            moved |= merge_dir(&source, &target)?;
            let _ = fs::remove_dir(&source);
        }
    }
    Ok(moved)
}

/// Modification time and size of a note file, as last seen. Two different stamps mean the
/// file was rewritten in between (by this app, another program, or Nextcloud bringing a
/// version saved on another computer, which keeps that computer's modification time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FileStamp {
    pub modified: SystemTime,
    pub len: u64,
}

impl FileStamp {
    /// Stamp of a file's metadata.
    pub fn from_metadata(metadata: &fs::Metadata) -> io::Result<FileStamp> {
        Ok(FileStamp {
            modified: metadata.modified()?,
            len: metadata.len(),
        })
    }

    /// Current stamp of the file at `path`.
    pub fn of(path: &Path) -> io::Result<FileStamp> {
        FileStamp::from_metadata(&fs::metadata(path)?)
    }
}

/// Failure of a library operation.
#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    /// The folder is not a library (no `_ardoise/` folder).
    #[error("{} is not an Ardoise library", path.display())]
    NotALibrary {
        /// Folder given.
        path: PathBuf,
    },
    /// A file or folder could not be read or written.
    #[error("cannot access {}: {source}", path.display())]
    Io {
        /// Path involved.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
    /// A note file cannot be read or written.
    #[error("{}: {source}", path.display())]
    Format {
        /// Note file involved.
        path: PathBuf,
        /// What is wrong with it.
        #[source]
        source: FormatError,
    },
    /// The note changed on disk since it was loaded (probably saved on another computer and
    /// synced): it was left untouched.
    #[error(
        "{} was changed elsewhere (probably on another computer) since it was opened; it was not overwritten",
        path.display()
    )]
    Conflict {
        /// Note file involved.
        path: PathBuf,
    },
    /// The note no longer exists (deleted or moved, here or on another computer).
    #[error("{} no longer exists", path.display())]
    NotFound {
        /// Path that was expected.
        path: PathBuf,
    },
    /// The path is not a note file inside this library.
    #[error("{} is not a note of this library", path.display())]
    NotInLibrary {
        /// Path given.
        path: PathBuf,
    },
    /// A binder or file name cannot be used.
    #[error("invalid name {name:?}: {reason}")]
    InvalidName {
        /// Name given.
        name: String,
        /// Why it is refused.
        reason: &'static str,
    },
    /// Every ` (n)` variant of a name is taken.
    #[error("no free file name for {}", path.display())]
    NoFreeName {
        /// First name tried.
        path: PathBuf,
    },
    /// No trash item has this identifier.
    #[error("{id} is not in the trash")]
    NotInTrash {
        /// Identifier given.
        id: String,
    },
    /// The folder cannot be watched.
    #[error("cannot watch {}: {message}", path.display())]
    Watch {
        /// Folder involved.
        path: PathBuf,
        /// Diagnostic from the system.
        message: String,
    },
}

impl LibraryError {
    /// Stable identifier for the interface (`errorCodes.*`).
    pub fn code(&self) -> &'static str {
        match self {
            LibraryError::NotALibrary { .. } => "notALibrary",
            LibraryError::Io { .. } => "libraryAccess",
            LibraryError::Format { source, .. } => source.code(),
            LibraryError::Conflict { .. } => "noteConflict",
            LibraryError::NotFound { .. } => "noteNotFound",
            LibraryError::NotInLibrary { .. } => "notInLibrary",
            LibraryError::InvalidName { .. } => "invalidName",
            LibraryError::NoFreeName { .. } => "noFreeName",
            LibraryError::NotInTrash { .. } => "notInTrash",
            LibraryError::Watch { .. } => "libraryWatch",
        }
    }
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> LibraryError + '_ {
    move |source| LibraryError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn format_error(path: &Path) -> impl FnOnce(FormatError) -> LibraryError + '_ {
    move |source| LibraryError::Format {
        path: path.to_path_buf(),
        source,
    }
}

/// Current time in Unix milliseconds.
pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// One note as listed: its `note.json` metadata and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEntry {
    pub path: PathBuf,
    /// Folder relative to the library root; empty for the root itself.
    pub binder: PathBuf,
    pub file_name: String,
    pub id: String,
    pub title: String,
    /// Unix milliseconds.
    pub created: i64,
    /// Unix milliseconds.
    pub modified: i64,
    pub tags: Vec<String>,
    pub favorite: bool,
    /// To pass back to [`Library::save`].
    pub stamp: FileStamp,
    /// Set when the file is a sync client's conflict copy.
    pub conflict: Option<ConflictName>,
}

/// A file that could not be listed (damaged, made by a newer version, unreadable folder…).
#[derive(Debug)]
pub struct ListProblem {
    pub path: PathBuf,
    pub error: LibraryError,
}

/// Result of [`Library::list`]: the readable notes, and what could not be read.
#[derive(Debug, Default)]
pub struct Listing {
    /// Sorted by path.
    pub notes: Vec<NoteEntry>,
    pub problems: Vec<ListProblem>,
    /// Every binder (sub-folder), relative to the root, sorted; empty ones included.
    pub binders: Vec<PathBuf>,
}

/// The notes of a past [`Listing`], to list again without reopening the files that did not
/// change ([`Library::list_with`]).
#[derive(Debug, Default)]
pub struct HeaderCache {
    entries: HashMap<PathBuf, NoteEntry>,
}

impl HeaderCache {
    /// The listed note at `path`, if its file still has `stamp`.
    pub fn get(&self, path: &Path, stamp: &FileStamp) -> Option<&NoteEntry> {
        self.entries.get(path).filter(|entry| entry.stamp == *stamp)
    }
}

/// A tag and how many listed notes carry it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagCount {
    pub tag: String,
    pub count: usize,
}

/// A conflict copy and the note it was copied from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictCopy {
    pub copy: PathBuf,
    pub original: PathBuf,
    /// Whether the original file is still there.
    pub original_exists: bool,
    pub kind: ConflictKind,
    pub marker: String,
}

impl Listing {
    /// Every tag, case-insensitively merged (the first spelling met wins), sorted.
    pub fn tags(&self) -> Vec<TagCount> {
        let mut tags: BTreeMap<String, TagCount> = BTreeMap::new();
        for tag in self.notes.iter().flat_map(|n| &n.tags) {
            tags.entry(tag.to_lowercase())
                .or_insert_with(|| TagCount {
                    tag: tag.clone(),
                    count: 0,
                })
                .count += 1;
        }
        tags.into_values().collect()
    }

    /// Every conflict copy, with the note it belongs to.
    pub fn conflicts(&self) -> Vec<ConflictCopy> {
        self.notes
            .iter()
            .filter_map(|entry| {
                let conflict = entry.conflict.as_ref()?;
                let original = entry.path.with_file_name(&conflict.original_name);
                Some(ConflictCopy {
                    copy: entry.path.clone(),
                    original_exists: original.is_file(),
                    original,
                    kind: conflict.kind,
                    marker: conflict.marker.clone(),
                })
            })
            .collect()
    }
}

/// Normalises tags: spaces trimmed and collapsed, empty ones dropped, case-insensitive
/// duplicates removed (the first spelling is kept), order kept.
pub fn normalize_tags(tags: &[String]) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    tags.iter()
        .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|t| !t.is_empty() && seen.insert(t.to_lowercase()))
        .collect()
}

/// A note read from the library, with the stamp to pass back when saving it.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedNote {
    pub path: PathBuf,
    pub file: NoteFile,
    pub stamp: FileStamp,
}

/// Where a note was written, and its new stamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedNote {
    pub path: PathBuf,
    pub stamp: FileStamp,
}

/// A library folder.
#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
}

impl Library {
    /// Opens the library at `root`, which must have been [`init`]ialised.
    pub fn open(root: impl Into<PathBuf>) -> Result<Library, LibraryError> {
        let root = root.into();
        if !is_library(&root) {
            return Err(LibraryError::NotALibrary { path: root });
        }
        migrate_legacy_meta_dir(&root).map_err(io_error(&root))?;
        Ok(Library { root })
    }

    /// The library folder.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// `_ardoise/` inside the library.
    pub fn meta_dir(&self) -> PathBuf {
        self.root.join(META_DIR)
    }

    /// Absolute folder of a binder given relative to the root (empty = the root). Each part
    /// must be a valid, visible folder name.
    pub fn binder_dir(&self, binder: &Path) -> Result<PathBuf, LibraryError> {
        let mut dir = self.root.clone();
        for component in binder.components() {
            let Component::Normal(part) = component else {
                return Err(LibraryError::InvalidName {
                    name: binder.display().to_string(),
                    reason: "a binder is a path relative to the library, without ..",
                });
            };
            let part = part.to_str().ok_or_else(|| LibraryError::InvalidName {
                name: binder.display().to_string(),
                reason: "the name is not valid Unicode",
            })?;
            if let Some(reason) = invalid_name_reason(part) {
                return Err(LibraryError::InvalidName {
                    name: part.to_owned(),
                    reason,
                });
            }
            dir.push(part);
        }
        Ok(dir)
    }

    /// Checks that `path` (absolute, or relative to the root) is a note file of this library,
    /// outside hidden folders and `_ardoise/`, and returns it absolute.
    pub fn note_path(&self, path: &Path) -> Result<PathBuf, LibraryError> {
        let not_in_library = || LibraryError::NotInLibrary {
            path: path.to_path_buf(),
        };
        let relative = if path.is_absolute() {
            path.strip_prefix(&self.root)
                .map_err(|_| not_in_library())?
        } else {
            path
        };
        let mut parts = Vec::new();
        for component in relative.components() {
            match component {
                Component::Normal(part) => {
                    let part = part.to_str().ok_or_else(not_in_library)?;
                    if walk::is_skipped_name(part) {
                        return Err(not_in_library());
                    }
                    parts.push(part);
                }
                _ => return Err(not_in_library()),
            }
        }
        match parts.last() {
            Some(name) if names::is_note_file_name(name) => {}
            _ => return Err(not_in_library()),
        }
        Ok(parts.iter().fold(self.root.clone(), |p, part| p.join(part)))
    }

    /// Lists every note, reading only each file's `note.json`. Files that cannot be read are
    /// reported in [`Listing::problems`] and do not stop the listing.
    pub fn list(&self) -> Listing {
        self.list_with(&mut HeaderCache::default())
    }

    /// [`list`](Self::list), reusing the header of every note whose file did not change since
    /// `cache` saw it (same path and stamp): opening a note's archive is what costs on a
    /// synced folder with a thousand notes. The cache then holds exactly the notes listed.
    pub fn list_with(&self, cache: &mut HeaderCache) -> Listing {
        let mut found = Vec::new();
        let mut binders = Vec::new();
        let walk_problems = walk::walk(&self.root, |f| match f {
            walk::Found::Note {
                path,
                binder,
                stamp,
            } => found.push((path, binder, stamp)),
            walk::Found::Binder(binder) => binders.push(binder),
        });
        let mut listing = Listing::default();
        for (path, error) in walk_problems {
            listing.problems.push(ListProblem {
                error: LibraryError::Io {
                    path: path.clone(),
                    source: error,
                },
                path,
            });
        }
        let mut kept = HashMap::with_capacity(found.len());
        for (path, binder, stamp) in found {
            let cached = stamp.and_then(|stamp| {
                let entry = cache.entries.remove(&path)?;
                (entry.stamp == stamp && entry.binder == binder).then_some(entry)
            });
            let entry = match cached {
                Some(entry) => Ok(entry),
                None => read_entry(&path, binder),
            };
            match entry {
                Ok(entry) => {
                    kept.insert(path, entry.clone());
                    listing.notes.push(entry);
                }
                Err(error) => listing.problems.push(ListProblem { path, error }),
            }
        }
        cache.entries = kept;
        binders.sort();
        listing.binders = binders;
        listing.notes.sort_by(|a, b| a.path.cmp(&b.path));
        listing.problems.sort_by(|a, b| a.path.cmp(&b.path));
        listing
    }

    /// Every binder (sub-folder), relative to the root, sorted; empty ones included.
    pub fn binders(&self) -> Vec<PathBuf> {
        let mut binders = Vec::new();
        walk::walk(&self.root, |f| {
            if let walk::Found::Binder(binder) = f {
                binders.push(binder);
            }
        });
        binders.sort();
        binders
    }

    /// Creates a binder (and its parents) if needed, and returns its folder.
    pub fn create_binder(&self, binder: &Path) -> Result<PathBuf, LibraryError> {
        let dir = self.binder_dir(binder)?;
        fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        Ok(dir)
    }

    /// Every tag of every note, with counts.
    pub fn all_tags(&self) -> Vec<TagCount> {
        self.list().tags()
    }

    /// Reads a whole note, with the stamp to pass back to [`save`](Self::save).
    pub fn read(&self, path: &Path) -> Result<LoadedNote, LibraryError> {
        self.read_with(path, |_| {})
    }

    /// [`read`](Self::read), calling `after_read` between reading the file and checking that
    /// it did not change meanwhile (tests use it to replace the file at that moment).
    fn read_with(
        &self,
        path: &Path,
        mut after_read: impl FnMut(&Path),
    ) -> Result<LoadedNote, LibraryError> {
        const ATTEMPTS: u32 = 3;
        let path = self.note_path(path)?;
        // The stamp must describe the bytes read: retry while the file is replaced meanwhile.
        let mut attempt = 1;
        loop {
            let before = self.stamp_of(&path)?;
            let file = format::read_note(&path).map_err(format_error(&path))?;
            after_read(&path);
            let after = self.stamp_of(&path)?;
            if before == after || attempt == ATTEMPTS {
                // Still changing at the last attempt: the bytes read belong to the version of
                // `before` or to a later one, never to an earlier one. `before` is the stamp a
                // save may safely compare with: if the file is no longer exactly that version,
                // the save is refused as a conflict rather than overwriting what came since.
                return Ok(LoadedNote {
                    path,
                    file,
                    stamp: before,
                });
            }
            attempt += 1;
        }
    }

    fn stamp_of(&self, path: &Path) -> Result<FileStamp, LibraryError> {
        FileStamp::of(path).map_err(|source| match source.kind() {
            io::ErrorKind::NotFound => LibraryError::NotFound {
                path: path.to_path_buf(),
            },
            _ => LibraryError::Io {
                path: path.to_path_buf(),
                source,
            },
        })
    }

    fn check_stamp(&self, path: &Path, expected: &FileStamp) -> Result<(), LibraryError> {
        if self.stamp_of(path)? == *expected {
            Ok(())
        } else {
            Err(LibraryError::Conflict {
                path: path.to_path_buf(),
            })
        }
    }

    /// Creates a new note file `YYYY-MM-DD Title.ardoise` in `binder` (created if needed),
    /// adding ` (2)`, ` (3)`… if the name is taken. Never replaces an existing file.
    pub fn create(&self, binder: &Path, file: &NoteFile) -> Result<SavedNote, LibraryError> {
        let dir = self.binder_dir(binder)?;
        let stem = note_file_stem(file.note.created, &file.note.title);
        let first = note_path(&dir, &stem);
        // Encoded first: an invalid note leaves no trace, not even an empty binder.
        let bytes = format::to_bytes(file).map_err(format_error(&first))?;
        fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        for n in 1..=MAX_SUFFIX {
            let path = note_path(&dir, &with_suffix(&stem, n));
            if exists(&path) {
                continue;
            }
            match write_atomic_new(&path, &bytes) {
                Ok(metadata) => {
                    let stamp = FileStamp::from_metadata(&metadata).map_err(io_error(&path))?;
                    return Ok(SavedNote { path, stamp });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(io_error(&path)(e)),
            }
        }
        Err(LibraryError::NoFreeName { path: first })
    }

    /// Saves a note over its file, unless the file changed since it was loaded with
    /// `expected` ([`LibraryError::Conflict`]) or has disappeared
    /// ([`LibraryError::NotFound`]). Returns the new stamp. The file name is not changed:
    /// use [`retitle`](Self::retitle) for that.
    ///
    /// The file's stamp is checked before anything is written, then again once the new
    /// version is completely written next to it, right before every attempt to rename it over
    /// the note (the rename waits while a sync client or an antivirus holds the file open). A
    /// version brought by Nextcloud meanwhile is therefore left untouched; only the instant
    /// between the last check and the rename system call stays uncovered.
    pub fn save(
        &self,
        path: &Path,
        file: &NoteFile,
        expected: &FileStamp,
    ) -> Result<FileStamp, LibraryError> {
        self.save_with(path, file, expected, || {})
    }

    /// [`save`](Self::save), calling `before_check` right before each check made once the new
    /// version is written (tests use it to land another version at the worst moment).
    fn save_with(
        &self,
        path: &Path,
        file: &NoteFile,
        expected: &FileStamp,
        mut before_check: impl FnMut(),
    ) -> Result<FileStamp, LibraryError> {
        let path = self.note_path(path)?;
        let bytes = format::to_bytes(file).map_err(format_error(&path))?;
        // A save that is refused anyway writes nothing, not even a temporary file.
        self.check_stamp(&path, expected)?;
        let metadata = write_atomic_checked(&path, &bytes, || {
            before_check();
            self.check_stamp(&path, expected)
        })
        .map_err(|error| match error {
            CheckedWriteError::Refused(error) => error,
            CheckedWriteError::Io(source) => io_error(&path)(source),
        })?;
        FileStamp::from_metadata(&metadata).map_err(io_error(&path))
    }

    /// Reads a note, applies `change` to it, and writes it back if the file has not changed
    /// meanwhile (nor since `expected`, when given).
    fn rewrite(
        &self,
        path: &Path,
        expected: Option<&FileStamp>,
        change: impl FnOnce(&mut NoteFile),
    ) -> Result<(NoteFile, FileStamp), LibraryError> {
        let loaded = self.read(path)?;
        if let Some(expected) = expected
            && loaded.stamp != *expected
        {
            return Err(LibraryError::Conflict { path: loaded.path });
        }
        let mut file = loaded.file;
        change(&mut file);
        file.note.modified = now_ms().max(file.note.modified);
        let stamp = self.save(&loaded.path, &file, &loaded.stamp)?;
        Ok((file, stamp))
    }

    /// Changes a note's title and renames its file to match (`YYYY-MM-DD New title.ardoise`,
    /// with a ` (n)` suffix if taken).
    pub fn retitle(
        &self,
        path: &Path,
        title: &str,
        expected: Option<&FileStamp>,
    ) -> Result<SavedNote, LibraryError> {
        let path = self.note_path(path)?;
        let (file, stamp) = self.rewrite(&path, expected, |file| {
            file.note.title = title.to_owned();
        })?;
        let desired = note_file_stem(file.note.created, &file.note.title);
        let current_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let current = note_stem(current_name);
        if stem_matches(current, &desired) {
            return Ok(SavedNote { path, stamp });
        }
        let dir = path.parent().unwrap_or(&self.root).to_path_buf();
        let case_only = current.to_lowercase() == desired.to_lowercase();
        let target = note_path(&dir, &desired);
        let renamed = if case_only && rename_no_replace(&path, &target).is_ok() {
            // Only the case changed: for Windows it is the same name, renamed in place.
            Ok(target)
        } else {
            self.move_unique(&path, &dir, &desired)
        };
        Ok(SavedNote {
            path: renamed_or_kept(&path, renamed)?,
            stamp,
        })
    }

    /// Moves `from` into `dir` as `stem.ardoise`, or `stem (n).ardoise` if taken. Never
    /// replaces a file.
    fn move_unique(&self, from: &Path, dir: &Path, stem: &str) -> Result<PathBuf, LibraryError> {
        for n in 1..=MAX_SUFFIX {
            let target = note_path(dir, &with_suffix(stem, n));
            if exists(&target) {
                continue;
            }
            match rename_no_replace(from, &target) {
                Ok(()) => return Ok(target),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) if e.kind() == io::ErrorKind::NotFound && !exists(from) => {
                    return Err(LibraryError::NotFound {
                        path: from.to_path_buf(),
                    });
                }
                Err(e) => return Err(io_error(from)(e)),
            }
        }
        Err(LibraryError::NoFreeName {
            path: note_path(dir, stem),
        })
    }

    /// Moves a note to another binder (created if needed), keeping its file name (with a
    /// ` (n)` suffix if taken there). The file is not rewritten, so its stamp stays valid.
    pub fn move_to(&self, path: &Path, binder: &Path) -> Result<PathBuf, LibraryError> {
        let path = self.note_path(path)?;
        if !path.is_file() {
            return Err(LibraryError::NotFound { path });
        }
        let dir = self.create_binder(binder)?;
        // Windows folder names ignore case: "classe" is the binder "Classe".
        let same_folder = path.parent().is_some_and(|parent| {
            parent.to_string_lossy().to_lowercase() == dir.to_string_lossy().to_lowercase()
        });
        if same_folder {
            return Ok(path);
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        self.move_unique(&path, &dir, note_stem(name))
    }

    /// Replaces a note's tags (normalised by [`normalize_tags`]). Returns the new stamp.
    pub fn set_tags(
        &self,
        path: &Path,
        tags: &[String],
        expected: Option<&FileStamp>,
    ) -> Result<FileStamp, LibraryError> {
        let tags = normalize_tags(tags);
        let (_, stamp) = self.rewrite(path, expected, |file| file.note.tags = tags)?;
        Ok(stamp)
    }

    /// Marks or unmarks a note as a favourite. Returns the new stamp.
    pub fn set_favorite(
        &self,
        path: &Path,
        favorite: bool,
        expected: Option<&FileStamp>,
    ) -> Result<FileStamp, LibraryError> {
        let (_, stamp) = self.rewrite(path, expected, |file| file.note.favorite = favorite)?;
        Ok(stamp)
    }
}

fn read_entry(path: &Path, binder: PathBuf) -> Result<NoteEntry, LibraryError> {
    let stamp = FileStamp::of(path).map_err(io_error(path))?;
    let header = format::read_header(path).map_err(format_error(path))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_owned();
    let note = header.note;
    Ok(NoteEntry {
        path: path.to_path_buf(),
        binder,
        conflict: parse_conflict_name(&file_name),
        file_name,
        id: note.id,
        title: note.title,
        created: note.created,
        modified: note.modified,
        tags: note.tags,
        favorite: note.favorite,
        stamp,
    })
}

/// The path of a note whose new title is saved, once its file was (or was not) renamed to match.
/// A rename that failed (a sync client holds the file, a permission) leaves the file under its
/// old name: the title inside the file is right, the name catches up at the next retitle, and
/// the caller still gets the stamp of the save it must give back at its next one (an error would
/// lose it, and the editor's next save would be refused against its own write). A note that
/// vanished meanwhile is an error.
fn renamed_or_kept(
    path: &Path,
    renamed: Result<PathBuf, LibraryError>,
) -> Result<PathBuf, LibraryError> {
    match renamed {
        Ok(new_path) => Ok(new_path),
        Err(error @ LibraryError::NotFound { .. }) => Err(error),
        Err(_) => Ok(path.to_path_buf()),
    }
}

#[cfg(test)]
pub(crate) mod tests;
