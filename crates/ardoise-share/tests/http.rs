//! The share's HTTP surface over real TCP: the page and its assets, 404 for everything else,
//! the limits, the port.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ardoise_share::{Limits, LiveFeed, Share, ShareConfig, ShareError};
use common::*;

const CSP: &str = "default-src 'self'; script-src 'self' 'wasm-unsafe-eval'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

fn assert_common_headers(answer: &Response) {
    assert_eq!(answer.header("connection"), Some("close"), "{answer:?}");
    assert_eq!(answer.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(answer.header("referrer-policy"), Some("no-referrer"));
}

#[test]
fn serves_the_page_and_its_assets() {
    let h = Harness::start();

    for target in ["/", "/?lang=fr"] {
        let page = h.get(target);
        assert_eq!(page.status, 200);
        assert_common_headers(&page);
        assert_eq!(
            page.header("content-type"),
            Some("text/html; charset=utf-8")
        );
        assert_eq!(page.header("cache-control"), Some("no-store"));
        assert_eq!(page.header("content-security-policy"), Some(CSP));
        assert_eq!(page.header("x-frame-options"), Some("DENY"));
        assert_eq!(page.body, PAGE.as_bytes());
    }

    for target in [SCRIPT_PATH.to_owned(), format!("{SCRIPT_PATH}?v=2")] {
        let script = h.get(&target);
        assert_eq!(script.status, 200);
        assert_common_headers(&script);
        assert_eq!(script.header("content-type"), Some("text/javascript"));
        assert_eq!(
            script.header("cache-control"),
            Some("public, max-age=31536000, immutable")
        );
        assert_eq!(script.header("content-security-policy"), None);
        assert_eq!(script.body, SCRIPT.as_bytes());
    }

    let wasm = h.get(WASM_PATH);
    assert_eq!(wasm.status, 200);
    assert_eq!(wasm.header("content-type"), Some("application/wasm"));
    assert_eq!(wasm.body, WASM);

    // HTTP/1.0 clients get the same.
    let old = exchange(h.port(), b"GET / HTTP/1.0\r\n\r\n").unwrap();
    assert_eq!(old.status, 200);
    assert_eq!(old.body, PAGE.as_bytes());
}

#[test]
fn every_other_request_is_not_found() {
    let h = Harness::start();
    let targets = [
        "/assets/../x",
        "/assets/%2e%2e/x",
        "/assets/%2E%2E/viewer-abc123.js",
        "/assets/..%2fviewer-abc123.js",
        "//assets/x",
        "//assets/viewer-abc123.js",
        "/assets\\x",
        "/assets\\viewer-abc123.js",
        "/ASSETS/x",
        "/ASSETS/viewer-abc123.js",
        "/assets/VIEWER-ABC123.JS",
        "/assets/./viewer-abc123.js",
        "/assets/viewer-abc123.js/",
        "/assets/viewer-abc123.js%00",
        "/assets/viewer-abc123.js#x",
        "/assets/",
        "/assets",
        "/index.html",
        "index.html",
        "/viewer.html",
        "/.vite/manifest.json",
        "/_ardoise/device.json",
        "/_ardoise/",
        "/events/../",
        "/events/",
        "/events/x",
        "/join",
        "/%2F",
        "/%2e%2e/",
        "/..",
        "/favicon.ico",
        "http://host/",
        "http://127.0.0.1/assets/viewer-abc123.js",
        "*",
    ];
    for target in targets {
        let answer = h.get(target);
        assert_eq!(answer.status, 404, "GET {target}");
        assert_common_headers(&answer);
        assert_eq!(answer.header("cache-control"), Some("no-store"));
        assert!(!String::from_utf8_lossy(&answer.body).contains("secret"));
    }

    let other_methods = [
        ("POST", "/"),
        ("HEAD", "/"),
        ("PUT", "/"),
        ("OPTIONS", "/"),
        ("OPTIONS", "*"),
        ("get", "/"),
        ("HEAD", SCRIPT_PATH),
        ("POST", SCRIPT_PATH),
        ("DELETE", SCRIPT_PATH),
        ("GET", "/join"),
        ("PUT", "/join"),
        ("post", "/join"),
        ("POST", "/events"),
        ("HEAD", "/events"),
        ("PATCH", "/events"),
        ("CONNECT", "127.0.0.1:80"),
        ("TRACE", "/"),
    ];
    for (method, target) in other_methods {
        let answer = request(h.port(), method, target);
        assert_eq!(answer.status, 404, "{method} {target}");
    }

    // A malformed request.
    let answer = exchange(h.port(), b"GARBAGE\r\n\r\n").unwrap();
    assert_eq!(answer.status, 400);
    let answer = exchange(h.port(), b"GET / HTTP/2.0\r\n\r\n").unwrap();
    assert_eq!(answer.status, 400);
}

#[test]
fn oversize_heads_are_refused() {
    let h = Harness::start();

    // A header of 20 KiB: 431, or the connection closed.
    let mut raw = String::from("GET / HTTP/1.1\r\nX-Big: ");
    raw.push_str(&"a".repeat(20 * 1024));
    raw.push_str("\r\n\r\n");
    match exchange(h.port(), raw.as_bytes()) {
        Ok(answer) => assert_eq!(answer.status, 431),
        Err(error) => eprintln!("closed: {error}"),
    }

    // More than 32 headers.
    let mut raw = String::from("GET / HTTP/1.1\r\n");
    for i in 0..40 {
        raw.push_str(&format!("X-{i}: {i}\r\n"));
    }
    raw.push_str("\r\n");
    let answer = exchange(h.port(), raw.as_bytes()).unwrap();
    assert_eq!(answer.status, 431);

    // A head that never ends: the server answers after 16 KiB and closes; it is not stuck.
    let mut stream = connect(h.port());
    let _ = stream.write_all(b"GET / HTTP/1.1\r\n");
    let filler = vec![b'a'; 1024 * 1024];
    let _ = stream.write_all(&filler);
    let mut bytes = Vec::new();
    let _ = stream.read_to_end(&mut bytes);
    if !bytes.is_empty() {
        assert!(bytes.starts_with(b"HTTP/1.1 431 "));
    }

    // Still serving.
    assert_eq!(h.get("/").status, 200);
}

#[test]
fn a_silent_client_does_not_block_others() {
    let h = Harness::start();
    let _silent = connect(h.port());
    let mut partial = connect(h.port());
    partial.write_all(b"GET / HTTP/1.1\r\nHost:").unwrap();
    let started = Instant::now();
    assert_eq!(h.get("/").status, 200);
    assert!(started.elapsed() < Duration::from_secs(1));
}

/// The server closed `stream` (end of stream or reset) within `within`, not a read timeout.
fn closed_soon(stream: &mut TcpStream, within: Duration) -> bool {
    stream.set_read_timeout(Some(within)).unwrap();
    let mut byte = [0u8; 1];
    match stream.read(&mut byte) {
        Ok(n) => n == 0,
        Err(error) => !matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
    }
}

/// Nothing comes on `stream` within `wait`, and it is not closed.
fn still_open(stream: &mut TcpStream, wait: Duration) -> bool {
    stream.set_read_timeout(Some(wait)).unwrap();
    let mut byte = [0u8; 1];
    matches!(
        stream.read(&mut byte),
        Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    )
}

fn serves(port: u16) -> bool {
    exchange(port, b"GET / HTTP/1.1\r\n\r\n").is_ok_and(|answer| answer.status == 200)
}

#[test]
fn connections_per_address_are_limited() {
    let h = Harness::start();
    let mut idle: Vec<TcpStream> = (0..8).map(|_| connect(h.port())).collect();
    // Let the server accept them all.
    std::thread::sleep(Duration::from_millis(200));
    let mut ninth = connect(h.port());
    let started = Instant::now();
    assert!(closed_soon(&mut ninth, SOON), "the ninth is closed at once");
    assert!(started.elapsed() < Duration::from_secs(2));
    for stream in &mut idle {
        assert!(still_open(stream, Duration::from_millis(20)));
    }
    // One leaves: room for another.
    drop(idle.pop());
    assert!(wait_until(SOON, || serves(h.port())));
}

#[test]
fn the_oldest_connection_without_a_request_makes_room() {
    let h = Harness::with_limits(Limits {
        connections: 4,
        reserved: 0,
        connections_per_address: 64,
        ..Limits::default()
    });
    let mut idle = Vec::new();
    for _ in 0..4 {
        idle.push(connect(h.port()));
        // Accepted in this order.
        std::thread::sleep(Duration::from_millis(30));
    }
    // Every slot is taken by a connection that sent nothing: a newcomer is served, and the
    // oldest of them was closed for it.
    let answer = exchange(h.port(), b"GET / HTTP/1.1\r\n\r\n").unwrap();
    assert_eq!(answer.status, 200);
    assert!(closed_soon(&mut idle[0], SOON));
    for stream in &mut idle[1..] {
        assert!(still_open(stream, Duration::from_millis(20)));
    }
}

#[test]
fn connections_that_sent_their_request_never_make_room() {
    let h = Harness::with_limits(Limits {
        connections: 2,
        reserved: 0,
        connections_per_address: 64,
        streams_per_address: 32,
        ..Limits::default()
    });
    let cookie = h.cookie();
    // Until the join's connection has ended, the second stream finds no room.
    let mut opened = Vec::new();
    assert!(wait_until(SOON, || {
        if let Some(Ok(stream)) = try_open_events(h.port(), Some(&cookie)) {
            opened.push(stream);
        }
        opened.len() == 2
    }));
    let (mut a, mut b) = (opened.remove(0), opened.remove(0));
    // Both slots hold event streams: a newcomer is closed at once.
    let mut extra = connect(h.port());
    assert!(closed_soon(&mut extra, SOON));
    for stream in [&mut a, &mut b] {
        assert_eq!(stream.next_event(SOON), Some(SseEvent::Retry(2000)));
        assert_eq!(stream.expect(), waiting());
    }
    h.share.show_note(ardoise_share::NoteUpdate {
        full: note("n", 1),
        append: None,
    });
    assert_eq!(a.expect(), json(&note("n", 1)));
    assert_eq!(b.expect(), json(&note("n", 1)));
}

/// A request that holds its slot: a `/join` whose body never comes (until the request's
/// deadline).
const HELD: &[u8] =
    b"POST /join HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: 100\r\n\r\n";

/// Hostile PCs (`sources`) that keep `each` requests open, reconnecting at once whenever one is
/// closed, until `stop`. Counts the connections the server closed.
fn hostile_pcs(
    port: u16,
    sources: Vec<Ipv4Addr>,
    each: usize,
    stop: Arc<AtomicBool>,
    closed: Arc<AtomicUsize>,
    ready: std::sync::mpsc::Sender<()>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        // A connection that sent `HELD` (`None` if it could not even connect: tried again at
        // the next turn). Refused (no room): closed at once, seen at the next turn.
        let open = |source: Ipv4Addr| {
            let mut stream = try_connect_from(source, port).ok()?;
            let _ = stream.write_all(HELD);
            // Polled without waiting (Windows rounds very short read timeouts up).
            stream.set_nonblocking(true).ok()?;
            Some(stream)
        };
        // Closed (end of stream or reset), or answered: the server let go of this request.
        let gone = |stream: &mut Option<TcpStream>| {
            let Some(stream) = stream else { return true };
            let mut byte = [0u8; 1];
            match stream.peek(&mut byte) {
                Ok(_) => true,
                Err(error) => !matches!(error.kind(), ErrorKind::WouldBlock),
            }
        };
        let mut held: Vec<(Ipv4Addr, Option<TcpStream>)> = sources
            .iter()
            .flat_map(|source| (0..each).map(move |_| *source))
            .map(|source| (source, open(source)))
            .collect();
        let _ = ready.send(());
        loop {
            let stopping = stop.load(Ordering::SeqCst);
            for (source, stream) in &mut held {
                if gone(stream) {
                    closed.fetch_add(1, Ordering::SeqCst);
                    if !stopping {
                        *stream = open(*source);
                    }
                }
            }
            if stopping {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    })
}

/// True once the server refuses a connection from `source` at once, which it does only when
/// `source` holds as many connections as it may: the server accepts connections in the order
/// they come, so every connection opened from there before was let in.
fn holds_all_it_may(source: Ipv4Addr, port: u16) -> bool {
    closed_soon(&mut connect_from(source, port), Duration::from_millis(200))
}

#[test]
fn a_few_pcs_cannot_keep_the_projector_out() {
    // 12 slots, 4 of them for screens that joined, 4 per address.
    let h = Harness::with_limits(Limits {
        connections: 12,
        reserved: 4,
        connections_per_address: 4,
        ..Limits::default()
    });
    let port = h.port();
    // Two hostile PCs fill the 8 other slots with requests they never finish, and take any
    // slot they lose back at once (as one laptop with several addresses can).
    let stop = Arc::new(AtomicBool::new(false));
    let closed = Arc::new(AtomicUsize::new(0));
    let (ready, started) = std::sync::mpsc::channel();
    let hostile = hostile_pcs(
        port,
        vec![pc(2), pc(3)],
        4,
        Arc::clone(&stop),
        Arc::clone(&closed),
        ready,
    );
    started.recv_timeout(SOON).unwrap();
    assert!(holds_all_it_may(pc(2), port) && holds_all_it_may(pc(3), port));
    assert_eq!(closed.load(Ordering::SeqCst), 0, "all 8 were let in");

    // The projector loads the page several times, joins, and opens its stream: each time a
    // request of the address that holds the most slots makes room (the hostile PCs take the
    // slot back between two requests).
    let projector = pc(1);
    let get = |target: &str| {
        std::thread::sleep(Duration::from_millis(20));
        let raw = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
        exchange_from(projector, port, raw.as_bytes()).expect("an answer")
    };
    for _ in 0..3 {
        assert_eq!(get("/").status, 200);
        assert_eq!(get(SCRIPT_PATH).body, SCRIPT.as_bytes());
        assert_eq!(get(WASM_PATH).body, WASM);
    }
    let cookie = cookie_from(projector, port);
    let mut stream = open_events_from(projector, port, Some(&cookie)).expect("200");
    assert_eq!(stream.next_event(SOON), Some(SseEvent::Retry(2000)));
    assert_eq!(stream.expect(), waiting());
    // It joined: its address now takes the reserved slots, whatever the others do.
    for _ in 0..3 {
        assert_eq!(get("/").status, 200);
    }
    h.share.show_note(ardoise_share::NoteUpdate {
        full: note("n", 2),
        append: None,
    });
    assert_eq!(stream.expect(), json(&note("n", 2)));

    stop.store(true, Ordering::SeqCst);
    hostile.join().unwrap();
    let lost = closed.load(Ordering::SeqCst);
    eprintln!("the hostile PCs lost {lost} connections");
    assert!(lost >= 3, "the hostile PCs made room: {lost}");
}

#[test]
fn pcs_without_the_code_never_take_the_reserved_slots() {
    let h = Harness::with_limits(Limits {
        connections: 6,
        reserved: 2,
        connections_per_address: 8,
        ..Limits::default()
    });
    let port = h.port();
    // The projector joins first.
    let projector = pc(1);
    let cookie = cookie_from(projector, port);
    // Then 4 hostile addresses each hold one of the 4 shared slots: none holds two more than
    // a newcomer, so fair share closes none of them for another PC without the code.
    let stop = Arc::new(AtomicBool::new(false));
    let closed = Arc::new(AtomicUsize::new(0));
    let (ready, started) = std::sync::mpsc::channel();
    let hostile = hostile_pcs(
        port,
        vec![pc(2), pc(3), pc(4), pc(5)],
        1,
        Arc::clone(&stop),
        Arc::clone(&closed),
        ready,
    );
    started.recv_timeout(SOON).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    let mut stranger = connect_from(pc(6), port);
    assert!(closed_soon(&mut stranger, SOON), "no room without the code");
    // The projector, which holds a token, still gets in: its stream and the page.
    let mut stream = open_events_from(projector, port, Some(&cookie)).expect("200");
    assert_eq!(stream.next_event(SOON), Some(SseEvent::Retry(2000)));
    assert_eq!(stream.expect(), waiting());
    let raw = format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    let page = exchange_from(projector, port, raw.as_bytes()).unwrap();
    assert_eq!(page.status, 200);

    stop.store(true, Ordering::SeqCst);
    hostile.join().unwrap();
    assert_eq!(
        closed.load(Ordering::SeqCst),
        0,
        "nothing was closed for it"
    );
}

#[test]
fn a_page_of_another_site_gets_nothing() {
    let h = Harness::start();
    let port = h.port();
    let get = |target: &str, host: &str| {
        let raw = format!("GET {target} HTTP/1.1\r\nHost: {host}\r\n\r\n");
        exchange(port, raw.as_bytes()).unwrap()
    };
    // The names a DNS rebinding page would send, or another port.
    for host in [
        "evil.example".to_owned(),
        format!("evil.example:{port}"),
        format!("127.0.0.1.nip.example:{port}"),
        format!("localhost.evil.example:{port}"),
        format!("127.0.0.1:{}", port.wrapping_add(1)),
        format!("[::1]:{port}"),
    ] {
        for target in ["/", SCRIPT_PATH, "/events", "/nothing"] {
            let answer = get(target, &host);
            assert_eq!(answer.status, 421, "{host} {target}");
            assert_common_headers(&answer);
            assert!(!String::from_utf8_lossy(&answer.body).contains("Ardoise"));
        }
    }
    // Even with a valid cookie, the stream is not opened.
    let cookie = h.cookie();
    let raw =
        format!("GET /events HTTP/1.1\r\nHost: evil.example:{port}\r\nCookie: {cookie}\r\n\r\n");
    assert_eq!(exchange(port, raw.as_bytes()).unwrap().status, 421);
    assert_eq!(h.share.status().viewers, 0);
    // Two Host headers.
    let raw = format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nHost: evil.example\r\n\r\n");
    assert_eq!(exchange(port, raw.as_bytes()).unwrap().status, 421);
    // The share's own names: an IPv4 address with or without the port, `localhost`, and on
    // Windows this computer's name.
    let mut ours = vec![
        format!("127.0.0.1:{port}"),
        "127.0.0.1".to_owned(),
        format!("192.168.1.23:{port}"),
        format!("localhost:{port}"),
        "LOCALHOST".to_owned(),
    ];
    if cfg!(windows)
        && let Ok(name) = std::env::var("COMPUTERNAME")
        && !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        ours.push(format!("{}:{port}", name.to_lowercase()));
    }
    for host in &ours {
        let answer = get("/", host);
        assert_eq!(answer.status, 200, "{host}");
        assert_eq!(answer.body, PAGE.as_bytes());
    }
}

