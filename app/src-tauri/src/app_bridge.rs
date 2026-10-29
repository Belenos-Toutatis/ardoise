//! The open app's side of the assistant's bridge (SPEC §4.9; `docs/design/mcp.md`, "As built:
//! bridge and guide"): the named pipe that `ardoise.exe --mcp` reaches through
//! `ardoise_mcp::bridge`. Only the windowed app serves it (not `--mcp`, `--self-test` nor the
//! other command-line modes); when the pipe cannot be created (another copy of the app holds
//! it), the app runs without the bridge.
//!
//! Requests are carried out as the interface carries out a click: what the Svelte interface
//! owns (the screen shown, the editor, the live window's projection and paging) is asked of
//! the main window through the `bridge-request` event, which answers with the `bridge_reply`
//! command (correlated by request id, with a timeout); the rest (the library, the live
//! session's notes) is read here. Nothing deletes, changes a setting or touches the tablet.
//!
//! `last_live_note` survives a restart of the app: the notes of the last live session are
//! remembered in per-PC state (`%LOCALAPPDATA%\Ardoise\last-live-session-<hash>.json`, the hash
//! of the copy's settings location, as in the pipe's name), never in the library, when live mode
//! stops and when the app closes.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ardoise_core::library::{LEGACY_META_DIR, META_DIR, NOTE_EXTENSION};
use ardoise_mcp::ToolError;
use ardoise_mcp::bridge::pipe::{
    PipeError, PipeServer, pipe_name_for, settings_hash, settings_key,
};
use ardoise_mcp::bridge::protocol::wire_code;
use ardoise_mcp::bridge::{
    AppBridge, AppView, LastLive, LiveNote, LiveSession, LiveStatus, NoApp, OpenedNote, PipeBridge,
    PresentationAction, PresentationState,
};
use ardoise_mcp::error::code;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, State, Webview};

use crate::AppContext;
use crate::commands::{CommandError, CommandResult, current_state};
use crate::library::now_ms;
use crate::live::{LiveHub, LiveOverview, LiveRuntime};

/// Label of the main window, which answers the requests.
const MAIN: &str = "main";
/// The event carrying a request to the main window.
const BRIDGE_REQUEST: &str = "bridge-request";
/// How long the window has to say what it shows (`app_state` then answers without it).
const VIEW_TIMEOUT: Duration = Duration::from_secs(3);
/// How long it has to carry out an action (the client waits 20 s: this error arrives first).
const ACTION_TIMEOUT: Duration = Duration::from_secs(15);
/// The last live session, in [`state_dir`]: `last-live-session-<hash of the copy>.json`.
const LAST_LIVE_STEM: &str = "last-live-session";
/// Its name before it was kept per copy (shared by every copy of the PC): read when the copy has
/// no file of its own yet.
const LEGACY_LAST_LIVE_FILE: &str = "last-live-session.json";

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn not_responding(message: impl ToString) -> ToolError {
    ToolError::new(code::APP_NOT_RESPONDING, message)
}

// ---------------------------------------------------------------------------------------------
// Requests to the interface

/// A request for the main window (`app-bridge.svelte.ts` in the interface).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum UiRequest {
    /// What it shows.
    View,
    /// Show this note in the editor.
    OpenNote { path: String },
    /// Drive the live window's projection and pages.
    Presentation {
        action: PresentationAction,
        page: Option<u32>,
    },
}

/// The window's answer: `ok` (any value) or `error` (`{ code, message }`).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct UiAnswer {
    #[serde(default)]
    pub ok: Option<Value>,
    #[serde(default)]
    pub error: Option<UiError>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct UiError {
    pub code: String,
    pub message: String,
}

impl UiAnswer {
    fn into_result(self) -> Result<Value, ToolError> {
        match self.error {
            Some(error) => Err(ToolError::new(wire_code(&error.code), error.message)),
            None => Ok(self.ok.unwrap_or(Value::Null)),
        }
    }
}

/// What the window says it shows.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UiView {
    screen: String,
    #[serde(default)]
    note: Option<String>,
    #[serde(default)]
    unsaved: bool,
}

/// Requests waiting for the window's answer, by id.
#[derive(Default)]
pub struct Pending {
    next: AtomicU64,
    waiting: Mutex<HashMap<u64, Sender<UiAnswer>>>,
}

