//! The tablet service (`ardoise_device::service`, milestone M4) against the simulated Slate:
//! registration, sync (deletion only once saved, no duplicate, cancellation), reconnection,
//! live mode (button, the four positions), and the service part of the live-mode latency.
//!
//! Virtual time everywhere, except the latency test (real time, about 3 s).
//! `cargo test -p ardoise-sim --test service -- --nocapture` prints the measures.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod common;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ardoise_core::library::Library;
use ardoise_core::settings::Orientation;
use ardoise_device::identity::{self, DEVICE_FORMAT, DeviceRecord, HostId};
use ardoise_device::pages::LibrarySink;
use ardoise_device::service::{
    Connection, LiveInput, LiveSink, LiveState, PageContext, PageStore, RegisterStep,
    ServiceConfig, TabletOptions, TabletService, TabletStatus,
};
use ardoise_device::session::{DeviceError, DownloadedPage};
use ardoise_proto::live::LiveRecord;
use ardoise_sim::realtime::LIVE_SCRIPT_SECONDS;
use ardoise_sim::{SimConfig, SimFactory, SimHooks, SimTablet, drawings};

const WAIT: Duration = Duration::from_secs(30);
const ID: [u8; 6] = [0x89, 0xea, 0x78, 0x88, 0xff, 0xcb];
/// 2026-10-24 12:00:00 UTC, the "now" of the notes written.
const NOW_MS: i64 = 1_792_843_200_000;

fn new_library() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    ardoise_core::library::init(dir.path()).unwrap();
    dir
}

/// The tablet was registered from this library before (on this or another PC).
fn registered_beforehand(tablet: &SimTablet, root: &Path) {
    let config = tablet.config();
    tablet.set_registered_id(Some(ID));
    let record = DeviceRecord {
        format: DEVICE_FORMAT,
        uuid: HostId(ID),
        address: config.address.clone(),
        name: config.name.clone(),
        protocol: "slate".into(),
        width: config.width,
        height: config.height,
        registered: "2026-10-24T14:00:00+02:00".into(),
        extra: Default::default(),
    };
    identity::save(root, &record).unwrap();
}

fn with_pages(tablet: &SimTablet, count: usize) {
    let config = tablet.config();
    for page in drawings::reference_pages(config.width, config.height)
        .iter()
        .take(count)
    {
        tablet.draw_page(page);
    }
}

