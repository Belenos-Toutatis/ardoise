//! Live mode and the classroom (SPEC §4.2): the live session (pages that become notes, layers,
//! style, blackboard, laser pointer, view of the projected area) shared by every window, fed by
//! the tablet service and sent to the windows in batches through Tauri channels.
//!
//! Contract: docs/design/tablet-live-classroom.md (work package B), "Live session".
//!
//! - [`model`]: the shapes the interface receives and sends (`app/ui/src/lib/live.ts`);
//! - [`session`]: the session itself, pure (strokes, hover, pages, layers, view);
//! - [`hub`]: the session shared between threads, the batches sent to the windows, the pages
//!   written to the library;
//! - [`activity`]: whether live mode runs or the presentation window is open, without a lock
//!   (background work waits meanwhile).

pub mod activity;
pub mod hub;
pub mod model;
pub mod session;

#[cfg(test)]
pub(crate) mod tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ardoise_core::note::PageSize;
use ardoise_core::settings::LiveButton;
use ardoise_device::service::{TabletService, TabletStatus};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, Webview};

use crate::AppContext;
use crate::commands::{CommandError, CommandResult, store};
use crate::diagnostic::report_language;
use crate::library::{library_error, library_of};
use crate::tablet::TabletRuntime;

pub use activity::Activity;
pub use hub::{ButtonSetting, HubConfig, LiveError, LiveHub, PagePosition, Start, StartOptions};
pub use model::{LiveBatch, LiveCommand, LiveSnapshot, LiveStyle, LiveViewState};

use crate::narration::{self, NarrationRuntime};

/// The live session of the running app.
pub struct LiveRuntime {
    hub: LiveHub,
}

impl Default for LiveRuntime {
    fn default() -> Self {
        Self {
            hub: LiveHub::new(HubConfig::default()),
        }
    }
}

impl LiveRuntime {
    pub fn hub(&self) -> &LiveHub {
        &self.hub
    }
}

impl LiveError {
    /// Error for the interface.
    pub(crate) fn command_error(self) -> CommandError {
        match self {
            LiveError::NotRunning | LiveError::Stopped => CommandError::new("liveNotRunning", self),
            LiveError::Library(error) => library_error(error),
            LiveError::Narration(error) => narration::narration_error(error),
        }
    }
}

/// A narration recorded in live mode goes to its pages before they are written for the last
/// time (live mode stops, the app closes, a new session starts). A failure is logged: leaving
/// live mode must not fail because of the microphone.
fn finish_live_narration(narration: &NarrationRuntime, hub: &LiveHub) {
    if let Some(Err(error)) = narration::finish_live(narration, hub) {
        eprintln!("live mode: the narration could not be kept: {error:?}");
    }
}

/// The drawing area of the tablet, from what the service knows (last connection, then the
/// registration).
fn tablet_size(status: &TabletStatus) -> Option<PageSize> {
    let (width, height) = match (&status.device, &status.registered) {
        (Some(device), _) => (device.width, device.height),
        (None, Some(registered)) => (registered.width, registered.height),
        (None, None) => return None,
    };
    (width > 0 && height > 0).then_some(PageSize { width, height })
}

/// Reads `[tablet] button` when the button is pressed (it may change during live mode); the
/// default (new page) when the settings cannot be read.
fn button_setting(context: &AppContext) -> ButtonSetting {
    let store = store(context).ok().cloned();
    Arc::new(move || {
        store
            .as_ref()
            .and_then(|store| store.load().ok())
            .map_or(LiveButton::default(), |settings| settings.tablet.button)
    })
}

/// The main window went away (the app ends): live mode stops and every page is written, so that
/// closing the app without "Stop" loses nothing; a page the library cannot take is kept in
/// [`backup_dir`].
pub fn main_window_closed(app: &AppHandle) {
    if let (Some(narration), Some(runtime)) = (
        app.try_state::<NarrationRuntime>(),
        app.try_state::<LiveRuntime>(),
    ) {
        finish_live_narration(&narration, &runtime.hub);
    }
    if let Some(tablet) = app.try_state::<TabletRuntime>() {
        let _ = tablet.with_service(TabletService::stop_live);
    }
    if let Some(runtime) = app.try_state::<LiveRuntime>() {
        close_session(&runtime.hub, &backup_dir());
    }
}

/// Where the pages that could not be written to the library go when the app ends:
/// `%LOCALAPPDATA%\Ardoise\live-backup` (local to this PC, outside the library); under the
/// temporary folder when `LOCALAPPDATA` is not set (Linux).
pub fn backup_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|dir| !dir.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("Ardoise")
        .join("live-backup")
}

