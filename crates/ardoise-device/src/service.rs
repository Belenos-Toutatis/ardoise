//! The tablet as the application sees it (SPEC §4.1, §4.2): one worker thread that owns the
//! connector, the [`Manager`](crate::manager::Manager) and the current session, connects to the
//! registered tablet when it comes in range, syncs its pages, runs the registration and live mode,
//! and reports a [`TabletStatus`] at every change.
//!
//! Contract: docs/design/tablet-live-classroom.md. The types and the public methods are fixed;
//! the worker behind them is in `service/worker.rs`.
//!
//! The handle sends requests to the worker over a channel; blocking requests (rename, set the
//! clock, forget) wait for its answer up to [`REQUEST_TIMEOUT`]. The worker does one bounded
//! step at a time (a scan slice, one page of a sync, one live notification...) and looks at its
//! requests between steps; an interrupt flag (the `Manager`'s cancellation flag) cuts short a
//! scan, a download or the registration when a request cannot wait (cancel, forget, another
//! library, shutdown), or while the worker only searches for the tablet. The flag stays raised
//! as long as such a request waits in the channel ([`Interrupts`]).

mod journal;
mod worker;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use ardoise_core::note::PageSize;
use ardoise_core::settings::{LiveButton, Orientation, TabletSettings};
use ardoise_proto::live::LiveRecord;
use serde::{Deserialize, Serialize};

use crate::session::{DeviceError, DownloadedPage, valid_name};
use crate::transport::{Connector, TransportError};

/// What the service is doing with the tablet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Connection {
    /// No library, or no usable Bluetooth adapter (see [`TabletStatus::error`]).
    Unavailable,
    /// No tablet registered from this library.
    NotRegistered,
    /// The registration wizard runs (see [`TabletStatus::register_step`]).
    Registering,
    /// Waiting for the registered tablet to come in range (button pressed, or pages to send).
    Searching,
    Connecting,
    /// Connected and idle, until the tablet drops the link.
    Connected,
    Syncing,
    Live,
}

/// Steps of the registration, as the wizard shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RegisterStep {
    /// The user must hold the button about 6 s, until the LED blinks blue.
    HoldButton,
    /// Looking for a tablet in registration mode.
    Searching,
    Connecting,
    /// The user must press the button briefly to confirm.
    PressButton,
    Done,
}

/// What waits for the tablet to connect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Pending {
    Sync,
    Live,
}

/// The registered tablet (`device.json`), as `TabletInfo` in `note.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisteredTablet {
    pub name: String,
    pub address: String,
    /// RFC 3339.
    pub registered: String,
    pub width: u32,
    pub height: u32,
}

/// What the tablet said about itself at the last connection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDetails {
    pub name: String,
    pub firmware: String,
    pub battery_percent: u8,
    pub charging: bool,
    /// Drawing area in device units (10 µm).
    pub width: u32,
    pub height: u32,
    /// Pages waiting in the tablet's memory.
    pub stored_pages: u16,
    /// `Bamboo Slate A4`, `Bamboo Slate A5`, or `Wacom (W × H)` (see [`model_name`]).
    pub model: String,
    /// When this was read, unix milliseconds.
    pub read_at: i64,
}

/// Progress of the sync under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncProgress {
    /// 1-based index of the page being downloaded.
    pub current: u16,
    pub total: u16,
    /// Pages saved so far.
    pub saved: u16,
}

/// Outcome of the last sync.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LastSync {
    /// Unix milliseconds.
    pub at: i64,
    /// Paths of the notes written.
    pub saved: Vec<String>,
    /// Pages deleted from the tablet after being saved.
    pub deleted: u16,
}

/// A failure shown to the user: `code` selects a translated sentence (`errorCodes.*`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct StatusError {
    pub code: String,
    pub message: String,
}

impl StatusError {
    pub fn new(code: &str, message: impl ToString) -> Self {
        Self {
            code: code.to_owned(),
            message: message.to_string(),
        }
    }
}

/// The options of `[tablet]` in `ardoise.toml` that the service follows (`TabletOptions` in
/// `tablet.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TabletOptions {
    pub auto_sync: bool,
    pub delete_after_import: bool,
    pub orientation: Orientation,
    pub button: LiveButton,
}

impl Default for TabletOptions {
    fn default() -> Self {
        Self::from(&TabletSettings::default())
    }
}

impl From<&TabletSettings> for TabletOptions {
    fn from(settings: &TabletSettings) -> Self {
        Self {
            auto_sync: settings.auto_sync,
            delete_after_import: settings.delete_after_import,
            orientation: settings.orientation,
            button: settings.button,
        }
    }
}

