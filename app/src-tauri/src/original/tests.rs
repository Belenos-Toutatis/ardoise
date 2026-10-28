use super::*;
use crate::library::tests::{library, sample_note};
use crate::notes::{open_in, save_in};
use crate::tablet::store_page_in;
use crate::thumbnails::thumbnail_png;
use ardoise_core::library::FileStamp;
use ardoise_core::note::{Origin, Paper};
use ardoise_core::settings::SettingsStore;
use ardoise_device::service::PageContext;
use ardoise_ink::{Point, Rgba};
use ardoise_sim::SimTablet;
use std::path::PathBuf;

const SLATE: PageSize = PageSize {
    width: ardoise_proto::SLATE_LARGE_WIDTH,
    height: ardoise_proto::SLATE_LARGE_HEIGHT,
};

/// A page as the simulated tablet sends it.
fn sim_page() -> DownloadedPage {
    let tablet = SimTablet::slate_large();
    let config = tablet.config();
    tablet.draw_page(&ardoise_sim::drawings::square_and_circle(
        config.width,
        config.height,
    ));
    let stored = &tablet.stored_pages()[0];
    DownloadedPage {
        bytes: stored.bytes.clone(),
        timestamp: Some(stored.timestamp),
        announced_size: u32::try_from(stored.bytes.len()).unwrap(),
    }
}

/// The ink of an original.
fn ink(original: NoteOriginal) -> (PageSize, Vec<Layer>, Vec<Stroke>) {
    match original {
        NoteOriginal::Tablet {
            page,
            layers,
            strokes,
        }
        | NoteOriginal::FirstVersion {
            page,
            layers,
            strokes,
            ..
        } => (page, layers, strokes),
        NoteOriginal::None => panic!("no original"),
    }
}

/// What the editor does with "Revenir à l'original": the original's ink in the current note,
/// saved normally.
fn go_back(library: &Library, path: &Path, stamp: &FileStamp, original: NoteOriginal) -> FileStamp {
    let mut note = library.read(path).unwrap().file.note;
    (note.page, note.layers, note.strokes) = ink(original);
    save_in(library, path, note, stamp).unwrap()
}

/// An edit of the ink, of the title, tags and paper too; returns the new stamp.
fn edit(library: &Library, path: &Path, stamp: &FileStamp) -> FileStamp {
    let mut note = open_in(library, path).unwrap().note;
    note.strokes.truncate(1);
    note.strokes[0].color = Rgba::rgb(0xd0, 0x20, 0x20);
    for p in &mut note.strokes[0].points {
        p.x += 500.0;
    }
    note.strokes.push(Stroke {
        points: vec![
            Point {
                x: 100.0,
                y: 100.0,
                pressure: 0.5,
                t: 1,
            },
            Point {
                x: 900.0,
                y: 300.0,
                pressure: 0.5,
                t: 20,
            },
        ],
        ..note.strokes[0].clone()
    });
    note.page = PageSize {
        width: note.page.height,
        height: note.page.width,
    };
    note.title = "Cours".into();
    note.tags = vec!["physique".into()];
    note.background.paper = Paper::Cream;
    save_in(library, path, note, stamp).unwrap()
}

#[test]
fn a_tablet_note_goes_back_to_what_the_tablet_sent() {
    for orientation in Orientation::ALL {
        let settings_dir = tempfile::tempdir().unwrap();
        let store = SettingsStore::locate(settings_dir.path(), settings_dir.path(), "PC-TEST");
        let settings = store
            .update(|s| s.tablet.orientation = orientation)
            .unwrap();
        let (dir, library) = library();
        let context = PageContext {
            size: SLATE,
            orientation,
        };
        let path = store_page_in(
            Some(&store),
            Some("fr-FR"),
            dir.path(),
            sim_page(),
            &context,
        )
        .unwrap();
        let synced = library.read(&path).unwrap();
        let tablet = TabletContext::of(dir.path(), &settings, Some("fr-FR"));
        assert_eq!(
            tablet.size, SLATE,
            "no registered tablet: the Bamboo Slate's area"
        );
        assert_eq!(
            (tablet.orientation, tablet.layer_name),
            (orientation, "Calque 1")
        );

        // Never edited: the tablet's page is the original already.
        let expected = (
            synced.file.note.page,
            synced.file.note.layers.clone(),
            synced.file.note.strokes.clone(),
        );
        assert!(!expected.2.is_empty());
        let original = original_in(&library, &path, &tablet).unwrap();
        assert!(matches!(original, NoteOriginal::Tablet { .. }));
        assert_eq!(ink(original), expected, "{orientation:?}");

        // Edited and saved: still the tablet's page, and no second copy is kept.
        let stamp = edit(&library, &path, &synced.stamp);
        let after = library.read(&path).unwrap().file;
        assert_ne!(after.note.strokes, expected.2);
        assert_eq!(after.original, synced.file.original);
        assert!(after.original_note.is_none());
        let original = original_in(&library, &path, &tablet).unwrap();
        assert_eq!(ink(original.clone()), expected);

        // Back to the original: the ink as synced, the rest as edited.
        go_back(&library, &path, &stamp, original);
        let restored = library.read(&path).unwrap().file.note;
        assert_eq!(
            (restored.page, restored.layers, restored.strokes),
            expected,
            "{orientation:?}"
        );
        assert_eq!(restored.title, "Cours");
        assert_eq!(restored.tags, ["physique"]);
        assert_eq!(restored.background.paper, Paper::Cream);
    }
}

