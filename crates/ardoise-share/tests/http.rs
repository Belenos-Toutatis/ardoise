//! The share's HTTP surface over real TCP: the page and its assets, 404 for everything else,
//! the limits, the port.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ardoise_share::{LiveFeed, Share, ShareConfig, ShareError};
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

#[test]
fn connections_are_limited() {
    let h = Harness::start();
    let idle: Vec<TcpStream> = (0..64).map(|_| connect(h.port())).collect();
    // Let the server accept them all.
    std::thread::sleep(Duration::from_millis(300));
    let mut extra = connect(h.port());
    let started = Instant::now();
    let mut byte = [0u8; 1];
    // Closed at once: end of stream or reset, not a timeout.
    match extra.read(&mut byte) {
        Ok(n) => assert_eq!(n, 0),
        Err(error) => assert!(
            !matches!(
                error.kind(),
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
            ),
            "{error}"
        ),
    }
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(idle);
    // The idle connections end when their clients close: the server serves again.
    assert!(wait_until(SOON, || {
        exchange(h.port(), b"GET / HTTP/1.1\r\n\r\n").is_ok_and(|answer| answer.status == 200)
    }));
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
