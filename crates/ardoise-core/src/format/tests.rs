use std::fs;
use std::io::{Cursor, Write};
use std::path::{Path, PathBuf};

use ardoise_ink::{Point, Rgba, Stroke, Tool};
use serde_json::{Value, json};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use super::*;
use crate::note::{A4_PORTRAIT, Background, Layer, Origin, Paper, Template, Timing};
use crate::testutil::{Lcg, blank_note};

/// What a save/load cycle makes of a note: points snapped to the stored precision.
fn stored(note: &Note) -> Note {
    let mut note = note.clone();
    for stroke in &mut note.strokes {
        for point in &mut stroke.points {
            *point = strokes::round_trip_point(point).unwrap();
        }
    }
    note
}

fn random_file(rng: &mut Lcg) -> NoteFile {
    let mut file = NoteFile::new(rng.note());
    let blob = |rng: &mut Lcg| -> Option<Vec<u8>> {
        (rng.below(2) == 0).then(|| (0..rng.below(300)).map(|_| rng.next_u32() as u8).collect())
    };
    file.original = blob(rng);
    file.thumbnail = blob(rng);
    file.audio = blob(rng);
    file.text = blob(rng);
    if let Some(bytes) = blob(rng) {
        file.background = Some(BackgroundFile {
            extension: ["png", "pdf", "jpg"][rng.below(3) as usize].to_owned(),
            bytes,
        });
    }
    file
}

/// Builds an archive by hand, for files this version would not write itself.
fn raw_archive(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in members {
        if name.ends_with('/') {
            zip.add_directory(*name, SimpleFileOptions::default())
                .unwrap();
        } else {
            zip.start_file(*name, SimpleFileOptions::default()).unwrap();
            zip.write_all(bytes).unwrap();
        }
    }
    zip.finish().unwrap().into_inner()
}

fn note_json_value(file: &NoteFile) -> Value {
    serde_json::from_slice(&note_json(file, FormatVersion::CURRENT).unwrap()).unwrap()
}

fn member_names(bytes: &[u8]) -> Vec<String> {
    let archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
    archive
        .file_names()
        .map(|n| n.unwrap().into_owned())
        .collect()
}

#[test]
fn random_notes_round_trip() {
    let mut rng = Lcg::new(0xa5d0_15e5);
    for _ in 0..250 {
        let file = random_file(&mut rng);
        let bytes = to_bytes(&file).unwrap();
        let read = read_from_bytes(&bytes).unwrap();
        let mut expected = file.clone();
        expected.note = stored(&file.note);
        assert_eq!(read, expected);
        // Saving what was read gives back exactly the same note.
        let again = read_from_bytes(&to_bytes(&read).unwrap()).unwrap();
        assert_eq!(again, read);
        // The header alone matches, without strokes.
        let header = read_header_from_bytes(&bytes).unwrap();
        assert_eq!(header.format, FormatVersion::CURRENT);
        assert_eq!(header.note, metadata_only(&file.note));
    }
}

#[test]
fn empty_note_round_trips() {
    let file = NoteFile::new(blank_note("id-empty", ""));
    let bytes = to_bytes(&file).unwrap();
    assert_eq!(member_names(&bytes), [NOTE_JSON, STROKES_BIN]);
    assert_eq!(read_from_bytes(&bytes).unwrap(), file);
}

#[test]
fn many_layers_extreme_coordinates_and_long_times() {
    let mut note = blank_note("id-big", "Grande note");
    note.layers = (0..1000)
        .map(|i| Layer {
            name: format!("L{i}"),
            visible: i % 2 == 0,
        })
        .collect();
    note.strokes = (0..1000u32)
        .map(|i| Stroke {
            tool: Tool::ALL[i as usize % Tool::ALL.len()],
            color: Rgba::from_rgba_u32(i.wrapping_mul(2_654_435_761)),
            width: i as f32 * 0.37,
            layer: 999 - i,
            points: vec![
                Point {
                    x: -strokes::MAX_COORD,
                    y: strokes::MAX_COORD,
                    pressure: 0.0,
                    t: u32::MAX - i,
                },
                Point {
                    x: i as f32 * 1234.567,
                    y: -(i as f32) * 0.015,
                    pressure: 1.0,
                    t: i,
                },
            ],
        })
        .collect();
    let file = NoteFile::new(note);
    let read = read_from_bytes(&to_bytes(&file).unwrap()).unwrap();
    assert_eq!(read.note, stored(&file.note));
    assert_eq!(read.note.strokes[0].points[0].t, u32::MAX);
    assert_eq!(read.note.strokes[0].points[0].x, -strokes::MAX_COORD);
    assert_eq!(read.note.strokes[999].layer, 0);
}

