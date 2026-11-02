//! The worker thread of the tablet service: a state machine that does one bounded step at a
//! time (a scan slice, a connection, one page of a sync, one live notification, a short idle
//! wait) and handles the requests of the handle between steps.
//!
//! What it owns: the connector (inside the `Manager`), the `Manager` of the current identity
//! root, the session with the connected tablet, the sync under way, the live sink.

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, UNIX_EPOCH};

use ardoise_core::library::Library;
use ardoise_core::note::PageSize;
use ardoise_proto::live::{self, LivePacket};

use super::journal::Journal;
use super::{
    ADAPTER_RECHECK, BUTTON_TIMEOUT, Connection, ConnectorFactory, DeviceDetails, Interrupts,
    LastSync, LiveInput, LiveSink, LiveState, PageContext, PageStore, Pending, REGISTER_SEARCH,
    RETRY_DELAY, RegisterStep, RegisteredTablet, Request, SEARCH_SLICE, Sent, ServiceConfig,
    ServiceHooks, StatusCallback, StatusError, SyncProgress, TabletOptions, TabletStatus,
    model_name,
};
use crate::identity::{self, DeviceRecord};
use crate::manager::Manager;
use crate::pages::{ImportedPages, orient_record, page_note_id};
use crate::session::{DeviceError, DeviceInfo, LiveNotification, Session, unix_seconds};
use crate::transport::{Advertisement, Channel};

/// While connected and idle: how often the link is checked (a lost link is noticed this fast,
/// and requests wait at most this long).
const IDLE_POLL: Duration = Duration::from_millis(50);
/// In live mode: longest wait for a notification before the requests are looked at.
const LIVE_POLL: Duration = Duration::from_millis(20);
/// Pause between two unsuccessful scans (a virtual-time simulator returns at once).
const SEARCH_PAUSE: Duration = Duration::from_millis(20);
/// How often `device.json` is read again while no tablet is registered: another PC may
/// register it through the shared library.
const REGISTRATION_RECHECK: Duration = Duration::from_secs(10);
/// Live notifications between two checks of the log's length.
const ROLL_CHECK: u32 = 1000;
/// While connected and idle: how often the number of stored pages is read again, so that a
/// page finished while the tablet stays connected shows, and syncs (SPEC §4.1). The tablet may
/// keep the link longer for these requests (supposed: protocol.md does not say what resets its
/// idle timer; to watch at H2).
pub(super) const PAGE_COUNT_INTERVAL: Duration = Duration::from_secs(30);
/// Errors that disappear by themselves when their cause does (adapter, library).
const CONDITION_ERRORS: [&str; 3] = ["bluetoothOff", "bluetoothMissing", "noLibrary"];

/// Publishes the status: only real changes reach the callback, in order (only the worker
/// publishes, after the first status).
#[derive(Clone)]
pub(super) struct Publisher {
    status: Arc<Mutex<TabletStatus>>,
    on_status: Arc<StatusCallback>,
}

impl Publisher {
    pub(super) fn new(status: Arc<Mutex<TabletStatus>>, on_status: Arc<StatusCallback>) -> Self {
        Self { status, on_status }
    }

    /// Applies `change`; calls the callback (lock released) when the status changed.
    pub(super) fn update(&self, change: impl FnOnce(&mut TabletStatus)) {
        let changed = {
            let mut status = self.status.lock().unwrap_or_else(PoisonError::into_inner);
            let before = status.clone();
            change(&mut status);
            (*status != before).then(|| status.clone())
        };
        if let Some(status) = changed {
            (self.on_status)(&status);
        }
    }

