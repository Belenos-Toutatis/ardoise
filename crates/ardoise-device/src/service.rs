//! The tablet as the application sees it (SPEC §4.1, §4.2): one worker thread that owns the
//! connector, the [`Manager`](crate::manager::Manager) and the current session, connects to the
//! registered tablet when it comes in range, syncs its pages, runs the registration and live mode,
//! and reports a [`TabletStatus`] at every change.
//!
//! Contract: docs/design/tablet-live-classroom.md. The types and the public methods are fixed;
//! the worker behind them is work package A.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::SystemTime;

use ardoise_core::note::PageSize;
use ardoise_core::settings::{Hand, LiveButton, TabletSettings};
use ardoise_proto::live::LiveRecord;
use serde::{Deserialize, Serialize};

use crate::session::{DeviceError, DownloadedPage};
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
    pub hand: Hand,
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
            hand: settings.hand,
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
    pub hand: Hand,
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
        hand: Hand,
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

/// Handle to the worker. Dropping it stops the worker.
pub struct TabletService {
    status: Arc<Mutex<TabletStatus>>,
}

fn not_ready() -> DeviceError {
    DeviceError::Unexpected("the tablet service is not implemented yet".into())
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
        let _ = (connector, hooks, store_page);
        let status = TabletStatus::initial(config.simulated, config.options);
        on_status(&status);
        TabletService {
            status: Arc::new(Mutex::new(status)),
        }
    }

    /// The current status.
    pub fn status(&self) -> TabletStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Starts the registration wizard (progress in the status).
    pub fn register(&self) {}

    /// Syncs now, or as soon as the tablet connects.
    pub fn sync_now(&self) {}

    /// Cancels the registration or the sync under way.
    pub fn cancel(&self) {}

    /// Renames the tablet (connected only).
    pub fn rename(&self, name: &str) -> Result<(), DeviceError> {
        let _ = name;
        Err(not_ready())
    }

    /// Sets the tablet clock (connected only); returns the drift left, in seconds.
    pub fn set_time(&self) -> Result<i64, DeviceError> {
        Err(not_ready())
    }

    pub fn set_options(&self, options: TabletOptions) {
        let _ = options;
    }

    /// Disconnects and forgets the registration (`device.json`).
    pub fn forget(&self) -> Result<(), DeviceError> {
        Err(not_ready())
    }

    /// Follows another library (or none).
    pub fn set_library(&self, library: Option<PathBuf>, identity_root: Option<PathBuf>) {
        let _ = (library, identity_root);
    }

    /// Enters live mode now or as soon as the tablet connects; `sink` receives it.
    pub fn start_live(&self, sink: Box<dyn LiveSink>) {
        let _ = sink;
    }

    /// Leaves live mode (the link stays open).
    pub fn stop_live(&self) {}

    /// Stops the worker and closes the link.
    pub fn shutdown(self) {}
}
