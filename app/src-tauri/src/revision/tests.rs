use super::*;
use crate::library::tests::{library, sample_note, write_sample};
use crate::notes::{open_in, save_in};
use ardoise_core::format::NoteFile;
use ardoise_core::library::SavedNote;
use std::fs;

/// A revision of `source` in the root of `library`.
fn write_revision(library: &Library, source: &Note, title: &str) -> SavedNote {
    let mut file = NoteFile::new(sample_note(title));
    file.mark_as_revision_of(source, 1_793_948_883_407, "Lignes redressées");
    file.add_revision_entry(1_793_949_000_000, "Un « g » refait");
    library.create(Path::new(""), &file).unwrap()
}

use ardoise_core::note::Note;

fn png_size(png: &[u8]) -> (u32, u32) {
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
    let read = |at: usize| u32::from_be_bytes(png[at..at + 4].try_into().unwrap());
    (read(16), read(20))
}

#[test]
fn a_note_that_is_not_a_revision_opens_without_one() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Ordinaire");
    let opened = open_in(&library, &saved.path).unwrap();
    assert_eq!(opened.revision, None);
    let json = serde_json::to_value(&opened).unwrap();
    assert!(json["revision"].is_null());
}

#[test]
fn a_revision_opens_with_its_account_and_its_source_found_by_id() {
    let (_dir, library) = library();
    let source = write_sample(&library, "Classe", "Brouillon");
    let source_note = library.read(&source.path).unwrap().file.note;
    let copy = write_revision(&library, &source_note, "Propre");

    // The source is renamed and moved after the copy was made: it is found by its id.
    let renamed = library
        .retitle(&source.path, "Cours d'optique", None)
        .unwrap();
    let moved = library
        .move_to(&renamed.path, Path::new("Archives"))
        .unwrap();

    let opened = open_in(&library, &copy.path).unwrap();
    let revision = opened.revision.expect("a revision");
    assert_eq!(revision.of, source_note.id);
    assert_eq!(revision.of_title, "Brouillon", "the title of that time");
    assert_eq!(revision.summary.len(), 2);
    assert_eq!(revision.summary[1].text, "Un « g » refait");
    let link = revision.source.expect("the source is in the library");
    assert_eq!(link.path, crate::library::path_text(&moved));
    assert_eq!(link.title, "Cours d'optique");

    // What the interface receives.
    let json = serde_json::to_value(open_in(&library, &copy.path).unwrap()).unwrap();
    assert_eq!(json["revision"]["ofTitle"], "Brouillon");
    assert_eq!(json["revision"]["created"], 1_793_948_883_407_i64);
    assert_eq!(json["revision"]["summary"][0]["at"], 1_793_948_883_407_i64);
    assert_eq!(json["revision"]["source"]["title"], "Cours d'optique");
    // The copy's own note is the note, unchanged by the account.
    assert_eq!(json["note"]["title"], "Propre");
}

#[test]
fn a_source_that_is_gone_is_reported_as_gone() {
    let (_dir, library) = library();
    let source = write_sample(&library, "", "Brouillon");
    let source_note = library.read(&source.path).unwrap().file.note;
    let copy = write_revision(&library, &source_note, "Propre");
    let item = library.trash(&source.path).unwrap();

    let revision = open_in(&library, &copy.path).unwrap().revision.unwrap();
    assert_eq!(revision.source, None, "in the trash is gone");
    assert_eq!(
        revision.of_title, "Brouillon",
        "the account still says whose copy it is"
    );
    assert_eq!(find_source(&library, &source_note.id), None);
    let json = serde_json::to_value(open_in(&library, &copy.path).unwrap()).unwrap();
    assert!(json["revision"]["source"].is_null());

    // Restored from the trash: found again.
    library.restore(&item.id).unwrap();
    assert!(find_source(&library, &source_note.id).is_some());
}

#[test]
fn a_conflict_copy_of_the_source_is_not_preferred_to_it() {
    let (dir, library) = library();
    let source = write_sample(&library, "", "Cours");
    let id = library.read(&source.path).unwrap().file.note.id;
    let name = source
        .path
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let conflict = dir
        .path()
        .join(name.replace(".ardoise", " (conflicted copy 2026-10-25).ardoise"));
    fs::copy(&source.path, &conflict).unwrap();
    assert_eq!(crate::library::list(&library).notes.len(), 2);

    let link = find_source(&library, &id).unwrap();
    assert_eq!(link.path, crate::library::path_text(&source.path));
}

#[test]
fn saving_the_editor_keeps_the_revision() {
    let (_dir, library) = library();
    let source = write_sample(&library, "", "Brouillon");
    let source_note = library.read(&source.path).unwrap().file.note;
    let copy = write_revision(&library, &source_note, "Propre");
    let opened = open_in(&library, &copy.path).unwrap();
    let mut edited = opened.note.clone();
    edited.strokes.clear();
    save_in(&library, &copy.path, edited, &opened.stamp).unwrap();
    let after = open_in(&library, &copy.path).unwrap();
    assert!(after.note.strokes.is_empty());
    assert_eq!(after.revision, opened.revision);
}

#[test]
fn the_picture_fits_the_box_whatever_its_shape() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Page");
    let page = sample_note("x").page;
    let aspect = page.width as f32 / page.height as f32;

    let (w, h) = png_size(&picture_in(&library, &saved.path, 600, 4000).unwrap());
    assert_eq!(w, 600, "the width decides in a tall box");
    assert!((w as f32 / h as f32 - aspect).abs() < 0.01);

    let (w, h) = png_size(&picture_in(&library, &saved.path, 4000, 300).unwrap());
    assert_eq!(h, 300, "the height decides in a wide box");
    assert!((w as f32 / h as f32 - aspect).abs() < 0.02);

    // Absurd boxes are clamped, not refused.
    let (w, h) = png_size(&picture_in(&library, &saved.path, 1_000_000, 1_000_000).unwrap());
    assert!(w <= MAX_PICTURE_PX && h <= MAX_PICTURE_PX);
    let (w, _) = png_size(&picture_in(&library, &saved.path, 0, 0).unwrap());
    assert!(w >= MIN_PICTURE_PX - 1);
}

#[test]
fn the_picture_is_the_note_as_last_saved() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Page");
    let before = picture_in(&library, &saved.path, 400, 400).unwrap();
    assert_eq!(before, picture_in(&library, &saved.path, 400, 400).unwrap());

    let opened = open_in(&library, &saved.path).unwrap();
    let mut edited = opened.note.clone();
    edited.strokes.clear();
    save_in(&library, &saved.path, edited, &opened.stamp).unwrap();
    assert_ne!(before, picture_in(&library, &saved.path, 400, 400).unwrap());
}

#[test]
fn only_notes_of_the_library_are_drawn() {
    let (_dir, library) = library();
    let outside = tempfile::tempdir().unwrap();
    let elsewhere = outside.path().join("2026-10-24 Ailleurs.ardoise");
    ardoise_core::format::write_note(&elsewhere, &NoteFile::new(sample_note("Ailleurs"))).unwrap();
    let error = picture_in(&library, &elsewhere, 300, 300).unwrap_err();
    assert_eq!(error.code(), "notInLibrary");
    let missing = library.root().join("2026-10-24 Absente.ardoise");
    assert_eq!(
        picture_in(&library, &missing, 300, 300).unwrap_err().code(),
        "noteNotFound"
    );
}
