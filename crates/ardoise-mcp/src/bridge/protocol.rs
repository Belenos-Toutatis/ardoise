//! The protocol of the bridge between `ardoise.exe --mcp` and the open app: one JSON request per
//! line from the server, one JSON response per line from the app (`docs/design/mcp.md`, "As
//! built: bridge and guide"). Platform-free: the named pipe only carries these lines.
//!
//! ```text
//! → {"v":1,"id":7,"request":{"kind":"presentation","action":"page","page":2}}
//! ← {"v":1,"id":7,"ok":{"open":true,"screen":"\\\\.\\DISPLAY2","page":2,"pages":3}}
//! ← {"v":1,"id":7,"error":{"code":"liveNotRunning","message":"…"}}
//! ```
//!
//! Every line is at most [`MAX_LINE`] bytes; a request of another version, of an unknown kind or
//! that does not parse is answered with an error, never executed.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::bridge::AppBridge;
use crate::error::{ToolError, code};

/// Version of the protocol: the server and the app must speak the same one (they are the same
/// executable in practice, but a copy of another version may be open).
pub const PROTOCOL_VERSION: u32 = 1;

/// Longest line accepted, newline excluded (64 KiB): a longer request is refused and the
/// connection closed.
pub const MAX_LINE: usize = 64 * 1024;

/// A request of the server to the app. None deletes anything, changes a setting or touches the
/// tablet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    /// What the app shows.
    AppState,
    /// Show the note at `path` (absolute, inside the library) in the editor.
    OpenNote { path: String },
    /// Drive the presentation of live mode.
    Presentation {
        action: PresentationAction,
        /// 1-based, with [`PresentationAction::Page`].
        #[serde(default, skip_serializing_if = "Option::is_none")]
        page: Option<u32>,
    },
    /// The notes written by the current or last live session.
    LastLiveNote,
}

/// What the `presentation` tool does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PresentationAction {
    /// Project the live page on the screen chosen in the live window (the "Project" button).
    Start,
    /// Close the presentation window ("Stop projecting"); live mode goes on.
    Stop,
    /// The next page (a new blank one after the last, as the → key does).
    Next,
    /// The previous page.
    Previous,
    /// The page numbered `page` (1-based, an existing one).
    Page,
}

/// One line sent to the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub v: u32,
    pub id: u64,
    pub request: Request,
}

/// An error on the wire: the code of the interface's `errorCodes.*` or of the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WireError {
    pub code: String,
    pub message: String,
}

/// One line sent back: `ok` or `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub v: u32,
    /// The request's id (0 when the request could not be read).
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ok: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

// ---------------------------------------------------------------------------------------------
// Answers

/// What the app shows (`app_state`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppView {
    /// Version of the open app.
    pub version: String,
    /// Absolute path of the library open in the app, if any.
    pub library: Option<String>,
    /// `library`, `editor`, `live`, `setup` (no library chosen yet), `loading`, or `unknown`
    /// when the window did not answer in time.
    pub screen: String,
    /// Absolute path of the note in the editor.
    pub note: Option<String>,
    /// The note in the editor has changes not saved yet.
    pub unsaved_changes: bool,
    /// Live mode, once a session was started since the app opened.
    pub live: Option<LiveStatus>,
}

/// Where live mode stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveStatus {
    /// `idle` (stopped), `waiting` (for the tablet), `live` or `reconnecting`.
    pub state: String,
    /// Started and not stopped.
    pub running: bool,
    /// The current page, 1-based, and the number of pages.
    pub page: u32,
    pub pages: u32,
    /// The screen of the presentation window, when it is open.
    pub presentation: Option<String>,
}

/// The note shown in the editor (`open_note`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenedNote {
    /// Absolute path.
    pub path: String,
}

/// The presentation after an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresentationState {
    /// The presentation window is open.
    pub open: bool,
    /// Its screen.
    pub screen: Option<String>,
    /// Current page, 1-based, and number of pages (absent without live mode on screen).
    pub page: Option<u32>,
    pub pages: Option<u32>,
}

/// One page of a live session that became a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveNote {
    /// 1-based.
    pub page: u32,
    pub id: String,
    /// Absolute path when it was written.
    pub path: String,
}

