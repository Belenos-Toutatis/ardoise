//! Tests of touching up a page during live mode: the session's rules, then the hub (order of the
//! events, what is written, the simulated pen writing meanwhile). The resumed sheet has its own in
//! `resume/tests.rs`, the viewers in `share/tests.rs`.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ardoise_core::note::{A4_LANDSCAPE, Layer};
use ardoise_core::settings::{LiveButton, Orientation};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use serde_json::{Value, json};

use super::{
    LiveEditError, MAX_EDIT_STROKES, MAX_UNDO_BYTES, MAX_UNDO_STEPS, Prepared, StrokesEdit,
    grouped, insert_at, remove_at,
};
use crate::library::tests::library;
use crate::live::hub::{BatchSink, HubConfig, LiveError, LiveHub};
use crate::live::model::{
    LiveBatch, LiveCommandKind, LiveEvent, LiveSessionState, LiveStyle, LiveViewState,
    ViewerIndices,
};
use crate::live::session::LiveSession;
use crate::live::tests::{
    A5, Window, config, fixed, notes_in, options, pen, send, start, stream_simulated_live, stroke,
    wait_until,
};

const T0: f64 = 1_792_843_200_000.0;
const RED: Rgba = Rgba::rgb(200, 0, 0);
const BLUE: Rgba = Rgba::rgb(0, 0, 200);

fn edit_json(value: Value) -> StrokesEdit {
    serde_json::from_value(value).unwrap()
}

pub(crate) fn delete(indices: &[u32]) -> StrokesEdit {
    edit_json(json!({ "kind": "delete", "indices": indices }))
}

pub(crate) fn recolour(indices: &[u32], color: Rgba) -> StrokesEdit {
    edit_json(json!({
        "kind": "recolour",
        "indices": indices,
        "color": { "r": color.r, "g": color.g, "b": color.b, "a": color.a },
    }))
}

fn running() -> LiveSession {
    let mut session = LiveSession::new(
        1,
        A4_LANDSCAPE,
        LiveStyle::default(),
        LiveViewState::whole(A4_LANDSCAPE),
        true,
        T0 as i64 - 60_000,
    );
    let mut out = Vec::new();
    session.set_state(LiveSessionState::Waiting, &mut out);
    session.started(29_700, 21_600, Orientation::Landscape, &mut out);
    session
}

/// The tablet writes a finished stroke of two points, which starts at `x`.
fn write_to(session: &mut LiveSession, x: u16, out: &mut Vec<LiveEvent>) {
    session.records(&[pen(x, 100, 500), pen(x + 10, 100, 500)], T0, out);
    session.records(&[pen(x + 20, 100, 0)], T0 + 5.0, out);
}

fn write(session: &mut LiveSession, x: u16) {
    write_to(session, x, &mut Vec::new());
}

/// A session with `count` finished strokes, starting at 1000, 2000...
fn with_strokes(count: u16) -> LiveSession {
    let mut session = running();
    for i in 1..=count {
        write(&mut session, 1000 * i);
    }
    session
}

/// Where each stroke of the page starts: says which strokes it holds, in which order.
fn starts(session: &LiveSession) -> Vec<f32> {
    let page = &session.pages()[session.current()];
    page.note.strokes.iter().map(|s| s.points[0].x).collect()
}

fn colours(session: &LiveSession) -> Vec<Rgba> {
    let page = &session.pages()[session.current()];
    page.note.strokes.iter().map(|s| s.color).collect()
}

fn revision(session: &LiveSession) -> u64 {
    session.snapshot().revision
}

impl LiveSession {
    fn edit(&mut self, edit: StrokesEdit, out: &mut Vec<LiveEvent>) -> Result<u64, LiveEditError> {
        let (page, revision) = (self.current(), self.snapshot().revision);
        self.edit_strokes(page, revision, edit.checked()?, out)
    }

    fn undo(&mut self, out: &mut Vec<LiveEvent>) -> bool {
        self.undo_edit(self.current(), None, out).unwrap()
    }

    fn redo(&mut self, out: &mut Vec<LiveEvent>) -> bool {
        self.redo_edit(self.current(), out).unwrap()
    }
}

/// What a window keeps, updated from the events as `LiveState` does: the finished strokes and the
/// stroke in progress. `viewer`: reads the viewer's numbering, as a viewer's `LiveState` does.
#[derive(Default)]
struct Follower {
    page: usize,
    strokes: Vec<Stroke>,
    current: Option<Stroke>,
    viewer: bool,
}

impl Follower {
    fn apply(&mut self, event: &LiveEvent) {
        match event {
            LiveEvent::Snapshot { snapshot } => {
                self.page = snapshot.page_index;
                self.strokes = snapshot.page.strokes.clone();
                if self.viewer {
                    let layers = &snapshot.page.layers;
                    self.strokes
                        .retain(|stroke| crate::share::page::is_drawn(layers, stroke));
                }
                self.current = None;
            }
            LiveEvent::StrokeStart { stroke, .. } => {
                self.current = Some(Stroke {
                    tool: stroke.tool,
                    color: stroke.color,
                    width: stroke.width,
                    layer: stroke.layer,
                    points: stroke.points.iter().map(point_of).collect(),
                });
            }
            LiveEvent::Points { points, .. } => {
                let current = self.current.as_mut().expect("points without a stroke");
                current.points.extend(points.iter().map(point_of));
            }
            LiveEvent::StrokeEnd { .. } => {
                self.strokes
                    .push(self.current.take().expect("an end without a stroke"));
            }
            LiveEvent::StrokesEdited {
                page_index,
                removed,
                inserted,
                recoloured,
                viewers,
                ..
            } => {
                assert_eq!(*page_index, self.page);
                let shared = viewers.as_deref().filter(|_| self.viewer);
                let removed = shared.map_or(removed, |viewers| &viewers.removed);
                for &index in removed.iter().rev() {
                    self.strokes.remove(index as usize);
                }
                for (i, placed) in inserted.iter().enumerate() {
                    let index = shared.map_or(placed.index, |viewers| viewers.inserted[i]);
                    self.strokes.insert(index as usize, placed.stroke.clone());
                }
                let recoloured = shared.map_or(recoloured, |viewers| &viewers.recoloured);
                for group in recoloured {
                    for &index in &group.indices {
                        self.strokes[index as usize].color = group.color;
                    }
                }
            }
            _ => {}
        }
    }

    fn follow(&mut self, events: &[LiveEvent]) {
        for event in events {
            self.apply(event);
        }
    }
}

fn point_of(p: &crate::live::model::LivePoint) -> Point {
    Point {
        x: p.x,
        y: p.y,
        pressure: p.p,
        t: p.t,
    }
}

// --- The session -----------------------------------------------------------------------------------

#[test]
fn deleting_takes_the_strokes_out_and_says_which() {
    let mut session = with_strokes(4);
    let before = revision(&session);
    let version = session.pages()[0].version;
    let mut out = Vec::new();
    // In any order: the event gives them ascending.
    let after = session.edit(delete(&[2, 0]), &mut out).unwrap();
    assert_eq!(starts(&session), [2000.0, 4000.0]);
    assert_ne!(after, before);
    assert_eq!(after, revision(&session));
    assert!(session.pages()[0].version > version);
    assert!(session.pages()[0].needs_write());
    let [
        LiveEvent::StrokesEdited {
            page_index,
            revision: told,
            removed,
            inserted,
            recoloured,
            can_undo,
            can_redo,
            viewers,
        },
    ] = &out[..]
    else {
        panic!("{out:?}");
    };
    assert_eq!((*page_index, *told), (0, after));
    assert_eq!(removed, &[0, 2]);
    assert!(inserted.is_empty() && recoloured.is_empty());
    assert!(*can_undo && !*can_redo);
    assert!(
        viewers.is_none(),
        "no hidden layer: the viewers are numbered alike"
    );
    assert!(session.snapshot().can_undo);
}

