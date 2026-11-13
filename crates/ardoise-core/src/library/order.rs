//! The free order of a binder (SPEC §4.3, `docs/design/library-order.md`): a small file in the
//! binder's own folder, `_ardoise-order.json`, which follows the sync with the notes.
//!
//! ```json
//! { "version": 1,
//!   "changed": 1792843200000,
//!   "notes": ["6f0c9a52-3d7e-4f0a-9a43-1c2b5e8d7f10", "…"] }
//! ```
//!
//! - Notes are identified by their id (`note.json`), never by file name: renaming a note
//!   (retitle, change of date) keeps its place. This file lists the notes of **this** binder
//!   only; each binder has its own, and a parent binder is shown by the interface as its own
//!   notes first, then those of each sub-binder in that sub-binder's order.
//! - The file is not a note and not hidden (a name starting with a dot is not synchronised by
//!   Nextcloud's default settings): the listing carries its content ([`Listing::orders`]), the
//!   note list never shows it, and the watcher reports its changes ([`LibraryChange::OrderChanged`]).
//! - What is not in the file (a note that arrived, a note whose id is unknown) is placed by the
//!   interface at the end, by date; what is in the file but is not (or no longer) a note of the
//!   binder is ignored. A file that is missing, damaged or too large means "no order": the
//!   listing never fails because of it.
//! - Two computers writing it at once make Nextcloud keep a conflict copy
//!   (`_ardoise-order (conflicted copy …).json`). The file with the greatest `changed` (the
//!   writer's clock, Unix milliseconds) is the one used, the main file winning a tie, then the
//!   later modification time: no note is lost, since a note the winner does not list is placed at
//!   the end. Reading never writes; the next write is newer than every file it read, and deletes
//!   the copies it read.
//! - Fields this version does not know are kept when the file is rewritten. A file of a newer
//!   `version` is read as well as possible and never rewritten ([`LibraryError::OrderNewer`]).
//! - Every write is atomic and checked against the file it replaces, as the tag colours'.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::conflicts::parse_conflict_name;
use super::tag_colors::{Loaded, load};
use super::walk::is_skipped_name;
use super::{FileStamp, Library, LibraryError, io_error, now_ms};
use crate::fsutil::{CheckedWriteError, remove_file_with_retry, write_atomic_checked};

/// Name of the order file in a binder's folder.
pub const ORDER_FILE: &str = "_ardoise-order.json";

/// Version this code writes.
const VERSION: u64 = 1;
/// Larger files are taken for damaged: one id is about 40 bytes, so this is a hundred thousand
/// notes in one binder.
const MAX_FILE_BYTES: u64 = 8 * 1024 * 1024;
/// Most notes one order holds; the rest of a longer list is dropped (they go at the end).
pub const MAX_ORDERED_NOTES: usize = 100_000;
/// Longest note id kept (a UUID is 36 characters).
const MAX_ID_CHARS: usize = 200;
/// How many times a write starts again when the file changes under it.
const ATTEMPTS: u32 = 3;

