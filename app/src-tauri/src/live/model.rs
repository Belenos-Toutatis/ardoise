//! Shapes exchanged with the interface (mirrored by `app/ui/src/lib/live.ts`; contract:
//! docs/design/tablet-live-classroom.md, "TypeScript mirror").

use std::time::{SystemTime, UNIX_EPOCH};

use ardoise_core::note::{Note, PageSize};
use ardoise_core::settings::Orientation;
use ardoise_device::pages::TABLET_PEN_WIDTH;
use ardoise_ink::{Rgba, Tool};
use serde::{Deserialize, Serialize};

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
    /// How the tablet is held (the live window's quick switch shows it). On a resumed sheet held
    /// the other way round than the setting, how it is to be held for that page.
    pub orientation: Orientation,
    /// Whether the current page turns with the tablet. A resumed sheet never does: the interface
    /// disables the quick switch on it.
    pub can_turn_page: bool,
    /// Layer receiving new strokes (the current page's last visible one).
    pub layer: u32,
    pub style: LiveStyle,
    pub view: LiveViewState,
    pub hover: Option<HoverPoint>,
    /// The screen the presentation window is on, or `None` when it is closed.
    pub presentation: Option<MonitorInfo>,
    /// Paths of the notes saved so far in this session, in the order they were created.
    pub saved: Vec<String>,
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

/// One thing that happened, in order.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LiveEvent {
    /// Page change, clear, layer, style, state, blackboard, presentation opened or closed.
    Snapshot {
        snapshot: Box<LiveSnapshot>,
    },
    /// A stroke with its first point(s).
    StrokeStart {
        page_index: usize,
        stroke: LiveStroke,
    },
    /// Appended to the stroke in progress.
    Points {
        page_index: usize,
        points: Vec<LivePoint>,
    },
    StrokeEnd {
        page_index: usize,
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
    /// window that subscribes later. About the library: never sent to the viewers of the share.
    SheetForked {
        page_index: usize,
        reason: ForkReason,
        /// The note left as it is.
        original: String,
        /// The new note.
        path: String,
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
