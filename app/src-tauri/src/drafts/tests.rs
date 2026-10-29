//! Tests of the draft store: where drafts go, what they keep, and the copy made of one.

use std::fs;
use std::time::{Duration, UNIX_EPOCH};

use ardoise_core::note::{BackgroundImage, ImageRect};

use super::*;
use crate::library::path_text;
use crate::library::tests::{library, sample_note, write_sample};

const LIBRARY: &str = r"D:\Nextcloud\Notes";

fn stamp(secs: u64, len: u64) -> FileStamp {
    FileStamp {
        modified: UNIX_EPOCH + Duration::from_secs(secs),
        len,
    }
}

fn record(library: &str, id: &str, saved_at: i64, session: &str) -> DraftRecord {
    let mut note = sample_note("Cours de physique");
    note.id = id.to_owned();
    DraftRecord {
        version: DRAFT_VERSION,
        library: library.to_owned(),
        path: format!(r"{library}\Cours de physique.ardoise"),
        id: id.to_owned(),
        saved_at,
        base_stamp: Some(stamp(1_800_000_000, 1234)),
        session: session.to_owned(),
        image: None,
        note,
    }
}

fn with_image(mut record: DraftRecord, key: &str) -> DraftRecord {
    record.note.background_image = Some(BackgroundImage {
        name: "carte.png".into(),
        pdf_page: None,
        rect: ImageRect {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 800.0,
        },
        quarter_turns: 0,
        opacity: 1.0,
    });
    record.image = Some(DraftImage {
        key: key.to_owned(),
        extension: "png".into(),
        width: 10,
        height: 8,
    });
    record
}

fn png(bytes: &[u8]) -> BackgroundFile {
    BackgroundFile {
        extension: "png".into(),
        bytes: bytes.to_vec(),
    }
}

fn files(store: &DraftStore) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(store.dir())
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn drafts_live_per_copy_outside_the_library() {
    let local = Path::new(r"C:\Users\prof\AppData\Local");
    assert_eq!(
        DraftStore::for_copy(local, Some("0badcafe")).dir(),
        local.join("Ardoise").join("drafts").join("0badcafe")
    );
    assert_eq!(
        DraftStore::for_copy(local, None).dir(),
        local.join("Ardoise").join("drafts").join("shared")
    );
}

#[test]
fn the_file_name_depends_on_the_library_and_the_note_only() {
    let stem = draft_stem(LIBRARY, "4f1c-id");
    assert!(stem.starts_with("draft-") && stem.len() == "draft-".len() + 16);
    // The same library written differently (case, separators, long-path prefix, final slash).
    assert_eq!(stem, draft_stem(r"d:/nextcloud/NOTES/", "4f1c-id"));
    assert_eq!(stem, draft_stem(r"\\?\D:\Nextcloud\Notes", "4f1c-id"));
    assert_ne!(stem, draft_stem(LIBRARY, "4f1c-id2"));
    assert_ne!(stem, draft_stem(r"D:\Autre", "4f1c-id"));
    // Any id gives a plain file name.
    let odd = draft_stem(LIBRARY, r"..\..\x:y*?<>|");
    assert!(odd.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'));
}

#[test]
fn writes_reads_back_and_replaces_a_draft() {
    let dir = tempfile::tempdir().unwrap();
    let store = DraftStore::new(dir.path().join("drafts"));
    assert_eq!(store.read(LIBRARY, "n1"), None);

    let first = record(LIBRARY, "n1", 1000, "s1");
    store.write(&first, None).unwrap();
    assert_eq!(store.read(LIBRARY, "n1"), Some(first.clone()));
    // Found whatever the spelling of the library.
    assert!(store.read(&LIBRARY.to_lowercase(), "n1").is_some());

    let mut second = record(LIBRARY, "n1", 2000, "s1");
    second.note.strokes.clear();
    store.write(&second, None).unwrap();
    assert_eq!(store.read(LIBRARY, "n1"), Some(second));
    assert_eq!(
        files(&store).len(),
        1,
        "one file per note: {:?}",
        files(&store)
    );
    // No temporary file is left behind by the atomic writes.
    assert!(files(&store).iter().all(|f| f.ends_with(".json")));
}

#[test]
fn lists_the_drafts_of_one_library_most_recent_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = DraftStore::new(dir.path());
    store
        .write(&record(LIBRARY, "a", 1000, "s1"), None)
        .unwrap();
    store
        .write(&record(LIBRARY, "b", 3000, "s2"), None)
        .unwrap();
    store
        .write(&record(r"E:\Autre bibliothèque", "c", 2000, "s1"), None)
        .unwrap();
    // Neither another version nor a damaged file nor a stranger is listed (nor removed).
    let mut future = record(LIBRARY, "d", 4000, "s1");
    future.version = DRAFT_VERSION + 1;
    fs::write(
        dir.path()
            .join(format!("{}.json", draft_stem(LIBRARY, "d"))),
        serde_json::to_vec(&future).unwrap(),
    )
    .unwrap();
    fs::write(dir.path().join("draft-0000000000000000.json"), b"{ damaged").unwrap();
    fs::write(dir.path().join("notes.json"), b"{}").unwrap();

    let ids: Vec<String> = store.list(LIBRARY).into_iter().map(|r| r.id).collect();
    assert_eq!(ids, ["b", "a"]);
    assert_eq!(store.list(r"e:\autre bibliothèque").len(), 1);
    assert_eq!(store.read(LIBRARY, "d"), None);
    assert_eq!(files(&store).len(), 6);
    // An empty or missing folder lists nothing.
    assert!(
        DraftStore::new(dir.path().join("none"))
            .list(LIBRARY)
            .is_empty()
    );
}