#[test]
fn note_json_layout() {
    let mut note = blank_note("4f1c-uuid", "Cours");
    note.tags = vec!["maths".to_owned()];
    note.origin = Origin::Inkspace {
        id: "abc".to_owned(),
    };
    let value = note_json_value(&NoteFile::new(note));
    assert_eq!(value["format"]["version"], 1);
    assert_eq!(value["format"]["minor"], 0);
    assert_eq!(
        value["format"]["writer"],
        format!("Ardoise {}", env!("CARGO_PKG_VERSION"))
    );
    assert!(value.get("strokes").is_none());
    assert_eq!(value["id"], "4f1c-uuid");
    assert_eq!(value["page"], json!({"width": 29700, "height": 21600}));
    assert_eq!(
        value["background"],
        json!({"paper": "white", "template": "blank", "texture": false})
    );
    assert_eq!(value["origin"], json!({"kind": "inkspace", "id": "abc"}));
    assert_eq!(value["timing"], "recorded");
    // "format" comes first, so that a human opening the file sees the version at once.
    let text = String::from_utf8(
        note_json(&NoteFile::new(blank_note("a", "b")), FormatVersion::CURRENT).unwrap(),
    )
    .unwrap();
    assert!(text.trim_start().starts_with("{\n  \"format\""), "{text}");
}

#[test]
fn documented_note_json_example_parses() {
    // The example of docs/file-format.md.
    let text = r#"{
      "format": { "version": 1, "minor": 0, "writer": "Ardoise 0.1.0" },
      "background": { "paper": "cream", "template": "seyes", "texture": true },
      "created": 1792843200000,
      "id": "6f0c9a52-3d7e-4f0a-9a43-1c2b5e8d7f10",
      "layers": [ { "name": "Calque 1", "visible": true } ],
      "modified": 1792846800000,
      "origin": { "kind": "tablet" },
      "page": { "width": 29700, "height": 21600 },
      "tags": [ "maths", "3e B" ],
      "timing": "recorded",
      "title": "Théorème de Pythagore"
    }"#;
    let parsed = parse_note_json(text.as_bytes()).unwrap();
    assert_eq!(parsed.format, FormatVersion { major: 1, minor: 0 });
    assert_eq!(parsed.writer.as_deref(), Some("Ardoise 0.1.0"));
    assert_eq!(parsed.note.title, "Théorème de Pythagore");
    assert_eq!(parsed.note.background.paper, Paper::Cream);
    assert_eq!(parsed.note.background.template, Template::Seyes);
    assert_eq!(parsed.note.origin, Origin::Tablet);
    assert!(parsed.unknown_fields.is_empty());

    let custom = parse_note_json(
        br#"{"format":{"version":1},"background":{"paper":{"custom":{"r":1,"g":2,"b":3,"a":255}},"template":"blank","texture":false},"created":0,"id":"x","layers":[{"name":"","visible":true}],"modified":0,"origin":{"kind":"inkspace","id":"u"},"page":{"width":1,"height":1},"tags":[],"timing":"synthesized","title":""}"#,
    )
    .unwrap();
    assert_eq!(
        custom.note.background.paper,
        Paper::Custom(Rgba {
            r: 1,
            g: 2,
            b: 3,
            a: 255
        })
    );
    assert_eq!(custom.format, FormatVersion { major: 1, minor: 0 });
}

fn future_minor_archive() -> Vec<u8> {
    let mut value = note_json_value(&NoteFile::new(blank_note("future", "Du futur")));
    value["format"] = json!({"version": 1, "minor": 7, "writer": "Ardoise 9.9", "futureKey": true});
    value["futureField"] = json!({"nested": [1, 2, {"deep": "yes"}]});
    value["futureList"] = json!(["a", "b"]);
    let json = serde_json::to_vec(&value).unwrap();
    let strokes = strokes::encode(&[]).unwrap();
    raw_archive(&[
        (NOTE_JSON, &json),
        (STROKES_BIN, &strokes),
        ("future/", b""),
        ("future/extra.bin", b"\x00\x01\x02 future"),
        (THUMB_PNG, b"thumb"),
        ("zzz.txt", b"last"),
    ])
}