/// The app's page store without the app: `LibrarySink` in the library root. `refuse` makes
/// the n-th page (1-based) fail; `before` runs before each page is stored.
fn library_store(refuse: Option<u16>, before: Option<Box<dyn Fn(u16) + Send>>) -> PageStore {
    let count = AtomicU16::new(0);
    Box::new(
        move |root: &Path, page: DownloadedPage, context: &PageContext| {
            let n = count.fetch_add(1, Ordering::SeqCst) + 1;
            if let Some(before) = &before {
                before(n);
            }
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

struct Running {
    service: TabletService,
    statuses: Receiver<TabletStatus>,
    /// Every status seen so far.
    seen: Vec<TabletStatus>,
}

impl Running {
    fn start(
        tablet: &SimTablet,
        root: &Path,
        options: TabletOptions,
        factory: SimFactory,
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
            Box::new(factory),
            Arc::new(SimHooks::with_delay(tablet.clone(), Duration::ZERO)),
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

    /// Virtual time: the simulated tablet answers at once.
    fn virtual_time(
        tablet: &SimTablet,
        root: &Path,
        options: TabletOptions,
        store: PageStore,
    ) -> Running {
        Self::start(
            tablet,
            root,
            options,
            SimFactory::virtual_time(tablet.clone()),
            store,
        )
    }

    /// Waits for a status accepted by `what` (seen now or later).
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

    fn connected(&mut self) -> TabletStatus {
        self.wait(|s| s.connection == Connection::Connected && s.sync.is_none())
    }
}

/// Hands every live input to the test.
struct ChannelSink(Sender<LiveInput>);

impl LiveSink for ChannelSink {
    fn input(&mut self, input: LiveInput) {
        let _ = self.0.send(input);
    }
}

/// Reads live inputs until `what` accepts one; returns how many records came before.
fn until(inputs: &Receiver<LiveInput>, what: impl Fn(&LiveInput) -> bool) -> (LiveInput, usize) {
    let mut records = 0;
    loop {
        let input = inputs.recv_timeout(WAIT).expect("live input in time");
        if what(&input) {
            return (input, records);
        }
        if let LiveInput::Records { records: r, .. } = &input {
            records += r.len();
        }
    }
}

fn notes(root: &Path) -> Vec<PathBuf> {
    let listing = Library::open(root).unwrap().list();
    assert!(listing.problems.is_empty(), "{:?}", listing.problems);
    listing.notes.into_iter().map(|n| n.path).collect()
}

#[test]
fn registration_then_sync_of_three_pages_makes_three_notes_and_empties_the_tablet() {
    let _report = common::report("service: registration, then sync of 3 pages");
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 3);
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    run.wait(|s| s.connection == Connection::NotRegistered);
    run.service.register();
    let status = run.wait(|s| s.last_sync.is_some());

    // The wizard went through every step, in order.
    let mut steps: Vec<RegisterStep> = run.seen.iter().filter_map(|s| s.register_step).collect();
    steps.dedup();
    assert_eq!(
        steps,
        [
            RegisterStep::HoldButton,
            RegisterStep::Searching,
            RegisterStep::Connecting,
            RegisterStep::PressButton,
            RegisterStep::Done,
        ]
    );
    let progress: Vec<_> = run
        .seen
        .iter()
        .filter_map(|s| s.sync)
        .map(|p| (p.current, p.total, p.saved))
        .collect();
    assert!(progress.contains(&(3, 3, 3)), "{progress:?}");
    assert!(run.seen.iter().any(|s| s.connection == Connection::Syncing));

    let last = status.last_sync.clone().unwrap();
    assert_eq!(last.saved.len(), 3);
    assert_eq!(last.deleted, 3);
    assert_eq!(status.error, None);
    assert_eq!(status.device.as_ref().unwrap().stored_pages, 0);
    assert_eq!(status.registered.as_ref().unwrap().name, "Bamboo Slate");
    assert_eq!(
        tablet.registered_id().map(HostId),
        Some(identity::load(library.path()).unwrap().unwrap().uuid)
    );

    // Three readable notes in the library, the tablet empty.
    let written = notes(library.path());
    assert_eq!(written.len(), 3);
    let opened = Library::open(library.path()).unwrap();
    let mut listed: Vec<String> = written.iter().map(|p| p.display().to_string()).collect();
    let mut saved = last.saved.clone();
    listed.sort();
    saved.sort();
    assert_eq!(listed, saved);
    for path in &written {
        let note = opened.read(path).unwrap().file;
        assert!(!note.note.strokes.is_empty());
        assert!(note.original.is_some());
    }
    assert_eq!(tablet.stored_page_count(), 0);
    say!(
        "registered {} ({}), synced {} pages -> {} notes, {} deleted from the tablet",
        status.device.as_ref().unwrap().name,
        status.device.as_ref().unwrap().model,
        3,
        written.len(),
        last.deleted
    );
    run.service.shutdown();
}

#[test]
fn the_registration_can_be_cancelled_while_searching() {
    let library = new_library();
    // Real time and nobody to hold the button: the search really waits.
    let tablet = SimTablet::real_time(SimConfig::slate_large());
    let (sender, statuses) = mpsc::channel();
    let sender = Mutex::new(sender);
    let service = TabletService::start(
        ServiceConfig {
            library: Some(library.path().to_owned()),
            identity_root: Some(library.path().to_owned()),
            options: TabletOptions::default(),
            simulated: true,
        },
        Box::new(SimFactory::real_time(tablet.clone())),
        Arc::new(ardoise_device::service::NoHooks),
        Box::new(move |status| {
            let _ = sender.lock().unwrap().send(status.clone());
        }),
        library_store(None, None),
    );
    let mut run = Running {
        service,
        statuses,
        seen: Vec::new(),
    };
    run.wait(|s| s.connection == Connection::NotRegistered);
    run.service.register();
    run.wait(|s| s.register_step == Some(RegisterStep::Searching));
    let asked = Instant::now();
    run.service.cancel();
    let status = run.wait(|s| s.register_step.is_none());
    // Noticed at the next advertisement, not at the end of the 5 s scan slice.
    assert!(
        asked.elapsed() < Duration::from_secs(2),
        "{:?}",
        asked.elapsed()
    );
    assert_eq!(status.connection, Connection::NotRegistered);
    assert_eq!(status.error, None);
    assert_eq!(identity::load(library.path()).unwrap(), None);
    run.service.shutdown();
}

#[test]
fn a_page_that_cannot_be_saved_stays_on_the_tablet() {
    let _report = common::report("service: a page that cannot be saved");
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(Some(2), None),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.clone().unwrap();
    assert_eq!((last.saved.len(), last.deleted), (1, 1));
    let error = status.error.clone().unwrap();
    assert_eq!(error.code, "libraryAccess");
    assert!(error.message.contains("the disk is full"), "{error:?}");
    // The page that failed and the one after it are still on the tablet.
    assert_eq!(tablet.stored_page_count(), 2);
    assert_eq!(notes(library.path()).len(), 1);
    assert_eq!(status.device.as_ref().unwrap().stored_pages, 2);
    // The failure's log came back to the library.
    let logs = library.path().join("_ardoise").join("diagnostics");
    let files: Vec<_> = std::fs::read_dir(&logs).unwrap().flatten().collect();
    assert_eq!(files.len(), 1);
    let text = std::fs::read_to_string(files[0].path()).unwrap();
    assert!(text.contains("the disk is full"));
    say!(
        "page 2 refused by the store: 1 note, {} pages left on the tablet, error {}, log {}",
        tablet.stored_page_count(),
        error.code,
        files[0].file_name().to_string_lossy()
    );
    run.service.shutdown();
}

#[test]
fn a_page_finished_during_the_sync_is_synced_on_the_same_connection() {
    // The first hardware session (2026-11-02) left a page behind: counted after the sync, then
    // refused (`ec` in paper mode). It is now read at once, on the same connection.
    let _report = common::report("service: a page finished during the sync");
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
    // While page 2 is stored, the user finishes another page on the tablet (a new drawing:
    // the reference pages moved aside).
    let drawing = tablet.clone();
    let before = move |n: u16| {
        if n == 2 {
            let config = drawing.config();
            let mut page = drawings::square_and_circle(config.width, config.height);
            for stroke in &mut page {
                for point in &mut stroke.points {
                    point.x -= 1_000;
                }
            }
            drawing.draw_page(&page);
        }
    };
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, Some(Box::new(before))),
    );
    let first = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.saved.len() == 3));
    assert_eq!(first.last_sync.unwrap().error, None);
    let second = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.saved.len() == 1));
    let last = second.last_sync.clone().unwrap();
    assert_eq!((last.deleted, last.error), (1, None));
    assert_eq!(second.error, None);
    assert_eq!(tablet.stored_page_count(), 0);
    assert_eq!(notes(library.path()).len(), 4);
    assert_eq!(tablet.stats().connections, 1, "the same connection");
    assert_eq!(second.device.unwrap().stored_pages, 0);
    say!("3 pages synced, a 4th finished meanwhile: synced right after, same connection");
    run.service.shutdown();
}

