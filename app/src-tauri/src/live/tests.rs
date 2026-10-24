//! Tests of the live hub: windows and batches, pages written to the library and read back,
//! the JSON shapes of `app/ui/src/lib/live.ts`, and the latency of the Rust side.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ardoise_core::library::Library;
use ardoise_core::note::{Origin, PageSize, Paper};
use ardoise_core::settings::{Hand, LiveButton};
use ardoise_device::service::{LiveInput, LiveSink, LiveState};
use ardoise_ink::{Rgba, Tool};
use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::Point as PenPoint;
use serde_json::{Value, json};
use tauri::ipc::{Channel, InvokeResponseBody};

use super::hub::{
    BatchSink, ButtonSetting, HubConfig, HubSink, LiveError, LiveHub, Start, StartOptions,
};
use super::model::{
    LiveBatch, LiveCommand, LiveCommandKind, LiveEvent, LivePoint, LiveSessionState, LiveStyle,
    LiveViewState,
};
use super::session::LiveSession;
use crate::library::path_text;
use crate::library::tests::library;
use crate::presentation::MonitorInfo;

const A5: PageSize = PageSize {
    width: 21_600,
    height: 14_800,
};

/// A window: keeps the batches it receives; refuses them once `closed`.
#[derive(Default)]
struct Window {
    batches: Mutex<Vec<LiveBatch>>,
    closed: AtomicBool,
}

impl BatchSink for Window {
    fn send(&self, batch: LiveBatch) -> Result<(), String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err("the window is closed".into());
        }
        self.batches.lock().unwrap().push(batch);
        Ok(())
    }
}

impl Window {
    fn events(&self) -> Vec<LiveEvent> {
        self.batches
            .lock()
            .unwrap()
            .iter()
            .flat_map(|batch| batch.events.clone())
            .collect()
    }

    fn has(&self, wanted: impl Fn(&LiveEvent) -> bool) -> bool {
        self.events().iter().any(wanted)
    }
}

fn is_stroke_end(event: &LiveEvent) -> bool {
    matches!(event, LiveEvent::StrokeEnd { .. })
}

/// Points carried by strokeStart and points events.
fn points_of(events: &[LiveEvent]) -> Vec<LivePoint> {
    events
        .iter()
        .flat_map(|event| match event {
            LiveEvent::StrokeStart { stroke, .. } => stroke.points.clone(),
            LiveEvent::Points { points, .. } => points.clone(),
            _ => Vec::new(),
        })
        .collect()
}

fn config() -> HubConfig {
    HubConfig {
        flush_interval: Duration::from_millis(4),
        save_interval: Duration::from_millis(150),
    }
}

fn fixed(button: LiveButton) -> ButtonSetting {
    Arc::new(move || button)
}

fn options(library: &Library, button: ButtonSetting) -> StartOptions {
    StartOptions {
        library: library.clone(),
        french: true,
        size: A5,
        button,
    }
}

/// Starts a session that streams from an A5 tablet.
fn start(hub: &LiveHub, options: StartOptions) -> HubSink {
    let Start::New { mut sink, snapshot } = hub.start(options).unwrap() else {
        panic!("a session is already running");
    };
    assert_eq!(snapshot.state, LiveSessionState::Waiting);
    sink.input(LiveInput::Started {
        width: A5.width,
        height: A5.height,
        hand: Hand::Right,
    });
    sink
}

fn pen(x: u16, y: u16, pressure: u16) -> LiveRecord {
    LiveRecord::Pen(PenPoint { x, y, pressure })
}

fn send(sink: &mut HubSink, records: &[LiveRecord]) {
    sink.input(LiveInput::Records {
        records: records.to_vec(),
        received: SystemTime::now(),
    });
}

/// `n` samples with pressure, then the pen lifted (hovering), in notifications of 3 records.
fn stroke(sink: &mut HubSink, x: u16, n: u16) {
    let mut records: Vec<LiveRecord> = (0..n)
        .map(|i| pen(x + i * 10, 1000 + i * 5, 800 + i))
        .collect();
    records.push(pen(x + n * 10, 1000, 0));
    for chunk in records.chunks(3) {
        send(sink, chunk);
    }
}

fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn notes_in(library: &Library) -> Vec<PathBuf> {
    library.list().notes.into_iter().map(|e| e.path).collect()
}

