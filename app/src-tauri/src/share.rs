//! Sharing the sheet on the local network (SPEC §4.10; docs/design/lan-share.md, "App"): starts
//! and stops the server of `ardoise-share`, tells it what the main window shows (live mode
//! through the [`LiveHub`], the editor's note, or the waiting screen), and keeps the windows told
//! of its status (address, code, screens connected).
//!
//! - [`assets`]: the viewer's files, from the interface embedded in the executable;
//! - [`feed`]: live mode, one hub subscription per viewer, stripped of library paths;
//! - [`page`]: the editor's note, whole or as the strokes added, with its paper.

pub mod assets;
pub mod feed;
pub mod page;

#[cfg(test)]
mod tests;

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use ardoise_core::note::Note;
use ardoise_share::qr::{QrCode, qr_code};
use ardoise_share::{Share, ShareConfig, ShareError, Shown};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, Webview};

use crate::AppContext;
use crate::background::staging;
use crate::commands::{CommandError, CommandResult, store};
use crate::live::{LiveHub, LiveRuntime};
use crate::notes::background_png;
use crate::thumbnails::png_data_url;
use assets::{AssetSource, viewer_assets};
use feed::HubFeed;
use page::{NoteSharer, Paper};

/// Test-only: listen on `127.0.0.1` instead of every interface (the e2e journeys: no firewall
/// prompt, no firewall rule left for each temporary copy of the executable).
const LOOPBACK_VARIABLE: &str = "ARDOISE_SHARE_LOOPBACK";

/// What the main window shows (`ShareContent` in `share.ts`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ShareContent {
    Waiting,
    Live,
    /// The note open in the editor, unsaved changes included.
    Note {
        note: Box<Note>,
        /// The staged pixels of its background image (`background::Staging`).
        #[serde(default)]
        image_key: Option<String>,
    },
}

/// `ShareStatus` in `share.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareStatus {
    pub active: bool,
    pub port: u16,
    /// `K7M Q2X` while active.
    pub code: Option<String>,
    /// `http://192.168.1.23:8765/`…, private LAN addresses first.
    pub addresses: Vec<String>,
    /// QR code of the first address.
    pub qr: Option<QrCode>,
    pub viewers: usize,
    pub paused: bool,
    pub shown: Shown,
}

impl ShareStatus {
    fn inactive(port: u16) -> ShareStatus {
        ShareStatus {
            active: false,
            port,
            code: None,
            addresses: Vec::new(),
            qr: None,
            viewers: 0,
            paused: false,
            shown: Shown::Waiting,
        }
    }
}

/// Error for the interface.
/// Error for the interface. A port in use carries only the port's number, which the translated
/// sentence (`errorCodes.sharePortInUse`) shows as `{detail}`.
fn share_error(error: &ShareError) -> CommandError {
    match error {
        ShareError::PortInUse { port } => CommandError::new(error.code(), port),
        other => CommandError::new(other.code(), other),
    }
}

/// A window following the status: false from `send` drops it.
type Subscriber = Box<dyn Fn(&ShareStatus) -> bool + Send + Sync>;

/// A running share and what was worked out when it started.
struct Running {
    share: Arc<Share>,
    addresses: Vec<String>,
    qr: Option<QrCode>,
}

struct Inner {
    running: Mutex<Option<Running>>,
    /// Held during a start, so that two starts never make two servers.
    starting: Mutex<()>,
    /// The last content the main window gave, also while no share runs.
    content: Mutex<ShareContent>,
    /// Bumped by each content given: only the latest one is applied.
    sequence: AtomicU64,
    /// Held while a content is applied (so contents are applied one at a time, in order).
    notes: Mutex<NoteSharer>,
    subscribers: Mutex<Vec<(String, Subscriber)>>,
}

/// The share of the running app (managed state).
pub struct ShareRuntime {
    inner: Arc<Inner>,
    /// The embedded interface ([`assets::share_context_assets`]); unset in a build that loads it
    /// from the dev server, and in the tests.
    assets: OnceLock<AssetSource>,
}

impl Default for ShareRuntime {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner {
                running: Mutex::new(None),
                starting: Mutex::new(()),
                content: Mutex::new(ShareContent::Waiting),
                sequence: AtomicU64::new(0),
                notes: Mutex::new(NoteSharer::default()),
                subscribers: Mutex::new(Vec::new()),
            }),
            assets: OnceLock::new(),
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The addresses to type on the other PC, private LAN ones first.
fn addresses_for(bind: Ipv4Addr, port: u16) -> Vec<String> {
    let ips = if bind.is_unspecified() {
        ardoise_share::addresses::local_ipv4()
    } else {
        vec![bind]
    };
    ips.iter()
        .map(|ip| format!("http://{ip}:{port}/"))
        .collect()
}