#[test]
fn a_sync_refused_by_the_tablet_is_shown_as_failed_and_its_log_ends_with_the_disconnection() {
    let _report = common::report("service: a sync refused by the tablet");
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 2);
    registered_beforehand(&tablet, library.path());
    // `ec` refused at the connection (then again from idle mode) and once more by the sync.
    tablet.refuse_next_requests(
        ardoise_proto::message::opcode::FILE_TRANSFER_REPORTING_TYPE,
        3,
    );
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    let failed = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.error.is_some()));
    let last = failed.last_sync.clone().unwrap();
    assert_eq!((last.saved.len(), last.deleted), (0, 0));
    let error = last.error.unwrap();
    assert_eq!(error.code, "syncRefused");
    assert!(error.message.contains("InvalidState"), "{error:?}");
    assert_eq!(failed.error.as_ref().unwrap().code, "syncRefused");
    assert_eq!(tablet.stored_page_count(), 2, "nothing deleted");

    // The next connection syncs: the failure is replaced by the success.
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.error.is_none()));
    assert_eq!(status.last_sync.unwrap().saved.len(), 2);
    assert_eq!(status.error, None);
    assert_eq!(tablet.stored_page_count(), 0);
    assert_eq!(tablet.stats().connections, 2);
    run.service.shutdown();

    // The failed connection's log: why, then the disconnection and the end line.
    let logs = library.path().join("_ardoise").join("diagnostics");
    let files: Vec<_> = std::fs::read_dir(&logs).unwrap().flatten().collect();
    assert_eq!(files.len(), 1, "only the failure is written");
    let (_, entries) = ardoise_device::log::read_log(&files[0].path()).unwrap();
    let infos: Vec<&str> = entries.iter().map(|e| e.info.as_str()).collect();
    assert!(
        infos
            .iter()
            .any(|i| i.starts_with("sync failed before the first page")),
        "{infos:#?}"
    );
    assert_eq!(
        infos[infos.len() - 2..],
        ["disconnect", "end of this log: closed after a failed sync"],
        "{infos:#?}"
    );
    say!(
        "sync refused: shown ({}), {} pages kept, log ends with the disconnection; next connection synced",
        error.code,
        2
    );
}

