//! Text frames (page objects) through the MCP tools: **no tool that rewrites a note ever loses its
//! objects**, a split puts each in exactly one part, the read tools show them and the search finds
//! them. Through rmcp's in-memory transport on a temporary library of synthetic notes.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use ardoise_core::format::NoteFile;
use ardoise_core::note::{Note, PageObject};
use ardoise_ink::Rgba;
use serde_json::{Value, json};
use support::*;

/// A note with strokes (three waves in the first 800 ms, a circle between 2000 and 2600 ms) and
/// three text frames: a title and a definition (the second appears at 2500 ms, after the cut the
/// tests make at 1500 ms), and an answer on a hidden layer.
const FICHE: &str = "66666666-6666-4666-8666-666666666666";

/// A text frame as `note.json` writes it (`font` is `"latinModern"` or `"andika"`), measured.
#[allow(clippy::too_many_arguments)]
fn frame(
    id: &str,
    layer: u32,
    t: u32,
    (x, y): (f32, f32),
    font: &str,
    bold: bool,
    size: f32,
    text: &str,
) -> PageObject {
    let mut object = PageObject::from_value(json!({
        "kind": "text", "id": id, "layer": layer, "t": t, "x": x, "y": y,
        "text": text, "font": font, "bold": bold, "size": size,
        "color": serde_json::to_value(Rgba::rgb(10, 20, 200)).unwrap(),
    }))
    .unwrap();
    object.measure();
    object
}

fn fiche() -> Note {
    let mut note = make_note(FICHE, "Fiche de cours", 4);
    note.strokes.push(circle(15_000.0, 12_000.0, 2_000));
    note.objects = vec![
        frame(
            "titre",
            0,
            0,
            (1_000.0, 1_000.0),
            "latinModern",
            true,
            900.0,
            "Les métaux",
        ),
        frame(
            "definition",
            0,
            2_500,
            (15_000.0, 14_000.0),
            "andika",
            false,
            400.0,
            "Cuivre : conducteur\nFer : magnétique",
        ),
        frame(
            "reponse",
            1,
            0,
            (1_000.0, 8_000.0),
            "andika",
            false,
            600.0,
            "Réponse cachée",
        ),
    ];
    note
}

fn add_fiche(session: &Session) {
    session
        .harness
        .library()
        .create(Path::new("Physique"), &NoteFile::new(fiche()))
        .unwrap();
    // What the recogniser reads: handwriting only, never the typed text.
    session.harness.fake.by_id.lock().unwrap().insert(
        FICHE.into(),
        vec!["Les métaux conduisent le courant".into()],
    );
}

fn note_of(session: &Session, id: &str) -> Note {
    let path = session.harness.path_of(id);
    session.harness.library().read(&path).unwrap().file.note
}

fn ids(note: &Note) -> Vec<&str> {
    note.objects.iter().map(|o| o.id.as_str()).collect()
}

fn fingerprint(session: &Session, id: &str) -> Vec<u8> {
    fs::read(session.harness.path_of(id)).unwrap()
}

fn recolor(id: usize) -> Value {
    json!({ "op": "recolor", "select": { "ids": [id] }, "color": "#ff0000" })
}

// ---------------------------------------------------------------------------------------------
// Rewriting tools keep the objects

#[tokio::test(flavor = "current_thread")]
async fn a_revision_keeps_the_text_frames_as_they_are() {
    let session = session().await;
    add_fiche(&session);
    let source = note_of(&session, FICHE);
    let before = fingerprint(&session, FICHE);

    // `create_revision`: a copy that has the frames, untouched, by whatever orders.
    let made = session
        .ok(
            "create_revision",
            json!({
                "note": FICHE,
                "orders": [recolor(0), { "op": "recolor", "select": { "ids": [1, 2] }, "color": "#00aa00" }],
                "summary": "Couleurs",
            }),
        )
        .await;
    let copy_id = made["note"]["id"].as_str().unwrap().to_owned();
    let copy = note_of(&session, &copy_id);
    assert_eq!(copy.objects, source.objects);
    assert_ne!(copy.strokes, source.strokes);
    assert_eq!(fingerprint(&session, FICHE), before);

    // `edit_revision` on the copy: still the frames.
    session
        .ok(
            "edit_revision",
            json!({ "note": copy_id, "orders": [recolor(3)], "summary": "Cercle" }),
        )
        .await;
    assert_eq!(note_of(&session, &copy_id).objects, source.objects);

    // `preview_revision` writes nothing.
    let preview = session
        .call(
            "preview_revision",
            json!({ "note": FICHE, "orders": [recolor(0)] }),
        )
        .await;
    assert_ne!(preview.is_error, Some(true), "{preview:?}");
    assert_eq!(fingerprint(&session, FICHE), before);
}