    fn get(&self) -> TabletStatus {
        self.status
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

/// A sync under way.
struct SyncRun {
    library: Library,
    root: PathBuf,
    imported: ImportedPages,
    /// Pages on the tablet when the sync started.
    total: u16,
    /// Pages this sync reads (only the oldest one without deletion).
    to_read: u16,
    done: u16,
    delete: bool,
    saved: Vec<String>,
    deleted: u16,
    context: PageContext,
}

/// Live mode asked by the hub.
struct LiveRun {
    sink: Box<dyn LiveSink>,
    /// `Started` was sent to this sink.
    started: bool,
    /// The session is in live mode for this sink.
    active: bool,
    notifications: u32,
}

/// Why a synced page is not stored.
enum Known {
    /// Its note is in the library (path).
    InLibrary(PathBuf),
    /// Its note was deleted by the user (sync without deletion).
    InTrash,
}

pub(super) struct Worker {
    config: ServiceConfig,
    factory: Box<dyn ConnectorFactory>,
    hooks: Arc<dyn ServiceHooks>,
    publisher: Publisher,
    store_page: PageStore,
    requests: Receiver<Sent>,
    interrupts: Arc<Interrupts>,
    /// The `Manager`'s cancellation flag (raised by `interrupts`).
    interrupt: Arc<AtomicBool>,
    manager: Option<Manager>,
    journal: Option<Journal>,
    record: Option<DeviceRecord>,
    session: Option<Session>,
    sync: Option<SyncRun>,
    /// A sync waits (asked, automatic, or interrupted by a lost link).
    want_sync: bool,
    live: Option<LiveRun>,
    /// When the page count was last read on this connection (the session clock's monotonic
    /// time), `None` without a connection.
    counted_at: Option<Duration>,
    stop: bool,
}

impl Worker {
    pub(super) fn new(
        config: ServiceConfig,
        factory: Box<dyn ConnectorFactory>,
        hooks: Arc<dyn ServiceHooks>,
        publisher: Publisher,
        store_page: PageStore,
        requests: Receiver<Sent>,
        interrupts: Arc<Interrupts>,
    ) -> Self {
        Self {
            config,
            factory,
            hooks,
            publisher,
            store_page,
            requests,
            interrupt: interrupts.flag(),
            interrupts,
            manager: None,
            journal: None,
            record: None,
            session: None,
            sync: None,
            want_sync: false,
            live: None,
            counted_at: None,
            stop: false,
        }
    }

    pub(super) fn run(mut self) {
        self.load_record();
        while !self.stop {
            self.take_requests();
            if self.stop {
                break;
            }
            self.step();
        }
        self.close();
    }

    // --- requests ---------------------------------------------------------------------------

    /// Handles every request already sent, without waiting.
    fn take_requests(&mut self) {
        while !self.stop {
            match self.requests.try_recv() {
                Ok(sent) => self.take(sent),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => self.stop = true,
            }
        }
    }

    /// Handles a request taken from the channel. The interrupt flag is lowered only for a
    /// request taken, and only when no other interrupting request waits ([`Interrupts`]).
    fn take(&mut self, sent: Sent) {
        if sent.interrupting {
            self.interrupts.taken();
        }
        self.handle(sent.request);
    }

    /// Waits up to `timeout` (forever with `None`) for a request and handles it.
    fn wait(&mut self, timeout: Option<Duration>) {
        let received = match timeout {
            Some(timeout) => self
                .requests
                .recv_timeout(timeout)
                .map_err(|e| e == RecvTimeoutError::Disconnected),
            None => self.requests.recv().map_err(|_| true),
        };
        match received {
            Ok(sent) => self.take(sent),
            Err(gone) => self.stop |= gone,
        }
    }

    fn handle(&mut self, request: Request) {
        match request {
            Request::Register => self.register(),
            Request::SyncNow => self.sync_now(),
            Request::Cancel => self.cancel(),
            Request::Rename(name, answer) => {
                let result = self.rename(&name);
                let _ = answer.send(result);
            }
            Request::SetTime(answer) => {
                let result = self.set_time();
                let _ = answer.send(result);
            }
            Request::SetOptions(options) => self.set_options(options),
            Request::Forget(answer) => {
                let result = self.forget();
                let _ = answer.send(result);
            }
            Request::SetLibrary(library, root) => self.set_library(library, root),
            Request::StartLive(sink) => self.start_live(sink),
            Request::StopLive => self.stop_live(),
            Request::Shutdown => self.stop = true,
        }
    }

    // --- steps ------------------------------------------------------------------------------

    fn step(&mut self) {
        if self.session.is_some() {
            self.connected_step();
            return;
        }
        if self.config.library.is_none() || self.config.identity_root.is_none() {
            self.unavailable(StatusError::from(&DeviceError::NoLibrary));
            self.wait(None);
            return;
        }
        if !self.ensure_manager() {
            self.wait(Some(ADAPTER_RECHECK));
            return;
        }
        self.load_record();
        let Some(record) = self.record.clone() else {
            self.publish(Connection::NotRegistered);
            self.wait(Some(REGISTRATION_RECHECK));
            return;
        };
        self.search(&record);
    }

    /// Checks the adapter and makes the `Manager` of the identity root if needed. `false`
    /// (status `unavailable`) when Bluetooth cannot be used.
    fn ensure_manager(&mut self) -> bool {
        if let Err(error) = self.factory.check() {
            self.unavailable(error);
            return false;
        }
        if self.manager.is_none() {
            let Some(root) = self.config.identity_root.clone() else {
                return false;
            };
            let connector = match self.factory.connector() {
                Ok(connector) => connector,
                Err(error) => {
                    self.unavailable(StatusError::from(&DeviceError::Transport(error)));
                    return false;
                }
            };
            let transport = if self.config.simulated { "sim" } else { "ble" };
            let journal = Journal::new(transport, connector.clock());
            let manager = Manager::new(connector, root, journal.shared())
                .with_cancel_flag(self.interrupt.clone());
            self.manager = Some(manager);
            self.journal = Some(journal);
        }
        self.publisher.update(|status| {
            if status
                .error
                .as_ref()
                .is_some_and(|e| CONDITION_ERRORS.contains(&e.code.as_str()))
            {
                status.error = None;
            }
        });
        true
    }

    /// Looks for the registered tablet during one slice, and connects when it is seen.
    fn search(&mut self, record: &DeviceRecord) {
        self.publish(Connection::Searching);
        if let Some(journal) = self.journal.as_mut() {
            journal.begin();
        }
        let Some(manager) = self.manager.as_mut() else {
            return;
        };
        match manager.find_registered(record, SEARCH_SLICE) {
            Ok(Some(tablet)) => self.connect(record, &tablet),
            Ok(None) => self.wait(Some(SEARCH_PAUSE)),
            // Interrupted by a request: handled before the next step.
            Err(DeviceError::Cancelled) => {}
            Err(error) => self.connection_failed(&error),
        }
    }

    /// Connects (auth, clock), reads the tablet's information and its page count.
    fn connect(&mut self, record: &DeviceRecord, tablet: &Advertisement) {
        self.publish(Connection::Connecting);
        let Some(manager) = self.manager.as_mut() else {
            return;
        };
        let result =
            manager
                .connect(record, Some(&tablet.address))
                .and_then(|(mut session, _drift)| {
                    let info = session.info()?;
                    let pages = session.stored_pages()?;
                    Ok((session, info, pages))
                });
        match result {
            Ok((session, info, pages)) => {
                self.session = Some(session);
                self.connected(&info, pages);
            }
            Err(DeviceError::Cancelled) => {}
            Err(error) => self.connection_failed(&error),
        }
    }

    /// The tablet is connected and authenticated: status, `device.json`, automatic sync.
    fn connected(&mut self, info: &DeviceInfo, pages: u16) {
        if let Some(journal) = self.journal.as_mut() {
            journal.success();
            journal.note(format!(
                "connected to {} ({}), {pages} pages stored",
                info.name, info.firmware
            ));
        }
        self.remember_info(info);
        self.counted_at = self.session_time();
        if self.config.options.auto_sync && pages > 0 {
            self.want_sync = true;
        }
        let details = self.details(info, pages);
        let registered = self.record.as_ref().map(registered_tablet);
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Connected;
            status.device = Some(details);
            status.registered = registered;
            status.pending = pending;
            status.error = None;
        });
    }

