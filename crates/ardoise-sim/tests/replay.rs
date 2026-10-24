//! "Un journal produit avec le simulateur se rejoue à l'identique": a session recorded
//! against the simulator (through `Manager`, whose transports feed the `.jsonl` log) is
//! replayed from the file through the same operations, into a second log with the same
//! exchanges; and a host that deviates from the recording is stopped with a precise error.
//!
//! `cargo test -p ardoise-sim --test replay -- --nocapture` prints the comparison.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod common;

use std::path::{Path, PathBuf};
use std::time::Duration;

use ardoise_device::identity::{self, HostId};
use ardoise_device::log::{self, DiagnosticLog, Direction, LogEntry, lock};
use ardoise_device::manager::Manager;
use ardoise_device::session::DeviceError;
use ardoise_device::transport::{Connector, TransportError};
use ardoise_proto::live::{LivePacket, LiveRecord};
use ardoise_sim::replay::first_difference;
use ardoise_sim::{ReplayConnector, ReplayOptions, SimTablet, drawings};

const WAIT: Duration = Duration::from_secs(10);
const LIVE: Duration = Duration::from_secs(3);

/// What the operations observed; equal for the recording and its replay.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    address: String,
    pages: u16,
    samples: u64,
    reconnections: u32,
    battery: u8,
}

/// Find in registration mode, register, sync two pages with deletion, 3 s of live mode in
/// which the tablet drops the link once, disconnect, reconnect with the saved identity and
/// read the battery. `tablet` performs the physical actions when recording; a replay has no
/// tablet.
fn exercise(manager: &mut Manager, tablet: Option<&SimTablet>) -> Result<Outcome, DeviceError> {
    if let Some(tablet) = tablet {
        let config = tablet.config();
        let pages = drawings::reference_pages(config.width, config.height);
        tablet.draw_page(&pages[2]);
        tablet.draw_page(&pages[1]);
        tablet.hold_button();
    }
    let found = manager
        .find(true, WAIT)?
        .ok_or(DeviceError::Timeout("tablet"))?;
    let (mut session, _) = manager.register(&found, WAIT, &mut || {
        if let Some(tablet) = tablet {
            tablet.press_button();
        }
    })?;
    let record = manager
        .registration()?
        .ok_or(DeviceError::Timeout("record"))?;
    let summary = session.sync(true, &mut |_, _, _| Ok(()))?;

    if let Some(tablet) = tablet {
        let config = tablet.config();
        tablet.draw_live(drawings::live_script(3, config.width, config.height));
        tablet.disconnect_after(Duration::from_millis(1_234));
    }
    let start = manager.clock().monotonic();
    let mut samples = 0;
    let reconnections = manager.live(&mut session, &record, start + LIVE, &mut |packet| {
        if let LivePacket::Samples(records) = packet {
            samples += records
                .iter()
                .filter(|r| matches!(r, LiveRecord::Pen(_)))
                .count() as u64;
        }
    })?;
    session.disconnect()?;

    let (mut session, _) = manager.connect(&record, None)?;
    let (battery, _) = session.battery()?;
    session.disconnect()?;
    Ok(Outcome {
        address: found.address,
        pages: summary.downloaded,
        samples,
        reconnections,
        battery,
    })
}

/// Records `exercise` against a fresh simulated tablet in `library`; returns the log path,
/// the registered identity and the outcome.
fn record(library: &Path) -> (PathBuf, HostId, Outcome) {
    let tablet = SimTablet::slate_large();
    let log = DiagnosticLog::create(library, "sim", tablet.clock())
        .unwrap()
        .shared();
    let path = lock(&log).path().unwrap().to_owned();
    let mut manager = Manager::new(
        Box::new(tablet.connector()),
        library.to_owned(),
        log.clone(),
    );
    let outcome = exercise(&mut manager, Some(&tablet)).unwrap();
    lock(&log).flush().unwrap();
    let id = identity::load(library).unwrap().unwrap().uuid;
    (path, id, outcome)
}

/// Replays `exercise` from `connector` with identity `id` into a new log in `library`.
fn replay(
    connector: &ReplayConnector,
    id: Option<HostId>,
    library: &Path,
) -> (PathBuf, Result<Outcome, DeviceError>) {
    let log = DiagnosticLog::create(library, "replay", connector.clock())
        .unwrap()
        .shared();
    let path = lock(&log).path().unwrap().to_owned();
    let manager = Manager::new(Box::new(connector.clone()), library.to_owned(), log.clone());
    let mut manager = match id {
        Some(id) => manager.with_host_id(id),
        None => manager,
    };
    let outcome = exercise(&mut manager, None);
    lock(&log).flush().unwrap();
    (path, outcome)
}

fn count(entries: &[LogEntry], dir: Direction) -> usize {
    entries.iter().filter(|e| e.dir == dir).count()
}

fn replay_message(error: DeviceError) -> String {
    match error {
        DeviceError::Transport(TransportError::Replay(message)) => message,
        other => panic!("expected a replay mismatch, got {other:?}"),
    }
}

