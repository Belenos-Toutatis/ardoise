//! Tests of the editor's projection (docs/design/paper-projection-narration.md §2.3, §2.9): the
//! events each update makes, the refusals, the window's subscription and the editor's watch, the
//! windows' numbers, and the wire format the projection window reads (live's).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ardoise_core::format::BackgroundFile;
use ardoise_core::note::{BackgroundImage, ImageRect, Note, PageSize, Paper};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use serde_json::json;

use super::*;
use crate::background::{Staged, Staging};
use crate::library::blank_note;
use crate::live::model::{LiveBatch, LiveEvent, LivePen, LiveSessionState, wall_now_ms};
use crate::presentation::MonitorInfo;

/// The window: keeps the batches it receives; refuses them once `closed`.
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
    fn batches(&self) -> Vec<LiveBatch> {
        self.batches.lock().unwrap().clone()
    }

    /// The events of every batch, and forgets them.
    fn take(&self) -> Vec<LiveEvent> {
        std::mem::take(&mut *self.batches.lock().unwrap())
            .into_iter()
            .flat_map(|batch| batch.events)
            .collect()
    }
}

/// The editor's watch: keeps what it is told.
#[derive(Default)]
struct Watch {
    told: Mutex<Vec<ProjectionStatus>>,
    closed: AtomicBool,
}

impl StatusSink for Watch {
    fn send(&self, status: ProjectionStatus) -> Result<(), String> {
        if self.closed.load(Ordering::SeqCst) {
            return Err("the editor went".into());
        }
        self.told.lock().unwrap().push(status);
        Ok(())
    }
}

impl Watch {
    fn told(&self) -> Vec<ProjectionStatus> {
        self.told.lock().unwrap().clone()
    }
}

fn projector() -> MonitorInfo {
    let mut monitor = MonitorInfo::new("HDMI", 1920, 0, 1920, 1080, 1.0, false, None);
    monitor.chosen = true;
    monitor
}

fn other_screen() -> MonitorInfo {
    let mut monitor = MonitorInfo::new("DP", -1280, 0, 1280, 1024, 1.25, false, None);
    monitor.chosen = true;
    monitor
}

/// A projection whose window is open on the projector.
fn open() -> ProjectionRuntime {
    let projection = ProjectionRuntime::default();
    let window = projection.next_window();
    assert!(projection.opened(window, projector()));
    projection
}

/// The window, subscribed (its first batch forgotten).
fn subscribed(projection: &ProjectionRuntime) -> Arc<Window> {
    let window = Arc::new(Window::default());
    projection.subscribe(window.clone());
    window.take();
    window
}

fn stroke(n: u16) -> Stroke {
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 50.0,
        layer: 0,
        points: (0..3)
            .map(|i| Point {
                x: 1000.0 + 100.0 * f32::from(n),
                y: 1000.0 + 10.0 * i as f32,
                pressure: 0.5,
                t: u32::from(n) * 100 + i,
            })
            .collect(),
    }
}

fn note(strokes: u16) -> Note {
    let mut note = blank_note("Leçon", "Calque 1", 1_792_843_200_000);
    note.strokes = (0..strokes).map(stroke).collect();
    note
}

fn whole_page() -> ProjectionView {
    ProjectionView {
        zoom: 1.0,
        x: 14_850.0,
        y: 10_800.0,
    }
}

fn page(note: Note) -> ProjectionUpdate {
    ProjectionUpdate::Page {
        note,
        image_key: None,
        view: whole_page(),
    }
}

fn update(projection: &ProjectionRuntime, update: ProjectionUpdate) -> CommandResult<()> {
    projection.update(update, &Staging::default())
}

fn point(x: f64, wall: f64) -> ProjectionPoint {
    ProjectionPoint {
        x,
        y: 2000.0,
        p: 0.5,
        t: 40.0,
        wall,
    }
}

fn begin(id: f64, points: Vec<ProjectionPoint>) -> ProjectionUpdate {
    ProjectionUpdate::StrokeBegin {
        id,
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: 50.0,
        layer: 0.0,
        points,
    }
}

fn strokes(keep: f64, tail: Vec<Stroke>, ended: Option<f64>) -> ProjectionUpdate {
    ProjectionUpdate::Strokes { keep, tail, ended }
}

