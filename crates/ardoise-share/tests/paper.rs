//! `GET /paper/<key>`, the picture of the paper of a live page, over real TCP: who gets it, what
//! else can be reached (nothing), and a slow download that leaves the event stream alone.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ardoise_share::{Limits, LiveFeed, LivePaper, NoteUpdate, Share, ShareConfig};
use common::*;
use serde_json::Value;

const KEY: &str = "0123456789abcdef";

/// `len` bytes that do not repeat, as compressed pixels do not: a test sees that they all arrive
/// in order.
fn picture(len: usize) -> Arc<[u8]> {
    let mut seed = 0x2545_f491_4f6c_dd1d_u64;
    let bytes: Vec<u8> = (0..len)
        .map(|_| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed as u8
        })
        .collect();
    Arc::from(bytes)
}

fn paper(key: &str, png: &Arc<[u8]>) -> Option<LivePaper> {
    Some(LivePaper {
        key: key.to_owned(),
        png: Arc::clone(png),
    })
}

fn request_with_cookie(port: u16, method: &str, target: &str, cookie: &str) -> Vec<u8> {
    format!("{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\n\r\n")
        .into_bytes()
}

/// A share in live mode with the paper `KEY` held, and the cookie of a viewer.
fn sharing(limits: Limits, png: &Arc<[u8]>) -> (Harness, String) {
    let h = Harness::with_limits(limits);
    h.share.show_live();
    h.share.show_live_paper(paper(KEY, png));
    let cookie = h.cookie();
    (h, cookie)
}

fn fetch(h: &Harness, cookie: &str, key: &str) -> Response {
    let target = format!("/paper/{key}");
    exchange(
        h.port(),
        &request_with_cookie(h.port(), "GET", &target, cookie),
    )
    .expect("an answer")
}

/// A download that was asked for and is not being read.
fn start_download(port: u16, cookie: &str, key: &str) -> TcpStream {
    let mut stream = connect(port);
    let target = format!("/paper/{key}");
    stream
        .write_all(&request_with_cookie(port, "GET", &target, cookie))
        .unwrap();
    stream
}

/// Reads until the server closes the connection or `within`: all the bytes, and whether it closed.
fn read_to_end(stream: &mut TcpStream, within: Duration) -> (Vec<u8>, bool) {
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    let deadline = Instant::now() + within;
    let mut bytes = Vec::new();
    let mut chunk = vec![0u8; 256 * 1024];
    while Instant::now() < deadline {
        match stream.read(&mut chunk) {
            Ok(0) => return (bytes, true),
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => return (bytes, true),
        }
    }
    (bytes, false)
}

/// The share let go of `png`: only the test holds it.
fn let_go(png: &Arc<[u8]>) -> bool {
    wait_until(SOON, || Arc::strong_count(png) == 1)
}

