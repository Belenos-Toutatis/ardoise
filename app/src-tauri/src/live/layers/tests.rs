//! Tests of the layers in live mode (M14): what the layers zone asks, once checked; a move and a
//! merge as the editor does them; the session's two layers of a page (the tablet's and the
//! computer's) through every command and op; refusals and their codes; the hub once stopped. A
//! resumed sheet has its own in `resume/tests.rs`.

use ardoise_core::note::{Layer, Note};
use ardoise_core::settings::LiveButton;
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use serde_json::{Value, json};

use super::{
    CheckedLayerOp, LayerOp, MAX_LAYER_NAME, MAX_LAYERS, follow_merge, follow_move, merge_layer,
    move_layer,
};
use crate::library::blank_note;
use crate::library::tests::library;
use crate::live::edits::LiveEditError;
use crate::live::edits::tests::delete;
use crate::live::hub::{LiveError, LiveHub};
use crate::live::model::{LiveCommandKind, LiveEvent, LiveSessionState};
use crate::live::pen::tests::{T0, at_once, begin, end, points, running};
use crate::live::session::LiveSession;
use crate::live::tests::{config, fixed, options, pen, start, stroke, text_frame};

fn op(value: Value) -> LayerOp {
    serde_json::from_value(value).unwrap()
}

fn checked(value: Value) -> Result<CheckedLayerOp, LiveEditError> {
    op(value).checked()
}

fn layer(name: &str, visible: bool) -> Layer {
    Layer {
        name: name.to_owned(),
        visible,
    }
}

/// A stroke of layer `layer` starting at `x`.
fn on(layer: u32, x: f32) -> Stroke {
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 60.0,
        layer,
        points: vec![Point {
            x,
            y: 0.0,
            pressure: 0.5,
            t: 0,
        }],
    }
}

/// (layer, start) of every stroke of `note`, in list order.
fn ink(note: &Note) -> Vec<(u32, f32)> {
    note.strokes
        .iter()
        .map(|s| (s.layer, s.points[0].x))
        .collect()
}

// --- What an op is, once checked ---------------------------------------------------------------

#[test]
fn the_ops_are_read_as_live_ts_writes_them() {
    assert_eq!(checked(json!({ "kind": "add" })), Ok(CheckedLayerOp::Add));
    assert_eq!(
        checked(json!({ "kind": "rename", "layer": 1, "name": "  Correction " })),
        Ok(CheckedLayerOp::Rename {
            layer: 1,
            name: "Correction".into()
        })
    );
    assert_eq!(
        checked(json!({ "kind": "visible", "layer": 2, "visible": false })),
        Ok(CheckedLayerOp::Visible {
            layer: 2,
            visible: false
        })
    );
    assert_eq!(
        checked(json!({ "kind": "move", "from": 0, "to": 1 })),
        Ok(CheckedLayerOp::Move { from: 0, to: 1 })
    );
    assert_eq!(
        checked(json!({ "kind": "merge", "layer": 1 })),
        Ok(CheckedLayerOp::Merge { layer: 1 })
    );
    assert_eq!(
        checked(json!({ "kind": "computer", "layer": 3 })),
        Ok(CheckedLayerOp::Computer { layer: 3 })
    );
    assert_eq!(
        checked(json!({ "kind": "tablet", "layer": 0 })),
        Ok(CheckedLayerOp::Tablet { layer: 0 })
    );
    assert!(serde_json::from_value::<LayerOp>(json!({ "kind": "delete", "layer": 0 })).is_err());
    // Only choosing where a pen acts changes no note.
    assert!(!op(json!({ "kind": "computer", "layer": 3 })).changes_note());
    assert!(!op(json!({ "kind": "tablet", "layer": 3 })).changes_note());
    assert!(op(json!({ "kind": "add" })).changes_note());
    assert!(op(json!({ "kind": "visible", "layer": 0, "visible": true })).changes_note());
}

