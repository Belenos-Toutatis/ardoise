//! Duplicating a note, and changing its creation date (SPEC §4.3, Q20).
//!
//! A duplicate is the note's file written again under a new id, in the same binder: everything
//! the file holds is carried over (docs/design/library-order.md, « Duplicating »), except what
//! makes it *this* note: its id, its title (followed by the copy word), and the favourite mark.
//! The creation date stays the original's: it is also the time origin of every point of the
//! strokes and of the narration's offset, so changing it would take the copy out of step with
//! its own sound.

use std::path::{Path, PathBuf};

use super::{FileStamp, Library, LibraryError, SavedNote, local_date, now_ms};
use crate::note::Note;

/// The latest creation date accepted: 9999-12-31 00:00 UTC, so that the year in a file name
/// has four digits whatever the time zone.
const MAX_CREATED_MS: i64 = 253_402_214_400_000;

/// The title of a copy of a note titled `title`: `title (word)`, and `title (word 2)`,
/// `title (word 3)`… for the copy of a copy, so that copies are told apart. An untitled note
/// gives `(word)`.
pub fn copy_title(title: &str, word: &str) -> String {
    let title = title.trim();
    let word = word.trim();
    // `(word)` or `(word N)` at the end of the title: this is a copy already.
    let numbered = title.strip_suffix(')').and_then(|rest| {
        let (before, inside) = rest.rsplit_once('(')?;
        let number = if inside.eq_ignore_ascii_case(word) {
            1
        } else {
            let (name, n) = inside.rsplit_once(' ')?;
            if !name.eq_ignore_ascii_case(word) {
                return None;
            }
            n.parse::<u32>().ok().filter(|n| *n >= 2)?
        };
        Some((before.trim_end(), number))
    });
    match numbered {
        Some(("", n)) => format!("({word} {})", n + 1),
        Some((base, n)) => format!("{base} ({word} {})", n + 1),
        None if title.is_empty() => format!("({word})"),
        None => format!("{title} ({word})"),
    }
}

impl Library {
    /// Writes a copy of the note at `path` next to it, with `new_id` as its id, and returns where
    /// it is with its metadata (the strokes are not repeated).
    ///
    /// - The copy is in the same binder, has the same creation date, and a title made by
    ///   [`copy_title`] from `copy_word` (the interface's translated word). Its file name is the
    ///   usual `YYYY-MM-DD Title.ardoise`, with a ` (n)` suffix if taken. It is not a favourite.
    ///   `modified` is now.
    /// - Everything else is kept as it is: tags, layers, ink, paper, background image and its
    ///   pixels, the narration, the tablet's original (so « Revenir à l'original » works on the
    ///   copy), the note before its first edit, the recognised text if the file holds some, the
    ///   thumbnail, the origin, the blackboard position and every other key or member this
    ///   version does not know. A revision stays a revision of the same note.
    /// - In a binder that has a free order (`order`), the copy is listed right after its original.
    ///
    /// Nothing is written when the original cannot be read. Never replaces a file.
    pub fn duplicate(
        &self,
        path: &Path,
        new_id: &str,
        copy_word: &str,
    ) -> Result<(SavedNote, Note), LibraryError> {
        if new_id.trim().is_empty() {
            return Err(LibraryError::InvalidName {
                name: new_id.to_owned(),
                reason: "the note id is empty",
            });
        }
        let loaded = self.read(path)?;
        let binder = binder_of(self, &loaded.path);
        let original_id = loaded.file.note.id.clone();
        let mut file = loaded.file;
        file.note.id = new_id.to_owned();
        file.note.title = copy_title(&file.note.title, copy_word);
        file.note.favorite = false;
        file.note.modified = now_ms().max(file.note.modified);
        let saved = self.create(&binder, &file)?;
        // Best effort: the copy exists and sits next to its original (same date) in any case.
        let _ = self.place_after_in_order(&binder, &original_id, new_id);
        file.note.strokes = Vec::new();
        Ok((saved, file.note))
    }

    /// Sets the creation date of a note (Unix milliseconds) and renames its file to match
    /// (`YYYY-MM-DD Title.ardoise`, with a ` (n)` suffix if taken). The date is also what the
    /// library sorts by. Strokes and narration are not touched: their times are offsets from the
    /// date, so they stay in step with each other. The same date writes nothing.
    ///
    /// Fails with [`LibraryError::InvalidDate`] for a date that cannot be written in a file
    /// name (before 1970 or after the year 9999), and with [`LibraryError::Conflict`] when
    /// `expected` is not the file's stamp any more.
    pub fn set_created(
        &self,
        path: &Path,
        created: i64,
        expected: Option<&FileStamp>,
    ) -> Result<SavedNote, LibraryError> {
        if !(0..=MAX_CREATED_MS).contains(&created) || local_date(created).is_none() {
            return Err(LibraryError::InvalidDate { created });
        }
        let loaded = self.read(path)?;
        if let Some(expected) = expected
            && loaded.stamp != *expected
        {
            return Err(LibraryError::Conflict { path: loaded.path });
        }
        if loaded.file.note.created == created {
            return Ok(SavedNote {
                path: loaded.path,
                stamp: loaded.stamp,
            });
        }
        let mut file = loaded.file;
        file.note.created = created;
        file.note.modified = now_ms().max(file.note.modified);
        let stamp = self.save(&loaded.path, &file, &loaded.stamp)?;
        self.rename_to_match(loaded.path, &file, stamp)
    }
}

