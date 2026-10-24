//! Proof that the importer never reads Inkspace's `db\` settings database, whose `profile`
//! key holds an access token and an e-mail address (SPEC §6.3).
//!
//! A fake `LocalState` gets a real `db\` LevelDB whose `profile` value (and every other value,
//! and the text log) carries a canary. The import and the preview comparison run through one
//! tracked reader; the test then checks every recorded access under `db\` and searches the
//! whole result for the canary.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::Path;

use ardoise_inkspace::{AccessKind, TrackedReader, compare_with_preview, import_local_state};
use common::{SynthNote, SynthStroke};
use uuid::Uuid;

const CANARY: &str = "CANARY-5f0c2b7e-profile-access-token-and-e-mail";

fn utf16le(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// `db\`: an Inkspace-like settings LevelDB (UTF-16LE keys and JSON values), canary inside.
fn write_settings_db(db: &Path) {
    let value =
        |name: &str| format!("{{\"{name}\":\"{CANARY}\",\"email\":\"{CANARY}@example.invalid\"}}");
    let mut entries = Vec::new();
    for key in ["device", "profile", "queue", "settings"] {
        entries.push((utf16le(key), utf16le(&value(key))));
        // An ASCII copy too, so the canary is findable in either encoding.
        entries.push((format!("{key}-ascii").into_bytes(), value(key).into_bytes()));
    }
    common::write_leveldb(db, &entries, true);
    // LevelDB's text log, as Inkspace leaves it.
    std::fs::write(db.join("LOG"), format!("profile {CANARY}\n")).unwrap();
    std::fs::write(db.join("LOG.old"), format!("profile {CANARY}\n")).unwrap();
    // Precondition: every file that can hold data carries the canary. CURRENT, LOCK and the
    // MANIFEST have a fixed LevelDB format and cannot.
    let mut checked = Vec::new();
    for entry in std::fs::read_dir(db).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let bytes = std::fs::read(&path).unwrap();
        if name == "CURRENT" || name == "LOCK" || name.starts_with("MANIFEST-") || bytes.is_empty()
        {
            continue;
        }
        assert!(
            contains(&bytes, CANARY.as_bytes()) || contains(&bytes, &utf16le(CANARY)),
            "canary missing from {name}"
        );
        checked.push(name);
    }
    assert!(
        checked.iter().any(|n| n.ends_with(".ldb"))
            && checked.iter().any(|n| n == "LOG")
            && checked.iter().any(|n| n == "LOG.old"),
        "expected a table file, LOG and LOG.old with the canary: {checked:?}"
    );
}

fn stroke() -> SynthStroke {
    let samples: Vec<(f64, f64)> = (0..40)
        .map(|i| (300.0 + i as f64, 400.0 + 0.5 * i as f64))
        .collect();
    SynthStroke::through(&samples, 0x0000_00ff, Some("200"), Some(1_700_000_001_000))
}

#[test]
fn importer_never_reads_the_profile_key() {
    let root = tempfile::tempdir().unwrap();
    let exact = SynthNote {
        id: Uuid::from_u128(0x1111_2222_3333_4444_8555_6666_7777_8888),
        created: 1_700_000_000_000,
        modified: 1_700_000_100_000,
        landscape: true,
        layers: vec![vec![stroke()]],
    };
    // Its preview has another timestamp: the importer must list its folder to find it.
    let fallback = SynthNote {
        id: Uuid::from_u128(0x9999_2222_3333_4444_8555_6666_7777_8888),
        modified: 1_700_000_200_000,
        ..exact.clone()
    };
    let local_state = common::write_local_state(root.path(), &[exact.clone(), fallback.clone()]);
    let db = local_state.join("db");
    write_settings_db(&db);

    // Previews (read for the comparison), thumbnails and a stray note folder (never read).
    let blank = ardoise_render::Image {
        width: 1188,
        height: 864,
        pixels: vec![255; 1188 * 864 * 4],
    };
    let png = common::encode_png(&blank);
    assert!(!contains(&png, CANARY.as_bytes()));
    let notes = db.join("notes");
    let exact_dir = notes.join(exact.id.to_string());
    let fallback_dir = notes.join(fallback.id.to_string());
    std::fs::create_dir_all(&exact_dir).unwrap();
    std::fs::create_dir_all(&fallback_dir).unwrap();
    std::fs::write(
        exact_dir.join(format!("preview_{}.png", exact.modified)),
        &png,
    )
    .unwrap();
    std::fs::write(
        exact_dir.join(format!("thumb_{}.png", exact.modified)),
        CANARY,
    )
    .unwrap();
    std::fs::write(fallback_dir.join("preview_1600000000000.png"), CANARY).unwrap();
    std::fs::write(fallback_dir.join("preview_1650000000000.png"), &png).unwrap();
    std::fs::write(fallback_dir.join("thumb_1650000000000.png"), CANARY).unwrap();
    let stray = notes.join(Uuid::from_u128(42).to_string());
    std::fs::create_dir_all(&stray).unwrap();
    std::fs::write(stray.join("preview_1.png"), CANARY).unwrap();

    // Import and compare, as `ardoise.exe --import-inkspace --compare` does.
    let reader = TrackedReader::new();
    let report = import_local_state(&local_state, &reader).unwrap();
    assert_eq!(report.notes.len(), 2, "{:?}", report.failures);
    let comparisons: Vec<_> = report
        .notes
        .iter()
        .map(|n| compare_with_preview(&reader, n))
        .collect();
    assert!(comparisons.iter().all(Result::is_ok), "{comparisons:?}");

    // Every access under db\ is one of the two preview folders or a preview inside them.
    let accesses = reader.accesses();
    let mut preview_reads = Vec::new();
    for access in accesses.iter().filter(|a| a.path.starts_with(&db)) {
        let in_preview_dir = |p: &Path| {
            p.parent() == Some(exact_dir.as_path()) || p.parent() == Some(fallback_dir.as_path())
        };
        let is_preview = access.path.file_name().is_some_and(|n| {
            let n = n.to_string_lossy();
            n.starts_with("preview_") && n.ends_with(".png")
        }) && in_preview_dir(&access.path);
        let is_preview_dir = access.path == exact_dir || access.path == fallback_dir;
        match access.kind {
            AccessKind::Read => {
                assert!(is_preview, "read outside the previews: {access:?}");
                preview_reads.push(access.path.clone());
            }
            AccessKind::List | AccessKind::Stat => assert!(
                is_preview || is_preview_dir,
                "unexpected access under db: {access:?}"
            ),
        }
    }
    // The comparison read exactly the two expected previews (the newest one for `fallback`).
    preview_reads.sort();
    let mut expected = vec![
        exact_dir.join(format!("preview_{}.png", exact.modified)),
        fallback_dir.join("preview_1650000000000.png"),
    ];
    expected.sort();
    assert_eq!(preview_reads, expected);
    // Nothing in db\ itself was even listed.
    assert!(!accesses.iter().any(|a| a.path == db || a.path == notes));

    // The canary appears nowhere in the result, in any encoding.
    let mut output = format!("{report:#?}\n{comparisons:#?}\n");
    for failure in &report.failures {
        output.push_str(&failure.error.to_string());
    }
    for comparison in &comparisons {
        if let Err(e) = comparison {
            output.push_str(&e.to_string());
        }
    }
    let utf16_debug: String = CANARY.chars().map(|c| format!("{c}\\0")).collect();
    assert!(!output.contains(CANARY));
    assert!(!output.contains(&utf16_debug));
    assert!(!output.contains("CANARY"));
}