#[test]
fn another_program_cannot_take_the_port() {
    use socket2::{Domain, Protocol, Socket, Type};
    let h = Harness::start();
    let port = h.port();
    let reusing = || {
        let socket = Socket::new(Domain::IPV4, Type::STREAM, Some(Protocol::TCP)).unwrap();
        socket.set_reuse_address(true).unwrap();
        socket
    };
    // A program that asks to share the port (`SO_REUSEADDR`) on the share's address: refused.
    // (Windows 11 already refuses this one without `SO_EXCLUSIVEADDRUSE`; the option, checked
    // by a unit test, also covers a single address taken next to the app's wildcard, which a
    // test cannot open without the firewall's prompt.)
    let thief = reusing();
    let bound = thief.bind(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)).into());
    eprintln!("the share's address with SO_REUSEADDR: {bound:?}");
    assert!(
        bound.is_err(),
        "127.0.0.1:{port} was bound by another socket"
    );
    // On all addresses: Linux refuses it too; Windows lets a socket bind the wildcard next to
    // one bound to a single address (the share in this test; in the app the share binds the
    // wildcard itself, which leaves no address to take), and connections to the share's
    // address still reach the share.
    let wildcard = reusing();
    let bound = wildcard.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)).into());
    if bound.is_ok() {
        wildcard.listen(8).unwrap();
    }
    #[cfg(not(windows))]
    assert!(bound.is_err(), "0.0.0.0:{port} was bound by another socket");
    for _ in 0..5 {
        assert_eq!(h.get("/").status, 200);
    }
}

