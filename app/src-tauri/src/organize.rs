//! Organising the library (milestone M10): renaming and deleting binders from the sidebar,
//! and the colours of tags (`_ardoise/tags.json`, shared by every PC through the library; see
//! `ardoise_core::library::tag_colors`). Shapes mirror `app/ui/src/lib/api.ts` (section
//! "Library: organising").

use std::collections::BTreeMap;
use std::path::Path;

use ardoise_core::library::{FileStamp, Library, LibraryError};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::library::{TrashEntry, binder_path, library_error, library_of, path_text, slash_path};
use crate::notes::blocking;

/// A note that an operation on several notes could not handle, with the reason
/// (`{ code, message }`, translated by the interface).
#[derive(Debug, Serialize)]
pub struct FailedNote {
    pub path: String,
    pub error: CommandError,
}

/// A note moved to the trash: where it was, and the id of its trash entry.
#[derive(Debug, Serialize)]
pub struct TrashedNote {
    pub path: String,
    pub id: String,
}

/// What `trash_notes` did (`TrashedNotes` in `api.ts`).
#[derive(Debug, Serialize)]
pub struct TrashedNotes {
    pub trashed: Vec<TrashedNote>,
    pub failed: Vec<FailedNote>,
}

/// Moves each note to the trash; one that fails (gone meanwhile, held open…) does not stop the
/// others.
pub fn trash_notes_in(library: &Library, paths: &[String]) -> TrashedNotes {
    let mut done = TrashedNotes {
        trashed: Vec::new(),
        failed: Vec::new(),
    };
    for path in paths {
        match library.trash(Path::new(path)) {
            Ok(item) => done.trashed.push(TrashedNote {
                path: path.clone(),
                id: item.id,
            }),
            Err(error) => done.failed.push(FailedNote {
                path: path.clone(),
                error: library_error(error),
            }),
        }
    }
    done
}

/// A note a move took from `from` to `to` (full paths: a name taken in the target gives it a ` (n)` suffix), with
/// the stamp of its file there (a move does not rewrite the file: it is the stamp the undo expects to find; `None`
/// when it cannot be read).
#[derive(Debug, Serialize)]
pub struct MovedNote {
    pub from: String,
    pub to: String,
    pub stamp: Option<FileStamp>,
}

/// What `move_notes` and `move_notes_back` did (`MovedNotes` in `api.ts`): each note moved, and each that could
/// not be (`path` is where it was asked for).
#[derive(Debug, Serialize)]
pub struct MovedNotes {
    pub moved: Vec<MovedNote>,
    pub failed: Vec<FailedNote>,
}

/// Moves each note into `binder` (created if needed); one that fails (gone meanwhile, held open…) does not stop the
/// others. A note already there stays (`from` = `to`).
pub fn move_notes_in(library: &Library, paths: &[String], binder: &str) -> MovedNotes {
    let binder = binder_path(binder);
    let mut done = MovedNotes {
        moved: Vec::new(),
        failed: Vec::new(),
    };
    for path in paths {
        match library.move_to(Path::new(path), &binder) {
            Ok(to) => done.moved.push(MovedNote {
                from: path.clone(),
                stamp: FileStamp::of(&to).ok(),
                to: path_text(&to),
            }),
            Err(error) => done.failed.push(FailedNote {
                path: path.clone(),
                error: library_error(error),
            }),
        }
    }
    done
}

/// A note to put back where a move took it from: where it is now (`path`), the stamp it had when it was moved, and the
/// path it had before (`original`).
#[derive(Debug, Deserialize)]
pub struct MoveBack {
    pub path: String,
    pub stamp: FileStamp,
    pub original: String,
}

