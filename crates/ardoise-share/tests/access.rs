//! Joining with the code, the session cookie, the attempts limit and the stream limit, over
//! real TCP.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Write;
use std::time::Duration;

use ardoise_share::Limits;

use common::*;

fn is_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[test]
fn wrong_then_right_code() {
    let h = Harness::start();

    let wrong = exchange(h.port(), &join_request("AAAAAA")).unwrap();
    assert_eq!(wrong.status, 403);
    assert_eq!(wrong.header("content-type"), Some("application/json"));
    assert_eq!(wrong.header("set-cookie"), None);
    assert_eq!(
        wrong.json(),
        serde_json::json!({ "error": "wrongCode", "remaining": 4 })
    );

    // Any case, with spaces or a dash.
    let mut tokens = Vec::new();
    for typed in ["K7MQ2X", "k7m q2x", "k7m-Q2X", " K7M Q2X "] {
        let right = exchange(h.port(), &join_request(typed)).unwrap();
        assert_eq!(right.status, 200, "{typed}");
        assert_eq!(right.json(), serde_json::json!({ "joined": true }));
        assert_eq!(right.header("connection"), Some("close"));
        assert_eq!(right.header("cache-control"), Some("no-store"));
        let set_cookie = right.header("set-cookie").unwrap();
        let token = set_cookie
            .strip_prefix("ardoise_share=")
            .and_then(|rest| rest.strip_suffix("; Path=/; HttpOnly; SameSite=Strict"))
            .unwrap_or_else(|| panic!("{set_cookie}"));
        assert!(is_token(token), "{token}");
        tokens.push(token.to_owned());
    }
    tokens.sort();
    tokens.dedup();
    assert_eq!(tokens.len(), 4, "a new token for each join");

    // Every token given opens the stream.
    for token in &tokens {
        let cookie = format!("ardoise_share={token}");
        let mut stream = open_events(h.port(), Some(&cookie)).expect("200");
        assert_eq!(stream.next_event(SOON), Some(SseEvent::Retry(2000)));
        assert_eq!(stream.expect(), waiting());
    }
}