/// Stops the session and writes its pages to the library; every page that cannot be written
/// there gets a complete copy in `backup`, whose path is logged (standard error, and
/// `live-backup.log` next to the copies, since a windowed app has no console). Returns the
/// copies.
fn close_session(hub: &LiveHub, backup: &Path) -> Vec<PathBuf> {
    if let Err(error) = hub.stop() {
        eprintln!("live mode: cannot write the pages to the library when closing: {error}");
    }
    let mut copies = Vec::new();
    for (index, copy) in hub.back_up_unsaved(backup) {
        let line = match &copy {
            Ok(path) => format!(
                "page {} could not be written to the library; backup copy: {}",
                index + 1,
                path.display()
            ),
            Err(error) => format!(
                "page {} could not be written to the library, nor its backup copy: {error}",
                index + 1
            ),
        };
        eprintln!("live mode: {line}");
        log_backup(backup, &line);
        if let Ok(path) = copy {
            copies.push(path);
        }
    }
    copies
}

/// Appends `line` (with the time) to `live-backup.log` in `dir`. Best effort.
fn log_backup(dir: &Path, line: &str) {
    use std::io::Write as _;
    let written = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("live-backup.log"))
        .and_then(|mut log| {
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            writeln!(log, "{now} {line}")
        });
    if let Err(error) = written {
        eprintln!("live mode: cannot write live-backup.log: {error}");
    }
}

/// Sends the session to this window from now on (one subscription per window: a new one
/// replaces the previous). The first batch starts with the snapshot returned here.
#[tauri::command]
pub async fn live_subscribe(
    webview: Webview,
    runtime: State<'_, LiveRuntime>,
    on_event: Channel<LiveBatch>,
) -> CommandResult<LiveSnapshot> {
    Ok(runtime.hub.subscribe(webview.label(), Arc::new(on_event)))
}

/// Starts live mode (or returns the session running): a new session waits for the tablet,
/// which streams as soon as it is connected.
#[tauri::command]
pub async fn live_start(
    context: State<'_, AppContext>,
    tablet: State<'_, TabletRuntime>,
    runtime: State<'_, LiveRuntime>,
    narration: State<'_, NarrationRuntime>,
) -> CommandResult<LiveSnapshot> {
    let library = library_of(&context)?;
    // A narration still recording for a session that stopped by itself (the tablet's service
    // left live mode) belongs to that session's pages, not to the new one.
    if !runtime.hub.page_position().is_some_and(|p| p.running) {
        finish_live_narration(&narration, &runtime.hub);
    }
    let size = tablet
        .with_service(|service| tablet_size(&service.status()))
        .ok()
        .flatten()
        .unwrap_or(ardoise_core::note::A4_LANDSCAPE);
    let options = StartOptions {
        library,
        french: report_language(&context) == "fr",
        size,
        button: button_setting(&context),
    };
    match runtime
        .hub
        .start(options)
        .map_err(LiveError::command_error)?
    {
        Start::Running(snapshot) => Ok(snapshot),
        Start::New { snapshot, sink } => {
            let id = sink.session();
            // The hub's lock is not held here: the service may call the sink at once.
            match tablet.with_service(move |service| service.start_live(Box::new(sink))) {
                Ok(()) => Ok(snapshot),
                Err(error) => {
                    runtime.hub.abort_start(id);
                    Err(error)
                }
            }
        }
    }
}

/// Leaves live mode and writes every page; returns the paths of the session's notes. A
/// narration recording in live mode is stopped first and goes with its pages.
#[tauri::command]
pub async fn live_stop(
    tablet: State<'_, TabletRuntime>,
    runtime: State<'_, LiveRuntime>,
    narration: State<'_, NarrationRuntime>,
) -> CommandResult<Vec<String>> {
    finish_live_narration(&narration, &runtime.hub);
    // Without a running service there is nothing to stop on the tablet's side.
    let _ = tablet.with_service(TabletService::stop_live);
    runtime.hub.stop().map_err(LiveError::command_error)
}

#[tauri::command]
pub async fn live_set_style(
    runtime: State<'_, LiveRuntime>,
    style: LiveStyle,
) -> CommandResult<()> {
    runtime
        .hub
        .set_style(style)
        .map_err(LiveError::command_error)
}

#[tauri::command]
pub async fn live_command(
    runtime: State<'_, LiveRuntime>,
    command: LiveCommand,
) -> CommandResult<()> {
    runtime
        .hub
        .command(command.kind)
        .map_err(LiveError::command_error)
}

#[tauri::command]
pub async fn live_set_view(
    runtime: State<'_, LiveRuntime>,
    view: LiveViewState,
) -> CommandResult<()> {
    runtime.hub.set_view(view).map_err(LiveError::command_error)
}
