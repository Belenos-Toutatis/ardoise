//! Tests of the share in the app: the viewer's files from the build manifest, what a viewer
//! receives of live mode (stripped), the editor's note (whole or appended, paper rendered once),
//! and a share run over real TCP with a real live hub.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime};

use ardoise_core::note::{
    A4_LANDSCAPE, Background, BackgroundImage, ImageRect, Layer, Note, Origin, PageSize, Paper,
    Template, Timing,
};
use ardoise_core::settings::{LiveButton, Orientation};
use ardoise_device::service::{LiveInput, LiveSink};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::Point as PenPoint;
use serde_json::{Value, json};

use super::assets::{AssetSource, ViewerAssetsError, content_type, viewer_assets};
use super::feed::{Memo, batch_json, subscriber_label};
use super::page::{NoteSharer, PAPER_PX, Paper as SharedPaper};
use super::{ShareContent, ShareRuntime, render_paper};
use crate::library::tests::library;
use crate::live::LiveHub;
use crate::live::hub::{HubConfig, HubSink, Start, StartOptions};
use crate::live::model::{
    LiveBatch, LiveEvent, LiveMark, LiveSnapshot, LiveStyle, LiveViewState, MarkPoint,
};
use crate::presentation::MonitorInfo;

// --- The viewer's files ----------------------------------------------------------------------

const MANIFEST: &str = r#"{
  "index.html": { "file": "assets/index-A1.js", "src": "index.html", "isEntry": true,
                  "imports": ["_ink-C3.js"], "css": ["assets/index-B2.css"] },
  "viewer.html": { "file": "assets/viewer-D4.js", "src": "viewer.html", "isEntry": true,
                   "imports": ["_ink-C3.js", "_render-E5.js"], "css": ["assets/viewer-F6.css"] },
  "_ink-C3.js": { "file": "assets/ink-C3.js", "assets": ["assets/ardoise_ink-G7.wasm"] },
  "_render-E5.js": { "file": "assets/render-E5.js", "imports": ["_ink-C3.js"] }
}"#;

/// The built interface of a test: the files of both pages and the manifest.
fn built(manifest: &str) -> HashMap<String, Vec<u8>> {
    let mut files: HashMap<String, Vec<u8>> = [
        "index.html",
        "viewer.html",
        "assets/index-A1.js",
        "assets/index-B2.css",
        "assets/viewer-D4.js",
        "assets/viewer-F6.css",
        "assets/ink-C3.js",
        "assets/render-E5.js",
        "assets/ardoise_ink-G7.wasm",
    ]
    .into_iter()
    .map(|path| (path.to_owned(), format!("<{path}>").into_bytes()))
    .collect();
    files.insert(".vite/manifest.json".into(), manifest.as_bytes().to_vec());
    files
}

fn source(files: HashMap<String, Vec<u8>>) -> AssetSource {
    Arc::new(move |path: &str| files.get(path).cloned())
}

#[test]
fn the_viewer_gets_its_page_and_only_the_files_the_manifest_lists_for_it() {
    let files = built(MANIFEST);
    let assets = viewer_assets(&|path: &str| files.get(path).cloned()).unwrap();
    let mut keys: Vec<&str> = assets.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "/",
            "/assets/ardoise_ink-G7.wasm",
            "/assets/ink-C3.js",
            "/assets/render-E5.js",
            "/assets/viewer-D4.js",
            "/assets/viewer-F6.css",
        ]
    );
    assert_eq!(&*assets["/"].bytes, b"<viewer.html>");
    assert_eq!(assets["/"].content_type, "text/html; charset=utf-8");
    assert_eq!(
        assets["/assets/ardoise_ink-G7.wasm"].content_type,
        "application/wasm"
    );
    assert_eq!(
        assets["/assets/viewer-D4.js"].content_type,
        "text/javascript; charset=utf-8"
    );
    // The main window's page and scripts are not the viewer's.
    assert!(!assets.contains_key("/assets/index-A1.js"));
    assert!(!assets.contains_key("/index.html"));
}

#[test]
fn a_build_without_the_viewer_or_with_odd_names_is_refused() {
    // A development build: nothing embedded.
    let none = viewer_assets(&|_: &str| None);
    assert!(matches!(none, Err(ViewerAssetsError::Missing(_))));
    // No manifest.
    let mut files = built(MANIFEST);
    files.remove(".vite/manifest.json");
    let missing = viewer_assets(&|path: &str| files.get(path).cloned());
    assert!(matches!(missing, Err(ViewerAssetsError::Missing(_))));
    // A listed file that is not there.
    let mut files = built(MANIFEST);
    files.remove("assets/ink-C3.js");
    let missing = viewer_assets(&|path: &str| files.get(path).cloned());
    assert!(matches!(missing, Err(ViewerAssetsError::Missing(_))));
    // Names outside `assets/` or with path tricks are never read.
    for odd in [
        "../ardoise.toml",
        "assets/../x.js",
        "/assets/x.js",
        "assets/.hidden",
        "assets/a\\b.js",
        "index.html",
    ] {
        let manifest = format!(
            r#"{{ "viewer.html": {{ "file": "assets/viewer-D4.js",
                    "assets": ["assets/ardoise_ink-G7.wasm", {}] }} }}"#,
            serde_json::to_string(odd).unwrap()
        );
        let mut files = built(&manifest);
        files.insert(odd.to_owned(), b"secret".to_vec());
        let refused = viewer_assets(&|path: &str| files.get(path).cloned());
        assert!(
            matches!(refused, Err(ViewerAssetsError::Manifest(_))),
            "{odd}"
        );
    }
    // A viewer without its WebAssembly module would draw no stroke.
    let no_wasm = built(r#"{ "viewer.html": { "file": "assets/viewer-D4.js" } }"#);
    assert!(matches!(
        viewer_assets(&|path: &str| no_wasm.get(path).cloned()),
        Err(ViewerAssetsError::Manifest(_))
    ));
    let broken = built("{ not json");
    assert!(matches!(
        viewer_assets(&|path: &str| broken.get(path).cloned()),
        Err(ViewerAssetsError::Manifest(_))
    ));
}

