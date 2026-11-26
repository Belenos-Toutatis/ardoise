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
use ardoise_device::pages::{LibrarySink, is_partial_page};
use ardoise_device::service::{
    Connection, ExportFailed, ExportFailure, LiveInput, LiveSink, LiveState, PageContext,
    PageStore, Pending, RegisterStep, ServiceConfig, TabletOptions, TabletService, TabletStatus,
};
use ardoise_device::session::{DeviceError, DownloadedPage};
use ardoise_device::transport::TabletKind;
use ardoise_proto::live::LiveRecord;
use ardoise_sim::realtime::LIVE_SCRIPT_SECONDS;
use ardoise_sim::{
    SimBoard, SimBoardConfig, SimConfig, SimFactory, SimHooks, SimTablet, drawings,
    reference_board_pages,
};

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

/// The file of the first reference page followed by a packet that does not decode: a DELTA with
/// the axis mask 01, which no device is known to send (protocol.md §6.4).
fn unreadable_page() -> Vec<u8> {
    let scratch = SimTablet::slate_large();
    with_pages(&scratch, 1);
    let mut bytes = scratch.stored_pages()[0].bytes.clone();
    bytes.extend_from_slice(&[0x04, 0x01]);
    assert!(ardoise_proto::stroke::decode(&bytes).is_err());
    bytes
}

/// Reference pages 2 to `count`.
fn with_later_pages(tablet: &SimTablet, count: usize) {
    let config = tablet.config();
    for page in drawings::reference_pages(config.width, config.height)
        .iter()
        .take(count)
        .skip(1)
    {
        tablet.draw_page(page);
    }
}

/// The notes of the library made from only part of a page.
fn partial_notes(root: &Path) -> Vec<(PathBuf, ardoise_core::format::NoteFile)> {
    let library = Library::open(root).unwrap();
    notes(root)
        .into_iter()
        .map(|path| {
            let file = library.read(&path).unwrap().file;
            (path, file)
        })
        .filter(|(_, file)| is_partial_page(file))
        .collect()
}

fn stem(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().into_owned()
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
            sink.store_page(page)
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
            rescue_fallback: None,
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
            rescue_fallback: None,
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
    run.service.sync_now(false).unwrap();
    run.wait(|s| s.connection == Connection::Syncing);
    let status = run.connected();
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted), (0, 0));
    assert_eq!(notes(library.path()).len(), 1);

    // Its note deleted by the user: not imported again either.
    let opened = Library::open(library.path()).unwrap();
    opened.trash(&notes(library.path())[0]).unwrap();
    run.service.sync_now(false).unwrap();
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
    run.service.sync_now(false).unwrap();
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
fn a_page_that_cannot_be_decoded_becomes_a_note_and_the_sync_goes_on() {
    let _report = common::report("service: a page that cannot be decoded");
    let library = new_library();
    let tablet = SimTablet::slate_large();
    let raw = unreadable_page();
    tablet.store_raw_page(raw.clone());
    with_later_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    let status = run.wait(|s| s.last_sync.is_some());

    // Three notes, the tablet empty, on one connection.
    let last = status.last_sync.clone().unwrap();
    assert_eq!((last.saved.len(), last.deleted), (3, 3));
    assert_eq!(tablet.stored_page_count(), 0);
    let written = notes(library.path());
    assert_eq!(written.len(), 3);

    // The note of the first page: what could be read (here every stroke, the bad packet being the
    // last), all its bytes, and the mark. The other notes are whole.
    let partial = partial_notes(library.path());
    assert_eq!(partial.len(), 1);
    let (path, file) = &partial[0];
    assert_eq!(file.original.as_deref(), Some(raw.as_slice()));
    assert!(!file.note.strokes.is_empty());
    assert_eq!(
        file.note.strokes.len(),
        ardoise_proto::stroke::decode(&raw[..raw.len() - 2])
            .unwrap()
            .strokes
            .iter()
            .filter(|s| !s.points.is_empty())
            .count()
    );
    assert!(last.saved.contains(&path.display().to_string()));

    // The user is told, with the note's name.
    let error = last.error.clone().unwrap();
    assert_eq!(error.code, "pagePartial");
    assert_eq!(error.message, stem(path));
    assert_eq!(status.error, Some(error.clone()));
    assert_eq!(status.device.as_ref().unwrap().stored_pages, 0);

    // Nothing to do the second time: the error goes.
    run.seen.clear();
    run.service.sync_now(false).unwrap();
    run.wait(|s| s.connection == Connection::Syncing);
    let status = run.connected();
    let again = status.last_sync.unwrap();
    assert_eq!((again.saved.len(), again.deleted), (0, 0));
    assert_eq!((again.error, status.error), (None, None));
    assert_eq!(notes(library.path()).len(), 3);
    assert_eq!(tablet.stats().connections, 1);
    run.service.shutdown();

    // The log of the connection is kept: why, and the note's place.
    let logs = library.path().join("_ardoise").join("diagnostics");
    let files: Vec<_> = std::fs::read_dir(&logs).unwrap().flatten().collect();
    assert_eq!(files.len(), 1);
    let (_, entries) = ardoise_device::log::read_log(&files[0].path()).unwrap();
    let infos: Vec<&str> = entries.iter().map(|e| e.info.as_str()).collect();
    let line = infos
        .iter()
        .find(|i| i.starts_with("page 1 cannot be read whole"))
        .unwrap_or_else(|| panic!("{infos:#?}"));
    assert!(line.contains("reserved axis mask 01"), "{line}");
    assert!(line.contains(&path.display().to_string()), "{line}");
    assert!(
        infos
            .iter()
            .any(|i| i.starts_with("sync finished: 3 of 3 pages saved, 3 deleted; only part")),
        "{infos:#?}"
    );
    say!(
        "page 1 of 3 unreadable: 3 notes, tablet empty, told {} ({}), log kept",
        error.code,
        error.message
    );
}