#[test]
fn a_battery_level_announced_by_the_tablet_is_shown() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    registered_beforehand(&tablet, library.path());
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    let status = run.connected();
    assert_eq!(status.device.unwrap().battery_percent, 87);
    // As the real Slate A4 does, about once a minute while discharging: `f1 02 29 00`.
    tablet.set_battery(41, false);
    let status = run.wait(|s| s.device.as_ref().is_some_and(|d| d.battery_percent == 41));
    assert_eq!(status.connection, Connection::Connected);
    run.service.shutdown();
}

#[test]
fn without_deletion_a_page_is_imported_once() {
    let _report = common::report("service: sync without deletion");
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
    let options = TabletOptions {
        delete_after_import: false,
        ..TabletOptions::default()
    };
    let mut run =
        Running::virtual_time(&tablet, library.path(), options, library_store(None, None));
    let status = run.wait(|s| s.last_sync.is_some());
    // Only the oldest page can be read without deleting it.
    assert_eq!(status.last_sync.unwrap().saved.len(), 1);
    assert_eq!(tablet.stored_page_count(), 3);
    assert_eq!(notes(library.path()).len(), 1);

    // Synced again: the page is recognised, nothing is written.
    run.seen.clear();
    run.service.sync_now();
    run.wait(|s| s.connection == Connection::Syncing);
    let status = run.connected();
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted), (0, 0));
    assert_eq!(notes(library.path()).len(), 1);

    // Its note deleted by the user: not imported again either.
    let opened = Library::open(library.path()).unwrap();
    opened.trash(&notes(library.path())[0]).unwrap();
    run.service.sync_now();
    run.wait(|s| s.connection == Connection::Syncing);
    run.connected();
    assert!(notes(library.path()).is_empty());
    assert_eq!(tablet.stored_page_count(), 3);
    say!("3 syncs without deletion: 1 note written once, nothing imported twice");
    run.service.shutdown();
}

