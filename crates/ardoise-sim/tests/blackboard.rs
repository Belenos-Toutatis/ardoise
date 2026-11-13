//! The Xiaomi blackboard (milestone M18, docs/design/blackboard.md) against its simulated
//! tablet: adding it without registration, its sync (a page deleted only once its note is
//! written and read back, imports on request only when deletion is off), live mode (the
//! strokes written, the laser pointer, the "page stored" event, a lost link recovered), and the
//! two tablets known at once.
//!
//! Virtual time everywhere. `cargo test -p ardoise-sim --test blackboard -- --nocapture`
//! prints what each test saw.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::time::{Duration, Instant};

use ardoise_core::library::Library;
use ardoise_core::note::Timing;
use ardoise_core::settings::Orientation;
use ardoise_device::identity::{self, DEVICE_FORMAT, DeviceRecord, HostId};
use ardoise_device::pages::{BLACKBOARD_PAGE, LibrarySink};
use ardoise_device::service::{
    Connection, LastSync, LiveInput, LiveSink, LiveState, PageContext, PageStore, ServiceConfig,
    ServiceHooks, TabletOptions, TabletService, TabletStatus,
};
use ardoise_device::session::{DeviceError, DownloadedPage};
use ardoise_device::transport::TabletKind;
use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::Point;
use ardoise_sim::{
    BoardStoredPage, PenScript, SimBoard, SimBoardConfig, SimConfig, SimDevice, SimFactory,
    SimTablet, reference_board_pages,
};

const WAIT: Duration = Duration::from_secs(30);
const ID: [u8; 6] = [0x89, 0xea, 0x78, 0x88, 0xff, 0xcb];
const NOW_MS: i64 = 1_792_843_200_000;

fn new_library() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    ardoise_core::library::init(dir.path()).unwrap();
    dir
}

/// A blackboard holding the three reference pages.
fn board_with_pages(clock: Option<Arc<ardoise_sim::VirtualClock>>) -> SimBoard {
    let config = SimBoardConfig::blackboard();
    let board = match clock {
        Some(clock) => SimBoard::with_clock(config, clock),
        None => SimBoard::new(config),
    };
    for page in reference_board_pages() {
        board.draw_page(&page);
    }
    board
}

/// The blackboard was added from this library before.
fn added_beforehand(board: &SimBoard, root: &Path) {
    let record = identity::BlackboardRecord {
        format: identity::BLACKBOARD_FORMAT,
        address: board.config().address,
        name: "Mijia LCD blackboard".into(),
        protocol: identity::BLACKBOARD_PROTOCOL.into(),
        model: "jiqid.sxb.v1".into(),
        added: "2026-11-12T20:00:00+01:00".into(),
        extra: Default::default(),
    };
    identity::save_blackboard(root, &record).unwrap();
}

/// The app's page store without the app (`LibrarySink`: written, then read back), the n-th page
/// (1-based) refused when `refuse` says so; every page offered is recorded with whether the
/// blackboard still held it at that moment.
fn library_store(
    board: SimBoard,
    refuse: Option<u16>,
    offered: Arc<Mutex<Vec<(u32, bool)>>>,
) -> PageStore {
    let count = AtomicU16::new(0);
    Box::new(
        move |root: &Path, page: DownloadedPage, context: &PageContext| {
            let n = count.fetch_add(1, Ordering::SeqCst) + 1;
            let time = u32::try_from(page.timestamp.unwrap_or(0)).unwrap_or(0);
            let still_there = board.stored_pages().iter().any(|p| p.time == time);
            offered.lock().unwrap().push((time, still_there));
            if refuse == Some(n) {
                return Err("the disk is full".into());
            }
            let library = Library::open(root).map_err(|e| e.to_string())?;
            let mut sink = LibrarySink::new(&library, "", context.size, "Calque 1", || NOW_MS)
                .with_orientation(context.orientation);
            sink.store(page).map(|saved| saved.path)
        },
    )
}

/// Writes `script` on the blackboard each time its live mode starts.
struct Writer {
    board: SimBoard,
    script: PenScript,
}

impl ServiceHooks for Writer {
    fn blackboard_live_started(&self) {
        self.board.draw_live(self.script.clone());
    }
}

struct Running {
    service: Arc<TabletService>,
    statuses: Receiver<TabletStatus>,
    seen: Vec<TabletStatus>,
}

impl Running {
    fn start(
        devices: Vec<SimDevice>,
        root: &Path,
        options: TabletOptions,
        hooks: Arc<dyn ServiceHooks>,
        store: PageStore,
    ) -> Running {
        let (sender, statuses) = mpsc::channel();
        let sender = Mutex::new(sender);
        let config = ServiceConfig {
            library: Some(root.to_owned()),
            identity_root: Some(root.to_owned()),
            options,
            simulated: true,
        };
        let service = TabletService::start(
            config,
            Box::new(SimFactory::of(devices, false)),
            hooks,
            Box::new(move |status| {
                let _ = sender.lock().unwrap().send(status.clone());
            }),
            store,
        );
        Running {
            service: Arc::new(service),
            statuses,
            seen: Vec::new(),
        }
    }

    fn wait(&mut self, what: impl Fn(&TabletStatus) -> bool) -> TabletStatus {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.statuses.recv_timeout(left) {
                Ok(status) => {
                    self.seen.push(status.clone());
                    if what(&status) {
                        return status;
                    }
                }
                Err(_) => panic!(
                    "status not reached; last: {:#?}",
                    self.seen.last().or(Some(&self.service.status()))
                ),
            }
        }
    }
}

struct ChannelSink(Sender<LiveInput>);

impl LiveSink for ChannelSink {
    fn input(&mut self, input: LiveInput) {
        let _ = self.0.send(input);
    }
}

fn notes(root: &Path) -> Vec<PathBuf> {
    let listing = Library::open(root).unwrap().list();
    assert!(listing.problems.is_empty(), "{:?}", listing.problems);
    listing.notes.into_iter().map(|n| n.path).collect()
}

fn options(auto_sync: bool, delete_after_import: bool) -> TabletOptions {
    TabletOptions {
        auto_sync,
        delete_after_import,
        ..TabletOptions::default()
    }
}