#[test]
fn numbers_that_are_no_index_and_bad_names_get_the_layers_code() {
    for bad in [json!(-1), json!(1.5), json!(5e9)] {
        for value in [
            json!({ "kind": "computer", "layer": bad }),
            json!({ "kind": "move", "from": 0, "to": bad }),
            json!({ "kind": "rename", "layer": bad, "name": "a" }),
        ] {
            let error = checked(value).unwrap_err();
            assert_eq!(error, LiveEditError::BadLayer);
            assert_eq!(error.code(), "liveLayerInvalid");
        }
    }
    let name = |name: String| checked(json!({ "kind": "rename", "layer": 0, "name": name }));
    for bad in [
        "".to_owned(),
        " \t\n ".to_owned(),
        "é".repeat(MAX_LAYER_NAME + 1),
    ] {
        let error = name(bad).unwrap_err();
        assert_eq!(error, LiveEditError::BadLayerName);
        assert_eq!(error.code(), "liveLayerInvalid");
    }
    // Characters, not bytes.
    assert!(name("é".repeat(MAX_LAYER_NAME)).is_ok());
    assert_eq!(LiveEditError::TooManyLayers.code(), "liveLayerInvalid");
}

#[test]
fn indices_follow_a_move_and_a_merge_as_in_the_editor() {
    // `ops.moveIndex` of the editor, on four layers.
    let moved = |from, to| (0..4).map(|i| follow_move(i, from, to)).collect::<Vec<_>>();
    assert_eq!(moved(0, 2), [2, 0, 1, 3]);
    assert_eq!(moved(3, 1), [0, 2, 3, 1]);
    assert_eq!(moved(1, 2), [0, 2, 1, 3]);
    // A merge of 2 into 1: 2 becomes 1, the ones above go down.
    let merged = (0..4).map(|i| follow_merge(i, 2)).collect::<Vec<_>>();
    assert_eq!(merged, [0, 1, 1, 2]);
}

#[test]
fn a_move_and_a_merge_change_the_page_as_the_editor_does() {
    let mut note = blank_note("", "Calque 1", 0);
    note.layers = vec![
        layer("Cours", true),
        layer("Réponses", false),
        layer("Correction", true),
    ];
    note.strokes = vec![on(0, 1.0), on(2, 2.0), on(1, 3.0), on(7, 4.0), on(0, 5.0)];

    // Move: places stay, layers follow; a stroke of a layer that does not exist keeps its number.
    let mut moved = note.clone();
    move_layer(&mut moved, 2, 0);
    let names: Vec<&str> = moved.layers.iter().map(|l| l.name.as_str()).collect();
    assert_eq!(names, ["Correction", "Cours", "Réponses"]);
    assert_eq!(
        ink(&moved),
        [(1, 1.0), (0, 2.0), (2, 3.0), (7, 4.0), (1, 5.0)]
    );

    // Merge of « Réponses » (hidden) into « Cours »: its strokes after all the others, the lower
    // layer keeps its name and shows (either showed); the strokes above go down, orphans too.
    let mut merged = note.clone();
    merge_layer(&mut merged, 1);
    assert_eq!(
        merged.layers,
        [layer("Cours", true), layer("Correction", true)]
    );
    assert_eq!(
        ink(&merged),
        [(0, 1.0), (1, 2.0), (6, 4.0), (0, 5.0), (0, 3.0)]
    );
    // Into a hidden layer: its ink shows.
    let mut into_hidden = note;
    into_hidden.layers.swap(0, 1);
    merge_layer(&mut into_hidden, 1);
    assert!(into_hidden.layers[0].visible);
    assert_eq!(into_hidden.layers[0].name, "Réponses");
}