#[test]
fn the_picture_is_a_complete_answer_for_a_viewer_that_joined() {
    let png = picture(100_000);
    let (h, cookie) = sharing(Limits::default(), &png);
    let answer = fetch(&h, &cookie, KEY);
    assert_eq!(answer.status, 200);
    assert_eq!(answer.body, &*png);
    assert_eq!(answer.header("content-type"), Some("image/png"));
    assert_eq!(answer.header("content-length"), Some("100000"));
    assert_eq!(answer.header("cache-control"), Some("no-store"));
    assert_eq!(answer.header("connection"), Some("close"));
    assert_eq!(answer.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(answer.header("referrer-policy"), Some("no-referrer"));
    assert!(answer.header("set-cookie").is_none());

    // The query string is ignored, as everywhere.
    assert_eq!(fetch(&h, &cookie, &format!("{KEY}?v=1")).status, 200);
}

#[test]
fn without_the_cookie_nothing_tells_whether_a_key_exists() {
    let png = picture(1000);
    let (h, _) = sharing(Limits::default(), &png);
    let port = h.port();
    for key in [KEY, "ffffffffffffffff", "", "../events", "x"] {
        let target = format!("/paper/{key}");
        for answer in [
            h.get(&target),
            exchange(
                port,
                &request_with_cookie(port, "GET", &target, "ardoise_share=0123456789abcdef"),
            )
            .unwrap(),
            exchange(port, &request_with_cookie(port, "GET", &target, "other=1")).unwrap(),
        ] {
            assert_eq!(answer.status, 401, "{target}");
            assert_eq!(
                answer.header("content-type"),
                Some("text/plain; charset=utf-8")
            );
            assert_eq!(answer.body, b"Unauthorized\n");
        }
    }
    // A foreign `Host` is refused before anything, with the right cookie too.
    let cookie = h.cookie();
    let raw =
        format!("GET /paper/{KEY} HTTP/1.1\r\nHost: evil.example\r\nCookie: {cookie}\r\n\r\n");
    assert_eq!(exchange(port, raw.as_bytes()).unwrap().status, 421);
}

#[test]
fn with_the_cookie_only_the_current_key_gives_anything() {
    let png = picture(1000);
    let (h, cookie) = sharing(Limits::default(), &png);
    let port = h.port();
    // Nothing is decoded nor looked up: the key is compared as it is.
    for key in [
        "",
        "/",
        "0123456789abcde",
        "0123456789abcdef0",
        "0123456789ABCDEF",
        "ffffffffffffffff",
        "0123456789abcdef/",
        "0123456789abcdef/..",
        "./0123456789abcdef",
        "../0123456789abcdef",
        "%30123456789abcdef",
        "0123456789abcdef%00",
        "..",
        "../..",
        "../events",
        "../assets/viewer-abc123.js",
        "..%2f..%2f..%2fardoise.toml",
        "C:\\Windows\\win.ini",
        "%2e%2e%5c%2e%2e%5cardoise.toml",
        "_ardoise/device.json",
        "photo.png",
    ] {
        let answer = fetch(&h, &cookie, key);
        assert_eq!(answer.status, 404, "{key}");
        assert_eq!(answer.body, b"Not Found\n", "{key}");
    }
    // Neither any other way to name the route, nor any other method.
    for target in [
        "/paper",
        "//paper/0123456789abcdef",
        "/PAPER/0123456789abcdef",
        "/paper%2F0123456789abcdef",
        "/assets/0123456789abcdef",
        "/assets/../paper/0123456789abcdef",
        "/0123456789abcdef",
    ] {
        let answer = exchange(port, &request_with_cookie(port, "GET", target, &cookie)).unwrap();
        assert_eq!(answer.status, 404, "{target}");
    }
    for method in ["POST", "PUT", "DELETE", "PATCH", "OPTIONS", "HEAD", "get"] {
        let target = format!("/paper/{KEY}");
        let answer = exchange(port, &request_with_cookie(port, method, &target, &cookie)).unwrap();
        assert_eq!(answer.status, 404, "{method}");
    }
}

#[test]
fn only_the_paper_of_what_is_shown_can_be_fetched() {
    let png = picture(1000);
    let later = picture(2000);
    let h = Harness::start();
    let cookie = h.cookie();
    *h.feed.frozen.lock().unwrap() = Some(snapshot_batch(0));

    // Recorded, not shown: the waiting screen.
    h.share.show_live_paper(paper(KEY, &png));
    assert_eq!(fetch(&h, &cookie, KEY).status, 404);
    h.share.show_live();
    assert_eq!(fetch(&h, &cookie, KEY).status, 200);

    // A pause freezes the paper; the one made meanwhile is not shown yet, whatever is asked.
    h.share.set_paused(true);
    h.share.show_live_paper(paper("fedcba9876543210", &later));
    assert_eq!(fetch(&h, &cookie, KEY).body, &*png);
    assert_eq!(fetch(&h, &cookie, "fedcba9876543210").status, 404);
    h.share.set_paused(false);
    assert_eq!(fetch(&h, &cookie, KEY).status, 404);
    assert_eq!(fetch(&h, &cookie, "fedcba9876543210").body, &*later);

    // The editor's note: nothing of the sheet.
    h.share.show_note(NoteUpdate {
        full: note("n1", 1),
        append: None,
    });
    assert_eq!(fetch(&h, &cookie, "fedcba9876543210").status, 404);
    h.share.show_live();
    assert_eq!(fetch(&h, &cookie, "fedcba9876543210").status, 200);

    // A plain page: nothing.
    h.share.show_live_paper(None);
    assert_eq!(fetch(&h, &cookie, "fedcba9876543210").status, 404);
    assert!(let_go(&later));
}

#[test]
fn the_stream_says_where_the_picture_is_and_never_carries_it() {
    let png = picture(100_000);
    let h = Harness::start();
    let mut viewer = h.viewer();
    assert_eq!(viewer.expect(), waiting());
    h.share.show_live();
    assert_eq!(viewer.expect()["reset"], true);

    h.share.show_live_paper(paper(KEY, &png));
    let told = viewer.expect();
    assert_eq!(told["type"], "livePaper");
    assert_eq!(told["key"], KEY);
    let keys: Vec<&str> = told
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["key", "type", "version"]);
    assert!(told.to_string().len() < 100, "{told}");

    // The same picture again: no message. Other pixels under the same key: another version, and
    // the address now answers with them.
    h.share.show_live_paper(paper(KEY, &png));
    viewer.expect_silence(Duration::from_millis(100));
    let other = picture(50_000);
    h.share.show_live_paper(paper(KEY, &other));
    let again = viewer.expect();
    assert_eq!(again["key"], KEY);
    assert_ne!(again["version"], told["version"]);
    assert!(let_go(&png));
    let cookie = h.cookie();
    assert_eq!(fetch(&h, &cookie, KEY).body, &*other);
}