    /// A connection attempt failed: shown, logged, tried again later.
    fn connection_failed(&mut self, error: &DeviceError) {
        if matches!(
            error,
            DeviceError::Transport(
                crate::transport::TransportError::NoAdapter
                    | crate::transport::TransportError::AdapterOff
            )
        ) {
            self.unavailable(StatusError::from(error));
            self.wait(Some(ADAPTER_RECHECK));
            return;
        }
        self.problem(error);
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Searching;
            status.pending = pending;
        });
        self.wait(Some(RETRY_DELAY));
    }

    fn connected_step(&mut self) {
        if self.sync.is_some() {
            self.sync_step();
        } else if self.live.is_some() {
            self.live_step();
        } else if self.want_sync {
            self.start_sync();
        } else if self.page_count_due() {
            self.count_pages();
        } else {
            let result = match self.session.as_mut() {
                Some(session) => session.idle(Duration::ZERO),
                None => return,
            };
            match result {
                Ok(()) => {
                    self.publish(Connection::Connected);
                    self.wait(Some(IDLE_POLL));
                }
                Err(error) => self.session_failed(error),
            }
        }
    }

    /// Whether the page count is due again (connected and idle, [`PAGE_COUNT_INTERVAL`] after
    /// the last time it was read).
    fn page_count_due(&self) -> bool {
        match (self.counted_at, self.session_time()) {
            (Some(counted), Some(now)) => now.saturating_sub(counted) >= PAGE_COUNT_INTERVAL,
            _ => false,
        }
    }

    /// Reads the page count again: the status follows, and new pages are synced when the
    /// automatic sync is on. A refusal or a lost link is handled as any failure of the idle
    /// link.
    fn count_pages(&mut self) {
        self.counted_at = self.session_time();
        let counted = match self.session.as_mut() {
            Some(session) => session.stored_pages(),
            None => return,
        };
        let pages = match counted {
            Ok(pages) => pages,
            Err(error) => return self.session_failed(error),
        };
        let before = self.stored_pages();
        if pages != before {
            self.note(format!("{pages} pages stored (were {before})"));
        }
        if self.config.options.auto_sync && pages > before {
            self.want_sync = true;
        }
        let pending = self.pending();
        self.publisher.update(|status| {
            if let Some(device) = status.device.as_mut() {
                device.stored_pages = pages;
            }
            status.pending = pending;
        });
    }

    /// The session reported `error` outside a sync or live mode.
    fn session_failed(&mut self, error: DeviceError) {
        match error {
            DeviceError::Cancelled => {}
            DeviceError::Disconnected => self.link_lost(),
            other => {
                self.problem(&other);
                // The link is in an unknown state: start again from a fresh connection.
                self.close_session();
                self.publish(Connection::Searching);
            }
        }
    }

    /// The tablet dropped the link (switched off, out of range, idle): back to searching.
    fn link_lost(&mut self) {
        self.note("the tablet disconnected");
        self.session = None;
        self.counted_at = None;
        if let Some(run) = self.sync.take() {
            // The page in transfer stays on the tablet; the sync goes on at the next connection.
            self.want_sync = true;
            self.keep("the link was lost during a sync");
            self.finish_sync(run, Some(&DeviceError::Disconnected));
        }
        if let Some(run) = self.live.as_mut()
            && run.active
        {
            run.active = false;
            run.sink.input(LiveInput::State(LiveState::Reconnecting));
        }
        self.write_journal();
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Searching;
            status.pending = pending;
            if status.register_step == Some(RegisterStep::Done) {
                status.register_step = None;
            }
        });
    }

    // --- sync -------------------------------------------------------------------------------

    fn sync_now(&mut self) {
        if self.sync.is_some() {
            return;
        }
        if self.record.is_none() {
            self.show_error(&DeviceError::NoTabletRegistered);
            return;
        }
        self.want_sync = true;
        let pending = self.pending();
        self.publisher.update(|status| status.pending = pending);
    }

    fn start_sync(&mut self) {
        self.want_sync = false;
        let Some(root) = self.config.library.clone() else {
            self.show_error(&DeviceError::NoLibrary);
            return;
        };
        let library = match Library::open(&root) {
            Ok(library) => library,
            Err(error) => {
                self.show_error(&DeviceError::Storage(error.to_string()));
                return;
            }
        };
        let counted = match self.session.as_mut() {
            Some(session) => session
                .set_file_transfer_reporting()
                .and_then(|()| session.stored_pages()),
            None => return,
        };
        let total = match counted {
            Ok(total) => total,
            Err(error) => {
                self.session_failed(error);
                return;
            }
        };
        let delete = self.config.options.delete_after_import;
        // Without deletion the tablet only lets us read its oldest page.
        let to_read = if delete { total } else { total.min(1) };
        let imported = if to_read > 0 {
            ImportedPages::scan(&library)
        } else {
            ImportedPages::default()
        };
        let context = PageContext {
            size: self.page_size(),
            orientation: self.config.options.orientation,
        };
        self.note(format!(
            "sync: {total} pages on the tablet, {to_read} to read, delete after import: {delete}"
        ));
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Syncing;
            status.pending = pending;
            status.sync = Some(SyncProgress {
                current: 0,
                total: to_read,
                saved: 0,
            });
            if let Some(device) = status.device.as_mut() {
                device.stored_pages = total;
            }
        });
        let run = SyncRun {
            library,
            root,
            imported,
            total,
            to_read,
            done: 0,
            delete,
            saved: Vec::new(),
            deleted: 0,
            context,
        };
        if to_read == 0 {
            self.finish_sync(run, None);
        } else {
            self.sync = Some(run);
        }
    }

    /// Downloads, stores and (when allowed, once stored) deletes one page.
    fn sync_step(&mut self) {
        let (Some(run), Some(session)) = (self.sync.as_mut(), self.session.as_mut()) else {
            return;
        };
        let index = run.done + 1;
        let (to_read, saved) = (run.to_read, saved_count(run));
        self.publisher.update(|status| {
            status.sync = Some(SyncProgress {
                current: index,
                total: to_read,
                saved,
            });
        });
        let page = match session.download_oldest() {
            // Notifications can be lost on the way (BLE stack buffers): try once more.
            Err(DeviceError::CrcMismatch { .. }) => session.download_oldest(),
            other => other,
        };
        let page = match page {
            Ok(page) => page,
            Err(error) => return self.sync_failed(error),
        };
        let known = match run.imported.find(&run.library, &page) {
            Some(path) => Some(Known::InLibrary(path)),
            None if !run.delete && run.imported.in_trash(&page) => Some(Known::InTrash),
            None => None,
        };
        let message = match known {
            Some(Known::InLibrary(path)) => {
                format!("page {index} already imported as {}", path.display())
            }
            Some(Known::InTrash) => format!("page {index} already imported, its note deleted"),
            None => {
                let id = page_note_id(&page);
                // The store is the app's code (thumbnail, export): a panic there must not
                // end the service; the page then stays on the tablet.
                let stored = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    (self.store_page)(&run.root, page, &run.context)
                }))
                .unwrap_or_else(|_| Err("the page store failed unexpectedly".into()));
                match stored {
                    Ok(path) => {
                        let message = format!("page {index} saved as {}", path.display());
                        run.imported.add(id, &path);
                        run.saved.push(path.display().to_string());
                        message
                    }
                    // The page stays on the tablet.
                    Err(message) => return self.sync_failed(DeviceError::Storage(message)),
                }
            }
        };
        // Only now, the page safe in the library, may it leave the tablet; a cancellation
        // arriving meanwhile takes effect after it (cancel stops between pages).
        if run.delete {
            session.set_cancel_flag(Arc::new(AtomicBool::new(false)));
            let deleted = session.delete_oldest();
            session.set_cancel_flag(self.interrupt.clone());
            if let Err(error) = deleted {
                return self.sync_failed(error);
            }
            run.deleted += 1;
        }
        run.done = index;
        let finished = run.done >= run.to_read;
        let saved = saved_count(run);
        self.note(message);
        self.publisher.update(|status| {
            status.sync = Some(SyncProgress {
                current: index,
                total: to_read,
                saved,
            });
        });
        if finished && let Some(run) = self.sync.take() {
            self.finish_sync(run, None);
        }
    }

    fn sync_failed(&mut self, error: DeviceError) {
        let Some(run) = self.sync.take() else {
            return;
        };
        match error {
            // Cancelled by a request: the pages already saved stay saved (and deleted).
            DeviceError::Cancelled => self.finish_sync(run, None),
            DeviceError::Disconnected => {
                self.sync = Some(run);
                self.link_lost();
            }
            other => {
                self.problem(&other);
                self.finish_sync(run, Some(&other));
            }
        }
    }

    /// Ends a sync: `last_sync`, the page count, back to connected.
    fn finish_sync(&mut self, run: SyncRun, error: Option<&DeviceError>) {
        self.note(format!(
            "sync finished: {} of {} pages saved, {} deleted",
            run.saved.len(),
            run.to_read,
            run.deleted
        ));
        let at = self.now_ms();
        let remaining = run.total.saturating_sub(run.deleted);
        // The sync just counted the pages.
        self.counted_at = self.session_time();
        let error = error.map(StatusError::from);
        let connection = if self.session.is_some() {
            Connection::Connected
        } else {
            Connection::Searching
        };
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = connection;
            status.pending = pending;
            status.sync = None;
            status.last_sync = Some(LastSync {
                at,
                saved: run.saved,
                deleted: run.deleted,
            });
            if let Some(device) = status.device.as_mut() {
                device.stored_pages = remaining;
            }
            status.error = error;
        });
    }

    fn cancel(&mut self) {
        if let Some(run) = self.sync.take() {
            self.note("sync cancelled");
            self.finish_sync(run, None);
        }
        if self.want_sync {
            self.want_sync = false;
            let pending = self.pending();
            self.publisher.update(|status| status.pending = pending);
        }
    }

    // --- live mode --------------------------------------------------------------------------

    fn start_live(&mut self, mut sink: Box<dyn LiveSink>) {
        let mut active = false;
        if let Some(mut old) = self.live.take() {
            old.sink.input(LiveInput::State(LiveState::Stopped));
            active = old.active;
        }
        let refused = if self.config.library.is_none() || self.config.identity_root.is_none() {
            Some(DeviceError::NoLibrary)
        } else if self.record.is_none() {
            Some(DeviceError::NoTabletRegistered)
        } else {
            None
        };
        if let Some(error) = refused {
            sink.input(LiveInput::State(LiveState::Stopped));
            self.show_error(&error);
            return;
        }
        let mut run = LiveRun {
            sink,
            started: false,
            active,
            notifications: 0,
        };
        if active {
            // Live mode already streams (another sink before): the new one starts at once.
            self.started(&mut run);
        } else if self.session.is_none() || self.sync.is_some() {
            run.sink.input(LiveInput::State(LiveState::Waiting));
        }
        self.live = Some(run);
        let pending = self.pending();
        self.publisher.update(|status| status.pending = pending);
    }

    /// Tells a sink that live mode streams.
    fn started(&self, run: &mut LiveRun) {
        if !run.started {
            let (width, height) = self.tablet_size();
            run.sink.input(LiveInput::Started {
                width,
                height,
                orientation: self.config.options.orientation,
            });
            run.started = true;
        }
        run.sink.input(LiveInput::State(LiveState::Live));
    }

    /// Tells the live sink that the tablet is held another way, whether live mode streams or
    /// still waits for the tablet (not connected, reconnecting): the session takes the new way at
    /// once, so the interface shows the switch, and the `Started` that follows finds nothing to
    /// turn again. Done on this thread between two notifications: the records before are
    /// oriented the old way, the ones after the new way.
    fn reoriented(&mut self) {
        let (width, height) = self.tablet_size();
        let orientation = self.config.options.orientation;
        if let Some(run) = self.live.as_mut() {
            run.sink.input(LiveInput::Reoriented {
                width,
                height,
                orientation,
            });
        }
        self.note(format!("live orientation changed: {orientation:?}"));
    }

    /// Enters live mode if needed, then forwards one notification.
    fn live_step(&mut self) {
        let Some(session) = self.session.as_mut() else {
            return;
        };
        if !session.in_live_mode() {
            if let Err(error) = session.start_live() {
                return self.live_failed(error);
            }
            let Some(mut run) = self.live.take() else {
                return;
            };
            self.started(&mut run);
            run.active = true;
            self.live = Some(run);
            self.note("live mode started");
            self.publish(Connection::Live);
            self.hooks.live_started();
            return;
        }
        match session.next_live_event(LIVE_POLL) {
            Ok(Some(notification)) => self.forward(notification),
            Ok(None) | Err(DeviceError::Cancelled) => {}
            Err(error) => self.live_failed(error),
        }
    }

    fn live_failed(&mut self, error: DeviceError) {
        match error {
            DeviceError::Cancelled => {}
            DeviceError::Disconnected => self.link_lost(),
            other => {
                if let Some(mut run) = self.live.take() {
                    run.sink.input(LiveInput::State(LiveState::Stopped));
                }
                self.session_failed(other);
            }
        }
    }

    /// Hands a live notification to the sink: pen data (oriented), proximity, the button.
    /// Anything else is written to the log, so that the real signature of the button can be
    /// found at H2 (`BUTTON_SYSEVENTS` is a hypothesis).
    fn forward(&mut self, notification: LiveNotification) {
        let (width, height) = self.tablet_size();
        let orientation = self.config.options.orientation;
        let input = match notification.channel {
            Channel::Live => match live::decode(&notification.data) {
                Ok(LivePacket::Samples(records)) => Some(LiveInput::Records {
                    records: records
                        .into_iter()
                        .map(|record| orient_record(record, width, height, orientation))
                        .collect(),
                    received: notification.received,
                }),
                Ok(LivePacket::ProximityIn { .. }) => Some(LiveInput::ProximityIn {
                    received: notification.received,
                }),
                Ok(LivePacket::Status { .. } | LivePacket::Unknown(_)) | Err(_) => None,
            },
            Channel::SysEvent if live::is_button_sysevent(&notification.data) => {
                Some(LiveInput::Button)
            }
            Channel::Command | Channel::PageData | Channel::SysEvent => None,
        };
        match input {
            Some(input) => {
                if input == LiveInput::Button {
                    self.note(format!(
                        "button pressed (sysevent {})",
                        ardoise_proto::hex::encode(&notification.data)
                    ));
                }
                if let Some(run) = self.live.as_mut() {
                    run.sink.input(input);
                }
            }
            None => {
                self.note(format!(
                    "unclassified during live mode: {} {}",
                    channel_name(notification.channel),
                    ardoise_proto::hex::encode(&notification.data)
                ));
                self.keep("unclassified notification during live mode");
            }
        }
        let roll = self.live.as_mut().is_some_and(|run| {
            run.notifications = run.notifications.wrapping_add(1);
            run.notifications % ROLL_CHECK == 0
        });
        if roll {
            let root = self.config.identity_root.clone();
            if let Some(journal) = self.journal.as_mut() {
                journal.roll(root.as_deref());
            }
        }
    }

    fn stop_live(&mut self) {
        if self.live.is_none() {
            return;
        }
        // What the tablet sent before the stop is still handed over (or logged).
        while let Some(session) = self.session.as_mut()
            && session.in_live_mode()
            && let Ok(Some(notification)) = session.next_live_event(Duration::ZERO)
        {
            self.forward(notification);
        }
        let Some(mut run) = self.live.take() else {
            return;
        };
        let mut lost = false;
        if let Some(session) = self.session.as_mut()
            && session.in_live_mode()
            && let Err(error) = session.stop_live()
        {
            lost = error == DeviceError::Disconnected;
            self.note(format!("could not leave live mode: {error}"));
        }
        run.sink.input(LiveInput::State(LiveState::Stopped));
        self.note("live mode stopped");
        self.write_journal();
        if lost {
            self.link_lost();
        } else if self.session.is_some() {
            self.counted_at = self.session_time();
            self.publish(Connection::Connected);
        } else {
            let pending = self.pending();
            self.publisher.update(|status| status.pending = pending);
        }
    }

    // --- registration -----------------------------------------------------------------------

    fn register(&mut self) {
        if self.sync.is_some() || self.live.as_ref().is_some_and(|run| run.active) {
            self.show_error(&DeviceError::Busy);
            return;
        }
        if self.config.library.is_none() || self.config.identity_root.is_none() {
            self.show_error(&DeviceError::NoLibrary);
            return;
        }
        if !self.ensure_manager() {
            return;
        }
        // Registering takes the tablet over: a link open to it is closed.
        self.close_session();
        if let Some(journal) = self.journal.as_mut() {
            journal.begin();
            journal.note("registration");
        }
        self.publisher.update(|status| {
            status.connection = Connection::Registering;
            status.register_step = Some(RegisterStep::HoldButton);
            status.error = None;
            status.sync = None;
        });
        self.hooks.registration_started();
        self.publisher
            .update(|status| status.register_step = Some(RegisterStep::Searching));
        let found = match self.manager.as_mut() {
            Some(manager) => manager.find(true, REGISTER_SEARCH),
            None => return,
        };
        let tablet = match found {
            Ok(Some(tablet)) => tablet,
            Ok(None) => {
                return self
                    .registration_failed(&DeviceError::Timeout("tablet in registration mode"));
            }
            Err(error) => return self.registration_failed(&error),
        };
        self.publisher
            .update(|status| status.register_step = Some(RegisterStep::Connecting));
        let publisher = self.publisher.clone();
        let hooks = self.hooks.clone();
        let result = match self.manager.as_mut() {
            Some(manager) => manager
                .register(&tablet, BUTTON_TIMEOUT, &mut || {
                    publisher.update(|status| {
                        status.register_step = Some(RegisterStep::PressButton);
                    });
                    hooks.waiting_for_button();
                })
                .and_then(|(mut session, registration)| {
                    let pages = session.stored_pages()?;
                    Ok((session, registration.info, pages))
                }),
            None => return,
        };
        // `Manager::register` saved the identity as soon as the tablet confirmed.
        self.load_record();
        match result {
            Ok((session, info, pages)) => {
                self.session = Some(session);
                self.note("registered");
                self.publisher
                    .update(|status| status.register_step = Some(RegisterStep::Done));
                self.connected(&info, pages);
            }
            Err(error) => self.registration_failed(&error),
        }
    }

    fn registration_failed(&mut self, error: &DeviceError) {
        let cancelled = *error == DeviceError::Cancelled;
        if cancelled {
            self.note("registration cancelled");
        } else {
            self.keep(error.to_string());
            self.write_journal();
        }
        let connection = if self.record.is_some() {
            Connection::Searching
        } else {
            Connection::NotRegistered
        };
        let error = (!cancelled).then(|| StatusError::from(error));
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = connection;
            status.pending = pending;
            status.register_step = None;
            status.error = error;
        });
    }

    // --- settings of the tablet --------------------------------------------------------------

    /// Fails unless the tablet is connected and idle.
    fn idle_session(&mut self) -> Result<&mut Session, DeviceError> {
        if self.sync.is_some() || self.live.as_ref().is_some_and(|run| run.active) {
            return Err(DeviceError::Busy);
        }
        if self.record.is_none() && self.session.is_none() {
            return Err(DeviceError::NoTabletRegistered);
        }
        self.session.as_mut().ok_or(DeviceError::NotConnected)
    }

    fn rename(&mut self, name: &str) -> Result<(), DeviceError> {
        let session = self.idle_session()?;
        let result = session.set_name(name).and_then(|()| session.name());
        let actual = match result {
            Ok(actual) => actual,
            Err(error) => {
                let shown = StatusError::from(&error);
                if error == DeviceError::Disconnected {
                    self.link_lost();
                }
                self.note(format!("rename failed: {}", shown.message));
                return Err(error);
            }
        };
        self.note(format!("renamed to {actual:?}"));
        if let (Some(record), Some(root)) =
            (self.record.as_mut(), self.config.identity_root.as_ref())
            && record.name != actual
        {
            record.name.clone_from(&actual);
            identity::save(root, record).map_err(|e| DeviceError::Storage(e.to_string()))?;
        }
        let registered = self.record.as_ref().map(registered_tablet);
        self.publisher.update(|status| {
            if let Some(device) = status.device.as_mut() {
                device.name.clone_from(&actual);
            }
            status.registered = registered;
        });
        Ok(())
    }

    fn set_time(&mut self) -> Result<i64, DeviceError> {
        let session = self.idle_session()?;
        let clock = session.clock().clone();
        let result = session
            .set_time()
            .and_then(|()| session.read_time())
            .map(|time| time - unix_seconds(&*clock));
        if result == Err(DeviceError::Disconnected) {
            self.link_lost();
        }
        if let Ok(drift) = &result {
            self.note(format!("clock set, {drift} s off"));
        }
        result
    }

    fn set_options(&mut self, options: TabletOptions) {
        let before = self.config.options;
        self.config.options = options;
        if options.orientation != before.orientation {
            self.reoriented();
        }
        if options.auto_sync
            && !before.auto_sync
            && self.session.is_some()
            && self.stored_pages() > 0
        {
            self.want_sync = true;
        }
        self.publisher.update(|status| status.options = options);
    }

    fn forget(&mut self) -> Result<(), DeviceError> {
        let root = self
            .config
            .identity_root
            .clone()
            .ok_or(DeviceError::NoLibrary)?;
        if let Some(run) = self.sync.take() {
            self.finish_sync(run, None);
        }
        self.stop_live();
        self.close_session();
        self.want_sync = false;
        identity::forget(&root).map_err(|e| DeviceError::Storage(e.to_string()))?;
        self.record = None;
        self.note("registration forgotten");
        self.publisher.update(|status| {
            status.connection = Connection::NotRegistered;
            status.registered = None;
            status.device = None;
            status.register_step = None;
            status.sync = None;
            status.pending = None;
            status.error = None;
        });
        Ok(())
    }

    fn set_library(&mut self, library: Option<PathBuf>, root: Option<PathBuf>) {
        if library == self.config.library && root == self.config.identity_root {
            return;
        }
        if let Some(run) = self.sync.take() {
            // The pages left go to the new library.
            self.finish_sync(run, None);
            self.want_sync = true;
        }
        let reset = root != self.config.identity_root || library.is_none();
        self.config.library = library;
        if !reset {
            return;
        }
        self.stop_live();
        self.close_session();
        self.write_journal();
        self.manager = None;
        self.journal = None;
        self.want_sync = false;
        self.config.identity_root = root;
        self.record = None;
        self.publisher.update(|status| {
            status.registered = None;
            status.device = None;
            status.register_step = None;
            status.sync = None;
            status.pending = None;
            status.error = None;
        });
        self.load_record();
    }

    /// Leaves live mode and closes the link (shutdown).
    fn close(&mut self) {
        if let Some(run) = self.sync.take() {
            self.finish_sync(run, None);
        }
        self.stop_live();
        self.close_session();
        self.write_journal();
    }

    fn close_session(&mut self) {
        self.counted_at = None;
        if let Some(session) = self.session.take()
            && let Err(error) = session.disconnect()
        {
            self.note(format!("disconnect failed: {error}"));
        }
    }

    // --- helpers ----------------------------------------------------------------------------

    /// Reads `device.json` of the identity root.
    fn load_record(&mut self) {
        let Some(root) = self.config.identity_root.clone() else {
            self.record = None;
            return;
        };
        let loaded = identity::load(&root);
        let error = match loaded {
            Ok(record) => {
                self.record = record;
                None
            }
            Err(error) => {
                self.record = None;
                Some(StatusError::new("libraryAccess", error))
            }
        };
        let registered = self.record.as_ref().map(registered_tablet);
        self.publisher.update(|status| {
            status.registered = registered;
            if error.is_some() {
                status.error = error;
            }
        });
    }

    /// Keeps `device.json` in step with the tablet (renamed elsewhere, other size).
    fn remember_info(&mut self, info: &DeviceInfo) {
        let (Some(record), Some(root)) = (self.record.as_mut(), self.config.identity_root.as_ref())
        else {
            return;
        };
        if record.name == info.name && record.width == info.width && record.height == info.height {
            return;
        }
        record.name.clone_from(&info.name);
        record.width = info.width;
        record.height = info.height;
        if let Err(error) = identity::save(root, record) {
            let message = format!("cannot update device.json: {error}");
            self.note(message);
        }
    }

    fn details(&self, info: &DeviceInfo, stored_pages: u16) -> DeviceDetails {
        DeviceDetails {
            name: info.name.clone(),
            firmware: info.firmware.clone(),
            battery_percent: info.battery_percent,
            charging: info.charging,
            width: info.width,
            height: info.height,
            stored_pages,
            model: model_name(info.width, info.height),
            read_at: self.now_ms(),
        }
    }

    /// Drawing area of the tablet: as read at the connection, else as registered.
    fn tablet_size(&self) -> (u32, u32) {
        if let Some(device) = self.publisher.get().device {
            return (device.width, device.height);
        }
        match &self.record {
            Some(record) => (record.width, record.height),
            None => (
                ardoise_proto::SLATE_LARGE_WIDTH,
                ardoise_proto::SLATE_LARGE_HEIGHT,
            ),
        }
    }

    fn page_size(&self) -> PageSize {
        let (width, height) = self.tablet_size();
        PageSize { width, height }
    }

    fn stored_pages(&self) -> u16 {
        self.publisher
            .get()
            .device
            .map_or(0, |device| device.stored_pages)
    }

    /// Monotonic time of the session's clock, `None` without a connection.
    fn session_time(&self) -> Option<Duration> {
        self.session
            .as_ref()
            .map(|session| session.clock().monotonic())
    }

    /// Unix milliseconds on the connector's clock (the system clock without connector).
    fn now_ms(&self) -> i64 {
        let now = match &self.manager {
            Some(manager) => manager.clock().now(),
            None => std::time::SystemTime::now(),
        };
        now.duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
    }

    /// What waits for the tablet.
    fn pending(&self) -> Option<Pending> {
        if self.live.as_ref().is_some_and(|run| !run.active) {
            Some(Pending::Live)
        } else if self.want_sync {
            Some(Pending::Sync)
        } else {
            None
        }
    }

    fn publish(&self, connection: Connection) {
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = connection;
            status.pending = pending;
        });
    }

    fn unavailable(&self, error: StatusError) {
        self.publisher.update(|status| {
            status.connection = Connection::Unavailable;
            status.error = Some(error);
        });
    }

    fn show_error(&self, error: &DeviceError) {
        let error = StatusError::from(error);
        self.publisher.update(|status| status.error = Some(error));
    }

    /// A failure: shown, and its log written.
    fn problem(&mut self, error: &DeviceError) {
        self.keep(error.to_string());
        self.write_journal();
        self.show_error(error);
    }

    fn note(&self, text: impl Into<String>) {
        if let Some(journal) = &self.journal {
            journal.note(text);
        }
    }

    fn keep(&mut self, reason: impl Into<String>) {
        if let Some(journal) = self.journal.as_mut() {
            journal.keep(reason);
        }
    }

    fn write_journal(&mut self) {
        let root = self.config.identity_root.clone();
        if let Some(journal) = self.journal.as_mut() {
            journal.write(root.as_deref());
        }
    }
}

fn saved_count(run: &SyncRun) -> u16 {
    u16::try_from(run.saved.len()).unwrap_or(u16::MAX)
}

fn registered_tablet(record: &DeviceRecord) -> RegisteredTablet {
    RegisteredTablet {
        name: record.name.clone(),
        address: record.address.clone(),
        registered: record.registered.clone(),
        width: record.width,
        height: record.height,
    }
}

/// The channel as the diagnostic log names it.
fn channel_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Command => "command",
        Channel::PageData => "page_data",
        Channel::Live => "live",
        Channel::SysEvent => "sys_event",
    }
}
