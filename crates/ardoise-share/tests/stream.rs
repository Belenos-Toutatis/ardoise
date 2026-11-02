//! The event stream under load: a slow viewer is disconnected without ever blocking the sender,
//! and the latency of a live batch to a client on localhost.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::time::{Duration, Instant};

use common::*;

#[test]
fn a_slow_viewer_is_disconnected_without_blocking_the_sender() {
    let h = Harness::start();
    h.share.show_live();
    let mut fast = h.viewer();
    assert_eq!(fast.expect()["reset"], true);
    // Opens its stream, then never reads again.
    let _slow = h.viewer();
    let slow = h.feed.of(2);
    let fast_feed = h.feed.of(1);

    let batch = format!(
        r#"{{"events":[{{"type":"points","data":"{}"}}],"sentAt":0}}"#,
        "x".repeat(16 * 1024)
    );
    let mut sent = 0usize;
    let mut slowest = Duration::ZERO;
    let started = Instant::now();
    let mut refused = false;
    while sent < 100_000 {
        let before = Instant::now();
        let result = slow.send(&batch);
        slowest = slowest.max(before.elapsed());
        if result.is_err() {
            refused = true;
            break;
        }
        sent += 1;
        // The fast viewer keeps up meanwhile.
        if sent.is_multiple_of(256) {
            fast_feed.send(&point_batch(sent as u64)).unwrap();
            assert_eq!(fast.expect(), live(false, &point_batch(sent as u64)));
        }
    }
    let total = started.elapsed();
    eprintln!(
        "slow viewer refused after {sent} batches of 16 KiB in {total:?}; slowest send {slowest:?}"
    );
    assert!(refused, "the slow viewer was disconnected");
    assert!(sent >= 1024, "the queue holds 1024 messages at least");
    // A send that waited for the network would wait forever here (the client never reads): any
    // bound well above a scheduler delay shows that none did.
    assert!(
        slowest < Duration::from_millis(250),
        "a send never blocks: {slowest:?}"
    );

    // It is removed and its feed stopped; once refused, always refused.
    assert!(wait_until(SOON, || h.feed.stopped() == vec![2]));
    assert!(wait_until(SOON, || h.share.status().viewers == 1));
    assert!(slow.send(&point_batch(0)).is_err());

    // The fast viewer still gets everything.
    fast_feed.send(&point_batch(1)).unwrap();
    assert_eq!(fast.expect(), live(false, &point_batch(1)));
}

#[test]
fn latency_of_a_live_batch_on_localhost() {
    let h = Harness::start();
    h.share.show_live();
    let mut viewer = h.viewer();
    assert_eq!(viewer.expect()["reset"], true);
    let feed = h.feed.last();

    let mut samples = Vec::new();
    for i in 0..300u64 {
        let batch = point_batch(i);
        let sent = Instant::now();
        feed.send(&batch).unwrap();
        let message = viewer.expect();
        samples.push(sent.elapsed());
        assert_eq!(message, live(false, &batch));
        // A pause between batches, as between two flushes of the hub.
        std::thread::sleep(Duration::from_millis(2));
    }
    samples.sort_unstable();
    let median = samples[samples.len() / 2];
    let p95 = samples[samples.len() * 95 / 100];
    let max = samples[samples.len() - 1];
    eprintln!(
        "LiveViewer::send to bytes read on localhost: median {median:?}, p95 {p95:?}, max {max:?}"
    );
    assert!(median < Duration::from_millis(20), "median {median:?}");
}
