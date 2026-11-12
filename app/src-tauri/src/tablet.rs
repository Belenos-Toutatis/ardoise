//! The tablet in the interface (SPEC §4.1): the registered tablet (`_ardoise/device.json`,
//! shared by every computer of the library), and the commands of the tablet service
//! (`ardoise_device::service`): status, registration, sync, settings, the simulated tablet.
//! The Xiaomi blackboard (`_ardoise/blackboard.json`) is added, synced, used in live mode and
//! forgotten through the same service ([`tablet_find_blackboards`], [`tablet_add_blackboard`],
//! [`tablet_forget_blackboard`]).
//!
//! Contract: docs/design/tablet-live-classroom.md (work package A), docs/design/blackboard.md
//! (the blackboard's commands and status fields).
//!
//! - [`TabletRuntime`] holds the service of the running app, the windows subscribed to its
//!   status, and the simulated tablet of `--simulated-tablet`. The guided test of the real
//!   tablet stops the service while it runs ([`TabletPause`]).
//! - The service writes each synced page through [`store_page_in`]: the note (thumbnail,
//!   checked by reading it back) in the library root, then the automatic export as the settings
//!   say at that moment.
//! - Every command has a `*_in` function holding its logic, tested without Tauri.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use ardoise_ble::{AdapterInfo, AdapterStatus, BleError};
use ardoise_core::library::Library;
use ardoise_core::settings::{Language, Orientation, Settings, SettingsStore};
use ardoise_device::identity::{self, IdentityError};
use ardoise_device::pages::LibrarySink;
use ardoise_device::service::{
    self, Connection, ConnectorFactory, NoHooks, PageContext, PageStore, ServiceConfig,
    ServiceHooks, StatusCallback, StatusError, TabletOptions, TabletService, TabletStatus,
};
use ardoise_device::session::{DeviceError, DownloadedPage, valid_name};
use ardoise_device::transport::{Connector, TransportError};
use ardoise_sim::realtime::{APP_HOOK_DELAY, SimFactory, SimHooks, app_board, app_tablet};
use ardoise_sim::{SimBoard, SimDevice, SimTablet};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager as _, State, Webview};

use crate::AppContext;
use crate::autoexport::{AutoExportReport, after_save_hook};
use crate::commands::{CommandError, CommandResult, current_state};
use crate::library::{first_layer_name, library_of, now_ms};

/// `TabletInfo` in `note.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabletInfo {
    pub name: String,
    pub address: String,
    /// RFC 3339.
    pub registered: String,
    pub width: u32,
    pub height: u32,
}

fn identity_error(error: IdentityError) -> CommandError {
    CommandError::new("libraryAccess", error)
}

/// A device error for the interface (`errorCodes.*`).
fn device_error(error: DeviceError) -> CommandError {
    CommandError::new(service::error_code(&error), error)
}

pub fn tablet_info_in(library: &Library) -> CommandResult<Option<TabletInfo>> {
    let record = identity::load(library.root()).map_err(identity_error)?;
    Ok(record.map(|record| TabletInfo {
        name: record.name,
        address: record.address,
        registered: record.registered,
        width: record.width,
        height: record.height,
    }))
}

pub fn forget_tablet_in(library: &Library) -> CommandResult<()> {
    identity::forget(library.root()).map_err(identity_error)
}

/// What `tablet_subscribe` sends (`TabletEvent` in `tablet.ts`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TabletEvent {
    Status { status: TabletStatus },
}

/// Receives the tablet events of one window; returns `false` once the window is gone (the
/// subscriber is then dropped).
pub type Subscriber = Box<dyn Fn(&TabletEvent) -> bool + Send + Sync>;

/// The subscribers with the label of their window: one per window, the latest.
type Subscribers = Arc<Mutex<Vec<(String, Subscriber)>>>;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sends `status` to every subscriber, dropping those whose window is gone.
fn broadcast(subscribers: &Subscribers, status: &TabletStatus) {
    let event = TabletEvent::Status {
        status: status.clone(),
    };
    lock(subscribers).retain(|(_, subscriber)| subscriber(&event));
}

fn not_running() -> CommandError {
    CommandError::new("tablet", "the tablet service is not running")
}

/// The simulated tablets replacing Bluetooth (`--simulated-tablet`, `--simulated-blackboard`).
pub struct Simulation {
    /// The simulated Bamboo Slate, if any.
    pub tablet: Option<SimTablet>,
    /// The simulated Xiaomi blackboard, if any.
    pub board: Option<SimBoard>,
    /// Holds the simulated registration and the simulated blackboard's file: never the library
    /// (a throwaway folder).
    pub identity_root: Option<PathBuf>,
    /// Delay of the simulated user's actions (holding and pressing the button).
    pub hook_delay: Duration,
}

impl Simulation {
    /// The simulated tablets of `ardoise.exe --simulated-tablet` (`slate`: a Bamboo Slate Large
    /// in real time holding the three reference pages) and `--simulated-blackboard` (`board`: the
    /// Xiaomi blackboard in real time holding the three reference pages, on the Slate's clock
    /// when both are simulated), the user acting after 1 s, the registration in
    /// `%TEMP%\ardoise-simulation-*` (like `--diagnostic --simulated`).
    pub fn app(slate: bool, board: bool) -> Simulation {
        let identity_root = match crate::diagnostic::simulation_library() {
            Ok(dir) => Some(dir),
            Err(error) => {
                eprintln!("Ardoise: cannot create the simulated tablet's folder: {error}");
                None
            }
        };
        let tablet = slate.then(app_tablet);
        let board = board.then(|| app_board(tablet.as_ref().map(SimTablet::clock)));
        Simulation {
            tablet,
            board,
            identity_root,
            hook_delay: APP_HOOK_DELAY,
        }
    }

    /// The simulated devices in range.
    fn devices(&self) -> Vec<SimDevice> {
        let slate = self.tablet.clone().map(SimDevice::Slate);
        let board = self.board.clone().map(SimDevice::Board);
        slate.into_iter().chain(board).collect()
    }

