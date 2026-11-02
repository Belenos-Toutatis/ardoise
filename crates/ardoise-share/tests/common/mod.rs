//! Helpers of the share's integration tests: a fake live feed, a share on 127.0.0.1 with a free
//! port, and a small HTTP and Server-Sent Events client over std's `TcpStream`.

#![allow(dead_code)]

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ardoise_share::{
    Asset, Assets, LiveFeed, LiveViewer, OnChange, Share, ShareConfig, ShareStatus,
};
use serde_json::Value;

pub const CODE: &str = "K7MQ2X";
pub const PAGE: &str =
    "<!doctype html><title>Ardoise</title><script src=\"/assets/viewer-abc123.js\"></script>";
pub const SCRIPT: &str = "console.log('viewer');";
pub const WASM: &[u8] = b"\0asm\x01\0\0\0";
pub const SCRIPT_PATH: &str = "/assets/viewer-abc123.js";
pub const WASM_PATH: &str = "/assets/ardoise_ink-def456.wasm";
/// Generous bound for anything that should happen at once.
pub const SOON: Duration = Duration::from_secs(5);

/// The viewer's files, plus keys that must never be served (outside `/` and `/assets/`).
pub fn assets() -> Assets {
    let asset = |content_type: &str, bytes: &[u8]| Asset {
        content_type: content_type.to_owned(),
        bytes: Arc::from(bytes),
    };
    let mut assets = Assets::new();
    assets.insert(
        "/".into(),
        asset("text/html; charset=utf-8", PAGE.as_bytes()),
    );
    assets.insert(
        SCRIPT_PATH.into(),
        asset("text/javascript", SCRIPT.as_bytes()),
    );
    assets.insert(WASM_PATH.into(), asset("application/wasm", WASM));
    assets.insert(
        "/.vite/manifest.json".into(),
        asset("application/json", b"{\"secret\":true}"),
    );
    assets.insert("index.html".into(), asset("text/html", b"secret"));
    assets
}

/// A live feed that records what the share asks, and sends a snapshot batch on `start` (like the
/// hub: the first batch of a subscription starts with a snapshot).
#[derive(Default)]
pub struct FakeFeed {
    starts: Mutex<Vec<LiveViewer>>,
    stops: Mutex<Vec<u64>>,
    pub frozen: Mutex<Option<String>>,
    snapshots: AtomicU64,
}

impl FakeFeed {
    /// Ids of the viewers started, in order (one per subscription).
    pub fn started(&self) -> Vec<u64> {
        self.starts
            .lock()
            .unwrap()
            .iter()
            .map(LiveViewer::id)
            .collect()
    }

    pub fn stopped(&self) -> Vec<u64> {
        self.stops.lock().unwrap().clone()
    }

    /// The handle of the last subscription started.
    pub fn last(&self) -> LiveViewer {
        self.starts
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a viewer was started")
    }

    /// The handle of the last subscription of the viewer `id`.
    pub fn of(&self, id: u64) -> LiveViewer {
        self.starts
            .lock()
            .unwrap()
            .iter()
            .rev()
            .find(|viewer| viewer.id() == id)
            .cloned()
            .expect("this viewer was started")
    }
}

impl LiveFeed for FakeFeed {
    fn start(&self, viewer: LiveViewer) {
        let n = self.snapshots.fetch_add(1, Ordering::SeqCst) + 1;
        let _ = viewer.send(&snapshot_batch(n));
        self.starts.lock().unwrap().push(viewer);
    }

    fn stop(&self, viewer: u64) {
        self.stops.lock().unwrap().push(viewer);
    }

    fn freeze(&self) -> Option<String> {
        self.frozen.lock().unwrap().clone()
    }
}

