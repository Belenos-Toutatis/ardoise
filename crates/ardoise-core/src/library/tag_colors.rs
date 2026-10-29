//! Tag colours: `_ardoise/tags.json`, owned by the library (a note only carries tag names).
//!
//! ```json
//! { "version": 1,
//!   "tags": { "physique": { "color": "blue", "changed": 1792843200000 },
//!             "3e b":     { "color": null,   "changed": 1792843300000 } } }
//! ```
//!
//! - A tag is identified by its [`tag_key`]: spaces collapsed, lowercase, like the listing
//!   merges tags case-insensitively. `color` is a palette identifier chosen by the interface.
//! - `null` is a tombstone: a colour removed on one computer beats an older colour that comes
//!   back from a conflict copy made on another one. `changed` is the Unix time of the change.
//! - Two computers changing colours at the same time make Nextcloud keep a conflict copy
//!   (`tags (conflicted copy …).json`). Entries are merged per tag: the most recent `changed`
//!   wins, so no colour is lost silently. Reading merges in memory and never writes; the next
//!   write folds the copies into `tags.json` and deletes them.
//! - Fields this version does not know (top level and per entry) are kept when rewriting.
//! - A file that cannot be understood is skipped when reading, and moved aside (never
//!   deleted) before the next write. A file of a newer version is read as well as possible
//!   and never rewritten.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry as MapEntry;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::conflicts::parse_conflict_name;
use super::walk::is_skipped_name;
use super::{FileStamp, Library, LibraryError, io_error, now_ms};
use crate::fsutil::{
    CheckedWriteError, remove_file_with_retry, rename_no_replace, write_atomic_checked,
};

/// Name of the file in `_ardoise/`.
pub const TAG_COLORS_FILE: &str = "tags.json";

/// Version this code writes.
const VERSION: u64 = 1;
/// Larger files are taken for damaged: the real one is a few kilobytes.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// How many times a write starts again when the file changes under it.
const ATTEMPTS: u32 = 3;
const MAX_COLOR_CHARS: usize = 32;
/// Names tried for a damaged file moved aside.
const MAX_ASIDE_NAMES: u32 = 100;