#[test]
fn recolouring_keeps_the_places_and_the_revision() {
    let mut session = with_strokes(3);
    let before = revision(&session);
    let mut out = Vec::new();
    assert_eq!(session.edit(recolour(&[2, 0], RED), &mut out), Ok(before));
    assert_eq!(colours(&session), [RED, Rgba::BLACK, RED]);
    let [
        LiveEvent::StrokesEdited {
            removed,
            inserted,
            recoloured,
            revision: told,
            ..
        },
    ] = &out[..]
    else {
        panic!("{out:?}");
    };
    assert_eq!(*told, before);
    assert!(removed.is_empty() && inserted.is_empty());
    assert_eq!(recoloured.len(), 1);
    assert_eq!(
        (recoloured[0].color, &recoloured[0].indices[..]),
        (RED, &[0, 2][..])
    );
    // Two clicks in a row on the swatches: the second one is not stale.
    out.clear();
    assert_eq!(session.edit(recolour(&[0], BLUE), &mut out), Ok(before));
    assert_eq!(colours(&session)[0], BLUE);
}

#[test]
fn undo_and_redo_give_back_exactly_what_was_there() {
    let mut session = with_strokes(4);
    let original = session.pages()[0].note.strokes.clone();
    let start = revision(&session);
    let mut out = Vec::new();

    // Strokes of different colours take one colour; the undo gives each its own back.
    session.edit(recolour(&[0], RED), &mut out).unwrap();
    session.edit(recolour(&[1], BLUE), &mut out).unwrap();
    let coloured = session.pages()[0].note.strokes.clone();
    session
        .edit(recolour(&[0, 1, 2], Rgba::WHITE), &mut out)
        .unwrap();
    assert_eq!(colours(&session)[..3], [Rgba::WHITE; 3]);
    assert_eq!(revision(&session), start, "colours move no index");
    out.clear();
    assert!(session.undo(&mut out));
    assert_eq!(session.pages()[0].note.strokes, coloured);
    let [LiveEvent::StrokesEdited { recoloured, .. }] = &out[..] else {
        panic!("{out:?}");
    };
    assert_eq!(recoloured.len(), 3, "one entry per colour given back");
    out.clear();
    assert!(session.redo(&mut out));
    assert_eq!(colours(&session)[..3], [Rgba::WHITE; 3]);
    assert!(session.undo(&mut out));

    // A delete, undone and redone: each step moves the indices, so each changes the revision.
    let mut seen = vec![revision(&session)];
    session.edit(delete(&[1, 3]), &mut out).unwrap();
    seen.push(revision(&session));
    let deleted = session.pages()[0].note.strokes.clone();
    out.clear();
    assert!(session.undo(&mut out));
    seen.push(revision(&session));
    assert_eq!(session.pages()[0].note.strokes, coloured);
    let [
        LiveEvent::StrokesEdited {
            inserted, removed, ..
        },
    ] = &out[..]
    else {
        panic!("{out:?}");
    };
    assert!(removed.is_empty());
    let places: Vec<u32> = inserted.iter().map(|placed| placed.index).collect();
    assert_eq!(places, [1, 3]);
    assert_eq!(inserted[0].stroke, coloured[1]);
    assert!(session.redo(&mut out));
    seen.push(revision(&session));
    assert_eq!(session.pages()[0].note.strokes, deleted);
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 4, "a revision is never given twice");

    // All the way back, and nothing more.
    while session.undo(&mut out) {}
    assert_eq!(session.pages()[0].note.strokes, original);
    assert!(!session.snapshot().can_undo);
    assert!(session.snapshot().can_redo);
    out.clear();
    assert!(!session.undo(&mut out));
    assert!(out.is_empty(), "nothing to undo: no event");
    while session.redo(&mut out) {}
    assert_eq!(session.pages()[0].note.strokes, deleted);
    assert!(!session.snapshot().can_redo);
    out.clear();
    assert!(!session.redo(&mut out));
    assert!(out.is_empty());
}

#[test]
fn a_window_that_follows_the_events_has_the_list_of_the_page() {
    let mut session = with_strokes(5);
    let mut window = Follower::default();
    window.follow(&[LiveEvent::Snapshot {
        snapshot: Arc::new(session.snapshot()),
    }]);
    let mut out = Vec::new();
    session.edit(delete(&[0, 2, 4]), &mut out).unwrap();
    write_to(&mut session, 7000, &mut out);
    session.edit(recolour(&[0, 2], RED), &mut out).unwrap();
    session.undo(&mut out);
    session.undo(&mut out);
    write_to(&mut session, 8000, &mut out);
    session.redo(&mut out);
    window.follow(&out);
    assert_eq!(window.strokes, session.pages()[0].note.strokes);
    assert_eq!(starts(&session), [2000.0, 4000.0, 7000.0, 8000.0]);
}

#[test]
fn a_stale_page_or_revision_is_refused_and_changes_nothing() {
    let mut session = with_strokes(3);
    let revision_at_start = revision(&session);
    let mut out = Vec::new();
    session.edit(delete(&[0]), &mut out).unwrap();
    let kept = session.pages()[0].note.strokes.clone();
    let version = session.pages()[0].version;
    out.clear();
    // Chosen before the delete: index 1 is another stroke now.
    let stale = delete(&[1]).checked().unwrap();
    assert_eq!(
        session.edit_strokes(0, revision_at_start, stale.clone(), &mut out),
        Err(LiveEditError::Stale)
    );
    // Chosen on another page.
    let now = revision(&session);
    assert_eq!(
        session.edit_strokes(1, now, stale.clone(), &mut out),
        Err(LiveEditError::Stale)
    );
    assert_eq!(session.pages()[0].note.strokes, kept);
    assert_eq!(session.pages()[0].version, version);
    assert!(out.is_empty());
    assert_eq!(LiveEditError::Stale.code(), "liveEditStale");

    // A page that was left: its selection is stale, whatever its number was.
    session.command(LiveCommandKind::NewPage, LiveButton::NewPage, 0, &mut out);
    assert_eq!(
        session.edit_strokes(0, now, stale, &mut out),
        Err(LiveEditError::Stale)
    );
    assert_eq!(
        session.undo_edit(0, None, &mut out).unwrap_err(),
        LiveEditError::Stale
    );
    assert_eq!(
        session.redo_edit(0, &mut out).unwrap_err(),
        LiveEditError::Stale
    );
}