#[test]
fn a_page_already_saved_is_deleted_without_a_second_note() {
    // The link was lost after the save and before the deletion: the next sync finds the note.
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 2);
    registered_beforehand(&tablet, library.path());
    let keep = TabletOptions {
        delete_after_import: false,
        auto_sync: true,
        ..TabletOptions::default()
    };
    let mut run = Running::virtual_time(&tablet, library.path(), keep, library_store(None, None));
    run.wait(|s| s.last_sync.is_some());
    assert_eq!(notes(library.path()).len(), 1);
    run.service.set_options(TabletOptions::default());
    run.service.sync_now();
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.deleted == 2));
    assert_eq!(
        status.last_sync.unwrap().saved.len(),
        1,
        "only the new page"
    );
    assert_eq!(notes(library.path()).len(), 2);
    assert_eq!(tablet.stored_page_count(), 0);
    run.service.shutdown();
}

#[test]
fn cancel_stops_the_sync_between_pages() {
    let _report = common::report("service: cancelled sync");
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
    // The first page waits for the test, which cancels meanwhile.
    let (reached, page_reached) = mpsc::channel();
    let (go, wait_go) = mpsc::channel::<()>();
    let wait_go = Mutex::new(wait_go);
    let reached = Mutex::new(reached);
    let before = move |n: u16| {
        if n == 1 {
            reached.lock().unwrap().send(()).unwrap();
            wait_go.lock().unwrap().recv_timeout(WAIT).unwrap();
        }
    };
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, Some(Box::new(before))),
    );
    page_reached.recv_timeout(WAIT).unwrap();
    run.service.cancel();
    go.send(()).unwrap();
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    // The page being saved is saved and deleted; the others stay.
    assert_eq!((last.saved.len(), last.deleted), (1, 1));
    assert_eq!(status.error, None);
    assert_eq!(tablet.stored_page_count(), 2);
    assert_eq!(notes(library.path()).len(), 1);
    say!("cancel during page 1 of 3: 1 note, 1 deleted, 2 left on the tablet");
    run.service.shutdown();
}

#[test]
fn a_sync_asked_while_away_runs_when_the_tablet_comes() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 2);
    registered_beforehand(&tablet, library.path());
    tablet.power_off();
    let options = TabletOptions {
        auto_sync: false,
        ..TabletOptions::default()
    };
    let mut run =
        Running::virtual_time(&tablet, library.path(), options, library_store(None, None));
    run.wait(|s| s.connection == Connection::Searching);
    run.service.sync_now();
    let status = run.wait(|s| s.pending == Some(ardoise_device::service::Pending::Sync));
    assert_eq!(status.connection, Connection::Searching);
    tablet.power_on();
    let status = run.wait(|s| s.last_sync.is_some());
    assert_eq!(status.last_sync.unwrap().saved.len(), 2);
    assert_eq!(status.pending, None);
    run.service.shutdown();
}

