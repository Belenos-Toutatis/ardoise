//! Library commands (milestone M3): listing, new notes and binders, moves, trash, tags,
//! favourites and titles, on this computer's library (see `app/ui/src/lib/api.ts`, section
//! "Library", and `docs/design/interface.md`).
//!
//! Every path received from the interface goes through `ardoise_core::library::Library`,
//! which refuses anything that is not a note file inside the library (`notInLibrary`).
//! Shapes sent to the interface mirror `app/ui/src/lib/note.ts`: camelCase, binders as
//! `/`-separated relative paths.

use std::path::{Component, Path, PathBuf};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

use ardoise_core::format::NoteFile;
use ardoise_core::library::{
    FileStamp, HeaderCache, Library, LibraryError, NoteEntry, TagCount, TrashItem,
    parse_conflict_name,
};
use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Note, Origin, Timing};
use serde::Serialize;
use tauri::State;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult, current_state};
use crate::notes::blocking;
use crate::thumbnails::thumbnail_png;

/// Error sent to the interface for a library failure.
pub fn library_error(error: LibraryError) -> CommandError {
    CommandError::new(error.code(), error)
}

/// This computer's library (code `noLibrary` when none is chosen).
pub fn library_of(context: &AppContext) -> CommandResult<Library> {
    let state = current_state(context);
    let root = state
        .library()
        .ok_or_else(|| CommandError::new("noLibrary", "no library is chosen on this computer"))?;
    Library::open(root).map_err(library_error)
}

/// Current time in Unix milliseconds.
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

/// `path` as text for the interface.
pub fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// A relative path as `a/b/c`, whatever the system's separator.
pub fn slash_path(path: &Path) -> String {
    path.components()
        .filter_map(|c| match c {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// A binder from the interface (`a/b`, empty for the root). Every part is kept as given, so
/// that `..` or a drive reaches `Library::binder_dir` and is refused there.
pub fn binder_path(binder: &str) -> PathBuf {
    binder
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .collect()
}

/// `{ originalName }`: the file is a sync client's conflict copy of `originalName`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    pub original_name: String,
}

/// One note of the listing (`NoteSummary` in `note.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteSummary {
    pub path: String,
    /// Relative to the library root, `/`-separated; empty for the root.
    pub binder: String,
    pub file_name: String,
    pub id: String,
    pub title: String,
    pub created: i64,
    pub modified: i64,
    pub tags: Vec<String>,
    pub favorite: bool,
    pub stamp: FileStamp,
    pub conflict: Option<ConflictInfo>,
}

impl From<NoteEntry> for NoteSummary {
    fn from(entry: NoteEntry) -> Self {
        NoteSummary {
            path: path_text(&entry.path),
            binder: slash_path(&entry.binder),
            file_name: entry.file_name,
            id: entry.id,
            title: entry.title,
            created: entry.created,
            modified: entry.modified,
            tags: entry.tags,
            favorite: entry.favorite,
            stamp: entry.stamp,
            conflict: entry.conflict.map(|c| ConflictInfo {
                original_name: c.original_name,
            }),
        }
    }
}

/// Summary of a note just written at `path`.
pub(crate) fn summary_of(
    library: &Library,
    path: &Path,
    note: &Note,
    stamp: FileStamp,
) -> NoteSummary {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let binder = path
        .parent()
        .and_then(|parent| parent.strip_prefix(library.root()).ok())
        .map(slash_path)
        .unwrap_or_default();
    NoteSummary {
        path: path_text(path),
        binder,
        conflict: parse_conflict_name(&file_name).map(|c| ConflictInfo {
            original_name: c.original_name,
        }),
        file_name,
        id: note.id.clone(),
        title: note.title.clone(),
        created: note.created,
        modified: note.modified,
        tags: note.tags.clone(),
        favorite: note.favorite,
        stamp,
    }
}

/// A file that could not be listed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ListingProblem {
    pub path: String,
    /// The `errorCodes.*` key of the library errors (`noteNewerVersion`: "update Ardoise"...).
    pub code: String,
    /// The technical detail, English.
    pub message: String,
}

/// The whole library (`LibraryListing` in `note.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LibraryListing {
    pub notes: Vec<NoteSummary>,
    /// Every binder, relative and `/`-separated, sorted.
    pub binders: Vec<String>,
    pub tags: Vec<TagCount>,
    pub problems: Vec<ListingProblem>,
}

