use std::fs;

use ardoise_core::format::read_note;
use ardoise_core::note::Origin;
use ardoise_inkspace::synthetic::{self, SynthNote, SynthStroke};
use ardoise_inkspace::{AccessKind, locate_with_env};
use uuid::Uuid;

use super::*;
use crate::library::tests::library as new_library;

static NOT_CANCELLED: AtomicBool = AtomicBool::new(false);

fn stroke(x0: f64, y0: f64, date: Option<i64>) -> SynthStroke {
    let samples: Vec<(f64, f64)> = (0..30)
        .map(|i| (x0 + i as f64, y0 + 0.5 * i as f64))
        .collect();
    SynthStroke::through(&samples, 0x51A7_F9FF, date.map(|_| "200"), date)
}

fn synth_note(n: u128, landscape: bool) -> SynthNote {
    let created = 1_700_000_000_000 + n as i64 * 86_400_000;
    SynthNote {
        id: Uuid::from_u128(0x0123_4567_89ab_4cde_8f01_0000_0000_0000 + n),
        created,
        modified: created + 60_000,
        landscape,
        layers: vec![vec![
            stroke(200.0, 300.0, Some(created + 1_000)),
            stroke(250.0, 320.0, Some(created + 3_000)),
        ]],
    }
}

/// A `LocalState` folder holding `count` made-up notes.
fn local_state_with(count: u128) -> (tempfile::TempDir, PathBuf) {
    let root = tempfile::tempdir().unwrap();
    let notes: Vec<_> = (0..count).map(|n| synth_note(n, n % 2 == 0)).collect();
    let local_state = synthetic::write_local_state(root.path(), &notes).unwrap();
    (root, local_state)
}

fn import(
    local_state: &Path,
    library: &Library,
    binder: &str,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(ImportProgress),
) -> CommandResult<LibraryImport> {
    import_into_library(
        local_state,
        library,
        &binder_path(binder),
        &TrackedReader::new(),
        cancel,
        progress,
    )
}

fn quiet(local_state: &Path, library: &Library, binder: &str) -> LibraryImport {
    import(local_state, library, binder, &NOT_CANCELLED, &mut |_| {}).unwrap()
}

/// Every file under `dir` with its bytes, sorted.
fn snapshot(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in fs::read_dir(&d).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                stack.push(path);
            } else {
                let bytes = fs::read(&path).unwrap();
                out.push((path, bytes));
            }
        }
    }
    out.sort();
    out
}

/// The note files of the library, outside `_ardoise/`.
fn note_files(library: &Library) -> Vec<PathBuf> {
    snapshot(library.root())
        .into_iter()
        .map(|(path, _)| path)
        .filter(|path| !path.starts_with(library.meta_dir()))
        .collect()
}

#[test]
fn notes_go_into_the_binder_once() {
    let (_source, local_state) = local_state_with(3);
    let before = snapshot(&local_state);
    let (_dir, library) = new_library();
    let mut events = Vec::new();
    let report = import(
        &local_state,
        &library,
        DEFAULT_BINDER,
        &NOT_CANCELLED,
        &mut |p| events.push((p.done, p.total)),
    )
    .unwrap();
    assert_eq!(
        report,
        LibraryImport {
            binder: "Inkspace".into(),
            total: 3,
            imported: 3,
            skipped: 0,
            failed: Vec::new(),
            cancelled: false,
        }
    );
    assert_eq!(events, [(0, 3), (1, 3), (2, 3), (3, 3)]);

    // Complete notes with their thumbnail, original dates and Inkspace origin, in the binder.
    let files = note_files(&library);
    assert_eq!(files.len(), 3, "{files:?}");
    let mut origins = Vec::new();
    for path in &files {
        assert_eq!(
            path.parent(),
            Some(library.root().join("Inkspace").as_path())
        );
        let file = read_note(path).unwrap();
        assert!(file.thumbnail.is_some());
        assert_eq!(file.note.strokes.len(), 2);
        let Origin::Inkspace { id } = &file.note.origin else {
            panic!("not an Inkspace note: {:?}", file.note.origin);
        };
        origins.push(id.clone());
        assert_eq!(
            file.note.id,
            ardoise_inkspace::convert::note_id(id, 0),
            "the id comes from Inkspace's"
        );
    }
    origins.sort();
    let mut expected: Vec<_> = (0..3).map(|n| synth_note(n, true).id.to_string()).collect();
    expected.sort();
    assert_eq!(origins, expected);

    // A second import adds nothing.
    let again = quiet(&local_state, &library, DEFAULT_BINDER);
    assert_eq!((again.imported, again.skipped), (0, 3));
    assert_eq!(note_files(&library).len(), 3);
    // Inkspace's folder was only read.
    assert_eq!(snapshot(&local_state), before);
}

