//! The bridge tools (`app_state`, `open_note`, `presentation`, `last_live_note`) through rmcp's
//! in-memory transport, with a fake open app: notes named by id or path reach the app as
//! absolute paths, answers come back with the notes' identity in the library, arguments are
//! checked, and every tool says `appNotRunning` without an app.

#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use ardoise_core::format::NoteFile;
use ardoise_core::library::{self, Library};
use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Note, Origin, Timing};
use ardoise_hwr::structure::Structure;
use ardoise_hwr::{HwrError, Language, Recognition};
use ardoise_mcp::bridge::{
    AppBridge, AppView, LastLive, LiveNote, LiveSession, LiveStatus, OpenedNote,
    PresentationAction, PresentationState,
};
use ardoise_mcp::{Environment, Handwriting, Host, ServerOptions, TextSearch, ToolError, serve};
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::{RoleClient, RunningService};
use serde_json::{Value, json};

const OPTIQUE: &str = "11111111-1111-4111-8111-111111111111";
const LIVE: &str = "55555555-5555-4555-8555-555555555555";

struct Env {
    root: PathBuf,
}

impl Environment for Env {
    fn library(&self) -> Result<Library, ToolError> {
        Ok(Library::open(&self.root)?)
    }

    fn language(&self) -> Language {
        Language::Fr
    }
}

struct NoHandwriting;

impl Handwriting for NoHandwriting {
    fn recognition_of(
        &self,
        _note: &Note,
        _language: Option<Language>,
    ) -> Result<Arc<Recognition>, HwrError> {
        Err(HwrError::Unsupported)
    }

    fn structure_of(&self, _note: &Note, _recognition: &Recognition) -> Structure {
        Structure {
            items: Vec::new(),
            analyzed: false,
        }
    }

    fn search(&self, _query: &str) -> TextSearch {
        TextSearch::default()
    }
}

/// The open app: answers from what the test sets, and remembers what it was asked.
#[derive(Default)]
struct FakeApp {
    asked: Mutex<Vec<String>>,
    editor: Mutex<Option<String>>,
    live_notes: Mutex<Vec<LiveNote>>,
}

impl AppBridge for FakeApp {
    fn app_state(&self) -> Result<AppView, ToolError> {
        let note = self.editor.lock().unwrap().clone();
        Ok(AppView {
            version: "0.1.0".into(),
            library: Some("C:\\Bibliothèque".into()),
            screen: if note.is_some() { "editor" } else { "library" }.into(),
            note,
            unsaved_changes: false,
            live: Some(LiveStatus {
                state: "live".into(),
                running: true,
                page: 2,
                pages: 3,
                presentation: Some("\\\\.\\DISPLAY2".into()),
            }),
        })
    }

    fn open_note(&self, path: &str) -> Result<OpenedNote, ToolError> {
        self.asked.lock().unwrap().push(format!("open {path}"));
        *self.editor.lock().unwrap() = Some(path.to_owned());
        Ok(OpenedNote { path: path.into() })
    }

    fn presentation(
        &self,
        action: PresentationAction,
        page: Option<u32>,
    ) -> Result<PresentationState, ToolError> {
        self.asked
            .lock()
            .unwrap()
            .push(format!("presentation {action:?} {page:?}"));
        if page == Some(9) {
            return Err(ToolError::new("invalidArguments", "there are 3 pages"));
        }
        Ok(PresentationState {
            open: action != PresentationAction::Stop,
            screen: Some("Projector".into()),
            page: Some(page.unwrap_or(2)),
            pages: Some(3),
        })
    }

    fn last_live_note(&self) -> Result<LastLive, ToolError> {
        let notes = self.live_notes.lock().unwrap().clone();
        Ok(LastLive {
            session: (!notes.is_empty()).then_some(LiveSession {
                running: false,
                this_run: true,
                ended: Some(1_792_843_200_000),
                notes,
            }),
        })
    }
}

