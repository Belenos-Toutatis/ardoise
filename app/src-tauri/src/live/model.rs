//! Shapes exchanged with the interface (mirrored by `app/ui/src/lib/live.ts`; contract:
//! docs/design/tablet-live-classroom.md, "TypeScript mirror").

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use ardoise_core::note::{Note, PageSize, Paper};
use ardoise_core::settings::Orientation;
use ardoise_device::pages::TABLET_PEN_WIDTH;
use ardoise_ink::{Rgba, Stroke, Tool};
use serde::{Deserialize, Serialize, Serializer};

use crate::presentation::MonitorInfo;

/// Widest accepted nominal width, device units (5 cm): anything above is a typing error.
const MAX_STYLE_WIDTH: f32 = 5000.0;
/// Narrowest accepted nominal width, device units (10 µm).
const MIN_STYLE_WIDTH: f32 = 1.0;
/// Most zoomed-in view: an eighth of the page fills the screen.
pub const MAX_ZOOM: f64 = 8.0;

/// Ink of the strokes to come.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LiveStyle {
    pub tool: Tool,
    pub color: Rgba,
    /// Nominal diameter at full pressure, device units.
    pub width: f32,
}

impl LiveStyle {
    /// The tablet's own pen: black ballpoint, the width of synced pages.
    pub const PEN: LiveStyle = LiveStyle {
        tool: Tool::Ballpoint,
        color: Rgba::BLACK,
        width: TABLET_PEN_WIDTH,
    };

    /// White chalk, the default ink of the blackboard.
    pub fn chalk() -> LiveStyle {
        LiveStyle {
            tool: Tool::Chalk,
            color: Rgba::WHITE,
            width: Tool::Chalk.default_width(),
        }
    }

    /// A width the tools can draw: the tool's default for a missing or absurd one.
    pub fn sanitized(self) -> LiveStyle {
        let width = if self.width.is_finite() && self.width > 0.0 {
            self.width.clamp(MIN_STYLE_WIDTH, MAX_STYLE_WIDTH)
        } else {
            self.tool.default_width()
        };
        LiveStyle { width, ..self }
    }
}

impl Default for LiveStyle {
    fn default() -> Self {
        Self::PEN
    }
}

/// View of the projected area, shared by every window.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LiveViewState {
    /// 1 = the whole page, up to [`MAX_ZOOM`].
    pub zoom: f64,
    /// Centre of the projected area, page units.
    pub x: f64,
    pub y: f64,
    /// Blackboard: slate paper, chalk.
    pub board: bool,
    /// The hovering pen shows a dot and writes nothing.
    pub laser: bool,
}

impl LiveViewState {
    /// The whole page.
    pub fn whole(page: PageSize) -> LiveViewState {
        LiveViewState {
            zoom: 1.0,
            x: f64::from(page.width) / 2.0,
            y: f64::from(page.height) / 2.0,
            board: false,
            laser: false,
        }
    }

    /// Zoom within 1..=8 and a centre on the page (a missing value becomes the default).
    pub fn sanitized(self, page: PageSize) -> LiveViewState {
        let (width, height) = (f64::from(page.width), f64::from(page.height));
        let within = |value: f64, max: f64| {
            if value.is_finite() {
                value.clamp(0.0, max)
            } else {
                max / 2.0
            }
        };
        LiveViewState {
            zoom: if self.zoom.is_finite() {
                self.zoom.clamp(1.0, MAX_ZOOM)
            } else {
                1.0
            },
            x: within(self.x, width),
            y: within(self.y, height),
            ..self
        }
    }
}

/// Most marks kept over a page (the rest of a longer list is dropped).
pub const MAX_MARKS: usize = 200;
/// Most points kept in one mark.
pub const MAX_MARK_POINTS: usize = 4000;
/// Most points kept in all the marks of a page together (far more than anyone scribbles over a
/// slide; it bounds what every viewer is sent at each change).
pub const MAX_MARKS_POINTS: usize = 20_000;

/// A point of a mark written over the presentation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MarkPoint {
    pub x: f32,
    pub y: f32,
    /// Pressure, 0..=1.
    pub p: f32,
}

/// A temporary mark written over the presentation (docs/design/tablet-live-classroom.md, "M8:
/// writing over the presentation"): only what is drawn, in page units. Held in memory by the
/// session for the viewers of the share; never part of a page, never written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LiveMark {
    pub tool: Tool,
    pub color: Rgba,
    /// Line diameter at full pressure, page units.
    pub width: f32,
    pub points: Vec<MarkPoint>,
}