#[test]
fn adding_the_blackboard_then_its_pages_become_dated_notes() {
    let _report = common::report("blackboard: add it, then the automatic sync");
    let library = new_library();
    let board = board_with_pages(None);
    let times: Vec<u32> = board.stored_pages().iter().map(|p| p.time).collect();
    let offered = Arc::new(Mutex::new(Vec::new()));
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        library_store(board.clone(), None, offered.clone()),
    );
    run.wait(|s| s.connection == Connection::NotRegistered);
    // Nothing known: the blackboard is not connected to by itself.
    assert_eq!(board.stats().connections, 0);
    assert_eq!(
        run.service.add_blackboard("00:00:5E:00:53:B1"),
        Err(DeviceError::TabletNotFound(
            "00:00:5E:00:53:B1 was not heard by the search".into()
        ))
    );

    run.service.find_blackboards();
    let found = run.wait(|s| s.board_search.as_ref().is_some_and(|b| !b.found.is_empty()));
    assert_eq!(found.connection, Connection::Adding);
    let heard = &found.board_search.as_ref().unwrap().found[0];
    assert_eq!(heard.address, "00:00:5E:00:53:B1");
    assert_eq!(heard.name.as_deref(), Some("xiaomi.dev.ble"));
    run.service.add_blackboard("00:00:5e:00:53:b1").unwrap();
    let status = run.wait(|s| s.last_sync.is_some());
    say!("after the sync: {:?}", status.last_sync);

    // Saved next to device.json, which stays absent (an older Ardoise sees no tablet).
    let saved = identity::load_blackboard(library.path()).unwrap().unwrap();
    assert_eq!(saved.address, "00:00:5E:00:53:B1");
    assert_eq!(saved.protocol, "xiaomi-blackboard");
    assert!(identity::load(library.path()).unwrap().is_none());
    assert_eq!(status.known.len(), 1);
    assert_eq!(status.known[0].kind, TabletKind::Blackboard);
    assert_eq!(status.registered, None);

    // Connected, information read, three pages imported and deleted, each after its note.
    assert!(
        run.seen.iter().any(
            |s| s.connection == Connection::Syncing && s.active == Some(TabletKind::Blackboard)
        )
    );
    let device = status.device.clone().unwrap();
    assert_eq!(device.kind, TabletKind::Blackboard);
    assert_eq!(device.battery_percent, 90);
    assert_eq!((device.width, device.height), (20_500, 27_200));
    assert_eq!(device.stored_pages, 0);
    assert!(device.memory.is_some());
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted), (3, 3));
    assert_eq!(last.error, None);
    assert_eq!(board.stored_page_count(), 0);
    assert_eq!(
        *offered.lock().unwrap(),
        times.iter().map(|t| (*t, true)).collect::<Vec<_>>()
    );

    // The notes: dated by the page's time, portrait, synthesised times.
    let opened = Library::open(library.path()).unwrap();
    let mut created: Vec<i64> = notes(library.path())
        .iter()
        .map(|path| {
            let note = opened.read(path).unwrap().file.note;
            assert_eq!(note.page, BLACKBOARD_PAGE);
            assert_eq!(note.timing, Timing::Synthesized);
            assert!(!note.strokes.is_empty());
            note.created
        })
        .collect();
    created.sort_unstable();
    assert_eq!(
        created,
        times
            .iter()
            .map(|t| i64::from(*t) * 1000)
            .collect::<Vec<_>>()
    );
    // The tablet's clock was set at the connection; nothing else was ever written to it.
    let stats = board.stats();
    say!("blackboard: {stats:?}");
    assert_eq!(stats.unknown_requests, 0);
    assert_eq!((stats.downloads, stats.deletes), (3, 3));
    run.service.shutdown();
}

#[test]
fn a_page_is_deleted_only_after_its_note_is_written_and_checked() {
    let _report = common::report("blackboard: a page that cannot be saved stays on the tablet");
    let library = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    let offered = Arc::new(Mutex::new(Vec::new()));
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        library_store(board.clone(), Some(2), offered.clone()),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    say!("{last:?}");
    assert_eq!((last.saved.len(), last.deleted), (1, 1));
    assert_eq!(last.error.as_ref().unwrap().code, "libraryAccess");
    // The second page was offered while still on the tablet, refused, and kept there.
    assert_eq!(board.stored_page_count(), 2);
    assert!(offered.lock().unwrap().iter().all(|(_, there)| *there));
    assert_eq!(notes(library.path()).len(), 1);

    // On request, the rest comes, without the first one twice.
    run.service.sync_now(false).unwrap();
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.error.is_none()));
    assert_eq!(status.last_sync.unwrap().deleted, 2);
    assert_eq!(board.stored_page_count(), 0);
    assert_eq!(notes(library.path()).len(), 3);
    run.service.shutdown();
}

#[test]
fn without_deletion_the_pages_come_on_a_confirmed_request_only_and_still_leave_the_tablet() {
    let _report = common::report("blackboard: delete after import off");
    let library = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    let offered = Arc::new(Mutex::new(Vec::new()));
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        options(true, false),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        library_store(board.clone(), None, offered.clone()),
    );
    let connected = run.wait(|s| s.connection == Connection::Connected);
    assert_eq!(connected.device.unwrap().stored_pages, 3);
    // No automatic sync.
    assert!(run.seen.iter().all(|s| s.connection != Connection::Syncing));
    assert_eq!(board.stats().downloads, 0);
    // Without confirming the deletion: refused, nothing done.
    assert_eq!(
        run.service.sync_now(false),
        Err(DeviceError::BlackboardConfirmDelete)
    );
    assert_eq!(board.stats().downloads, 0);
    run.service.sync_now(true).unwrap();
    let status = run.wait(|s| s.last_sync.is_some());
    assert_eq!(status.last_sync.unwrap().deleted, 3);
    assert_eq!(board.stored_page_count(), 0);
    assert_eq!(notes(library.path()).len(), 3);
    run.service.shutdown();
}

/// A short handwriting: hovering, two strokes, the pen leaving between them.
fn handwriting() -> PenScript {
    let line = |y: u16, pressure: u16, n: u16| -> Vec<Point> {
        (0..n)
            .map(|i| Point {
                x: 3000 + i * 40,
                y,
                pressure,
            })
            .collect()
    };
    let mut first = line(5000, 0, 6);
    first.extend(line(5000, 800, 30));
    let mut second = line(9000, 0, 3);
    second.extend(line(9000, 1500, 25));
    PenScript::new()
        .near(first)
        .away(Duration::from_millis(300))
        .near(second)
        .away(Duration::from_millis(200))
}

/// Reads live inputs until `what` accepts one; returns the records before it.
fn records_until(
    inputs: &Receiver<LiveInput>,
    what: impl Fn(&LiveInput) -> bool,
) -> (LiveInput, Vec<LiveRecord>) {
    let mut records = Vec::new();
    loop {
        let input = inputs.recv_timeout(WAIT).expect("live input in time");
        if what(&input) {
            return (input, records);
        }
        if let LiveInput::Records { records: r, .. } = &input {
            records.extend(r.iter().copied());
        }
    }
}

fn touching(records: &[LiveRecord]) -> Vec<(u16, u16, u16)> {
    records
        .iter()
        .filter_map(|r| match r {
            LiveRecord::Pen(p) if p.pressure > 0 => Some((p.x, p.y, p.pressure)),
            _ => None,
        })
        .collect()
}

#[test]
fn live_mode_gives_the_strokes_written_and_goes_on_when_a_page_is_stored() {
    let _report = common::report("blackboard: live mode");
    let library = new_library();
    let board = SimBoard::blackboard();
    added_beforehand(&board, library.path());
    let script = handwriting();
    let expected: Vec<(u16, u16, u16)> = script
        .points()
        .filter(|p| p.pressure > 0)
        .map(|p| (p.x, p.y, p.pressure))
        .collect();
    let offered = Arc::new(Mutex::new(Vec::new()));
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions {
            // Not used by the blackboard: its page is never turned.
            orientation: Orientation::PortraitLeft,
            ..TabletOptions::default()
        },
        Arc::new(Writer {
            board: board.clone(),
            script,
        }),
        library_store(board.clone(), None, offered),
    );
    run.wait(|s| s.connection == Connection::Connected);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    let (started, _) = records_until(&inputs, |i| matches!(i, LiveInput::Started { .. }));
    assert_eq!(
        started,
        LiveInput::Started {
            width: 20_500,
            height: 27_200,
            orientation: Orientation::Landscape,
            kind: TabletKind::Blackboard,
        }
    );
    run.wait(|s| s.connection == Connection::Live && s.active == Some(TabletKind::Blackboard));
    // Everything the hand wrote, in order: touching records exactly, hovering ones for the
    // pointer, the pen leaving twice.
    let mut received = Vec::new();
    while touching(&received).len() < expected.len() {
        let input = inputs.recv_timeout(WAIT).expect("records in time");
        if let LiveInput::Records { records, .. } = input {
            received.extend(records);
        }
    }
    assert_eq!(touching(&received), expected);
    assert!(
        received
            .iter()
            .any(|r| matches!(r, LiveRecord::Pen(p) if p.pressure == 0))
    );
    say!("{} records received live", received.len());

    // The store button during live mode: the page is stored, live mode goes on.
    board.press_store();
    board.draw_live(handwriting());
    let mut more = Vec::new();
    while touching(&more).len() < expected.len() {
        match inputs.recv_timeout(WAIT).expect("records in time") {
            LiveInput::Records { records, .. } => more.extend(records),
            LiveInput::State(state) => panic!("live mode interrupted: {state:?}"),
            _ => {}
        }
    }
    assert_eq!(touching(&more), expected);
    assert_eq!(board.stats().page_stored_events, 1);
    run.service.stop_live();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Stopped));
    // Afterwards the stored page is counted and imported (deletion on): a safe duplicate of the
    // live note.
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.saved.len() == 1));
    assert_eq!(status.device.unwrap().stored_pages, 0);
    assert_eq!(board.stored_page_count(), 0);
    run.service.shutdown();
}