/// The current or last live session (`last_live_note`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveSession {
    /// Still running.
    pub running: bool,
    /// The session ran since the app opened; `false`: remembered from an earlier run on this PC.
    pub this_run: bool,
    /// When the session ended (Unix ms), if it did and it is known.
    pub ended: Option<i64>,
    /// Its pages that became notes, in page order.
    pub notes: Vec<LiveNote>,
}

/// Answer of `last_live_note`: no session at all is not an error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastLive {
    pub session: Option<LiveSession>,
}

// ---------------------------------------------------------------------------------------------
// Server side: one request line in, one response line out

/// The answer to one request line (without its newline), as one line (with its newline). The
/// request is checked (size, version, kind, fields) before `bridge` is asked anything.
pub fn answer_line(line: &[u8], bridge: &dyn AppBridge) -> Vec<u8> {
    let response = match parse_request(line) {
        Ok(envelope) => respond(envelope.id, dispatch(&envelope.request, bridge)),
        Err((id, error)) => respond(id, Err(error)),
    };
    encode(&response)
}

/// The response line when a request is longer than [`MAX_LINE`].
pub fn too_long_line() -> Vec<u8> {
    encode(&respond(
        0,
        Err(ToolError::new(
            code::INVALID_REQUEST,
            format!("request longer than {MAX_LINE} bytes"),
        )),
    ))
}

/// Reads one request line; on failure, the id it had (0 if unknown) and the error to answer.
pub fn parse_request(line: &[u8]) -> Result<Envelope, (u64, ToolError)> {
    let invalid = |id, message: String| (id, ToolError::new(code::INVALID_REQUEST, message));
    if line.len() > MAX_LINE {
        return Err(invalid(0, format!("request longer than {MAX_LINE} bytes")));
    }
    let value: Value = serde_json::from_slice(line)
        .map_err(|error| invalid(0, format!("not a JSON request: {error}")))?;
    let id = value.get("id").and_then(Value::as_u64).unwrap_or(0);
    match value.get("v").and_then(Value::as_u64) {
        Some(v) if v == u64::from(PROTOCOL_VERSION) => {}
        Some(v) => {
            return Err((
                id,
                ToolError::new(
                    code::BRIDGE_VERSION,
                    format!(
                        "the open Ardoise speaks version {PROTOCOL_VERSION} of the bridge, this server version {v}: use the same copy of Ardoise for both"
                    ),
                ),
            ));
        }
        None => return Err(invalid(id, "the request has no version `v`".into())),
    }
    if value.get("id").and_then(Value::as_u64).is_none() {
        return Err(invalid(0, "the request has no numeric `id`".into()));
    }
    let kind = value
        .get("request")
        .and_then(|request| request.get("kind"))
        .and_then(Value::as_str);
    let fields: &[&str] = match kind {
        Some("app_state" | "last_live_note") => &["kind"],
        Some("open_note") => &["kind", "path"],
        Some("presentation") => &["kind", "action", "page"],
        Some(kind) => return Err(invalid(id, format!("unknown request `{kind}`"))),
        None => return Err(invalid(id, "the request has no `kind`".into())),
    };
    // Nothing the app would not read: a request with more fields is from another version.
    let extra = |object: Option<&serde_json::Map<String, Value>>, allowed: &[&str]| {
        object.and_then(|object| {
            object
                .keys()
                .find(|key| !allowed.contains(&key.as_str()))
                .cloned()
        })
    };
    if let Some(field) = extra(value.as_object(), &["v", "id", "request"])
        .or_else(|| extra(value.get("request").and_then(Value::as_object), fields))
    {
        return Err(invalid(id, format!("unknown field `{field}`")));
    }
    serde_json::from_value(value).map_err(|error| invalid(id, format!("bad request: {error}")))
}

/// Runs `request` on `bridge`; the answer as a JSON value.
pub fn dispatch(request: &Request, bridge: &dyn AppBridge) -> Result<Value, ToolError> {
    match request {
        Request::AppState => to_value(bridge.app_state()?),
        Request::OpenNote { path } => to_value(bridge.open_note(path)?),
        Request::Presentation { action, page } => to_value(bridge.presentation(*action, *page)?),
        Request::LastLiveNote => to_value(bridge.last_live_note()?),
    }
}

fn to_value(value: impl Serialize) -> Result<Value, ToolError> {
    serde_json::to_value(value)
        .map_err(|error| ToolError::new("unexpected", format!("cannot encode the answer: {error}")))
}