#[test]
fn bad_lists_and_colours_are_refused_before_anything_changes() {
    let refused = |value: Value| edit_json(value).checked().unwrap_err();
    let color = json!({ "r": 1, "g": 2, "b": 3, "a": 255 });
    assert_eq!(
        refused(json!({ "kind": "delete", "indices": [] })),
        LiveEditError::Empty
    );
    assert_eq!(
        refused(json!({ "kind": "recolour", "indices": [], "color": color })),
        LiveEditError::Empty
    );
    for index in [json!(-1), json!(1.5), json!(5e9)] {
        assert_eq!(
            refused(json!({ "kind": "delete", "indices": [0, index] })),
            LiveEditError::NotAnIndex
        );
    }
    assert_eq!(
        refused(json!({ "kind": "delete", "indices": [3, 1, 3] })),
        LiveEditError::Duplicate { index: 3 }
    );
    let huge: Vec<u32> = (0..=MAX_EDIT_STROKES as u32).collect();
    assert_eq!(
        delete(&huge).checked().unwrap_err(),
        LiveEditError::TooMany {
            count: MAX_EDIT_STROKES + 1
        }
    );
    for bad in [
        json!({ "r": 300, "g": 2, "b": 3, "a": 255 }),
        json!({ "r": 1, "g": -2, "b": 3, "a": 255 }),
        json!({ "r": 1, "g": 2, "b": 3.5, "a": 255 }),
        // Fully transparent: the ink would be gone without having been deleted.
        json!({ "r": 1, "g": 2, "b": 3, "a": 0 }),
    ] {
        assert_eq!(
            refused(json!({ "kind": "recolour", "indices": [0], "color": bad })),
            LiveEditError::BadColor
        );
    }
    // Other shapes do not even reach the session.
    for value in [
        json!({ "kind": "move", "indices": [0] }),
        json!({ "kind": "delete" }),
        json!({ "kind": "recolour", "indices": [0] }),
        json!({ "kind": "delete", "indices": [null] }),
    ] {
        assert!(serde_json::from_value::<StrokesEdit>(value).is_err());
    }
    // A valid colour keeps its alpha; a highlighter's can be translucent.
    let checked = edit_json(json!({
        "kind": "recolour", "indices": [4, 2], "color": { "r": 9, "g": 8, "b": 7, "a": 128 }
    }))
    .checked()
    .unwrap();
    assert_eq!(checked.indices, [2, 4]);

    // What depends on the page: out of range, and refused as a whole.
    let mut session = with_strokes(2);
    let mut out = Vec::new();
    let error = session.edit(delete(&[0, 2]), &mut out).unwrap_err();
    assert_eq!(
        error,
        LiveEditError::OutOfRange {
            index: 2,
            strokes: 2
        }
    );
    assert_eq!(error.code(), "liveEditInvalid");
    assert_eq!(starts(&session), [1000.0, 2000.0]);
    assert!(out.is_empty());
    assert!(!session.snapshot().can_undo);
}

#[test]
fn the_tablets_writing_keeps_the_revision_so_a_selection_stays_valid() {
    let mut session = with_strokes(3);
    let chosen_at = revision(&session);
    // The window chose stroke 1. The tablet goes on writing; a new layer, a page turn change no
    // index either.
    write(&mut session, 7000);
    write(&mut session, 8000);
    let mut out = Vec::new();
    session.command(LiveCommandKind::NewLayer, LiveButton::NewPage, 0, &mut out);
    session.reorient(29_700, 21_600, Orientation::LandscapeFlipped, &mut out);
    assert_eq!(revision(&session), chosen_at);
    session.reorient(29_700, 21_600, Orientation::Landscape, &mut out);
    let edit = delete(&[1]).checked().unwrap();
    session.edit_strokes(0, chosen_at, edit, &mut out).unwrap();
    assert_eq!(starts(&session), [1000.0, 3000.0, 7000.0, 8000.0]);
}

#[test]
fn an_edit_that_changes_nothing_is_no_step() {
    let mut session = with_strokes(2);
    let version = session.pages()[0].version;
    let before = revision(&session);
    let mut out = Vec::new();
    assert_eq!(
        session.edit(recolour(&[0, 1], Rgba::BLACK), &mut out),
        Ok(before)
    );
    assert!(out.is_empty());
    assert_eq!(session.pages()[0].version, version);
    assert!(!session.snapshot().can_undo);
}

#[test]
fn a_stroke_written_after_a_delete_survives_its_undo_and_stays_after() {
    let mut session = with_strokes(3);
    let mut out = Vec::new();
    session.edit(delete(&[0, 1]), &mut out).unwrap();
    write(&mut session, 7000);
    assert_eq!(starts(&session), [3000.0, 7000.0]);
    assert!(session.undo(&mut out));
    assert_eq!(starts(&session), [1000.0, 2000.0, 3000.0, 7000.0]);
    // Written between the undo and the redo: the redo takes the same strokes, and only them.
    write(&mut session, 8000);
    assert!(session.redo(&mut out));
    assert_eq!(starts(&session), [3000.0, 7000.0, 8000.0]);
    assert!(session.undo(&mut out));
    assert_eq!(starts(&session), [1000.0, 2000.0, 3000.0, 7000.0, 8000.0]);

    // Several edits, written in between: undone in reverse order, each at its own places.
    session.edit(delete(&[2]), &mut out).unwrap();
    write(&mut session, 9000);
    session.edit(recolour(&[0, 4], RED), &mut out).unwrap();
    session.edit(delete(&[0, 1]), &mut out).unwrap();
    write(&mut session, 9500);
    assert_eq!(starts(&session), [7000.0, 8000.0, 9000.0, 9500.0]);
    assert!(session.undo(&mut out));
    assert_eq!(
        starts(&session),
        [1000.0, 2000.0, 7000.0, 8000.0, 9000.0, 9500.0]
    );
    assert_eq!(colours(&session)[0], RED);
    assert_eq!(colours(&session)[4], RED, "the stroke written in between");
    assert!(session.undo(&mut out));
    assert_eq!(colours(&session), [Rgba::BLACK; 6]);
    assert!(session.undo(&mut out));
    assert_eq!(
        starts(&session),
        [1000.0, 2000.0, 3000.0, 7000.0, 8000.0, 9000.0, 9500.0]
    );
}

#[test]
fn the_tablets_writing_empties_no_redo_step_but_a_new_edit_does() {
    let mut session = with_strokes(3);
    let mut out = Vec::new();
    session.edit(delete(&[0]), &mut out).unwrap();
    session.undo(&mut out);
    write(&mut session, 7000);
    assert!(session.snapshot().can_redo);
    session.edit(recolour(&[1], RED), &mut out).unwrap();
    assert!(!session.snapshot().can_redo);
    out.clear();
    assert!(!session.redo(&mut out));
}

#[test]
fn the_stroke_in_progress_is_untouched_and_joins_at_the_tail() {
    let mut session = with_strokes(2);
    let mut out = Vec::new();
    session.records(&[pen(5000, 100, 500)], T0 + 50.0, &mut out);
    let before = revision(&session);
    // Not in the list: an index for it does not exist yet.
    assert_eq!(
        session.edit(delete(&[2]), &mut out).unwrap_err(),
        LiveEditError::OutOfRange {
            index: 2,
            strokes: 2
        }
    );
    out.clear();
    session.edit(delete(&[0]), &mut out).unwrap();
    session.edit(recolour(&[0], RED), &mut out).unwrap();
    session.undo(&mut out);
    session.redo(&mut out);
    session.undo(&mut out);
    session.undo(&mut out);
    assert_ne!(revision(&session), before);
    assert!(
        out.iter().all(|e| !matches!(e, LiveEvent::Snapshot { .. })),
        "an edit is no snapshot"
    );
    assert!(session.in_progress().is_some(), "still in progress");
    out.clear();
    session.records(&[pen(5010, 100, 500)], T0 + 55.0, &mut out);
    assert!(matches!(&out[0], LiveEvent::Points { points, .. } if points.len() == 1));
    session.records(&[pen(5020, 100, 0)], T0 + 60.0, &mut out);
    assert_eq!(starts(&session), [1000.0, 2000.0, 5000.0]);
    assert_eq!(session.pages()[0].note.strokes[2].points.len(), 2);
    assert_eq!(
        colours(&session)[2],
        Rgba::BLACK,
        "written with its own ink"
    );
}

