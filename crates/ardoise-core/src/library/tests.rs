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
    edit_elsewhere_ahead(path, title, Duration::from_secs(3));
}

/// [`edit_elsewhere`], with a modification time `ahead` of now.
fn edit_elsewhere_ahead(path: &Path, title: &str, ahead: Duration) {
    let mut file = crate::format::read_note(path).unwrap();
    file.note.title = title.to_owned();
    fs::write(path, crate::format::to_bytes(&file).unwrap()).unwrap();
    let later = SystemTime::now() + ahead;
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
    assert_eq!(library.meta_dir(), dir.path().join("_ardoise"));
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
    // Including names Nextcloud never synchronises: a binder ending in `.part` would also be
    // skipped by the listing, and its notes would vanish from the app.
    for binder in [
        "..",
        "../outside",
        ".hidden",
        "CON",
        "a:b",
        "fin.",
        "Classe.part",
        "Classe/3e.PART",
        "Brouillon~",
        "Thumbs.db",
    ] {
        let error = library
            .create(Path::new(binder), &note_file("a", "x"))
            .unwrap_err();
        assert!(
            matches!(error, LibraryError::InvalidName { .. }),
            "{binder}"
        );
        assert_eq!(error.code(), "invalidName");
    }
    assert_eq!(names_in(dir.path()), ["_ardoise"]);
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

/// Nextcloud brings another computer's version after the first check, once the new version
/// is written next to the note but before it is renamed over it: the save must notice.
#[test]
fn save_rechecks_once_the_new_version_is_written() {
    let (dir, library) = new_library();
    let saved = library
        .create(Path::new(""), &note_file("a", "Cours"))
        .unwrap();
    let loaded = library.read(&saved.path).unwrap();
    let mut ours = loaded.file.clone();
    ours.note.title = "Ma version".to_owned();

    let mut theirs: Option<Vec<u8>> = None;
    let error = library
        .save_with(&loaded.path, &ours, &loaded.stamp, || {
            if theirs.is_none() {
                // Our new version is complete and waiting to be renamed.
                let pending: Vec<String> = names_in(dir.path())
                    .into_iter()
                    .filter(|n| n.ends_with(".part"))
                    .collect();
                assert_eq!(pending.len(), 1, "{pending:?}");
                edit_elsewhere(&saved.path, "Version de l'autre PC");
                theirs = Some(fs::read(&saved.path).unwrap());
            }
        })
        .unwrap_err();
    assert!(matches!(error, LibraryError::Conflict { .. }), "{error:?}");
    assert_eq!(Some(fs::read(&saved.path).unwrap()), theirs);
    assert_eq!(
        names_in(dir.path()),
        ["2026-10-24 Cours.ardoise", "_ardoise"],
        "temporary file left behind"
    );

    // Deleted at that moment: reported as such, nothing recreated.
    let loaded = library.read(&saved.path).unwrap();
    let error = library
        .save_with(&loaded.path, &ours, &loaded.stamp, || {
            let _ = fs::remove_file(&saved.path);
        })
        .unwrap_err();
    assert!(matches!(error, LibraryError::NotFound { .. }), "{error:?}");
    assert_eq!(names_in(dir.path()), ["_ardoise"]);
}

/// The file is replaced while it is read, again and again: the stamp returned must never be
/// newer than the content, or a later save would overwrite the newer version.
#[test]
fn read_never_returns_a_stamp_newer_than_its_content() {
    let (_dir, library) = new_library();
    let saved = library
        .create(Path::new(""), &note_file("a", "Version 0"))
        .unwrap();
    let mut version = 0;
    let loaded = library
        .read_with(&saved.path, |path| {
            version += 1;
            edit_elsewhere_ahead(
                path,
                &format!("Version {version}"),
                Duration::from_secs(3 + version),
            );
        })
        .unwrap();
    assert_eq!(version, 3, "attempts");
    // The last attempt read version 2 while version 3 was landing.
    assert_eq!(loaded.file.note.title, "Version 2");
    assert_ne!(loaded.stamp, FileStamp::of(&saved.path).unwrap());
    let error = library
        .save(&loaded.path, &loaded.file, &loaded.stamp)
        .unwrap_err();
    assert!(matches!(error, LibraryError::Conflict { .. }), "{error:?}");
    assert_eq!(
        library.read(&saved.path).unwrap().file.note.title,
        "Version 3"
    );
}

/// Nextcloud brings another computer's version while the save waits for a sync client or an
/// antivirus to release the file: the save must not replace it.
#[cfg(windows)]
#[test]
fn save_rechecks_after_waiting_for_a_locked_file() {
    use std::io::{Seek, Write};
    use std::os::windows::fs::OpenOptionsExt;

    let (dir, library) = new_library();
    let saved = library
        .create(Path::new(""), &note_file("a", "Cours"))
        .unwrap();
    let loaded = library.read(&saved.path).unwrap();
    let mut theirs = loaded.file.clone();
    theirs.note.title = "Version de l'autre PC".to_owned();
    theirs.note.tags = vec!["leur version".to_owned()];
    let their_bytes = crate::format::to_bytes(&theirs).unwrap();

    // A handle that shares nothing, as a sync client writing the file holds.
    let mut lock = File::options()
        .read(true)
        .write(true)
        .share_mode(0)
        .open(&saved.path)
        .unwrap();
    let writer_bytes = their_bytes.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        lock.set_len(0).unwrap();
        lock.rewind().unwrap();
        lock.write_all(&writer_bytes).unwrap();
        lock.set_modified(SystemTime::now() + Duration::from_secs(3))
            .unwrap();
        drop(lock);
    });
    let mut ours = loaded.file.clone();
    ours.note.title = "Ma version".to_owned();
    let error = library
        .save(&loaded.path, &ours, &loaded.stamp)
        .unwrap_err();
    writer.join().unwrap();
    assert!(matches!(error, LibraryError::Conflict { .. }), "{error:?}");
    assert_eq!(fs::read(&saved.path).unwrap(), their_bytes);
    assert_eq!(
        names_in(dir.path()),
        ["2026-10-24 Cours.ardoise", "_ardoise"]
    );
}

