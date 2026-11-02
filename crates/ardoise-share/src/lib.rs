//! Sharing of the sheet on the local network (SPEC §4.10, design in `docs/design/lan-share.md`):
//! a small read-only HTTP/1.1 server that a browser on another PC of the same network opens to
//! see what the Ardoise PC chooses to show (the live sheet, the page of the note open in the
//! editor, or a waiting screen).
//!
//! - Three routes only: `GET /` and `GET /assets/<file>` (the viewer, served from bytes the app
//!   hands over), `POST /join` (the 6-character code gives an HttpOnly session cookie) and
//!   `GET /events` (Server-Sent Events). Every other request gets 404; no path ever reaches a
//!   file.
//! - Only the Ardoise PC sends; the browser receives.
//! - Started and stopped by hand ([`Share::start`], [`Share::stop`]); dropping the [`Share`] stops
//!   it too. When `stop` returns the port is closed.
//!
//! This crate knows nothing of Tauri: live batches and notes arrive as JSON text, live mode
//! through a [`LiveFeed`] the app implements over its `LiveHub`.

pub mod addresses;
pub mod qr;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::Arc;

use serde::Serialize;

/// Port used when `ardoise.toml` does not choose one (`[share] port`).
pub const DEFAULT_PORT: u16 = 8765;

/// Where and how the server listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareConfig {
    /// `0.0.0.0` (every IPv4 interface) in the app; `127.0.0.1` in the tests.
    pub bind: Ipv4Addr,
    /// 0 takes a free port (tests): [`Share::port`] says which.
    pub port: u16,
    /// The code to ask for, instead of a random one (tests only). Normalised like the codes typed
    /// in the page.
    pub code: Option<String>,
}

impl Default for ShareConfig {
    fn default() -> Self {
        Self {
            bind: Ipv4Addr::UNSPECIFIED,
            port: DEFAULT_PORT,
            code: None,
        }
    }
}

/// One file of the viewer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// `Content-Type` header value (`text/html; charset=utf-8`, `text/javascript`, `application/wasm`…).
    pub content_type: String,
    pub bytes: Arc<[u8]>,
}

/// The viewer's files by request path: `/` (the page), `/assets/<file>` (its scripts, styles,
/// WebAssembly). Paths are looked up verbatim: nothing else is ever served.
pub type Assets = HashMap<String, Asset>;

/// What the share shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Shown {
    /// Nothing to share: the waiting screen.
    #[default]
    Waiting,
    /// The live sheet, through the [`LiveFeed`].
    Live,
    /// The page of the note open in the editor.
    Note,
}

/// Where a running share stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareStatus {
    /// The port actually listened on.
    pub port: u16,
    /// The code the page asks for, 6 characters without separator (`K7MQ2X`).
    pub code: String,
    /// Event streams open (screens connected).
    pub viewers: usize,
    pub paused: bool,
    pub shown: Shown,
}

/// A change of the note shown, as complete JSON messages of the event stream
/// (`docs/design/lan-share.md`, "The event stream").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoteUpdate {
    /// The `note` message: the whole page. Kept for the viewers that connect later.
    pub full: String,
    /// The `noteStrokes` message relative to the previous [`Share::show_note`], when the change
    /// only added strokes at the end; sent instead of `full` to the viewers already showing it.
    pub append: Option<String>,
}

/// The live session, for the share: the app implements it over its `LiveHub` (one hub
/// subscription per viewer). Its methods may be called while the share holds its own lock: they
/// must not call back into [`Share`].
pub trait LiveFeed: Send + Sync + 'static {
    /// Sends the live session to `viewer` from now on: first a batch that starts with a snapshot
    /// (and the stroke in progress), then every batch, until [`LiveFeed::stop`] or until a send
    /// fails.
    fn start(&self, viewer: LiveViewer);
    /// Stops sending to the viewer `viewer` (its id).
    fn stop(&self, viewer: u64);
    /// The session now as one `LiveBatch` (JSON) starting with a snapshot: the frozen image of a
    /// paused share. `None` when there is no session to show.
    fn freeze(&self) -> Option<String>;
}

