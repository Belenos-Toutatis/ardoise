//! The worker thread of the tablet service: a state machine that does one bounded step at a
//! time (a scan slice, a connection, one page of a sync, one live notification, a short idle
//! wait) and handles the requests of the handle between steps.
//!
//! What it owns: the connector (inside the `Manager`), the `Manager` of the current identity
//! root, the session with the connected tablet (a Slate's [`Session`] or the blackboard's
//! [`BoardSession`], never both), the sync under way, the live sink. The blackboard's steps are in
//! [`board`] (docs/design/blackboard.md).

mod board;

use std::panic::AssertUnwindSafe;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, UNIX_EPOCH};

use ardoise_core::library::Library;
use ardoise_core::note::PageSize;
use ardoise_core::settings::{BoardPosition, Orientation};
use ardoise_proto::live::{self, LivePacket};
use ardoise_proto::sysevent::{self, SysEvent};

use super::journal::Journal;
use super::{
    ADAPTER_RECHECK, BLACKBOARD_MODEL_NAME, BUTTON_TIMEOUT, Connection, ConnectorFactory,
    DeviceDetails, Interrupts, KnownTablet, LastSync, LiveInput, LiveSink, LiveState, PageContext,
    PageStore, Pending, REGISTER_SEARCH, RETRY_DELAY, RegisterStep, RegisteredTablet, Request,
    SEARCH_SLICE, Sent, ServiceConfig, ServiceHooks, StatusCallback, StatusError, SyncProgress,
    TabletOptions, TabletStatus, model_name, sync_error,
};
use crate::blackboard::{BoardSession, BoardSync};
use crate::identity::{self, BlackboardRecord, DeviceRecord};
use crate::manager::Manager;
use crate::pages::{ImportedPages, Stored, orient_record};
use crate::rescue::RescueKind;
use crate::session::{
    DeviceError, DeviceInfo, DownloadedPage, LiveNotification, Session, unix_seconds,
};
use crate::transport::{Advertisement, Channel, TabletKind};

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
            // `active` names the tablet of a connection only while there is one.
            if !matches!(
                status.connection,
                Connection::Connecting
                    | Connection::Connected
                    | Connection::Syncing
                    | Connection::Live
            ) {
                status.active = None;
            }
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

/// Why a sync may delete the blackboard's pages: it must, to read the next one (SPEC §4.1).
/// The permission of a sync stays with it when it resumes (lost link, fresh link, other library).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum BoardPermission {
    /// "Delete after import" was on: checked again whenever the sync starts or resumes.
    Option,
    /// The user confirmed the deletion while the blackboard was the connected tablet
    /// (`tablet_sync` with `confirmBlackboardDelete`).
    Confirmed,
}

/// A sync under way.
struct SyncRun {
    /// The tablet it reads.
    kind: TabletKind,
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
    /// What allowed a sync of the blackboard to delete its pages (`None` for the Slate's).
    permission: Option<BoardPermission>,
    /// Where the blackboard's sync stands (its shared per-page step).
    board: BoardSync,
    /// A failure that did not stop the sync (a rescue copy that could not be imported): said at
    /// its end when nothing else is.
    deferred: Option<DeviceError>,
}

/// Where a live run that has not streamed yet stands with the blackboard's stored pages (SPEC
/// §4.2, docs/design/blackboard.md §9.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreLive {
    /// Not looked at yet.
    Unchecked,
    /// A sync of its pages was started for it: once no sync runs or waits, they are counted
    /// again.
    Syncing,
    /// Nothing to fetch (the tablet held no page), or the run already streams: it may start.
    Checked,
}