#[tokio::test(flavor = "current_thread")]
async fn straightening_keeps_the_text_frames_on_the_copy_and_in_place() {
    let session = session().await;
    add_fiche(&session);
    let source = note_of(&session, FICHE);

    // On the note: a copy, with the frames.
    let result = session
        .ok(
            "straighten_shapes",
            json!({ "note": FICHE, "strokes": [3] }),
        )
        .await;
    assert_eq!(result["made_a_copy"], true, "{result}");
    let copy_id = result["note"]["id"].as_str().unwrap().to_owned();
    assert_eq!(note_of(&session, &copy_id).objects, source.objects);

    // On that revision: in place, the frames still there.
    let again = session
        .ok(
            "straighten_shapes",
            json!({ "note": copy_id, "strokes": [3], "tolerance": 5 }),
        )
        .await;
    if again["saved"] == true {
        assert_eq!(again["made_a_copy"], false);
    }
    assert_eq!(note_of(&session, &copy_id).objects, source.objects);
}

#[tokio::test(flavor = "current_thread")]
async fn each_text_frame_ends_up_in_one_part_of_a_split_at_an_instant() {
    let session = session().await;
    add_fiche(&session);
    let source = note_of(&session, FICHE);
    let before = fingerprint(&session, FICHE);

    let split = session
        .ok("split_note", json!({ "note": FICHE, "at_ms": 1500 }))
        .await;
    assert_eq!(fingerprint(&session, FICHE), before);
    let kept = note_of(&session, split["kept"]["id"].as_str().unwrap());
    let moved = note_of(&session, split["created"]["id"].as_str().unwrap());
    // Written before the cut: the title and the hidden answer; after it: the definition.
    assert_eq!(ids(&kept), ["titre", "reponse"]);
    assert_eq!(ids(&moved), ["definition"]);
    assert_eq!(kept.objects[0], source.objects[0]);
    // On the second part's own clock, which starts at its first sample (the circle, at 2000 ms).
    assert_eq!(moved.objects[0].t, 500);
    all_objects_once(&source, &[&kept, &moved]);
}

#[tokio::test(flavor = "current_thread")]
async fn each_text_frame_ends_up_in_one_part_of_a_split_by_a_zone() {
    let session = session().await;
    add_fiche(&session);
    let source = note_of(&session, FICHE);

    // The lower right of the page: the centre of the definition is in, the others' are not.
    let split = session
        .ok(
            "split_note",
            json!({ "note": FICHE, "polygon": [[120, 120], [290, 120], [290, 200], [120, 200]] }),
        )
        .await;
    let kept = note_of(&session, split["kept"]["id"].as_str().unwrap());
    let moved = note_of(&session, split["created"]["id"].as_str().unwrap());
    assert_eq!(ids(&moved), ["definition"]);
    // A zone only moves what is visible: the hidden layer's frame stays with the note.
    assert_eq!(ids(&kept), ["titre", "reponse"]);
    all_objects_once(&source, &[&kept, &moved]);
}

/// Every object of `source` is in exactly one of `parts`.
fn all_objects_once(source: &Note, parts: &[&Note]) {
    let mut seen = HashSet::new();
    for part in parts {
        for object in &part.objects {
            assert!(seen.insert(object.id.clone()), "{} twice", object.id);
        }
    }
    let expected: HashSet<String> = source.objects.iter().map(|o| o.id.clone()).collect();
    assert_eq!(seen, expected);
}