/// The paper as a PNG data URL (`render_background` with the staged image); `None` when it
/// cannot be made (the viewer shows the paper colour).
fn render_paper(paper: &Paper) -> Option<String> {
    let picture = paper
        .image
        .as_ref()
        .and_then(|(_, key)| staging().picture(key));
    let image = paper
        .image
        .clone()
        .zip(picture.as_deref())
        .map(|((image, _), picture)| (image, picture));
    background_png(paper.page, paper.background, paper.width_px(), image)
        .ok()
        .map(|png| png_data_url(&png))
}

impl Inner {
    fn status(&self, configured_port: u16) -> ShareStatus {
        let running = lock(&self.running);
        match running.as_ref() {
            Some(running) => {
                let crate_status = running.share.status();
                ShareStatus {
                    active: true,
                    port: crate_status.port,
                    code: Some(ardoise_share::format_code(&crate_status.code)),
                    addresses: running.addresses.clone(),
                    qr: running.qr.clone(),
                    viewers: crate_status.viewers,
                    paused: crate_status.paused,
                    shown: crate_status.shown,
                }
            }
            None => ShareStatus::inactive(configured_port),
        }
    }

    /// Tells every window; a window that went away is dropped.
    fn broadcast(&self, status: &ShareStatus) {
        lock(&self.subscribers).retain(|(_, send)| send(status));
    }

    /// The share running now.
    fn share(&self) -> Option<Arc<Share>> {
        lock(&self.running)
            .as_ref()
            .map(|running| running.share.clone())
    }

    /// Records `content` and applies it to the running share unless a newer content arrived
    /// meanwhile. Slow (a note's paper may be rendered): call it off the async runtime.
    fn show(&self, content: ShareContent) {
        let sequence = {
            let mut current = lock(&self.content);
            *current = content.clone();
            self.sequence.fetch_add(1, Ordering::SeqCst) + 1
        };
        self.apply(sequence, content);
    }

    /// Applies the content recorded as `sequence` (skipped when a newer one exists: it is
    /// applied by its own call).
    fn apply(&self, sequence: u64, content: ShareContent) {
        let mut notes = lock(&self.notes);
        if self.sequence.load(Ordering::SeqCst) != sequence {
            return;
        }
        let Some(share) = self.share() else {
            return;
        };
        match content {
            // The viewers leave the note: the next one goes whole, even the same note again.
            ShareContent::Waiting => {
                notes.reset();
                share.show_waiting();
            }
            ShareContent::Live => {
                notes.reset();
                share.show_live();
            }
            ShareContent::Note { note, image_key } => {
                if let Some(update) = notes.update(*note, image_key, render_paper) {
                    share.show_note(update);
                }
            }
        }
    }

    /// Applies the recorded content again (a share that just started).
    fn reapply(&self) {
        let (sequence, content) = {
            let current = lock(&self.content);
            (self.sequence.load(Ordering::SeqCst), current.clone())
        };
        lock(&self.notes).reset();
        self.apply(sequence, content);
    }
}

impl ShareRuntime {
    /// The embedded interface, set once by `lib.rs` before the app starts.
    pub fn set_assets(&self, source: AssetSource) {
        let _ = self.assets.set(source);
    }

    /// Whether the viewer is embedded (`--self-test`): the number of its files.
    pub fn viewer_check(&self) -> Result<usize, String> {
        let source = self
            .assets
            .get()
            .ok_or_else(|| "the interface is not embedded in this build".to_owned())?;
        viewer_assets(source.as_ref())
            .map(|assets| assets.len())
            .map_err(|error| error.to_string())
    }

    /// Starts sharing (or returns the share running) on `bind:port`, live mode from `hub`.
    fn start(&self, bind: Ipv4Addr, port: u16, hub: Arc<LiveHub>) -> CommandResult<ShareStatus> {
        // One start at a time (a double click); `running` itself is only held briefly, never
        // while the server starts, so that its status callback can always read it.
        let _starting = lock(&self.inner.starting);
        if lock(&self.inner.running).is_none() {
            let source = self.assets.get().ok_or_else(|| {
                CommandError::new(
                    "shareUnavailable",
                    "the interface is not embedded in this build of Ardoise",
                )
            })?;
            let assets = viewer_assets(source.as_ref())
                .map_err(|error| CommandError::new("shareUnavailable", error))?;
            let inner = Arc::downgrade(&self.inner);
            let on_change = Box::new(move |_status: ardoise_share::ShareStatus| {
                if let Some(inner) = inner.upgrade() {
                    let status = inner.status(port);
                    inner.broadcast(&status);
                }
            });
            let config = ShareConfig {
                bind,
                port,
                code: None,
            };
            let share = Share::start(config, assets, Arc::new(HubFeed::new(hub)), on_change)
                .map_err(|error| share_error(&error))?;
            let port = share.port();
            let addresses = addresses_for(bind, port);
            let qr = addresses.first().and_then(|address| qr_code(address));
            *lock(&self.inner.running) = Some(Running {
                share: Arc::new(share),
                addresses,
                qr,
            });
            self.inner.reapply();
        }
        let status = self.inner.status(port);
        self.inner.broadcast(&status);
        Ok(status)
    }