fn code(result: CommandResult<()>) -> &'static str {
    result.unwrap_err().code()
}

fn snapshot_of(event: &LiveEvent) -> &LiveSnapshot {
    match event {
        LiveEvent::Snapshot { snapshot } => snapshot,
        other => panic!("not a snapshot: {other:?}"),
    }
}

/// What a window subscribing now would start from (without replacing the subscription).
fn shown(projection: &ProjectionRuntime) -> LiveSnapshot {
    lock(&projection.state).snapshot()
}

/// A new window subscribes: the snapshot it is answered, and its first batch.
fn subscribe_events(projection: &ProjectionRuntime) -> (Arc<LiveSnapshot>, Vec<LiveEvent>) {
    let window = Arc::new(Window::default());
    let snapshot = projection.subscribe(window.clone());
    (snapshot, window.take())
}

#[test]
fn a_page_is_the_snapshot_of_a_one_page_session() {
    let projection = open();
    let window = subscribed(&projection);
    let mut lesson = note(3);
    lesson.background.paper = Paper::Cream;
    update(
        &projection,
        ProjectionUpdate::Page {
            note: lesson.clone(),
            image_key: None,
            view: ProjectionView {
                zoom: 2.0,
                x: 5000.0,
                y: 4000.0,
            },
        },
    )
    .unwrap();
    let events = window.take();
    assert_eq!(events.len(), 1, "{events:?}");
    let snapshot = snapshot_of(&events[0]);
    assert_eq!(snapshot.state, LiveSessionState::Live);
    // The note itself, its own id: the window is local.
    assert_eq!(snapshot.page, lesson);
    assert_eq!((snapshot.page_index, snapshot.page_count), (0, 1));
    assert!(!snapshot.can_turn_page);
    assert_eq!((snapshot.layer, snapshot.computer_layer), (0, 0));
    assert_eq!(snapshot.style, LiveStyle::default());
    assert_eq!(
        snapshot.view,
        LiveViewState {
            zoom: 2.0,
            x: 5000.0,
            y: 4000.0,
            board: false,
            laser: false,
        }
    );
    assert_eq!(snapshot.hover, None);
    assert_eq!(snapshot.presentation, Some(projector()));
    assert!(snapshot.saved.is_empty());
    assert!(!snapshot.can_undo && !snapshot.can_redo);
    assert_eq!(snapshot.page_paper, Paper::Cream);
    assert_eq!(snapshot.image_pixels, None);
    assert!(snapshot.revision > 0);
}

#[test]
fn the_view_is_kept_within_the_page_and_the_zooms() {
    let projection = open();
    let window = subscribed(&projection);
    update(
        &projection,
        ProjectionUpdate::Page {
            note: note(0),
            image_key: None,
            view: ProjectionView {
                zoom: 40.0,
                x: -5.0,
                y: f64::NAN,
            },
        },
    )
    .unwrap();
    let events = window.take();
    let view = snapshot_of(&events[0]).view;
    assert_eq!((view.zoom, view.x, view.y), (8.0, 0.0, 10_800.0));
    // A view: live's `view`, sanitized the same way; the same view again sends nothing.
    let moved = ProjectionView {
        zoom: 0.5,
        x: 40_000.0,
        y: 3000.0,
    };
    update(&projection, ProjectionUpdate::View { view: moved }).unwrap();
    update(&projection, ProjectionUpdate::View { view: moved }).unwrap();
    let events = window.take();
    assert_eq!(
        events,
        vec![LiveEvent::View {
            view: LiveViewState {
                zoom: 1.0,
                x: 29_700.0,
                y: 3000.0,
                board: false,
                laser: false,
            }
        }]
    );
    // A late window starts from it.
    assert_eq!(subscribe_events(&projection).0.view.x, 29_700.0);
}