#[test]
fn unknown_fields_and_members_survive_a_rewrite() {
    let file = read_from_bytes(&future_minor_archive()).unwrap();
    assert_eq!(file.note.title, "Du futur");
    assert_eq!(
        file.unknown_fields["futureField"],
        json!({"nested": [1, 2, {"deep": "yes"}]})
    );
    assert_eq!(file.unknown_fields["futureList"], json!(["a", "b"]));
    assert_eq!(file.unknown_fields.len(), 2);
    assert_eq!(file.thumbnail.as_deref(), Some(&b"thumb"[..]));
    let names: Vec<&str> = file
        .unknown_members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, ["future/", "future/extra.bin", "zzz.txt"]);

    // Edit and save with this version: everything unknown is still there.
    let mut edited = file.clone();
    edited.note.title = "Modifiée".to_owned();
    let bytes = to_bytes(&edited).unwrap();
    let reread = read_from_bytes(&bytes).unwrap();
    assert_eq!(reread, edited);
    let value: Value = {
        let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
        let mut json = Vec::new();
        archive
            .by_name(NOTE_JSON)
            .unwrap()
            .read_to_end(&mut json)
            .unwrap();
        serde_json::from_slice(&json).unwrap()
    };
    assert_eq!(value["futureField"]["nested"][2]["deep"], "yes");
    assert_eq!(value["title"], "Modifiée");
    // The format object is this version's own.
    assert_eq!(value["format"]["minor"], 0);
    let names = member_names(&bytes);
    assert!(names.contains(&"future/extra.bin".to_owned()));
    assert!(names.contains(&"future/".to_owned()));
}

#[test]
fn unknown_fields_never_override_known_ones() {
    let mut file = NoteFile::new(blank_note("id", "Vrai titre"));
    file.unknown_fields
        .insert("title".to_owned(), json!("faux"));
    file.unknown_fields
        .insert("format".to_owned(), json!({"version": 99}));
    file.unknown_fields
        .insert("strokes".to_owned(), json!([1, 2]));
    file.unknown_fields.insert("extra".to_owned(), json!(1));
    let read = read_from_bytes(&to_bytes(&file).unwrap()).unwrap();
    assert_eq!(read.note.title, "Vrai titre");
    assert_eq!(read.unknown_fields.len(), 1);
    assert_eq!(read.unknown_fields["extra"], json!(1));
}

#[test]
fn newer_major_version_is_refused() {
    // A version-2 file may change everything: only format.version is looked at.
    let json = br#"{"format": {"version": 2, "minor": 3}, "somethingNew": true}"#;
    let bytes = raw_archive(&[(NOTE_JSON, json), ("strokes2.bin", b"??")]);
    let error = read_from_bytes(&bytes).unwrap_err();
    assert!(
        matches!(
            error,
            FormatError::NewerVersion {
                found: FormatVersion { major: 2, minor: 3 }
            }
        ),
        "{error:?}"
    );
    assert_eq!(error.code(), "noteNewerVersion");
    let message = error.to_string();
    assert!(
        message.contains("made by a newer version of Ardoise"),
        "{message}"
    );
    assert!(message.contains("2.3"), "{message}");
    assert!(matches!(
        read_header_from_bytes(&bytes),
        Err(FormatError::NewerVersion { .. })
    ));

    let huge = br#"{"format": {"version": 99999999999}}"#;
    assert!(matches!(
        read_from_bytes(&raw_archive(&[(NOTE_JSON, huge)])),
        Err(FormatError::NewerVersion { .. })
    ));
}

#[test]
fn newer_minor_version_is_accepted() {
    let file = NoteFile::new(blank_note("id", "Mineure"));
    let bytes = to_bytes_with_version(
        &file,
        FormatVersion {
            major: 1,
            minor: 42,
        },
    )
    .unwrap();
    let header = read_header_from_bytes(&bytes).unwrap();
    assert_eq!(
        header.format,
        FormatVersion {
            major: 1,
            minor: 42
        }
    );
    assert_eq!(read_from_bytes(&bytes).unwrap(), file);
}

#[test]
fn invalid_format_object_is_refused() {
    let strokes = strokes::encode(&[]).unwrap();
    for json in [
        &br#"{"id": "no format"}"#[..],
        br#"{"format": {"version": "1"}}"#,
        br#"{"format": {"version": 0}}"#,
        br#"{"format": {"version": -1}}"#,
        br#"{"format": 1}"#,
        br#"[1, 2, 3]"#,
        br#"{"format": {"version": 1}, "title": "missing fields"}"#,
        br#"{"format": {"version": 1}"#,
        b"\xff\xfe not json",
    ] {
        let bytes = raw_archive(&[(NOTE_JSON, json), (STROKES_BIN, &strokes)]);
        let error = read_from_bytes(&bytes).unwrap_err();
        assert!(
            matches!(error, FormatError::Metadata(_)),
            "{json:?}: {error:?}"
        );
        assert_eq!(error.code(), "noteDamaged");
    }
}