/// Bytes that look like another tablet's page, or like nothing: an empty note that holds them.
#[test]
fn a_page_without_a_readable_stroke_becomes_an_empty_note_that_holds_its_bytes() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    // Whole 7-byte records of the blackboard, as far as the bytes can tell.
    let raw = vec![0xa1; 21];
    tablet.store_raw_page(raw.clone());
    with_later_pages(&tablet, 2);
    registered_beforehand(&tablet, library.path());
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted), (2, 2));
    assert_eq!(last.error.unwrap().code, "pagePartial");
    let partial = partial_notes(library.path());
    assert_eq!(partial.len(), 1);
    let file = &partial[0].1;
    assert!(file.note.strokes.is_empty());
    assert_eq!(file.original.as_deref(), Some(raw.as_slice()));
    assert_eq!(
        (file.note.page.width, file.note.page.height),
        (29_700, 21_600),
        "the Slate's page"
    );
    assert_eq!(tablet.stored_page_count(), 0);
    run.service.shutdown();
}

#[test]
fn without_deletion_a_page_read_in_part_is_imported_once() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    let raw = unreadable_page();
    tablet.store_raw_page(raw.clone());
    with_later_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
    let keep = TabletOptions {
        delete_after_import: false,
        ..TabletOptions::default()
    };
    let mut run = Running::virtual_time(&tablet, library.path(), keep, library_store(None, None));
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted), (1, 0));
    assert_eq!(last.error.unwrap().code, "pagePartial");
    assert_eq!(tablet.stored_page_count(), 3);
    assert_eq!(tablet.stored_pages()[0].bytes, raw);

    // Synced again, the note edited meanwhile: the page is recognised, nothing is written, the
    // edits and the mark stay.
    let opened = Library::open(library.path()).unwrap();
    let path = notes(library.path())[0].clone();
    let mut loaded = opened.read(&path).unwrap();
    loaded.file.note.title = "Corrigée".into();
    loaded.file.note.strokes.pop();
    opened.save(&path, &loaded.file, &loaded.stamp).unwrap();
    run.service.sync_now(false).unwrap();
    run.wait(|s| s.connection == Connection::Syncing);
    let status = run.connected();
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted, last.error), (0, 0, None));
    let partial = partial_notes(library.path());
    assert_eq!(partial.len(), 1);
    assert_eq!(partial[0].1.note.title, "Corrigée");
    assert_eq!(partial[0].1.note.strokes, loaded.file.note.strokes);
    assert_eq!(partial[0].1.original.as_deref(), Some(raw.as_slice()));

    // Deletion on: the page goes without a second note, and the later pages come.
    run.service.set_options(TabletOptions::default());
    run.service.sync_now(false).unwrap();
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.deleted == 3));
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.error), (2, None));
    assert_eq!(notes(library.path()).len(), 3);
    assert_eq!(tablet.stored_page_count(), 0);
    run.service.shutdown();
}

/// Its note in the trash, deletion off: not imported again (the rule of any page).
#[test]
fn a_page_read_in_part_whose_note_was_deleted_is_not_imported_again_without_deletion() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    tablet.store_raw_page(unreadable_page());
    registered_beforehand(&tablet, library.path());
    let keep = TabletOptions {
        delete_after_import: false,
        ..TabletOptions::default()
    };
    let mut run = Running::virtual_time(&tablet, library.path(), keep, library_store(None, None));
    run.wait(|s| s.last_sync.is_some());
    let opened = Library::open(library.path()).unwrap();
    opened.trash(&notes(library.path())[0]).unwrap();
    run.service.sync_now(false).unwrap();
    run.wait(|s| s.connection == Connection::Syncing);
    run.connected();
    assert!(notes(library.path()).is_empty());
    assert_eq!(tablet.stored_page_count(), 1);
    run.service.shutdown();
}