#[test]
fn a_link_lost_during_live_mode_is_recovered() {
    let _report = common::report("blackboard: link lost during live mode");
    let library = new_library();
    let board = SimBoard::blackboard();
    added_beforehand(&board, library.path());
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(Writer {
            board: board.clone(),
            script: handwriting(),
        }),
        Box::new(|_, _, _| Err("no page in this test".into())),
    );
    run.wait(|s| s.connection == Connection::Connected);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    let (_, before) = records_until(&inputs, |i| matches!(i, LiveInput::Records { .. }));
    assert!(before.is_empty());
    board.disconnect_after(Duration::from_millis(100));
    let (_, _) = records_until(&inputs, |i| *i == LiveInput::State(LiveState::Reconnecting));
    let (_, _) = records_until(&inputs, |i| *i == LiveInput::State(LiveState::Live));
    // Records flow again on the new link.
    let (_, _) = records_until(&inputs, |i| matches!(i, LiveInput::Records { .. }));
    let stats = board.stats();
    say!("blackboard: {stats:?}");
    assert_eq!(stats.links_lost, 1);
    assert_eq!(stats.connections, 2);
    run.wait(|s| s.connection == Connection::Live);
    run.service.stop_live();
    run.service.shutdown();
}

#[test]
fn both_tablets_known_the_one_switched_on_connects_and_the_slate_goes_first() {
    let _report = common::report("two tablets: whichever is on");
    let library = new_library();
    let tablet = SimTablet::new(SimConfig::slate_large());
    let board = SimBoard::with_clock(SimBoardConfig::blackboard(), tablet.clock());
    // Both known from this library.
    tablet.set_registered_id(Some(ID));
    let config = tablet.config();
    identity::save(
        library.path(),
        &DeviceRecord {
            format: DEVICE_FORMAT,
            uuid: HostId(ID),
            address: config.address.clone(),
            name: config.name.clone(),
            protocol: "slate".into(),
            width: config.width,
            height: config.height,
            registered: "2026-10-24T14:00:00+02:00".into(),
            extra: Default::default(),
        },
    )
    .unwrap();
    added_beforehand(&board, library.path());
    tablet.power_off();
    let mut run = Running::start(
        vec![
            SimDevice::Slate(tablet.clone()),
            SimDevice::Board(board.clone()),
        ],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        Box::new(|_, _, _| Err("no page in this test".into())),
    );
    let status = run.wait(|s| s.connection == Connection::Connected);
    assert_eq!(status.active, Some(TabletKind::Blackboard));
    assert_eq!(
        status.known.iter().map(|k| k.kind).collect::<Vec<_>>(),
        [TabletKind::Slate, TabletKind::Blackboard]
    );
    // Renaming is the Slate's.
    assert_eq!(
        run.service.rename("Classe"),
        Err(DeviceError::NotSupported(
            "the blackboard has no name to change"
        ))
    );
    assert_eq!(run.service.set_time(), Ok(0));
    // The Slate is switched on (its button pressed): it gets the link at the next look.
    tablet.power_on();
    tablet.clock().advance(Duration::from_secs(11));
    let status =
        run.wait(|s| s.connection == Connection::Connected && s.active == Some(TabletKind::Slate));
    assert_eq!(status.device.unwrap().kind, TabletKind::Slate);
    assert!(!board.is_connected());
    // Forgetting the blackboard keeps the Slate connected and known.
    run.service.forget_blackboard().unwrap();
    let status = run.wait(|s| s.known.len() == 1);
    assert_eq!(status.known[0].kind, TabletKind::Slate);
    assert_eq!(status.active, Some(TabletKind::Slate));
    assert!(identity::load_blackboard(library.path()).unwrap().is_none());
    assert!(identity::load(library.path()).unwrap().is_some());
    run.service.shutdown();
}

#[test]
fn a_search_that_hears_nothing_ends_with_a_translated_error() {
    let _report = common::report("blackboard: nothing nearby");
    let library = new_library();
    let board = SimBoard::blackboard();
    board.power_off();
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board)),
        Box::new(|_, _, _| Err("no page in this test".into())),
    );
    run.wait(|s| s.connection == Connection::NotRegistered);
    run.service.find_blackboards();
    run.wait(|s| s.connection == Connection::Adding);
    let status = run.wait(|s| s.connection == Connection::NotRegistered);
    assert_eq!(status.error.unwrap().code, "blackboardNotFound");
    assert_eq!(status.board_search, None);
    // A search can be cancelled too.
    run.service.find_blackboards();
    run.wait(|s| s.connection == Connection::Adding);
    run.service.cancel();
    let status = run.wait(|s| s.connection == Connection::NotRegistered);
    assert_eq!(status.board_search, None);
    run.service.shutdown();
}

// --- the service's sync of the blackboard, fault by fault (the app's own per-page step) -------

/// The service, for actions a test runs on the worker's thread (in a page store, a fault).
#[derive(Clone, Default)]
struct Later(Arc<OnceLock<Weak<TabletService>>>);

impl Later {
    fn set(&self, run: &Running) {
        let _ = self.0.set(Arc::downgrade(&run.service));
    }

    fn service(&self) -> Arc<TabletService> {
        self.0
            .get()
            .and_then(Weak::upgrade)
            .expect("the service runs")
    }
}

/// The page store of the app without the app ([`LibrarySink`]), running `before` first with
/// the 1-based number of the page offered.
fn store_with(before: impl Fn(u16) -> Result<(), String> + Send + 'static) -> PageStore {
    let count = AtomicU16::new(0);
    Box::new(
        move |root: &Path, page: DownloadedPage, context: &PageContext| {
            let n = count.fetch_add(1, Ordering::SeqCst) + 1;
            before(n)?;
            let library = Library::open(root).map_err(|e| e.to_string())?;
            let mut sink = LibrarySink::new(&library, "", context.size, "Calque 1", || NOW_MS)
                .with_orientation(context.orientation);
            sink.store(page).map(|saved| saved.path)
        },
    )
}

/// The bytes kept with each note of the library (`original.bin`), sorted.
fn kept_bytes(root: &Path) -> Vec<Vec<u8>> {
    let library = Library::open(root).unwrap();
    let mut kept: Vec<Vec<u8>> = notes(root)
        .iter()
        .map(|path| {
            library
                .read(path)
                .unwrap()
                .file
                .original
                .expect("the page's bytes are kept")
        })
        .collect();
    kept.sort();
    kept
}

fn sorted(pages: &[BoardStoredPage]) -> Vec<Vec<u8>> {
    let mut bytes: Vec<Vec<u8>> = pages.iter().map(|page| page.bytes.clone()).collect();
    bytes.sort();
    bytes
}

/// Another sync ended than `before` (the `last_sync` of an earlier status).
fn synced_after(status: &TabletStatus, before: &Option<LastSync>) -> bool {
    status.last_sync.is_some() && status.last_sync != *before
}

fn last_at(status: &TabletStatus) -> Option<LastSync> {
    status.last_sync.clone()
}