/// A reader that is slow, at about `rate` bytes a second: the whole answer, and how long it took.
fn read_slowly(stream: &mut TcpStream, rate: usize, within: Duration) -> (Vec<u8>, Duration) {
    let started = Instant::now();
    stream
        .set_read_timeout(Some(Duration::from_millis(500)))
        .unwrap();
    let step = Duration::from_millis(50);
    let mut chunk = vec![0u8; rate / 20];
    let mut bytes = Vec::new();
    while started.elapsed() < within {
        let tick = Instant::now();
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => bytes.extend_from_slice(&chunk[..n]),
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(error) => panic!("the download was cut: {error}"),
        }
        // Complete: a browser closes here.
        if let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&bytes[..at]).to_lowercase();
            let length: usize = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .map_or(usize::MAX, |length| length.parse().unwrap());
            if bytes.len() >= at + 4 + length {
                break;
            }
        }
        std::thread::sleep(step.saturating_sub(tick.elapsed()));
    }
    (bytes, started.elapsed())
}

/// The finding that made this route (`docs/design/lan-share.md`, "The paper of a live page"): a
/// photo written in the stream, at less than about 700 KB/s, got the viewer disconnected after 15 s
/// with the picture unfinished, for ever. Here the download takes many times the time allowed for
/// any 64 KiB, while the batches go on at the pace of a pen.
#[test]
fn a_slow_download_leaves_the_stream_alone() {
    let png = picture(8_000_000);
    let limits = Limits {
        stall: Duration::from_secs(1),
        ..Limits::default()
    };
    let (h, cookie) = sharing(limits, &png);
    let mut viewer = open_events(h.port(), Some(&cookie)).unwrap();
    assert_eq!(viewer.next_event(SOON), Some(SseEvent::Retry(2000)));
    assert_eq!(viewer.expect()["reset"], true);
    assert_eq!(viewer.expect()["type"], "livePaper");
    let live = h.feed.last();

    let mut download = start_download(h.port(), &cookie, KEY);
    let reader =
        std::thread::spawn(move || read_slowly(&mut download, 1_000_000, Duration::from_secs(60)));

    // 60 batches a second, each read as it comes, until the picture has arrived.
    let mut slowest = Duration::ZERO;
    let mut batches = 0u64;
    while !reader.is_finished() {
        let batch = point_batch(batches);
        let sent = Instant::now();
        live.send(&batch).expect("the viewer stays");
        assert_eq!(viewer.expect(), live_message(&batch));
        slowest = slowest.max(sent.elapsed());
        batches += 1;
        std::thread::sleep(Duration::from_millis(16));
    }
    let (bytes, took) = reader.join().unwrap();
    let at = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    assert_eq!(&bytes[at + 4..], &*png, "the whole picture, in order");
    eprintln!(
        "8 MB read in {took:?} with {batches} batches on the stream meanwhile, slowest {slowest:?}"
    );
    assert!(
        took > Duration::from_secs(2),
        "the reader was slow: {took:?}"
    );
    assert!(batches > 60, "{batches}");
    assert!(slowest < Duration::from_secs(1), "{slowest:?}");
    assert_eq!(h.share.status().viewers, 1);
}

