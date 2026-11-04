//! Tests of the share in the app: the viewer's files from the build manifest, what a viewer
//! receives of live mode (stripped), the editor's note (whole or appended, paper rendered once),
//! and a share run over real TCP with a real live hub.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use ardoise_core::format::{BackgroundFile, NoteFile};
use ardoise_core::library::Library;
use ardoise_core::note::{
    A4_LANDSCAPE, Background, BackgroundImage, ImageRect, Layer, Note, Origin, PageSize, Paper,
    Template, Timing,
};
use ardoise_core::settings::{LiveButton, Orientation};
use ardoise_device::service::{LiveInput, LiveSink};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::Point as PenPoint;
use ardoise_render::{
    Image, NoteView, Picture, RenderOptions, decode_png, encode_png, render_note,
};
use ardoise_share::LivePaper;
use serde_json::{Value, json};

use super::assets::{AssetSource, ViewerAssetsError, content_type, viewer_assets};
use super::feed::{Memo, batch_json, subscriber_label};
use super::live_paper::{LivePapers, paper_key};
use super::page::{NoteSharer, PAPER_PX, Paper as SharedPaper};
use super::{ShareContent, ShareRuntime, render_paper};
use crate::background::tests::halves;
use crate::library::blank_note;
use crate::library::tests::library;
use crate::live::LiveHub;
use crate::live::StrokesEdit;
use crate::live::hub::{HubConfig, HubSink, Start, StartOptions};
use crate::live::model::{
    LiveBatch, LiveEvent, LiveMark, LiveSnapshot, LiveStyle, LiveViewState, MarkPoint,
    PlacedStroke, RecolouredStrokes, ViewerIndices,
};
use crate::notes::background_png;
use crate::presentation::MonitorInfo;
use crate::thumbnails::tests::png_of;

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
        revision: 7 << 32,
        can_undo: true,
        can_redo: true,
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

// --- Touching up during live mode ------------------------------------------------------------------

fn delete_strokes(indices: &[u32]) -> StrokesEdit {
    serde_json::from_value(json!({ "kind": "delete", "indices": indices })).unwrap()
}

fn recolour_strokes(indices: &[u32]) -> StrokesEdit {
    serde_json::from_value(json!({
        "kind": "recolour",
        "indices": indices,
        "color": { "r": 200, "g": 0, "b": 0, "a": 255 },
    }))
    .unwrap()
}

/// The `strokesEdited` events of a message.
fn edits_in(message: &Value) -> Vec<Value> {
    live_events(message)
        .into_iter()
        .filter(|event| event["type"] == "strokesEdited")
        .collect()
}

/// What a viewer's `LiveState` does with the messages it gets: the strokes it draws, each named by
/// where it starts on the page, with its colour.
#[derive(Default)]
struct ViewerList(Vec<(f64, Value)>);

impl ViewerList {
    fn entry(stroke: &Value) -> (f64, Value) {
        (
            stroke["points"][0]["x"].as_f64().unwrap(),
            stroke["color"].clone(),
        )
    }

    fn apply(&mut self, message: &Value) {
        for event in live_events(message) {
            match event["type"].as_str() {
                Some("snapshot") => {
                    self.0 = event["snapshot"]["page"]["strokes"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(Self::entry)
                        .collect();
                }
                Some("strokesEdited") => {
                    for index in event["removed"].as_array().unwrap().iter().rev() {
                        self.0.remove(index.as_u64().unwrap() as usize);
                    }
                    for placed in event["inserted"].as_array().unwrap() {
                        let index = placed["index"].as_u64().unwrap() as usize;
                        self.0.insert(index, Self::entry(&placed["stroke"]));
                    }
                    for group in event["recoloured"].as_array().unwrap() {
                        for index in group["indices"].as_array().unwrap() {
                            self.0[index.as_u64().unwrap() as usize].1 = group["color"].clone();
                        }
                    }
                }
                _ => {}
            }
        }
    }

    fn starts(&self) -> Vec<f64> {
        self.0.iter().map(|(start, _)| *start).collect()
    }
}

/// Reads until a batch holds an edit, keeping `list` up to date.
fn next_edit(viewer: &mut Viewer, list: &mut ViewerList) -> Value {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        assert!(Instant::now() < deadline, "timed out waiting for an edit");
        let (message, _) = viewer.next().expect("the stream");
        if message["type"] != "live" {
            continue;
        }
        list.apply(&message);
        if let Some(edit) = edits_in(&message).pop() {
            return edit;
        }
    }
}

#[test]
fn a_viewer_gets_an_edit_with_only_what_is_drawn() {
    let event = LiveEvent::StrokesEdited {
        page_index: 2,
        revision: 7 << 32,
        removed: vec![4],
        inserted: Arc::new(vec![PlacedStroke {
            index: 1,
            stroke: pen_stroke(1000.0, 2),
        }]),
        recoloured: vec![RecolouredStrokes {
            color: Rgba::rgb(200, 0, 0),
            indices: vec![0],
        }],
        can_undo: true,
        can_redo: false,
        viewers: None,
    };
    let batch = LiveBatch {
        events: vec![event],
        sent_at: 1.0,
    };
    let value: Value = serde_json::from_str(&batch_json(&batch).unwrap()).unwrap();
    let edit = &value["events"][0];
    // An allow list: what the viewer needs to keep its list, nothing of the history or revision.
    assert_eq!(
        keys(edit),
        ["inserted", "pageIndex", "recoloured", "removed", "type"]
    );
    assert_eq!(edit["type"], "strokesEdited");
    assert_eq!(edit["pageIndex"], 2);
    assert_eq!(edit["removed"], json!([4]));
    assert_eq!(keys(&edit["inserted"][0]), ["index", "stroke"]);
    assert_eq!(
        keys(&edit["inserted"][0]["stroke"]),
        ["color", "layer", "points", "tool", "width"]
    );
    assert_eq!(keys(&edit["recoloured"][0]), ["color", "indices"]);
}