#[test]
fn the_snapshot_gives_the_size_of_the_staged_image() {
    let staging = Staging::default();
    let key = staging.put(Staged {
        file: BackgroundFile {
            extension: "png".into(),
            bytes: vec![1, 2, 3, 4],
        },
        width: 1240,
        height: 1754,
    });
    let mut sheet = note(1);
    sheet.background_image = Some(BackgroundImage {
        name: "fiche.pdf".into(),
        pdf_page: Some(2),
        rect: ImageRect {
            x: 0.0,
            y: 0.0,
            width: 14_000.0,
            height: 21_600.0,
        },
        quarter_turns: 0,
        opacity: 1.0,
    });
    let projection = open();
    let shown = |note: Note, key: Option<&str>| {
        projection
            .update(
                ProjectionUpdate::Page {
                    note,
                    image_key: key.map(str::to_owned),
                    view: whole_page(),
                },
                &staging,
            )
            .unwrap();
        subscribe_events(&projection).0.image_pixels.clone()
    };
    assert_eq!(
        shown(sheet.clone(), Some(key.as_str())),
        Some(ImagePixels {
            key: key.clone(),
            width: 1240,
            height: 1754,
        })
    );
    // Not staged (another note's image being staged): the paper alone.
    assert_eq!(shown(sheet.clone(), Some("0000-4")), None);
    assert_eq!(shown(sheet, None), None);
    // A key without an image on the page: nothing to place.
    assert_eq!(shown(note(1), Some(key.as_str())), None);
}

#[test]
fn strokes_replace_the_tail_of_the_page() {
    let projection = open();
    update(&projection, page(note(4))).unwrap();
    let window = subscribed(&projection);
    let before = shown(&projection).revision;
    // Undo the last two strokes and write another one.
    update(&projection, strokes(2.0, vec![stroke(9)], None)).unwrap();
    let events = window.take();
    let [
        LiveEvent::StrokesEdited {
            page_index,
            revision,
            removed,
            inserted,
            recoloured,
            can_undo,
            can_redo,
            viewers,
        },
    ] = events.as_slice()
    else {
        panic!("{events:?}");
    };
    assert_eq!(*page_index, 0);
    assert_eq!(*revision, before + 1);
    assert_eq!(removed, &vec![2, 3]);
    assert_eq!(
        inserted.as_slice(),
        &[PlacedStroke {
            index: 2,
            stroke: stroke(9),
        }]
    );
    assert!(recoloured.is_empty() && !can_undo && !can_redo && viewers.is_none());
    // A late window draws the page as it is now.
    let snapshot = shown(&projection);
    assert_eq!(snapshot.page.strokes, vec![stroke(0), stroke(1), stroke(9)]);
    assert_eq!(snapshot.revision, before + 1);
    // Appending: nothing removed.
    update(&projection, strokes(3.0, vec![stroke(10)], None)).unwrap();
    let events = window.take();
    assert!(
        matches!(&events[..], [LiveEvent::StrokesEdited { removed, inserted, .. }]
            if removed.is_empty() && inserted[0].index == 3),
        "{events:?}"
    );
    // Nothing changed: no event.
    update(&projection, strokes(4.0, Vec::new(), None)).unwrap();
    assert!(window.take().is_empty());
    // Everything erased.
    update(&projection, strokes(0.0, Vec::new(), None)).unwrap();
    let events = window.take();
    assert!(
        matches!(&events[..], [LiveEvent::StrokesEdited { removed, inserted, .. }]
            if removed == &vec![0, 1, 2, 3] && inserted.is_empty()),
        "{events:?}"
    );
}

