//! Renaming and trashing binders (folders of the library).
//!
//! A binder is an ordinary folder: renaming one is a folder rename, which does not rewrite the
//! notes inside (their bytes and stamps stay as they were). Deleting one never removes a file
//! outright: its notes go to the trash like any other note, each one remembering its own
//! binder, and only the folders left empty are removed.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::names::exists;
use super::walk::{self, Found};
use super::{Library, LibraryError, ListProblem, TrashItem, invalid_name_reason, io_error};
use crate::fsutil::rename_no_replace;

/// What [`Library::trash_binder`] did.
#[derive(Debug, Default)]
pub struct BinderTrashed {
    /// Notes moved to the trash, in path order.
    pub trashed: Vec<TrashItem>,
    /// Notes that could not be trashed, and folders that could not be read (the other notes
    /// are trashed anyway).
    pub failed: Vec<ListProblem>,
    /// Folders kept because they still hold something (a file that is not a note, a hidden
    /// file, a note that failed, a folder that was kept), relative to the library root,
    /// deepest first.
    pub kept: Vec<PathBuf>,
}

fn path_depth(path: &Path) -> usize {
    path.components().count()
}

impl Library {
    /// Absolute folder of an existing binder: `binder` is not the root, its parts are valid
    /// names, and every level is a real folder (a file, a link or a junction is not one).
    fn existing_binder_dir(&self, binder: &Path) -> Result<PathBuf, LibraryError> {
        if binder.components().next().is_none() {
            return Err(LibraryError::InvalidName {
                name: String::new(),
                reason: "the library itself is not a binder",
            });
        }
        let target = self.binder_dir(binder)?;
        let not_found = || LibraryError::BinderNotFound {
            path: binder.to_path_buf(),
        };
        let mut level = self.root().to_path_buf();
        for component in binder.components() {
            level.push(component);
            match level.symlink_metadata() {
                Ok(metadata) if metadata.is_dir() => {}
                Ok(_) => return Err(not_found()),
                Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(not_found()),
                Err(e) => return Err(io_error(&level)(e)),
            }
        }
        Ok(target)
    }

    /// Renames the binder `binder` (relative path, not empty, e.g. `Classe/3e B`) to
    /// `new_name` (one folder name) in the same parent, and returns the new relative path.
    /// Never replaces an existing folder or file ([`LibraryError::BinderExists`]), even one
    /// that appears meanwhile. A change of case only ("classe" to "Classe") works. The same
    /// name is a no-op. The notes inside are not rewritten.
    pub fn rename_binder(&self, binder: &Path, new_name: &str) -> Result<PathBuf, LibraryError> {
        if let Some(reason) = invalid_name_reason(new_name) {
            return Err(LibraryError::InvalidName {
                name: new_name.to_owned(),
                reason,
            });
        }
        if new_name.contains(['/', '\\']) {
            return Err(LibraryError::InvalidName {
                name: new_name.to_owned(),
                reason: "a binder name is one folder name, without a separator",
            });
        }
        let dir = self.existing_binder_dir(binder)?;
        let current = binder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if current == new_name {
            return Ok(binder.to_path_buf());
        }
        let new_binder = binder.with_file_name(new_name);
        let target = dir.with_file_name(new_name);
        // Windows names ignore case: "classe" is "Classe" itself, so it is not "taken".
        let case_only = current.to_lowercase() == new_name.to_lowercase();
        let taken = || LibraryError::BinderExists {
            path: new_binder.clone(),
        };
        if !case_only && exists(&target) {
            return Err(taken());
        }
        match rename_no_replace(&dir, &target) {
            Ok(()) => Ok(new_binder),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(taken()),
            Err(e) if e.kind() == io::ErrorKind::NotFound && !exists(&dir) => {
                Err(LibraryError::BinderNotFound {
                    path: binder.to_path_buf(),
                })
            }
            Err(e) => Err(io_error(&dir)(e)),
        }
    }