fn respond(id: u64, outcome: Result<Value, ToolError>) -> Response {
    match outcome {
        Ok(value) => Response {
            v: PROTOCOL_VERSION,
            id,
            ok: Some(value),
            error: None,
        },
        Err(error) => Response {
            v: PROTOCOL_VERSION,
            id,
            ok: None,
            error: Some(WireError {
                code: error.code.to_owned(),
                message: error.message,
            }),
        },
    }
}

/// `value` as one JSON line ending with `\n`.
pub fn encode(value: &impl Serialize) -> Vec<u8> {
    // Serialising these plain structures cannot fail; an empty object stands in if it ever did.
    let mut line = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    line.push(b'\n');
    line
}

// ---------------------------------------------------------------------------------------------
// Client side

/// The request line for `request` (with its newline).
pub fn request_line(id: u64, request: &Request) -> Vec<u8> {
    encode(&Envelope {
        v: PROTOCOL_VERSION,
        id,
        request: request.clone(),
    })
}

/// Reads the response line to request `id` and decodes its answer as a `T`.
pub fn read_response<T: DeserializeOwned>(line: &[u8], id: u64) -> Result<T, ToolError> {
    let not_understood = |detail: String| {
        ToolError::new(
            code::BRIDGE_VERSION,
            format!("the open Ardoise gave an answer this server does not understand: {detail}"),
        )
    };
    let response: Response =
        serde_json::from_slice(line).map_err(|error| not_understood(error.to_string()))?;
    if let Some(error) = response.error {
        return Err(ToolError::new(wire_code(&error.code), error.message));
    }
    if response.v != PROTOCOL_VERSION {
        return Err(not_understood(format!("version {}", response.v)));
    }
    if response.id != id {
        return Err(not_understood(format!(
            "answer to request {} instead of {id}",
            response.id
        )));
    }
    let value = response
        .ok
        .ok_or_else(|| not_understood("neither `ok` nor `error`".into()))?;
    serde_json::from_value(value).map_err(|error| not_understood(error.to_string()))
}

/// The codes the app may answer (the interface's `errorCodes.*` it uses, and the server's):
/// [`ToolError`] holds static codes, and an unknown one becomes `unexpected`.
const WIRE_CODES: [&str; 12] = [
    code::INVALID_ARGUMENTS,
    code::APP_NOT_RUNNING,
    code::APP_NOT_RESPONDING,
    code::APP_BUSY,
    code::BRIDGE_VERSION,
    code::INVALID_REQUEST,
    "liveNotRunning",
    "noMonitor",
    "noteNotFound",
    "noLibrary",
    "presentation",
    "unexpected",
];

/// The static code for `code` received on the wire (`unexpected` for one this server does not know).
pub fn wire_code(code: &str) -> &'static str {
    WIRE_CODES
        .iter()
        .find(|known| **known == code)
        .copied()
        .unwrap_or("unexpected")
}

// ---------------------------------------------------------------------------------------------
// Lines

/// Bytes received and not yet a whole line, bounded by [`MAX_LINE`].
#[derive(Debug, Default)]
pub struct LineBuffer {
    pending: Vec<u8>,
}

/// The line being received is longer than [`MAX_LINE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineTooLong;

impl LineBuffer {
    pub fn new() -> LineBuffer {
        LineBuffer::default()
    }

    /// Adds received bytes.
    pub fn push(&mut self, bytes: &[u8]) {
        self.pending.extend_from_slice(bytes);
    }

    /// The next complete line (without `\n` and a `\r` before it), if one was received;
    /// [`LineTooLong`] once the line under way is over the limit, with or without its end.
    pub fn next_line(&mut self) -> Result<Option<Vec<u8>>, LineTooLong> {
        match self.pending.iter().position(|&byte| byte == b'\n') {
            Some(end) if end > MAX_LINE => Err(LineTooLong),
            Some(end) => {
                let mut line: Vec<u8> = self.pending.drain(..=end).collect();
                line.pop();
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                Ok(Some(line))
            }
            None if self.pending.len() > MAX_LINE => Err(LineTooLong),
            None => Ok(None),
        }
    }

    /// Bytes received after the last complete line.
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bridge::NoApp;
    use serde_json::json;
    use std::sync::Mutex;

    /// Answers every request, and remembers what it was asked.
    #[derive(Default)]
    struct Echo {
        asked: Mutex<Vec<String>>,
    }