    /// Stops sharing: every browser is told, the port closes. Can be repeated.
    pub fn stop(&self) {
        let running = lock(&self.inner.running).take();
        if let Some(running) = running {
            running.share.stop();
            lock(&self.inner.notes).reset();
        }
        // What the window showed then may be gone by the next share: it says again.
        *lock(&self.inner.content) = ShareContent::Waiting;
    }

    fn status(&self, configured_port: u16) -> ShareStatus {
        self.inner.status(configured_port)
    }

    fn subscribe(&self, label: &str, subscriber: Subscriber) {
        let mut subscribers = lock(&self.inner.subscribers);
        subscribers.retain(|(other, _)| other != label);
        subscribers.push((label.to_owned(), subscriber));
    }
}

/// `[share] port` of `ardoise.toml` (the default when the settings cannot be read).
fn configured_port(context: &AppContext) -> u16 {
    store(context)
        .ok()
        .and_then(|store| store.load().ok())
        .map_or(ardoise_core::settings::DEFAULT_SHARE_PORT, |settings| {
            settings.share.port
        })
}

/// Every IPv4 interface, or the loopback one for the e2e journeys ([`LOOPBACK_VARIABLE`]).
fn bind_address() -> Ipv4Addr {
    if std::env::var_os(LOOPBACK_VARIABLE).is_some_and(|value| value == "1") {
        Ipv4Addr::LOCALHOST
    } else {
        Ipv4Addr::UNSPECIFIED
    }
}

/// The app exits: the port closes and every browser is told the share ended.
pub fn stop_at_exit(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<ShareRuntime>() {
        runtime.stop();
    }
}

#[tauri::command]
pub async fn share_status(
    context: State<'_, AppContext>,
    runtime: State<'_, ShareRuntime>,
) -> CommandResult<ShareStatus> {
    Ok(runtime.status(configured_port(&context)))
}

/// The status now; then the status at every change (one subscription per window).
#[tauri::command]
pub async fn share_subscribe(
    webview: Webview,
    context: State<'_, AppContext>,
    runtime: State<'_, ShareRuntime>,
    on_event: Channel<ShareStatus>,
) -> CommandResult<ShareStatus> {
    runtime.subscribe(
        webview.label(),
        Box::new(move |status| on_event.send(status.clone()).is_ok()),
    );
    Ok(runtime.status(configured_port(&context)))
}

/// Starts sharing (or returns the share running); it shows what the main window last said.
#[tauri::command]
pub async fn share_start(app: AppHandle) -> CommandResult<ShareStatus> {
    crate::notes::blocking(move || {
        let context = app.state::<AppContext>();
        let runtime = app.state::<ShareRuntime>();
        let hub = app.state::<LiveRuntime>().shared_hub();
        runtime.start(bind_address(), configured_port(&context), hub)
    })
    .await
}

#[tauri::command]
pub async fn share_stop(app: AppHandle) -> CommandResult<ShareStatus> {
    crate::notes::blocking(move || {
        let context = app.state::<AppContext>();
        let runtime = app.state::<ShareRuntime>();
        runtime.stop();
        let status = runtime.status(configured_port(&context));
        runtime.inner.broadcast(&status);
        Ok(status)
    })
    .await
}

#[tauri::command]
pub async fn share_set_paused(
    context: State<'_, AppContext>,
    runtime: State<'_, ShareRuntime>,
    paused: bool,
) -> CommandResult<ShareStatus> {
    if let Some(share) = runtime.inner.share() {
        share.set_paused(paused);
    }
    let status = runtime.status(configured_port(&context));
    runtime.inner.broadcast(&status);
    Ok(status)
}

/// What the main window shows now. Recorded while no share runs (shown when one starts).
#[tauri::command]
pub async fn share_show(app: AppHandle, content: ShareContent) -> CommandResult<()> {
    crate::notes::blocking(move || {
        app.state::<ShareRuntime>().inner.show(content);
        Ok(())
    })
    .await
}
