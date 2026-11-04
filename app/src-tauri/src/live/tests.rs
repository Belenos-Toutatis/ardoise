//! Tests of the live hub: windows and batches, pages written to the library and read back,
//! the JSON shapes of `app/ui/src/lib/live.ts`, and the latency of the Rust side.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ardoise_core::library::Library;
use ardoise_core::note::{Origin, PageSize, Paper};
use ardoise_core::settings::{LiveButton, Orientation};
use ardoise_device::service::{LiveInput, LiveSink, LiveState};
use ardoise_ink::{Rgba, Tool};
use ardoise_media::microphone::simulated_narration;
use ardoise_media::replay::ReplayOptions;
use ardoise_media::{Narration, NarrationBase, decode_narration};
use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::Point as PenPoint;
use serde_json::{Value, json};
use tauri::ipc::{Channel, InvokeResponseBody};

use super::hub::{
    BatchSink, ButtonSetting, HubConfig, HubSink, LiveError, LiveHub, Start, StartOptions,
};
use super::model::{
    LiveBatch, LiveCommand, LiveCommandKind, LiveEvent, LiveMark, LivePoint, LiveSessionState,
    LiveStyle, LiveViewState, MAX_MARK_POINTS, MAX_MARKS, MAX_MARKS_POINTS, MarkPoint,
};
use super::session::LiveSession;
use crate::library::tests::library;
use crate::library::{now_ms, path_text};
use crate::narration::{PageChange, PageLog, give_to_hub};
use crate::presentation::MonitorInfo;

pub(super) const A5: PageSize = PageSize {
    width: 21_600,
    height: 14_800,
};

/// A window: keeps the batches it receives; refuses them once `closed`.
#[derive(Default)]
pub(super) struct Window {
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
    pub(super) fn events(&self) -> Vec<LiveEvent> {
        self.batches
            .lock()
            .unwrap()
            .iter()
            .flat_map(|batch| batch.events.clone())
            .collect()
    }