#[test]
fn clearing_the_page_and_its_restoration_are_steps_of_the_history() {
    let mut session = with_strokes(2);
    let before = revision(&session);
    let mut out = Vec::new();
    // Still a snapshot: it takes the stroke in progress too.
    session.records(&[pen(5000, 100, 500)], T0 + 50.0, &mut out);
    out.clear();
    session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
    let [LiveEvent::Snapshot { snapshot }] = &out[..] else {
        panic!("{out:?}");
    };
    assert!(snapshot.page.strokes.is_empty());
    assert!(snapshot.can_undo);
    assert_ne!(snapshot.revision, before);
    assert!(session.in_progress().is_none());

    // Clearing a page that is empty: no step, the same revision.
    let cleared = revision(&session);
    session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
    assert_eq!(revision(&session), cleared);

    // Ctrl+Z gives the ink back under what was written since, as « Undo the clearing » does.
    write(&mut session, 7000);
    assert!(session.undo(&mut out));
    assert_eq!(starts(&session), [1000.0, 2000.0, 7000.0]);
    // A redo clears only what the clearing took: what was written since stays.
    assert!(session.redo(&mut out));
    assert_eq!(starts(&session), [7000.0]);
    out.clear();
    assert!(session.restore_cleared_with(None, &mut out));
    assert_eq!(starts(&session), [1000.0, 2000.0, 7000.0]);
    assert!(!session.restore_cleared_with(None, &mut out), "only once");

    // Edits made after a clearing come off with it (and can be redone); two clearings, one at a time.
    session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
    write(&mut session, 7500);
    session.edit(recolour(&[0], RED), &mut out).unwrap();
    session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
    write(&mut session, 8000);
    session.edit(delete(&[0]), &mut out).unwrap();
    assert!(session.restore_cleared_with(None, &mut out));
    assert_eq!(starts(&session), [7500.0, 8000.0]);
    assert!(session.restore_cleared_with(None, &mut out));
    assert_eq!(starts(&session), [1000.0, 2000.0, 7000.0, 7500.0, 8000.0]);
    assert_eq!(colours(&session), [Rgba::BLACK; 5]);
    assert!(session.snapshot().can_redo);
}

#[test]
fn clearing_leaves_the_ink_of_a_hidden_layer() {
    let mut session = hidden_layer_page();
    assert_eq!(starts(&session), [1000.0, 2000.0, 3000.0, 4000.0]);
    let mut out = Vec::new();
    session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
    assert_eq!(
        starts(&session),
        [1000.0, 3000.0],
        "the hidden strokes stay"
    );
    assert!(session.undo(&mut out));
    assert_eq!(starts(&session), [1000.0, 2000.0, 3000.0, 4000.0]);
}

/// A page with a hidden layer: strokes 0 and 2 are on it.
fn hidden_layer_page() -> LiveSession {
    let mut session = with_strokes(4);
    let page = session.page_mut(0).unwrap();
    page.note.layers = vec![
        Layer {
            name: "Réponses".into(),
            visible: false,
        },
        Layer {
            name: "Calque 2".into(),
            visible: true,
        },
    ];
    for (i, stroke) in page.note.strokes.iter_mut().enumerate() {
        stroke.layer = u32::from(i % 2 == 1);
    }
    session
}

#[test]
fn the_ink_of_a_hidden_layer_cannot_be_edited() {
    let mut session = hidden_layer_page();
    let mut out = Vec::new();
    for edit in [delete(&[1, 2]), recolour(&[0], RED)] {
        let index = u32::from(matches!(edit, StrokesEdit::Delete { .. })) * 2;
        let error = session.edit(edit, &mut out).unwrap_err();
        assert_eq!(error, LiveEditError::Hidden { index });
        assert_eq!(error.code(), "liveEditInvalid");
    }
    assert!(out.is_empty());
    assert_eq!(starts(&session).len(), 4);
}

#[test]
fn on_a_page_with_a_hidden_layer_a_viewer_is_told_in_its_own_numbering() {
    let mut session = hidden_layer_page();
    let snapshot = session.snapshot();
    let mut viewer = Follower {
        viewer: true,
        ..Follower::default()
    };
    viewer.follow(&[LiveEvent::Snapshot {
        snapshot: Arc::new(snapshot),
    }]);
    let mut out = Vec::new();
    let drawn = |session: &LiveSession| -> Vec<Stroke> {
        let page = &session.pages()[0];
        let drawn = |s: &&Stroke| crate::share::page::is_drawn(&page.note.layers, s);
        page.note.strokes.iter().filter(drawn).cloned().collect()
    };
    assert_eq!(viewer.strokes, drawn(&session));
    // [h a h b] for the page, [a b] for the viewer: b is 3 here, 1 there.
    session.edit(delete(&[3]), &mut out).unwrap();
    let [
        LiveEvent::StrokesEdited {
            removed, viewers, ..
        },
    ] = &out[..]
    else {
        panic!("{out:?}");
    };
    assert_eq!(removed, &[3]);
    assert_eq!(
        viewers.as_deref(),
        Some(&ViewerIndices {
            removed: vec![1],
            inserted: Vec::new(),
            recoloured: Vec::new(),
        })
    );
    viewer.follow(&out);
    assert_eq!(viewer.strokes, drawn(&session));

    // Whatever comes next stays right: the tablet, colours, undo, redo.
    type Step<'a> = &'a dyn Fn(&mut LiveSession, &mut Vec<LiveEvent>);
    let steps: [Step; 7] = [
        &|s, out| {
            s.undo(out);
        },
        &|s, out| {
            s.edit(recolour(&[1, 3], RED), out).unwrap();
        },
        &|s, out| {
            s.edit(delete(&[1]), out).unwrap();
        },
        &|s, out| {
            s.undo(out);
            s.undo(out);
        },
        &|s, out| {
            s.redo(out);
            s.redo(out);
        },
        &|s, out| {
            s.undo(out);
        },
        &|s, out| {
            s.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, out);
        },
    ];
    for step in steps {
        out.clear();
        step(&mut session, &mut out);
        viewer.follow(&out);
        assert_eq!(viewer.strokes, drawn(&session));
    }
    out.clear();
    assert!(session.undo(&mut out));
    let [LiveEvent::StrokesEdited { inserted, .. }] = &out[..] else {
        panic!("{out:?}");
    };
    // What came back is what shows: never a hidden stroke.
    assert!(inserted.iter().all(|placed| placed.stroke.layer == 1));
    viewer.follow(&out);
    assert_eq!(viewer.strokes, drawn(&session));
}

#[test]
fn a_turned_page_turns_what_can_come_back() {
    // The same page with and without the delete: once undone, the same strokes.
    let mut session = with_strokes(3);
    let mut twin = with_strokes(3);
    let mut out = Vec::new();
    session.edit(delete(&[0, 2]), &mut out).unwrap();
    for orientation in [Orientation::PortraitLeft, Orientation::LandscapeFlipped] {
        session.reorient(29_700, 21_600, orientation, &mut out);
        twin.reorient(29_700, 21_600, orientation, &mut out);
    }
    assert!(session.snapshot().can_undo, "the history stays");
    assert!(session.undo(&mut out));
    assert_eq!(
        session.pages()[0].note.strokes,
        twin.pages()[0].note.strokes
    );
    // And the redo takes the same strokes, however the page is held by then.
    session.reorient(29_700, 21_600, Orientation::PortraitRight, &mut out);
    twin.reorient(29_700, 21_600, Orientation::PortraitRight, &mut out);
    assert!(session.redo(&mut out));
    assert_eq!(
        session.pages()[0].note.strokes[0],
        twin.pages()[0].note.strokes[1]
    );
    assert!(session.undo(&mut out));
    assert_eq!(
        session.pages()[0].note.strokes,
        twin.pages()[0].note.strokes
    );

    // « Undo the clearing » no longer goes away with a quarter turn: the ink turns with the page.
    session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
    session.reorient(29_700, 21_600, Orientation::Landscape, &mut out);
    twin.reorient(29_700, 21_600, Orientation::Landscape, &mut out);
    assert!(session.restore_cleared_with(None, &mut out));
    assert_eq!(
        session.pages()[0].note.strokes,
        twin.pages()[0].note.strokes
    );
}