    /// The person in front of them.
    fn hooks(&self) -> SimHooks {
        let hooks = match (&self.tablet, &self.board) {
            (Some(tablet), _) => SimHooks::with_delay(tablet.clone(), self.hook_delay),
            (None, Some(board)) => SimHooks::for_board(board.clone()),
            (None, None) => SimHooks::with_delay(app_tablet(), self.hook_delay),
        };
        match (&self.tablet, &self.board) {
            (Some(_), Some(board)) => hooks.with_board(board.clone()),
            _ => hooks,
        }
    }
}

/// Bluetooth LE through `ardoise_ble`, the adapter checked before each search.
struct BleFactory;

impl ConnectorFactory for BleFactory {
    fn check(&mut self) -> Result<(), StatusError> {
        adapter_check(ardoise_ble::adapter_info())
    }

    fn connector(&mut self) -> Result<Box<dyn Connector>, TransportError> {
        Ok(Box::new(ardoise_ble::BleConnector::new()?))
    }
}

/// `bluetoothOff` / `bluetoothMissing` from the state of the adapter.
fn adapter_check(info: Result<AdapterInfo, BleError>) -> Result<(), StatusError> {
    match info {
        Ok(AdapterInfo {
            status: AdapterStatus::On,
            ..
        }) => Ok(()),
        Ok(AdapterInfo {
            status: AdapterStatus::Off,
            ..
        }) => Err(StatusError::from(&DeviceError::Transport(
            TransportError::AdapterOff,
        ))),
        Ok(AdapterInfo { status, .. }) => Err(StatusError::new(
            "bluetoothMissing",
            format!("Bluetooth adapter: {status}"),
        )),
        Err(error) => Err(StatusError::new("bluetoothMissing", error)),
    }
}

/// `"fr"` or `"en"`, as the interface (and `diagnostic::report_language`) chooses it.
pub(crate) fn interface_language(settings: &Settings, system_locale: Option<&str>) -> &'static str {
    let french = match settings.language {
        Language::Fr => true,
        Language::En => false,
        Language::Auto => system_locale.is_some_and(|l| l.to_lowercase().starts_with("fr")),
    };
    if french { "fr" } else { "en" }
}

/// Writes a synced page as a new note in the library root (`root`): the points turned for
/// the orientation of `context`, a thumbnail, the note read back to check it (`LibrarySink::store`).
/// Then the automatic export, with the settings read now: a failed export is logged and never
/// fails the sync (the note is safe). An `Err` keeps the page on the tablet.
pub fn store_page_in(
    settings: Option<&SettingsStore>,
    system_locale: Option<&str>,
    root: &Path,
    page: DownloadedPage,
    context: &PageContext,
) -> Result<PathBuf, String> {
    let settings = match settings.map(SettingsStore::load) {
        Some(Ok(settings)) => settings,
        Some(Err(error)) => {
            eprintln!("Ardoise: settings unreadable, no automatic export: {error}");
            Settings::default()
        }
        None => Settings::default(),
    };
    let library = Library::open(root).map_err(|e| e.to_string())?;
    let layer = first_layer_name(interface_language(&settings, system_locale));
    let mut report = AutoExportReport::default();
    let saved = {
        let mut sink = LibrarySink::new(&library, "", context.size, layer, now_ms)
            .with_orientation(context.orientation)
            .with_thumbnail(crate::thumbnails::thumbnail_png)
            .with_after_save(after_save_hook(
                &settings.auto_export,
                &library,
                |saved, outcome| report.record(saved, outcome),
            ));
        sink.store(page)?
    };
    for (path, error) in &report.failures {
        eprintln!(
            "Ardoise: automatic export of {} failed: {error}",
            path.display()
        );
    }
    for (path, dpi) in &report.lowered {
        eprintln!(
            "Ardoise: {} exported at {dpi} dpi (page too large)",
            path.display()
        );
    }
    Ok(saved.path)
}

/// The tablet service of the running app (see [`start_service`]).
#[derive(Default)]
pub struct TabletRuntime {
    /// Shared, so that a command waiting for the tablet does not hold the lock.
    service: Mutex<Option<Arc<TabletService>>>,
    subscribers: Subscribers,
    /// The simulated tablet, with `--simulated-tablet`.
    simulated: Mutex<Option<SimTablet>>,
    /// The simulated blackboard, with `--simulated-blackboard`.
    simulated_board: Mutex<Option<SimBoard>>,
    /// The simulated tablet's registration folder, which replaces the library's.
    identity_override: Mutex<Option<PathBuf>>,
    /// The status shown while the guided test holds the real tablet (the service is stopped
    /// meanwhile, see [`TabletRuntime::pause`]).
    paused: Mutex<Option<TabletStatus>>,
}

impl TabletRuntime {
    /// Runs `f` with the service; `tablet` error when it is not running. The lock is released
    /// before `f` runs: a request waiting for the tablet (up to `service::REQUEST_TIMEOUT`)
    /// does not hold up the other commands.
    pub fn with_service<T>(&self, f: impl FnOnce(&TabletService) -> T) -> CommandResult<T> {
        let service = lock(&self.service).clone();
        match service {
            Some(service) => Ok(f(&service)),
            None => Err(not_running()),
        }
    }

    /// The status of the service, or the one shown while the guided test holds the tablet.
    pub fn status(&self) -> CommandResult<TabletStatus> {
        match self.with_service(TabletService::status) {
            Ok(status) => Ok(status),
            Err(error) => lock(&self.paused).clone().ok_or(error),
        }
    }

    /// Started with `--simulated-tablet` or `--simulated-blackboard`.
    fn is_simulated(&self) -> bool {
        lock(&self.simulated).is_some() || lock(&self.simulated_board).is_some()
    }

