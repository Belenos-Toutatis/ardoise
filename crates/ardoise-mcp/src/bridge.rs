//! The bridge to the open app: the tools `app_state`, `open_note`, `presentation` and
//! `last_live_note` (`docs/design/mcp.md`, "As built: bridge and guide").
//!
//! - [`AppBridge`]: what the open app answers. The tools ask it; the app implements it
//!   (`app/src-tauri/src/app_bridge.rs`) and serves it on a named pipe ([`pipe::PipeServer`]);
//!   `--mcp` reaches it through [`PipeBridge`], which implements the same trait over the pipe.
//! - [`protocol`]: the lines exchanged (platform-free, unit-tested);
//! - [`pipe`]: the named pipe (Windows; nothing elsewhere).
//!
//! The tools name notes like the others (`id` or path in the library) and send the app absolute
//! paths; the app checks them against its library. No request deletes anything, changes a
//! setting or touches the tablet.

pub mod pipe;
pub mod protocol;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use ardoise_core::library::{Library, NoteEntry};
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::CallToolResult;
use rmcp::{ErrorData, tool, tool_router};
use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub use protocol::{
    AppView, LastLive, LiveNote, LiveSession, LiveStatus, OpenedNote, PresentationAction,
    PresentationState, Request,
};

use crate::error::{ToolError, ToolResult, code, json_result};
use crate::host::Host;
use crate::notes::{NoteSummary, find_note, iso, relative_path};
use crate::server::ArdoiseServer;
use pipe::{PipeClient, PipeError};

/// What the open app answers. Every method fails with `appNotRunning` when no app is open.
pub trait AppBridge: Send + Sync {
    /// What the app shows.
    fn app_state(&self) -> Result<AppView, ToolError>;

    /// Shows the note at `path` (absolute, in the library) in the editor, as a click on it in
    /// the library does.
    fn open_note(&self, path: &str) -> Result<OpenedNote, ToolError>;

    /// Drives the presentation of live mode, as the live window's buttons and keys do.
    fn presentation(
        &self,
        action: PresentationAction,
        page: Option<u32>,
    ) -> Result<PresentationState, ToolError>;

    /// The notes of the current or last live session.
    fn last_live_note(&self) -> Result<LastLive, ToolError>;
}

/// The error of every bridge tool when no app is open.
pub fn not_running() -> ToolError {
    ToolError::new(code::APP_NOT_RUNNING, "Ardoise is not open")
}

/// No app is open: every request fails with `appNotRunning`.
pub struct NoApp;

impl AppBridge for NoApp {
    fn app_state(&self) -> Result<AppView, ToolError> {
        Err(not_running())
    }

    fn open_note(&self, _path: &str) -> Result<OpenedNote, ToolError> {
        Err(not_running())
    }

    fn presentation(
        &self,
        _action: PresentationAction,
        _page: Option<u32>,
    ) -> Result<PresentationState, ToolError> {
        Err(not_running())
    }

    fn last_live_note(&self) -> Result<LastLive, ToolError> {
        Err(not_running())
    }
}

// ---------------------------------------------------------------------------------------------
// The client of the pipe

/// How long `--mcp` waits for the app, per kind of request (the app gives its window a little
/// less, so that its own clearer error arrives first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timeouts {
    /// To connect while every instance of the pipe is busy.
    pub connect: Duration,
    /// `app_state`, `last_live_note`.
    pub quick: Duration,
    /// `open_note`, `presentation` (the window acts, then answers).
    pub action: Duration,
}

impl Default for Timeouts {
    fn default() -> Self {
        Timeouts {
            connect: Duration::from_secs(2),
            quick: Duration::from_secs(8),
            action: Duration::from_secs(20),
        }
    }
}

/// [`AppBridge`] over the named pipe of the open app: one connection per request, so an app
/// opened or closed meanwhile is followed.
pub struct PipeBridge {
    name: String,
    timeouts: Timeouts,
    next_id: AtomicU64,
}

impl PipeBridge {
    /// The bridge to the app serving the pipe `name` ([`pipe::pipe_name_for`]).
    pub fn new(name: impl Into<String>) -> PipeBridge {
        PipeBridge::with_timeouts(name, Timeouts::default())
    }

