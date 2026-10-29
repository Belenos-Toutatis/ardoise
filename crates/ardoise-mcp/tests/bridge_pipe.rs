//! The bridge's named pipe for real (Windows): a server and its client in this process, on a
//! pipe name of their own. Round trip of every request, the DACL read back from Windows, one
//! server per name, the client's "not open" and timeout paths, malformed and oversized lines.

#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use ardoise_mcp::ToolError;
use ardoise_mcp::bridge::pipe::{
    MAX_CONNECTIONS, PipeClient, PipeError, PipeServer, current_user_sid,
};
use ardoise_mcp::bridge::protocol::{MAX_LINE, Response};
use ardoise_mcp::bridge::{
    AppBridge, AppView, LastLive, LiveNote, LiveSession, OpenedNote, PipeBridge,
    PresentationAction, PresentationState, Timeouts,
};

/// A pipe name no other test (or other run) uses.
fn unique_name() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    format!(
        r"\\.\pipe\ardoise-test-{}-{}-{nanos}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    )
}

/// The app's side: fixed answers, optionally slow.
#[derive(Default)]
struct App {
    delay: Duration,
}

impl AppBridge for App {
    fn app_state(&self) -> Result<AppView, ToolError> {
        std::thread::sleep(self.delay);
        Ok(AppView {
            version: "9.9.9".into(),
            library: Some(r"C:\Bibliothèque".into()),
            screen: "editor".into(),
            note: Some(r"C:\Bibliothèque\Cours d'optique.ardoise".into()),
            unsaved_changes: true,
            live: None,
        })
    }

    fn open_note(&self, path: &str) -> Result<OpenedNote, ToolError> {
        Ok(OpenedNote { path: path.into() })
    }

    fn presentation(
        &self,
        action: PresentationAction,
        page: Option<u32>,
    ) -> Result<PresentationState, ToolError> {
        if action == PresentationAction::Start {
            return Err(ToolError::new("liveNotRunning", "live mode is not open"));
        }
        Ok(PresentationState {
            open: false,
            screen: None,
            page,
            pages: Some(4),
        })
    }

    fn last_live_note(&self) -> Result<LastLive, ToolError> {
        Ok(LastLive {
            session: Some(LiveSession {
                running: true,
                this_run: true,
                ended: None,
                notes: vec![LiveNote {
                    page: 1,
                    id: "abc".into(),
                    path: r"C:\Bibliothèque\live.ardoise".into(),
                }],
            }),
        })
    }
}

fn serve(app: App) -> (PipeServer, String) {
    let name = unique_name();
    let server = PipeServer::start(&name, Arc::new(app)).unwrap();
    (server, name)
}

#[test]
fn every_request_makes_the_round_trip_through_the_pipe() {
    let (server, name) = serve(App::default());
    let bridge = PipeBridge::new(&name);
    let view = bridge.app_state().unwrap();
    assert_eq!(view.version, "9.9.9");
    assert_eq!(
        view.note.as_deref(),
        Some(r"C:\Bibliothèque\Cours d'optique.ardoise")
    );
    assert!(view.unsaved_changes);
    let opened = bridge.open_note(r"C:\Bibliothèque\é è.ardoise").unwrap();
    assert_eq!(opened.path, r"C:\Bibliothèque\é è.ardoise");
    let state = bridge
        .presentation(PresentationAction::Page, Some(3))
        .unwrap();
    assert_eq!((state.page, state.pages), (Some(3), Some(4)));
    let refused = bridge
        .presentation(PresentationAction::Start, None)
        .unwrap_err();
    assert_eq!(refused.code, "liveNotRunning");
    let last = bridge.last_live_note().unwrap();
    assert_eq!(last.session.unwrap().notes[0].id, "abc");
    server.stop();
    // Once stopped, the app is not open any more.
    assert_eq!(bridge.app_state().unwrap_err().code, "appNotRunning");
}

#[test]
fn one_connection_carries_several_requests() {
    let (_server, name) = serve(App::default());
    let mut client = PipeClient::connect(&name, Duration::from_secs(2)).unwrap();
    for id in 1..=3u64 {
        let line = format!("{{\"v\":1,\"id\":{id},\"request\":{{\"kind\":\"app_state\"}}}}\n");
        let answer = client
            .exchange(line.as_bytes(), Duration::from_secs(5))
            .unwrap();
        let response: Response = serde_json::from_slice(&answer).unwrap();
        assert_eq!(response.id, id);
        assert!(response.ok.is_some());
    }
}

#[test]
fn only_the_current_user_may_open_the_pipe() {
    let (_server, name) = serve(App::default());
    let client = PipeClient::connect(&name, Duration::from_secs(2)).unwrap();
    let rules = client.access_rules().unwrap();
    let me = current_user_sid().unwrap();
    assert!(me.starts_with("S-1-5-"), "{me}");
    let allowed: Vec<_> = rules.iter().filter(|rule| rule.allow).collect();
    assert_eq!(allowed.len(), 1, "{rules:?}");
    assert_eq!(allowed[0].sid, me);
    // Network logons are denied; nobody else (Everyone, Users, Administrators, anonymous) appears.
    assert!(
        rules
            .iter()
            .any(|rule| !rule.allow && rule.sid == "S-1-5-2"),
        "{rules:?}"
    );
    for other in [
        "S-1-1-0",
        "S-1-5-32-545",
        "S-1-5-32-544",
        "S-1-5-7",
        "S-1-5-11",
    ] {
        assert!(
            !rules.iter().any(|rule| rule.sid == other),
            "{other} in {rules:?}"
        );
    }
    assert_eq!(rules.len(), 2, "{rules:?}");
}