#[test]
fn a_page_whose_strokes_can_come_back_is_not_a_blank_page() {
    // Not written yet: without the history it would just take the new size, ink left behind.
    let mut session = with_strokes(1);
    let mut twin = with_strokes(1);
    let mut out = Vec::new();
    session.edit(delete(&[0]), &mut out).unwrap();
    assert!(session.pages()[0].file.is_none());
    session.reorient(29_700, 21_600, Orientation::PortraitLeft, &mut out);
    twin.reorient(29_700, 21_600, Orientation::PortraitLeft, &mut out);
    assert_eq!(session.pages()[0].note.page, twin.pages()[0].note.page);
    assert!(session.undo(&mut out));
    assert_eq!(
        session.pages()[0].note.strokes,
        twin.pages()[0].note.strokes
    );

    // Left, then the tablet turned, then shown again: it follows, what can come back too.
    let mut session = with_strokes(2);
    let mut twin = with_strokes(2);
    session.edit(delete(&[0]), &mut out).unwrap();
    for session in [&mut session, &mut twin] {
        session.command(LiveCommandKind::NewPage, LiveButton::NewPage, 0, &mut out);
        session.reorient(29_700, 21_600, Orientation::PortraitRight, &mut out);
        session.command(
            LiveCommandKind::PreviousPage,
            LiveButton::NewPage,
            0,
            &mut out,
        );
    }
    assert_eq!(session.current(), 0);
    assert!(session.undo(&mut out));
    assert_eq!(
        session.pages()[0].note.strokes,
        twin.pages()[0].note.strokes
    );
}

#[test]
fn each_page_keeps_its_history_when_it_is_left() {
    let mut session = with_strokes(3);
    let mut out = Vec::new();
    session.edit(delete(&[0]), &mut out).unwrap();
    session.undo(&mut out);
    session.edit(recolour(&[1], RED), &mut out).unwrap();
    let on_first = revision(&session);

    // Another page: nothing to undo there, and its own revision.
    session.command(LiveCommandKind::NextPage, LiveButton::NewPage, 0, &mut out);
    assert_eq!(session.current(), 1);
    assert!(!session.snapshot().can_undo);
    out.clear();
    assert!(!session.undo(&mut out));
    write(&mut session, 7000);
    session.edit(delete(&[0]), &mut out).unwrap();
    assert!(session.snapshot().can_undo);
    assert_eq!(session.pages()[0].note.strokes.len(), 3);

    // Back on the first page: it is as it was left, and undoes what was done on it.
    session.command(
        LiveCommandKind::PreviousPage,
        LiveButton::NewPage,
        0,
        &mut out,
    );
    let snapshot = session.snapshot();
    assert_eq!(snapshot.revision, on_first);
    assert!(snapshot.can_undo && !snapshot.can_redo);
    assert!(session.undo(&mut out));
    assert_eq!(colours(&session), [Rgba::BLACK; 3]);
    assert!(
        session.undo_edit(1, None, &mut out).is_err(),
        "not that page"
    );
    assert_eq!(session.pages()[1].note.strokes.len(), 0);
}

#[test]
fn stopping_drops_the_histories() {
    let mut session = with_strokes(2);
    let mut out = Vec::new();
    session.edit(delete(&[0]), &mut out).unwrap();
    session.command(LiveCommandKind::NewPage, LiveButton::NewPage, 0, &mut out);
    write(&mut session, 7000);
    session.edit(delete(&[0]), &mut out).unwrap();
    session.undo(&mut out);
    assert!(session.snapshot().can_redo);
    out.clear();
    session.set_state(LiveSessionState::Idle, &mut out);
    let Some(LiveEvent::Snapshot { snapshot }) = out.last() else {
        panic!("{out:?}");
    };
    assert!(!snapshot.can_undo && !snapshot.can_redo);
    assert!(!session.undo(&mut out));
    // The other page too.
    session.command(
        LiveCommandKind::PreviousPage,
        LiveButton::NewPage,
        0,
        &mut out,
    );
    assert!(!session.snapshot().can_undo);
}

#[test]
fn two_sessions_never_share_a_revision() {
    let first = LiveSession::new(
        1,
        A5,
        LiveStyle::default(),
        LiveViewState::whole(A5),
        true,
        0,
    );
    let second = LiveSession::new(
        2,
        A5,
        LiveStyle::default(),
        LiveViewState::whole(A5),
        true,
        0,
    );
    assert_ne!(first.snapshot().revision, second.snapshot().revision);
    // A number the interface holds exactly, even after millions of sessions.
    assert!(second.snapshot().revision < 1 << 53);
}

#[test]
fn a_narration_still_goes_on_a_page_whose_strokes_have_all_left() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    let session = hub.page_position().unwrap().session;
    assert_eq!(hub.page_origins(session), Some(vec![None]));
    stroke(&mut sink, 1000, 3);
    let origin = hub.page_origins(session).unwrap()[0];
    assert!(origin.is_some());
    wait_until("the note", || !notes_in(&library).is_empty());
    let (page, revision) = at(&hub);
    hub.edit_strokes(page, revision, delete(&[0])).unwrap();
    assert_eq!(hub.page_origins(session).unwrap()[0], origin);
    assert!(hub.set_page_audio(0, b"OggS narration".to_vec()));
    let saved = hub.stop().unwrap();
    let written = library.read(std::path::Path::new(&saved[0])).unwrap().file;
    assert!(written.note.strokes.is_empty());
    assert_eq!(written.audio.as_deref(), Some(&b"OggS narration"[..]));
}

// --- The history -----------------------------------------------------------------------------------

#[test]
fn the_history_keeps_the_last_steps_and_a_bounded_memory() {
    let mut session = with_strokes(2);
    let mut out = Vec::new();
    // More steps than kept: the oldest go, the others still undo right.
    let color = |i: usize| Rgba::rgb(i as u8, 0, 1);
    for i in 0..MAX_UNDO_STEPS + 20 {
        session.edit(recolour(&[0], color(i)), &mut out).unwrap();
    }
    let mut undone = 0;
    while session.undo(&mut out) {
        undone += 1;
    }
    assert_eq!(undone, MAX_UNDO_STEPS);
    assert_eq!(
        colours(&session)[0],
        color(19),
        "as the first step kept left it"
    );

    // More memory than kept: steps go, the last one always stays.
    let points = |n: usize| -> Vec<Point> {
        vec![
            Point {
                x: 1.0,
                y: 2.0,
                pressure: 0.5,
                t: 0,
            };
            n
        ]
    };
    let heavy = |n: usize| Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 60.0,
        layer: 0,
        points: points(n),
    };
    let quarter = MAX_UNDO_BYTES / 4 / size_of::<Point>();
    let mut session = with_strokes(0);
    for _ in 0..6 {
        let page = session.page_mut(0).unwrap();
        page.note.strokes.push(heavy(quarter));
        session.edit(delete(&[0]), &mut out).unwrap();
    }
    let mut undone = 0;
    while session.undo(&mut out) {
        undone += 1;
    }
    assert!((3..=4).contains(&undone), "{undone} steps kept");
    assert_eq!(session.pages()[0].note.strokes.len(), undone);
    while session.redo(&mut out) {}

    // Alone, a step larger than the limit stays: it holds no more than the page held, and
    // « Clear the page » of a very long lesson must still be undone.
    let mut session = with_strokes(0);
    let page = session.page_mut(0).unwrap();
    page.note
        .strokes
        .push(heavy(MAX_UNDO_BYTES / size_of::<Point>() + 1));
    session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
    assert!(session.snapshot().can_undo);
    assert!(session.undo(&mut out));
    assert_eq!(session.pages()[0].note.strokes.len(), 1);
}