    /// Starts the service for this PC's library and `[tablet]` settings: Bluetooth, or the
    /// simulated tablet. A service already running is stopped first.
    pub fn start_in(&self, context: &AppContext, simulation: Option<Simulation>) {
        // Two services would compete for the tablet.
        self.stop();
        *lock(&self.paused) = None;
        let library = current_state(context).library().map(PathBuf::from);
        let store = context.store.as_ref().ok().cloned();
        let settings = store
            .as_ref()
            .and_then(|store| store.load().ok())
            .unwrap_or_default();
        let options = TabletOptions::from(&settings.tablet);
        let (factory, hooks, identity_root): (
            Box<dyn ConnectorFactory>,
            Arc<dyn ServiceHooks>,
            Option<PathBuf>,
        ) = match &simulation {
            Some(simulation) => (
                Box::new(SimFactory::of(simulation.devices(), true)),
                Arc::new(simulation.hooks()),
                simulation.identity_root.clone(),
            ),
            None => (Box::new(BleFactory), Arc::new(NoHooks), library.clone()),
        };
        let simulated = simulation.is_some();
        *lock(&self.simulated) = simulation.as_ref().and_then(|s| s.tablet.clone());
        *lock(&self.simulated_board) = simulation.as_ref().and_then(|s| s.board.clone());
        *lock(&self.identity_override) = simulation.and_then(|s| s.identity_root);
        let config = ServiceConfig {
            library,
            identity_root,
            options,
            simulated,
        };
        let subscribers = self.subscribers.clone();
        let on_status: StatusCallback = Box::new(move |status| broadcast(&subscribers, status));
        let locale = context.system_locale.clone();
        let store_page: PageStore = Box::new(move |root, page, context| {
            store_page_in(store.as_ref(), locale.as_deref(), root, page, context)
        });
        let service = TabletService::start(config, factory, hooks, on_status, store_page);
        *lock(&self.service) = Some(Arc::new(service));
    }

    /// The library of this PC changed: the service follows it (the simulated tablet keeps its
    /// own registration folder).
    pub fn library_changed_in(&self, context: &AppContext) -> CommandResult<()> {
        let library = current_state(context).library().map(PathBuf::from);
        let identity_root = match lock(&self.identity_override).clone() {
            Some(root) => Some(root),
            None if self.is_simulated() => None,
            None => library.clone(),
        };
        self.with_service(|service| service.set_library(library, identity_root))
    }

    /// Adds the subscriber of the window `label`, replacing the window's previous one.
    pub fn subscribe(&self, label: &str, subscriber: Subscriber) {
        let mut subscribers = lock(&self.subscribers);
        subscribers.retain(|(window, _)| window != label);
        subscribers.push((label.to_owned(), subscriber));
    }

    /// Stops the service (closes the link).
    pub fn stop(&self) {
        let service = lock(&self.service).take();
        if let Some(service) = service {
            service.shutdown();
        }
    }

    /// Stops the service as the application exits: the link is closed, and the log of a
    /// connection that must be kept ends with the disconnection and its end line. Waits at most
    /// `limit`, so that a Bluetooth operation that cannot be cut short never holds up the exit.
    pub fn stop_within(&self, limit: Duration) {
        let Some(service) = lock(&self.service).take() else {
            return;
        };
        let (done, finished) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("tablet-stop".into())
            .spawn(move || {
                service.shutdown();
                let _ = done.send(());
            });
        if spawned.is_ok() {
            let _ = finished.recv_timeout(limit);
        }
    }

    /// Stops the service while the guided test talks to the real tablet: two programs cannot
    /// hold it at once, and the test may register it again. The windows are told the tablet is
    /// busy until [`resume_in`](Self::resume_in). Returns whether the service was stopped:
    /// the simulated tablet (`--simulated-tablet`) does not use Bluetooth and keeps running.
    pub fn pause(&self) -> bool {
        if self.is_simulated() {
            return false;
        }
        let Some(service) = lock(&self.service).take() else {
            return false;
        };
        service.shutdown();
        let mut status = service.status();
        status.connection = Connection::Unavailable;
        status.active = None;
        status.board_search = None;
        status.register_step = None;
        status.sync = None;
        status.pending = None;
        status.error = Some(StatusError::new(
            "tabletBusy",
            "the guided tablet test is using the tablet",
        ));
        *lock(&self.paused) = Some(status.clone());
        broadcast(&self.subscribers, &status);
        true
    }

    /// Starts the service again after [`pause`](Self::pause), with the library and the
    /// settings of now; it reads `device.json` again (the test may have registered the tablet).
    pub fn resume_in(&self, context: &AppContext) {
        let paused = lock(&self.paused).take().is_some();
        if paused && lock(&self.service).is_none() {
            self.start_in(context, None);
        }
    }
}

/// Keeps the tablet service stopped while the guided test uses the real tablet
/// ([`TabletRuntime::pause`]); starts it again when dropped: the test ended, was cancelled,
/// failed or panicked.
pub struct TabletPause {
    app: AppHandle,
    paused: bool,
}

impl TabletPause {
    pub fn new(app: &AppHandle) -> TabletPause {
        let paused = app
            .try_state::<TabletRuntime>()
            .is_some_and(|runtime| runtime.pause());
        TabletPause {
            app: app.clone(),
            paused,
        }
    }
}

impl Drop for TabletPause {
    fn drop(&mut self) {
        if self.paused
            && let (Some(runtime), Some(context)) = (
                self.app.try_state::<TabletRuntime>(),
                self.app.try_state::<AppContext>(),
            )
        {
            runtime.resume_in(&context);
        }
    }
}

/// Starts the tablet service for this PC's library (once, from `setup`). With `slate` or
/// `board`, the simulated Slate, the simulated blackboard or both, in real time, replace
/// Bluetooth.
pub fn start_service(app: &AppHandle, slate: bool, board: bool) {
    let simulation = (slate || board).then(|| Simulation::app(slate, board));
    let context = app.state::<AppContext>();
    app.state::<TabletRuntime>().start_in(&context, simulation);
}