#[test]
fn a_share_started_again_at_once_gets_its_port_back() {
    let h = Harness::start();
    let port = h.port();
    // Answers close their connections first: they leave this side in TIME_WAIT.
    for _ in 0..5 {
        assert_eq!(h.get("/").status, 200);
    }
    let cookie = h.cookie();
    let mut viewer = open_events(port, Some(&cookie)).expect("200");
    assert_eq!(viewer.next_event(SOON), Some(SseEvent::Retry(2000)));
    drop(h);
    let mut seen = Vec::new();
    assert!(viewer.read_until_closed(SOON, &mut seen));
    // Stop, start again on the same port at once: as when the teacher stops sharing and
    // shares again.
    let again = Share::start(
        ShareConfig {
            port,
            ..test_config()
        },
        assets(),
        Arc::new(FakeFeed::default()) as Arc<dyn LiveFeed>,
        Box::new(|_| {}),
    );
    let again = match again {
        Ok(share) => share,
        Err(error) => panic!("port {port} not bound again: {error:?}"),
    };
    assert_eq!(again.port(), port);
    assert_eq!(request(port, "GET", "/").status, 200);
}

#[test]
fn a_browser_trying_https_is_turned_away_at_once() {
    let h = Harness::start();
    // The start of a TLS handshake (a browser that turned `http://` into `https://`): closed
    // at once, without an answer, so that the browser falls back to `http://` without waiting.
    let mut tls = connect(h.port());
    tls.write_all(&[
        0x16, 0x03, 0x01, 0x02, 0x00, 0x01, 0x00, 0x01, 0xfc, 0x03, 0x03,
    ])
    .unwrap();
    let started = Instant::now();
    let mut bytes = Vec::new();
    let _ = tls.read_to_end(&mut bytes);
    let took = started.elapsed();
    assert!(bytes.is_empty());
    assert!(took < Duration::from_secs(1), "{took:?}");
    // The page is still served.
    assert_eq!(request(h.port(), "GET", "/").status, 200);
}