/// Puts each note back (`Library::move_back`: only if it is still the file that was moved, never over another note);
/// one that cannot go back does not stop the others.
pub fn move_notes_back_in(library: &Library, items: &[MoveBack]) -> MovedNotes {
    let mut done = MovedNotes {
        moved: Vec::new(),
        failed: Vec::new(),
    };
    for item in items {
        match library.move_back(
            Path::new(&item.path),
            Path::new(&item.original),
            &item.stamp,
        ) {
            Ok(to) => done.moved.push(MovedNote {
                from: item.path.clone(),
                stamp: FileStamp::of(&to).ok(),
                to: path_text(&to),
            }),
            Err(error) => done.failed.push(FailedNote {
                path: item.path.clone(),
                error: library_error(error),
            }),
        }
    }
    done
}

/// What deleting a binder did (`BinderDeleted` in `api.ts`).
#[derive(Debug, Serialize)]
pub struct BinderDeleted {
    /// The notes now in the trash, each restorable to its own binder.
    pub trashed: Vec<TrashEntry>,
    pub failed: Vec<FailedNote>,
    /// Folders kept because they still hold something that is not a note (relative,
    /// `/`-separated, deepest first).
    pub kept: Vec<String>,
}

pub fn rename_binder_in(library: &Library, binder: &str, name: &str) -> CommandResult<String> {
    library
        .rename_binder(&binder_path(binder), name.trim())
        .map(|renamed| slash_path(&renamed))
        .map_err(library_error)
}

pub fn delete_binder_in(library: &Library, binder: &str) -> CommandResult<BinderDeleted> {
    let done = library
        .trash_binder(&binder_path(binder))
        .map_err(library_error)?;
    Ok(BinderDeleted {
        trashed: done.trashed.into_iter().map(TrashEntry::from).collect(),
        failed: done
            .failed
            .into_iter()
            .map(|problem| FailedNote {
                path: path_text(&problem.path),
                error: library_error(problem.error),
            })
            .collect(),
        kept: done.kept.iter().map(|folder| slash_path(folder)).collect(),
    })
}

/// A failure of the tag colours: `tags.json` changing under every attempt is not a note
/// conflict, whatever the core calls it.
fn tag_colors_error(error: LibraryError) -> CommandError {
    match error {
        LibraryError::Conflict { .. } => CommandError::new("tagColorsConflict", error),
        error => library_error(error),
    }
}

pub fn set_tag_color_in(
    library: &Library,
    tag: &str,
    color: Option<&str>,
) -> CommandResult<BTreeMap<String, String>> {
    library.set_tag_color(tag, color).map_err(tag_colors_error)
}

// --- Commands ---------------------------------------------------------------------------

/// Renames the binder `binder` (`a/b`); returns its new path. Never replaces a folder.
#[tauri::command]
pub async fn rename_binder(
    context: State<'_, AppContext>,
    binder: String,
    name: String,
) -> CommandResult<String> {
    let library = library_of(&context)?;
    blocking(move || rename_binder_in(&library, &binder, &name)).await
}

/// Puts every note of the binder (sub-binders included) in the trash, then removes the
/// folders left empty.
#[tauri::command]
pub async fn delete_binder(
    context: State<'_, AppContext>,
    binder: String,
) -> CommandResult<BinderDeleted> {
    let library = library_of(&context)?;
    blocking(move || delete_binder_in(&library, &binder)).await
}

/// Moves notes into a binder: the ones moved (with their new paths) and the ones that failed.
#[tauri::command]
pub async fn move_notes(
    context: State<'_, AppContext>,
    paths: Vec<String>,
    binder: String,
) -> CommandResult<MovedNotes> {
    let library = library_of(&context)?;
    blocking(move || Ok(move_notes_in(&library, &paths, &binder))).await
}

/// Undoes a move: puts each note back in its binder under its name, if it did not change since.
#[tauri::command]
pub async fn move_notes_back(
    context: State<'_, AppContext>,
    items: Vec<MoveBack>,
) -> CommandResult<MovedNotes> {
    let library = library_of(&context)?;
    blocking(move || Ok(move_notes_back_in(&library, &items))).await
}

/// Colour of every tag that has one, keyed by the tag in lower case (`tag_key`).
#[tauri::command]
pub async fn tag_colors(context: State<'_, AppContext>) -> CommandResult<BTreeMap<String, String>> {
    let library = library_of(&context)?;
    blocking(move || library.tag_colors().map_err(library_error)).await
}