/// The page objects follow their layer as the strokes do, as the editor's `ops.moveLayer` and
/// `ops.mergeDown` move them (`lib/editor/ops.ts`): a move keeps their order, a merge puts those of
/// the merged layer after the others, the ones above go down; their placement never changes.
#[test]
fn a_move_and_a_merge_take_the_page_objects_with_their_layer() {
    let mut note = blank_note("", "Calque 1", 0);
    note.layers = vec![
        layer("Cours", true),
        layer("Réponses", false),
        layer("Correction", true),
    ];
    note.strokes = vec![on(1, 1.0), on(2, 2.0)];
    note.objects = ["a", "b", "c", "d"]
        .iter()
        .zip([1, 0, 2, 0])
        .enumerate()
        .map(|(i, (id, layer))| text_frame(id, layer, 0, 100.0 * i as f32, 50.0))
        .collect();
    fn objects(note: &Note) -> Vec<(&str, u32)> {
        note.objects
            .iter()
            .map(|o| (o.id.as_str(), o.layer))
            .collect()
    }

    let mut moved = note.clone();
    move_layer(&mut moved, 2, 0);
    assert_eq!(objects(&moved), [("a", 2), ("b", 1), ("c", 0), ("d", 1)]);
    assert_eq!(ink(&moved), [(2, 1.0), (0, 2.0)]);

    let mut merged = note.clone();
    merge_layer(&mut merged, 1);
    assert_eq!(objects(&merged), [("b", 0), ("c", 1), ("d", 0), ("a", 0)]);
    assert_eq!(ink(&merged), [(1, 2.0), (0, 1.0)]);
    for object in merged.objects.iter().chain(&moved.objects) {
        let was = note.objects.iter().find(|o| o.id == object.id).unwrap();
        assert_eq!((object.x, object.y), (was.x, was.y));
    }
}

// --- The session's two layers ------------------------------------------------------------------

fn layers_of(session: &LiveSession) -> (u32, u32) {
    let snapshot = session.snapshot();
    assert_eq!(
        (snapshot.layer, snapshot.computer_layer),
        session.pages()[session.current()].pen_layers(),
    );
    (snapshot.layer, snapshot.computer_layer)
}

fn names(session: &LiveSession) -> Vec<String> {
    session.pages()[session.current()]
        .note
        .layers
        .iter()
        .map(|l| l.name.clone())
        .collect()
}

fn snapshots(out: &[LiveEvent]) -> usize {
    out.iter()
        .filter(|e| matches!(e, LiveEvent::Snapshot { .. }))
        .count()
}

#[test]
fn the_two_layers_at_the_start_on_a_new_page_and_with_the_tablet_s_button() {
    let mut session = running();
    // At the start: the computer on the tablet's layer.
    assert_eq!(layers_of(&session), (0, 0));
    let mut out = Vec::new();
    // « Nouveau calque » (the tablet's button, or `newLayer`): the tablet goes to it, the
    // computer stays.
    session.command(LiveCommandKind::NewLayer, LiveButton::NewPage, 0, &mut out);
    assert_eq!(layers_of(&session), (1, 0));
    session.command(LiveCommandKind::Button, LiveButton::NewLayer, 0, &mut out);
    assert_eq!(layers_of(&session), (2, 0));
    assert_eq!(names(&session), ["Calque 1", "Calque 2", "Calque 3"]);
    // A new page: both on its one layer.
    session.command(LiveCommandKind::NextPage, LiveButton::NewPage, 0, &mut out);
    assert_eq!(layers_of(&session), (0, 0));
    session.command(LiveCommandKind::NewPage, LiveButton::NewPage, 0, &mut out);
    assert_eq!(layers_of(&session), (0, 0));
    session.command(LiveCommandKind::Button, LiveButton::NewPage, 0, &mut out);
    assert_eq!(layers_of(&session), (0, 0));
    // A page left keeps its two layers.
    session.layer_op(CheckedLayerOp::Add, &mut out).unwrap();
    assert_eq!(layers_of(&session), (0, 1));
    for _ in 0..3 {
        session.command(
            LiveCommandKind::PreviousPage,
            LiveButton::NewPage,
            0,
            &mut out,
        );
    }
    assert_eq!(session.current(), 0);
    assert_eq!(layers_of(&session), (2, 0));
    for _ in 0..3 {
        session.command(LiveCommandKind::NextPage, LiveButton::NewPage, 0, &mut out);
    }
    assert_eq!(layers_of(&session), (0, 1));
}