#[test]
fn the_stroke_in_progress_is_a_computer_stroke_with_the_pointers_time() {
    let projection = open();
    update(&projection, page(note(0))).unwrap();
    let window = subscribed(&projection);
    let wall = wall_now_ms();
    update(
        &projection,
        ProjectionUpdate::StrokeBegin {
            id: 7.0,
            tool: Tool::FeltTip,
            color: Rgba::WHITE,
            width: f32::NAN,
            layer: 1.0,
            points: vec![ProjectionPoint {
                x: 99_999.0,
                y: -50_000.0,
                p: 1.5,
                t: 12.7,
                wall,
            }],
        },
    )
    .unwrap();
    update(
        &projection,
        ProjectionUpdate::StrokePoints {
            id: 7.0,
            points: vec![point(1000.0, wall + 4.0), point(1100.0, wall + 8.0)],
        },
    )
    .unwrap();
    let events = window.take();
    let [
        LiveEvent::StrokeStart {
            page_index: 0,
            stroke,
            pen: LivePen::Computer,
            id: Some(7),
            hidden: false,
        },
        LiveEvent::Points {
            page_index: 0,
            points,
            pen: LivePen::Computer,
            id: Some(7),
            hidden: false,
        },
    ] = events.as_slice()
    else {
        panic!("{events:?}");
    };
    assert_eq!(
        (stroke.tool, stroke.color, stroke.layer),
        (Tool::FeltTip, Rgba::WHITE, 1)
    );
    // A width the tool can draw.
    assert_eq!(stroke.width, Tool::FeltTip.default_width());
    // Within one page around the page, the pressure within 0..1, the time floored, `recv` the
    // pointer's wall time.
    assert_eq!(
        stroke.points,
        vec![LivePoint {
            x: 59_400.0,
            y: -21_600.0,
            p: 1.0,
            t: 12,
            recv: wall,
        }]
    );
    assert_eq!(points.len(), 2);
    assert_eq!((points[0].x, points[0].recv), (1000.0, wall + 4.0));
    assert_eq!(points[1].recv, wall + 8.0);
    // Another stroke's points: ignored.
    update(
        &projection,
        ProjectionUpdate::StrokePoints {
            id: 8.0,
            points: vec![point(1.0, wall)],
        },
    )
    .unwrap();
    assert!(window.take().is_empty());
    // Live's wire format: the window's `LiveState` reads it as the computer's pen.
    let start = serde_json::to_value(&events[0]).unwrap();
    assert_eq!(start["type"], "strokeStart");
    assert_eq!(start["pageIndex"], 0);
    assert_eq!(start["pen"], "computer");
    assert_eq!(start["id"], 7);
    assert_eq!(start["stroke"]["layer"], 1);
    assert_eq!(start["stroke"]["points"][0]["recv"], json!(wall));
    let more = serde_json::to_value(&events[1]).unwrap();
    assert_eq!(more["type"], "points");
    assert_eq!(more["pen"], "computer");
}

#[test]
fn an_ended_stroke_joins_the_page_in_the_same_batch() {
    let projection = open();
    update(&projection, page(note(2))).unwrap();
    let window = subscribed(&projection);
    update(&projection, begin(3.0, vec![point(500.0, wall_now_ms())])).unwrap();
    window.take();
    update(&projection, strokes(2.0, vec![stroke(5)], Some(3.0))).unwrap();
    let batches = window.batches();
    assert_eq!(batches.len(), 1, "one batch: no frame without the stroke");
    let events = &batches[0].events;
    assert!(
        matches!(
            &events[..],
            [
                LiveEvent::StrokesEdited { inserted, .. },
                LiveEvent::StrokeCancel {
                    page_index: 0,
                    pen: LivePen::Computer,
                    id: Some(3),
                    hidden: false,
                },
            ] if inserted.len() == 1
        ),
        "{events:?}"
    );
    window.take();
    // It is no longer in progress: a late window gets the page alone.
    let (_, events) = subscribe_events(&projection);
    assert_eq!(events.len(), 1);
    assert_eq!(snapshot_of(&events[0]).page.strokes.len(), 3);
}

#[test]
fn an_end_for_another_stroke_keeps_the_one_in_progress() {
    let projection = open();
    update(&projection, page(note(0))).unwrap();
    update(&projection, begin(4.0, vec![point(500.0, 1.0)])).unwrap();
    let window = subscribed(&projection);
    update(&projection, strokes(0.0, vec![stroke(1)], Some(3.0))).unwrap();
    let events = window.take();
    assert!(
        matches!(&events[..], [LiveEvent::StrokesEdited { .. }]),
        "{events:?}"
    );
    let (_, events) = subscribe_events(&projection);
    assert!(matches!(
        events[1],
        LiveEvent::StrokeStart { id: Some(4), .. }
    ));
}

