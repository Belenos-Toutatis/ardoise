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

mod access;
pub mod addresses;
mod content;
mod http;
mod journal;
pub mod qr;
mod server;

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::Serialize;

/// Port used when `ardoise.toml` does not choose one (`[share] port`).
pub const DEFAULT_PORT: u16 = 8765;

/// How many ports after the wanted one are tried when it cannot be bound (another program uses
/// it, or Windows reserves it): the first that binds is used.
pub const FALLBACK_PORTS: u16 = 20;

/// Where and how the server listens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShareConfig {
    /// `0.0.0.0` (every IPv4 interface) in the app; `127.0.0.1` in the tests.
    pub bind: Ipv4Addr,
    /// The port wanted. 0 takes a free port (tests): [`Share::port`] says which.
    pub port: u16,
    /// How many ports after `port` are tried, in order, when it cannot be bound (the largest
    /// is 65535); [`Share::port`] says which one was bound. 0 tries `port` alone. Ignored when
    /// `port` is 0.
    pub fallback_ports: u16,
    /// The code to ask for, instead of a random one (tests only). Normalised like the codes typed
    /// in the page.
    pub code: Option<String>,
    /// A file where the share writes one line per connection event (diagnostics: what was asked,
    /// from which address, how long each step took; never the code, a cookie or a query string).
    pub journal: Option<std::path::PathBuf>,
}

impl Default for ShareConfig {
    fn default() -> Self {
        Self {
            bind: Ipv4Addr::UNSPECIFIED,
            port: DEFAULT_PORT,
            fallback_ports: FALLBACK_PORTS,
            code: None,
            journal: None,
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
/// must not call back into [`Share`] (calling [`LiveViewer::send`] is fine: it never takes that
/// lock). They are called from the thread that changed what is shown (`show_*`, `set_paused`,
/// `stop`) or from the share's thread (a viewer connects or leaves).
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

/// A viewer seen by the [`LiveFeed`]: where its live batches go. Each [`LiveFeed::start`] gives a
/// new one; it is refused (`Err(ViewerGone)`) once the share stopped that subscription, so a
/// batch sent late never arrives after what replaced live mode.
#[derive(Clone)]
pub struct LiveViewer {
    id: u64,
    /// The live subscription this handle belongs to.
    subscription: u64,
    viewer: Arc<content::Viewer>,
}

impl std::fmt::Debug for LiveViewer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LiveViewer")
            .field("id", &self.id)
            .field("subscription", &self.subscription)
            .finish()
    }
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
    /// (`reset: true` for the first one). Never blocks and never takes the share's lock (only
    /// this viewer's queue lock, briefly): a full queue (1 024 messages) disconnects the viewer.
    /// `Err` means: stop sending to this handle (the viewer left, was too slow, or the share
    /// stopped this subscription).
    pub fn send(&self, batch_json: &str) -> Result<(), ViewerGone> {
        self.viewer.send_live(self.subscription, batch_json)
    }
}

/// Called when the number of viewers, the pause or what is shown changes (outside the share's
/// lock, from its thread). Changes in quick succession may give a single call, always with the
/// latest status. It runs on the share's thread: it must return quickly, and it must not call
/// [`Share::stop`] (nor drop the [`Share`]): `stop` could not wait for the thread it runs on, so
/// the port might still be open when it returns. Never called once [`Share::stop`] has returned.
pub type OnChange = Box<dyn Fn(ShareStatus) + Send + Sync>;

/// Failure to start a share.
#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    /// Another program listens on the port (or Windows reserves it), and no other was asked for.
    #[error(
        "port {port} is already used by another program: close it, or choose another port in the settings of Ardoise"
    )]
    PortInUse { port: u16 },
    /// Not one of the ports from `first` to `last` could be bound.
    #[error(
        "no free port from {first} to {last}: close the programs that use them, or choose another port in the settings of Ardoise"
    )]
    NoFreePort { first: u16, last: u16 },
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
            ShareError::NoFreePort { .. } => "shareNoFreePort",
            ShareError::Io(_) | ShareError::Random(_) => "shareFailed",
        }
    }
}

/// The server's limits (`docs/design/lan-share.md`, "Threads and lifetime"). Not part of the
/// interface: the defaults are the design's; the tests shrink them to reach them from one
/// loopback address.
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Connections open at once. When there is no room, a connection is closed for the
    /// newcomer (fair share): the oldest one that has not yet sent a whole request head, of the
    /// newcomer's address or of one that holds more connections; else the oldest request of the
    /// address that holds the most, if it holds at least two more than the newcomer's (and an
    /// address holding a session token takes the room of one without first); never an event
    /// stream. Otherwise the newcomer is closed at once.
    pub connections: usize,
    /// Of [`Limits::connections`], the slots that only an address holding a session token may
    /// take: a screen that has joined always finds room, whatever the PCs without the code do.
    pub reserved: usize,
    /// Connections open at once from one address (more are closed at once).
    pub connections_per_address: usize,
    /// Event streams open at once (more get 503).
    pub streams: usize,
    /// Event streams open at once from one address (more get 503).
    pub streams_per_address: usize,
    /// Event streams one address may open in a row (more get 503): each new stream makes the
    /// app copy and serialise the whole page.
    pub stream_opens: u32,
    /// Then one more stream per address every this long.
    pub stream_open_every: Duration,
    /// Time for the first byte of a request to arrive. Longer than browsers keep a connection
    /// they opened ahead of a request (Chrome: 10 s): a browser that sends its request on such
    /// a connection just as the server closes it gets nothing, retries on the next one, and
    /// gives up after two (`ERR_TOO_MANY_RETRIES`, seen on a phone). A connection that waits
    /// takes no room from others: it is the first closed to make room (fair share).
    pub first_byte: Duration,
    /// Time for the whole request head (and the body of `POST /join`) to arrive, counted from
    /// the connection.
    pub request: Duration,
    /// Time between two comments on an idle event stream.
    pub ping: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            connections: 64,
            reserved: 16,
            connections_per_address: 8,
            streams: 32,
            streams_per_address: 4,
            stream_opens: 8,
            stream_open_every: Duration::from_secs(2),
            first_byte: Duration::from_secs(20),
            request: Duration::from_secs(30),
            ping: Duration::from_secs(15),
        }
    }
}

