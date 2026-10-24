//! Walking the library folder tree: which folders are binders and which files are notes.

use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};

use super::names::is_note_file_name;

/// Deepest binder nesting explored (protection against pathological trees).
const MAX_DEPTH: usize = 64;

/// What the walk finds.
pub(crate) enum Found {
    /// A binder (folder), relative to the root.
    Binder(PathBuf),
    /// A note file: absolute path and its binder relative to the root.
    Note { path: PathBuf, binder: PathBuf },
}

/// Whether a file or folder name is skipped: hidden (leading dot, which covers `.ardoise/`,
/// the `.part` files of atomic writes, Nextcloud's `.~` downloads and `.sync_*.db`), or a
/// temporary `.part` file.
pub(crate) fn is_skipped_name(name: &str) -> bool {
    name.starts_with('.') || name.to_ascii_lowercase().ends_with(".part")
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
pub(crate) fn walk(root: &Path, mut visit: impl FnMut(Found)) -> Vec<(PathBuf, io::Error)> {
    let mut problems = Vec::new();
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
            } else if file_type.is_file() && is_note_file_name(&name) {
                visit(Found::Note {
                    path: entry.path(),
                    binder: relative.clone(),
                });
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_notes_and_binders_and_skips_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for folder in [".ardoise/trash", "Classe/3e B", "Perso", ".cache"] {
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
            ".ardoise/trash/e.ardoise",
            ".cache/f.ardoise",
        ] {
            fs::write(root.join(file), b"x").unwrap();
        }
        let mut notes = Vec::new();
        let mut binders = Vec::new();
        let problems = walk(root, |found| match found {
            Found::Binder(b) => binders.push(b),
            Found::Note { path, binder } => {
                notes.push((path.strip_prefix(root).unwrap().to_path_buf(), binder))
            }
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

    #[test]
    fn skipped_names() {
        assert!(is_skipped_name(".ardoise"));
        assert!(is_skipped_name(".x.ardoise.42.0.part"));
        assert!(is_skipped_name("x.ardoise.PART"));
        assert!(is_skipped_name(".sync_abc.db"));
        assert!(!is_skipped_name("2026-10-24 Cours.ardoise"));
        assert!(!is_skipped_name("Classe"));
    }
}
