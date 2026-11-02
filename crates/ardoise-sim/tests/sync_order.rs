//! The order of `ec` (set the file-transfer reporting type) around the modes, through `Manager`
//! and `Session`, against a simulated Slate that refuses `ec` in paper mode as the real Slate A4
//! did in the first hardware session (2026-11-02): the paths of the app at connection, after a
//! registration, after live mode, and a second sync in the same connection.
//!
//! `cargo test -p ardoise-sim --test sync_order -- --nocapture` prints the frames sent.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod common;

use std::time::Duration;

use ardoise_device::identity::{self, DEVICE_FORMAT, DeviceRecord, HostId};
use ardoise_device::log::{DiagnosticLog, Direction, SharedLog, lock};
use ardoise_device::manager::Manager;
use ardoise_device::session::{DeviceError, Session};
use ardoise_proto::message::opcode;
use ardoise_sim::{SimTablet, drawings};

const WAIT: Duration = Duration::from_secs(10);
const ID: [u8; 6] = [0x89, 0xea, 0x78, 0x88, 0xff, 0xcb];

fn with_pages(tablet: &SimTablet, count: usize) {
    let config = tablet.config();
    for page in drawings::reference_pages(config.width, config.height)
        .iter()
        .cycle()
        .take(count)
    {
        tablet.draw_page(page);
    }
}

/// The tablet was registered from `library` before.
fn registered_beforehand(tablet: &SimTablet, library: &std::path::Path) -> DeviceRecord {
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
        registered: "2026-11-02T11:00:00+01:00".into(),
        extra: Default::default(),
    };
    identity::save(library, &record).unwrap();
    record
}

fn manager(tablet: &SimTablet, library: &std::path::Path) -> (Manager, SharedLog) {
    let log = DiagnosticLog::in_memory("sim", tablet.clock()).shared();
    let manager = Manager::new(
        Box::new(tablet.connector()),
        library.to_owned(),
        log.clone(),
    );
    (manager, log)
}

/// The app's connection: authentication and clock, information, then the page count (which
/// puts the tablet in paper mode), as the tablet service does before any sync.
fn connect_like_the_app(manager: &mut Manager, record: &DeviceRecord) -> (Session, u16) {
    let (mut session, _) = manager.connect(record, None).unwrap();
    session.info().unwrap();
    let pages = session.stored_pages().unwrap();
    (session, pages)
}

fn sync_all(session: &mut Session) -> Result<u16, DeviceError> {
    session
        .sync(true, &mut |_, _, _| Ok(()))
        .map(|summary| summary.deleted)
}

/// The frames written in `log`, one per line, `ec` and `b1` named.
fn sent(log: &SharedLog) -> Vec<String> {
    lock(log)
        .entries()
        .iter()
        .filter(|entry| entry.dir == Direction::Tx)
        .map(|entry| entry.hex.clone().unwrap_or_default())
        .collect()
}