#[test]
fn the_tablet_size_comes_from_the_registration() {
    let (dir, _library) = library();
    let record = serde_json::json!({
        "format": 1,
        "uuid": "a1b2c3d4e5f6",
        "address": "00:00:00:00:00:00",
        "name": "Bamboo Slate",
        "protocol": "slate",
        "width": 21_600,
        "height": 14_800,
        "registered": "2026-10-25T10:00:00Z"
    });
    let file = identity::device_file(dir.path());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, record.to_string()).unwrap();
    let tablet = TabletContext::of(dir.path(), &Settings::default(), Some("en-GB"));
    assert_eq!(
        tablet.size,
        PageSize {
            width: 21_600,
            height: 14_800
        }
    );
    assert_eq!(
        (tablet.orientation, tablet.layer_name),
        (Orientation::Landscape, "Layer 1")
    );
}

/// A note written the way live mode or the Inkspace import writes one (no `original.bin`).
fn write_plain(library: &Library, origin: Origin) -> (PathBuf, FileStamp) {
    let mut note = sample_note("Direct");
    note.origin = origin;
    note.strokes.push(Stroke {
        layer: 0,
        ..note.strokes[0].clone()
    });
    for p in &mut note.strokes[1].points {
        p.y += 2000.0;
    }
    let mut file = NoteFile::new(note);
    file.thumbnail = thumbnail_png(&file.note);
    file.audio = Some(vec![4; 32]);
    let saved = library.create(Path::new(""), &file).unwrap();
    (saved.path, saved.stamp)
}

#[test]
fn live_and_inkspace_notes_keep_their_first_version() {
    for origin in [
        Origin::Live,
        Origin::Inkspace {
            id: "3f2a9c1e-77aa-4c3b-9e5d-0123456789ab".into(),
        },
    ] {
        let (_dir, library) = library();
        let (path, stamp) = write_plain(&library, origin.clone());
        let tablet = TabletContext::of(Path::new("unused"), &Settings::default(), None);
        let written = library.read(&path).unwrap().file;
        assert_eq!(
            original_in(&library, &path, &tablet).unwrap(),
            NoteOriginal::None,
            "never edited: nothing to go back to"
        );

        let stamp = edit(&library, &path, &stamp);
        let after = library.read(&path).unwrap().file;
        let kept = after.original_note.clone().expect("original.ardoise");
        // Note and strokes only.
        let nested = read_from_bytes(&kept).unwrap();
        assert_eq!(nested.note, written.note);
        assert!(nested.thumbnail.is_none() && nested.audio.is_none());
        assert_eq!(after.audio, written.audio, "the narration stays");
        let expected = (
            written.note.page,
            written.note.layers.clone(),
            written.note.strokes.clone(),
        );
        let original = original_in(&library, &path, &tablet).unwrap();
        assert!(matches!(original, NoteOriginal::FirstVersion { .. }));
        assert_eq!(ink(original), expected, "{origin:?}");

        // A second edit never touches it.
        let stamp = edit(&library, &path, &stamp);
        assert_eq!(
            library.read(&path).unwrap().file.original_note,
            Some(kept.clone())
        );

        // Back to the original, saved: the first ink, the edited title; the original stays.
        let original = original_in(&library, &path, &tablet).unwrap();
        go_back(&library, &path, &stamp, original);
        let restored = library.read(&path).unwrap().file;
        assert_eq!(
            (
                restored.note.page,
                restored.note.layers,
                restored.note.strokes
            ),
            expected
        );
        assert_eq!(restored.note.title, "Cours");
        assert_eq!(restored.note.origin, origin);
        assert_eq!(restored.original_note, Some(kept));
    }
}