#[test]
fn a_note_already_imported_is_skipped_wherever_it_is() {
    let (_source, local_state) = local_state_with(3);
    let (_dir, library) = new_library();
    quiet(&local_state, &library, DEFAULT_BINDER);
    let files = note_files(&library);
    // Moved to another binder, and another one to the trash (deleted on purpose: it must not
    // come back).
    library
        .move_to(&files[0], Path::new("Physique/3e"))
        .unwrap();
    library.trash(&files[1]).unwrap();
    let again = quiet(&local_state, &library, "Autre");
    assert_eq!((again.imported, again.skipped), (0, 3));
    assert!(!library.root().join("Autre").exists());
}

#[test]
fn other_binders_and_the_root() {
    let (_source, local_state) = local_state_with(2);
    let (_dir, library) = new_library();
    let report = quiet(&local_state, &library, "Classe/Physique");
    assert_eq!(
        (report.binder.as_str(), report.imported),
        ("Classe/Physique", 2)
    );
    assert_eq!(
        fs::read_dir(library.root().join("Classe").join("Physique"))
            .unwrap()
            .count(),
        2
    );

    let (_dir, library) = new_library();
    let report = quiet(&local_state, &library, "");
    assert_eq!((report.binder.as_str(), report.imported), ("", 2));
    assert!(
        note_files(&library)
            .iter()
            .all(|p| p.parent() == Some(library.root()))
    );
}

#[test]
fn a_bad_binder_is_refused_before_inkspace_is_read() {
    let (_source, local_state) = local_state_with(1);
    let (_dir, library) = new_library();
    for bad in ["..", "a/../b", "_ardoise", "x:y"] {
        let reader = TrackedReader::new();
        let error = import_into_library(
            &local_state,
            &library,
            &binder_path(bad),
            &reader,
            &NOT_CANCELLED,
            &mut |_| {},
        )
        .unwrap_err();
        assert_eq!(error.code(), "invalidName", "{bad}");
        assert!(reader.accesses().is_empty(), "{bad}");
    }
    assert!(note_files(&library).is_empty());
}

#[test]
fn notes_are_never_written_into_inkspace_s_folder() {
    let (source, local_state) = local_state_with(1);
    let before = snapshot(source.path());
    // A library inside the LocalState folder…
    let inside = local_state.join("Ardoise");
    fs::create_dir(&inside).unwrap();
    ardoise_core::library::init(&inside).unwrap();
    let library = Library::open(&inside).unwrap();
    let error = quiet_error(&local_state, &library, DEFAULT_BINDER);
    assert_eq!(error.code(), "inkspaceTarget");
    // …or a binder leading into a copy of Inkspace's folder kept in the library.
    let (dir, library) = new_library();
    let copy = synthetic::write_local_state(&dir.path().join("Sauvegarde"), &[synth_note(0, true)])
        .unwrap();
    let error = quiet_error(&copy, &library, "Sauvegarde/LocalState/Notes");
    assert_eq!(error.code(), "inkspaceTarget");
    fs::remove_dir_all(&inside).unwrap();
    assert_eq!(snapshot(source.path()), before);
    // The copy is a fine source for a binder elsewhere.
    assert_eq!(quiet(&copy, &library, DEFAULT_BINDER).imported, 1);
}