impl Pending {
    fn register(&self) -> (u64, Receiver<UiAnswer>) {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (sender, receiver) = mpsc::channel();
        lock(&self.waiting).insert(id, sender);
        (id, receiver)
    }

    /// Hands `answer` to request `id`; `false` when nobody waits for it (late or unknown).
    fn answer(&self, id: u64, answer: UiAnswer) -> bool {
        let waiting = lock(&self.waiting).remove(&id);
        waiting.is_some_and(|sender| sender.send(answer).is_ok())
    }

    fn forget(&self, id: u64) {
        lock(&self.waiting).remove(&id);
    }

    /// Waits for the answer to `id` at most `timeout`.
    fn wait(&self, id: u64, answer: &Receiver<UiAnswer>, timeout: Duration) -> Option<UiAnswer> {
        let outcome = answer.recv_timeout(timeout).ok();
        self.forget(id);
        outcome
    }
}

/// The main window as the bridge sees it (a fake in the tests).
trait Window: Send + Sync {
    /// Sends `request` and waits for the answer at most `timeout`.
    fn ask(&self, request: UiRequest, timeout: Duration) -> Result<Value, ToolError>;
}

/// The real main window: the event out, `bridge_reply` back.
struct MainWindow {
    app: AppHandle,
    pending: Arc<Pending>,
}

impl Window for MainWindow {
    fn ask(&self, request: UiRequest, timeout: Duration) -> Result<Value, ToolError> {
        if self.app.get_webview_window(MAIN).is_none() {
            return Err(not_responding("the main window of Ardoise is closed"));
        }
        let (id, answer) = self.pending.register();
        let payload = serde_json::json!({ "id": id, "request": request });
        if let Err(error) = self.app.emit_to(MAIN, BRIDGE_REQUEST, payload) {
            self.pending.forget(id);
            return Err(not_responding(format!(
                "cannot reach the window of Ardoise: {error}"
            )));
        }
        self.pending
            .wait(id, &answer, timeout)
            .ok_or_else(|| {
                not_responding(
                    "the window of Ardoise did not answer in time (still loading, or busy)",
                )
            })?
            .into_result()
    }
}

// ---------------------------------------------------------------------------------------------
// The app's answers

/// What the app gives the bridge.
struct AppSide {
    window: Box<dyn Window>,
    /// The library of this PC, as the app has it now.
    library: Box<dyn Fn() -> Option<PathBuf> + Send + Sync>,
    /// The live session shown.
    live: Box<dyn Fn() -> Option<LiveOverview> + Send + Sync>,
    /// The file remembering the last live session ([`record_file`]).
    record: PathBuf,
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// `idle`, `waiting`, `live` or `reconnecting`, as the interface spells them.
fn state_text(overview: &LiveOverview) -> String {
    serde_json::to_value(overview.state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unknown".into())
}

fn live_status(overview: &LiveOverview) -> LiveStatus {
    LiveStatus {
        state: state_text(overview),
        running: overview.running,
        page: u32::try_from(overview.page + 1).unwrap_or(u32::MAX),
        pages: u32::try_from(overview.pages).unwrap_or(u32::MAX),
        presentation: overview.presentation.clone(),
    }
}

fn live_notes(overview: &LiveOverview) -> Vec<LiveNote> {
    overview
        .notes
        .iter()
        .map(|(index, id, path)| LiveNote {
            page: u32::try_from(index + 1).unwrap_or(u32::MAX),
            id: id.clone(),
            path: path_text(path),
        })
        .collect()
}

/// A path compared as Windows does: case and kind of slash ignored.
fn path_key(path: &Path) -> String {
    path.to_string_lossy().replace('/', "\\").to_lowercase()
}

/// The path of the note file `path` of the library at `root` (absolute, inside it, existing,
/// under no `_ardoise`, `.ardoise` or other hidden folder, whatever its depth); the assistant
/// cannot make the app open anything else. The path returned is the real one (through links and
/// with the case the disk has) under `root` as the app knows it, not the one given: the window
/// compares paths with its own.
fn note_in_library(root: &Path, path: &Path) -> Result<PathBuf, ToolError> {
    let invalid = |message: &str| {
        ToolError::new(
            code::INVALID_ARGUMENTS,
            format!("{message}: {}", path.display()),
        )
    };
    if !path.is_absolute() {
        return Err(invalid("not an absolute path"));
    }
    let is_note = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case(NOTE_EXTENSION));
    if !is_note {
        return Err(invalid("not a note file"));
    }
    let not_found = || {
        ToolError::new(
            "noteNotFound",
            format!("no note at {} in the library", path.display()),
        )
    };
    let real = std::fs::canonicalize(path).map_err(|_| not_found())?;
    let real_root = std::fs::canonicalize(root).map_err(|_| not_found())?;
    let inside = real
        .strip_prefix(&real_root)
        .map_err(|_| invalid("outside the library"))?;
    let hidden = inside.components().any(|part| {
        matches!(part, Component::Normal(name)
        if name.to_str().is_none_or(|name| {
            name.starts_with('.')
                || name.eq_ignore_ascii_case(META_DIR)
                || name.eq_ignore_ascii_case(LEGACY_META_DIR)
        }))
    });
    if hidden || !real.is_file() {
        return Err(not_found());
    }
    Ok(root.join(inside))
}

/// The last live session, as remembered on this PC.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct LastLiveRecord {
    /// The library its notes went to.
    library: String,
    /// When it was remembered (live mode stopped, or the app closed), Unix ms.
    ended: i64,
    notes: Vec<LiveNote>,
}

