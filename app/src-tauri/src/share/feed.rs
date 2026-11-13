//! Live mode for the share (docs/design/lan-share.md, "What a viewer receives"): each viewer is a
//! subscriber of the [`LiveHub`] like a window, so it gets the batches the presentation window
//! gets (its first one starting with the snapshot and the stroke in progress), but written
//! again from an allow list of what the viewer draws: nothing else of the hub's types can reach
//! a browser, whatever fields they get later.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use ardoise_core::note::{Background, Origin, PageObject, PageSize, Timing};
use ardoise_core::settings::Orientation;
use ardoise_ink::{Rgba, Stroke, Tool};
use ardoise_share::{LiveFeed, LiveViewer};
use serde::Serialize;

use super::live_paper::{LivePapers, paper_key};
use super::page::{is_drawn, shared_objects};
use crate::live::LiveHub;
use crate::live::hub::BatchSink;
use crate::live::model::{
    HoverPoint, LiveBatch, LiveEvent, LiveMark, LivePen, LivePoint, LiveSessionState, LiveSnapshot,
    LiveStroke, LiveStyle, LiveViewState, PlacedStroke, RecolouredStrokes, ViewerIndices,
    wall_now_ms,
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
    /// The paper of the live page, which goes outside the batches.
    papers: Arc<LivePapers>,
}

impl HubFeed {
    pub fn new(hub: Arc<LiveHub>) -> HubFeed {
        HubFeed {
            papers: LivePapers::new(hub.clone()),
            hub,
            memo: Arc::new(Memo::default()),
        }
    }

    /// The paper of the live page: the share receives it once it runs.
    pub fn papers(&self) -> Arc<LivePapers> {
        self.papers.clone()
    }
}

impl LiveFeed for HubFeed {
    fn start(&self, viewer: LiveViewer) {
        let label = subscriber_label(viewer.id());
        let sink = ShareSink {
            viewer,
            memo: self.memo.clone(),
            papers: self.papers.clone(),
        };
        let snapshot = self.hub.subscribe(&label, Arc::new(sink));
        self.papers.subscribed(&snapshot);
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
    papers: Arc<LivePapers>,
}

impl BatchSink for ShareSink {
    fn send(&self, batch: LiveBatch) -> Result<(), String> {
        // A comparison per snapshot, here on the flusher; the paper is made elsewhere.
        for event in &batch.events {
            if let LiveEvent::Snapshot { snapshot } = event {
                self.papers.saw(snapshot);
            }
        }
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
            && same_events(events, &batch.events)
        {
            return Some(json.clone());
        }
        let json: Arc<str> = batch_json(&batch)?.into();
        *last = Some((batch.events, json.clone()));
        Some(json)
    }
}

/// Whether two batches hold the same events. Viewers up to date were all queued the same snapshot,
/// which is not worth comparing point by point with itself.
fn same_events(a: &[LiveEvent], b: &[LiveEvent]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|pair| match pair {
            (LiveEvent::Snapshot { snapshot: a }, LiveEvent::Snapshot { snapshot: b }) => {
                Arc::ptr_eq(a, b) || a == b
            }
            (a, b) => a == b,
        })
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
/// (the system's message), which are about the library; `history` (the control window's undo);
/// every event of a stroke in progress on a hidden layer (M14: the tablet's or the computer's), so
/// that a viewer's list never holds a hidden stroke. The pen of a stroke in progress is told (two
/// may be in progress at once), not the control window's number for the gesture.
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
        #[serde(skip_serializing_if = "LivePen::is_tablet")]
        pen: LivePen,
    },
    Points {
        page_index: usize,
        points: &'a [LivePoint],
        #[serde(skip_serializing_if = "LivePen::is_tablet")]
        pen: LivePen,
    },
    StrokeEnd {
        page_index: usize,
        #[serde(skip_serializing_if = "LivePen::is_tablet")]
        pen: LivePen,
    },
    StrokeCancel {
        page_index: usize,
        #[serde(skip_serializing_if = "LivePen::is_tablet")]
        pen: LivePen,
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
    /// Strokes deleted, back or recoloured, numbered as in the viewer's list, which has no hidden
    /// strokes (`ViewerIndices`): a hidden one is never named, nor sent.
    StrokesEdited {
        page_index: usize,
        removed: &'a [u32],
        inserted: Vec<SharedPlaced<'a>>,
        recoloured: &'a [RecolouredStrokes],
    },
}

/// A stroke that comes back, at its place in the viewer's list.
#[derive(Serialize)]
struct SharedPlaced<'a> {
    index: u32,
    stroke: &'a Stroke,
}

