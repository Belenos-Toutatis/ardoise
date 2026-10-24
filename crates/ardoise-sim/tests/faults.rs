//! Faults injected in the simulated tablet, seen through `Manager` and `Session`: corrupted
//! transfers (one retry, never a deletion of an unsaved page) and links dropped by the
//! tablet around live mode.
//!
//! `cargo test -p ardoise-sim --test faults -- --nocapture` prints each case.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod common;

use std::time::Duration;

use ardoise_device::identity::DeviceRecord;
use ardoise_device::log::DiagnosticLog;
use ardoise_device::manager::Manager;
use ardoise_device::session::{DeviceError, Session};
use ardoise_proto::live::{LivePacket, LiveRecord};
use ardoise_sim::{SimConfig, SimTablet, drawings};
use common::points;

const WAIT: Duration = Duration::from_secs(10);

/// A manager over `tablet`, registered, with `pages` reference pages on the tablet.
fn registered(
    tablet: &SimTablet,
    library: &std::path::Path,
    pages: usize,
) -> (Manager, Session, DeviceRecord) {
    let config = tablet.config();
    for page in drawings::reference_pages(config.width, config.height)
        .iter()
        .take(pages)
    {
        tablet.draw_page(page);
    }
    let log = DiagnosticLog::in_memory("sim", tablet.clock()).shared();
    let mut manager = Manager::new(Box::new(tablet.connector()), library.to_owned(), log);
    tablet.hold_button();
    let found = manager.find(true, WAIT).unwrap().unwrap();
    let (session, _) = manager
        .register(&found, WAIT, &mut || tablet.press_button())
        .unwrap();
    let record = manager.registration().unwrap().unwrap();
    (manager, session, record)
}

fn samples(packet: &LivePacket) -> u64 {
    match packet {
        LivePacket::Samples(records) => records
            .iter()
            .filter(|r| matches!(r, LiveRecord::Pen(_)))
            .count() as u64,
        _ => 0,
    }
}

#[test]
fn a_corrupted_page_is_downloaded_again_once() {
    let _report = common::report("a_corrupted_page_is_downloaded_again_once");
    let tablet = SimTablet::slate_large();
    let library = tempfile::tempdir().unwrap();
    let (_manager, mut session, _) = registered(&tablet, library.path(), 2);
    let expected = tablet.stored_pages();
    tablet.corrupt_next_transfer();
    let mut received = Vec::new();
    let summary = session
        .sync(true, &mut |page, _, _| {
            received.push(page);
            Ok(())
        })
        .unwrap();
    say!(
        "first transfer corrupted: {} downloads for {} pages, {} deleted",
        tablet.stats().downloads,
        summary.downloaded,
        summary.deleted
    );
    assert_eq!((summary.downloaded, summary.deleted), (2, 2));
    assert_eq!(tablet.stats().downloads, 3, "one retry");
    for (page, stored) in received.iter().zip(&expected) {
        assert_eq!(page.bytes, stored.bytes);
    }
    assert_eq!(tablet.stored_page_count(), 0);
}

#[test]
fn two_corrupted_transfers_in_a_row_fail_the_sync_and_delete_nothing() {
    let _report =
        common::report("two_corrupted_transfers_in_a_row_fail_the_sync_and_delete_nothing");
    let tablet = SimTablet::slate_large();
    let library = tempfile::tempdir().unwrap();
    let (_manager, mut session, _) = registered(&tablet, library.path(), 2);
    tablet.corrupt_next_transfer();
    tablet.corrupt_next_transfer();
    let mut sink_calls = 0;
    let result = session.sync(true, &mut |_, _, _| {
        sink_calls += 1;
        Ok(())
    });
    say!("download and retry corrupted: {result:?}");
    assert!(
        matches!(result, Err(DeviceError::CrcMismatch { .. })),
        "{result:?}"
    );
    assert_eq!(sink_calls, 0);
    assert_eq!(tablet.stats().downloads, 2);
    assert_eq!(tablet.stored_page_count(), 2, "nothing deleted");

    say!("the next sync gets both pages");
    let mut pages = Vec::new();
    let summary = session
        .sync(true, &mut |page, _, _| {
            pages.push(page);
            Ok(())
        })
        .unwrap();
    assert_eq!(summary.deleted, 2);
    let decoded = ardoise_proto::stroke::decode(&pages[0].bytes).unwrap();
    assert_eq!(
        points(&decoded.strokes),
        points(&drawings::square_and_circle(29_700, 21_600))
    );
}

#[test]
fn live_mode_reconnects_when_the_link_was_lost_while_the_user_read_the_prompt() {
    let _report = common::report(
        "live_mode_reconnects_when_the_link_was_lost_while_the_user_read_the_prompt",
    );
    let config = SimConfig {
        idle_timeout: Some(Duration::from_secs(10)),
        ..SimConfig::slate_large()
    };
    let tablet = SimTablet::new(config);
    let library = tempfile::tempdir().unwrap();
    let (mut manager, mut session, record) = registered(&tablet, library.path(), 0);
    let clock = tablet.clock();
    // The user takes 15 s to read the prompt: the tablet drops the idle link meanwhile.
    clock.advance(Duration::from_secs(15));
    assert!(!tablet.is_connected());
    let script = drawings::live_script(5, 29_700, 21_600);
    let expected = script.sample_count() as u64;
    tablet.draw_live(script);
    let start = clock.monotonic();
    let mut received = 0;
    let reconnections = manager
        .live(
            &mut session,
            &record,
            start + Duration::from_secs(5),
            &mut |packet| received += samples(packet),
        )
        .unwrap();
    say!(
        "link lost before live mode: {reconnections} reconnection, {received} samples of {expected}"
    );
    assert_eq!(reconnections, 1);
    assert_eq!(received, expected, "the pen script starts with live mode");
    assert_eq!(clock.monotonic() - start, Duration::from_secs(5));
    assert_eq!(tablet.stats().connections, 2);
}

#[test]
fn live_mode_reconnects_after_each_idle_disconnection() {
    let _report = common::report("live_mode_reconnects_after_each_idle_disconnection");
    let config = SimConfig {
        idle_timeout: Some(Duration::from_secs(10)),
        ..SimConfig::slate_large()
    };
    let tablet = SimTablet::new(config);
    let library = tempfile::tempdir().unwrap();
    let (mut manager, mut session, record) = registered(&tablet, library.path(), 0);
    let script = drawings::live_script(30, 29_700, 21_600);
    let expected = script.sample_count() as u64;
    tablet.draw_live(script);
    let clock = tablet.clock();
    let start = clock.monotonic();
    let mut received = 0;
    let reconnections = manager
        .live(
            &mut session,
            &record,
            start + Duration::from_secs(30),
            &mut |packet| received += samples(packet),
        )
        .unwrap();
    let stats = tablet.stats();
    say!(
        "idle drops at 10, 20 and 30 s: {reconnections} reconnections, {} links lost, \
         {received} samples of {expected}",
        stats.links_lost
    );
    assert_eq!(reconnections, 2);
    assert_eq!(stats.links_lost, 3);
    assert_eq!(received, stats.live_samples, "nothing lost by the host");
    assert!(expected - received <= 6, "{}", expected - received);
    assert_eq!(clock.monotonic() - start, Duration::from_secs(30));
}