fn quiet_error(local_state: &Path, library: &Library, binder: &str) -> CommandError {
    import(local_state, library, binder, &NOT_CANCELLED, &mut |_| {}).unwrap_err()
}

#[test]
fn cancelling_keeps_the_notes_written_and_the_next_import_adds_the_rest() {
    let (_source, local_state) = local_state_with(4);
    let (_dir, library) = new_library();
    let cancel = AtomicBool::new(false);
    let report = import(&local_state, &library, DEFAULT_BINDER, &cancel, &mut |p| {
        // The user clicks "stop" while the third note is written: it stops before the third.
        if p.done == 2 {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();
    assert!(report.cancelled);
    assert_eq!((report.imported, report.skipped, report.total), (2, 0, 4));
    // Each note written is complete; nothing half-written lies around (no temporary file).
    let files = note_files(&library);
    assert_eq!(files.len(), 2, "{files:?}");
    for path in &files {
        assert_eq!(path.extension().unwrap(), "ardoise");
        read_note(path).unwrap();
    }
    let rest = quiet(&local_state, &library, DEFAULT_BINDER);
    assert_eq!((rest.imported, rest.skipped, rest.cancelled), (2, 2, false));

    // Stopped before the first note: nothing at all.
    let (_dir, library) = new_library();
    let report = import(
        &local_state,
        &library,
        DEFAULT_BINDER,
        &AtomicBool::new(true),
        &mut |_| {},
    )
    .unwrap();
    assert!(report.cancelled && report.imported == 0);
    assert!(!library.root().join(DEFAULT_BINDER).exists());
}

#[test]
fn damaged_and_unwritable_notes_are_reported_and_the_others_imported() {
    let root = tempfile::tempdir().unwrap();
    let mut broken = synth_note(1, true);
    broken.layers[0][0].raw_payload = Some(vec![0x05, 0x22, 0x03, 0x01]);
    let local_state =
        synthetic::write_local_state(root.path(), &[synth_note(0, true), broken.clone()]).unwrap();
    let (_dir, library) = new_library();
    let report = quiet(&local_state, &library, DEFAULT_BINDER);
    assert_eq!((report.total, report.imported), (2, 1));
    assert_eq!(report.failed.len(), 1);
    let failure = &report.failed[0];
    assert_eq!(failure.inkspace_id, broken.id.to_string());
    assert_eq!((failure.code, failure.created), (NOTE_DAMAGED, None));
    assert!(!failure.message.is_empty());

    // A binder that cannot be created: every note fails with the library's code and its date.
    let (_dir, library) = new_library();
    fs::write(library.root().join("Bloqué"), b"a file, not a folder").unwrap();
    let report = quiet(&local_state, &library, "Bloqué");
    assert_eq!(report.imported, 0);
    let codes: Vec<_> = report.failed.iter().map(|f| (f.code, f.created)).collect();
    assert_eq!(
        codes,
        [
            (NOTE_DAMAGED, None),
            ("libraryAccess", Some(synth_note(0, true).created))
        ]
    );
}

#[test]
fn surveys_count_what_an_import_would_do() {
    let (_source, local_state) = local_state_with(3);
    let reader = TrackedReader::new();
    let source = survey(&local_state, None, &reader).unwrap();
    assert_eq!(
        source,
        InkspaceSource {
            path: path_text(&local_state),
            notes: 3,
            already_imported: 0,
            unreadable: 0,
        }
    );
    let (_dir, library) = new_library();
    let cancel = AtomicBool::new(false);
    import(&local_state, &library, DEFAULT_BINDER, &cancel, &mut |p| {
        if p.done == 2 {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();
    let source = survey(&local_state, Some(&library), &reader).unwrap();
    assert_eq!((source.notes, source.already_imported), (3, 2));
    // Nothing was written by a survey.
    assert_eq!(note_files(&library).len(), 2);
}

#[test]
fn folders_that_hold_no_inkspace_notes() {
    let root = tempfile::tempdir().unwrap();
    let reader = TrackedReader::new();
    let error = local_state_of(&reader, root.path()).unwrap_err();
    assert_eq!(error.code(), "inkspaceNotFound");
    // A note store that is not one.
    let store = root.path().join("WacomCloudDb").join("_");
    fs::create_dir_all(&store).unwrap();
    fs::write(store.join("CURRENT"), b"MANIFEST-000002\n").unwrap();
    fs::write(store.join("MANIFEST-000002"), b"not a manifest").unwrap();
    let local_state = local_state_of(&reader, &store).unwrap();
    assert_eq!(local_state, root.path());
    assert_eq!(
        read_notes(&local_state, &reader).unwrap_err().code(),
        "inkspaceDamaged"
    );
    // An unreadable store (its files are folders here).
    fs::remove_file(store.join("CURRENT")).unwrap();
    fs::create_dir(store.join("CURRENT")).unwrap();
    assert_eq!(
        survey(&local_state, None, &reader).unwrap_err().code(),
        "inkspaceDamaged"
    );
}

#[test]
fn one_import_at_a_time_with_a_fresh_cancel_flag() {
    let runtime = InkspaceImport::default();
    runtime.cancel.store(true, Ordering::Relaxed);
    let (running, cancel) = runtime.begin().unwrap();
    assert!(!cancel.load(Ordering::Relaxed));
    assert_eq!(runtime.begin().unwrap_err().code(), "inkspaceBusy");
    drop(running);
    assert!(runtime.begin().is_ok());
}

#[test]
fn shapes_sent_to_the_interface() {
    let report = LibraryImport {
        binder: "Inkspace".into(),
        total: 3,
        imported: 1,
        skipped: 1,
        failed: vec![ImportFailure {
            inkspace_id: "id".into(),
            created: Some(5),
            code: NOTE_DAMAGED,
            message: "m".into(),
        }],
        cancelled: false,
    };
    assert_eq!(
        serde_json::to_value(&report).unwrap(),
        serde_json::json!({
            "binder": "Inkspace", "total": 3, "imported": 1, "skipped": 1, "cancelled": false,
            "failed": [{ "inkspaceId": "id", "created": 5, "code": "inkspaceNoteDamaged", "message": "m" }],
        })
    );
    let source = InkspaceSource {
        path: "p".into(),
        notes: 2,
        already_imported: 1,
        unreadable: 0,
    };
    assert_eq!(
        serde_json::to_value(&source).unwrap(),
        serde_json::json!({ "path": "p", "notes": 2, "alreadyImported": 1, "unreadable": 0 })
    );
    assert_eq!(
        serde_json::to_value(ImportProgress { done: 1, total: 2 }).unwrap(),
        serde_json::json!({ "done": 1, "total": 2 })
    );
}

const CANARY: &str = "CANARY-9d41-profile-access-token-and-e-mail";

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// SPEC §6.3 on the interface's path: detection in `Packages`, the folder chosen by hand in
/// every accepted form, the survey and the import into the library. Nothing under `db\` is
/// read (its `profile` key holds an access token and an e-mail address), and nothing of it
/// reaches the library or the report.
#[test]
fn the_interface_s_import_never_reads_the_profile_key() {
    let root = tempfile::tempdir().unwrap();
    let local = root.path().join("AppData").join("Local");
    let package = local
        .join("Packages")
        .join("D91E29CF.InkspaceApp_38kynpdw5g1aw");
    let notes = [synth_note(0, true), synth_note(1, false)];
    let local_state = synthetic::write_local_state(&package, &notes).unwrap();
    let db = local_state.join("db");
    synthetic::write_settings_db(&db, CANARY).unwrap();
    for note in &notes {
        let folder = db.join("notes").join(note.id.to_string());
        fs::create_dir_all(&folder).unwrap();
        fs::write(
            folder.join(format!("preview_{}.png", note.modified)),
            CANARY,
        )
        .unwrap();
        fs::write(folder.join(format!("thumb_{}.png", note.modified)), CANARY).unwrap();
    }
    let before = snapshot(&package);

    let reader = TrackedReader::new();
    let env = |name: &str| (name == "LOCALAPPDATA").then(|| local.clone().into_os_string());
    let detected = locate_with_env(&reader, env).unwrap();
    assert_eq!(detected, local_state);
    for chosen in [&local_state, &package, &local_state.join("WacomCloudDb")] {
        assert_eq!(local_state_of(&reader, chosen).unwrap(), local_state);
    }
    assert_eq!(
        local_state_of(&reader, &db).unwrap_err().code(),
        "inkspaceNotFound"
    );
    let (_dir, library) = new_library();
    let source = survey(&detected, Some(&library), &reader).unwrap();
    assert_eq!(source.notes, 2);
    let report = import_into_library(
        &detected,
        &library,
        Path::new(DEFAULT_BINDER),
        &reader,
        &NOT_CANCELLED,
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(report.imported, 2);

    for access in reader.accesses().iter().filter(|a| a.path.starts_with(&db)) {
        assert_ne!(access.kind, AccessKind::Read, "read under db: {access:?}");
        if access.kind == AccessKind::List {
            assert!(
                access.path.parent() == Some(db.join("notes").as_path()),
                "listing under db: {access:?}"
            );
        }
    }
    let utf16 = synthetic::utf16le(CANARY);
    for (path, bytes) in snapshot(library.root()) {
        assert!(
            !contains(&bytes, CANARY.as_bytes()) && !contains(&bytes, &utf16),
            "{path:?}"
        );
    }
    let json = serde_json::to_string(&(&source, &report)).unwrap();
    assert!(!json.contains("CANARY"), "{json}");
    assert_eq!(snapshot(&package), before);
}

// --- The command line ----------------------------------------------------------------------

#[test]
fn the_command_line_imports_once_too() {
    let (_source, local_state) = local_state_with(2);
    let library = tempfile::tempdir().unwrap();
    let args = ImportArgs {
        local_state: Some(local_state.clone()),
        library: Some(library.path().to_path_buf()),
        compare: false,
    };
    assert_eq!(run_cli(&args), 0);
    assert_eq!(run_cli(&args), 0);
    let opened = Library::open(library.path()).unwrap();
    assert_eq!(note_files(&opened).len(), 2);
    // The package folder above LocalState is accepted too.
    let from_package = ImportArgs {
        local_state: local_state.parent().map(Path::to_path_buf),
        ..args
    };
    assert_eq!(run_cli(&from_package), 0);
    assert_eq!(note_files(&opened).len(), 2);
}

#[test]
fn the_demo_inkspace_data_imports() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("Packages").join("LocalState");
    assert_eq!(cli::run_demo_cli(&dir), 0);
    let reader = TrackedReader::new();
    let report = read_notes(&dir, &reader).unwrap();
    assert_eq!(report.notes.len(), 4);
    assert!(report.failures.is_empty());
    let (_dir, library) = new_library();
    assert_eq!(quiet(&dir, &library, DEFAULT_BINDER).imported, 4);
    // Never into a folder that holds anything (real Inkspace data above all).
    let before = snapshot(&dir);
    assert_eq!(cli::run_demo_cli(&dir), 1);
    assert_eq!(snapshot(&dir), before);
    assert_eq!(cli::run_demo_cli(Path::new("")), 2);
}