/// A failure that stops the sync is said first; the page read in part is said at the end of the
/// next sync, when nothing else is.
#[test]
fn a_page_read_in_part_never_hides_a_failure_and_is_said_afterwards() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    tablet.store_raw_page(unreadable_page());
    with_later_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(Some(2), None),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted), (1, 1));
    assert_eq!(last.error.unwrap().code, "libraryAccess");
    assert_eq!(tablet.stored_page_count(), 2);
    let partial = partial_notes(library.path());
    assert_eq!(partial.len(), 1);

    run.service.sync_now(false).unwrap();
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.deleted == 2));
    let error = status.last_sync.unwrap().error.unwrap();
    assert_eq!(
        (error.code.as_str(), error.message),
        ("pagePartial", stem(&partial[0].0))
    );
    assert_eq!(notes(library.path()).len(), 3);

    // Said once.
    run.service.sync_now(false).unwrap();
    run.wait(|s| s.connection == Connection::Syncing);
    assert_eq!(run.connected().last_sync.unwrap().error, None);
    run.service.shutdown();
}

/// The link lost right after the note was written and before the deletion: the sync that resumes
/// deletes the page without a second note, and says it.
#[test]
fn a_page_read_in_part_is_said_by_the_sync_that_resumes_after_a_lost_link() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    tablet.store_raw_page(unreadable_page());
    with_later_pages(&tablet, 2);
    registered_beforehand(&tablet, library.path());
    let dropping = tablet.clone();
    let before = move |n: u16| {
        if n == 1 {
            dropping.disconnect_after(Duration::ZERO);
        }
    };
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, Some(Box::new(before))),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    assert_eq!(last.error.unwrap().code, "tabletNotConnected");
    assert_eq!((last.saved.len(), last.deleted), (1, 0));
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.deleted == 2));
    let last = status.last_sync.unwrap();
    assert_eq!(last.saved.len(), 1, "no second note of the first page");
    assert_eq!(last.error.unwrap().code, "pagePartial");
    assert_eq!(notes(library.path()).len(), 2);
    assert_eq!(partial_notes(library.path()).len(), 1);
    assert_eq!(tablet.stored_page_count(), 0);
    run.service.shutdown();
}

/// A page finished meanwhile is synced at once, as after any other sync, and that second sync
/// says the page read in part again: it would otherwise be shown for an instant.
#[test]
fn the_sync_of_a_page_finished_meanwhile_still_says_the_page_read_in_part() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    tablet.store_raw_page(unreadable_page());
    registered_beforehand(&tablet, library.path());
    let drawing = tablet.clone();
    let before = move |n: u16| {
        if n == 1 {
            with_later_pages(&drawing, 2);
        }
    };
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, Some(Box::new(before))),
    );
    let first = run.wait(|s| s.last_sync.is_some()).last_sync.unwrap();
    assert_eq!(first.saved.len(), 1);
    assert_eq!(first.error.clone().unwrap().code, "pagePartial");
    let second = run
        .wait(|s| s.last_sync.as_ref().is_some_and(|l| l.saved != first.saved))
        .last_sync
        .unwrap();
    assert_eq!((second.saved.len(), second.deleted), (1, 1));
    assert_eq!(second.error, first.error);
    assert_eq!(tablet.stored_page_count(), 0);
    assert_eq!(tablet.stats().connections, 1, "the same connection");
    run.service.shutdown();
}

/// The log of the one connection that was kept: its lines.
fn kept_log(root: &Path) -> Vec<String> {
    let logs = root.join("_ardoise").join("diagnostics");
    let files: Vec<_> = std::fs::read_dir(&logs).unwrap().flatten().collect();
    assert_eq!(files.len(), 1);
    let (_, entries) = ardoise_device::log::read_log(&files[0].path()).unwrap();
    entries.into_iter().map(|e| e.info).collect()
}

/// The tablet stops a transfer early, with the right CRC of what it sent: the page is asked once
/// more, and the second transfer, whole, is what is stored and deleted.
#[test]
fn a_page_cut_short_once_is_downloaded_again() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 2);
    registered_beforehand(&tablet, library.path());
    let raw = tablet.stored_pages()[0].bytes.clone();
    tablet.cut_next_transfer(2);
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted, last.error), (2, 2, None));
    assert_eq!(tablet.stats().downloads, 3);
    assert_eq!(tablet.stored_page_count(), 0);
    assert!(partial_notes(library.path()).is_empty());
    let opened = Library::open(library.path()).unwrap();
    let originals: Vec<_> = notes(library.path())
        .iter()
        .filter_map(|path| opened.read(path).unwrap().file.original)
        .collect();
    assert!(originals.contains(&raw));
    run.service.shutdown();
}