    impl AppBridge for Echo {
        fn app_state(&self) -> Result<AppView, ToolError> {
            self.asked.lock().unwrap().push("app_state".into());
            Ok(AppView {
                version: "1.2.3".into(),
                library: Some("C:\\Notes".into()),
                screen: "library".into(),
                note: None,
                unsaved_changes: false,
                live: None,
            })
        }

        fn open_note(&self, path: &str) -> Result<OpenedNote, ToolError> {
            self.asked.lock().unwrap().push(format!("open {path}"));
            Ok(OpenedNote { path: path.into() })
        }

        fn presentation(
            &self,
            action: PresentationAction,
            page: Option<u32>,
        ) -> Result<PresentationState, ToolError> {
            self.asked
                .lock()
                .unwrap()
                .push(format!("presentation {action:?} {page:?}"));
            if action == PresentationAction::Start {
                return Err(ToolError::new("liveNotRunning", "no live mode"));
            }
            Ok(PresentationState {
                open: false,
                screen: None,
                page,
                pages: Some(3),
            })
        }

        fn last_live_note(&self) -> Result<LastLive, ToolError> {
            Ok(LastLive { session: None })
        }
    }

    fn round_trip<T: DeserializeOwned>(
        request: Request,
        bridge: &dyn AppBridge,
    ) -> Result<T, ToolError> {
        let line = request_line(41, &request);
        let answer = answer_line(&line[..line.len() - 1], bridge);
        assert_eq!(answer.last(), Some(&b'\n'));
        read_response(&answer[..answer.len() - 1], 41)
    }

    #[test]
    fn every_request_makes_the_round_trip() {
        let echo = Echo::default();
        let view: AppView = round_trip(Request::AppState, &echo).unwrap();
        assert_eq!(view.screen, "library");
        let opened: OpenedNote = round_trip(
            Request::OpenNote {
                path: "C:\\Notes\\Cours d'optique.ardoise".into(),
            },
            &echo,
        )
        .unwrap();
        assert_eq!(opened.path, "C:\\Notes\\Cours d'optique.ardoise");
        let state: PresentationState = round_trip(
            Request::Presentation {
                action: PresentationAction::Page,
                page: Some(2),
            },
            &echo,
        )
        .unwrap();
        assert_eq!((state.page, state.pages), (Some(2), Some(3)));
        let live: LastLive = round_trip(Request::LastLiveNote, &echo).unwrap();
        assert_eq!(live.session, None);
        assert_eq!(
            *echo.asked.lock().unwrap(),
            [
                "app_state",
                "open C:\\Notes\\Cours d'optique.ardoise",
                "presentation Page Some(2)",
            ]
        );
    }

    #[test]
    fn the_wire_spells_requests_as_documented() {
        let line = request_line(
            7,
            &Request::Presentation {
                action: PresentationAction::Next,
                page: None,
            },
        );
        let value: Value = serde_json::from_slice(&line).unwrap();
        assert_eq!(
            value,
            json!({ "v": 1, "id": 7, "request": { "kind": "presentation", "action": "next" } })
        );
        let line = request_line(8, &Request::LastLiveNote);
        assert_eq!(
            String::from_utf8(line).unwrap(),
            "{\"v\":1,\"id\":8,\"request\":{\"kind\":\"last_live_note\"}}\n"
        );
    }

    #[test]
    fn errors_of_the_app_keep_their_code() {
        let error = round_trip::<PresentationState>(
            Request::Presentation {
                action: PresentationAction::Start,
                page: None,
            },
            &Echo::default(),
        )
        .unwrap_err();
        assert_eq!(error.code, "liveNotRunning");
        assert_eq!(error.message, "no live mode");
        // Without an app, every request says so.
        let error = round_trip::<AppView>(Request::AppState, &NoApp).unwrap_err();
        assert_eq!(error.code, code::APP_NOT_RUNNING);
        assert_eq!(error.message, "Ardoise is not open");
    }

    fn refusal(line: &[u8]) -> (u64, WireError) {
        let echo = Echo::default();
        let answer = answer_line(line, &echo);
        assert!(echo.asked.lock().unwrap().is_empty(), "the app was asked");
        let response: Response = serde_json::from_slice(&answer).unwrap();
        assert_eq!(response.ok, None);
        (response.id, response.error.unwrap())
    }