/// The file remembering the last live session of the copy of Ardoise named `copy` (the hash of
/// its settings location, [`settings_hash`]), in `dir`: copies with their own settings (and so,
/// often, their own library) keep their own; without a settings location, the shared name.
fn record_file(dir: &Path, copy: Option<&str>) -> PathBuf {
    match copy {
        Some(hash) => dir.join(format!("{LAST_LIVE_STEM}-{hash}.json")),
        None => dir.join(LEGACY_LAST_LIVE_FILE),
    }
}

/// The record file of the running copy.
fn record_of(context: &AppContext) -> PathBuf {
    let copy = context
        .store
        .as_ref()
        .ok()
        .map(|store| settings_hash(&settings_key(store.path())));
    record_file(&state_dir(), copy.as_deref())
}

/// The remembered session: the copy's own file, else the file shared by every copy before
/// records were kept per copy (still given only while its library is open).
fn read_record(file: &Path) -> Option<LastLiveRecord> {
    let read = |path: &Path| -> Option<LastLiveRecord> {
        serde_json::from_slice(&std::fs::read(path).ok()?).ok()
    };
    read(file).or_else(|| {
        let legacy = file.with_file_name(LEGACY_LAST_LIVE_FILE);
        (legacy != file).then(|| read(&legacy)).flatten()
    })
}

/// Remembers the session of `overview` (when it wrote notes). Atomic; outside the library.
fn write_record(file: &Path, overview: &LiveOverview, ended: i64) -> std::io::Result<()> {
    let (Some(library), false) = (&overview.library, overview.notes.is_empty()) else {
        return Ok(());
    };
    let record = LastLiveRecord {
        library: path_text(library),
        ended,
        notes: live_notes(overview),
    };
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let bytes = serde_json::to_vec_pretty(&record).map_err(std::io::Error::other)?;
    ardoise_core::fsutil::write_atomic(file, &bytes)
}

/// Per-PC state of the app: `%LOCALAPPDATA%\Ardoise` (the temporary folder without it).
fn state_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|dir| !dir.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("Ardoise")
}

/// Remembers the notes of the session of `hub` for `last_live_note` after a restart (live mode
/// stopped, or the app closing). Best effort: a failure is logged.
pub fn remember_live_session(context: &AppContext, hub: &LiveHub) {
    if let Some(overview) = hub.overview()
        && let Err(error) = write_record(&record_of(context), &overview, now_ms())
    {
        eprintln!("assistant bridge: cannot remember the last live session: {error}");
    }
}

impl AppSide {
    fn ask_as<T: serde::de::DeserializeOwned>(
        &self,
        request: UiRequest,
        timeout: Duration,
    ) -> Result<T, ToolError> {
        let answer = self.window.ask(request, timeout)?;
        serde_json::from_value(answer).map_err(|error| {
            ToolError::new(
                "unexpected",
                format!("the window answered something else: {error}"),
            )
        })
    }
}