/// Twice: a failed transfer. Nothing is stored, nothing deleted, and the user is told so; the next
/// sync starts with that page again.
#[test]
fn a_page_cut_short_twice_stays_on_the_tablet() {
    let _report = common::report("service: a page cut short by the tablet");
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 2);
    registered_beforehand(&tablet, library.path());
    let raw = tablet.stored_pages()[0].bytes.clone();
    let cut = raw.len() - 1;
    assert!(ardoise_proto::stroke::decode(&raw[..cut]).is_err());
    assert!(
        !ardoise_proto::stroke::decode_partial(&raw[..cut])
            .strokes
            .is_empty(),
        "strokes that would have been made into a note"
    );
    tablet.cut_next_transfer(cut);
    tablet.cut_next_transfer(cut);
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted), (0, 0));
    let error = last.error.unwrap();
    assert_eq!(error.code, "pageCutShort");
    assert!(
        error
            .message
            .contains(&format!("{cut} of the {} bytes", raw.len())),
        "{error:?}"
    );
    assert_eq!(tablet.stored_page_count(), 2);
    assert_eq!(tablet.stored_pages()[0].bytes, raw);
    assert!(notes(library.path()).is_empty());

    run.service.sync_now(false).unwrap();
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.deleted == 2));
    assert_eq!(status.last_sync.unwrap().error, None);
    assert_eq!(notes(library.path()).len(), 2);
    assert!(partial_notes(library.path()).is_empty());
    run.service.shutdown();
    // Announced against received, in the log kept for the failure.
    let infos = kept_log(library.path());
    assert!(
        infos.contains(&format!(
            "page 1: {0} bytes received, {0} announced",
            raw.len()
        )),
        "{infos:#?}"
    );
    say!(
        "cut twice: {} ({}), nothing stored",
        error.code,
        error.message
    );
}

/// Shorter than announced but read to its end: asked once more (it may have been cut between two
/// packets), then a page as it came.
#[test]
fn a_short_page_that_decodes_is_asked_once_more_then_stored() {
    for cuts in [1, 2] {
        let library = new_library();
        let tablet = SimTablet::slate_large();
        with_pages(&tablet, 1);
        registered_beforehand(&tablet, library.path());
        let raw = tablet.stored_pages()[0].bytes.clone();
        // Without its last end mark.
        let cut = raw.len() - ardoise_proto::stroke::STROKE_END_PACKET.len();
        for _ in 0..cuts {
            tablet.cut_next_transfer(cut);
        }
        let mut run = Running::virtual_time(
            &tablet,
            library.path(),
            TabletOptions::default(),
            library_store(None, None),
        );
        let last = run.wait(|s| s.last_sync.is_some()).last_sync.unwrap();
        assert_eq!((last.saved.len(), last.deleted, last.error), (1, 1, None));
        assert_eq!(tablet.stats().downloads, 2);
        let opened = Library::open(library.path()).unwrap();
        let file = opened.read(&notes(library.path())[0]).unwrap().file;
        let kept = if cuts == 1 { raw.len() } else { cut };
        assert_eq!(file.original.as_deref(), Some(&raw[..kept]));
        assert!(!is_partial_page(&file));
        run.service.shutdown();
    }
}

/// A page the tablet announces without any byte: an empty note, nothing to warn about.
#[test]
fn a_page_without_any_byte_warns_of_nothing() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    tablet.store_raw_page(Vec::new());
    with_later_pages(&tablet, 2);
    registered_beforehand(&tablet, library.path());
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    let last = run.wait(|s| s.last_sync.is_some()).last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted, last.error), (2, 2, None));
    assert!(partial_notes(library.path()).is_empty());
    assert_eq!(tablet.stored_page_count(), 0);
    run.service.shutdown();
}

/// Cancelled while the page read in part is stored: that page is stored and deleted, the others
/// stay, and the page is said all the same.
#[test]
fn a_sync_cancelled_after_a_page_read_in_part_still_says_it() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    tablet.store_raw_page(unreadable_page());
    with_later_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
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
    let options = TabletOptions {
        auto_sync: false,
        ..TabletOptions::default()
    };
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        options,
        library_store(None, Some(Box::new(before))),
    );
    run.connected();
    run.service.sync_now(false).unwrap();
    page_reached.recv_timeout(WAIT).unwrap();
    run.service.cancel();
    go.send(()).unwrap();
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted), (1, 1));
    let error = last.error.unwrap();
    let partial = partial_notes(library.path());
    assert_eq!(
        (error.code.as_str(), error.message),
        ("pagePartial", stem(&partial[0].0))
    );
    assert_eq!(tablet.stored_page_count(), 2);
    // The others, when asked; said once.
    run.service.sync_now(false).unwrap();
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.deleted == 2));
    assert_eq!(status.last_sync.unwrap().error, None);
    assert_eq!(notes(library.path()).len(), 3);
    run.service.shutdown();
}