/// Everything the interface shows about the tablet (`TabletStatus` in `tablet.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabletStatus {
    pub connection: Connection,
    pub registered: Option<RegisteredTablet>,
    pub device: Option<DeviceDetails>,
    pub register_step: Option<RegisterStep>,
    pub sync: Option<SyncProgress>,
    pub last_sync: Option<LastSync>,
    pub pending: Option<Pending>,
    pub error: Option<StatusError>,
    pub simulated: bool,
    pub options: TabletOptions,
}

impl TabletStatus {
    /// The status before the worker looked at anything.
    pub fn initial(simulated: bool, options: TabletOptions) -> Self {
        Self {
            connection: Connection::Unavailable,
            registered: None,
            device: None,
            register_step: None,
            sync: None,
            last_sync: None,
            pending: None,
            error: None,
            simulated,
            options,
        }
    }
}

/// The display name of a tablet model from its drawing area.
pub fn model_name(width: u32, height: u32) -> String {
    match (width, height) {
        (ardoise_proto::SLATE_LARGE_WIDTH, ardoise_proto::SLATE_LARGE_HEIGHT) => {
            "Bamboo Slate A4".into()
        }
        (ardoise_proto::SLATE_SMALL_WIDTH, ardoise_proto::SLATE_SMALL_HEIGHT) => {
            "Bamboo Slate A5".into()
        }
        (width, height) => format!("Wacom ({width} × {height})"),
    }
}

/// Where the service works.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceConfig {
    /// Library receiving the synced notes; `None` when no library is chosen.
    pub library: Option<PathBuf>,
    /// Folder whose `_ardoise/device.json` holds the registration: the library, or a throwaway
    /// folder for the simulated tablet (it must never replace the real tablet's registration).
    pub identity_root: Option<PathBuf>,
    pub options: TabletOptions,
    pub simulated: bool,
}

/// Makes the connector (Bluetooth, or the simulated tablet in real time).
pub trait ConnectorFactory: Send {
    /// Checks the adapter before connecting (`bluetoothOff`, `bluetoothMissing`).
    fn check(&mut self) -> Result<(), StatusError> {
        Ok(())
    }

    fn connector(&mut self) -> Result<Box<dyn Connector>, TransportError>;
}

/// Physical actions of the user, played by the simulated tablet; nothing for a real one.
pub trait ServiceHooks: Send + Sync {
    /// The wizard asks to hold the button.
    fn registration_started(&self) {}
    /// The wizard asks to press the button to confirm.
    fn waiting_for_button(&self) {}
    /// Live mode streams: the pen may move.
    fn live_started(&self) {}
}

/// No physical action (real tablet).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoHooks;

impl ServiceHooks for NoHooks {}

/// Called at every status change (on the worker thread: keep it short).
pub type StatusCallback = Box<dyn Fn(&TabletStatus) + Send + Sync>;

/// What a downloaded page becomes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageContext {
    /// Drawing area of the tablet.
    pub size: PageSize,
    pub orientation: Orientation,
}

/// Writes a downloaded page as a note of the library (`&Path`, its root) and checks it reads back
/// (`pages::LibrarySink::store`); returns the note's path. `Err` keeps the page on the tablet.
pub type PageStore =
    Box<dyn FnMut(&Path, DownloadedPage, &PageContext) -> Result<PathBuf, String> + Send>;

/// State of live mode, as the hub reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveState {
    /// Asked, waiting for the tablet.
    Waiting,
    Live,
    /// The link was lost; reopening it.
    Reconnecting,
    Stopped,
}

/// What live mode delivers, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum LiveInput {
    /// Live mode starts on a tablet of this size (always before the first records).
    Started {
        width: u32,
        height: u32,
        orientation: Orientation,
    },
    /// The tablet is held another way while live mode streams (the quick switch of the live
    /// window): the records that follow are turned for `orientation`; same `width` × `height`.
    Reoriented {
        width: u32,
        height: u32,
        orientation: Orientation,
    },
    /// Records of one notification, 5 ms apart. `received`: when the transport delivered the
    /// notification (latency stamp, system clock).
    Records {
        records: Vec<LiveRecord>,
        received: SystemTime,
    },
    /// The pen came near.
    ProximityIn {
        received: SystemTime,
    },
    /// The tablet's button was pressed.
    Button,
    State(LiveState),
}

/// Receives live mode (the app's live hub). Called on the worker thread: keep it short.
pub trait LiveSink: Send {
    fn input(&mut self, input: LiveInput);
}