    pub(super) fn has(&self, wanted: impl Fn(&LiveEvent) -> bool) -> bool {
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

pub(super) fn config() -> HubConfig {
    HubConfig {
        flush_interval: Duration::from_millis(4),
        save_interval: Duration::from_millis(150),
    }
}

pub(super) fn fixed(button: LiveButton) -> ButtonSetting {
    Arc::new(move || button)
}

pub(super) fn options(library: &Library, button: ButtonSetting) -> StartOptions {
    StartOptions {
        library: library.clone(),
        french: true,
        size: A5,
        orientation: Orientation::Landscape,
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
        orientation: Orientation::Landscape,
    });
    sink
}

pub(super) fn pen(x: u16, y: u16, pressure: u16) -> LiveRecord {
    LiveRecord::Pen(PenPoint { x, y, pressure })
}

pub(super) fn send(sink: &mut HubSink, records: &[LiveRecord]) {
    sink.input(LiveInput::Records {
        records: records.to_vec(),
        received: SystemTime::now(),
    });
}

/// `n` samples with pressure, then the pen lifted (hovering), in notifications of 3 records.
pub(super) fn stroke(sink: &mut HubSink, x: u16, n: u16) {
    let mut records: Vec<LiveRecord> = (0..n)
        .map(|i| pen(x + i * 10, 1000 + i * 5, 800 + i))
        .collect();
    records.push(pen(x + n * 10, 1000, 0));
    for chunk in records.chunks(3) {
        send(sink, chunk);
    }
}

pub(super) fn wait_until(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

pub(super) fn notes_in(library: &Library) -> Vec<PathBuf> {
    library.list().notes.into_iter().map(|e| e.path).collect()
}

pub(super) fn stroke_count(library: &Library, path: &Path) -> Option<usize> {
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
fn a_portrait_session_has_portrait_pages_and_switching_turns_the_page_in_progress() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let portrait = PageSize {
        width: A5.height,
        height: A5.width,
    };
    let Start::New { mut sink, snapshot } = hub
        .start(StartOptions {
            orientation: Orientation::PortraitLeft,
            ..options(&library, fixed(LiveButton::NewPage))
        })
        .unwrap()
    else {
        panic!("a session is already running");
    };
    assert_eq!(snapshot.orientation, Orientation::PortraitLeft);
    assert_eq!(snapshot.page.page, portrait);
    sink.input(LiveInput::Started {
        width: A5.width,
        height: A5.height,
        orientation: Orientation::PortraitLeft,
    });
    // The service hands over points that are already page coordinates.
    stroke(&mut sink, 3000, 20);
    wait_until("the note of page 1", || notes_in(&library).len() == 1);
    let path = notes_in(&library).remove(0);
    let note = library.read(&path).unwrap().file.note;
    assert_eq!(note.page, portrait);
    let first = note.strokes[0].points[0];
    assert_eq!((first.x, first.y), (3000.0, 1000.0));

    // The other portrait position: a half turn, page size unchanged, ink turned with it.
    sink.input(LiveInput::Reoriented {
        width: A5.width,
        height: A5.height,
        orientation: Orientation::PortraitRight,
    });
    wait_until("the page rewritten turned", || {
        library
            .read(&path)
            .is_ok_and(|loaded| loaded.file.note.strokes[0].points[0].x != 3000.0)
    });
    let turned = library.read(&path).unwrap().file.note;
    assert_eq!(turned.page, portrait);
    let first = turned.strokes[0].points[0];
    assert_eq!((first.x, first.y), (14_800.0 - 3000.0, 21_600.0 - 1000.0));
    assert_eq!(turned.strokes[0].points.len(), 20);
    // Every window got a snapshot with the new position and the ink.
    assert!(window.has(|e| matches!(
        e,
        LiveEvent::Snapshot { snapshot } if snapshot.orientation == Orientation::PortraitRight
            && snapshot.page.strokes.len() == 1
    )));

    // A quarter turn more: the page becomes landscape, its ink turned a quarter.
    sink.input(LiveInput::Reoriented {
        width: A5.width,
        height: A5.height,
        orientation: Orientation::Landscape,
    });
    wait_until("the page rewritten as landscape", || {
        library
            .read(&path)
            .is_ok_and(|loaded| loaded.file.note.page == A5)
    });
    let landscape = library.read(&path).unwrap().file.note;
    // PortraitRight -> Landscape is one quarter turn clockwise: (x, y) on the 14 800 x 21 600
    // page becomes (21 600 - y, x).
    let first = landscape.strokes[0].points[0];
    assert_eq!((first.x, first.y), (21_600.0 - 20_600.0, 11_800.0));
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

    // Its ink changed elsewhere (edited on another PC): left alone, the page goes on in a new note.
    let mut loaded = library.read(&moved).unwrap();
    let extra = loaded.file.note.strokes[0].clone();
    loaded.file.note.strokes.push(extra);
    library
        .save(&loaded.path, &loaded.file, &loaded.stamp)
        .unwrap();
    stroke(&mut sink, 5000, 5);
    wait_until("a new note", || notes_in(&library).len() == 2);
    let kept = library.read(&moved).unwrap().file.note;
    assert_eq!(kept.strokes.len(), 3);
    let saved = hub.stop().unwrap();
    assert_eq!(saved.len(), 1);
    let new = library.read(Path::new(&saved[0])).unwrap().file.note;
    assert_eq!(Path::new(&saved[0]).parent(), Some(dir.path()));
    assert_eq!(new.strokes.len(), 3);
    assert_ne!(new.id, kept.id);
}

#[test]
fn a_note_retitled_or_tagged_meanwhile_keeps_receiving_the_pages() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    stroke(&mut sink, 3000, 5);
    wait_until("the note", || notes_in(&library).len() == 1);
    let created = notes_in(&library).remove(0);
    let id = library.read(&created).unwrap().file.note.id;

    // Retitled and tagged from the library (the file is renamed and rewritten) during the lesson.
    let renamed = library.retitle(&created, "Cours", None).unwrap().path;
    library
        .set_tags(&renamed, &["4e".to_string()], None)
        .unwrap();
    stroke(&mut sink, 4000, 5);
    wait_until("the renamed note rewritten", || {
        stroke_count(&library, &renamed) == Some(2)
    });
    // The same note: no duplicate, the outside title and tags kept, the page's id unchanged.
    assert_eq!(notes_in(&library), std::slice::from_ref(&renamed));
    let note = library.read(&renamed).unwrap().file.note;
    assert_eq!(
        (note.title.as_str(), note.tags.clone()),
        ("Cours", vec!["4e".to_string()])
    );
    assert_eq!(note.id, id);

    // Tagged again without a rename: the conflict is merged too, and the first changes survive
    // the writes that follow.
    library
        .set_tags(&renamed, &["4e".to_string(), "optique".to_string()], None)
        .unwrap();
    stroke(&mut sink, 5000, 5);
    wait_until("the third stroke", || {
        stroke_count(&library, &renamed) == Some(3)
    });
    stroke(&mut sink, 6000, 5);
    wait_until("the fourth stroke", || {
        stroke_count(&library, &renamed) == Some(4)
    });
    assert_eq!(notes_in(&library), std::slice::from_ref(&renamed));
    let note = library.read(&renamed).unwrap().file.note;
    assert_eq!(note.title, "Cours");
    assert_eq!(note.tags, ["4e", "optique"]);
    assert_eq!(hub.stop().unwrap(), [path_text(&renamed)]);
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
fn a_stopped_session_no_longer_changes_its_notes() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewLayer)));
    stroke(&mut sink, 3000, 5);
    let saved = hub.stop().unwrap();
    assert_eq!(saved.len(), 1);
    let path = Path::new(&saved[0]);
    let written = std::fs::read(path).unwrap();

    // Clearing, a new layer (also through the button) and the blackboard would rewrite it.
    for kind in [
        LiveCommandKind::ClearPage,
        LiveCommandKind::NewLayer,
        LiveCommandKind::Button,
    ] {
        assert!(
            matches!(hub.command(kind), Err(LiveError::Stopped)),
            "{kind:?}"
        );
    }
    let mut view = hub.snapshot().view;
    view.board = true;
    assert!(matches!(hub.set_view(view), Err(LiveError::Stopped)));
    // Paging through, moving the view and the style are still allowed.
    let mut view = hub.snapshot().view;
    view.zoom = 2.0;
    view.laser = true;
    hub.set_view(view).unwrap();
    hub.set_style(LiveStyle::chalk()).unwrap();
    hub.command(LiveCommandKind::NextPage).unwrap();
    hub.command(LiveCommandKind::PreviousPage).unwrap();
    let snapshot = hub.snapshot();
    assert_eq!((snapshot.page_count, snapshot.page_index), (2, 0));
    assert_eq!(snapshot.page.strokes.len(), 1);
    assert_eq!(snapshot.page.layers.len(), 1);
    assert_eq!(snapshot.page.background.paper, Paper::White);
    assert_eq!(hub.stop().unwrap(), saved);
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        std::fs::read(path).unwrap(),
        written,
        "the note is unchanged"
    );

    // The button set to a new page only pages.
    let hub = LiveHub::new(config());
    start(&hub, options(&library, fixed(LiveButton::NewPage)));
    hub.stop().unwrap();
    hub.command(LiveCommandKind::Button).unwrap();
    assert_eq!(hub.snapshot().page_count, 2);
}