/// The marks the session keeps from what a window sent: at most [`MAX_MARKS`] marks and
/// [`MAX_MARKS_POINTS`] points (a mark beyond is dropped, the one crossing the limit is cut),
/// each mark at most [`MAX_MARK_POINTS`] points, no point without finite coordinates (a missing
/// pressure counts as full), coordinates within one page size around the page, a width a tool
/// can draw. A mark left without a point or without a usable width is dropped.
pub fn sanitize_marks(marks: Vec<LiveMark>, page: PageSize) -> Vec<LiveMark> {
    let (width, height) = (page.width as f32, page.height as f32);
    let mut budget = MAX_MARKS_POINTS;
    let mut kept = Vec::new();
    for mark in marks.into_iter().take(MAX_MARKS) {
        if budget == 0 {
            break;
        }
        if !mark.width.is_finite() || mark.width <= 0.0 {
            continue;
        }
        let points: Vec<MarkPoint> = mark
            .points
            .iter()
            .filter(|p| p.x.is_finite() && p.y.is_finite())
            .take(MAX_MARK_POINTS.min(budget))
            .map(|p| MarkPoint {
                x: p.x.clamp(-width, 2.0 * width),
                y: p.y.clamp(-height, 2.0 * height),
                p: if p.p.is_finite() {
                    p.p.clamp(0.0, 1.0)
                } else {
                    1.0
                },
            })
            .collect();
        if points.is_empty() {
            continue;
        }
        budget -= points.len();
        kept.push(LiveMark {
            width: mark.width.clamp(MIN_STYLE_WIDTH, MAX_STYLE_WIDTH),
            points,
            ..mark
        });
    }
    kept
}

/// What a [`LiveCommand`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveCommandKind {
    /// To the next page (a new one after the last).
    NextPage,
    PreviousPage,
    /// A new page at the end, which becomes the current one.
    NewPage,
    /// A new layer on the current page; new strokes go there.
    NewLayer,
    /// Removes the strokes of the current page.
    ClearPage,
    /// What the tablet's button does (`[tablet] button`: new page or new layer).
    Button,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveCommand {
    pub kind: LiveCommandKind,
}

/// Why a resumed sheet became a new note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ForkReason {
    /// It changed elsewhere (another PC, Nextcloud) since it was resumed.
    Changed,
    /// It no longer exists.
    Deleted,
}

/// State of the live session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LiveSessionState {
    /// Not started, or stopped.
    Idle,
    /// Started, waiting for the tablet to stream.
    Waiting,
    Live,
    /// The link was lost; the tablet service reopens it.
    Reconnecting,
}

/// Where the hovering pen is, page units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct HoverPoint {
    pub x: f32,
    pub y: f32,
}

/// Which pen wrote a stroke in progress (docs/design/tablet-live-classroom.md, "M14"): the tablet,
/// or the computer's pen or mouse in the control window. Written in the events only for the
/// computer: the tablet's events stay as they were, byte for byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LivePen {
    #[default]
    Tablet,
    Computer,
}

impl LivePen {
    /// For `skip_serializing_if`: the tablet is the pen an event names by saying nothing.
    pub fn is_tablet(&self) -> bool {
        *self == LivePen::Tablet
    }
}

/// Everything a window needs to draw the session from scratch.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveSnapshot {
    pub state: LiveSessionState,
    /// The current page: its finished strokes, background and layers. The stroke in progress,
    /// if any, is not in it: it follows the snapshot as a `strokeStart` event.
    pub page: Note,
    /// 0-based.
    pub page_index: usize,
    pub page_count: usize,
    /// How the tablet is held (the live window's quick switch shows it). On a page that does not
    /// turn held the other way round than the setting, how it is to be held for that page.
    pub orientation: Orientation,
    /// Whether the current page turns with the tablet. A resumed sheet never does, nor a page
    /// whose background image was written on (M17): the interface disables the quick switch there.
    pub can_turn_page: bool,
    /// The tablet's layer of the current page: where its ink goes (M14: it may be hidden).
    pub layer: u32,
    /// The computer's layer of the current page: where the computer's pen, its eraser end and the
    /// selection act. Not given to the share's viewers.
    pub computer_layer: u32,
    pub style: LiveStyle,
    pub view: LiveViewState,
    pub hover: Option<HoverPoint>,
    /// The screen the presentation window is on, or `None` when it is closed.
    pub presentation: Option<MonitorInfo>,
    /// Paths of the notes saved so far in this session, in the order they were created.
    pub saved: Vec<String>,
    /// What the indices of the current page's strokes mean: a command that names strokes
    /// (`live_edit_strokes`) gives it back and is refused once it changed. It changes only when
    /// strokes leave or come back ([`LiveEvent::StrokesEdited`], « Clear the page ») and when the
    /// page turns its ink (a quarter or half turn of the tablet: a move worked out before would
    /// apply to strokes that are elsewhere), never when the tablet appends a stroke. Absent on the
    /// share's viewers.
    pub revision: u64,
    /// The current page has an edit to undo, or to redo. Absent on the share's viewers.
    pub can_undo: bool,
    pub can_redo: bool,
    /// The current page's own paper (what it has without the blackboard): the live window's Page tab
    /// shows it (M17). Not in the share's allow list (`SharedSnapshot`).
    pub page_paper: Paper,
    /// The key and size of the pixels of the page's image; `None` without one. The windows key their
    /// paper on it, the Fond tab places the image with it (M17). Not in the share's allow list.
    pub image_pixels: Option<ImagePixels>,
}