/// Checks the rule learnt from the real tablet on every connection of `log`: `ec` is never
/// written while the tablet is in paper or live mode.
fn assert_ec_only_outside_paper_and_live(log: &SharedLog) {
    let mut mode = None;
    for entry in lock(log).entries() {
        match entry.dir {
            Direction::Event if entry.info.starts_with("connected") => mode = None,
            Direction::Tx => {
                let bytes = entry.bytes().unwrap();
                match bytes.as_slice() {
                    [opcode::SET_MODE, 0x01, m] => mode = Some(*m),
                    [opcode::FILE_TRANSFER_REPORTING_TYPE, ..] => assert!(
                        !matches!(mode, Some(0x00 | 0x01)),
                        "ec written in mode {mode:?} at t={}",
                        entry.t
                    ),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

#[test]
fn the_app_connection_then_a_sync_downloads_every_page() {
    let _report = common::report("connection (info, page count in paper mode), then sync");
    let library = tempfile::tempdir().unwrap();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 3);
    let record = registered_beforehand(&tablet, library.path());
    let (mut manager, log) = manager(&tablet, library.path());
    let (mut session, pages) = connect_like_the_app(&mut manager, &record);
    assert_eq!(pages, 3);
    let deleted = sync_all(&mut session);
    say!("frames: {}", sent(&log).join(" | "));
    assert_eq!(deleted, Ok(3), "the 2026-11-02 sessions failed here");
    assert_eq!(tablet.stored_page_count(), 0);
    assert_ec_only_outside_paper_and_live(&log);
    // Once per connection, before the first paper mode (protocol.md §3.3).
    let frames = sent(&log);
    let ec = frames.iter().position(|f| f.starts_with("ec")).unwrap();
    let paper = frames.iter().position(|f| f == "b1 01 01").unwrap();
    assert!(ec < paper, "{frames:?}");
    assert_eq!(frames.iter().filter(|f| f.starts_with("ec")).count(), 1);
}

#[test]
fn a_registration_then_a_sync_downloads_every_page() {
    let _report = common::report("registration (info, page count), then sync");
    let library = tempfile::tempdir().unwrap();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 2);
    let (mut manager, log) = manager(&tablet, library.path());
    tablet.hold_button();
    let found = manager.find(true, WAIT).unwrap().unwrap();
    let (mut session, _) = manager
        .register(&found, WAIT, &mut || tablet.press_button())
        .unwrap();
    assert_eq!(session.stored_pages().unwrap(), 2);
    assert_eq!(sync_all(&mut session), Ok(2));
    assert_eq!(tablet.stored_page_count(), 0);
    assert_ec_only_outside_paper_and_live(&log);
}

#[test]
fn a_sync_after_live_mode_then_a_second_sync_in_the_same_connection() {
    let _report = common::report("live mode, sync, a page finished, a second sync");
    let library = tempfile::tempdir().unwrap();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 4);
    let record = registered_beforehand(&tablet, library.path());
    let (mut manager, log) = manager(&tablet, library.path());
    let (mut session, pages) = connect_like_the_app(&mut manager, &record);
    assert_eq!(pages, 4);

    say!("live mode, then idle mode: the order of the only sync that worked on 2026-11-02");
    session.start_live().unwrap();
    session.stop_live().unwrap();
    assert_eq!(sync_all(&mut session), Ok(4));

    say!("a page finished meanwhile: counted, then synced in the same connection");
    with_pages(&tablet, 1);
    assert_eq!(session.stored_pages().unwrap(), 1);
    let second = sync_all(&mut session);
    say!("frames: {}", sent(&log).join(" | "));
    assert_eq!(second, Ok(1), "the 2026-11-02 second sync failed here");
    assert_eq!(tablet.stored_page_count(), 0);
    assert_ec_only_outside_paper_and_live(&log);

    say!("and again after another live session");
    with_pages(&tablet, 2);
    session.start_live().unwrap();
    session.stop_live().unwrap();
    assert_eq!(sync_all(&mut session), Ok(2));
    assert_ec_only_outside_paper_and_live(&log);
}

#[test]
fn a_tablet_left_in_paper_mode_is_put_in_idle_mode_before_ec() {
    let _report = common::report("ec refused once: idle mode, then ec again");
    let library = tempfile::tempdir().unwrap();
    let tablet = SimTablet::slate_large();
    with_pages(&tablet, 1);
    let record = registered_beforehand(&tablet, library.path());
    let (mut manager, log) = manager(&tablet, library.path());
    let (mut session, _) = manager.connect(&record, None).unwrap();
    // The tablet refuses the first ec, as if it were in paper mode without our knowing.
    tablet.refuse_next_requests(opcode::FILE_TRANSFER_REPORTING_TYPE, 1);
    assert_eq!(session.stored_pages().unwrap(), 1);
    assert_eq!(sync_all(&mut session), Ok(1));
    let frames = sent(&log);
    say!("frames: {}", frames.join(" | "));
    let ec: Vec<usize> = frames
        .iter()
        .enumerate()
        .filter(|(_, f)| f.starts_with("ec"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(ec.len(), 2, "{frames:?}");
    assert_eq!(frames[ec[1] - 1], "b1 01 02", "idle mode before the retry");

    say!("always refused: the page count works, the sync fails with the refusal, nothing deleted");
    let (mut manager, _) = self::manager(&tablet, library.path());
    drop(session);
    with_pages(&tablet, 1);
    let (mut session, _) = manager.connect(&record, None).unwrap();
    tablet.refuse_next_requests(opcode::FILE_TRANSFER_REPORTING_TYPE, 3);
    assert_eq!(session.stored_pages(), Ok(1));
    assert_eq!(
        sync_all(&mut session),
        Err(DeviceError::Refused(
            ardoise_proto::StatusCode::InvalidState
        ))
    );
    assert_eq!(tablet.stored_page_count(), 1);
}