#[test]
fn the_copy_prepared_is_used_only_for_the_strokes_it_was_made_from() {
    let mut session = with_strokes(3);
    let mut out = Vec::new();
    session.edit(delete(&[1]), &mut out).unwrap();
    let prepared = Prepared::of(session.next_restored().unwrap());
    assert_eq!(prepared.copy.len(), 1);
    // Made from them: moved in.
    let source = prepared.copy[0].points.as_ptr();
    assert_eq!(session.undo_edit(0, Some(prepared), &mut out), Ok(true));
    assert_eq!(session.pages()[0].note.strokes[1].points.as_ptr(), source);
    assert_eq!(starts(&session), [1000.0, 2000.0, 3000.0]);

    // Another command got in between: the strokes that come back are the right ones all the same.
    session.edit(delete(&[0]), &mut out).unwrap();
    let old = Prepared::of(session.next_restored().unwrap());
    session.edit(delete(&[0]), &mut out).unwrap();
    assert_eq!(session.undo_edit(0, Some(old), &mut out), Ok(true));
    assert_eq!(starts(&session), [2000.0, 3000.0]);
}

#[test]
fn places_move_as_little_as_possible_and_come_back_exact() {
    let stroke_at = |x: f32| Stroke {
        tool: Tool::Pencil,
        color: Rgba::BLACK,
        width: 1.0,
        layer: 0,
        points: vec![Point {
            x,
            y: 0.0,
            pressure: 1.0,
            t: 0,
        }],
    };
    let start: Vec<Stroke> = (0..6).map(|i| stroke_at(i as f32)).collect();
    let mut list = start.clone();
    let taken = remove_at(&mut list, &[0, 3, 5]);
    let xs = |list: &[Stroke]| list.iter().map(|s| s.points[0].x).collect::<Vec<_>>();
    assert_eq!(xs(&list), [1.0, 2.0, 4.0]);
    assert_eq!(taken.iter().map(|p| p.index).collect::<Vec<_>>(), [0, 3, 5]);
    assert_eq!(taken[1].stroke, start[3]);
    // With two strokes written at the tail meanwhile.
    list.push(stroke_at(6.0));
    list.push(stroke_at(7.0));
    insert_at(&mut list, taken.into_iter().map(|p| (p.index, p.stroke)));
    assert_eq!(xs(&list), [0.0, 1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0]);
    // Nothing named: nothing moves.
    assert!(remove_at(&mut list, &[]).is_empty());
    insert_at(&mut list, Vec::<(u32, Stroke)>::new().into_iter());
    assert_eq!(list.len(), 8);

    let groups = grouped([(1, RED), (2, BLUE), (3, RED)].into_iter());
    assert_eq!(groups.len(), 2);
    assert_eq!(
        (groups[0].color, &groups[0].indices[..]),
        (RED, &[1, 3][..])
    );
    assert_eq!((groups[1].color, &groups[1].indices[..]), (BLUE, &[2][..]));
}

// --- The hub ---------------------------------------------------------------------------------------

/// The hub's page and revision, as a window knows them from its snapshot and events.
fn at(hub: &LiveHub) -> (usize, u64) {
    let snapshot = hub.snapshot();
    (snapshot.page_index, snapshot.revision)
}

fn stroke_starts(hub: &LiveHub) -> Vec<f32> {
    hub.snapshot()
        .page
        .strokes
        .iter()
        .map(|s| s.points[0].x)
        .collect()
}

#[test]
fn edits_need_a_running_session_and_their_errors_have_codes() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    assert!(matches!(
        hub.edit_strokes(0, 0, delete(&[0])),
        Err(LiveError::NotRunning)
    ));
    assert!(matches!(hub.undo_edit(0), Err(LiveError::NotRunning)));
    assert!(matches!(hub.redo_edit(0), Err(LiveError::NotRunning)));

    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    stroke(&mut sink, 1000, 3);
    stroke(&mut sink, 2000, 3);
    let (page, revision) = at(&hub);
    // Nothing to do with the state of the page: refused before the lock.
    let code = |error: LiveError| error.command_error().parts().0;
    assert_eq!(
        code(hub.edit_strokes(page, revision, delete(&[])).unwrap_err()),
        "liveEditInvalid"
    );
    let after = hub.edit_strokes(page, revision, delete(&[0])).unwrap();
    assert_ne!(after, revision);
    assert_eq!(
        code(hub.edit_strokes(page, revision, delete(&[0])).unwrap_err()),
        "liveEditStale"
    );
    assert_eq!(code(hub.undo_edit(page + 1).unwrap_err()), "liveEditStale");
    assert_eq!(code(hub.redo_edit(page + 1).unwrap_err()), "liveEditStale");
    assert!(hub.undo_edit(page).unwrap());
    assert!(hub.redo_edit(page).unwrap());
    assert!(!hub.redo_edit(page).unwrap());
    assert_eq!(stroke_starts(&hub), [2000.0]);

    // Once stopped its notes are in the library: as for the other changes, refused.
    let (page, revision) = at(&hub);
    hub.stop().unwrap();
    assert!(matches!(
        hub.edit_strokes(page, revision, delete(&[0])),
        Err(LiveError::Stopped)
    ));
    assert_eq!(code(hub.undo_edit(page).unwrap_err()), "liveNotRunning");
    assert!(matches!(hub.redo_edit(page), Err(LiveError::Stopped)));
    assert!(!hub.snapshot().can_undo);
}

#[test]
fn the_event_keeps_its_order_among_the_points_and_is_never_merged() {
    let (_dir, library) = library();
    let hub = LiveHub::new(HubConfig {
        // Everything of this test falls in the same few batches: nothing goes missing in them.
        flush_interval: Duration::from_millis(40),
        save_interval: Duration::from_secs(60),
    });
    let window = Arc::new(Window::default());
    hub.subscribe("main", window.clone());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    stroke(&mut sink, 1000, 3);
    stroke(&mut sink, 2000, 3);
    let (page, mut revision) = at(&hub);
    send(&mut sink, &[pen(5000, 100, 500)]);
    revision = hub.edit_strokes(page, revision, delete(&[0])).unwrap();
    send(&mut sink, &[pen(5010, 100, 500)]);
    hub.edit_strokes(page, revision, recolour(&[0], RED))
        .unwrap();
    hub.edit_strokes(page, revision, recolour(&[0], BLUE))
        .unwrap();
    send(&mut sink, &[pen(5020, 100, 500)]);
    assert!(hub.undo_edit(page).unwrap());
    send(&mut sink, &[pen(5030, 100, 0)]);
    wait_until("every event", || {
        window
            .events()
            .iter()
            .filter(|e| matches!(e, LiveEvent::StrokeEnd { .. }))
            .count()
            == 3
    });
    let events = window.events();
    let from = events
        .iter()
        .position(
            |e| matches!(e, LiveEvent::StrokeStart { stroke, .. } if stroke.points[0].x == 5000.0),
        )
        .unwrap();
    let kinds: Vec<&str> = events[from..]
        .iter()
        .filter_map(|e| match e {
            LiveEvent::StrokeStart { .. } => Some("start"),
            LiveEvent::Points { .. } => Some("points"),
            LiveEvent::StrokeEnd { .. } => Some("end"),
            LiveEvent::StrokesEdited { .. } => Some("edited"),
            _ => None,
        })
        .collect();
    assert_eq!(
        kinds,
        [
            "start", "edited", "points", "edited", "edited", "points", "edited", "end"
        ]
    );
    // A window that applies them in order has the list of the page.
    let mut follower = Follower::default();
    follower.follow(&events);
    assert_eq!(follower.strokes, hub.snapshot().page.strokes);
}

