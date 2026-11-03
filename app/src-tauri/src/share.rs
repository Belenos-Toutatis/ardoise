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
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError, TryLockError};

use ardoise_core::note::Note;
use ardoise_core::settings::DEFAULT_SHARE_PORT;
use ardoise_share::qr::{QrCode, qr_code};
use ardoise_share::{Share, ShareConfig, ShareError, Shown};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, Webview};

use crate::AppContext;
use crate::background::staging;
use crate::commands::{CommandError, CommandResult, store, update};
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
    /// The port listened on; the one this PC would ask for (`configured_port`) when inactive.
    pub port: u16,
    /// The port the share was started asking for; it differs from `port` when another
    /// program held that one and a following port was taken.
    pub requested_port: u16,
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
            requested_port: port,
            code: None,
            addresses: Vec::new(),
            qr: None,
            viewers: 0,
            paused: false,
            shown: Shown::Waiting,
        }
    }
}

/// Error for the interface. A port in use carries only the port's number, and no free port the
/// range tried (`8765-8785`): the translated sentences (`errorCodes.sharePortInUse`,
/// `errorCodes.shareNoFreePort`) show it as `{detail}`.
fn share_error(error: &ShareError) -> CommandError {
    match error {
        ShareError::PortInUse { port } => CommandError::new(error.code(), port),
        ShareError::NoFreePort { first, last } => {
            CommandError::new(error.code(), format!("{first}-{last}"))
        }
        other => CommandError::new(other.code(), other),
    }
}

/// A window following the status: false from `send` drops it.
type Subscriber = Box<dyn Fn(&ShareStatus) -> bool + Send + Sync>;

/// A running share and what was worked out when it started.
struct Running {
    share: Arc<Share>,
    /// The port asked for when it started (`ShareStatus::requested_port`).
    requested_port: u16,
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
                    requested_port: running.requested_port,
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