#[test]
fn a_connection_opened_ahead_of_its_request_is_kept_longer_than_browsers_keep_it() {
    // Chrome keeps a connection it opened ahead (preconnect) for 10 s.
    let limits = Limits::default();
    assert!(
        limits.first_byte > Duration::from_secs(10),
        "{:?}",
        limits.first_byte
    );
    assert!(limits.request > limits.first_byte, "{:?}", limits.request);
}

#[test]
fn a_request_must_start_soon_and_end_in_time() {
    let h = Harness::with_limits(Limits {
        first_byte: Duration::from_millis(200),
        request: Duration::from_millis(800),
        ..Limits::default()
    });
    // Nothing sent: closed once the first byte is late, without an answer.
    let mut silent = connect(h.port());
    let started = Instant::now();
    let mut bytes = Vec::new();
    let _ = silent.read_to_end(&mut bytes);
    let took = started.elapsed();
    assert!(bytes.is_empty());
    assert!(
        took >= Duration::from_millis(150) && took < Duration::from_secs(2),
        "{took:?}"
    );

    // A head begun in time and never finished: closed when the request is late.
    let mut slow = connect(h.port());
    slow.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\n")
        .unwrap();
    let started = Instant::now();
    let mut bytes = Vec::new();
    let _ = slow.read_to_end(&mut bytes);
    let took = started.elapsed();
    assert!(bytes.is_empty());
    assert!(
        took >= Duration::from_millis(500) && took < Duration::from_secs(3),
        "{took:?}"
    );

    // A head that keeps coming in time is served.
    let mut steady = connect(h.port());
    steady.set_nodelay(true).unwrap();
    for byte in b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n" {
        steady.write_all(&[*byte]).unwrap();
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut bytes = Vec::new();
    steady.read_to_end(&mut bytes).unwrap();
    assert!(bytes.starts_with(b"HTTP/1.1 200 OK\r\n"));
}