/// Gives `tag` the colour `color` (a palette id of the interface), or none; returns every
/// colour afterwards.
#[tauri::command]
pub async fn set_tag_color(
    context: State<'_, AppContext>,
    tag: String,
    color: Option<String>,
) -> CommandResult<BTreeMap<String, String>> {
    let library = library_of(&context)?;
    blocking(move || set_tag_color_in(&library, &tag, color.as_deref())).await
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::library::tests::{library, write_sample};

    #[test]
    fn renames_a_binder_without_replacing_one() {
        let (dir, library) = library();
        write_sample(&library, "Classe/3e B", "Cours");
        library.create_binder(Path::new("Classe/3e C")).unwrap();

        assert_eq!(
            rename_binder_in(&library, "Classe/3e B", " 3e D ").unwrap(),
            "Classe/3e D"
        );
        assert!(dir.path().join("Classe").join("3e D").is_dir());
        let error = rename_binder_in(&library, "Classe/3e D", "3e C").unwrap_err();
        assert_eq!(error.code(), "binderExists");
        assert!(dir.path().join("Classe").join("3e D").is_dir());
        assert_eq!(
            rename_binder_in(&library, "Absent", "X")
                .unwrap_err()
                .code(),
            "binderNotFound"
        );
        assert_eq!(
            rename_binder_in(&library, "Classe", "a/b")
                .unwrap_err()
                .code(),
            "invalidName"
        );
        assert_eq!(
            rename_binder_in(&library, "", "X").unwrap_err().code(),
            "invalidName"
        );
    }

    #[test]
    fn deleting_a_binder_trashes_its_notes_restorably_and_keeps_other_files() {
        let (dir, library) = library();
        write_sample(&library, "Classe/3e B", "Cours");
        write_sample(&library, "Classe/3e B/TP", "Mesures");
        write_sample(&library, "Classe/Vieux", "Ancien");
        fs::write(dir.path().join("Classe/Vieux/sujet.pdf"), b"%PDF").unwrap();
        write_sample(&library, "Perso", "Reste");

        let done = delete_binder_in(&library, "Classe").unwrap();
        assert_eq!(done.trashed.len(), 3);
        assert!(done.failed.is_empty());
        assert!(
            done.trashed
                .iter()
                .any(|entry| entry.original.starts_with("Classe/3e B/TP/"))
        );
        assert_eq!(done.kept, ["Classe/Vieux", "Classe"]);
        assert!(!dir.path().join("Classe/3e B").exists());
        assert!(dir.path().join("Classe/Vieux/sujet.pdf").is_file());
        assert_eq!(library.list().notes.len(), 1);

        // The JSON shape the interface reads.
        let json = serde_json::to_value(&done).unwrap();
        assert!(json["trashed"][0]["id"].is_string());
        assert!(json["trashed"][0]["original"].is_string());
        assert!(json["kept"][0].is_string());

        // Every note goes back to its binder, recreated.
        for entry in &done.trashed {
            let restored = library.restore(&entry.id).unwrap();
            assert_eq!(
                restored.strip_prefix(library.root()).unwrap(),
                Path::new(&entry.original)
            );
        }
        assert_eq!(library.list().notes.len(), 4);
        assert_eq!(
            delete_binder_in(&library, "Absent").unwrap_err().code(),
            "binderNotFound"
        );
    }

    #[test]
    fn trashing_notes_goes_on_after_a_failure() {
        let (_dir, library) = library();
        let a = path_text(&write_sample(&library, "", "Un").path);
        let b = path_text(&write_sample(&library, "Classe", "Deux").path);
        let gone = path_text(&library.root().join("2026-10-24 Absente.ardoise"));
        let done = trash_notes_in(&library, &[a.clone(), gone.clone(), b.clone()]);
        assert_eq!(
            done.trashed
                .iter()
                .map(|t| t.path.as_str())
                .collect::<Vec<_>>(),
            [a.as_str(), b.as_str()]
        );
        assert_eq!(done.failed.len(), 1);
        assert_eq!(done.failed[0].path, gone);
        assert_eq!(done.failed[0].error.code(), "noteNotFound");
        assert!(library.list().notes.is_empty());
        // The ids restore the notes where they were.
        for trashed in &done.trashed {
            assert_eq!(
                path_text(&library.restore(&trashed.id).unwrap()),
                trashed.path
            );
        }
        let json = serde_json::to_value(&done).unwrap();
        assert!(json["trashed"][0]["id"].is_string() && json["trashed"][0]["path"].is_string());
        assert_eq!(json["failed"][0]["error"]["code"], "noteNotFound");
    }

    #[test]
    fn tag_colors_are_set_read_and_removed() {
        let (dir, library) = library();
        assert!(library.tag_colors().unwrap().is_empty());
        let colors = set_tag_color_in(&library, "Physique", Some("blue")).unwrap();
        assert_eq!(colors.get("physique").map(String::as_str), Some("blue"));
        assert!(dir.path().join("_ardoise").join("tags.json").is_file());
        let colors = set_tag_color_in(&library, "physique", None).unwrap();
        assert!(colors.is_empty());
        assert_eq!(
            set_tag_color_in(&library, "x", Some("Bleu !"))
                .unwrap_err()
                .code(),
            "invalidTagColor"
        );
        // The shape the interface reads: `{ tag: colour }`.
        let colors = set_tag_color_in(&library, "3e B", Some("green")).unwrap();
        assert_eq!(
            serde_json::to_value(&colors).unwrap(),
            serde_json::json!({ "3e b": "green" })
        );
    }

    #[test]
    fn a_tags_file_that_keeps_changing_is_not_a_note_conflict() {
        let error = tag_colors_error(LibraryError::Conflict {
            path: "tags.json".into(),
        });
        assert_eq!(error.code(), "tagColorsConflict");
        let error = tag_colors_error(LibraryError::InvalidTagColor { color: "X".into() });
        assert_eq!(error.code(), "invalidTagColor");
    }

    fn text(path: &Path) -> String {
        path_text(path)
    }

    #[test]
    fn a_move_says_what_moved_and_what_failed_and_goes_back() {
        let (dir, library) = library();
        let a = write_sample(&library, "Perso", "Un");
        let b = write_sample(&library, "Perso", "Deux");
        let missing = text(&dir.path().join("Perso").join("2026-10-24 Absent.ardoise"));
        let done = move_notes_in(
            &library,
            &[text(&a.path), missing.clone(), text(&b.path)],
            "Classe/3e B",
        );
        // One failure does not stop the others.
        assert_eq!(done.moved.len(), 2);
        assert_eq!(done.failed.len(), 1);
        assert_eq!(done.failed[0].path, missing);
        assert_eq!(done.failed[0].error.code(), "noteNotFound");
        // The shape the interface reads.
        let json = serde_json::to_value(&done).unwrap();
        assert_eq!(json["moved"][0]["from"], text(&a.path));
        // The stamp the undo expects: a move does not rewrite the file.
        assert_eq!(done.moved[0].stamp, Some(a.stamp));
        assert_eq!(json["failed"][0]["error"]["code"], "noteNotFound");

        // Back: each note only if it is still the file that was moved.
        library
            .set_tags(Path::new(&done.moved[1].to), &["x".to_owned()], None)
            .unwrap();
        let items = [(&a, &done.moved[0]), (&b, &done.moved[1])].map(|(saved, moved)| MoveBack {
            path: moved.to.clone(),
            stamp: saved.stamp,
            original: text(&saved.path),
        });
        let back = move_notes_back_in(&library, &items);
        assert_eq!(back.moved.len(), 1);
        assert_eq!(back.moved[0].to, text(&a.path));
        assert!(a.path.is_file());
        assert_eq!(back.failed.len(), 1);
        assert_eq!(back.failed[0].path, done.moved[1].to);
        assert_eq!(back.failed[0].error.code(), "noteConflict");
        assert!(Path::new(&done.moved[1].to).is_file());
        assert!(!b.path.exists());
    }
}