#[test]
fn a_begin_replaces_the_stroke_in_progress_and_a_cancel_drops_it() {
    let projection = open();
    update(&projection, page(note(0))).unwrap();
    let window = subscribed(&projection);
    update(&projection, begin(1.0, vec![point(500.0, 1.0)])).unwrap();
    window.take();
    update(&projection, begin(2.0, vec![point(600.0, 2.0)])).unwrap();
    let events = window.take();
    // The first stroke is dropped, not finished: it is not on the page.
    assert!(
        matches!(
            &events[..],
            [
                LiveEvent::StrokeCancel {
                    id: Some(1),
                    pen: LivePen::Computer,
                    ..
                },
                LiveEvent::StrokeStart {
                    id: Some(2),
                    pen: LivePen::Computer,
                    ..
                },
            ]
        ),
        "{events:?}"
    );
    // Another stroke's cancel: nothing.
    update(&projection, ProjectionUpdate::StrokeCancel { id: 1.0 }).unwrap();
    assert!(window.take().is_empty());
    update(&projection, ProjectionUpdate::StrokeCancel { id: 2.0 }).unwrap();
    let events = window.take();
    assert!(
        matches!(
            &events[..],
            [LiveEvent::StrokeCancel {
                id: Some(2),
                pen: LivePen::Computer,
                ..
            }]
        ),
        "{events:?}"
    );
    // Points of the dropped stroke: ignored.
    update(
        &projection,
        ProjectionUpdate::StrokePoints {
            id: 2.0,
            points: vec![point(700.0, 3.0)],
        },
    )
    .unwrap();
    assert!(window.take().is_empty());
    assert_eq!(subscribe_events(&projection).1.len(), 1);
}

#[test]
fn a_page_keeps_the_stroke_in_progress() {
    let projection = open();
    update(&projection, page(note(0))).unwrap();
    update(&projection, begin(5.0, vec![point(500.0, 1.0)])).unwrap();
    let window = subscribed(&projection);
    update(&projection, page(note(2))).unwrap();
    let events = window.take();
    assert!(
        matches!(
            &events[..],
            [LiveEvent::Snapshot { .. }, LiveEvent::StrokeStart { id: Some(5), stroke, .. }]
                if stroke.points.len() == 1
        ),
        "{events:?}"
    );
}

#[test]
fn the_stroke_in_progress_stops_growing_at_its_limit() {
    let projection = open();
    update(&projection, page(note(0))).unwrap();
    let points: Vec<ProjectionPoint> = (0..MAX_PROJECTION_POINTS)
        .map(|i| point(i as f64, 1.0))
        .collect();
    update(&projection, begin(1.0, points.clone())).unwrap();
    let calls = MAX_PROJECTED_STROKE_POINTS / MAX_PROJECTION_POINTS;
    for _ in 0..calls {
        update(
            &projection,
            ProjectionUpdate::StrokePoints {
                id: 1.0,
                points: points.clone(),
            },
        )
        .unwrap();
    }
    let window = subscribed(&projection);
    update(
        &projection,
        ProjectionUpdate::StrokePoints {
            id: 1.0,
            points: points.clone(),
        },
    )
    .unwrap();
    assert!(window.take().is_empty(), "beyond the limit: nothing sent");
    let (_, events) = subscribe_events(&projection);
    let LiveEvent::StrokeStart { stroke, .. } = &events[1] else {
        panic!("{events:?}");
    };
    assert_eq!(stroke.points.len(), MAX_PROJECTED_STROKE_POINTS);
}

#[test]
fn what_the_projection_does_not_hold_is_stale() {
    // Closed: nothing is kept.
    let closed = ProjectionRuntime::default();
    assert_eq!(code(update(&closed, page(note(1)))), STALE);
    assert!(subscribe_events(&closed).0.page.strokes.is_empty());

    let projection = open();
    // No page yet.
    assert_eq!(
        code(update(&projection, strokes(0.0, Vec::new(), None))),
        STALE
    );
    assert_eq!(
        code(update(
            &projection,
            ProjectionUpdate::View { view: whole_page() }
        )),
        STALE
    );
    assert_eq!(
        code(update(&projection, begin(1.0, vec![point(1.0, 1.0)]))),
        STALE
    );
    assert_eq!(
        code(update(
            &projection,
            ProjectionUpdate::StrokePoints {
                id: 1.0,
                points: Vec::new()
            }
        )),
        STALE
    );
    // Nothing in progress to cancel: no harm.
    update(&projection, ProjectionUpdate::StrokeCancel { id: 1.0 }).unwrap();
    // More strokes kept than the page holds.
    update(&projection, page(note(2))).unwrap();
    let window = subscribed(&projection);
    assert_eq!(
        code(update(&projection, strokes(3.0, vec![stroke(1)], None))),
        STALE
    );
    assert!(window.take().is_empty());
    // The page is untouched: the feed sends it again.
    assert_eq!(subscribe_events(&projection).0.page.strokes.len(), 2);
}