pub fn snapshot_batch(n: u64) -> String {
    format!(r#"{{"events":[{{"type":"snapshot","n":{n}}}],"sentAt":0}}"#)
}

pub fn point_batch(i: u64) -> String {
    format!(r#"{{"events":[{{"type":"point","i":{i}}}],"sentAt":1}}"#)
}

/// A share started for a test, its fake feed and the statuses given to `on_change`.
pub struct Harness {
    pub share: Share,
    pub feed: Arc<FakeFeed>,
    pub changes: Arc<Mutex<Vec<ShareStatus>>>,
}

impl Harness {
    pub fn start() -> Self {
        let feed = Arc::new(FakeFeed::default());
        let changes = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&changes);
        let on_change: OnChange = Box::new(move |status| recorded.lock().unwrap().push(status));
        let share = Share::start(
            test_config(),
            assets(),
            Arc::clone(&feed) as Arc<dyn LiveFeed>,
            on_change,
        )
        .expect("the share starts");
        Self {
            share,
            feed,
            changes,
        }
    }

    pub fn port(&self) -> u16 {
        self.share.port()
    }

    pub fn get(&self, target: &str) -> Response {
        request(self.port(), "GET", target)
    }

    /// Joins with the code: the cookie to send (`ardoise_share=…`).
    pub fn cookie(&self) -> String {
        let answer = exchange(self.port(), &join_request(CODE)).expect("join answered");
        assert_eq!(answer.status, 204, "{answer:?}");
        session_cookie(&answer)
    }

    /// A viewer that joined and opened the event stream (the `retry` line already read).
    pub fn viewer(&self) -> EventStream {
        let cookie = self.cookie();
        let mut stream = open_events(self.port(), Some(&cookie)).expect("the stream opens");
        assert_eq!(stream.next_event(SOON), Some(SseEvent::Retry(2000)));
        stream
    }

    /// The last status given to `on_change`.
    pub fn last_change(&self) -> Option<ShareStatus> {
        self.changes.lock().unwrap().last().cloned()
    }
}

pub fn test_config() -> ShareConfig {
    ShareConfig {
        bind: Ipv4Addr::LOCALHOST,
        port: 0,
        code: Some(CODE.into()),
    }
}

/// Waits until `condition` holds (at most `timeout`).
pub fn wait_until(timeout: Duration, mut condition: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if condition() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

pub fn connect(port: u16) -> TcpStream {
    let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connects");
    stream.set_read_timeout(Some(SOON)).unwrap();
    stream.set_write_timeout(Some(SOON)).unwrap();
    stream
}

/// An HTTP answer.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    /// The value of the header `name` (any case); panics if it is given twice.
    pub fn header(&self, name: &str) -> Option<&str> {
        let mut values = self
            .headers
            .iter()
            .filter(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str());
        let first = values.next();
        assert!(values.next().is_none(), "header {name} given twice");
        first
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).expect("a JSON body")
    }
}

/// Parses a response head (`None` if incomplete or not HTTP).
fn parse_head(bytes: &[u8]) -> Option<(Response, usize)> {
    let end = bytes.windows(4).position(|w| w == b"\r\n\r\n")?;
    let text = std::str::from_utf8(&bytes[..end]).ok()?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next()?;
    let mut parts = status_line.splitn(3, ' ');
    assert_eq!(parts.next()?, "HTTP/1.1");
    let status = parts.next()?.parse().ok()?;
    let headers = lines
        .map(|line| {
            let (name, value) = line.split_once(':').expect("a header line");
            (name.to_owned(), value.trim().to_owned())
        })
        .collect();
    Some((
        Response {
            status,
            headers,
            body: Vec::new(),
        },
        end + 4,
    ))
}

/// Sends `raw` and reads the answer until the server closes the connection.
pub fn exchange(port: u16, raw: &[u8]) -> std::io::Result<Response> {
    let mut stream = connect(port);
    stream.write_all(raw)?;
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    let (mut response, at) = parse_head(&bytes)
        .ok_or_else(|| std::io::Error::new(ErrorKind::InvalidData, "no HTTP answer"))?;
    response.body = bytes[at..].to_vec();
    if let Some(length) = response.header("content-length") {
        assert_eq!(length.parse::<usize>().unwrap(), response.body.len());
    }
    Ok(response)
}

/// A request without body.
pub fn request(port: u16, method: &str, target: &str) -> Response {
    let raw = format!("{method} {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    exchange(port, raw.as_bytes()).expect("an answer")
}

/// `POST /join` with the code in a JSON body.
pub fn join_request(code: &str) -> Vec<u8> {
    let body = serde_json::json!({ "code": code }).to_string();
    format!(
        "POST /join HTTP/1.1\r\nHost: share\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// The `name=value` part of the session cookie set by a join.
pub fn session_cookie(answer: &Response) -> String {
    let set = answer.header("set-cookie").expect("a cookie");
    set.split(';').next().unwrap().trim().to_owned()
}

/// One event of a stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    Retry(u64),
    Comment(String),
    Data(String),
}

/// An open event stream.
pub struct EventStream {
    pub stream: TcpStream,
    pub head: Response,
    buf: Vec<u8>,
    closed: bool,
}

impl std::fmt::Debug for EventStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventStream")
            .field("head", &self.head)
            .finish()
    }
}

/// Opens `/events`: the stream, or the answer when it is not 200.
pub fn open_events(port: u16, cookie: Option<&str>) -> Result<EventStream, Response> {
    let mut stream = connect(port);
    let cookie = cookie
        .map(|c| format!("Cookie: {c}\r\n"))
        .unwrap_or_default();
    let raw =
        format!("GET /events HTTP/1.1\r\nHost: share\r\nAccept: text/event-stream\r\n{cookie}\r\n");
    stream.write_all(raw.as_bytes()).unwrap();
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let (head, at) = loop {
        if let Some(parsed) = parse_head(&buf) {
            break parsed;
        }
        let n = stream.read(&mut chunk).expect("an answer");
        assert!(n > 0, "closed before the answer");
        buf.extend_from_slice(&chunk[..n]);
    };
    if head.status != 200 {
        let mut rest = buf[at..].to_vec();
        let _ = stream.read_to_end(&mut rest);
        return Err(Response { body: rest, ..head });
    }
    Ok(EventStream {
        stream,
        head,
        buf: buf[at..].to_vec(),
        closed: false,
    })
}

