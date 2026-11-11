//! The tools that retouch a note on a copy (`preview_revision`, `create_revision`,
//! `edit_revision`, and the read tools that go with them), through rmcp's in-memory transport on a
//! temporary library of synthetic notes. The guarantee they are about: **no tool ever writes the
//! file of a note that is not a revision**, byte for byte.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::fs;

use ardoise_core::format::{BackgroundFile, NoteFile};
use ardoise_core::library::FileStamp;
use ardoise_core::note::{BackgroundImage, ImageRect, Paper, Template};
use ardoise_hwr::Language;
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_mcp::ServerOptions;
use base64::Engine as _;
use rmcp::model::CallToolResult;
use serde_json::{Value, json};
use support::*;

/// A note with everything a copy must keep.
const RICH: &str = "55555555-5555-4555-8555-555555555555";

fn recolor(id: usize) -> Value {
    json!({ "op": "recolor", "select": { "ids": [id] }, "color": "#ff0000" })
}

/// The bytes and the stamp of the file of the note `id`.
fn fingerprint(session: &Session, id: &str) -> (Vec<u8>, FileStamp) {
    let path = session.harness.path_of(id);
    let stamp = session.harness.library().read(&path).unwrap().stamp;
    (fs::read(path).unwrap(), stamp)
}

fn file_of(session: &Session, id: &str) -> NoteFile {
    let path = session.harness.path_of(id);
    session.harness.library().read(&path).unwrap().file
}

fn png() -> Vec<u8> {
    let image = ardoise_render::Image {
        width: 4,
        height: 4,
        pixels: vec![200; 4 * 4 * 4],
    };
    ardoise_render::encode_png(&image).unwrap()
}

/// Adds the note `RICH` to the library: ruled cream paper, two layers, a background image, tags,
/// a narration, a text member, a thumbnail, a favourite.
fn add_rich_note(session: &Session) {
    let mut note = make_note(RICH, "Rapport d'incident", 3);
    // Three waves and a circle drawn after them.
    note.strokes.push(circle(15_000.0, 12_000.0, 2_000));
    note.tags = vec!["incident".into(), "vie scolaire".into()];
    note.favorite = true;
    note.background.paper = Paper::Cream;
    note.background.template = Template::Lined;
    note.background_image = Some(BackgroundImage {
        name: "fiche.png".into(),
        pdf_page: None,
        rect: ImageRect {
            x: 0.0,
            y: 0.0,
            width: 29_700.0,
            height: 21_600.0,
        },
        quarter_turns: 0,
        opacity: 1.0,
    });
    let mut file = NoteFile::new(note);
    file.thumbnail = Some(b"thumbnail of the source".to_vec());
    file.audio = Some(vec![1, 2, 3, 4, 5]);
    file.text = Some(br#"{"reserved":true}"#.to_vec());
    file.background = Some(BackgroundFile {
        extension: "png".into(),
        bytes: png(),
    });
    session
        .harness
        .library()
        .create(std::path::Path::new("Physique"), &file)
        .unwrap();
}

fn text_of(result: &CallToolResult) -> Value {
    let text = result
        .content
        .iter()
        .find_map(|content| content.as_text())
        .expect("a text content");
    serde_json::from_str(&text.text).unwrap()
}

fn decoded(result: &CallToolResult) -> ardoise_render::Image {
    let image = result
        .content
        .iter()
        .find_map(|content| content.as_image())
        .expect("an image content");
    assert_eq!(image.mime_type, "image/png");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&image.data)
        .unwrap();
    ardoise_render::decode_png(&bytes).unwrap()
}

fn red_pixels(image: &ardoise_render::Image) -> usize {
    (0..image.width)
        .flat_map(|x| (0..image.height).map(move |y| (x, y)))
        .filter_map(|(x, y)| image.pixel(x, y))
        .filter(|p| p[0] > 180 && p[1] < 90 && p[2] < 90 && p[3] == 255)
        .count()
}

// ---------------------------------------------------------------------------------------------
// The guarantee: the source is never written

