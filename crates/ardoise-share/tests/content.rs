//! What the viewers receive (`docs/design/lan-share.md`, "Content, always chosen by the Ardoise
//! PC"), over real TCP with a fake live feed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::net::{Ipv4Addr, TcpStream};
use std::time::{Duration, Instant};

use ardoise_share::{NoteUpdate, Shown, ViewerGone};
use common::*;

const QUIET: Duration = Duration::from_millis(150);

fn full(id: &str, strokes: usize) -> NoteUpdate {
    NoteUpdate {
        full: note(id, strokes),
        append: None,
    }
}

fn grown(id: &str, from: usize, to: usize) -> NoteUpdate {
    NoteUpdate {
        full: note(id, to),
        append: Some(note_strokes(from, to)),
    }
}

#[test]
fn waiting_on_connect() {
    let h = Harness::start();
    let mut viewer = h.viewer();
    assert_eq!(viewer.expect(), waiting());
    viewer.expect_silence(QUIET);
    // Showing what is already shown sends nothing.
    h.share.show_waiting();
    viewer.expect_silence(QUIET);
}

#[test]
fn note_full_then_append_and_late_joiners() {
    let h = Harness::start();
    let mut a = h.viewer();
    assert_eq!(a.expect(), waiting());

    // Switching from another content always sends the whole note, even with `append`.
    h.share.show_note(grown("n1", 0, 1));
    assert_eq!(a.expect(), json(&note("n1", 1)));

    // Strokes added: `append` only.
    h.share.show_note(grown("n1", 1, 3));
    assert_eq!(a.expect(), json(&note_strokes(1, 3)));

    // A late joiner gets the whole note as it is now.
    let mut b = h.viewer();
    assert_eq!(b.expect(), json(&note("n1", 3)));

    // Then both follow the increments.
    h.share.show_note(grown("n1", 3, 4));
    assert_eq!(a.expect(), json(&note_strokes(3, 4)));
    assert_eq!(b.expect(), json(&note_strokes(3, 4)));

    // Another note (or strokes erased): the whole note.
    h.share.show_note(full("n2", 2));
    assert_eq!(a.expect(), json(&note("n2", 2)));
    assert_eq!(b.expect(), json(&note("n2", 2)));

    // The same note again: nothing.
    h.share.show_note(full("n2", 2));
    a.expect_silence(QUIET);

    // Waiting, then the note again: the whole note.
    h.share.show_waiting();
    assert_eq!(a.expect(), waiting());
    assert_eq!(b.expect(), waiting());
    h.share.show_note(grown("n2", 2, 3));
    assert_eq!(a.expect(), json(&note("n2", 3)));
    assert_eq!(b.expect(), json(&note("n2", 3)));
    assert_eq!(h.share.status().shown, Shown::Note);
}

#[test]
fn live_viewers_get_a_snapshot_then_the_batches() {
    let h = Harness::start();
    let mut a = h.viewer();
    assert_eq!(a.expect(), waiting());

    h.share.show_live();
    assert_eq!(h.feed.started().len(), 1);
    let live_a = h.feed.last();
    assert_eq!(a.expect(), live(true, &snapshot_batch(1)));
    live_a.send(&point_batch(1)).unwrap();
    live_a.send(&point_batch(2)).unwrap();
    assert_eq!(a.expect(), live(false, &point_batch(1)));
    assert_eq!(a.expect(), live(false, &point_batch(2)));

    // A late joiner gets its own subscription: a snapshot (reset), then the increments.
    let mut b = h.viewer();
    assert_eq!(h.feed.started().len(), 2);
    let live_b = h.feed.last();
    assert_ne!(live_b.id(), live_a.id());
    assert_eq!(b.expect(), live(true, &snapshot_batch(2)));
    live_b.send(&point_batch(3)).unwrap();
    assert_eq!(b.expect(), live(false, &point_batch(3)));
    a.expect_silence(QUIET);

    // Batches are passed verbatim, even pretty-printed ones (several `data:` lines).
    let pretty = "{\n  \"events\": [],\n  \"sentAt\": 7\n}";
    live_a.send(pretty).unwrap();
    assert_eq!(a.expect(), live(false, pretty));

    // Showing live mode again changes nothing.
    h.share.show_live();
    assert_eq!(h.feed.started().len(), 2);
    a.expect_silence(QUIET);
    assert!(h.feed.stopped().is_empty());
}