#[test]
fn numbers_that_make_no_sense_are_invalid() {
    let projection = open();
    update(&projection, page(note(2))).unwrap();
    let window = subscribed(&projection);
    for keep in [1.5, -1.0, f64::NAN, f64::INFINITY, 5e9] {
        assert_eq!(
            code(update(&projection, strokes(keep, Vec::new(), None))),
            INVALID,
            "{keep}"
        );
    }
    assert_eq!(
        code(update(&projection, strokes(2.0, Vec::new(), Some(0.5)))),
        INVALID
    );
    assert_eq!(
        code(update(&projection, begin(1.5, vec![point(1.0, 1.0)]))),
        INVALID
    );
    let mut on_a_layer = begin(1.0, vec![point(1.0, 1.0)]);
    if let ProjectionUpdate::StrokeBegin { layer, .. } = &mut on_a_layer {
        *layer = -2.0;
    }
    assert_eq!(code(update(&projection, on_a_layer)), INVALID);
    for bad in [
        ProjectionPoint {
            x: f64::NAN,
            ..point(1.0, 1.0)
        },
        ProjectionPoint {
            p: f64::INFINITY,
            ..point(1.0, 1.0)
        },
        ProjectionPoint {
            wall: f64::NAN,
            ..point(1.0, 1.0)
        },
    ] {
        assert_eq!(code(update(&projection, begin(1.0, vec![bad]))), INVALID);
    }
    let too_many = vec![point(1.0, 1.0); MAX_PROJECTION_POINTS + 1];
    assert_eq!(code(update(&projection, begin(1.0, too_many))), INVALID);
    assert_eq!(
        code(update(
            &projection,
            ProjectionUpdate::StrokeCancel { id: -1.0 }
        )),
        INVALID
    );
    // A page the window cannot draw.
    let mut flat = note(0);
    flat.page = PageSize {
        width: 0,
        height: 100,
    };
    assert_eq!(code(update(&projection, page(flat))), INVALID);
    let mut no_layer = note(0);
    no_layer.layers.clear();
    assert_eq!(code(update(&projection, page(no_layer))), INVALID);
    assert!(window.take().is_empty());
    // Nothing of it was applied.
    let snapshot = subscribe_events(&projection).0;
    assert_eq!(snapshot.page.strokes.len(), 2);
    assert_eq!(snapshot.page.layers.len(), 1);
}

#[test]
fn more_strokes_than_the_limit_are_invalid() {
    // Counted, not built: a million strokes would weigh tens of megabytes in a test.
    stroke_count(MAX_PROJECTION_STROKES, "on the page").unwrap();
    assert_eq!(
        stroke_count(MAX_PROJECTION_STROKES + 1, "on the page")
            .unwrap_err()
            .code(),
        INVALID
    );
    // Every index of a page within the limit fits the events' `u32`.
    assert!(u32::try_from(MAX_PROJECTION_STROKES).is_ok());
}

#[test]
fn a_late_window_gets_the_snapshot_and_the_stroke_in_progress() {
    let projection = open();
    // Nobody subscribed yet: the updates are applied, not sent.
    update(&projection, page(note(1))).unwrap();
    update(&projection, strokes(1.0, vec![stroke(1)], None)).unwrap();
    update(&projection, begin(9.0, vec![point(500.0, 1.0)])).unwrap();
    update(
        &projection,
        ProjectionUpdate::StrokePoints {
            id: 9.0,
            points: vec![point(600.0, 2.0), point(700.0, 3.0)],
        },
    )
    .unwrap();
    let window = Arc::new(Window::default());
    let before = wall_now_ms();
    let snapshot = projection.subscribe(window.clone());
    let after = wall_now_ms();
    let batches = window.batches();
    assert_eq!(batches.len(), 1);
    assert!(before <= batches[0].sent_at && batches[0].sent_at <= after);
    let events = &batches[0].events;
    assert_eq!(snapshot_of(&events[0]), snapshot.as_ref());
    assert_eq!(snapshot.page.strokes, vec![stroke(0), stroke(1)]);
    let LiveEvent::StrokeStart {
        stroke,
        id: Some(9),
        pen: LivePen::Computer,
        ..
    } = &events[1]
    else {
        panic!("{events:?}");
    };
    let recv: Vec<f64> = stroke.points.iter().map(|p| p.recv).collect();
    assert_eq!(recv, vec![1.0, 2.0, 3.0]);
    assert_eq!(events.len(), 2);
}

