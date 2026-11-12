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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ardoise_core::library::Library;
use ardoise_core::note::Timing;
use ardoise_core::settings::Orientation;
use ardoise_device::blackboard::BoardSession;
use ardoise_device::identity::{self, DEVICE_FORMAT, DeviceRecord, HostId};
use ardoise_device::log::DiagnosticLog;
use ardoise_device::manager::Manager;
use ardoise_device::pages::{BLACKBOARD_PAGE, LibrarySink};
use ardoise_device::service::{
    Connection, LiveInput, LiveSink, LiveState, PageContext, PageStore, ServiceConfig,
    ServiceHooks, TabletOptions, TabletService, TabletStatus,
};
use ardoise_device::session::{DeviceError, DownloadedPage};
use ardoise_device::transport::{Connector, TabletKind};
use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::Point;
use ardoise_sim::{
    PenScript, SimBoard, SimBoardConfig, SimConfig, SimDevice, SimFactory, SimTablet,
    reference_board_pages,
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
    service: TabletService,
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
            service,
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
    run.service.sync_now();
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.error.is_none()));
    assert_eq!(status.last_sync.unwrap().deleted, 2);
    assert_eq!(board.stored_page_count(), 0);
    assert_eq!(notes(library.path()).len(), 3);
    run.service.shutdown();
}

#[test]
fn without_deletion_the_pages_come_on_request_only_and_still_leave_the_tablet() {
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
    run.service.sync_now();
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

/// The session against the simulated tablet, through the manager (its log describes every
/// frame in the blackboard's protocol).
fn session(board: &SimBoard) -> (BoardSession, Manager) {
    let connector = board.connector();
    let log = DiagnosticLog::in_memory("sim", connector.clock()).shared();
    let mut manager = Manager::new(Box::new(connector), std::env::temp_dir(), log);
    let session = manager.connect_blackboard(&board.config().address).unwrap();
    (session, manager)
}

#[test]
fn transfer_faults_stop_the_sync_and_nothing_is_lost() {
    let _report = common::report("blackboard: transfer faults");
    let board = board_with_pages(None);
    let (session, manager) = session(&board);
    let mut session = session.with_timeouts(Duration::from_secs(5), Duration::from_millis(500));

    // A transfer that stalls once is tried again and completes.
    board.stall_next_download(100);
    let mut kept = Vec::new();
    let summary = session
        .sync(&mut |page, _| {
            assert_eq!(page.bytes.len(), page.announced_size as usize);
            kept.push(page.timestamp);
            Ok(())
        })
        .unwrap();
    assert_eq!((summary.downloaded, summary.deleted, kept.len()), (3, 3, 3));
    assert_eq!(board.stored_page_count(), 0);

    // Twice: a typed error, the page stays, nothing is handed over.
    for page in reference_board_pages() {
        board.draw_page(&page);
    }
    let size = board.stored_pages()[0].bytes.len();
    board.stall_next_download(70);
    board.stall_next_download(140);
    let result = session.sync(&mut |_, _| panic!("an incomplete page is never handed over"));
    say!("stalled twice: {result:?}");
    assert_eq!(
        result,
        Err(DeviceError::TransferIncomplete {
            expected: u32::try_from(size).unwrap(),
            received: 140
        })
    );
    assert_eq!(board.stored_page_count(), 3);

    // A delete the tablet ignores: the sync stops instead of importing the page again.
    board.ignore_next_delete();
    let mut handed = 0;
    let result = session.sync(&mut |_, _| {
        handed += 1;
        Ok(())
    });
    assert!(
        matches!(result, Err(DeviceError::PageNotDeleted(_))),
        "{result:?}"
    );
    assert_eq!(handed, 1);
    assert_eq!(board.stored_page_count(), 3, "nothing lost");

    // The log names the frames in the blackboard's protocol, and nothing in it is unknown.
    let entries: Vec<String> = ardoise_device::log::lock(manager.log())
        .entries()
        .iter()
        .map(|e| e.info.clone())
        .collect();
    assert!(
        entries.iter().any(|e| e.starts_with("BOARD SET_TIME")),
        "{entries:?}"
    );
    assert!(entries.iter().any(|e| e.starts_with("BOARD DOWNLOAD")));
    assert!(entries.iter().any(|e| e.starts_with("BOARD DELETE")));
    assert!(entries.iter().any(|e| e.starts_with("BOARD_DATA")));
    assert!(
        !entries.iter().any(|e| e.contains("unknown")),
        "{entries:?}"
    );
}