/// Makes the folder of `library` unusable (a file takes its place), or usable again.
pub(super) fn break_folder(root: &Path, broken: bool) {
    let away = root.with_extension("away");
    if broken {
        std::fs::rename(root, &away).unwrap();
        std::fs::write(root, b"not a folder").unwrap();
        return;
    }
    std::fs::remove_file(root).unwrap();
    // The live writer keeps retrying in the background and may recreate the folder (and write
    // the page into it) between the two calls: move what it wrote into the real folder, retry.
    for _ in 0..50 {
        match std::fs::rename(&away, root) {
            Ok(()) => return,
            Err(_) if root.is_dir() => {
                for entry in std::fs::read_dir(root).unwrap().flatten() {
                    let _ = std::fs::rename(entry.path(), away.join(entry.file_name()));
                }
                let _ = std::fs::remove_dir(root);
            }
            Err(error) => panic!("cannot put the library folder back: {error}"),
        }
    }
    panic!("the library folder kept being recreated");
}

fn save_failures(window: &Window) -> Vec<usize> {
    window
        .events()
        .iter()
        .filter_map(|e| match e {
            LiveEvent::SaveFailed { page_index, .. } => Some(*page_index),
            _ => None,
        })
        .collect()
}

#[test]
fn a_page_that_cannot_be_written_is_shown_and_backed_up_when_the_app_closes() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Notes");
    std::fs::create_dir(&root).unwrap();
    ardoise_core::library::init(&root).unwrap();
    let library = Library::open(&root).unwrap();
    let hub = LiveHub::new(config());
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));

    // The library cannot be written: the windows are told once, however often it is retried.
    break_folder(&root, true);
    stroke(&mut sink, 3000, 5);
    wait_until("the failure", || !save_failures(&window).is_empty());
    std::thread::sleep(config().save_interval * 4);
    assert_eq!(save_failures(&window), [0]);
    let message = window
        .events()
        .iter()
        .find_map(|e| match e {
            LiveEvent::SaveFailed { message, .. } => Some(message.clone()),
            _ => None,
        })
        .unwrap();
    assert!(!message.is_empty());
    // A window opened meanwhile learns it too.
    let late = Arc::new(Window::default());
    hub.subscribe("presentation", late.clone());
    wait_until("the failure in the late window", || {
        save_failures(&late) == [0]
    });

    // It can be written again: `saved` follows, the failure is over.
    break_folder(&root, false);
    wait_until("the note", || notes_in(&library).len() == 1);
    wait_until("the saved event", || {
        window.has(|e| matches!(e, LiveEvent::Saved { paths } if paths.len() == 1))
    });

    // It fails again: told again (once); a page written meanwhile does not hide it.
    break_folder(&root, true);
    stroke(&mut sink, 4000, 5);
    wait_until("the second failure", || save_failures(&window) == [0, 0]);
    std::thread::sleep(config().save_interval * 3);
    assert_eq!(save_failures(&window), [0, 0]);

    // The app closes: the page goes to the backup folder, complete, and the path is logged.
    let backup = dir.path().join("live-backup");
    let copies = super::close_session(&hub, &backup);
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].parent(), Some(backup.as_path()));
    let copy = ardoise_core::format::read_note(&copies[0]).unwrap().note;
    assert_eq!(copy.strokes.len(), 2);
    assert_eq!(copy.origin, Origin::Live);
    assert_ne!(copy.id, hub.snapshot().page.id, "a copy, with its own id");
    let log = std::fs::read_to_string(backup.join("live-backup.log")).unwrap();
    assert!(log.contains(&copies[0].display().to_string()), "{log}");
    assert_eq!(
        std::fs::read_dir(&backup).unwrap().count(),
        2,
        "the copy and the log: the atomic write leaves nothing behind"
    );

    // Nothing left to save: nothing backed up.
    break_folder(&root, false);
    assert_eq!(hub.stop().unwrap().len(), 1);
    assert!(super::close_session(&hub, &backup).is_empty());
    assert!(super::backup_dir().ends_with(Path::new("Ardoise").join("live-backup")));
}