/// Every operation works in a binder whose path is over 300 characters long.
#[test]
fn long_paths_work_for_every_operation() {
    let (dir, library) = new_library();
    let mut binder = PathBuf::new();
    while dir.path().join(&binder).to_string_lossy().chars().count() < 300 {
        binder.push("Classeur au nom très long pour dépasser les deux cent soixante caractères");
    }
    let mut other = binder.clone();
    other.push("Autre");

    let saved = library.create(&binder, &note_file("a", "Cours")).unwrap();
    assert!(saved.path.as_os_str().len() > 300);
    let twin = library.create(&binder, &note_file("b", "Cours")).unwrap();
    assert!(twin.path.ends_with("2026-10-24 Cours (2).ardoise"));
    let loaded = library.read(&saved.path).unwrap();
    library
        .save(&saved.path, &loaded.file, &loaded.stamp)
        .unwrap();
    let renamed = library.retitle(&saved.path, "Cours final", None).unwrap();
    assert!(renamed.path.ends_with("2026-10-24 Cours final.ardoise"));
    let cased = library.retitle(&renamed.path, "COURS FINAL", None).unwrap();
    assert!(cased.path.ends_with("2026-10-24 COURS FINAL.ardoise"));
    let moved = library.move_to(&cased.path, &other).unwrap();
    assert_eq!(moved.parent(), Some(dir.path().join(&other).as_path()));
    let item = library.trash(&moved).unwrap();
    assert!(!moved.exists());
    let restored = library.restore(&item.id).unwrap();
    assert_eq!(restored, moved);
    assert_eq!(library.read(&restored).unwrap().file.note.id, "a");
    assert_eq!(library.list().notes.len(), 2);
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
        dir.path().join("_ardoise").join("x.ardoise"),
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
fn a_note_that_is_not_in_the_library_can_be_put_in_the_trash() {
    let (dir, library) = new_library();
    let note = library
        .create(Path::new("Classe"), &note_file("note-1", "Cours"))
        .unwrap();
    let mut copy = library.read(&note.path).unwrap().file;
    copy.note.id = "copy-1".into();
    copy.note.title = "Cours (before straightening)".into();
    let beside = note
        .path
        .with_file_name("Cours (before straightening).ardoise");

    // Twice: both are kept, under two names of the trash, and nothing appears in the binder.
    let first = library.trash_copy(&copy, &beside).unwrap();
    let second = library.trash_copy(&copy, &beside).unwrap();
    assert_ne!(first.id, second.id);
    assert_eq!(
        first.original,
        Path::new("Classe/Cours (before straightening).ardoise")
    );
    assert_eq!(first.title.as_deref(), Some("Cours (before straightening)"));
    assert_eq!(first.note_id.as_deref(), Some("copy-1"));
    assert_eq!(library.trash_items().unwrap().len(), 2);
    assert_eq!(library.list().notes.len(), 1);
    assert_eq!(names_in(&dir.path().join("Classe")).len(), 1);

    // Restored next to the note, with the content given; the note itself is untouched.
    let restored = library.restore(&first.id).unwrap();
    assert_eq!(restored, beside);
    assert_eq!(library.read(&restored).unwrap().file.note, copy.note);
    let again = library.restore(&second.id).unwrap();
    assert_eq!(
        again.file_name().unwrap(),
        "Cours (before straightening) (2).ardoise"
    );
    assert_eq!(library.read(&note.path).unwrap().file.note.id, "note-1");

    // Only a note path of this library.
    let outside = dir.path().join("..").join("x.ardoise");
    assert!(library.trash_copy(&copy, &outside).is_err());
    assert!(
        library
            .trash_copy(&copy, Path::new("_ardoise/x.ardoise"))
            .is_err()
    );
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
        watcher.wait_until_scanned();
        Watched {
            _watcher: watcher,
            changes,
            seen: Vec::new(),
        }
    }

    /// Collects changes until `wanted` has been seen; returns everything seen meanwhile.
    fn wait_for(&mut self, wanted: &LibraryChange) -> Vec<LibraryChange> {
        self.wait_until(wanted, |c| c == wanted)
    }

    /// Collects changes until one satisfies `wanted` (described by `what`); returns
    /// everything seen meanwhile.
    fn wait_until(
        &mut self,
        what: &dyn std::fmt::Debug,
        wanted: impl Fn(&LibraryChange) -> bool,
    ) -> Vec<LibraryChange> {
        let deadline = std::time::Instant::now() + PATIENCE;
        while !self.seen.iter().any(&wanted) {
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
                Err(_) => panic!("{what:?} not reported; seen {:?}", self.seen),
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
    fs::write(dir.path().join("_ardoise").join("diagnostics.log"), b"log").unwrap();
    fs::write(dir.path().join(".tmp.ardoise.1.0.part"), b"partial").unwrap();
    fs::write(dir.path().join("notes.txt"), b"text").unwrap();

    let created = library
        .create(Path::new("Classe"), &note_file("a", "Cours"))
        .unwrap();
    // Every change carries the file's stamp: equal to the one our own write returned, it
    // tells the application that the change is its own.
    let seen = watched.wait_for(&LibraryChange::Created {
        path: created.path.clone(),
        stamp: created.stamp,
    });
    assert!(
        seen.iter().all(|c| c.path() == created.path),
        "unexpected changes: {seen:?}"
    );

    // Saved (atomically replaced).
    let loaded = library.read(&created.path).unwrap();
    let stamp = library
        .save(&created.path, &loaded.file, &loaded.stamp)
        .unwrap();
    assert_ne!(stamp, created.stamp);
    watched.wait_for(&LibraryChange::Changed {
        path: created.path.clone(),
        stamp,
    });

    // Retitled: renamed (and rewritten).
    let renamed = library.retitle(&created.path, "Cours final", None).unwrap();
    watched.wait_for(&LibraryChange::Renamed {
        from: created.path.clone(),
        to: renamed.path.clone(),
        stamp: renamed.stamp,
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
        stamp: a.stamp,
    });

    // A file dropped in by another program (or synced by Nextcloud). It may be seen while it
    // is being written, so only its path is awaited.
    let copy = dir.path().join("Copie.ardoise");
    fs::copy(&moved, &copy).unwrap();
    watched.wait_until(
        &copy,
        |c| matches!(c, LibraryChange::Created { path, .. } if *path == copy),
    );
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

#[test]
fn an_old_hidden_meta_folder_is_renamed() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join(LEGACY_META_DIR);
    fs::create_dir_all(legacy.join("diagnostics")).unwrap();
    fs::write(legacy.join("device.json"), b"{\"uuid\":\"x\"}").unwrap();
    fs::write(legacy.join("diagnostics").join("a.jsonl"), b"log").unwrap();
    assert!(
        is_library(dir.path()),
        "still recognised before the migration"
    );

    let library = Library::open(dir.path()).unwrap();
    assert!(!legacy.exists());
    let meta = library.meta_dir();
    assert_eq!(
        fs::read(meta.join("device.json")).unwrap(),
        b"{\"uuid\":\"x\"}"
    );
    assert_eq!(
        fs::read(meta.join("diagnostics").join("a.jsonl")).unwrap(),
        b"log"
    );
    // Nothing left to do the second time.
    assert!(!migrate_legacy_meta_dir(dir.path()).unwrap());
}

#[test]
fn old_and_new_meta_folders_are_merged_without_overwriting() {
    let dir = tempfile::tempdir().unwrap();
    let legacy = dir.path().join(LEGACY_META_DIR);
    let current = dir.path().join(META_DIR);
    fs::create_dir_all(legacy.join("diagnostics")).unwrap();
    fs::create_dir_all(current.join("diagnostics")).unwrap();
    // The synced registration (current) wins over the one that never left this PC.
    fs::write(legacy.join("device.json"), b"old").unwrap();
    fs::write(current.join("device.json"), b"new").unwrap();
    fs::write(legacy.join("diagnostics").join("from-this-pc.jsonl"), b"a").unwrap();
    fs::write(
        current.join("diagnostics").join("from-other-pc.jsonl"),
        b"b",
    )
    .unwrap();

    init(dir.path()).unwrap();

    assert_eq!(fs::read(current.join("device.json")).unwrap(), b"new");
    assert_eq!(
        fs::read(current.join("diagnostics").join("from-this-pc.jsonl")).unwrap(),
        b"a"
    );
    assert_eq!(
        fs::read(current.join("diagnostics").join("from-other-pc.jsonl")).unwrap(),
        b"b"
    );
    // The clashing file is kept where it was, nothing else remains there.
    assert_eq!(fs::read(legacy.join("device.json")).unwrap(), b"old");
    assert!(!legacy.join("diagnostics").exists());
}

#[test]
fn a_binder_cannot_take_the_library_folder_name() {
    let (_dir, library) = new_library();
    assert!(library.create_binder(Path::new("_ardoise")).is_err());
    assert!(library.create_binder(Path::new("Classe/_Ardoise")).is_err());
}

#[test]
fn favourites_are_set_listed_and_cleared() {
    let (_dir, library) = new_library();
    let note = library
        .create(Path::new(""), &note_file("f", "Fav"))
        .unwrap();
    assert!(!library.list().notes[0].favorite);
    let stamp = library
        .set_favorite(&note.path, true, Some(&note.stamp))
        .unwrap();
    assert!(library.list().notes[0].favorite);
    library
        .set_favorite(&note.path, false, Some(&stamp))
        .unwrap();
    assert!(!library.list().notes[0].favorite);
    // A stale stamp is a conflict, like every other change.
    assert!(matches!(
        library.set_favorite(&note.path, true, Some(&stamp)),
        Err(LibraryError::Conflict { .. })
    ));
}

#[test]
fn a_failed_rename_after_a_retitle_keeps_the_saved_note_at_its_path() {
    let path = Path::new("a/2026-10-24 Old.ardoise");
    let busy = LibraryError::Io {
        path: path.to_path_buf(),
        source: io::Error::from(io::ErrorKind::PermissionDenied),
    };
    assert_eq!(renamed_or_kept(path, Err(busy)).unwrap(), path);
    let moved = PathBuf::from("a/2026-10-24 New.ardoise");
    assert_eq!(renamed_or_kept(path, Ok(moved.clone())).unwrap(), moved);
    let gone = LibraryError::NotFound {
        path: path.to_path_buf(),
    };
    assert!(matches!(
        renamed_or_kept(path, Err(gone)),
        Err(LibraryError::NotFound { .. })
    ));
}

#[test]
fn holds_notes_looks_below_and_ignores_the_library_folder() {
    let (dir, library) = new_library();
    assert!(!holds_notes(dir.path()));
    fs::create_dir_all(dir.path().join("Export/2026")).unwrap();
    fs::write(dir.path().join("Export/2026/a.pdf"), b"%PDF").unwrap();
    assert!(!holds_notes(&dir.path().join("Export")));
    library
        .create(Path::new("Export/2026"), &note_file("n1", "Dedans"))
        .unwrap();
    assert!(holds_notes(&dir.path().join("Export")));
    assert!(!holds_notes(&dir.path().join("absent")));
}

#[test]
fn a_cached_listing_reopens_only_the_notes_that_changed() {
    let (dir, library) = new_library();
    let kept = library.create(Path::new(""), &note_file("a", "Gardée")).unwrap();
    let edited = library.create(Path::new("B"), &note_file("b", "Avant")).unwrap();
    let gone = library.create(Path::new("B"), &note_file("c", "Perdue")).unwrap();
    let mut cache = HeaderCache::default();
    let first = library.list_with(&mut cache);
    assert_eq!(first.notes.len(), 3);
    assert_eq!(first.binders, [PathBuf::from("B")]);

    // The kept note's file is damaged, but its stamp (time and size) is the same: a cached
    // listing does not reopen it, so it still lists; a plain listing reports it.
    let bytes = fs::read(&kept.path).unwrap();
    fs::write(&kept.path, vec![b'x'; bytes.len()]).unwrap();
    File::options()
        .write(true)
        .open(&kept.path)
        .unwrap()
        .set_modified(kept.stamp.modified)
        .unwrap();
    assert_eq!(library.list().problems.len(), 1);

    let loaded = library.read(&edited.path).unwrap();
    let mut file = loaded.file;
    file.note.title = "Après".into();
    library.save(&edited.path, &file, &loaded.stamp).unwrap();
    library.trash(&gone.path).unwrap();

    let second = library.list_with(&mut cache);
    assert!(second.problems.is_empty(), "{:?}", second.problems);
    let titles: Vec<&str> = second.notes.iter().map(|n| n.title.as_str()).collect();
    assert_eq!(titles.len(), 2);
    assert!(titles.contains(&"Gardée") && titles.contains(&"Après"));
    assert_eq!(cache.entries.len(), 2);
    drop(dir);
}
