//! The read tools, through rmcp's in-memory transport on a temporary library of synthetic notes
//! and a fake handwriting service: schemas listed, arguments validated, errors coded.

#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ardoise_core::format::NoteFile;
use ardoise_core::library::{self, Library};
use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Note, Origin, PageSize, Timing};
use ardoise_hwr::structure::{Structure, from_recognition, ink_boxes};
use ardoise_hwr::{Bounds, HwrError, Language, Line, Recognition, Word};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_mcp::{
    Environment, Handwriting, Host, ServerOptions, TextHit, TextSearch, TextStatus, ToolError,
    serve,
};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RoleClient, RunningService};
use serde_json::{Value, json};

/// 2026-10-24 at noon UTC.
const DAY0: i64 = 1_792_843_200_000;
const DAY_MS: i64 = 86_400_000;

// ---------------------------------------------------------------------------------------------
// Fakes

struct Env {
    root: PathBuf,
    hidden: Option<String>,
}

impl Environment for Env {
    fn library(&self) -> Result<Library, ToolError> {
        Ok(Library::open(&self.root)?)
    }

    fn hidden_binder(&self) -> Option<String> {
        self.hidden.clone()
    }

    fn language(&self) -> Language {
        Language::Fr
    }
}

#[derive(Default)]
struct Fake {
    /// Lines of text by note id (what the "recognizer" reads).
    by_id: Mutex<HashMap<String, Vec<String>>>,
    /// The same by absolute path (what the search finds).
    by_path: Mutex<HashMap<PathBuf, Vec<String>>>,
    unsupported: bool,
    unread: usize,
    /// The languages asked for.
    asked: Mutex<Vec<Option<Language>>>,
}

fn fold(text: &str) -> String {
    ardoise_hwr::text::fold(text)
}

impl Handwriting for Fake {
    fn recognition_of(
        &self,
        note: &Note,
        language: Option<Language>,
    ) -> Result<Arc<Recognition>, HwrError> {
        self.asked.lock().unwrap().push(language);
        if self.unsupported {
            return Err(HwrError::Unsupported);
        }
        let lines = self
            .by_id
            .lock()
            .unwrap()
            .get(&note.id)
            .cloned()
            .unwrap_or_default();
        let lines = lines
            .iter()
            .enumerate()
            .map(|(row, text)| {
                Line::new(
                    text.split(' ')
                        .enumerate()
                        .map(|(column, word)| Word {
                            text: word.to_owned(),
                            alternatives: Vec::new(),
                            bounds: Bounds {
                                x: 1_000.0 + column as f32 * 1_500.0,
                                y: 1_000.0 + row as f32 * 2_000.0,
                                width: 1_400.0,
                                height: 600.0,
                            },
                        })
                        .collect(),
                )
            })
            .collect();
        Ok(Arc::new(Recognition {
            language: language.unwrap_or(Language::Fr),
            recognizer: "fake".into(),
            lines,
        }))
    }

    fn structure_of(&self, note: &Note, recognition: &Recognition) -> Structure {
        Structure {
            items: from_recognition(recognition, &ink_boxes(note)),
            analyzed: false,
        }
    }