#[test]
fn on_a_page_with_a_hidden_layer_an_edit_goes_in_the_numbering_of_the_viewers() {
    let event = LiveEvent::StrokesEdited {
        page_index: 0,
        revision: 1,
        removed: vec![5],
        inserted: Arc::new(vec![PlacedStroke {
            index: 3,
            stroke: pen_stroke(1000.0, 2),
        }]),
        recoloured: vec![RecolouredStrokes {
            color: Rgba::BLACK,
            indices: vec![4],
        }],
        can_undo: true,
        can_redo: false,
        viewers: Some(Box::new(ViewerIndices {
            removed: vec![2],
            inserted: vec![1],
            recoloured: vec![RecolouredStrokes {
                color: Rgba::BLACK,
                indices: vec![2],
            }],
        })),
    };
    let batch = LiveBatch {
        events: vec![event],
        sent_at: 1.0,
    };
    let value: Value = serde_json::from_str(&batch_json(&batch).unwrap()).unwrap();
    let edit = &value["events"][0];
    assert_eq!(edit["removed"], json!([2]));
    assert_eq!(edit["inserted"][0]["index"], 1);
    assert_eq!(edit["recoloured"][0]["indices"], json!([2]));
}

#[test]
fn a_viewer_follows_the_edits_and_the_paused_share_keeps_its_image() {
    let (dir, library) = library();
    let library_name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let (runtime, hub) = sharing_runtime();
    runtime.inner.show(ShareContent::Live);
    let status = runtime
        .start(Ipv4Addr::LOCALHOST, 0, hub.clone(), None)
        .unwrap();
    let mut sink = start_session(&hub, &library);
    for i in 0..4 {
        write_stroke(&mut sink, 1000 + 2000 * i, 3);
    }
    let cookie = join(status.port, &status.code.unwrap());
    let mut viewer = Viewer::open(status.port, &cookie);
    let mut list = ViewerList::default();
    let first = viewer.until("the first batch", Duration::from_secs(5), |m| {
        m["type"] == "live" && m["reset"] == true
    });
    list.apply(&first);
    assert_eq!(list.starts(), [1000.0, 3000.0, 5000.0, 7000.0]);
    let snapshot = &live_events(&first)[0]["snapshot"];
    assert!(snapshot.get("revision").is_none() && snapshot.get("canUndo").is_none());

    // Each edit arrives as itself, not as a page sent again; the indices are the page's here.
    let revision = hub.snapshot().revision;
    let revision = hub.edit_strokes(0, revision, delete_strokes(&[1])).unwrap();
    let edit = next_edit(&mut viewer, &mut list);
    assert_eq!(
        keys(&edit),
        ["inserted", "pageIndex", "recoloured", "removed", "type"]
    );
    assert_eq!(edit["removed"], json!([1]));
    assert_eq!(list.starts(), [1000.0, 5000.0, 7000.0]);
    hub.edit_strokes(0, revision, recolour_strokes(&[0]))
        .unwrap();
    next_edit(&mut viewer, &mut list);
    assert_eq!(list.0[0].1, json!({ "r": 200, "g": 0, "b": 0, "a": 255 }));
    assert!(hub.undo_edit(0).unwrap());
    next_edit(&mut viewer, &mut list);
    assert_eq!(list.0[0].1, json!({ "r": 0, "g": 0, "b": 0, "a": 255 }));
    assert!(hub.undo_edit(0).unwrap());
    let edit = next_edit(&mut viewer, &mut list);
    assert_eq!(edit["inserted"][0]["index"], 1);
    assert_eq!(list.starts(), [1000.0, 3000.0, 5000.0, 7000.0]);

    // Paused: its frozen image, then nothing of what happens; on resume, the state as it is.
    runtime.inner.share().unwrap().set_paused(true);
    let frozen = viewer.until("the frozen batch", Duration::from_secs(5), |m| {
        m["type"] == "live" && m["reset"] == true
    });
    list.apply(&frozen);
    assert_eq!(list.starts(), [1000.0, 3000.0, 5000.0, 7000.0]);
    viewer.until("the pause", Duration::from_secs(5), |m| {
        m["type"] == "pause"
    });
    let revision = hub.snapshot().revision;
    hub.edit_strokes(0, revision, delete_strokes(&[0, 3]))
        .unwrap();
    std::thread::sleep(Duration::from_millis(50));
    runtime.inner.share().unwrap().set_paused(false);
    let mut edits_during_the_pause = 0;
    let fresh = loop {
        let (message, _) = viewer.next().expect("the stream");
        edits_during_the_pause += edits_in(&message).len();
        if message["type"] == "live" && message["reset"] == true {
            break message;
        }
    };
    assert_eq!(edits_during_the_pause, 0);
    list.apply(&fresh);
    assert_eq!(list.starts(), [3000.0, 5000.0]);

    // Nothing of the library went out with them.
    for text in &viewer.texts {
        assert!(!text.contains(&library_name), "{text}");
    }
    runtime.stop();
}

