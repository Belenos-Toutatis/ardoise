//! Live mode and the classroom (SPEC §4.2): the live session (pages that become notes, layers,
//! style, blackboard, laser pointer, view of the projected area) shared by every window, fed by
//! the tablet service and sent to the windows in batches through Tauri channels.
//!
//! Contract: docs/design/tablet-live-classroom.md (work package B), "Live session".
//!
//! - [`model`]: the shapes the interface receives and sends (`app/ui/src/lib/live.ts`);
//! - [`session`]: the session itself, pure (strokes, hover, pages, layers, view);
//! - [`hub`]: the session shared between threads, the batches sent to the windows, the pages
//!   written to the library.

pub mod hub;
pub mod model;
pub mod session;

#[cfg(test)]
mod tests;

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

pub use hub::{ButtonSetting, HubConfig, LiveError, LiveHub, Start, StartOptions};
pub use model::{LiveBatch, LiveCommand, LiveSnapshot, LiveStyle, LiveViewState};

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
    fn command_error(self) -> CommandError {
        match self {
            LiveError::NotRunning => CommandError::new("liveNotRunning", self),
            LiveError::Library(error) => library_error(error),
        }
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
/// closing the app without "Stop" loses nothing.
pub fn main_window_closed(app: &AppHandle) {
    if let Some(tablet) = app.try_state::<TabletRuntime>() {
        let _ = tablet.with_service(TabletService::stop_live);
    }
    if let Some(runtime) = app.try_state::<LiveRuntime>()
        && let Err(error) = runtime.hub.stop()
    {
        eprintln!("live mode: cannot write the pages when closing: {error}");
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
) -> CommandResult<LiveSnapshot> {
    let library = library_of(&context)?;
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

/// Leaves live mode and writes every page; returns the paths of the session's notes.
#[tauri::command]
pub async fn live_stop(
    tablet: State<'_, TabletRuntime>,
    runtime: State<'_, LiveRuntime>,
) -> CommandResult<Vec<String>> {
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
