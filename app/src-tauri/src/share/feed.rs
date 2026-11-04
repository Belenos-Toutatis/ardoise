//! Live mode for the share (docs/design/lan-share.md, "What a viewer receives"): each viewer is a
//! subscriber of the [`LiveHub`] like a window, so it gets the batches the presentation window
//! gets (its first one starting with the snapshot and the stroke in progress), but written
//! again from an allow list of what the viewer draws: nothing else of the hub's types can reach
//! a browser, whatever fields they get later.

use std::sync::{Arc, Mutex, PoisonError};

use ardoise_core::note::{Background, Origin, PageSize, Timing};
use ardoise_core::settings::Orientation;
use ardoise_ink::{Rgba, Stroke, Tool};
use ardoise_share::{LiveFeed, LiveViewer};
use serde::Serialize;

use super::page::is_drawn;
use crate::live::LiveHub;
use crate::live::hub::BatchSink;
use crate::live::model::{
    HoverPoint, LiveBatch, LiveEvent, LiveMark, LivePoint, LiveSessionState, LiveSnapshot,
    LiveStroke, LiveStyle, LiveViewState, wall_now_ms,
};

/// The hub's subscription label of viewer `id` (never a window's label).
pub fn subscriber_label(id: u64) -> String {
    format!("share:{id}")
}

/// [`LiveFeed`] over the app's hub.
pub struct HubFeed {
    hub: Arc<LiveHub>,
    /// Shared by the viewers' sinks, so that a batch they all get is written once.
    memo: Arc<Memo>,
}

impl HubFeed {
    pub fn new(hub: Arc<LiveHub>) -> HubFeed {
        HubFeed {
            hub,
            memo: Arc::new(Memo::default()),
        }
    }
}

impl LiveFeed for HubFeed {
    fn start(&self, viewer: LiveViewer) {
        let label = subscriber_label(viewer.id());
        let sink = ShareSink {
            viewer,
            memo: self.memo.clone(),
        };
        self.hub.subscribe(&label, Arc::new(sink));
    }

    fn stop(&self, viewer: u64) {
        self.hub.unsubscribe(&subscriber_label(viewer));
    }

    fn freeze(&self) -> Option<String> {
        let batch = LiveBatch {
            events: self.hub.current_events(),
            sent_at: wall_now_ms(),
        };
        batch_json(&batch)
    }
}

/// Where the hub's flusher sends one viewer's batches.
struct ShareSink {
    viewer: LiveViewer,
    memo: Arc<Memo>,
}

impl BatchSink for ShareSink {
    fn send(&self, batch: LiveBatch) -> Result<(), String> {
        match self.memo.json(batch) {
            Some(json) => self.viewer.send(&json).map_err(|error| error.to_string()),
            // Only what is never shared (a page written to the library).
            None => Ok(()),
        }
    }
}

/// The last batch written and its text. The hub's flusher hands each viewer its own copy of the
/// events queued since the last flush: viewers that were up to date get equal batches one after
/// the other, and the text (the whole page, after a page change) is made once for all of them.
/// Comparing the events is much cheaper than writing them. The last batch stays in memory until
/// the next one comes.
#[derive(Default)]
pub struct Memo {
    last: Mutex<Option<(Vec<LiveEvent>, Arc<str>)>>,
}

impl Memo {
    /// The JSON the viewers get for `batch`; `None` when nothing in it is shared. Its `sentAt`
    /// is the one of the first viewer's copy (microseconds apart).
    pub fn json(&self, batch: LiveBatch) -> Option<Arc<str>> {
        let mut last = self.last.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((events, json)) = last.as_ref()
            && *events == batch.events
        {
            return Some(json.clone());
        }
        let json: Arc<str> = batch_json(&batch)?.into();
        *last = Some((batch.events, json.clone()));
        Some(json)
    }
}

/// `batch` as the viewers get it (`LiveBatch` of `live.ts`), `None` when it holds nothing that
/// may leave the PC.
pub fn batch_json(batch: &LiveBatch) -> Option<String> {
    let events: Vec<SharedEvent> = batch.events.iter().filter_map(SharedEvent::of).collect();
    if events.is_empty() {
        return None;
    }
    let shared = SharedBatch {
        events,
        sent_at: batch.sent_at,
    };
    serde_json::to_string(&shared).ok()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SharedBatch<'a> {
    events: Vec<SharedEvent<'a>>,
    sent_at: f64,
}

/// The events a viewer gets. Left out: `saved` and `sheetForked` (paths of notes) and `saveFailed`
/// (the system's message), which are about the library.
#[derive(Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum SharedEvent<'a> {
    Snapshot {
        snapshot: SharedSnapshot<'a>,
    },
    StrokeStart {
        page_index: usize,
        stroke: &'a LiveStroke,
    },
    Points {
        page_index: usize,
        points: &'a [LivePoint],
    },
    StrokeEnd {
        page_index: usize,
    },
    Hover {
        x: f32,
        y: f32,
    },
    HoverEnd,
    View {
        view: &'a LiveViewState,
    },
    /// The marks written over the presentation, drawn on top of the page.
    Marks {
        marks: Vec<SharedMark>,
    },
}