#[test]
fn a_draft_is_only_found_for_its_own_note() {
    let dir = tempfile::tempdir().unwrap();
    let store = DraftStore::new(dir.path());
    let draft = record(LIBRARY, "n1", 1000, "s1");
    store.write(&draft, None).unwrap();
    // Another note's draft under this name (a hash collision) is not taken for it.
    let stranger = dir
        .path()
        .join(format!("{}.json", draft_stem(LIBRARY, "n2")));
    fs::copy(
        dir.path()
            .join(format!("{}.json", draft_stem(LIBRARY, "n1"))),
        stranger,
    )
    .unwrap();
    assert_eq!(store.read(LIBRARY, "n2"), None);
}

#[test]
fn keeps_the_staged_image_once_per_key() {
    let dir = tempfile::tempdir().unwrap();
    let store = DraftStore::new(dir.path());
    let draft = with_image(record(LIBRARY, "n1", 1000, "s1"), "00ff-12");
    store.write(&draft, Some(&png(b"first pixels"))).unwrap();
    assert_eq!(store.image(&draft), Some(png(b"first pixels")));
    // The same key again: the file is not written again.
    store.write(&draft, Some(&png(b"other bytes"))).unwrap();
    assert_eq!(store.image(&draft), Some(png(b"first pixels")));
    assert_eq!(files(&store).len(), 2);

    // Another image replaces it.
    let next = with_image(record(LIBRARY, "n1", 2000, "s1"), "abcd-99");
    store.write(&next, Some(&png(b"second"))).unwrap();
    assert_eq!(store.image(&next), Some(png(b"second")));
    assert_eq!(files(&store).len(), 2, "{:?}", files(&store));

    // The image removed from the note: its file goes.
    store
        .write(&record(LIBRARY, "n1", 3000, "s1"), Some(&png(b"x")))
        .unwrap();
    assert_eq!(store.read(LIBRARY, "n1").unwrap().image, None);
    assert_eq!(files(&store).len(), 1);
}

#[test]
fn an_image_without_its_pixels_or_with_an_odd_key_is_not_named() {
    let dir = tempfile::tempdir().unwrap();
    let store = DraftStore::new(dir.path());
    let draft = with_image(record(LIBRARY, "n1", 1000, "s1"), "00ff-12");
    store.write(&draft, None).unwrap();
    assert_eq!(store.read(LIBRARY, "n1").unwrap().image, None);
    let odd = with_image(record(LIBRARY, "n1", 1000, "s1"), r"..\..\evil");
    store.write(&odd, Some(&png(b"x"))).unwrap();
    assert_eq!(store.read(LIBRARY, "n1").unwrap().image, None);
    assert_eq!(files(&store).len(), 1);
}