#[test]
fn switching_content_stops_the_feed() {
    let h = Harness::start();
    let mut a = h.viewer();
    assert_eq!(a.expect(), waiting());
    h.share.show_live();
    let first = h.feed.last();
    assert_eq!(a.expect(), live(true, &snapshot_batch(1)));

    h.share.show_waiting();
    assert_eq!(h.feed.stopped(), vec![first.id()]);
    assert_eq!(a.expect(), waiting());
    // A batch sent late by the old subscription is refused and never arrives.
    assert_eq!(first.send(&point_batch(1)), Err(ViewerGone));
    a.expect_silence(QUIET);

    // Live again: a new subscription, a new snapshot.
    h.share.show_live();
    let second = h.feed.last();
    assert_eq!(second.id(), first.id());
    assert_eq!(a.expect(), live(true, &snapshot_batch(2)));
    assert_eq!(first.send(&point_batch(2)), Err(ViewerGone));
    second.send(&point_batch(3)).unwrap();
    assert_eq!(a.expect(), live(false, &point_batch(3)));

    // A note replaces live mode.
    h.share.show_note(grown("n", 0, 2));
    assert_eq!(h.feed.stopped(), vec![first.id(), first.id()]);
    assert_eq!(a.expect(), json(&note("n", 2)));
    assert_eq!(second.send(&point_batch(4)), Err(ViewerGone));
    a.expect_silence(QUIET);
}

#[test]
fn pause_freezes_live_mode() {
    let h = Harness::start();
    let frozen = r#"{"events":[{"type":"snapshot","frozen":true}],"sentAt":9}"#;
    *h.feed.frozen.lock().unwrap() = Some(frozen.to_owned());
    let mut a = h.viewer();
    let mut b = h.viewer();
    assert_eq!(a.expect(), waiting());
    assert_eq!(b.expect(), waiting());
    h.share.show_live();
    let (live_a, live_b) = (h.feed.of(1), h.feed.of(2));
    assert!(a.expect()["reset"].as_bool().unwrap());
    assert!(b.expect()["reset"].as_bool().unwrap());

    h.share.set_paused(true);
    let mut stopped = h.feed.stopped();
    stopped.sort_unstable();
    assert_eq!(stopped, vec![1, 2]);
    for viewer in [&mut a, &mut b] {
        assert_eq!(viewer.expect(), live(true, frozen));
        assert_eq!(viewer.expect(), pause(true));
    }
    assert_eq!(live_a.send(&point_batch(1)), Err(ViewerGone));
    assert_eq!(live_b.send(&point_batch(1)), Err(ViewerGone));
    let status = h.share.status();
    assert!(status.paused);
    assert_eq!(status.shown, Shown::Live);

    // A viewer joining during the pause gets the frozen image and the pause; no subscription.
    let mut c = h.viewer();
    assert_eq!(c.expect(), live(true, frozen));
    assert_eq!(c.expect(), pause(true));
    assert_eq!(h.feed.started().len(), 2);

    // Changes during the pause are recorded, not sent.
    h.share.show_note(full("n", 1));
    h.share.show_live();
    h.share.set_paused(true);
    a.expect_silence(QUIET);
    c.expect_silence(Duration::ZERO);
    assert_eq!(h.feed.started().len(), 2);

    // Resume: the pause ends, then live mode starts again with a fresh snapshot for everyone.
    h.share.set_paused(false);
    assert_eq!(h.feed.started().len(), 5);
    for viewer in [&mut a, &mut b, &mut c] {
        assert_eq!(viewer.expect(), pause(false));
        let message = viewer.expect();
        assert_eq!(message["type"], "live");
        assert_eq!(message["reset"], true);
        assert_eq!(message["batch"]["events"][0]["type"], "snapshot");
    }
    h.feed.of(1).send(&point_batch(5)).unwrap();
    assert_eq!(a.expect(), live(false, &point_batch(5)));
    assert!(!h.share.status().paused);
}