#[derive(Debug, Serialize, Deserialize)]
struct OrderFile {
    version: u64,
    /// Unix milliseconds of the write; the greatest wins between a file and its conflict copies.
    #[serde(default)]
    changed: i64,
    #[serde(default)]
    notes: Vec<String>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// Whether `name` is an order file (or the conflict copy of one): `_ardoise-order.json`, or
/// `_ardoise-order (conflicted copy …).json` and the other wordings of a sync client.
pub fn is_order_file_name(name: &str) -> bool {
    name.eq_ignore_ascii_case(ORDER_FILE)
        || parse_conflict_name(name)
            .is_some_and(|conflict| conflict.original_name.eq_ignore_ascii_case(ORDER_FILE))
}

/// The ids of `ids` that can be kept: not empty, not too long, the first of each, at most
/// [`MAX_ORDERED_NOTES`].
fn clean_ids(ids: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut seen = HashSet::new();
    ids.into_iter()
        .filter(|id| {
            !id.is_empty() && id.chars().count() <= MAX_ID_CHARS && seen.insert(id.clone())
        })
        .take(MAX_ORDERED_NOTES)
        .collect()
}

/// State of `_ardoise-order.json` itself.
enum Main {
    Missing,
    Damaged(FileStamp),
    Readable(FileStamp),
}

/// A conflict copy that was read, with the version that was read.
struct ReadCopy {
    path: PathBuf,
    stamp: FileStamp,
}

/// The file that wins: the order to use.
struct Winner {
    /// `(changed, is the main file, modification time)`: the greater wins.
    rank: (i64, bool, SystemTime),
    notes: Vec<String>,
}

/// The order file of a folder and its conflict copies, read.
struct State {
    main: Main,
    copies: Vec<ReadCopy>,
    /// First file written by a newer version, if any.
    newer: Option<PathBuf>,
    winner: Option<Winner>,
    extra: Map<String, Value>,
}

impl State {
    fn read(dir: &Path, names: &[String]) -> Result<State, LibraryError> {
        let mut state = State {
            main: Main::Missing,
            copies: Vec::new(),
            newer: None,
            winner: None,
            extra: Map::new(),
        };
        let mut names: Vec<&String> = names.iter().collect();
        names.sort();
        for name in names {
            let path = dir.join(name);
            let is_main = name.eq_ignore_ascii_case(ORDER_FILE);
            match load::<OrderFile>(&path, MAX_FILE_BYTES) {
                Loaded::Missing => {}
                // Held by something else for now: no order this time, and no write.
                Loaded::Unreadable(source) if is_main => return Err(io_error(&path)(source)),
                Loaded::Unreadable(_) => {}
                Loaded::Damaged(stamp) if is_main => state.main = Main::Damaged(stamp),
                Loaded::Damaged(_) => {}
                Loaded::Readable(file, stamp) => {
                    if file.version > VERSION && state.newer.is_none() {
                        state.newer = Some(path.clone());
                    }
                    if file.notes.len() > MAX_ORDERED_NOTES * 2 {
                        continue;
                    }
                    let rank = (file.changed, is_main, stamp.modified);
                    if state.winner.as_ref().is_none_or(|best| rank > best.rank) {
                        state.winner = Some(Winner {
                            rank,
                            notes: clean_ids(file.notes),
                        });
                    }
                    if is_main {
                        state.main = Main::Readable(stamp);
                        state.extra = file.extra;
                    } else {
                        state.copies.push(ReadCopy { path, stamp });
                    }
                }
            }
        }
        Ok(state)
    }
}

/// The order files of one folder named `names` (files found by the walk): the order to use, or
/// `None` when none can be read. Never fails: the listing goes on without an order.
pub(super) fn read_listed(dir: &Path, names: &[String]) -> Option<Vec<String>> {
    let mut state = State::read(dir, names).ok()?;
    state.winner.take().map(|winner| winner.notes)
}

/// The names of the order files in `dir` (the main file and its conflict copies).
fn names_in(dir: &Path) -> Result<Vec<String>, LibraryError> {
    let mut names = Vec::new();
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(io_error(dir))?;
                let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                    continue;
                };
                if !is_skipped_name(&name)
                    && is_order_file_name(&name)
                    && entry.file_type().is_ok_and(|kind| kind.is_file())
                {
                    names.push(name);
                }
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_error(dir)(e)),
    }
    Ok(names)
}

/// Whether `dir` holds nothing but order files (so a binder deleted with its notes can go).
/// Removes them when it does; returns whether the folder is now empty.
pub(super) fn remove_if_only_order_files(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    let mut found = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let name = entry.file_name().to_string_lossy().into_owned();
        if !is_order_file_name(&name) || !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            return false;
        }
        found.push(entry.path());
    }
    found
        .iter()
        .all(|path| remove_file_with_retry(path).is_ok())
}

