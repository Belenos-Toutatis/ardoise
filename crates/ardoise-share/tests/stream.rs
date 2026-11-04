//! The event stream under load: a slow viewer is disconnected without ever blocking the sender,
//! the latency of a live batch to a client on localhost, pings, and `end` after a long write.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::{Read, Write};
use std::time::{Duration, Instant};

use ardoise_share::Limits;
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
    // 1024 messages are held (queued, or taken and not yet written); the feed's snapshot may be
    // one of them.
    assert!(sent >= 1023, "the queue holds 1024 messages");
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

#[test]
fn idle_streams_get_pings() {
    let h = Harness::with_limits(Limits {
        ping: Duration::from_millis(100),
        ..Limits::default()
    });
    let mut viewer = h.viewer();
    assert_eq!(viewer.expect(), waiting());
    for _ in 0..2 {
        assert_eq!(
            viewer.next_event(SOON),
            Some(SseEvent::Comment("ping".into()))
        );
    }
    // Messages still come between them.
    h.share.show_live();
    assert_eq!(viewer.expect()["reset"], true);
}

#[test]
fn at_stop_a_long_write_is_finished_before_end() {
    let h = Harness::start();
    h.share.show_live();
    let mut viewer = h.viewer();
    assert_eq!(viewer.expect()["reset"], true);
    let live = h.feed.last();
    // More than the socket buffers hold: the server is blocked in the middle of a write.
    let big = format!(
        r#"{{"events":[{{"type":"points","data":"{}"}}],"sentAt":0}}"#,
        "x".repeat(256 * 1024)
    );
    for _ in 0..64 {
        live.send(&big).unwrap();
    }
    std::thread::sleep(Duration::from_millis(200));

    // The share stops while the viewer reads again.
    let share = &h.share;
    std::thread::scope(|scope| {
        let stopper = scope.spawn(|| {
            let started = Instant::now();
            share.stop();
            started.elapsed()
        });
        let mut seen = Vec::new();
        assert!(viewer.read_until_closed(SOON, &mut seen), "closed");
        let took = stopper.join().unwrap();
        eprintln!("stop took {took:?}; {} messages before end", seen.len() - 1);
        assert!(took < Duration::from_secs(2), "{took:?}");
        assert_eq!(seen.last(), Some(&end()), "end comes last");
        assert_eq!(viewer.leftover(), 0, "no message cut in the middle");
        assert!(seen[..seen.len() - 1].iter().all(|m| m["type"] == "live"));
    });
}

/// The bytes of an event stream before its first message.
fn bytes_before_first_message(port: u16, cookie: &str, extra: &str) -> usize {
    let mut stream = connect(port);
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    write!(
        stream,
        "GET /events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\n{extra}\r\n"
    )
    .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 65536];
    loop {
        if let Some(at) = bytes.windows(5).position(|w| w == b"data:") {
            return at;
        }
        let n = stream.read(&mut chunk).expect("the stream goes on");
        assert!(n > 0, "closed before the first message");
        bytes.extend_from_slice(&chunk[..n]);
    }
}

#[test]
fn a_stream_through_a_proxy_starts_with_enough_to_pass_its_filter() {
    let h = Harness::start();
    let cookie = h.cookie();
    let direct = bytes_before_first_message(h.port(), &cookie, "");
    assert!(direct < 1024, "{direct} bytes before the first message");
    // e2guardian held streams up to 450 KB, at a school (2026-11-04).
    for proxy in [
        "X-Forwarded-For: 192.168.1.2\r\n",
        "Via: 1.1 proxy\r\n",
        "Forwarded: for=192.168.1.2\r\n",
    ] {
        let padded = bytes_before_first_message(h.port(), &cookie, proxy);
        assert!(padded >= 2 * 1024 * 1024, "{proxy:?}: {padded} bytes");
    }
}
