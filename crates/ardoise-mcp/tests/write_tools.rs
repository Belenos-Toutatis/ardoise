//! The write tools, through rmcp's in-memory transport on a temporary library of synthetic
//! notes, with fake write services made of `ardoise-core`'s operations: each tool does its
//! work, the files stay atomic and never overwritten, nothing is deleted for good, relative paths
//! are refused, errors carry codes, and a read-only server has none of them.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support;

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use ardoise_core::library::Library;
use ardoise_hwr::Language;
use ardoise_mcp::write::{ExportFormat, ExportTarget};
use ardoise_mcp::{Host, ServerOptions, build_tool_router};
use rmcp::model::CallToolRequestParams;
use serde_json::{Value, json};
use support::*;

const WRITE_TOOLS: [&str; 9] = [
    "create_note",
    "export_note",
    "merge_notes",
    "move_note",
    "set_note_text",
    "split_note",
    "straighten_shapes",
    "trash_note",
    "update_note",
];

/// Tools that give the same result when called twice.
const IDEMPOTENT: [&str; 3] = ["move_note", "set_note_text", "update_note"];

/// Tools that change or remove what is there (`destructiveHint`: the MCP specification keeps
/// `false` for the tools that only add), even when the app can restore it.
const DESTRUCTIVE: [&str; 5] = [
    "move_note",
    "split_note",
    "straighten_shapes",
    "trash_note",
    "update_note",
];

/// Every file under `root` outside `_ardoise` that is not a note: leftovers of an interrupted
/// atomic write would show here.
fn strays(root: &Path) -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap().flatten() {
            let path = entry.path();
            if path.file_name().unwrap() == "_ardoise" {
                continue;
            }
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_none_or(|e| e != "ardoise") {
                out.push(path);
            }
        }
    }
    let mut out = Vec::new();
    walk(root, &mut out);
    out
}

fn titles(library: &Library) -> Vec<String> {
    let mut titles: Vec<String> = library.list().notes.into_iter().map(|n| n.title).collect();
    titles.sort();
    titles
}

// ---------------------------------------------------------------------------------------------
// The tool list, annotations, read-only

#[tokio::test(flavor = "current_thread")]
async fn every_write_tool_is_listed_with_a_schema_and_annotations() {
    let session = session().await;
    let tools = session.client.list_all_tools().await.unwrap();
    for name in WRITE_TOOLS {
        let tool = tools
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} is not listed"));
        assert!(
            tool.description.as_deref().is_some_and(|d| d.len() > 60),
            "{name} has no description"
        );
        assert_eq!(tool.input_schema.get("type").unwrap(), "object", "{name}");
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(false), "{name}");
        assert_eq!(
            annotations.destructive_hint,
            Some(DESTRUCTIVE.contains(&name)),
            "{name}"
        );
        assert_eq!(
            annotations.idempotent_hint,
            Some(IDEMPOTENT.contains(&name)),
            "{name}"
        );
    }
    let schema = |name: &str| {
        let tool = tools.iter().find(|t| t.name == name).unwrap();
        Value::Object((*tool.input_schema).clone())
    };
    assert_eq!(schema("trash_note")["required"], json!(["note"]));
    assert_eq!(schema("move_note")["required"], json!(["note", "binder"]));
    assert_eq!(
        schema("merge_notes")["required"],
        json!(["notes", "layout"])
    );
    assert!(schema("create_note")["properties"]["paper"].is_object());
    assert!(schema("split_note")["properties"]["polygon"].is_object());
    // The 11 formats, as the plain `enum` of the ones without a comment and one entry for each
    // of the others.
    let formats = &schema("export_note")["$defs"]["ExportFormat"]["oneOf"];
    let count: usize = formats
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["enum"].as_array().map_or(1, Vec::len))
        .sum();
    assert_eq!(count, 11);
}