#[test]
fn missing_members_are_reported() {
    let json = note_json(&NoteFile::new(blank_note("a", "b")), FormatVersion::CURRENT).unwrap();
    let strokes = strokes::encode(&[]).unwrap();
    assert!(matches!(
        read_from_bytes(&raw_archive(&[(STROKES_BIN, &strokes)])),
        Err(FormatError::MissingMember(NOTE_JSON))
    ));
    assert!(matches!(
        read_from_bytes(&raw_archive(&[(NOTE_JSON, &json)])),
        Err(FormatError::MissingMember(STROKES_BIN))
    ));
    // The header alone does not need strokes.bin.
    assert!(read_header_from_bytes(&raw_archive(&[(NOTE_JSON, &json)])).is_ok());
}

#[test]
fn damaged_strokes_are_reported() {
    let json = note_json(&NoteFile::new(blank_note("a", "b")), FormatVersion::CURRENT).unwrap();
    let bytes = raw_archive(&[(NOTE_JSON, &json), (STROKES_BIN, b"ASTK\x01\x05")]);
    assert!(matches!(
        read_from_bytes(&bytes),
        Err(FormatError::Strokes(StrokesError::CountTooLarge { .. }))
    ));
}

#[test]
fn model_invariants_are_checked_both_ways() {
    let mut no_layer = blank_note("a", "b");
    no_layer.layers.clear();
    assert!(matches!(
        to_bytes(&NoteFile::new(no_layer)),
        Err(FormatError::InvalidNote(_))
    ));

    let mut bad_layer = blank_note("a", "b");
    bad_layer.strokes.push(Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 1.0,
        layer: 1,
        points: Vec::new(),
    });
    let error = to_bytes(&NoteFile::new(bad_layer.clone())).unwrap_err();
    assert_eq!(error.code(), "noteInvalid");

    let mut zero_page = blank_note("a", "b");
    zero_page.page.width = 0;
    assert!(to_bytes(&NoteFile::new(zero_page)).is_err());

    // A file written by someone else with a stroke on a missing layer is refused on read.
    let json = note_json(&NoteFile::new(bad_layer.clone()), FormatVersion::CURRENT).unwrap();
    let strokes = strokes::encode(&bad_layer.strokes).unwrap();
    let bytes = raw_archive(&[(NOTE_JSON, &json), (STROKES_BIN, &strokes)]);
    assert!(matches!(
        read_from_bytes(&bytes),
        Err(FormatError::InvalidNote(_))
    ));

    let mut nan = blank_note("a", "b");
    nan.strokes.push(Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 1.0,
        layer: 0,
        points: vec![Point {
            x: f32::NAN,
            y: 0.0,
            pressure: 0.5,
            t: 0,
        }],
    });
    let error = to_bytes(&NoteFile::new(nan)).unwrap_err();
    assert!(matches!(
        error,
        FormatError::Strokes(StrokesError::InvalidPoint { .. })
    ));
    assert_eq!(error.code(), "noteInvalid");
}

#[test]
fn not_an_archive_is_refused() {
    for bytes in [
        &b""[..],
        b"PK",
        b"hello, this is not a zip file at all",
        b"PK\x05\x06\x00\x00\x00\x00\xff\xff\xff\xff\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00",
    ] {
        let error = read_from_bytes(bytes).unwrap_err();
        assert!(matches!(error, FormatError::Archive(_)), "{error:?}");
        assert_eq!(error.code(), "noteDamaged");
        assert!(read_header_from_bytes(bytes).is_err());
    }
}

#[test]
fn every_truncation_is_an_error() {
    let mut rng = Lcg::new(99);
    let bytes = to_bytes(&random_file(&mut rng)).unwrap();
    for len in 0..bytes.len() {
        assert!(
            read_from_bytes(&bytes[..len]).is_err(),
            "prefix of {len} bytes"
        );
    }
}

#[test]
fn damaged_bytes_never_panic() {
    let mut rng = Lcg::new(1234);
    let mut file = random_file(&mut rng);
    file.note.strokes = rng.strokes(3, 20);
    for stroke in &mut file.note.strokes {
        stroke.layer = 0;
    }
    let valid = to_bytes(&file).unwrap();
    let mut errors = 0;
    for _ in 0..2000 {
        let mut bytes = valid.clone();
        for _ in 0..1 + rng.below(3) {
            let at = rng.below(bytes.len() as u64) as usize;
            bytes[at] ^= 1 << rng.below(8);
        }
        if read_from_bytes(&bytes).is_err() {
            errors += 1;
        }
        let _ = read_header_from_bytes(&bytes);
    }
    // CRCs catch nearly every flipped bit in the data.
    assert!(errors > 1500, "only {errors} damaged files detected");
}

