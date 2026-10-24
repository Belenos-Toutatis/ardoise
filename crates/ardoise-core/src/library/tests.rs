use std::fs::{self, File};
use std::sync::mpsc;
use std::time::Duration;

use super::*;
use crate::format::{FormatVersion, NoteFile};
use crate::testutil::blank_note;

/// 2026-10-24 at noon UTC: the same calendar day in every usual time zone.
const NOON: i64 = 1_792_843_200_000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

fn new_library() -> (tempfile::TempDir, Library) {
    let dir = tempfile::tempdir().unwrap();
    init(dir.path()).unwrap();
    let library = Library::open(dir.path()).unwrap();
    (dir, library)
}

fn note_file(id: &str, title: &str) -> NoteFile {
    NoteFile::new(blank_note(id, title))
}

fn names_in(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

/// Rewrites a file as another computer would, with a distinct modification time.
fn edit_elsewhere(path: &Path, title: &str) {
    let mut file = crate::format::read_note(path).unwrap();
    file.note.title = title.to_owned();
    fs::write(path, crate::format::to_bytes(&file).unwrap()).unwrap();
    let later = SystemTime::now() + Duration::from_secs(3);
    File::options()
        .write(true)
        .open(path)
        .unwrap()
        .set_modified(later)
        .unwrap();
}

#[test]
fn init_creates_marker_and_keeps_content() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("note.txt"), "keep me").unwrap();
    assert!(!is_library(dir.path()));

    init(dir.path()).unwrap();
    assert!(is_library(dir.path()));
    assert_eq!(
        fs::read_to_string(dir.path().join("note.txt")).unwrap(),
        "keep me"
    );

    // Initialising twice is harmless.
    init(dir.path()).unwrap();
    assert!(is_library(dir.path()));
}

#[test]
fn init_refuses_missing_folder() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing");
    let err = init(&missing).unwrap_err();
    assert_eq!(err.kind(), io::ErrorKind::NotFound);
    assert!(!missing.exists());
}

#[test]
fn open_requires_a_library() {
    let dir = tempfile::tempdir().unwrap();
    let error = Library::open(dir.path()).unwrap_err();
    assert!(matches!(error, LibraryError::NotALibrary { .. }));
    assert_eq!(error.code(), "notALibrary");
    init(dir.path()).unwrap();
    let library = Library::open(dir.path()).unwrap();
    assert_eq!(library.root(), dir.path());
    assert_eq!(library.meta_dir(), dir.path().join(".ardoise"));
}

#[test]
fn create_names_files_after_date_and_title() {
    let (dir, library) = new_library();
    let saved = library
        .create(Path::new(""), &note_file("a", "Titre de la note"))
        .unwrap();
    assert_eq!(
        saved.path,
        dir.path().join("2026-10-24 Titre de la note.ardoise")
    );
    assert_eq!(saved.stamp, FileStamp::of(&saved.path).unwrap());

    // Same title: unique names, never a replaced file.
    let second = library
        .create(Path::new(""), &note_file("b", "Titre de la note"))
        .unwrap();
    let third = library
        .create(Path::new(""), &note_file("c", "Titre de la note"))
        .unwrap();
    assert_eq!(
        second.path,
        dir.path().join("2026-10-24 Titre de la note (2).ardoise")
    );
    assert_eq!(
        third.path,
        dir.path().join("2026-10-24 Titre de la note (3).ardoise")
    );
    assert_eq!(library.read(&saved.path).unwrap().file.note.id, "a");
    assert_eq!(library.read(&second.path).unwrap().file.note.id, "b");

    // Sanitised title, binder created on the way.
    let odd = library
        .create(
            Path::new("Classe/3e B"),
            &note_file("d", "Cours 1/2 : « Pythagore » ?"),
        )
        .unwrap();
    assert_eq!(
        odd.path,
        dir.path()
            .join("Classe")
            .join("3e B")
            .join("2026-10-24 Cours 1-2 - « Pythagore ».ardoise")
    );
    let untitled = library
        .create(Path::new(""), &note_file("e", "  "))
        .unwrap();
    assert_eq!(untitled.path, dir.path().join("2026-10-24.ardoise"));
    // No temporary file is left anywhere.
    assert!(names_in(dir.path()).iter().all(|n| !n.ends_with(".part")));
}

