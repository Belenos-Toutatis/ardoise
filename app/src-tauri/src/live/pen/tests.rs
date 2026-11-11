//! Tests of the computer's pen in live mode (M14): what a call is once checked, its times, and the
//! session's computer stroke beside the tablet's (begin, points, end, cancel; what ends or drops
//! it; refusals and limits). Undo is in `edits/tests.rs`, layers in `layers/tests.rs`, the hub and
//! the share in `live/tests.rs` and `share/tests.rs`.

use ardoise_core::note::A4_LANDSCAPE;
use ardoise_core::settings::{LiveButton, Orientation};
use ardoise_ink::{Rgba, Tool};
use serde_json::{Value, json};

use super::{
    CheckedPen, ComputerPenInput, ComputerSample, MAX_COMPUTER_POINTS, MAX_PEN_SAMPLES,
    MAX_SAMPLE_AGE_MS, PenAction, PenSample, note_time, sample_wall,
};
use crate::live::edits::LiveEditError;
use crate::live::layers::CheckedLayerOp;
use crate::live::model::{
    LiveCommandKind, LiveEvent, LivePen, LiveSessionState, LiveStyle, LiveViewState,
};
use crate::live::session::LiveSession;
use crate::live::tests::pen;

/// Wall clock of the session tests (Unix ms, a whole number).
pub(crate) const T0: f64 = 1_792_843_200_000.0;

/// The computer's ink of the design's default: red felt tip, 120.
pub(crate) const RED_PEN: LiveStyle = LiveStyle {
    tool: Tool::FeltTip,
    color: Rgba::rgb(216, 0, 0),
    width: 120.0,
};