fn stroke_count(library: &Library, path: &Path) -> Option<usize> {
    library
        .read(path)
        .ok()
        .map(|loaded| loaded.file.note.strokes.len())
}

#[test]
fn every_window_gets_the_same_batches_and_a_closed_one_is_dropped() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let main = Arc::new(Window::default());
    let projector = Arc::new(Window::default());
    let closed = Arc::new(Window::default());
    closed.closed.store(true, Ordering::SeqCst);
    let first = hub.subscribe("main", main.clone());
    assert_eq!(first.state, LiveSessionState::Idle);
    assert_eq!(first.page_count, 1);
    hub.subscribe("presentation", projector.clone());
    hub.subscribe("other", closed.clone());

    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    stroke(&mut sink, 3000, 20);
    wait_until("the end of the stroke in both windows", || {
        main.has(is_stroke_end) && projector.has(is_stroke_end)
    });
    wait_until("the closed window to be dropped", || {
        hub.subscriber_count() == 2
    });

    let (a, b) = (main.events(), projector.events());
    let common = a.len().min(b.len());
    assert_eq!(a[..common], b[..common]);
    // The first batch starts with the snapshot the subscription returned.
    assert!(
        matches!(&a[0], LiveEvent::Snapshot { snapshot } if snapshot.state == LiveSessionState::Idle)
    );
    // Every point once, in order, sent after it was received.
    let points = points_of(&a);
    assert_eq!(points.len(), 20);
    assert!(points.windows(2).all(|w| w[0].t + 5 <= w[1].t));
    let batches = main.batches.lock().unwrap();
    for batch in batches.iter() {
        for point in points_of(&batch.events) {
            assert!(batch.sent_at >= point.recv);
        }
    }
    // A window subscribing again replaces its subscription.
    drop(batches);
    hub.subscribe("main", Arc::new(Window::default()));
    assert_eq!(hub.subscriber_count(), 2);
    hub.stop().unwrap();
}

#[test]
fn batches_go_through_tauri_channels_as_the_interface_reads_them() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let received = Arc::new(Mutex::new(Vec::<String>::new()));
    let channel = {
        let received = received.clone();
        Channel::<LiveBatch>::new(move |body| {
            if let InvokeResponseBody::Json(json) = body {
                received.lock().unwrap().push(json);
            }
            Ok(())
        })
    };
    hub.subscribe("main", Arc::new(channel));
    let gone = Channel::<LiveBatch>::new(|_| Err(tauri::Error::WebviewNotFound));
    hub.subscribe("presentation", Arc::new(gone));
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    stroke(&mut sink, 3000, 4);
    wait_until("the stroke's end", || {
        received
            .lock()
            .unwrap()
            .iter()
            .any(|json| json.contains(r#""type":"strokeEnd""#))
    });
    wait_until("the closed window to be dropped", || {
        hub.subscriber_count() == 1
    });
    let batches: Vec<Value> = received
        .lock()
        .unwrap()
        .iter()
        .map(|json| serde_json::from_str(json).unwrap())
        .collect();
    for batch in &batches {
        assert_eq!(keys(batch), ["events", "sentAt"]);
        assert!(batch["sentAt"].as_f64().unwrap() > 1.7e12);
    }
    assert_eq!(batches[0]["events"][0]["type"], "snapshot");
    hub.stop().unwrap();
}

#[test]
fn a_window_opened_mid_stroke_gets_the_stroke_so_far() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    send(
        &mut sink,
        &[pen(100, 100, 500), pen(110, 100, 500), pen(120, 100, 500)],
    );
    let late = Arc::new(Window::default());
    let snapshot = hub.subscribe("presentation", late.clone());
    assert!(
        snapshot.page.strokes.is_empty(),
        "the stroke is in progress"
    );
    send(&mut sink, &[pen(130, 100, 500), pen(140, 100, 0)]);
    wait_until("the stroke's end", || late.has(is_stroke_end));
    let events = late.events();
    assert!(matches!(events[0], LiveEvent::Snapshot { .. }));
    assert!(matches!(events[1], LiveEvent::StrokeStart { .. }));
    let xs: Vec<f32> = points_of(&events).iter().map(|p| p.x).collect();
    assert_eq!(xs, [100.0, 110.0, 120.0, 130.0]);
    hub.stop().unwrap();
}