#[test]
fn a_log_produced_with_the_simulator_replays_identically() {
    let _report = common::report("a_log_produced_with_the_simulator_replays_identically");
    let recorded_library = tempfile::tempdir().unwrap();
    let (recorded_path, id, recorded) = record(recorded_library.path());
    let (recorded_header, recorded_entries) = log::read_log(&recorded_path).unwrap();
    say!(
        "recorded against the simulator: {} entries ({} tx, {} rx, {} events, {} notes), \
         identity {id}, outcome {recorded:?}",
        recorded_entries.len(),
        count(&recorded_entries, Direction::Tx),
        count(&recorded_entries, Direction::Rx),
        count(&recorded_entries, Direction::Event),
        count(&recorded_entries, Direction::Note)
    );
    assert_eq!(recorded.pages, 2);
    assert_eq!(recorded.reconnections, 1);
    assert!(recorded.samples > 400);
    let disconnected = recorded_entries
        .iter()
        .filter(|e| e.dir == Direction::Event && e.info == "disconnected")
        .count();
    assert_eq!(disconnected, 1, "the drop is in the log");

    let connector = ReplayConnector::from_file(&recorded_path, ReplayOptions::default()).unwrap();
    let replay_library = tempfile::tempdir().unwrap();
    let (replayed_path, replayed) = replay(&connector, Some(id), replay_library.path());
    let replayed = replayed.unwrap();
    say!("replayed from the file with the recorded identity: outcome {replayed:?}");
    assert_eq!(replayed, recorded);
    assert_eq!(connector.remaining(), 0, "the whole log was replayed");
    assert_eq!(connector.failure(), None);

    let (replayed_header, replayed_entries) = log::read_log(&replayed_path).unwrap();
    assert_eq!(replayed_header.transport, "replay");
    assert_eq!(replayed_header.created, recorded_header.created);
    assert_eq!(
        first_difference(&recorded_entries, &replayed_entries),
        None,
        "same (dir, ch, hex) sequence"
    );
    let times = |entries: &[LogEntry]| entries.iter().map(|e| e.t).collect::<Vec<_>>();
    assert_eq!(
        times(&replayed_entries),
        times(&recorded_entries),
        "same times"
    );
    let replayed_record = identity::load(replay_library.path()).unwrap().unwrap();
    assert_eq!(replayed_record.uuid, id);
    say!(
        "second log: {} entries, identical (dir, ch, hex) sequence and identical times \
         (last t = {:.3} s)",
        replayed_entries.len(),
        replayed_entries.last().unwrap().t
    );
    say!("REPLAY OK");
}

#[test]
fn a_deviating_host_is_stopped_with_a_precise_mismatch() {
    let _report = common::report("a_deviating_host_is_stopped_with_a_precise_mismatch");
    let recorded_library = tempfile::tempdir().unwrap();
    let (recorded_path, id, _) = record(recorded_library.path());
    let (_, entries) = log::read_log(&recorded_path).unwrap();
    let recorded_e7 = entries
        .iter()
        .find(|e| e.dir == Direction::Tx)
        .and_then(|e| e.hex.clone())
        .unwrap();

    say!("replay without the recorded identity: the registration differs");
    let connector = ReplayConnector::from_file(&recorded_path, ReplayOptions::default()).unwrap();
    let library = tempfile::tempdir().unwrap();
    let (_, result) = replay(&connector, None, library.path());
    let message = replay_message(result.unwrap_err());
    say!("random identity: {message}");
    assert!(message.starts_with("log line "), "{message}");
    assert!(
        message.contains(&format!("expected tx {recorded_e7}")),
        "{message}"
    );
    assert!(message.contains("the host wrote tx e7 06 "), "{message}");
    assert_eq!(connector.failure(), Some(message));

    let connector = ReplayConnector::from_file(&recorded_path, ReplayOptions::default()).unwrap();
    let library = tempfile::tempdir().unwrap();
    let log = DiagnosticLog::in_memory("replay", connector.clock()).shared();
    let mut manager =
        Manager::new(Box::new(connector.clone()), library.path().to_owned(), log).with_host_id(id);
    let found = manager.find(true, WAIT).unwrap().unwrap();
    let (mut session, _) = manager.register(&found, WAIT, &mut || {}).unwrap();
    let error = session.battery().unwrap_err();
    let message = replay_message(error);
    say!("battery asked where a sync was recorded: {message}");
    assert!(
        message.contains("expected tx ec 06 06 00 00 00 00 00: the host wrote tx b9 01 00 instead"),
        "{message}"
    );
}

#[test]
fn a_real_capture_replays_with_another_set_time_only_in_lenient_mode() {
    let _report =
        common::report("a_real_capture_replays_with_another_set_time_only_in_lenient_mode");
    let recorded_library = tempfile::tempdir().unwrap();
    let (recorded_path, id, recorded) = record(recorded_library.path());
    let (header, mut entries) = log::read_log(&recorded_path).unwrap();
    // As if the capture had been made a year earlier: every SET_TIME payload differs.
    let mut changed = 0;
    for entry in &mut entries {
        if entry.dir == Direction::Tx
            && let Some(hex) = &entry.hex
            && hex.starts_with("b6 06 26")
        {
            entry.hex = Some(hex.replacen("b6 06 26", "b6 06 25", 1));
            changed += 1;
        }
    }
    say!("{changed} SET_TIME payloads changed in the capture");
    assert_eq!(
        changed, 3,
        "registration, live reconnection, final connection"
    );

    let strict = ReplayConnector::new(&header, entries.clone(), ReplayOptions::default()).unwrap();
    let library = tempfile::tempdir().unwrap();
    let (_, result) = replay(&strict, Some(id), library.path());
    let message = replay_message(result.unwrap_err());
    say!("strict: {message}");
    assert!(message.contains("expected tx b6 06 25"), "{message}");

    let options = ReplayOptions {
        lenient_set_time: true,
        ..ReplayOptions::default()
    };
    let lenient = ReplayConnector::new(&header, entries, options).unwrap();
    let library = tempfile::tempdir().unwrap();
    let (_, result) = replay(&lenient, Some(id), library.path());
    let replayed = result.unwrap();
    say!("lenient: replayed to the end, outcome {replayed:?}");
    assert_eq!(replayed, recorded);
    assert_eq!(lenient.remaining(), 0);
}
