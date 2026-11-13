//! Walking the library folder tree: which folders are binders and which files are notes.

use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::FileStamp;
use super::names::is_note_file_name;

/// Deepest binder nesting explored (protection against pathological trees).
const MAX_DEPTH: usize = 64;

/// What the walk finds.
pub(crate) enum Found {
    /// A binder (folder), relative to the root.
    Binder(PathBuf),
    /// A note file: absolute path, its binder relative to the root, and its stamp as the
    /// directory listing gave it (free on Windows, where the listing carries the times and
    /// sizes; `None` if it could not be read).
    Note {
        path: PathBuf,
        binder: PathBuf,
        stamp: Option<FileStamp>,
    },
    /// An order file of a binder, or a conflict copy of one ([`super::order`]): its folder (the
    /// binder, relative to the root), its absolute path and its stamp as the listing gave it.
    Order {
        binder: PathBuf,
        path: PathBuf,
        stamp: Option<FileStamp>,
    },
}

/// Whether a file or folder name is skipped: the library's own folder `_ardoise/`, hidden
/// names (leading dot, which covers the old `.ardoise/`, the `.part` files of atomic writes,
/// Nextcloud's `.~` downloads and `.sync_*.db`), or a temporary `.part` file.
pub(crate) fn is_skipped_name(name: &str) -> bool {
    name.starts_with('.')
        || name.eq_ignore_ascii_case(super::META_DIR)
        || name.to_ascii_lowercase().ends_with(".part")
}

/// Folders Windows marks hidden or system (`$RECYCLE.BIN`, `System Volume Information`…).
#[cfg(windows)]
fn is_hidden_folder(entry: &DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    const HIDDEN: u32 = 0x2;
    const SYSTEM: u32 = 0x4;
    entry
        .metadata()
        .is_ok_and(|m| m.file_attributes() & (HIDDEN | SYSTEM) != 0)
}

#[cfg(not(windows))]
fn is_hidden_folder(_entry: &DirEntry) -> bool {
    false
}

/// Walks the tree under `root`, calling `visit` for every binder and note, in no particular
/// order. Links and junctions are not followed. Unreadable folders are returned, not fatal.
pub(crate) fn walk(root: &Path, visit: impl FnMut(Found)) -> Vec<(PathBuf, io::Error)> {
    walk_bounded(root, usize::MAX, visit).problems
}

/// What [`walk_bounded`] did.
pub(crate) struct Walked {
    /// Folders that could not be read.
    pub problems: Vec<(PathBuf, io::Error)>,
    /// False when the walk stopped because more than `max_entries` directory entries exist.
    pub complete: bool,
}

/// [`walk`], giving up before visiting more than `max_entries` directory entries (every entry
/// read from a folder counts, skipped ones included).
pub(crate) fn walk_bounded(
    root: &Path,
    max_entries: usize,
    mut visit: impl FnMut(Found),
) -> Walked {
    let mut problems = Vec::new();
    let mut visited = 0usize;
    let mut pending = vec![(PathBuf::new(), 0usize)];
    while let Some((relative, depth)) = pending.pop() {
        let dir = root.join(&relative);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) => {
                problems.push((dir, error));
                continue;
            }
        };
        for entry in entries {
            if visited >= max_entries {
                return Walked {
                    problems,
                    complete: false,
                };
            }
            visited += 1;
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    problems.push((dir.clone(), error));
                    continue;
                }
            };
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                // Not valid Unicode: cannot be one of our names.
                continue;
            };
            if is_skipped_name(&name) {
                continue;
            }
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                if depth < MAX_DEPTH && !is_hidden_folder(&entry) {
                    let child = relative.join(&name);
                    visit(Found::Binder(child.clone()));
                    pending.push((child, depth + 1));
                }
            } else if file_type.is_file() && super::order::is_order_file_name(&name) {
                visit(Found::Order {
                    binder: relative.clone(),
                    path: entry.path(),
                    stamp: entry
                        .metadata()
                        .ok()
                        .and_then(|metadata| FileStamp::from_metadata(&metadata).ok()),
                });
            } else if file_type.is_file() && is_note_file_name(&name) {
                visit(Found::Note {
                    path: entry.path(),
                    binder: relative.clone(),
                    stamp: entry
                        .metadata()
                        .ok()
                        .and_then(|metadata| FileStamp::from_metadata(&metadata).ok()),
                });
            }
        }
    }
    Walked {
        problems,
        complete: true,
    }
}

/// How many note files a folder holds, see [`count_notes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteCount {
    pub notes: usize,
    /// False when the walk stopped at `max_entries` (the folder may hold more).
    pub complete: bool,
}