/// The binder (relative to the root) of the note file at `path`.
fn binder_of(library: &Library, path: &Path) -> PathBuf {
    path.parent()
        .and_then(|parent| parent.strip_prefix(library.root()).ok())
        .map(Path::to_path_buf)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::format::{self, Member, NoteFile};
    use crate::library::init;
    use crate::note::{BackgroundImage, ImageRect, Origin};
    use crate::testutil::blank_note;
    use ardoise_ink::{Point, Rgba, Stroke, Tool};

    const WORD: &str = "copie";

    fn new_library() -> (tempfile::TempDir, Library) {
        let dir = tempfile::tempdir().unwrap();
        init(dir.path()).unwrap();
        let library = Library::open(dir.path()).unwrap();
        (dir, library)
    }

    /// A note that holds every optional member and an unknown key and member.
    fn full_file(id: &str, title: &str) -> NoteFile {
        let mut note = blank_note(id, title);
        note.created = 1_792_843_200_000;
        note.modified = note.created;
        note.tags = vec!["maths".into(), "3e B".into()];
        note.favorite = true;
        note.origin = Origin::Tablet;
        note.background_image = Some(BackgroundImage {
            name: "fiche.pdf".into(),
            pdf_page: Some(2),
            rect: ImageRect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 200.0,
            },
            quarter_turns: 1,
            opacity: 0.5,
        });
        note.strokes.push(Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 60.0,
            layer: 0,
            points: (0..5)
                .map(|i| Point {
                    x: 100.0 * i as f32,
                    y: 50.0,
                    pressure: 0.5,
                    t: 10 * i,
                })
                .collect(),
        });
        let mut file = NoteFile::new(note);
        file.original = Some(vec![1, 2, 3, 4]);
        file.original_note = Some(vec![9, 9]);
        file.thumbnail = Some(vec![0x89, b'P', b'N', b'G']);
        file.audio = Some(vec![b'O', b'g', b'g', b'S', 1, 2, 3]);
        file.background = Some(format::BackgroundFile {
            extension: "png".into(),
            bytes: vec![5, 6, 7],
        });
        file.text = Some(b"{\"words\": []}".to_vec());
        file.mark_as_revision_of(
            &blank_note("source", "Cours"),
            1_792_000_000_000,
            "Redressé.",
        );
        file.unknown_fields
            .insert("boardPosition".into(), "buttonsLeft".into());
        file.unknown_fields
            .insert("futureKey".into(), serde_json::json!({ "a": [1, 2] }));
        file.unknown_members.push(Member {
            name: "future.bin".into(),
            bytes: vec![42; 10],
        });
        file
    }

    #[test]
    fn copy_titles() {
        assert_eq!(copy_title("Cours", WORD), "Cours (copie)");
        assert_eq!(copy_title("  Cours  ", WORD), "Cours (copie)");
        assert_eq!(copy_title("", WORD), "(copie)");
        assert_eq!(copy_title("   ", WORD), "(copie)");
        // The copy of a copy is numbered.
        assert_eq!(copy_title("Cours (copie)", WORD), "Cours (copie 2)");
        assert_eq!(copy_title("Cours (copie 2)", WORD), "Cours (copie 3)");
        assert_eq!(copy_title("Cours (Copie 9)", WORD), "Cours (copie 10)");
        assert_eq!(copy_title("(copie)", WORD), "(copie 2)");
        // Other parentheses are part of the title.
        assert_eq!(copy_title("Cours (2)", WORD), "Cours (2) (copie)");
        assert_eq!(
            copy_title("Cours (copie 1)", WORD),
            "Cours (copie 1) (copie)"
        );
        assert_eq!(copy_title("Cours (copy)", WORD), "Cours (copy) (copie)");
        assert_eq!(copy_title("Cours (copy)", "copy"), "Cours (copy 2)");
    }

    #[test]
    fn a_duplicate_keeps_everything_and_changes_only_what_makes_it_another_note() {
        let (_dir, library) = new_library();
        let original = library
            .create(Path::new("Classe/3e B"), &full_file("orig-id", "Cours"))
            .unwrap();
        let before = fs::read(&original.path).unwrap();
        let loaded = library.read(&original.path).unwrap();

        let (saved, note) = library.duplicate(&original.path, "copy-id", WORD).unwrap();

        // The original is untouched, byte for byte.
        assert_eq!(fs::read(&original.path).unwrap(), before);
        // The copy: same binder, same date in its name, the title with the copy word.
        assert_ne!(saved.path, original.path);
        assert_eq!(saved.path.parent(), original.path.parent());
        let name = saved
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(name.ends_with(" Cours (copie).ardoise"), "{name}");
        assert_eq!(
            name[..10],
            original.path.file_name().unwrap().to_string_lossy()[..10]
        );
        assert_eq!(
            (note.id.as_str(), note.title.as_str()),
            ("copy-id", "Cours (copie)")
        );
        assert!(
            note.strokes.is_empty(),
            "the summary does not repeat the strokes"
        );

        let copy = library.read(&saved.path).unwrap();
        let (a, b) = (&loaded.file, &copy.file);
        assert_eq!(b.note.id, "copy-id");
        assert_eq!(b.note.title, "Cours (copie)");
        assert!(!b.note.favorite && a.note.favorite);
        assert!(b.note.modified >= a.note.modified);
        // The same date, ink, layers, paper, tags, origin and background image.
        assert_eq!(b.note.created, a.note.created);
        assert_eq!(b.note.strokes, a.note.strokes);
        assert_eq!(b.note.layers, a.note.layers);
        assert_eq!(b.note.page, a.note.page);
        assert_eq!(b.note.background, a.note.background);
        assert_eq!(b.note.background_image, a.note.background_image);
        assert_eq!(b.note.tags, a.note.tags);
        assert_eq!(b.note.timing, a.note.timing);
        assert_eq!(b.note.origin, Origin::Tablet);
        // Every member: the tablet's original, the note before its first edit, the thumbnail,
        // the narration, the background pixels, the recognised text, the revision, and what this
        // version does not know (a key of note.json, and a member).
        assert_eq!(b.original, a.original);
        assert_eq!(b.original_note, a.original_note);
        assert_eq!(b.thumbnail, a.thumbnail);
        assert_eq!(b.audio, a.audio);
        assert_eq!(b.background, a.background);
        assert_eq!(b.text, a.text);
        assert_eq!(b.revision, a.revision);
        assert!(b.revision.is_some());
        assert_eq!(b.unknown_fields, a.unknown_fields);
        assert_eq!(b.unknown_fields["boardPosition"], "buttonsLeft");
        assert_eq!(b.unknown_members, a.unknown_members);
        assert_eq!(library.list().notes.len(), 2);
    }

    #[test]
    fn duplicating_the_copy_numbers_it_and_names_are_never_taken() {
        let (_dir, library) = new_library();
        let first = library
            .create(Path::new(""), &NoteFile::new(blank_note("a", "Cours")))
            .unwrap();
        let (second, _) = library.duplicate(&first.path, "b", WORD).unwrap();
        let (third, note) = library.duplicate(&second.path, "c", WORD).unwrap();
        assert_eq!(note.title, "Cours (copie 2)");
        // The same note duplicated twice: two files, one title, no replacement.
        let (fourth, _) = library.duplicate(&first.path, "d", WORD).unwrap();
        let (fifth, _) = library.duplicate(&first.path, "e", WORD).unwrap();
        let paths = [
            &first.path,
            &second.path,
            &third.path,
            &fourth.path,
            &fifth.path,
        ];
        for (i, a) in paths.iter().enumerate() {
            for b in &paths[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert_eq!(library.list().notes.len(), 5);
        assert!(library.list().problems.is_empty());
    }

    #[test]
    fn an_untitled_note_gives_a_copy_with_the_copy_word_as_its_title() {
        let (_dir, library) = new_library();
        let mut note = blank_note("a", "");
        note.created = 1_792_843_200_000;
        let first = library.create(Path::new(""), &NoteFile::new(note)).unwrap();
        let (copy, note) = library.duplicate(&first.path, "b", WORD).unwrap();
        assert_eq!(note.title, "(copie)");
        let name = copy
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(name.ends_with(" (copie).ardoise"), "{name}");
        assert_ne!(copy.path, first.path);
    }

    #[test]
    fn a_duplicate_is_in_the_binders_order_right_after_its_original() {
        let (_dir, library) = new_library();
        let a = library
            .create(Path::new("Classe"), &NoteFile::new(blank_note("a", "A")))
            .unwrap();
        library
            .create(Path::new("Classe"), &NoteFile::new(blank_note("b", "B")))
            .unwrap();
        // No order file: none is created.
        library.duplicate(&a.path, "a2", WORD).unwrap();
        assert!(library.list().orders.is_empty());
        library
            .set_binder_order(Path::new("Classe"), &["b".into(), "a".into()])
            .unwrap();
        library.duplicate(&a.path, "a3", WORD).unwrap();
        assert_eq!(library.binder_order(Path::new("Classe")), ["b", "a", "a3"]);
    }

    #[test]
    fn a_damaged_or_missing_note_is_refused_and_nothing_is_written() {
        let (dir, library) = new_library();
        let error = library
            .duplicate(&dir.path().join("2026-10-24 Rien.ardoise"), "x", WORD)
            .unwrap_err();
        assert!(matches!(error, LibraryError::NotFound { .. }), "{error:?}");
        fs::write(dir.path().join("2026-10-24 Abîmée.ardoise"), b"not a zip").unwrap();
        let error = library
            .duplicate(&dir.path().join("2026-10-24 Abîmée.ardoise"), "x", WORD)
            .unwrap_err();
        assert_eq!(error.code(), "noteDamaged");
        let error = library
            .duplicate(&dir.path().join("2026-10-24 Abîmée.ardoise"), " ", WORD)
            .unwrap_err();
        assert_eq!(error.code(), "invalidName");
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn changing_the_date_renames_the_file_and_keeps_the_note() {
        let (_dir, library) = new_library();
        let mut file = full_file("id", "Cours");
        file.note.created = 1_792_843_200_000;
        let saved = library.create(Path::new("Classe"), &file).unwrap();
        let before = library.read(&saved.path).unwrap();
        assert!(saved.path.to_string_lossy().contains("2026-10-2"));

        // 2025-03-04 12:00 UTC.
        let wanted = 1_741_089_600_000;
        let moved = library
            .set_created(&saved.path, wanted, Some(&saved.stamp))
            .unwrap();
        assert_ne!(moved.path, saved.path);
        assert!(!saved.path.exists());
        let name = moved
            .path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(name.starts_with("2025-03-0"), "{name}");
        assert!(name.ends_with(" Cours.ardoise"), "{name}");
        assert_eq!(moved.path.parent(), saved.path.parent());

        let after = library.read(&moved.path).unwrap();
        assert_eq!(after.file.note.created, wanted);
        assert_eq!(after.file.note.id, "id");
        assert!(after.file.note.modified >= before.file.note.modified);
        // Only the date (and `modified`) changed.
        let mut expected = before.file.clone();
        expected.note.created = wanted;
        expected.note.modified = after.file.note.modified;
        assert_eq!(after.file, expected);
        // The listing sorts by it.
        assert_eq!(library.list().notes[0].created, wanted);
        // The stamp returned is the file's.
        assert_eq!(after.stamp, moved.stamp);
    }

    #[test]
    fn the_same_date_writes_nothing_and_a_stale_stamp_or_a_wrong_date_is_refused() {
        let (_dir, library) = new_library();
        let mut note = blank_note("id", "Cours");
        note.created = 1_792_843_200_000;
        let saved = library.create(Path::new(""), &NoteFile::new(note)).unwrap();
        let same = library
            .set_created(&saved.path, 1_792_843_200_000, Some(&saved.stamp))
            .unwrap();
        assert_eq!(
            (same.path.as_path(), same.stamp),
            (saved.path.as_path(), saved.stamp)
        );

        for bad in [-1, i64::MAX, i64::MIN, 253_402_300_800_000 * 2] {
            let error = library.set_created(&saved.path, bad, None).unwrap_err();
            assert_eq!(error.code(), "invalidDate", "{bad}");
        }
        let stale = FileStamp {
            len: saved.stamp.len + 1,
            ..saved.stamp
        };
        let error = library
            .set_created(&saved.path, 1_741_089_600_000, Some(&stale))
            .unwrap_err();
        assert_eq!(error.code(), "noteConflict");
        assert!(saved.path.exists());
        assert_eq!(
            library.read(&saved.path).unwrap().file.note.created,
            1_792_843_200_000
        );
    }

    #[test]
    fn a_renamed_note_keeps_its_place_in_the_order_because_it_is_known_by_id() {
        let (_dir, library) = new_library();
        let a = library
            .create(Path::new("Classe"), &NoteFile::new(blank_note("a", "A")))
            .unwrap();
        library
            .create(Path::new("Classe"), &NoteFile::new(blank_note("b", "B")))
            .unwrap();
        library
            .set_binder_order(Path::new("Classe"), &["b".into(), "a".into()])
            .unwrap();
        library
            .set_created(&a.path, 1_741_089_600_000, None)
            .unwrap();
        assert_eq!(library.binder_order(Path::new("Classe")), ["b", "a"]);
        assert_eq!(
            library.list().notes.iter().filter(|n| n.id == "a").count(),
            1
        );
    }
}