/// Whether the main file is still the version `expected` (`None`: still absent).
fn main_is_unchanged(path: &Path, expected: Option<&FileStamp>) -> Result<(), ()> {
    match (FileStamp::of(path), expected) {
        (Ok(now), Some(expected)) if now == *expected => Ok(()),
        (Err(e), None) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        _ => Err(()),
    }
}

impl Library {
    /// The order of the notes of `binder` (relative to the library root; empty for the root
    /// itself): note ids, first to last. Empty when there is no file, or none that can be read.
    pub fn binder_order(&self, binder: &Path) -> Vec<String> {
        let Ok(dir) = self.binder_dir(binder) else {
            return Vec::new();
        };
        let Ok(names) = names_in(&dir) else {
            return Vec::new();
        };
        read_listed(&dir, &names).unwrap_or_default()
    }

    /// Stores `ids` as the order of the notes of `binder`, which must exist: the notes of the
    /// binder as they are shown, first to last. Blank, over-long and repeated ids are dropped. The
    /// order files of the folder (the file and its conflict copies) are read first; the copies are
    /// deleted once their content is superseded.
    ///
    /// Fails with [`LibraryError::BinderNotFound`] for a binder that does not exist,
    /// [`LibraryError::OrderNewer`] when the file comes from a newer version (nothing is
    /// written), and [`LibraryError::Conflict`] when it kept changing under the write.
    pub fn set_binder_order(&self, binder: &Path, ids: &[String]) -> Result<(), LibraryError> {
        let ids = clean_ids(ids.iter().cloned());
        self.update_order(binder, |_| Some(ids.clone()))
    }

    /// Puts the note `id` right after `after` in the order of `binder`, when `binder` has an
    /// order that lists `after` (a duplicate goes after its original). With no order, or `after`
    /// unlisted, nothing is written: the copy is then at the end like any note that arrives, and
    /// next to its original, which has the same date.
    pub(super) fn place_after_in_order(
        &self,
        binder: &Path,
        after: &str,
        id: &str,
    ) -> Result<(), LibraryError> {
        self.update_order(binder, |current| {
            let mut ids: Vec<String> = current?.iter().filter(|n| *n != id).cloned().collect();
            let at = ids.iter().position(|n| n == after)?;
            ids.insert(at + 1, id.to_owned());
            Some(ids)
        })
    }