#[test]
fn over_the_network_a_page_with_a_hidden_layer_keeps_viewers_right_and_its_secret() {
    let (_dir, library) = library();
    // The ink hidden on the sheet is at x = 111 and 333: neither may ever be on the wire.
    let hidden_x = [111.0, 333.0];
    let at = |x: f32, layer: u32| Stroke {
        layer,
        ..pen_stroke(x, 2)
    };
    let mut note = note_with(vec![
        at(hidden_x[0], 1),
        at(1000.0, 0),
        at(hidden_x[1], 1),
        at(2000.0, 0),
    ]);
    note.layers.push(Layer {
        name: "Réponses".into(),
        visible: false,
    });
    note.page = A5;
    let saved = library
        .create(
            std::path::Path::new(""),
            &ardoise_core::format::NoteFile::new(note),
        )
        .unwrap();
    let (runtime, hub) = sharing_runtime();
    runtime.inner.show(ShareContent::Live);
    let status = runtime
        .start(Ipv4Addr::LOCALHOST, 0, hub.clone(), None)
        .unwrap();
    let options = StartOptions {
        library: library.clone(),
        french: true,
        size: A5,
        orientation: Orientation::Landscape,
        button: Arc::new(|| LiveButton::NewPage),
    };
    let Start::New { mut sink, snapshot } = hub.resume(options, &saved.path).unwrap() else {
        panic!("a session is already running");
    };
    assert_eq!(snapshot.page.strokes.len(), 4);
    sink.input(LiveInput::Started {
        width: A5.width,
        height: A5.height,
        orientation: Orientation::Landscape,
    });
    let cookie = join(status.port, &status.code.unwrap());
    let mut viewer = Viewer::open(status.port, &cookie);
    let mut list = ViewerList::default();
    let first = viewer.until("the first batch", Duration::from_secs(5), |m| {
        m["type"] == "live" && m["reset"] == true
    });
    list.apply(&first);
    assert_eq!(list.starts(), [1000.0, 2000.0]);

    // For the page, stroke 3; for the viewer, its second one.
    let revision = hub.snapshot().revision;
    let revision = hub.edit_strokes(0, revision, delete_strokes(&[3])).unwrap();
    let edit = next_edit(&mut viewer, &mut list);
    assert_eq!(edit["removed"], json!([1]));
    assert_eq!(list.starts(), [1000.0]);
    hub.edit_strokes(0, revision, recolour_strokes(&[1]))
        .unwrap();
    let edit = next_edit(&mut viewer, &mut list);
    assert_eq!(edit["recoloured"][0]["indices"], json!([0]));
    assert!(hub.undo_edit(0).unwrap());
    next_edit(&mut viewer, &mut list);
    assert!(hub.undo_edit(0).unwrap());
    let edit = next_edit(&mut viewer, &mut list);
    assert_eq!(edit["inserted"][0]["index"], 1);
    assert_eq!(list.starts(), [1000.0, 2000.0]);

    // « Clear the page » leaves the hidden ink, and its undo brings back only what shows.
    hub.command(crate::live::model::LiveCommandKind::ClearPage)
        .unwrap();
    let cleared = viewer.until("the clearing", Duration::from_secs(5), |m| {
        live_events(m).iter().any(|e| e["type"] == "snapshot")
    });
    list.apply(&cleared);
    assert_eq!(list.starts(), Vec::<f64>::new());
    assert_eq!(hub.snapshot().page.strokes.len(), 2);
    assert!(hub.restore_cleared().unwrap());
    next_edit(&mut viewer, &mut list);
    assert_eq!(list.starts(), [1000.0, 2000.0]);

    for text in &viewer.texts {
        for x in hidden_x {
            assert!(!text.contains(&format!("\"x\":{x:.1}")), "{x} in {text}");
        }
        assert!(!text.contains("Réponses"), "{text}");
    }
    runtime.stop();
    hub.stop().unwrap();
}

/// Whether some subscriber of the hub is a viewer of the share.
fn hub_has_share_subscribers(hub: &LiveHub) -> bool {
    hub.subscriber_labels()
        .iter()
        .any(|label| label.starts_with("share:"))
}

// --- The paper of a resumed sheet -----------------------------------------------------------------

/// Where the image of [`classroom_sheet`] goes on its A5 page: its left half is red, its right
/// half blue, on either side of x = 10 000.
const IMAGE_RECT: ImageRect = ImageRect {
    x: 5_000.0,
    y: 2_000.0,
    width: 10_000.0,
    height: 5_000.0,
};

/// Where the hidden answers of [`classroom_sheet`] are: never in a message.
const HIDDEN_X: f32 = 7_777.0;

fn note_of_the_class() -> Note {
    let mut note = blank_note("Contrôle de Léa", "Réponses de Léa", 1_787_000_000_000);
    note.page = A5;
    note.tags = vec!["élèves".to_owned()];
    note.background = Background {
        paper: Paper::Cream,
        template: Template::Seyes,
        texture: false,
    };
    note.background_image = Some(BackgroundImage {
        name: "photo de la classe.png".to_owned(),
        pdf_page: None,
        rect: IMAGE_RECT,
        quarter_turns: 0,
        opacity: 1.0,
    });
    note.layers = vec![
        Layer {
            name: "Réponses de Léa".to_owned(),
            visible: false,
        },
        Layer {
            name: "Consignes".to_owned(),
            visible: true,
        },
    ];
    let mut hidden = pen_stroke(HIDDEN_X, 3);
    hidden.layer = 0;
    let mut shown = pen_stroke(1_000.0, 3);
    shown.layer = 1;
    note.strokes = vec![hidden, shown];
    note
}

/// A note of the library with a template, an image, a hidden layer, a title and tags: only its
/// paper and its visible ink may reach a viewer.
fn classroom_sheet(library: &Library) -> PathBuf {
    let mut file = NoteFile::new(note_of_the_class());
    file.background = Some(BackgroundFile {
        extension: "png".to_owned(),
        bytes: encode_png(&halves(40, 20)).unwrap(),
    });
    library.create(Path::new(""), &file).unwrap().path
}

/// Resumes the note at `path` on an A5 tablet that streams.
fn resume_sheet(hub: &LiveHub, library: &Library, path: &Path) -> HubSink {
    let options = StartOptions {
        library: library.clone(),
        french: true,
        size: A5,
        orientation: Orientation::Landscape,
        button: Arc::new(|| LiveButton::NewPage),
    };
    let Start::New { mut sink, .. } = hub.resume(options, path).unwrap() else {
        panic!("a session is already running");
    };
    sink.input(LiveInput::Started {
        width: A5.width,
        height: A5.height,
        orientation: Orientation::Landscape,
    });
    sink
}