#[test]
fn the_original_is_taken_at_the_first_change_of_the_ink() {
    let (_dir, library) = library();
    let tablet = TabletContext::of(Path::new("unused"), &Settings::default(), None);

    // A blank note filled by a first save ("save as a copy", a note made in Ardoise): an empty
    // page is nothing to go back to.
    let blank = NoteFile::new(crate::library::blank_note("Copie", "Calque 1", 1));
    let saved = library.create(Path::new(""), &blank).unwrap();
    let content = sample_note("Copie");
    let mut filled = blank.note.clone();
    filled.strokes = content.strokes.clone();
    let stamp = save_in(&library, &saved.path, filled, &saved.stamp).unwrap();
    let first = library.read(&saved.path).unwrap().file;
    assert!(first.original_note.is_none());
    assert_eq!(
        original_in(&library, &saved.path, &tablet).unwrap(),
        NoteOriginal::None
    );

    // Only the paper changes: still nothing kept.
    let mut paper = open_in(&library, &saved.path).unwrap().note;
    paper.background.paper = Paper::Slate;
    let stamp = save_in(&library, &saved.path, paper, &stamp).unwrap();
    assert!(
        library
            .read(&saved.path)
            .unwrap()
            .file
            .original_note
            .is_none()
    );

    // The first change of the ink keeps the version before it.
    edit(&library, &saved.path, &stamp);
    let (_, _, strokes) = ink(original_in(&library, &saved.path, &tablet).unwrap());
    // As the first save stored them (the format quantises positions).
    assert_eq!(strokes, first.note.strokes);
}

#[test]
fn keep_original_never_replaces_an_original() {
    let mut file = NoteFile::new(sample_note("A"));
    let mut edited = file.note.clone();
    edited.strokes.clear();
    file.original = Some(vec![1, 2, 3]);
    keep_original(&mut file, &edited).unwrap();
    assert!(
        file.original_note.is_none(),
        "a tablet note keeps original.bin only"
    );

    file.original = None;
    keep_original(&mut file, &edited).unwrap();
    let first = file.original_note.clone().unwrap();
    file.note = edited.clone();
    let mut again = edited;
    again.strokes = sample_note("B").strokes;
    keep_original(&mut file, &again).unwrap();
    assert_eq!(file.original_note, Some(first));
}

#[test]
fn a_damaged_original_is_reported() {
    let tablet = TabletContext::of(Path::new("unused"), &Settings::default(), None);
    let mut file = NoteFile::new(sample_note("A"));
    file.original_note = Some(b"not a zip".to_vec());
    assert_eq!(
        original_of(&file, &tablet).unwrap_err().code(),
        "originalDamaged"
    );
    file.original_note = None;
    file.original = Some(vec![0xff; 7]);
    assert_eq!(
        original_of(&file, &tablet).unwrap_err().code(),
        "originalDamaged"
    );

    let (_dir, library) = library();
    let elsewhere = tempfile::tempdir().unwrap();
    let outside = elsewhere.path().join("2026-10-25 Ailleurs.ardoise");
    ardoise_core::format::write_note(&outside, &NoteFile::new(sample_note("A"))).unwrap();
    assert_eq!(
        original_in(&library, &outside, &tablet).unwrap_err().code(),
        "notInLibrary"
    );
}

#[test]
fn the_answer_has_the_shape_of_the_interface() {
    assert_eq!(
        serde_json::to_value(NoteOriginal::None).unwrap(),
        serde_json::json!({ "source": "none" })
    );
    let note = sample_note("A");
    let json = serde_json::to_value(NoteOriginal::FirstVersion {
        page: note.page,
        layers: note.layers.clone(),
        strokes: note.strokes.clone(),
        background_image: None,
    })
    .unwrap();
    assert!(json.get("backgroundImage").is_none());
    assert_eq!(json["source"], "firstVersion");
    assert_eq!(json["page"]["width"], note.page.width);
    assert_eq!(json["layers"][0]["visible"], true);
    assert!(json["strokes"][0]["points"][0]["pressure"].is_number());
    assert_eq!(json["strokes"][0]["color"]["a"], 255);
    let tablet = serde_json::to_value(NoteOriginal::Tablet {
        page: note.page,
        layers: vec![],
        strokes: vec![],
    })
    .unwrap();
    assert_eq!(tablet["source"], "tablet");
    assert!(tablet.get("backgroundImage").is_none());
    let with_image = serde_json::to_value(NoteOriginal::FirstVersion {
        page: note.page,
        layers: vec![],
        strokes: vec![],
        background_image: Some(fiche()),
    })
    .unwrap();
    assert_eq!(with_image["backgroundImage"]["name"], "fiche.pdf");
    assert_eq!(with_image["backgroundImage"]["pdfPage"], 2);
    assert_eq!(with_image["backgroundImage"]["quarterTurns"], 0);
}