/// Longest wait for the server's thread in [`Share::stop`].
const STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// A running share. Dropping it stops it.
pub struct Share {
    server: Arc<server::Server>,
    /// The server's thread and its end signal, until stopped.
    thread: Mutex<Option<(std::thread::JoinHandle<()>, std::sync::mpsc::Receiver<()>)>>,
}

impl Share {
    /// Listens on `config.bind:config.port` (bound before returning: a port in use is an error at
    /// once, after the following ports `config.fallback_ports` were tried) and serves `assets`, live mode through `feed`, until [`Share::stop`]. Shows the
    /// waiting screen until told otherwise; a new random code unless `config.code` is set.
    pub fn start(
        config: ShareConfig,
        assets: Assets,
        feed: Arc<dyn LiveFeed>,
        on_change: OnChange,
    ) -> Result<Share, ShareError> {
        Self::start_with_limits(config, Limits::default(), assets, feed, on_change)
    }

    /// [`Share::start`] with other limits (tests).
    #[doc(hidden)]
    pub fn start_with_limits(
        config: ShareConfig,
        limits: Limits,
        assets: Assets,
        feed: Arc<dyn LiveFeed>,
        on_change: OnChange,
    ) -> Result<Share, ShareError> {
        let running = server::start(config, limits, assets, feed, on_change)?;
        Ok(Share {
            server: running.server,
            thread: Mutex::new(Some((running.thread, running.done))),
        })
    }

    /// The port listened on.
    pub fn port(&self) -> u16 {
        self.server.port
    }

    /// The port that was wanted (`config.port`); [`Share::port`] differs when another program
    /// held it and a following one was taken. 0 when any port was asked for.
    pub fn requested_port(&self) -> u16 {
        self.server.requested_port
    }

    /// The code, 6 characters without separator.
    pub fn code(&self) -> String {
        self.server.code.clone()
    }

    pub fn status(&self) -> ShareStatus {
        self.server.status()
    }

    /// Shows the waiting screen. Ignored when already shown or once stopped.
    pub fn show_waiting(&self) {
        self.server.content.show_waiting();
    }

    /// Shows the live sheet: every viewer is handed to the feed. Ignored when already shown or
    /// once stopped.
    pub fn show_live(&self) {
        self.server.content.show_live();
    }

    /// Shows (or updates) the note's page. Ignored when `update.full` is what is shown already.
    pub fn show_note(&self, update: NoteUpdate) {
        self.server.content.show_note(update);
    }

    /// Freezes the shared image (`true`) or follows the content again (`false`).
    pub fn set_paused(&self, paused: bool) {
        self.server.content.set_paused(paused);
    }

    /// Tells every viewer `end`, closes every connection and the port, and waits for the
    /// server's thread (at most 2 s). Can be repeated. Must not be called from `on_change`: on
    /// the share's own thread it cannot wait (it then only asks the thread to stop).
    pub fn stop(&self) {
        // Held until the thread is joined: a second caller also returns with the port closed.
        let mut thread = self.thread.lock().unwrap_or_else(PoisonError::into_inner);
        self.server.content.stop();
        self.server.signal_shutdown();
        let Some((handle, done)) = thread.take() else {
            return;
        };
        if handle.thread().id() == std::thread::current().id() {
            return;
        }
        match done.recv_timeout(STOP_TIMEOUT) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                let _ = handle.join();
            }
            // Should not happen (every wait of the server is bounded): leave the thread.
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
        }
    }
}

impl Drop for Share {
    fn drop(&mut self) {
        self.stop();
    }
}

/// `K7MQ2X` → `K7M Q2X` (how the app shows the code): groups of three characters.
pub fn format_code(code: &str) -> String {
    let chars: Vec<char> = code.chars().collect();
    chars
        .chunks(3)
        .map(|group| group.iter().collect::<String>())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_shown_in_groups_of_three() {
        assert_eq!(format_code("K7MQ2X"), "K7M Q2X");
        assert_eq!(format_code("ABC"), "ABC");
        assert_eq!(format_code("ABCDEFG"), "ABC DEF G");
        assert_eq!(format_code(""), "");
    }

    #[test]
    fn error_codes() {
        assert_eq!(
            ShareError::PortInUse { port: 8765 }.code(),
            "sharePortInUse"
        );
        assert_eq!(ShareError::Random("x".into()).code(), "shareFailed");
        assert!(
            ShareError::PortInUse { port: 8765 }
                .to_string()
                .contains("8765")
        );
        let none = ShareError::NoFreePort {
            first: 8765,
            last: 8785,
        };
        assert_eq!(none.code(), "shareNoFreePort");
        assert!(none.to_string().contains("8765") && none.to_string().contains("8785"));
    }

    #[test]
    fn share_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Share>();
        assert_send_sync::<LiveViewer>();
    }
}