impl AppBridge for AppSide {
    fn app_state(&self) -> Result<AppView, ToolError> {
        // What the window shows; without its answer the rest is still worth giving.
        let view = match self.ask_as::<UiView>(UiRequest::View, VIEW_TIMEOUT) {
            Ok(view) => view,
            Err(error) if error.code == code::APP_NOT_RESPONDING || error.code == "unexpected" => {
                UiView {
                    screen: "unknown".into(),
                    note: None,
                    unsaved: false,
                }
            }
            Err(error) => return Err(error),
        };
        Ok(AppView {
            version: env!("CARGO_PKG_VERSION").to_owned(),
            library: (self.library)().as_deref().map(path_text),
            screen: view.screen,
            note: view.note,
            unsaved_changes: view.unsaved,
            live: (self.live)().as_ref().map(live_status),
        })
    }

    fn open_note(&self, path: &str) -> Result<OpenedNote, ToolError> {
        let root = (self.library)()
            .ok_or_else(|| ToolError::new("noLibrary", "no library is open in Ardoise"))?;
        let path = note_in_library(&root, Path::new(path))?;
        self.ask_as(
            UiRequest::OpenNote {
                path: path_text(&path),
            },
            ACTION_TIMEOUT,
        )
    }

    fn presentation(
        &self,
        action: PresentationAction,
        page: Option<u32>,
    ) -> Result<PresentationState, ToolError> {
        self.ask_as(UiRequest::Presentation { action, page }, ACTION_TIMEOUT)
    }

    fn last_live_note(&self) -> Result<LastLive, ToolError> {
        let record = read_record(&self.record);
        // This run's session, once it wrote a note (a session left blank is not the lesson).
        if let Some(overview) = (self.live)().filter(|overview| !overview.notes.is_empty()) {
            let notes = live_notes(&overview);
            let ended = record
                .filter(|record| !overview.running && record.notes == notes)
                .map(|record| record.ended);
            return Ok(LastLive {
                session: Some(LiveSession {
                    running: overview.running,
                    this_run: true,
                    ended,
                    notes,
                }),
            });
        }
        // An earlier run's, if it wrote to the library open now.
        let library = (self.library)().map(|root| path_key(&root));
        let session = record
            .filter(|record| Some(path_key(Path::new(&record.library))) == library)
            .map(|record| LiveSession {
                running: false,
                this_run: false,
                ended: Some(record.ended),
                notes: record.notes,
            });
        Ok(LastLive { session })
    }
}

// ---------------------------------------------------------------------------------------------
// Starting, the commands

/// The bridge of the running app (managed by Tauri).
#[derive(Default)]
pub struct BridgeRuntime {
    pending: Arc<Pending>,
    server: Mutex<Option<PipeServer>>,
    /// Why the bridge is not served, once [`start`] has run (`None`: served, or not started).
    refused: Mutex<Option<BridgeStatus>>,
}

/// Whether assistants can drive this window (Settings > Assistant shows it).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatus {
    /// `serving`; `retrying` (a failure, tried again); `anotherCopy` (another Ardoise started
    /// from this copy serves it); `unavailable` (the pipe could not be created); `off` (not on
    /// this system, or not started).
    pub state: &'static str,
    /// The technical reason, for `retrying` and `unavailable`.
    pub detail: Option<String>,
}

impl BridgeStatus {
    fn new(state: &'static str, detail: Option<String>) -> BridgeStatus {
        BridgeStatus { state, detail }
    }
}

impl BridgeRuntime {
    /// The bridge's state now.
    pub fn status(&self) -> BridgeStatus {
        if let Some(server) = lock(&self.server).as_ref() {
            return match server.trouble() {
                Some(trouble) => BridgeStatus::new("retrying", Some(trouble)),
                None => BridgeStatus::new("serving", None),
            };
        }
        lock(&self.refused)
            .clone()
            .unwrap_or_else(|| BridgeStatus::new("off", None))
    }
}

/// The pipe of this copy of Ardoise for the current user; `None` without a settings location
/// or outside Windows.
fn pipe_name(context: &AppContext) -> Option<String> {
    let store = context.store.as_ref().ok()?;
    match pipe_name_for(store.path()) {
        Ok(name) => Some(name),
        Err(PipeError::Unsupported) => None,
        Err(error) => {
            eprintln!("assistant bridge: no pipe name: {error}");
            None
        }
    }
}