#[test]
fn pages_become_notes_written_atomically_and_read_back() {
    let (dir, library) = library();
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));

    // Hovering writes nothing; the note appears when the first stroke ends.
    send(&mut sink, &[pen(10, 10, 0), pen(20, 20, 0)]);
    std::thread::sleep(Duration::from_millis(30));
    assert!(notes_in(&library).is_empty());
    stroke(&mut sink, 3000, 30);
    wait_until("the note of page 1", || notes_in(&library).len() == 1);
    let path = notes_in(&library).remove(0);
    assert_eq!(path.parent(), Some(dir.path()), "in the library root");
    let loaded = library.read(&path).unwrap();
    let note = &loaded.file.note;
    assert_eq!(note.origin, Origin::Live);
    assert_eq!(note.title, "");
    assert_eq!(note.page, A5);
    assert_eq!(note.layers[0].name, "Calque 1");
    assert_eq!(note.background.paper, Paper::White);
    assert_eq!(note.strokes.len(), 1);
    let points = &note.strokes[0].points;
    assert_eq!(points.len(), 30);
    assert_eq!(points[0].t, 0);
    // Stored in 1/65535 (format/strokes.rs).
    assert!((points[0].pressure - 800.0 / 2047.0).abs() < 1e-4);
    assert!(loaded.file.thumbnail.is_some(), "the library grid shows it");
    let first_path = path_text(&path);
    wait_until("the saved event", || {
        window.has(|e| {
            *e == LiveEvent::Saved {
                paths: vec![first_path.clone()],
            }
        })
    });

    // More strokes: the same file is rewritten within the save interval.
    stroke(&mut sink, 6000, 10);
    wait_until("page 1 rewritten", || {
        stroke_count(&library, &path) == Some(2)
    });
    assert_eq!(notes_in(&library), std::slice::from_ref(&path));

    // Page 2 on the blackboard: a second note.
    hub.command(LiveCommandKind::NextPage).unwrap();
    let mut view = hub.snapshot().view;
    view.board = true;
    hub.set_view(view).unwrap();
    stroke(&mut sink, 9000, 5);
    stroke(&mut sink, 9500, 5);
    wait_until("the note of page 2", || notes_in(&library).len() == 2);

    // Stop: everything written, the paths in page order.
    let saved = hub.stop().unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(saved[0], first_path);
    assert_eq!(hub.snapshot().saved, saved);
    assert_eq!(hub.snapshot().state, LiveSessionState::Idle);
    let second = library.read(Path::new(&saved[1])).unwrap().file.note;
    assert_eq!(second.strokes.len(), 2);
    assert_eq!(second.background.paper, Paper::Slate);
    assert_eq!(
        (second.strokes[0].tool, second.strokes[0].color),
        (Tool::Chalk, Rgba::WHITE)
    );
    assert_ne!(second.id, note.id);
    // The atomic writes leave nothing behind.
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name != "_ardoise")
        .collect();
    names.sort();
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(
        names.iter().all(|name| name.ends_with(".ardoise")),
        "{names:?}"
    );

    // The tablet's input after the stop is ignored; stopping again changes nothing.
    stroke(&mut sink, 3000, 5);
    assert_eq!(hub.stop().unwrap(), saved);
    assert_eq!(stroke_count(&library, Path::new(&saved[1])), Some(2));
}

#[test]
fn a_note_moved_meanwhile_is_followed_and_one_changed_elsewhere_is_kept() {
    let (dir, library) = library();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    stroke(&mut sink, 3000, 5);
    wait_until("the note", || notes_in(&library).len() == 1);
    let created = notes_in(&library).remove(0);

    // Moved to a binder from the library grid: the next writes follow it.
    let moved = library.move_to(&created, Path::new("Classe")).unwrap();
    stroke(&mut sink, 4000, 5);
    wait_until("the moved note rewritten", || {
        stroke_count(&library, &moved) == Some(2)
    });
    assert_eq!(notes_in(&library), std::slice::from_ref(&moved));

    // Changed elsewhere (renamed on another PC): left alone, the page goes on in a new note.
    let renamed = library.retitle(&moved, "Cours", None).unwrap().path;
    stroke(&mut sink, 5000, 5);
    wait_until("a new note", || notes_in(&library).len() == 2);
    let kept = library.read(&renamed).unwrap().file.note;
    assert_eq!((kept.title.as_str(), kept.strokes.len()), ("Cours", 2));
    let saved = hub.stop().unwrap();
    assert_eq!(saved.len(), 1);
    let new = library.read(Path::new(&saved[0])).unwrap().file.note;
    assert_eq!(Path::new(&saved[0]).parent(), Some(dir.path()));
    assert_eq!(new.strokes.len(), 3);
    assert_ne!(new.id, kept.id);
}

