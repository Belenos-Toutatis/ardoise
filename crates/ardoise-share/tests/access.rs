//! Joining with the code, the session cookie, the attempts limit and the stream limit, over
//! real TCP.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::io::Write;
use std::time::Duration;

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
        assert_eq!(right.status, 204, "{typed}");
        assert!(right.body.is_empty());
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

#[test]
fn bad_join_requests() {
    let h = Harness::start();
    let bad = [
        // No body.
        "POST /join HTTP/1.1\r\nHost: x\r\n\r\n".to_owned(),
        "POST /join HTTP/1.1\r\nContent-Length: 0\r\n\r\n".to_owned(),
        // Not JSON, not the expected shape.
        "POST /join HTTP/1.1\r\nContent-Length: 6\r\n\r\nK7MQ2X".to_owned(),
        "POST /join HTTP/1.1\r\nContent-Length: 11\r\n\r\n{\"code\":42}".to_owned(),
        "POST /join HTTP/1.1\r\nContent-Length: 2\r\n\r\n[]".to_owned(),
        // Chunked bodies are not accepted.
        "POST /join HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n12\r\n{\"code\":\"K7MQ2X\"}\r\n0\r\n\r\n"
            .to_owned(),
        // Two different lengths.
        "POST /join HTTP/1.1\r\nContent-Length: 17\r\nContent-Length: 18\r\n\r\n{\"code\":\"K7MQ2X\"}"
            .to_owned(),
    ];
    for raw in &bad {
        let answer = exchange(h.port(), raw.as_bytes()).unwrap();
        assert_eq!(answer.status, 400, "{raw}");
        assert_eq!(answer.json(), serde_json::json!({ "error": "badRequest" }));
    }

    let too_long = "POST /join HTTP/1.1\r\nContent-Length: 1025\r\n\r\n";
    let answer = exchange(h.port(), too_long.as_bytes()).unwrap();
    assert_eq!(answer.status, 413);
    assert_eq!(answer.json(), serde_json::json!({ "error": "tooLarge" }));

    // None of these counted as a wrong code.
    let right = exchange(h.port(), &join_request(CODE)).unwrap();
    assert_eq!(right.status, 204);
}

#[test]
fn a_body_sent_in_pieces() {
    let h = Harness::start();
    let mut stream = connect(h.port());
    let body = br#"{"code":"k7mq2x"}"#;
    stream
        .write_all(
            format!(
                "POST /join HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
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
        bytes.starts_with(b"HTTP/1.1 204 No Content\r\n"),
        "{}",
        String::from_utf8_lossy(&bytes)
    );
}

#[test]
fn event_streams_are_limited() {
    let h = Harness::start();
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
