//! The share's HTTP surface over real TCP: the page and its assets, 404 for everything else,
//! the limits, the port.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::Arc;
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
    slow.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n").unwrap();
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
    for byte in b"GET / HTTP/1.1\r\nHost: x\r\n\r\n" {
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
        "GET /?x HTTP/1.1\r\nHost: share\r\nAccept: */*\r\n\r\n".to_owned(),
        // Bare line feeds.
        "GET / HTTP/1.1\nHost: share\n\n".to_owned(),
        // A long head (12 KiB), still under the limit.
        format!(
            "GET / HTTP/1.1\r\nHost: share\r\nX-Long: {}\r\n\r\n",
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