/// The tablet refuses to delete the page read in part: the note is there, the refusal is said; the
/// next sync deletes the page without a second note, and says it.
#[test]
fn a_page_read_in_part_whose_deletion_is_refused_is_deleted_and_said_by_the_next_sync() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    let raw = unreadable_page();
    tablet.store_raw_page(raw.clone());
    with_later_pages(&tablet, 2);
    registered_beforehand(&tablet, library.path());
    tablet.refuse_next_requests(ardoise_proto::message::opcode::DELETE_OLDEST, 1);
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, None),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    assert_eq!((last.saved.len(), last.deleted), (1, 0));
    assert_eq!(last.error.unwrap().code, "syncRefused");
    assert_eq!(tablet.stored_page_count(), 2);
    let partial = partial_notes(library.path());
    assert_eq!(partial.len(), 1);
    assert_eq!(partial[0].1.original.as_deref(), Some(raw.as_slice()));

    run.service.sync_now(false).unwrap();
    let status = run.wait(|s| s.last_sync.as_ref().is_some_and(|l| l.deleted == 2));
    let last = status.last_sync.unwrap();
    assert_eq!(last.saved.len(), 1, "no second note of the first page");
    let error = last.error.unwrap();
    assert_eq!(
        (error.code.as_str(), error.message),
        ("pagePartial", stem(&partial[0].0))
    );
    assert_eq!(notes(library.path()).len(), 2);
    assert_eq!(tablet.stored_page_count(), 0);
    run.service.shutdown();
    // Said in the log although the deletion failed.
    let infos = kept_log(library.path());
    assert!(
        infos
            .iter()
            .any(|i| i.starts_with("page 1 cannot be read whole")),
        "{infos:#?}"
    );
}

/// A page finished meanwhile asks for a sync at once, which cannot start (the tablet refuses, or
/// the link is lost): what the first sync said is replaced or about to be, so the sync that works
/// says it again.
#[test]
fn a_page_read_in_part_is_said_again_when_the_sync_that_follows_cannot_start() {
    for lost in [false, true] {
        let library = new_library();
        let tablet = SimTablet::slate_large();
        tablet.store_raw_page(unreadable_page());
        registered_beforehand(&tablet, library.path());
        let drawing = tablet.clone();
        let before = move |n: u16| {
            if n == 1 {
                with_later_pages(&drawing, 2);
                // The next count is the one after the sync; the one after that starts the sync
                // of the page finished meanwhile.
                let count = ardoise_proto::message::opcode::FILES_COUNT;
                if lost {
                    drawing.lose_link_at_request(count, 1);
                } else {
                    drawing.refuse_requests_after(count, 1, 1);
                }
            }
        };
        let mut run = Running::virtual_time(
            &tablet,
            library.path(),
            TabletOptions::default(),
            library_store(None, Some(Box::new(before))),
        );
        let told = run.wait(|s| s.last_sync.is_some()).last_sync.unwrap();
        assert_eq!(told.deleted, 1);
        assert_eq!(told.error.clone().unwrap().code, "pagePartial");
        if !lost {
            let failed = run
                .wait(|s| s.last_sync.as_ref().is_some_and(|l| l.error != told.error))
                .last_sync
                .unwrap();
            assert_eq!(failed.error.unwrap().code, "syncRefused");
        }
        let last = run
            .wait(|s| {
                s.last_sync
                    .as_ref()
                    .is_some_and(|l| l.deleted == 1 && l.saved != told.saved)
            })
            .last_sync
            .unwrap();
        assert_eq!(last.saved.len(), 1);
        assert_eq!(last.error, told.error, "lost: {lost}");
        assert_eq!(tablet.stats().connections, 2, "lost: {lost}");
        assert_eq!(tablet.stored_page_count(), 0);
        run.service.shutdown();
    }
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
fn stopping_live_mode_that_waits_for_a_sync_leaves_the_sync_shown() {
    // The automatic sync of the connection runs; live mode asked meanwhile waits for it. Leaving
    // live mode then must not show the tablet as idle: the progress and « Annuler » stay.
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
    // Each page waits for the test before it is stored.
    let (reached, page_reached) = mpsc::channel();
    let (go, wait_go) = mpsc::channel::<()>();
    let wait_go = Mutex::new(wait_go);
    let reached = Mutex::new(reached);
    let before = move |n: u16| {
        reached.lock().unwrap().send(n).unwrap();
        wait_go.lock().unwrap().recv_timeout(WAIT).unwrap();
    };
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        library_store(None, Some(Box::new(before))),
    );
    assert_eq!(page_reached.recv_timeout(WAIT).unwrap(), 1);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    go.send(()).unwrap();
    // Taken between pages 1 and 2: live mode waits for the sync.
    assert_eq!(page_reached.recv_timeout(WAIT).unwrap(), 2);
    assert_eq!(
        inputs.recv_timeout(WAIT).unwrap(),
        LiveInput::State(LiveState::Waiting)
    );
    run.service.stop_live();
    go.send(()).unwrap();
    // Taken between pages 2 and 3: live mode is left while the sync goes on.
    assert_eq!(page_reached.recv_timeout(WAIT).unwrap(), 3);
    assert_eq!(
        inputs.recv_timeout(WAIT).unwrap(),
        LiveInput::State(LiveState::Stopped)
    );
    let status = run.service.status();
    assert_eq!(status.connection, Connection::Syncing, "{status:#?}");
    assert!(status.sync.is_some());
    assert_eq!(status.pending, None);
    go.send(()).unwrap();
    let status = run.wait(|s| s.last_sync.is_some());
    assert_eq!(status.last_sync.unwrap().saved.len(), 3);
    assert_eq!(status.connection, Connection::Connected);
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
    run.service.sync_now(false).unwrap();
    let status = run.wait(|s| s.pending == Some(ardoise_device::service::Pending::Sync));
    assert_eq!(status.connection, Connection::Searching);
    tablet.power_on();
    let status = run.wait(|s| s.last_sync.is_some());
    assert_eq!(status.last_sync.unwrap().saved.len(), 2);
    assert_eq!(status.pending, None);
    run.service.shutdown();
}