#[test]
fn removing_a_draft_takes_its_image_and_never_fails_when_absent() {
    let dir = tempfile::tempdir().unwrap();
    let store = DraftStore::new(dir.path());
    store
        .write(
            &with_image(record(LIBRARY, "n1", 1000, "s1"), "00ff-12"),
            Some(&png(b"pixels")),
        )
        .unwrap();
    store
        .write(&record(LIBRARY, "n2", 1000, "s1"), None)
        .unwrap();
    store.remove(LIBRARY, "n1").unwrap();
    assert_eq!(store.read(LIBRARY, "n1"), None);
    assert_eq!(files(&store).len(), 1);
    store.remove(LIBRARY, "n1").unwrap();
    DraftStore::new(dir.path().join("none"))
        .remove(LIBRARY, "n1")
        .unwrap();
    assert!(store.read(LIBRARY, "n2").is_some());
}

#[test]
fn the_draft_keeps_the_image_only_for_a_note_that_shows_one() {
    let staged = Staged {
        file: png(b"pixels"),
        width: 10,
        height: 8,
    };
    let plain = draft_of(
        LIBRARY,
        "p",
        "n1",
        sample_note("x"),
        None,
        "s",
        Some(("k-1", &staged)),
    );
    assert_eq!(plain.image, None);
    let shown = with_image(record(LIBRARY, "n1", 0, "s"), "k-1");
    let draft = draft_of(
        LIBRARY,
        "p",
        "n1",
        shown.note,
        Some(stamp(1, 2)),
        "s",
        Some(("k-1", &staged)),
    );
    assert_eq!(
        draft.image,
        Some(DraftImage {
            key: "k-1".into(),
            extension: "png".into(),
            width: 10,
            height: 8,
        })
    );
    assert_eq!(draft.base_stamp, Some(stamp(1, 2)));
    assert!(draft.saved_at > 0);
}

#[test]
fn restoring_as_a_copy_makes_a_new_note_beside_the_old_one() {
    let (dir, library) = library();
    let original = write_sample(&library, "Physique/3e", "Cours");
    let before = fs::read(&original.path).unwrap();
    let root = path_text(dir.path());
    let mut draft = record(&root, "n1", 1000, "s1");
    draft.path = path_text(&original.path);
    draft.note.strokes.truncate(0);

    let copy = restore_copy_in(&library, &draft, None, "  Cours (récupéré)  ").unwrap();
    assert_eq!(copy.binder, "Physique/3e");
    assert_eq!(copy.title, "Cours (récupéré)");
    assert_ne!(copy.id, "n1");
    assert_ne!(copy.path, path_text(&original.path));
    let opened = crate::notes::open_in(&library, Path::new(&copy.path)).unwrap();
    assert!(opened.note.strokes.is_empty());
    assert_eq!(opened.note.origin, Origin::Ardoise);
    // The note it came from is untouched.
    assert_eq!(fs::read(&original.path).unwrap(), before);

    // Its binder gone: the copy goes to the root.
    draft.path = path_text(&dir.path().join("Disparu").join("Cours.ardoise"));
    let lost = restore_copy_in(&library, &draft, None, "Perdu").unwrap();
    assert_eq!(lost.binder, "");
}

#[test]
fn a_copy_keeps_the_image_kept_with_the_draft_and_drops_one_without_pixels() {
    let (dir, library) = library();
    let root = path_text(dir.path());
    let png = ardoise_render::encode_png(&crate::background::tests::halves(4, 2)).unwrap();
    let picture = ardoise_render::picture::import_image(&png).unwrap();
    let member = BackgroundFile {
        extension: picture.extension().to_owned(),
        bytes: picture.bytes.clone(),
    };
    let draft = with_image(record(&root, "n1", 1000, "s1"), "k-1");

    let copy = restore_copy_in(&library, &draft, Some(member.clone()), "Avec image").unwrap();
    let loaded = library.read(Path::new(&copy.path)).unwrap();
    assert_eq!(loaded.file.background, Some(member));
    assert!(loaded.file.note.background_image.is_some());

    let bare = restore_copy_in(&library, &draft, None, "Sans image").unwrap();
    let loaded = library.read(Path::new(&bare.path)).unwrap();
    assert_eq!(loaded.file.background, None);
    assert_eq!(loaded.file.note.background_image, None);
}