/// The key and size of the pixels of a page's background image (`LiveSnapshot::image_pixels`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImagePixels {
    /// `background::key_of` the member's bytes: tells two files of the same name apart.
    pub key: String,
    pub width: u32,
    pub height: u32,
}

/// A point of the stroke in progress.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct LivePoint {
    pub x: f32,
    pub y: f32,
    /// Pressure, 0..=1.
    pub p: f32,
    /// Milliseconds since the note's `created`.
    pub t: u32,
    /// Wall-clock time (ms since the Unix epoch, fractional) when the notification holding the
    /// point came out of the transport.
    pub recv: f64,
}

/// A stroke as `strokeStart` sends it: a `Stroke` whose points carry their receipt time.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct LiveStroke {
    pub tool: Tool,
    pub color: Rgba,
    pub width: f32,
    pub layer: u32,
    pub points: Vec<LivePoint>,
}

/// A stroke that comes back on a page, and where it stands once it is back.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PlacedStroke {
    pub index: u32,
    pub stroke: Stroke,
}

/// Strokes that take one colour.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RecolouredStrokes {
    pub color: Rgba,
    /// Ascending.
    pub indices: Vec<u32>,
}

/// A `strokesEdited` in the numbering of the share's viewers, for a page with hidden layers: they
/// have no hidden strokes, so their list is shorter than the page's. Worked out with the edit
/// (`feed.rs` knows nothing of the page). Per stroke (M14: an undo may give back, a redo take, a
/// stroke of a layer hidden since): `removed` and `recoloured` hold the visible strokes only;
/// `inserted` follows the order of the event's `inserted`, `None` for a hidden stroke, which the
/// viewers never get.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewerIndices {
    pub removed: Vec<u32>,
    pub inserted: Vec<Option<u32>>,
    pub recoloured: Vec<RecolouredStrokes>,
}

/// The strokes of an event are not copied for each window (see [`LiveEvent::StrokesEdited`]).
fn shared_strokes<S: Serializer>(
    strokes: &Arc<Vec<PlacedStroke>>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    strokes.as_slice().serialize(serializer)
}

/// Nor is a snapshot: every window's queue holds the same one (see [`LiveEvent::Snapshot`]).
fn shared_snapshot<S: Serializer>(
    snapshot: &Arc<LiveSnapshot>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    snapshot.as_ref().serialize(serializer)
}

/// The answer to a command that returns the snapshot a window's queue already holds: written once
/// more as it is, not copied first.
#[derive(Debug, Clone, PartialEq)]
pub struct SnapshotReply(pub Arc<LiveSnapshot>);

impl Serialize for SnapshotReply {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        shared_snapshot(&self.0, serializer)
    }
}

/// A resumed sheet that went on in a new note: what [`LiveEvent::SheetForked`] says, and what
/// `live_stop` gives back for a fork made by the last write, when the window is about to close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForkedSheet {
    pub page_index: usize,
    pub reason: ForkReason,
    /// The note left as it is.
    pub original: String,
    /// The new note.
    pub path: String,
}