/// A Xiaomi blackboard on `tablet`'s clock holding the first `pages` reference pages, added from
/// this library before, switched off.
fn board_known(tablet: &SimTablet, root: &Path, pages: usize) -> SimBoard {
    let board = SimBoard::with_clock(SimBoardConfig::blackboard(), tablet.clock());
    for page in reference_board_pages().iter().take(pages) {
        board.draw_page(page);
    }
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
    board.power_off();
    board
}

/// The automatic sync and "delete after import" off.
const BOTH_OFF: TabletOptions = TabletOptions {
    auto_sync: false,
    delete_after_import: false,
    orientation: Orientation::Landscape,
    board_position: ardoise_core::settings::BoardPosition::ButtonsBottom,
    button: ardoise_core::settings::LiveButton::NewPage,
};

/// Both tablets known, both switched off: the Slate holding 3 pages, the blackboard
/// `board_pages`; the service started with [`BOTH_OFF`] and `store`.
fn both_away_with(
    library: &Path,
    board_pages: usize,
    store: PageStore,
) -> (SimTablet, SimBoard, Running) {
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 3);
    registered_beforehand(&tablet, library);
    let board = board_known(&tablet, library, board_pages);
    tablet.power_off();
    let factory = SimFactory::virtual_time(tablet.clone()).with_board(board.clone());
    let run = Running::start(&tablet, library, BOTH_OFF, factory, store);
    (tablet, board, run)
}

/// [`both_away_with`] the library's store.
fn both_away(library: &Path, board_pages: usize) -> (SimTablet, SimBoard, Running) {
    both_away_with(library, board_pages, library_store(None, None))
}

/// « Sync » clicked with no tablet connected, then the blackboard switched on: connected, the
/// sync still waiting for the Slate.
fn asked_then_board_connected(run: &mut Running, board: &SimBoard) -> TabletStatus {
    run.wait(|s| s.connection == Connection::Searching);
    run.service.sync_now(false).unwrap();
    run.wait(|s| s.pending == Some(Pending::SlateSync));
    board.power_on();
    let status = run.wait(|s| {
        s.connection == Connection::Connected && s.active == Some(TabletKind::Blackboard)
    });
    assert_eq!(status.pending, Some(Pending::SlateSync), "{status:#?}");
    status
}

/// The Slate is switched on while the blackboard holds the link: it gets the link at the
/// worker's next look for it (every 10 s of the shared clock).
fn slate_asks(tablet: &SimTablet) {
    tablet.power_on();
    tablet.clock().advance(Duration::from_secs(11));
}

/// Both tablets known, the automatic sync and "delete after import" off (Q28): « Sync » clicked
/// while no tablet is connected waits for the Bamboo Slate. The blackboard connecting first does
/// not take it (its pages are imported on a request confirmed while it is connected, Q15), and
/// does not drop it either: the Slate's page is imported when it connects next.
#[test]
fn a_sync_asked_while_no_tablet_is_connected_waits_for_the_slate_past_the_blackboard() {
    let _report = common::report("two tablets: a sync asked while away waits for the Slate");
    let library = new_library();
    let (tablet, board, mut run) = both_away(library.path(), 3);
    // Said as the Slate's from the start, and still once the blackboard is connected.
    let status = asked_then_board_connected(&mut run, &board);
    assert_eq!(status.error, None);
    assert_eq!(
        board.stats().downloads,
        0,
        "nothing asked of the blackboard"
    );
    slate_asks(&tablet);
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    say!("{last:?}");
    // The Slate's oldest page (no deletion: the tablet hands over only that one).
    assert_eq!((last.saved.len(), last.deleted, last.error), (1, 0, None));
    assert_eq!(status.active, Some(TabletKind::Slate));
    assert_eq!(status.pending, None);
    assert_eq!((board.stats().downloads, board.stored_page_count()), (0, 3));
    assert_eq!(notes(library.path()).len(), 1);
    run.service.shutdown();
}