/// Background work reads whether live mode runs without the hub's lock: the flag follows every
/// way a session starts and ends, failures included.
#[test]
fn the_activity_follows_every_start_and_end_of_live_mode() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Notes");
    std::fs::create_dir(&root).unwrap();
    ardoise_core::library::init(&root).unwrap();
    let library = Library::open(&root).unwrap();
    let hub = LiveHub::new(config());
    let activity = hub.activity();
    assert!(!activity.busy(), "not before the first start");

    // Started, streaming, then the tablet's service leaves live mode by itself (tablet gone).
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    assert!(activity.live() && activity.busy());
    sink.input(LiveInput::State(LiveState::Reconnecting));
    assert!(activity.live(), "still live while reconnecting");
    sink.input(LiveInput::State(LiveState::Stopped));
    assert!(!activity.live());
    assert!(!hub.page_position().unwrap().running);

    // Started again and stopped by the user.
    start(&hub, options(&library, fixed(LiveButton::NewPage)));
    assert!(activity.live());
    hub.stop().unwrap();
    assert!(!activity.live());

    // The service could not start the session.
    let Start::New { sink, .. } = hub
        .start(options(&library, fixed(LiveButton::NewPage)))
        .unwrap()
    else {
        panic!("already running");
    };
    assert!(activity.live());
    hub.abort_start(sink.session());
    assert!(!activity.live());

    // Stopped while the pages cannot be written: live mode has ended all the same.
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    stroke(&mut sink, 3000, 5);
    break_folder(&root, true);
    let _ = hub.stop();
    assert!(!activity.live());
    break_folder(&root, false);

    // The app closes while live mode runs.
    start(&hub, options(&library, fixed(LiveButton::NewPage)));
    let backup = tempfile::tempdir().unwrap();
    super::close_session(&hub, backup.path());
    assert!(!activity.live());

    // The presentation window has its own part; the session's view of it does not count.
    hub.set_presentation(Some(MonitorInfo::new(
        "HDMI", 1920, 0, 1920, 1080, 1.0, false, None,
    )));
    assert!(!activity.busy());
    let window = activity.presentation_opened();
    assert!(activity.busy() && !activity.live());
    activity.presentation_closed(window);
    assert!(!activity.busy());
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

/// A red mark of `n` points starting at `x`.
fn mark(x: f32, n: usize) -> LiveMark {
    LiveMark {
        tool: Tool::FeltTip,
        color: Rgba {
            r: 220,
            g: 38,
            b: 38,
            a: 255,
        },
        width: 140.0,
        points: (0..n)
            .map(|i| MarkPoint {
                x: x + i as f32 * 10.0,
                y: 500.0,
                p: 0.5,
            })
            .collect(),
    }
}

fn marks_of(events: &[LiveEvent]) -> Vec<Vec<LiveMark>> {
    events
        .iter()
        .filter_map(|event| match event {
            LiveEvent::Marks { marks } => Some(marks.clone()),
            _ => None,
        })
        .collect()
}

fn marks_session() -> LiveSession {
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
    session.started(A5.width, A5.height, Orientation::Landscape, &mut out);
    session
}

#[test]
fn the_marks_of_a_page_are_kept_until_the_page_changes() {
    let mut session = marks_session();
    let mut out = Vec::new();
    assert_eq!(session.marks_event(), None);

    session.set_marks(0, vec![mark(100.0, 5)], &mut out);
    assert_eq!(marks_of(&out), [vec![mark(100.0, 5)]]);
    assert_eq!(
        session.marks_event(),
        Some(LiveEvent::Marks {
            marks: vec![mark(100.0, 5)]
        })
    );

    // The same list again says nothing; a list for another page is not the page's.
    out.clear();
    session.set_marks(0, vec![mark(100.0, 5)], &mut out);
    session.set_marks(3, vec![mark(7.0, 2)], &mut out);
    assert!(out.is_empty());
    assert!(session.marks_event().is_some());

    // A change of style does not touch them, nor does clearing the page (the presentation
    // window keeps its marks then too).
    session.set_style(LiveStyle::chalk(), &mut out);
    session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
    assert!(marks_of(&out).is_empty());
    assert!(session.marks_event().is_some());

    // The page changes: the snapshot is followed by an empty list, and nothing is kept.
    out.clear();
    session.command(LiveCommandKind::NextPage, LiveButton::NewPage, 0, &mut out);
    assert!(matches!(out[0], LiveEvent::Snapshot { .. }));
    assert_eq!(marks_of(&out), [Vec::<LiveMark>::new()]);
    assert_eq!(session.marks_event(), None);

    // On the new page: again, then back to the first page.
    out.clear();
    session.set_marks(1, vec![mark(1.0, 3), mark(50.0, 4)], &mut out);
    session.command(
        LiveCommandKind::PreviousPage,
        LiveButton::NewPage,
        0,
        &mut out,
    );
    assert_eq!(
        marks_of(&out),
        [vec![mark(1.0, 3), mark(50.0, 4)], Vec::new()]
    );

    // The window clearing its marks (an empty list) is a change only when there are some.
    out.clear();
    session.set_marks(0, vec![mark(1.0, 1)], &mut out);
    session.set_marks(0, Vec::new(), &mut out);
    session.set_marks(0, Vec::new(), &mut out);
    assert_eq!(marks_of(&out), [vec![mark(1.0, 1)], Vec::new()]);
}