#[test]
fn a_stalled_transfer_is_retried_on_a_fresh_link_and_a_second_stall_keeps_the_page() {
    let _report = common::report("blackboard: a stalled transfer, once then twice");
    let library = new_library();
    let board = board_with_pages(None);
    let pages = board.stored_pages();
    added_beforehand(&board, library.path());
    // The first download stops after 100 bytes: tried again on a fresh link.
    board.stall_next_download(100);
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.clone().unwrap();
    say!("stalled once: {last:?}, {:?}", board.stats());
    assert_eq!((last.saved.len(), last.deleted, last.error), (3, 3, None));
    assert_eq!(
        board.stats().connections,
        2,
        "the retry ran on a fresh link"
    );
    assert!(
        kept_bytes(library.path()) == sorted(&pages),
        "every page as stored"
    );

    // Twice: a typed error, the page stays, nothing wrong kept.
    board.stall_next_download(70);
    board.stall_next_download(140);
    let before = last_at(&status);
    board.draw_page(&reference_board_pages()[0]);
    let status = run.wait(|s| synced_after(s, &before));
    let last = status.last_sync.clone().unwrap();
    say!("stalled twice: {last:?}");
    assert_eq!(last.error.as_ref().unwrap().code, "transferIncomplete");
    assert_eq!((last.saved.len(), last.deleted), (0, 0));
    assert_eq!(board.stored_page_count(), 1);
    assert_eq!(notes(library.path()).len(), 3);
    // The stream was dirty: the link is opened again, the blackboard stays connected.
    let status =
        run.wait(|s| s.connection == Connection::Connected && board.stats().connections >= 4);
    assert_eq!(status.active, Some(TabletKind::Blackboard));
    run.service.shutdown();
}

#[test]
fn the_rest_of_an_earlier_transfer_never_joins_another_page() {
    let _report = common::report("blackboard: late bytes of a stalled transfer");
    let library = new_library();
    let board = board_with_pages(None);
    let first = board.stored_pages();
    added_beforehand(&board, library.path());
    // The review's first case: 140 bytes (20 whole records), a stall, the rest late. Retried on
    // the same link, the rest followed by the retry's first 140 bytes would make the page's
    // size, whole records of known statuses, in the wrong order.
    board.delay_next_download(140);
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.clone().unwrap();
    say!("late rest: {last:?}, {:?}", board.stats());
    assert_eq!((last.saved.len(), last.deleted, last.error), (3, 3, None));
    assert!(
        kept_bytes(library.path()) == sorted(&first),
        "every page as stored"
    );

    // The second case: a page entirely late. Its bytes would have completed the retry's count,
    // then the retry's own bytes would have become the start of the next page.
    let before = last_at(&status);
    board.delay_next_download(0);
    for page in reference_board_pages().iter().take(2) {
        board.draw_page(page);
    }
    let second = board.stored_pages();
    assert_eq!(second.len(), 2);
    let status = run.wait(|s| {
        synced_after(s, &before) && s.device.as_ref().is_some_and(|d| d.stored_pages == 0)
    });
    let last = status.last_sync.clone().unwrap();
    say!("late page: {last:?}, {:?}", board.stats());
    assert_eq!(last.error, None);
    let mut all = first.clone();
    all.extend(second);
    assert!(
        kept_bytes(library.path()) == sorted(&all),
        "every page as stored"
    );
    assert_eq!(board.stored_page_count(), 0);
    assert_eq!(board.stats().unknown_requests, 0);
    run.service.shutdown();
}

#[test]
fn a_cancelled_download_leaves_no_phantom_record_in_live_mode() {
    let _report = common::report("blackboard: cancel a download, then live mode");
    let library = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    let script = handwriting();
    let written: Vec<(u16, u16)> = script.points().map(|p| (p.x, p.y)).collect();
    let expected: Vec<(u16, u16, u16)> = script
        .points()
        .filter(|p| p.pressure > 0)
        .map(|p| (p.x, p.y, p.pressure))
        .collect();
    let later = Later::default();
    let (sender, inputs) = mpsc::channel();
    // The teacher cancels the sync to go live, in the middle of the first download. The page
    // comes over time, as from the real tablet: without the fresh link, the rest of it would
    // arrive after live mode started on the same link, and pass for pen records.
    board.pace_downloads(Duration::from_millis(10));
    let service = later.clone();
    board.on_next_download(move || {
        let service = service.service();
        service.cancel();
        service.start_live(Box::new(ChannelSink(sender)));
    });
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(Writer {
            board: board.clone(),
            script,
        }),
        store_with(|_| Ok(())),
    );
    later.set(&run);
    run.wait(|s| s.connection == Connection::Live);
    let mut received = Vec::new();
    while touching(&received).len() < expected.len() {
        match inputs.recv_timeout(WAIT).expect("records in time") {
            LiveInput::Records { records, .. } => received.extend(records),
            LiveInput::State(LiveState::Stopped) => panic!("live mode stopped"),
            _ => {}
        }
    }
    say!(
        "{} records received live, {:?}",
        received.len(),
        board.stats()
    );
    // Only what the hand wrote: the rest of the page cut short never passes for pen records.
    assert_eq!(touching(&received), expected);
    for record in &received {
        if let LiveRecord::Pen(point) = record {
            assert!(written.contains(&(point.x, point.y)), "a phantom record");
        }
    }
    let status = run.service.status();
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted, last.error), (0, 0, None));
    assert_eq!(board.stored_page_count(), 3, "nothing deleted");
    assert_eq!(
        board.stats().connections,
        2,
        "a fresh link before live mode"
    );
    run.service.stop_live();
    run.service.shutdown();
}

#[test]
fn a_slow_delete_is_waited_for_and_an_ignored_one_stops_the_sync() {
    let _report = common::report("blackboard: slow and ignored deletes");
    let library = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    // The tablet takes 200 ms to erase the first page: described again once, a little later.
    board.delay_next_delete(Duration::from_millis(200));
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.clone().unwrap();
    assert_eq!((last.saved.len(), last.deleted, last.error), (3, 3, None));
    assert_eq!(board.stored_page_count(), 0);

    // A delete the tablet ignores: the sync stops instead of importing the page again.
    board.ignore_next_delete();
    let before = last_at(&status);
    for page in reference_board_pages().iter().take(2) {
        board.draw_page(page);
    }
    let status = run.wait(|s| synced_after(s, &before));
    let last = status.last_sync.clone().unwrap();
    say!("ignored delete: {last:?}");
    assert_eq!(last.error.as_ref().unwrap().code, "pageNotDeleted");
    assert_eq!((last.saved.len(), last.deleted), (1, 0));
    assert_eq!(board.stored_page_count(), 2, "nothing lost");
    assert_eq!(status.device.unwrap().stored_pages, 2);
    assert_eq!(notes(library.path()).len(), 4);
    run.service.shutdown();
}

#[test]
fn an_unreadable_page_is_refused_and_stays_on_the_tablet() {
    let _report = common::report("blackboard: a page that is not records");
    let library = new_library();
    let board = SimBoard::blackboard();
    let pages = reference_board_pages();
    board.draw_page(&pages[0]);
    // Ten bytes: not whole records.
    board.store_raw_page(vec![0xa1, 1, 2, 3, 4, 5, 6, 0xa1, 1, 2]);
    board.draw_page(&pages[1]);
    added_beforehand(&board, library.path());
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.clone().unwrap();
    say!("{last:?}");
    let error = last.error.as_ref().unwrap();
    assert_eq!(error.code, "pageUnreadable");
    assert!(
        error
            .message
            .ends_with("1 records, 0 of an unknown status, 3 bytes of a partial record"),
        "{}",
        error.message
    );
    assert_eq!((last.saved.len(), last.deleted), (1, 1));
    assert_eq!(
        board.stored_page_count(),
        2,
        "the unreadable page and the next stay"
    );
    assert_eq!(notes(library.path()).len(), 1);
    // The diagnostic log has the reason, in counts.
    run.wait(|s| s.connection == Connection::Connected);
    run.service.shutdown();
    let diagnostics = library.path().join("_ardoise").join("diagnostics");
    let logs: String = std::fs::read_dir(&diagnostics)
        .unwrap()
        .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
        .collect();
    assert!(logs.contains("3 bytes of a partial record"));
}