#[test]
fn a_head_sent_one_byte_at_a_time() {
    let h = Harness::start();
    for head in [
        "GET /?x HTTP/1.1\r\nHost: localhost\r\nAccept: */*\r\n\r\n".to_owned(),
        // Bare line feeds.
        "GET / HTTP/1.1\nHost: localhost\n\n".to_owned(),
        // A long head (12 KiB), still under the limit.
        format!(
            "GET / HTTP/1.1\r\nHost: localhost\r\nX-Long: {}\r\n\r\n",
            "a".repeat(12 * 1024)
        ),
    ] {
        let mut stream = connect(h.port());
        stream.set_nodelay(true).unwrap();
        let started = Instant::now();
        for byte in head.as_bytes() {
            stream.write_all(&[*byte]).unwrap();
        }
        let mut bytes = Vec::new();
        stream.read_to_end(&mut bytes).unwrap();
        assert!(
            bytes.starts_with(b"HTTP/1.1 200 OK\r\n"),
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        assert!(bytes.ends_with(PAGE.as_bytes()));
        eprintln!(
            "{} bytes one at a time: {:?}",
            head.len(),
            started.elapsed()
        );
    }
}

#[test]
fn a_port_in_use_is_an_error_at_once() {
    let h = Harness::start();
    let config = ShareConfig {
        port: h.port(),
        ..test_config()
    };
    let error = Share::start(
        config,
        assets(),
        Arc::new(FakeFeed::default()) as Arc<dyn LiveFeed>,
        Box::new(|_| {}),
    )
    .err()
    .expect("refused");
    assert!(
        matches!(error, ShareError::PortInUse { port } if port == h.port()),
        "{error:?}"
    );
    assert_eq!(error.code(), "sharePortInUse");
    // The first share still serves.
    assert_eq!(h.get("/").status, 200);
}

/// Plain listeners (as another program has them) on `count` consecutive ports of the loopback
/// interface, and the first port. The ports come from the system's, then the next ones are tried:
/// the search starts again when one of them is taken.
fn hold_consecutive_ports(count: u16) -> (Vec<TcpListener>, u16) {
    for _ in 0..200 {
        let first = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = first.local_addr().unwrap().port();
        if u32::from(port) + u32::from(count) > 65_535 {
            continue;
        }
        let mut held = vec![first];
        for next in 1..count {
            match TcpListener::bind((Ipv4Addr::LOCALHOST, port + next)) {
                Ok(listener) => held.push(listener),
                Err(_) => break,
            }
        }
        // The port after them must be free too, for the share to move there: not one that
        // Windows reserves (Hyper-V ranges answer "permission denied" to everyone).
        let room = TcpListener::bind((Ipv4Addr::LOCALHOST, port.saturating_add(count))).is_ok();
        if held.len() == usize::from(count) && room {
            return (held, port);
        }
    }
    panic!("no {count} consecutive free ports");
}

fn start_on(port: u16, fallback_ports: u16) -> Result<Share, ShareError> {
    Share::start(
        ShareConfig {
            port,
            fallback_ports,
            ..test_config()
        },
        assets(),
        Arc::new(FakeFeed::default()) as Arc<dyn LiveFeed>,
        Box::new(|_| {}),
    )
}

#[test]
fn a_port_in_use_moves_to_the_next_free_one() {
    let (held, port) = hold_consecutive_ports(1);
    let share = start_on(port, 20).expect("a following port is taken");
    assert_eq!(share.requested_port(), port);
    assert!(
        share.port() > port && share.port() <= port + 20,
        "{} for {port}",
        share.port()
    );
    // The share really listens there, and the other program still has its port.
    assert_eq!(request(share.port(), "GET", "/").status, 200);
    assert_eq!(share.status().port, share.port());
    drop(held);
}

#[test]
fn several_ports_in_use_are_skipped_in_order() {
    let (held, port) = hold_consecutive_ports(4);
    let share = start_on(port, 20).expect("a port after the four held ones");
    assert!(share.port() >= port + 4, "{} for {port}", share.port());
    assert_eq!(request(share.port(), "GET", "/").status, 200);
    // A second share takes the next port still: ports once bound are in use too.
    let second = start_on(port, 20).expect("the port after the first share's");
    assert!(second.port() > share.port(), "{}", second.port());
    drop(held);
}

#[test]
fn a_free_port_is_taken_as_asked() {
    // A port found free can be taken by another test's client before the share binds it (a
    // client that closed first keeps its port for a while): a few tries.
    let (share, free) = (0..10)
        .find_map(|_| {
            let probe = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).ok()?;
            let free = probe.local_addr().ok()?.port();
            drop(probe);
            let share = start_on(free, 20).ok()?;
            (share.port() == free).then_some((share, free))
        })
        .expect("a free port taken as asked");
    assert_eq!((share.port(), share.requested_port()), (free, free));
    // Port 0 asks for any port: it is bound once, whatever the fallback.
    let any = start_on(0, 20).expect("any port");
    assert_ne!(any.port(), 0);
    assert_eq!(any.requested_port(), 0);
}