#[test]
fn closing_the_presentation_window_removes_its_marks() {
    let mut session = marks_session();
    let mut out = Vec::new();
    // Marks without a presentation window announced (the control window or a test sent them):
    // they stay.
    session.set_marks(0, vec![mark(1.0, 2)], &mut out);
    session.set_presentation(None, &mut out);
    assert!(session.marks_event().is_some());

    let screen = MonitorInfo::new("HDMI", 1920, 0, 1920, 1080, 1.0, false, None);
    session.set_presentation(Some(screen), &mut out);
    out.clear();
    session.set_presentation(None, &mut out);
    assert!(matches!(out[0], LiveEvent::Snapshot { .. }));
    assert_eq!(marks_of(&out), [Vec::<LiveMark>::new()]);
    assert_eq!(session.marks_event(), None);
}

#[test]
fn turning_the_page_over_removes_the_marks() {
    let mut session = marks_session();
    let mut out = Vec::new();
    session.set_marks(0, vec![mark(100.0, 5)], &mut out);
    out.clear();
    // The quick switch of the orientation keeps the page index but turns the page.
    session.reorient(A5.height, A5.width, Orientation::PortraitLeft, &mut out);
    assert!(matches!(out[0], LiveEvent::Snapshot { .. }));
    assert_eq!(marks_of(&out), [Vec::<LiveMark>::new()]);
}

#[test]
fn the_marks_are_bounded_and_cleaned() {
    let mut session = marks_session();
    let mut out = Vec::new();

    // Too many marks: the first ones stay.
    let many: Vec<LiveMark> = (0..MAX_MARKS + 50).map(|i| mark(i as f32, 2)).collect();
    session.set_marks(0, many, &mut out);
    let kept = &marks_of(&out)[0];
    assert_eq!(kept.len(), MAX_MARKS);
    assert_eq!(kept[MAX_MARKS - 1].points[0].x, (MAX_MARKS - 1) as f32);

    // Too many points in one mark, and in all of them.
    out.clear();
    session.set_marks(0, vec![mark(0.0, MAX_MARK_POINTS + 10)], &mut out);
    assert_eq!(marks_of(&out)[0][0].points.len(), MAX_MARK_POINTS);
    out.clear();
    let wide: Vec<LiveMark> = (0..10).map(|i| mark(i as f32, MAX_MARK_POINTS)).collect();
    session.set_marks(0, wide, &mut out);
    let total: usize = marks_of(&out)[0].iter().map(|m| m.points.len()).sum();
    assert_eq!(total, MAX_MARKS_POINTS);

    // Values a tool cannot draw.
    out.clear();
    let mut odd = mark(10.0, 3);
    odd.points[1].x = f32::NAN;
    odd.points[2] = MarkPoint {
        x: 1.0e9,
        y: -1.0e9,
        p: 7.0,
    };
    odd.points.push(MarkPoint {
        x: 20.0,
        y: 20.0,
        p: f32::NAN,
    });
    let mut infinite = mark(0.0, 3);
    infinite.width = f32::INFINITY;
    let mut thin = mark(0.0, 3);
    thin.width = 0.0;
    let mut huge = mark(5.0, 2);
    huge.width = 1.0e7;
    let empty = mark(0.0, 0);
    session.set_marks(0, vec![odd, infinite, thin, empty, huge], &mut out);
    let cleaned = &marks_of(&out)[0];
    assert_eq!(cleaned.len(), 2);
    // The point without coordinates is gone; the others are brought back within bounds.
    let points = &cleaned[0].points;
    assert_eq!(points.len(), 3);
    assert_eq!(
        points[1],
        MarkPoint {
            x: 2.0 * A5.width as f32,
            y: -(A5.height as f32),
            p: 1.0
        }
    );
    assert_eq!(points[2].p, 1.0);
    assert_eq!(cleaned[1].width, 5000.0);
}

#[test]
fn the_marks_json_is_what_the_viewer_draws() {
    let event = LiveEvent::Marks {
        marks: vec![mark(100.0, 2)],
    };
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        json!({
            "type": "marks",
            "marks": [{
                "tool": "feltTip",
                "color": { "r": 220, "g": 38, "b": 38, "a": 255 },
                "width": 140.0,
                "points": [{ "x": 100.0, "y": 500.0, "p": 0.5 }, { "x": 110.0, "y": 500.0, "p": 0.5 }]
            }]
        })
    );
    // What the window sends is read as it is written.
    let read: Vec<LiveMark> = serde_json::from_value(json!([{
        "tool": "feltTip", "color": { "r": 220, "g": 38, "b": 38, "a": 255 }, "width": 140,
        "points": [{ "x": 100, "y": 500, "p": 0.5 }, { "x": 110, "y": 500, "p": 0.5 }]
    }]))
    .unwrap();
    assert_eq!(read, vec![mark(100.0, 2)]);
    let unknown = json!([{ "tool": "laser", "color": {}, "width": 1, "points": [] }]);
    assert!(serde_json::from_value::<Vec<LiveMark>>(unknown).is_err());
}