#[test]
fn the_json_is_what_live_ts_reads() {
    let mut session = with_strokes(2);
    let mut out = Vec::new();
    session.edit(delete(&[1]), &mut out).unwrap();
    session.undo(&mut out);
    session.edit(recolour(&[0], RED), &mut out).unwrap();
    let revision = session.snapshot().revision;
    let json: Vec<Value> = out
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    assert_eq!(
        json[0],
        json!({
            "type": "strokesEdited",
            "pageIndex": 0,
            "revision": revision - 1,
            "removed": [1],
            "inserted": [],
            "recoloured": [],
            "canUndo": true,
            "canRedo": false,
        })
    );
    assert_eq!(json[1]["removed"], json!([]));
    assert_eq!(json[1]["canRedo"], true);
    assert_eq!(json[1]["canUndo"], false);
    let placed = &json[1]["inserted"][0];
    assert_eq!(placed["index"], 1);
    assert_eq!(placed["stroke"]["layer"], 0);
    assert_eq!(
        placed["stroke"]["color"],
        json!({ "r": 0, "g": 0, "b": 0, "a": 255 })
    );
    assert_eq!(placed["stroke"]["points"][0]["x"], 2000.0);
    assert!(placed["stroke"]["points"][0].get("pressure").is_some());
    assert_eq!(
        json[2]["recoloured"],
        json!([{ "color": { "r": 200, "g": 0, "b": 0, "a": 255 }, "indices": [0] }])
    );
    let snapshot = serde_json::to_value(session.snapshot()).unwrap();
    assert_eq!(snapshot["revision"], revision);
    assert_eq!(snapshot["canUndo"], true);
    assert_eq!(snapshot["canRedo"], false);
}

#[test]
fn an_edit_reaches_the_file_within_the_usual_delay_and_at_stop() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    stroke(&mut sink, 1000, 3);
    stroke(&mut sink, 2000, 3);
    wait_until("the note", || !notes_in(&library).is_empty());
    let path = notes_in(&library)[0].clone();
    let strokes_on_disk = || {
        library
            .read(&path)
            .map(|loaded| loaded.file.note.strokes)
            .unwrap_or_default()
    };
    wait_until("the second stroke on disk", || strokes_on_disk().len() == 2);
    // Let the saver go to sleep: nothing is due any more.
    std::thread::sleep(config().save_interval * 2);

    let (page, revision) = at(&hub);
    let wrote = Instant::now();
    hub.edit_strokes(page, revision, delete(&[0])).unwrap();
    wait_until("the delete on disk", || strokes_on_disk().len() == 1);
    println!("the edit reached the file after {:?}", wrote.elapsed());
    assert!(wrote.elapsed() < config().save_interval * 20);
    assert_eq!(strokes_on_disk()[0].points[0].x, 2000.0);

    // Asleep again: an undo alone wakes the saver as well.
    std::thread::sleep(config().save_interval * 2);
    assert!(hub.undo_edit(page).unwrap());
    wait_until("the undo on disk", || strokes_on_disk().len() == 2);

    // A change of colour too, however short the time before the stop.
    std::thread::sleep(config().save_interval * 2);
    let revision = at(&hub).1;
    hub.edit_strokes(page, revision, recolour(&[0, 1], RED))
        .unwrap();
    wait_until("the colour on disk", || {
        strokes_on_disk().get(1).is_some_and(|s| s.color == RED)
    });
    hub.edit_strokes(page, revision, recolour(&[0], BLUE))
        .unwrap();
    hub.stop().unwrap();
    let on_disk = strokes_on_disk();
    assert_eq!((on_disk[0].color, on_disk[1].color), (BLUE, RED));
    assert_eq!(notes_in(&library).len(), 1);
}

#[test]
fn a_late_window_starts_from_the_edited_page_and_its_revision() {
    let (_dir, library) = library();
    let hub = LiveHub::new(config());
    let mut sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    stroke(&mut sink, 1000, 3);
    stroke(&mut sink, 2000, 3);
    let (page, revision) = at(&hub);
    let revision = hub.edit_strokes(page, revision, delete(&[0])).unwrap();
    hub.edit_strokes(page, revision, recolour(&[0], RED))
        .unwrap();

    let late = Arc::new(Window::default());
    let snapshot = hub.subscribe("late", late.clone());
    assert_eq!(snapshot.revision, revision);
    assert!(snapshot.can_undo);
    assert_eq!(snapshot.page.strokes.len(), 1);
    assert_eq!(snapshot.page.strokes[0].color, RED);
    wait_until("its first batch", || !late.events().is_empty());
    assert!(matches!(&late.events()[0],
        LiveEvent::Snapshot { snapshot } if snapshot.revision == revision));
    // What it gets from then on is enough.
    assert!(hub.undo_edit(page).unwrap());
    assert!(hub.undo_edit(page).unwrap());
    wait_until("the edits", || {
        late.events()
            .iter()
            .filter(|e| matches!(e, LiveEvent::StrokesEdited { .. }))
            .count()
            == 2
    });
    let mut follower = Follower::default();
    follower.follow(&late.events());
    assert_eq!(follower.strokes, hub.snapshot().page.strokes);
    assert_eq!(follower.strokes.len(), 2);
}

/// The simulated tablet writes in real time, through the tablet service, while edits keep arriving:
/// no ink is lost or reordered, what the tablet wrote stays, a window keeps the page's list, and the
/// tablet's points are not delayed.
#[test]
fn the_simulated_pen_writes_while_edits_arrive() {
    let (_dir, library) = library();
    let hub = LiveHub::new(HubConfig::default());
    let window = Arc::new(Window::default());
    hub.subscribe("edits", window.clone());
    let mut edits = 0;
    let samples = stream_simulated_live(&hub, &library, 4, || {
        let until = Instant::now() + Duration::from_millis(3500);
        let mut cycle = 0u32;
        while Instant::now() < until {
            std::thread::sleep(Duration::from_millis(25));
            let (page, revision) = at(&hub);
            let strokes = hub.snapshot().page.strokes.len() as u32;
            if strokes == 0 {
                continue;
            }
            cycle += 1;
            let chosen: Vec<u32> = (0..strokes).step_by(2).collect();
            let done = match cycle % 4 {
                0 => hub.undo_edit(page).map(|_| ()),
                1 => hub
                    .edit_strokes(page, revision, delete(&chosen))
                    .map(|_| ()),
                2 => hub
                    .edit_strokes(page, revision, recolour(&[0], Rgba::rgb(cycle as u8, 0, 0)))
                    .map(|_| ()),
                _ => hub.undo_edit(page).map(|_| ()),
            };
            // Out of range or stale only if the tablet's page changed: a one-page session here.
            done.unwrap();
            edits += 1;
        }
        // Everything undone: all that the tablet wrote is there, as it wrote it.
        let (page, _) = at(&hub);
        while hub.undo_edit(page).unwrap() {}
    });
    assert!(edits > 20, "{edits} edits");
    wait_until("the end of the session", || {
        window.has(|e| matches!(e, LiveEvent::Snapshot { snapshot } if snapshot.state == LiveSessionState::Idle && !snapshot.page.strokes.is_empty()))
    });
    let events = window.events();
    let edited = |e: &&LiveEvent| matches!(e, LiveEvent::StrokesEdited { .. });
    assert!(events.iter().filter(edited).count() > 20);

    // The list a window built from the events is what the session has, in the same order...
    let mut follower = Follower::default();
    follower.follow(&events);
    let written = hub.snapshot().page.strokes;
    assert_eq!(follower.strokes, written);
    // ... which is every stroke the tablet wrote, each with all its points, in writing order.
    let ended = events
        .iter()
        .filter(|e| matches!(e, LiveEvent::StrokeEnd { .. }))
        .count();
    assert!(ended > 5, "{ended} strokes");
    assert_eq!(written.len(), ended);
    let mut last = 0;
    for stroke in &written {
        for point in &stroke.points {
            assert!(point.t >= last, "the writing order");
            last = point.t;
        }
    }
    // The note written at the stop holds the same.
    let notes = notes_in(&library);
    assert_eq!(notes.len(), 1);
    let on_disk = library.read(&notes[0]).unwrap().file.note;
    assert_eq!(on_disk.strokes.len(), written.len());

    let n = samples.len();
    assert!(n > 200, "{n} points");
    let (median, p95, max) = crate::live::tests::figures(&samples);
    println!(
        "simulator -> hub latency with {edits} edits meanwhile: {n} points, median {median:.3} ms, \
         p95 {p95:.3} ms, max {max:.3} ms"
    );
    assert!(median < 10.0, "median {median} ms");
}