/// One thing that happened, in order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LiveEvent {
    /// Page change, clear, layer, style, state, blackboard, presentation opened or closed. Not an
    /// edit of strokes: [`StrokesEdited`](Self::StrokesEdited) says that for less. Shared, since
    /// each window and viewer has its own queue: the page is not copied for each under the hub's
    /// lock.
    Snapshot {
        #[serde(serialize_with = "shared_snapshot")]
        snapshot: Arc<LiveSnapshot>,
    },
    /// A stroke with its first point(s). Two strokes may be in progress at once, one per pen.
    StrokeStart {
        page_index: usize,
        stroke: LiveStroke,
        /// Written only for the computer.
        #[serde(skip_serializing_if = "LivePen::is_tablet")]
        pen: LivePen,
        /// The control window's number for the computer's gesture; `None` for the tablet.
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
        /// The stroke's layer was hidden when the event was made: the share leaves it out.
        #[serde(skip)]
        hidden: bool,
    },
    /// Appended to the stroke in progress of that pen.
    Points {
        page_index: usize,
        points: Vec<LivePoint>,
        #[serde(skip_serializing_if = "LivePen::is_tablet")]
        pen: LivePen,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
        #[serde(skip)]
        hidden: bool,
    },
    /// The stroke in progress of that pen joined its page, at the tail of its list.
    StrokeEnd {
        page_index: usize,
        #[serde(skip_serializing_if = "LivePen::is_tablet")]
        pen: LivePen,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
        #[serde(skip)]
        hidden: bool,
    },
    /// The stroke in progress of that pen was dropped: nothing of it joins the page (the window
    /// cancelled it, or « Clear the page »).
    StrokeCancel {
        page_index: usize,
        #[serde(skip_serializing_if = "LivePen::is_tablet")]
        pen: LivePen,
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
        #[serde(skip)]
        hidden: bool,
    },
    /// What page `page_index` can undo and redo, after a step that came with no other event
    /// telling it (a computer stroke recorded). Not for the share's viewers.
    History {
        page_index: usize,
        can_undo: bool,
        can_redo: bool,
    },
    /// The pen hovers here (at most one per batch: the latest position).
    Hover {
        x: f32,
        y: f32,
    },
    /// The pen left, or touched the paper.
    HoverEnd,
    View {
        view: LiveViewState,
    },
    /// The marks written over the presentation, the whole list for the current page (empty:
    /// none, or the page changed). Only the share's viewers draw them: the presentation window
    /// keeps its own marks.
    Marks {
        marks: Vec<LiveMark>,
    },
    /// Paths of the notes saved so far in this session (the whole list, as in the snapshot).
    /// Pages still failing to be written are announced again right after it (`saveFailed`).
    Saved {
        paths: Vec<String>,
    },
    /// Page `page_index` (0-based) could not be written to the library: sent once until a write
    /// of it succeeds, again after every `saved` while it keeps failing, and to a window that
    /// subscribes meanwhile. Its changes stay in memory and are tried again.
    SaveFailed {
        page_index: usize,
        /// The library error's code: the interface says it in words (`errorCodes.*`).
        code: String,
        message: String,
    },
    /// The resumed sheet on page `page_index` changed elsewhere or was deleted: nothing was
    /// overwritten, and the page goes on in a new note in the same binder. Sent once, and to a
    /// window that subscribes later while the session runs (a stopped session's forks are for
    /// `live_stop` to give back, not for the next lesson's view). About the library: never sent to
    /// the viewers of the share.
    SheetForked {
        page_index: usize,
        reason: ForkReason,
        /// The note left as it is.
        original: String,
        /// The new note.
        path: String,
    },
    /// The tablet that connected cannot write on the sheet this session resumes (`notSupported`:
    /// a Slate sheet with the blackboard, a blackboard page with the Slate): the session ended at
    /// once, `idle`, nothing written on the sheet; the tablet leaves live mode. Sent once, to the
    /// windows subscribed then; never to the viewers of the share.
    Refused {
        /// `errorCodes.*`.
        code: String,
        message: String,
    },
    /// Finished strokes of page `page_index` were deleted, came back or changed colour (an edit,
    /// its undo or redo). Applied to the list in this order: `removed`, then `inserted`, then
    /// `recoloured`; the stroke in progress is not concerned. Strokes that are not named keep
    /// their place in the list and their object in the window, so that their outlines are not
    /// computed again. Never merged with another event. « Clear the page » is a snapshot instead:
    /// it also drops the stroke in progress.
    StrokesEdited {
        page_index: usize,
        /// The page's revision after this event.
        revision: u64,
        /// Indices of the strokes that leave, in the list before this event, ascending.
        removed: Vec<u32>,
        /// The strokes that come back, each with its index in the list after this event,
        /// ascending. Shared, since each window has its own queue: their points are not copied
        /// under the hub's lock.
        #[serde(serialize_with = "shared_strokes")]
        inserted: Arc<Vec<PlacedStroke>>,
        /// New colours; indices in the list after this event.
        recoloured: Vec<RecolouredStrokes>,
        can_undo: bool,
        can_redo: bool,
        /// For the share only (`feed.rs`): `None` when the page has no hidden layer, and then a
        /// viewer's list and the page's are numbered alike.
        #[serde(skip)]
        viewers: Option<Box<ViewerIndices>>,
    },
}

/// What one channel message carries.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveBatch {
    pub events: Vec<LiveEvent>,
    /// Wall-clock time (ms since the Unix epoch, fractional) when the batch was handed to the
    /// channel.
    pub sent_at: f64,
}

/// `time` as fractional milliseconds since the Unix epoch (0 before it). Latency stamps use the
/// system clock on purpose (docs/design/tablet-live-classroom.md, "Rules").
pub fn wall_ms(time: SystemTime) -> f64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0.0, |elapsed| elapsed.as_secs_f64() * 1000.0)
}

/// Now, as [`wall_ms`].
pub fn wall_now_ms() -> f64 {
    wall_ms(SystemTime::now())
}