#[test]
fn the_real_build_of_the_interface_gives_the_viewer_its_files() {
    // `npm run build` (run by `cargo xtask check` before the Rust tests) writes `app/dist`.
    let dist = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist");
    if !dist.join(".vite/manifest.json").exists() {
        println!("no built interface in {}: skipped", dist.display());
        return;
    }
    let assets = viewer_assets(&|path: &str| std::fs::read(dist.join(path)).ok()).unwrap();
    let page = String::from_utf8_lossy(&assets["/"].bytes).into_owned();
    // Every script and style sheet the page references is served.
    for reference in page.split('"').filter(|part| part.starts_with("/assets/")) {
        assert!(assets.contains_key(reference), "{reference} not served");
    }
    assert!(
        assets
            .iter()
            .any(|(path, asset)| path.ends_with(".wasm") && asset.bytes.starts_with(b"\0asm"))
    );
    // Nothing of the main window's page.
    let main = std::fs::read_to_string(dist.join("index.html")).unwrap();
    for reference in main
        .split('"')
        .filter(|part| part.starts_with("/assets/index-"))
    {
        assert!(!assets.contains_key(reference), "{reference} served");
    }
}

#[test]
fn a_port_in_use_is_told_with_its_number() {
    let error = super::share_error(&ardoise_share::ShareError::PortInUse { port: 8765 });
    assert_eq!(error.parts(), ("sharePortInUse", "8765"));
}

#[test]
fn no_free_port_is_told_with_the_range() {
    let error = super::share_error(&ardoise_share::ShareError::NoFreePort {
        first: 8765,
        last: 8785,
    });
    assert_eq!(error.parts(), ("shareNoFreePort", "8765-8785"));
}

/// A plain listener on a port of the loopback interface chosen by the system, as another
/// program has it.
fn other_program() -> (TcpListener, u16) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

fn sharing_runtime() -> (ShareRuntime, Arc<LiveHub>) {
    let runtime = ShareRuntime::default();
    runtime.set_assets(source(built(MANIFEST)));
    (runtime, Arc::new(LiveHub::new(hub_config())))
}

#[test]
fn a_port_held_by_another_program_moves_the_share_to_the_next_one() {
    let (runtime, hub) = sharing_runtime();
    let (_other, taken) = other_program();
    let status = runtime
        .start(Ipv4Addr::LOCALHOST, taken, hub, None)
        .unwrap();
    assert!(status.active);
    assert_eq!(status.requested_port, taken);
    assert!(
        status.port > taken && u32::from(status.port) <= u32::from(taken) + 20,
        "{} for {taken}",
        status.port
    );
    assert_eq!(
        status.addresses,
        [format!("http://127.0.0.1:{}/", status.port)]
    );
    // The status keeps saying both ports while the share runs.
    let later = runtime.status(taken);
    assert_eq!((later.port, later.requested_port), (status.port, taken));
    let (_, body) = exchange_ok(status.port);
    assert!(!body.is_empty());
    // Stopped, it is the port asked for that is shown.
    runtime.stop();
    let after = runtime.status(taken);
    assert!(!after.active);
    assert_eq!((after.port, after.requested_port), (taken, taken));
}

/// `GET /` on the loopback interface: the status line and the body.
fn exchange_ok(port: u16) -> (u16, Vec<u8>) {
    let (status, _, body) = exchange(
        port,
        "GET / HTTP/1.1
Host: 127.0.0.1

",
    );
    assert_eq!(status, 200);
    (status, body)
}

#[test]
fn the_port_found_is_kept_for_this_pc_and_used_by_the_next_share() {
    let dir = tempfile::tempdir().unwrap();
    let context = crate::library::tests::context(dir.path());
    let (runtime, hub) = sharing_runtime();
    let (_other, taken) = other_program();

    // The PC asks for `taken`: another program holds it.
    super::set_port_in(&context, i64::from(taken)).unwrap();
    assert_eq!(super::configured_port(&context), taken);
    let status =
        super::start_and_remember(&runtime, &context, Ipv4Addr::LOCALHOST, hub.clone()).unwrap();
    let found = status.port;
    assert_ne!(found, taken);
    assert_eq!(status.requested_port, taken);

    // Kept in the section of this PC; the shared `[share] port` is left alone.
    let settings = super::store(&context).unwrap().load().unwrap();
    assert_eq!(settings.machine_share_port("PC-A"), Some(found));
    assert_eq!(
        settings.share.port,
        ardoise_core::settings::DEFAULT_SHARE_PORT
    );
    assert_eq!(super::configured_port(&context), found);
    assert_eq!(super::port_setting(&context).port, found);

    // Starting again returns the share running, and the setting is not written again.
    let again =
        super::start_and_remember(&runtime, &context, Ipv4Addr::LOCALHOST, hub.clone()).unwrap();
    assert_eq!(again.port, found);
    runtime.stop();

    // The next share asks for the port kept: no notice to give.
    let next =
        super::start_and_remember(&runtime, &context, Ipv4Addr::LOCALHOST, hub.clone()).unwrap();
    assert_eq!((next.port, next.requested_port), (found, found));
    runtime.stop();
}

#[test]
fn a_share_on_the_port_asked_keeps_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let context = crate::library::tests::context(dir.path());
    let (runtime, hub) = sharing_runtime();
    // A port found free can be taken by another test's client before the share binds it (a
    // client that closed first keeps its port for a while): a few tries.
    let (status, free, before) = (0..10)
        .find_map(|_| {
            let free = other_program().1;
            super::set_port_in(&context, i64::from(free)).unwrap();
            let before = std::fs::read_to_string(super::store(&context).unwrap().path()).unwrap();
            let status =
                super::start_and_remember(&runtime, &context, Ipv4Addr::LOCALHOST, hub.clone())
                    .unwrap();
            if status.port == free {
                return Some((status, free, before));
            }
            runtime.stop();
            None
        })
        .expect("a free port taken as asked");
    assert_eq!((status.port, status.requested_port), (free, free));
    let after = std::fs::read_to_string(super::store(&context).unwrap().path()).unwrap();
    assert_eq!(before, after);
    runtime.stop();
}