#[test]
fn every_op_moves_or_keeps_the_two_layers() {
    let mut session = running();
    let mut out = Vec::new();
    // « Ajouter un calque »: the computer goes to it, the tablet stays; touched, a snapshot.
    let version = session.pages()[0].version;
    session.layer_op(CheckedLayerOp::Add, &mut out).unwrap();
    assert_eq!(snapshots(&out), 1);
    assert_eq!(layers_of(&session), (0, 1));
    assert_eq!(names(&session), ["Calque 1", "Calque 2"]);
    assert!(session.pages()[0].version > version);
    session.layer_op(CheckedLayerOp::Add, &mut out).unwrap();
    assert_eq!(layers_of(&session), (0, 2));

    // Choosing: the index; nothing to write.
    let version = session.pages()[0].version;
    out.clear();
    session
        .layer_op(CheckedLayerOp::Tablet { layer: 1 }, &mut out)
        .unwrap();
    session
        .layer_op(CheckedLayerOp::Computer { layer: 0 }, &mut out)
        .unwrap();
    assert_eq!(snapshots(&out), 2);
    assert_eq!(layers_of(&session), (1, 0));
    assert_eq!(session.pages()[0].version, version);

    // Rename, show and hide: neither moves.
    session
        .layer_op(
            CheckedLayerOp::Rename {
                layer: 1,
                name: "Correction".into(),
            },
            &mut out,
        )
        .unwrap();
    session
        .layer_op(
            CheckedLayerOp::Visible {
                layer: 1,
                visible: false,
            },
            &mut out,
        )
        .unwrap();
    assert_eq!(names(&session), ["Calque 1", "Correction", "Calque 3"]);
    assert!(!session.snapshot().page.layers[1].visible);
    assert_eq!(layers_of(&session), (1, 0));
    assert!(session.pages()[0].version > version);

    // Move: both follow their layer. Layers [C1, Correction, C3]; tablet on Correction (1),
    // computer on C1 (0). Correction to the top: [C1, C3, Correction].
    session
        .layer_op(CheckedLayerOp::Move { from: 1, to: 2 }, &mut out)
        .unwrap();
    assert_eq!(names(&session), ["Calque 1", "Calque 3", "Correction"]);
    assert_eq!(layers_of(&session), (2, 0));
    // C1 to the top: [C3, Correction, C1].
    session
        .layer_op(CheckedLayerOp::Move { from: 0, to: 2 }, &mut out)
        .unwrap();
    assert_eq!(names(&session), ["Calque 3", "Correction", "Calque 1"]);
    assert_eq!(layers_of(&session), (1, 2));

    // Merge: C1 into Correction: the computer goes down with it; then the tablet's layer goes
    // into C3.
    session
        .layer_op(CheckedLayerOp::Merge { layer: 2 }, &mut out)
        .unwrap();
    assert_eq!(names(&session), ["Calque 3", "Correction"]);
    assert_eq!(layers_of(&session), (1, 1));
    session
        .layer_op(CheckedLayerOp::Merge { layer: 1 }, &mut out)
        .unwrap();
    assert_eq!(names(&session), ["Calque 3"]);
    assert_eq!(layers_of(&session), (0, 0));
}

#[test]
fn an_op_that_changes_nothing_sends_nothing() {
    let mut session = running();
    let mut out = Vec::new();
    session.layer_op(CheckedLayerOp::Add, &mut out).unwrap();
    let version = session.pages()[0].version;
    out.clear();
    for same in [
        CheckedLayerOp::Rename {
            layer: 1,
            name: "Calque 2".into(),
        },
        CheckedLayerOp::Visible {
            layer: 0,
            visible: true,
        },
        CheckedLayerOp::Computer { layer: 1 },
        CheckedLayerOp::Tablet { layer: 0 },
    ] {
        session.layer_op(same, &mut out).unwrap();
    }
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(session.pages()[0].version, version);
}