/// Longest scan for the registered tablet before the worker looks at its requests again
/// (`Manager::find_registered`).
pub const SEARCH_SLICE: Duration = Duration::from_secs(5);
/// How long the wizard looks for a tablet in registration mode.
pub const REGISTER_SEARCH: Duration = Duration::from_secs(60);
/// How long the wizard waits for the button press that confirms the registration.
pub const BUTTON_TIMEOUT: Duration = Duration::from_secs(30);
/// Wait before trying again after a failed connection.
pub const RETRY_DELAY: Duration = Duration::from_secs(5);
/// How often a missing or switched-off Bluetooth adapter is checked again.
pub const ADAPTER_RECHECK: Duration = Duration::from_secs(10);
/// How long a blocking request (rename, set the clock, forget) waits for the worker.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The `errorCodes.*` key of a device error (the interface translates it).
pub fn error_code(error: &DeviceError) -> &'static str {
    match error {
        DeviceError::Transport(TransportError::NoAdapter) => "bluetoothMissing",
        DeviceError::Transport(TransportError::AdapterOff) => "bluetoothOff",
        DeviceError::Transport(TransportError::Timeout | TransportError::NotFound(_))
        | DeviceError::Timeout(_) => "tabletTimeout",
        DeviceError::Disconnected | DeviceError::NotConnected => "tabletNotConnected",
        DeviceError::NotRegistered | DeviceError::Refused(_) => "tabletRefused",
        DeviceError::Busy => "tabletBusy",
        DeviceError::NoTabletRegistered => "tabletNotRegistered",
        DeviceError::NoLibrary => "noLibrary",
        DeviceError::InvalidName(_) => "invalidName",
        DeviceError::Storage(_) => "libraryAccess",
        DeviceError::Transport(_)
        | DeviceError::Unexpected(_)
        | DeviceError::CrcMismatch { .. }
        | DeviceError::Cancelled => "tablet",
    }
}

impl From<&DeviceError> for StatusError {
    fn from(error: &DeviceError) -> Self {
        StatusError::new(error_code(error), error)
    }
}

/// The answer to a blocking request.
type Answer<T> = SyncSender<Result<T, DeviceError>>;

/// The `Manager`'s cancellation flag, raised exactly while requests that must cut short what
/// the worker waits for are in the channel: such a request is counted before it is sent and
/// uncounted when the worker takes it. The worker therefore never lowers the flag for a request
/// it has not taken yet, whatever the order in which the two threads run.
struct Interrupts {
    /// Interrupting requests sent and not taken yet.
    waiting: Mutex<u32>,
    flag: Arc<AtomicBool>,
}