/// The headers of the notes listed last, so that a listing reopens only the notes whose file
/// changed (every `library-changed` event, the app's own saves included, lists the library
/// again; opening a thousand notes' archives on a synced folder takes seconds).
static HEADERS: LazyLock<Mutex<HeaderCache>> = LazyLock::new(Mutex::default);

/// The id of the note at `path` as the last listing read it, when its file still has `stamp`:
/// no need to open the file again (the thumbnails ask for it for every card).
pub fn listed_note_id(path: &Path, stamp: &FileStamp) -> Option<String> {
    let headers = HEADERS.lock().unwrap_or_else(PoisonError::into_inner);
    headers.get(path, stamp).map(|entry| entry.id.clone())
}

pub fn list(library: &Library) -> LibraryListing {
    let listing = library.list_with(&mut HEADERS.lock().unwrap_or_else(PoisonError::into_inner));
    let tags = listing.tags();
    let binders: Vec<String> = listing.binders.iter().map(|b| slash_path(b)).collect();
    LibraryListing {
        tags,
        binders,
        problems: listing
            .problems
            .into_iter()
            .map(|p| ListingProblem {
                path: path_text(&p.path),
                code: p.error.code().to_owned(),
                message: p.error.to_string(),
            })
            .collect(),
        notes: listing.notes.into_iter().map(NoteSummary::from).collect(),
    }
}

/// Takes the automatic export folder (`folder`, relative and `/`-separated, as
/// `AutoExportState::valid_folder` gives it) and its sub-folders out of the binders: they hold
/// exported PDF and PNG files, not notes. A binder that does hold notes stays, so that a
/// binder of the same name made by the user never hides its notes. Names are compared without
/// regard to case, as Windows does.
pub fn hide_export_folder(listing: &mut LibraryListing, folder: Option<&str>) {
    let Some(folder) = folder else { return };
    let within = |binder: &str, parent: &str| {
        binder == parent
            || binder
                .strip_prefix(parent)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    let folder = folder.to_lowercase();
    let note_binders: Vec<String> = listing
        .notes
        .iter()
        .map(|note| note.binder.to_lowercase())
        .collect();
    listing.binders.retain(|binder| {
        let binder = binder.to_lowercase();
        !within(&binder, &folder) || note_binders.iter().any(|n| within(n, &binder))
    });
}

/// Name of the first layer of a new note, in the interface language (`"fr"` or `"en"`).
pub fn first_layer_name(language: &str) -> &'static str {
    if language == "fr" {
        "Calque 1"
    } else {
        "Layer 1"
    }
}

/// A new blank note: A4 landscape, white paper without lines, one layer.
pub fn blank_note(title: &str, layer_name: &str, now: i64) -> Note {
    Note {
        id: uuid::Uuid::new_v4().to_string(),
        title: title.trim().to_owned(),
        created: now,
        modified: now,
        page: A4_LANDSCAPE,
        background: Background::default(),
        background_image: None,
        tags: Vec::new(),
        favorite: false,
        layers: vec![Layer {
            name: layer_name.to_owned(),
            visible: true,
        }],
        strokes: Vec::new(),
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

pub fn create_note_in(
    library: &Library,
    binder: &str,
    title: &str,
    layer_name: &str,
) -> Result<NoteSummary, LibraryError> {
    let mut file = NoteFile::new(blank_note(title, layer_name, now_ms()));
    file.thumbnail = thumbnail_png(&file.note);
    let saved = library.create(&binder_path(binder), &file)?;
    Ok(summary_of(library, &saved.path, &file.note, saved.stamp))
}

pub fn move_notes_in(
    library: &Library,
    paths: &[String],
    binder: &str,
) -> Result<Vec<String>, LibraryError> {
    let binder = binder_path(binder);
    paths
        .iter()
        .map(|path| {
            library
                .move_to(Path::new(path), &binder)
                .map(|p| path_text(&p))
        })
        .collect()
}

/// A note in the trash (`TrashEntry` in `note.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashEntry {
    pub id: String,
    pub title: Option<String>,
    /// Relative to the library root, `/`-separated.
    pub original: String,
    /// Unix milliseconds.
    pub deleted: i64,
    pub note_id: Option<String>,
}

impl From<TrashItem> for TrashEntry {
    fn from(item: TrashItem) -> Self {
        TrashEntry {
            id: item.id,
            title: item.title,
            original: slash_path(&item.original),
            deleted: item.deleted,
            note_id: item.note_id,
        }
    }
}

pub fn trash_list_in(library: &Library) -> Result<Vec<TrashEntry>, LibraryError> {
    Ok(library
        .trash_items()?
        .into_iter()
        .map(TrashEntry::from)
        .collect())
}

/// Where a retitled note is now, and its new stamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RetitledNote {
    pub path: String,
    pub stamp: FileStamp,
}