#[test]
fn without_a_page_the_window_gets_an_idle_empty_page() {
    let projection = open();
    let (snapshot, events) = subscribe_events(&projection);
    assert_eq!(events.len(), 1);
    assert_eq!(snapshot.state, LiveSessionState::Idle);
    assert_eq!(snapshot.page.id, "");
    assert_eq!(snapshot.page.page, ardoise_core::note::A4_LANDSCAPE);
    assert!(snapshot.page.strokes.is_empty());
    assert_eq!(snapshot.page.layers.len(), 1);
    assert_eq!(snapshot.presentation, Some(projector()));
    assert_eq!(snapshot.view, LiveViewState::whole(snapshot.page.page));
    // As the browser stand-in has it (`mock-projection.ts`).
    let wire = serde_json::to_value(snapshot.as_ref()).unwrap();
    assert_eq!(wire["state"], "idle");
    assert_eq!(wire["pagePaper"], "white");
    assert_eq!(wire["imagePixels"], json!(null));
    assert_eq!(wire["canTurnPage"], false);
}

#[test]
fn a_new_subscription_replaces_the_previous_and_a_failed_send_drops_it() {
    let projection = open();
    update(&projection, page(note(0))).unwrap();
    let first = subscribed(&projection);
    let second = subscribed(&projection);
    update(&projection, strokes(0.0, vec![stroke(1)], None)).unwrap();
    assert!(first.take().is_empty());
    assert_eq!(second.take().len(), 1);
    // The window went: dropped at the first failure, the updates go on.
    second.closed.store(true, Ordering::SeqCst);
    update(&projection, strokes(1.0, vec![stroke(2)], None)).unwrap();
    assert!(lock(&projection.state).subscriber.is_none());
    update(&projection, strokes(2.0, vec![stroke(3)], None)).unwrap();
    assert_eq!(subscribe_events(&projection).0.page.strokes.len(), 3);
}

#[test]
fn the_watch_is_told_when_the_window_opens_moves_and_closes() {
    let projection = ProjectionRuntime::default();
    let watch = Arc::new(Watch::default());
    assert_eq!(
        projection.watch(watch.clone()),
        ProjectionStatus { open: None }
    );
    let window = projection.next_window();
    assert!(projection.opened(window, projector()));
    // Opened again on the same screen: nothing new.
    let again = projection.next_window();
    assert!(projection.opened(again, projector()));
    // Moved to another screen.
    let moved = projection.next_window();
    assert!(projection.opened(moved, other_screen()));
    assert_eq!(
        watch.told(),
        vec![
            ProjectionStatus {
                open: Some(projector())
            },
            ProjectionStatus {
                open: Some(other_screen())
            },
        ]
    );
    assert_eq!(projection.status().open, Some(other_screen()));
    // Closed: the page, the stroke and the window's subscription go.
    update(&projection, page(note(2))).unwrap();
    update(&projection, begin(1.0, vec![point(1.0, 1.0)])).unwrap();
    let subscriber = subscribed(&projection);
    projection.closed();
    assert_eq!(watch.told().last(), Some(&ProjectionStatus { open: None }));
    assert_eq!(watch.told().len(), 3);
    {
        let state = lock(&projection.state);
        assert!(state.scene.is_none() && state.current.is_none() && state.subscriber.is_none());
    }
    // Updates in flight are stale; the window's subscription had gone.
    assert_eq!(
        code(update(&projection, strokes(2.0, Vec::new(), None))),
        STALE
    );
    assert!(subscriber.take().is_empty());
    // Closed twice: told once.
    projection.closed();
    assert_eq!(watch.told().len(), 3);
    // The editor went: its watch is dropped at the first failure.
    watch.closed.store(true, Ordering::SeqCst);
    let window = projection.next_window();
    assert!(projection.opened(window, projector()));
    assert!(lock(&projection.state).watcher.is_none());
}