#[test]
fn the_marks_reach_the_windows_coalesced_and_late_ones_start_from_them() {
    let (_dir, library) = library();
    // A long flush interval: the changes made meanwhile wait in the queue and are merged.
    let hub = LiveHub::new(HubConfig {
        flush_interval: Duration::from_millis(400),
        ..config()
    });
    assert!(matches!(
        hub.set_marks(0, vec![mark(1.0, 1)]),
        Err(LiveError::NotRunning)
    ));
    let window = Arc::new(Window::default());
    hub.subscribe("presentation", window.clone());
    let _sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    wait_until("the first batch", || !window.events().is_empty());

    for n in 1..=30 {
        hub.set_marks(0, vec![mark(1.0, n)]).unwrap();
    }
    wait_until("the marks", || {
        marks_of(&window.events())
            .last()
            .map(|marks| marks[0].points.len())
            == Some(30)
    });
    let seen = marks_of(&window.events());
    assert!(seen.len() <= 3, "{} lists for 30 changes", seen.len());

    // A viewer arriving later (and the frozen image of a paused share) start from them.
    let late = Arc::new(Window::default());
    hub.subscribe("share:1", late.clone());
    wait_until("the late window's first batch", || {
        !late.events().is_empty()
    });
    let events = late.events();
    assert!(matches!(events[0], LiveEvent::Snapshot { .. }));
    assert_eq!(marks_of(&events), [vec![mark(1.0, 30)]]);
    assert_eq!(marks_of(&hub.current_events()), [vec![mark(1.0, 30)]]);

    // The page changes: both windows are told, and the image taken after that has no marks.
    hub.command(LiveCommandKind::NextPage).unwrap();
    wait_until("the empty lists", || {
        marks_of(&window.events()).last() == Some(&Vec::new())
            && marks_of(&late.events()).last() == Some(&Vec::new())
    });
    assert!(marks_of(&hub.current_events()).is_empty());
    // A list written for the page just left is refused.
    hub.set_marks(0, vec![mark(1.0, 4)]).unwrap();
    assert!(marks_of(&hub.current_events()).is_empty());

    // Marks stay allowed once the session is stopped (the presentation stays up); a new
    // session starts without them.
    hub.set_marks(1, vec![mark(2.0, 4)]).unwrap();
    hub.stop().unwrap();
    hub.set_marks(1, vec![mark(2.0, 5)]).unwrap();
    assert_eq!(marks_of(&hub.current_events()), [vec![mark(2.0, 5)]]);
    let _again = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    assert!(marks_of(&hub.current_events()).is_empty());
    wait_until("the empty list of the new session", || {
        marks_of(&late.events()).last() == Some(&Vec::new())
    });
}