#[test]
fn the_stream_needs_the_cookie() {
    let h = Harness::start();
    let refused = [
        None,
        Some("ardoise_share="),
        Some("ardoise_share=0000000000000000000000000000000000000000000000000000000000000000"),
        Some("other=1"),
        Some("ARDOISE_SHARE=x"),
    ];
    for cookie in refused {
        let answer = open_events(h.port(), cookie).expect_err("refused");
        assert_eq!(answer.status, 401, "{cookie:?}");
        assert_eq!(answer.header("connection"), Some("close"));
    }

    let cookie = h.cookie();
    // A wrong token next to the right one is fine.
    let with_others = format!("theme=dark; ardoise_share=00; {cookie}");
    let mut stream = open_events(h.port(), Some(&with_others)).expect("200");
    assert_eq!(stream.head.status, 200);
    assert_eq!(
        stream.head.header("content-type"),
        Some("text/event-stream")
    );
    assert_eq!(stream.head.header("cache-control"), Some("no-store"));
    assert_eq!(stream.head.header("connection"), Some("close"));
    assert_eq!(
        stream.head.header("x-content-type-options"),
        Some("nosniff")
    );
    assert_eq!(stream.head.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(stream.head.header("content-length"), None);
    assert_eq!(stream.next_event(SOON), Some(SseEvent::Retry(2000)));
    assert_eq!(stream.expect(), waiting());

    // The token of another share is refused.
    let other = Harness::start();
    let answer = open_events(other.port(), Some(&cookie)).expect_err("refused");
    assert_eq!(answer.status, 401);
}

#[test]
fn too_many_wrong_codes() {
    let h = Harness::start();
    for remaining in (0..5).rev() {
        let wrong = exchange(h.port(), &join_request("WRONG2")).unwrap();
        assert_eq!(wrong.status, 403);
        assert_eq!(
            wrong.json(),
            serde_json::json!({ "error": "wrongCode", "remaining": remaining })
        );
    }
    // The sixth attempt, even with the right code.
    let refused = exchange(h.port(), &join_request(CODE)).unwrap();
    assert_eq!(refused.status, 429);
    let retry_after: u64 = refused.header("retry-after").unwrap().parse().unwrap();
    assert!((295..=300).contains(&retry_after), "{retry_after}");
    assert_eq!(
        refused.json(),
        serde_json::json!({ "error": "tooManyAttempts", "retryAfter": retry_after })
    );
    assert_eq!(refused.header("set-cookie"), None);
    // The page itself is still served.
    assert_eq!(h.get("/").status, 200);
}

/// A `POST /join` with `headers` (each ending with CRLF) and `body`.
fn join_with(headers: &str, body: &str) -> Vec<u8> {
    format!(
        "POST /join HTTP/1.1\r\n{headers}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

const JSON: &str = "Content-Type: application/json\r\n";

/// Two wrong codes still allowed for the address: the requests before did not count.
fn assert_no_attempt_counted(port: u16) {
    let wrong = exchange(port, &join_request("WRONG2")).unwrap();
    assert_eq!(
        wrong.json(),
        serde_json::json!({ "error": "wrongCode", "remaining": 4 })
    );
    let right = exchange(port, &join_request(CODE)).unwrap();
    assert_eq!(right.status, 200);
}

#[test]
fn bad_join_requests() {
    let h = Harness::start();
    let bad = [
        // No body.
        format!("POST /join HTTP/1.1\r\nHost: localhost\r\n{JSON}\r\n"),
        format!("POST /join HTTP/1.1\r\n{JSON}Content-Length: 0\r\n\r\n"),
        // Not JSON, not the expected shape.
        String::from_utf8(join_with(JSON, "K7MQ2X")).unwrap(),
        String::from_utf8(join_with(JSON, "{\"code\":42}")).unwrap(),
        String::from_utf8(join_with(JSON, "[]")).unwrap(),
        // Chunked bodies are not accepted.
        format!(
            "POST /join HTTP/1.1\r\n{JSON}Transfer-Encoding: chunked\r\n\r\n12\r\n{{\"code\":\"K7MQ2X\"}}\r\n0\r\n\r\n"
        ),
        // Two different lengths.
        format!(
            "POST /join HTTP/1.1\r\n{JSON}Content-Length: 17\r\nContent-Length: 18\r\n\r\n{{\"code\":\"K7MQ2X\"}}"
        ),
    ];
    for raw in &bad {
        let answer = exchange(h.port(), raw.as_bytes()).unwrap();
        assert_eq!(answer.status, 400, "{raw}");
        assert_eq!(answer.json(), serde_json::json!({ "error": "badRequest" }));
    }

    let too_long = format!("POST /join HTTP/1.1\r\n{JSON}Content-Length: 1025\r\n\r\n");
    let answer = exchange(h.port(), too_long.as_bytes()).unwrap();
    assert_eq!(answer.status, 413);
    assert_eq!(answer.json(), serde_json::json!({ "error": "tooLarge" }));

    assert_no_attempt_counted(h.port());
}

#[test]
fn join_needs_a_json_content_type() {
    let h = Harness::start();
    let body = r#"{"code":"K7MQ2X"}"#;
    for headers in [
        "",
        "Content-Type: text/plain\r\n",
        "Content-Type: text/plain;charset=UTF-8\r\n",
        "Content-Type: application/x-www-form-urlencoded\r\n",
        "Content-Type: multipart/form-data; boundary=x\r\n",
        "Content-Type: application/json\r\nContent-Type: text/plain\r\n",
    ] {
        let answer = exchange(h.port(), &join_with(headers, body)).unwrap();
        assert_eq!(answer.status, 415, "{headers:?}");
        assert_eq!(answer.json(), serde_json::json!({ "error": "badRequest" }));
        assert_eq!(answer.header("set-cookie"), None);
    }
    for headers in [
        "Content-Type: application/json; charset=utf-8\r\n",
        "content-type: Application/JSON\r\n",
    ] {
        let answer = exchange(h.port(), &join_with(headers, body)).unwrap();
        assert_eq!(answer.status, 200, "{headers:?}");
    }
    assert_no_attempt_counted(h.port());
}

#[test]
fn join_from_another_site_is_refused() {
    let h = Harness::start();
    let port = h.port();
    let body = r#"{"code":"K7MQ2X"}"#;
    let host = format!("Host: 127.0.0.1:{port}\r\n");
    for origin in [
        "Origin: http://evil.example\r\n".to_owned(),
        "Origin: null\r\n".to_owned(),
        format!("Origin: https://127.0.0.1:{port}\r\n"),
        "Origin: http://127.0.0.1\r\n".to_owned(),
        format!("Origin: http://127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\n"),
    ] {
        let answer = exchange(port, &join_with(&format!("{host}{origin}{JSON}"), body)).unwrap();
        assert_eq!(answer.status, 403, "{origin:?}");
        assert_eq!(answer.json(), serde_json::json!({ "error": "badOrigin" }));
        assert_eq!(answer.header("set-cookie"), None);
    }
    // An origin without a host to compare with.
    let answer = exchange(
        port,
        &join_with(&format!("Origin: http://127.0.0.1:{port}\r\n{JSON}"), body),
    )
    .unwrap();
    assert_eq!(answer.status, 403);
    // The page's own origin, or no origin at all (not a browser).
    let same = format!("{host}Origin: http://127.0.0.1:{port}\r\n{JSON}");
    assert_eq!(exchange(port, &join_with(&same, body)).unwrap().status, 200);
    assert_eq!(
        exchange(port, &join_with(&format!("{host}{JSON}"), body))
            .unwrap()
            .status,
        200
    );
    assert_no_attempt_counted(port);
}

#[test]
fn join_from_a_page_of_another_site_is_misdirected() {
    let h = Harness::start();
    let port = h.port();
    let body = r#"{"code":"K7MQ2X"}"#;
    for host in [
        "Host: evil.example\r\n".to_owned(),
        format!("Host: evil.example:{port}\r\n"),
        // The page's origin matches its (foreign) host: still refused.
        format!("Host: evil.example:{port}\r\nOrigin: http://evil.example:{port}\r\n"),
    ] {
        let answer = exchange(port, &join_with(&format!("{host}{JSON}"), body)).unwrap();
        assert_eq!(answer.status, 421, "{host:?}");
        assert_eq!(answer.header("set-cookie"), None);
    }
    // Wrong codes from such a page are not counted either.
    for _ in 0..10 {
        let raw = join_with(
            &format!("Host: evil.example\r\n{JSON}"),
            r#"{"code":"WRONG2"}"#,
        );
        assert_eq!(exchange(port, &raw).unwrap().status, 421);
    }
    assert_no_attempt_counted(port);
    // The share's own name with the page's origin.
    let same = format!("Host: localhost:{port}\r\nOrigin: http://localhost:{port}\r\n{JSON}");
    assert_eq!(exchange(port, &join_with(&same, body)).unwrap().status, 200);
}

#[test]
fn event_streams_are_opened_only_so_often() {
    let every = Duration::from_millis(1000);
    let h = Harness::with_limits(Limits {
        stream_opens: 3,
        stream_open_every: every,
        ..Limits::default()
    });
    let port = h.port();
    let cookie = h.cookie();
    // Three in a row (each closed before the next).
    for _ in 0..3 {
        let mut stream = open_events(port, Some(&cookie)).expect("200");
        assert_eq!(stream.next_event(SOON), Some(SseEvent::Retry(2000)));
        assert_eq!(stream.expect(), waiting());
    }
    // The fourth waits: 503, which the viewer retries 5 s later ("too many screens").
    let started = std::time::Instant::now();
    let refused = open_events(port, Some(&cookie)).expect_err("refused");
    assert_eq!(refused.status, 503);
    let retry_after: u64 = refused.header("retry-after").unwrap().parse().unwrap();
    assert_eq!(retry_after, 1);
    // Another address is not affected.
    let mut other = open_events_from(pc(2), port, Some(&cookie)).expect("200");
    assert_eq!(other.next_event(SOON), Some(SseEvent::Retry(2000)));
    assert_eq!(other.expect(), waiting());
    drop(other);
    // One more every second.
    let wait = every.saturating_sub(started.elapsed()) + Duration::from_millis(100);
    std::thread::sleep(wait);
    let mut again = open_events(port, Some(&cookie)).expect("200");
    assert_eq!(again.next_event(SOON), Some(SseEvent::Retry(2000)));
    assert_eq!(again.expect(), waiting());
    assert_eq!(
        open_events(port, Some(&cookie))
            .expect_err("refused")
            .status,
        503
    );
}

#[test]
fn a_viewer_reconnecting_quickly_gets_eight_streams_in_a_row() {
    // The viewer's own delays after a cut (`viewer-stream.ts`: 0.5 s, 1, 2, 4, then 5 s; the
    // count starts again after each stream that opens): with the default limits (8 in a row,
    // then one every 2 s), a stream cut at once every time is refused only after its 8th quick
    // reconnection, and the viewer's 5 s retry after a 503 is then always let in.
    let defaults = Limits::default();
    assert_eq!(defaults.stream_opens, 8);
    assert_eq!(defaults.stream_open_every, Duration::from_secs(2));
    let h = Harness::start();
    let cookie = h.cookie();
    for _ in 0..8 {
        let mut stream = open_events(h.port(), Some(&cookie)).expect("200");
        assert_eq!(stream.next_event(SOON), Some(SseEvent::Retry(2000)));
        std::thread::sleep(Duration::from_millis(10));
    }
    let refused = open_events(h.port(), Some(&cookie)).expect_err("refused");
    assert_eq!(refused.status, 503);
    let retry_after: u64 = refused.header("retry-after").unwrap().parse().unwrap();
    assert!(retry_after <= 2, "{retry_after}");
}

#[test]
fn a_body_sent_in_pieces() {
    let h = Harness::start();
    let mut stream = connect(h.port());
    let body = br#"{"code":"k7mq2x"}"#;
    stream
        .write_all(
            format!(
                "POST /join HTTP/1.1\r\nHost: 127.0.0.1\r\n{JSON}Content-Length: {}\r\n\r\n",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    stream.write_all(&body[..5]).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    stream.write_all(&body[5..]).unwrap();
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut stream, &mut bytes).unwrap();
    assert!(
        bytes.starts_with(b"HTTP/1.1 200 OK\r\n"),
        "{}",
        String::from_utf8_lossy(&bytes)
    );

    // The whole request one byte at a time.
    let mut stream = connect(h.port());
    stream.set_nodelay(true).unwrap();
    for byte in join_request(CODE) {
        stream.write_all(&[byte]).unwrap();
    }
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut stream, &mut bytes).unwrap();
    assert!(bytes.starts_with(b"HTTP/1.1 200 OK\r\n"));
}

#[test]
fn an_address_keeps_its_last_eight_tokens() {
    let h = Harness::start();
    let cookies: Vec<String> = (0..9).map(|_| h.cookie()).collect();
    // The first was forgotten for the ninth.
    let refused = open_events(h.port(), Some(&cookies[0])).expect_err("refused");
    assert_eq!(refused.status, 401);
    // The last ones open streams (4 at a time from one address).
    for cookie in &cookies[5..] {
        let mut stream = open_events(h.port(), Some(cookie)).expect("200");
        assert_eq!(stream.next_event(SOON), Some(SseEvent::Retry(2000)));
        assert_eq!(stream.expect(), waiting());
    }
}

#[test]
fn event_streams_per_address_are_limited() {
    let h = Harness::start();
    let cookie = h.cookie();
    let mut streams: Vec<EventStream> = (0..4)
        .map(|_| open_events(h.port(), Some(&cookie)).expect("200"))
        .collect();
    assert_eq!(h.share.status().viewers, 4);
    let refused = open_events(h.port(), Some(&cookie)).expect_err("refused");
    assert_eq!(refused.status, 503);
    assert_eq!(refused.header("retry-after"), Some("5"));
    assert_eq!(h.share.status().viewers, 4);

    // One leaves: another may come.
    drop(streams.pop());
    assert!(wait_until(SOON, || h.share.status().viewers == 3));
    let mut again = open_events(h.port(), Some(&cookie)).expect("200");
    assert_eq!(again.next_event(SOON), Some(SseEvent::Retry(2000)));
    assert_eq!(again.expect(), waiting());
}

#[test]
fn event_streams_are_limited() {
    let h = Harness::with_limits(Limits {
        connections_per_address: 64,
        streams_per_address: 64,
        stream_opens: 64,
        ..Limits::default()
    });
    let cookie = h.cookie();
    let mut streams: Vec<EventStream> = (0..32)
        .map(|_| open_events(h.port(), Some(&cookie)).expect("200"))
        .collect();
    assert_eq!(h.share.status().viewers, 32);
    let refused = open_events(h.port(), Some(&cookie)).expect_err("refused");
    assert_eq!(refused.status, 503);
    assert_eq!(refused.header("retry-after"), Some("5"));

    // One leaves: another may come.
    drop(streams.pop());
    assert!(wait_until(SOON, || h.share.status().viewers == 31));
    let mut again = open_events(h.port(), Some(&cookie)).expect("200");
    assert_eq!(again.next_event(SOON), Some(SseEvent::Retry(2000)));
    assert_eq!(again.expect(), waiting());
}