#[test]
fn zip_bombs_are_refused() {
    // 17 MiB of spaces compress to a few kB.
    let mut json = b"{\"format\": {\"version\": 1}".to_vec();
    json.resize(17 << 20, b' ');
    json.push(b'}');
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    zip.start_file(
        NOTE_JSON,
        SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
    )
    .unwrap();
    zip.write_all(&json).unwrap();
    let bytes = zip.finish().unwrap().into_inner();
    assert!(bytes.len() < 1 << 20);
    assert!(matches!(
        read_from_bytes(&bytes),
        Err(FormatError::MemberTooLarge { ref name, limit }) if name == NOTE_JSON && limit == MAX_NOTE_JSON_BYTES
    ));
}

#[test]
fn many_compressible_members_are_refused_beyond_the_total_budget() {
    let json = note_json(&NoteFile::new(blank_note("a", "b")), FormatVersion::CURRENT).unwrap();
    let strokes = strokes::encode(&[]).unwrap();
    let zeros = vec![0_u8; 64 << 10];
    let names: Vec<String> = (0..5).map(|i| format!("bomb{i}.bin")).collect();
    let mut members: Vec<(&str, &[u8])> = vec![(NOTE_JSON, &json), (STROKES_BIN, &strokes)];
    for name in &names {
        members.push((name, &zeros));
    }
    let bytes = raw_archive(&members);
    assert!(bytes.len() < zeros.len() / 4, "{} bytes", bytes.len());

    // A small budget stands in for MAX_TOTAL_BYTES, so that the test stays fast: three members
    // fit, the fourth crosses the budget, whatever the per-member limit allows.
    let read = (json.len() + strokes.len()) as u64;
    let budget = read + 3 * zeros.len() as u64 + 100;
    let error = read_archive_within(Cursor::new(&bytes), budget).unwrap_err();
    assert!(
        matches!(error, FormatError::MemberTooLarge { ref name, limit: 100 } if name == "bomb3.bin"),
        "{error:?}"
    );
    assert_eq!(error.code(), "noteDamaged");
    let exact = read + 5 * zeros.len() as u64;
    assert_eq!(
        read_archive_within(Cursor::new(&bytes), exact)
            .unwrap()
            .unknown_members
            .len(),
        5
    );
    // The real budget: 1.5 GiB.
    assert_eq!(MAX_TOTAL_BYTES, 1536 << 20);
    assert_eq!(read_from_bytes(&bytes).unwrap().unknown_members.len(), 5);
}

#[test]
fn member_names_are_checked_on_write() {
    let mut file = NoteFile::new(blank_note("a", "b"));
    file.unknown_members.push(Member {
        name: "note.json".to_owned(),
        bytes: b"{}".to_vec(),
    });
    assert!(matches!(to_bytes(&file), Err(FormatError::InvalidNote(_))));

    let mut file = NoteFile::new(blank_note("a", "b"));
    for _ in 0..2 {
        file.unknown_members.push(Member {
            name: "x.bin".to_owned(),
            bytes: Vec::new(),
        });
    }
    assert!(matches!(to_bytes(&file), Err(FormatError::InvalidNote(_))));

    let mut file = NoteFile::new(blank_note("a", "b"));
    file.background = Some(BackgroundFile {
        extension: "../PNG".to_owned(),
        bytes: Vec::new(),
    });
    assert!(matches!(to_bytes(&file), Err(FormatError::InvalidNote(_))));
}

#[test]
fn duplicate_or_odd_members_in_a_file_are_kept_safely() {
    let json = note_json(&NoteFile::new(blank_note("a", "b")), FormatVersion::CURRENT).unwrap();
    let strokes = strokes::encode(&[]).unwrap();
    let bytes = raw_archive(&[
        (NOTE_JSON, &json),
        (STROKES_BIN, &strokes),
        ("background.png", b"first"),
        ("background.pdf", b"second"),
        ("background.Bad Ext", b"third"),
    ]);
    let file = read_from_bytes(&bytes).unwrap();
    assert_eq!(
        file.background,
        Some(BackgroundFile {
            extension: "png".to_owned(),
            bytes: b"first".to_vec()
        })
    );
    let names: Vec<&str> = file
        .unknown_members
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names, ["background.pdf", "background.Bad Ext"]);
    assert_eq!(read_from_bytes(&to_bytes(&file).unwrap()).unwrap(), file);
}