#[test]
fn create_refuses_invalid_notes_and_binders() {
    let (dir, library) = new_library();
    let mut bad = note_file("a", "x");
    bad.note.layers.clear();
    assert!(matches!(
        library.create(Path::new("Nouveau"), &bad),
        Err(LibraryError::Format { .. })
    ));
    for binder in ["..", "../outside", ".hidden", "CON", "a:b", "fin."] {
        let error = library
            .create(Path::new(binder), &note_file("a", "x"))
            .unwrap_err();
        assert!(
            matches!(error, LibraryError::InvalidName { .. }),
            "{binder}"
        );
        assert_eq!(error.code(), "invalidName");
    }
    assert_eq!(names_in(dir.path()), [".ardoise"]);
}

#[test]
fn list_reads_metadata_and_reports_problems() {
    let (dir, library) = new_library();
    let mut tagged = note_file("id-1", "Premier");
    tagged.note.tags = vec!["maths".to_owned(), "3e".to_owned()];
    let first = library.create(Path::new(""), &tagged).unwrap();
    let second = library
        .create(Path::new("Classe"), &note_file("id-2", "Second"))
        .unwrap();
    fs::create_dir(dir.path().join("Vide")).unwrap();

    // Things that are not notes, or not to be listed.
    fs::write(dir.path().join("notes.txt"), b"x").unwrap();
    fs::write(dir.path().join(".hidden.ardoise"), b"x").unwrap();
    fs::write(
        dir.path().join(".2026-10-24 Premier.ardoise.1.0.part"),
        b"x",
    )
    .unwrap();
    fs::create_dir_all(library.trash_dir()).unwrap();
    fs::copy(&first.path, library.trash_dir().join("x.ardoise")).unwrap();

    // Files that cannot be read: reported, not fatal.
    let damaged = dir.path().join("Abîmée.ardoise");
    fs::write(&damaged, b"PK\x03\x04 broken").unwrap();
    let future = dir.path().join("Future.ardoise");
    let mut file = note_file("id-3", "Future");
    file.note.title = "Future".to_owned();
    fs::write(
        &future,
        crate::format::to_bytes_with_version(&file, FormatVersion { major: 2, minor: 0 }).unwrap(),
    )
    .unwrap();

    let listing = library.list();
    let paths: Vec<&Path> = listing.notes.iter().map(|n| n.path.as_path()).collect();
    assert_eq!(paths, [first.path.as_path(), second.path.as_path()]);
    let entry = &listing.notes[0];
    assert_eq!(entry.id, "id-1");
    assert_eq!(entry.title, "Premier");
    assert_eq!(entry.file_name, "2026-10-24 Premier.ardoise");
    assert_eq!(entry.binder, PathBuf::new());
    assert_eq!(entry.created, NOON);
    assert_eq!(entry.modified, NOON);
    assert_eq!(entry.tags, ["maths", "3e"]);
    assert_eq!(entry.stamp, first.stamp);
    assert_eq!(entry.conflict, None);
    assert_eq!(listing.notes[1].binder, PathBuf::from("Classe"));

    let problems: Vec<(&Path, &str)> = listing
        .problems
        .iter()
        .map(|p| (p.path.as_path(), p.error.code()))
        .collect();
    assert_eq!(
        problems,
        [
            (damaged.as_path(), "noteDamaged"),
            (future.as_path(), "noteNewerVersion")
        ]
    );
    assert!(
        listing.problems[1]
            .error
            .to_string()
            .contains("newer version of Ardoise")
    );

    assert_eq!(
        library.binders(),
        [PathBuf::from("Classe"), PathBuf::from("Vide")]
    );
}