#[tokio::test(flavor = "current_thread")]
async fn a_read_only_server_lists_no_write_tool_and_refuses_each_call() {
    // The router itself: the write group is not there, and is there in a full server.
    let read_only: Vec<String> = build_tool_router(ServerOptions { read_only: true })
        .list_all()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    let full: Vec<String> = build_tool_router(ServerOptions::default())
        .list_all()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    for name in WRITE_TOOLS {
        assert!(!read_only.iter().any(|n| n == name), "{name} in read-only");
        assert!(full.iter().any(|n| n == name), "{name} missing");
    }
    assert_eq!(read_only.len(), 7);

    // Through the protocol: not listed, and a call fails without touching the library.
    let session = session_with(Language::En, ServerOptions { read_only: true }).await;
    let listed = session.client.list_all_tools().await.unwrap();
    for name in WRITE_TOOLS {
        assert!(!listed.iter().any(|t| t.name == name), "{name} is listed");
    }
    let before = titles(&session.harness.library());
    let id = OPTIQUE.to_owned();
    let calls = [
        ("create_note", json!({ "title": "x" })),
        ("update_note", json!({ "note": id, "title": "x" })),
        ("move_note", json!({ "note": id, "binder": "x" })),
        ("trash_note", json!({ "note": id })),
        (
            "merge_notes",
            json!({ "notes": [id, CHIMIE], "layout": "overlay" }),
        ),
        ("split_note", json!({ "note": id, "at_ms": 100 })),
        ("straighten_shapes", json!({ "note": id })),
        (
            "export_note",
            json!({ "notes": [id], "format": "png", "path": "C:\\x" }),
        ),
        ("set_note_text", json!({ "note": id, "text": "x" })),
    ];
    for (name, arguments) in calls {
        let outcome = session
            .client
            .call_tool(
                CallToolRequestParams::new(name)
                    .with_arguments(arguments.as_object().cloned().unwrap()),
            )
            .await;
        match outcome {
            Err(_) => {}
            Ok(result) => assert_eq!(result.is_error, Some(true), "{name} was accepted"),
        }
    }
    assert_eq!(titles(&session.harness.library()), before);
    assert!(session.harness.write.exports.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_host_without_write_services_fails_with_a_code() {
    let harness = Harness::new(Language::En);
    let host = Host::new(
        Arc::new(Env {
            root: harness.root.clone(),
            hidden: None,
            language: Language::En,
        }),
        harness.fake.clone(),
    );
    let session = session_on(harness, host, ServerOptions::default()).await;
    for (tool, arguments) in [
        ("create_note", json!({ "title": "x" })),
        (
            "export_note",
            json!({ "notes": [OPTIQUE], "format": "pdf", "path": std::env::temp_dir().to_string_lossy() }),
        ),
        ("set_note_text", json!({ "note": OPTIQUE, "text": "x" })),
        ("split_note", json!({ "note": OPTIQUE, "at_ms": 100 })),
    ] {
        let error = session.failure(tool, arguments).await;
        assert_eq!(error["code"], "writeUnavailable", "{tool}");
    }
    // An export that fails leaves no folder made for it.
    let out = export_dir();
    let nested = out.path().join("new").join("deeper");
    let error = session
        .failure(
            "export_note",
            json!({ "notes": [OPTIQUE], "format": "md", "path": nested.to_string_lossy() }),
        )
        .await;
    assert_eq!(error["code"], "writeUnavailable");
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    // The tools that are the library's own need no service.
    session
        .ok("update_note", json!({ "note": OPTIQUE, "favorite": false }))
        .await;
}

// ---------------------------------------------------------------------------------------------
// create_note

#[tokio::test(flavor = "current_thread")]
async fn a_blank_note_is_made_like_the_apps_new_note() {
    let session = session().await;
    let created = session.ok("create_note", json!({})).await;
    let note = &created["note"];
    assert_eq!(note["title"], "");
    assert_eq!(note["binder"], "");
    assert_eq!(note["page"]["width_mm"], 297.0);
    assert_eq!(note["page"]["height_mm"], 216.0);
    assert_eq!(note["stroke_count"], 0);
    let loaded = session
        .harness
        .library()
        .read(&session.harness.root.join(note["path"].as_str().unwrap()))
        .unwrap();
    assert_eq!(loaded.file.note.layers[0].name, "Layer 1");
    assert_eq!(loaded.file.note.id, note["id"].as_str().unwrap());

    // French app, French name of the first layer.
    let fr = session_with(Language::Fr, ServerOptions::default()).await;
    let made = fr.ok("create_note", json!({ "title": "Séance" })).await;
    let loaded = fr
        .harness
        .library()
        .read(&fr.harness.root.join(made["note"]["path"].as_str().unwrap()))
        .unwrap();
    assert_eq!(loaded.file.note.layers[0].name, "Calque 1");
}

#[tokio::test(flavor = "current_thread")]
async fn a_note_takes_its_title_binder_tags_paper_and_template() {
    let session = session().await;
    let created = session
        .ok(
            "create_note",
            json!({
                "title": "  Électricité  ",
                "binder": "/Classe/3e B/",
                "tags": ["physique", "Physique", " 3e B "],
                "paper": "a4",
                "orientation": "landscape",
                "template": "seyes",
            }),
        )
        .await;
    let note = &created["note"];
    assert_eq!(note["title"], "Électricité");
    assert_eq!(note["binder"], "Classe/3e B");
    assert_eq!(note["tags"], json!(["physique", "3e B"]));
    assert_eq!(note["page"]["width_mm"], 297.0);
    assert_eq!(note["page"]["height_mm"], 210.0);
    let loaded = session
        .harness
        .library()
        .read(&session.harness.root.join(note["path"].as_str().unwrap()))
        .unwrap();
    assert_eq!(
        loaded.file.note.background.template,
        ardoise_core::note::Template::Seyes
    );

    // A custom page, in millimetres.
    let custom = session
        .ok(
            "create_note",
            json!({ "paper": { "width": 100.5, "height": 50 }, "orientation": "portrait" }),
        )
        .await;
    assert_eq!(custom["note"]["page"]["width_mm"], 50.0);
    assert_eq!(custom["note"]["page"]["height_mm"], 100.5);
    // The same title twice: the file name takes " (2)", nothing is replaced.
    let again = session
        .ok(
            "create_note",
            json!({ "title": "Électricité", "binder": "Classe/3e B" }),
        )
        .await;
    assert_ne!(again["note"]["path"], created["note"]["path"]);
    assert!(
        again["note"]["path"]
            .as_str()
            .unwrap()
            .ends_with("(2).ardoise")
    );
    assert!(strays(&session.harness.root).is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_note_can_be_made_on_a_file_that_is_given_by_an_absolute_path() {
    let session = session().await;
    let dir = tempfile::tempdir().unwrap();
    let pdf = dir.path().join("fiche.pdf");
    fs::write(&pdf, b"%PDF-1.4 fake").unwrap();

    let created = session
        .ok(
            "create_note",
            json!({
                "from_file": pdf.to_string_lossy(),
                "pdf_page": 3,
                "title": "Fiche",
                "tags": ["à faire"],
                "binder": "Classe",
            }),
        )
        .await;
    assert_eq!(created["note"]["title"], "Fiche");
    assert_eq!(created["note"]["tags"], json!(["à faire"]));
    assert_eq!(created["note"]["binder"], "Classe");
    let imports = session.harness.write.imports.lock().unwrap().clone();
    assert_eq!(imports, [(pdf.clone(), 3)]);

    // Relative, missing, and arguments that do not go with a file: refused, nothing created.
    let count = session.harness.library().list().notes.len();
    for arguments in [
        json!({ "from_file": "fiche.pdf" }),
        json!({ "from_file": dir.path().join("absent.pdf").to_string_lossy() }),
        json!({ "from_file": pdf.to_string_lossy(), "paper": "a5" }),
        json!({ "from_file": pdf.to_string_lossy(), "template": "lined" }),
        json!({ "pdf_page": 2 }),
        json!({ "paper": { "width": 5, "height": 100 } }),
    ] {
        let error = session.failure("create_note", arguments.clone()).await;
        assert_eq!(error["code"], "invalidArguments", "{arguments}: {error}");
    }
    assert_eq!(session.harness.library().list().notes.len(), count);
}

// ---------------------------------------------------------------------------------------------
// update_note, move_note, trash_note

#[tokio::test(flavor = "current_thread")]
async fn metadata_changes_in_one_atomic_save_and_the_ink_is_kept() {
    let session = session().await;
    let library = session.harness.library();
    let before = library.read(&session.harness.path_of(CHIMIE)).unwrap();

    let updated = session
        .ok(
            "update_note",
            json!({
                "note": CHIMIE,
                "title": "Chimie – les métaux",
                "tags": ["Chimie", "cours"],
                "add_tags": ["3e", "chimie"],
                "remove_tags": ["COURS"],
                "favorite": true,
            }),
        )
        .await;
    assert_eq!(updated["changed"], json!(["tags", "favorite", "title"]));
    let note = &updated["note"];
    assert_eq!(note["title"], "Chimie – les métaux");
    assert_eq!(note["tags"], json!(["Chimie", "3e"]));
    assert_eq!(note["favorite"], true);
    // The file was renamed to match the title, in the same binder.
    assert!(
        note["path"].as_str().unwrap().starts_with("Physique/")
            && note["path"]
                .as_str()
                .unwrap()
                .contains("Chimie – les métaux"),
        "{note}"
    );
    let after = library.read(&session.harness.path_of(CHIMIE)).unwrap();
    assert_eq!(after.file.note.strokes, before.file.note.strokes);
    assert_eq!(after.file.note.id, CHIMIE);
    assert!(after.file.note.modified >= before.file.note.modified);
    assert!(strays(&session.harness.root).is_empty());

    // The same call again changes nothing (idempotent), and an empty title untitles the note.
    let same = session
        .ok(
            "update_note",
            json!({ "note": CHIMIE, "title": "Chimie – les métaux", "favorite": true }),
        )
        .await;
    assert_eq!(same["changed"], json!([]));
    let untitled = session
        .ok("update_note", json!({ "note": CHIMIE, "title": "" }))
        .await;
    assert_eq!(untitled["note"]["title"], "");

    // Nothing to change, or no such note: coded errors.
    let nothing = session
        .failure("update_note", json!({ "note": CHIMIE }))
        .await;
    assert_eq!(nothing["code"], "invalidArguments");
    let missing = session
        .failure(
            "update_note",
            json!({ "note": "Absente/x.ardoise", "favorite": true }),
        )
        .await;
    assert_eq!(missing["code"], "noteNotFound");
}

#[tokio::test(flavor = "current_thread")]
async fn a_note_moves_to_a_binder_and_never_replaces_a_note_there() {
    let session = session().await;
    let moved = session
        .ok(
            "move_note",
            json!({ "note": CHIMIE, "binder": "/Archives/2026/" }),
        )
        .await;
    assert_eq!(moved["note"]["binder"], "Archives/2026");
    assert_eq!(moved["moved"], true);
    assert_eq!(moved["renamed"], false);
    assert!(session.harness.root.join("Archives/2026").is_dir());
    // Again: already there, nothing happens.
    let again = session
        .ok(
            "move_note",
            json!({ "note": CHIMIE, "binder": "Archives/2026" }),
        )
        .await;
    assert_eq!(again["moved"], false);

    // A note of the same file name already there: the moved one gets " (2)".
    let twin = session
        .ok("create_note", json!({ "title": "Chimie des métaux" }))
        .await;
    let twin_id = twin["note"]["id"].as_str().unwrap().to_owned();
    // Same creation day as CHIMIE, so the same file name.
    let library = session.harness.library();
    let twin_path = session.harness.path_of(&twin_id);
    let renamed = library
        .retitle(&twin_path, "Chimie des métaux", None)
        .unwrap();
    assert!(renamed.path.exists());
    let moved_twin = session
        .ok(
            "move_note",
            json!({ "note": twin_id, "binder": "Archives/2026" }),
        )
        .await;
    assert_eq!(moved_twin["note"]["binder"], "Archives/2026");
    let in_archives: Vec<_> = fs::read_dir(session.harness.root.join("Archives/2026"))
        .unwrap()
        .flatten()
        .collect();
    assert_eq!(in_archives.len(), 2, "no note was replaced");

    // To the root, and out of the library: refused.
    let root = session
        .ok("move_note", json!({ "note": CHIMIE, "binder": "" }))
        .await;
    assert_eq!(root["note"]["binder"], "");
    for binder in ["../dehors", "a:b", "_ardoise"] {
        let error = session
            .failure("move_note", json!({ "note": CHIMIE, "binder": binder }))
            .await;
        assert_eq!(error["code"], "invalidName", "{binder}: {error}");
    }
    assert!(strays(&session.harness.root).is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn a_trashed_note_can_be_restored() {
    let session = session().await;
    let library = session.harness.library();
    let path = session.harness.path_of(DEVOIR);
    let bytes = fs::read(&path).unwrap();

    let trashed = session.ok("trash_note", json!({ "note": DEVOIR })).await;
    assert_eq!(trashed["trashed"]["title"], "Devoir surveillé");
    assert!(!path.exists());
    assert!(library.list().notes.iter().all(|n| n.id != DEVOIR));
    let items = library.trash_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, trashed["trash_id"].as_str().unwrap());

    // The app's restore puts the very same file back.
    let restored = library.restore(&items[0].id).unwrap();
    assert_eq!(restored, path);
    assert_eq!(fs::read(&path).unwrap(), bytes);

    // Trashing something that is not there is a coded error.
    let again = session
        .failure("trash_note", json!({ "note": "Inconnue" }))
        .await;
    assert_eq!(again["code"], "noteNotFound");
}

// ---------------------------------------------------------------------------------------------
// merge_notes, split_note

#[tokio::test(flavor = "current_thread")]
async fn merging_gives_a_new_note_in_the_first_notes_binder() {
    let session = session().await;
    let before = fs::read(session.harness.path_of(OPTIQUE)).unwrap();
    let merged = session
        .ok(
            "merge_notes",
            json!({ "notes": [OPTIQUE, CHIMIE], "layout": "endToEnd" }),
        )
        .await;
    let note = &merged["note"];
    assert_eq!(merged["sources"], 2);
    assert_eq!(note["binder"], "Physique/4e");
    assert_eq!(note["title"], "Cours d'optique (assembled)");
    // Two A4 pages one below the other, all the strokes.
    assert_eq!(note["page"]["width_mm"], 297.0);
    assert_eq!(note["page"]["height_mm"], 432.0);
    assert_eq!(note["stroke_count"], 3 + 4);
    assert_eq!(fs::read(session.harness.path_of(OPTIQUE)).unwrap(), before);
    assert_eq!(session.harness.library().list().notes.len(), 5);

    let titled = session
        .ok(
            "merge_notes",
            json!({ "notes": [CHIMIE, OPTIQUE], "layout": "overlay", "title": "Bilan" }),
        )
        .await;
    assert_eq!(titled["note"]["title"], "Bilan");
    assert_eq!(titled["note"]["binder"], "Physique");
    assert_eq!(titled["note"]["page"]["height_mm"], 216.0);

    // French app: the suffix of the app's own proposal.
    let fr = session_with(Language::Fr, ServerOptions::default()).await;
    let made = fr
        .ok(
            "merge_notes",
            json!({ "notes": [VIDE, DEVOIR], "layout": "overlay" }),
        )
        .await;
    assert!(
        made["note"]["title"]
            .as_str()
            .unwrap()
            .ends_with("(assemblage)"),
        "{made}"
    );

    let single = session
        .failure(
            "merge_notes",
            json!({ "notes": [OPTIQUE], "layout": "overlay" }),
        )
        .await;
    assert_eq!(single["code"], "nothingToMerge");
    let bad = session
        .failure(
            "merge_notes",
            json!({ "notes": [OPTIQUE, "Inconnue"], "layout": "overlay" }),
        )
        .await;
    assert_eq!(bad["code"], "noteNotFound");
    let layout = session
        .failure(
            "merge_notes",
            json!({ "notes": [OPTIQUE, CHIMIE], "layout": "stack" }),
        )
        .await;
    assert_eq!(layout["code"], "invalidArguments");
}

#[tokio::test(flavor = "current_thread")]
async fn a_note_is_split_at_an_instant_or_by_a_zone() {
    let session = session().await;
    // CHIMIE: three waves written in the first 800 ms, a circle between 2000 and 2600 ms.
    let split = session
        .ok("split_note", json!({ "note": CHIMIE, "at_ms": 1500 }))
        .await;
    assert_eq!(split["kept"]["stroke_count"], 3);
    assert_eq!(split["created"]["stroke_count"], 1);
    assert_eq!(split["created"]["binder"], "Physique");
    assert_eq!(split["created"]["title"], "Chimie des métaux (continued)");
    assert_eq!(split["kept"]["id"], CHIMIE);
    assert_ne!(split["created"]["id"], CHIMIE);

    // By zone: the polygon (millimetres) takes the first wave of the optics note only.
    let zone = session
        .ok(
            "split_note",
            json!({
                "note": OPTIQUE,
                "polygon": [[0, 40], [297, 40], [297, 60], [0, 60]],
                "title": "Extrait",
            }),
        )
        .await;
    assert_eq!(zone["created"]["title"], "Extrait");
    assert_eq!(zone["created"]["stroke_count"], 1);
    assert_eq!(zone["kept"]["stroke_count"], 2);

    // Bad requests: both or none of the two, a polygon without area, nothing to split.
    for arguments in [
        json!({ "note": OPTIQUE }),
        json!({ "note": OPTIQUE, "at_ms": 10, "polygon": [[0, 0], [1, 0], [1, 1]] }),
        json!({ "note": OPTIQUE, "polygon": [[0, 0], [1, 0]] }),
    ] {
        let error = session.failure("split_note", arguments.clone()).await;
        assert_eq!(error["code"], "invalidArguments", "{arguments}");
    }
    let nothing = session
        .failure("split_note", json!({ "note": DEVOIR, "at_ms": 10_000_000 }))
        .await;
    assert_eq!(nothing["code"], "nothingToSplit");
    assert!(strays(&session.harness.root).is_empty());
}

// ---------------------------------------------------------------------------------------------
// straighten_shapes

/// The note `id` as it is in its file.
fn note_of(session: &Session, id: &str) -> ardoise_core::note::Note {
    let path = session.harness.path_of(id);
    session.harness.library().read(&path).unwrap().file.note
}

#[tokio::test(flavor = "current_thread")]
async fn drawn_shapes_are_straightened_in_one_save_and_the_version_before_is_kept() {
    let session = session().await;
    let path = session.harness.path_of(CHIMIE);
    let library = session.harness.library();
    let loaded = library.read(&path).unwrap();
    let before = loaded.file.note.strokes.clone();
    assert_eq!(before.len(), 4);
    // The ink analysis finds the circle (3) as a drawing; the waves are handwriting, and one of
    // them (2) is on a hidden layer: neither is tried by default.
    session
        .harness
        .fake
        .drawings
        .lock()
        .unwrap()
        .insert(CHIMIE.into(), vec![2, 3]);

    let result = session
        .ok("straighten_shapes", json!({ "note": CHIMIE }))
        .await;
    assert_eq!(result["chosen_by"], "ink_analysis");
    assert_eq!(result["strokes_tried"], json!([3]), "{result}");
    assert_eq!(result["straightened"], 1, "{result}");
    assert_eq!(result["saved"], true);
    assert_eq!(*session.harness.write.saves.lock().unwrap(), 1);
    let shapes = result["shapes"].as_array().unwrap();
    assert!(
        shapes
            .iter()
            .any(|s| s["shape"] == "ellipse" && s["strokes"] == json!([3]))
    );
    // The circle was replaced; the order of the other strokes is kept.
    let after = library.read(&path).unwrap().file.note.strokes;
    assert_eq!(after[..3], before[..3]);
    assert_ne!(after[3], before[3]);
    assert_eq!(after[3].layer, before[3].layer);
    assert_eq!(after[3].tool, before[3].tool);
    assert!(strays(&session.harness.root).is_empty());

    // The version before is in the trash, as a note of its own that the app restores next to
    // the note.
    let kept = &result["previous_version"];
    let items = library.trash_items().unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, kept["in_trash"].as_str().unwrap());
    assert_eq!(
        items[0].title.as_deref(),
        Some("Chimie des métaux (before straightening)")
    );
    assert_eq!(kept["title"], "Chimie des métaux (before straightening)");
    assert!(kept["restore"].as_str().unwrap().contains("30 days"));
    let copy_id = kept["id"].as_str().unwrap();
    assert_ne!(copy_id, CHIMIE, "a new id: it never stands for the note");
    assert_eq!(items[0].note_id.as_deref(), Some(copy_id));
    let restored = library.restore(&items[0].id).unwrap();
    assert_eq!(restored.parent(), path.parent());
    let file = library.read(&restored).unwrap().file;
    assert_eq!(file.note.strokes, before);
    assert_eq!(file.note.id, copy_id);
    assert_eq!(file.note.tags, loaded.file.note.tags);
    assert_eq!(
        session.harness.path_of(CHIMIE),
        path,
        "the note itself stays"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn the_version_before_straightening_is_named_in_the_apps_language() {
    let session = session_with(Language::Fr, ServerOptions::default()).await;
    session
        .harness
        .fake
        .drawings
        .lock()
        .unwrap()
        .insert(CHIMIE.into(), vec![3]);
    let result = session
        .ok("straighten_shapes", json!({ "note": CHIMIE }))
        .await;
    assert_eq!(
        result["previous_version"]["title"],
        "Chimie des métaux (avant redressement)"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn without_the_ink_analysis_the_strokes_must_be_given() {
    let session = session().await;
    let path = session.harness.path_of(CHIMIE);
    let before = fs::read(&path).unwrap();
    // No analysis (the fake's heuristics): the default is refused, nothing is written.
    let error = session
        .failure("straighten_shapes", json!({ "note": CHIMIE }))
        .await;
    assert_eq!(error["code"], "invalidArguments");
    let message = error["message"].as_str().unwrap();
    assert!(
        message.contains("ink analysis") && message.contains("`strokes`"),
        "{message}"
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(session.harness.library().trash_items().unwrap().is_empty());

    // Given, they are tried whatever the analysis.
    let result = session
        .ok(
            "straighten_shapes",
            json!({ "note": CHIMIE, "strokes": [3] }),
        )
        .await;
    assert_eq!(result["chosen_by"], "given");
    assert_eq!(result["straightened"], 1);
    assert_eq!(session.harness.library().trash_items().unwrap().len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn straightening_only_the_chosen_strokes_and_leaving_the_rest_alone() {
    let session = session().await;
    let path = session.harness.path_of(CHIMIE);
    let before = fs::read(&path).unwrap();

    // The waves are no shape: nothing changes, nothing is saved, nothing is kept.
    let none = session
        .ok(
            "straighten_shapes",
            json!({ "note": CHIMIE, "strokes": [0, 1, 2] }),
        )
        .await;
    assert_eq!(none["straightened"], 0);
    assert_eq!(none["saved"], false);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(*session.harness.write.saves.lock().unwrap(), 0);
    assert!(session.harness.library().trash_items().unwrap().is_empty());
    // A note whose analysis finds no drawing: the same.
    session
        .harness
        .fake
        .drawings
        .lock()
        .unwrap()
        .insert(OPTIQUE.into(), Vec::new());
    let none = session
        .ok("straighten_shapes", json!({ "note": OPTIQUE }))
        .await;
    assert_eq!(none["strokes_tried"], json!([]));
    assert_eq!(none["saved"], false);

    let error = session
        .failure(
            "straighten_shapes",
            json!({ "note": CHIMIE, "strokes": [4] }),
        )
        .await;
    assert_eq!(error["code"], "invalidArguments");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(note_of(&session, CHIMIE).strokes.len(), 4);
}

#[tokio::test(flavor = "current_thread")]
async fn a_note_changed_meanwhile_is_not_overwritten() {
    let session = session().await;
    let path = session.harness.path_of(CHIMIE);
    let before = session
        .harness
        .library()
        .read(&path)
        .unwrap()
        .file
        .note
        .strokes;
    // Another PC saves the note between the tool's read and its write.
    let root = session.harness.root.clone();
    *session.harness.write.before_save.lock().unwrap() = Some(Box::new(move |note| {
        Library::open(&root)
            .unwrap()
            .set_favorite(note, true, None)
            .unwrap();
    }));
    let error = session
        .failure(
            "straighten_shapes",
            json!({ "note": CHIMIE, "strokes": [3] }),
        )
        .await;
    assert_eq!(error["code"], "noteConflict", "{error}");
    // The other version is still there, untouched by the tool.
    let kept = session.harness.library().read(&path).unwrap().file.note;
    assert!(kept.favorite);
    assert_eq!(kept.strokes, before, "the circle was not replaced");
    assert!(strays(&session.harness.root).is_empty());
    // The copy made before the save is taken back: nothing was replaced.
    assert!(session.harness.library().trash_items().unwrap().is_empty());
}

// ---------------------------------------------------------------------------------------------
// export_note

fn export_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

fn files(value: &Value) -> Vec<PathBuf> {
    value["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| PathBuf::from(f.as_str().unwrap()))
        .collect()
}

#[tokio::test(flavor = "current_thread")]
async fn an_export_to_a_folder_is_named_after_the_note_and_never_overwrites() {
    let session = session().await;
    let out = export_dir();
    let folder = out.path().join("Exports du jour");
    let arguments = json!({
        "notes": [OPTIQUE],
        "format": "png",
        "path": folder.to_string_lossy(),
    });
    let first = files(&session.ok("export_note", arguments.clone()).await);
    let second = files(&session.ok("export_note", arguments.clone()).await);
    let third = files(&session.ok("export_note", arguments).await);
    let stem = "2026-10-24 Cours d'optique";
    assert_eq!(first, [folder.join(format!("{stem}.png"))]);
    assert_eq!(second, [folder.join(format!("{stem} (2).png"))]);
    assert_eq!(third, [folder.join(format!("{stem} (3).png"))]);
    for file in first.iter().chain(&second).chain(&third) {
        assert!(file.is_file());
    }
    // The request the app gets carries the defaults of the app's export dialog.
    let request = session.harness.write.exports.lock().unwrap()[0].clone();
    assert_eq!(request.options.dpi, 150);
    assert_eq!(request.options.quality, 90);
    assert!(request.options.background);
    assert_eq!(request.notes, [session.harness.path_of(OPTIQUE)]);
    assert!(matches!(request.target, ExportTarget::File(_)));
}

#[tokio::test(flavor = "current_thread")]
async fn an_export_to_a_file_takes_the_name_asked_and_its_options() {
    let session = session().await;
    let out = export_dir();
    let file = out.path().join("sous").join("optique.jpg");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, b"precious").unwrap();
    let result = session
        .ok(
            "export_note",
            json!({
                "notes": [OPTIQUE],
                "format": "jpg",
                "path": file.to_string_lossy(),
                "dpi": 300,
                "quality": 70,
                "background": false,
            }),
        )
        .await;
    assert_eq!(
        files(&result),
        [out.path().join("sous").join("optique (2).jpg")]
    );
    assert_eq!(
        fs::read(&file).unwrap(),
        b"precious",
        "the file is untouched"
    );
    assert_eq!(result["format"], "jpg");
    assert_eq!(result["details"]["fake"], true);
    let request = session.harness.write.exports.lock().unwrap()[0].clone();
    assert_eq!(request.format, ExportFormat::Jpg);
    assert_eq!(
        (
            request.options.dpi,
            request.options.quality,
            request.options.background
        ),
        (300, 70, false)
    );

    // A folder that does not exist is created; a text export takes the language of the app, or
    // the one asked.
    let md = out.path().join("md");
    let result = session
        .ok(
            "export_note",
            json!({ "notes": [OPTIQUE, CHIMIE], "format": "md", "path": md.to_string_lossy(), "language": "fr" }),
        )
        .await;
    assert_eq!(files(&result), [md.join("Ardoise export.md")]);
    let request = session.harness.write.exports.lock().unwrap()[1].clone();
    assert_eq!(request.options.language, Language::Fr);
    assert_eq!(request.notes.len(), 2);

    // A Markdown name whose drawings' folder is taken (a previous export's, its `.md` gone)
    // moves on: the app would put the drawings among the other export's.
    fs::create_dir(md.join("cours_images")).unwrap();
    let result = session
        .ok(
            "export_note",
            json!({ "notes": [OPTIQUE], "format": "md", "path": md.join("cours.md").to_string_lossy() }),
        )
        .await;
    assert_eq!(files(&result), [md.join("cours (2).md")]);
}

#[tokio::test(flavor = "current_thread")]
async fn several_images_go_to_a_folder_and_a_pdf_to_one_file() {
    let session = session().await;
    let out = export_dir();
    let result = session
        .ok(
            "export_note",
            json!({ "notes": [OPTIQUE, CHIMIE, DEVOIR], "format": "webp", "path": out.path().to_string_lossy() }),
        )
        .await;
    let written = files(&result);
    assert_eq!(written.len(), 3);
    assert_eq!(
        written.iter().collect::<HashSet<_>>().len(),
        3,
        "one file per note"
    );
    let request = session.harness.write.exports.lock().unwrap()[0].clone();
    assert!(matches!(request.target, ExportTarget::Folder(_)));

    let pdf = out.path().join("classe.pdf");
    let result = session
        .ok(
            "export_note",
            json!({ "notes": [OPTIQUE, CHIMIE], "format": "pdf", "path": pdf.to_string_lossy(), "pdf_page": "a4" }),
        )
        .await;
    assert_eq!(files(&result), [pdf]);
}

#[tokio::test(flavor = "current_thread")]
async fn bad_export_requests_are_refused_before_anything_is_written() {
    let session = session().await;
    let out = export_dir();
    let dir = out.path().to_string_lossy().into_owned();
    let cases = [
        // Relative paths.
        json!({ "notes": [OPTIQUE], "format": "png", "path": "exports/a.png" }),
        json!({ "notes": [OPTIQUE], "format": "png", "path": "a.png" }),
        // A file name that does not fit the format; several images in one file.
        json!({ "notes": [OPTIQUE], "format": "png", "path": out.path().join("a.jpg").to_string_lossy() }),
        json!({ "notes": [OPTIQUE, CHIMIE], "format": "png", "path": out.path().join("a.png").to_string_lossy() }),
        // Options out of range; a replay of several notes.
        json!({ "notes": [OPTIQUE], "format": "png", "path": dir, "dpi": 20 }),
        json!({ "notes": [OPTIQUE], "format": "jpg", "path": dir, "quality": 0 }),
        json!({ "notes": [OPTIQUE, CHIMIE], "format": "mp4", "path": dir }),
        json!({ "notes": [OPTIQUE], "format": "gif", "path": dir, "speed": 2, "duration_seconds": 10 }),
        // Replays beyond their limits (the assistant cannot cancel one).
        json!({ "notes": [OPTIQUE], "format": "webm", "path": dir, "duration_seconds": 3600 }),
        json!({ "notes": [OPTIQUE], "format": "webm", "path": dir, "speed": 1000 }),
        json!({ "notes": [OPTIQUE], "format": "webm", "path": dir, "frame_rate": 1000 }),
        json!({ "notes": [OPTIQUE], "format": "gif", "path": dir, "hold_end_seconds": 3600 }),
        json!({ "notes": [], "format": "png", "path": dir }),
        // An unknown format.
        json!({ "notes": [OPTIQUE], "format": "bmp", "path": dir }),
    ];
    for arguments in cases {
        let error = session.failure("export_note", arguments.clone()).await;
        assert_eq!(error["code"], "invalidArguments", "{arguments}: {error}");
    }
    let missing = session
        .failure(
            "export_note",
            json!({ "notes": ["Inconnue"], "format": "png", "path": dir }),
        )
        .await;
    assert_eq!(missing["code"], "noteNotFound");
    assert!(session.harness.write.exports.lock().unwrap().is_empty());
    assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_replay_is_one_note_with_its_options() {
    let session = session().await;
    let out = export_dir();
    let result = session
        .ok(
            "export_note",
            json!({
                "notes": [CHIMIE],
                "format": "mp4",
                "path": out.path().to_string_lossy(),
                "resolution": "720p",
                "frame_rate": 24,
                "duration_seconds": 12.5,
                "trim_pauses": true,
                "hold_end_seconds": 1,
                "narration": false,
            }),
        )
        .await;
    assert_eq!(
        files(&result),
        [out.path().join("2026-10-26 Chimie des métaux.mp4")]
    );
    let request = session.harness.write.exports.lock().unwrap()[0].clone();
    let video = request.options.video;
    assert_eq!(video.frame_rate, 24);
    assert_eq!(
        video.timing,
        ardoise_mcp::write::VideoTiming::Duration(12.5)
    );
    assert!(video.trim_pauses && !video.narration);
    assert_eq!(video.resolution, ardoise_mcp::write::VideoResolution::Hd);
    assert_eq!(video.hold_end_seconds, 1.0);
}

// ---------------------------------------------------------------------------------------------
// set_note_text

#[tokio::test(flavor = "current_thread")]
async fn a_corrected_text_is_used_by_the_search_and_returned_flagged() {
    let session = session().await;
    let path = session.harness.path_of(OPTIQUE);
    let before = fs::read(&path).unwrap();

    // Before: the recogniser's text finds "lumière", not the corrected word.
    let found = session
        .ok("search_notes", json!({ "query": "refraction" }))
        .await;
    assert_eq!(found["total"], 0);
    let plain = session
        .ok("get_note_text", json!({ "note": OPTIQUE }))
        .await;
    assert_eq!(plain["is_correction"], false);

    let stored = session
        .ok(
            "set_note_text",
            json!({ "note": OPTIQUE, "text": "Cours d'optique\r\nLa réfraction dévie la lumière\n" }),
        )
        .await;
    assert_eq!(stored["stored"], true);
    assert_eq!(stored["removed"], false);

    let found = session
        .ok("search_notes", json!({ "query": "refraction" }))
        .await;
    assert_eq!(found["total"], 1);
    assert_eq!(found["notes"][0]["id"], OPTIQUE);
    assert!(
        found["notes"][0]["matched_in"]
            .as_array()
            .unwrap()
            .contains(&json!("text"))
    );
    // The correction replaces the recogniser's words in the search.
    let old = session
        .ok("search_notes", json!({ "query": "propage" }))
        .await;
    assert_eq!(old["total"], 0);

    let text = session
        .ok("get_note_text", json!({ "note": OPTIQUE }))
        .await;
    assert_eq!(text["is_correction"], true);
    assert_eq!(
        text["text"],
        "Cours d'optique\nLa réfraction dévie la lumière"
    );
    assert!(
        text["recognised_text"]
            .as_str()
            .unwrap()
            .contains("se propage")
    );
    // The lines and the structure, with their positions, stay the recogniser's.
    assert!(text["lines"][0]["bounds_mm"]["width"].as_f64().unwrap() > 0.0);

    // Never written into the note.
    assert_eq!(fs::read(&path).unwrap(), before);

    // An empty text removes the correction.
    let removed = session
        .ok("set_note_text", json!({ "note": OPTIQUE, "text": "  " }))
        .await;
    assert_eq!(removed["removed"], true);
    let back = session
        .ok("get_note_text", json!({ "note": OPTIQUE }))
        .await;
    assert_eq!(back["is_correction"], false);
    assert_eq!(
        session
            .ok("search_notes", json!({ "query": "refraction" }))
            .await["total"],
        0
    );

    let missing = session
        .failure("set_note_text", json!({ "note": "Inconnue", "text": "x" }))
        .await;
    assert_eq!(missing["code"], "noteNotFound");
    let huge = session
        .failure(
            "set_note_text",
            json!({ "note": OPTIQUE, "text": "x".repeat(200_001) }),
        )
        .await;
    assert_eq!(huge["code"], "invalidArguments");
}