#[test]
fn pause_without_a_session_freezes_the_waiting_screen() {
    let h = Harness::start();
    let mut a = h.viewer();
    assert_eq!(a.expect(), waiting());
    h.share.show_live();
    assert_eq!(a.expect()["reset"], true);
    // `freeze` gives nothing: the viewers all show the waiting screen.
    h.share.set_paused(true);
    assert_eq!(a.expect(), waiting());
    assert_eq!(a.expect(), pause(true));
    let mut b = h.viewer();
    assert_eq!(b.expect(), waiting());
    assert_eq!(b.expect(), pause(true));
}

#[test]
fn pause_with_a_note() {
    let h = Harness::start();
    let mut a = h.viewer();
    assert_eq!(a.expect(), waiting());
    h.share.show_note(full("n", 1));
    assert_eq!(a.expect(), json(&note("n", 1)));

    // The viewers show the note already: only the pause.
    h.share.set_paused(true);
    assert_eq!(a.expect(), pause(true));
    a.expect_silence(QUIET);

    // A late joiner gets the frozen note, even after changes made during the pause.
    h.share.show_note(grown("n", 1, 2));
    let mut b = h.viewer();
    assert_eq!(b.expect(), json(&note("n", 1)));
    assert_eq!(b.expect(), pause(true));
    h.share.show_note(grown("n", 2, 3));
    a.expect_silence(QUIET);
    b.expect_silence(Duration::ZERO);

    // Resume: the pause ends and the note as it is now, whole.
    h.share.set_paused(false);
    for viewer in [&mut a, &mut b] {
        assert_eq!(viewer.expect(), pause(false));
        assert_eq!(viewer.expect(), json(&note("n", 3)));
    }
    // Then increments again.
    h.share.show_note(grown("n", 3, 4));
    assert_eq!(a.expect(), json(&note_strokes(3, 4)));
    assert_eq!(b.expect(), json(&note_strokes(3, 4)));

    // A pause without change: nothing else is sent at resume.
    h.share.set_paused(true);
    h.share.set_paused(false);
    assert_eq!(a.expect(), pause(true));
    assert_eq!(a.expect(), pause(false));
    a.expect_silence(QUIET);

    // Pausing twice, or resuming when not paused, sends nothing.
    h.share.set_paused(false);
    a.expect_silence(QUIET);
}

#[test]
fn a_viewer_that_leaves_is_stopped() {
    let h = Harness::start();
    h.share.show_live();
    let mut a = h.viewer();
    let b = h.viewer();
    assert_eq!(a.expect()["reset"], true);
    assert_eq!(h.share.status().viewers, 2);
    let live_b = h.feed.of(2);

    drop(b);
    assert!(wait_until(SOON, || h.feed.stopped() == vec![2]));
    assert!(wait_until(SOON, || h.share.status().viewers == 1));
    assert_eq!(live_b.send(&point_batch(1)), Err(ViewerGone));

    // The other viewer is not affected.
    h.feed.of(1).send(&point_batch(2)).unwrap();
    assert_eq!(a.expect(), live(false, &point_batch(2)));
}