#[tokio::test(flavor = "current_thread")]
async fn splitting_a_revision_saves_what_stays_with_its_own_frames() {
    let session = session().await;
    add_fiche(&session);
    let made = session
        .ok(
            "create_revision",
            json!({ "note": FICHE, "orders": [recolor(0)], "summary": "Couleur" }),
        )
        .await;
    let revision = made["note"]["id"].as_str().unwrap().to_owned();
    let split = session
        .ok("split_note", json!({ "note": revision, "at_ms": 1500 }))
        .await;
    assert_eq!(split["made_copies"], false);
    // What stays is saved over the revision: its title frame and the hidden answer; the
    // definition went to the new note, and is not on the revision any more.
    let kept = note_of(&session, &revision);
    let moved = note_of(&session, split["created"]["id"].as_str().unwrap());
    assert_eq!(ids(&kept), ["titre", "reponse"]);
    assert_eq!(ids(&moved), ["definition"]);
    // The source of it all still has its three.
    assert_eq!(note_of(&session, FICHE).objects.len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn merging_and_updating_keep_the_text_frames() {
    let session = session().await;
    add_fiche(&session);
    let source = note_of(&session, FICHE);

    let merged = session
        .ok(
            "merge_notes",
            json!({ "notes": [FICHE, OPTIQUE], "layout": "endToEnd" }),
        )
        .await;
    let merged = note_of(&session, merged["note"]["id"].as_str().unwrap());
    assert_eq!(merged.objects.len(), 3);
    let texts: Vec<_> = merged
        .objects
        .iter()
        .filter_map(|o| o.as_text().map(|t| t.text.clone()))
        .collect();
    assert!(texts.contains(&"Les métaux".to_owned()));

    session
        .ok(
            "update_note",
            json!({ "note": FICHE, "add_tags": ["chimie"], "favorite": true }),
        )
        .await;
    assert_eq!(note_of(&session, FICHE).objects, source.objects);
    session
        .ok("move_note", json!({ "note": FICHE, "binder": "Archives" }))
        .await;
    assert_eq!(note_of(&session, FICHE).objects, source.objects);
}

// ---------------------------------------------------------------------------------------------
// Reading

#[tokio::test(flavor = "current_thread")]
async fn get_note_lists_the_text_frames_in_millimetres() {
    let session = session().await;
    add_fiche(&session);
    let note = session.ok("get_note", json!({ "note": FICHE })).await;
    let frames = note["text_frames"].as_array().unwrap();
    // Reading order: top to bottom.
    let order: Vec<_> = frames.iter().map(|f| f["id"].as_str().unwrap()).collect();
    assert_eq!(order, ["titre", "reponse", "definition"]);
    let title = &frames[0];
    assert_eq!(title["text"], "Les métaux");
    assert_eq!(title["font"], "latinModern");
    assert_eq!(title["bold"], true);
    assert_eq!(title["size_mm"], 9.0);
    assert_eq!(title["color"], "#0a14c8");
    assert_eq!(title["layer"], 0);
    assert_eq!(title["visible"], true);
    assert_eq!(title["bounds_mm"]["x"], 10.0);
    assert_eq!(title["bounds_mm"]["y"], 10.0);
    assert!(title["bounds_mm"]["width"].as_f64().unwrap() > 20.0);
    assert!(title["bounds_mm"]["height"].as_f64().unwrap() > 5.0);
    assert_eq!(frames[1]["visible"], false);
    assert_eq!(frames[2]["font"], "andika");
    assert_eq!(frames[2]["size_mm"], 4.0);
    assert_eq!(frames[2]["text"], "Cuivre : conducteur\nFer : magnétique");
    assert_eq!(frames[2]["bounds_mm"]["x"], 150.0);
    // No object of a kind this version does not know here.
    assert!(note.get("other_objects").is_none());

    // A note without frames still has the key, empty.
    let plain = session.ok("get_note", json!({ "note": OPTIQUE })).await;
    assert_eq!(plain["text_frames"], json!([]));
}

#[tokio::test(flavor = "current_thread")]
async fn get_note_says_when_the_page_holds_objects_of_an_unknown_kind() {
    let session = session().await;
    let mut note = fiche();
    note.objects.push(
        PageObject::from_value(json!({
            "kind": "sticker", "id": "s1", "layer": 0, "x": 100, "y": 100,
            "width": 500, "height": 500, "emoji": "x"
        }))
        .unwrap(),
    );
    session
        .harness
        .library()
        .create(Path::new(""), &NoteFile::new(note))
        .unwrap();
    let result = session.ok("get_note", json!({ "note": FICHE })).await;
    assert_eq!(result["other_objects"]["count"], 1);
    assert_eq!(result["other_objects"]["kinds"], json!(["sticker"]));
    assert_eq!(result["text_frames"].as_array().unwrap().len(), 3);
}

#[tokio::test(flavor = "current_thread")]
async fn get_note_text_gives_the_typed_text_apart_from_the_recognised_lines() {
    let session = session().await;
    add_fiche(&session);
    let text = session.ok("get_note_text", json!({ "note": FICHE })).await;
    // The handwriting is as recognised; the typed text is not mixed into it.
    assert_eq!(text["text"], "Les métaux conduisent le courant");
    assert_eq!(text["is_correction"], false);
    // Visible layers only, reading order, line breaks inside a frame kept.
    assert_eq!(
        text["typed_text"],
        "Les métaux\nCuivre : conducteur\nFer : magnétique"
    );
    let frames = text["typed_frames"].as_array().unwrap();
    assert_eq!(frames.len(), 3);
    assert!(frames.iter().all(|f| f["source"] == "typed"));
    assert_eq!(frames[0]["id"], "titre");
    assert_eq!(frames[0]["bounds_mm"]["x"], 10.0);

    // A correction of the handwriting leaves the typed text alone.
    session
        .ok(
            "set_note_text",
            json!({ "note": FICHE, "text": "Les métaux conduisent l'électricité" }),
        )
        .await;
    let again = session.ok("get_note_text", json!({ "note": FICHE })).await;
    assert_eq!(again["is_correction"], true);
    assert_eq!(again["typed_text"], text["typed_text"]);

    let plain = session
        .ok("get_note_text", json!({ "note": OPTIQUE }))
        .await;
    assert_eq!(plain["typed_text"], "");
    assert_eq!(plain["typed_frames"], json!([]));
}

#[tokio::test(flavor = "current_thread")]
async fn the_search_finds_a_note_by_the_text_of_its_frames() {
    let session = session().await;
    add_fiche(&session);
    // Accents and case ignored, a word of a multi-line frame, with the snippet.
    let found = session
        .ok("search_notes", json!({ "query": "MAGNETIQUE" }))
        .await;
    assert_eq!(found["total"], 1, "{found}");
    let hit = &found["notes"][0];
    assert_eq!(hit["id"], FICHE);
    assert_eq!(hit["matched_in"], json!(["typed_text"]));
    assert_eq!(hit["snippet"]["word"], "magnétique");
    assert_eq!(hit["snippet"]["before"], "métaux Cuivre : conducteur Fer :");
    assert_eq!(hit["snippet"]["after"], "");

    // Typed text and handwriting together, each word in its own place.
    let both = session
        .ok("search_notes", json!({ "query": "cuivre courant" }))
        .await;
    assert_eq!(both["total"], 1, "{both}");
    let places = both["notes"][0]["matched_in"].as_array().unwrap();
    assert!(places.contains(&json!("typed_text")) && places.contains(&json!("text")));

    // A frame on a hidden layer is not on the page: the search, like the handwriting's, skips it.
    let hidden = session
        .ok("search_notes", json!({ "query": "cachée" }))
        .await;
    assert_eq!(hidden["total"], 0, "{hidden}");
    // Nothing found where there is nothing.
    let none = session
        .ok("search_notes", json!({ "query": "ardoise" }))
        .await;
    assert_eq!(none["total"], 0);
}