    pub fn with_timeouts(name: impl Into<String>, timeouts: Timeouts) -> PipeBridge {
        PipeBridge {
            name: name.into(),
            timeouts,
            next_id: AtomicU64::new(1),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    fn ask<T: DeserializeOwned>(
        &self,
        request: Request,
        timeout: Duration,
    ) -> Result<T, ToolError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut client =
            PipeClient::connect(&self.name, self.timeouts.connect).map_err(pipe_error)?;
        let answer = client
            .exchange(&protocol::request_line(id, &request), timeout)
            .map_err(pipe_error)?;
        protocol::read_response(&answer, id)
    }
}

/// What the assistant is told when the pipe fails.
fn pipe_error(error: PipeError) -> ToolError {
    match error {
        PipeError::NotRunning | PipeError::Unsupported => not_running(),
        PipeError::Foreign(detail) => ToolError::new(
            code::APP_NOT_RUNNING,
            format!("Ardoise is not open (the bridge's pipe is held by another program: {detail})"),
        ),
        PipeError::Timeout => ToolError::new(
            code::APP_NOT_RESPONDING,
            "Ardoise is open but did not answer in time: it may be busy; try again",
        ),
        PipeError::TooLong => ToolError::new(
            code::INVALID_REQUEST,
            "the answer of Ardoise is longer than the bridge allows",
        ),
        PipeError::AlreadyServed(_) | PipeError::Io(_) => ToolError::new(
            code::APP_NOT_RESPONDING,
            format!("the connection to Ardoise was lost: {error}"),
        ),
    }
}

impl AppBridge for PipeBridge {
    fn app_state(&self) -> Result<AppView, ToolError> {
        self.ask(Request::AppState, self.timeouts.quick)
    }

    fn open_note(&self, path: &str) -> Result<OpenedNote, ToolError> {
        self.ask(
            Request::OpenNote {
                path: path.to_owned(),
            },
            self.timeouts.action,
        )
    }

    fn presentation(
        &self,
        action: PresentationAction,
        page: Option<u32>,
    ) -> Result<PresentationState, ToolError> {
        self.ask(Request::Presentation { action, page }, self.timeouts.action)
    }