    fn search(&self, query: &str) -> TextSearch {
        let words: Vec<String> = fold(query).split_whitespace().map(str::to_owned).collect();
        let mut hits = Vec::new();
        for (path, lines) in self.by_path.lock().unwrap().iter() {
            let mut found = Vec::new();
            let mut excerpt = None;
            for word in &words {
                for line in lines {
                    let tokens: Vec<&str> = line.split(' ').collect();
                    if let Some(at) = tokens
                        .iter()
                        .position(|t| fold(t).starts_with(word.as_str()))
                    {
                        found.push(word.clone());
                        excerpt.get_or_insert((
                            tokens[..at].join(" "),
                            tokens[at].to_owned(),
                            tokens[at + 1..].join(" "),
                        ));
                        break;
                    }
                }
            }
            if let Some((before, word, after)) = excerpt {
                hits.push(TextHit {
                    path: path.clone(),
                    words: found,
                    before,
                    word,
                    after,
                });
            }
        }
        TextSearch {
            hits,
            status: TextStatus {
                available: !self.unsupported,
                reason: self.unsupported.then(|| "unsupported".to_owned()),
                unread: self.unread,
            },
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Synthetic notes

fn wave(layer: u32, tool: Tool, y0: f32) -> Stroke {
    Stroke {
        tool,
        color: Rgba::rgb(0, 0, 255),
        width: 60.0,
        layer,
        points: (0..100)
            .map(|i| Point {
                x: 1_000.0 + i as f32 * 100.0,
                y: y0 + (i as f32 * 0.2).sin() * 400.0,
                pressure: 0.8,
                t: i as u32 * 8,
            })
            .collect(),
    }
}

fn make_note(id: &str, title: &str, day: i64) -> Note {
    Note {
        id: id.to_owned(),
        title: title.to_owned(),
        created: DAY0 + day * DAY_MS,
        modified: DAY0 + day * DAY_MS + 60_000,
        page: A4_LANDSCAPE,
        background: Background::default(),
        background_image: None,
        tags: Vec::new(),
        favorite: false,
        layers: vec![
            Layer {
                name: "Calque 1".into(),
                visible: true,
            },
            Layer {
                name: "Corrections".into(),
                visible: false,
            },
        ],
        strokes: vec![
            wave(0, Tool::Ballpoint, 5_000.0),
            wave(0, Tool::Pencil, 9_000.0),
            wave(1, Tool::Ballpoint, 13_000.0),
        ],
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    root: PathBuf,
    fake: Arc<Fake>,
}

const OPTIQUE: &str = "11111111-1111-4111-8111-111111111111";
const CHIMIE: &str = "22222222-2222-4222-8222-222222222222";
const DEVOIR: &str = "33333333-3333-4333-8333-333333333333";
const VIDE: &str = "44444444-4444-4444-8444-444444444444";

impl Harness {
    /// A library with four notes in two binders, a folder of exports, and a fake recogniser.
    fn new(unsupported: bool, unread: usize) -> Harness {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Bibliothèque");
        std::fs::create_dir(&root).unwrap();
        library::init(&root).unwrap();
        let library = Library::open(&root).unwrap();
        let fake = Arc::new(Fake {
            unsupported,
            unread,
            ..Fake::default()
        });

        let mut optique = make_note(OPTIQUE, "Cours d'optique", 0);
        optique.tags = vec!["Physique".into(), "Quatrième".into()];
        optique.favorite = true;
        let mut chimie = make_note(CHIMIE, "Chimie des métaux", 2);
        chimie.tags = vec!["chimie".into()];
        let mut devoir = make_note(DEVOIR, "Devoir surveillé", 5);
        devoir.tags = vec!["physique".into()];
        devoir.page = PageSize {
            width: 21_600,
            height: 29_700,
        };
        let mut vide = make_note(VIDE, "", 6);
        vide.strokes.clear();

        let put = |binder: &str, note: Note, lines: &[&str]| {
            let id = note.id.clone();
            let saved = library
                .create(Path::new(binder), &NoteFile::new(note))
                .unwrap();
            let lines: Vec<String> = lines.iter().map(|l| (*l).to_owned()).collect();
            fake.by_id.lock().unwrap().insert(id, lines.clone());
            if !lines.is_empty() {
                fake.by_path.lock().unwrap().insert(saved.path, lines);
            }
        };
        put(
            "Physique/4e",
            optique,
            &["Cours d'optique", "La lumière se propage en ligne droite"],
        );
        put("Physique", chimie, &["Les métaux conduisent le courant"]);
        put("", devoir, &["Exercice 1"]);
        put("", vide, &[]);
        library.create_binder(Path::new("Exports/PDF")).unwrap();

        Harness {
            _dir: dir,
            root,
            fake,
        }
    }

    fn host(&self) -> Host {
        Host::new(
            Arc::new(Env {
                root: self.root.clone(),
                hidden: Some("Exports".into()),
            }),
            self.fake.clone(),
        )
    }
}

struct Session {
    client: RunningService<RoleClient, ()>,
    harness: Harness,
}

async fn session_with(unsupported: bool, unread: usize, options: ServerOptions) -> Session {
    let harness = Harness::new(unsupported, unread);
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    let host = harness.host();
    tokio::spawn(async move {
        let _ = serve(host, options, server_io).await;
    });
    let client = ().serve(client_io).await.unwrap();
    Session { client, harness }
}

async fn session() -> Session {
    session_with(false, 0, ServerOptions::default()).await
}

impl Session {
    async fn call(&self, tool: &'static str, arguments: Value) -> CallToolResult {
        let arguments = arguments.as_object().cloned().unwrap_or_default();
        self.client
            .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments))
            .await
            .unwrap()
    }

    /// The structured content of a successful call.
    async fn ok(&self, tool: &'static str, arguments: Value) -> Value {
        let result = self.call(tool, arguments).await;
        assert_ne!(result.is_error, Some(true), "{tool} failed: {result:?}");
        result.structured_content.expect("structured content")
    }

    /// The `{ code, message }` of a failed call.
    async fn failure(&self, tool: &'static str, arguments: Value) -> Value {
        let result = self.call(tool, arguments).await;
        assert_eq!(
            result.is_error,
            Some(true),
            "{tool} should fail: {result:?}"
        );
        result.structured_content.unwrap_or_else(|| {
            // Arguments that do not fit the schema are reported by rmcp as plain text.
            json!({ "code": "invalidArguments", "message": result.content[0].as_text().unwrap().text })
        })
    }
}

fn titles(value: &Value) -> Vec<String> {
    value["notes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["title"].as_str().unwrap().to_owned())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Tool list

const READ_TOOLS: [&str; 7] = [
    "get_note",
    "get_note_text",
    "list_binders",
    "list_notes",
    "list_tags",
    "render_note",
    "search_notes",
];

#[tokio::test(flavor = "current_thread")]
async fn every_read_tool_is_listed_with_a_schema_and_annotations() {
    let session = session().await;
    // The other groups of tools (write, bridge) have their own tests: the read tools are all
    // there, with the hints of a read tool.
    let tools: Vec<_> = session
        .client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .filter(|t| READ_TOOLS.contains(&t.name.as_ref()))
        .collect();
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort();
    assert_eq!(names, READ_TOOLS);
    for tool in &tools {
        assert!(
            tool.description.as_deref().is_some_and(|d| d.len() > 30),
            "{} has no description",
            tool.name
        );
        assert_eq!(
            tool.input_schema.get("type").unwrap(),
            "object",
            "{}",
            tool.name
        );
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(true), "{}", tool.name);
        assert_eq!(annotations.destructive_hint, Some(false), "{}", tool.name);
    }
    let get_note = tools.iter().find(|t| t.name == "get_note").unwrap();
    let schema = Value::Object((*get_note.input_schema).clone());
    assert_eq!(schema["required"], json!(["note"]));
    assert!(schema["properties"]["strokes"].is_object(), "{schema}");
    assert!(schema["properties"]["max_points"].is_object(), "{schema}");
    let render = tools.iter().find(|t| t.name == "render_note").unwrap();
    let schema = Value::Object((*render.input_schema).clone());
    assert!(schema["properties"]["region"].is_object(), "{schema}");
}

#[tokio::test(flavor = "current_thread")]
async fn read_only_serves_the_read_tools_alone() {
    let session = session_with(false, 0, ServerOptions { read_only: true }).await;
    let tools = session.client.list_all_tools().await.unwrap();
    let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_ref()).collect();
    names.sort();
    assert_eq!(names, READ_TOOLS);
    // The router of a read-only server is the read group and nothing else.
    let read_only = ardoise_mcp::build_tool_router(ServerOptions { read_only: true });
    assert_eq!(read_only.list_all().len(), READ_TOOLS.len());
}

#[tokio::test(flavor = "current_thread")]
async fn the_server_introduces_itself() {
    let session = session().await;
    let info = session.client.peer_info().expect("server info");
    assert_eq!(
        info.server_info.as_ref().map(|i| i.name.as_str()),
        Some("ardoise")
    );
    assert!(info.capabilities.tools.is_some());
    assert!(
        info.instructions
            .as_deref()
            .unwrap_or_default()
            .contains("millimetres")
    );
    assert!(info.capabilities.prompts.is_some());
}

// ---------------------------------------------------------------------------------------------
// list_notes, list_binders, list_tags

#[tokio::test(flavor = "current_thread")]
async fn list_notes_gives_summaries_newest_first() {
    let session = session().await;
    let all = session.ok("list_notes", json!({})).await;
    assert_eq!(all["total"], 4);
    assert_eq!(
        titles(&all),
        [
            "",
            "Devoir surveillé",
            "Chimie des métaux",
            "Cours d'optique"
        ]
    );

    let optique = &all["notes"][3];
    assert_eq!(optique["id"], OPTIQUE);
    assert_eq!(
        optique["path"]
            .as_str()
            .unwrap()
            .split_once(".ardoise")
            .unwrap()
            .1,
        ""
    );
    assert!(
        optique["path"]
            .as_str()
            .unwrap()
            .starts_with("Physique/4e/"),
        "{optique}"
    );
    assert_eq!(optique["binder"], "Physique/4e");
    assert_eq!(optique["tags"], json!(["Physique", "Quatrième"]));
    assert_eq!(optique["favorite"], true);
    assert_eq!(
        optique["page"],
        json!({ "width_mm": 297.0, "height_mm": 216.0, "orientation": "landscape" })
    );
    assert_eq!(optique["stroke_count"], 3);
    assert_eq!(optique["has_background_image"], false);
    // ISO 8601 with an offset.
    let created = optique["created"].as_str().unwrap();
    assert!(chrono_like(created), "{created}");

    let devoir = &all["notes"][1];
    assert_eq!(devoir["page"]["orientation"], "portrait");
    assert_eq!(devoir["page"]["width_mm"], 216.0);
}

/// `YYYY-MM-DDTHH:MM:SS` followed by `Z` or `+HH:MM`.
fn chrono_like(text: &str) -> bool {
    text.len() >= 25
        && text.as_bytes()[4] == b'-'
        && text.as_bytes()[10] == b'T'
        && (text.ends_with('Z') || text[19..].contains(['+', '-']))
}

#[tokio::test(flavor = "current_thread")]
async fn list_notes_filters_sorts_and_pages() {
    let session = session().await;

    let physique = session
        .ok("list_notes", json!({ "binder": "Physique" }))
        .await;
    assert_eq!(titles(&physique), ["Chimie des métaux", "Cours d'optique"]);
    let slash = session
        .ok("list_notes", json!({ "binder": "/physique/4E/" }))
        .await;
    assert_eq!(titles(&slash), ["Cours d'optique"]);

    // Tags: case and accents are ignored.
    let tagged = session.ok("list_notes", json!({ "tag": "PHYSIQUE" })).await;
    assert_eq!(titles(&tagged), ["Devoir surveillé", "Cours d'optique"]);
    let accent = session
        .ok("list_notes", json!({ "tag": "quatrieme" }))
        .await;
    assert_eq!(titles(&accent), ["Cours d'optique"]);

    let favorites = session.ok("list_notes", json!({ "favorite": true })).await;
    assert_eq!(titles(&favorites), ["Cours d'optique"]);
    let others = session.ok("list_notes", json!({ "favorite": false })).await;
    assert_eq!(others["total"], 3);

    // Dates: DAY0 is 2026-10-24 (noon UTC), the notes are 0, 2, 5 and 6 days later.
    let since = session
        .ok("list_notes", json!({ "from": "2026-10-26" }))
        .await;
    assert_eq!(
        titles(&since),
        ["", "Devoir surveillé", "Chimie des métaux"]
    );
    let until = session
        .ok("list_notes", json!({ "to": "2026-10-26" }))
        .await;
    assert_eq!(titles(&until), ["Chimie des métaux", "Cours d'optique"]);
    let window = session
        .ok(
            "list_notes",
            json!({ "from": "2026-10-25", "to": "2026-10-26" }),
        )
        .await;
    assert_eq!(window["total"], 1);

    let oldest = session.ok("list_notes", json!({ "sort": "oldest" })).await;
    assert_eq!(oldest["notes"][0]["title"], "Cours d'optique");
    let by_title = session.ok("list_notes", json!({ "sort": "title" })).await;
    assert_eq!(
        titles(&by_title),
        [
            "",
            "Chimie des métaux",
            "Cours d'optique",
            "Devoir surveillé"
        ]
    );

    let page = session
        .ok(
            "list_notes",
            json!({ "sort": "title", "limit": 2, "offset": 1 }),
        )
        .await;
    assert_eq!(
        (
            page["total"].as_i64(),
            page["limit"].as_i64(),
            page["offset"].as_i64()
        ),
        (Some(4), Some(2), Some(1))
    );
    assert_eq!(titles(&page), ["Chimie des métaux", "Cours d'optique"]);
    let huge = session.ok("list_notes", json!({ "limit": 100_000 })).await;
    assert_eq!(huge["limit"], 500);
}

#[tokio::test(flavor = "current_thread")]
async fn list_notes_refuses_bad_arguments() {
    let session = session().await;
    for arguments in [
        json!({ "from": "yesterday" }),
        json!({ "to": "26/10/2026" }),
        json!({ "sort": "random" }),
        json!({ "limit": "ten" }),
        json!({ "favorite": "yes" }),
    ] {
        let error = session.failure("list_notes", arguments.clone()).await;
        assert_eq!(error["code"], "invalidArguments", "{arguments}: {error}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn binders_and_tags_are_counted() {
    let session = session().await;
    let binders = session.ok("list_binders", json!({})).await;
    let list: Vec<(&str, i64, i64)> = binders["binders"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            (
                b["path"].as_str().unwrap(),
                b["note_count"].as_i64().unwrap(),
                b["own_note_count"].as_i64().unwrap(),
            )
        })
        .collect();
    // The export folder is left out; sub-binders count in their parent.
    assert_eq!(list, [("Physique", 2, 1), ("Physique/4e", 1, 1)]);
    assert_eq!(binders["notes_at_root"], 2);
    assert_eq!(binders["total_notes"], 4);

    let tags = session.ok("list_tags", json!({})).await;
    let tags: Vec<(&str, i64)> = tags["tags"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| (t["tag"].as_str().unwrap(), t["count"].as_i64().unwrap()))
        .collect();
    assert_eq!(tags, [("chimie", 1), ("physique", 2), ("Quatrième", 1)]);
}

// ---------------------------------------------------------------------------------------------
// search_notes

#[tokio::test(flavor = "current_thread")]
async fn search_finds_titles_tags_and_handwriting() {
    let session = session_with(false, 3, ServerOptions::default()).await;

    // Title, accents and case ignored.
    let by_title = session
        .ok("search_notes", json!({ "query": "METAUX" }))
        .await;
    assert_eq!(titles(&by_title), ["Chimie des métaux"]);
    assert_eq!(by_title["notes"][0]["matched_in"], json!(["title", "text"]));

    // Handwriting: a word that begins a recognised word, with its snippet.
    let by_text = session
        .ok("search_notes", json!({ "query": "lumiere" }))
        .await;
    assert_eq!(by_text["total"], 1);
    assert_eq!(by_text["notes"][0]["id"], OPTIQUE);
    assert_eq!(by_text["notes"][0]["matched_in"], json!(["text"]));
    assert_eq!(by_text["notes"][0]["snippet"]["word"], "lumière");
    assert_eq!(by_text["notes"][0]["snippet"]["before"], "La");

    // Every word must be found, in the title, the tags, the binder or the text.
    let both = session
        .ok(
            "search_notes",
            json!({ "query": "optique quatrieme droite" }),
        )
        .await;
    assert_eq!(both["total"], 1);
    assert_eq!(
        both["notes"][0]["matched_in"],
        json!(["title", "tags", "text"])
    );
    let none = session
        .ok("search_notes", json!({ "query": "optique chimie" }))
        .await;
    assert_eq!(none["total"], 0);

    // The binder counts too.
    let binder = session
        .ok("search_notes", json!({ "query": "physique" }))
        .await;
    assert_eq!(binder["total"], 3);

    // Notes not read yet are counted, with a hint.
    let status = &by_text["text_search"];
    assert_eq!(status["available"], true);
    assert_eq!(status["unread_notes"], 3);
    assert!(status["hint"].as_str().unwrap().contains("get_note_text"));

    let limited = session
        .ok("search_notes", json!({ "query": "physique", "limit": 1 }))
        .await;
    assert_eq!(
        (
            limited["total"].as_i64(),
            limited["notes"].as_array().unwrap().len()
        ),
        (Some(3), 1)
    );

    let error = session
        .failure("search_notes", json!({ "query": "   " }))
        .await;
    assert_eq!(error["code"], "invalidArguments");
    let error = session.failure("search_notes", json!({})).await;
    assert_eq!(error["code"], "invalidArguments");
}

#[tokio::test(flavor = "current_thread")]
async fn search_still_works_on_titles_without_recognition() {
    let session = session_with(true, 0, ServerOptions::default()).await;
    let found = session
        .ok("search_notes", json!({ "query": "optique" }))
        .await;
    assert_eq!(titles(&found), ["Cours d'optique"]);
    assert_eq!(found["text_search"]["available"], false);
    assert_eq!(found["text_search"]["reason"], "unsupported");
}

// ---------------------------------------------------------------------------------------------
// get_note

#[tokio::test(flavor = "current_thread")]
async fn a_note_is_named_by_id_or_path() {
    let session = session().await;
    let by_id = session.ok("get_note", json!({ "note": OPTIQUE })).await;
    let path = by_id["note"]["path"].as_str().unwrap().to_owned();
    assert!(
        path.starts_with("Physique/4e/") && path.ends_with(".ardoise"),
        "{path}"
    );

    let upper_id = session
        .ok("get_note", json!({ "note": OPTIQUE.to_uppercase() }))
        .await;
    assert_eq!(upper_id["note"]["id"], OPTIQUE);
    for spelling in [
        path.clone(),
        path.to_uppercase(),
        path.replace('/', "\\"),
        path.trim_end_matches(".ardoise").to_owned(),
        format!("/{path}"),
        session
            .harness
            .root
            .join(&path)
            .to_string_lossy()
            .into_owned(),
    ] {
        let found = session
            .ok("get_note", json!({ "note": spelling.clone() }))
            .await;
        assert_eq!(found["note"]["id"], OPTIQUE, "{spelling}");
    }

    for unknown in [
        "55555555-5555-4555-8555-555555555555",
        "Physique/absent.ardoise",
        "../../etc/passwd",
    ] {
        let error = session
            .failure("get_note", json!({ "note": unknown }))
            .await;
        assert_eq!(error["code"], "noteNotFound", "{unknown}");
    }
    let error = session.failure("get_note", json!({ "note": "" })).await;
    assert_eq!(error["code"], "invalidArguments");
    let error = session.failure("get_note", json!({})).await;
    assert_eq!(error["code"], "invalidArguments");
}

#[tokio::test(flavor = "current_thread")]
async fn get_note_describes_the_note() {
    let session = session().await;
    let note = session.ok("get_note", json!({ "note": OPTIQUE })).await;
    assert_eq!(note["note"]["title"], "Cours d'optique");
    assert_eq!(note["origin"], json!({ "kind": "ardoise" }));
    assert_eq!(note["timing"], "recorded");
    assert_eq!(
        note["layers"],
        json!([
            { "index": 0, "name": "Calque 1", "visible": true, "stroke_count": 2 },
            { "index": 1, "name": "Corrections", "visible": false, "stroke_count": 1 },
        ])
    );
    assert_eq!(note["background"]["paper"], "white");
    assert_eq!(note["background"]["template"], "blank");
    assert_eq!(note["background"]["image"], Value::Null);
    assert_eq!(note["duration_ms"], 99 * 8);
    assert_eq!(note["has_narration"], false);
    // No strokes unless asked for.
    assert!(note.get("strokes").is_none());
}

#[tokio::test(flavor = "current_thread")]
async fn get_note_returns_strokes_in_millimetres() {
    let session = session().await;
    let summary = session
        .ok("get_note", json!({ "note": OPTIQUE, "strokes": "summary" }))
        .await;
    let strokes = summary["strokes"].as_array().unwrap();
    assert_eq!(strokes.len(), 3);
    let first = &strokes[0];
    assert_eq!(first["index"], 0);
    assert_eq!(first["tool"], "ballpoint");
    assert_eq!(first["color"], "#0000ff");
    assert_eq!(first["width_mm"], 0.6);
    assert_eq!(first["point_count"], 100);
    assert_eq!(first["start_ms"], 0);
    assert_eq!(first["end_ms"], 99 * 8);
    // x from 1000 to 10900 device units: 10 to 109 mm.
    assert_eq!(first["bounds_mm"]["x"], 10.0);
    assert_eq!(first["bounds_mm"]["width"], 99.0);
    assert!(first.get("points_mm").is_none());
    assert_eq!(strokes[1]["tool"], "pencil");

    let points = session
        .ok(
            "get_note",
            json!({ "note": OPTIQUE, "strokes": "points", "max_points": 60 }),
        )
        .await;
    let strokes = points["strokes"].as_array().unwrap();
    let total: usize = strokes
        .iter()
        .map(|s| s["points_mm"].as_array().map_or(0, Vec::len))
        .sum();
    assert!((6..=60).contains(&total), "{total}");
    let line = strokes[0]["points_mm"].as_array().unwrap();
    assert_eq!(line[0][0], 10.0);
    assert_eq!(line.last().unwrap()[0], 109.0);

    // A budget too small for every stroke leaves the last ones out, and says so.
    let starved = session
        .ok(
            "get_note",
            json!({ "note": OPTIQUE, "strokes": "points", "max_points": 4 }),
        )
        .await;
    assert_eq!(starved["strokes_without_points"], 1);
    assert!(starved["strokes"][2].get("points_mm").is_none());
    assert!(
        starved["points_note"]
            .as_str()
            .unwrap()
            .contains("max_points")
    );

    let empty = session
        .ok("get_note", json!({ "note": VIDE, "strokes": "points" }))
        .await;
    assert_eq!(empty["strokes"], json!([]));
    assert_eq!(empty["duration_ms"], 0);

    let error = session
        .failure("get_note", json!({ "note": OPTIQUE, "strokes": "all" }))
        .await;
    assert_eq!(error["code"], "invalidArguments");
}

// ---------------------------------------------------------------------------------------------
// get_note_text

#[tokio::test(flavor = "current_thread")]
async fn get_note_text_gives_lines_with_positions_and_structure() {
    let session = session().await;
    let text = session
        .ok("get_note_text", json!({ "note": OPTIQUE }))
        .await;
    assert_eq!(text["note"]["id"], OPTIQUE);
    assert_eq!(text["language"], "fr");
    assert_eq!(text["recognizer"], "fake");
    assert_eq!(
        text["text"],
        "Cours d'optique\nLa lumière se propage en ligne droite"
    );
    let lines = text["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 2);
    assert_eq!(lines[0]["text"], "Cours d'optique");
    // Two words from x = 1000 (10 mm), 1500 units apart, 1400 wide: 10 mm to 25 + 14 mm.
    assert_eq!(
        lines[0]["bounds_mm"],
        json!({ "x": 10.0, "y": 10.0, "width": 29.0, "height": 6.0 })
    );
    assert_eq!(lines[1]["bounds_mm"]["y"], 30.0);
    assert_eq!(text["structure"]["source"], "layout_heuristics");
    let kinds: Vec<&str> = text["structure"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["kind"].as_str().unwrap())
        .collect();
    assert!(kinds.contains(&"paragraph"), "{kinds:?}");
    // The pencil strokes the fake text does not cover are drawings.
    assert!(kinds.contains(&"drawing"), "{kinds:?}");
    let drawing = text["structure"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["kind"] == "drawing")
        .unwrap();
    assert!(drawing["bounds_mm"]["width"].as_f64().unwrap() > 0.0);
    assert!(!drawing["strokes"].as_array().unwrap().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn get_note_text_passes_the_language_and_reports_missing_recognition() {
    let session = session().await;
    session.ok("get_note_text", json!({ "note": CHIMIE })).await;
    let english = session
        .ok("get_note_text", json!({ "note": CHIMIE, "language": "en" }))
        .await;
    assert_eq!(english["language"], "en");
    assert_eq!(
        *session.harness.fake.asked.lock().unwrap(),
        vec![None, Some(Language::En)]
    );
    let error = session
        .failure("get_note_text", json!({ "note": CHIMIE, "language": "de" }))
        .await;
    assert_eq!(error["code"], "invalidArguments");
    let error = session
        .failure("get_note_text", json!({ "note": "absent" }))
        .await;
    assert_eq!(error["code"], "noteNotFound");

    // Two files with one id (a copy made in the Explorer): refused, with both paths, instead
    // of silently reading the most recent; a path names the one meant.
    let library = session.harness.library().unwrap();
    library
        .create(
            Path::new("Copie"),
            &NoteFile::new(make_note(CHIMIE, "Chimie des métaux", 2)),
        )
        .unwrap();
    let error = session
        .failure("get_note_text", json!({ "note": CHIMIE }))
        .await;
    assert_eq!(error["code"], "invalidArguments");
    let message = error["message"].as_str().unwrap();
    assert!(message.contains("Copie/") && message.contains("Physique/"), "{message}");
    let named = session
        .ok("get_note_text", json!({ "note": "Physique/2026-10-26 Chimie des métaux" }))
        .await;
    assert_eq!(named["lines"], json!(["Les métaux conduisent le courant"]));

    // A note without ink has an empty text.
    let empty = session.ok("get_note_text", json!({ "note": VIDE })).await;
    assert_eq!(empty["text"], "");
    assert_eq!(empty["lines"], json!([]));

    // No recognition on this PC: the interface's code.
    let session = session_with(true, 0, ServerOptions::default()).await;
    let error = session
        .failure("get_note_text", json!({ "note": CHIMIE }))
        .await;
    assert_eq!(error["code"], "handwritingUnsupported");
}

// ---------------------------------------------------------------------------------------------
// render_note

/// The PNG and the JSON next to it.
fn image_and_info(result: &CallToolResult) -> (ardoise_render::Image, Value) {
    assert_ne!(result.is_error, Some(true), "{result:?}");
    assert_eq!(result.content.len(), 2);
    let image = result.content[0]
        .as_image()
        .unwrap_or_else(|| panic!("first content is not an image: {result:?}"));
    assert_eq!(image.mime_type, "image/png");
    use base64::Engine as _;
    let png = base64::engine::general_purpose::STANDARD
        .decode(&image.data)
        .unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let decoded = ardoise_render::decode_png(&png).unwrap();
    let info: Value = serde_json::from_str(&result.content[1].as_text().unwrap().text).unwrap();
    (decoded, info)
}

fn dark_pixels(image: &ardoise_render::Image) -> usize {
    image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] < 200 || p[1] < 200 || p[2] < 200)
        .count()
}

#[tokio::test(flavor = "current_thread")]
async fn render_returns_a_png_of_the_page() {
    let session = session().await;
    let result = session
        .call("render_note", json!({ "note": OPTIQUE }))
        .await;
    let (image, info) = image_and_info(&result);
    // 297 x 216 mm at 100 dpi: 1169 x 850.
    assert_eq!((image.width, image.height), (1169, 850));
    assert_eq!(info["width_px"], 1169);
    assert_eq!(info["dpi"], 100.0);
    assert_eq!(info["dpi_lowered"], false);
    assert_eq!(info["layers_drawn"], json!(["Calque 1"]));
    assert_eq!(info["note"]["id"], OPTIQUE);
    assert!(dark_pixels(&image) > 500, "the ink is drawn");

    // The hidden layer shows when it is asked for, by name or by index.
    let named = session
        .call(
            "render_note",
            json!({ "note": OPTIQUE, "layers": ["Corrections"] }),
        )
        .await;
    let (named_image, named_info) = image_and_info(&named);
    assert_eq!(named_info["layers_drawn"], json!(["Corrections"]));
    assert_ne!(named_image.pixels, image.pixels);
    let indexed = session
        .call("render_note", json!({ "note": OPTIQUE, "layers": [1] }))
        .await;
    assert_eq!(image_and_info(&indexed).0.pixels, named_image.pixels);
}

#[tokio::test(flavor = "current_thread")]
async fn render_never_exceeds_two_thousand_pixels() {
    let session = session().await;
    let result = session
        .call("render_note", json!({ "note": OPTIQUE, "dpi": 300 }))
        .await;
    let (image, info) = image_and_info(&result);
    assert_eq!(image.width.max(image.height), 2000);
    assert_eq!(info["requested_dpi"], 300);
    assert_eq!(info["dpi_lowered"], true);
    assert_eq!(info["dpi_limited_by"], "image_size");
    let dpi = info["dpi"].as_f64().unwrap();
    assert!((dpi - 2000.0 * 25.4 / 297.0).abs() < 0.1, "{dpi}");

    // Asking for more than the maximum is brought back to 300 dpi (then lowered to fit).
    let result = session
        .call("render_note", json!({ "note": OPTIQUE, "dpi": 5000 }))
        .await;
    assert_eq!(image_and_info(&result).1["requested_dpi"], 300);

    // A small region can be seen at the full 300 dpi.
    let result = session
        .call(
            "render_note",
            json!({ "note": OPTIQUE, "dpi": 300, "region": { "x": 10, "y": 40, "width": 50, "height": 30 } }),
        )
        .await;
    let (image, info) = image_and_info(&result);
    assert_eq!((image.width, image.height), (591, 355));
    assert_eq!(info["dpi"], 300.0);
    assert_eq!(
        info["region_mm"],
        json!({ "x": 10.0, "y": 40.0, "width": 50.0, "height": 30.0 })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_region_of_a_very_large_page_is_drawn_at_a_resolution_the_page_allows() {
    let session = session().await;
    // A 2 m square page (custom paper, or a poster PDF), its ink near the top-left corner.
    let library = Library::open(&session.harness.root).unwrap();
    let mut poster = make_note("55555555-5555-4555-8555-555555555555", "Affiche", 7);
    poster.page = PageSize {
        width: 200_000,
        height: 200_000,
    };
    library
        .create(Path::new(""), &NoteFile::new(poster))
        .unwrap();
    // 50 x 50 mm at 300 dpi would need the whole page at 23 622 px a side (2.2 GB).
    let result = session
        .call(
            "render_note",
            json!({ "note": "55555555-5555-4555-8555-555555555555", "dpi": 300, "region": { "x": 0, "y": 40, "width": 50, "height": 50 } }),
        )
        .await;
    assert_ne!(result.is_error, Some(true), "{result:?}");
    let (image, info) = image_and_info(&result);
    assert_eq!(info["dpi_lowered"], true);
    assert_eq!(info["dpi_limited_by"], "page_size");
    let dpi = info["dpi"].as_f64().unwrap();
    // The page drawn holds at most 24 million pixels.
    let page_side = 2000.0 / 25.4 * dpi;
    assert!(page_side * page_side <= 24_000_000.0 * 1.01, "{dpi}");
    assert!(dpi > 50.0, "{dpi}");
    let side = (50.0 / 25.4 * dpi).round() as u32;
    assert!(image.width.abs_diff(side) <= 1 && image.height.abs_diff(side) <= 1);
    assert!(dark_pixels(&image) > 0, "the ink is in the region");
    // The whole page, at the dpi asked: the image size lowers it, as for any page.
    let result = session
        .call(
            "render_note",
            json!({ "note": "55555555-5555-4555-8555-555555555555", "dpi": 300 }),
        )
        .await;
    let (image, info) = image_and_info(&result);
    assert_eq!(image.width, 2000);
    assert_eq!(info["dpi_limited_by"], "image_size");
}

#[tokio::test(flavor = "current_thread")]
async fn render_crops_a_region_of_the_page() {
    let session = session().await;
    // The first stroke wanders around y = 50 mm, x from 10 to 109 mm.
    let on_ink = session
        .call(
            "render_note",
            json!({ "note": OPTIQUE, "region": { "x": 0, "y": 40, "width": 120, "height": 20 } }),
        )
        .await;
    let (ink, _) = image_and_info(&on_ink);
    assert_eq!((ink.width, ink.height), (472, 79));
    assert!(dark_pixels(&ink) > 100);
    let blank = session
        .call(
            "render_note",
            json!({ "note": OPTIQUE, "region": { "x": 200, "y": 150, "width": 50, "height": 40 } }),
        )
        .await;
    assert_eq!(dark_pixels(&image_and_info(&blank).0), 0);

    // A region that sticks out of the page is cut at its edge.
    let cut = session
        .call(
            "render_note",
            json!({ "note": OPTIQUE, "region": { "x": 250, "y": 190, "width": 100, "height": 100 } }),
        )
        .await;
    let (image, info) = image_and_info(&cut);
    assert_eq!(
        info["region_mm"],
        json!({ "x": 250.0, "y": 190.0, "width": 47.0, "height": 26.0 })
    );
    assert_eq!((image.width, image.height), (185, 102));

    for region in [
        json!({ "x": 400, "y": 0, "width": 10, "height": 10 }),
        json!({ "x": 0, "y": 0, "width": 0, "height": 10 }),
        json!({ "x": 0, "y": 0, "width": -5, "height": 10 }),
    ] {
        let error = session
            .failure(
                "render_note",
                json!({ "note": OPTIQUE, "region": region.clone() }),
            )
            .await;
        assert_eq!(error["code"], "invalidArguments", "{region}");
    }
    let error = session
        .failure(
            "render_note",
            json!({ "note": OPTIQUE, "region": { "x": 0, "y": 0 } }),
        )
        .await;
    assert_eq!(error["code"], "invalidArguments");
}

#[tokio::test(flavor = "current_thread")]
async fn render_refuses_unknown_layers_and_notes() {
    let session = session().await;
    for layers in [json!(["Nope"]), json!([7])] {
        let error = session
            .failure(
                "render_note",
                json!({ "note": OPTIQUE, "layers": layers.clone() }),
            )
            .await;
        assert_eq!(error["code"], "invalidArguments", "{layers}");
        assert!(error["message"].as_str().unwrap().contains("Calque 1"));
    }
    let error = session
        .failure("render_note", json!({ "note": "absent" }))
        .await;
    assert_eq!(error["code"], "noteNotFound");
}