#[test]
fn save_checks_the_modification_time() {
    let (_dir, library) = new_library();
    let saved = library
        .create(Path::new(""), &note_file("a", "Cours"))
        .unwrap();
    let mut loaded = library.read(&saved.path).unwrap();
    assert_eq!(loaded.stamp, saved.stamp);

    loaded.file.note.tags = vec!["v2".to_owned()];
    let stamp = library
        .save(&loaded.path, &loaded.file, &loaded.stamp)
        .unwrap();
    assert_ne!(stamp, loaded.stamp);
    assert_eq!(stamp, FileStamp::of(&loaded.path).unwrap());
    assert_eq!(library.read(&saved.path).unwrap().file.note.tags, ["v2"]);

    // Saving again with the old stamp is refused: someone (here, us) wrote in between.
    let error = library
        .save(&loaded.path, &loaded.file, &loaded.stamp)
        .unwrap_err();
    assert!(matches!(error, LibraryError::Conflict { .. }));
    assert_eq!(error.code(), "noteConflict");

    // Another computer saves; Nextcloud brings its version. Our save must not overwrite it.
    edit_elsewhere(&saved.path, "Version de l'autre PC");
    let error = library.save(&saved.path, &loaded.file, &stamp).unwrap_err();
    assert!(matches!(error, LibraryError::Conflict { .. }));
    assert_eq!(
        library.read(&saved.path).unwrap().file.note.title,
        "Version de l'autre PC"
    );

    // Deleted elsewhere: reported as such, and nothing is recreated.
    fs::remove_file(&saved.path).unwrap();
    let error = library.save(&saved.path, &loaded.file, &stamp).unwrap_err();
    assert!(matches!(error, LibraryError::NotFound { .. }));
    assert_eq!(error.code(), "noteNotFound");
    assert!(!saved.path.exists());
}

#[test]
fn paths_outside_the_library_are_refused() {
    let (dir, library) = new_library();
    let saved = library
        .create(Path::new(""), &note_file("a", "Cours"))
        .unwrap();
    let file = note_file("a", "Cours");
    let elsewhere = tempfile::tempdir().unwrap();
    for path in [
        elsewhere.path().join("x.ardoise"),
        dir.path().join(".ardoise").join("x.ardoise"),
        dir.path().join("notes.txt"),
        dir.path().join("..").join("x.ardoise"),
        PathBuf::from("../x.ardoise"),
        dir.path().to_path_buf(),
    ] {
        let error = library.save(&path, &file, &saved.stamp).unwrap_err();
        assert!(
            matches!(error, LibraryError::NotInLibrary { .. }),
            "{path:?}: {error:?}"
        );
        assert_eq!(error.code(), "notInLibrary");
    }
    // Relative paths are relative to the root.
    assert_eq!(
        library
            .note_path(Path::new("2026-10-24 Cours.ardoise"))
            .unwrap(),
        saved.path
    );
}

#[test]
fn retitle_renames_the_file() {
    let (dir, library) = new_library();
    let mut draft = note_file("a", "Brouillon");
    draft.note.modified = 0;
    let saved = library.create(Path::new("Classe"), &draft).unwrap();
    let before = now_ms();
    let renamed = library
        .retitle(&saved.path, "Cours: définitive", Some(&saved.stamp))
        .unwrap();
    let expected = dir
        .path()
        .join("Classe")
        .join("2026-10-24 Cours- définitive.ardoise");
    assert_eq!(renamed.path, expected);
    assert!(!saved.path.exists());
    let loaded = library.read(&renamed.path).unwrap();
    assert_eq!(loaded.file.note.title, "Cours: définitive");
    assert!(loaded.file.note.modified >= before);
    assert_eq!(loaded.stamp, renamed.stamp);

    // Same title again: nothing moves.
    let same = library
        .retitle(&renamed.path, "Cours: définitive", None)
        .unwrap();
    assert_eq!(same.path, expected);

    // A taken name gets a suffix; renaming it back to its title keeps the suffix.
    let other = library
        .create(Path::new("Classe"), &note_file("b", "Autre"))
        .unwrap();
    let clash = library
        .retitle(&other.path, "Cours: définitive", None)
        .unwrap();
    assert_eq!(
        clash.path,
        dir.path()
            .join("Classe")
            .join("2026-10-24 Cours- définitive (2).ardoise")
    );
    let kept = library
        .retitle(&clash.path, "Cours: définitive", None)
        .unwrap();
    assert_eq!(kept.path, clash.path);
    assert_eq!(library.read(&expected).unwrap().file.note.id, "a");

    // Only the case changes.
    let cased = library
        .retitle(&expected, "COURS: DÉFINITIVE", None)
        .unwrap();
    assert_eq!(
        cased.path.file_name().unwrap(),
        "2026-10-24 COURS- DÉFINITIVE.ardoise"
    );
    assert_eq!(library.read(&cased.path).unwrap().file.note.id, "a");
    assert_eq!(library.list().notes.len(), 2);

    // A stale stamp is a conflict and changes nothing.
    let error = library
        .retitle(&cased.path, "Autre titre", Some(&saved.stamp))
        .unwrap_err();
    assert!(matches!(error, LibraryError::Conflict { .. }));
    assert!(cased.path.exists());
}