    /// Moves every note under `binder` (sub-binders included) to the trash with
    /// [`trash`](Self::trash), so that each one can be restored to its own binder, then
    /// removes the folders left empty, deepest first. Only empty folders are removed: a file
    /// that is not a note, a hidden file, or a note that could not be trashed keeps its folder
    /// (and the folders above it), and links and junctions are never followed.
    pub fn trash_binder(&self, binder: &Path) -> Result<BinderTrashed, LibraryError> {
        let dir = self.existing_binder_dir(binder)?;
        let mut notes = Vec::new();
        // Relative to `dir`; the binder itself is the empty path.
        let mut folders = vec![PathBuf::new()];
        let problems = walk::walk(&dir, |found| match found {
            Found::Note { path, .. } => notes.push(path),
            Found::Binder(folder) => folders.push(folder),
        });
        notes.sort();
        let mut result = BinderTrashed::default();
        for (path, source) in problems {
            result.failed.push(ListProblem {
                error: LibraryError::Io {
                    path: path.clone(),
                    source,
                },
                path,
            });
        }
        for path in notes {
            match self.trash(&path) {
                Ok(item) => result.trashed.push(item),
                Err(error) => result.failed.push(ListProblem { path, error }),
            }
        }
        folders.sort_by(|a, b| path_depth(b).cmp(&path_depth(a)).then_with(|| a.cmp(b)));
        for folder in folders {
            // `remove_dir` only removes an empty folder (or a link to one, never its target).
            match fs::remove_dir(dir.join(&folder)) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(_) if folder.as_os_str().is_empty() => result.kept.push(binder.to_path_buf()),
                Err(_) => result.kept.push(binder.join(&folder)),
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::NoteFile;
    use crate::library::{FileStamp, init};
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

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn renames_a_binder_and_its_notes_follow_untouched() {
        let (dir, library) = new_library();
        let a = note(&library, "Classe/3e B", "a", "Cours");
        let b = note(&library, "Classe/3e B/TP", "b", "TP");
        let bytes = (fs::read(&a).unwrap(), fs::read(&b).unwrap());
        let stamps = (FileStamp::of(&a).unwrap(), FileStamp::of(&b).unwrap());

        let renamed = library
            .rename_binder(Path::new("Classe/3e B"), "3e C")
            .unwrap();
        assert_eq!(renamed, Path::new("Classe/3e C"));
        assert!(!dir.path().join("Classe/3e B").exists());

        let listing = library.list();
        assert!(listing.problems.is_empty());
        let found: Vec<_> = listing
            .notes
            .iter()
            .map(|n| (n.id.clone(), n.binder.clone(), n.stamp))
            .collect();
        assert_eq!(
            found,
            [
                ("a".to_owned(), PathBuf::from("Classe/3e C"), stamps.0),
                ("b".to_owned(), PathBuf::from("Classe/3e C/TP"), stamps.1),
            ]
        );
        let new_a = dir.path().join("Classe/3e C").join(a.file_name().unwrap());
        let new_b = dir
            .path()
            .join("Classe/3e C/TP")
            .join(b.file_name().unwrap());
        assert_eq!(fs::read(new_a).unwrap(), bytes.0);
        assert_eq!(fs::read(new_b).unwrap(), bytes.1);
        assert_eq!(
            library.binders(),
            [
                PathBuf::from("Classe"),
                PathBuf::from("Classe/3e C"),
                PathBuf::from("Classe/3e C/TP"),
            ]
        );
    }

    #[test]
    fn renames_a_top_level_binder() {
        let (_dir, library) = new_library();
        note(&library, "Classe", "a", "Cours");
        let renamed = library.rename_binder(Path::new("Classe"), "Lycée").unwrap();
        assert_eq!(renamed, Path::new("Lycée"));
        assert_eq!(library.binders(), [PathBuf::from("Lycée")]);
    }

    #[test]
    fn an_existing_folder_or_file_is_never_replaced() {
        let (dir, library) = new_library();
        note(&library, "Classe", "a", "Cours");
        note(&library, "Autre", "b", "Autre");
        library.create_binder(Path::new("Vide")).unwrap();
        fs::write(dir.path().join("fichier.txt"), b"keep").unwrap();

        // An existing folder, even an empty one, and an existing file.
        for taken in ["Autre", "Vide", "fichier.txt"] {
            let error = library
                .rename_binder(Path::new("Classe"), taken)
                .unwrap_err();
            assert!(
                matches!(&error, LibraryError::BinderExists { path } if path == Path::new(taken)),
                "{error:?}"
            );
            assert_eq!(error.code(), "binderExists");
        }
        // Windows names ignore case: "AUTRE" is "Autre".
        if cfg!(windows) {
            assert!(matches!(
                library.rename_binder(Path::new("Classe"), "AUTRE"),
                Err(LibraryError::BinderExists { .. })
            ));
        }
        assert_eq!(
            names_in(dir.path()),
            ["Autre", "Classe", "Vide", "_ardoise", "fichier.txt"]
        );
        assert_eq!(fs::read(dir.path().join("fichier.txt")).unwrap(), b"keep");
        assert_eq!(library.list().notes.len(), 2);
    }

    #[test]
    fn a_change_of_case_only_works() {
        let (dir, library) = new_library();
        let saved = note(&library, "classe", "a", "Cours");
        let stamp = FileStamp::of(&saved).unwrap();
        let renamed = library
            .rename_binder(Path::new("classe"), "Classe")
            .unwrap();
        assert_eq!(renamed, Path::new("Classe"));
        assert_eq!(names_in(dir.path()), ["Classe", "_ardoise"]);
        let listing = library.list();
        assert_eq!(listing.notes.len(), 1);
        assert_eq!(listing.notes[0].binder, Path::new("Classe"));
        assert_eq!(listing.notes[0].stamp, stamp);
    }

    #[cfg(not(windows))]
    #[test]
    fn a_case_variant_that_is_another_folder_is_not_replaced() {
        let (dir, library) = new_library();
        note(&library, "classe", "a", "Cours");
        note(&library, "Classe", "b", "Autre");
        let error = library
            .rename_binder(Path::new("classe"), "Classe")
            .unwrap_err();
        assert!(matches!(error, LibraryError::BinderExists { .. }));
        assert_eq!(names_in(dir.path()), ["Classe", "_ardoise", "classe"]);
        assert_eq!(library.list().notes.len(), 2);
    }

    #[test]
    fn the_same_name_is_a_no_op() {
        let (dir, library) = new_library();
        let saved = note(&library, "Classe/3e B", "a", "Cours");
        let stamp = FileStamp::of(&saved).unwrap();
        let same = library
            .rename_binder(Path::new("Classe/3e B"), "3e B")
            .unwrap();
        assert_eq!(same, Path::new("Classe/3e B"));
        assert_eq!(FileStamp::of(&saved).unwrap(), stamp);
        assert_eq!(names_in(&dir.path().join("Classe")), ["3e B"]);
    }

    #[test]
    fn a_missing_binder_a_file_and_the_root_are_refused() {
        let (dir, library) = new_library();
        fs::write(dir.path().join("fichier"), b"x").unwrap();
        for missing in ["Nulle part", "Classe/3e B", "fichier"] {
            let error = library
                .rename_binder(Path::new(missing), "Nouveau")
                .unwrap_err();
            assert!(
                matches!(error, LibraryError::BinderNotFound { .. }),
                "{missing}"
            );
            assert_eq!(error.code(), "binderNotFound");
            let error = library.trash_binder(Path::new(missing)).unwrap_err();
            assert!(
                matches!(error, LibraryError::BinderNotFound { .. }),
                "{missing}"
            );
        }
        for root in ["", "."] {
            let error = library
                .rename_binder(Path::new(root), "Nouveau")
                .unwrap_err();
            assert!(
                matches!(error, LibraryError::InvalidName { .. }),
                "{root:?}"
            );
            let error = library.trash_binder(Path::new(root)).unwrap_err();
            assert!(
                matches!(error, LibraryError::InvalidName { .. }),
                "{root:?}"
            );
        }
        for bad in ["../x", "/abs", "_ardoise/trash", ".hidden"] {
            let error = library
                .rename_binder(Path::new(bad), "Nouveau")
                .unwrap_err();
            assert!(matches!(error, LibraryError::InvalidName { .. }), "{bad}");
        }
        assert_eq!(names_in(dir.path()), ["_ardoise", "fichier"]);
    }

    #[test]
    fn invalid_new_names_are_refused() {
        let (dir, library) = new_library();
        note(&library, "Classe", "a", "Cours");
        for bad in [
            "..",
            ".",
            "a/b",
            r"a\b",
            "/",
            "_ardoise",
            "_ARDOISE",
            "",
            " ",
            "fin.",
            ".caché",
            "CON",
            "nul.txt",
            "a:b",
            "a*b",
            "Thumbs.db",
        ] {
            let error = library.rename_binder(Path::new("Classe"), bad).unwrap_err();
            assert!(matches!(error, LibraryError::InvalidName { .. }), "{bad:?}");
            assert_eq!(error.code(), "invalidName");
        }
        assert_eq!(names_in(dir.path()), ["Classe", "_ardoise"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_not_a_binder() {
        let (dir, library) = new_library();
        let outside = tempfile::tempdir().unwrap();
        fs::create_dir(outside.path().join("sub")).unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("Lien")).unwrap();
        for binder in ["Lien", "Lien/sub"] {
            assert!(matches!(
                library.rename_binder(Path::new(binder), "Autre"),
                Err(LibraryError::BinderNotFound { .. })
            ));
            assert!(matches!(
                library.trash_binder(Path::new(binder)),
                Err(LibraryError::BinderNotFound { .. })
            ));
        }
        assert!(outside.path().join("sub").exists());
    }

    #[test]
    fn trashes_nested_notes_and_removes_the_empty_folders() {
        let (dir, library) = new_library();
        let a = note(&library, "Classe", "a", "Cours");
        let b = note(&library, "Classe/3e B", "b", "TP");
        let c = note(&library, "Classe/3e B/Devoirs", "c", "DM");
        let keep = note(&library, "Autre", "k", "Reste");
        library.create_binder(Path::new("Classe/Vide")).unwrap();
        let bytes: Vec<_> = [&a, &b, &c].iter().map(|p| fs::read(p).unwrap()).collect();

        let result = library.trash_binder(Path::new("Classe")).unwrap();
        assert!(result.failed.is_empty() && result.kept.is_empty());
        assert_eq!(result.trashed.len(), 3);
        let originals: Vec<_> = result.trashed.iter().map(|t| t.original.clone()).collect();
        assert_eq!(
            originals,
            [
                PathBuf::from("Classe/2026-10-24 Cours.ardoise"),
                PathBuf::from("Classe/3e B/2026-10-24 TP.ardoise"),
                PathBuf::from("Classe/3e B/Devoirs/2026-10-24 DM.ardoise"),
            ]
        );
        assert_eq!(library.binders(), [PathBuf::from("Autre")]);
        assert!(!dir.path().join("Classe").exists());
        assert_eq!(library.list().notes.len(), 1);
        assert!(keep.exists());
        assert_eq!(library.trash_items().unwrap().len(), 3);

        // Each note goes back to its own binder, which is recreated.
        for (item, bytes) in result.trashed.iter().zip(&bytes) {
            let restored = library.restore(&item.id).unwrap();
            assert_eq!(dir.path().join(&item.original), restored);
            assert_eq!(&fs::read(restored).unwrap(), bytes);
        }
        assert_eq!(library.list().notes.len(), 4);
        assert!(library.trash_items().unwrap().is_empty());
        assert!(a.exists() && b.exists() && c.exists());
    }

    #[test]
    fn a_folder_holding_something_else_is_kept() {
        let (dir, library) = new_library();
        note(&library, "Classe", "a", "Cours");
        note(&library, "Classe/3e B", "b", "TP");
        note(&library, "Classe/3e B/Devoirs", "c", "DM");
        note(&library, "Classe/Seul", "d", "Seul");
        fs::write(dir.path().join("Classe/3e B/cours.pdf"), b"%PDF").unwrap();
        fs::write(dir.path().join("Classe/Seul/.hidden"), b"h").unwrap();
        fs::create_dir(dir.path().join("Classe/Seul/.cache")).unwrap();

        let result = library.trash_binder(Path::new("Classe")).unwrap();
        assert_eq!(result.trashed.len(), 4);
        assert!(result.failed.is_empty());
        // Deepest first; the folders above a kept one hold it, so they stay too.
        assert_eq!(
            result.kept,
            [
                PathBuf::from("Classe/3e B"),
                PathBuf::from("Classe/Seul"),
                PathBuf::from("Classe"),
            ]
        );
        assert_eq!(
            fs::read(dir.path().join("Classe/3e B/cours.pdf")).unwrap(),
            b"%PDF"
        );
        assert!(dir.path().join("Classe/Seul/.hidden").exists());
        // The emptied sub-folder is gone, the others are still binders.
        assert!(!dir.path().join("Classe/3e B/Devoirs").exists());
        assert_eq!(
            library.binders(),
            [
                PathBuf::from("Classe"),
                PathBuf::from("Classe/3e B"),
                PathBuf::from("Classe/Seul")
            ]
        );
        assert!(library.list().notes.is_empty());
    }

    #[test]
    fn a_binder_without_notes_just_goes() {
        let (dir, library) = new_library();
        library.create_binder(Path::new("A/B")).unwrap();
        let result = library.trash_binder(Path::new("A")).unwrap();
        assert!(result.trashed.is_empty() && result.failed.is_empty() && result.kept.is_empty());
        assert_eq!(names_in(dir.path()), ["_ardoise"]);
        assert!(library.binders().is_empty());
    }

    #[test]
    fn a_note_that_cannot_be_trashed_keeps_its_folder() {
        let (dir, library) = new_library();
        let good = note(&library, "Classe", "a", "Cours");
        // A trash that cannot be created: a file where the folder should be.
        fs::write(dir.path().join("_ardoise/trash"), b"x").unwrap();
        let result = library.trash_binder(Path::new("Classe")).unwrap();
        assert!(result.trashed.is_empty());
        assert_eq!(result.failed.len(), 1);
        assert_eq!(result.failed[0].path, good);
        assert_eq!(result.kept, [PathBuf::from("Classe")]);
        assert!(good.exists());
    }

    #[test]
    fn a_damaged_note_is_trashed_like_the_others() {
        let (dir, library) = new_library();
        let folder = dir.path().join("Classe");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join("x.ardoise"), b"not a zip").unwrap();
        let result = library.trash_binder(Path::new("Classe")).unwrap();
        assert_eq!(result.trashed.len(), 1);
        assert!(!folder.exists());
        assert_eq!(fs::read(&result.trashed[0].path).unwrap(), b"not a zip");
    }
}