#[test]
fn live_mode_goes_on_after_a_lost_link_and_reports_the_button() {
    let _report = common::report("service: live mode, button, lost link");
    let library = new_library();
    let tablet = SimTablet::slate_large();
    registered_beforehand(&tablet, library.path());
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    run.connected();
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    let config = tablet.config();
    assert_eq!(
        inputs.recv_timeout(WAIT).unwrap(),
        LiveInput::Started {
            width: config.width,
            height: config.height,
            orientation: Orientation::Landscape
        }
    );
    assert_eq!(
        inputs.recv_timeout(WAIT).unwrap(),
        LiveInput::State(LiveState::Live)
    );
    run.wait(|s| s.connection == Connection::Live);
    let (_, before) = until(&inputs, |i| matches!(i, LiveInput::Records { .. }));
    assert_eq!(before, 0);

    // The button while the pen writes.
    assert!(tablet.is_streaming());
    tablet.press_button();
    let (_, records) = until(&inputs, |i| *i == LiveInput::Button);
    assert_eq!(tablet.stats().live_buttons, 1);

    // The tablet drops the link: reconnection, then live mode again.
    tablet.disconnect_after(Duration::ZERO);
    let (_, more) = until(&inputs, |i| *i == LiveInput::State(LiveState::Reconnecting));
    until(&inputs, |i| *i == LiveInput::State(LiveState::Live));
    let (_, _) = until(&inputs, |i| matches!(i, LiveInput::Records { .. }));
    assert_eq!(tablet.stats().connections, 2);
    run.service.stop_live();
    until(&inputs, |i| *i == LiveInput::State(LiveState::Stopped));
    // Statuses of the reconnection (connected, live pending) come first.
    let status = run.wait(|s| s.connection == Connection::Connected && s.pending.is_none());
    assert_eq!(status.error, None);
    say!(
        "live: {} records before the button, {} more before the lost link; reconnected and live again",
        records,
        more
    );
    run.service.shutdown();
}

#[test]
fn flipped_landscape_turns_synced_pages_and_live_points() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 1);
    registered_beforehand(&tablet, library.path());
    let options = TabletOptions {
        orientation: Orientation::LandscapeFlipped,
        ..TabletOptions::default()
    };
    let mut run =
        Running::virtual_time(&tablet, library.path(), options, library_store(None, None));
    run.wait(|s| s.last_sync.is_some());
    let config = tablet.config();
    let (w, h) = (config.width as f32, config.height as f32);

    let path = &notes(library.path())[0];
    let note = Library::open(library.path())
        .unwrap()
        .read(path)
        .unwrap()
        .file;
    let drawn = drawings::square_and_circle(config.width, config.height)[0].points[0];
    let first = note.note.strokes[0].points[0];
    assert_eq!(
        (first.x, first.y),
        (w - f32::from(drawn.x), h - f32::from(drawn.y))
    );

    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    let (records, _) = until(&inputs, |i| matches!(i, LiveInput::Records { .. }));
    let LiveInput::Records { records, .. } = records else {
        unreachable!()
    };
    let script = drawings::live_script(LIVE_SCRIPT_SECONDS, config.width, config.height);
    let expected = script.points().next().unwrap();
    assert_eq!(
        records[0],
        LiveRecord::Pen(ardoise_proto::stroke::Point {
            x: (config.width - u32::from(expected.x)) as u16,
            y: (config.height - u32::from(expected.y)) as u16,
            pressure: expected.pressure,
        })
    );
    run.service.stop_live();
    run.service.shutdown();
}

/// Where the tablet's own point `(x, y)` lands for a position: written out here on purpose, apart
/// from `pages::orient`, from the geometry of a tablet turned on the desk (`w` x `h` = its area).
fn expected_point(orientation: Orientation, (x, y): (u32, u32), (w, h): (u32, u32)) -> (u32, u32) {
    match orientation {
        Orientation::Landscape => (x, y),
        Orientation::LandscapeFlipped => (w - x, h - y),
        // A quarter turn clockwise: the tablet's top edge is on the right.
        Orientation::PortraitLeft => (h - y, x),
        // A quarter turn anticlockwise: the tablet's top edge is on the left.
        Orientation::PortraitRight => (y, w - x),
    }
}