#[test]
fn move_to_another_binder() {
    let (dir, library) = new_library();
    let saved = library
        .create(Path::new(""), &note_file("a", "Cours"))
        .unwrap();
    let moved = library
        .move_to(&saved.path, Path::new("Classe/3e B"))
        .unwrap();
    assert_eq!(
        moved,
        dir.path()
            .join("Classe")
            .join("3e B")
            .join("2026-10-24 Cours.ardoise")
    );
    assert!(!saved.path.exists());
    // Not rewritten: the stamp loaded before the move is still valid for saving.
    assert_eq!(FileStamp::of(&moved).unwrap(), saved.stamp);
    library
        .save(&moved, &note_file("a", "Cours"), &saved.stamp)
        .unwrap();

    // Same binder: nothing happens.
    assert_eq!(
        library.move_to(&moved, Path::new("Classe/3e B")).unwrap(),
        moved
    );
    #[cfg(windows)]
    assert_eq!(
        library.move_to(&moved, Path::new("classe/3E b")).unwrap(),
        moved
    );

    // Name taken in the target binder.
    let twin = library
        .create(Path::new(""), &note_file("b", "Cours"))
        .unwrap();
    let moved_twin = library
        .move_to(&twin.path, Path::new("Classe/3e B"))
        .unwrap();
    assert_eq!(
        moved_twin.file_name().unwrap(),
        "2026-10-24 Cours (2).ardoise"
    );

    // Back to the root.
    let back = library.move_to(&moved_twin, Path::new("")).unwrap();
    assert_eq!(back, dir.path().join("2026-10-24 Cours (2).ardoise"));

    assert!(matches!(
        library.move_to(&saved.path, Path::new("")),
        Err(LibraryError::NotFound { .. })
    ));
    assert!(matches!(
        library.move_to(&back, Path::new("../out")),
        Err(LibraryError::InvalidName { .. })
    ));
}

#[test]
fn tags_are_normalised_and_counted() {
    let (_dir, library) = new_library();
    let a = library.create(Path::new(""), &note_file("a", "A")).unwrap();
    let b = library
        .create(Path::new("Classe"), &note_file("b", "B"))
        .unwrap();
    let stamp = library
        .set_tags(
            &a.path,
            &[
                " Maths ".to_owned(),
                "3e  B".to_owned(),
                "maths".to_owned(),
                "".to_owned(),
            ],
            Some(&a.stamp),
        )
        .unwrap();
    assert_eq!(
        library.read(&a.path).unwrap().file.note.tags,
        ["Maths", "3e B"]
    );
    library
        .set_tags(&b.path, &["MATHS".to_owned(), "physique".to_owned()], None)
        .unwrap();
    assert_eq!(
        library.all_tags(),
        [
            TagCount {
                tag: "3e B".to_owned(),
                count: 1
            },
            TagCount {
                tag: "Maths".to_owned(),
                count: 2
            },
            TagCount {
                tag: "physique".to_owned(),
                count: 1
            },
        ]
    );
    // A stale stamp is refused.
    assert!(matches!(
        library.set_tags(&a.path, &[], Some(&a.stamp)),
        Err(LibraryError::Conflict { .. })
    ));
    library.set_tags(&a.path, &[], Some(&stamp)).unwrap();
    assert!(library.read(&a.path).unwrap().file.note.tags.is_empty());
}

#[test]
fn unknown_content_survives_library_operations() {
    let (_dir, library) = new_library();
    let mut file = note_file("a", "Cours");
    file.original = Some(vec![1, 2, 3]);
    file.unknown_fields
        .insert("future".to_owned(), serde_json::json!({"x": 1}));
    file.unknown_members.push(crate::format::Member {
        name: "future.bin".to_owned(),
        bytes: vec![9; 10],
    });
    let saved = library.create(Path::new(""), &file).unwrap();
    library
        .set_tags(&saved.path, &["t".to_owned()], None)
        .unwrap();
    let renamed = library.retitle(&saved.path, "Renommée", None).unwrap();
    let loaded = library.read(&renamed.path).unwrap().file;
    assert_eq!(loaded.original, file.original);
    assert_eq!(loaded.unknown_fields, file.unknown_fields);
    assert_eq!(loaded.unknown_members, file.unknown_members);
}