#[test]
fn without_a_free_port_in_the_range_the_error_names_the_range() {
    let (held, port) = hold_consecutive_ports(3);
    let error = start_on(port, 2)
        .err()
        .expect("every port of the range is held");
    assert!(
        matches!(error, ShareError::NoFreePort { first, last } if first == port && last == port + 2),
        "{error:?}"
    );
    assert_eq!(error.code(), "shareNoFreePort");
    let text = error.to_string();
    assert!(
        text.contains(&port.to_string()) && text.contains(&(port + 2).to_string()),
        "{text}"
    );
    // Only the port wanted: the single-port error, as before.
    let error = start_on(port, 0).err().expect("held");
    assert!(
        matches!(error, ShareError::PortInUse { port: p } if p == port),
        "{error:?}"
    );
    drop(held);
}

#[test]
fn the_search_stops_at_port_65535() {
    // Whatever the port, the range never goes past 65535 (no wrap-around to 0, which would
    // take any port).
    let share = start_on(65_535, 20);
    match share {
        Ok(share) => assert_eq!(share.port(), 65_535),
        Err(error) => assert!(
            matches!(error, ShareError::PortInUse { port: 65_535 }),
            "{error:?}"
        ),
    }
}

#[test]
fn a_random_code_without_configuration() {
    let config = ShareConfig {
        code: None,
        ..test_config()
    };
    let share = Share::start(
        config,
        assets(),
        Arc::new(FakeFeed::default()) as Arc<dyn LiveFeed>,
        Box::new(|_| {}),
    )
    .unwrap();
    let code = share.code();
    assert_eq!(code.len(), 6);
    assert!(
        code.bytes()
            .all(|b| b"ABCDEFGHJKMNPQRSTUVWXYZ23456789".contains(&b))
    );
    assert_eq!(share.status().code, code);
    // A configured code is normalised like a typed one.
    let config = ShareConfig {
        code: Some("ab2-cd3".into()),
        ..test_config()
    };
    let share = Share::start(
        config,
        assets(),
        Arc::new(FakeFeed::default()) as Arc<dyn LiveFeed>,
        Box::new(|_| {}),
    )
    .unwrap();
    assert_eq!(share.code(), "AB2CD3");
}