/// The library changed (`set_library`): the service follows it.
pub fn library_changed(app: &AppHandle) {
    let context = app.state::<AppContext>();
    if let Err(error) = app.state::<TabletRuntime>().library_changed_in(&context) {
        eprintln!("Ardoise: the tablet service cannot follow the library: {error:?}");
    }
}

/// Runs a command that may wait for the tablet (up to `service::REQUEST_TIMEOUT`) off the
/// async runtime.
async fn blocking<T: Send + 'static>(
    app: AppHandle,
    run: impl FnOnce(&AppHandle) -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(move || run(&app))
        .await
        .map_err(|error| CommandError::new("tablet", error))?
}

/// Subscribes the window `label` (replacing its previous subscription) and returns the status.
pub fn tablet_subscribe_in(
    runtime: &TabletRuntime,
    label: &str,
    subscriber: Subscriber,
) -> CommandResult<TabletStatus> {
    runtime.status()?;
    // Subscribed first: a change right now is sent, never lost.
    runtime.subscribe(label, subscriber);
    runtime.status()
}

pub fn tablet_rename_in(runtime: &TabletRuntime, name: &str) -> CommandResult<TabletStatus> {
    let name = name.trim();
    if !valid_name(name) {
        return Err(CommandError::new("invalidName", name));
    }
    runtime
        .with_service(|service| service.rename(name))?
        .map_err(device_error)?;
    runtime.with_service(TabletService::status)
}

pub fn tablet_set_time_in(runtime: &TabletRuntime) -> CommandResult<i64> {
    runtime
        .with_service(TabletService::set_time)?
        .map_err(device_error)
}

/// Saves `[tablet]` in `ardoise.toml` (shared by every PC; unknown keys kept), then the
/// service follows the new options.
pub fn tablet_set_options_in(
    runtime: &TabletRuntime,
    context: &AppContext,
    options: TabletOptions,
) -> CommandResult<TabletStatus> {
    crate::commands::update(context, |settings| {
        settings.tablet.auto_sync = options.auto_sync;
        settings.tablet.delete_after_import = options.delete_after_import;
        settings.tablet.orientation = options.orientation;
        settings.tablet.button = options.button;
    })?;
    let mut status = runtime.with_service(|service| {
        service.set_options(options);
        service.status()
    })?;
    // The worker publishes them a moment later.
    status.options = options;
    Ok(status)
}

/// The quick switch of the live window: saves how the tablet is held in `[tablet]` and tells the
/// service, which turns the live page at once (`LiveInput::Reoriented`). Without a running
/// service only the setting changes.
pub fn tablet_set_orientation_in(
    runtime: &TabletRuntime,
    context: &AppContext,
    orientation: Orientation,
) -> CommandResult<()> {
    let settings =
        crate::commands::store(context)?.update(|s| s.tablet.orientation = orientation)?;
    let options = TabletOptions::from(&settings.tablet);
    let _ = runtime.with_service(|service| service.set_options(options));
    Ok(())
}

pub fn sim_press_button_in(runtime: &TabletRuntime) -> CommandResult<()> {
    match lock(&runtime.simulated).as_ref() {
        Some(tablet) => {
            tablet.press_button();
            Ok(())
        }
        None => Err(CommandError::new("simulationOnly", "no simulated tablet")),
    }
}

/// Adds the blackboard heard at `address` by the search under way (`tablet_find_blackboards`):
/// `blackboard.json` is written, then the service connects to it. Returns the status.
pub fn tablet_add_blackboard_in(
    runtime: &TabletRuntime,
    address: &str,
) -> CommandResult<TabletStatus> {
    runtime
        .with_service(|service| service.add_blackboard(address))?
        .map_err(device_error)?;
    runtime.with_service(TabletService::status)
}

/// Forgets the blackboard (`blackboard.json`) as [`forget_tablet_through`] forgets the Slate:
/// through the service when it runs, else in the library; with the simulated tablets, only in
/// their throwaway folder.
pub fn forget_blackboard_through(
    runtime: &TabletRuntime,
    context: &AppContext,
) -> CommandResult<()> {
    match runtime.with_service(TabletService::forget_blackboard) {
        Ok(result) => result.map_err(device_error),
        Err(_) if runtime.is_simulated() => match lock(&runtime.identity_override).clone() {
            Some(root) => identity::forget_blackboard(&root).map_err(identity_error),
            None => Ok(()),
        },
        Err(_) => identity::forget_blackboard(library_of(context)?.root()).map_err(identity_error),
    }
}

/// Presses the simulated blackboard's store button (`--simulated-blackboard` only).
pub fn sim_blackboard_store_page_in(runtime: &TabletRuntime) -> CommandResult<()> {
    match lock(&runtime.simulated_board).as_ref() {
        Some(board) => {
            board.press_store();
            Ok(())
        }
        None => Err(CommandError::new(
            "simulationOnly",
            "no simulated blackboard",
        )),
    }
}

/// Forgets the registered tablet: through the service when it runs (it also closes the link;
/// with the simulated tablet only its throwaway registration is forgotten), else in the
/// library. With the simulated tablet the library's registration (the real tablet's) is never
/// touched, even when the service is not running.
pub fn forget_tablet_through(runtime: &TabletRuntime, context: &AppContext) -> CommandResult<()> {
    match runtime.with_service(TabletService::forget) {
        Ok(result) => result.map_err(device_error),
        Err(_) if runtime.is_simulated() => match lock(&runtime.identity_override).clone() {
            Some(root) => identity::forget(&root).map_err(identity_error),
            None => Ok(()),
        },
        Err(_) => forget_tablet_in(&library_of(context)?),
    }
}

#[tauri::command]
pub async fn tablet_status(runtime: State<'_, TabletRuntime>) -> CommandResult<TabletStatus> {
    runtime.status()
}