#[test]
fn trash_restore_and_purge() {
    let (dir, library) = new_library();
    assert!(library.trash_items().unwrap().is_empty());
    let saved = library
        .create(Path::new("Classe"), &note_file("id-a", "Cours"))
        .unwrap();
    let bytes = fs::read(&saved.path).unwrap();

    let item = library.trash(&saved.path).unwrap();
    assert!(!saved.path.exists());
    assert!(library.list().notes.is_empty());
    assert_eq!(
        item.original,
        PathBuf::from("Classe/2026-10-24 Cours.ardoise")
    );
    assert_eq!(item.title.as_deref(), Some("Cours"));
    assert_eq!(item.note_id.as_deref(), Some("id-a"));
    assert!(
        item.id.ends_with(" 2026-10-24 Cours.ardoise"),
        "{}",
        item.id
    );
    assert_eq!(item.path, library.trash_dir().join(&item.id));
    // The bytes are untouched.
    assert_eq!(fs::read(&item.path).unwrap(), bytes);
    assert_eq!(library.trash_items().unwrap(), std::slice::from_ref(&item));

    // Restore to the original place, recreating the binder.
    fs::remove_dir(dir.path().join("Classe")).unwrap();
    let restored = library.restore(&item.id).unwrap();
    assert_eq!(restored, saved.path);
    assert_eq!(fs::read(&restored).unwrap(), bytes);
    assert!(library.trash_items().unwrap().is_empty());
    assert!(
        names_in(&library.trash_dir()).is_empty(),
        "record left behind"
    );

    // Name taken meanwhile: restored with a suffix.
    let item = library.trash(&restored).unwrap();
    library
        .create(Path::new("Classe"), &note_file("id-b", "Cours"))
        .unwrap();
    let restored = library.restore(&item.id).unwrap();
    assert_eq!(
        restored.file_name().unwrap(),
        "2026-10-24 Cours (2).ardoise"
    );
    assert_eq!(library.read(&restored).unwrap().file.note.id, "id-a");

    // Purge: only what is older than 30 days.
    let item = library.trash(&restored).unwrap();
    assert_eq!(
        library
            .purge_expired_at(item.deleted + 29 * DAY_MS)
            .unwrap(),
        0
    );
    assert_eq!(library.trash_items().unwrap().len(), 1);
    assert_eq!(
        library
            .purge_expired_at(item.deleted + 31 * DAY_MS)
            .unwrap(),
        1
    );
    assert!(library.trash_items().unwrap().is_empty());
    assert!(names_in(&library.trash_dir()).is_empty());
    assert_eq!(library.purge_expired().unwrap(), 0);

    let error = library.restore(&item.id).unwrap_err();
    assert!(matches!(error, LibraryError::NotInTrash { .. }));
    assert_eq!(error.code(), "notInTrash");
}

#[test]
fn trash_purge_now_and_odd_contents() {
    let (dir, library) = new_library();
    let a = library.create(Path::new(""), &note_file("a", "A")).unwrap();
    let b = library.create(Path::new(""), &note_file("b", "B")).unwrap();
    let c = library.create(Path::new(""), &note_file("c", "C")).unwrap();
    let item_a = library.trash(&a.path).unwrap();
    library.trash(&b.path).unwrap();
    library.trash(&c.path).unwrap();
    assert_eq!(library.trash_items().unwrap().len(), 3);

    library.purge(&item_a.id).unwrap();
    assert_eq!(library.trash_items().unwrap().len(), 2);
    for bad in [
        "../x.ardoise",
        "..\\x.ardoise",
        "C:x.ardoise",
        "nothing.ardoise",
        "",
    ] {
        assert!(
            matches!(library.purge(bad), Err(LibraryError::NotInTrash { .. })),
            "{bad}"
        );
        assert!(matches!(
            library.restore(bad),
            Err(LibraryError::NotInTrash { .. })
        ));
    }

    // A note put in the trash by hand (no record), and a record left by an interruption.
    fs::write(
        library.trash_dir().join("Perdue.ardoise"),
        crate::format::to_bytes(&note_file("p", "P")).unwrap(),
    )
    .unwrap();
    fs::write(
        library.trash_dir().join("1 Orpheline.ardoise.json"),
        br#"{"original": "Orpheline.ardoise", "deleted": 1}"#,
    )
    .unwrap();
    // A record pointing outside the library is not followed.
    fs::write(
        library.trash_dir().join("1792843200000 Evil.ardoise"),
        crate::format::to_bytes(&note_file("e", "E")).unwrap(),
    )
    .unwrap();
    fs::write(
        library.trash_dir().join("1792843200000 Evil.ardoise.json"),
        br#"{"original": "../../Evil.ardoise", "deleted": 1792843200000}"#,
    )
    .unwrap();
    let items = library.trash_items().unwrap();
    let lost = items.iter().find(|i| i.id == "Perdue.ardoise").unwrap();
    assert_eq!(lost.original, PathBuf::from("Perdue.ardoise"));
    let evil = items
        .iter()
        .find(|i| i.id.ends_with("Evil.ardoise"))
        .unwrap();
    assert_eq!(evil.original, PathBuf::from("Evil.ardoise"));
    assert_eq!(
        library.restore(&evil.id).unwrap(),
        dir.path().join("Evil.ardoise")
    );
    assert_eq!(
        library.restore("Perdue.ardoise").unwrap(),
        dir.path().join("Perdue.ardoise")
    );

    assert_eq!(library.empty_trash().unwrap(), 2);
    assert!(names_in(&library.trash_dir()).is_empty());
}