#[test]
fn commands_need_a_session_and_stop_can_be_repeated() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    assert!(matches!(
        hub.command(LiveCommandKind::NextPage),
        Err(LiveError::NotRunning)
    ));
    assert!(matches!(
        hub.set_style(LiveStyle::chalk()),
        Err(LiveError::NotRunning)
    ));
    assert!(matches!(
        hub.set_view(LiveViewState::whole(A5)),
        Err(LiveError::NotRunning)
    ));
    assert!(hub.stop().unwrap().is_empty());

    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    let Start::Running(running) = hub
        .start(options(&library, fixed(LiveButton::NewPage)))
        .unwrap()
    else {
        panic!("a second session started");
    };
    assert_eq!(running.state, LiveSessionState::Live);
    assert!(
        hub.stop().unwrap().is_empty(),
        "nothing drawn, nothing written"
    );
    // Stopped, the session stays shown and its pages can still be turned.
    hub.command(LiveCommandKind::NewPage).unwrap();
    assert_eq!(hub.snapshot().page_count, 2);
    // A new session replaces it; the old one's input is ignored.
    let mut new = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    assert_eq!(hub.snapshot().page_count, 1);
    stroke(&mut sink, 3000, 5);
    assert!(hub.snapshot().page.strokes.is_empty());
    stroke(&mut new, 3000, 5);
    assert_eq!(hub.snapshot().page.strokes.len(), 1);
    assert_eq!(hub.stop().unwrap().len(), 1);
    assert!(notes_in(&library).len() == 1);
}

#[test]
fn the_tablet_button_does_what_the_settings_say() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let setting = Arc::new(AtomicU8::new(0));
    let button: ButtonSetting = {
        let setting = setting.clone();
        Arc::new(move || {
            if setting.load(Ordering::SeqCst) == 1 {
                LiveButton::NewLayer
            } else {
                LiveButton::NewPage
            }
        })
    };
    let mut sink = start(&hub, options(&library, button));
    sink.input(LiveInput::Button);
    assert_eq!(hub.snapshot().page_count, 2);
    // Changed during live mode: read at every press.
    setting.store(1, Ordering::SeqCst);
    sink.input(LiveInput::Button);
    let snapshot = hub.snapshot();
    assert_eq!((snapshot.page_count, snapshot.layer), (2, 1));
    hub.command(LiveCommandKind::Button).unwrap();
    assert_eq!(hub.snapshot().layer, 2);
    assert_eq!(hub.snapshot().page.layers[2].name, "Calque 3");
    hub.stop().unwrap();
}

#[test]
fn the_tablet_states_are_shown() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    assert_eq!(hub.snapshot().state, LiveSessionState::Live);
    assert_eq!(hub.snapshot().page.page, A5);
    send(&mut sink, &[pen(1, 1, 300)]);
    sink.input(LiveInput::State(LiveState::Reconnecting));
    assert_eq!(hub.snapshot().state, LiveSessionState::Reconnecting);
    assert_eq!(hub.snapshot().page.strokes.len(), 1, "the stroke ended");
    sink.input(LiveInput::State(LiveState::Live));
    sink.input(LiveInput::ProximityIn {
        received: SystemTime::now(),
    });
    assert_eq!(hub.snapshot().state, LiveSessionState::Live);
    sink.input(LiveInput::State(LiveState::Stopped));
    assert_eq!(hub.snapshot().state, LiveSessionState::Idle);
    wait_until("the page written", || notes_in(&library).len() == 1);
    hub.stop().unwrap();
}