#[test]
fn portrait_positions_make_portrait_notes_and_turn_live_points() {
    for orientation in [Orientation::PortraitLeft, Orientation::PortraitRight] {
        let library = new_library();
        let tablet = SimTablet::slate_large();
        with_pages(&tablet, 1);
        registered_beforehand(&tablet, library.path());
        let options = TabletOptions {
            orientation,
            ..TabletOptions::default()
        };
        let mut run =
            Running::virtual_time(&tablet, library.path(), options, library_store(None, None));
        run.wait(|s| s.last_sync.is_some());
        let config = tablet.config();
        let area = (config.width, config.height);

        let path = &notes(library.path())[0];
        let note = Library::open(library.path())
            .unwrap()
            .read(path)
            .unwrap()
            .file;
        // The page is portrait: the tablet's width and height swapped.
        assert_eq!(
            (note.note.page.width, note.note.page.height),
            (config.height, config.width),
            "{orientation:?}"
        );
        let drawn = drawings::square_and_circle(config.width, config.height)[0].points[0];
        let first = note.note.strokes[0].points[0];
        let (x, y) = expected_point(orientation, (u32::from(drawn.x), u32::from(drawn.y)), area);
        assert_eq!((first.x, first.y), (x as f32, y as f32), "{orientation:?}");
        for stroke in &note.note.strokes {
            for p in &stroke.points {
                assert!(p.x >= 0.0 && p.x <= config.height as f32, "{p:?}");
                assert!(p.y >= 0.0 && p.y <= config.width as f32, "{p:?}");
            }
        }

        let (sender, inputs) = mpsc::channel();
        run.service.start_live(Box::new(ChannelSink(sender)));
        let (started, _) = until(&inputs, |i| matches!(i, LiveInput::Started { .. }));
        assert_eq!(
            started,
            LiveInput::Started {
                width: config.width,
                height: config.height,
                orientation
            }
        );
        let (records, _) = until(&inputs, |i| matches!(i, LiveInput::Records { .. }));
        let LiveInput::Records { records, .. } = records else {
            unreachable!()
        };
        let script = drawings::live_script(LIVE_SCRIPT_SECONDS, config.width, config.height);
        let first_point = script.points().next().unwrap();
        let (x, y) = expected_point(
            orientation,
            (u32::from(first_point.x), u32::from(first_point.y)),
            area,
        );
        assert_eq!(
            records[0],
            LiveRecord::Pen(ardoise_proto::stroke::Point {
                x: x as u16,
                y: y as u16,
                pressure: first_point.pressure,
            }),
            "{orientation:?}"
        );
        run.service.stop_live();
        run.service.shutdown();
    }
}

#[test]
fn switching_the_position_during_live_mode_turns_the_points_that_follow() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    registered_beforehand(&tablet, library.path());
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    run.connected();
    let config = tablet.config();
    let area = (config.width, config.height);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    let (_, _) = until(&inputs, |i| matches!(i, LiveInput::Records { .. }));

    // The script's own points: every record received, turned back, must be one of them.
    let script = drawings::live_script(LIVE_SCRIPT_SECONDS, config.width, config.height);
    let known: std::collections::HashSet<(u32, u32)> = script
        .points()
        .map(|p| (u32::from(p.x), u32::from(p.y)))
        .collect();

    let position = Orientation::PortraitRight;
    run.service.set_options(TabletOptions {
        orientation: position,
        ..TabletOptions::default()
    });
    let (switched, _) = until(&inputs, |i| matches!(i, LiveInput::Reoriented { .. }));
    assert_eq!(
        switched,
        LiveInput::Reoriented {
            width: config.width,
            height: config.height,
            orientation: position
        }
    );
    let mut turned = 0;
    while turned < 50 {
        let (input, _) = until(&inputs, |i| matches!(i, LiveInput::Records { .. }));
        let LiveInput::Records { records, .. } = input else {
            unreachable!()
        };
        for record in records {
            if let LiveRecord::Pen(p) = record {
                // Page is `height` wide and `width` tall now; undo the anticlockwise turn
                // (x' = y, y' = w - x).
                assert!(
                    u32::from(p.x) <= area.1 && u32::from(p.y) <= area.0,
                    "{p:?}"
                );
                let (x, y) = (area.0 - u32::from(p.y), u32::from(p.x));
                assert!(
                    known.contains(&(x, y)),
                    "{p:?} does not come from the script"
                );
                turned += 1;
            }
        }
    }
    // The same position again changes nothing for the sink.
    run.service.set_options(TabletOptions {
        orientation: position,
        ..TabletOptions::default()
    });
    run.service.stop_live();
    run.service.shutdown();
}