#[test]
fn the_status_names_the_blackboard_after_each_kind_of_sync() {
    let _report = common::report("blackboard: the status after a sync");
    let library = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    let later = Later::default();
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    later.set(&run);
    let connected = |s: &TabletStatus| {
        s.connection == Connection::Connected && s.active == Some(TabletKind::Blackboard)
    };
    // The automatic sync at the connection: connected afterwards, the pages left going down.
    let status = run.wait(|s| s.last_sync.is_some());
    assert!(connected(&status), "{status:?}");
    let left: Vec<u16> = run
        .seen
        .iter()
        .filter(|s| s.connection == Connection::Syncing)
        .filter_map(|s| s.device.as_ref().map(|d| d.stored_pages))
        .collect();
    say!("pages left while syncing: {left:?}");
    assert!(left.contains(&2) && left.contains(&1), "{left:?}");

    // An explicit sync of nothing.
    let before = last_at(&status);
    run.service.sync_now(false).unwrap();
    let status = run.wait(|s| synced_after(s, &before));
    assert_eq!(status.last_sync.as_ref().unwrap().saved.len(), 0);
    assert!(connected(&status), "{status:?}");

    // An explicit sync of pages, with deletion off: confirmed.
    let options = options(true, false);
    run.service.set_options(options);
    run.wait(|s| s.options == options);
    for page in reference_board_pages().iter().take(2) {
        board.draw_page(page);
    }
    run.wait(|s| s.device.as_ref().is_some_and(|d| d.stored_pages == 2));
    let before = last_at(&run.service.status());
    assert_eq!(
        run.service.sync_now(false),
        Err(DeviceError::BlackboardConfirmDelete)
    );
    run.service.sync_now(true).unwrap();
    let status = run.wait(|s| synced_after(s, &before));
    assert_eq!(status.last_sync.as_ref().unwrap().deleted, 2);
    assert!(connected(&status), "{status:?}");

    // A cancelled sync: still connected, then the link opened again (its stream was dirty).
    for page in reference_board_pages().iter().take(2) {
        board.draw_page(page);
    }
    run.wait(|s| s.device.as_ref().is_some_and(|d| d.stored_pages == 2));
    let service = later.clone();
    board.on_next_download(move || service.service().cancel());
    let links = board.stats().connections;
    let before = last_at(&run.service.status());
    run.service.sync_now(true).unwrap();
    let status = run.wait(|s| synced_after(s, &before));
    let last = status.last_sync.clone().unwrap();
    assert_eq!((last.deleted, last.error), (0, None));
    assert!(connected(&status), "{status:?}");
    let status = run.wait(|s| connected(s) && board.stats().connections > links);
    assert_eq!(status.device.unwrap().stored_pages, 2);
    assert_eq!(board.stored_page_count(), 2);
    run.service.shutdown();
}

#[test]
fn a_page_stored_during_a_failed_sync_is_counted_at_once() {
    let _report = common::report("blackboard: the page stored event during a sync");
    let library = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    let pressed = board.clone();
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        // The user stores a page with the button while the first is saved (the event is taken
        // in before the second page); the second page cannot be saved.
        store_with(move |n| {
            if n == 1 {
                pressed.draw_page(&reference_board_pages()[0]);
            }
            if n == 2 {
                return Err("the disk is full".into());
            }
            Ok(())
        }),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    assert_eq!(
        status.last_sync.unwrap().error.unwrap().code,
        "libraryAccess"
    );
    // Counted at once (the virtual clock stands still while the link is idle): synced again.
    let status = run.wait(|s| {
        s.last_sync.as_ref().is_some_and(|l| l.error.is_none())
            && s.device.as_ref().is_some_and(|d| d.stored_pages == 0)
    });
    say!("{:?}", status.last_sync);
    assert_eq!(board.stored_page_count(), 0);
    assert_eq!(notes(library.path()).len(), 4);
    run.service.shutdown();
}

/// A Slate registered from `root`, with the identity `ID`.
fn slate_known(tablet: &SimTablet, root: &Path) {
    tablet.set_registered_id(Some(ID));
    let config = tablet.config();
    identity::save(
        root,
        &DeviceRecord {
            format: DEVICE_FORMAT,
            uuid: HostId(ID),
            address: config.address.clone(),
            name: config.name.clone(),
            protocol: "slate".into(),
            width: config.width,
            height: config.height,
            registered: "2026-10-24T14:00:00+02:00".into(),
            extra: Default::default(),
        },
    )
    .unwrap();
}

/// Waits until the virtual clock moved `by` (the worker searching meanwhile).
fn searched_for(clock: &ardoise_sim::VirtualClock, by: Duration) {
    let until = clock.monotonic() + by;
    let deadline = Instant::now() + WAIT;
    while clock.monotonic() < until {
        assert!(Instant::now() < deadline, "the worker does not search");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn live_mode_resumes_only_on_the_tablet_it_started_on() {
    let _report = common::report("two tablets: live mode keeps its tablet");
    let library = new_library();
    let tablet = SimTablet::new(SimConfig::slate_large());
    let board = SimBoard::with_clock(SimBoardConfig::blackboard(), tablet.clock());
    slate_known(&tablet, library.path());
    added_beforehand(&board, library.path());
    board.power_off();
    let mut run = Running::start(
        vec![
            SimDevice::Slate(tablet.clone()),
            SimDevice::Board(board.clone()),
        ],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Connected && s.active == Some(TabletKind::Slate));
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    let (started, _) = records_until(&inputs, |i| matches!(i, LiveInput::Started { .. }));
    let config = tablet.config();
    assert!(
        matches!(started, LiveInput::Started { width, height, .. } if (width, height) == (config.width, config.height))
    );
    run.wait(|s| s.connection == Connection::Live);

    // The Slate goes away; the blackboard, switched on, is not taken for it.
    board.power_on();
    tablet.power_off();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Reconnecting));
    searched_for(&tablet.clock(), Duration::from_secs(60));
    assert_eq!(board.stats().connections, 0, "the blackboard is left alone");
    tablet.power_on();
    let (next, _) = records_until(&inputs, |i| {
        matches!(
            i,
            LiveInput::State(LiveState::Live) | LiveInput::Started { .. }
        )
    });
    assert_eq!(
        next,
        LiveInput::State(LiveState::Live),
        "the same page goes on"
    );
    run.wait(|s| s.connection == Connection::Live && s.active == Some(TabletKind::Slate));
    run.service.stop_live();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Stopped));
    run.service.shutdown();

    // The other way: live mode on the blackboard, the Slate switched on while it is away.
    let tablet = SimTablet::new(SimConfig::slate_large());
    let board = SimBoard::with_clock(SimBoardConfig::blackboard(), tablet.clock());
    slate_known(&tablet, library.path());
    tablet.power_off();
    let mut run = Running::start(
        vec![
            SimDevice::Slate(tablet.clone()),
            SimDevice::Board(board.clone()),
        ],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Connected && s.active == Some(TabletKind::Blackboard));
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    let (started, _) = records_until(&inputs, |i| matches!(i, LiveInput::Started { .. }));
    assert!(matches!(
        started,
        LiveInput::Started {
            width: 20_500,
            height: 27_200,
            ..
        }
    ));
    tablet.power_on();
    board.power_off();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Reconnecting));
    searched_for(&tablet.clock(), Duration::from_secs(60));
    assert_eq!(tablet.stats().connections, 0, "the Slate is left alone");
    board.power_on();
    let (next, _) = records_until(&inputs, |i| {
        matches!(
            i,
            LiveInput::State(LiveState::Live) | LiveInput::Started { .. }
        )
    });
    assert_eq!(next, LiveInput::State(LiveState::Live));
    run.wait(|s| s.connection == Connection::Live && s.active == Some(TabletKind::Blackboard));
    run.service.stop_live();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Stopped));
    run.service.shutdown();

    // Live mode asked with nothing connected takes the first tablet that connects.
    let tablet = SimTablet::new(SimConfig::slate_large());
    let board = SimBoard::with_clock(SimBoardConfig::blackboard(), tablet.clock());
    slate_known(&tablet, library.path());
    tablet.power_off();
    board.power_off();
    let mut run = Running::start(
        vec![
            SimDevice::Slate(tablet.clone()),
            SimDevice::Board(board.clone()),
        ],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Searching);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Waiting));
    board.power_on();
    let (started, _) = records_until(&inputs, |i| matches!(i, LiveInput::Started { .. }));
    assert!(matches!(
        started,
        LiveInput::Started {
            width: 20_500,
            height: 27_200,
            ..
        }
    ));
    run.service.stop_live();
    run.service.shutdown();
}