pub(super) fn keys(value: &Value) -> Vec<&str> {
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
    session.started(A5.width, A5.height, Orientation::Landscape, &mut out);
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
    out.push(LiveEvent::SaveFailed {
        page_index: 1,
        code: "libraryAccess".into(),
        message: "disk full".into(),
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
            "saved",
            "saveFailed"
        ]
    );

    let snapshot = &json[1]["snapshot"];
    assert_eq!(
        keys(snapshot),
        [
            "canTurnPage",
            "hover",
            "layer",
            "orientation",
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
    assert_eq!(snapshot["canTurnPage"], true);
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
    assert_eq!(
        json[11],
        json!({ "type": "saveFailed", "pageIndex": 1, "code": "libraryAccess", "message": "disk full" })
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
/// Registers a simulated tablet, then streams `seconds` of handwriting in real time through the
/// tablet service into `hub` (live mode started and stopped as the app does). `while_live` runs
/// on the calling thread once the stream has started; the stream stops one second after its
/// end. Returns `sentAt − recv` of every point the window got, sorted (milliseconds).
pub(crate) fn stream_simulated_live(
    hub: &LiveHub,
    library: &Library,
    seconds: u32,
    while_live: impl FnOnce(),
) -> Vec<f64> {
    let start = || {
        hub.start(options(library, fixed(LiveButton::NewPage)))
            .unwrap()
    };
    stream_simulated_start(hub, library, seconds, while_live, start)
}

/// [`stream_simulated_live`], for the session `start` makes.
pub(super) fn stream_simulated_start(
    hub: &LiveHub,
    library: &Library,
    seconds: u32,
    while_live: impl FnOnce(),
    start: impl FnOnce() -> Start,
) -> Vec<f64> {
    use ardoise_device::service::{
        ConnectorFactory, PageStore, ServiceConfig, ServiceHooks, TabletOptions, TabletService,
    };
    use ardoise_device::transport::{Connector, TransportError};
    use ardoise_sim::SimTablet;
    use ardoise_sim::realtime::RealTimeConnector;

    struct Factory(SimTablet);
    impl ConnectorFactory for Factory {
        fn connector(&mut self) -> Result<Box<dyn Connector>, TransportError> {
            Ok(Box::new(RealTimeConnector::new(self.0.clone())))
        }
    }
    /// The person in front of the simulated tablet.
    struct Hands(SimTablet, u32);
    impl ServiceHooks for Hands {
        fn registration_started(&self) {
            self.0.hold_button();
        }
        fn waiting_for_button(&self) {
            self.0.press_button();
        }
        fn live_started(&self) {
            self.0
                .draw_live(ardoise_sim::drawings::live_script(self.1, 29_700, 21_600));
        }
    }

    let identity = tempfile::tempdir().unwrap();
    let tablet = SimTablet::slate_large();
    let status = Arc::new(Mutex::new(None));
    let on_status = {
        let status = status.clone();
        Box::new(move |s: &ardoise_device::service::TabletStatus| {
            *status.lock().unwrap() = Some(s.clone());
        })
    };
    let store_page: PageStore = Box::new(|_, _, _| Err("no page expected".into()));
    let service = TabletService::start(
        ServiceConfig {
            library: Some(library.root().to_path_buf()),
            identity_root: Some(identity.path().to_path_buf()),
            options: TabletOptions::default(),
            simulated: true,
        },
        Box::new(Factory(tablet.clone())),
        Arc::new(Hands(tablet.clone(), seconds)),
        on_status,
        store_page,
    );
    service.register();
    wait_until("the registration", || {
        status
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|s| s.registered.is_some())
    });

    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let Start::New { sink, .. } = start() else {
        panic!("already running");
    };
    service.start_live(Box::new(sink));
    let begin = Instant::now();
    while_live();
    std::thread::sleep(
        (begin + Duration::from_secs(u64::from(seconds) + 1))
            .saturating_duration_since(Instant::now()),
    );
    service.stop_live();
    hub.stop().unwrap();
    service.shutdown();
    hub.unsubscribe("main");

    let mut samples: Vec<f64> = window
        .batches
        .lock()
        .unwrap()
        .iter()
        .flat_map(|batch| {
            points_of(&batch.events)
                .into_iter()
                .map(move |point| batch.sent_at - point.recv)
        })
        .collect();
    samples.sort_by(f64::total_cmp);
    samples
}

/// Median, 95th percentile and maximum of sorted `samples`.
pub(crate) fn figures(samples: &[f64]) -> (f64, f64, f64) {
    let n = samples.len();
    assert!(n > 0, "no samples");
    (
        samples[n / 2],
        samples[(n * 95 / 100).min(n - 1)],
        samples[n - 1],
    )
}

/// The whole Rust part of the latency: the simulator streams 10 s of handwriting in real time
/// through the tablet service (registration included) to the hub; prints the median and the
/// 95th percentile of `sentAt − recv`.
#[test]
fn simulator_service_hub_latency() {
    let (_dir, library) = library();
    let hub = LiveHub::new(HubConfig::default());
    let samples = stream_simulated_live(&hub, &library, 10, || {});
    let n = samples.len();
    assert!(n > 500, "{n} points");
    let (median, p95, _) = figures(&samples);
    println!(
        "simulator -> service -> hub latency (sentAt - recv): {n} points, median {median:.3} ms, p95 {p95:.3} ms",
    );
    assert!(median < 10.0);
}

// --- Narration recorded in live mode ------------------------------------------------------------

pub(super) fn note_narration(seconds: f64, start_ms: i64) -> Narration {
    Narration {
        clip: simulated_narration(Duration::from_secs_f64(seconds), 1),
        base: NarrationBase::Note,
        options: ReplayOptions::default(),
        start_ms,
    }
}

pub(super) fn audio_of(library: &Library, path: &Path) -> Option<Vec<u8>> {
    library.read(path).ok().and_then(|loaded| loaded.file.audio)
}

pub(super) fn change(at: Duration, page: usize) -> PageChange {
    PageChange { at, page }
}

#[test]
fn the_position_and_time_origins_of_the_pages_are_known() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    assert_eq!(hub.page_position(), None, "never started");
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    let first = hub.page_position().unwrap();
    assert!(first.running);
    assert_eq!(first.page, 0);
    // No stroke yet: no time origin.
    assert_eq!(hub.page_origins(first.session), Some(vec![None]));
    let before = now_ms();
    stroke(&mut sink, 3000, 5);
    let origin = hub.page_origins(first.session).unwrap()[0].unwrap();
    // The first sample of a notification is timed 5 ms per record before its receipt.
    assert!((before - 20..=now_ms()).contains(&origin), "{origin}");
    assert_eq!(origin, hub.snapshot().page.created);
    hub.command(LiveCommandKind::NextPage).unwrap();
    assert_eq!(
        hub.page_origins(first.session),
        Some(vec![Some(origin), None])
    );
    assert_eq!(hub.page_position().unwrap().page, 1);
    assert_eq!(hub.page_origins(first.session + 1), None);
    hub.stop().unwrap();
    let stopped = hub.page_position().unwrap();
    assert!(!stopped.running);
    assert_eq!(stopped.session, first.session);
}

#[test]
fn the_narration_of_a_page_is_written_with_it_and_kept_on_later_writes() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    stroke(&mut sink, 3000, 5);
    wait_until("the note", || notes_in(&library).len() == 1);
    let path = notes_in(&library).remove(0);
    assert_eq!(audio_of(&library, &path), None);

    // Ready-made bytes: what the hub writes, not what the encoder makes.
    let audio = b"OggS narration of page 1".to_vec();
    assert!(hub.set_page_audio(0, audio.clone()));
    wait_until("the audio member", || {
        audio_of(&library, &path).as_ref() == Some(&audio)
    });
    // Later writes of the page keep it.
    stroke(&mut sink, 4000, 5);
    wait_until("page 1 rewritten", || {
        stroke_count(&library, &path) == Some(2)
    });
    assert_eq!(audio_of(&library, &path), Some(audio.clone()));
    // A page without any stroke gets nothing.
    hub.command(LiveCommandKind::NextPage).unwrap();
    assert!(!hub.set_page_audio(1, b"x".to_vec()));
    let saved = hub.stop().unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(audio_of(&library, &path), Some(audio.clone()));

    // A page that cannot be written keeps its narration in the backup copy.
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Notes");
    std::fs::create_dir(&root).unwrap();
    ardoise_core::library::init(&root).unwrap();
    let broken = Library::open(&root).unwrap();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&broken, fixed(LiveButton::NewPage)));
    break_folder(&root, true);
    stroke(&mut sink, 3000, 5);
    assert!(hub.set_page_audio(0, audio.clone()));
    let copies = hub.back_up_unsaved(&dir.path().join("backup"));
    assert_eq!(copies.len(), 1);
    let copy = copies[0].1.as_ref().unwrap();
    assert_eq!(
        ardoise_core::format::read_note(copy).unwrap().audio,
        Some(audio.clone())
    );
    // Written once the library is back, with its narration.
    break_folder(&root, false);
    let saved = hub.stop().unwrap();
    assert_eq!(audio_of(&broken, Path::new(&saved[0])), Some(audio));
}