/// A session that runs, its tablet streaming (an A4 held in landscape), its one page created a
/// minute before [`T0`].
pub(crate) fn running() -> LiveSession {
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

/// A sample at (`x`, 100) of a mouse (pressure 0.5), taken at `ms` on the window's clock.
pub(crate) fn sample(x: f64, ms: f64) -> ComputerSample {
    ComputerSample {
        x,
        y: 100.0,
        p: 0.5,
        ms,
    }
}

/// Samples at `xs`, all taken when the call leaves (`ms` = `now` = 0).
pub(crate) fn at_once(xs: &[f64]) -> Vec<ComputerSample> {
    xs.iter().map(|&x| sample(x, 0.0)).collect()
}

pub(crate) fn begin(
    stroke: u32,
    page_index: usize,
    samples: Vec<ComputerSample>,
) -> ComputerPenInput {
    ComputerPenInput::Begin {
        stroke,
        page_index,
        style: RED_PEN,
        samples,
        now: 0.0,
    }
}

pub(crate) fn points(stroke: u32, samples: Vec<ComputerSample>) -> ComputerPenInput {
    ComputerPenInput::Points {
        stroke,
        samples,
        now: 0.0,
    }
}

pub(crate) fn end(stroke: u32, samples: Vec<ComputerSample>) -> ComputerPenInput {
    ComputerPenInput::End {
        stroke,
        samples,
        now: 0.0,
    }
}

impl LiveSession {
    /// A call of the computer's pen received at `wall_now`, checked as the hub checks it.
    pub(crate) fn computer_call(
        &mut self,
        input: ComputerPenInput,
        wall_now: f64,
        out: &mut Vec<LiveEvent>,
    ) -> Result<bool, LiveEditError> {
        self.computer_pen(input.checked()?, wall_now, out)
    }

    /// A finished computer stroke on the page on screen, starting at `x` (two samples).
    pub(crate) fn computer_stroke(&mut self, id: u32, x: f64, out: &mut Vec<LiveEvent>) {
        let page = self.current();
        assert_eq!(
            self.computer_call(begin(id, page, at_once(&[x, x + 10.0])), T0, out),
            Ok(true)
        );
        assert_eq!(
            self.computer_call(end(id, Vec::new()), T0 + 5.0, out),
            Ok(true)
        );
    }

    /// A layer operation on the page on screen.
    pub(crate) fn layer_op(
        &mut self,
        op: CheckedLayerOp,
        out: &mut Vec<LiveEvent>,
    ) -> Result<(), LiveEditError> {
        self.layers(self.current(), op, out)
    }
}

/// The pen and the window's number of a stroke event.
fn pen_of(event: &LiveEvent) -> Option<(&'static str, LivePen, Option<u32>)> {
    match event {
        LiveEvent::StrokeStart { pen, id, .. } => Some(("start", *pen, *id)),
        LiveEvent::Points { pen, id, .. } => Some(("points", *pen, *id)),
        LiveEvent::StrokeEnd { pen, id, .. } => Some(("end", *pen, *id)),
        LiveEvent::StrokeCancel { pen, id, .. } => Some(("cancel", *pen, *id)),
        _ => None,
    }
}

fn stroke_events(out: &[LiveEvent]) -> Vec<(&'static str, LivePen, Option<u32>)> {
    out.iter().filter_map(pen_of).collect()
}

const TABLET: LivePen = LivePen::Tablet;
const COMPUTER: LivePen = LivePen::Computer;

// --- What a call is, once checked --------------------------------------------------------------

#[test]
fn the_window_s_calls_are_read_as_live_ts_writes_them() {
    let read = |value: Value| serde_json::from_value::<ComputerPenInput>(value).unwrap();
    let style =
        json!({ "tool": "feltTip", "color": { "r": 216, "g": 0, "b": 0, "a": 255 }, "width": 120 });
    let samples = json!([{ "x": 10.5, "y": 20, "p": 0.25, "ms": 1000.5 }]);
    assert_eq!(
        read(
            json!({ "kind": "begin", "stroke": 1, "pageIndex": 2, "style": style, "samples": samples, "now": 1002 })
        ),
        ComputerPenInput::Begin {
            stroke: 1,
            page_index: 2,
            style: RED_PEN,
            samples: vec![ComputerSample {
                x: 10.5,
                y: 20.0,
                p: 0.25,
                ms: 1000.5
            }],
            now: 1002.0,
        }
    );
    assert!(matches!(
        read(json!({ "kind": "points", "stroke": 3, "samples": [], "now": 5 })),
        ComputerPenInput::Points { stroke: 3, .. }
    ));
    assert!(matches!(
        read(json!({ "kind": "end", "stroke": 3, "samples": [], "now": 5 })),
        ComputerPenInput::End { stroke: 3, .. }
    ));
    assert_eq!(
        read(json!({ "kind": "cancel", "stroke": 4 })),
        ComputerPenInput::Cancel { stroke: 4 }
    );
    assert!(
        serde_json::from_value::<ComputerPenInput>(json!({ "kind": "erase", "stroke": 1 }))
            .is_err()
    );
}

#[test]
fn samples_are_checked_before_the_lock() {
    let input = ComputerPenInput::Begin {
        stroke: 7,
        page_index: 0,
        style: LiveStyle {
            width: f32::NAN,
            ..RED_PEN
        },
        samples: vec![
            ComputerSample {
                x: f64::NAN,
                y: 1.0,
                p: 0.5,
                ms: 0.0,
            },
            ComputerSample {
                x: 1.0,
                y: f64::INFINITY,
                p: 0.5,
                ms: 0.0,
            },
            ComputerSample {
                x: 1.0,
                y: 2.0,
                p: 3.0,
                ms: 95.0,
            },
            ComputerSample {
                x: 3.0,
                y: 4.0,
                p: f64::NAN,
                ms: 120.0,
            },
            ComputerSample {
                x: 5.0,
                y: 6.0,
                p: -1.0,
                ms: f64::NAN,
            },
        ],
        now: 100.0,
    };
    let CheckedPen {
        stroke,
        action,
        samples,
    } = input.checked().unwrap();
    assert_eq!(stroke, 7);
    // The style is one the tools draw: the tool's own width for a missing one.
    assert_eq!(
        action,
        PenAction::Begin {
            page_index: 0,
            style: LiveStyle {
                width: Tool::FeltTip.default_width(),
                ..RED_PEN
            }
        }
    );
    // Without finite coordinates, dropped; pressure within 0..=1 (1 when it is no number); an
    // age never negative (a sample stamped after the call), 0 when it is no number.
    assert_eq!(
        samples,
        [
            PenSample {
                x: 1.0,
                y: 2.0,
                p: 1.0,
                age: 5.0
            },
            PenSample {
                x: 3.0,
                y: 4.0,
                p: 1.0,
                age: 0.0
            },
            PenSample {
                x: 5.0,
                y: 6.0,
                p: 0.0,
                age: 0.0
            },
        ]
    );

    // A begin needs a sample; the others may come with none.
    let nothing = vec![sample(f64::NAN, 0.0)];
    let refused = begin(1, 0, nothing.clone()).checked().unwrap_err();
    assert_eq!(refused, LiveEditError::NoSample);
    assert_eq!(refused.code(), "liveEditInvalid");
    assert!(begin(1, 0, Vec::new()).checked().is_err());
    assert!(
        points(1, nothing.clone())
            .checked()
            .unwrap()
            .samples
            .is_empty()
    );
    assert!(end(1, nothing).checked().is_ok());

    // At most so many samples in one call, whatever the call.
    let many = at_once(&[1.0; MAX_PEN_SAMPLES]);
    assert_eq!(
        begin(1, 0, many.clone()).checked().unwrap().samples.len(),
        MAX_PEN_SAMPLES
    );
    let mut too_many = many;
    too_many.push(sample(1.0, 0.0));
    for input in [
        begin(1, 0, too_many.clone()),
        points(1, too_many.clone()),
        end(1, too_many),
    ] {
        let error = input.checked().unwrap_err();
        assert_eq!(
            error,
            LiveEditError::TooManySamples {
                count: MAX_PEN_SAMPLES + 1
            }
        );
        assert_eq!(error.code(), "liveEditInvalid");
    }

    // Coordinates are kept within one page size around the page, as the marks are.
    let far = PenSample {
        x: 1e30,
        y: -1e30,
        p: 1.0,
        age: 0.0,
    };
    assert_eq!(far.within(100.0, 50.0), (200.0, -50.0));
    let huge = points(
        1,
        vec![ComputerSample {
            x: 1e300,
            y: -1e300,
            p: 0.5,
            ms: 0.0,
        }],
    );
    let sample = huge.checked().unwrap().samples[0];
    assert_eq!(sample.within(100.0, 50.0), (200.0, -50.0));
}

#[test]
fn a_sample_s_wall_time_is_anchored_at_its_call() {
    const NOW: f64 = 1_800_000_000_000.25;
    // `now − ms` before the call; the clocks of the window and of the session never meet.
    assert_eq!(sample_wall(NOW, 12.5, None), NOW - 12.5);
    assert_eq!(sample_wall(NOW, 0.0, None), NOW);
    // Never before the previous sample of the stroke...
    assert_eq!(sample_wall(NOW, 12.5, Some(NOW - 3.0)), NOW - 3.0);
    // ... nor after the call, nor longer before it than allowed.
    assert_eq!(sample_wall(NOW, 0.0, Some(NOW + 50.0)), NOW);
    assert_eq!(sample_wall(NOW, 1e9, None), NOW - MAX_SAMPLE_AGE_MS);

    // Note time: whole milliseconds since the origin, never before the stroke's previous time.
    assert_eq!(note_time(1000.9, 0, None), 1000);
    assert_eq!(note_time(5.0, 10, None), 0);
    assert_eq!(note_time(1000.0, 0, Some(2000)), 2000);
    assert_eq!(note_time(f64::NAN, 0, None), 0);
    assert_eq!(note_time(1e20, 0, None), u32::MAX);
}

// --- The session's computer stroke ---------------------------------------------------------------

#[test]
fn a_computer_stroke_begins_goes_on_and_ends_on_its_layer_as_a_step() {
    let mut session = running();
    let mut out = Vec::new();
    assert_eq!(
        session.computer_call(begin(1, 0, at_once(&[100.0, 110.0])), T0, &mut out),
        Ok(true)
    );
    // Sent at once, with the pen and the window's number.
    let [
        LiveEvent::StrokeStart {
            page_index: 0,
            stroke,
            pen: COMPUTER,
            id: Some(1),
            hidden: false,
        },
    ] = &out[..]
    else {
        panic!("{out:?}");
    };
    assert_eq!(
        (stroke.tool, stroke.color, stroke.width, stroke.layer),
        (Tool::FeltTip, RED_PEN.color, 120.0, 0)
    );
    assert_eq!(stroke.points.len(), 2);
    assert_eq!((stroke.points[0].recv, stroke.points[0].p), (T0, 0.5));
    // Not in the page while in progress, nor a step yet.
    assert!(session.pages()[0].note.strokes.is_empty());
    assert!(!session.snapshot().can_undo);

    out.clear();
    assert_eq!(
        session.computer_call(points(1, at_once(&[120.0])), T0 + 8.0, &mut out),
        Ok(true)
    );
    assert!(
        matches!(&out[..], [LiveEvent::Points { points, pen: COMPUTER, id: Some(1), .. }] if points.len() == 1)
    );
    // A call with nothing new says the stroke goes on, and sends nothing.
    out.clear();
    assert_eq!(
        session.computer_call(points(1, Vec::new()), T0 + 9.0, &mut out),
        Ok(true)
    );
    assert!(out.is_empty());

    let revision = session.snapshot().revision;
    assert_eq!(
        session.computer_call(end(1, at_once(&[130.0])), T0 + 16.0, &mut out),
        Ok(true)
    );
    assert_eq!(
        stroke_events(&out),
        [("points", COMPUTER, Some(1)), ("end", COMPUTER, Some(1))]
    );
    assert!(matches!(
        out.last(),
        Some(LiveEvent::History {
            page_index: 0,
            can_undo: true,
            can_redo: false
        })
    ));
    let page = &session.pages()[0];
    assert_eq!(page.note.strokes.len(), 1);
    let written = &page.note.strokes[0];
    assert_eq!(
        (written.tool, written.color, written.layer),
        (Tool::FeltTip, RED_PEN.color, 0)
    );
    let xs: Vec<f32> = written.points.iter().map(|p| p.x).collect();
    assert_eq!(xs, [100.0, 110.0, 120.0, 130.0]);
    // The page's first stroke creates its note, at once; no place moved, the revision stays.
    assert!(page.urgent && page.needs_write());
    assert_eq!(session.snapshot().revision, revision);
    assert!(session.snapshot().can_undo);
    assert!(session.in_progress().is_empty());

    // Done: the same number no longer names a stroke.
    out.clear();
    assert_eq!(
        session.computer_call(points(1, at_once(&[1.0])), T0 + 20.0, &mut out),
        Ok(false)
    );
    assert_eq!(
        session.computer_call(end(1, Vec::new()), T0 + 20.0, &mut out),
        Ok(false)
    );
    assert_eq!(
        session.computer_call(ComputerPenInput::Cancel { stroke: 1 }, T0, &mut out),
        Ok(false)
    );
    assert!(out.is_empty());
}

#[test]
fn the_json_of_the_computer_s_events_is_what_live_ts_reads() {
    let mut session = running();
    let mut out = Vec::new();
    session
        .computer_call(begin(4, 0, at_once(&[100.0])), T0, &mut out)
        .unwrap();
    session
        .computer_call(points(4, at_once(&[110.0])), T0 + 4.0, &mut out)
        .unwrap();
    session
        .computer_call(end(4, Vec::new()), T0 + 8.0, &mut out)
        .unwrap();
    session
        .computer_call(begin(5, 0, at_once(&[200.0])), T0 + 10.0, &mut out)
        .unwrap();
    session
        .computer_call(ComputerPenInput::Cancel { stroke: 5 }, T0 + 12.0, &mut out)
        .unwrap();
    let json: Vec<Value> = out
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    assert_eq!(json[0]["type"], "strokeStart");
    assert_eq!(
        (json[0]["pen"].clone(), json[0]["id"].clone()),
        (json!("computer"), json!(4))
    );
    assert_eq!(
        json[0]["stroke"],
        json!({
            "tool": "feltTip",
            "color": { "r": 216, "g": 0, "b": 0, "a": 255 },
            "width": 120.0,
            "layer": 0,
            "points": [{ "x": 100.0, "y": 100.0, "p": 0.5, "t": 0, "recv": T0 }]
        })
    );
    assert_eq!(
        json[1],
        json!({ "type": "points", "pageIndex": 0, "pen": "computer", "id": 4,
                "points": [{ "x": 110.0, "y": 100.0, "p": 0.5, "t": 4, "recv": T0 + 4.0 }] })
    );
    assert_eq!(
        json[2],
        json!({ "type": "strokeEnd", "pageIndex": 0, "pen": "computer", "id": 4 })
    );
    assert_eq!(
        json[3],
        json!({ "type": "history", "pageIndex": 0, "canUndo": true, "canRedo": false })
    );
    assert_eq!(
        json[5],
        json!({ "type": "strokeCancel", "pageIndex": 0, "pen": "computer", "id": 5 })
    );
    // The tablet's events say nothing of a pen: as they always were.
    out.clear();
    session.records(&[pen(10, 20, 2047), pen(30, 40, 0)], T0 + 100.0, &mut out);
    let json: Vec<Value> = out
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    let start = json.iter().find(|e| e["type"] == "strokeStart").unwrap();
    assert!(
        start.get("pen").is_none() && start.get("id").is_none() && start.get("hidden").is_none()
    );
    let end = json.iter().find(|e| e["type"] == "strokeEnd").unwrap();
    assert_eq!(end, &json!({ "type": "strokeEnd", "pageIndex": 0 }));
}

#[test]
fn on_a_fresh_page_the_first_sample_of_either_pen_is_the_note_s_time_origin() {
    let mut session = running();
    let mut out = Vec::new();
    // The window took the samples 10 and 2 ms before its call; the page is fresh.
    let input = ComputerPenInput::Begin {
        stroke: 1,
        page_index: 0,
        style: RED_PEN,
        samples: vec![sample(1.0, 100.0), sample(2.0, 108.0)],
        now: 110.0,
    };
    session.computer_call(input, T0, &mut out).unwrap();
    assert_eq!(session.pages()[0].note.created, T0 as i64 - 10);
    assert_eq!(session.pages()[0].time_origin(), Some(T0 as i64 - 10));
    // Received 10 ms later on the session's clock, taken 4 ms before that call.
    let input = ComputerPenInput::Points {
        stroke: 1,
        samples: vec![sample(3.0, 116.0)],
        now: 120.0,
    };
    session.computer_call(input, T0 + 10.0, &mut out).unwrap();

    // The tablet writes meanwhile: its times count from the same origin, its own 5 ms spacing
    // only among its samples.
    session.records(&[pen(1, 1, 800), pen(2, 2, 800)], T0 + 20.0, &mut out);
    // The computer again, between the tablet's two samples of a burst: neither pushes the other.
    let input = ComputerPenInput::Points {
        stroke: 1,
        samples: vec![sample(4.0, 130.0)],
        now: 130.0,
    };
    session.computer_call(input, T0 + 21.0, &mut out).unwrap();
    session.records(&[pen(3, 3, 800), pen(3, 3, 0)], T0 + 21.0, &mut out);
    session
        .computer_call(end(1, Vec::new()), T0 + 30.0, &mut out)
        .unwrap();

    let strokes = &session.pages()[0].note.strokes;
    let times = |i: usize| strokes[i].points.iter().map(|p| p.t).collect::<Vec<_>>();
    // The tablet's stroke ended first: it is first in the list.
    assert_eq!(strokes[0].tool, Tool::Ballpoint);
    assert_eq!(times(0), [25, 30, 35]);
    assert_eq!(times(1), [0, 8, 16, 31]);
    // `recv` is each sample's wall time.
    let recv: Vec<f64> = points_recv(&out);
    assert_eq!(recv, [T0 - 10.0, T0 - 2.0, T0 + 6.0, T0 + 21.0]);
}

/// The `recv` of every computer point sent.
fn points_recv(out: &[LiveEvent]) -> Vec<f64> {
    out.iter()
        .flat_map(|event| match event {
            LiveEvent::StrokeStart {
                stroke,
                pen: COMPUTER,
                ..
            } => stroke.points.iter().map(|p| p.recv).collect(),
            LiveEvent::Points {
                points,
                pen: COMPUTER,
                ..
            } => points.iter().map(|p| p.recv).collect(),
            _ => Vec::new(),
        })
        .collect()
}

#[test]
fn times_never_go_back_within_a_computer_stroke() {
    let mut session = running();
    let mut out = Vec::new();
    session
        .computer_call(begin(1, 0, at_once(&[1.0])), T0, &mut out)
        .unwrap();
    // A sample stamped before the previous one (another clock, a late event): the previous time.
    let late = ComputerPenInput::Points {
        stroke: 1,
        samples: vec![sample(2.0, 0.0)],
        now: 50.0,
    };
    session.computer_call(late, T0 + 20.0, &mut out).unwrap();
    // The session's clock went back: no time before the previous one either.
    session
        .computer_call(points(1, at_once(&[3.0])), T0 - 1000.0, &mut out)
        .unwrap();
    session
        .computer_call(end(1, Vec::new()), T0 + 40.0, &mut out)
        .unwrap();
    let times: Vec<u32> = session.pages()[0].note.strokes[0]
        .points
        .iter()
        .map(|p| p.t)
        .collect();
    assert_eq!(times, [0, 0, 0]);
}

#[test]
fn two_strokes_in_progress_at_once_each_with_its_own_events() {
    let mut session = running();
    let mut out = Vec::new();
    session.records(&[pen(5000, 5000, 800), pen(5010, 5000, 800)], T0, &mut out);
    session
        .computer_call(begin(3, 0, at_once(&[100.0])), T0 + 1.0, &mut out)
        .unwrap();
    session.records(&[pen(5020, 5000, 800)], T0 + 5.0, &mut out);
    session
        .computer_call(points(3, at_once(&[110.0])), T0 + 6.0, &mut out)
        .unwrap();
    // A snapshot in the middle is followed by both strokes so far, the tablet's first.
    let at = out.len();
    session.set_style(LiveStyle::chalk(), &mut out);
    assert!(matches!(out[at], LiveEvent::Snapshot { .. }));
    assert_eq!(
        stroke_events(&out[at..]),
        [("start", TABLET, None), ("start", COMPUTER, Some(3))]
    );
    let [
        LiveEvent::StrokeStart { stroke: tablet, .. },
        LiveEvent::StrokeStart {
            stroke: computer, ..
        },
    ] = &out[at + 1..]
    else {
        panic!("{out:?}");
    };
    assert_eq!((tablet.points.len(), computer.points.len()), (3, 2));
    assert_eq!(session.in_progress().len(), 2);

    session.records(&[pen(5030, 5000, 0)], T0 + 10.0, &mut out);
    session
        .computer_call(end(3, at_once(&[120.0])), T0 + 11.0, &mut out)
        .unwrap();
    assert_eq!(
        stroke_events(&out[..at]),
        [
            ("start", TABLET, None),
            ("start", COMPUTER, Some(3)),
            ("points", TABLET, None),
            ("points", COMPUTER, Some(3)),
        ]
    );
    assert_eq!(
        stroke_events(&out[at + 3..]),
        [
            ("end", TABLET, None),
            ("points", COMPUTER, Some(3)),
            ("end", COMPUTER, Some(3))
        ]
    );
    // Each kept its own points and ink, in the order they ended.
    let strokes = &session.pages()[0].note.strokes;
    assert_eq!(strokes.len(), 2);
    assert_eq!(
        (strokes[0].tool, strokes[0].points.len()),
        (Tool::Ballpoint, 3)
    );
    assert_eq!(
        (strokes[1].tool, strokes[1].points.len()),
        (Tool::FeltTip, 3)
    );
}

#[test]
fn a_page_change_a_quarter_turn_and_the_stop_end_the_computer_s_stroke_and_keep_it() {
    // A page change.
    let mut session = running();
    let mut out = Vec::new();
    session
        .computer_call(begin(1, 0, at_once(&[100.0, 200.0])), T0, &mut out)
        .unwrap();
    out.clear();
    session.command(
        LiveCommandKind::NextPage,
        LiveButton::NewPage,
        T0 as i64,
        &mut out,
    );
    assert_eq!(stroke_events(&out), [("end", COMPUTER, Some(1))]);
    let snapshot = out
        .iter()
        .position(|e| matches!(e, LiveEvent::Snapshot { .. }))
        .unwrap();
    let ended = out
        .iter()
        .position(|e| matches!(e, LiveEvent::StrokeEnd { .. }))
        .unwrap();
    assert!(ended < snapshot, "ended on the page it was drawn on");
    assert_eq!(session.pages()[0].note.strokes.len(), 1);
    assert_eq!(
        session.computer_call(points(1, at_once(&[300.0])), T0, &mut out),
        Ok(false)
    );
    // The tablet's button set to « nouvelle page » too.
    session
        .computer_call(begin(2, 1, at_once(&[100.0])), T0, &mut out)
        .unwrap();
    session.command(
        LiveCommandKind::Button,
        LiveButton::NewPage,
        T0 as i64,
        &mut out,
    );
    assert_eq!(session.pages()[1].note.strokes.len(), 1);
    assert_eq!(
        session.computer_call(end(2, Vec::new()), T0, &mut out),
        Ok(false)
    );
    // A new layer from the tablet's button does not end it.
    session
        .computer_call(begin(3, 2, at_once(&[100.0])), T0, &mut out)
        .unwrap();
    session.command(
        LiveCommandKind::NewLayer,
        LiveButton::NewLayer,
        T0 as i64,
        &mut out,
    );
    assert_eq!(
        session.computer_call(points(3, at_once(&[110.0])), T0, &mut out),
        Ok(true)
    );
    session
        .computer_call(ComputerPenInput::Cancel { stroke: 3 }, T0, &mut out)
        .unwrap();

    // A quarter turn: the stroke ends before the page turns, and turns with its ink.
    let mut session = running();
    session
        .computer_call(begin(1, 0, at_once(&[1000.0])), T0, &mut out)
        .unwrap();
    out.clear();
    session.reorient(29_700, 21_600, Orientation::PortraitLeft, &mut out);
    assert_eq!(stroke_events(&out), [("end", COMPUTER, Some(1))]);
    let point = session.pages()[0].note.strokes[0].points[0];
    // (x, y) -> (21 600 - y, x).
    assert_eq!((point.x, point.y), (21_500.0, 1000.0));
    assert_eq!(
        session.computer_call(points(1, at_once(&[1.0])), T0, &mut out),
        Ok(false)
    );

    // A lost link ends the tablet's stroke, not the computer's: it writes while the session runs.
    let mut session = running();
    session.records(&[pen(1, 1, 800)], T0, &mut out);
    session
        .computer_call(begin(1, 0, at_once(&[100.0])), T0, &mut out)
        .unwrap();
    out.clear();
    session.set_state(LiveSessionState::Reconnecting, &mut out);
    assert_eq!(
        stroke_events(&out),
        [("end", TABLET, None), ("start", COMPUTER, Some(1))]
    );
    assert_eq!(
        session.computer_call(points(1, at_once(&[110.0])), T0, &mut out),
        Ok(true)
    );
    assert_eq!(
        session.computer_call(end(1, Vec::new()), T0, &mut out),
        Ok(true)
    );
    session
        .computer_call(begin(2, 0, at_once(&[300.0])), T0, &mut out)
        .unwrap();

    // The stop: the stroke is kept, then the history goes as for any stop.
    out.clear();
    session.set_state(LiveSessionState::Idle, &mut out);
    assert_eq!(stroke_events(&out), [("end", COMPUTER, Some(2))]);
    assert_eq!(session.pages()[0].note.strokes.len(), 3);
    let Some(LiveEvent::Snapshot { snapshot }) = out.last() else {
        panic!("{out:?}");
    };
    assert!(!snapshot.can_undo);
    // Nothing more is taken.
    assert_eq!(
        session.computer_call(begin(3, 0, at_once(&[1.0])), T0, &mut out),
        Ok(false)
    );
    assert_eq!(session.pages()[0].note.strokes.len(), 3);
}

#[test]
fn a_clearing_and_a_cancel_drop_the_computer_s_stroke() {
    let mut session = running();
    let mut out = Vec::new();
    session.computer_stroke(1, 100.0, &mut out);
    session
        .computer_call(begin(2, 0, at_once(&[500.0])), T0, &mut out)
        .unwrap();
    out.clear();
    session.command(
        LiveCommandKind::ClearPage,
        LiveButton::NewPage,
        T0 as i64,
        &mut out,
    );
    // The window is told which gesture went, before the snapshot replaces the page.
    assert!(matches!(
        &out[..],
        [
            LiveEvent::StrokeCancel {
                page_index: 0,
                pen: COMPUTER,
                id: Some(2),
                hidden: false
            },
            LiveEvent::Snapshot { .. }
        ]
    ));
    assert!(session.pages()[0].note.strokes.is_empty());
    assert!(session.in_progress().is_empty());
    assert_eq!(
        session.computer_call(points(2, at_once(&[510.0])), T0, &mut out),
        Ok(false)
    );

    // Cancelled by the window: nothing joins the page, no step.
    let can_undo = session.snapshot().can_undo;
    session
        .computer_call(begin(3, 0, at_once(&[600.0])), T0, &mut out)
        .unwrap();
    out.clear();
    assert_eq!(
        session.computer_call(ComputerPenInput::Cancel { stroke: 9 }, T0, &mut out),
        Ok(false)
    );
    assert!(out.is_empty(), "not that gesture");
    assert_eq!(
        session.computer_call(ComputerPenInput::Cancel { stroke: 3 }, T0, &mut out),
        Ok(true)
    );
    assert_eq!(stroke_events(&out), [("cancel", COMPUTER, Some(3))]);
    assert!(session.pages()[0].note.strokes.is_empty());
    assert_eq!(session.snapshot().can_undo, can_undo);
}

#[test]
fn a_new_gesture_ends_the_one_in_progress_and_other_numbers_are_ignored() {
    let mut session = running();
    let mut out = Vec::new();
    session
        .computer_call(begin(1, 0, at_once(&[100.0])), T0, &mut out)
        .unwrap();
    assert_eq!(
        session.computer_call(points(2, at_once(&[110.0])), T0, &mut out),
        Ok(false)
    );
    out.clear();
    session
        .computer_call(begin(2, 0, at_once(&[200.0])), T0, &mut out)
        .unwrap();
    assert_eq!(
        stroke_events(&out),
        [("end", COMPUTER, Some(1)), ("start", COMPUTER, Some(2))]
    );
    assert_eq!(
        session.computer_call(points(1, at_once(&[120.0])), T0, &mut out),
        Ok(false)
    );
    session
        .computer_call(end(2, Vec::new()), T0, &mut out)
        .unwrap();
    let starts: Vec<f32> = session.pages()[0]
        .note
        .strokes
        .iter()
        .map(|s| s.points[0].x)
        .collect();
    assert_eq!(starts, [100.0, 200.0]);
}

#[test]
fn a_begin_on_another_page_or_a_hidden_layer_is_refused_and_changes_nothing() {
    let mut session = running();
    let mut out = Vec::new();
    let error = session
        .computer_call(begin(1, 1, at_once(&[1.0])), T0, &mut out)
        .unwrap_err();
    assert_eq!(
        (error.clone(), error.code()),
        (LiveEditError::Stale, "liveEditStale")
    );

    // The computer's layer hidden: its pen does not write there.
    session
        .layer_op(
            CheckedLayerOp::Visible {
                layer: 0,
                visible: false,
            },
            &mut out,
        )
        .unwrap();
    out.clear();
    let error = session
        .computer_call(begin(1, 0, at_once(&[1.0])), T0, &mut out)
        .unwrap_err();
    assert_eq!(
        (error.clone(), error.code()),
        (LiveEditError::LayerHidden, "liveLayerHidden")
    );
    assert!(out.is_empty());
    assert!(session.in_progress().is_empty());
    assert!(
        session.pages()[0].fresh_for_test(),
        "not even a time origin"
    );
    // A stroke in progress stays when a begin is refused.
    session
        .layer_op(
            CheckedLayerOp::Visible {
                layer: 0,
                visible: true,
            },
            &mut out,
        )
        .unwrap();
    session
        .computer_call(begin(1, 0, at_once(&[1.0])), T0, &mut out)
        .unwrap();
    assert!(
        session
            .computer_call(begin(2, 3, at_once(&[1.0])), T0, &mut out)
            .is_err()
    );
    assert_eq!(
        session.computer_call(points(1, at_once(&[2.0])), T0, &mut out),
        Ok(true)
    );
}

#[test]
fn a_stroke_whose_layer_is_hidden_meanwhile_goes_on_unseen() {
    let mut session = running();
    let mut out = Vec::new();
    session
        .computer_call(begin(1, 0, at_once(&[1.0])), T0, &mut out)
        .unwrap();
    out.clear();
    session
        .layer_op(
            CheckedLayerOp::Visible {
                layer: 0,
                visible: false,
            },
            &mut out,
        )
        .unwrap();
    // The snapshot is followed by the stroke so far, hidden.
    assert!(matches!(
        &out[..],
        [
            LiveEvent::Snapshot { .. },
            LiveEvent::StrokeStart {
                hidden: true,
                id: Some(1),
                ..
            }
        ]
    ));
    out.clear();
    assert_eq!(
        session.computer_call(points(1, at_once(&[2.0])), T0, &mut out),
        Ok(true)
    );
    session
        .computer_call(end(1, Vec::new()), T0, &mut out)
        .unwrap();
    assert!(matches!(&out[0], LiveEvent::Points { hidden: true, .. }));
    assert!(matches!(&out[1], LiveEvent::StrokeEnd { hidden: true, .. }));
    assert_eq!(session.pages()[0].note.strokes.len(), 1);
}

#[test]
fn a_stroke_reaching_the_most_points_ends_there_and_is_kept() {
    let mut session = running();
    let mut out = Vec::new();
    let batch = at_once(&[50.0; MAX_PEN_SAMPLES]);
    session
        .computer_call(begin(1, 0, batch.clone()), T0, &mut out)
        .unwrap();
    let mut answers = Vec::new();
    for _ in 0..MAX_COMPUTER_POINTS / MAX_PEN_SAMPLES + 1 {
        answers.push(
            session
                .computer_call(points(1, batch.clone()), T0, &mut out)
                .unwrap(),
        );
    }
    // Every call goes on until the one that reaches the limit; then the gesture is over.
    let last = answers.iter().position(|going_on| !going_on).unwrap();
    assert!(answers[..last].iter().all(|going_on| *going_on));
    assert_eq!(last, MAX_COMPUTER_POINTS / MAX_PEN_SAMPLES - 1);
    let strokes = &session.pages()[0].note.strokes;
    assert_eq!(strokes.len(), 1);
    assert_eq!(strokes[0].points.len(), MAX_COMPUTER_POINTS);
    assert!(
        out.iter()
            .any(|e| matches!(e, LiveEvent::StrokeEnd { pen: COMPUTER, .. }))
    );
}

impl crate::live::session::Page {
    /// No stroke was started on the page yet.
    fn fresh_for_test(&self) -> bool {
        self.time_origin().is_none()
    }
}