#[test]
fn the_presentation_window_is_announced_to_every_window() {
    let hub = LiveHub::new(config());
    let main = Arc::new(Window::default());
    hub.subscribe("main", main.clone());
    hub.subscribe("presentation", Arc::new(Window::default()));
    let screen = MonitorInfo::new("HDMI", 1920, 0, 1920, 1080, 1.0, false, None);
    hub.set_presentation(Some(screen.clone()));
    assert_eq!(hub.snapshot().presentation, Some(screen.clone()));
    hub.presentation_closed("presentation");
    assert_eq!(hub.subscriber_count(), 1);
    assert_eq!(hub.snapshot().presentation, None);
    wait_until("both snapshots", || {
        let presentations: Vec<Option<MonitorInfo>> = main
            .events()
            .iter()
            .filter_map(|e| match e {
                LiveEvent::Snapshot { snapshot } => Some(snapshot.presentation.clone()),
                _ => None,
            })
            .collect();
        presentations == [None, Some(screen.clone()), None]
    });
}

fn keys(value: &Value) -> Vec<&str> {
    let mut keys: Vec<&str> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    keys.sort_unstable();
    keys
}

#[test]
fn the_json_has_the_shapes_of_live_ts() {
    let mut session = LiveSession::new(
        1,
        A5,
        LiveStyle::default(),
        LiveViewState::whole(A5),
        false,
        0,
    );
    let mut out = Vec::new();
    session.set_state(LiveSessionState::Waiting, &mut out);
    session.started(A5.width, A5.height, &mut out);
    session.records(&[pen(10, 20, 0), pen(30, 40, 2047)], 1.5e12, &mut out);
    session.records(
        &[pen(50, 60, 1024), pen(70, 80, 0)],
        1.5e12 + 10.0,
        &mut out,
    );
    let mut view = session.view();
    view.zoom = 2.0;
    session.set_view(view, &mut out);
    session.records(&[LiveRecord::ProximityOut], 1.5e12 + 20.0, &mut out);
    out.push(LiveEvent::Saved {
        paths: vec!["C:\\Notes\\2026-10-24.ardoise".into()],
    });
    let json: Vec<Value> = out
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    let types: Vec<&str> = json.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(
        types,
        [
            "snapshot",
            "snapshot",
            "hover",
            "hoverEnd",
            "strokeStart",
            "points",
            "strokeEnd",
            "hover",
            "view",
            "hoverEnd",
            "saved"
        ]
    );

    let snapshot = &json[1]["snapshot"];
    assert_eq!(
        keys(snapshot),
        [
            "hover",
            "layer",
            "page",
            "pageCount",
            "pageIndex",
            "presentation",
            "saved",
            "state",
            "style",
            "view"
        ]
    );
    assert_eq!(snapshot["state"], "live");
    assert_eq!(
        (snapshot["pageIndex"].clone(), snapshot["pageCount"].clone()),
        (json!(0), json!(1))
    );
    assert_eq!(snapshot["layer"], 0);
    assert_eq!(snapshot["hover"], Value::Null);
    assert_eq!(snapshot["presentation"], Value::Null);
    assert_eq!(snapshot["saved"], json!([]));
    assert_eq!(
        snapshot["style"],
        json!({ "tool": "ballpoint", "color": { "r": 0, "g": 0, "b": 0, "a": 255 }, "width": 60.0 })
    );
    assert_eq!(
        snapshot["view"],
        json!({ "zoom": 1.0, "x": 10800.0, "y": 7400.0, "board": false, "laser": false })
    );
    // The page is a `Note` (note.ts).
    let page = &snapshot["page"];
    assert_eq!(
        keys(page),
        [
            "background",
            "created",
            "id",
            "layers",
            "modified",
            "origin",
            "page",
            "strokes",
            "tags",
            "timing",
            "title"
        ]
    );
    assert_eq!(page["origin"], json!({ "kind": "live" }));
    assert_eq!(page["page"], json!({ "width": 21600, "height": 14800 }));
    assert_eq!(page["background"]["paper"], "white");

    assert_eq!(json[2], json!({ "type": "hover", "x": 10.0, "y": 20.0 }));
    assert_eq!(json[3], json!({ "type": "hoverEnd" }));
    let start = &json[4];
    assert_eq!(keys(start), ["pageIndex", "stroke", "type"]);
    assert_eq!(
        keys(&start["stroke"]),
        ["color", "layer", "points", "tool", "width"]
    );
    assert_eq!(
        start["stroke"]["points"][0],
        json!({ "x": 30.0, "y": 40.0, "p": 1.0, "t": 0, "recv": 1.5e12 })
    );
    assert_eq!(keys(&json[5]), ["pageIndex", "points", "type"]);
    assert_eq!(json[5]["points"][0]["t"], 5);
    assert_eq!(json[6], json!({ "type": "strokeEnd", "pageIndex": 0 }));
    assert_eq!(
        json[8],
        json!({ "type": "view", "view": { "zoom": 2.0, "x": 10800.0, "y": 7400.0, "board": false, "laser": false } })
    );
    assert_eq!(
        json[10],
        json!({ "type": "saved", "paths": ["C:\\Notes\\2026-10-24.ardoise"] })
    );
    let batch = serde_json::to_value(LiveBatch {
        events: vec![LiveEvent::HoverEnd],
        sent_at: 1.5e12 + 0.25,
    })
    .unwrap();
    assert_eq!(
        batch,
        json!({ "events": [{ "type": "hoverEnd" }], "sentAt": 1.5e12 + 0.25 })
    );
    for (state, text) in [
        (LiveSessionState::Idle, "idle"),
        (LiveSessionState::Waiting, "waiting"),
        (LiveSessionState::Live, "live"),
        (LiveSessionState::Reconnecting, "reconnecting"),
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), text);
    }
}