impl<'a> SharedEvent<'a> {
    fn of(event: &'a LiveEvent) -> Option<SharedEvent<'a>> {
        Some(match event {
            LiveEvent::Snapshot { snapshot } => SharedEvent::Snapshot {
                snapshot: SharedSnapshot::of(snapshot),
            },
            LiveEvent::StrokeStart {
                page_index,
                stroke,
                pen,
                hidden: false,
                ..
            } => SharedEvent::StrokeStart {
                page_index: *page_index,
                stroke,
                pen: *pen,
            },
            LiveEvent::Points {
                page_index,
                points,
                pen,
                hidden: false,
                ..
            } => SharedEvent::Points {
                page_index: *page_index,
                points,
                pen: *pen,
            },
            LiveEvent::StrokeEnd {
                page_index,
                pen,
                hidden: false,
                ..
            } => SharedEvent::StrokeEnd {
                page_index: *page_index,
                pen: *pen,
            },
            LiveEvent::StrokeCancel {
                page_index,
                pen,
                hidden: false,
                ..
            } => SharedEvent::StrokeCancel {
                page_index: *page_index,
                pen: *pen,
            },
            LiveEvent::Hover { x, y } => SharedEvent::Hover { x: *x, y: *y },
            LiveEvent::HoverEnd => SharedEvent::HoverEnd,
            LiveEvent::View { view } => SharedEvent::View { view },
            LiveEvent::Marks { marks } => SharedEvent::Marks {
                marks: marks.iter().map(SharedMark::of).collect(),
            },
            LiveEvent::StrokesEdited {
                page_index,
                removed,
                inserted,
                recoloured,
                viewers,
                ..
            } => {
                return shared_edit(
                    *page_index,
                    removed,
                    inserted,
                    recoloured,
                    viewers.as_deref(),
                );
            }
            // What is left of a stroke in progress is on a hidden layer, whichever pen.
            LiveEvent::StrokeStart { .. }
            | LiveEvent::Points { .. }
            | LiveEvent::StrokeEnd { .. }
            | LiveEvent::StrokeCancel { .. }
            | LiveEvent::History { .. }
            | LiveEvent::Saved { .. }
            | LiveEvent::SaveFailed { .. }
            | LiveEvent::SheetForked { .. }
            | LiveEvent::Refused { .. } => return None,
        })
    }
}

/// A `strokesEdited` in the viewers' numbering (`viewers`: `None` on a page without a hidden
/// layer, numbered alike), without the strokes they never had; `None` when nothing of it concerns
/// them (only a stroke of a layer hidden since went or came back): not even that something
/// happened there goes out.
fn shared_edit<'a>(
    page_index: usize,
    removed: &'a [u32],
    inserted: &'a [PlacedStroke],
    recoloured: &'a [RecolouredStrokes],
    viewers: Option<&'a ViewerIndices>,
) -> Option<SharedEvent<'a>> {
    let inserted: Vec<SharedPlaced<'a>> = inserted
        .iter()
        .enumerate()
        .filter_map(|(i, placed)| {
            let index = match viewers {
                // A hidden stroke, or one the list does not name: never sent.
                Some(viewers) => viewers.inserted.get(i).copied().flatten()?,
                None => placed.index,
            };
            Some(SharedPlaced {
                index,
                stroke: &placed.stroke,
            })
        })
        .collect();
    let (removed, recoloured) = match viewers {
        Some(viewers) => (viewers.removed.as_slice(), viewers.recoloured.as_slice()),
        None => (removed, recoloured),
    };
    let recolours = recoloured.iter().any(|group| !group.indices.is_empty());
    if removed.is_empty() && inserted.is_empty() && !recolours {
        return None;
    }
    Some(SharedEvent::StrokesEdited {
        page_index,
        removed,
        inserted,
        recoloured,
    })
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
    /// The paper's picture the page wants ([`paper_key`]), which comes in a `livePaper` message:
    /// absent on plain paper.
    #[serde(skip_serializing_if = "Option::is_none")]
    paper_key: Option<String>,
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
            paper_key: paper_key(snapshot),
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

/// What the viewers call the page with id `id`: stable for this run of the app, one per page, which
/// is all the viewer's page-change detection needs (`LiveState.noteId`). Not the id itself, which
/// on a resumed sheet is the identity of a note of the library, nor a digest anyone could check
/// against an id they know: the salt is drawn at each run.
fn viewer_page_id(id: &str) -> String {
    static SALT: OnceLock<u128> = OnceLock::new();
    let salt = SALT.get_or_init(|| uuid::Uuid::new_v4().as_u128());
    let mut hasher = DefaultHasher::new();
    salt.hash(&mut hasher);
    id.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// The page of a snapshot as the viewer reads a `Note` (`note.ts`): what is drawn, and the
/// fields the type requires with neutral values. The id, title, dates, tags, favourite mark, layer
/// names, background image and origin of the page stay on the PC, and so do the strokes and the
/// page objects of a layer that is hidden (a resumed sheet may have some: it is an old note, with
/// its own date).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SharedLivePage<'a> {
    /// [`viewer_page_id`].
    id: String,
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
    /// The text frames of a resumed sheet the viewer draws ([`shared_objects`]), as `note.json`
    /// writes them; absent when there is none, as in `note.json`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    objects: Vec<PageObject>,
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
            id: viewer_page_id(&note.id),
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
            objects: shared_objects(&note.layers, &note.objects),
            timing: note.timing,
            origin: Origin::Ardoise,
        }
    }
}