/// A valid note archive in which `name` appears twice, holding `first` then `second` (the zip
/// writer refuses to make one: it is written with a placeholder name of the same length,
/// then patched in the local headers and the central directory).
fn archive_with_duplicate(name: &str, first: &[u8], second: &[u8]) -> Vec<u8> {
    let json = note_json(&NoteFile::new(blank_note("a", "b")), FormatVersion::CURRENT).unwrap();
    let strokes = strokes::encode(&[]).unwrap();
    let placeholder = format!("§{}", &name[2..]);
    assert_eq!(placeholder.len(), name.len());
    let mut members: Vec<(&str, &[u8])> = vec![(name, first), (&placeholder, second)];
    for (known, bytes) in [(NOTE_JSON, &json), (STROKES_BIN, &strokes)] {
        if known != name {
            members.insert(0, (known, bytes));
        }
    }
    let mut bytes = raw_archive(&members);
    let (from, to) = (placeholder.as_bytes(), name.as_bytes());
    let mut patched = 0;
    let mut i = 0;
    while i + from.len() <= bytes.len() {
        if &bytes[i..i + from.len()] == from {
            bytes[i..i + from.len()].copy_from_slice(to);
            patched += 1;
        }
        i += 1;
    }
    assert_eq!(patched, 2, "local header and central directory");
    bytes
}

#[test]
fn duplicate_member_names_are_refused() {
    let json = note_json(&NoteFile::new(blank_note("a", "b")), FormatVersion::CURRENT).unwrap();
    let mut other = NoteFile::new(blank_note("other", "Other"));
    other.note.tags = vec!["x".to_owned()];
    let other_json = note_json(&other, FormatVersion::CURRENT).unwrap();
    let strokes = strokes::encode(&[]).unwrap();
    let cases = [
        archive_with_duplicate("x.bin", b"first", b"second"),
        archive_with_duplicate("background.png", b"first", b"second"),
        archive_with_duplicate(THUMB_PNG, b"first", b"second"),
        archive_with_duplicate(NOTE_JSON, &json, &other_json),
        archive_with_duplicate(STROKES_BIN, &strokes, &strokes),
    ];
    for bytes in cases {
        let error = read_from_bytes(&bytes).unwrap_err();
        assert!(matches!(error, FormatError::Archive(_)), "{error:?}");
        assert_eq!(error.code(), "noteDamaged");
        assert!(error.to_string().contains("twice"), "{error}");
        // Listing refuses it too, so that a note shown in the list can always be opened.
        assert!(read_header_from_bytes(&bytes).is_err());
    }
}

#[test]
fn a_new_background_replaces_an_unknown_member_of_the_same_name() {
    let json = note_json(&NoteFile::new(blank_note("a", "b")), FormatVersion::CURRENT).unwrap();
    let strokes = strokes::encode(&[]).unwrap();
    let bytes = raw_archive(&[
        (NOTE_JSON, &json),
        (STROKES_BIN, &strokes),
        ("background.png", b"png"),
        ("background.pdf", b"old pdf"),
        ("background.jpg", b"old jpg"),
    ]);
    let mut file = read_from_bytes(&bytes).unwrap();
    assert_eq!(file.unknown_members.len(), 2);

    // The app sets a PDF background: the known background wins over the unknown member.
    file.background = Some(BackgroundFile {
        extension: "pdf".to_owned(),
        bytes: b"new pdf".to_vec(),
    });
    let written = to_bytes(&file).unwrap();
    assert_eq!(
        member_names(&written),
        [NOTE_JSON, STROKES_BIN, "background.pdf", "background.jpg"]
    );
    let back = read_from_bytes(&written).unwrap();
    assert_eq!(back.background, file.background);
    assert_eq!(
        back.unknown_members,
        [Member {
            name: "background.jpg".to_owned(),
            bytes: b"old jpg".to_vec()
        }]
    );
}