/// The background image of page 2 of `fiche.pdf`, over an A4 landscape page.
fn fiche() -> BackgroundImage {
    BackgroundImage {
        name: "fiche.pdf".into(),
        pdf_page: Some(2),
        rect: ardoise_core::note::ImageRect {
            x: 0.0,
            y: 1000.0,
            width: 29_700.0,
            height: 19_600.0,
        },
        quarter_turns: 0,
        opacity: 0.8,
    }
}

#[test]
fn the_first_version_gives_the_placement_of_its_background_image() {
    let (_dir, library) = library();
    let tablet = TabletContext::of(Path::new("unused"), &Settings::default(), None);
    let mut note = sample_note("Fiche");
    note.page = ardoise_core::note::A4_LANDSCAPE;
    note.background_image = Some(fiche());
    let mut file = NoteFile::new(note);
    file.background = Some(ardoise_core::format::BackgroundFile {
        extension: "png".into(),
        bytes: ardoise_render::encode_png(&crate::background::tests::halves(30, 20)).unwrap(),
    });
    let saved = library.create(Path::new(""), &file).unwrap();

    // Turned a quarter turn (page, strokes and image) and saved.
    let mut turned = open_in(&library, &saved.path).unwrap().note;
    turned.page = PageSize {
        width: turned.page.height,
        height: turned.page.width,
    };
    for p in &mut turned.strokes[0].points {
        p.x += 10.0;
    }
    let image = turned.background_image.as_mut().unwrap();
    image.quarter_turns = 1;
    image.rect = ardoise_core::note::ImageRect {
        x: 1000.0,
        y: 0.0,
        width: 19_600.0,
        height: 29_700.0,
    };
    save_in(&library, &saved.path, turned, &saved.stamp).unwrap();
    let after = library.read(&saved.path).unwrap().file;
    assert_eq!(after.note.background_image.unwrap().quarter_turns, 1);
    // `original.ardoise` describes the image but does not hold its pixels.
    let nested = read_from_bytes(after.original_note.as_ref().unwrap()).unwrap();
    assert_eq!(nested.note.background_image, Some(fiche()));
    assert!(nested.background.is_none());

    match original_in(&library, &saved.path, &tablet).unwrap() {
        NoteOriginal::FirstVersion {
            page,
            background_image,
            ..
        } => {
            assert_eq!(page, ardoise_core::note::A4_LANDSCAPE);
            assert_eq!(background_image, Some(fiche()));
        }
        other => panic!("{other:?}"),
    }

    // A note whose first version had no image: none.
    let (path, stamp) = write_plain(&library, Origin::Live);
    edit(&library, &path, &stamp);
    assert!(matches!(
        original_in(&library, &path, &tablet).unwrap(),
        NoteOriginal::FirstVersion {
            background_image: None,
            ..
        }
    ));
}

#[test]
fn a_split_keeps_the_original_of_the_note_and_none_for_the_new_one() {
    use crate::operations::{SplitSpec, split_in};
    let (_dir, library) = library();
    let tablet = TabletContext::of(Path::new("unused"), &Settings::default(), None);
    let (path, stamp) = write_plain(&library, Origin::Live);
    let before = library.read(&path).unwrap().file.note;
    let mut note = before.clone();
    // The second stroke is drawn later than the first.
    for p in &mut note.strokes[1].points {
        p.t += 10_000;
    }
    let result = split_in(
        &library,
        &path,
        note.clone(),
        &stamp,
        &SplitSpec::Time { at: 5_000 },
        "Suite",
        None,
    )
    .unwrap();
    // The kept part is an edit of the note: its original is the note before the split.
    let (_, _, strokes) = ink(original_in(&library, &path, &tablet).unwrap());
    assert_eq!(strokes, before.strokes);
    // The new note was never edited: no original until its first edit.
    let created = PathBuf::from(&result.created.path);
    assert_eq!(
        original_in(&library, &created, &tablet).unwrap(),
        NoteOriginal::None
    );
}