    fn last_live_note(&self) -> Result<LastLive, ToolError> {
        self.ask(Request::LastLiveNote, self.timeouts.quick)
    }
}

// ---------------------------------------------------------------------------------------------
// The tools

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct OpenNoteArgs {
    /// The note's `id`, or its path relative to the library as `list_notes` gives it.
    pub note: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PresentationArgs {
    /// `start` projects the live page on the screen chosen in Ardoise's live window, `stop` closes the projection (live mode goes on), `next` / `previous` change page (`next` from the last page adds a blank one, like the → key), `page` shows page `page`.
    pub action: PresentationAction,
    /// With `action: "page"` only: the page to show, numbered from 1.
    #[schemars(range(min = 1))]
    pub page: Option<u32>,
}

/// A note named in a result: its identity in the library now.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct NoteRef {
    /// Absent when the note is no longer in the library.
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    /// Relative to the library (absolute when outside it).
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
}

fn note_ref(library: Option<&Library>, path: &str) -> NoteRef {
    let found = library.and_then(|library| {
        find_note(library, path)
            .ok()
            .map(|entry| (relative_path(library, &entry.path), entry))
    });
    match found {
        Some((relative, entry)) => NoteRef {
            id: Some(entry.id),
            path: relative,
            title: Some(entry.title),
        },
        None => NoteRef {
            id: None,
            path: path.to_owned(),
            title: None,
        },
    }
}

/// `app_state`.
pub fn app_state(host: &Host) -> ToolResult {
    let view = host.app.app_state()?;
    let library = host.library().ok();
    let note = view
        .note
        .as_deref()
        .map(|path| note_ref(library.as_ref(), path));
    let live = view.live.as_ref();
    json_result(serde_json::json!({
        "open": true,
        "version": view.version,
        "library": view.library,
        "screen": view.screen,
        "note": note,
        "unsaved_changes": view.unsaved_changes,
        "live_mode": live.map(|live| serde_json::json!({
            "state": live.state,
            "running": live.running,
            "page": live.page,
            "pages": live.pages,
        })),
        "presentation_open": live.is_some_and(|live| live.presentation.is_some()),
        "presentation_screen": live.and_then(|live| live.presentation.clone()),
        "current_page": live.map(|live| live.page),
    }))
}

/// `open_note`: the note is looked up in the library first (a wrong name is said as such).
pub fn open_note(host: &Host, args: OpenNoteArgs) -> ToolResult {
    let library = host.library()?;
    let entry = find_note(&library, &args.note)?;
    host.app.open_note(&entry.path.to_string_lossy())?;
    json_result(serde_json::json!({
        "opened": NoteSummary::of_entry(&library, &entry),
    }))
}

/// `presentation`.
pub fn presentation(host: &Host, args: PresentationArgs) -> ToolResult {
    match (args.action, args.page) {
        (PresentationAction::Page, None) => {
            return Err(ToolError::invalid(
                "`page` is needed with the action `page` (pages are numbered from 1)",
            ));
        }
        (PresentationAction::Page, Some(0)) => {
            return Err(ToolError::invalid("pages are numbered from 1"));
        }
        (PresentationAction::Page, Some(_)) => {}
        (_, Some(_)) => {
            return Err(ToolError::invalid(
                "`page` goes with the action `page` only",
            ));
        }
        (_, None) => {}
    }
    let state = host.app.presentation(args.action, args.page)?;
    json_result(serde_json::json!({
        "presentation_open": state.open,
        "screen": state.screen,
        "page": state.page,
        "pages": state.pages,
    }))
}

/// One page of the live session in the result.
#[derive(Debug, Clone, PartialEq, Serialize)]
struct LivePage {
    /// 1-based.
    page: u32,
    /// The note is in the library now (it may have been moved: it is found by its id).
    found: bool,
    #[serde(flatten)]
    note: Option<NoteSummary>,
    /// When it is not found: its id and where it was written.
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    written_at: Option<String>,
}

fn live_page(library: Option<&Library>, notes: &[NoteEntry], page: &LiveNote) -> LivePage {
    // Several files may share an id (a sync client's conflict copy): the original wins.
    let entry = notes
        .iter()
        .filter(|entry| entry.id.eq_ignore_ascii_case(&page.id))
        .min_by_key(|entry| entry.conflict.is_some());
    match (library, entry) {
        (Some(library), Some(entry)) => LivePage {
            page: page.page,
            found: true,
            note: Some(NoteSummary::of_entry(library, entry)),
            id: None,
            written_at: None,
        },
        _ => LivePage {
            page: page.page,
            found: false,
            note: None,
            id: Some(page.id.clone()),
            written_at: Some(page.path.clone()),
        },
    }
}

/// `last_live_note`.
pub fn last_live_note(host: &Host) -> ToolResult {
    let last = host.app.last_live_note()?;
    let Some(session) = last.session else {
        return json_result(serde_json::json!({
            "session": null,
            "note": null,
            "notes": [],
            "hint": "no live session is known on this computer (live mode was not used, or only before the notes were written)",
        }));
    };
    let library = host.library().ok();
    let listing = library.as_ref().map(Library::list);
    let entries = listing
        .as_ref()
        .map_or(&[][..], |listing| &listing.notes[..]);
    let pages: Vec<LivePage> = session
        .notes
        .iter()
        .map(|note| live_page(library.as_ref(), entries, note))
        .collect();
    // The note to clean up: the last page written that is still in the library.
    let note = pages.iter().rev().find(|page| page.found).cloned();
    json_result(serde_json::json!({
        "session": {
            "running": session.running,
            "this_run": session.this_run,
            "ended": session.ended.map(iso),
        },
        "note": note,
        "notes": pages,
    }))
}

#[tool_router(router = bridge_tools, vis = "pub(crate)")]
impl ArdoiseServer {
    /// What the open Ardoise shows: the library, the editor (which note, unsaved changes) or live mode (state, current page, presentation open). Fails with `appNotRunning` when Ardoise is not open.
    #[tool(
        name = "app_state",
        annotations(
            title = "State of the open app",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn app_state(&self) -> Result<CallToolResult, ErrorData> {
        self.blocking(app_state).await
    }

    /// Show a note in the editor of the open Ardoise, as a click on it in the library does. Refused (`appBusy`) while the editor holds unsaved changes or live mode is on screen.
    #[tool(
        name = "open_note",
        annotations(
            title = "Open a note in the app",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn open_note(
        &self,
        Parameters(args): Parameters<OpenNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| open_note(host, args)).await
    }

    /// Drive the classroom presentation of live mode (the live page projected full screen): start or stop projecting, next, previous or a given page. Live mode must be open in Ardoise (`liveNotRunning` otherwise); the teacher starts it, the assistant never touches the tablet.
    #[tool(
        name = "presentation",
        annotations(
            title = "Drive the live presentation",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn presentation(
        &self,
        Parameters(args): Parameters<PresentationArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.blocking(move |host| presentation(host, args)).await
    }

    /// The notes written during the current or last live session (one note per page), the last page first in `note`: what to offer a clean version of at the end of a lesson. Remembered on this computer across restarts of Ardoise.
    #[tool(
        name = "last_live_note",
        annotations(
            title = "Notes of the last live session",
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn last_live_note(&self) -> Result<CallToolResult, ErrorData> {
        self.blocking(last_live_note).await
    }
}

impl ArdoiseServer {
    /// The bridge tools.
    pub(crate) fn bridge_router() -> ToolRouter<ArdoiseServer> {
        Self::bridge_tools()
    }
}