#[test]
fn rename_set_the_clock_and_forget() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    registered_beforehand(&tablet, library.path());
    tablet.set_clock_offset(-3600);
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    run.connected();
    run.service.rename("Classe 3eB").unwrap();
    let status = run.service.status();
    assert_eq!(status.device.unwrap().name, "Classe 3eB");
    assert_eq!(status.registered.unwrap().name, "Classe 3eB");
    assert_eq!(tablet.config().name, "Classe 3eB");
    assert_eq!(
        identity::load(library.path()).unwrap().unwrap().name,
        "Classe 3eB"
    );
    assert_eq!(
        run.service.rename("Tablette é"),
        Err(DeviceError::InvalidName("Tablette é".into()))
    );
    // Connecting already set the clock; setting it again leaves no drift.
    assert_eq!(run.service.set_time(), Ok(0));

    run.service.forget().unwrap();
    assert_eq!(identity::load(library.path()).unwrap(), None);
    let status = run.wait(|s| s.connection == Connection::NotRegistered);
    assert_eq!(status.registered, None);
    assert!(!tablet.is_connected());
    assert_eq!(run.service.set_time(), Err(DeviceError::NoTabletRegistered));
    run.service.shutdown();
}

/// Latency of the service (SPEC §4.2): for every record, the time from the notification's
/// receipt (stamped by `Session`) to the call of the `LiveSink`, with the simulated tablet in
/// real time (200 samples per second).
#[test]
fn latency_of_live_mode_through_the_service_in_real_time() {
    let _report = common::report("service: live-mode latency (real time)");
    let library = new_library();
    let tablet = SimTablet::real_time(SimConfig::slate_large());
    registered_beforehand(&tablet, library.path());
    let mut run = Running::start(
        &tablet,
        library.path(),
        TabletOptions::default(),
        SimFactory::real_time(tablet.clone()),
        library_store(None, None),
    );
    run.connected();

    struct Measure(Arc<Mutex<Vec<f64>>>);
    impl LiveSink for Measure {
        fn input(&mut self, input: LiveInput) {
            if let LiveInput::Records { records, received } = input {
                let now = SystemTime::now();
                let ms = now
                    .duration_since(received)
                    .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
                let pens = records
                    .iter()
                    .filter(|r| matches!(r, LiveRecord::Pen(_)))
                    .count();
                self.0.lock().unwrap().extend(std::iter::repeat_n(ms, pens));
            }
        }
    }
    let samples = Arc::new(Mutex::new(Vec::new()));
    let started = Instant::now();
    run.service.start_live(Box::new(Measure(samples.clone())));
    run.wait(|s| s.connection == Connection::Live);
    std::thread::sleep(Duration::from_secs(3));
    run.service.stop_live();
    run.connected();
    let elapsed = started.elapsed();
    let mut samples = samples.lock().unwrap().clone();
    samples.sort_by(f64::total_cmp);
    let count = samples.len();
    assert!(count > 300, "only {count} samples in {elapsed:?}");
    let median = samples[count / 2];
    let p95 = samples[(count * 95 / 100).min(count - 1)];
    let max = samples[count - 1];
    let rate = count as f64 / elapsed.as_secs_f64();
    say!(
        "latency (receipt -> LiveSink): {count} samples in {:.1} s ({rate:.0}/s), median {median:.3} ms, p95 {p95:.3} ms, max {max:.3} ms",
        elapsed.as_secs_f64()
    );
    println!(
        "service latency: samples={count} median_ms={median:.3} p95_ms={p95:.3} max_ms={max:.3}"
    );
    assert!(median < 30.0, "median {median} ms");
    run.service.shutdown();
}