/// Serves the bridge (the windowed app only). A failure is logged and the app runs without it.
pub fn start(app: &AppHandle) {
    let Some(name) = pipe_name(&app.state::<AppContext>()) else {
        return;
    };
    let runtime = app.state::<BridgeRuntime>();
    let library_app = app.clone();
    let live_app = app.clone();
    let side = AppSide {
        window: Box::new(MainWindow {
            app: app.clone(),
            pending: Arc::clone(&runtime.pending),
        }),
        library: Box::new(move || {
            current_state(&library_app.state::<AppContext>())
                .library()
                .map(PathBuf::from)
        }),
        live: Box::new(move || live_app.state::<LiveRuntime>().hub().overview()),
        record: record_of(&app.state::<AppContext>()),
    };
    let refused = match PipeServer::start(&name, Arc::new(side)) {
        Ok(server) => {
            *lock(&runtime.server) = Some(server);
            return;
        }
        Err(PipeError::AlreadyServed(_)) => {
            eprintln!(
                "assistant bridge: another Ardoise started from this copy serves the assistants; this one runs without the bridge"
            );
            BridgeStatus::new("anotherCopy", None)
        }
        Err(PipeError::Unsupported) => BridgeStatus::new("off", None),
        Err(error) => {
            eprintln!("assistant bridge: not available: {error}");
            BridgeStatus::new("unavailable", Some(error.to_string()))
        }
    };
    *lock(&runtime.refused) = Some(refused);
}

/// The bridge of `ardoise.exe --mcp`: the app started from the same copy, through its pipe.
pub fn client(context: &AppContext) -> Arc<dyn AppBridge> {
    match pipe_name(context) {
        Some(name) => Arc::new(PipeBridge::new(name)),
        None => Arc::new(NoApp),
    }
}

/// The main window's answer to request `id` of the bridge.
#[tauri::command]
pub fn bridge_reply(
    webview: Webview,
    runtime: State<'_, BridgeRuntime>,
    id: u64,
    answer: UiAnswer,
) -> CommandResult<()> {
    if webview.label() != MAIN {
        return Err(CommandError::new(
            "unexpected",
            "only the main window answers the assistant's bridge",
        ));
    }
    runtime.pending.answer(id, answer);
    Ok(())
}

/// How to start the MCP server of this copy (Settings, "Assistant (MCP)").
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpLaunch {
    /// Absolute path of the running executable.
    pub exe: String,
    /// Whether assistants can drive this window now.
    pub bridge: BridgeStatus,
}

