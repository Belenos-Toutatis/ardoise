//! End-to-end import of synthetic Inkspace data (made up here, publishable).

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use ardoise_core::note::{A4_LANDSCAPE, A4_PORTRAIT, Origin, Timing};
use ardoise_ink::{Rgba, Tool};
use ardoise_inkspace::compare::{self, PREVIEW_SCALE};
use ardoise_inkspace::convert::note_id;
use ardoise_inkspace::{DecodeError, ImportError, THRESHOLDS, TrackedReader, import_local_state};
use common::{SynthNote, SynthStroke};
use uuid::Uuid;

const LANDSCAPE_ID: u128 = 0x0a0b_0c0d_1111_4222_8333_4444_5555_6666;
const PORTRAIT_ID: u128 = 0x0f0e_0d0c_1111_4222_8333_4444_5555_6666;

fn samples(x0: f64, y0: f64, n: usize) -> Vec<(f64, f64)> {
    (0..n)
        .map(|i| (x0 + 0.6 * i as f64, y0 + 0.3 * i as f64))
        .collect()
}

fn notes() -> Vec<SynthNote> {
    vec![
        SynthNote {
            id: Uuid::from_u128(LANDSCAPE_ID),
            created: 1_600_000_100_000,
            modified: 1_600_001_000_000,
            landscape: true,
            layers: vec![vec![
                SynthStroke::through(
                    &samples(100.0, 200.0, 50),
                    0x51A7_F9FF,
                    Some("200"),
                    Some(1_600_000_101_000),
                ),
                SynthStroke::through(
                    &samples(150.0, 220.0, 30),
                    0xBB3E_9AFF,
                    Some("60"),
                    Some(1_600_000_105_000),
                ),
            ]],
        },
        SynthNote {
            id: Uuid::from_u128(PORTRAIT_ID),
            created: 1_650_000_000_000,
            modified: 1_650_000_000_000,
            landscape: false,
            layers: vec![
                vec![SynthStroke::through(
                    &samples(400.0, 600.0, 20),
                    0x0000_00FF,
                    None,
                    None,
                )],
                vec![SynthStroke::through(
                    &samples(420.0, 650.0, 10),
                    0xD800_00FF,
                    Some("0"),
                    Some(1_650_000_000_000),
                )],
            ],
        },
    ]
}

#[test]
fn imports_synthetic_notes() {
    let root = tempfile::tempdir().unwrap();
    let local_state = common::write_local_state(root.path(), &notes());
    let reader = TrackedReader::new();
    let report = import_local_state(&local_state, &reader).unwrap();
    assert!(report.failures.is_empty(), "{:?}", report.failures);
    assert_eq!(report.notes.len(), 2);
    assert_eq!(report.total(), 2);

    let landscape = report
        .notes
        .iter()
        .find(|n| n.inkspace_id == Uuid::from_u128(LANDSCAPE_ID).to_string())
        .unwrap();
    let note = &landscape.note;
    assert_eq!(note.id, note_id(&landscape.inkspace_id, 0));
    assert_eq!(
        note.origin,
        Origin::Inkspace {
            id: landscape.inkspace_id.clone()
        }
    );
    assert_eq!(note.page, A4_LANDSCAPE);
    assert_eq!(note.created, 1_600_000_100_000);
    assert_eq!(note.modified, 1_600_001_000_000);
    assert_eq!(note.timing, Timing::Recorded);
    assert_eq!(note.layers.len(), 1);
    assert_eq!(note.strokes.len(), 2);
    let first = &note.strokes[0];
    assert_eq!(first.tool, Tool::Ballpoint);
    assert_eq!(first.color, Rgba::rgb(0x51, 0xA7, 0xF9));
    assert_eq!(first.points.len(), 50);
    // Stroke space (100, 200) → page (200, 864 − 100) × 25.
    assert!((first.points[0].x - 5000.0).abs() < 1e-3);
    assert!((first.points[0].y - 19_100.0).abs() < 1e-3);
    assert_eq!(first.points[0].t, 1000);
    assert_eq!(first.points[2].t, 1010);
    assert_eq!(note.strokes[1].points[1].t, 5017);
    assert_eq!(landscape.stats.control_points, 52 + 32);
    assert_eq!(landscape.stats.points, 80);

    let portrait = report
        .notes
        .iter()
        .find(|n| n.inkspace_id == Uuid::from_u128(PORTRAIT_ID).to_string())
        .unwrap();
    assert_eq!(portrait.note.page, A4_PORTRAIT);
    assert_eq!(portrait.note.timing, Timing::Synthesized);
    assert_eq!(portrait.stats.timing, Timing::Synthesized);
    assert_eq!(portrait.note.layers.len(), 2);
    assert_eq!(portrait.note.strokes[1].layer, 1);
    let p = &portrait.note.strokes[0].points[0];
    assert!((p.x - 10_000.0).abs() < 1e-3 && (p.y - 15_000.0).abs() < 1e-3);
    // Synthesised: 200 Hz, then a gap.
    assert_eq!(portrait.note.strokes[0].points[1].t, 5);
    assert!(portrait.note.strokes[1].points[0].t > portrait.note.strokes[0].points[19].t);
    // No preview folders at all.
    assert!(report.notes.iter().all(|n| n.preview.is_none()));
}