#[test]
fn the_interface_values_are_read() {
    let style: LiveStyle = serde_json::from_value(json!({
        "tool": "chalk", "color": { "r": 255, "g": 255, "b": 255, "a": 255 }, "width": 180
    }))
    .unwrap();
    assert_eq!(style, LiveStyle::chalk());
    let view: LiveViewState = serde_json::from_value(json!({
        "zoom": 2, "x": 100, "y": 200.5, "board": true, "laser": false
    }))
    .unwrap();
    assert_eq!(
        (view.zoom, view.x, view.y, view.board),
        (2.0, 100.0, 200.5, true)
    );
    for (text, kind) in [
        ("nextPage", LiveCommandKind::NextPage),
        ("previousPage", LiveCommandKind::PreviousPage),
        ("newPage", LiveCommandKind::NewPage),
        ("newLayer", LiveCommandKind::NewLayer),
        ("clearPage", LiveCommandKind::ClearPage),
        ("button", LiveCommandKind::Button),
    ] {
        let command: LiveCommand = serde_json::from_value(json!({ "kind": text })).unwrap();
        assert_eq!(command.kind, kind);
    }
    assert!(serde_json::from_value::<LiveCommand>(json!({ "kind": "undo" })).is_err());
}

/// Latency of the hub (SPEC §4.2: under 30 ms from a point's receipt to its display): 3 s of
/// simulated handwriting streamed in real time (a notification of up to 3 records every 15 ms,
/// stamped when it is "received"), measured as `sentAt − recv` for every point a window gets.
/// `cargo test -p ardoise hub_latency -- --nocapture` prints the figures.
#[test]
fn hub_latency() {
    let (_dir, library) = library();
    let hub = LiveHub::new(HubConfig::default());
    struct Latency(Mutex<Vec<f64>>);
    impl BatchSink for Latency {
        fn send(&self, batch: LiveBatch) -> Result<(), String> {
            let mut samples = self.0.lock().unwrap();
            for point in points_of(&batch.events) {
                samples.push(batch.sent_at - point.recv);
            }
            Ok(())
        }
    }
    let latency = Arc::new(Latency(Mutex::new(Vec::new())));
    hub.subscribe("main", latency.clone());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));

    let script = ardoise_sim::drawings::live_script(3, A5.width, A5.height);
    let begin = Instant::now();
    let mut due = Duration::ZERO;
    let sample = Duration::from_millis(5);
    for segment in script.segments() {
        match segment {
            ardoise_sim::PenSegment::Near(points) => {
                let records: Vec<LiveRecord> = points
                    .iter()
                    .map(|p| LiveRecord::Pen(*p))
                    .chain([LiveRecord::ProximityOut])
                    .collect();
                for chunk in records.chunks(3) {
                    due += sample * u32::try_from(chunk.len()).unwrap();
                    std::thread::sleep((begin + due).saturating_duration_since(Instant::now()));
                    send(&mut sink, chunk);
                }
            }
            ardoise_sim::PenSegment::Away(pause) => due += *pause,
        }
    }
    std::thread::sleep(Duration::from_millis(50));
    let mut samples = latency.0.lock().unwrap().clone();
    hub.stop().unwrap();

    samples.sort_by(f64::total_cmp);
    let n = samples.len();
    assert!(n > 200, "{n} points");
    let median = samples[n / 2];
    let p95 = samples[(n * 95 / 100).min(n - 1)];
    println!(
        "hub latency (sentAt - recv): {n} points, median {median:.3} ms, p95 {p95:.3} ms, max {:.3} ms",
        samples[n - 1]
    );
    assert!(samples[0] >= 0.0);
    assert!(median < 10.0, "median {median} ms");
    assert!(p95 < 30.0, "95th percentile {p95} ms");
}