impl Interrupts {
    fn new() -> Self {
        Self {
            waiting: Mutex::new(0),
            flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The flag the `Manager` and the sessions look at.
    fn flag(&self) -> Arc<AtomicBool> {
        self.flag.clone()
    }

    /// An interrupting request is about to be sent.
    fn raise(&self) {
        let mut waiting = self.waiting.lock().unwrap_or_else(PoisonError::into_inner);
        *waiting = waiting.saturating_add(1);
        self.flag.store(true, Ordering::SeqCst);
    }

    /// The worker took an interrupting request.
    fn taken(&self) {
        let mut waiting = self.waiting.lock().unwrap_or_else(PoisonError::into_inner);
        *waiting = waiting.saturating_sub(1);
        if *waiting == 0 {
            self.flag.store(false, Ordering::SeqCst);
        }
    }
}

/// A request as it travels to the worker: whether it was counted by [`Interrupts::raise`].
struct Sent {
    request: Request,
    interrupting: bool,
}

/// What the handle asks the worker.
enum Request {
    Register,
    SyncNow,
    Cancel,
    Rename(String, Answer<()>),
    SetTime(Answer<i64>),
    SetOptions(TabletOptions),
    Forget(Answer<()>),
    SetLibrary(Option<PathBuf>, Option<PathBuf>),
    StartLive(Box<dyn LiveSink>),
    StopLive,
    Shutdown,
}

/// Handle to the worker. Dropping it stops the worker. It can be shared between threads (an
/// `Arc`): a blocking request waits without holding anything that others need.
pub struct TabletService {
    status: Arc<Mutex<TabletStatus>>,
    requests: Sender<Sent>,
    /// The `Manager`'s cancellation flag: cuts short what the worker waits for.
    interrupts: Arc<Interrupts>,
    worker: Mutex<Option<JoinHandle<()>>>,
}

impl TabletService {
    /// Starts the worker.
    pub fn start(
        config: ServiceConfig,
        connector: Box<dyn ConnectorFactory>,
        hooks: Arc<dyn ServiceHooks>,
        on_status: StatusCallback,
        store_page: PageStore,
    ) -> TabletService {
        let status = TabletStatus::initial(config.simulated, config.options);
        on_status(&status);
        let status = Arc::new(Mutex::new(status));
        let (requests, receiver) = mpsc::channel();
        let interrupts = Arc::new(Interrupts::new());
        let publisher = worker::Publisher::new(status.clone(), Arc::new(on_status));
        let failed = publisher.clone();
        let worker = worker::Worker::new(
            config,
            connector,
            hooks,
            publisher,
            store_page,
            receiver,
            interrupts.clone(),
        );
        let spawned = std::thread::Builder::new()
            .name("tablet-service".into())
            .spawn(move || worker.run());
        let worker = match spawned {
            Ok(handle) => Some(handle),
            Err(error) => {
                failed.update(|status| {
                    status.error = Some(StatusError::new(
                        "tablet",
                        format!("cannot start the tablet service: {error}"),
                    ));
                });
                None
            }
        };
        TabletService {
            status,
            requests,
            interrupts,
            worker: Mutex::new(worker),
        }
    }

    /// The current status.
    pub fn status(&self) -> TabletStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Sends a request; `urgent` ones (and any request while the worker only searches for the
    /// tablet) interrupt what the worker waits for, until it takes them ([`Interrupts`]).
    fn send(&self, request: Request, urgent: bool) -> Result<(), DeviceError> {
        let interrupting = urgent || self.status().connection == Connection::Searching;
        if interrupting {
            self.interrupts.raise();
        }
        self.requests
            .send(Sent {
                request,
                interrupting,
            })
            .map_err(|_| DeviceError::Unexpected("the tablet service has stopped".into()))
    }

    /// Sends a blocking request and waits for the worker's answer.
    fn ask<T>(
        &self,
        request: impl FnOnce(Answer<T>) -> Request,
        urgent: bool,
    ) -> Result<T, DeviceError> {
        let (answer, receiver) = mpsc::sync_channel(1);
        self.send(request(answer), urgent)?;
        match receiver.recv_timeout(REQUEST_TIMEOUT) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(DeviceError::Timeout("the tablet service")),
            Err(RecvTimeoutError::Disconnected) => Err(DeviceError::Unexpected(
                "the tablet service has stopped".into(),
            )),
        }
    }

    /// What the status says about a command that needs the connected, idle tablet (the
    /// worker checks again).
    fn check_connected(&self) -> Result<(), DeviceError> {
        match self.status().connection {
            Connection::Connected => Ok(()),
            Connection::Syncing | Connection::Live | Connection::Registering => {
                Err(DeviceError::Busy)
            }
            Connection::NotRegistered => Err(DeviceError::NoTabletRegistered),
            Connection::Unavailable | Connection::Searching | Connection::Connecting => {
                Err(DeviceError::NotConnected)
            }
        }
    }

    /// Starts the registration wizard (progress in the status).
    pub fn register(&self) {
        let _ = self.send(Request::Register, false);
    }

    /// Syncs now, or as soon as the tablet connects.
    pub fn sync_now(&self) {
        let _ = self.send(Request::SyncNow, false);
    }

    /// Cancels the registration or the sync under way.
    pub fn cancel(&self) {
        let _ = self.send(Request::Cancel, true);
    }

    /// Renames the tablet (connected only).
    pub fn rename(&self, name: &str) -> Result<(), DeviceError> {
        if !valid_name(name) {
            return Err(DeviceError::InvalidName(name.to_owned()));
        }
        self.check_connected()?;
        self.ask(|answer| Request::Rename(name.to_owned(), answer), false)
    }

    /// Sets the tablet clock (connected only); returns the drift left, in seconds.
    pub fn set_time(&self) -> Result<i64, DeviceError> {
        self.check_connected()?;
        self.ask(Request::SetTime, false)
    }

    pub fn set_options(&self, options: TabletOptions) {
        let _ = self.send(Request::SetOptions(options), false);
    }

    /// Disconnects and forgets the registration (`device.json`).
    pub fn forget(&self) -> Result<(), DeviceError> {
        self.ask(Request::Forget, true)
    }

    /// Follows another library (or none).
    pub fn set_library(&self, library: Option<PathBuf>, identity_root: Option<PathBuf>) {
        let _ = self.send(Request::SetLibrary(library, identity_root), true);
    }

    /// Enters live mode now or as soon as the tablet connects; `sink` receives it.
    pub fn start_live(&self, sink: Box<dyn LiveSink>) {
        let _ = self.send(Request::StartLive(sink), false);
    }

    /// Leaves live mode (the link stays open).
    pub fn stop_live(&self) {
        let _ = self.send(Request::StopLive, false);
    }

    /// Stops the worker and closes the link; returns once the worker has ended. A blocking
    /// request still waiting on another thread then fails at once.
    pub fn shutdown(&self) {
        let worker = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(worker) = worker {
            let _ = self.send(Request::Shutdown, true);
            let _ = worker.join();
        }
    }
}

impl Drop for TabletService {
    fn drop(&mut self) {
        // Not joined: the worker finishes closing the link on its own.
        let worker = self
            .worker
            .get_mut()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if worker.is_some() {
            let _ = self.send(Request::Shutdown, true);
        }
    }
}

#[cfg(test)]
mod tests;