impl<'a> SharedEvent<'a> {
    fn of(event: &'a LiveEvent) -> Option<SharedEvent<'a>> {
        Some(match event {
            LiveEvent::Snapshot { snapshot } => SharedEvent::Snapshot {
                snapshot: SharedSnapshot::of(snapshot),
            },
            LiveEvent::StrokeStart { page_index, stroke } => SharedEvent::StrokeStart {
                page_index: *page_index,
                stroke,
            },
            LiveEvent::Points { page_index, points } => SharedEvent::Points {
                page_index: *page_index,
                points,
            },
            LiveEvent::StrokeEnd { page_index } => SharedEvent::StrokeEnd {
                page_index: *page_index,
            },
            LiveEvent::Hover { x, y } => SharedEvent::Hover { x: *x, y: *y },
            LiveEvent::HoverEnd => SharedEvent::HoverEnd,
            LiveEvent::View { view } => SharedEvent::View { view },
            LiveEvent::Marks { marks } => SharedEvent::Marks {
                marks: marks.iter().map(SharedMark::of).collect(),
            },
            LiveEvent::Saved { .. }
            | LiveEvent::SaveFailed { .. }
            | LiveEvent::SheetForked { .. } => return None,
        })
    }
}

/// A mark as the viewer draws it: the ink and the points, nothing else.
#[derive(Serialize)]
struct SharedMark {
    tool: Tool,
    color: Rgba,
    width: f32,
    points: Vec<SharedMarkPoint>,
}

#[derive(Serialize)]
struct SharedMarkPoint {
    x: f32,
    y: f32,
    p: f32,
}

impl SharedMark {
    fn of(mark: &LiveMark) -> SharedMark {
        SharedMark {
            tool: mark.tool,
            color: mark.color,
            width: mark.width,
            points: mark
                .points
                .iter()
                .map(|p| SharedMarkPoint {
                    x: p.x,
                    y: p.y,
                    p: p.p,
                })
                .collect(),
        }
    }
}

/// A [`LiveSnapshot`] as the viewer reads it: the screen the presentation window is on and the
/// paths of the saved notes are always empty.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SharedSnapshot<'a> {
    state: LiveSessionState,
    page: SharedLivePage<'a>,
    page_index: usize,
    page_count: usize,
    orientation: Orientation,
    layer: u32,
    style: &'a LiveStyle,
    view: &'a LiveViewState,
    hover: Option<&'a HoverPoint>,
    /// Always `null`.
    presentation: (),
    /// Always `[]`.
    saved: [(); 0],
}

impl<'a> SharedSnapshot<'a> {
    fn of(snapshot: &'a LiveSnapshot) -> SharedSnapshot<'a> {
        SharedSnapshot {
            state: snapshot.state,
            page: SharedLivePage::of(snapshot),
            page_index: snapshot.page_index,
            page_count: snapshot.page_count,
            orientation: snapshot.orientation,
            layer: snapshot.layer,
            style: &snapshot.style,
            view: &snapshot.view,
            hover: snapshot.hover.as_ref(),
            presentation: (),
            saved: [],
        }
    }
}

/// The page of a snapshot as the viewer reads a `Note` (`note.ts`): what is drawn, and the
/// fields the type requires with neutral values. The title, dates, tags, favourite mark, layer
/// names, background image and origin of the page stay on the PC, and so do the strokes of a layer
/// that is hidden (a resumed sheet may have some: it is an old note, with its own date).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SharedLivePage<'a> {
    id: &'a str,
    /// Always empty.
    title: &'static str,
    /// Always 0, like `modified`: the point times need no date.
    created: i64,
    modified: i64,
    page: PageSize,
    background: &'a Background,
    /// Always empty.
    tags: [(); 0],
    layers: Vec<SharedLayer>,
    strokes: Vec<&'a Stroke>,
    timing: Timing,
    origin: Origin,
}

/// A layer without its name (typed by the user: the viewer only needs whether it shows).
#[derive(Serialize)]
struct SharedLayer {
    name: &'static str,
    visible: bool,
}

impl<'a> SharedLivePage<'a> {
    fn of(snapshot: &'a LiveSnapshot) -> SharedLivePage<'a> {
        let note = &snapshot.page;
        SharedLivePage {
            id: &note.id,
            title: "",
            created: 0,
            modified: 0,
            page: note.page,
            background: &note.background,
            tags: [],
            layers: note
                .layers
                .iter()
                .map(|layer| SharedLayer {
                    name: "",
                    visible: layer.visible,
                })
                .collect(),
            strokes: note
                .strokes
                .iter()
                .filter(|stroke| is_drawn(&note.layers, stroke))
                .collect(),
            timing: note.timing,
            origin: Origin::Ardoise,
        }
    }
}