#[test]
fn dropping_the_share_closes_the_port() {
    let h = Harness::start();
    let port = h.port();
    assert_eq!(h.get("/").status, 200);
    let started = Instant::now();
    drop(h);
    assert!(started.elapsed() < Duration::from_secs(2));
    let refused = TcpStream::connect((Ipv4Addr::LOCALHOST, port));
    assert!(refused.is_err(), "the port is closed");
}

#[test]
fn the_journal_tells_each_connection_without_the_code_the_cookie_or_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("share.log");
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
    assert_eq!(request(port, "GET", "/").status, 200);
    let joined = exchange(port, &join_request(CODE)).unwrap();
    assert_eq!(joined.status, 200);
    let cookie = session_cookie(&joined);
    assert_eq!(request(port, "GET", "/secret-folder?name=x").status, 404);
    share.stop();
    let text = std::fs::read_to_string(&path).unwrap();
    for wanted in [
        "share started on port",
        "connected",
        "GET / after",
        "POST /join after",
        "answered 200 in",
        "GET (other) after",
        "answered 404 in",
    ] {
        assert!(
            text.contains(wanted),
            "{wanted:?} missing from:
{text}"
        );
    }
    let token = cookie
        .split_once('=')
        .map_or(cookie.as_str(), |(_, token)| token);
    for secret in [CODE, token, "secret-folder", "name=x"] {
        assert!(
            !text.contains(secret),
            "{secret:?} in:
{text}"
        );
    }
}