fn note(id: &str, title: &str) -> Note {
    Note {
        id: id.to_owned(),
        title: title.to_owned(),
        created: 1_792_843_200_000,
        modified: 1_792_843_260_000,
        page: A4_LANDSCAPE,
        background: Background::default(),
        background_image: None,
        tags: Vec::new(),
        favorite: false,
        layers: vec![Layer {
            name: "Calque 1".into(),
            visible: true,
        }],
        strokes: Vec::new(),
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

struct Session {
    client: RunningService<RoleClient, ()>,
    root: PathBuf,
    /// Path of « Cours d'optique », relative to the library.
    optique: String,
    app: Arc<FakeApp>,
    _dir: tempfile::TempDir,
}

async fn session_with(app: Option<Arc<FakeApp>>, options: ServerOptions) -> Session {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Bibliothèque");
    std::fs::create_dir(&root).unwrap();
    library::init(&root).unwrap();
    let library = Library::open(&root).unwrap();
    let optique = library
        .create(
            Path::new("Physique"),
            &NoteFile::new(note(OPTIQUE, "Cours d'optique")),
        )
        .unwrap()
        .path
        .strip_prefix(&root)
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    library
        .create(Path::new(""), &NoteFile::new(note(LIVE, "")))
        .unwrap();
    let fake = app.clone().unwrap_or_default();
    let mut host = Host::new(
        Arc::new(Env { root: root.clone() }),
        Arc::new(NoHandwriting),
    );
    if let Some(app) = app {
        host = host.with_app(app);
    }
    let (server_io, client_io) = tokio::io::duplex(1 << 20);
    tokio::spawn(async move {
        let _ = serve(host, options, server_io).await;
    });
    let client = ().serve(client_io).await.unwrap();
    Session {
        client,
        root,
        optique,
        app: fake,
        _dir: dir,
    }
}

impl Session {
    async fn call(&self, tool: &'static str, arguments: Value) -> CallToolResult {
        let arguments = arguments.as_object().cloned().unwrap_or_default();
        self.client
            .call_tool(CallToolRequestParams::new(tool).with_arguments(arguments))
            .await
            .unwrap()
    }

    async fn ok(&self, tool: &'static str, arguments: Value) -> Value {
        let result = self.call(tool, arguments).await;
        assert_ne!(result.is_error, Some(true), "{tool} failed: {result:?}");
        result.structured_content.expect("structured content")
    }

    async fn failure(&self, tool: &'static str, arguments: Value) -> Value {
        let result = self.call(tool, arguments).await;
        assert_eq!(
            result.is_error,
            Some(true),
            "{tool} should fail: {result:?}"
        );
        result.structured_content.expect("structured error")
    }

    fn abs(&self, relative: &str) -> String {
        let relative = relative.replace('/', std::path::MAIN_SEPARATOR_STR);
        self.root.join(relative).to_string_lossy().into_owned()
    }
}

#[tokio::test]
async fn the_bridge_tools_are_listed_with_their_annotations() {
    let session = session_with(Some(Arc::default()), ServerOptions::default()).await;
    let tools = session.client.list_all_tools().await.unwrap();
    for (name, read_only) in [
        ("app_state", true),
        ("open_note", false),
        ("presentation", false),
        ("last_live_note", true),
    ] {
        let tool = tools
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("{name} missing"));
        let annotations = tool.annotations.as_ref().expect("annotations");
        assert_eq!(annotations.read_only_hint, Some(read_only), "{name}");
        assert_eq!(annotations.destructive_hint, Some(false), "{name}");
        assert!(tool.description.as_deref().is_some_and(|d| !d.is_empty()));
    }
    // `--read-only` keeps the bridge out, `app_state` included.
    let read_only = session_with(Some(Arc::default()), ServerOptions { read_only: true }).await;
    let names: Vec<String> = read_only
        .client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    for name in ["app_state", "open_note", "presentation", "last_live_note"] {
        assert!(
            !names.iter().any(|n| n == name),
            "{name} in a read-only server"
        );
    }
}

#[tokio::test]
async fn without_the_app_every_bridge_tool_says_so() {
    let session = session_with(None, ServerOptions::default()).await;
    for (tool, args) in [
        ("app_state", json!({})),
        ("open_note", json!({ "note": OPTIQUE })),
        ("presentation", json!({ "action": "start" })),
        ("last_live_note", json!({})),
    ] {
        let error = session.failure(tool, args).await;
        assert_eq!(error["code"], "appNotRunning", "{tool}: {error}");
        assert_eq!(error["message"], "Ardoise is not open", "{tool}");
    }
}

#[tokio::test]
async fn app_state_names_the_note_in_the_editor() {
    let session = session_with(Some(Arc::default()), ServerOptions::default()).await;
    let state = session.ok("app_state", json!({})).await;
    assert_eq!(state["open"], true);
    assert_eq!(state["screen"], "library");
    assert_eq!(state["note"], Value::Null);
    assert_eq!(state["presentation_open"], true);
    assert_eq!(state["current_page"], 2);
    assert_eq!(state["live_mode"]["pages"], 3);

    *session.app.editor.lock().unwrap() = Some(session.abs(&session.optique));
    let state = session.ok("app_state", json!({})).await;
    assert_eq!(state["screen"], "editor");
    assert_eq!(state["note"]["id"], OPTIQUE);
    assert_eq!(state["note"]["path"], session.optique.as_str());
    assert_eq!(state["note"]["title"], "Cours d'optique");
}

#[tokio::test]
async fn open_note_sends_the_app_an_absolute_path_in_the_library() {
    let session = session_with(Some(Arc::default()), ServerOptions::default()).await;
    let opened = session.ok("open_note", json!({ "note": OPTIQUE })).await;
    assert_eq!(opened["opened"]["id"], OPTIQUE);
    assert_eq!(opened["opened"]["path"], session.optique.as_str());
    let by_path = session
        .ok(
            "open_note",
            json!({ "note": session.optique.to_uppercase() }),
        )
        .await;
    assert_eq!(by_path["opened"]["id"], OPTIQUE);
    let expected = format!("open {}", session.abs(&session.optique));
    assert_eq!(
        *session.app.asked.lock().unwrap(),
        [expected.clone(), expected]
    );
    // A note that does not exist never reaches the app.
    let error = session
        .failure("open_note", json!({ "note": "Absente" }))
        .await;
    assert_eq!(error["code"], "noteNotFound");
    assert_eq!(session.app.asked.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn presentation_checks_its_arguments_before_asking_the_app() {
    let session = session_with(Some(Arc::default()), ServerOptions::default()).await;
    let started = session
        .ok("presentation", json!({ "action": "start" }))
        .await;
    assert_eq!(started["presentation_open"], true);
    assert_eq!(started["screen"], "Projector");
    let page = session
        .ok("presentation", json!({ "action": "page", "page": 3 }))
        .await;
    assert_eq!(
        (page["page"].clone(), page["pages"].clone()),
        (json!(3), json!(3))
    );
    let stopped = session
        .ok("presentation", json!({ "action": "stop" }))
        .await;
    assert_eq!(stopped["presentation_open"], false);
    for args in [
        json!({ "action": "page" }),
        json!({ "action": "page", "page": 0 }),
        json!({ "action": "next", "page": 2 }),
    ] {
        let error = session.failure("presentation", args.clone()).await;
        assert_eq!(error["code"], "invalidArguments", "{args}");
    }
    // An action the schema does not know is refused by rmcp.
    let unknown = session
        .call("presentation", json!({ "action": "explode" }))
        .await;
    assert_eq!(unknown.is_error, Some(true));
    // The app's own refusal keeps its code.
    let error = session
        .failure("presentation", json!({ "action": "page", "page": 9 }))
        .await;
    assert_eq!(error["code"], "invalidArguments");
    assert_eq!(error["message"], "there are 3 pages");
    assert_eq!(
        *session.app.asked.lock().unwrap(),
        [
            "presentation Start None",
            "presentation Page Some(3)",
            "presentation Stop None",
            "presentation Page Some(9)",
        ]
    );
}

#[tokio::test]
async fn last_live_note_finds_the_notes_by_their_id() {
    let session = session_with(Some(Arc::default()), ServerOptions::default()).await;
    let none = session.ok("last_live_note", json!({})).await;
    assert_eq!(none["session"], Value::Null);
    assert_eq!(none["notes"], json!([]));

    // The live page was moved since it was written; a second page went to the trash.
    *session.app.live_notes.lock().unwrap() = vec![
        LiveNote {
            page: 1,
            id: LIVE.into(),
            path: session.abs("old place.ardoise"),
        },
        LiveNote {
            page: 2,
            id: "66666666-6666-4666-8666-666666666666".into(),
            path: session.abs("gone.ardoise"),
        },
    ];
    let last = session.ok("last_live_note", json!({})).await;
    assert_eq!(last["session"]["running"], false);
    assert_eq!(last["session"]["this_run"], true);
    assert!(
        last["session"]["ended"]
            .as_str()
            .unwrap()
            .starts_with("2026-")
    );
    let notes = last["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0]["found"], true);
    assert_eq!(notes[0]["id"], LIVE);
    assert_eq!(notes[0]["page"], 1);
    assert!(notes[0]["path"].as_str().unwrap().ends_with(".ardoise"));
    assert_eq!(notes[1]["found"], false);
    assert_eq!(notes[1]["id"], "66666666-6666-4666-8666-666666666666");
    // The note to clean up is the last page still in the library.
    assert_eq!(last["note"]["id"], LIVE);
}