#[test]
fn the_blackboards_pages_are_deleted_only_with_a_permission() {
    let _report = common::report("blackboard: who may delete its pages");
    let off = options(true, false);

    // Both known, nothing connected, deletion off: a sync asked (for the Slate) does nothing
    // for the blackboard that connects.
    let library = new_library();
    let tablet = SimTablet::new(SimConfig::slate_large());
    let board = board_with_pages(Some(tablet.clock()));
    slate_known(&tablet, library.path());
    added_beforehand(&board, library.path());
    tablet.power_off();
    board.power_off();
    let mut run = Running::start(
        vec![
            SimDevice::Slate(tablet.clone()),
            SimDevice::Board(board.clone()),
        ],
        library.path(),
        off,
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Searching);
    run.service.sync_now(false).unwrap();
    run.wait(|s| s.pending == Some(ardoise_device::service::Pending::Sync));
    board.power_on();
    let status = run.wait(|s| s.connection == Connection::Connected && s.pending.is_none());
    assert_eq!(status.active, Some(TabletKind::Blackboard));
    assert_eq!(status.error, None);
    assert_eq!((board.stats().downloads, board.stored_page_count()), (0, 3));
    run.service.shutdown();

    // Deletion on at the start, a lost link, deletion turned off meanwhile: the sync resumed
    // does not delete.
    let library = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    let later = Later::default();
    let (dropped, service) = (board.clone(), later.clone());
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(move |n| {
            if n == 1 {
                dropped.disconnect_at_next_request();
                service.service().set_options(off);
            }
            Ok(())
        }),
    );
    later.set(&run);
    let status = run.wait(|s| s.last_sync.is_some());
    assert_eq!(
        status.last_sync.unwrap().error.unwrap().code,
        "tabletNotConnected"
    );
    run.wait(|s| s.connection == Connection::Connected && s.options == off);
    let status = run.wait(|s| s.connection == Connection::Connected && s.pending.is_none());
    assert!(
        run.seen
            .iter()
            .filter(|s| s.connection == Connection::Syncing)
            .count()
            > 0
    );
    assert_eq!(status.device.unwrap().stored_pages, 3);
    assert_eq!(board.stats().downloads, 1);
    assert_eq!(
        board.stored_page_count(),
        3,
        "the first page kept, not deleted"
    );
    assert_eq!(notes(library.path()).len(), 1);
    run.service.shutdown();

    // A confirmed sync whose link drops before its first page goes on at the reconnection.
    let library = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        off,
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Connected);
    board.disconnect_at_next_request();
    run.service.sync_now(true).unwrap();
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.deleted == 3));
    assert_eq!(status.last_sync.unwrap().error, None);
    assert_eq!(board.stats().connections, 2);
    assert_eq!(board.stored_page_count(), 0);
    run.service.shutdown();

    // A confirmed sync goes on into the library chosen meanwhile.
    let library = new_library();
    let other = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    let later = Later::default();
    let (service, root, moved) = (
        later.clone(),
        library.path().to_owned(),
        other.path().to_owned(),
    );
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        off,
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(move |n| {
            if n == 1 {
                service
                    .service()
                    .set_library(Some(moved.clone()), Some(root.clone()));
            }
            Ok(())
        }),
    );
    later.set(&run);
    run.wait(|s| s.connection == Connection::Connected);
    run.service.sync_now(true).unwrap();
    run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.deleted == 2));
    assert_eq!(board.stored_page_count(), 0);
    assert_eq!(
        (notes(library.path()).len(), notes(other.path()).len()),
        (1, 2)
    );
    run.service.shutdown();
}

#[test]
fn forgetting_a_tablet_ends_what_waits_for_it_unless_the_other_holds_the_link() {
    let _report = common::report("forget: what waits for the tablet");
    // A library that only knows a Slate, live mode waiting for it: as before the blackboard.
    let library = new_library();
    let tablet = SimTablet::new(SimConfig::slate_large());
    slate_known(&tablet, library.path());
    tablet.power_off();
    let mut run = Running::start(
        vec![SimDevice::Slate(tablet.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::with_delay(
            tablet.clone(),
            Duration::ZERO,
        )),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Searching);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    run.service.sync_now(false).unwrap();
    run.wait(|s| s.pending == Some(ardoise_device::service::Pending::Live));
    run.service.forget().unwrap();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Stopped));
    let status = run.wait(|s| s.connection == Connection::NotRegistered);
    assert_eq!((status.pending, status.registered), (None, None));
    run.service.shutdown();

    // A library that only knows the blackboard, live mode waiting for it.
    let library = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    board.power_off();
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Searching);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    run.wait(|s| s.pending == Some(ardoise_device::service::Pending::Live));
    run.service.forget_blackboard().unwrap();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Stopped));
    let status = run.wait(|s| s.connection == Connection::NotRegistered);
    assert_eq!((status.pending, status.known.len()), (None, 0));
    run.service.shutdown();

    // Both known, the blackboard syncing: forgetting the Slate leaves its sync alone.
    let library = new_library();
    let tablet = SimTablet::new(SimConfig::slate_large());
    let board = board_with_pages(Some(tablet.clock()));
    slate_known(&tablet, library.path());
    added_beforehand(&board, library.path());
    tablet.power_off();
    let later = Later::default();
    let (service, started) = (later.clone(), Arc::new(Mutex::new(None)));
    let forgotten = started.clone();
    board.on_next_download(move || {
        let service = service.service();
        *forgotten.lock().unwrap() = Some(std::thread::spawn(move || service.forget()));
        // The request is sent while this download runs.
        std::thread::sleep(Duration::from_millis(100));
    });
    let mut run = Running::start(
        vec![
            SimDevice::Slate(tablet.clone()),
            SimDevice::Board(board.clone()),
        ],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    later.set(&run);
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    say!("{last:?}");
    assert_eq!((last.saved.len(), last.deleted, last.error), (3, 3, None));
    let forget = started
        .lock()
        .unwrap()
        .take()
        .expect("forgotten during the sync");
    forget.join().unwrap().unwrap();
    let status = run.service.status();
    assert_eq!(status.known[0].kind, TabletKind::Blackboard);
    assert!(identity::load(library.path()).unwrap().is_none());
    run.service.shutdown();
}

#[test]
fn an_unreadable_blackboard_file_means_no_blackboard_and_no_error() {
    let _report = common::report("blackboard: blackboard.json cannot be read");
    let library = new_library();
    let file = identity::blackboard_file(library.path());
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(&file, "{ not json").unwrap();
    let board = board_with_pages(None);
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    let status = run.wait(|s| s.connection == Connection::NotRegistered);
    assert_eq!((status.error, status.known.len()), (None, 0));
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(run.service.status().error, None);
    assert_eq!(
        std::fs::read_to_string(&file).unwrap(),
        "{ not json",
        "left as it is"
    );
    assert_eq!(board.stats().connections, 0);
    run.service.shutdown();
}

