//! The editor's projection (SPEC §4.4 « Projeter la note de l'éditeur », `docs/questions.md` Q13;
//! docs/design/paper-projection-narration.md §2): the editor's page full screen on a screen, in a
//! window of its own (label `projection`, `index.html?view=projection`) that speaks live's own
//! vocabulary (`LiveBatch`), fed by the editor with small updates (`lib/projection-feed.ts`).
//!
//! Package 0 (the contract): the shapes and the five commands, as stubs. Package C fills in the
//! rest (the scene, the events, the window).

use std::sync::Mutex;

use ardoise_core::note::Note;
use ardoise_ink::{Rgba, Stroke, Tool};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::State;
use tauri::ipc::Channel;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::live::{LiveBatch, LiveRuntime, LiveSnapshot};
use crate::presentation::MonitorInfo;

/// Most points one update may carry (`StrokeBegin`, `StrokePoints`).
#[allow(dead_code)] // Read by package C.
pub const MAX_PROJECTION_POINTS: usize = 4096;
/// Most strokes one `Strokes` tail may carry.
#[allow(dead_code)] // Read by package C.
pub const MAX_PROJECTION_STROKES: usize = 1 << 20;

/// What the editor tells the projection (`ProjectionUpdate` in `projection.ts`); numbers that are
/// indices or identifiers arrive as plain numbers and are checked by the projection
/// (`projectionInvalid`).
// `Page` carries a whole note and is rare (one call each time the layout changes): not boxed.
#[allow(dead_code, clippy::large_enum_variant)] // Read by package C.
#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProjectionUpdate {
    /// The whole page (a note opened, its page, paper, image, layers or image key changed).
    Page {
        note: Note,
        image_key: Option<String>,
        view: ProjectionView,
    },
    /// The strokes from `keep` on are replaced by `tail`; `ended`: the stroke in progress `id` is now
    /// among them.
    Strokes {
        keep: f64,
        tail: Vec<Stroke>,
        ended: Option<f64>,
    },
    View {
        view: ProjectionView,
    },
    StrokeBegin {
        id: f64,
        tool: Tool,
        color: Rgba,
        width: f32,
        layer: f64,
        points: Vec<ProjectionPoint>,
    },
    StrokePoints {
        id: f64,
        points: Vec<ProjectionPoint>,
    },
    StrokeCancel {
        id: f64,
    },
}

/// The projected area: the zoom (1 = the whole page) and its centre, page units.
#[allow(dead_code)] // Read by package C.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectionView {
    pub zoom: f64,
    pub x: f64,
    pub y: f64,
}

/// A point of the editor's stroke in progress; `wall`: Unix ms of the pointer sample (the window's
/// latency figures count from it).
#[allow(dead_code)] // Read by package C.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ProjectionPoint {
    pub x: f64,
    pub y: f64,
    pub p: f64,
    pub t: f64,
    pub wall: f64,
}

/// Where the projection is open: what the editor's Projection tab shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionStatus {
    pub open: Option<MonitorInfo>,
}

/// What the projection keeps: the projected page and the window's subscription (package C).
#[derive(Default)]
struct Projection {
    /// The screen the window is on.
    #[allow(dead_code)] // Read by package C.
    open: Option<MonitorInfo>,
}

/// The editor's projection of the running app.
#[derive(Default)]
pub struct ProjectionRuntime {
    #[allow(dead_code)] // Read by package C.
    state: Mutex<Projection>,
}

/// The stub's answer to everything that needs the real thing.
fn not_built() -> CommandError {
    CommandError::new("unexpected", "not built yet")
}

/// Opens the projection window full screen on `monitor` (else the saved or default screen).
///
/// Package 0: a stub, nothing calls it yet.
#[tauri::command]
pub async fn projection_open(
    _app: AppHandle,
    _context: State<'_, AppContext>,
    _runtime: State<'_, LiveRuntime>,
    _projection: State<'_, ProjectionRuntime>,
    monitor: Option<String>,
) -> CommandResult<MonitorInfo> {
    let _ = monitor;
    Err(not_built())
}

/// Closes the projection window.
#[tauri::command]
pub async fn projection_close(
    _app: AppHandle,
    _projection: State<'_, ProjectionRuntime>,
) -> CommandResult<()> {
    Ok(())
}

/// The editor's watch on the window: the status now, then at every change.
#[tauri::command]
pub async fn projection_watch(
    _projection: State<'_, ProjectionRuntime>,
    on_event: Channel<ProjectionStatus>,
) -> CommandResult<ProjectionStatus> {
    let _ = on_event;
    Ok(ProjectionStatus { open: None })
}

/// The window's subscription: the snapshot now, then live's batches.
#[tauri::command]
pub async fn projection_subscribe(
    _projection: State<'_, ProjectionRuntime>,
    on_event: Channel<LiveBatch>,
) -> CommandResult<LiveSnapshot> {
    let _ = on_event;
    Err(not_built())
}

/// One update from the editor, applied and sent to the window.
#[tauri::command]
pub async fn projection_update(
    _projection: State<'_, ProjectionRuntime>,
    update: ProjectionUpdate,
) -> CommandResult<()> {
    let _ = update;
    Ok(())
}