pub fn retitle_in(
    library: &Library,
    path: &str,
    title: &str,
    stamp: &FileStamp,
) -> Result<RetitledNote, LibraryError> {
    let saved = library.retitle(Path::new(path), title.trim(), Some(stamp))?;
    Ok(RetitledNote {
        path: path_text(&saved.path),
        stamp: saved.stamp,
    })
}

/// The listing the interface shows: the whole library, without the automatic export folder
/// while the automatic export is on.
#[cfg(test)]
fn list_for_interface(context: &AppContext) -> CommandResult<LibraryListing> {
    Ok(list_without_exports(
        &library_of(context)?,
        export_folder(context).as_deref(),
    ))
}

/// The automatic export folder to hide, only while the export is on: with it off, the folder
/// is an ordinary binder.
fn export_folder(context: &AppContext) -> Option<String> {
    let state = current_state(context);
    if state.auto_export().is_active() {
        state.auto_export().valid_folder()
    } else {
        None
    }
}

fn list_without_exports(library: &Library, export_folder: Option<&str>) -> LibraryListing {
    let mut listing = list(library);
    hide_export_folder(&mut listing, export_folder);
    listing
}

// --- Commands ---------------------------------------------------------------------------
//
// Async: Tauri runs them on a worker thread instead of the main thread, so disk access never
// freezes the window.

/// Runs `action` on the library, on a blocking thread: the disk of a synced folder can be slow, and the
/// async runtime has only a few threads.
async fn with_library_blocking<T: Send + 'static>(
    context: &AppContext,
    action: impl FnOnce(&Library) -> Result<T, LibraryError> + Send + 'static,
) -> CommandResult<T> {
    let library = library_of(context)?;
    blocking(move || action(&library).map_err(library_error)).await
}

#[tauri::command]
pub async fn library_list(context: State<'_, AppContext>) -> CommandResult<LibraryListing> {
    let library = library_of(&context)?;
    let export = export_folder(&context);
    blocking(move || Ok(list_without_exports(&library, export.as_deref()))).await
}

#[tauri::command]
pub async fn create_note(
    context: State<'_, AppContext>,
    binder: String,
    title: String,
) -> CommandResult<NoteSummary> {
    let layer = first_layer_name(&crate::diagnostic::report_language(&context));
    with_library_blocking(&context, move |library| {
        create_note_in(library, &binder, &title, layer)
    })
    .await
}

#[tauri::command]
pub async fn create_binder(context: State<'_, AppContext>, binder: String) -> CommandResult<()> {
    with_library_blocking(&context, move |library| {
        library.create_binder(&binder_path(&binder)).map(|_| ())
    })
    .await
}

#[tauri::command]
pub async fn move_notes(
    context: State<'_, AppContext>,
    paths: Vec<String>,
    binder: String,
) -> CommandResult<Vec<String>> {
    with_library_blocking(&context, move |library| {
        move_notes_in(library, &paths, &binder)
    })
    .await
}

#[tauri::command]
pub async fn trash_notes(context: State<'_, AppContext>, paths: Vec<String>) -> CommandResult<()> {
    with_library_blocking(&context, move |library| {
        paths
            .iter()
            .try_for_each(|path| library.trash(Path::new(path)).map(|_| ()))
    })
    .await
}

#[tauri::command]
pub async fn trash_list(context: State<'_, AppContext>) -> CommandResult<Vec<TrashEntry>> {
    with_library_blocking(&context, trash_list_in).await
}

#[tauri::command]
pub async fn trash_restore(context: State<'_, AppContext>, id: String) -> CommandResult<String> {
    with_library_blocking(&context, move |library| {
        library.restore(&id).map(|p| path_text(&p))
    })
    .await
}

#[tauri::command]
pub async fn trash_purge(context: State<'_, AppContext>, id: String) -> CommandResult<()> {
    with_library_blocking(&context, move |library| library.purge(&id)).await
}

#[tauri::command]
pub async fn trash_empty(context: State<'_, AppContext>) -> CommandResult<usize> {
    with_library_blocking(&context, Library::empty_trash).await
}