#[test]
fn a_name_already_served_is_never_served_twice() {
    let (_server, name) = serve(App::default());
    match PipeServer::start(&name, Arc::new(App::default())) {
        Err(PipeError::AlreadyServed(served)) => assert_eq!(served, name),
        Err(other) => panic!("unexpected error: {other}"),
        Ok(_) => panic!("a second server took the name"),
    }
    // The first one still answers.
    assert_eq!(PipeBridge::new(&name).app_state().unwrap().version, "9.9.9");
}

#[test]
fn without_a_server_the_app_is_not_open() {
    let started = Instant::now();
    let error = PipeBridge::new(unique_name()).open_note("x").unwrap_err();
    assert_eq!(error.code, "appNotRunning");
    assert_eq!(error.message, "Ardoise is not open");
    // Said at once, not after a timeout.
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_slow_app_times_out_with_its_own_code() {
    let (_server, name) = serve(App {
        delay: Duration::from_secs(3),
    });
    let bridge = PipeBridge::with_timeouts(
        &name,
        Timeouts {
            connect: Duration::from_secs(1),
            quick: Duration::from_millis(400),
            action: Duration::from_millis(400),
        },
    );
    let started = Instant::now();
    let error = bridge.app_state().unwrap_err();
    assert_eq!(error.code, "appNotResponding", "{error:?}");
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn malformed_and_oversized_lines_are_refused() {
    let (_server, name) = serve(App::default());
    let mut client = PipeClient::connect(&name, Duration::from_secs(2)).unwrap();
    for line in [
        &b"hello\n"[..],
        b"{\"v\":1,\"id\":4,\"request\":{\"kind\":\"forget_tablet\"}}\n",
        b"{\"v\":7,\"id\":4,\"request\":{\"kind\":\"app_state\"}}\n",
    ] {
        let answer = client.exchange(line, Duration::from_secs(5)).unwrap();
        let response: Response = serde_json::from_slice(&answer).unwrap();
        let code = response.error.expect("an error").code;
        assert!(
            code == "invalidRequest" || code == "bridgeVersion",
            "{code}"
        );
    }
    // A line over the limit gets one error, then the connection is closed.
    let mut long = vec![b'a'; MAX_LINE + 10];
    long.push(b'\n');
    client.send_raw(&long, Duration::from_secs(5)).unwrap();
    let answer = client.read_line(Duration::from_secs(5)).unwrap().unwrap();
    let response: Response = serde_json::from_slice(&answer).unwrap();
    assert_eq!(response.error.unwrap().code, "invalidRequest");
    assert_eq!(client.read_line(Duration::from_secs(5)).unwrap(), None);
}

fn app_state_line(id: u64) -> Vec<u8> {
    format!("{{\"v\":1,\"id\":{id},\"request\":{{\"kind\":\"app_state\"}}}}\n").into_bytes()
}

#[test]
fn a_request_far_over_the_limit_still_gets_its_answer() {
    let (_server, name) = serve(App::default());
    let mut client = PipeClient::connect(&name, Duration::from_secs(2)).unwrap();
    // Far more than the pipe holds: the app answers and closes while the client still writes.
    let mut long = vec![b'a'; 4 * MAX_LINE * 4];
    long.push(b'\n');
    let answer = client.exchange(&long, Duration::from_secs(5)).unwrap();
    let response: Response = serde_json::from_slice(&answer).unwrap();
    assert_eq!(response.error.unwrap().code, "invalidRequest");
    assert_eq!(client.read_line(Duration::from_secs(5)).unwrap(), None);
}

#[test]
fn connections_over_the_limit_are_told_the_app_is_busy() {
    let (_server, name) = serve(App::default());
    // Every place taken: each client is counted once it has had an answer.
    let mut held = Vec::new();
    for id in 1..=MAX_CONNECTIONS as u64 {
        let mut client = PipeClient::connect(&name, Duration::from_secs(2)).unwrap();
        let answer = client
            .exchange(&app_state_line(id), Duration::from_secs(5))
            .unwrap();
        let response: Response = serde_json::from_slice(&answer).unwrap();
        assert_eq!(response.id, id);
        assert!(response.ok.is_some());
        held.push(client);
    }
    // One more: told so, under its own id, then the connection is closed.
    let mut extra = PipeClient::connect(&name, Duration::from_secs(2)).unwrap();
    let answer = extra
        .exchange(&app_state_line(77), Duration::from_secs(5))
        .unwrap();
    let response: Response = serde_json::from_slice(&answer).unwrap();
    assert_eq!(response.id, 77);
    assert_eq!(response.error.unwrap().code, "appBusy");
    assert_eq!(extra.read_line(Duration::from_secs(5)).unwrap(), None);
    // Through the tools' client: `appBusy`, not a lost connection.
    let error = PipeBridge::new(&name).app_state().unwrap_err();
    assert_eq!(error.code, "appBusy", "{error:?}");
    assert!(error.message.contains("try again"), "{error:?}");
    // A place freed: requests go through again (the server sees the close at its next read).
    drop(held.pop());
    let bridge = PipeBridge::new(&name);
    let started = Instant::now();
    loop {
        match bridge.app_state() {
            Ok(view) => {
                assert_eq!(view.version, "9.9.9");
                break;
            }
            Err(error) if error.code == "appBusy" && started.elapsed() < Duration::from_secs(3) => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("{error:?}"),
        }
    }
    // The clients held all along still work.
    let answer = held[0]
        .exchange(&app_state_line(9), Duration::from_secs(5))
        .unwrap();
    assert_eq!(serde_json::from_slice::<Response>(&answer).unwrap().id, 9);
}