#[tauri::command]
pub fn mcp_launch_info(runtime: State<'_, BridgeRuntime>) -> CommandResult<McpLaunch> {
    let exe = std::env::current_exe().map_err(|error| CommandError::new("unexpected", error))?;
    let exe = path_text(&exe);
    Ok(McpLaunch {
        exe: exe.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or(exe),
        bridge: runtime.status(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::live::model::LiveSessionState;
    use serde_json::json;

    /// How the fake window answers.
    type Script = Box<dyn Fn(&UiRequest) -> Result<Value, ToolError> + Send + Sync>;

    /// The window: answers from a script, and remembers what it was asked.
    struct FakeWindow {
        asked: Mutex<Vec<UiRequest>>,
        answer: Script,
    }

    impl Window for Arc<FakeWindow> {
        fn ask(&self, request: UiRequest, _timeout: Duration) -> Result<Value, ToolError> {
            lock(&self.asked).push(request.clone());
            (self.answer)(&request)
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        state: PathBuf,
        window: Arc<FakeWindow>,
        live: Arc<Mutex<Option<LiveOverview>>>,
    }

    impl Fixture {
        fn new(
            answer: impl Fn(&UiRequest) -> Result<Value, ToolError> + Send + Sync + 'static,
        ) -> Fixture {
            let dir = tempfile::tempdir().unwrap();
            let root = dir.path().join("Bibliothèque");
            std::fs::create_dir_all(root.join("Physique")).unwrap();
            std::fs::create_dir_all(root.join(META_DIR).join("trash")).unwrap();
            std::fs::write(root.join("Physique").join("Cours.ardoise"), b"x").unwrap();
            std::fs::write(
                root.join(META_DIR).join("trash").join("Vieux.ardoise"),
                b"x",
            )
            .unwrap();
            std::fs::create_dir_all(root.join("Physique").join(META_DIR)).unwrap();
            std::fs::write(
                root.join("Physique").join(META_DIR).join("Profond.ardoise"),
                b"x",
            )
            .unwrap();
            std::fs::create_dir_all(root.join("Physique").join(".cache")).unwrap();
            std::fs::write(
                root.join("Physique").join(".cache").join("Cache.ardoise"),
                b"x",
            )
            .unwrap();
            std::fs::write(root.join("notes.txt"), b"x").unwrap();
            std::fs::write(dir.path().join("Ailleurs.ardoise"), b"x").unwrap();
            let state = dir.path().join("state");
            Fixture {
                root,
                state,
                window: Arc::new(FakeWindow {
                    asked: Mutex::new(Vec::new()),
                    answer: Box::new(answer),
                }),
                live: Arc::new(Mutex::new(None)),
                _dir: dir,
            }
        }

        fn side(&self) -> AppSide {
            let root = self.root.clone();
            let live = Arc::clone(&self.live);
            AppSide {
                window: Box::new(Arc::clone(&self.window)),
                library: Box::new(move || Some(root.clone())),
                live: Box::new(move || lock(&live).clone()),
                record: self.record(),
            }
        }

        /// The record file of the copy under test.
        fn record(&self) -> PathBuf {
            record_file(&self.state, Some("0123abcd"))
        }

        fn overview(&self, running: bool, notes: &[(usize, &str)]) -> LiveOverview {
            LiveOverview {
                state: if running {
                    LiveSessionState::Live
                } else {
                    LiveSessionState::Idle
                },
                running,
                page: 1,
                pages: 2,
                presentation: Some(r"\\.\DISPLAY2".into()),
                library: Some(self.root.clone()),
                notes: notes
                    .iter()
                    .map(|(page, id)| {
                        (
                            *page,
                            (*id).to_owned(),
                            self.root.join(format!("{id}.ardoise")),
                        )
                    })
                    .collect(),
            }
        }
    }

    #[test]
    fn app_state_joins_the_window_and_the_live_session() {
        let fixture = Fixture::new(|_| {
            Ok(json!({ "screen": "editor", "note": "C:\\B\\Cours.ardoise", "unsaved": true }))
        });
        *lock(&fixture.live) = Some(fixture.overview(true, &[]));
        let view = fixture.side().app_state().unwrap();
        assert_eq!(view.screen, "editor");
        assert_eq!(view.note.as_deref(), Some("C:\\B\\Cours.ardoise"));
        assert!(view.unsaved_changes);
        assert_eq!(view.library, Some(path_text(&fixture.root)));
        let live = view.live.unwrap();
        assert_eq!((live.state.as_str(), live.page, live.pages), ("live", 2, 2));
        assert_eq!(live.presentation.as_deref(), Some(r"\\.\DISPLAY2"));
        assert_eq!(*lock(&fixture.window.asked), [UiRequest::View]);
    }

    #[test]
    fn app_state_still_answers_when_the_window_does_not() {
        let fixture = Fixture::new(|_| Err(not_responding("loading")));
        let view = fixture.side().app_state().unwrap();
        assert_eq!(view.screen, "unknown");
        assert_eq!(view.live, None);
    }

    #[test]
    fn only_notes_of_the_library_are_opened() {
        let fixture = Fixture::new(|request| match request {
            UiRequest::OpenNote { path } => Ok(json!({ "path": path })),
            _ => Ok(Value::Null),
        });
        let side = fixture.side();
        let note = fixture.root.join("Physique").join("Cours.ardoise");
        assert_eq!(
            side.open_note(&path_text(&note)).unwrap().path,
            path_text(&note)
        );
        let refused = [
            (fixture.root.join("notes.txt"), code::INVALID_ARGUMENTS),
            (
                fixture.root.join("..").join("Ailleurs.ardoise"),
                code::INVALID_ARGUMENTS,
            ),
            (
                fixture
                    .root
                    .join(META_DIR)
                    .join("trash")
                    .join("Vieux.ardoise"),
                "noteNotFound",
            ),
            (fixture.root.join("Absente.ardoise"), "noteNotFound"),
            (
                fixture
                    .root
                    .join("Physique")
                    .join(META_DIR)
                    .join("Profond.ardoise"),
                "noteNotFound",
            ),
            (
                fixture
                    .root
                    .join("Physique")
                    .join(".cache")
                    .join("Cache.ardoise"),
                "noteNotFound",
            ),
            (
                PathBuf::from("Physique/Cours.ardoise"),
                code::INVALID_ARGUMENTS,
            ),
        ];
        for (path, expected) in refused {
            let error = side.open_note(&path_text(&path)).unwrap_err();
            assert_eq!(error.code, expected, "{}", path.display());
        }
        // Only the note of the library reached the window.
        assert_eq!(lock(&fixture.window.asked).len(), 1);
    }

    #[test]
    fn the_note_is_opened_by_its_real_path_under_the_library_root() {
        let fixture = Fixture::new(|request| match request {
            UiRequest::OpenNote { path } => Ok(json!({ "path": path })),
            _ => Ok(Value::Null),
        });
        // Case is not significant on Windows: the window gets the path as the disk spells it.
        let given = fixture.root.join("PHYSIQUE").join("cours.ARDOISE");
        if given.exists() {
            let opened = fixture.side().open_note(&path_text(&given)).unwrap();
            let real = fixture.root.join("Physique").join("Cours.ardoise");
            assert_eq!(opened.path, path_text(&real));
        }
    }

    #[test]
    fn the_window_refusals_keep_their_codes() {
        let fixture = Fixture::new(|request| match request {
            UiRequest::Presentation { .. } => Err(ToolError::new(
                "liveNotRunning",
                "live mode is not open in Ardoise",
            )),
            _ => Ok(Value::Null),
        });
        let error = fixture
            .side()
            .presentation(PresentationAction::Next, None)
            .unwrap_err();
        assert_eq!(error.code, "liveNotRunning");
        // The answer of the window, decoded.
        let answer: UiAnswer = serde_json::from_value(json!({
            "error": { "code": "appBusy", "message": "unsaved changes" }
        }))
        .unwrap();
        assert_eq!(answer.into_result().unwrap_err().code, code::APP_BUSY);
        // A code of the interface the bridge does not list (a live command's error) is kept;
        // anything that is not a plain identifier is not.
        let new: UiAnswer = serde_json::from_value(json!({
            "error": { "code": "liveTabletLost", "message": "?" }
        }))
        .unwrap();
        assert_eq!(new.into_result().unwrap_err().code, "liveTabletLost");
        let odd: UiAnswer = serde_json::from_value(json!({
            "error": { "code": "not a code!", "message": "?" }
        }))
        .unwrap();
        assert_eq!(odd.into_result().unwrap_err().code, "unexpected");
    }

    #[test]
    fn the_bridge_says_whether_it_serves() {
        let runtime = BridgeRuntime::default();
        assert_eq!(runtime.status(), BridgeStatus::new("off", None));
        *lock(&runtime.refused) = Some(BridgeStatus::new("anotherCopy", None));
        assert_eq!(runtime.status().state, "anotherCopy");
        assert_eq!(
            serde_json::to_value(BridgeStatus::new("retrying", Some("x".into()))).unwrap(),
            json!({ "state": "retrying", "detail": "x" })
        );
    }

    #[test]
    fn requests_are_spelled_as_the_interface_reads_them() {
        assert_eq!(
            serde_json::to_value(UiRequest::Presentation {
                action: PresentationAction::Page,
                page: Some(2)
            })
            .unwrap(),
            json!({ "kind": "presentation", "action": "page", "page": 2 })
        );
        assert_eq!(
            serde_json::to_value(UiRequest::OpenNote {
                path: "C:\\a.ardoise".into()
            })
            .unwrap(),
            json!({ "kind": "openNote", "path": "C:\\a.ardoise" })
        );
        assert_eq!(
            serde_json::to_value(UiRequest::View).unwrap(),
            json!({ "kind": "view" })
        );
    }

    #[test]
    fn answers_find_their_request() {
        let pending = Pending::default();
        let (first, first_answer) = pending.register();
        let (second, second_answer) = pending.register();
        assert_ne!(first, second);
        assert!(pending.answer(
            second,
            UiAnswer {
                ok: Some(json!(2)),
                error: None
            }
        ));
        assert!(pending.answer(
            first,
            UiAnswer {
                ok: Some(json!(1)),
                error: None
            }
        ));
        let timeout = Duration::from_millis(10);
        assert_eq!(
            pending.wait(first, &first_answer, timeout).unwrap().ok,
            Some(json!(1))
        );
        assert_eq!(
            pending.wait(second, &second_answer, timeout).unwrap().ok,
            Some(json!(2))
        );
        // A late or unknown answer is dropped; a request nobody answers times out.
        assert!(!pending.answer(first, UiAnswer::default()));
        let (third, third_answer) = pending.register();
        assert_eq!(pending.wait(third, &third_answer, timeout), None);
        assert!(!pending.answer(third, UiAnswer::default()));
    }

    #[test]
    fn the_last_live_session_is_remembered_across_runs() {
        let fixture = Fixture::new(|_| Ok(Value::Null));
        let side = fixture.side();
        // Nothing yet.
        assert_eq!(side.last_live_note().unwrap().session, None);

        // A session runs: it is this run's.
        *lock(&fixture.live) = Some(fixture.overview(true, &[(0, "a"), (1, "b")]));
        let running = side.last_live_note().unwrap().session.unwrap();
        assert!(running.running && running.this_run);
        assert_eq!(running.notes.len(), 2);
        assert_eq!(
            (running.notes[1].page, running.notes[1].id.as_str()),
            (2, "b")
        );
        assert_eq!(running.ended, None);

        // It stops and is remembered.
        let stopped = fixture.overview(false, &[(0, "a"), (1, "b")]);
        write_record(&fixture.record(), &stopped, 1_792_843_200_000).unwrap();
        *lock(&fixture.live) = Some(stopped);
        let ended = side.last_live_note().unwrap().session.unwrap();
        assert!(!ended.running && ended.this_run);
        assert_eq!(ended.ended, Some(1_792_843_200_000));

        // Next run: no session yet, the remembered one is given.
        *lock(&fixture.live) = None;
        let remembered = side.last_live_note().unwrap().session.unwrap();
        assert!(!remembered.running && !remembered.this_run);
        assert_eq!(remembered.notes.len(), 2);
        // A session of this run that wrote nothing does not hide it.
        *lock(&fixture.live) = Some(fixture.overview(true, &[]));
        assert!(!side.last_live_note().unwrap().session.unwrap().this_run);

        // Another library open now: the remembered session is not of it.
        let other = AppSide {
            library: Box::new(|| Some(PathBuf::from("D:\\Autre"))),
            live: Box::new(|| None),
            ..fixture.side()
        };
        assert_eq!(other.last_live_note().unwrap().session, None);
        // A session without notes is not written.
        let blank = fixture.overview(false, &[]);
        let file = record_file(&fixture.state.join("blank"), Some("0123abcd"));
        write_record(&file, &blank, 1).unwrap();
        assert!(!file.exists());
    }

    #[test]
    fn each_copy_remembers_its_own_live_session() {
        let fixture = Fixture::new(|_| Ok(Value::Null));
        let ours = fixture.overview(false, &[(0, "ours")]);
        let mut theirs = fixture.overview(false, &[(0, "theirs")]);
        theirs.library = Some(PathBuf::from("D:\\Autre"));
        // Another copy (other settings) stops live mode after this one: nothing is lost.
        write_record(&fixture.record(), &ours, 1).unwrap();
        write_record(&record_file(&fixture.state, Some("ffff0000")), &theirs, 2).unwrap();
        let remembered = fixture.side().last_live_note().unwrap().session.unwrap();
        assert_eq!(remembered.notes[0].id, "ours");
        assert_eq!(
            record_file(&fixture.state, Some("0123abcd")).file_name(),
            Some(std::ffi::OsStr::new("last-live-session-0123abcd.json"))
        );
    }

    #[test]
    fn the_shared_record_of_earlier_versions_is_still_read() {
        let fixture = Fixture::new(|_| Ok(Value::Null));
        let legacy = record_file(&fixture.state, None);
        assert_eq!(
            legacy.file_name(),
            Some(std::ffi::OsStr::new(LEGACY_LAST_LIVE_FILE))
        );
        write_record(&legacy, &fixture.overview(false, &[(0, "old")]), 1).unwrap();
        // The copy has no file of its own yet: the shared one is given (its library is open).
        let side = fixture.side();
        assert_eq!(
            side.last_live_note().unwrap().session.unwrap().notes[0].id,
            "old"
        );
        // Once the copy has its own, the shared one is not read any more.
        write_record(
            &fixture.record(),
            &fixture.overview(false, &[(0, "new")]),
            2,
        )
        .unwrap();
        assert_eq!(
            side.last_live_note().unwrap().session.unwrap().notes[0].id,
            "new"
        );
    }
}