/// How many note files `folder` holds, as the listing would find them (same skipped names,
/// hidden/system folders and links as the library walk), stopping after `max_entries`
/// directory entries visited. The folder need not be a library; one that cannot be read
/// counts as empty. Used to show what a folder holds before it is chosen as the library.
pub fn count_notes(folder: &Path, max_entries: usize) -> NoteCount {
    let mut notes = 0;
    let walked = walk_bounded(folder, max_entries, |found| {
        if matches!(found, Found::Note { .. }) {
            notes += 1;
        }
    });
    NoteCount {
        notes,
        complete: walked.complete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_notes_and_binders_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for folder in [
            "_ardoise/trash",
            ".ardoise",
            "Classe/3e B",
            "Perso",
            ".cache",
        ] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        for file in [
            "a.ardoise",
            "b.ARDOISE",
            ".hidden.ardoise",
            ".a.ardoise.123.0.part",
            "c.ardoise.part",
            "notes.txt",
            "Classe/3e B/c.ardoise",
            "Perso/d.ardoise",
            "_ardoise/trash/e.ardoise",
            ".ardoise/old.ardoise",
            ".cache/f.ardoise",
        ] {
            fs::write(root.join(file), b"x").unwrap();
        }
        let mut notes = Vec::new();
        let mut binders = Vec::new();
        let problems = walk(root, |found| match found {
            Found::Binder(b) => binders.push(b),
            Found::Note { path, binder, .. } => {
                notes.push((path.strip_prefix(root).unwrap().to_path_buf(), binder))
            }
            // The order files are not notes (they have their own test, in `order.rs`).
            Found::Order { .. } => {}
        });
        assert!(problems.is_empty());
        notes.sort();
        binders.sort();
        assert_eq!(
            notes,
            [
                (
                    PathBuf::from("Classe/3e B/c.ardoise"),
                    PathBuf::from("Classe/3e B")
                ),
                (PathBuf::from("Perso/d.ardoise"), PathBuf::from("Perso")),
                (PathBuf::from("a.ardoise"), PathBuf::new()),
                (PathBuf::from("b.ARDOISE"), PathBuf::new()),
            ]
        );
        assert_eq!(
            binders,
            [
                PathBuf::from("Classe"),
                PathBuf::from("Classe").join("3e B"),
                PathBuf::from("Perso")
            ]
        );
    }

    #[test]
    fn missing_root_is_reported() {
        let dir = tempfile::tempdir().unwrap();
        let problems = walk(&dir.path().join("missing"), |_| panic!("nothing to find"));
        assert_eq!(problems.len(), 1);
    }

    fn tree_for_counting(root: &Path) {
        for folder in ["Classe/3e B", "_ardoise/trash", ".hidden", "Vide"] {
            fs::create_dir_all(root.join(folder)).unwrap();
        }
        for file in [
            "a.ardoise",
            "Classe/b.ARDOISE",
            "Classe/3e B/c.ardoise",
            "Classe/3e B/c.ardoise.part",
            "_ardoise/trash/x.ardoise",
            ".hidden/y.ardoise",
            ".z.ardoise",
            "cours.pdf",
            "Classe/notes.txt",
        ] {
            fs::write(root.join(file), b"x").unwrap();
        }
    }

    #[test]
    fn counts_notes_like_the_listing() {
        let dir = tempfile::tempdir().unwrap();
        tree_for_counting(dir.path());
        let count = count_notes(dir.path(), 1000);
        assert_eq!(
            count,
            NoteCount {
                notes: 3,
                complete: true
            }
        );
        // Same result as the walk the listing uses.
        let mut walked = 0;
        walk(dir.path(), |f| {
            walked += usize::from(matches!(f, Found::Note { .. }))
        });
        assert_eq!(walked, count.notes);
    }

    #[test]
    fn counting_stops_at_the_budget() {
        let dir = tempfile::tempdir().unwrap();
        for n in 0..20 {
            fs::write(dir.path().join(format!("{n}.ardoise")), b"x").unwrap();
        }
        let few = count_notes(dir.path(), 5);
        assert!(!few.complete);
        assert!(few.notes <= 5);
        // Exactly as many entries as the budget: nothing is left out.
        assert_eq!(
            count_notes(dir.path(), 20),
            NoteCount {
                notes: 20,
                complete: true
            }
        );
        assert!(!count_notes(dir.path(), 19).complete);
        assert_eq!(
            count_notes(dir.path(), 0),
            NoteCount {
                notes: 0,
                complete: false
            }
        );
    }

    #[test]
    fn counting_a_missing_or_empty_folder() {
        let dir = tempfile::tempdir().unwrap();
        let empty = NoteCount {
            notes: 0,
            complete: true,
        };
        assert_eq!(count_notes(dir.path(), 10), empty);
        assert_eq!(count_notes(&dir.path().join("missing"), 10), empty);
    }

    #[test]
    fn skipped_names() {
        assert!(is_skipped_name(".ardoise"));
        assert!(is_skipped_name("_ardoise"));
        assert!(is_skipped_name("_Ardoise"));
        assert!(!is_skipped_name("_ardoise notes"));
        assert!(is_skipped_name(".x.ardoise.42.0.part"));
        assert!(is_skipped_name("x.ardoise.PART"));
        assert!(is_skipped_name(".sync_abc.db"));
        assert!(!is_skipped_name("2026-10-24 Cours.ardoise"));
        assert!(!is_skipped_name("Classe"));
    }
}