#[test]
fn the_port_setting_is_this_pc_s_then_the_shared_one_then_8765() {
    let dir = tempfile::tempdir().unwrap();
    let context = crate::library::tests::context(dir.path());
    let setting = super::port_setting(&context);
    assert_eq!(
        (setting.port, setting.shared_port, setting.own),
        (8765, 8765, false)
    );

    std::fs::write(
        super::store(&context).unwrap().path(),
        "[share]
port = 18765
",
    )
    .unwrap();
    let setting = super::port_setting(&context);
    assert_eq!(
        (setting.port, setting.shared_port, setting.own),
        (18765, 18765, false)
    );

    let setting = super::set_port_in(&context, 9000).unwrap();
    assert_eq!(
        (setting.port, setting.shared_port, setting.own),
        (9000, 18765, true)
    );
    assert_eq!(super::configured_port(&context), 9000);
    // Another PC of the same file still reads the shared port.
    let other = ardoise_core::settings::SettingsStore::locate(dir.path(), dir.path(), "PC-B");
    assert_eq!(other.load().unwrap().share_port_for("PC-B"), 18765);

    // The edges are accepted, anything else is refused and changes nothing.
    assert_eq!(super::set_port_in(&context, 1024).unwrap().port, 1024);
    assert_eq!(super::set_port_in(&context, 65_535).unwrap().port, 65_535);
    for wrong in [0, 80, 1023, 65_536, 70_000, -1, i64::MAX] {
        let error = super::set_port_in(&context, wrong).unwrap_err();
        assert_eq!(error.parts(), ("sharePortInvalid", "1024-65535"), "{wrong}");
    }
    assert_eq!(super::port_setting(&context).port, 65_535);
}

#[test]
fn content_types_follow_the_extension() {
    assert_eq!(content_type("a.CSS"), "text/css; charset=utf-8");
    assert_eq!(content_type("a.woff2"), "font/woff2");
    assert_eq!(content_type("a.svg"), "image/svg+xml");
    assert_eq!(content_type("noextension"), "application/octet-stream");
}

// --- Live mode: stripping ----------------------------------------------------------------------

fn note_with(strokes: Vec<Stroke>) -> Note {
    Note {
        id: "4b0c7a52-6a3b-4f39-9a0e-3c2b8f8e1d7a".into(),
        title: "Contrôle de chimie de Léa".into(),
        created: 1_792_843_200_000,
        modified: 1_792_843_200_000,
        page: A4_LANDSCAPE,
        background: Background::default(),
        background_image: None,
        tags: vec!["élèves".into()],
        favorite: true,
        layers: vec![Layer {
            name: "Calque 1".into(),
            visible: true,
        }],
        strokes,
        timing: Timing::Recorded,
        origin: Origin::Live,
    }
}

fn pen_stroke(x: f32, n: u32) -> Stroke {
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 60.0,
        layer: 0,
        points: (0..n)
            .map(|i| Point {
                x: x + 100.0 * i as f32,
                y: 2000.0,
                pressure: 0.5,
                t: 5 * i,
            })
            .collect(),
    }
}

fn snapshot_of(note: Note) -> LiveSnapshot {
    LiveSnapshot {
        state: crate::live::model::LiveSessionState::Live,
        page: note,
        page_index: 0,
        page_count: 1,
        orientation: Orientation::Landscape,
        can_turn_page: true,
        layer: 0,
        style: LiveStyle::default(),
        view: LiveViewState::whole(A4_LANDSCAPE),
        hover: None,
        presentation: Some(MonitorInfo {
            key: "VIDEOPROJECTEUR SALLE 12@1920,0".into(),
            name: "VIDEOPROJECTEUR SALLE 12".into(),
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
            scale: 1.0,
            primary: false,
            chosen: true,
        }),
        saved: vec!["C:\\Users\\Prof\\Nextcloud\\Notes\\page.ardoise".into()],
    }
}