#[test]
fn status_and_changes() {
    let h = Harness::start();
    let status = h.share.status();
    assert_eq!(status.port, h.port());
    assert_eq!(status.code, CODE);
    assert_eq!(h.share.code(), CODE);
    assert_eq!(status.viewers, 0);
    assert!(!status.paused);
    assert_eq!(status.shown, Shown::Waiting);
    assert_eq!(
        serde_json::to_value(&status).unwrap(),
        serde_json::json!({ "port": h.port(), "code": CODE, "viewers": 0, "paused": false, "shown": "waiting" })
    );

    let a = h.viewer();
    assert!(wait_until(SOON, || h
        .last_change()
        .is_some_and(|s| s.viewers == 1)));
    let b = h.viewer();
    assert!(wait_until(SOON, || h
        .last_change()
        .is_some_and(|s| s.viewers == 2)));

    h.share.show_live();
    assert!(wait_until(SOON, || h
        .last_change()
        .is_some_and(|s| s.shown == Shown::Live)));
    h.share.set_paused(true);
    assert!(wait_until(SOON, || h
        .last_change()
        .is_some_and(|s| s.paused)));
    h.share.show_note(full("n", 1));
    assert!(wait_until(SOON, || h
        .last_change()
        .is_some_and(|s| s.shown == Shown::Note && s.paused)));

    drop(a);
    drop(b);
    assert!(wait_until(SOON, || h
        .last_change()
        .is_some_and(|s| s.viewers == 0)));
    let last = h.last_change().unwrap();
    assert_eq!(last, h.share.status());
    assert_eq!(last.port, h.port());
    assert_eq!(last.code, CODE);
}

#[test]
fn stop_sends_end_and_closes_the_port() {
    let h = Harness::start();
    let port = h.port();
    let mut a = h.viewer();
    let mut b = h.viewer();
    assert_eq!(a.expect(), waiting());
    assert_eq!(b.expect(), waiting());
    h.share.show_live();
    assert_eq!(a.expect()["reset"], true);
    assert_eq!(b.expect()["reset"], true);
    let live_a = h.feed.of(1);
    assert!(wait_until(SOON, || h
        .last_change()
        .is_some_and(|s| s.shown == Shown::Live && s.viewers == 2)));

    let started = Instant::now();
    h.share.stop();
    let took = started.elapsed();
    assert!(took < Duration::from_secs(2), "{took:?}");
    eprintln!("stop took {took:?}");

    for viewer in [&mut a, &mut b] {
        let mut seen = Vec::new();
        assert!(viewer.read_until_closed(SOON, &mut seen), "closed");
        assert_eq!(seen.last(), Some(&end()), "{seen:?}");
    }
    let mut stopped = h.feed.stopped();
    stopped.sort_unstable();
    assert_eq!(stopped, vec![1, 2]);
    assert_eq!(live_a.send(&point_batch(1)), Err(ViewerGone));
    assert!(
        TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err(),
        "the port is closed"
    );

    // Repeated, and everything else, is harmless; no change is reported any more.
    let changes = h.changes.lock().unwrap().len();
    h.share.stop();
    h.share.show_waiting();
    h.share.show_note(full("n", 1));
    h.share.set_paused(true);
    h.share.show_live();
    std::thread::sleep(QUIET);
    assert_eq!(h.changes.lock().unwrap().len(), changes);
    assert_eq!(h.share.status().viewers, 0);
    assert_eq!(h.feed.started().len(), 2);
}

#[test]
fn stop_with_a_viewer_that_does_not_read() {
    let h = Harness::start();
    let port = h.port();
    h.share.show_live();
    let mut stuck = h.viewer();
    let live = h.feed.last();
    // Fill its socket: the server is blocked writing to it.
    let big = format!(
        r#"{{"events":[{{"type":"points","data":"{}"}}]}}"#,
        "x".repeat(256 * 1024)
    );
    for _ in 0..64 {
        if live.send(&big).is_err() {
            break;
        }
    }
    std::thread::sleep(Duration::from_millis(100));
    let started = Instant::now();
    h.share.stop();
    let took = started.elapsed();
    assert!(took < Duration::from_millis(2500), "{took:?}");
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
    let mut seen = Vec::new();
    assert!(stuck.read_until_closed(Duration::from_secs(10), &mut seen));
}