#[test]
fn a_merge_puts_the_upper_ink_above_drops_the_history_and_changes_the_revision() {
    let mut session = running();
    let mut out = Vec::new();
    // Layer 0: strokes at 1000 and 3000; layer 1: 2000 (the tablet's, after « nouveau calque »)
    // and a computer stroke at 4000.
    session.records(&[pen(1000, 100, 800), pen(1000, 100, 0)], T0, &mut out);
    session.command(LiveCommandKind::NewLayer, LiveButton::NewPage, 0, &mut out);
    session.records(
        &[pen(2000, 100, 800), pen(2000, 100, 0)],
        T0 + 100.0,
        &mut out,
    );
    session
        .layer_op(CheckedLayerOp::Tablet { layer: 0 }, &mut out)
        .unwrap();
    session.records(
        &[pen(3000, 100, 800), pen(3000, 100, 0)],
        T0 + 200.0,
        &mut out,
    );
    session
        .layer_op(CheckedLayerOp::Computer { layer: 1 }, &mut out)
        .unwrap();
    session.computer_stroke(1, 4000.0, &mut out);
    let page = &session.pages()[0];
    assert_eq!(
        ink(&page.note),
        [(0, 1000.0), (1, 2000.0), (0, 3000.0), (1, 4000.0)]
    );
    let revision = session.snapshot().revision;
    assert!(session.snapshot().can_undo);

    out.clear();
    session
        .layer_op(CheckedLayerOp::Merge { layer: 1 }, &mut out)
        .unwrap();
    assert_eq!(snapshots(&out), 1);
    let snapshot = session.snapshot();
    assert_eq!(
        ink(&snapshot.page),
        [(0, 1000.0), (0, 3000.0), (0, 2000.0), (0, 4000.0)]
    );
    // Places moved: a selection is stale, and the steps would name other strokes.
    assert_ne!(snapshot.revision, revision);
    assert!(!snapshot.can_undo && !snapshot.can_redo);
    out.clear();
    assert!(!session.undo_edit(0, None, &mut out).unwrap());
    assert_eq!(session.snapshot().page.strokes.len(), 4);
    let refused = delete(&[0]).checked().unwrap();
    assert_eq!(
        session.edit_strokes(0, revision, refused, &mut out),
        Err(LiveEditError::Stale)
    );
}