#[test]
fn the_browser_closes_first_so_this_pc_keeps_no_address_pair_reserved() {
    let h = Harness::start();
    let mut stream = connect(h.port());
    stream
        .write_all(format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{}\r\n\r\n", h.port()).as_bytes())
        .unwrap();
    // The whole answer arrives…
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 8192];
    stream
        .set_read_timeout(Some(Duration::from_millis(300)))
        .unwrap();
    let complete = |bytes: &[u8]| {
        let text = String::from_utf8_lossy(bytes);
        let Some(end) = text.find("\r\n\r\n") else {
            return false;
        };
        let length: usize = text
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .and_then(|value| value.trim().parse().ok())
            .unwrap_or(0);
        bytes.len() >= end + 4 + length
    };
    while !complete(&bytes) {
        let n = stream.read(&mut chunk).unwrap();
        assert!(n > 0, "closed before the end of the answer");
        bytes.extend_from_slice(&chunk[..n]);
    }
    // …but the server does not close first: it waits for the browser, which closes as soon as
    // it has read a `Connection: close` answer (the side that closes first keeps the address
    // pair reserved for 2 minutes).
    let error = stream.read(&mut chunk).expect_err("still open");
    assert!(
        matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut),
        "{error:?}"
    );
    // A client that never closes is closed by the server after a while.
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let started = Instant::now();
    let n = stream.read(&mut chunk).unwrap_or(0);
    assert_eq!(n, 0);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "{:?}",
        started.elapsed()
    );
}