/// Rendering a picture takes a while in an unoptimised build.
const RENDERING: Duration = Duration::from_secs(120);
const SOON: Duration = Duration::from_secs(10);

impl Viewer {
    /// Waits as long as a picture takes to render.
    fn patient(self) -> Viewer {
        self.reader
            .get_ref()
            .set_read_timeout(Some(RENDERING))
            .unwrap();
        self
    }

    /// Messages telling of a paper received so far.
    fn papers(&self) -> usize {
        self.texts
            .iter()
            .filter(|text| text.contains(r#""type":"livePaper""#))
            .count()
    }

    /// Reads up to the end of the stroke just written: what was on its way came before it.
    fn stroke_written(&mut self) {
        self.until("the stroke", SOON, |m| {
            live_events(m).iter().any(|e| e["type"] == "strokeEnd")
        });
    }

    /// Reads up to the snapshot of page `index`.
    fn snapshot_of_page(&mut self, index: usize) -> Value {
        let of_the_page =
            |e: &Value| e["type"] == "snapshot" && e["snapshot"]["pageIndex"] == index;
        let message = self.until("the snapshot", SOON, |m| {
            live_events(m).iter().any(of_the_page)
        });
        live_events(&message)
            .iter()
            .find(|e| of_the_page(e))
            .unwrap()["snapshot"]
            .clone()
    }
}

/// A request of a browser that joined, on a connection of its own: the status and the head of the
/// answer as text, and its body.
fn fetch(port: u16, target: &str, cookie: Option<&str>) -> (u16, String, Vec<u8>) {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    stream.set_read_timeout(Some(SOON)).unwrap();
    let cookie = cookie.map_or(String::new(), |cookie| format!("Cookie: {cookie}\r\n"));
    write!(
        stream,
        "GET {target} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{cookie}\r\n"
    )
    .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    let (head, length) = loop {
        let n = stream.read(&mut chunk).unwrap();
        assert_ne!(n, 0, "closed without an answer");
        bytes.extend_from_slice(&chunk[..n]);
        let Some(at) = bytes.windows(4).position(|w| w == b"\r\n\r\n") else {
            continue;
        };
        let head = String::from_utf8(bytes.drain(..at + 4).collect()).unwrap();
        let length: usize = head
            .lines()
            .find_map(|line| line.strip_prefix("Content-Length: "))
            .unwrap()
            .parse()
            .unwrap();
        break (head, length);
    };
    while bytes.len() < length {
        let n = stream.read(&mut chunk).unwrap();
        assert_ne!(n, 0, "the answer was cut");
        bytes.extend_from_slice(&chunk[..n]);
    }
    let status = head[9..12].parse().unwrap();
    (status, head, bytes)
}

/// The picture a `livePaper` message tells of, fetched as a viewer does.
fn fetch_paper(port: u16, cookie: &str, paper: &Value) -> Vec<u8> {
    let target = format!("/paper/{}", paper["key"].as_str().unwrap());
    let (status, head, png) = fetch(port, &target, Some(cookie));
    assert_eq!(status, 200, "{head}");
    assert!(head.contains("Content-Type: image/png\r\n"), "{head}");
    assert!(head.contains("Cache-Control: no-store\r\n"), "{head}");
    png
}

fn rgb(image: &Image, x: u32, y: u32) -> [u8; 3] {
    let [r, g, b, _] = image.pixel(x, y).unwrap();
    [r, g, b]
}

fn near(a: [u8; 3], b: [u8; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 8)
}

const RED: [u8; 3] = [220, 30, 30];
const BLUE: [u8; 3] = [30, 40, 220];
/// The renderer's cream and slate papers.
const CREAM: [u8; 3] = [250, 244, 226];
const SLATE: [u8; 3] = [42, 50, 48];

/// A point of the page in the pixels of a picture [`PAPER_PX`] wide (the A5 page is landscape).
fn at(x: u32, y: u32) -> (u32, u32) {
    (x * PAPER_PX / A5.width, y * PAPER_PX / A5.width)
}

/// Whether `colour` shows outside the image, near the page's lower right corner: between the
/// lines of the template.
fn paper_shows(drawn: &Image, colour: [u8; 3]) -> bool {
    let (x, y) = at(20_350, 13_000);
    (y..y + 40).any(|y| near(rgb(drawn, x, y), colour))
}

#[test]
fn a_resumed_sheet_lets_each_viewer_fetch_its_paper() {
    let (dir, library) = library();
    let library_name = dir
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let path = classroom_sheet(&library);
    let hub = Arc::new(LiveHub::new(hub_config()));
    let runtime = ShareRuntime::default();
    runtime.set_assets(source(built(MANIFEST)));
    runtime.inner.show(ShareContent::Live);
    let status = runtime
        .start(Ipv4Addr::LOCALHOST, 0, hub.clone(), None)
        .unwrap();
    let (port, code) = (status.port, status.code.clone().unwrap());
    let mut sink = resume_sheet(&hub, &library, &path);
    let cookie = join(port, &code);

    // The first viewer: its snapshot says which paper the page wants; the picture follows.
    let mut a = Viewer::open(port, &cookie).patient();
    let first = a.until("the snapshot", SOON, |m| m["type"] == "live");
    assert_eq!(first["reset"], true);
    let snapshot = live_events(&first)[0]["snapshot"].clone();
    let key = snapshot["paperKey"].as_str().unwrap().to_owned();
    assert_eq!(key.len(), 16);
    // Nothing of the image's description, and no stroke of the hidden layer; the layer list stays
    // whole, so that the strokes' indices stay valid.
    let page = &snapshot["page"];
    assert!(page.get("backgroundImage").is_none(), "{page}");
    assert_eq!(page["strokes"].as_array().unwrap().len(), 1);
    assert_eq!(page["strokes"][0]["layer"], 1);
    assert_eq!(
        page["layers"],
        json!([{"name": "", "visible": false}, {"name": "", "visible": true}])
    );
    let paper = a.until("the paper", RENDERING, |m| m["type"] == "livePaper");
    // Only that: the stream carries no picture, and nothing else of the note can travel with it.
    assert_eq!(keys(&paper), ["key", "type", "version"]);
    assert_eq!(paper["key"], key.as_str());
    assert!(paper.to_string().len() < 100, "{paper}");
    // The picture is one request away, for those who joined, under this key alone.
    let target = format!("/paper/{key}");
    assert_eq!(fetch(port, &target, None).0, 401);
    assert_eq!(fetch(port, "/paper/ffffffffffffffff", None).0, 401);
    assert_eq!(fetch(port, "/paper/ffffffffffffffff", Some(&cookie)).0, 404);
    assert_eq!(
        fetch(port, &format!("/paper/{key}/../.."), Some(&cookie)).0,
        404
    );
    let png = fetch_paper(port, &cookie, &paper);
    // Nothing of the note in the file either, metadata included.
    for secret in ["Léa", "élèves", "Consignes", "photo de la classe"] {
        let bytes = secret.as_bytes();
        assert!(
            !png.windows(bytes.len()).any(|w| w == bytes),
            "{secret} in the picture"
        );
    }
    // The page's whole paper, with the image where the note says: strokes in page units fall on it.
    let drawn = decode_png(&png).unwrap();
    assert_eq!(drawn.width, PAPER_PX);
    assert!(drawn.height.abs_diff(A5.height * PAPER_PX / A5.width) <= 1);
    let (x, y) = at(7_000, 4_000);
    assert!(near(rgb(&drawn, x, y), RED), "{:?}", rgb(&drawn, x, y));
    let (x, y) = at(13_000, 4_000);
    assert!(near(rgb(&drawn, x, y), BLUE), "{:?}", rgb(&drawn, x, y));
    assert!(paper_shows(&drawn, CREAM));

    // Writing tells nothing again.
    write_stroke(&mut sink, 2000, 10);
    a.stroke_written();
    assert_eq!(a.papers(), 1);

    // A late joiner is told right after its snapshot, the others nothing.
    let mut b = Viewer::open(port, &cookie).patient();
    let (message, _) = b.next().unwrap();
    assert_eq!(message["reset"], true);
    assert_eq!(
        live_events(&message)[0]["snapshot"]["paperKey"],
        key.as_str()
    );
    let (message, _) = b.next().unwrap();
    assert_eq!(message["type"], "livePaper");
    assert_eq!(message["key"], key.as_str());
    write_stroke(&mut sink, 4000, 10);
    a.stroke_written();
    b.stroke_written();
    assert_eq!((a.papers(), b.papers()), (1, 1));

    // The next page is plain: its snapshot wants no picture, which hides it at once, a viewer
    // joining there is told of none, and the picture cannot be fetched any more.
    hub.command(crate::live::model::LiveCommandKind::NextPage)
        .unwrap();
    assert!(a.snapshot_of_page(1).get("paperKey").is_none());
    assert!(b.snapshot_of_page(1).get("paperKey").is_none());
    std::thread::sleep(Duration::from_millis(300));
    let mut c = Viewer::open(port, &cookie).patient();
    let (message, _) = c.next().unwrap();
    assert!(
        live_events(&message)[0]["snapshot"]
            .get("paperKey")
            .is_none()
    );
    write_stroke(&mut sink, 6000, 10);
    for viewer in [&mut a, &mut b, &mut c] {
        viewer.stroke_written();
    }
    assert_eq!((a.papers(), b.papers(), c.papers()), (1, 1, 0));
    assert_eq!(fetch(port, &target, Some(&cookie)).0, 404);

    // Back on the sheet: everyone is told (those who kept the picture fetch nothing), and it can be
    // fetched again.
    hub.command(crate::live::model::LiveCommandKind::PreviousPage)
        .unwrap();
    for viewer in [&mut a, &mut b, &mut c] {
        assert_eq!(viewer.snapshot_of_page(0)["paperKey"], key.as_str());
    }
    for viewer in [&mut a, &mut b, &mut c] {
        let told = viewer.until("the paper", SOON, |m| m["type"] == "livePaper");
        assert_eq!(told, paper, "the same key and the same version");
    }
    assert_eq!(fetch_paper(port, &cookie, &paper), png);
    std::thread::sleep(Duration::from_millis(300));
    write_stroke(&mut sink, 8000, 10);
    for viewer in [&mut a, &mut b, &mut c] {
        viewer.stroke_written();
    }
    assert_eq!((a.papers(), b.papers(), c.papers()), (2, 2, 1));

    // Pause: the frozen image, the paper it asks for and the badge; the same for a viewer joining
    // during the pause, and the picture can still be fetched.
    let share = runtime.inner.share().unwrap();
    share.set_paused(true);
    let frozen = a.until("the frozen image", SOON, |m| {
        m["type"] == "live" && m["reset"] == true
    });
    assert_eq!(
        live_events(&frozen)[0]["snapshot"]["paperKey"],
        key.as_str()
    );
    a.until("the pause", SOON, |m| m["type"] == "pause");
    assert_eq!(fetch_paper(port, &cookie, &paper), png);
    let mut d = Viewer::open(port, &cookie).patient();
    let types: Vec<Value> = (0..3)
        .map(|_| d.next().unwrap().0["type"].clone())
        .collect();
    assert_eq!(types, ["live", "livePaper", "pause"]);
    // Resume: a fresh snapshot, and the same paper: nothing to fetch.
    share.set_paused(false);
    for viewer in [&mut a, &mut b, &mut c, &mut d] {
        viewer.until("the resume", SOON, |m| {
            m["type"] == "pause" && m["paused"] == false
        });
        viewer.until("the fresh snapshot", SOON, |m| {
            m["type"] == "live" && m["reset"] == true
        });
        let told = viewer.until("the paper", RENDERING, |m| m["type"] == "livePaper");
        assert_eq!(told, paper);
    }
    write_stroke(&mut sink, 10_000, 10);
    for viewer in [&mut a, &mut b, &mut c, &mut d] {
        viewer.stroke_written();
    }

    // A viewer that reconnects is a new viewer: it is told again.
    let texts_b = std::mem::take(&mut b.texts);
    drop(b);
    let deadline = Instant::now() + SOON;
    while runtime.status(8765).viewers != 3 {
        assert!(Instant::now() < deadline, "the stream did not close");
        std::thread::sleep(Duration::from_millis(5));
    }
    let mut b = Viewer::open(port, &cookie).patient();
    b.texts = texts_b;
    assert_eq!(b.next().unwrap().0["reset"], true);
    assert_eq!(b.next().unwrap().0["type"], "livePaper");

    // The blackboard: the same sheet on slate paper, as the editor would show it, a new picture.
    hub.set_view(LiveViewState {
        board: true,
        ..LiveViewState::whole(A5)
    })
    .unwrap();
    let on_board = a.snapshot_of_page(0);
    let board_key = on_board["paperKey"].as_str().unwrap().to_owned();
    assert_ne!(board_key, key);
    let paper = a.until("the slate paper", RENDERING, |m| m["type"] == "livePaper");
    assert_eq!(paper["key"], board_key.as_str());
    let drawn = decode_png(&fetch_paper(port, &cookie, &paper)).unwrap();
    assert!(paper_shows(&drawn, SLATE));
    let (x, y) = at(7_000, 4_000);
    assert!(near(rgb(&drawn, x, y), RED), "{:?}", rgb(&drawn, x, y));
    hub.set_view(LiveViewState::whole(A5)).unwrap();
    let paper = a.until("the paper again", RENDERING, |m| m["type"] == "livePaper");
    assert_eq!(paper["key"], key.as_str());
    // The previous paper is gone from the server.
    assert_eq!(
        fetch(port, &format!("/paper/{board_key}"), Some(&cookie)).0,
        404
    );

    // Nothing else of the note ever went out, and nothing of the hidden layer.
    let hidden_x = format!("\"x\":{HIDDEN_X}");
    for viewer in [&a, &b, &c, &d] {
        for text in &viewer.texts {
            for secret in [
                library_name.as_str(),
                "Léa",
                "élèves",
                "Consignes",
                "photo de la classe",
                ".ardoise",
                "backgroundImage",
                hidden_x.as_str(),
            ] {
                assert!(!text.contains(secret), "{secret} in a message");
            }
        }
    }

    runtime.stop();
    assert!(!hub_has_share_subscribers(&hub));
}

/// The fix for a photo on a shared Wi-Fi, with the real hub: the picture takes its time, and the
/// strokes keep coming on the stream meanwhile.
#[test]
fn the_strokes_do_not_wait_for_a_slow_download_of_the_paper() {
    let (_dir, library) = library();
    let path = classroom_sheet(&library);
    let hub = Arc::new(LiveHub::new(hub_config()));
    let runtime = ShareRuntime::default();
    runtime.set_assets(source(built(MANIFEST)));
    runtime.inner.show(ShareContent::Live);
    let status = runtime
        .start(Ipv4Addr::LOCALHOST, 0, hub.clone(), None)
        .unwrap();
    let (port, code) = (status.port, status.code.clone().unwrap());
    let mut sink = resume_sheet(&hub, &library, &path);
    let cookie = join(port, &code);
    let mut viewer = Viewer::open(port, &cookie).patient();
    let paper = viewer.until("the paper", RENDERING, |m| m["type"] == "livePaper");

    // Asked for, and not read at all.
    let mut download = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).unwrap();
    write!(
        download,
        "GET /paper/{} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nCookie: {cookie}\r\n\r\n",
        paper["key"].as_str().unwrap()
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(100));
    for x in [2000, 4000, 6000] {
        write_stroke(&mut sink, x, 10);
        viewer.stroke_written();
    }
    assert_eq!(runtime.status(8765).viewers, 1);
    assert_eq!(viewer.papers(), 1);
    drop(download);
    runtime.stop();
}

#[test]
fn ordinary_live_pages_send_no_paper_and_start_no_work() {
    let (_dir, library) = library();
    let hub = Arc::new(LiveHub::new(hub_config()));
    let runtime = ShareRuntime::default();
    runtime.set_assets(source(built(MANIFEST)));
    runtime.inner.show(ShareContent::Live);
    let status = runtime
        .start(Ipv4Addr::LOCALHOST, 0, hub.clone(), None)
        .unwrap();
    let port = status.port;
    let mut sink = start_session(&hub, &library);
    let mut viewer = Viewer::open(port, &join(port, &status.code.clone().unwrap()));
    let first = viewer.until("the snapshot", SOON, |m| m["type"] == "live");
    assert!(live_events(&first)[0]["snapshot"].get("paperKey").is_none());
    write_stroke(&mut sink, 2000, 10);
    viewer.stroke_written();
    hub.set_view(LiveViewState {
        board: true,
        ..LiveViewState::whole(A5)
    })
    .unwrap();
    assert!(viewer.snapshot_of_page(0).get("paperKey").is_none());
    hub.command(crate::live::model::LiveCommandKind::NextPage)
        .unwrap();
    assert!(viewer.snapshot_of_page(1).get("paperKey").is_none());
    write_stroke(&mut sink, 4000, 10);
    viewer.stroke_written();
    assert_eq!(viewer.papers(), 0);
    runtime.stop();
}

/// Waits for the paper's thread to be done.
fn settled(papers: &LivePapers) {
    let deadline = Instant::now() + RENDERING;
    while !papers.idle() {
        assert!(Instant::now() < deadline, "the paper took too long");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn the_paper_is_made_apart_once_and_kept_for_the_way_back() {
    let (_dir, library) = library();
    let path = classroom_sheet(&library);
    let hub = Arc::new(LiveHub::new(hub_config()));
    let papers = LivePapers::new(hub.clone());
    let sent: Arc<Mutex<Vec<Option<LivePaper>>>> = Arc::default();
    let log = sent.clone();
    papers.attach(Box::new(move |paper| {
        log.lock().unwrap().push(paper);
    }));

    // Plain paper: no key, no thread, nothing to send, as long as it lasts.
    let plain = snapshot_of(note_with(Vec::new()));
    let mut slate = plain.clone();
    slate.view.board = true;
    for snapshot in [&plain, &slate] {
        papers.saw(snapshot);
        papers.subscribed(snapshot);
        assert!(papers.idle());
    }
    assert_eq!(papers.renders(), 0);
    assert!(sent.lock().unwrap().is_empty());

    // The sheet: rendered once, whatever the strokes do.
    let _sink = resume_sheet(&hub, &library, &path);
    let sheet = hub.snapshot();
    papers.saw(&sheet);
    settled(&papers);
    assert_eq!(papers.renders(), 1);
    let first = sent.lock().unwrap().first().cloned().flatten().unwrap();
    assert_eq!(Some(first.key.as_str()), paper_key(&sheet).as_deref());
    assert_eq!(&first.png[..8], b"\x89PNG\r\n\x1a\n");
    let mut written = sheet.clone();
    written.page.strokes.clear();
    papers.saw(&written);
    assert!(papers.idle(), "the same paper asks for nothing");

    // To a plain page and back: told, not rendered again.
    papers.saw(&plain);
    settled(&papers);
    assert_eq!(sent.lock().unwrap().last(), Some(&None));
    papers.saw(&sheet);
    settled(&papers);
    assert_eq!(sent.lock().unwrap().last(), Some(&Some(first.clone())));
    assert_eq!(papers.renders(), 1);
    // The bytes themselves, not a copy: the share and the cache hold one picture.
    let last = sent.lock().unwrap().last().cloned().flatten().unwrap();
    assert!(Arc::ptr_eq(&last.png, &first.png));

    // A viewer coming back looks again, in case the file changed; here it has not.
    let count = sent.lock().unwrap().len();
    papers.subscribed(&sheet);
    settled(&papers);
    assert_eq!(sent.lock().unwrap().len(), count + 1);
    assert_eq!(sent.lock().unwrap().last(), Some(&Some(first)));
    assert_eq!(papers.renders(), 1);

    // The blackboard is another picture; both stay, turning it on and off renders nothing more.
    let mut board = sheet.clone();
    board.view.board = true;
    for snapshot in [&board, &sheet, &board, &sheet] {
        papers.saw(snapshot);
        settled(&papers);
    }
    assert_eq!(papers.renders(), 2);
}

#[test]
fn a_paper_asked_before_the_share_runs_goes_when_it_does() {
    let (_dir, library) = library();
    let path = classroom_sheet(&library);
    let hub = Arc::new(LiveHub::new(hub_config()));
    let papers = LivePapers::new(hub.clone());
    let _sink = resume_sheet(&hub, &library, &path);
    papers.saw(&hub.snapshot());
    assert_eq!(papers.renders(), 0);
    let sent: Arc<Mutex<Vec<Option<LivePaper>>>> = Arc::default();
    let log = sent.clone();
    papers.attach(Box::new(move |paper| {
        log.lock().unwrap().push(paper);
    }));
    settled(&papers);
    assert_eq!(sent.lock().unwrap().len(), 1);
}

#[test]
fn the_paper_key_follows_what_the_picture_shows() {
    let of = |note: Note| paper_key(&snapshot_of(note));
    // Plain paper of any colour, also on the blackboard.
    assert_eq!(of(note_with(Vec::new())), None);
    let mut plain_board = snapshot_of(note_with(Vec::new()));
    plain_board.view.board = true;
    assert_eq!(paper_key(&plain_board), None);

    let sheet = note_of_the_class();
    let key = of(sheet.clone()).unwrap();
    assert_eq!(key.len(), 16);
    assert!(key.chars().all(|c| c.is_ascii_hexdigit()));
    // The strokes, the title and the tags change nothing...
    let mut same = sheet.clone();
    same.strokes.clear();
    same.title = "Autre".to_owned();
    same.tags.clear();
    assert_eq!(of(same).as_ref(), Some(&key));
    // ...the paper does: another place for the image, another image, another template.
    let mut moved = sheet.clone();
    moved.background_image.as_mut().unwrap().rect.x += 1.0;
    assert_ne!(of(moved).as_ref(), Some(&key));
    let mut page_two = sheet.clone();
    page_two.background_image.as_mut().unwrap().pdf_page = Some(2);
    assert_ne!(of(page_two).as_ref(), Some(&key));
    let mut lined = sheet.clone();
    lined.background.template = Template::Lined;
    assert_ne!(of(lined).as_ref(), Some(&key));
    // The same with the blackboard as with slate paper: the picture is the same.
    let mut slate = sheet.clone();
    slate.background.paper = Paper::Slate;
    let mut board = snapshot_of(sheet.clone());
    board.view.board = true;
    assert_eq!(paper_key(&board), of(slate));
    // A template or a texture alone is enough.
    let mut textured = note_with(Vec::new());
    textured.background.texture = true;
    assert!(of(textured).is_some());
}

/// A straight stroke of black ink through the page's points `from` and `to`.
fn black_line(from: (f32, f32), to: (f32, f32)) -> Stroke {
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 60.0,
        layer: 0,
        points: (0..=40u16)
            .map(|i| {
                let f = f32::from(i) / 40.0;
                Point {
                    x: from.0 + (to.0 - from.0) * f,
                    y: from.1 + (to.1 - from.1) * f,
                    pressure: 0.8,
                    t: u32::from(i) * 5,
                }
            })
            .collect(),
    }
}

#[test]
fn ink_falls_on_the_paper_as_in_the_editor_whatever_the_image_s_place() {
    // Scale 0.1: a page point (x, y) is the pixel (x / 10, y / 10). The picture is the left half
    // red and the right half blue; the stroke crosses the line between them.
    let page = PageSize {
        width: 20_000,
        height: 10_000,
    };
    let picture = Picture::decode(encode_png(&halves(40, 20)).unwrap()).unwrap();
    struct Case {
        quarter_turns: u8,
        rect: ImageRect,
        stroke: Stroke,
        /// Pixels of the paper on either side of the stroke, red then blue.
        red: (u32, u32),
        blue: (u32, u32),
    }
    let cases = [
        // Upright: the red half on the left of x = 8 000.
        Case {
            quarter_turns: 0,
            rect: ImageRect {
                x: 4_000.0,
                y: 2_000.0,
                width: 8_000.0,
                height: 4_000.0,
            },
            stroke: black_line((8_000.0, 2_500.0), (8_000.0, 5_500.0)),
            red: (780, 400),
            blue: (820, 400),
        },
        // A quarter turn clockwise: the red half on top, above y = 5 000; it reaches outside the
        // page on the left, which must not move it.
        Case {
            quarter_turns: 1,
            rect: ImageRect {
                x: -1_000.0,
                y: 1_000.0,
                width: 4_000.0,
                height: 8_000.0,
            },
            stroke: black_line((500.0, 5_000.0), (2_500.0, 5_000.0)),
            red: (150, 480),
            blue: (150, 520),
        },
    ];
    for case in cases {
        let image = BackgroundImage {
            name: "demi.png".to_owned(),
            pdf_page: None,
            rect: case.rect,
            quarter_turns: case.quarter_turns,
            opacity: 1.0,
        };
        let background = Background::default();
        // What the live windows and the viewers draw under the strokes.
        let paper = decode_png(
            &background_png(page, background, 2_000, Some((image.clone(), &picture))).unwrap(),
        )
        .unwrap();
        assert!(near(rgb(&paper, case.red.0, case.red.1), RED));
        assert!(near(rgb(&paper, case.blue.0, case.blue.1), BLUE));

        // The same scene as the editor's, ink included.
        let mut note = blank_note("", "", 0);
        note.page = page;
        note.background = background;
        note.background_image = Some(image);
        note.strokes = vec![case.stroke.clone()];
        let editor = render_note(
            NoteView::new(&note, Some(&picture)),
            &RenderOptions::fit_width(page, 2_000),
        )
        .unwrap();
        assert_eq!((editor.width, editor.height), (paper.width, paper.height));
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
        for y in 0..editor.height {
            for x in 0..editor.width {
                if !near(rgb(&editor, x, y), rgb(&paper, x, y)) {
                    (left, top) = (left.min(x), top.min(y));
                    (right, bottom) = (right.max(x), bottom.max(y));
                }
            }
        }
        // Where the two pictures differ is the stroke, at the page's scale, and nowhere else.
        let points = &case.stroke.points;
        let (first, last) = (points[0], points[points.len() - 1]);
        let margin = 5;
        assert!(
            left + margin >= (first.x.min(last.x) / 10.0) as u32,
            "left {left}"
        );
        assert!(
            top + margin >= (first.y.min(last.y) / 10.0) as u32,
            "top {top}"
        );
        assert!(
            right <= (first.x.max(last.x) / 10.0) as u32 + margin,
            "right {right}"
        );
        assert!(
            bottom <= (first.y.max(last.y) / 10.0) as u32 + margin,
            "bottom {bottom}"
        );
        let middle = ((first.x + last.x) / 20.0, (first.y + last.y) / 20.0);
        let ink = rgb(&editor, middle.0 as u32, middle.1 as u32);
        assert!(near(ink, [0, 0, 0]), "{ink:?}");
    }
}

#[test]
fn the_windows_get_the_image_of_the_resumed_sheet_and_no_other_page_does() {
    let (_dir, library) = library();
    let path = classroom_sheet(&library);
    let hub = Arc::new(LiveHub::new(hub_config()));
    let _sink = resume_sheet(&hub, &library, &path);
    let (image, _) = hub.page_image(0).unwrap();
    assert_eq!(image.rect, IMAGE_RECT);
    let background = Background {
        paper: Paper::Slate,
        ..Background::default()
    };
    let url =
        crate::live::page_background_url(hub.page_image(0), A5, background, PAPER_PX).unwrap();
    let drawn = decode_png(&png_of(&url)).unwrap();
    assert_eq!(drawn.width, PAPER_PX);
    let (x, y) = at(7_000, 4_000);
    assert!(near(rgb(&drawn, x, y), RED));
    assert!(paper_shows(&drawn, SLATE));

    // The next page has no image; a page that does not exist neither.
    hub.command(crate::live::model::LiveCommandKind::NextPage)
        .unwrap();
    assert!(hub.page_image(1).is_none());
    assert!(hub.page_image(7).is_none());
    let url = crate::live::page_background_url(None, A5, background, 512).unwrap();
    let plain = decode_png(&png_of(&url)).unwrap();
    assert!(near(rgb(&plain, 100, 100), SLATE));
}