#[test]
fn a_move_takes_along_what_can_come_back_and_the_strokes_in_progress() {
    // What can come back: a stroke deleted (kept by the undo stack) and a computer stroke undone
    // (kept by the redo stack), both on layer 1.
    let mut session = running();
    let mut out = Vec::new();
    session.layer_op(CheckedLayerOp::Add, &mut out).unwrap();
    session.computer_stroke(1, 100.0, &mut out);
    let revision = session.snapshot().revision;
    session
        .edit_strokes(0, revision, delete(&[0]).checked().unwrap(), &mut out)
        .unwrap();
    session.computer_stroke(2, 200.0, &mut out);
    assert!(session.undo_edit(0, None, &mut out).unwrap());
    assert!(session.pages()[0].note.strokes.is_empty());
    let revision = session.snapshot().revision;

    // Layer 1 to the bottom: [Calque 2, Calque 1].
    session
        .layer_op(CheckedLayerOp::Move { from: 1, to: 0 }, &mut out)
        .unwrap();
    assert_eq!(names(&session), ["Calque 2", "Calque 1"]);
    assert_eq!(layers_of(&session), (1, 0));
    // Places are kept: the revision and the history too.
    assert_eq!(session.snapshot().revision, revision);
    assert!(session.snapshot().can_undo && session.snapshot().can_redo);
    // The redo gives the computer's stroke back on its layer, now 0...
    assert!(session.redo_edit(0, None, &mut out).unwrap());
    assert_eq!(ink(&session.pages()[0].note), [(0, 200.0)]);
    // ... and the undo of the delete gives back the deleted one on it too.
    assert!(session.undo_edit(0, None, &mut out).unwrap());
    assert!(session.undo_edit(0, None, &mut out).unwrap());
    assert_eq!(ink(&session.pages()[0].note), [(0, 100.0)]);

    // The strokes in progress: the computer's on layer 0 (Calque 2), the tablet's on 1 (Calque 1).
    session
        .computer_call(begin(3, 0, at_once(&[300.0])), T0, &mut out)
        .unwrap();
    session.records(&[pen(400, 100, 800)], T0, &mut out);
    out.clear();
    session
        .layer_op(CheckedLayerOp::Move { from: 0, to: 1 }, &mut out)
        .unwrap();
    assert_eq!(names(&session), ["Calque 1", "Calque 2"]);
    assert_eq!(layers_of(&session), (0, 1));
    // No stroke in progress ended; the snapshot is followed by both, on their layers' new place.
    assert!(!out.iter().any(|e| matches!(e, LiveEvent::StrokeEnd { .. })));
    let layers: Vec<u32> = out
        .iter()
        .filter_map(|e| match e {
            LiveEvent::StrokeStart { stroke, .. } => Some(stroke.layer),
            _ => None,
        })
        .collect();
    assert_eq!(layers, [0, 1]);
    session
        .computer_call(end(3, Vec::new()), T0, &mut out)
        .unwrap();
    session.records(&[pen(400, 100, 0)], T0 + 5.0, &mut out);
    assert_eq!(
        ink(&session.pages()[0].note),
        [(1, 100.0), (1, 300.0), (0, 400.0)]
    );
}

#[test]
fn refusals_change_nothing_and_have_their_codes() {
    let mut session = running();
    let mut out = Vec::new();
    session.layer_op(CheckedLayerOp::Add, &mut out).unwrap();
    let before = session.snapshot();
    out.clear();
    let refused = |session: &mut LiveSession, op: CheckedLayerOp| {
        let mut out = Vec::new();
        let error = session.layer_op(op, &mut out).unwrap_err();
        assert!(out.is_empty());
        error
    };
    for op in [
        CheckedLayerOp::Rename {
            layer: 2,
            name: "a".into(),
        },
        CheckedLayerOp::Visible {
            layer: 9,
            visible: false,
        },
        CheckedLayerOp::Move { from: 0, to: 2 },
        CheckedLayerOp::Move { from: 1, to: 1 },
        CheckedLayerOp::Merge { layer: 0 },
        CheckedLayerOp::Merge { layer: 2 },
        CheckedLayerOp::Computer { layer: 2 },
        CheckedLayerOp::Tablet { layer: 5 },
    ] {
        let error = refused(&mut session, op);
        assert_eq!(
            (error.clone(), error.code()),
            (LiveEditError::BadLayer, "liveLayerInvalid")
        );
    }
    // Another page on screen.
    let error = session
        .layers(1, CheckedLayerOp::Add, &mut out)
        .unwrap_err();
    assert_eq!(
        (error.clone(), error.code()),
        (LiveEditError::Stale, "liveEditStale")
    );
    assert_eq!(session.snapshot(), before);

    // As many layers as a page may get: « Ajouter » is refused, the tablet's button does nothing.
    while session.pages()[0].note.layers.len() < MAX_LAYERS {
        session.layer_op(CheckedLayerOp::Add, &mut out).unwrap();
    }
    let error = refused(&mut session, CheckedLayerOp::Add);
    assert_eq!(
        (error.clone(), error.code()),
        (LiveEditError::TooManyLayers, "liveLayerInvalid")
    );
    out.clear();
    session.command(LiveCommandKind::NewLayer, LiveButton::NewPage, 0, &mut out);
    assert!(out.is_empty());
    assert_eq!(session.pages()[0].note.layers.len(), MAX_LAYERS);
}