#[tauri::command]
pub async fn set_note_tags(
    context: State<'_, AppContext>,
    path: String,
    tags: Vec<String>,
    stamp: FileStamp,
) -> CommandResult<FileStamp> {
    with_library_blocking(&context, move |library| {
        library.set_tags(Path::new(&path), &tags, Some(&stamp))
    })
    .await
}

#[tauri::command]
pub async fn set_note_favorite(
    context: State<'_, AppContext>,
    path: String,
    favorite: bool,
    stamp: FileStamp,
) -> CommandResult<FileStamp> {
    with_library_blocking(&context, move |library| {
        library.set_favorite(Path::new(&path), favorite, Some(&stamp))
    })
    .await
}

#[tauri::command]
pub async fn retitle_note(
    context: State<'_, AppContext>,
    path: String,
    title: String,
    stamp: FileStamp,
) -> CommandResult<RetitledNote> {
    with_library_blocking(&context, move |library| {
        retitle_in(library, &path, &title, &stamp)
    })
    .await
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;

    use ardoise_core::format::read_note;
    use ardoise_core::library::SavedNote;
    use ardoise_core::settings::SettingsStore;
    use ardoise_ink::{Point, Rgba, Stroke, Tool};

    use super::*;

    /// A fresh library in a temporary folder.
    pub(crate) fn library() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        ardoise_core::library::init(dir.path()).unwrap();
        let library = Library::open(dir.path()).unwrap();
        (dir, library)
    }

    pub(crate) fn sample_note(title: &str) -> Note {
        let mut note = blank_note(title, "Calque 1", 1_792_843_200_000);
        note.strokes.push(Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 60.0,
            layer: 0,
            points: (0..30)
                .map(|i| Point {
                    x: 3000.0 + 300.0 * i as f32,
                    y: 5000.0 + 800.0 * (i as f32 / 3.0).sin(),
                    pressure: 0.6,
                    t: 8 * i,
                })
                .collect(),
        });
        note
    }

    /// Writes a sample note with its thumbnail into `binder`.
    pub(crate) fn write_sample(library: &Library, binder: &str, title: &str) -> SavedNote {
        let mut file = NoteFile::new(sample_note(title));
        file.thumbnail = thumbnail_png(&file.note);
        library.create(&binder_path(binder), &file).unwrap()
    }

    pub(crate) fn context(settings_dir: &Path) -> AppContext {
        AppContext {
            store: Ok(SettingsStore::locate(settings_dir, settings_dir, "PC-A")),
            system_locale: Some("fr-FR".to_owned()),
            self_test: None,
            open_diagnostic: false,
        }
    }

    fn text(path: &Path) -> String {
        path_text(path)
    }

    #[test]
    fn the_id_of_a_listed_note_is_known_while_its_file_is_unchanged() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        list(&library);
        let id = library.read(&saved.path).unwrap().file.note.id;
        assert_eq!(listed_note_id(&saved.path, &saved.stamp), Some(id));
        let other = FileStamp {
            len: saved.stamp.len + 1,
            ..saved.stamp
        };
        assert_eq!(listed_note_id(&saved.path, &other), None);
    }

    #[test]
    fn an_unreadable_note_is_listed_with_its_error_code() {
        let (dir, library) = library();
        fs::write(dir.path().join("2026-10-24 Abîmée.ardoise"), b"not a zip").unwrap();
        let problems = list(&library).problems;
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].code, "noteDamaged");
    }

    #[test]
    fn no_library_chosen_is_reported() {
        let settings = tempfile::tempdir().unwrap();
        let error = library_of(&context(settings.path())).unwrap_err();
        assert_eq!(error.code(), "noLibrary");

        let (dir, _) = library();
        let context = context(settings.path());
        crate::commands::choose_library(&context, &text(dir.path())).unwrap();
        assert_eq!(library_of(&context).unwrap().root(), dir.path());
    }

    #[test]
    fn lists_notes_binders_tags_conflicts_and_problems() {
        let (dir, library) = library();
        let root_note = write_sample(&library, "", "Essai");
        let class_note = write_sample(&library, "Classe/3e B", "Électricité");
        library
            .set_tags(&class_note.path, &["physique".into(), "3e".into()], None)
            .unwrap();
        library
            .set_tags(&root_note.path, &["Physique".into()], None)
            .unwrap();
        library.create_binder(Path::new("Vide")).unwrap();
        let copy = root_note.path.with_file_name(format!(
            "{} (conflicted copy 2026-10-24 101530).ardoise",
            root_note.path.file_stem().unwrap().to_string_lossy()
        ));
        fs::copy(&root_note.path, &copy).unwrap();
        fs::write(dir.path().join("2026-10-24 Abîmée.ardoise"), b"not a zip").unwrap();

        let listing = list(&library);
        assert_eq!(listing.notes.len(), 3);
        assert_eq!(listing.binders, ["Classe", "Classe/3e B", "Vide"]);
        let class = listing
            .notes
            .iter()
            .find(|n| n.title == "Électricité")
            .unwrap();
        assert_eq!(class.binder, "Classe/3e B");
        assert_eq!(class.path, text(&class_note.path));
        let conflict = listing.notes.iter().find(|n| n.conflict.is_some()).unwrap();
        assert_eq!(
            conflict.conflict.as_ref().unwrap().original_name,
            root_note.path.file_name().unwrap().to_string_lossy()
        );
        assert_eq!(
            listing.tags,
            vec![
                TagCount {
                    tag: "3e".into(),
                    count: 1
                },
                TagCount {
                    tag: "Physique".into(),
                    count: 3
                },
            ]
        );
        assert_eq!(listing.problems.len(), 1);
        assert!(listing.problems[0].path.ends_with("Abîmée.ardoise"));
        assert!(!listing.problems[0].message.is_empty());

        // The JSON shape the interface reads (`note.ts`).
        let json = serde_json::to_value(&listing).unwrap();
        let note = json["notes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["title"] == "Électricité")
            .unwrap();
        for key in [
            "path", "binder", "fileName", "id", "title", "created", "modified", "tags", "favorite",
            "stamp", "conflict",
        ] {
            assert!(note.get(key).is_some(), "missing {key} in {note}");
        }
        assert!(note["conflict"].is_null());
        assert!(note["stamp"]["modified"]["secs_since_epoch"].is_u64());
        assert!(note["stamp"]["len"].is_u64());
        let copy_json = json["notes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| !n["conflict"].is_null())
            .unwrap();
        assert!(copy_json["conflict"]["originalName"].is_string());
        assert_eq!(
            json["tags"][0],
            serde_json::json!({"tag": "3e", "count": 1})
        );
        assert!(json["problems"][0]["message"].is_string());
    }

    #[test]
    fn creates_blank_notes_with_a_thumbnail() {
        let (_dir, library) = library();
        let summary = create_note_in(&library, "Classe/3e B", " Cours ", "Calque 1").unwrap();
        assert_eq!(summary.binder, "Classe/3e B");
        assert_eq!(summary.title, "Cours");
        assert!(summary.file_name.ends_with(" Cours.ardoise"));
        assert!(summary.conflict.is_none());
        let file = read_note(Path::new(&summary.path)).unwrap();
        assert_eq!(file.note.page, A4_LANDSCAPE);
        assert_eq!(file.note.background, Background::default());
        assert_eq!(file.note.layers.len(), 1);
        assert_eq!(file.note.layers[0].name, "Calque 1");
        assert_eq!(file.note.origin, Origin::Ardoise);
        assert_eq!(file.note.timing, Timing::Recorded);
        assert_eq!(file.note.created, file.note.modified);
        assert!(file.note.strokes.is_empty());
        let id = uuid::Uuid::parse_str(&file.note.id).unwrap();
        assert_eq!(id.get_version_num(), 4);
        assert_eq!(summary.id, file.note.id);
        assert!(file.thumbnail.is_some());
        assert_eq!(
            FileStamp::of(Path::new(&summary.path)).unwrap(),
            summary.stamp
        );

        // Same title again: a second file, never a replacement.
        let again = create_note_in(&library, "Classe/3e B", "Cours", "Layer 1").unwrap();
        assert_ne!(again.path, summary.path);
        assert_eq!(first_layer_name("fr"), "Calque 1");
        assert_eq!(first_layer_name("en"), "Layer 1");

        let error = create_note_in(&library, "../dehors", "X", "Layer 1").unwrap_err();
        assert_eq!(error.code(), "invalidName");
        let error = create_note_in(&library, "_ardoise", "X", "Layer 1");
        assert!(error.is_err());
    }

    #[test]
    fn layer_name_follows_the_interface_language() {
        let settings = tempfile::tempdir().unwrap();
        let context = context(settings.path());
        assert_eq!(
            first_layer_name(&crate::diagnostic::report_language(&context)),
            "Calque 1"
        );
    }

    #[test]
    fn creates_binders_and_moves_notes() {
        let (_dir, library) = library();
        let a = write_sample(&library, "", "Un");
        let b = write_sample(&library, "", "Deux");
        library.create_binder(&binder_path("Perso/Idées")).unwrap();
        assert!(library.root().join("Perso").join("Idées").is_dir());

        let moved =
            move_notes_in(&library, &[text(&a.path), text(&b.path)], "Classe/3e B").unwrap();
        assert_eq!(moved.len(), 2);
        for path in &moved {
            assert!(Path::new(path).is_file());
            assert!(Path::new(path).starts_with(library.root().join("Classe").join("3e B")));
        }
        assert!(!a.path.exists());
        // The file is not rewritten: its stamp is still valid.
        assert_eq!(FileStamp::of(Path::new(&moved[0])).unwrap(), a.stamp);

        let outside = tempfile::tempdir().unwrap();
        let error = move_notes_in(
            &library,
            &[text(&outside.path().join("2026-10-24 X.ardoise"))],
            "",
        )
        .unwrap_err();
        assert_eq!(error.code(), "notInLibrary");
    }

    #[test]
    fn trash_restore_purge_and_empty() {
        let (_dir, library) = library();
        let a = write_sample(&library, "Classe", "Un");
        let b = write_sample(&library, "", "Deux");
        let c = write_sample(&library, "", "Trois");
        for saved in [&a, &b, &c] {
            library.trash(&saved.path).unwrap();
        }
        let entries = trash_list_in(&library).unwrap();
        assert_eq!(entries.len(), 3);
        let un = entries
            .iter()
            .find(|e| e.title.as_deref() == Some("Un"))
            .unwrap();
        assert!(un.original.starts_with("Classe/"), "{}", un.original);
        assert!(un.note_id.is_some());
        assert!(un.deleted > 0);
        let json = serde_json::to_value(un).unwrap();
        for key in ["id", "title", "original", "deleted", "noteId"] {
            assert!(json.get(key).is_some(), "missing {key} in {json}");
        }

        let restored = library.restore(&un.id).unwrap();
        assert_eq!(restored, a.path);
        assert!(a.path.is_file());

        let deux = trash_list_in(&library)
            .unwrap()
            .into_iter()
            .find(|e| e.title.as_deref() == Some("Deux"))
            .unwrap();
        library.purge(&deux.id).unwrap();
        assert_eq!(trash_list_in(&library).unwrap().len(), 1);
        assert_eq!(library.empty_trash().unwrap(), 1);
        assert!(trash_list_in(&library).unwrap().is_empty());
        assert_eq!(library.restore(&deux.id).unwrap_err().code(), "notInTrash");
    }

    #[test]
    fn tags_and_favourite_need_the_current_stamp() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let stamp = library
            .set_tags(
                &saved.path,
                &[" chimie ".into(), "Chimie".into()],
                Some(&saved.stamp),
            )
            .unwrap();
        assert_eq!(
            library.read(&saved.path).unwrap().file.note.tags,
            ["chimie"]
        );

        // The listed stamp is now stale: refused, nothing changed.
        let error = library
            .set_favorite(&saved.path, true, Some(&saved.stamp))
            .unwrap_err();
        assert_eq!(error.code(), "noteConflict");
        assert!(!library.read(&saved.path).unwrap().file.note.favorite);

        let stamp = library
            .set_favorite(&saved.path, true, Some(&stamp))
            .unwrap();
        assert!(library.read(&saved.path).unwrap().file.note.favorite);
        assert_eq!(FileStamp::of(&saved.path).unwrap(), stamp);
    }

    #[test]
    fn retitling_renames_the_file() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "Perso", "Ancien");
        let retitled =
            retitle_in(&library, &text(&saved.path), "Nouveau titre", &saved.stamp).unwrap();
        assert!(
            retitled.path.ends_with(" Nouveau titre.ardoise"),
            "{}",
            retitled.path
        );
        assert!(!saved.path.exists());
        let loaded = library.read(Path::new(&retitled.path)).unwrap();
        assert_eq!(loaded.file.note.title, "Nouveau titre");
        assert_eq!(loaded.stamp, retitled.stamp);
        let json = serde_json::to_value(&retitled).unwrap();
        assert!(json["path"].is_string() && json["stamp"]["len"].is_u64());

        let error = retitle_in(&library, &retitled.path, "Encore", &saved.stamp).unwrap_err();
        assert_eq!(error.code(), "noteConflict");
    }

    #[test]
    fn binder_paths_from_the_interface() {
        assert_eq!(binder_path(""), PathBuf::new());
        assert_eq!(binder_path("Classe/3e B"), Path::new("Classe").join("3e B"));
        assert_eq!(binder_path("a\\b/"), Path::new("a").join("b"));
        assert_eq!(slash_path(&Path::new("Classe").join("3e B")), "Classe/3e B");
    }

    fn binders(listing: &LibraryListing) -> Vec<&str> {
        listing.binders.iter().map(String::as_str).collect()
    }

    #[test]
    fn the_automatic_export_folder_is_not_a_binder() {
        let (dir, library) = library();
        write_sample(&library, "Classe", "Cours");
        fs::create_dir_all(dir.path().join("Exports").join("Classe")).unwrap();
        fs::write(
            dir.path().join("Exports").join("Classe").join("Cours.pdf"),
            b"%PDF",
        )
        .unwrap();
        library.create_binder(Path::new("Exports2")).unwrap();

        let mut listing = list(&library);
        assert_eq!(
            binders(&listing),
            ["Classe", "Exports", "Exports/Classe", "Exports2"]
        );
        hide_export_folder(&mut listing, Some("Exports"));
        assert_eq!(binders(&listing), ["Classe", "Exports2"]);

        // Case does not matter, no folder hides nothing.
        let mut listing = list(&library);
        hide_export_folder(&mut listing, Some("exports"));
        assert_eq!(binders(&listing), ["Classe", "Exports2"]);
        let mut listing = list(&library);
        hide_export_folder(&mut listing, None);
        assert_eq!(listing.binders.len(), 4);
    }

    #[test]
    fn a_nested_export_folder_hides_only_itself_and_what_is_below() {
        let (_dir, library) = library();
        library
            .create_binder(Path::new("Lecture/Tel/2026"))
            .unwrap();
        library.create_binder(Path::new("Lecture/Notes")).unwrap();
        let mut listing = list(&library);
        hide_export_folder(&mut listing, Some("Lecture/Tel"));
        assert_eq!(binders(&listing), ["Lecture", "Lecture/Notes"]);
    }

    #[test]
    fn a_binder_with_notes_is_never_hidden() {
        let (_dir, library) = library();
        write_sample(&library, "Exports", "Mes notes");
        write_sample(&library, "Exports/Sous-dossier", "Autre");
        library.create_binder(Path::new("Exports/Vide")).unwrap();
        let mut listing = list(&library);
        hide_export_folder(&mut listing, Some("Exports"));
        assert_eq!(binders(&listing), ["Exports", "Exports/Sous-dossier"]);
        assert_eq!(listing.notes.len(), 2);
    }

    #[test]
    fn the_listing_command_follows_the_settings() {
        let settings = tempfile::tempdir().unwrap();
        let (dir, library) = library();
        let context = context(settings.path());
        crate::commands::choose_library(&context, &text(dir.path())).unwrap();
        library.create_binder(Path::new("Exports")).unwrap();
        library.create_binder(Path::new("Classe")).unwrap();

        let hidden = |context: &AppContext| list_for_interface(context).unwrap().binders;
        // The export is off: an empty binder named like the default folder stays visible.
        assert_eq!(hidden(&context), ["Classe", "Exports"]);
        let auto_export = |format, folder: &str| ardoise_core::settings::AutoExport {
            format,
            folder: folder.into(),
            ..Default::default()
        };
        use ardoise_core::settings::AutoExportFormat;
        crate::commands::set_auto_export_in(
            &context,
            auto_export(AutoExportFormat::Pdf, "Exports"),
        )
        .unwrap();
        assert_eq!(hidden(&context), ["Classe"]);
        crate::commands::set_auto_export_in(&context, auto_export(AutoExportFormat::Png, "Classe"))
            .unwrap();
        assert_eq!(hidden(&context), ["Exports"]);
        // Off again: everything is visible.
        crate::commands::set_auto_export_in(&context, auto_export(AutoExportFormat::Off, "Classe"))
            .unwrap();
        assert_eq!(hidden(&context), ["Classe", "Exports"]);
    }
}