    /// Reads the order of `binder`, lets `change` make the new one (`None`: leave things as
    /// they are), and writes it if the file has not changed meanwhile.
    fn update_order(
        &self,
        binder: &Path,
        change: impl Fn(Option<&Vec<String>>) -> Option<Vec<String>>,
    ) -> Result<(), LibraryError> {
        let dir = self.binder_dir(binder)?;
        if !dir.is_dir() {
            return Err(LibraryError::BinderNotFound {
                path: binder.to_path_buf(),
            });
        }
        let path = dir.join(ORDER_FILE);
        for _ in 0..ATTEMPTS {
            let state = State::read(&dir, &names_in(&dir)?)?;
            if let Some(path) = &state.newer {
                return Err(LibraryError::OrderNewer { path: path.clone() });
            }
            let expected = match &state.main {
                Main::Missing => None,
                Main::Readable(stamp) | Main::Damaged(stamp) => Some(*stamp),
            };
            let current = state.winner.as_ref().map(|winner| &winner.notes);
            let Some(ids) = change(current).map(clean_ids) else {
                return Ok(());
            };
            if current == Some(&ids) && state.copies.is_empty() {
                // Nothing to change: the file already says so.
                return Ok(());
            }
            // Later than every file it supersedes, even if a clock is behind.
            let changed = state.winner.as_ref().map_or(now_ms(), |winner| {
                now_ms().max(winner.rank.0.saturating_add(1))
            });
            let mut bytes = serde_json::to_vec_pretty(&OrderFile {
                version: VERSION,
                changed,
                notes: ids,
                extra: state.extra.clone(),
            })
            .map_err(|e| io_error(&path)(io::Error::other(e)))?;
            bytes.push(b'\n');
            match write_atomic_checked(&path, &bytes, || {
                main_is_unchanged(&path, expected.as_ref())
            }) {
                Ok(_) => {
                    for copy in &state.copies {
                        // A copy that changed since it was read holds something new.
                        if FileStamp::of(&copy.path).is_ok_and(|stamp| stamp == copy.stamp) {
                            let _ = remove_file_with_retry(&copy.path);
                        }
                    }
                    return Ok(());
                }
                Err(CheckedWriteError::Refused(())) => continue,
                Err(CheckedWriteError::Io(e)) => return Err(io_error(&path)(e)),
            }
        }
        Err(LibraryError::Conflict { path })
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::format::NoteFile;
    use crate::library::init;
    use crate::testutil::blank_note;

    fn new_library() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path()).unwrap();
        let library = Library::open(dir.path()).unwrap();
        (dir, library)
    }

    fn note(library: &Library, binder: &str, id: &str, title: &str) -> PathBuf {
        library
            .create(Path::new(binder), &NoteFile::new(blank_note(id, title)))
            .unwrap()
            .path
    }

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| (*id).to_owned()).collect()
    }

    fn order_path(library: &Library, binder: &str) -> PathBuf {
        library
            .binder_dir(Path::new(binder))
            .unwrap()
            .join(ORDER_FILE)
    }

    fn write_order(path: &Path, changed: i64, notes: &[&str]) {
        let json = serde_json::json!({ "version": 1, "changed": changed, "notes": notes });
        fs::write(path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn file_names_of_orders_and_of_their_conflict_copies() {
        for name in [
            ORDER_FILE,
            "_ARDOISE-ORDER.JSON",
            "_ardoise-order (conflicted copy 2026-11-13 101530).json",
            "_ardoise-order (conflicted copy camille 2026-11-13 101530).json",
            "_ardoise-order_conflict-20261113-101530.json",
        ] {
            assert!(is_order_file_name(name), "{name}");
        }
        for name in [
            "order.json",
            "_ardoise-order.json.part",
            "tags.json",
            "2026-11-13 Cours.ardoise",
            "_ardoise-order.txt",
            "_ardoise-order (copie).json",
        ] {
            assert!(!is_order_file_name(name), "{name}");
        }
    }

    #[test]
    fn no_file_means_no_order_and_nothing_is_written() {
        let (dir, library) = new_library();
        note(&library, "Classe", "a", "Cours");
        assert!(library.binder_order(Path::new("Classe")).is_empty());
        assert!(library.list().orders.is_empty());
        assert_eq!(names(&dir.path().join("Classe")).len(), 1);
    }

    #[test]
    fn an_order_is_stored_in_the_binder_and_comes_back_in_the_listing() {
        let (dir, library) = new_library();
        note(&library, "Classe", "a", "Cours");
        note(&library, "Classe/TP", "b", "TP");
        library
            .set_binder_order(Path::new("Classe"), &ids(&["b", "a"]))
            .unwrap();
        assert_eq!(library.binder_order(Path::new("Classe")), ids(&["b", "a"]));
        let listing = library.list();
        assert_eq!(listing.orders.len(), 1);
        assert_eq!(listing.orders[Path::new("Classe")], ids(&["b", "a"]));
        // The file is in the binder's folder, next to the notes, and is never a note.
        assert!(dir.path().join("Classe").join(ORDER_FILE).is_file());
        assert_eq!(listing.notes.len(), 2);
        assert!(listing.problems.is_empty());
        assert_eq!(
            listing.binders,
            [PathBuf::from("Classe"), PathBuf::from("Classe/TP")]
        );
        // The other binders have none.
        assert!(library.binder_order(Path::new("Classe/TP")).is_empty());
        // The text is a small JSON object.
        let value: Value =
            serde_json::from_slice(&fs::read(order_path(&library, "Classe")).unwrap()).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["notes"], serde_json::json!(["b", "a"]));
        assert!(value["changed"].as_i64().unwrap() > 0);
    }

    #[test]
    fn the_root_has_an_order_too() {
        let (dir, library) = new_library();
        library
            .set_binder_order(Path::new(""), &ids(&["x"]))
            .unwrap();
        assert!(dir.path().join(ORDER_FILE).is_file());
        assert_eq!(library.list().orders[Path::new("")], ids(&["x"]));
    }

    #[test]
    fn a_missing_binder_is_refused_and_ids_are_cleaned() {
        let (_dir, library) = new_library();
        let error = library
            .set_binder_order(Path::new("Nulle part"), &ids(&["a"]))
            .unwrap_err();
        assert!(matches!(error, LibraryError::BinderNotFound { .. }));
        assert_eq!(error.code(), "binderNotFound");
        let error = library
            .set_binder_order(Path::new("../dehors"), &ids(&["a"]))
            .unwrap_err();
        assert_eq!(error.code(), "invalidName");

        library.create_binder(Path::new("Classe")).unwrap();
        let long = "x".repeat(MAX_ID_CHARS + 1);
        library
            .set_binder_order(
                Path::new("Classe"),
                &[
                    "a".into(),
                    String::new(),
                    "b".into(),
                    "a".into(),
                    long,
                    "c".into(),
                ],
            )
            .unwrap();
        assert_eq!(
            library.binder_order(Path::new("Classe")),
            ids(&["a", "b", "c"])
        );
    }

    #[test]
    fn a_damaged_file_means_no_order_and_the_listing_still_works() {
        let (_dir, library) = new_library();
        note(&library, "Classe", "a", "Cours");
        let path = order_path(&library, "Classe");
        for content in [
            &b"not json"[..],
            b"",
            b"[1, 2]",
            b"{\"version\": 1, \"notes\": \"a\"}",
            b"{\"notes\": [1, 2]}",
        ] {
            fs::write(&path, content).unwrap();
            let listing = library.list();
            assert!(listing.orders.is_empty(), "{content:?}");
            assert!(listing.problems.is_empty());
            assert_eq!(listing.notes.len(), 1);
            assert!(library.binder_order(Path::new("Classe")).is_empty());
        }
        // A file too large is taken for damaged.
        let huge = vec![b' '; (MAX_FILE_BYTES + 1) as usize];
        fs::write(&path, huge).unwrap();
        assert!(library.list().orders.is_empty());
        // The next write replaces the damaged file.
        library
            .set_binder_order(Path::new("Classe"), &ids(&["a"]))
            .unwrap();
        assert_eq!(library.binder_order(Path::new("Classe")), ids(&["a"]));
    }

    #[test]
    fn unknown_ids_and_unknown_fields_are_tolerated_and_kept() {
        let (_dir, library) = new_library();
        library.create_binder(Path::new("Classe")).unwrap();
        let path = order_path(&library, "Classe");
        let json = serde_json::json!({
            "version": 1, "changed": 5, "notes": ["gone", "a"],
            "futureKey": { "x": [1, 2] }
        });
        fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
        // Ids of notes that do not exist are simply listed (the interface ignores them).
        assert_eq!(
            library.binder_order(Path::new("Classe")),
            ids(&["gone", "a"])
        );
        library
            .set_binder_order(Path::new("Classe"), &ids(&["a", "b"]))
            .unwrap();
        let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["futureKey"], serde_json::json!({ "x": [1, 2] }));
        assert_eq!(value["notes"], serde_json::json!(["a", "b"]));
        assert!(value["changed"].as_i64().unwrap() > 5);
    }

    #[test]
    fn a_file_of_a_newer_version_is_read_and_never_rewritten() {
        let (_dir, library) = new_library();
        library.create_binder(Path::new("Classe")).unwrap();
        let path = order_path(&library, "Classe");
        let json = serde_json::json!({ "version": 9, "changed": 5, "notes": ["a", "b"] });
        let bytes = serde_json::to_vec(&json).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert_eq!(library.binder_order(Path::new("Classe")), ids(&["a", "b"]));
        let error = library
            .set_binder_order(Path::new("Classe"), &ids(&["b", "a"]))
            .unwrap_err();
        assert!(
            matches!(error, LibraryError::OrderNewer { .. }),
            "{error:?}"
        );
        assert_eq!(error.code(), "orderNewer");
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    #[test]
    fn the_most_recent_of_the_file_and_its_conflict_copies_is_used() {
        let (dir, library) = new_library();
        note(&library, "Classe", "a", "Cours");
        library.create_binder(Path::new("Classe")).unwrap();
        let folder = dir.path().join("Classe");
        let main = folder.join(ORDER_FILE);
        let copy = folder.join("_ardoise-order (conflicted copy 2026-11-13 101530).json");
        let older_copy = folder.join("_ardoise-order (conflicted copy 2026-11-12 101530).json");
        write_order(&main, 100, &["a", "b", "c"]);
        write_order(&copy, 200, &["c", "b", "a"]);
        write_order(&older_copy, 50, &["b"]);
        assert_eq!(
            library.binder_order(Path::new("Classe")),
            ids(&["c", "b", "a"])
        );
        let listing = library.list();
        assert_eq!(listing.orders[Path::new("Classe")], ids(&["c", "b", "a"]));
        // The copies never show as notes, nor as conflicts of notes.
        assert_eq!(listing.notes.len(), 1);
        assert!(listing.conflicts().is_empty());
        assert!(listing.problems.is_empty());
        // Reading wrote nothing.
        assert_eq!(names(&folder).len(), 4);

        // The main file wins a tie; a damaged copy is ignored.
        write_order(&copy, 100, &["c", "b", "a"]);
        fs::write(&older_copy, b"garbage").unwrap();
        assert_eq!(
            library.binder_order(Path::new("Classe")),
            ids(&["a", "b", "c"])
        );

        // The next write is newer than the copies it read, and folds them in: the readable
        // copy is deleted, the damaged one (which could be something else) stays.
        library
            .set_binder_order(Path::new("Classe"), &ids(&["b", "a"]))
            .unwrap();
        assert_eq!(library.binder_order(Path::new("Classe")), ids(&["b", "a"]));
        let kept = names(&folder);
        assert!(kept.contains(&ORDER_FILE.to_owned()));
        assert!(
            kept.contains(&"_ardoise-order (conflicted copy 2026-11-12 101530).json".to_owned())
        );
        assert!(!kept.iter().any(|n| n.contains("2026-11-13")), "{kept:?}");
        let value: Value = serde_json::from_slice(&fs::read(&main).unwrap()).unwrap();
        assert!(value["changed"].as_i64().unwrap() > 200 - 1);
    }

    #[test]
    fn a_conflict_copy_alone_is_the_order_when_the_main_file_is_damaged_or_missing() {
        let (dir, library) = new_library();
        library.create_binder(Path::new("Classe")).unwrap();
        let folder = dir.path().join("Classe");
        let copy = folder.join("_ardoise-order (conflicted copy 2026-11-13 101530).json");
        write_order(&copy, 10, &["x", "y"]);
        assert_eq!(library.binder_order(Path::new("Classe")), ids(&["x", "y"]));
        fs::write(folder.join(ORDER_FILE), b"???").unwrap();
        assert_eq!(library.binder_order(Path::new("Classe")), ids(&["x", "y"]));
    }

    #[test]
    fn a_write_that_finds_the_file_changed_meanwhile_starts_again() {
        let (_dir, library) = new_library();
        library.create_binder(Path::new("Classe")).unwrap();
        let path = order_path(&library, "Classe");
        write_order(&path, 1, &["a"]);
        // `update_order` reads, then writes only if the file is the version it read: the closure
        // runs between the two, and replaces the file the first time.
        let replaced = std::cell::Cell::new(false);
        library
            .update_order(Path::new("Classe"), |current| {
                if !replaced.replace(true) {
                    // Another computer's version lands (its modification time is later).
                    write_order(&path, 7, &["z", "a"]);
                    let later = SystemTime::now() + Duration::from_secs(5);
                    fs::File::options()
                        .write(true)
                        .open(&path)
                        .unwrap()
                        .set_modified(later)
                        .unwrap();
                }
                let mut next = current.cloned().unwrap_or_default();
                next.push("n".into());
                Some(next)
            })
            .unwrap();
        // The second attempt saw the new version and added to it.
        assert_eq!(
            library.binder_order(Path::new("Classe")),
            ids(&["z", "a", "n"])
        );
    }

    #[test]
    fn a_duplicate_goes_right_after_its_original() {
        let (_dir, library) = new_library();
        library.create_binder(Path::new("Classe")).unwrap();
        // No order: nothing written.
        library
            .place_after_in_order(Path::new("Classe"), "a", "a2")
            .unwrap();
        assert!(!order_path(&library, "Classe").exists());
        library
            .set_binder_order(Path::new("Classe"), &ids(&["a", "b", "c"]))
            .unwrap();
        library
            .place_after_in_order(Path::new("Classe"), "b", "b2")
            .unwrap();
        assert_eq!(
            library.binder_order(Path::new("Classe")),
            ids(&["a", "b", "b2", "c"])
        );
        // An original that is not listed: nothing changes.
        library
            .place_after_in_order(Path::new("Classe"), "unknown", "u2")
            .unwrap();
        assert_eq!(
            library.binder_order(Path::new("Classe")),
            ids(&["a", "b", "b2", "c"])
        );
        // The copy is never listed twice.
        library
            .place_after_in_order(Path::new("Classe"), "a", "c")
            .unwrap();
        assert_eq!(
            library.binder_order(Path::new("Classe")),
            ids(&["a", "c", "b", "b2"])
        );
    }

    #[test]
    fn a_rename_or_a_move_inside_the_library_does_not_change_the_order_file() {
        let (dir, library) = new_library();
        let path = note(&library, "Classe", "a", "Cours");
        library
            .set_binder_order(Path::new("Classe"), &ids(&["a"]))
            .unwrap();
        let before = fs::read(order_path(&library, "Classe")).unwrap();
        // The id, not the file name, identifies the note: a retitle keeps it.
        let renamed = library.retitle(&path, "Autre titre", None).unwrap();
        assert_ne!(renamed.path, path);
        assert_eq!(fs::read(order_path(&library, "Classe")).unwrap(), before);
        assert_eq!(library.list().notes[0].id, "a");
        assert_eq!(library.binder_order(Path::new("Classe")), ids(&["a"]));
        // A renamed binder takes its order along.
        library
            .rename_binder(Path::new("Classe"), "Cours 3e")
            .unwrap();
        assert_eq!(library.binder_order(Path::new("Cours 3e")), ids(&["a"]));
        assert!(dir.path().join("Cours 3e").join(ORDER_FILE).is_file());
    }

    #[test]
    fn a_write_leaves_no_temporary_file_behind() {
        let (dir, library) = new_library();
        library.create_binder(Path::new("Classe")).unwrap();
        for n in 0..5 {
            library
                .set_binder_order(Path::new("Classe"), &[format!("id-{n}")])
                .unwrap();
        }
        assert_eq!(names(&dir.path().join("Classe")), [ORDER_FILE.to_owned()]);
    }
}