#[test]
fn conflict_copies_are_paired_with_their_original() {
    let (dir, library) = new_library();
    let saved = library
        .create(Path::new("Classe"), &note_file("a", "Cours"))
        .unwrap();
    let copy = dir
        .path()
        .join("Classe")
        .join("2026-10-24 Cours (conflicted copy 2026-10-24 101530).ardoise");
    fs::copy(&saved.path, &copy).unwrap();
    let orphan = dir
        .path()
        .join("2026-10-24 Perdue (conflicted copy camille 2026-10-25 080000).ardoise");
    fs::copy(&saved.path, &orphan).unwrap();

    let listing = library.list();
    assert_eq!(listing.notes.len(), 3);
    let conflicts = listing.conflicts();
    assert_eq!(
        conflicts,
        [
            ConflictCopy {
                copy: orphan.clone(),
                original: dir.path().join("2026-10-24 Perdue.ardoise"),
                original_exists: false,
                kind: ConflictKind::Nextcloud,
                marker: "(conflicted copy camille 2026-10-25 080000)".to_owned(),
            },
            ConflictCopy {
                copy: copy.clone(),
                original: saved.path.clone(),
                original_exists: true,
                kind: ConflictKind::Nextcloud,
                marker: "(conflicted copy 2026-10-24 101530)".to_owned(),
            },
        ]
    );
    let entry = listing.notes.iter().find(|n| n.path == copy).unwrap();
    assert_eq!(
        entry.conflict.as_ref().map(|c| c.original_name.as_str()),
        Some("2026-10-24 Cours.ardoise")
    );
}

#[test]
fn error_codes_are_stable() {
    let path = PathBuf::from("x");
    let io = || io::Error::other("x");
    let cases: Vec<(LibraryError, &str)> = vec![
        (
            LibraryError::Io {
                path: path.clone(),
                source: io(),
            },
            "libraryAccess",
        ),
        (
            LibraryError::NoFreeName { path: path.clone() },
            "noFreeName",
        ),
        (
            LibraryError::Watch {
                path: path.clone(),
                message: String::new(),
            },
            "libraryWatch",
        ),
        (
            LibraryError::Format {
                path: path.clone(),
                source: FormatError::NewerVersion {
                    found: FormatVersion { major: 2, minor: 0 },
                },
            },
            "noteNewerVersion",
        ),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error}");
    }
}

// ---------------------------------------------------------------------------------------
// Watching (real folder, real notifications)

const DEBOUNCE: Duration = Duration::from_millis(100);
const PATIENCE: Duration = Duration::from_secs(15);

struct Watched {
    _watcher: LibraryWatcher,
    changes: mpsc::Receiver<Vec<LibraryChange>>,
    seen: Vec<LibraryChange>,
}

impl Watched {
    fn start(library: &Library) -> Watched {
        let (sender, changes) = mpsc::channel();
        let watcher = library
            .watch(DEBOUNCE, move |batch| {
                let _ = sender.send(batch);
            })
            .unwrap();
        Watched {
            _watcher: watcher,
            changes,
            seen: Vec::new(),
        }
    }