// --- With the media pipeline (Opus encoding, merging) ---

#[test]
fn live_narration_goes_to_its_pages_through_the_hub() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    let session = hub.page_position().unwrap().session;
    stroke(&mut sink, 3000, 5);
    hub.command(LiveCommandKind::NextPage).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    stroke(&mut sink, 3000, 5);
    hub.command(LiveCommandKind::NextPage).unwrap();
    let origins = hub.page_origins(session).unwrap();
    let (first, second) = (origins[0].unwrap(), origins[1].unwrap());

    // 3 s recorded from 1 s before page 1's first stroke: page 1, page 2, page 3 (no stroke).
    let clip = simulated_narration(Duration::from_secs(3), 1);
    let log = PageLog {
        changes: vec![
            change(Duration::ZERO, 0),
            change(Duration::from_secs(1), 1),
            change(Duration::from_secs(2), 2),
        ],
        anchor_ms: None,
    };
    let wall_start = (first - 1000) as f64;
    let pages = give_to_hub(&hub, session, &log, wall_start, clip).unwrap();
    assert_eq!(pages, 2);
    let saved = hub.stop().unwrap();
    assert_eq!(saved.len(), 2);
    let narrations: Vec<Narration> = saved
        .iter()
        .map(|path| decode_narration(&audio_of(&library, Path::new(path)).unwrap()).unwrap())
        .collect();
    assert_eq!(narrations[0].base, NarrationBase::Note);
    assert_eq!(narrations[0].start_ms, -1000);
    assert_eq!(narrations[1].start_ms, first - second);
    for narration in &narrations {
        let ms = narration.duration().as_millis();
        assert!((990..=1010).contains(&ms), "{ms}");
    }

    // A second recording on page 1 is merged with the first one (also after the stop).
    assert!(
        hub.attach_narration(session, 0, note_narration(1.0, 5_000))
            .unwrap()
    );
    let saved = hub.stop().unwrap();
    let merged = decode_narration(&audio_of(&library, Path::new(&saved[0])).unwrap()).unwrap();
    assert_eq!(merged.start_ms, -1000);
    let ms = merged.duration().as_millis();
    assert!((6990..=7010).contains(&ms), "{ms}");
    // Another session, or a page without any stroke: nothing attached.
    assert!(
        !hub.attach_narration(session + 1, 0, note_narration(1.0, 0))
            .unwrap()
    );
    assert!(
        !hub.attach_narration(session, 2, note_narration(1.0, 0))
            .unwrap()
    );

    // The page keeps only the encoded narration, decoded again to merge: a page whose
    // narration cannot be read again keeps it as it is.
    let damaged = b"OggS damaged".to_vec();
    assert!(hub.set_page_audio(1, damaged.clone()));
    assert!(matches!(
        hub.attach_narration(session, 1, note_narration(1.0, 0)),
        Err(LiveError::Narration(_))
    ));
    let saved = hub.stop().unwrap();
    assert_eq!(audio_of(&library, Path::new(&saved[1])), Some(damaged));
}