#[tauri::command]
pub async fn tablet_subscribe(
    webview: Webview,
    runtime: State<'_, TabletRuntime>,
    on_event: Channel<TabletEvent>,
) -> CommandResult<TabletStatus> {
    tablet_subscribe_in(
        &runtime,
        webview.label(),
        Box::new(move |event| on_event.send(event.clone()).is_ok()),
    )
}

#[tauri::command]
pub async fn tablet_register(runtime: State<'_, TabletRuntime>) -> CommandResult<()> {
    runtime.with_service(TabletService::register)
}

#[tauri::command]
pub async fn tablet_cancel(runtime: State<'_, TabletRuntime>) -> CommandResult<()> {
    runtime.with_service(TabletService::cancel)
}

#[tauri::command]
pub async fn tablet_sync(runtime: State<'_, TabletRuntime>) -> CommandResult<()> {
    runtime.with_service(TabletService::sync_now)
}

#[tauri::command]
pub async fn tablet_rename(app: AppHandle, name: String) -> CommandResult<TabletStatus> {
    blocking(app, move |app| {
        tablet_rename_in(&app.state::<TabletRuntime>(), &name)
    })
    .await
}

#[tauri::command]
pub async fn tablet_set_time(app: AppHandle) -> CommandResult<i64> {
    blocking(app, |app| tablet_set_time_in(&app.state::<TabletRuntime>())).await
}

#[tauri::command]
pub async fn tablet_set_options(
    runtime: State<'_, TabletRuntime>,
    context: State<'_, AppContext>,
    options: TabletOptions,
) -> CommandResult<TabletStatus> {
    tablet_set_options_in(&runtime, &context, options)
}

#[tauri::command]
pub async fn tablet_set_orientation(
    runtime: State<'_, TabletRuntime>,
    context: State<'_, AppContext>,
    orientation: Orientation,
) -> CommandResult<()> {
    tablet_set_orientation_in(&runtime, &context, orientation)
}

/// Presses the simulated tablet's button (`--simulated-tablet` only, for the e2e journeys).
#[tauri::command]
pub async fn sim_press_button(runtime: State<'_, TabletRuntime>) -> CommandResult<()> {
    sim_press_button_in(&runtime)
}

/// Starts the search for Xiaomi blackboards nearby (`connection: adding`, what is heard in
/// `boardSearch`).
#[tauri::command]
pub async fn tablet_find_blackboards(runtime: State<'_, TabletRuntime>) -> CommandResult<()> {
    runtime.with_service(TabletService::find_blackboards)
}

#[tauri::command]
pub async fn tablet_add_blackboard(app: AppHandle, address: String) -> CommandResult<TabletStatus> {
    blocking(app, move |app| {
        tablet_add_blackboard_in(&app.state::<TabletRuntime>(), &address)
    })
    .await
}

#[tauri::command]
pub async fn tablet_forget_blackboard(app: AppHandle) -> CommandResult<()> {
    blocking(app, |app| {
        forget_blackboard_through(&app.state::<TabletRuntime>(), &app.state::<AppContext>())
    })
    .await
}

/// Presses the simulated blackboard's store button (`--simulated-blackboard` only, for the e2e
/// journeys).
#[tauri::command]
pub async fn sim_blackboard_store_page(runtime: State<'_, TabletRuntime>) -> CommandResult<()> {
    sim_blackboard_store_page_in(&runtime)
}

#[tauri::command]
pub async fn tablet_info(context: State<'_, AppContext>) -> CommandResult<Option<TabletInfo>> {
    tablet_info_in(&library_of(&context)?)
}