    /// Collects changes until `wanted` has been seen; returns everything seen meanwhile.
    fn wait_for(&mut self, wanted: &LibraryChange) -> Vec<LibraryChange> {
        let deadline = std::time::Instant::now() + PATIENCE;
        while !self.seen.contains(wanted) {
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            match self.changes.recv_timeout(left) {
                Ok(batch) => {
                    // One change per file per batch.
                    let mut paths: Vec<&Path> = batch
                        .iter()
                        .filter(|c| !matches!(c, LibraryChange::Changed { .. }))
                        .map(LibraryChange::path)
                        .collect();
                    paths.sort();
                    let before = paths.len();
                    paths.dedup();
                    assert_eq!(before, paths.len(), "duplicates in {batch:?}");
                    self.seen.extend(batch);
                }
                Err(_) => panic!("{wanted:?} not reported; seen {:?}", self.seen),
            }
        }
        std::mem::take(&mut self.seen)
    }
}

#[test]
fn watch_reports_note_changes() {
    let (dir, library) = new_library();
    let existing = library
        .create(Path::new(""), &note_file("x", "Existante"))
        .unwrap();
    let mut watched = Watched::start(&library);

    // Things that must never be reported: written before the note, checked after.
    fs::write(dir.path().join(".ardoise").join("diagnostics.log"), b"log").unwrap();
    fs::write(dir.path().join(".tmp.ardoise.1.0.part"), b"partial").unwrap();
    fs::write(dir.path().join("notes.txt"), b"text").unwrap();

    let created = library
        .create(Path::new("Classe"), &note_file("a", "Cours"))
        .unwrap();
    let seen = watched.wait_for(&LibraryChange::Created {
        path: created.path.clone(),
    });
    assert!(
        seen.iter().all(|c| c.path() == created.path),
        "unexpected changes: {seen:?}"
    );

    // Saved (atomically replaced).
    let loaded = library.read(&created.path).unwrap();
    library
        .save(&created.path, &loaded.file, &loaded.stamp)
        .unwrap();
    watched.wait_for(&LibraryChange::Changed {
        path: created.path.clone(),
    });

    // Retitled: renamed (and rewritten).
    let renamed = library.retitle(&created.path, "Cours final", None).unwrap();
    watched.wait_for(&LibraryChange::Renamed {
        from: created.path.clone(),
        to: renamed.path.clone(),
    });

    // Trashed: removed from the library's point of view.
    library.trash(&existing.path).unwrap();
    let seen = watched.wait_for(&LibraryChange::Removed {
        path: existing.path.clone(),
    });
    assert!(
        seen.iter()
            .all(|c| !c.path().starts_with(library.meta_dir())),
        "{seen:?}"
    );
}

#[test]
fn watch_follows_renamed_binders_and_external_copies() {
    let (dir, library) = new_library();
    let a = library
        .create(Path::new("Ancien"), &note_file("a", "A"))
        .unwrap();
    let mut watched = Watched::start(&library);

    fs::rename(dir.path().join("Ancien"), dir.path().join("Nouveau")).unwrap();
    let moved = dir.path().join("Nouveau").join("2026-10-24 A.ardoise");
    watched.wait_for(&LibraryChange::Renamed {
        from: a.path.clone(),
        to: moved.clone(),
    });

    // A file dropped in by another program (or synced by Nextcloud).
    let copy = dir.path().join("Copie.ardoise");
    fs::copy(&moved, &copy).unwrap();
    watched.wait_for(&LibraryChange::Created { path: copy.clone() });
    fs::remove_file(&copy).unwrap();
    watched.wait_for(&LibraryChange::Removed { path: copy });
}

#[test]
fn watching_stops_when_dropped() {
    let (dir, library) = new_library();
    let (sender, changes) = mpsc::channel();
    let watcher = library
        .watch(DEBOUNCE, move |batch| {
            let _ = sender.send(batch);
        })
        .unwrap();
    drop(watcher);
    fs::write(
        dir.path().join("Après.ardoise"),
        crate::format::to_bytes(&note_file("a", "A")).unwrap(),
    )
    .unwrap();
    // The callback (and its sender) is gone with the watcher.
    assert!(matches!(
        changes.recv_timeout(DEBOUNCE * 5),
        Err(mpsc::RecvTimeoutError::Disconnected)
    ));

    let missing = Library {
        root: dir.path().join("missing"),
    };
    assert!(matches!(
        missing.watch(DEBOUNCE, |_| {}),
        Err(LibraryError::Watch { .. })
    ));
}