#[tokio::test(flavor = "current_thread")]
async fn no_tool_writes_the_file_of_the_source() {
    let session = session().await;
    add_rich_note(&session);
    session
        .harness
        .fake
        .drawings
        .lock()
        .unwrap()
        .insert(RICH.into(), vec![3]);
    let before = fingerprint(&session, RICH);
    let listed = session.harness.library().list().notes.len();

    // Preview: in memory, nothing is written.
    let preview = session
        .call(
            "preview_revision",
            json!({ "note": RICH, "orders": [recolor(0)], "labels": true }),
        )
        .await;
    assert_ne!(preview.is_error, Some(true), "{preview:?}");
    assert_eq!(fingerprint(&session, RICH), before);
    assert_eq!(session.harness.library().list().notes.len(), listed);

    // Create: a new note next to it.
    let made = session
        .ok(
            "create_revision",
            json!({ "note": RICH, "orders": [recolor(0)], "summary": "Premier trait en rouge" }),
        )
        .await;
    assert_eq!(made["created"], true);
    assert_eq!(fingerprint(&session, RICH), before);
    assert_eq!(session.harness.library().list().notes.len(), listed + 1);

    // Straighten, on the source: a second copy.
    let straightened = session
        .ok("straighten_shapes", json!({ "note": RICH, "strokes": [3] }))
        .await;
    assert_eq!(straightened["made_a_copy"], true, "{straightened}");
    assert_eq!(fingerprint(&session, RICH), before);

    // Edit: refused, and nothing happens.
    let refused = session
        .failure(
            "edit_revision",
            json!({ "note": RICH, "orders": [recolor(0)], "summary": "Essai" }),
        )
        .await;
    assert_eq!(refused["code"], "notARevision", "{refused}");
    assert_eq!(fingerprint(&session, RICH), before);

    // Split, at an instant and by a zone: both parts are new notes.
    let listed = session.harness.library().list().notes.len();
    for (index, arguments) in [
        json!({ "note": RICH, "at_ms": 1500 }),
        json!({ "note": RICH, "polygon": [[0, 40], [297, 40], [297, 60], [0, 60]] }),
    ]
    .into_iter()
    .enumerate()
    {
        let split = session.ok("split_note", arguments).await;
        assert_eq!(split["original_unchanged"], true);
        assert_eq!(fingerprint(&session, RICH), before);
        assert_eq!(
            session.harness.library().list().notes.len(),
            listed + 2 * (index + 1)
        );
    }

    // Merge: a new note. A corrected transcription: in the handwriting cache.
    session
        .ok(
            "merge_notes",
            json!({ "notes": [RICH, OPTIQUE], "layout": "overlay" }),
        )
        .await;
    assert_eq!(fingerprint(&session, RICH), before);
    session
        .ok("set_note_text", json!({ "note": RICH, "text": "corrigé" }))
        .await;
    assert_eq!(fingerprint(&session, RICH), before);

    assert!(session.harness.library().trash_items().unwrap().is_empty());
    assert_eq!(*session.harness.write.saves.lock().unwrap(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn update_note_rewrites_the_file_without_touching_its_ink() {
    let session = session().await;
    add_rich_note(&session);
    let before = file_of(&session, RICH);
    let updated = session
        .ok(
            "update_note",
            json!({ "note": RICH, "title": "Autre titre", "add_tags": ["relu"], "favorite": false }),
        )
        .await;
    assert_eq!(updated["changed"], json!(["tags", "favorite", "title"]));
    let after = file_of(&session, RICH);
    assert_eq!(after.note.strokes, before.note.strokes);
    assert_eq!(after.note.layers, before.note.layers);
    assert_eq!(after.original, before.original);
    assert_eq!(after.original_note, before.original_note);
    assert_eq!(after.audio, before.audio);
}

// ---------------------------------------------------------------------------------------------
// split_note

#[tokio::test(flavor = "current_thread")]
async fn an_ordinary_note_is_split_into_two_new_notes_that_are_revisions_of_it() {
    let session = session().await;
    add_rich_note(&session);
    let before = fingerprint(&session, RICH);
    let source = file_of(&session, RICH);
    // Three waves in the first 800 ms, a circle from 2000 ms on.
    let split = session
        .ok("split_note", json!({ "note": RICH, "at_ms": 1500 }))
        .await;
    assert_eq!(split["made_copies"], true);
    assert_eq!(split["original_unchanged"], true);
    assert_eq!(split["original"]["id"], RICH);
    assert_eq!(split["original"]["stroke_count"], 4);
    let (kept_id, created_id) = (
        split["kept"]["id"].as_str().unwrap(),
        split["created"]["id"].as_str().unwrap(),
    );
    assert!(kept_id != RICH && created_id != RICH && kept_id != created_id);
    assert_eq!(split["kept"]["stroke_count"], 3);
    assert_eq!(split["created"]["stroke_count"], 1);
    assert_eq!(split["kept"]["title"], "Rapport d'incident (start)");
    assert_eq!(split["created"]["title"], "Rapport d'incident (continued)");
    assert_eq!(split["kept"]["binder"], "Physique");
    assert_eq!(split["created"]["binder"], "Physique");
    assert_eq!(fingerprint(&session, RICH), before);
    assert_eq!(*session.harness.write.saves.lock().unwrap(), 0);

    // What stays is the note with less ink: everything else of it, its "original" the source's ink.
    let kept = file_of(&session, kept_id);
    assert_eq!(kept.note.strokes, source.note.strokes[..3]);
    assert_eq!(kept.audio, source.audio);
    assert_eq!(kept.note.tags, source.note.tags);
    assert_eq!(kept.note.created, source.note.created);
    assert_eq!(kept.note.background_image, source.note.background_image);
    assert_eq!(kept.background, source.background);
    assert!(!kept.note.favorite);
    assert!(kept.thumbnail.is_some());
    let revision = kept.revision.as_ref().unwrap();
    assert_eq!(revision.of, RICH);
    assert_eq!(revision.of_title, "Rapport d'incident");
    assert!(revision.summary[0].text.contains("split"), "{revision:?}");
    let original =
        ardoise_core::format::read_from_bytes(kept.original_note.as_ref().unwrap()).unwrap();
    assert_eq!(original.note.strokes, source.note.strokes);

    // What moves has a clock of its own: its picture goes with it, not the narration.
    let created = file_of(&session, created_id);
    assert_eq!(created.note.strokes.len(), 1);
    assert_eq!(created.note.strokes[0].points[0].t, 0);
    assert_eq!(created.note.created, source.note.created + 2_000);
    assert_eq!(created.audio, None);
    assert_eq!(created.note.background_image, source.note.background_image);
    assert_eq!(created.background, source.background);
    assert_eq!(created.note.tags, source.note.tags);
    assert!(created.thumbnail.is_some());
    let revision = created.revision.as_ref().unwrap();
    assert_eq!(revision.of, RICH);
    assert!(revision.summary[0].text.contains("split"), "{revision:?}");
    assert_ne!(
        revision.summary[0].text,
        kept.revision.as_ref().unwrap().summary[0].text
    );

    // Splitting a word off and throwing it away takes nothing from the original.
    session
        .ok("trash_note", json!({ "note": created_id }))
        .await;
    assert_eq!(fingerprint(&session, RICH), before);
}

#[tokio::test(flavor = "current_thread")]
async fn the_parts_of_a_split_are_named_in_the_apps_language() {
    for (language, by_time, by_zone) in [
        (
            Language::En,
            ("Chimie des métaux (start)", "Chimie des métaux (continued)"),
            ("Chimie des métaux (rest)", "Chimie des métaux (excerpt)"),
        ),
        (
            Language::Fr,
            ("Chimie des métaux (début)", "Chimie des métaux (suite)"),
            ("Chimie des métaux (reste)", "Chimie des métaux (extrait)"),
        ),
    ] {
        let session = session_with(language, ServerOptions::default()).await;
        let split = session
            .ok("split_note", json!({ "note": CHIMIE, "at_ms": 1500 }))
            .await;
        assert_eq!(split["kept"]["title"], by_time.0);
        assert_eq!(split["created"]["title"], by_time.1);
        let zone = session
            .ok(
                "split_note",
                json!({ "note": CHIMIE, "polygon": [[0, 40], [297, 40], [297, 60], [0, 60]] }),
            )
            .await;
        assert_eq!(zone["kept"]["title"], by_zone.0);
        assert_eq!(zone["created"]["title"], by_zone.1);
        // A title given names what moves.
        let named = session
            .ok(
                "split_note",
                json!({ "note": CHIMIE, "at_ms": 1500, "title": "Le cercle" }),
            )
            .await;
        assert_eq!(named["created"]["title"], "Le cercle");
        assert_eq!(named["kept"]["title"], by_time.0);
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_revision_is_split_in_place_and_its_version_before_is_kept() {
    let session = session().await;
    let made = session
        .ok(
            "create_revision",
            json!({ "note": CHIMIE, "orders": [recolor(0)], "summary": "Rouge" }),
        )
        .await;
    let id = made["note"]["id"].as_str().unwrap().to_owned();
    let before = file_of(&session, &id);
    let source = fingerprint(&session, CHIMIE);

    let split = session
        .ok("split_note", json!({ "note": id, "at_ms": 1500 }))
        .await;
    assert_eq!(split["made_copies"], false);
    assert!(split.get("original_unchanged").is_none());
    assert_eq!(split["kept"]["id"], id.as_str());
    assert_eq!(split["kept"]["stroke_count"], 3);
    assert_eq!(split["created"]["stroke_count"], 1);
    assert_eq!(*session.harness.write.saves.lock().unwrap(), 1);

    // What stays is saved over it, with one more line in its account.
    let after = file_of(&session, &id);
    assert_eq!(after.note.strokes, before.note.strokes[..3]);
    let account = &after.revision.as_ref().unwrap().summary;
    assert_eq!(account.len(), 2);
    assert_eq!(account[0].text, "Rouge");
    assert!(account[1].text.contains("split"));

    // Its version before is in the trash, whole, a note of its own.
    let library = session.harness.library();
    let items = library.trash_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].title.as_deref(),
        Some("Chimie des métaux (clean) (before splitting)")
    );
    assert_eq!(split["previous_version"]["in_trash"], items[0].id);
    let restored = library.restore(&items[0].id).unwrap();
    let kept = library.read(&restored).unwrap().file;
    assert_eq!(kept.note.strokes, before.note.strokes);
    assert_ne!(kept.note.id, id);

    // What moved is a revision of this revision. The original of it all was never written.
    let created = file_of(&session, split["created"]["id"].as_str().unwrap());
    assert_eq!(created.revision.unwrap().of, id);
    assert_eq!(fingerprint(&session, CHIMIE), source);
}

#[tokio::test(flavor = "current_thread")]
async fn a_revision_changed_meanwhile_is_not_split_and_nothing_stays_of_the_attempt() {
    let session = session().await;
    let made = session
        .ok(
            "create_revision",
            json!({ "note": CHIMIE, "orders": [recolor(0)], "summary": "Rouge" }),
        )
        .await;
    let id = made["note"]["id"].as_str().unwrap().to_owned();
    let listed = session.harness.library().list().notes.len();
    let root = session.harness.root.clone();
    *session.harness.write.before_save.lock().unwrap() = Some(Box::new(move |note| {
        ardoise_core::library::Library::open(&root)
            .unwrap()
            .set_favorite(note, true, None)
            .unwrap();
    }));
    let error = session
        .failure("split_note", json!({ "note": id, "at_ms": 1500 }))
        .await;
    assert_eq!(error["code"], "noteConflict", "{error}");
    let kept = file_of(&session, &id);
    assert!(kept.note.favorite, "the other version stays");
    assert_eq!(kept.note.strokes.len(), 4);
    // The part made for it is taken back; the version before was not needed.
    assert_eq!(session.harness.library().list().notes.len(), listed);
    let items = session.harness.library().trash_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].title.as_deref(),
        Some("Chimie des métaux (clean) (continued)")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn nothing_is_made_when_a_split_leaves_a_part_empty() {
    let session = session().await;
    let before = fingerprint(&session, CHIMIE);
    let listed = session.harness.library().list().notes.len();
    let error = session
        .failure("split_note", json!({ "note": CHIMIE, "at_ms": 10_000_000 }))
        .await;
    assert_eq!(error["code"], "nothingToSplit");
    assert_eq!(session.harness.library().list().notes.len(), listed);
    assert!(session.harness.write.creations.lock().unwrap().is_empty());
    assert_eq!(fingerprint(&session, CHIMIE), before);
}

#[tokio::test(flavor = "current_thread")]
async fn when_the_second_part_cannot_be_written_the_first_is_taken_back() {
    let session = session().await;
    let before = fingerprint(&session, CHIMIE);
    let listed = session.harness.library().list().notes.len();
    *session.harness.write.creations_left.lock().unwrap() = Some(1);
    let error = session
        .failure("split_note", json!({ "note": CHIMIE, "at_ms": 1500 }))
        .await;
    assert_eq!(error["code"], "libraryAccess", "{error}");
    assert_eq!(session.harness.library().list().notes.len(), listed);
    assert_eq!(fingerprint(&session, CHIMIE), before);
    let items = session.harness.library().trash_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].title.as_deref(), Some("Chimie des métaux (start)"));
}

// ---------------------------------------------------------------------------------------------
// Orders are tied to the version seen

#[tokio::test(flavor = "current_thread")]
async fn the_version_of_a_note_is_given_by_get_note_and_by_the_preview() {
    let session = session().await;
    let read = session.ok("get_note", json!({ "note": CHIMIE })).await;
    let version = read["version"].as_str().unwrap().to_owned();
    assert!(!version.is_empty());
    let preview = session
        .call(
            "preview_revision",
            json!({ "note": CHIMIE, "orders": [recolor(0)] }),
        )
        .await;
    assert_eq!(text_of(&preview)["version"], version.as_str());
    // A change of the file changes it, a look does not.
    let again = session.ok("get_note", json!({ "note": CHIMIE })).await;
    assert_eq!(again["version"], version.as_str());
    session
        .ok("update_note", json!({ "note": CHIMIE, "favorite": true }))
        .await;
    let changed = session.ok("get_note", json!({ "note": CHIMIE })).await;
    assert_ne!(changed["version"], version.as_str());
}

/// The note as a user changes it by hand in the editor: stroke 0 removed, so that every other
/// stroke has another number.
fn the_user_deletes_the_first_stroke(session: &Session, id: &str) {
    let path = session.harness.path_of(id);
    let library = session.harness.library();
    let loaded = library.read(&path).unwrap();
    let mut file = loaded.file;
    file.note.strokes.remove(0);
    library.save(&path, &file, &loaded.stamp).unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn orders_written_for_another_version_are_refused() {
    let session = session().await;
    let delete = json!([{ "op": "delete", "select": { "ids": [1] } }]);
    let made = session
        .ok(
            "create_revision",
            json!({ "note": CHIMIE, "orders": [recolor(0)], "summary": "Rouge" }),
        )
        .await;
    let id = made["note"]["id"].as_str().unwrap().to_owned();
    // The answer gives the version of the copy: the next edit needs no other look.
    let version = made["version"].as_str().unwrap().to_owned();
    assert_eq!(version, session.version(&json!(id)).await);

    // The assistant previews, and waits for the user's answer. Meanwhile the user deletes a stroke:
    // number 1 is now another stroke.
    let preview = session
        .call("preview_revision", json!({ "note": id, "orders": delete }))
        .await;
    assert_eq!(text_of(&preview)["version"], version.as_str());
    the_user_deletes_the_first_stroke(&session, &id);
    let before = fingerprint(&session, &id);

    let error = session
        .failure(
            "edit_revision",
            json!({ "note": id, "orders": delete, "summary": "x", "expected_version": version }),
        )
        .await;
    assert_eq!(error["code"], "noteConflict", "{error}");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("Look at it again"), "{message}");
    assert!(message.contains("other numbers"), "{message}");
    assert_eq!(fingerprint(&session, &id), before);
    assert!(session.harness.library().trash_items().unwrap().is_empty());
    assert_eq!(*session.harness.write.saves.lock().unwrap(), 0);

    // Looking again gives the version that is accepted, and the answer the next one.
    let again = session.version(&json!(id)).await;
    let edited = session
        .ok(
            "edit_revision",
            json!({ "note": id, "orders": delete, "summary": "x", "expected_version": again }),
        )
        .await;
    assert_eq!(edited["saved"], true);
    assert_eq!(
        edited["version"].as_str().unwrap(),
        session.version(&json!(id)).await
    );

    // The same goes for a copy of a note that changed: nothing is made.
    let listed = session.harness.library().list().notes.len();
    let stale = session.version(&json!(CHIMIE)).await;
    the_user_deletes_the_first_stroke(&session, CHIMIE);
    let error = session
        .failure(
            "create_revision",
            json!({ "note": CHIMIE, "orders": delete, "summary": "x", "expected_version": stale }),
        )
        .await;
    assert_eq!(error["code"], "noteConflict", "{error}");
    assert_eq!(session.harness.library().list().notes.len(), listed);
}

#[tokio::test(flavor = "current_thread")]
async fn orders_without_a_version_are_refused() {
    let session = session().await;
    for name in ["create_revision", "edit_revision"] {
        let result = session
            .client
            .call_tool(
                rmcp::model::CallToolRequestParams::new(name).with_arguments(
                    json!({ "note": CHIMIE, "orders": [recolor(0)], "summary": "x" })
                        .as_object()
                        .cloned()
                        .unwrap(),
                ),
            )
            .await;
        match result {
            Err(_) => {}
            Ok(result) => assert_eq!(result.is_error, Some(true), "{name}"),
        }
    }
    assert_eq!(session.harness.library().list().notes.len(), 4);
    assert!(session.harness.write.creations.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_badly_written_order_is_named() {
    let session = session().await;
    let result = session
        .client
        .call_tool(
            rmcp::model::CallToolRequestParams::new("preview_revision").with_arguments(
                json!({
                    "note": CHIMIE,
                    "orders": [recolor(0), { "op": "recolor", "select": { "ids": [1] } }],
                })
                .as_object()
                .cloned()
                .unwrap(),
            ),
        )
        .await;
    let message = match result {
        Err(error) => error.to_string(),
        Ok(result) => {
            assert_eq!(result.is_error, Some(true), "{result:?}");
            result.content[0].as_text().unwrap().text.clone()
        }
    };
    assert!(message.contains("order 1 (recolor)"), "{message}");
    assert!(message.contains("missing field `color`"), "{message}");
    // All its fields at once, not the next missing one at the next try, and where to read the rest.
    assert!(message.contains("`color` (string)"), "{message}");
    assert!(message.contains("`select` (Select: "), "{message}");
    assert!(message.contains("`revision_orders`"), "{message}");

    // The list still names the orders to the assistant.
    let tools = session.client.list_all_tools().await.unwrap();
    let preview = tools
        .iter()
        .find(|tool| tool.name == "preview_revision")
        .unwrap();
    let schema = Value::Object((*preview.input_schema).clone()).to_string();
    assert!(
        schema.contains("\"replace\"") && schema.contains("tidy_line"),
        "{schema}"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn revision_orders_tells_every_order_and_what_its_fields_mean() {
    let session = session().await;
    let answer = text_of(&session.call("revision_orders", json!({})).await);
    let order = &answer["order"];
    let ops: Vec<&str> = order["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| variant["properties"]["op"]["const"].as_str().unwrap())
        .collect();
    for op in [
        "recolor",
        "restyle",
        "delete",
        "move",
        "scale",
        "rotate",
        "smooth",
        "trim",
        "replace",
        "add",
        "copy",
        "glyph",
        "retime",
        "tidy_line",
    ] {
        assert!(ops.contains(&op), "{op} is not told: {ops:?}");
    }
    // The types it names are given with it, and so is what the fields mean: a trial on the real
    // app had to find these by trial and error.
    assert!(order["$defs"]["Select"]["properties"]["ids"].is_object());
    assert!(order["$defs"]["Shape"].is_object());
    let text = answer.to_string();
    for meaning in [
        "Where the left side of this piece's box goes",
        "Where the bottom of this piece's box goes",
        "Keeps only these points",
        "Where the left side of the copied strokes' box goes",
        "`glyph` with `from` and `to`",
        "new_strokes",
    ] {
        assert!(text.contains(meaning), "{meaning} is not told");
    }
    assert!(!text.contains("Usually on a `copy`"));
}

// ---------------------------------------------------------------------------------------------
// Hidden layers, through the tools

#[tokio::test(flavor = "current_thread")]
async fn the_tools_tell_the_engine_which_layers_are_hidden() {
    let session = session().await;
    // CHIMIE: stroke 2 is on the hidden layer "Corrections".
    let everything = json!([{ "op": "recolor", "select": {}, "color": "#ff0000" }]);
    let preview = session
        .call(
            "preview_revision",
            json!({ "note": CHIMIE, "orders": everything }),
        )
        .await;
    let report = &text_of(&preview)["report"]["orders"][0];
    assert_eq!(report["selected"], 3);
    assert!(report.get("warnings").is_none(), "{report}");

    let made = session
        .ok(
            "create_revision",
            json!({ "note": CHIMIE, "orders": everything, "summary": "Rouge" }),
        )
        .await;
    let copy = file_of(&session, made["note"]["id"].as_str().unwrap());
    assert_eq!(copy.note.strokes[0].color, Rgba::rgb(255, 0, 0));
    assert_eq!(copy.note.strokes[2].color, Rgba::rgb(0, 0, 255));

    // Named: taken, and the report says what the picture does not show.
    let named = session
        .call(
            "preview_revision",
            json!({ "note": CHIMIE, "orders": [recolor(2)] }),
        )
        .await;
    let report = &text_of(&named)["report"]["orders"][0];
    assert_eq!(report["selected"], 1);
    assert!(
        report["warnings"][0].as_str().unwrap().contains("hidden"),
        "{report}"
    );
}

// ---------------------------------------------------------------------------------------------
// What is written can be read

#[tokio::test(flavor = "current_thread")]
async fn a_title_or_a_summary_no_list_could_show_is_refused() {
    let session = session().await;
    let long_title = "x".repeat(1_001);
    let long_summary = "y".repeat(20_001);
    let listed = session.harness.library().list().notes.len();
    let before = fingerprint(&session, CHIMIE);
    for (tool, arguments) in [
        (
            "create_revision",
            json!({ "note": CHIMIE, "orders": [recolor(0)], "summary": "x", "title": long_title }),
        ),
        (
            "create_revision",
            json!({ "note": CHIMIE, "orders": [recolor(0)], "summary": long_summary }),
        ),
        (
            "split_note",
            json!({ "note": CHIMIE, "at_ms": 1500, "title": long_title }),
        ),
        (
            "merge_notes",
            json!({ "notes": [CHIMIE, OPTIQUE], "layout": "overlay", "title": long_title }),
        ),
        (
            "update_note",
            json!({ "note": CHIMIE, "title": long_title }),
        ),
        ("create_note", json!({ "title": long_title })),
    ] {
        let error = session.failure(tool, arguments).await;
        assert_eq!(error["code"], "invalidArguments", "{tool}: {error}");
        assert!(
            error["message"].as_str().unwrap().contains("at most"),
            "{tool}: {error}"
        );
    }
    assert_eq!(session.harness.library().list().notes.len(), listed);
    assert_eq!(fingerprint(&session, CHIMIE), before);

    // On an edit too, and the limits themselves are accepted.
    let made = session
        .ok(
            "create_revision",
            json!({ "note": CHIMIE, "orders": [recolor(0)], "summary": "y".repeat(20_000), "title": "x".repeat(1_000) }),
        )
        .await;
    let error = session
        .failure(
            "edit_revision",
            json!({ "note": made["note"]["id"], "orders": [recolor(1)], "summary": long_summary }),
        )
        .await;
    assert_eq!(error["code"], "invalidArguments", "{error}");
}

#[tokio::test(flavor = "current_thread")]
async fn a_revision_that_could_no_longer_be_read_is_not_written() {
    use ardoise_core::format::{MAX_REVISION_JSON_BYTES, RevisionEntry, RevisionInfo};
    // A revision whose account has nearly reached what the reader takes.
    let session = session().await;
    let mut note = make_note(RICH, "Tant de retouches", 3);
    note.strokes.truncate(1);
    let mut file = NoteFile::new(note);
    file.revision = Some(RevisionInfo {
        of: CHIMIE.to_owned(),
        of_title: "Chimie".to_owned(),
        created: DAY0,
        summary: vec![RevisionEntry {
            at: DAY0,
            text: "x".repeat(MAX_REVISION_JSON_BYTES as usize - 1_000),
        }],
    });
    session
        .harness
        .library()
        .create(std::path::Path::new(""), &file)
        .unwrap();
    let before = fingerprint(&session, RICH);
    // Its version from its stamp: `get_note` would send the whole account through the test's pipe.
    let error = session
        .failure(
            "edit_revision",
            json!({
                "note": RICH,
                "orders": [recolor(0)],
                "summary": "y".repeat(20_000),
                "expected_version": ardoise_mcp::notes::version_of(&before.1),
            }),
        )
        .await;
    assert_eq!(error["code"], "noteInvalid", "{error}");
    assert!(error["message"].as_str().unwrap().contains("revision.json"));
    assert_eq!(fingerprint(&session, RICH), before);
    // The copy put in the trash for it is taken back.
    assert!(session.harness.library().trash_items().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn an_ordinary_note_is_refused_by_edit_revision_whatever_the_orders() {
    let session = session().await;
    for id in [OPTIQUE, CHIMIE, DEVOIR, VIDE] {
        let before = fingerprint(&session, id);
        let error = session
            .failure(
                "edit_revision",
                json!({ "note": id, "orders": [recolor(0)], "summary": "Essai" }),
            )
            .await;
        assert_eq!(error["code"], "notARevision", "{id}: {error}");
        assert_eq!(fingerprint(&session, id), before, "{id}");
    }
    assert!(session.harness.library().trash_items().unwrap().is_empty());
    // Even orders that are wrong say first that the note is not a revision.
    let error = session
        .failure(
            "edit_revision",
            json!({ "note": CHIMIE, "orders": [recolor(99)], "summary": "Essai" }),
        )
        .await;
    assert_eq!(error["code"], "notARevision");
}

// ---------------------------------------------------------------------------------------------
// create_revision

#[tokio::test(flavor = "current_thread")]
async fn the_copy_keeps_what_the_source_has_and_is_a_new_note() {
    let session = session().await;
    add_rich_note(&session);
    let source = file_of(&session, RICH);
    let made = session
        .ok(
            "create_revision",
            json!({
                "note": RICH,
                "orders": [recolor(0), { "op": "delete", "select": { "ids": [2] } }],
                "summary": "Premier trait en rouge ; un trait caché supprimé",
            }),
        )
        .await;
    let id = made["note"]["id"].as_str().unwrap();
    assert_ne!(id, RICH);
    assert_eq!(made["stroke_count"], 3);
    assert_eq!(made["original"]["id"], RICH);
    assert_eq!(made["report"]["strokes_before"], 4);
    assert_eq!(made["report"]["strokes_after"], 3);

    let copy = file_of(&session, id);
    // What is kept.
    assert_eq!(copy.note.background, source.note.background);
    assert_eq!(copy.note.page, source.note.page);
    assert_eq!(copy.note.layers, source.note.layers);
    assert_eq!(copy.note.background_image, source.note.background_image);
    assert_eq!(copy.background, source.background);
    assert_eq!(copy.note.tags, source.note.tags);
    assert_eq!(copy.audio, source.audio);
    assert_eq!(copy.text, source.text);
    assert_eq!(copy.note.created, source.note.created);
    assert_eq!(copy.note.timing, source.note.timing);
    // What is new.
    assert_ne!(copy.note.id, source.note.id);
    assert!(!copy.note.favorite, "the copy is not a favourite");
    assert!(source.note.favorite);
    assert!(copy.note.modified > source.note.modified);
    assert_ne!(copy.thumbnail, source.thumbnail);
    assert!(copy.thumbnail.is_some());
    // The ink: the strokes the orders left, recoloured.
    assert_eq!(copy.note.strokes.len(), 3);
    assert_eq!(copy.note.strokes[0].color, Rgba::rgb(255, 0, 0));
    assert_eq!(copy.note.strokes[1], source.note.strokes[1]);
    // The note next to its source, with its account.
    assert_eq!(made["note"]["binder"], "Physique");
    let revision = copy.revision.expect("a revision");
    assert_eq!(revision.of, RICH);
    assert_eq!(revision.of_title, "Rapport d'incident");
    assert_eq!(revision.summary.len(), 1);
    assert_eq!(
        revision.summary[0].text,
        "Premier trait en rouge ; un trait caché supprimé"
    );
    // "Back to the original" is the source's ink (the editor's rule, done by the app's side).
    let original = ardoise_core::format::read_from_bytes(&copy.original_note.expect("original"))
        .unwrap()
        .note;
    assert_eq!(original.strokes, source.note.strokes);
}

#[tokio::test(flavor = "current_thread")]
async fn the_default_title_follows_the_apps_language_and_an_untitled_note_has_one_too() {
    let en = session().await;
    let made = en
        .ok(
            "create_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0)], "summary": "x" }),
        )
        .await;
    assert_eq!(made["note"]["title"], "Cours d'optique (clean)");
    let untitled = en
        .ok(
            "create_revision",
            json!({
                "note": VIDE,
                "orders": [{ "op": "add", "shapes": [{ "line": { "from": [10, 10], "to": [50, 10] } }], "color": "#000000" }],
                "summary": "x",
            }),
        )
        .await;
    assert_eq!(untitled["note"]["title"], "Untitled note (clean)");

    let fr = session_with(Language::Fr, ServerOptions::default()).await;
    let made = fr
        .ok(
            "create_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0)], "summary": "x" }),
        )
        .await;
    assert_eq!(made["note"]["title"], "Cours d'optique (propre)");
    // A title given wins.
    let named = fr
        .ok(
            "create_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0)], "summary": "x", "title": "Optique, au propre" }),
        )
        .await;
    assert_eq!(named["note"]["title"], "Optique, au propre");
    assert_eq!(
        fr.harness.library().list().notes.len(),
        6,
        "two copies of the optics note and one more"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_revision_of_a_revision_starts_a_new_account() {
    let session = session().await;
    let first = session
        .ok(
            "create_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0)], "summary": "Première passe" }),
        )
        .await;
    let first_id = first["note"]["id"].as_str().unwrap().to_owned();
    let second = session
        .ok(
            "create_revision",
            json!({ "note": first_id, "orders": [recolor(1)], "summary": "Deuxième passe" }),
        )
        .await;
    let copy = file_of(&session, second["note"]["id"].as_str().unwrap());
    let revision = copy.revision.unwrap();
    assert_eq!(revision.of, first_id);
    assert_eq!(revision.summary.len(), 1);
    assert_eq!(revision.summary[0].text, "Deuxième passe");
}

#[tokio::test(flavor = "current_thread")]
async fn bad_requests_write_nothing() {
    let session = session().await;
    let listed = session.harness.library().list().notes.len();
    for (arguments, code) in [
        // No account of the changes.
        (
            json!({ "note": OPTIQUE, "orders": [recolor(0)], "summary": "   " }),
            "invalidArguments",
        ),
        // Nothing to do.
        (
            json!({ "note": OPTIQUE, "orders": [], "summary": "x" }),
            "invalidArguments",
        ),
        (
            json!({ "note": "Inconnue", "orders": [recolor(0)], "summary": "x" }),
            "noteNotFound",
        ),
        // A list the engine refuses.
        (
            json!({ "note": OPTIQUE, "orders": [recolor(99)], "summary": "x" }),
            "invalidOrders",
        ),
    ] {
        let error = session.failure("create_revision", arguments.clone()).await;
        assert_eq!(error["code"], code, "{arguments}: {error}");
    }
    // An unknown order, or an unknown field, does not even reach the engine.
    let error = session
        .failure(
            "create_revision",
            json!({ "note": OPTIQUE, "orders": [{ "op": "explode" }], "summary": "x" }),
        )
        .await;
    assert_eq!(error["code"], "invalidArguments");
    assert_eq!(session.harness.library().list().notes.len(), listed);
    assert!(session.harness.write.creations.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn the_message_of_invalid_orders_is_the_engines() {
    let session = session().await;
    let orders = json!([recolor(0), recolor(99)]);
    let error = session
        .failure(
            "create_revision",
            json!({ "note": OPTIQUE, "orders": orders, "summary": "x" }),
        )
        .await;
    assert_eq!(error["code"], "invalidOrders");
    // The same list given to the engine itself.
    let note = file_of(&session, OPTIQUE).note;
    let engine = ardoise_revise::apply(
        &ardoise_revise::Input {
            strokes: &note.strokes,
            page_mm: [
                note.page.width as f32 * 0.01,
                note.page.height as f32 * 0.01,
            ],
            layers: note.layers.len() as u32,
            visible_layers: &[true, false],
            narration: false,
        },
        &serde_json::from_value::<Vec<ardoise_revise::Order>>(orders).unwrap(),
    )
    .unwrap_err();
    assert_eq!(error["message"], engine.to_string());
    assert!(
        error["message"].as_str().unwrap().contains("order 1"),
        "{error}"
    );
    // The preview says the same.
    let preview = session
        .failure(
            "preview_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0), recolor(99)] }),
        )
        .await;
    assert_eq!(preview, error);
}

#[tokio::test(flavor = "current_thread")]
async fn a_narration_reaches_the_engine_and_nothing_moves_in_time() {
    // A long line drawn right after the waves does not fit before the circle of the chemistry
    // note: without a narration the circle moves on, with one it does not and the line is drawn
    // faster.
    let order = json!({
        "op": "add",
        "shapes": [{ "line": { "from": [10, 10], "to": [290, 10] } }],
        "color": "#000000",
        "after": 0,
    });
    let session = session().await;
    let plain = session
        .ok(
            "create_revision",
            json!({ "note": CHIMIE, "orders": [order], "summary": "Une ligne" }),
        )
        .await;
    assert_eq!(plain["report"]["strokes_pushed"], 1, "{plain}");
    assert_eq!(plain["report"]["strokes_faster"], 0);

    // The same note with a narration.
    let path = session.harness.path_of(CHIMIE);
    let library = session.harness.library();
    let loaded = library.read(&path).unwrap();
    let mut file = loaded.file;
    file.audio = Some(vec![9; 16]);
    library.save(&path, &file, &loaded.stamp).unwrap();
    let narrated = session
        .ok(
            "create_revision",
            json!({ "note": CHIMIE, "orders": [order], "summary": "Une ligne" }),
        )
        .await;
    assert_eq!(narrated["report"]["strokes_pushed"], 0, "{narrated}");
    assert_eq!(narrated["report"]["strokes_faster"], 1);
    // The copy keeps the narration, and the circle's times.
    let copy = file_of(&session, narrated["note"]["id"].as_str().unwrap());
    assert_eq!(copy.audio, Some(vec![9; 16]));
    assert_eq!(
        copy.note.strokes[3].points[0].t,
        file_of(&session, CHIMIE).note.strokes[3].points[0].t
    );
}

// ---------------------------------------------------------------------------------------------
// edit_revision

#[tokio::test(flavor = "current_thread")]
async fn a_revision_is_edited_in_place_with_the_numbers_get_note_gives() {
    let session = session().await;
    let made = session
        .ok(
            "create_revision",
            json!({
                "note": CHIMIE,
                "orders": [{ "op": "delete", "select": { "ids": [0] } }],
                "summary": "Premier trait supprimé",
            }),
        )
        .await;
    let id = made["note"]["id"].as_str().unwrap().to_owned();
    let path = session.harness.path_of(&id);
    let library = session.harness.library();
    let before = library.read(&path).unwrap();
    let source = fingerprint(&session, CHIMIE);

    // `get_note` on the copy gives the numbers the orders are about now: the old stroke 1 is 0.
    let note = session
        .ok("get_note", json!({ "note": id, "strokes": "summary" }))
        .await;
    let strokes = note["strokes"].as_array().unwrap();
    assert_eq!(strokes.len(), 3);
    let wanted = &strokes[1];
    assert_eq!(wanted["index"], 1);
    assert_eq!(
        note["revision"]["account"][0]["text"],
        "Premier trait supprimé"
    );
    assert_eq!(note["revision"]["of"], CHIMIE);

    let edited = session
        .ok(
            "edit_revision",
            json!({
                "note": id,
                "orders": [{ "op": "recolor", "select": { "ids": [1] }, "color": "#00aa00" }],
                "summary": "Deuxième trait en vert",
            }),
        )
        .await;
    assert_eq!(edited["saved"], true);
    assert_eq!(edited["account_entries"], 2);
    let after = library.read(&path).unwrap();
    assert_eq!(after.file.note.id, id);
    assert_eq!(after.file.note.strokes[1].color, Rgba::rgb(0, 170, 0));
    assert_eq!(after.file.note.strokes[0], before.file.note.strokes[0]);
    assert_eq!(after.file.note.strokes[2], before.file.note.strokes[2]);
    // The account grew by one line, the first untouched.
    let account = &after.file.revision.as_ref().unwrap().summary;
    assert_eq!(account.len(), 2);
    assert_eq!(account[0].text, "Premier trait supprimé");
    assert_eq!(account[1].text, "Deuxième trait en vert");
    assert!(account[1].at >= account[0].at);
    // The version before is in the trash, a note of its own.
    let items = library.trash_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].title.as_deref(),
        Some("Chimie des métaux (clean) (before retouching)")
    );
    assert_eq!(edited["previous_version"]["in_trash"], items[0].id);
    let restored = library.restore(&items[0].id).unwrap();
    let kept = library.read(&restored).unwrap().file;
    assert_eq!(kept.note.strokes, before.file.note.strokes);
    assert_ne!(kept.note.id, id);
    // The source is still as it was, and the edit saved once.
    assert_eq!(fingerprint(&session, CHIMIE), source);
    assert_eq!(*session.harness.write.saves.lock().unwrap(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn the_version_before_an_edit_is_named_in_the_apps_language() {
    let session = session_with(Language::Fr, ServerOptions::default()).await;
    let made = session
        .ok(
            "create_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0)], "summary": "x" }),
        )
        .await;
    let edited = session
        .ok(
            "edit_revision",
            json!({ "note": made["note"]["id"], "orders": [recolor(1)], "summary": "y" }),
        )
        .await;
    assert_eq!(
        edited["previous_version"]["title"],
        "Cours d'optique (propre) (avant retouche)"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn an_edit_respects_the_stamp_and_the_orders() {
    let session = session().await;
    let made = session
        .ok(
            "create_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0)], "summary": "x" }),
        )
        .await;
    let id = made["note"]["id"].as_str().unwrap().to_owned();
    let path = session.harness.path_of(&id);
    let before = fs::read(&path).unwrap();

    // Orders the engine refuses: nothing happens, nothing is put in the trash.
    let error = session
        .failure(
            "edit_revision",
            json!({ "note": id, "orders": [recolor(50)], "summary": "y" }),
        )
        .await;
    assert_eq!(error["code"], "invalidOrders");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(session.harness.library().trash_items().unwrap().is_empty());

    // Another PC saves the copy between the tool's read and its write.
    let root = session.harness.root.clone();
    *session.harness.write.before_save.lock().unwrap() = Some(Box::new(move |note| {
        ardoise_core::library::Library::open(&root)
            .unwrap()
            .set_favorite(note, true, None)
            .unwrap();
    }));
    let error = session
        .failure(
            "edit_revision",
            json!({ "note": id, "orders": [recolor(1)], "summary": "y" }),
        )
        .await;
    assert_eq!(error["code"], "noteConflict", "{error}");
    let kept = session.harness.library().read(&path).unwrap().file;
    assert!(kept.note.favorite, "the other version stays");
    assert_eq!(kept.revision.unwrap().summary.len(), 1);
    assert!(session.harness.library().trash_items().unwrap().is_empty());
}

// ---------------------------------------------------------------------------------------------
// preview_revision

#[tokio::test(flavor = "current_thread")]
async fn a_preview_answers_a_picture_and_the_report_and_writes_nothing() {
    let session = session().await;
    let before = fingerprint(&session, CHIMIE);
    let notes = session.harness.library().list().notes.len();
    let plain = session.call("render_note", json!({ "note": CHIMIE })).await;
    let result = session
        .call(
            "preview_revision",
            json!({ "note": CHIMIE, "orders": [recolor(0), recolor(1)] }),
        )
        .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let info = text_of(&result);
    assert_eq!(info["preview"], true);
    assert_eq!(info["written"], false);
    assert_eq!(info["stroke_count"], 4);
    assert_eq!(info["report"]["strokes_after"], 4);
    assert_eq!(info["report"]["orders"].as_array().unwrap().len(), 2);
    assert_eq!(info["report"]["orders"][0]["op"], "recolor");
    assert_eq!(info["report"]["orders"][0]["selected"], 1);
    assert_eq!(info["labels"], false);
    // The picture shows the strokes in red.
    let image = decoded(&result);
    assert!(red_pixels(&image) > 100);
    assert_eq!(red_pixels(&decoded(&plain)), 0);
    // Nothing was written.
    assert_eq!(fingerprint(&session, CHIMIE), before);
    assert_eq!(session.harness.library().list().notes.len(), notes);
    assert_eq!(*session.harness.write.saves.lock().unwrap(), 0);
    // An empty list previews the note as it is.
    let same = session
        .call("preview_revision", json!({ "note": CHIMIE, "orders": [] }))
        .await;
    assert_eq!(decoded(&same).pixels, decoded(&plain).pixels);
}

#[tokio::test(flavor = "current_thread")]
async fn a_preview_can_be_zoomed_and_labelled() {
    let session = session().await;
    let orders = json!([{ "op": "move", "select": { "ids": [3] }, "dx": 5.0, "dy": 0.0 }]);
    let bare = session
        .call(
            "preview_revision",
            json!({ "note": CHIMIE, "orders": orders, "region": { "x": 100, "y": 80, "width": 100, "height": 80 }, "dpi": 150 }),
        )
        .await;
    let labelled = session
        .call(
            "preview_revision",
            json!({ "note": CHIMIE, "orders": orders, "region": { "x": 100, "y": 80, "width": 100, "height": 80 }, "dpi": 150, "labels": true }),
        )
        .await;
    assert_ne!(labelled.is_error, Some(true), "{labelled:?}");
    let info = text_of(&labelled);
    assert_eq!(info["labels"], true);
    assert_eq!(info["region_mm"]["width"], 100.0);
    let (bare, labelled) = (decoded(&bare), decoded(&labelled));
    assert_eq!((bare.width, bare.height), (labelled.width, labelled.height));
    assert!(labelled.width.abs_diff(591) <= 1 && labelled.height.abs_diff(472) <= 1);
    assert_ne!(bare.pixels, labelled.pixels, "numbers were written");
    assert!(red_pixels(&labelled) > red_pixels(&bare) + 10);
}

#[tokio::test(flavor = "current_thread")]
async fn the_labels_of_a_preview_are_the_numbers_of_the_orders() {
    // After a `delete` of the first stroke the others keep their numbers: the labels say 1, 2, 3
    // for strokes that are the first three of the result. Compared with the picture of a note
    // drawn the same way, with the numbers 1, 2 and 3 given by hand.
    let session = session().await;
    let numbers = |spots: Vec<ardoise_render::LabelSpot>| -> Vec<usize> {
        spots.into_iter().map(|s| s.number).collect()
    };
    let note = file_of(&session, CHIMIE).note;
    let revised = ardoise_revise::apply(
        &ardoise_revise::Input {
            strokes: &note.strokes,
            page_mm: [297.0, 216.0],
            layers: 2,
            visible_layers: &[true, false],
            narration: false,
        },
        &serde_json::from_value::<Vec<ardoise_revise::Order>>(json!([
            { "op": "delete", "select": { "ids": [0] } },
            { "op": "copy", "select": { "ids": [3] }, "dx": 40.0 },
        ]))
        .unwrap(),
    )
    .unwrap();
    // 3 old strokes left, then the circle's copy, numbered 4.
    assert_eq!(revised.numbers, vec![1, 2, 3, 4]);
    let mut drawn = note.clone();
    drawn.strokes = revised.strokes.clone();
    // The tool's own function: only the strokes on visible layers are numbered (stroke 2, on the
    // layer "Corrections", is hidden).
    let spots = ardoise_mcp::read::label_spots(&drawn, &revised.numbers);
    assert_eq!(numbers(spots), vec![1, 3, 4]);

    // And through the tool: the labels differ from those of the plain note with its own numbers.
    let orders = json!([
        { "op": "delete", "select": { "ids": [0] } },
        { "op": "copy", "select": { "ids": [3] }, "dx": 40.0 },
    ]);
    let region = json!({ "x": 0, "y": 0, "width": 297, "height": 216 });
    let by_orders = session
        .call(
            "preview_revision",
            json!({ "note": CHIMIE, "orders": orders, "labels": true, "region": region, "dpi": 200 }),
        )
        .await;
    let by_place = session
        .call(
            "render_note",
            json!({ "note": CHIMIE, "labels": true, "region": region, "dpi": 200 }),
        )
        .await;
    assert_ne!(decoded(&by_orders).pixels, decoded(&by_place).pixels);
}

// ---------------------------------------------------------------------------------------------
// The read tools

#[tokio::test(flavor = "current_thread")]
async fn render_note_writes_the_stroke_numbers_on_request() {
    let session = session().await;
    let plain = decoded(&session.call("render_note", json!({ "note": CHIMIE })).await);
    let labelled_result = session
        .call("render_note", json!({ "note": CHIMIE, "labels": true }))
        .await;
    assert_eq!(text_of(&labelled_result)["labels"], true);
    let labelled = decoded(&labelled_result);
    assert_eq!(red_pixels(&plain), 0);
    assert!(red_pixels(&labelled) > 30);
    assert_ne!(plain.pixels, labelled.pixels);
    // `labels: false` is the plain picture.
    let off = decoded(
        &session
            .call("render_note", json!({ "note": CHIMIE, "labels": false }))
            .await,
    );
    assert_eq!(off.pixels, plain.pixels);
}

#[tokio::test(flavor = "current_thread")]
async fn get_strokes_gives_the_exact_samples() {
    let session = session().await;
    let result = session
        .ok("get_strokes", json!({ "note": CHIMIE, "strokes": [3, 0] }))
        .await;
    assert_eq!(
        result["columns"],
        json!(["i", "x_mm", "y_mm", "pressure", "t_ms"])
    );
    let note = file_of(&session, CHIMIE).note;
    let strokes = result["strokes"].as_array().unwrap();
    // In the order asked.
    assert_eq!(strokes[0]["index"], 3);
    assert_eq!(strokes[1]["index"], 0);
    let circle = &note.strokes[3];
    assert_eq!(strokes[0]["point_count"], circle.points.len());
    let samples = strokes[0]["samples"].as_array().unwrap();
    assert_eq!(samples.len(), circle.points.len());
    for (i, sample) in samples.iter().enumerate() {
        let p = &circle.points[i];
        assert_eq!(sample[0], i);
        assert!((sample[1].as_f64().unwrap() - f64::from(p.x) / 100.0).abs() < 0.006);
        assert!((sample[2].as_f64().unwrap() - f64::from(p.y) / 100.0).abs() < 0.006);
        assert!((sample[3].as_f64().unwrap() - f64::from(p.pressure)).abs() < 0.001);
        assert_eq!(sample[4], p.t);
    }

    // A step keeps every n-th sample and the last one.
    let stepped = session
        .ok(
            "get_strokes",
            json!({ "note": CHIMIE, "strokes": [3], "step": 50 }),
        )
        .await;
    let indices: Vec<u64> = stepped["strokes"][0]["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s[0].as_u64().unwrap())
        .collect();
    assert_eq!(indices, vec![0, 50, 100, 120]);
    assert_eq!(stepped["sample_total"], 4);

    // A stroke asked for twice comes once, at the first place it was asked for.
    let repeated = session
        .ok(
            "get_strokes",
            json!({ "note": CHIMIE, "strokes": [3, 0, 3, 1, 0], "step": 50 }),
        )
        .await;
    let order: Vec<u64> = repeated["strokes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["index"].as_u64().unwrap())
        .collect();
    assert_eq!(order, [3, 0, 1]);
}

#[tokio::test(flavor = "current_thread")]
async fn get_strokes_refuses_what_it_cannot_give() {
    let session = session().await;
    for (arguments, needle) in [
        (json!({ "note": CHIMIE, "strokes": [] }), "empty"),
        (json!({ "note": CHIMIE, "strokes": [4] }), "no stroke 4"),
        (
            json!({ "note": CHIMIE, "strokes": [0], "step": 0 }),
            "at least 1",
        ),
    ] {
        let error = session.failure("get_strokes", arguments).await;
        assert_eq!(error["code"], "invalidArguments");
        assert!(
            error["message"].as_str().unwrap().contains(needle),
            "{error}"
        );
    }
    // More than 20 000 samples at once: refused with the advice.
    let mut dense = make_note("66666666-6666-4666-8666-666666666666", "Dense", 4);
    dense.strokes = (0..3)
        .map(|s| Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 50.0,
            layer: 0,
            points: (0..8_000)
                .map(|i| Point {
                    x: i as f32,
                    y: 100.0 * s as f32,
                    pressure: 0.5,
                    t: i,
                })
                .collect(),
        })
        .collect();
    session
        .harness
        .library()
        .create(std::path::Path::new(""), &NoteFile::new(dense))
        .unwrap();
    let error = session
        .failure(
            "get_strokes",
            json!({ "note": "66666666-6666-4666-8666-666666666666", "strokes": [0, 1, 2] }),
        )
        .await;
    assert_eq!(error["code"], "invalidArguments");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("24000") && message.contains("`step`"),
        "{message}"
    );
    let ok = session
        .ok(
            "get_strokes",
            json!({ "note": "66666666-6666-4666-8666-666666666666", "strokes": [0, 1, 2], "step": 2 }),
        )
        .await;
    // 4 000 samples a stroke, and the last one of each.
    assert_eq!(ok["sample_total"], 12_003);
}

#[tokio::test(flavor = "current_thread")]
async fn ink_lines_finds_the_written_lines() {
    // Three lines of ten small "letters" each, 6 mm high, 15 mm apart, and a long rule.
    let mut note = make_note("77777777-7777-4777-8777-777777777777", "Trois lignes", 4);
    note.strokes = Vec::new();
    for line in 0..3 {
        for letter in 0..10 {
            let (x0, y0) = (
                2_000.0 + letter as f32 * 600.0,
                4_000.0 + line as f32 * 1_500.0,
            );
            note.strokes.push(Stroke {
                tool: Tool::Ballpoint,
                color: Rgba::BLACK,
                width: 40.0,
                layer: 0,
                points: (0..8)
                    .map(|i| Point {
                        x: x0 + 200.0 * (i as f32 * 0.8).sin().abs(),
                        y: y0 + i as f32 * 60.0,
                        pressure: 0.5,
                        t: (line * 10 + letter) * 100 + i,
                    })
                    .collect(),
            });
        }
    }
    let session = session().await;
    session
        .harness
        .library()
        .create(std::path::Path::new(""), &NoteFile::new(note))
        .unwrap();
    let result = session
        .ok(
            "ink_lines",
            json!({ "note": "77777777-7777-4777-8777-777777777777" }),
        )
        .await;
    assert_eq!(result["line_count"], 3, "{result}");
    let lines = result["lines"].as_array().unwrap();
    for (i, line) in lines.iter().enumerate() {
        assert_eq!(line["strokes"].as_array().unwrap().len(), 10, "line {i}");
        assert!(line["slope"].as_f64().unwrap().abs() < 1.0);
        assert!(line["body"].as_f64().unwrap() > 0.0);
        assert!(!line["words"].as_array().unwrap().is_empty());
    }
    // From top to bottom.
    assert!(lines[0]["baseline_left"].as_f64() < lines[2]["baseline_left"].as_f64());
    assert_eq!(result["apart"], json!([]));
    // Hidden strokes are not looked at.
    let hidden = session.ok("ink_lines", json!({ "note": CHIMIE })).await;
    assert!(hidden["lines"].is_array());
}

#[tokio::test(flavor = "current_thread")]
async fn get_note_gives_the_rules_of_a_ruled_template() {
    let session = session().await;
    add_rich_note(&session);
    let ruled = session.ok("get_note", json!({ "note": RICH })).await;
    assert_eq!(
        ruled["background"]["ruling"],
        json!({ "first_mm": 16.0, "pitch_mm": 8.0 })
    );
    assert!(ruled.get("revision").is_none(), "not a revision");
    let blank = session.ok("get_note", json!({ "note": OPTIQUE })).await;
    assert!(blank["background"].get("ruling").is_none());
    // A copy says whose copy it is.
    let made = session
        .ok(
            "create_revision",
            json!({ "note": RICH, "orders": [recolor(0)], "summary": "Rouge" }),
        )
        .await;
    let copy = session
        .ok("get_note", json!({ "note": made["note"]["id"] }))
        .await;
    assert_eq!(copy["revision"]["of"], RICH);
    assert_eq!(copy["revision"]["of_title"], "Rapport d'incident");
    assert_eq!(
        copy["background"]["ruling"],
        json!({ "first_mm": 16.0, "pitch_mm": 8.0 })
    );
}

// ---------------------------------------------------------------------------------------------
// Annotations, schema, read-only

#[tokio::test(flavor = "current_thread")]
async fn the_new_tools_are_described_and_annotated() {
    let session = session().await;
    let tools = session.client.list_all_tools().await.unwrap();
    let tool = |name: &str| tools.iter().find(|t| t.name == name).unwrap();
    for (name, read_only, destructive, idempotent) in [
        ("get_strokes", true, false, true),
        ("ink_lines", true, false, true),
        ("preview_revision", true, false, true),
        ("create_revision", false, false, false),
        ("edit_revision", false, true, false),
        ("straighten_shapes", false, true, false),
        // It only adds on an ordinary note, and rewrites a revision.
        ("split_note", false, true, false),
    ] {
        let annotations = tool(name).annotations.as_ref().unwrap();
        assert_eq!(annotations.read_only_hint, Some(read_only), "{name}");
        assert_eq!(annotations.destructive_hint, Some(destructive), "{name}");
        assert_eq!(annotations.idempotent_hint, Some(idempotent), "{name}");
        assert!(tool(name).description.as_deref().unwrap().len() > 80);
    }
    let schema = |name: &str| Value::Object((*tool(name).input_schema).clone());
    assert_eq!(
        schema("create_revision")["required"],
        json!(["note", "orders", "expected_version", "summary"])
    );
    assert_eq!(
        schema("edit_revision")["required"],
        json!(["note", "orders", "expected_version", "summary"])
    );
    assert_eq!(
        schema("preview_revision")["required"],
        json!(["note", "orders"])
    );
    assert_eq!(
        schema("get_strokes")["required"],
        json!(["note", "strokes"])
    );
    // The schema names the orders, and the tools send the assistant to what describes them.
    let preview = schema("preview_revision").to_string();
    for needle in ["tidy_line", "glyph", "retime", "recolor", "new_strokes"] {
        assert!(preview.contains(needle), "{needle} is not in the schema");
    }
    assert!(
        tool("preview_revision")
            .description
            .as_deref()
            .unwrap()
            .contains("`revision_orders`")
    );
    assert!(
        schema("render_note")["properties"]["labels"]["description"]
            .as_str()
            .unwrap()
            .contains("number")
    );
    // The straightening says it makes a copy.
    let straighten = tool("straighten_shapes").description.as_deref().unwrap();
    assert!(straighten.contains("copy") && straighten.contains("never changed"));
    assert!(!straighten.contains("(before straightening)"));
    // So does the split, and what it answers.
    let split = tool("split_note").description.as_deref().unwrap();
    assert!(split.contains("never changed") && split.contains("original_unchanged"));
    // The tools that take orders say how to tie them to what was seen.
    assert!(
        schema("edit_revision")["properties"]["expected_version"]["description"]
            .as_str()
            .unwrap()
            .contains("noteConflict")
    );
    for name in ["preview_revision", "create_revision", "edit_revision"] {
        assert!(
            tool(name)
                .description
                .as_deref()
                .unwrap()
                .contains("version")
        );
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_only_server_describes_no_tool_it_does_not_have() {
    let full = session().await;
    let every: Vec<String> = full
        .client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    let read_only = session_with(Language::En, ServerOptions { read_only: true }).await;
    let tools = read_only.client.list_all_tools().await.unwrap();
    let served: Vec<String> = tools.iter().map(|t| t.name.to_string()).collect();
    let mut absent: Vec<String> = every.into_iter().filter(|n| !served.contains(n)).collect();
    assert!(absent.iter().any(|n| n == "preview_revision"));
    // Nor the orders those tools take.
    absent.extend(["`trim`", "`glyph`", "`tidy_line`", "the orders"].map(str::to_owned));
    for tool in &tools {
        let text = format!(
            "{} {}",
            tool.description.as_deref().unwrap_or_default(),
            Value::Object((*tool.input_schema).clone())
        );
        for name in &absent {
            assert!(!text.contains(name.as_str()), "{}: {name}", tool.name);
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_only_server_has_the_read_tools_and_none_of_the_writing_ones() {
    let session = session_with(Language::En, ServerOptions { read_only: true }).await;
    let names: Vec<String> = session
        .client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    for name in ["get_strokes", "ink_lines", "render_note", "get_note"] {
        assert!(names.iter().any(|n| n == name), "{name} is missing");
    }
    for name in ["preview_revision", "create_revision", "edit_revision"] {
        assert!(!names.iter().any(|n| n == name), "{name} is listed");
    }
    let before = fingerprint(&session, OPTIQUE);
    let listed = session.harness.library().list().notes.len();
    for (name, arguments) in [
        ("preview_revision", json!({ "note": OPTIQUE, "orders": [] })),
        (
            "create_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0)], "summary": "x" }),
        ),
        (
            "edit_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0)], "summary": "x" }),
        ),
    ] {
        let outcome = session
            .client
            .call_tool(
                rmcp::model::CallToolRequestParams::new(name)
                    .with_arguments(arguments.as_object().cloned().unwrap()),
            )
            .await;
        match outcome {
            Err(_) => {}
            Ok(result) => assert_eq!(result.is_error, Some(true), "{name} was accepted"),
        }
    }
    assert_eq!(fingerprint(&session, OPTIQUE), before);
    assert_eq!(session.harness.library().list().notes.len(), listed);
}

#[tokio::test(flavor = "current_thread")]
async fn a_host_without_write_services_cannot_write_a_copy() {
    use std::sync::Arc;
    let harness = Harness::new(Language::En);
    let host = ardoise_mcp::Host::new(
        Arc::new(Env {
            root: harness.root.clone(),
            hidden: None,
            language: Language::En,
            tuning: Default::default(),
        }),
        harness.fake.clone(),
    );
    let session = session_on(harness, host, ServerOptions::default()).await;
    let error = session
        .failure(
            "create_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0)], "summary": "x" }),
        )
        .await;
    assert_eq!(error["code"], "writeUnavailable");
    // The preview needs no service.
    let preview = session
        .call(
            "preview_revision",
            json!({ "note": OPTIQUE, "orders": [recolor(0)] }),
        )
        .await;
    assert_ne!(preview.is_error, Some(true));
}

// ---------------------------------------------------------------------------------------------
// Real notes, when they are there

/// The real notes of `testdata/private/revise` (git-ignored; in this checkout or the one it is a
/// worktree of): the labels and the preview on pages as they are written. Skipped without them.
/// `ARDOISE_SAMPLES_DIR` makes it write the pictures there (private: never commit them).
#[tokio::test(flavor = "current_thread")]
async fn real_notes_are_labelled_and_previewed() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|dir| dir.join("testdata/private/revise"))
        .find(|dir| dir.is_dir());
    let Some(dir) = dir else {
        eprintln!("skipped: no testdata/private/revise here");
        return;
    };
    let pictures = std::env::var_os("ARDOISE_SAMPLES_DIR").map(std::path::PathBuf::from);
    let session = session().await;
    let mut seen = 0;
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "ardoise") {
            continue;
        }
        let file = ardoise_core::format::read_note(&path).unwrap();
        let (id, stem) = (
            file.note.id.clone(),
            path.file_stem().unwrap().to_string_lossy().into_owned(),
        );
        let strokes = file.note.strokes.len();
        session
            .harness
            .library()
            .create(std::path::Path::new("Privé"), &file)
            .unwrap();
        // The whole page with the numbers, then a zoom on its middle.
        let whole = session
            .call("render_note", json!({ "note": id, "labels": true }))
            .await;
        assert_ne!(whole.is_error, Some(true), "{stem}: {whole:?}");
        let page = text_of(&whole)["page"].clone();
        let (w, h) = (
            page["width_mm"].as_f64().unwrap(),
            page["height_mm"].as_f64().unwrap(),
        );
        let region = json!({ "x": w * 0.25, "y": h * 0.25, "width": w * 0.3, "height": h * 0.2 });
        let zoom = session
            .call(
                "preview_revision",
                json!({ "note": id, "orders": [], "labels": true, "region": region, "dpi": 250 }),
            )
            .await;
        assert_ne!(zoom.is_error, Some(true), "{stem}: {zoom:?}");
        let image = decoded(&zoom);
        println!(
            "{stem}: {strokes} strokes, zoom {} x {} px, {} red pixels",
            image.width,
            image.height,
            red_pixels(&image)
        );
        if let Some(out) = &pictures {
            fs::create_dir_all(out).unwrap();
            fs::write(
                out.join(format!("{stem}-labels-zoom.png")),
                ardoise_render::encode_png(&image).unwrap(),
            )
            .unwrap();
            fs::write(
                out.join(format!("{stem}-labels-page.png")),
                ardoise_render::encode_png(&decoded(&whole)).unwrap(),
            )
            .unwrap();
        }
        seen += 1;
    }
    eprintln!("{seen} real note(s) checked");
}