/// A viewer seen by the [`LiveFeed`]: where its live batches go.
#[derive(Clone)]
pub struct LiveViewer {
    id: u64,
    // Filled by package A (the viewer's queue, whether its first batch was sent).
}

/// The viewer went away, or was too slow and was disconnected: stop sending to it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the viewer is gone")]
pub struct ViewerGone;

impl LiveViewer {
    /// Id of the viewer (one per event stream; a reconnection is a new viewer).
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Queues one live batch (`LiveBatch` JSON) for the viewer, wrapped as a `live` message
    /// (`reset: true` for the first one). Never blocks: a full queue disconnects the viewer.
    pub fn send(&self, batch_json: &str) -> Result<(), ViewerGone> {
        let _ = batch_json;
        unimplemented!("package A")
    }
}

/// Called when the number of viewers, the pause or what is shown changes (outside the share's
/// lock, from its thread).
pub type OnChange = Box<dyn Fn(ShareStatus) + Send + Sync>;

/// Failure to start a share.
#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    /// Another program listens on the port.
    #[error(
        "port {port} is already used by another program: close it, or choose another port in ardoise.toml ([share] port)"
    )]
    PortInUse { port: u16 },
    /// Any other failure to listen or to start the server's thread.
    #[error("cannot start sharing: {0}")]
    Io(#[from] std::io::Error),
    /// The random generator of the system failed.
    #[error("cannot draw a random code: {0}")]
    Random(String),
}

impl ShareError {
    /// Error code for the interface (`errorCodes.*`).
    pub fn code(&self) -> &'static str {
        match self {
            ShareError::PortInUse { .. } => "sharePortInUse",
            ShareError::Io(_) | ShareError::Random(_) => "shareFailed",
        }
    }
}

/// A running share. Dropping it stops it.
pub struct Share {
    // Filled by package A.
}

impl Share {
    /// Listens on `config.bind:config.port` (bound before returning: a port in use is an error at
    /// once) and serves `assets`, live mode through `feed`, until [`Share::stop`]. Shows the
    /// waiting screen until told otherwise; a new random code unless `config.code` is set.
    pub fn start(
        config: ShareConfig,
        assets: Assets,
        feed: Arc<dyn LiveFeed>,
        on_change: OnChange,
    ) -> Result<Share, ShareError> {
        let _ = (config, assets, feed, on_change);
        unimplemented!("package A")
    }

    /// The port listened on.
    pub fn port(&self) -> u16 {
        unimplemented!("package A")
    }

    /// The code, 6 characters without separator.
    pub fn code(&self) -> String {
        unimplemented!("package A")
    }

    pub fn status(&self) -> ShareStatus {
        unimplemented!("package A")
    }

    /// Shows the waiting screen.
    pub fn show_waiting(&self) {
        unimplemented!("package A")
    }

    /// Shows the live sheet: every viewer is handed to the feed.
    pub fn show_live(&self) {
        unimplemented!("package A")
    }

    /// Shows (or updates) the note's page.
    pub fn show_note(&self, update: NoteUpdate) {
        let _ = update;
        unimplemented!("package A")
    }

    /// Freezes the shared image (`true`) or follows the content again (`false`).
    pub fn set_paused(&self, paused: bool) {
        let _ = paused;
        unimplemented!("package A")
    }

    /// Tells every viewer `end`, closes every connection and the port, and waits for the
    /// server's thread (at most 2 s). Can be repeated.
    pub fn stop(&self) {
        unimplemented!("package A")
    }
}

/// `K7MQ2X` → `K7M Q2X` (how the app shows the code).
pub fn format_code(code: &str) -> String {
    let _ = code;
    unimplemented!("package A")
}