    /// Forgets the note the viewers showed, unless a content is being applied: it may be
    /// rendering a paper, which takes seconds, and closing Ardoise must not wait for it. Nothing
    /// is lost by skipping: `reapply` forgets it again when a share starts.
    fn forget_note(&self) {
        match self.notes.try_lock() {
            Ok(mut notes) => notes.reset(),
            Err(TryLockError::Poisoned(poisoned)) => poisoned.into_inner().reset(),
            Err(TryLockError::WouldBlock) => {}
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

    /// Starts sharing (or returns the share running) on `bind:port` or, when another program
    /// holds it, on the first free port after it; live mode from `hub`; the journal of its
    /// connections in `journal`, if given.
    fn start(
        &self,
        bind: Ipv4Addr,
        port: u16,
        hub: Arc<LiveHub>,
        journal: Option<PathBuf>,
    ) -> CommandResult<ShareStatus> {
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
                fallback_ports: ardoise_share::FALLBACK_PORTS,
                code: None,
                journal,
            };
            let share = Share::start(config, assets, Arc::new(HubFeed::new(hub)), on_change)
                .map_err(|error| share_error(&error))?;
            let addresses = addresses_for(bind, share.port());
            let qr = addresses.first().and_then(|address| qr_code(address));
            *lock(&self.inner.running) = Some(Running {
                share: Arc::new(share),
                requested_port: port,
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
            self.inner.forget_note();
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

/// The port this PC asks for first: its own `share_port` in `ardoise.toml` (the section of this
/// computer), else `[share] port`, else 8765 (also when the settings cannot be read).
fn configured_port(context: &AppContext) -> u16 {
    port_setting(context).port
}

/// Keeps `port`, the one a share had to move to, for this PC: the address stays the same from
/// one share to the next. The share runs either way: a settings file that cannot be written
/// only costs the next share the same search.
fn remember_port(context: &AppContext, port: u16) {
    let Ok(store) = store(context) else {
        return;
    };
    let machine = store.machine().to_owned();
    if let Err(error) = update(context, |settings| {
        settings.set_share_port(&machine, Some(port));
    }) {
        eprintln!(
            "share: cannot keep port {port} for this PC: {}",
            error.parts().1
        );
    }
}

/// Where a share writes the journal of its connections: `share_YYYY-MM-DD_HHMMSS.log` beside
/// the tablet's diagnostic logs (`<library>/_ardoise/diagnostics/`), which come back to the
/// developer through the synchronised library. `None` without a library.
fn journal_path(context: &AppContext) -> Option<PathBuf> {
    let store = store(context).ok()?;
    let settings = store.load().ok()?;
    let library = settings.library_for(store.machine())?;
    journal_path_in(library, chrono::Local::now())
}

/// A free `share_<local time>.log` in `library`'s diagnostics folder (created if needed).
fn journal_path_in(library: &Path, now: chrono::DateTime<chrono::Local>) -> Option<PathBuf> {
    let dir = library
        .join(ardoise_core::library::META_DIR)
        .join(ardoise_device::log::DIAGNOSTICS_DIR);
    std::fs::create_dir_all(&dir).ok()?;
    let stem = format!("share_{}", now.format("%Y-%m-%d_%H%M%S"));
    let mut path = dir.join(format!("{stem}.log"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.log"));
        n += 1;
    }
    Some(path)
}

/// Starts the share on this PC's port; when it had to take another one, keeps that one.
fn start_and_remember(
    runtime: &ShareRuntime,
    context: &AppContext,
    bind: Ipv4Addr,
    hub: Arc<LiveHub>,
) -> CommandResult<ShareStatus> {
    let wanted = configured_port(context);
    let status = runtime.start(bind, wanted, hub, journal_path(context))?;
    if status.active && status.port != wanted && status.requested_port == wanted {
        remember_port(context, status.port);
    }
    Ok(status)
}

/// What the settings screen shows of the share's port (`SharePortSetting` in `share.ts`).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharePortSetting {
    /// The port this PC asks for first.
    pub port: u16,
    /// `[share] port` of `ardoise.toml`, which every PC without a port of its own uses.
    pub shared_port: u16,
    /// This PC has a port of its own (chosen in the settings, or kept after a conflict).
    pub own: bool,
}

/// Lowest and highest ports the settings accept (below 1024 the ports are the system's).
const LOWEST_PORT: u16 = 1024;
const HIGHEST_PORT: u16 = u16::MAX;

fn port_setting(context: &AppContext) -> SharePortSetting {
    let loaded = store(context).ok().and_then(|store| {
        let settings = store.load().ok()?;
        Some((settings, store.machine().to_owned()))
    });
    match loaded {
        Some((settings, machine)) => SharePortSetting {
            port: settings.share_port_for(&machine),
            shared_port: settings.share.port,
            own: settings.machine_share_port(&machine).is_some(),
        },
        None => SharePortSetting {
            port: DEFAULT_SHARE_PORT,
            shared_port: DEFAULT_SHARE_PORT,
            own: false,
        },
    }
}

/// Keeps `port` for the share of this PC (`sharePortInvalid` unless from 1024 to 65535).
fn set_port_in(context: &AppContext, port: i64) -> CommandResult<SharePortSetting> {
    let port = u16::try_from(port)
        .ok()
        .filter(|port| (LOWEST_PORT..=HIGHEST_PORT).contains(port))
        .ok_or_else(|| {
            CommandError::new("sharePortInvalid", format!("{LOWEST_PORT}-{HIGHEST_PORT}"))
        })?;
    let machine = store(context)?.machine().to_owned();
    update(context, |settings| {
        settings.set_share_port(&machine, Some(port));
    })?;
    Ok(port_setting(context))
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
        start_and_remember(&runtime, &context, bind_address(), hub)
    })
    .await
}

/// The port of the share on this PC (settings screen).
#[tauri::command]
pub async fn share_port(context: State<'_, AppContext>) -> CommandResult<SharePortSetting> {
    Ok(port_setting(&context))
}

/// Chooses the port of the share on this PC, from 1024 to 65535 (`sharePortInvalid` otherwise).
/// A share running keeps its port; the next one uses the new one.
#[tauri::command]
pub async fn set_share_port(
    context: State<'_, AppContext>,
    port: i64,
) -> CommandResult<SharePortSetting> {
    set_port_in(&context, port)
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