/// The sync that waits for the Slate past the blackboard (Q28), its neighbours: the blackboard
/// leaving before the Slate comes, and live mode run on the blackboard meanwhile.
#[test]
fn the_sync_that_waits_for_the_slate_outlasts_the_blackboard_and_its_live_mode() {
    let _report = common::report("two tablets: the Slate's sync outlasts the blackboard");
    // The blackboard switched off again: the request still waits, then runs on the Slate.
    let library = new_library();
    let (tablet, board, mut run) = both_away(library.path(), 3);
    asked_then_board_connected(&mut run, &board);
    board.power_off();
    let status = run.wait(|s| s.connection == Connection::Searching);
    assert_eq!(status.pending, Some(Pending::SlateSync));
    tablet.power_on();
    let status = run.wait(|s| s.last_sync.is_some());
    assert_eq!(status.last_sync.unwrap().saved.len(), 1);
    assert_eq!(
        (status.active, status.pending),
        (Some(TabletKind::Slate), None)
    );
    assert_eq!(board.stats().downloads, 0);
    run.service.shutdown();

    // Live mode on the blackboard meanwhile (it holds no page: it may start): the request waits
    // through it, and runs once the Slate comes.
    let library = new_library();
    let (tablet, board, mut run) = both_away(library.path(), 0);
    asked_then_board_connected(&mut run, &board);
    let (sender, inputs) = mpsc::channel();
    run.service.start_live(Box::new(ChannelSink(sender)));
    until(&inputs, |i| {
        matches!(
            i,
            LiveInput::Started {
                kind: TabletKind::Blackboard,
                ..
            }
        )
    });
    let status = run.wait(|s| s.connection == Connection::Live);
    assert_eq!(status.pending, Some(Pending::SlateSync));
    run.service.stop_live();
    until(&inputs, |i| *i == LiveInput::State(LiveState::Stopped));
    let status = run.wait(|s| s.connection == Connection::Connected);
    assert_eq!(status.pending, Some(Pending::SlateSync));
    slate_asks(&tablet);
    let status = run.wait(|s| s.last_sync.is_some());
    assert_eq!(status.last_sync.unwrap().saved.len(), 1);
    assert_eq!(
        (status.active, status.pending),
        (Some(TabletKind::Slate), None)
    );
    run.service.shutdown();
}

/// The sync that waits for the Slate past the blackboard (Q28) is dropped by the user's cancel,
/// or with the Slate forgotten; cancelling the blackboard's own sync leaves it.
#[test]
fn the_sync_that_waits_for_the_slate_goes_with_a_cancel_or_the_slate_forgotten() {
    let _report = common::report("two tablets: the Slate's sync cancelled");
    // Cancelled while the blackboard holds the link: the Slate connects without syncing.
    let library = new_library();
    let (tablet, board, mut run) = both_away(library.path(), 3);
    asked_then_board_connected(&mut run, &board);
    run.service.cancel();
    run.wait(|s| s.connection == Connection::Connected && s.pending.is_none());
    slate_asks(&tablet);
    run.wait(|s| s.connection == Connection::Connected && s.active == Some(TabletKind::Slate));
    std::thread::sleep(Duration::from_millis(200));
    let status = run.service.status();
    assert_eq!((status.last_sync, status.pending), (None, None));
    assert_eq!(tablet.stored_page_count(), 3);
    assert!(notes(library.path()).is_empty());
    run.service.shutdown();

    // Cancelled before any tablet connected.
    let library = new_library();
    let (_tablet, board, mut run) = both_away(library.path(), 3);
    run.wait(|s| s.connection == Connection::Searching);
    run.service.sync_now(false).unwrap();
    run.wait(|s| s.pending == Some(Pending::SlateSync));
    run.service.cancel();
    run.wait(|s| s.pending.is_none());
    board.power_on();
    let status = run.wait(|s| s.connection == Connection::Connected);
    assert_eq!(status.pending, None);
    run.service.shutdown();

    // The Slate forgotten while the blackboard holds the link: nothing waits any more.
    let library = new_library();
    let (_tablet, board, mut run) = both_away(library.path(), 3);
    asked_then_board_connected(&mut run, &board);
    run.service.forget().unwrap();
    let status = run.wait(|s| s.known.len() == 1);
    assert_eq!(status.pending, None);
    assert_eq!(status.active, Some(TabletKind::Blackboard));
    run.service.shutdown();

    // The blackboard's own sync (confirmed), cancelled between two of its pages: the Slate's
    // request still waits, and runs when the Slate comes.
    let library = new_library();
    let (reached, page_reached) = mpsc::channel();
    let (go, wait_go) = mpsc::channel::<()>();
    let (reached, wait_go) = (Mutex::new(reached), Mutex::new(wait_go));
    let before = move |n: u16| {
        if n == 1 {
            reached.lock().unwrap().send(()).unwrap();
            wait_go.lock().unwrap().recv_timeout(WAIT).unwrap();
        }
    };
    let store = library_store(None, Some(Box::new(before)));
    let (tablet, board, mut run) = both_away_with(library.path(), 3, store);
    asked_then_board_connected(&mut run, &board);
    run.service.sync_now(true).unwrap();
    page_reached.recv_timeout(WAIT).unwrap();
    run.service.cancel();
    go.send(()).unwrap();
    let status = run.wait(|s| s.last_sync.is_some());
    assert_eq!(status.active, Some(TabletKind::Blackboard));
    assert_eq!(status.pending, Some(Pending::SlateSync), "{status:#?}");
    assert!(board.stored_page_count() >= 2, "the sync stopped");
    let before = status.last_sync.unwrap().at;
    slate_asks(&tablet);
    let status = run.wait(|s| {
        s.active == Some(TabletKind::Slate) && s.last_sync.as_ref().is_some_and(|l| l.at > before)
    });
    assert_eq!(status.last_sync.unwrap().saved.len(), 1);
    assert_eq!(status.pending, None);
    run.service.shutdown();
}