/// Refuses the tablet that streams, as the live hub does with a sheet that tablet cannot write on.
struct Refusing(Sender<LiveInput>);

impl LiveSink for Refusing {
    fn input(&mut self, input: LiveInput) {
        let _ = self.0.send(input);
    }

    fn active(&self) -> bool {
        false
    }
}

#[test]
fn a_sink_that_refuses_the_tablet_ends_live_mode_and_the_link_stays() {
    let _report = common::report("blackboard: live mode refused by its sink");
    let library = new_library();
    let board = SimBoard::blackboard();
    added_beforehand(&board, library.path());
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(Writer {
            board: board.clone(),
            script: handwriting(),
        }),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Connected);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(Refusing(sender)));
    let (started, _) = records_until(&inputs, |i| matches!(i, LiveInput::Started { .. }));
    assert!(matches!(
        started,
        LiveInput::Started {
            kind: TabletKind::Blackboard,
            ..
        }
    ));
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Stopped));
    let status = run.wait(|s| s.connection == Connection::Connected && s.pending.is_none());
    assert_eq!(status.active, Some(TabletKind::Blackboard));
    assert!(!board.is_streaming());
    assert_eq!(board.stats().connections, 1);
    run.service.shutdown();
}

// --- after the second review of M18 ----------------------------------------------------------

/// Live inputs until `what` accepts one; fails on a stop or on a `Started` of another tablet
/// before it.
fn until_without_stop(
    inputs: &Receiver<LiveInput>,
    kind: TabletKind,
    what: impl Fn(&LiveInput) -> bool,
) -> Vec<LiveRecord> {
    let mut records = Vec::new();
    loop {
        let input = inputs.recv_timeout(WAIT).expect("live input in time");
        if what(&input) {
            return records;
        }
        match input {
            LiveInput::Records { records: r, .. } => records.extend(r),
            LiveInput::State(LiveState::Stopped) => panic!("live mode stopped"),
            LiveInput::Started { kind: other, .. } if other != kind => {
                panic!("live mode went on with the {}", other.name())
            }
            _ => {}
        }
    }
}

/// Every pen record received was written by the hand (`written`): nothing of a page.
fn assert_no_phantom(received: &[LiveRecord], written: &[(u16, u16)]) {
    for record in received {
        if let LiveRecord::Pen(point) = record {
            assert!(
                written.contains(&(point.x, point.y)),
                "a phantom record at {},{}",
                point.x,
                point.y
            );
        }
    }
}

#[test]
fn late_page_bytes_found_when_live_mode_starts_open_the_link_again() {
    let _report = common::report("blackboard: page bytes late at the start of live mode");
    let library = new_library();
    let board = SimBoard::blackboard();
    board.draw_page(&reference_board_pages()[0]);
    let page = board.stored_pages()[0].bytes.clone();
    added_beforehand(&board, library.path());
    board.power_off();
    let script = handwriting();
    let written: Vec<(u16, u16)> = script.points().map(|p| (p.x, p.y)).collect();
    let expected: Vec<(u16, u16, u16)> = script
        .points()
        .filter(|p| p.pressure > 0)
        .map(|p| (p.x, p.y, p.pressure))
        .collect();
    let later = Later::default();
    let (sender, inputs) = mpsc::channel();
    // Live mode is asked while the page downloads; right after the page is deleted, the tablet
    // sends more of it (a transfer thought complete): found just before live mode starts.
    let service = later.clone();
    board.on_next_download(move || service.service().start_live(Box::new(ChannelSink(sender))));
    let more = board.clone();
    board.on_next_delete(move || more.send_stream(&page[..14]));
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(Writer {
            board: board.clone(),
            script,
        }),
        store_with(|_| Ok(())),
    );
    later.set(&run);
    board.power_on();
    until_without_stop(&inputs, TabletKind::Blackboard, |i| {
        matches!(i, LiveInput::Started { .. })
    });
    let mut received = Vec::new();
    while touching(&received).len() < expected.len() {
        match inputs.recv_timeout(WAIT).expect("records in time") {
            LiveInput::Records { records, .. } => received.extend(records),
            LiveInput::State(LiveState::Stopped) => panic!("live mode stopped"),
            _ => {}
        }
    }
    say!(
        "{} records received live, {:?}",
        received.len(),
        board.stats()
    );
    assert_eq!(touching(&received), expected);
    assert_no_phantom(&received, &written);
    let status = run.wait(|s| s.connection == Connection::Live);
    assert_eq!(status.active, Some(TabletKind::Blackboard));
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted, last.error), (1, 1, None));
    // The late bytes reopened the link (the same rule as for a sync), never shown as an error.
    assert_eq!(
        board.stats().connections,
        2,
        "a fresh link before live mode"
    );
    assert!(
        run.seen.iter().all(|s| s.error.is_none()),
        "{:?}",
        run.seen.iter().find_map(|s| s.error.clone())
    );
    run.service.stop_live();
    run.service.shutdown();
}

#[test]
fn bytes_after_a_page_use_up_its_retry_and_the_sync_ends() {
    let _report = common::report("blackboard: more of a page while its note is written");
    let library = new_library();
    let board = board_with_pages(None);
    let pages = board.stored_pages();
    added_beforehand(&board, library.path());
    board.power_off();
    // The first download stops short (its retry, on a fresh link); there the page comes whole,
    // but more of it arrives while its note is written: the retry is used up, the sync ends
    // rather than open the link again and again.
    board.stall_next_download(70);
    let more = board.clone();
    let extra = pages[0].bytes[..14].to_vec();
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(move |n| {
            if n == 1 {
                more.send_stream(&extra);
            }
            Ok(())
        }),
    );
    board.power_on();
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.clone().unwrap();
    say!("{last:?}, {:?}", board.stats());
    assert_eq!(last.error.as_ref().unwrap().code, "transferIncomplete");
    // The note written before the bytes came stays (the guides say a doubtful transfer may
    // give a second note); nothing left the tablet.
    assert_eq!((last.saved.len(), last.deleted), (1, 0));
    assert_eq!(board.stored_page_count(), 3);
    assert_eq!(notes(library.path()).len(), 1);
    // The stream was dirty: one more fresh link, then nothing more.
    run.wait(|s| s.connection == Connection::Connected && board.stats().connections == 3);
    std::thread::sleep(Duration::from_millis(300));
    let stats = board.stats();
    assert_eq!(
        (stats.connections, stats.downloads, stats.deletes),
        (3, 2, 0)
    );
    assert_eq!(run.service.status().last_sync, Some(last));
    run.service.shutdown();
}