#[test]
fn the_late_destruction_of_an_older_window_keeps_the_new_one() {
    let projection = ProjectionRuntime::default();
    let watch = Arc::new(Watch::default());
    projection.watch(watch.clone());
    let first = projection.next_window();
    assert!(projection.opened(first, projector()));
    // « Arrêter », then « Projeter » again before the first window's destruction is reported.
    projection.closed();
    let second = projection.next_window();
    assert!(projection.opened(second, projector()));
    update(&projection, page(note(1))).unwrap();
    projection.window_destroyed(first);
    assert_eq!(projection.status().open, Some(projector()));
    assert_eq!(subscribe_events(&projection).0.page.strokes.len(), 1);
    // Moved to another screen: the window keeps its handler of the first opening.
    let moved = projection.next_window();
    assert!(projection.opened(moved, other_screen()));
    projection.window_destroyed(second);
    assert_eq!(projection.status().open, Some(other_screen()));
    // Its destruction (Esc in it): closed, the watch told.
    projection.window_destroyed(moved);
    assert_eq!(projection.status().open, None);
    assert_eq!(watch.told().last(), Some(&ProjectionStatus { open: None }));
    assert_eq!(code(update(&projection, page(note(1)))), STALE);
}

#[test]
fn a_window_destroyed_before_its_opening_is_recorded_is_not_open() {
    let projection = ProjectionRuntime::default();
    let window = projection.next_window();
    projection.window_destroyed(window);
    assert!(!projection.opened(window, projector()));
    assert_eq!(projection.status().open, None);
    // The next window opens.
    let next = projection.next_window();
    assert!(projection.opened(next, projector()));
    // Two openings at once: the older answer keeps the newer screen.
    let a = projection.next_window();
    let b = projection.next_window();
    assert!(projection.opened(b, other_screen()));
    assert!(projection.opened(a, projector()));
    assert_eq!(projection.status().open, Some(other_screen()));
    projection.window_destroyed(a);
    assert_eq!(projection.status().open, Some(other_screen()));
    projection.window_destroyed(b);
    assert_eq!(projection.status().open, None);
}

#[test]
fn the_status_and_the_edits_have_the_shape_of_the_interface() {
    assert_eq!(
        serde_json::to_value(ProjectionStatus {
            open: Some(projector())
        })
        .unwrap()["open"]["key"],
        "HDMI@1920,0"
    );
    let projection = open();
    update(&projection, page(note(2))).unwrap();
    let window = subscribed(&projection);
    update(&projection, begin(6.0, vec![point(1.0, 1.0)])).unwrap();
    window.take();
    update(&projection, strokes(1.0, vec![stroke(4)], Some(6.0))).unwrap();
    let batch = serde_json::to_value(&window.batches()[0]).unwrap();
    let edited = &batch["events"][0];
    assert_eq!(edited["type"], "strokesEdited");
    assert_eq!(edited["pageIndex"], 0);
    assert_eq!(edited["removed"], json!([1]));
    assert_eq!(edited["inserted"][0]["index"], 1);
    assert_eq!(edited["inserted"][0]["stroke"]["points"][0]["t"], 400);
    assert_eq!(edited["recoloured"], json!([]));
    assert_eq!(edited["canUndo"], false);
    assert!(edited.get("viewers").is_none());
    assert_eq!(
        batch["events"][1],
        json!({ "type": "strokeCancel", "pageIndex": 0, "pen": "computer", "id": 6 })
    );
    assert!(batch["sentAt"].as_f64().unwrap() > 0.0);
}

#[test]
fn an_update_arrives_as_the_feed_sends_it() {
    // `projection.ts`: whole numbers come as plain JSON numbers.
    let projection = open();
    let wire = json!({ "kind": "page", "note": note(1), "imageKey": null,
                       "view": { "zoom": 1, "x": 14850, "y": 10800 } });
    update(&projection, serde_json::from_value(wire).unwrap()).unwrap();
    let wire = json!({ "kind": "strokeBegin", "id": 2, "tool": "pencil",
                       "color": { "r": 10, "g": 20, "b": 30, "a": 255 }, "width": 30, "layer": 0,
                       "points": [{ "x": 1, "y": 2, "p": 0.5, "t": 3, "wall": 1.7e12 }] });
    update(&projection, serde_json::from_value(wire).unwrap()).unwrap();
    let wire = json!({ "kind": "strokes", "keep": 1, "tail": [stroke(1)], "ended": 2 });
    update(&projection, serde_json::from_value(wire).unwrap()).unwrap();
    let (snapshot, events) = subscribe_events(&projection);
    assert_eq!(snapshot.page.strokes.len(), 2);
    assert_eq!(events.len(), 1, "the stroke joined the page");
}