#[test]
fn compressed_members_are_stored_and_others_deflated() {
    let mut file = NoteFile::new(blank_note("a", "b"));
    file.thumbnail = Some(vec![0x89; 100]);
    file.audio = Some(vec![1; 100]);
    file.original = Some(vec![2; 100]);
    file.background = Some(BackgroundFile {
        extension: "pdf".to_owned(),
        bytes: vec![3; 100],
    });
    let bytes = to_bytes(&file).unwrap();
    let mut archive = ZipArchive::new(Cursor::new(&bytes)).unwrap();
    let method = |archive: &mut ZipArchive<Cursor<&Vec<u8>>>, name: &str| {
        archive.by_name(name).unwrap().compression()
    };
    assert_eq!(method(&mut archive, NOTE_JSON), CompressionMethod::Deflated);
    assert_eq!(
        method(&mut archive, STROKES_BIN),
        CompressionMethod::Deflated
    );
    assert_eq!(
        method(&mut archive, ORIGINAL_BIN),
        CompressionMethod::Deflated
    );
    assert_eq!(
        method(&mut archive, "background.pdf"),
        CompressionMethod::Deflated
    );
    assert_eq!(method(&mut archive, THUMB_PNG), CompressionMethod::Stored);
    assert_eq!(method(&mut archive, AUDIO_OPUS), CompressionMethod::Stored);
    assert_eq!(
        member_names(&bytes),
        [
            NOTE_JSON,
            STROKES_BIN,
            ORIGINAL_BIN,
            THUMB_PNG,
            AUDIO_OPUS,
            "background.pdf"
        ]
    );
}

#[test]
fn output_is_deterministic() {
    let mut rng = Lcg::new(5);
    let file = random_file(&mut rng);
    assert_eq!(to_bytes(&file).unwrap(), to_bytes(&file).unwrap());
}

#[test]
fn zip_time_follows_the_note() {
    let time = zip_time(1_792_843_200_000);
    assert_eq!(
        (time.year(), time.month(), time.day(), time.hour()),
        (2026, 10, 24, 12)
    );
    // Out of the DOS range: 1980-01-01.
    assert_eq!(zip_time(0), zip::DateTime::default());
    assert_eq!(zip_time(i64::MIN), zip::DateTime::default());
}

#[test]
fn files_on_disk() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("2026-10-24 Note.ardoise");
    let mut file = NoteFile::new(blank_note("disk", "Note"));
    file.thumbnail = Some(b"png bytes".to_vec());
    write_note(&path, &file).unwrap();
    assert_eq!(read_note(&path).unwrap(), file);
    assert_eq!(read_header(&path).unwrap().note.title, "Note");
    assert_eq!(
        read_thumbnail(&path).unwrap().as_deref(),
        Some(&b"png bytes"[..])
    );

    file.thumbnail = None;
    write_note(&path, &file).unwrap();
    assert_eq!(read_thumbnail(&path).unwrap(), None);
    // Only the note: no temporary file left behind.
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);

    let missing = dir.path().join("missing.ardoise");
    let error = read_note(&missing).unwrap_err();
    assert!(matches!(error, FormatError::Io { ref path, .. } if path == &missing));
    assert_eq!(error.code(), "noteAccess");

    // An invalid note is not written at all.
    let mut bad = file.clone();
    bad.note.layers.clear();
    assert!(write_note(&path, &bad).is_err());
    assert_eq!(read_note(&path).unwrap(), file);
}

// ---------------------------------------------------------------------------------------
// Fixtures: files written by format version 1, committed so that every later version is
// tested against them. Regenerate (only when adding a fixture) with
// `cargo test -p ardoise-core -- --ignored generate_v1_fixtures`.

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/format/v1")
}

/// Bytes of a 1×1 transparent PNG (opaque to the format).
const TINY_PNG: [u8; 67] = [
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89, 0x00, 0x00, 0x00, 0x0A, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9C, 0x63, 0x00, 0x01, 0x00, 0x00,
    0x05, 0x00, 0x01, 0x0D, 0x0A, 0x2D, 0xB4, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4E, 0x44, 0xAE,
    0x42, 0x60, 0x82,
];

fn fixture_minimal() -> NoteFile {
    NoteFile::new(blank_note(
        "00000000-0000-4000-8000-000000000001",
        "Minimal",
    ))
}