/// Live mode asked by the hub.
struct LiveRun {
    sink: Box<dyn LiveSink>,
    /// `Started` was sent to this sink.
    started: bool,
    /// The session is in live mode for this sink.
    active: bool,
    notifications: u32,
    /// The tablet live mode streamed from: after a lost link only that one is looked for and
    /// accepted (its page has that tablet's size and way of holding). `None` until a tablet
    /// streams.
    kind: Option<TabletKind>,
    /// The blackboard's stored pages before this run first streams (SPEC §4.2: fetched first,
    /// or the run refused).
    pre_live: PreLive,
    /// How the blackboard is held for this run: the position set when its `Started` was sent,
    /// kept for the whole run, reconnections included (its page has that size and way up;
    /// docs/design/blackboard.md, « Positions »). The Slate's runs follow their orientation
    /// instead.
    board_position: BoardPosition,
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
    /// The Slate (`device.json`).
    record: Option<DeviceRecord>,
    /// The blackboard (`blackboard.json`).
    board_record: Option<BlackboardRecord>,
    /// The link to a Slate.
    session: Option<Session>,
    /// The link to the blackboard (never at the same time as `session`).
    board: Option<BoardSession>,
    /// The search for a blackboard to add, while it runs.
    adding: Option<board::AddSearch>,
    sync: Option<SyncRun>,
    /// A sync waits (asked, automatic, or interrupted by a lost link).
    want_sync: bool,
    /// What allows the sync that waits to delete the blackboard's pages; without it, that sync
    /// does nothing for the blackboard.
    board_permission: Option<BoardPermission>,
    /// When the worker last looked for the Slate while the blackboard held the link.
    slate_checked_at: Option<Duration>,
    live: Option<LiveRun>,
    /// When the page count was last read on this connection (the session clock's monotonic
    /// time), `None` without a connection.
    counted_at: Option<Duration>,
    /// The page count is due now (after a sync that deleted pages), whatever `counted_at`.
    count_now: bool,
    /// The blackboard said a page was stored with its button: its pages are counted again as
    /// soon as nothing else runs.
    stored_event: bool,
    /// `blackboard.json` could not be read (noted once): no blackboard is known until one is
    /// added, which replaces the file.
    board_record_unreadable: bool,
    /// Pages of the blackboard the tablet may no longer hold whose rescue copy could be written
    /// nowhere: written out (or imported) at the next sync of the blackboard.
    held: Vec<(DownloadedPage, RescueKind)>,
    /// Pages set apart as doubtful by a sync cut short by a lost link: said at the end of the
    /// sync that resumes it.
    doubtful_untold: Vec<u32>,
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
            board_record: None,
            session: None,
            board: None,
            adding: None,
            sync: None,
            want_sync: false,
            board_permission: None,
            slate_checked_at: None,
            live: None,
            counted_at: None,
            count_now: false,
            stored_event: false,
            board_record_unreadable: false,
            held: Vec::new(),
            doubtful_untold: Vec::new(),
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
            Request::SyncNow { board_confirmed } => self.sync_now(board_confirmed),
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
            Request::FindBlackboards => self.find_blackboards(),
            Request::AddBlackboard(address, answer) => {
                let result = self.add_blackboard(&address);
                let _ = answer.send(result);
            }
            Request::ForgetBlackboard(answer) => {
                let result = self.forget_blackboard();
                let _ = answer.send(result);
            }
            Request::Shutdown => self.stop = true,
        }
    }

    // --- steps ------------------------------------------------------------------------------

    fn step(&mut self) {
        if self.session.is_some() {
            self.connected_step();
            return;
        }
        if self.board.is_some() {
            self.board_step();
            return;
        }
        if self.adding.is_some() {
            self.add_step();
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
        self.end_live_of_forgotten_tablet();
        // Live mode waiting for its tablet after a lost link: only that one is looked for.
        let live_kind = self.live_kind();
        match (self.record.clone(), self.board_record.clone()) {
            (None, None) => {
                self.publish(Connection::NotRegistered);
                self.wait(Some(REGISTRATION_RECHECK));
            }
            // A library that only knows a Slate: as before the blackboard existed.
            (Some(record), None) => self.search(&record),
            (Some(record), Some(_)) if live_kind == Some(TabletKind::Slate) => {
                self.search(&record);
            }
            (_, Some(board)) if live_kind == Some(TabletKind::Blackboard) => {
                self.search_known(None, &board);
            }
            (slate, Some(board)) => self.search_known(slate.as_ref(), &board),
        }
    }

    /// The tablet a live run streamed from (`None` before one streamed).
    fn live_kind(&self) -> Option<TabletKind> {
        self.live.as_ref().and_then(|run| run.kind)
    }

    /// Whether this library knows the tablet of `kind`.
    fn knows(&self, kind: TabletKind) -> bool {
        match kind {
            TabletKind::Slate => self.record.is_some(),
            TabletKind::Blackboard => self.board_record.is_some(),
        }
    }

    /// A live run whose tablet is no longer known (its file gone or unreadable: forgotten from
    /// another computer through the shared library) while the other tablet is known ends: the
    /// other one would otherwise be searched for, and must not take the run (its page has that
    /// tablet's size and way of holding). As `forget` does here: the sink is told live mode
    /// stopped, nothing else shown. With no tablet known at all the run waits, as before the
    /// blackboard existed (a Slate registered again takes it).
    fn end_live_of_forgotten_tablet(&mut self) {
        let Some(kind) = self.live_kind() else {
            return;
        };
        let other = match kind {
            TabletKind::Slate => TabletKind::Blackboard,
            TabletKind::Blackboard => TabletKind::Slate,
        };
        if self.knows(kind) || !self.knows(other) {
            return;
        }
        self.note(format!(
            "live mode ends: its tablet ({}) is no longer known",
            kind.name()
        ));
        self.stop_live();
    }

    /// A live run belongs to the tablet it started on (docs/design/blackboard.md §2.10): when
    /// the other tablet holds the link (`link`), the run ends before anything is asked of that
    /// tablet, nothing of it reaches the sink, and the status says why (`sheetOtherTablet`). The
    /// sink is told live mode stopped, as after `stop_live`; the link stays. `true` when it did.
    /// Not expected to happen (the run's tablet is the only one looked for while it waits, the
    /// registration and the search for a blackboard wait for it): the last guard.
    fn live_belongs_elsewhere(&mut self, link: TabletKind) -> bool {
        let Some(owner) = self.live_kind().filter(|kind| *kind != link) else {
            return false;
        };
        self.note(format!(
            "live mode ends: it was written with the {}, the {} holds the link",
            owner.name(),
            link.name()
        ));
        self.keep("live mode met the other tablet");
        if let Some(mut run) = self.live.take() {
            run.sink.input(LiveInput::State(LiveState::Stopped));
        }
        self.write_journal();
        self.show_error(&DeviceError::LiveOnOtherTablet { owner });
        self.counted_at = self.session_time();
        self.publish(Connection::Connected);
        true
    }

    /// The tablet holding the link, if any.
    fn link_kind(&self) -> Option<TabletKind> {
        if self.session.is_some() {
            Some(TabletKind::Slate)
        } else if self.board.is_some() {
            Some(TabletKind::Blackboard)
        } else {
            None
        }
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
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Connecting;
            status.active = Some(TabletKind::Slate);
            status.pending = pending;
        });
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
                "connected to {} ({}), {} stored",
                info.name,
                info.firmware,
                pages_text(pages)
            ));
        }
        self.remember_info(info);
        self.counted_at = self.session_time();
        if self.config.options.auto_sync && pages > 0 {
            self.want_sync = true;
        }
        let details = self.details(info, pages);
        let registered = self.record.as_ref().map(registered_tablet);
        let known = self.known_tablets();
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Connected;
            status.active = Some(TabletKind::Slate);
            status.device = Some(details);
            status.registered = registered;
            status.known = known;
            status.pending = pending;
            status.error = None;
        });
        // A level announced while the information was read is newer than the one read.
        self.battery_events();
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
        self.end_journal("the connection attempt failed");
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = Connection::Searching;
            status.pending = pending;
        });
        self.wait(Some(RETRY_DELAY));
    }

    fn connected_step(&mut self) {
        self.battery_events();
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
        if self.count_now && (self.session.is_some() || self.board.is_some()) {
            return true;
        }
        if self.stored_event && self.board.is_some() {
            return true;
        }
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
        self.count_now = false;
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
            self.note(format!(
                "page count: {} on the tablet now, {before} after the last count or sync",
                pages_text(pages)
            ));
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
                self.close_session("closed after a failure");
                self.publish(Connection::Searching);
            }
        }
    }

    /// The tablet dropped the link (switched off, out of range, idle): back to searching.
    fn link_lost(&mut self) {
        self.note("the tablet disconnected");
        self.session = None;
        self.board = None;
        self.counted_at = None;
        self.count_now = false;
        self.stored_event = false;
        if let Some(run) = self.sync.take() {
            // The page in transfer stays on the tablet; the sync goes on at the next connection,
            // with exactly the permission it had to delete the blackboard's pages.
            self.want_sync = true;
            self.board_permission = self.board_permission.max(run.permission);
            self.keep("the link was lost during a sync");
            self.finish_sync(run, Some(&DeviceError::Disconnected));
        }
        if let Some(run) = self.live.as_mut()
            && run.active
        {
            run.active = false;
            run.sink.input(LiveInput::State(LiveState::Reconnecting));
        }
        self.end_journal("the tablet disconnected");
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

    /// A sync asked by the user; `board_confirmed`: with the deletion of the blackboard's pages
    /// confirmed while the blackboard was the connected tablet. Without that, or "delete after
    /// import" on, the sync does nothing for the blackboard.
    fn sync_now(&mut self, board_confirmed: bool) {
        if self.sync.is_some() {
            return;
        }
        if self.record.is_none() && self.board_record.is_none() {
            self.show_error(&DeviceError::NoTabletRegistered);
            return;
        }
        let permission = if board_confirmed {
            Some(BoardPermission::Confirmed)
        } else if self.config.options.delete_after_import {
            Some(BoardPermission::Option)
        } else {
            None
        };
        if self.board.is_some() && permission.is_none() {
            // The blackboard connected after the request left (the handle checks the status
            // first): the interface must ask before its pages are deleted.
            self.note("a sync was asked without confirming the deletion of the blackboard's pages");
            self.show_error(&DeviceError::BlackboardConfirmDelete);
            return;
        }
        self.want_sync = true;
        self.board_permission = self.board_permission.max(permission);
        let pending = self.pending();
        self.publisher.update(|status| status.pending = pending);
    }

    fn start_sync(&mut self) {
        self.want_sync = false;
        self.board_permission = None;
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
        // `ec` when this link needs it (required to download), then paper mode and the count.
        let counted = match self.session.as_mut() {
            Some(session) => session
                .ensure_file_transfer_reporting()
                .and_then(|()| session.stored_pages()),
            None => return,
        };
        let total = match counted {
            Ok(total) => total,
            Err(error) => return self.sync_not_started(error, None),
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
            board_position: self.config.options.board_position,
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
            kind: TabletKind::Slate,
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
            permission: None,
            board: BoardSync::default(),
            deferred: None,
        };
        if to_read == 0 {
            self.finish_sync(run, None);
        } else {
            self.sync = Some(run);
        }
    }

    /// The sync could not even count the pages (the tablet refused `ec` or paper mode...): a
    /// failed sync, shown as such (`last_sync` with its error) and logged; the pages stay on the
    /// tablet. The link is closed (its state is unknown) and the next connection tries again.
    /// `permission`: what allowed it to delete the blackboard's pages, kept for that next try.
    fn sync_not_started(&mut self, error: DeviceError, permission: Option<BoardPermission>) {
        match error {
            // The blackboard's sync waits again: the request that cut it short decides (cancel
            // drops it, forgetting the Slate does not).
            DeviceError::Cancelled if permission.is_some() => {
                self.want_sync = true;
                self.board_permission = self.board_permission.max(permission);
            }
            DeviceError::Cancelled => {}
            DeviceError::Disconnected => {
                // Tried again at the next connection.
                self.want_sync = true;
                self.board_permission = self.board_permission.max(permission);
                self.link_lost();
            }
            other => {
                self.note(format!(
                    "sync failed before the first page: {other}; the pages stay on the tablet"
                ));
                self.keep(format!("sync failed: {other}"));
                let at = self.now_ms();
                let shown = sync_error(&other);
                self.publisher.update(|status| {
                    status.sync = None;
                    status.last_sync = Some(LastSync {
                        at,
                        saved: Vec::new(),
                        deleted: 0,
                        error: Some(shown.clone()),
                    });
                    status.error = Some(shown);
                });
                self.close_session("closed after a failed sync");
                self.publish(Connection::Searching);
            }
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
        let message = match keep_page(&mut self.store_page, run, page, index) {
            Ok(message) => message,
            // The page stays on the tablet.
            Err(error) => return self.sync_failed(error),
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
            // The blackboard's sync, cut short by a request (its stream left dirty, reopened
            // before anything else): the request decides. Cancelling ends the sync; forgetting
            // the Slate, for one, does not.
            DeviceError::Cancelled
                if run.kind == TabletKind::Blackboard && self.board.is_some() =>
            {
                self.note("the sync step was interrupted by a request");
                self.sync = Some(run);
            }
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

    /// Ends a sync: `last_sync`, the page count, back to connected. After a sync that deleted
    /// pages the count is read again at once: a page finished meanwhile (the tablet stays
    /// connected) is synced on the same connection.
    fn finish_sync(&mut self, run: SyncRun, error: Option<&DeviceError>) {
        // Pages set apart as doubtful are said (docs/design/blackboard.md §9.2): when nothing
        // else stopped the sync, or instead of an error that would say the page stayed on the
        // tablet; after a lost link, by the sync that resumes.
        let mut doubtful = std::mem::take(&mut self.doubtful_untold);
        for time in run.board.doubtful() {
            if !doubtful.contains(time) {
                doubtful.push(*time);
            }
        }
        // Only what is still kept apart (a page that came cleanly since lost its copy).
        if !doubtful.is_empty() {
            let kept: Vec<u32> = self
                .rescue_store()
                .pages(RescueKind::Doubtful)
                .iter()
                .map(|file| file.time)
                .chain(
                    self.held
                        .iter()
                        .filter(|(_, kind)| *kind == RescueKind::Doubtful)
                        .filter_map(|(page, _)| page.timestamp.and_then(|t| u32::try_from(t).ok())),
                )
                .collect();
            doubtful.retain(|time| kept.contains(time));
        }
        // A failure that did not stop the sync is said when nothing else is.
        let deferred = run.deferred.clone();
        let error = match (error, deferred.as_ref()) {
            (None, deferred) => deferred,
            (error, Some(deferred)) => {
                self.note(format!("also during this sync: {deferred}"));
                error
            }
            (error, None) => error,
        };
        if !doubtful.is_empty() && !matches!(error, Some(DeviceError::Disconnected)) {
            self.note(format!(
                "{} kept apart as doubtful (not made into notes): {}",
                pages_text(u16::try_from(doubtful.len()).unwrap_or(u16::MAX)),
                doubtful
                    .iter()
                    .map(|time| ardoise_proto::blackboard::utc(*time))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        // The doubtful page is what the user is told only when nothing else stopped the sync, or
        // when what stopped it was that very page (its error would say it stayed on the tablet).
        let told = match error {
            None => doubtful.first().copied(),
            Some(DeviceError::PageUnreadable { time, .. }) => {
                doubtful.iter().copied().find(|doubtful| doubtful == time)
            }
            Some(_) => None,
        }
        .map(|time| DeviceError::PageDoubtful { time });
        let error = match error {
            Some(DeviceError::Disconnected) => {
                self.doubtful_untold = doubtful;
                error
            }
            _ => told.as_ref().or(error),
        };
        match error {
            None => self.note(format!(
                "sync finished: {} of {} pages saved, {} deleted",
                run.saved.len(),
                run.to_read,
                run.deleted
            )),
            Some(error) => self.note(format!(
                "sync failed after {} of {} pages saved, {} deleted: {error}",
                run.saved.len(),
                run.to_read,
                run.deleted
            )),
        }
        let at = self.now_ms();
        let remaining = run.total.saturating_sub(run.deleted);
        // The sync just counted the pages; after deletions, the count is due again now.
        self.counted_at = self.session_time();
        self.count_now = error.is_none() && run.deleted > 0;
        let error = error.map(sync_error);
        // Whichever tablet still holds the link: the blackboard too.
        let active = self.link_kind();
        let connection = if active.is_some() {
            Connection::Connected
        } else {
            Connection::Searching
        };
        let pending = self.pending();
        self.publisher.update(|status| {
            status.connection = connection;
            status.active = active;
            status.pending = pending;
            status.sync = None;
            status.last_sync = Some(LastSync {
                at,
                saved: run.saved,
                deleted: run.deleted,
                error: error.clone(),
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
        if self.adding.is_some() {
            self.note("search for a blackboard cancelled");
            self.end_search(None);
        }
        if self.want_sync {
            self.want_sync = false;
            self.board_permission = None;
            let pending = self.pending();
            self.publisher.update(|status| status.pending = pending);
        }
    }

    // --- live mode --------------------------------------------------------------------------

    fn start_live(&mut self, mut sink: Box<dyn LiveSink>) {
        let mut active = false;
        let mut kind = None;
        if let Some(mut old) = self.live.take() {
            old.sink.input(LiveInput::State(LiveState::Stopped));
            active = old.active;
            // Still the same stream: the same tablet. Otherwise the new sink is told the size
            // of whichever tablet streams first (its own `Started`).
            kind = old.kind.filter(|_| active);
        }
        let refused = if self.config.library.is_none() || self.config.identity_root.is_none() {
            Some(DeviceError::NoLibrary)
        } else if self.record.is_none() && self.board_record.is_none() {
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
            kind,
            pre_live: if active {
                PreLive::Checked
            } else {
                PreLive::Unchecked
            },
            board_position: self.config.options.board_position,
        };
        if active {
            // Live mode already streams (another sink before): the new one starts at once.
            if !self.started(&mut run) {
                self.live = Some(run);
                return self.live_refused();
            }
        } else if (self.session.is_none() && self.board.is_none()) || self.sync.is_some() {
            run.sink.input(LiveInput::State(LiveState::Waiting));
        }
        self.live = Some(run);
        let pending = self.pending();
        self.publisher.update(|status| status.pending = pending);
    }

    /// Tells a sink that live mode streams. `false` when the sink refuses it after `Started`
    /// ([`LiveSink::active`]): live mode is then left ([`Self::live_refused`]).
    fn started(&self, run: &mut LiveRun) -> bool {
        if !run.started {
            let (width, height) = self.tablet_size();
            // The blackboard's position holds for the whole run from here.
            run.board_position = self.config.options.board_position;
            run.sink.input(LiveInput::Started {
                width,
                height,
                orientation: self.holding(run.board_position),
                kind: self.link_kind().unwrap_or(TabletKind::Slate),
            });
            run.started = true;
            if !run.sink.active() {
                return false;
            }
        }
        run.sink.input(LiveInput::State(LiveState::Live));
        true
    }

    /// The sink refused the tablet that streams (a sheet it cannot write on): live mode is left,
    /// the link stays.
    fn live_refused(&mut self) {
        self.note("live mode refused by its sink: this tablet cannot write on the sheet resumed");
        self.stop_live();
    }

    /// Tells the live sink that the tablet is held another way, whether live mode streams or
    /// still waits for the tablet (not connected, reconnecting): the session takes the new way at
    /// once, so the interface shows the switch, and the `Started` that follows finds nothing to
    /// turn again. Done on this thread between two notifications: the records before are
    /// oriented the old way, the ones after the new way.
    fn reoriented(&mut self) {
        if self.board.is_some() || self.live_kind() == Some(TabletKind::Blackboard) {
            // The Slate's orientation is not the blackboard's (its own is `board_position`, set
            // for a live run when it starts): nothing turns (also while its live mode waits for
            // it).
            self.note("orientation changed: not used by the blackboard");
            return;
        }
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
        if self.live_belongs_elsewhere(TabletKind::Slate) {
            return;
        }
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
            let accepted = self.started(&mut run);
            run.active = true;
            run.kind = Some(TabletKind::Slate);
            self.live = Some(run);
            if !accepted {
                return self.live_refused();
            }
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
    /// What is known and means nothing for live mode (the battery level, which
    /// [`Self::battery_events`] shows; the frames of a real Slate whose meaning is unknown) is
    /// only in the log's `rx` lines. Anything else is noted and keeps the log.
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
                Ok(LivePacket::Unexplained(_)) => return,
                Ok(LivePacket::Status { .. } | LivePacket::Unknown(_)) | Err(_) => None,
            },
            Channel::SysEvent => match sysevent::decode(&notification.data) {
                SysEvent::BatteryLevel { .. } | SysEvent::Unexplained { .. } => return,
                SysEvent::Unknown(_) => None,
            },
            Channel::Command if live::is_button_command(&notification.data) => {
                Some(LiveInput::Button)
            }
            Channel::Command | Channel::PageData | Channel::BoardData => None,
        };
        match input {
            Some(input) => {
                if input == LiveInput::Button {
                    self.note(format!(
                        "button pressed ({})",
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
        self.count_live_notification();
    }

    /// Counts a live notification; every [`ROLL_CHECK`] the log's length is looked at.
    fn count_live_notification(&mut self) {
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
        while let Some(board) = self.board.as_mut()
            && board.in_live_mode()
            && let Ok(Some(notification)) = board.next_live_event(Duration::ZERO)
        {
            self.board_forward(notification);
        }
        let Some(mut run) = self.live.take() else {
            return;
        };
        let mut lost = false;
        let stopped = match (self.session.as_mut(), self.board.as_mut()) {
            (Some(session), _) if session.in_live_mode() => session.stop_live(),
            (_, Some(board)) if board.in_live_mode() => board.stop_live(),
            _ => Ok(()),
        };
        if let Err(error) = stopped {
            lost = error == DeviceError::Disconnected;
            self.note(format!("could not leave live mode: {error}"));
        }
        run.sink.input(LiveInput::State(LiveState::Stopped));
        self.note("live mode stopped");
        self.write_journal();
        if lost {
            self.link_lost();
        } else if self.session.is_some() || self.board.is_some() {
            self.counted_at = self.session_time();
            self.publish(Connection::Connected);
        } else {
            let pending = self.pending();
            self.publisher.update(|status| status.pending = pending);
        }
    }

    // --- registration -----------------------------------------------------------------------

    fn register(&mut self) {
        // Live mode under way, or waiting for the blackboard it was written with: the Slate
        // registered would take the link, and the run must not change tablets. A run waiting for
        // a Slate (or for any tablet) lets the wizard run, as before the blackboard existed.
        let busy_live = self.live.as_ref().is_some_and(|run| {
            run.active || run.kind.is_some_and(|kind| kind != TabletKind::Slate)
        });
        if self.sync.is_some() || busy_live {
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
        // Registering takes the tablet over: a link open to it is closed (to the blackboard too:
        // one link at a time), and a search for a blackboard ends.
        if self.adding.is_some() {
            self.end_search(None);
        }
        self.close_session("registering takes the tablet over");
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
            self.end_journal("the registration failed");
        }
        let connection = self.waiting_connection();
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
        if self.board.is_some() {
            return Err(DeviceError::NotSupported(
                "the blackboard has no name to change",
            ));
        }
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
        if self.board.is_some() {
            return self.board_set_time();
        }
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
        if options.board_position != before.board_position {
            // A blackboard run keeps the position it started with (its page has that size and
            // way up); the next run, and the next sync, take the new one.
            let held = self
                .live
                .as_ref()
                .filter(|run| run.started && run.kind == Some(TabletKind::Blackboard))
                .map(|run| run.board_position);
            match held {
                Some(held) => self.note(format!(
                    "blackboard position changed to {}: this live run keeps {}",
                    options.board_position.name(),
                    held.name()
                )),
                None => self.note(format!(
                    "blackboard position changed to {}",
                    options.board_position.name()
                )),
            }
        }
        // The blackboard's pages are imported automatically only when they may be deleted.
        let may_sync =
            self.session.is_some() || (self.board.is_some() && options.delete_after_import);
        if options.auto_sync && !before.auto_sync && may_sync && self.stored_pages() > 0 {
            self.want_sync = true;
            if self.board.is_some() {
                self.board_permission = self.board_permission.max(Some(BoardPermission::Option));
            }
        }
        self.publisher.update(|status| status.options = options);
    }

    fn forget(&mut self) -> Result<(), DeviceError> {
        let root = self
            .config
            .identity_root
            .clone()
            .ok_or(DeviceError::NoLibrary)?;
        // As before the blackboard existed (live mode waiting stopped, nothing left pending),
        // unless the blackboard holds the link: its sync, live mode and what waits for it stay.
        if self.board.is_none() {
            if let Some(run) = self.sync.take() {
                self.finish_sync(run, None);
            }
            self.stop_live();
            self.close_session("the registration is forgotten");
            self.want_sync = false;
            self.board_permission = None;
        }
        identity::forget(&root).map_err(|e| DeviceError::Storage(e.to_string()))?;
        self.record = None;
        self.note("registration forgotten");
        let connection = self.waiting_connection();
        let board_connected = self.board.is_some();
        let known = self.known_tablets();
        let pending = self.pending();
        self.publisher.update(|status| {
            if !board_connected {
                status.connection = connection;
                status.sync = None;
                status.error = None;
            }
            if status
                .device
                .as_ref()
                .is_some_and(|d| d.kind == TabletKind::Slate)
            {
                status.device = None;
            }
            status.registered = None;
            status.known = known;
            status.register_step = None;
            status.pending = pending;
        });
        Ok(())
    }

    fn set_library(&mut self, library: Option<PathBuf>, root: Option<PathBuf>) {
        if library == self.config.library && root == self.config.identity_root {
            return;
        }
        if let Some(run) = self.sync.take() {
            // The pages left go to the new library, with the same permission to delete the
            // blackboard's.
            let permission = run.permission;
            self.finish_sync(run, None);
            self.want_sync = true;
            self.board_permission = self.board_permission.max(permission);
        }
        let reset = root != self.config.identity_root || library.is_none();
        self.config.library = library;
        if !reset {
            return;
        }
        self.stop_live();
        self.close_session("another library is chosen");
        self.write_journal();
        self.manager = None;
        self.journal = None;
        self.want_sync = false;
        self.board_permission = None;
        self.board_record_unreadable = false;
        self.adding = None;
        self.config.identity_root = root;
        self.record = None;
        self.board_record = None;
        self.publisher.update(|status| {
            status.registered = None;
            status.known = Vec::new();
            status.device = None;
            status.register_step = None;
            status.board_search = None;
            status.sync = None;
            status.pending = None;
            status.error = None;
        });
        self.load_record();
    }

    /// Leaves live mode and closes the link (shutdown: the application closes, or the guided
    /// test takes the tablet).
    fn close(&mut self) {
        if let Some(run) = self.sync.take() {
            self.finish_sync(run, None);
        }
        self.stop_live();
        self.close_session("the tablet service stopped");
        self.write_journal();
    }

    /// Closes the link to the tablet, if any: the log gets the disconnection and its end line
    /// (`why`), and is written again when it must be.
    fn close_session(&mut self, why: &str) {
        self.counted_at = None;
        self.count_now = false;
        self.stored_event = false;
        self.slate_checked_at = None;
        let closed = match (self.session.take(), self.board.take()) {
            (Some(session), _) => session.disconnect(),
            (None, Some(board)) => board.disconnect(),
            (None, None) => return,
        };
        if let Err(error) = closed {
            self.note(format!("disconnect failed: {error}"));
        }
        self.end_journal(why);
    }

    /// Ends the log of this connection with `why` and writes it when it must be.
    fn end_journal(&mut self, why: &str) {
        if let Some(journal) = self.journal.as_mut() {
            journal.end(why);
        }
        self.write_journal();
    }

    /// Shows the battery level the tablet announced on its own (sysevent `f1`), if it did.
    fn battery_events(&mut self) {
        let Some(percent) = self.session.as_mut().and_then(Session::take_battery_event) else {
            return;
        };
        self.publisher.update(|status| {
            if let Some(device) = status.device.as_mut() {
                device.battery_percent = percent;
            }
        });
    }

    // --- helpers ----------------------------------------------------------------------------

    /// Reads `device.json` and `blackboard.json` of the identity root.
    fn load_record(&mut self) {
        let Some(root) = self.config.identity_root.clone() else {
            self.record = None;
            self.board_record = None;
            return;
        };
        let mut error = None;
        match identity::load(&root) {
            Ok(record) => self.record = record,
            Err(failure) => {
                self.record = None;
                error = Some(StatusError::new("libraryAccess", failure));
            }
        }
        match identity::load_blackboard(&root) {
            Ok(record) => {
                self.board_record = record;
                self.board_record_unreadable = false;
            }
            // No blackboard known (a Slate goes on working), nothing shown: noted once in the
            // diagnostic log, the file left as it is until a blackboard is added again (which
            // replaces it).
            Err(failure) => {
                self.board_record = None;
                if !self.board_record_unreadable && self.journal.is_some() {
                    self.board_record_unreadable = true;
                    self.note(format!(
                        "blackboard.json cannot be read ({failure}): no blackboard is known until \
                         one is added"
                    ));
                    self.keep("blackboard.json cannot be read");
                    self.write_journal();
                }
            }
        }
        let registered = self.record.as_ref().map(registered_tablet);
        let known = self.known_tablets();
        self.publisher.update(|status| {
            status.registered = registered;
            status.known = known;
            if error.is_some() {
                status.error = error;
            }
        });
    }

    /// The tablets the library knows, the Slate first.
    fn known_tablets(&self) -> Vec<KnownTablet> {
        let slate = self.record.as_ref().map(|record| KnownTablet {
            kind: TabletKind::Slate,
            name: record.name.clone(),
            address: record.address.clone(),
            added: record.registered.clone(),
            width: record.width,
            height: record.height,
            model: model_name(record.width, record.height),
        });
        let board = self.board_record.as_ref().map(|record| KnownTablet {
            kind: TabletKind::Blackboard,
            name: record.name.clone(),
            address: record.address.clone(),
            added: record.added.clone(),
            width: ardoise_proto::blackboard::BOARD_WIDTH,
            height: ardoise_proto::blackboard::BOARD_HEIGHT,
            model: BLACKBOARD_MODEL_NAME.to_owned(),
        });
        slate.into_iter().chain(board).collect()
    }

    /// The state while no tablet is connected: searching when one is known.
    fn waiting_connection(&self) -> Connection {
        if self.record.is_some() || self.board_record.is_some() {
            Connection::Searching
        } else {
            Connection::NotRegistered
        }
    }

    /// How the tablet of the link is held, as the live hub counts it (the clockwise quarter
    /// turns from the tablet's own frame to the page, named as the Slate's positions): as the
    /// settings say for a Slate; for the blackboard, the quarter turns of `board` (the run's
    /// position), whose records [`board_forward`](Self::board_forward) turns that way.
    fn holding(&self, board: BoardPosition) -> Orientation {
        if self.board.is_some() {
            Orientation::from_quarter_turns(board.quarter_turns())
        } else {
            self.config.options.orientation
        }
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
            kind: TabletKind::Slate,
            name: info.name.clone(),
            firmware: info.firmware.clone(),
            battery_percent: info.battery_percent,
            charging: info.charging,
            width: info.width,
            height: info.height,
            stored_pages,
            model: model_name(info.width, info.height),
            read_at: self.now_ms(),
            memory: None,
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
        match (&self.session, &self.board) {
            (Some(session), _) => Some(session.clock().monotonic()),
            (None, Some(board)) => Some(board.clock().monotonic()),
            (None, None) => None,
        }
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
        // The tablet holding the link, whichever it is (`active` is cleared by the publisher
        // in the states without a connection).
        let active = self.link_kind();
        self.publisher.update(|status| {
            status.connection = connection;
            status.pending = pending;
            if active.is_some() {
                status.active = active;
            }
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

/// Stores `page` (the `index`-th of the sync) unless the library already has it: the line for
/// the log, or why it could not be stored (the page then stays on the tablet).
fn keep_page(
    store_page: &mut PageStore,
    run: &mut SyncRun,
    page: DownloadedPage,
    index: u16,
) -> Result<String, DeviceError> {
    keep_page_as(store_page, run, page, &format!("page {index}"))
}

/// [`keep_page`], the page named `name` in the log line.
fn keep_page_as(
    store_page: &mut PageStore,
    run: &mut SyncRun,
    page: DownloadedPage,
    name: &str,
) -> Result<String, DeviceError> {
    let known = match run.imported.find(&run.library, &page) {
        Some(path) => Some(Known::InLibrary(path)),
        None if !run.delete && run.imported.in_trash(&page) => Some(Known::InTrash),
        None => None,
    };
    match known {
        Some(Known::InLibrary(path)) => {
            Ok(format!("{name} already imported as {}", path.display()))
        }
        Some(Known::InTrash) => Ok(format!("{name} already imported, its note deleted")),
        None => {
            let recorded = page.clone();
            // The store is the app's code (thumbnail, export): a panic there must not end the
            // service; the page then stays on the tablet.
            let stored = std::panic::catch_unwind(AssertUnwindSafe(|| {
                store_page(&run.root, page, &run.context)
            }))
            .unwrap_or_else(|_| Err("the page store failed unexpectedly".into()));
            let stored = stored.map_err(DeviceError::Storage)?;
            run.imported.record(&recorded, &stored.path, stored.created);
            let shown = stored.path.display().to_string();
            if !run.saved.contains(&shown) {
                run.saved.push(shown);
            }
            Ok(match stored.how {
                Stored::New => format!("{name} saved as {}", stored.path.display()),
                Stored::NewNotUpdated(why) => format!(
                    "{name} saved as {} (a later store of a screen; the note of the earlier \
                     store was not updated: {why})",
                    stored.path.display()
                ),
                Stored::Updated { replaced } => format!(
                    "{name}, a later store of its screen, updated the note of the store of {}: {}",
                    ardoise_proto::blackboard::utc(replaced),
                    stored.path.display()
                ),
            })
        }
    }
}

/// `1 page`, `3 pages`.
fn pages_text(count: u16) -> String {
    if count == 1 {
        "1 page".into()
    } else {
        format!("{count} pages")
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
        Channel::BoardData => "board_data",
    }
}