/// What an edit costs under the hub's lock on a page of many strokes, next to what a snapshot would
/// cost per window. `cargo test -p ardoise large_page -- --nocapture` prints the figures. The test
/// build is 10 to 30 times slower than the app: it only fails on a cost out of all proportion.
#[test]
fn an_edit_on_a_large_page_costs_a_pass_over_its_list() {
    const STROKES: usize = 50_000;
    const POINTS: usize = 100;
    let mut session = running();
    {
        let page = session.page_mut(0).unwrap();
        page.note.strokes = (0..STROKES)
            .map(|i| Stroke {
                tool: Tool::Ballpoint,
                color: Rgba::BLACK,
                width: 60.0,
                layer: 0,
                points: (0..POINTS)
                    .map(|n| Point {
                        x: i as f32,
                        y: n as f32,
                        pressure: 0.5,
                        t: n as u32,
                    })
                    .collect(),
            })
            .collect();
    }
    let timed = |what: &str, work: &mut dyn FnMut()| {
        let begin = Instant::now();
        work();
        let took = begin.elapsed();
        println!("{what}: {took:?}");
        took
    };
    let mut out = Vec::new();
    let some: Vec<u32> = (0..1000).map(|i| i * 50).collect();
    let snapshot = timed("what one window's snapshot clones", &mut || {
        out.push(LiveEvent::Snapshot {
            snapshot: Arc::new(session.snapshot()),
        });
    });
    out.clear();
    let edit = delete(&some).checked().unwrap();
    let delete_time = timed("delete 1 000 strokes", &mut || {
        let revision = session.revision_for_test();
        session
            .edit_strokes(0, revision, edit.clone(), &mut out)
            .unwrap();
    });
    // Undo the way the hub does it: the copy is made before the lock is taken.
    let mut prepared = Some(Prepared::of(session.next_restored().unwrap()));
    let undo_time = timed("undo them, the copy already made", &mut || {
        session.undo_edit(0, prepared.take(), &mut out).unwrap();
    });
    let recolour_all: Vec<u32> = (0..STROKES as u32).collect();
    let edit = recolour(&recolour_all, RED).checked().unwrap();
    let recolour_time = timed("recolour all of them", &mut || {
        let revision = session.revision_for_test();
        session
            .edit_strokes(0, revision, edit.clone(), &mut out)
            .unwrap();
    });
    let mut cleared = Vec::new();
    let clear_time = timed("clear the page", &mut || {
        session.command(
            LiveCommandKind::ClearPage,
            LiveButton::NewPage,
            0,
            &mut cleared,
        );
    });
    assert_eq!(session.snapshot().page.strokes.len(), 0);
    println!(
        "{STROKES} strokes of {POINTS} points: snapshot {snapshot:?}, delete {delete_time:?}, \
         undo {undo_time:?}, recolour {recolour_time:?}, clear {clear_time:?}"
    );
    // Far less than a snapshot for one window, itself one per window.
    assert!(
        delete_time < snapshot,
        "{delete_time:?} against {snapshot:?}"
    );
}

/// What a change of style costs under the hub's lock with 32 windows and viewers on a page of
/// 50 000 strokes, against what it cost when each had its own copy of the snapshot (one clone per
/// subscriber, two more per subscription). `cargo test -p ardoise large_page -- --nocapture`
/// prints the figures; what the test fails on is that the windows do not share one snapshot.
#[test]
fn a_snapshot_on_a_large_page_is_copied_once_however_many_windows_there_are() {
    const STROKES: usize = 50_000;
    const POINTS: usize = 100;
    const WINDOWS: usize = 32;

    /// Keeps which snapshots it was sent, by address.
    #[derive(Default)]
    struct Seen(Mutex<Vec<usize>>);

    impl BatchSink for Seen {
        fn send(&self, batch: LiveBatch) -> Result<(), String> {
            let mut seen = self.0.lock().unwrap();
            for event in &batch.events {
                if let LiveEvent::Snapshot { snapshot } = event {
                    seen.push(Arc::as_ptr(snapshot) as usize);
                }
            }
            Ok(())
        }
    }

    let (_dir, library) = library();
    let hub = LiveHub::new(HubConfig {
        flush_interval: Duration::from_millis(4),
        save_interval: Duration::from_secs(3600),
    });
    let _sink = start(&hub, options(&library, fixed(LiveButton::NewPage)));
    hub.change_at_once(|session, _| {
        session.page_mut(0).unwrap().note.strokes = (0..STROKES)
            .map(|i| Stroke {
                tool: Tool::Ballpoint,
                color: Rgba::BLACK,
                width: 60.0,
                layer: 0,
                points: (0..POINTS)
                    .map(|n| Point {
                        x: i as f32,
                        y: n as f32,
                        pressure: 0.5,
                        t: n as u32,
                    })
                    .collect(),
            })
            .collect();
    });
    let windows: Vec<Arc<Seen>> = (0..WINDOWS).map(|_| Arc::default()).collect();
    for (n, window) in windows.iter().enumerate() {
        hub.subscribe(&format!("window{n}"), window.clone());
    }

    let snapshot = hub.snapshot();
    let begin = Instant::now();
    for _ in 0..WINDOWS {
        drop(snapshot.clone());
    }
    let before = begin.elapsed();
    let begin = Instant::now();
    hub.set_style(LiveStyle {
        color: RED,
        ..snapshot.style
    })
    .unwrap();
    let after = begin.elapsed();
    println!(
        "{STROKES} strokes of {POINTS} points, {WINDOWS} windows: a change of style held the lock \
         for {after:?}; one copy per window was {before:?}, not counting the snapshot taken"
    );

    wait_until("every window told", || {
        windows.iter().all(|w| w.0.lock().unwrap().len() == 2)
    });
    let last: Vec<usize> = windows
        .iter()
        .map(|w| *w.0.lock().unwrap().last().unwrap())
        .collect();
    assert!(last.iter().all(|address| *address == last[0]), "{last:?}");
    hub.stop().unwrap();
}

impl LiveSession {
    /// The revision, without cloning the page as `snapshot` does.
    fn revision_for_test(&self) -> u64 {
        self.pages()[self.current()].revision()
    }
}