#[test]
fn the_tablet_may_write_on_a_hidden_layer_unseen() {
    let mut session = running();
    let mut out = Vec::new();
    session.layer_op(CheckedLayerOp::Add, &mut out).unwrap();
    session
        .layer_op(
            CheckedLayerOp::Visible {
                layer: 0,
                visible: false,
            },
            &mut out,
        )
        .unwrap();
    // Hiding the tablet's layer does not move the tablet.
    assert_eq!(layers_of(&session), (0, 1));
    out.clear();
    session.records(&[pen(10, 10, 800), pen(20, 10, 800)], T0, &mut out);
    session.records(&[pen(30, 10, 0)], T0 + 10.0, &mut out);
    assert!(matches!(
        out[0],
        LiveEvent::StrokeStart { hidden: true, .. }
    ));
    assert!(
        out.iter()
            .any(|e| matches!(e, LiveEvent::StrokeEnd { hidden: true, .. }))
    );
    // Its ink is recorded there as usual.
    assert_eq!(ink(&session.pages()[0].note), [(0, 10.0)]);
    // Shown again mid-stroke: the snapshot is followed by the stroke, shown.
    session.records(&[pen(50, 10, 800)], T0 + 100.0, &mut out);
    out.clear();
    session
        .layer_op(
            CheckedLayerOp::Visible {
                layer: 0,
                visible: true,
            },
            &mut out,
        )
        .unwrap();
    assert!(matches!(
        &out[..],
        [
            LiveEvent::Snapshot { .. },
            LiveEvent::StrokeStart { hidden: false, .. }
        ]
    ));
}

// --- The hub -----------------------------------------------------------------------------------

#[test]
fn the_hub_refuses_ops_before_the_start_and_once_stopped_all_but_choosing_a_layer() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let add = || op(json!({ "kind": "add" }));
    assert!(matches!(hub.layers(0, add()), Err(LiveError::NotRunning)));
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    // A bad op is refused before the lock, with its code.
    let code = |error: LiveError| error.command_error().parts().0;
    assert_eq!(
        code(
            hub.layers(0, op(json!({ "kind": "merge", "layer": 0.5 })))
                .unwrap_err()
        ),
        "liveLayerInvalid"
    );
    assert_eq!(
        code(
            hub.layers(0, op(json!({ "kind": "merge", "layer": 0 })))
                .unwrap_err()
        ),
        "liveLayerInvalid"
    );
    assert_eq!(code(hub.layers(1, add()).unwrap_err()), "liveEditStale");
    hub.layers(0, add()).unwrap();
    assert_eq!(hub.snapshot().computer_layer, 1);
    stroke(&mut sink, 1000, 3);

    hub.stop().unwrap();
    assert_eq!(hub.snapshot().state, LiveSessionState::Idle);
    for refused in [
        add(),
        op(json!({ "kind": "rename", "layer": 0, "name": "a" })),
        op(json!({ "kind": "visible", "layer": 0, "visible": false })),
        op(json!({ "kind": "move", "from": 0, "to": 1 })),
        op(json!({ "kind": "merge", "layer": 1 })),
    ] {
        assert_eq!(code(hub.layers(0, refused).unwrap_err()), "liveNotRunning");
    }
    // Choosing where a pen acts changes no note: allowed, as paging is.
    hub.layers(0, op(json!({ "kind": "computer", "layer": 0 })))
        .unwrap();
    hub.layers(0, op(json!({ "kind": "tablet", "layer": 1 })))
        .unwrap();
    let snapshot = hub.snapshot();
    assert_eq!((snapshot.layer, snapshot.computer_layer), (1, 0));

    // And the computer's pen: a begin is refused, the rest says the stroke does not go on.
    assert_eq!(
        code(hub.computer_pen(begin(1, 0, at_once(&[1.0]))).unwrap_err()),
        "liveNotRunning"
    );
    assert!(!hub.computer_pen(points(1, at_once(&[1.0]))).unwrap());
}