/// Identifier of a tag in `tags.json`: spaces trimmed and collapsed (as
/// [`normalize_tags`](super::normalize_tags)), then lowercased. Empty for a blank tag.
pub fn tag_key(tag: &str) -> String {
    tag.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Whether `color` is a palette identifier: `^[a-z][a-z0-9-]{0,31}$`.
fn is_valid_color(color: &str) -> bool {
    let mut chars = color.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && color.len() <= MAX_COLOR_CHARS
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct Entry {
    /// `None`: the colour was removed (tombstone).
    #[serde(default)]
    color: Option<String>,
    /// Unix milliseconds.
    #[serde(default)]
    changed: i64,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

impl Entry {
    /// Whether `self` replaces `other` when both describe the same tag: the later change
    /// wins; on a tie a colour beats a tombstone, then the greater identifier (any rule would
    /// do, but the same one on every computer).
    fn beats(&self, other: &Entry) -> bool {
        (self.changed, self.color.as_deref()) > (other.changed, other.color.as_deref())
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TagsFile {
    version: u64,
    #[serde(default)]
    tags: BTreeMap<String, Entry>,
    #[serde(flatten)]
    extra: Map<String, Value>,
}

/// One file as found on disk.
enum Loaded {
    Missing,
    /// Could not be read (permissions, a sync client holding it…).
    Unreadable(io::Error),
    /// Not a tags file, or too large.
    Damaged(FileStamp),
    Readable(TagsFile, FileStamp),
}

/// Reads and parses `path`; the stamp describes the bytes parsed (or the version just before
/// them, if the file kept changing: a write then notices it).
fn load(path: &Path) -> Loaded {
    let stamp_of = |path: &Path| FileStamp::of(path);
    let mut attempt = 1;
    loop {
        let before = match stamp_of(path) {
            Ok(stamp) => stamp,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded::Missing,
            Err(e) => return Loaded::Unreadable(e),
        };
        if before.len > MAX_FILE_BYTES {
            return Loaded::Damaged(before);
        }
        let mut bytes = Vec::new();
        let read =
            fs::File::open(path).and_then(|f| f.take(MAX_FILE_BYTES + 1).read_to_end(&mut bytes));
        match read {
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded::Missing,
            Err(e) => return Loaded::Unreadable(e),
        }
        match stamp_of(path) {
            Ok(after) if after != before && attempt < ATTEMPTS => {
                attempt += 1;
                continue;
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Loaded::Missing,
            Err(e) => return Loaded::Unreadable(e),
        }
        if bytes.len() as u64 > MAX_FILE_BYTES {
            return Loaded::Damaged(before);
        }
        return match serde_json::from_slice::<TagsFile>(&bytes) {
            Ok(file) => Loaded::Readable(file, before),
            Err(_) => Loaded::Damaged(before),
        };
    }
}

/// State of `_ardoise/tags.json` itself.
enum Main {
    Missing,
    Unreadable(io::Error),
    Damaged(FileStamp),
    Readable(FileStamp),
}

/// A conflict copy that was merged, and the version of it that was read.
struct MergedCopy {
    path: PathBuf,
    stamp: FileStamp,
}

/// `tags.json` and its conflict copies, merged.
struct State {
    path: PathBuf,
    main: Main,
    /// Copies that were understood (and so can be deleted once their content is in `tags.json`).
    copies: Vec<MergedCopy>,
    /// First file written by a newer version of Ardoise, if any.
    newer: Option<PathBuf>,
    entries: BTreeMap<String, Entry>,
    extra: Map<String, Value>,
}

impl State {
    fn merge(&mut self, path: &Path, file: TagsFile) {
        if file.version > VERSION && self.newer.is_none() {
            self.newer = Some(path.to_path_buf());
        }
        for (key, entry) in file.tags {
            // A hand-edited key is brought to its canonical form.
            let key = tag_key(&key);
            if key.is_empty() {
                continue;
            }
            match self.entries.entry(key) {
                MapEntry::Vacant(slot) => {
                    slot.insert(entry);
                }
                MapEntry::Occupied(mut slot) => {
                    if entry.beats(slot.get()) {
                        slot.insert(entry);
                    }
                }
            }
        }
        for (name, value) in file.extra {
            self.extra.entry(name).or_insert(value);
        }
    }

    /// Colour of every tag that has one.
    fn colors(&self) -> BTreeMap<String, String> {
        self.entries
            .iter()
            .filter_map(|(key, entry)| {
                let color = entry.color.as_ref().filter(|c| is_valid_color(c))?;
                Some((key.clone(), color.clone()))
            })
            .collect()
    }
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
    /// Reads `tags.json` and every conflict copy of it in `_ardoise/`, and merges them.
    fn read_tag_state(&self) -> Result<State, LibraryError> {
        let dir = self.meta_dir();
        let mut state = State {
            path: dir.join(TAG_COLORS_FILE),
            main: Main::Missing,
            copies: Vec::new(),
            newer: None,
            entries: BTreeMap::new(),
            extra: Map::new(),
        };
        let main_path = state.path.clone();
        state.main = match load(&main_path) {
            Loaded::Missing => Main::Missing,
            Loaded::Unreadable(e) => Main::Unreadable(e),
            Loaded::Damaged(stamp) => Main::Damaged(stamp),
            Loaded::Readable(file, stamp) => {
                state.merge(&main_path, file);
                Main::Readable(stamp)
            }
        };
        let mut names = Vec::new();
        match fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(io_error(&dir))?;
                    let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    if is_skipped_name(&name) || !entry.file_type().is_ok_and(|t| t.is_file()) {
                        continue;
                    }
                    let is_copy = parse_conflict_name(&name)
                        .is_some_and(|c| c.original_name.to_lowercase() == TAG_COLORS_FILE);
                    if is_copy {
                        names.push(name);
                    }
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(io_error(&dir)(e)),
        }
        names.sort();
        for name in names {
            let path = dir.join(name);
            // A copy that cannot be understood is left alone: it is never deleted.
            if let Loaded::Readable(file, stamp) = load(&path) {
                state.merge(&path, file);
                state.copies.push(MergedCopy { path, stamp });
            }
        }
        Ok(state)
    }

    /// Colour id of every tag that has one, keyed by [`tag_key`]. Merges `tags.json` with
    /// every Nextcloud conflict copy of it in `_ardoise/`, in memory: reading never writes.
    /// No file means no colours; damaged files are skipped.
    pub fn tag_colors(&self) -> Result<BTreeMap<String, String>, LibraryError> {
        Ok(self.read_tag_state()?.colors())
    }

    /// Sets (`Some`) or removes (`None`) the colour of `tag`, and returns the colours
    /// afterwards. The change wins over every colour of that tag it replaces, wherever it was
    /// made. `tags.json` and its conflict copies are merged first; the copies are deleted once
    /// their content is safely in `tags.json`.
    ///
    /// Fails with [`LibraryError::InvalidTagColor`] for an identifier that is not
    /// `[a-z][a-z0-9-]{0,31}`, [`LibraryError::InvalidName`] for a blank tag,
    /// [`LibraryError::TagColorsNewer`] when a file comes from a newer version (nothing is
    /// written), and [`LibraryError::Conflict`] when `tags.json` kept changing under the write.
    pub fn set_tag_color(
        &self,
        tag: &str,
        color: Option<&str>,
    ) -> Result<BTreeMap<String, String>, LibraryError> {
        self.set_tag_color_at(tag, color, now_ms())
    }

    /// [`set_tag_color`](Self::set_tag_color) at a given time (Unix milliseconds).
    pub(crate) fn set_tag_color_at(
        &self,
        tag: &str,
        color: Option<&str>,
        now_ms: i64,
    ) -> Result<BTreeMap<String, String>, LibraryError> {
        self.set_tag_color_with(tag, color, now_ms, || {})
    }

    /// [`set_tag_color_at`](Self::set_tag_color_at), calling `before_check` right before each
    /// check made once the new file is written (tests use it to change `tags.json` at the
    /// worst moment).
    fn set_tag_color_with(
        &self,
        tag: &str,
        color: Option<&str>,
        now_ms: i64,
        mut before_check: impl FnMut(),
    ) -> Result<BTreeMap<String, String>, LibraryError> {
        let key = tag_key(tag);
        if key.is_empty() {
            return Err(LibraryError::InvalidName {
                name: tag.to_owned(),
                reason: "the tag is empty",
            });
        }
        if let Some(color) = color
            && !is_valid_color(color)
        {
            return Err(LibraryError::InvalidTagColor {
                color: color.to_owned(),
            });
        }
        let dir = self.meta_dir();
        let path = dir.join(TAG_COLORS_FILE);
        for _ in 0..ATTEMPTS {
            let mut state = self.read_tag_state()?;
            if let Some(path) = state.newer.take() {
                return Err(LibraryError::TagColorsNewer { path });
            }
            let expected = match std::mem::replace(&mut state.main, Main::Missing) {
                Main::Unreadable(e) => return Err(io_error(&path)(e)),
                Main::Missing => None,
                Main::Readable(stamp) => Some(stamp),
                Main::Damaged(stamp) => {
                    if FileStamp::of(&path).ok() != Some(stamp) {
                        // Replaced meanwhile (perhaps by a good version): look again.
                        continue;
                    }
                    set_aside(&path, now_ms)?;
                    None
                }
            };
            let mut entry = state.entries.remove(&key).unwrap_or(Entry {
                color: None,
                changed: i64::MIN,
                extra: Map::new(),
            });
            // Later than the entry it replaces, even if a clock is behind.
            entry.changed = now_ms.max(entry.changed.saturating_add(1));
            entry.color = color.map(str::to_owned);
            state.entries.insert(key.clone(), entry);

            let mut bytes = serde_json::to_vec_pretty(&TagsFile {
                version: VERSION,
                tags: state.entries.clone(),
                extra: state.extra.clone(),
            })
            .map_err(|e| io_error(&path)(io::Error::other(e)))?;
            bytes.push(b'\n');
            fs::create_dir_all(&dir).map_err(io_error(&dir))?;
            let written = write_atomic_checked(&path, &bytes, || {
                before_check();
                main_is_unchanged(&path, expected.as_ref())
            });
            match written {
                Ok(_) => {
                    for copy in &state.copies {
                        // A copy that changed since it was read holds something new.
                        if FileStamp::of(&copy.path).is_ok_and(|s| s == copy.stamp) {
                            let _ = remove_file_with_retry(&copy.path);
                        }
                    }
                    return Ok(state.colors());
                }
                Err(CheckedWriteError::Refused(())) => continue,
                Err(CheckedWriteError::Io(e)) => return Err(io_error(&path)(e)),
            }
        }
        Err(LibraryError::Conflict { path })
    }
}

/// Renames a damaged `tags.json` to `tags.damaged-<ms>.json` (kept for the user, never
/// deleted).
fn set_aside(path: &Path, now_ms: i64) -> Result<(), LibraryError> {
    let mut first = None;
    for n in 1..=MAX_ASIDE_NAMES {
        let name = if n == 1 {
            format!("tags.damaged-{now_ms}.json")
        } else {
            format!("tags.damaged-{now_ms}-{n}.json")
        };
        let target = path.with_file_name(name);
        first.get_or_insert_with(|| target.clone());
        match rename_no_replace(path, &target) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            // Gone meanwhile: the write checks that it is still absent.
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(io_error(path)(e)),
        }
    }
    Err(LibraryError::NoFreeName {
        path: first.unwrap_or_else(|| path.to_path_buf()),
    })
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, SystemTime};

    use super::*;
    use crate::library::init;

    const T0: i64 = 1_792_843_200_000;
    const COPY_NAME: &str = "tags (conflicted copy 2026-10-24 101530).json";

    fn new_library() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path()).unwrap();
        let library = Library::open(dir.path()).unwrap();
        (dir, library)
    }

    fn main_path(library: &Library) -> PathBuf {
        library.meta_dir().join(TAG_COLORS_FILE)
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// A tags file with the given `(tag, colour, changed)` entries, as another computer
    /// would have written it.
    fn write_tags(path: &Path, entries: &[(&str, Option<&str>, i64)]) {
        let tags: Map<String, Value> = entries
            .iter()
            .map(|(tag, color, changed)| {
                (
                    (*tag).to_owned(),
                    serde_json::json!({ "color": color, "changed": changed }),
                )
            })
            .collect();
        let json = serde_json::json!({ "version": 1, "tags": tags });
        fs::write(path, serde_json::to_vec_pretty(&json).unwrap()).unwrap();
    }

    fn colors(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    /// Gives a file another modification time, as a sync of a change would.
    fn touch_later(path: &Path) {
        let later = SystemTime::now() + Duration::from_secs(5);
        fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(later)
            .unwrap();
    }

    #[test]
    fn keys_are_trimmed_collapsed_and_lowercased() {
        assert_eq!(tag_key("  Physique   Chimie "), "physique chimie");
        assert_eq!(tag_key("3e B"), "3e b");
        assert_eq!(tag_key("É\tCOLE"), "é cole");
        assert_eq!(tag_key("  \t "), "");
    }

    #[test]
    fn color_ids_are_validated() {
        for good in ["a", "blue", "dark-blue2", "x-", &"a".repeat(32)] {
            assert!(is_valid_color(good), "{good}");
        }
        for bad in [
            "",
            "Blue",
            "1a",
            "-a",
            "a b",
            "a_b",
            "é",
            "blue!",
            &"a".repeat(33),
        ] {
            assert!(!is_valid_color(bad), "{bad}");
        }
    }

    #[test]
    fn no_file_means_no_colors_and_nothing_is_written() {
        let (_dir, library) = new_library();
        assert!(library.tag_colors().unwrap().is_empty());
        assert!(names_in(&library.meta_dir()).is_empty());
    }

    #[test]
    fn set_get_and_remove() {
        let (_dir, library) = new_library();
        let after = library
            .set_tag_color_at("Physique", Some("blue"), T0)
            .unwrap();
        assert_eq!(after, colors(&[("physique", "blue")]));
        assert_eq!(library.tag_colors().unwrap(), after);

        let text = fs::read_to_string(main_path(&library)).unwrap();
        assert!(text.ends_with("}\n"), "{text:?}");
        let json: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "version": 1,
                "tags": { "physique": { "color": "blue", "changed": T0 } }
            })
        );
        // Pretty-printed: one field per line.
        assert!(text.lines().count() > 5);

        library
            .set_tag_color_at("physique", Some("red"), T0 + 10)
            .unwrap();
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("physique", "red")])
        );

        // Removing leaves a tombstone, and the tag is gone from the map.
        let after = library.set_tag_color_at("PHYSIQUE", None, T0 + 20).unwrap();
        assert!(after.is_empty());
        assert!(library.tag_colors().unwrap().is_empty());
        let json: Value = serde_json::from_slice(&fs::read(main_path(&library)).unwrap()).unwrap();
        assert_eq!(
            json["tags"]["physique"],
            serde_json::json!({ "color": null, "changed": T0 + 20 })
        );
    }

    #[test]
    fn keys_are_normalised() {
        let (_dir, library) = new_library();
        library
            .set_tag_color("  Physique   Chimie ", Some("green"))
            .unwrap();
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("physique chimie", "green")])
        );
        // Setting it through another spelling changes the same entry.
        library
            .set_tag_color("physique CHIMIE", Some("pink"))
            .unwrap();
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("physique chimie", "pink")])
        );
    }

    #[test]
    fn invalid_color_and_empty_tag_are_refused() {
        let (_dir, library) = new_library();
        for bad in ["Blue", "", "red!", "1", &"a".repeat(33)] {
            let error = library.set_tag_color("x", Some(bad)).unwrap_err();
            assert!(
                matches!(error, LibraryError::InvalidTagColor { .. }),
                "{bad}"
            );
            assert_eq!(error.code(), "invalidTagColor");
        }
        for blank in ["", "   ", "\t\n"] {
            let error = library.set_tag_color(blank, Some("blue")).unwrap_err();
            assert!(matches!(error, LibraryError::InvalidName { .. }));
            assert_eq!(error.code(), "invalidName");
        }
        assert!(names_in(&library.meta_dir()).is_empty());
    }

    #[test]
    fn leaves_no_temporary_file() {
        let (_dir, library) = new_library();
        library.set_tag_color("a", Some("blue")).unwrap();
        library.set_tag_color("b", Some("red")).unwrap();
        library.set_tag_color("a", None).unwrap();
        assert_eq!(names_in(&library.meta_dir()), [TAG_COLORS_FILE]);
    }

    #[test]
    fn creates_the_meta_folder_if_needed() {
        let (dir, library) = new_library();
        fs::remove_dir(dir.path().join("_ardoise")).unwrap();
        assert!(library.tag_colors().unwrap().is_empty());
        library.set_tag_color("a", Some("blue")).unwrap();
        assert_eq!(library.tag_colors().unwrap(), colors(&[("a", "blue")]));
    }

    #[test]
    fn a_change_is_always_later_than_the_entry_it_replaces() {
        let (_dir, library) = new_library();
        // The clock is behind the entry already there.
        library.set_tag_color_at("a", Some("blue"), T0).unwrap();
        library
            .set_tag_color_at("a", Some("red"), T0 - 1000)
            .unwrap();
        library.set_tag_color_at("a", None, T0 - 1000).unwrap();
        let json: Value = serde_json::from_slice(&fs::read(main_path(&library)).unwrap()).unwrap();
        assert_eq!(json["tags"]["a"]["changed"], T0 + 2);
        assert!(library.tag_colors().unwrap().is_empty());
    }

    #[test]
    fn a_conflict_copy_is_merged_when_reading() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        write_tags(
            &meta.join(TAG_COLORS_FILE),
            &[("physique", Some("blue"), T0)],
        );
        write_tags(&meta.join(COPY_NAME), &[("chimie", Some("red"), T0 + 5)]);
        let before = names_in(&meta);
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("chimie", "red"), ("physique", "blue")])
        );
        // Reading changes nothing on disk.
        assert_eq!(names_in(&meta), before);
        assert_eq!(before.len(), 2);
    }

    #[test]
    fn a_copy_alone_is_read_too() {
        let (_dir, library) = new_library();
        write_tags(
            &library.meta_dir().join(COPY_NAME),
            &[("chimie", Some("red"), T0)],
        );
        assert_eq!(library.tag_colors().unwrap(), colors(&[("chimie", "red")]));
        // And a write creates tags.json from it.
        library.set_tag_color_at("a", Some("blue"), T0 + 1).unwrap();
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("a", "blue"), ("chimie", "red")])
        );
        assert_eq!(names_in(&library.meta_dir()), [TAG_COLORS_FILE]);
    }

    #[test]
    fn the_latest_change_wins_whichever_file_holds_it() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        write_tags(
            &meta.join(TAG_COLORS_FILE),
            &[
                ("newer-in-copy", Some("blue"), T0),
                ("newer-in-main", Some("blue"), T0 + 10),
            ],
        );
        write_tags(
            &meta.join(COPY_NAME),
            &[
                ("newer-in-copy", Some("red"), T0 + 10),
                ("newer-in-main", Some("red"), T0),
            ],
        );
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("newer-in-copy", "red"), ("newer-in-main", "blue")])
        );
    }

    #[test]
    fn a_newer_removal_beats_an_older_color_and_the_reverse() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        write_tags(
            &meta.join(TAG_COLORS_FILE),
            &[
                ("removed-later", None, T0 + 10),
                ("colored-later", None, T0),
            ],
        );
        write_tags(
            &meta.join(COPY_NAME),
            &[
                ("removed-later", Some("red"), T0),
                ("colored-later", Some("blue"), T0 + 10),
            ],
        );
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("colored-later", "blue")])
        );
    }

    #[test]
    fn ties_are_settled_the_same_way_on_every_computer() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        // A colour beats a tombstone; the greater identifier beats the smaller.
        write_tags(
            &meta.join(TAG_COLORS_FILE),
            &[("t1", None, T0), ("t2", Some("blue"), T0)],
        );
        write_tags(
            &meta.join(COPY_NAME),
            &[("t1", Some("red"), T0), ("t2", Some("amber"), T0)],
        );
        let expected = colors(&[("t1", "red"), ("t2", "blue")]);
        assert_eq!(library.tag_colors().unwrap(), expected);
        // Swapped roles give the same answer.
        let other = tempfile::tempdir().unwrap();
        init(other.path()).unwrap();
        let other = Library::open(other.path()).unwrap();
        write_tags(
            &other.meta_dir().join(TAG_COLORS_FILE),
            &[("t1", Some("red"), T0), ("t2", Some("amber"), T0)],
        );
        write_tags(
            &other.meta_dir().join(COPY_NAME),
            &[("t1", None, T0), ("t2", Some("blue"), T0)],
        );
        assert_eq!(other.tag_colors().unwrap(), expected);
    }

    #[test]
    fn hand_edited_keys_are_normalised_when_merging() {
        let (_dir, library) = new_library();
        write_tags(
            &main_path(&library),
            &[
                ("  Physique  ", Some("blue"), T0),
                ("physique", Some("red"), T0 + 1),
                ("  ", Some("pink"), T0),
            ],
        );
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("physique", "red")])
        );
    }

    #[test]
    fn only_copies_of_tags_json_count() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        write_tags(&meta.join(TAG_COLORS_FILE), &[("a", Some("blue"), T0)]);
        // The same file under another case, still a copy of tags.json.
        write_tags(
            &meta.join("TAGS (conflicted copy 2026-10-24 101530).JSON"),
            &[("b", Some("red"), T0)],
        );
        for ignored in [
            "other (conflicted copy 2026-10-24 101530).json",
            ".tags (conflicted copy 2026-10-24 101530).json",
            "tags (conflicted copy 2026-10-24 101530).json.part",
            "tags.damaged-123.json",
            "tags.json.bak",
        ] {
            write_tags(&meta.join(ignored), &[("ignored", Some("pink"), T0 + 99)]);
        }
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("a", "blue"), ("b", "red")])
        );
        library
            .set_tag_color_at("c", Some("green"), T0 + 1)
            .unwrap();
        // Only the real copy was folded in and deleted.
        assert_eq!(names_in(&meta).len(), 6);
        assert!(
            !meta
                .join("TAGS (conflicted copy 2026-10-24 101530).JSON")
                .exists()
        );
        assert!(
            meta.join("other (conflicted copy 2026-10-24 101530).json")
                .exists()
        );
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("a", "blue"), ("b", "red"), ("c", "green")])
        );
    }

    #[test]
    fn a_write_folds_the_copies_into_tags_json_and_deletes_them() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        write_tags(
            &meta.join(TAG_COLORS_FILE),
            &[("physique", Some("blue"), T0)],
        );
        write_tags(&meta.join(COPY_NAME), &[("chimie", Some("red"), T0 + 5)]);
        write_tags(
            &meta.join("tags (conflicted copy 2026-10-25 080000).json"),
            &[("maths", Some("green"), T0 + 6), ("physique", None, T0 + 7)],
        );

        let after = library
            .set_tag_color_at("svt", Some("pink"), T0 + 100)
            .unwrap();
        let expected = colors(&[("chimie", "red"), ("maths", "green"), ("svt", "pink")]);
        assert_eq!(after, expected);
        assert_eq!(names_in(&meta), [TAG_COLORS_FILE]);
        assert_eq!(library.tag_colors().unwrap(), expected);
        // The removal made in the second copy is kept as a tombstone.
        let json: Value = serde_json::from_slice(&fs::read(main_path(&library)).unwrap()).unwrap();
        assert_eq!(json["tags"]["physique"]["color"], Value::Null);
    }

    #[test]
    fn a_damaged_copy_is_skipped_and_never_deleted() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        write_tags(&meta.join(TAG_COLORS_FILE), &[("a", Some("blue"), T0)]);
        let damaged = meta.join(COPY_NAME);
        fs::write(&damaged, b"{ not json").unwrap();
        let huge = meta.join("tags (conflicted copy 2026-10-25 080000).json");
        fs::write(&huge, vec![b' '; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        let wrong_shape = meta.join("tags (conflicted copy 2026-10-26 080000).json");
        fs::write(&wrong_shape, br#"["a", "b"]"#).unwrap();

        assert_eq!(library.tag_colors().unwrap(), colors(&[("a", "blue")]));
        library.set_tag_color_at("b", Some("red"), T0 + 1).unwrap();
        assert_eq!(fs::read(&damaged).unwrap(), b"{ not json");
        assert!(huge.exists());
        assert_eq!(fs::read(&wrong_shape).unwrap(), br#"["a", "b"]"#);
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("a", "blue"), ("b", "red")])
        );
    }

    #[test]
    fn a_copy_changed_since_it_was_read_is_kept() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        write_tags(&meta.join(TAG_COLORS_FILE), &[("a", Some("blue"), T0)]);
        let copy = meta.join(COPY_NAME);
        write_tags(&copy, &[("b", Some("red"), T0)]);
        let mut calls = 0;
        library
            .set_tag_color_with("c", Some("green"), T0 + 1, || {
                calls += 1;
                // Another version of the copy arrives right before the main file is replaced.
                write_tags(&copy, &[("b", Some("red"), T0), ("d", Some("pink"), T0)]);
                touch_later(&copy);
            })
            .unwrap();
        assert_eq!(calls, 1);
        assert!(copy.exists());
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("a", "blue"), ("b", "red"), ("c", "green"), ("d", "pink")])
        );
    }

    #[test]
    fn a_damaged_main_file_is_moved_aside_then_replaced() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        library.set_tag_color_at("a", Some("blue"), T0).unwrap();
        for (n, content) in [
            (0, b"garbage, not json".to_vec()),
            (1, br#"{"tags": {}}"#.to_vec()),
            (2, vec![b' '; (MAX_FILE_BYTES + 1) as usize]),
        ] {
            fs::write(main_path(&library), &content).unwrap();
            assert!(library.tag_colors().unwrap().is_empty());
            let now = T0 + 1000 + n;
            let after = library.set_tag_color_at("b", Some("red"), now).unwrap();
            assert_eq!(after, colors(&[("b", "red")]));
            assert_eq!(
                fs::read(meta.join(format!("tags.damaged-{now}.json"))).unwrap(),
                content
            );
            assert_eq!(library.tag_colors().unwrap(), after);
        }
        assert_eq!(names_in(&meta).len(), 4);
    }

    #[test]
    fn a_damaged_main_file_still_gets_the_copies_merged() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        fs::write(main_path(&library), b"???").unwrap();
        write_tags(&meta.join(COPY_NAME), &[("chimie", Some("red"), T0)]);
        assert_eq!(library.tag_colors().unwrap(), colors(&[("chimie", "red")]));
        let after = library.set_tag_color_at("a", Some("blue"), T0 + 1).unwrap();
        assert_eq!(after, colors(&[("a", "blue"), ("chimie", "red")]));
        assert_eq!(
            names_in(&meta),
            [
                format!("tags.damaged-{}.json", T0 + 1),
                TAG_COLORS_FILE.to_owned()
            ]
        );
    }

    #[test]
    fn a_name_for_the_damaged_file_is_never_reused() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        fs::write(meta.join(format!("tags.damaged-{T0}.json")), b"earlier").unwrap();
        fs::write(main_path(&library), b"???").unwrap();
        library.set_tag_color_at("a", Some("blue"), T0).unwrap();
        assert_eq!(
            fs::read(meta.join(format!("tags.damaged-{T0}.json"))).unwrap(),
            b"earlier"
        );
        assert_eq!(
            fs::read(meta.join(format!("tags.damaged-{T0}-2.json"))).unwrap(),
            b"???"
        );
    }

    #[test]
    fn a_newer_version_is_read_but_never_written() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        let content = br##"{"version":2,"tags":{"physique":{"color":"blue","changed":5,"gradient":"#fff"}},"theme":"x"}"##;
        fs::write(main_path(&library), content).unwrap();
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("physique", "blue")])
        );

        let error = library.set_tag_color("a", Some("red")).unwrap_err();
        assert!(
            matches!(error, LibraryError::TagColorsNewer { ref path } if *path == main_path(&library))
        );
        assert_eq!(error.code(), "tagColorsNewer");
        assert_eq!(fs::read(main_path(&library)).unwrap(), content);
        assert_eq!(names_in(&meta), [TAG_COLORS_FILE]);

        // A newer copy is refused too: folding it in would drop what it holds.
        fs::write(main_path(&library), br#"{"version":1,"tags":{}}"#).unwrap();
        let copy = meta.join(COPY_NAME);
        fs::write(&copy, content).unwrap();
        let error = library.set_tag_color("a", Some("red")).unwrap_err();
        assert!(matches!(error, LibraryError::TagColorsNewer { ref path } if *path == copy));
        assert_eq!(fs::read(&copy).unwrap(), content);
        assert_eq!(
            fs::read(main_path(&library)).unwrap(),
            br#"{"version":1,"tags":{}}"#
        );
    }

    #[test]
    fn unknown_fields_survive_a_rewrite() {
        let (_dir, library) = new_library();
        let meta = library.meta_dir();
        fs::write(
            main_path(&library),
            serde_json::to_vec(&serde_json::json!({
                "version": 1,
                "palette": "v2",
                "tags": {
                    "physique": { "color": "blue", "changed": 5, "pinned": true },
                    "maths": { "color": "red", "changed": 6, "note": { "a": [1, 2] } }
                }
            }))
            .unwrap(),
        )
        .unwrap();
        // A field only a copy knows.
        fs::write(
            meta.join(COPY_NAME),
            br#"{"version":1,"other":1,"tags":{"svt":{"color":"pink","changed":9,"weight":3}}}"#,
        )
        .unwrap();

        library
            .set_tag_color_at("physique", Some("green"), T0)
            .unwrap();
        library
            .set_tag_color_at("lettres", Some("amber"), T0)
            .unwrap();
        let json: Value = serde_json::from_slice(&fs::read(main_path(&library)).unwrap()).unwrap();
        assert_eq!(json["version"], 1);
        assert_eq!(json["palette"], "v2");
        assert_eq!(json["other"], 1);
        assert_eq!(json["tags"]["physique"]["pinned"], true);
        assert_eq!(json["tags"]["physique"]["color"], "green");
        assert_eq!(
            json["tags"]["maths"]["note"],
            serde_json::json!({ "a": [1, 2] })
        );
        assert_eq!(json["tags"]["svt"]["weight"], 3);
        assert_eq!(json["tags"]["lettres"]["color"], "amber");
    }

    #[test]
    fn keys_are_sorted_in_the_file() {
        let (_dir, library) = new_library();
        for tag in ["zoo", "alpha", "milieu"] {
            library.set_tag_color_at(tag, Some("blue"), T0).unwrap();
        }
        let text = fs::read_to_string(main_path(&library)).unwrap();
        let position = |tag: &str| text.find(&format!("\"{tag}\"")).unwrap();
        assert!(position("alpha") < position("milieu") && position("milieu") < position("zoo"));
    }

    #[test]
    fn a_file_changed_meanwhile_is_read_again_and_merged() {
        let (_dir, library) = new_library();
        let path = main_path(&library);
        library.set_tag_color_at("a", Some("blue"), T0).unwrap();
        let mut calls = 0;
        library
            .set_tag_color_with("b", Some("red"), T0 + 1, || {
                calls += 1;
                if calls == 1 {
                    // Nextcloud brings another computer's version.
                    write_tags(
                        &path,
                        &[("a", Some("blue"), T0), ("c", Some("pink"), T0 + 1)],
                    );
                    touch_later(&path);
                }
            })
            .unwrap();
        assert_eq!(calls, 2);
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("a", "blue"), ("b", "red"), ("c", "pink")])
        );
        assert_eq!(names_in(&library.meta_dir()), [TAG_COLORS_FILE]);
    }

    #[test]
    fn a_file_that_keeps_changing_is_a_conflict_and_is_left_alone() {
        let (_dir, library) = new_library();
        let path = main_path(&library);
        library.set_tag_color_at("a", Some("blue"), T0).unwrap();
        let mut calls = 0;
        let error = library
            .set_tag_color_with("b", Some("red"), T0 + 1, || {
                calls += 1;
                let color = format!("c{calls}");
                write_tags(&path, &[("a", Some(&color), T0 + calls)]);
                touch_later(&path);
            })
            .unwrap_err();
        assert!(matches!(error, LibraryError::Conflict { .. }));
        assert_eq!(error.code(), "noteConflict");
        assert_eq!(calls, i64::from(ATTEMPTS));
        // The version that came in is what stays; no temporary file is left.
        assert_eq!(library.tag_colors().unwrap(), colors(&[("a", "c3")]));
        assert_eq!(names_in(&library.meta_dir()), [TAG_COLORS_FILE]);
    }

    #[test]
    fn a_main_file_appearing_meanwhile_is_not_overwritten() {
        let (_dir, library) = new_library();
        let path = main_path(&library);
        let mut calls = 0;
        library
            .set_tag_color_with("b", Some("red"), T0 + 1, || {
                calls += 1;
                if calls == 1 {
                    write_tags(&path, &[("a", Some("blue"), T0)]);
                }
            })
            .unwrap();
        assert_eq!(
            library.tag_colors().unwrap(),
            colors(&[("a", "blue"), ("b", "red")])
        );
    }
}