#[tauri::command]
pub async fn forget_tablet(app: AppHandle) -> CommandResult<()> {
    blocking(app, |app| {
        forget_tablet_through(&app.state::<TabletRuntime>(), &app.state::<AppContext>())
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::library;
    use ardoise_core::note::A4_LANDSCAPE;
    use ardoise_core::settings::{AutoExport, AutoExportFormat, LiveButton, Orientation};
    use ardoise_device::identity::{DEVICE_FORMAT, DeviceRecord, HostId};
    use ardoise_device::service::{Connection, RegisterStep};
    use std::sync::mpsc;
    use std::time::Instant;

    #[test]
    fn shows_and_forgets_the_registered_tablet() {
        let (_dir, library) = library();
        assert_eq!(tablet_info_in(&library).unwrap(), None);
        // Forgetting when nothing is registered is fine.
        forget_tablet_in(&library).unwrap();

        let record = DeviceRecord {
            format: DEVICE_FORMAT,
            uuid: HostId([0x89, 0xea, 0x78, 0x88, 0xff, 0xcb]),
            address: "E2:43:03:67:0E:01".into(),
            name: "Bamboo Slate".into(),
            protocol: "slate".into(),
            width: 29700,
            height: 21600,
            registered: "2026-10-24T14:03:12+02:00".into(),
            extra: Default::default(),
        };
        identity::save(library.root(), &record).unwrap();
        let info = tablet_info_in(&library).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            serde_json::json!({
                "name": "Bamboo Slate",
                "address": "E2:43:03:67:0E:01",
                "registered": "2026-10-24T14:03:12+02:00",
                "width": 29700,
                "height": 21600,
            })
        );

        forget_tablet_in(&library).unwrap();
        assert_eq!(tablet_info_in(&library).unwrap(), None);

        std::fs::write(identity::device_file(library.root()), "{broken").unwrap();
        assert_eq!(
            tablet_info_in(&library).unwrap_err().code(),
            "libraryAccess"
        );
    }

    #[test]
    fn adapter_states_become_translated_errors() {
        let info = |status| {
            Ok(AdapterInfo {
                status,
                name: Some("Intel".into()),
            })
        };
        assert_eq!(adapter_check(info(AdapterStatus::On)), Ok(()));
        assert_eq!(
            adapter_check(info(AdapterStatus::Off)).unwrap_err().code,
            "bluetoothOff"
        );
        for status in [AdapterStatus::NotFound, AdapterStatus::NoLowEnergy] {
            assert_eq!(
                adapter_check(info(status)).unwrap_err().code,
                "bluetoothMissing"
            );
        }
        let failed = adapter_check(Err(BleError::Windows("no WinRT".into())));
        assert_eq!(failed.unwrap_err().code, "bluetoothMissing");
        // Off Windows there is never an adapter.
        #[cfg(not(windows))]
        assert_eq!(BleFactory.check().unwrap_err().code, "bluetoothMissing");
    }

    #[test]
    fn the_layer_is_named_in_the_interface_language() {
        let mut settings = Settings::default();
        assert_eq!(interface_language(&settings, Some("fr-FR")), "fr");
        assert_eq!(interface_language(&settings, Some("en-GB")), "en");
        assert_eq!(interface_language(&settings, None), "en");
        settings.language = Language::Fr;
        assert_eq!(interface_language(&settings, None), "fr");
        settings.language = Language::En;
        assert_eq!(interface_language(&settings, Some("fr-FR")), "en");
    }

    fn context(settings_dir: &Path) -> AppContext {
        AppContext {
            store: Ok(SettingsStore::locate(settings_dir, settings_dir, "PC-TEST")),
            system_locale: Some("fr-FR".to_owned()),
            self_test: None,
            open_diagnostic: false,
        }
    }

    /// A page as the simulated tablet sends it.
    fn page() -> DownloadedPage {
        let tablet = SimTablet::slate_large();
        let config = tablet.config();
        tablet.draw_page(&ardoise_sim::drawings::square_and_circle(
            config.width,
            config.height,
        ));
        let stored = &tablet.stored_pages()[0];
        DownloadedPage {
            bytes: stored.bytes.clone(),
            timestamp: Some(stored.timestamp),
            announced_size: u32::try_from(stored.bytes.len()).unwrap(),
            kind: ardoise_device::transport::TabletKind::Slate,
        }
    }

    #[test]
    fn a_synced_page_becomes_a_note_with_its_thumbnail_and_export() {
        let settings = tempfile::tempdir().unwrap();
        let (dir, library) = library();
        let context = context(settings.path());
        let store = context.store.as_ref().unwrap();
        store
            .update(|s| {
                s.auto_export = AutoExport {
                    format: AutoExportFormat::Pdf,
                    folder: "Exports".into(),
                    ..AutoExport::default()
                }
            })
            .unwrap();
        let page_context = PageContext {
            size: A4_LANDSCAPE,
            orientation: Orientation::Landscape,
        };
        let path = store_page_in(
            Some(store),
            Some("fr-FR"),
            dir.path(),
            page(),
            &page_context,
        )
        .unwrap();
        let note = library.read(&path).unwrap().file;
        assert_eq!(note.note.layers[0].name, "Calque 1");
        assert!(note.thumbnail.is_some());
        assert_eq!(path.parent(), Some(dir.path()), "in the library root");
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        assert!(
            dir.path()
                .join("Exports")
                .join(format!("{stem}.pdf"))
                .is_file()
        );

        // An export that cannot be made does not fail the sync.
        store
            .update(|s| s.auto_export.folder = "../outside".into())
            .unwrap();
        let again = store_page_in(Some(store), None, dir.path(), page(), &page_context).unwrap();
        assert!(library.read(&again).is_ok());
        assert_eq!(
            library.read(&again).unwrap().file.note.layers[0].name,
            "Layer 1"
        );
    }

    /// Waits for a status event accepted by `what`.
    fn wait_for(
        events: &mpsc::Receiver<TabletStatus>,
        what: impl Fn(&TabletStatus) -> bool,
    ) -> TabletStatus {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let status = events.recv_timeout(left).expect("tablet status in time");
            if what(&status) {
                return status;
            }
        }
    }

    /// The proof of the M4 item "synchro réelle depuis l'interface", without a window: the app's
    /// commands drive the simulated tablet of `--simulated-tablet` (real time, three reference
    /// pages) through the same functions as the Tauri commands; the three pages become three
    /// notes of the library and leave the tablet.
    #[test]
    fn app_commands_with_the_simulated_tablet_make_three_notes() {
        let settings = tempfile::tempdir().unwrap();
        let notes_dir = tempfile::tempdir().unwrap();
        let throwaway = tempfile::tempdir().unwrap();
        ardoise_core::library::init(throwaway.path()).unwrap();
        let context = context(settings.path());
        crate::commands::choose_library(&context, &notes_dir.path().display().to_string()).unwrap();

        let runtime = TabletRuntime::default();
        let simulation = Simulation {
            tablet: Some(app_tablet()),
            board: None,
            identity_root: Some(throwaway.path().to_owned()),
            // The app waits 1 s; a shorter wait keeps the test quick on the same path.
            hook_delay: Duration::from_millis(50),
        };
        let tablet = simulation.tablet.clone().unwrap();
        runtime.start_in(&context, Some(simulation));

        let (sender, events) = mpsc::channel();
        let sender = Mutex::new(sender);
        let first = tablet_subscribe_in(
            &runtime,
            "main",
            Box::new(move |TabletEvent::Status { status }| {
                lock(&sender).send(status.clone()).is_ok()
            }),
        )
        .unwrap();
        assert!(first.simulated);
        wait_for(&events, |s| s.connection == Connection::NotRegistered);
        // A window that went away is dropped at the next status.
        runtime.subscribe("gone", Box::new(|_| false));
        assert_eq!(lock(&runtime.subscribers).len(), 2);

        runtime.with_service(TabletService::register).unwrap();
        let status = wait_for(&events, |s| s.last_sync.is_some());
        assert_eq!(lock(&runtime.subscribers).len(), 1);
        assert_eq!(status.register_step, Some(RegisterStep::Done));
        assert_eq!(status.error, None);
        let last = status.last_sync.clone().unwrap();
        assert_eq!((last.saved.len(), last.deleted), (3, 3));
        assert_eq!(tablet.stored_page_count(), 0);

        let library = Library::open(notes_dir.path()).unwrap();
        let listing = library.list();
        assert!(listing.problems.is_empty());
        let names: Vec<String> = listing.notes.iter().map(|n| n.file_name.clone()).collect();
        assert_eq!(names.len(), 3);
        for note in &listing.notes {
            let read = library.read(&note.path).unwrap().file;
            assert!(read.thumbnail.is_some());
            assert_eq!(read.note.layers[0].name, "Calque 1");
        }
        println!(
            "simulated sync: 3 pages -> {} notes: {}",
            names.len(),
            names.join(", ")
        );
        // The simulated registration never touches the library's.
        assert_eq!(identity::load(notes_dir.path()).unwrap(), None);
        assert!(identity::load(throwaway.path()).unwrap().is_some());

        // Tablet settings through the commands.
        let renamed = tablet_rename_in(&runtime, "  Classe 3eB ").unwrap();
        assert_eq!(renamed.device.unwrap().name, "Classe 3eB");
        assert_eq!(
            tablet_rename_in(&runtime, "Ardoise de la salle 12")
                .unwrap_err()
                .code(),
            "invalidName"
        );
        let drift = tablet_set_time_in(&runtime).unwrap();
        assert!(drift.abs() <= 1, "{drift}");
        let options = TabletOptions {
            auto_sync: false,
            delete_after_import: true,
            orientation: Orientation::LandscapeFlipped,
            button: LiveButton::NewLayer,
        };
        let status = tablet_set_options_in(&runtime, &context, options).unwrap();
        assert_eq!(status.options, options);
        let saved = context.store.as_ref().unwrap().load().unwrap().tablet;
        assert_eq!(saved.orientation, Orientation::LandscapeFlipped);
        assert_eq!(saved.button, LiveButton::NewLayer);
        assert!(!saved.auto_sync);
        // The live window's quick switch changes the position only, and the service follows.
        tablet_set_orientation_in(&runtime, &context, Orientation::PortraitRight).unwrap();
        let saved = context.store.as_ref().unwrap().load().unwrap().tablet;
        assert_eq!(saved.orientation, Orientation::PortraitRight);
        assert_eq!(saved.button, LiveButton::NewLayer);
        assert!(!saved.auto_sync);
        // The worker publishes the new options a moment later.
        let deadline = Instant::now() + Duration::from_secs(10);
        while runtime.status().unwrap().options.orientation != Orientation::PortraitRight {
            assert!(Instant::now() < deadline, "the service did not follow");
            std::thread::sleep(Duration::from_millis(20));
        }
        sim_press_button_in(&runtime).unwrap();

        // Another library: the simulated registration stays in its own folder.
        let other = tempfile::tempdir().unwrap();
        crate::commands::choose_library(&context, &other.path().display().to_string()).unwrap();
        runtime.library_changed_in(&context).unwrap();
        wait_for(&events, |s| {
            s.connection == Connection::Connected && s.registered.is_some()
        });

        forget_tablet_through(&runtime, &context).unwrap();
        assert!(identity::load(throwaway.path()).unwrap().is_none());
        wait_for(&events, |s| s.connection == Connection::NotRegistered);
        // The simulated tablet does not use Bluetooth: the guided test leaves it running.
        assert!(!runtime.pause());
        assert!(runtime.with_service(|_| ()).is_ok());
        runtime.stop();
        assert_eq!(
            sim_press_button_in(&TabletRuntime::default())
                .unwrap_err()
                .code(),
            "simulationOnly"
        );
        assert_eq!(
            tablet_set_time_in(&runtime).unwrap_err().code(),
            "tablet",
            "the service is stopped"
        );
        // The service stopped, forgetting still never touches the real tablet's registration.
        let real = tempfile::tempdir().unwrap();
        crate::commands::choose_library(&context, &real.path().display().to_string()).unwrap();
        identity::save(real.path(), &record()).unwrap();
        identity::save(throwaway.path(), &record()).unwrap();
        forget_tablet_through(&runtime, &context).unwrap();
        assert!(identity::load(real.path()).unwrap().is_some());
        assert!(identity::load(throwaway.path()).unwrap().is_none());
    }

    fn record() -> DeviceRecord {
        DeviceRecord {
            format: DEVICE_FORMAT,
            uuid: HostId([0x89, 0xea, 0x78, 0x88, 0xff, 0xcb]),
            address: "E2:43:03:67:0E:01".into(),
            name: "Bamboo Slate".into(),
            protocol: "slate".into(),
            width: 29700,
            height: 21600,
            registered: "2026-10-24T14:03:12+02:00".into(),
            extra: Default::default(),
        }
    }

    /// The blackboard through the app's commands, with the simulated one of
    /// `--simulated-blackboard` (real time, three reference pages): found, added, its pages
    /// synced into three dated notes and deleted from it, forgotten; the library's own files are
    /// never touched by the simulation.
    #[test]
    fn app_commands_add_the_simulated_blackboard_and_sync_it() {
        use ardoise_device::service::BLACKBOARD_NAME;
        use ardoise_device::transport::TabletKind;
        let settings = tempfile::tempdir().unwrap();
        let notes_dir = tempfile::tempdir().unwrap();
        let throwaway = tempfile::tempdir().unwrap();
        ardoise_core::library::init(throwaway.path()).unwrap();
        let context = context(settings.path());
        crate::commands::choose_library(&context, &notes_dir.path().display().to_string()).unwrap();
        let runtime = TabletRuntime::default();
        let board = app_board(None);
        let simulation = Simulation {
            tablet: None,
            board: Some(board.clone()),
            identity_root: Some(throwaway.path().to_owned()),
            hook_delay: Duration::from_millis(50),
        };
        runtime.start_in(&context, Some(simulation));
        let (sender, events) = mpsc::channel();
        let sender = Mutex::new(sender);
        let first = tablet_subscribe_in(
            &runtime,
            "main",
            Box::new(move |TabletEvent::Status { status }| {
                lock(&sender).send(status.clone()).is_ok()
            }),
        )
        .unwrap();
        assert!(first.simulated);
        wait_for(&events, |s| s.connection == Connection::NotRegistered);
        runtime
            .with_service(TabletService::find_blackboards)
            .unwrap();
        let heard = wait_for(&events, |s| {
            s.board_search
                .as_ref()
                .is_some_and(|search| !search.found.is_empty())
        });
        let address = heard.board_search.unwrap().found[0].address.clone();
        assert_eq!(
            tablet_add_blackboard_in(&runtime, "00:00:5E:00:53:00")
                .unwrap_err()
                .code(),
            "blackboardNotFound"
        );
        let added = tablet_add_blackboard_in(&runtime, &address).unwrap();
        assert_eq!(added.known[0].kind, TabletKind::Blackboard);
        assert_eq!(added.known[0].name, BLACKBOARD_NAME);
        let status = wait_for(&events, |s| s.last_sync.is_some());
        let last = status.last_sync.clone().unwrap();
        assert_eq!((last.saved.len(), last.deleted), (3, 3));
        assert_eq!(board.stored_page_count(), 0);
        let library = Library::open(notes_dir.path()).unwrap();
        for note in &library.list().notes {
            let read = library.read(&note.path).unwrap().file;
            assert!(read.thumbnail.is_some());
            assert_eq!(read.note.page, ardoise_device::pages::BLACKBOARD_PAGE);
            assert_eq!(read.note.layers[0].name, "Calque 1");
        }
        // The blackboard has no name to change; its clock is set again.
        assert_eq!(
            tablet_rename_in(&runtime, "Classe").unwrap_err().code(),
            "notSupported"
        );
        assert_eq!(tablet_set_time_in(&runtime).unwrap(), 0);
        sim_blackboard_store_page_in(&runtime).unwrap();
        assert_eq!(
            sim_press_button_in(&runtime).unwrap_err().code(),
            "simulationOnly",
            "no simulated Slate"
        );
        // Kept in the simulation's folder, never in the library.
        assert!(
            identity::load_blackboard(notes_dir.path())
                .unwrap()
                .is_none()
        );
        assert!(
            identity::load_blackboard(throwaway.path())
                .unwrap()
                .is_some()
        );
        forget_blackboard_through(&runtime, &context).unwrap();
        assert!(
            identity::load_blackboard(throwaway.path())
                .unwrap()
                .is_none()
        );
        wait_for(&events, |s| {
            s.connection == Connection::NotRegistered && s.known.is_empty()
        });
        runtime.stop();
        assert_eq!(
            sim_blackboard_store_page_in(&TabletRuntime::default())
                .unwrap_err()
                .code(),
            "simulationOnly"
        );
    }

    /// The guided test of the real tablet: the service stops while it runs (the windows are told
    /// the tablet is busy), then starts again and reads `device.json` again.
    #[test]
    fn the_service_pauses_during_the_real_tablet_test() {
        let settings = tempfile::tempdir().unwrap();
        let notes_dir = tempfile::tempdir().unwrap();
        let context = context(settings.path());
        crate::commands::choose_library(&context, &notes_dir.path().display().to_string()).unwrap();
        let runtime = TabletRuntime::default();
        runtime.start_in(&context, None);
        let (sender, events) = mpsc::channel();
        let sender = Mutex::new(sender);
        tablet_subscribe_in(
            &runtime,
            "main",
            Box::new(move |TabletEvent::Status { status }| {
                lock(&sender).send(status.clone()).is_ok()
            }),
        )
        .unwrap();
        // The same window subscribing again replaces its subscription.
        let (sender, again) = mpsc::channel();
        let sender = Mutex::new(sender);
        tablet_subscribe_in(
            &runtime,
            "main",
            Box::new(move |TabletEvent::Status { status }| {
                lock(&sender).send(status.clone()).is_ok()
            }),
        )
        .unwrap();
        assert_eq!(lock(&runtime.subscribers).len(), 1);
        assert_eq!(runtime.status().unwrap().registered, None);

        assert!(runtime.pause());
        assert!(runtime.with_service(|_| ()).is_err(), "stopped");
        let busy = runtime.status().unwrap();
        assert_eq!(busy.connection, Connection::Unavailable);
        assert_eq!(busy.error.as_ref().unwrap().code, "tabletBusy");
        wait_for(&again, |s| s == &busy);
        // The replaced subscription was dropped (its channel is closed) before that.
        assert!(events.iter().all(|s| s != busy));
        assert!(!runtime.pause(), "already stopped");

        // The test registered the tablet: the service started again knows it.
        identity::save(notes_dir.path(), &record()).unwrap();
        runtime.resume_in(&context);
        assert!(runtime.with_service(|_| ()).is_ok());
        let status = wait_for(&again, |s| s.registered.is_some());
        assert_eq!(status.registered.unwrap().name, "Bamboo Slate");
        assert_ne!(
            runtime.status().unwrap().error.map(|e| e.code),
            Some("tabletBusy".into())
        );
        // Resuming twice does not start a second service.
        runtime.resume_in(&context);
        runtime.stop();
        runtime.resume_in(&context);
        assert!(runtime.with_service(|_| ()).is_err());
    }

    /// A command waiting for the tablet does not hold the lock other commands need.
    #[test]
    fn the_lock_is_not_held_while_a_command_runs() {
        let settings = tempfile::tempdir().unwrap();
        let context = context(settings.path());
        let runtime = TabletRuntime::default();
        runtime.start_in(&context, None);
        // Before, the inner call waited forever for the outer one.
        let inner = runtime
            .with_service(|_| runtime.with_service(TabletService::status).is_ok())
            .unwrap();
        assert!(inner);
        runtime.stop();
    }
}