impl EventStream {
    /// Reads more bytes; false on timeout or end of stream.
    fn fill(&mut self, deadline: Instant) -> bool {
        if self.closed {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return false;
        }
        self.stream
            .set_read_timeout(Some((deadline - now).max(Duration::from_millis(1))))
            .unwrap();
        let mut chunk = [0u8; 64 * 1024];
        match self.stream.read(&mut chunk) {
            Ok(0) => {
                self.closed = true;
                false
            }
            Ok(n) => {
                self.buf.extend_from_slice(&chunk[..n]);
                true
            }
            Err(error) if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                false
            }
            Err(_) => {
                self.closed = true;
                false
            }
        }
    }

    /// The next event, or `None` on timeout or end of stream.
    pub fn next_event(&mut self, timeout: Duration) -> Option<SseEvent> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(end) = self.buf.windows(2).position(|w| w == b"\n\n") {
                let block: Vec<u8> = self.buf.drain(..end + 2).collect();
                let text = String::from_utf8(block[..end].to_vec()).expect("UTF-8 events");
                return Some(parse_event(&text));
            }
            if !self.fill(deadline) {
                return None;
            }
        }
    }

    /// The next `data:` message as JSON, skipping comments and `retry`.
    pub fn next_message(&mut self, timeout: Duration) -> Option<Value> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            match self.next_event(left)? {
                SseEvent::Data(data) => {
                    return Some(serde_json::from_str(&data).expect("JSON data"));
                }
                SseEvent::Retry(_) | SseEvent::Comment(_) => {}
            }
        }
    }

    /// The next message (panics if none comes soon).
    pub fn expect(&mut self) -> Value {
        self.next_message(SOON).expect("a message")
    }

    /// Asserts that no message comes within `wait`.
    pub fn expect_silence(&mut self, wait: Duration) {
        if let Some(message) = self.next_message(wait) {
            panic!("unexpected message {message}");
        }
    }

    /// Reads until the server closes the stream (true) or `timeout` (false); the messages read
    /// meanwhile are returned through `seen`.
    pub fn read_until_closed(&mut self, timeout: Duration, seen: &mut Vec<Value>) -> bool {
        let deadline = Instant::now() + timeout;
        loop {
            while let Some(end) = self.buf.windows(2).position(|w| w == b"\n\n") {
                let block: Vec<u8> = self.buf.drain(..end + 2).collect();
                if let SseEvent::Data(data) = parse_event(&String::from_utf8_lossy(&block[..end])) {
                    seen.push(serde_json::from_str(&data).expect("JSON data"));
                }
            }
            if self.closed {
                return true;
            }
            if !self.fill(deadline) && !self.closed {
                return false;
            }
        }
    }
}

fn parse_event(block: &str) -> SseEvent {
    let mut data: Vec<&str> = Vec::new();
    for line in block.split('\n') {
        if let Some(comment) = line.strip_prefix(':') {
            return SseEvent::Comment(comment.trim().to_owned());
        } else if let Some(retry) = line.strip_prefix("retry:") {
            return SseEvent::Retry(retry.trim().parse().expect("a retry delay"));
        } else if let Some(value) = line.strip_prefix("data:") {
            data.push(value.strip_prefix(' ').unwrap_or(value));
        } else {
            panic!("unexpected line {line:?}");
        }
    }
    SseEvent::Data(data.join("\n"))
}

/// The JSON of a message, for comparisons.
pub fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("JSON")
}

/// A `live` message as the viewer gets it.
pub fn live(reset: bool, batch: &str) -> Value {
    serde_json::json!({ "type": "live", "reset": reset, "batch": json(batch) })
}

pub fn waiting() -> Value {
    serde_json::json!({ "type": "waiting" })
}

pub fn pause(paused: bool) -> Value {
    serde_json::json!({ "type": "pause", "paused": paused })
}

pub fn end() -> Value {
    serde_json::json!({ "type": "end" })
}

/// A `note` message as the app builds it.
pub fn note(id: &str, strokes: usize) -> String {
    let strokes: Vec<Value> = (0..strokes)
        .map(|i| serde_json::json!({ "n": i }))
        .collect();
    serde_json::json!({
        "type": "note",
        "page": { "id": id, "page": { "width": 100, "height": 140 }, "background": { "template": "blank" }, "layers": [], "strokes": strokes },
        "background": null,
    })
    .to_string()
}

/// A `noteStrokes` message as the app builds it.
pub fn note_strokes(from: usize, to: usize) -> String {
    let strokes: Vec<Value> = (from..to).map(|i| serde_json::json!({ "n": i })).collect();
    serde_json::json!({ "type": "noteStrokes", "strokes": strokes }).to_string()
}