#[test]
fn a_viewer_never_gets_library_paths_titles_tags_nor_the_screen() {
    let batch = LiveBatch {
        events: vec![
            LiveEvent::Snapshot {
                snapshot: Box::new(snapshot_of(note_with(vec![pen_stroke(1000.0, 4)]))),
            },
            LiveEvent::Saved {
                paths: vec!["C:\\Users\\Prof\\Nextcloud\\Notes\\page.ardoise".into()],
            },
            LiveEvent::SaveFailed {
                page_index: 0,
                code: "io".into(),
                message: "C:\\Users\\Prof\\Nextcloud\\Notes: access denied".into(),
            },
            LiveEvent::HoverEnd,
            LiveEvent::StrokeEnd { page_index: 0 },
        ],
        sent_at: 1.0,
    };
    let text = batch_json(&batch).unwrap();
    for secret in [
        "Nextcloud",
        "Léa",
        "élèves",
        "VIDEOPROJECTEUR",
        "access denied",
    ] {
        assert!(!text.contains(secret), "{secret} in {text}");
    }
    let value: Value = serde_json::from_str(&text).unwrap();
    let events = value["events"].as_array().unwrap();
    let types: Vec<&str> = events.iter().map(|e| e["type"].as_str().unwrap()).collect();
    assert_eq!(types, ["snapshot", "hoverEnd", "strokeEnd"]);
    let snapshot = &events[0]["snapshot"];
    assert_eq!(snapshot["saved"], json!([]));
    assert_eq!(snapshot["presentation"], Value::Null);
    assert_eq!(snapshot["page"]["title"], "");
    assert_eq!(snapshot["page"]["tags"], json!([]));
    // Not a favourite (the field is left out when false).
    assert_ne!(snapshot["page"]["favorite"], true);
    // What is drawn stays.
    assert_eq!(snapshot["page"]["strokes"].as_array().unwrap().len(), 1);
    assert_eq!(snapshot["page"]["layers"][0]["visible"], true);
    assert_eq!(snapshot["view"]["zoom"], 1.0);
    assert_eq!(subscriber_label(7), "share:7");
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
fn a_viewer_gets_only_the_fields_the_viewer_draws() {
    // An allow list: a field added later to the hub's types is not in this set, so it does not
    // reach a browser until someone adds it here on purpose.
    let batch = LiveBatch {
        events: vec![LiveEvent::Snapshot {
            snapshot: Box::new(snapshot_of(note_with(vec![pen_stroke(1000.0, 4)]))),
        }],
        sent_at: 1.0,
    };
    let value: Value = serde_json::from_str(&batch_json(&batch).unwrap()).unwrap();
    assert_eq!(keys(&value), ["events", "sentAt"]);
    let snapshot = &value["events"][0]["snapshot"];
    assert_eq!(
        keys(snapshot),
        [
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
    // The page's origin (here a live page) is not told.
    assert_eq!(page["origin"], json!({"kind": "ardoise"}));
    assert_eq!(keys(&page["layers"][0]), ["name", "visible"]);
    assert_eq!(page["layers"][0]["name"], "");
}

/// A red mark of two points.
fn red_mark() -> LiveMark {
    LiveMark {
        tool: Tool::FeltTip,
        color: Rgba {
            r: 220,
            g: 38,
            b: 38,
            a: 255,
        },
        width: 140.0,
        points: vec![
            MarkPoint {
                x: 100.0,
                y: 500.0,
                p: 0.5,
            },
            MarkPoint {
                x: 200.0,
                y: 600.0,
                p: 1.0,
            },
        ],
    }
}

/// The marks of the events of a viewer's message.
fn marks_in(message: &Value) -> Vec<Value> {
    live_events(message)
        .into_iter()
        .filter(|event| event["type"] == "marks")
        .map(|event| event["marks"].clone())
        .collect()
}

#[test]
fn a_viewer_gets_the_marks_with_only_what_is_drawn() {
    let batch = LiveBatch {
        events: vec![LiveEvent::Marks {
            marks: vec![red_mark()],
        }],
        sent_at: 1.0,
    };
    let value: Value = serde_json::from_str(&batch_json(&batch).unwrap()).unwrap();
    let event = &value["events"][0];
    assert_eq!(keys(event), ["marks", "type"]);
    assert_eq!(event["type"], "marks");
    let mark = &event["marks"][0];
    assert_eq!(keys(mark), ["color", "points", "tool", "width"]);
    assert_eq!(keys(&mark["points"][0]), ["p", "x", "y"]);
    assert_eq!(
        mark,
        &json!({
            "tool": "feltTip",
            "color": { "r": 220, "g": 38, "b": 38, "a": 255 },
            "width": 140.0,
            "points": [{ "x": 100.0, "y": 500.0, "p": 0.5 }, { "x": 200.0, "y": 600.0, "p": 1.0 }]
        })
    );
    // An empty list is sent too: it is how the viewer's marks go.
    let none = LiveBatch {
        events: vec![LiveEvent::Marks { marks: Vec::new() }],
        sent_at: 1.0,
    };
    let value: Value = serde_json::from_str(&batch_json(&none).unwrap()).unwrap();
    assert_eq!(value["events"][0], json!({ "type": "marks", "marks": [] }));
}

#[test]
fn live_strokes_of_a_hidden_layer_never_reach_a_viewer() {
    let mut note = note_with(vec![pen_stroke(1000.0, 3), pen_stroke(5000.0, 3)]);
    note.layers.push(Layer {
        name: "Corrigé".into(),
        visible: true,
    });
    note.layers[0].visible = false;
    note.strokes[1].layer = 1;
    // A stroke of a layer that does not exist is drawn by the viewer: it stays.
    let mut orphan = pen_stroke(9000.0, 2);
    orphan.layer = 7;
    note.strokes.push(orphan);
    let batch = LiveBatch {
        events: vec![LiveEvent::Snapshot {
            snapshot: Box::new(snapshot_of(note)),
        }],
        sent_at: 1.0,
    };
    let text = batch_json(&batch).unwrap();
    let value: Value = serde_json::from_str(&text).unwrap();
    let page = &value["events"][0]["snapshot"]["page"];
    assert_eq!(page["layers"][0]["visible"], false);
    let layers: Vec<u64> = page["strokes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|stroke| stroke["layer"].as_u64().unwrap())
        .collect();
    assert_eq!(layers, [1, 7]);
    assert!(!text.contains("Corrigé"));
}

#[test]
fn viewers_with_the_same_batch_share_one_text() {
    let memo = Memo::default();
    let batch = |x: f32| LiveBatch {
        events: vec![
            LiveEvent::Snapshot {
                snapshot: Box::new(snapshot_of(note_with(vec![pen_stroke(x, 4)]))),
            },
            LiveEvent::HoverEnd,
        ],
        sent_at: 1.0,
    };
    let first = memo.json(batch(1000.0)).unwrap();
    let second = memo.json(batch(1000.0)).unwrap();
    assert!(Arc::ptr_eq(&first, &second));
    // A viewer that missed something gets what it is owed.
    let other = memo.json(batch(2000.0)).unwrap();
    assert!(!Arc::ptr_eq(&first, &other));
    assert_ne!(first, other);
    // Only library events: nothing to send, and nothing remembered over the last text.
    let only_library = LiveBatch {
        events: vec![LiveEvent::Saved { paths: Vec::new() }],
        sent_at: 1.0,
    };
    assert!(memo.json(only_library).is_none());
    assert!(Arc::ptr_eq(&other, &memo.json(batch(2000.0)).unwrap()));
}

// --- The editor's note -------------------------------------------------------------------------

fn parse(message: &str) -> Value {
    serde_json::from_str(message).unwrap()
}

#[test]
fn a_note_goes_whole_then_as_the_strokes_added() {
    let mut sharer = NoteSharer::default();
    let renders = AtomicUsize::new(0);
    let render = |_: &SharedPaper| {
        renders.fetch_add(1, Ordering::SeqCst);
        Some("data:image/png;base64,AAAA".to_owned())
    };

    // Plain paper: nothing rendered, background null.
    let first = sharer
        .update(note_with(vec![pen_stroke(1000.0, 3)]), None, render)
        .unwrap();
    assert_eq!(first.append, None);
    let full = parse(&first.full);
    assert_eq!(full["type"], "note");
    assert_eq!(full["background"], Value::Null);
    assert_eq!(full["page"]["strokes"].as_array().unwrap().len(), 1);
    assert_eq!(full["page"]["page"]["width"], 29_700);
    assert!(full["page"].get("title").is_none(), "{full}");
    assert!(!first.full.contains("Léa") && !first.full.contains("élèves"));
    assert_eq!(renders.load(Ordering::SeqCst), 0);

    // Nothing drawn changed (a new title): nothing to send.
    let mut retitled = note_with(vec![pen_stroke(1000.0, 3)]);
    retitled.title = "Autre".into();
    assert!(sharer.update(retitled, None, render).is_none());

    // Two strokes added: an append with them, and the whole page for later viewers.
    let added = sharer
        .update(
            note_with(vec![
                pen_stroke(1000.0, 3),
                pen_stroke(3000.0, 4),
                pen_stroke(5000.0, 5),
            ]),
            None,
            render,
        )
        .unwrap();
    let append = parse(added.append.as_deref().unwrap());
    assert_eq!(append["type"], "noteStrokes");
    assert_eq!(append["strokes"].as_array().unwrap().len(), 2);
    assert_eq!(append["strokes"][0]["points"].as_array().unwrap().len(), 4);
    assert_eq!(
        parse(&added.full)["page"]["strokes"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    // A stroke erased: the whole page, without its paper for the viewers showing it.
    let erased = sharer
        .update(note_with(vec![pen_stroke(1000.0, 3)]), None, render)
        .unwrap();
    let replaced = parse(erased.append.as_deref().unwrap());
    assert_eq!(replaced["type"], "note");
    assert!(replaced.get("background").is_none(), "{replaced}");
    assert_eq!(replaced["page"]["strokes"].as_array().unwrap().len(), 1);
    assert_eq!(parse(&erased.full)["background"], Value::Null);

    // A layer hidden: the whole page (without its paper); layer names never leave.
    let mut hidden = note_with(vec![pen_stroke(1000.0, 3), pen_stroke(3000.0, 2)]);
    hidden.layers[0].visible = false;
    hidden.layers[0].name = "Corrigé de Léa".into();
    let update = sharer.update(hidden, None, render).unwrap();
    let replaced = parse(update.append.as_deref().unwrap());
    assert_eq!(replaced["type"], "note");
    assert_eq!(replaced["page"]["layers"][0]["visible"], false);
    assert_eq!(replaced["page"]["layers"][0]["name"], "");
    assert!(!update.full.contains("Léa"));

    // Lined paper: rendered once, sent with the page, not rendered again for a new stroke.
    let mut lined = note_with(vec![pen_stroke(1000.0, 3)]);
    lined.background.template = Template::Lined;
    let with_paper = sharer.update(lined.clone(), None, render).unwrap();
    assert_eq!(
        parse(&with_paper.full)["background"],
        "data:image/png;base64,AAAA"
    );
    assert_eq!(renders.load(Ordering::SeqCst), 1);
    lined.strokes.push(pen_stroke(4000.0, 3));
    let next = sharer.update(lined.clone(), None, render).unwrap();
    assert!(next.append.is_some());
    assert_eq!(
        parse(&next.full)["background"],
        "data:image/png;base64,AAAA"
    );
    assert_eq!(renders.load(Ordering::SeqCst), 1);

    // Another note on the same paper: the whole page, the paper already shown.
    let mut other = note_with(vec![pen_stroke(1000.0, 3)]);
    other.id = "another".into();
    other.background.template = Template::Lined;
    let update = sharer.update(other.clone(), None, render).unwrap();
    assert!(
        parse(update.append.as_deref().unwrap())
            .get("background")
            .is_none()
    );
    assert_eq!(renders.load(Ordering::SeqCst), 1);

    // The viewers left the note (waiting screen, live mode): the same note again goes whole,
    // with its paper, although nothing in it changed.
    sharer.reset();
    let again = sharer.update(other, None, render).unwrap();
    assert_eq!(again.append, None);
    assert_eq!(
        parse(&again.full)["background"],
        "data:image/png;base64,AAAA"
    );
    assert_eq!(renders.load(Ordering::SeqCst), 1);

    // A new share starts from scratch.
    sharer.reset();
    assert_eq!(sharer.update(lined, None, render).unwrap().append, None);
}

#[test]
fn a_hidden_layer_is_not_sent_to_the_viewers() {
    let mut sharer = NoteSharer::default();
    let render = |_: &SharedPaper| None;
    let mut note = note_with(vec![pen_stroke(1000.0, 3), pen_stroke(4000.0, 3)]);
    note.layers.push(Layer {
        name: "Corrigé de Léa".into(),
        visible: false,
    });
    note.strokes[1].layer = 1;

    // The answers layer is hidden: its stroke is not in the page, the layer still is (strokes
    // refer to layers by index).
    let first = sharer.update(note.clone(), None, render).unwrap();
    let full = parse(&first.full);
    assert_eq!(full["page"]["strokes"].as_array().unwrap().len(), 1);
    assert_eq!(full["page"]["strokes"][0]["layer"], 0);
    assert_eq!(full["page"]["layers"].as_array().unwrap().len(), 2);
    assert_eq!(full["page"]["layers"][1]["visible"], false);
    assert!(!first.full.contains("Léa"));

    // Writing on the hidden layer changes nothing the viewers see: nothing is sent.
    note.strokes.push(pen_stroke(7000.0, 3));
    note.strokes[2].layer = 1;
    assert!(sharer.update(note.clone(), None, render).is_none());

    // Showing the layer sends the page whole, with its strokes; hiding it again takes them back.
    note.layers[1].visible = true;
    let shown = sharer.update(note.clone(), None, render).unwrap();
    assert_eq!(
        parse(shown.append.as_deref().unwrap())["page"]["strokes"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
    note.layers[1].visible = false;
    let hidden = sharer.update(note, None, render).unwrap();
    let replaced = parse(hidden.append.as_deref().unwrap());
    assert_eq!(replaced["type"], "note");
    assert_eq!(replaced["page"]["strokes"].as_array().unwrap().len(), 1);
}

#[test]
fn stopping_does_not_wait_for_a_note_being_applied() {
    let runtime = ShareRuntime::default();
    let render = |_: &SharedPaper| None;
    let note = || note_with(vec![pen_stroke(1000.0, 3)]);
    assert!(
        super::lock(&runtime.inner.notes)
            .update(note(), None, render)
            .is_some()
    );

    // A content is being applied (its paper rendering): the lock is taken, `stop` goes on.
    {
        let _applying = super::lock(&runtime.inner.notes);
        runtime.inner.forget_note();
        runtime.stop();
    }
    // Nothing was forgotten then: the viewers' note is the same one.
    assert!(
        super::lock(&runtime.inner.notes)
            .update(note(), None, render)
            .is_none()
    );

    // Nothing applied: the note is forgotten, the next one goes whole.
    runtime.inner.forget_note();
    assert!(
        super::lock(&runtime.inner.notes)
            .update(note(), None, render)
            .is_some()
    );
}

#[test]
fn the_paper_is_rendered_at_2560_pixels_on_its_longer_side() {
    let paper = |page: PageSize| SharedPaper {
        page,
        background: Background {
            paper: Paper::Cream,
            template: Template::Grid5mm,
            texture: false,
        },
        image: None,
    };
    assert_eq!(paper(A4_LANDSCAPE).width_px(), PAPER_PX);
    let portrait = paper(PageSize {
        width: 21_600,
        height: 29_700,
    });
    assert_eq!(portrait.width_px(), 2560 * 21_600 / 29_700);
    assert!(!portrait.is_plain());
    let png = render_paper(&portrait).unwrap();
    assert!(png.starts_with("data:image/png;base64,"), "{}", &png[..40]);

    let mut plain = paper(A4_LANDSCAPE);
    plain.background.template = Template::Blank;
    assert!(plain.is_plain());
    plain.image = Some((
        BackgroundImage {
            name: "fiche.pdf".into(),
            pdf_page: Some(1),
            rect: ImageRect {
                x: 0.0,
                y: 0.0,
                width: 1000.0,
                height: 1000.0,
            },
            quarter_turns: 0,
            opacity: 1.0,
        },
        "not-staged".into(),
    ));
    assert!(!plain.is_plain());
    // An image whose pixels are not staged is left out (the paper alone).
    assert!(render_paper(&plain).is_some());
}

#[test]
fn the_contents_arrive_as_the_interface_sends_them() {
    let live: ShareContent = serde_json::from_value(json!({ "kind": "live" })).unwrap();
    assert_eq!(live, ShareContent::Live);
    let waiting: ShareContent = serde_json::from_value(json!({ "kind": "waiting" })).unwrap();
    assert_eq!(waiting, ShareContent::Waiting);
    let note = serde_json::to_value(note_with(vec![pen_stroke(1000.0, 2)])).unwrap();
    let content: ShareContent =
        serde_json::from_value(json!({ "kind": "note", "note": note, "imageKey": "abc-12" }))
            .unwrap();
    let ShareContent::Note { note, image_key } = content else {
        panic!("not a note");
    };
    assert_eq!(note.strokes.len(), 1);
    assert_eq!(image_key.as_deref(), Some("abc-12"));
    let keyless: ShareContent = serde_json::from_value(
        json!({ "kind": "note", "note": serde_json::to_value(*note).unwrap(), "imageKey": null }),
    )
    .unwrap();
    assert!(matches!(
        keyless,
        ShareContent::Note {
            image_key: None,
            ..
        }
    ));
}

// --- A share over real TCP ---------------------------------------------------------------------

const A5: PageSize = PageSize {
    width: 21_600,
    height: 14_800,
};

fn hub_config() -> HubConfig {
    HubConfig {
        flush_interval: Duration::from_millis(4),
        save_interval: Duration::from_millis(100),
    }
}

/// Starts a live session that streams from an A5 tablet.
fn start_session(hub: &LiveHub, library: &ardoise_core::library::Library) -> HubSink {
    let options = StartOptions {
        library: library.clone(),
        french: true,
        size: A5,
        orientation: Orientation::Landscape,
        button: Arc::new(|| LiveButton::NewPage),
    };
    let Start::New { mut sink, .. } = hub.start(options).unwrap() else {
        panic!("a session is already running");
    };
    sink.input(LiveInput::Started {
        width: A5.width,
        height: A5.height,
        orientation: Orientation::Landscape,
    });
    sink
}

/// `n` samples with pressure then the pen lifted, three records per notification.
fn write_stroke(sink: &mut HubSink, x: u16, n: u16) {
    let pen = |x: u16, y: u16, pressure: u16| LiveRecord::Pen(PenPoint { x, y, pressure });
    let mut records: Vec<LiveRecord> = (0..n).map(|i| pen(x + i * 10, 1000, 800)).collect();
    records.push(pen(x + n * 10, 1000, 0));
    for chunk in records.chunks(3) {
        sink.input(LiveInput::Records {
            records: chunk.to_vec(),
            received: SystemTime::now(),
        });
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// An HTTP exchange on a new connection: the status, the head and the body.
fn exchange(port: u16, request: &str) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .expect("a response head");
    let head = String::from_utf8_lossy(&response[..split]).into_owned();
    let status = head
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .expect("a status");
    (status, head, response[split + 4..].to_vec())
}

/// Joins with `code` and returns the cookie to send back.
fn join(port: u16, code: &str) -> String {
    let body = json!({ "code": code }).to_string();
    let (status, head, _) = exchange(
        port,
        &format!(
            "POST /join HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        ),
    );
    assert_eq!(status, 200, "{head}");
    let cookie = head
        .lines()
        .find_map(|line| {
            let (name, value) = line.split_once(':')?;
            name.eq_ignore_ascii_case("set-cookie")
                .then(|| value.trim())
        })
        .expect("a cookie");
    assert!(cookie.contains("HttpOnly"), "{cookie}");
    cookie.split(';').next().unwrap().to_owned()
}

/// The event stream of a viewer: one JSON message at a time, with the moment it was read.
struct Viewer {
    reader: BufReader<TcpStream>,
    /// Every message's text, to look for what must never be sent.
    texts: Vec<String>,
}

impl Viewer {
    fn open(port: u16, cookie: &str) -> Viewer {
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .unwrap();
        write!(
            stream,
            "GET /events HTTP/1.1\r\nHost: 127.0.0.1\r\nCookie: {cookie}\r\n\r\n"
        )
        .unwrap();
        let mut reader = BufReader::new(stream);
        let mut status = String::new();
        reader.read_line(&mut status).unwrap();
        assert!(status.contains(" 200 "), "{status}");
        loop {
            let mut line = String::new();
            reader.read_line(&mut line).unwrap();
            if line == "\r\n" || line.is_empty() {
                break;
            }
        }
        Viewer {
            reader,
            texts: Vec::new(),
        }
    }

    /// The next message (comments and `retry` skipped); `None` when the stream ended.
    fn next(&mut self) -> Option<(Value, Instant)> {
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line).ok()? == 0 {
                return None;
            }
            if let Some(data) = line.strip_prefix("data: ") {
                let text = data.trim_end().to_owned();
                let value = serde_json::from_str(&text).unwrap();
                self.texts.push(text);
                return Some((value, Instant::now()));
            }
        }
    }

    /// Reads until a message satisfies `wanted`; panics after `within`.
    fn until(&mut self, what: &str, within: Duration, wanted: impl Fn(&Value) -> bool) -> Value {
        let deadline = Instant::now() + within;
        loop {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            let (message, _) = self
                .next()
                .unwrap_or_else(|| panic!("stream ended before {what}"));
            if wanted(&message) {
                return message;
            }
        }
    }
}

fn live_events(message: &Value) -> Vec<Value> {
    message["batch"]["events"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

/// The Unix time in fractional milliseconds (as `LivePoint.recv`).
fn wall_ms() -> f64 {
    crate::live::model::wall_now_ms()
}

#[test]
fn a_viewer_follows_live_mode_then_the_editors_note_and_is_told_when_the_share_stops() {
    let (dir, library) = library();
    let library_name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let hub = Arc::new(LiveHub::new(hub_config()));
    let runtime = ShareRuntime::default();

    // Without the embedded interface there is nothing to serve (and `--self-test` says so).
    assert!(runtime.viewer_check().is_err());
    let refused = runtime
        .start(Ipv4Addr::LOCALHOST, 0, hub.clone(), None)
        .unwrap_err();
    assert!(refused.is_code("shareUnavailable"));

    runtime.set_assets(source(built(MANIFEST)));
    assert_eq!(runtime.viewer_check(), Ok(6));
    // The window says live mode before the share starts: it is remembered.
    runtime.inner.show(ShareContent::Live);
    let status = runtime
        .start(Ipv4Addr::LOCALHOST, 0, hub.clone(), None)
        .unwrap();
    assert!(status.active && status.port != 0);
    assert_eq!(
        status.addresses,
        [format!("http://127.0.0.1:{}/", status.port)]
    );
    assert!(status.qr.as_ref().is_some_and(|qr| qr.size >= 21));
    let code = status.code.clone().unwrap();
    assert_eq!(code.len(), 7, "{code}");
    let port = status.port;
    // Starting again returns the share running.
    assert_eq!(
        runtime
            .start(Ipv4Addr::LOCALHOST, 0, hub.clone(), None)
            .unwrap()
            .code,
        Some(code.clone())
    );

    let (page, head, body) = exchange(port, "GET / HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
    assert_eq!(page, 200, "{head}");
    assert_eq!(body, b"<viewer.html>");
    let (wasm, head, _) = exchange(
        port,
        "GET /assets/ardoise_ink-G7.wasm HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n",
    );
    assert_eq!(wasm, 200, "{head}");
    assert!(head.contains("application/wasm"), "{head}");
    for path in ["/index.html", "/assets/index-A1.js", "/.vite/manifest.json"] {
        let (status, _, _) = exchange(
            port,
            &format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"),
        );
        assert_eq!(status, 404, "{path}");
    }

    let sink = start_session(&hub, &library);
    let cookie = join(port, &code.to_lowercase());
    let mut viewer = Viewer::open(port, &cookie);
    let first = viewer.until("the first live batch", Duration::from_secs(5), |m| {
        m["type"] == "live"
    });
    assert_eq!(first["reset"], true);
    let events = live_events(&first);
    assert_eq!(events[0]["type"], "snapshot");
    assert_eq!(events[0]["snapshot"]["saved"], json!([]));
    assert_eq!(events[0]["snapshot"]["presentation"], Value::Null);

    // Strokes arrive as increments, fast; the page written meanwhile is never announced.
    let mut delays = Vec::new();
    // The tablet writes while the viewer reads (as for real).
    let writer = std::thread::spawn(move || {
        let mut sink = sink;
        for i in 0..5 {
            write_stroke(&mut sink, 2000 + 2000 * i, 30);
        }
        sink
    });
    let mut strokes_ended = 0;
    let deadline = Instant::now() + Duration::from_secs(10);
    while strokes_ended < 5 {
        assert!(Instant::now() < deadline, "the strokes did not arrive");
        let (message, _) = viewer.next().expect("the stream");
        let arrival = wall_ms();
        if message["type"] != "live" {
            continue;
        }
        assert_eq!(message["reset"], false);
        for event in live_events(&message) {
            let points = match event["type"].as_str() {
                Some("strokeStart") => event["stroke"]["points"].clone(),
                Some("points") => event["points"].clone(),
                Some("strokeEnd") => {
                    strokes_ended += 1;
                    continue;
                }
                _ => continue,
            };
            for point in points.as_array().unwrap() {
                delays.push(arrival - point["recv"].as_f64().unwrap());
            }
        }
    }
    let _sink = writer.join().unwrap();
    delays.sort_by(f64::total_cmp);
    let median = delays[delays.len() / 2];
    println!(
        "share latency, hub input to the viewer's socket: median {median:.2} ms over {} points",
        delays.len()
    );
    assert!(median < 50.0, "median {median} ms");
    assert_eq!(runtime.status(8765).viewers, 1);

    // The marks written over the presentation reach the viewer as increments of the stream.
    hub.set_marks(0, vec![red_mark()]).unwrap();
    let marked = viewer.until("the marks", Duration::from_secs(5), |m| {
        m["type"] == "live" && !marks_in(m).is_empty()
    });
    assert_eq!(marked["reset"], false);
    assert_eq!(marks_in(&marked)[0].as_array().unwrap().len(), 1);

    // Pause: a frozen batch (with the marks), then the badge; resume: a fresh subscription.
    runtime.inner.share().unwrap().set_paused(true);
    let frozen = viewer.until("the frozen batch", Duration::from_secs(5), |m| {
        m["type"] == "live" && m["reset"] == true
    });
    assert_eq!(live_events(&frozen)[0]["type"], "snapshot");
    assert_eq!(marks_in(&frozen)[0].as_array().unwrap().len(), 1);
    viewer.until("the pause", Duration::from_secs(5), |m| {
        m["type"] == "pause"
    });
    runtime.inner.share().unwrap().set_paused(false);
    viewer.until("the resume", Duration::from_secs(5), |m| {
        m["type"] == "pause" && m["paused"] == false
    });
    let fresh = viewer.until("a fresh snapshot", Duration::from_secs(5), |m| {
        m["type"] == "live" && m["reset"] == true
    });
    // The marks are still there for the viewer that comes back; a page change removes them.
    assert_eq!(marks_in(&fresh)[0].as_array().unwrap().len(), 1);
    hub.command(crate::live::model::LiveCommandKind::NextPage)
        .unwrap();
    let cleared = viewer.until("the marks gone", Duration::from_secs(5), |m| {
        m["type"] == "live" && !marks_in(m).is_empty()
    });
    assert!(marks_in(&cleared).iter().all(|marks| marks == &json!([])));

    // Live mode stops; the main window goes back to the library, then opens a note.
    hub.stop().unwrap();
    runtime.inner.show(ShareContent::Waiting);
    viewer.until("the waiting screen", Duration::from_secs(5), |m| {
        m["type"] == "waiting"
    });
    let mut note = note_with(vec![pen_stroke(1000.0, 3)]);
    runtime.inner.show(ShareContent::Note {
        note: Box::new(note.clone()),
        image_key: None,
    });
    let whole = viewer.until("the note", Duration::from_secs(5), |m| m["type"] == "note");
    assert_eq!(whole["page"]["strokes"].as_array().unwrap().len(), 1);
    note.strokes.push(pen_stroke(5000.0, 4));
    runtime.inner.show(ShareContent::Note {
        note: Box::new(note.clone()),
        image_key: None,
    });
    let added = viewer.until("the stroke added", Duration::from_secs(5), |m| {
        m["type"] == "noteStrokes"
    });
    assert_eq!(added["strokes"].as_array().unwrap().len(), 1);

    // Back to the library, then the same note opened again, unchanged: it shows again.
    runtime.inner.show(ShareContent::Waiting);
    viewer.until("the waiting screen", Duration::from_secs(5), |m| {
        m["type"] == "waiting"
    });
    runtime.inner.show(ShareContent::Note {
        note: Box::new(note.clone()),
        image_key: None,
    });
    let reopened = viewer.until("the note again", Duration::from_secs(5), |m| {
        m["type"] == "note"
    });
    assert_eq!(reopened["page"]["strokes"].as_array().unwrap().len(), 2);

    // Nothing of the library ever went out: no folder, no title, no tag.
    for text in &viewer.texts {
        for secret in [library_name.as_str(), "Léa", "élèves", ".ardoise"] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
    }

    // A late joiner gets the note whole.
    let mut late = Viewer::open(port, &join(port, &code));
    let late_note = late.until("the note", Duration::from_secs(5), |m| m["type"] == "note");
    assert_eq!(late_note["page"]["strokes"].as_array().unwrap().len(), 2);

    // Stop: every viewer is told, the port closes, and the status says so.
    runtime.stop();
    viewer.until("the end", Duration::from_secs(5), |m| m["type"] == "end");
    late.until("the end", Duration::from_secs(5), |m| m["type"] == "end");
    assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
    let after = runtime.status(8765);
    assert!(!after.active && after.code.is_none() && after.port == 8765);
    runtime.stop();
    // The hub kept no subscription of the share.
    assert!(!hub_has_share_subscribers(&hub));
}

/// Whether some subscriber of the hub is a viewer of the share.
fn hub_has_share_subscribers(hub: &LiveHub) -> bool {
    hub.subscriber_labels()
        .iter()
        .any(|label| label.starts_with("share:"))
}