// End to end on the Rust side: simulated tablet in real time → tablet service → hub → window.
// It needs work package A (the service's worker and `ardoise_sim::realtime`), which does not
// exist on this branch: the test is kept in this comment, written against the contract, and is
// to be uncommented (and the `#[ignore]` removed) once package A is merged.
//
// /// The whole Rust part of the latency: the simulator streams 10 s of handwriting in real time
// /// through the tablet service (registration included) to the hub; prints the median and the
// /// 95th percentile of `sentAt − recv`.
// #[test]
// #[ignore = "needs the tablet service (work package A)"]
// fn simulator_service_hub_latency() {
//     use ardoise_device::service::{
//         ConnectorFactory, PageStore, ServiceConfig, ServiceHooks, TabletOptions, TabletService,
//     };
//     use ardoise_device::transport::{Connector, TransportError};
//     use ardoise_sim::SimTablet;
//     use ardoise_sim::realtime::RealTimeConnector;
//
//     struct Factory(SimTablet);
//     impl ConnectorFactory for Factory {
//         fn connector(&mut self) -> Result<Box<dyn Connector>, TransportError> {
//             Ok(Box::new(RealTimeConnector::new(self.0.clone())))
//         }
//     }
//     /// The person in front of the simulated tablet.
//     struct Hands(SimTablet);
//     impl ServiceHooks for Hands {
//         fn registration_started(&self) {
//             self.0.hold_button();
//         }
//         fn waiting_for_button(&self) {
//             self.0.press_button();
//         }
//         fn live_started(&self) {
//             self.0.draw_live(ardoise_sim::drawings::live_script(10, 29_700, 21_600));
//         }
//     }
//
//     let (dir, library) = library();
//     let identity = tempfile::tempdir().unwrap();
//     let tablet = SimTablet::slate_large();
//     let status = Arc::new(Mutex::new(None));
//     let on_status = {
//         let status = status.clone();
//         Box::new(move |s: &ardoise_device::service::TabletStatus| {
//             *status.lock().unwrap() = Some(s.clone());
//         })
//     };
//     let store_page: PageStore = Box::new(|_, _, _| Err("no page expected".into()));
//     let service = TabletService::start(
//         ServiceConfig {
//             library: Some(dir.path().to_path_buf()),
//             identity_root: Some(identity.path().to_path_buf()),
//             options: TabletOptions::default(),
//             simulated: true,
//         },
//         Box::new(Factory(tablet.clone())),
//         Arc::new(Hands(tablet.clone())),
//         on_status,
//         store_page,
//     );
//     service.register();
//     wait_until("the registration", || {
//         status.lock().unwrap().as_ref().is_some_and(|s| s.registered.is_some())
//     });
//
//     let hub = LiveHub::new(HubConfig::default());
//     let window = Arc::new(Window::default());
//     hub.subscribe("main", window.clone());
//     let Start::New { sink, .. } = hub.start(options(&library, fixed(LiveButton::NewPage))).unwrap()
//     else {
//         panic!("already running");
//     };
//     service.start_live(Box::new(sink));
//     std::thread::sleep(Duration::from_secs(11));
//     service.stop_live();
//     hub.stop().unwrap();
//     service.shutdown();
//
//     let mut samples: Vec<f64> = window
//         .batches
//         .lock()
//         .unwrap()
//         .iter()
//         .flat_map(|batch| {
//             points_of(&batch.events)
//                 .into_iter()
//                 .map(move |point| batch.sent_at - point.recv)
//         })
//         .collect();
//     samples.sort_by(f64::total_cmp);
//     let n = samples.len();
//     assert!(n > 500, "{n} points");
//     println!(
//         "simulator -> service -> hub latency (sentAt - recv): {n} points, median {:.3} ms, p95 {:.3} ms",
//         samples[n / 2],
//         samples[(n * 95 / 100).min(n - 1)]
//     );
//     assert!(samples[n / 2] < 10.0);
// }