fn live_message(batch: &str) -> Value {
    live(false, batch)
}

/// More than the buffers of a socket on loopback can hold (Windows grows them to megabytes).
const LARGER_THAN_BUFFERS: usize = 64 * 1024 * 1024;

fn large_picture() -> Arc<[u8]> {
    Arc::from(vec![7u8; LARGER_THAN_BUFFERS])
}

#[test]
fn a_client_that_stops_reading_loses_its_slot() {
    let png = large_picture();
    let limits = Limits {
        stall: Duration::from_millis(300),
        ..Limits::default()
    };
    let (h, cookie) = sharing(limits, &png);
    let mut stuck = start_download(h.port(), &cookie, KEY);
    // The server gives up: the rest of the picture never comes, however fast it is read from now on.
    std::thread::sleep(Duration::from_millis(1000));
    let (bytes, closed) = read_to_end(&mut stuck, SOON);
    assert!(closed, "the server closed it");
    assert!(bytes.len() < LARGER_THAN_BUFFERS, "{}", bytes.len());
}

#[test]
fn a_download_has_a_time_limit_however_it_reads() {
    let png = large_picture();
    let limits = Limits {
        paper_answer: Duration::from_millis(500),
        ..Limits::default()
    };
    let (h, cookie) = sharing(limits, &png);
    let mut download = start_download(h.port(), &cookie, KEY);
    let started = Instant::now();
    download
        .set_read_timeout(Some(Duration::from_millis(100)))
        .unwrap();
    // A client that keeps reading a little, for ever: the slot still comes back.
    let mut chunk = [0u8; 1024];
    let mut closed = false;
    let mut total = 0;
    while started.elapsed() < Duration::from_secs(30) {
        match download.read(&mut chunk) {
            Ok(0) => {
                closed = true;
                break;
            }
            Ok(n) => total += n,
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
            Err(_) => {
                closed = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(closed, "still open after {:?}", started.elapsed());
    assert!(total < LARGER_THAN_BUFFERS, "{total}");
}

#[test]
fn a_download_ends_when_the_paper_changes() {
    let png = large_picture();
    let (h, cookie) = sharing(Limits::default(), &png);
    let mut stuck = start_download(h.port(), &cookie, KEY);
    std::thread::sleep(Duration::from_millis(100));

    let other = picture(1000);
    h.share.show_live_paper(paper("fedcba9876543210", &other));
    assert!(let_go(&png), "nothing is kept for a slow client");
    let (bytes, closed) = read_to_end(&mut stuck, Duration::from_secs(30));
    assert!(closed);
    assert!(bytes.len() < LARGER_THAN_BUFFERS, "{}", bytes.len());

    assert_eq!(fetch(&h, &cookie, KEY).status, 404);
    assert_eq!(fetch(&h, &cookie, "fedcba9876543210").body, &*other);
}

#[test]
fn a_download_ends_when_the_sheet_is_no_longer_shown() {
    let png = large_picture();
    let (h, cookie) = sharing(Limits::default(), &png);
    let mut stuck = start_download(h.port(), &cookie, KEY);
    std::thread::sleep(Duration::from_millis(100));
    h.share.show_waiting();
    let (bytes, closed) = read_to_end(&mut stuck, Duration::from_secs(30));
    assert!(closed);
    assert!(bytes.len() < LARGER_THAN_BUFFERS, "{}", bytes.len());
}

#[test]
fn a_pause_leaves_a_download_of_the_frozen_paper_alone() {
    let png = picture(1_000_000);
    let (h, cookie) = sharing(Limits::default(), &png);
    *h.feed.frozen.lock().unwrap() = Some(snapshot_batch(0));
    let mut download = start_download(h.port(), &cookie, KEY);
    std::thread::sleep(Duration::from_millis(50));
    h.share.set_paused(true);
    let (bytes, _) = read_slowly(&mut download, 10_000_000, SOON);
    let at = bytes.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    assert_eq!(&bytes[at + 4..], &*png);
}

#[test]
fn stop_ends_the_downloads_and_lets_the_picture_go() {
    let png = large_picture();
    let (h, cookie) = sharing(Limits::default(), &png);
    let port = h.port();
    let mut stuck = start_download(port, &cookie, KEY);
    std::thread::sleep(Duration::from_millis(100));

    let started = Instant::now();
    h.share.stop();
    let took = started.elapsed();
    assert!(took < Duration::from_millis(1500), "{took:?}");
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
    assert_eq!(Arc::strong_count(&png), 1, "nothing left of it");
    let (bytes, closed) = read_to_end(&mut stuck, Duration::from_secs(30));
    assert!(closed);
    assert!(bytes.len() < LARGER_THAN_BUFFERS, "{}", bytes.len());
}

#[test]
fn downloads_count_as_requests_in_the_connection_limits() {
    let png = large_picture();
    let limits = Limits {
        connections: 2,
        reserved: 0,
        ..Limits::default()
    };
    let h = Harness::with_limits(limits);
    h.share.show_live();
    h.share.show_live_paper(paper(KEY, &png));
    let port = h.port();
    let holder = pc(2);
    let cookie = cookie_from(holder, port);
    let other = cookie_from(pc(3), port);

    // One PC holds every slot with downloads it does not read: a screen that joined still gets in
    // (fair share), so a slow client cannot keep the others out.
    let raw = request_with_cookie(port, "GET", &format!("/paper/{KEY}"), &cookie);
    let mut first = connect_from(holder, port);
    first.write_all(&raw).unwrap();
    let mut second = connect_from(holder, port);
    second.write_all(&raw).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let answer =
        exchange_from(pc(3), port, &request_with_cookie(port, "GET", "/", &other)).unwrap();
    assert_eq!(answer.status, 200);
}

#[test]
fn the_per_address_limit_holds_for_downloads() {
    let png = large_picture();
    let limits = Limits {
        connections_per_address: 2,
        ..Limits::default()
    };
    let (h, cookie) = sharing(limits, &png);
    let _first = start_download(h.port(), &cookie, KEY);
    let _second = start_download(h.port(), &cookie, KEY);
    std::thread::sleep(Duration::from_millis(100));
    let mut third = connect(h.port());
    let (bytes, closed) = read_to_end(&mut third, SOON);
    assert!(closed && bytes.is_empty(), "closed at once");
}

#[test]
fn the_journal_names_the_route_and_nothing_typed() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("share.log");
    let png = picture(1000);
    let share = Share::start(
        ShareConfig {
            journal: Some(path.clone()),
            ..test_config()
        },
        assets(),
        Arc::new(FakeFeed::default()) as Arc<dyn LiveFeed>,
        Box::new(|_| {}),
    )
    .unwrap();
    let port = share.port();
    share.show_live();
    share.show_live_paper(paper(KEY, &png));
    let cookie = session_cookie(&exchange(port, &join_request(CODE)).unwrap());
    let get = |target: &str| {
        exchange(port, &request_with_cookie(port, "GET", target, &cookie))
            .unwrap()
            .status
    };
    assert_eq!(get(&format!("/paper/{KEY}")), 200);
    assert_eq!(get("/paper/secret-folder"), 404);
    assert_eq!(get("/paper/..%2fsecret-folder?name=x"), 404);
    share.stop();
    let text = std::fs::read_to_string(&path).unwrap();
    for wanted in [
        format!("GET /paper/{KEY} after"),
        "(1000 bytes)".to_owned(),
        "GET (other) after".to_owned(),
        "answered 404 in".to_owned(),
    ] {
        assert!(text.contains(&wanted), "{wanted:?} missing from:\n{text}");
    }
    let token = cookie
        .split_once('=')
        .map_or(cookie.as_str(), |(_, token)| token);
    for secret in [token, "secret-folder", "name=x"] {
        assert!(!text.contains(secret), "{secret:?} in:\n{text}");
    }
}