    #[test]
    fn unknown_requests_are_refused_without_running_anything() {
        let (id, error) = refusal(br#"{"v":1,"id":3,"request":{"kind":"trash_note","note":"x"}}"#);
        assert_eq!(id, 3);
        assert_eq!(error.code, code::INVALID_REQUEST);
        assert!(error.message.contains("unknown request `trash_note`"));
        // Unknown fields, missing fields, wrong values.
        for line in [
            &br#"{"v":1,"id":3,"request":{"kind":"app_state","delete":true}}"#[..],
            br#"{"v":1,"id":3,"request":{"kind":"open_note"}}"#,
            br#"{"v":1,"id":3,"request":{"kind":"presentation","action":"explode"}}"#,
            br#"{"v":1,"id":3,"request":{"action":"start"}}"#,
            br#"{"v":1,"request":{"kind":"app_state"}}"#,
            br#"{"v":1,"id":-4,"request":{"kind":"app_state"}}"#,
            b"not json",
            b"",
        ] {
            let (_, error) = refusal(line);
            assert_eq!(
                error.code,
                code::INVALID_REQUEST,
                "{}",
                String::from_utf8_lossy(line)
            );
        }
    }

    #[test]
    fn another_version_is_refused() {
        let (id, error) = refusal(br#"{"v":2,"id":5,"request":{"kind":"app_state"}}"#);
        assert_eq!((id, error.code.as_str()), (5, code::BRIDGE_VERSION));
        let (_, error) = refusal(br#"{"id":5,"request":{"kind":"app_state"}}"#);
        assert_eq!(error.code, code::INVALID_REQUEST);
        // The client refuses an answer of another version, or to another request.
        let other = br#"{"v":2,"id":41,"ok":{"path":"x"}}"#;
        assert_eq!(
            read_response::<OpenedNote>(other, 41).unwrap_err().code,
            code::BRIDGE_VERSION
        );
        let late = br#"{"v":1,"id":40,"ok":{"path":"x"}}"#;
        assert_eq!(
            read_response::<OpenedNote>(late, 41).unwrap_err().code,
            code::BRIDGE_VERSION
        );
        // An error of a version that the client does not know still reaches the assistant.
        let refused = br#"{"v":2,"id":0,"error":{"code":"bridgeVersion","message":"versions"}}"#;
        assert_eq!(
            read_response::<OpenedNote>(refused, 41).unwrap_err().code,
            code::BRIDGE_VERSION
        );
    }

    #[test]
    fn requests_are_bounded() {
        let mut long = br#"{"v":1,"id":9,"request":{"kind":"open_note","path":""#.to_vec();
        long.extend(std::iter::repeat_n(b'a', MAX_LINE));
        long.extend(br#""}}"#);
        let (_, error) = refusal(&long);
        assert_eq!(error.code, code::INVALID_REQUEST);
        assert!(error.message.contains("longer than"));
        let answer: Response = serde_json::from_slice(&too_long_line()).unwrap();
        assert_eq!(answer.error.unwrap().code, code::INVALID_REQUEST);
    }

    #[test]
    fn lines_are_cut_and_bounded() {
        let mut buffer = LineBuffer::new();
        buffer.push(b"{\"a\":1}\r\n{\"b\"");
        assert_eq!(buffer.next_line(), Ok(Some(b"{\"a\":1}".to_vec())));
        assert_eq!(buffer.next_line(), Ok(None));
        buffer.push(b":2}\n");
        assert_eq!(buffer.next_line(), Ok(Some(b"{\"b\":2}".to_vec())));
        assert!(buffer.is_empty());
        // A line of exactly the limit passes; one byte more is refused, ended or not.
        buffer.push(&vec![b'x'; MAX_LINE]);
        buffer.push(b"\n");
        assert_eq!(buffer.next_line().unwrap().map(|l| l.len()), Some(MAX_LINE));
        buffer.push(&vec![b'x'; MAX_LINE + 1]);
        assert_eq!(buffer.next_line(), Err(LineTooLong));
        let mut ended = LineBuffer::new();
        ended.push(&vec![b'x'; MAX_LINE + 1]);
        ended.push(b"\n");
        assert_eq!(ended.next_line(), Err(LineTooLong));
    }

    #[test]
    fn unknown_codes_become_unexpected() {
        assert_eq!(wire_code("liveNotRunning"), "liveNotRunning");
        assert_eq!(wire_code("appBusy"), code::APP_BUSY);
        assert_eq!(wire_code("somethingNew"), "unexpected");
    }
}