/// With only the blackboard known, a sync asked while it is away and without "delete after
/// import" is dropped when it connects, as before Q28: no other tablet to wait for (the panel
/// does not offer it, `blackboardWaitsForRequest`).
#[test]
fn with_only_the_blackboard_known_a_sync_asked_while_away_is_dropped_at_its_connection() {
    let library = new_library();
    let tablet = SimTablet::slate_large();
    let board = board_known(&tablet, library.path(), 3);
    let factory = SimFactory::of(vec![ardoise_sim::SimDevice::Board(board.clone())], false);
    let mut run = Running::start(
        &tablet,
        library.path(),
        BOTH_OFF,
        factory,
        library_store(None, None),
    );
    run.wait(|s| s.connection == Connection::Searching);
    run.service.sync_now(false).unwrap();
    run.wait(|s| s.pending == Some(Pending::Sync));
    board.power_on();
    let status = run.wait(|s| s.connection == Connection::Connected);
    assert_eq!((status.pending, status.error), (None, None));
    assert_eq!((board.stats().downloads, board.stored_page_count()), (0, 3));
    run.service.shutdown();
}

/// Why the automatic export failed, as Windows says it.
const EXPORT_REASON: &str = "Access is denied. (os error 5)";

/// [`library_store`], the automatic export of every note failing as the app's store says it
/// (`StoredPage::export_failure`, Q28).
fn store_export_failing(refuse: Option<u16>) -> PageStore {
    let mut store = library_store(refuse, None);
    Box::new(
        move |root: &Path, page: DownloadedPage, context: &PageContext| {
            let mut stored = store(root, page, context)?;
            stored.export_failure = Some(ExportFailure {
                path: format!("Exports\\{}.pdf", stem(&stored.path)),
                reason: EXPORT_REASON.into(),
            });
            Ok(stored)
        },
    )
}

/// An automatic export that could not be written (Q28), for the Slate and for the blackboard:
/// every note is imported and its page deleted, the sync did not fail, and its end says how many
/// notes were not exported, the first file and why. A real failure of the sync stays its error,
/// and what was not exported is said beside it.
#[test]
fn an_automatic_export_that_cannot_be_written_is_said_at_the_end_of_the_sync() {
    let _report = common::report("service: an automatic export that cannot be written");
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        store_export_failing(None),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    say!("Slate: {last:?}");
    assert_eq!((last.saved.len(), last.deleted, last.error), (3, 3, None));
    assert_eq!(status.error, None);
    let first = stem(Path::new(&last.saved[0]));
    assert_eq!(
        last.export_failed,
        Some(ExportFailed {
            notes: 3,
            path: format!("Exports\\{first}.pdf"),
            reason: EXPORT_REASON.into(),
        })
    );
    assert_eq!(notes(library.path()).len(), 3);
    assert_eq!(tablet.stored_page_count(), 0);
    run.service.shutdown();

    // The second page cannot be stored: the sync failed, and the first page's export too.
    let library = new_library();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 3);
    registered_beforehand(&tablet, library.path());
    let mut run = Running::virtual_time(
        &tablet,
        library.path(),
        TabletOptions::default(),
        store_export_failing(Some(2)),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    say!("Slate, page 2 refused: {last:?}");
    assert_eq!(last.error.map(|e| e.code).as_deref(), Some("libraryAccess"));
    assert_eq!(
        status.error.map(|e| e.code).as_deref(),
        Some("libraryAccess")
    );
    assert_eq!(last.export_failed.map(|f| f.notes), Some(1));
    assert_eq!((last.saved.len(), tablet.stored_page_count()), (1, 2));
    run.service.shutdown();

    // The blackboard's pages.
    let library = new_library();
    let tablet = SimTablet::slate_large();
    let board = board_known(&tablet, library.path(), 3);
    board.power_on();
    let factory = SimFactory::of(vec![ardoise_sim::SimDevice::Board(board.clone())], false);
    let mut run = Running::start(
        &tablet,
        library.path(),
        TabletOptions::default(),
        factory,
        store_export_failing(None),
    );
    let status = run.wait(|s| s.last_sync.is_some());
    let last = status.last_sync.unwrap();
    say!("blackboard: {last:?}");
    assert_eq!((last.saved.len(), last.deleted, last.error), (3, 3, None));
    assert_eq!(
        (status.active, status.error),
        (Some(TabletKind::Blackboard), None)
    );
    assert_eq!(
        last.export_failed.map(|f| (f.notes, f.reason)),
        Some((3, EXPORT_REASON.into()))
    );
    assert_eq!(notes(library.path()).len(), 3);
    assert_eq!(board.stored_page_count(), 0);
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
            orientation: Orientation::Landscape,
            kind: TabletKind::Slate
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
                orientation,
                kind: TabletKind::Slate
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