#[test]
fn a_tablet_streaming_unasked_never_gives_a_page_and_the_sync_ends() {
    let _report = common::report("blackboard: a stream that never goes quiet");
    let library = new_library();
    let board = board_with_pages(None);
    added_beforehand(&board, library.path());
    board.power_off();
    // A tablet still in live mode after a reconnection (not measured): the pen hovers for ten
    // minutes without a pause, streamed to every link without being asked.
    board.stream_without_asking(true);
    board.draw_live(PenScript::new().near(vec![
        Point {
            x: 4000,
            y: 6000,
            pressure: 0
        };
        72_000
    ]));
    let offered = Arc::new(AtomicU16::new(0));
    let count = offered.clone();
    let mut run = Running::start(
        vec![SimDevice::Board(board.clone())],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(move |_| {
            count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
    );
    board.power_on();
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.clone().unwrap();
    say!("{last:?}, {:?}", board.stats());
    assert_eq!(last.error.as_ref().unwrap().code, "transferIncomplete");
    assert_eq!((last.saved.len(), last.deleted), (0, 0));
    // No download was ever asked while the stream was busy: no page from live records.
    assert_eq!(board.stats().downloads, 0);
    assert_eq!(offered.load(Ordering::SeqCst), 0);
    assert_eq!(board.stored_page_count(), 3);
    // One fresh link for the page's retry, one after the sync failed; no loop.
    run.wait(|s| s.connection == Connection::Connected && board.stats().connections == 3);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(board.stats().connections, 3);
    assert_eq!(run.service.status().last_sync, Some(last));
    run.service.shutdown();
}

/// Both tablets in one library (and one clock): the Slate registered from it, the blackboard
/// added; both switched off.
fn both_known(library: &Path) -> (SimTablet, SimBoard) {
    let tablet = SimTablet::new(SimConfig::slate_large());
    let board = SimBoard::with_clock(SimBoardConfig::blackboard(), tablet.clock());
    slate_known(&tablet, library);
    added_beforehand(&board, library);
    tablet.power_off();
    board.power_off();
    (tablet, board)
}

#[test]
fn while_a_live_run_waits_for_its_tablet_no_other_tablet_is_registered_or_added() {
    let _report = common::report("live mode keeps its tablet: registering and adding wait");
    // Live mode on the blackboard, its link lost: registering a Slate meanwhile is refused (it
    // would take the link and the run), the Slate is left alone; the blackboard back, the same
    // page goes on.
    let library = new_library();
    let tablet = SimTablet::new(SimConfig::slate_large());
    let board = SimBoard::with_clock(SimBoardConfig::blackboard(), tablet.clock());
    added_beforehand(&board, library.path());
    let mut run = Running::start(
        vec![
            SimDevice::Slate(tablet.clone()),
            SimDevice::Board(board.clone()),
        ],
        library.path(),
        TabletOptions::default(),
        Arc::new(
            ardoise_sim::SimHooks::with_delay(tablet.clone(), Duration::ZERO)
                .with_board(board.clone()),
        ),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Connected);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    records_until(&inputs, |i| {
        matches!(
            i,
            LiveInput::Started {
                kind: TabletKind::Blackboard,
                ..
            }
        )
    });
    board.power_off();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Reconnecting));
    run.service.register();
    let status = run.wait(|s| s.error.is_some());
    assert_eq!(status.error.unwrap().code, "tabletBusy");
    assert_eq!(
        (status.connection, status.register_step),
        (Connection::Searching, None)
    );
    searched_for(&tablet.clock(), Duration::from_secs(20));
    assert!(!tablet.in_register_mode());
    assert_eq!(tablet.stats().connections, 0, "the Slate is left alone");
    assert!(identity::load(library.path()).unwrap().is_none());
    board.power_on();
    until_without_stop(&inputs, TabletKind::Blackboard, |i| {
        *i == LiveInput::State(LiveState::Live)
    });
    run.wait(|s| s.connection == Connection::Live && s.active == Some(TabletKind::Blackboard));
    run.service.stop_live();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Stopped));
    run.service.shutdown();

    // Live mode on the Slate, its link lost: the search for a blackboard to add is refused (a
    // blackboard added would connect), the blackboard nearby is left alone.
    let library = new_library();
    let tablet = SimTablet::new(SimConfig::slate_large());
    let board = SimBoard::with_clock(SimBoardConfig::blackboard(), tablet.clock());
    slate_known(&tablet, library.path());
    let mut run = Running::start(
        vec![
            SimDevice::Slate(tablet.clone()),
            SimDevice::Board(board.clone()),
        ],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Connected);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    records_until(&inputs, |i| {
        matches!(
            i,
            LiveInput::Started {
                kind: TabletKind::Slate,
                ..
            }
        )
    });
    tablet.power_off();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Reconnecting));
    run.service.find_blackboards();
    let status = run.wait(|s| s.error.is_some());
    assert_eq!(status.error.unwrap().code, "tabletBusy");
    assert_eq!(
        (status.connection, status.board_search),
        (Connection::Searching, None)
    );
    assert_eq!(
        run.service.add_blackboard("00:00:5E:00:53:B1"),
        Err(DeviceError::TabletNotFound(
            "00:00:5E:00:53:B1 was not heard by the search".into()
        ))
    );
    searched_for(&tablet.clock(), Duration::from_secs(20));
    assert_eq!(board.stats().connections, 0, "the blackboard is left alone");
    assert!(identity::load_blackboard(library.path()).unwrap().is_none());
    tablet.power_on();
    until_without_stop(&inputs, TabletKind::Slate, |i| {
        *i == LiveInput::State(LiveState::Live)
    });
    run.service.stop_live();
    records_until(&inputs, |i| *i == LiveInput::State(LiveState::Stopped));
    run.service.shutdown();
}

#[test]
fn a_live_run_whose_tablet_is_forgotten_elsewhere_ends_and_the_other_tablet_writes_nothing() {
    let _report = common::report("live mode keeps its tablet: forgotten from another computer");
    // Live mode on the blackboard, its link lost; another computer forgets it (blackboard.json
    // gone from the shared library) while the Slate is switched on: the run ends, the Slate
    // connects without live mode.
    let library = new_library();
    let (tablet, board) = both_known(library.path());
    board.power_on();
    let mut run = Running::start(
        vec![
            SimDevice::Slate(tablet.clone()),
            SimDevice::Board(board.clone()),
        ],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Connected && s.active == Some(TabletKind::Blackboard));
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    until_without_stop(&inputs, TabletKind::Blackboard, |i| {
        *i == LiveInput::State(LiveState::Live)
    });
    board.power_off();
    until_without_stop(&inputs, TabletKind::Blackboard, |i| {
        *i == LiveInput::State(LiveState::Reconnecting)
    });
    identity::forget_blackboard(library.path()).unwrap();
    tablet.power_on();
    // The run ends; nothing of the Slate ever reaches it.
    let after = until_without_stop(&inputs, TabletKind::Blackboard, |i| {
        *i == LiveInput::State(LiveState::Stopped)
    });
    assert!(after.is_empty());
    // Through either guard: the run ended before the search (this one, no error), or when the
    // Slate connected (`sheetOtherTablet` in the status).
    run.wait(|s| {
        s.connection == Connection::Connected
            && s.active == Some(TabletKind::Slate)
            && s.pending.is_none()
    });
    assert!(!tablet.is_streaming());
    std::thread::sleep(Duration::from_millis(100));
    assert!(inputs.try_recv().is_err(), "nothing after the stop");
    run.service.shutdown();

    // The other way: live mode on the Slate, device.json gone while it is away, the blackboard
    // switched on.
    let library = new_library();
    let (tablet, board) = both_known(library.path());
    tablet.power_on();
    let mut run = Running::start(
        vec![
            SimDevice::Slate(tablet.clone()),
            SimDevice::Board(board.clone()),
        ],
        library.path(),
        TabletOptions::default(),
        Arc::new(ardoise_sim::SimHooks::for_board(board.clone())),
        store_with(|_| Ok(())),
    );
    run.wait(|s| s.connection == Connection::Connected && s.active == Some(TabletKind::Slate));
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    until_without_stop(&inputs, TabletKind::Slate, |i| {
        *i == LiveInput::State(LiveState::Live)
    });
    tablet.power_off();
    until_without_stop(&inputs, TabletKind::Slate, |i| {
        *i == LiveInput::State(LiveState::Reconnecting)
    });
    identity::forget(library.path()).unwrap();
    board.power_on();
    let after = until_without_stop(&inputs, TabletKind::Slate, |i| {
        *i == LiveInput::State(LiveState::Stopped)
    });
    assert!(after.is_empty());
    run.wait(|s| {
        s.connection == Connection::Connected
            && s.active == Some(TabletKind::Blackboard)
            && s.pending.is_none()
    });
    assert!(!board.is_streaming());
    assert_eq!(board.stats().live_records, 0);
    std::thread::sleep(Duration::from_millis(100));
    assert!(inputs.try_recv().is_err(), "nothing after the stop");
    run.service.shutdown();
}