#[test]
fn import_is_deterministic_and_leaves_the_source_untouched() {
    let root = tempfile::tempdir().unwrap();
    let local_state = common::write_local_state(root.path(), &notes());
    let before = common::snapshot(root.path());
    let first = import_local_state(&local_state, &TrackedReader::new()).unwrap();
    let second = import_local_state(&local_state, &TrackedReader::new()).unwrap();
    assert_eq!(first, second);
    assert_eq!(common::snapshot(root.path()), before);
}

#[test]
fn previews_are_found_and_compared() {
    let root = tempfile::tempdir().unwrap();
    let local_state = common::write_local_state(root.path(), &notes());
    let reader = TrackedReader::new();
    let report = import_local_state(&local_state, &reader).unwrap();
    // Write each note's own reference render as its "Inkspace preview", under another
    // timestamp for the portrait note (found by listing its folder).
    for n in &report.notes {
        let dir = local_state.join("db").join("notes").join(&n.inkspace_id);
        std::fs::create_dir_all(&dir).unwrap();
        let png = common::encode_png(&common::reference_render(&n.note, PREVIEW_SCALE));
        let stamp = if n.note.page == A4_LANDSCAPE {
            n.note.modified
        } else {
            42
        };
        std::fs::write(dir.join(format!("preview_{stamp}.png")), png).unwrap();
        std::fs::write(dir.join("preview_7.png"), b"older").unwrap();
    }
    let report = import_local_state(&local_state, &reader).unwrap();
    for n in &report.notes {
        let path = n.preview.as_deref().unwrap();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_ne!(name, "preview_7.png");
        let preview = compare::load_preview(&reader, path).unwrap();
        let rendered = common::reference_render(&n.note, PREVIEW_SCALE);
        let scores = compare::compare_images(&rendered, &preview).unwrap();
        assert_eq!(scores.iou, 1.0);
        assert!(scores.passes(&THRESHOLDS));
        assert!(scores.render_ink > 0);
    }
}

#[test]
fn a_corrupt_stroke_fails_only_its_note() {
    let mut data = notes();
    data[1].layers[0][0].raw_payload = Some(vec![0x05, 0x22, 0x03, 0x01]);
    let root = tempfile::tempdir().unwrap();
    let local_state = common::write_local_state(root.path(), &data);
    let report = import_local_state(&local_state, &TrackedReader::new()).unwrap();
    assert_eq!(report.notes.len(), 1);
    assert_eq!(report.failures.len(), 1);
    assert_eq!(report.total(), 2);
    assert_eq!(
        report.failures[0].inkspace_id,
        Uuid::from_u128(PORTRAIT_ID).to_string()
    );
    assert!(matches!(
        report.failures[0].error,
        DecodeError::Stroke { index: 0, .. }
    ));
}

#[test]
fn empty_and_rootless_stores() {
    let root = tempfile::tempdir().unwrap();
    let local_state = root.path().join("LocalState");
    let db = local_state.join("WacomCloudDb").join("_");
    common::write_leveldb(&db, &[(b"__db_ver__".to_vec(), b"1.1".to_vec())], false);
    let report = import_local_state(&local_state, &TrackedReader::new()).unwrap();
    assert_eq!(report.total(), 0);

    let rootless = tempfile::tempdir().unwrap();
    let local_state = rootless.path().join("LocalState");
    let db = local_state.join("WacomCloudDb").join("_");
    let key = [
        b"__doc__".as_slice(),
        &common::node_ref_uuid(Uuid::from_u128(1)),
    ]
    .concat();
    common::write_leveldb(&db, &[(key, vec![])], false);
    assert!(matches!(
        import_local_state(&local_state, &TrackedReader::new()),
        Err(ImportError::Malformed(DecodeError::MissingNode(_)))
    ));
}

#[test]
fn reads_only_the_database_files() {
    let root = tempfile::tempdir().unwrap();
    let local_state = common::write_local_state(root.path(), &notes());
    let reader = TrackedReader::new();
    import_local_state(&local_state, &reader).unwrap();
    let db = local_state.join("WacomCloudDb").join("_");
    for access in reader
        .accesses()
        .iter()
        .filter(|a| a.kind == ardoise_inkspace::AccessKind::Read)
    {
        assert_eq!(access.path.parent(), Some(db.as_path()));
        let name = access.path.file_name().unwrap().to_string_lossy();
        assert!(
            name == "CURRENT"
                || name.starts_with("MANIFEST-")
                || name.ends_with(".ldb")
                || name.ends_with(".log"),
            "{name}"
        );
        assert!(name != "LOCK" && name != "LOG");
    }
}