fn fixture_full() -> NoteFile {
    let mut note = blank_note(
        "00000000-0000-4000-8000-000000000002",
        "Théorème « complet »",
    );
    note.modified = note.created + 3_600_000;
    note.page = A4_PORTRAIT;
    note.background = Background {
        paper: Paper::Custom(Rgba {
            r: 10,
            g: 20,
            b: 30,
            a: 255,
        }),
        template: Template::Seyes,
        texture: true,
    };
    note.tags = vec!["maths".to_owned(), "3e B".to_owned()];
    note.layers.push(Layer {
        name: "Corrections".to_owned(),
        visible: false,
    });
    note.timing = Timing::Synthesized;
    note.origin = Origin::Inkspace {
        id: "a1b2c3d4-0000-4000-8000-00000000abcd".to_owned(),
    };
    note.strokes = Tool::ALL
        .iter()
        .enumerate()
        .map(|(i, &tool)| Stroke {
            tool,
            color: Rgba::from_rgba_u32(0x1020_30ff + i as u32),
            width: 25.0 + i as f32 * 10.5,
            layer: (i % 2) as u32,
            points: (0..5u32)
                .map(|k| Point {
                    x: 1000.0 + i as f32 * 2000.0 + k as f32 * 12.25,
                    y: 3000.5 - k as f32 * 7.75,
                    pressure: (k * 16_383) as f32 / 65_535.0,
                    t: i as u32 * 1000 + k * 5,
                })
                .collect(),
        })
        .collect();
    let mut file = NoteFile::new(stored(&note));
    file.original = Some((0u8..=255).collect());
    file.thumbnail = Some(TINY_PNG.to_vec());
    file.audio = Some(b"OggS fake opus".to_vec());
    file.background = Some(BackgroundFile {
        extension: "pdf".to_owned(),
        bytes: b"%PDF-1.4 fake".to_vec(),
    });
    file.text = Some(br#"{"lines":["a^2 + b^2 = c^2"]}"#.to_vec());
    file
}

/// A file as a later 1.x version might write it: unknown fields and members.
fn fixture_future_minor() -> NoteFile {
    let mut file = NoteFile::new(blank_note(
        "00000000-0000-4000-8000-000000000003",
        "Version mineure future",
    ));
    file.unknown_fields
        .insert("stickers".to_owned(), json!([{"kind": "star", "x": 10}]));
    file.unknown_members.push(Member {
        name: "future/".to_owned(),
        bytes: Vec::new(),
    });
    file.unknown_members.push(Member {
        name: "future/data.bin".to_owned(),
        bytes: vec![0xde, 0xad, 0xbe, 0xef],
    });
    file
}

fn fixtures() -> [(&'static str, NoteFile, FormatVersion); 3] {
    [
        (
            "minimal.ardoise",
            fixture_minimal(),
            FormatVersion { major: 1, minor: 0 },
        ),
        (
            "full.ardoise",
            fixture_full(),
            FormatVersion { major: 1, minor: 0 },
        ),
        (
            "future-minor.ardoise",
            fixture_future_minor(),
            FormatVersion { major: 1, minor: 3 },
        ),
    ]
}

#[test]
#[ignore = "writes the committed fixtures; run only when adding one"]
fn generate_v1_fixtures() {
    let dir = fixtures_dir();
    fs::create_dir_all(&dir).unwrap();
    for (name, file, version) in fixtures() {
        let bytes = to_bytes_with_version(&file, version).unwrap();
        fs::write(dir.join(name), bytes).unwrap();
    }
}

#[test]
fn v1_fixtures_are_read() {
    let dir = fixtures_dir();
    for (name, expected, version) in fixtures() {
        let path = dir.join(name);
        let file = read_note(&path).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(file, expected, "{name}");
        let header = read_header(&path).unwrap();
        assert_eq!(header.format, version, "{name}");
        assert!(
            header
                .writer
                .as_deref()
                .is_some_and(|w| w.starts_with("Ardoise "))
        );
    }

    // Spot checks written out by hand, independent of the builders above.
    let full = read_note(&dir.join("full.ardoise")).unwrap();
    assert_eq!(full.note.title, "Théorème « complet »");
    assert_eq!(full.note.page, A4_PORTRAIT);
    assert_eq!(full.note.tags, ["maths", "3e B"]);
    assert_eq!(full.note.layers.len(), 2);
    assert!(!full.note.layers[1].visible);
    assert_eq!(full.note.strokes.len(), 7);
    let chalk = &full.note.strokes[6];
    assert_eq!(chalk.tool, Tool::Chalk);
    assert_eq!(chalk.width, 88.0);
    assert_eq!(chalk.layer, 0);
    assert_eq!(chalk.color, Rgba::from_rgba_u32(0x1020_3105));
    let last = chalk.points[4];
    assert_eq!((last.x, last.y, last.t), (13_049.0, 2_969.5, 6020));
    assert!((last.pressure - 65_532.0 / 65_535.0).abs() < 1e-6);
    assert_eq!(full.original.as_ref().map(Vec::len), Some(256));
    assert_eq!(full.thumbnail.as_deref(), Some(&TINY_PNG[..]));
    assert_eq!(
        full.background.as_ref().map(|b| b.extension.as_str()),
        Some("pdf")
    );

    let future = read_note(&dir.join("future-minor.ardoise")).unwrap();
    assert_eq!(future.unknown_fields["stickers"][0]["kind"], "star");
    assert_eq!(future.unknown_members[1].bytes, [0xde, 0xad, 0xbe, 0xef]);
    let rewritten = read_from_bytes(&to_bytes(&future).unwrap()).unwrap();
    assert_eq!(rewritten, future);
}
