//! The editor's projection (SPEC §4.4 « Projeter la note de l'éditeur », `docs/questions.md` Q13;
//! docs/design/paper-projection-narration.md §2): the editor's page full screen on a screen, in a
//! window of its own (label `projection`, `index.html?view=projection`, see `presentation.rs`) that
//! speaks live's own vocabulary (`LiveBatch`), fed by the editor with small updates
//! (`lib/projection-feed.ts`).
//!
//! - **The scene** ([`Projection`]): the page projected (the editor's note as last sent, the size
//!   of its staged image, the projected area), the editor's stroke in progress, the window's
//!   subscription and the editor's watch. No thread: the editor paces its calls (one in flight),
//!   each update is applied and its batch sent from the command, `sent_at` stamped as the hub
//!   stamps its own.
//! - **Events**, live's: a `page` is the snapshot of a one-page session (then the stroke in
//!   progress); `strokes` a `strokesEdited` (then, in the same batch, the `strokeCancel` of the
//!   stroke in progress it brings to the page); the stroke in progress a computer stroke
//!   (`pen: computer`, the editor's number as its `id`, each point's `recv` the wall time of its
//!   pointer sample). So the window's drawing, its partial repaints and its latency figures
//!   (`__ardoiseLatency.computer`) are live's.
//! - **Refusals**, which the feed answers with the whole page (never shown): `projectionStale` when
//!   the projection does not hold what the update builds on (the window is closed, no page yet, a
//!   `keep` beyond its strokes); `projectionInvalid` for numbers that make no sense (an index or an
//!   identifier that is not a whole number, a point that is not finite, more than the limits).
//! - **The window**: one projector window at a time. Opening the projection destroys live's
//!   presentation window, and live's opening destroys the projection (`presentation.rs`, R1).
//!   However the window goes (Esc in it, `projection_close`, live's presentation, the main window
//!   going), the scene goes with it and the editor's watch is told. Each window opened, or moved to
//!   another screen, gets a number: the late destruction of an older window never closes a newer
//!   one.
//!
//! Batches are sent outside the scene's lock: a page may be megabytes to serialize, and the
//! window's destruction takes that lock on the main thread. A second lock, held from the making
//! of a batch to its sending, keeps them in the order they were made (a subscription and an update
//! at the same time).

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ardoise_core::note::{Note, PageSize};
use ardoise_core::settings::Orientation;
use ardoise_ink::{Rgba, Stroke, Tool};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::AppContext;
use crate::background::{self, Staging};
use crate::commands::{CommandError, CommandResult};
use crate::library::blank_note;
use crate::live::hub::BatchSink;
use crate::live::model::{
    ImagePixels, LiveEvent, LivePen, LivePoint, LiveSessionState, LiveStroke, PlacedStroke,
    wall_now_ms,
};
use crate::live::pen::MAX_COMPUTER_POINTS;
use crate::live::{LiveBatch, LiveRuntime, LiveSnapshot, LiveStyle, LiveViewState, SnapshotReply};
use crate::presentation::{self, MonitorInfo, PRESENTATION, ScreenWindow};

/// Most points one update may carry (`StrokeBegin`, `StrokePoints`).
pub const MAX_PROJECTION_POINTS: usize = 4096;
/// Most strokes one `Strokes` tail may carry, and the projected page altogether (a `Page`, or what
/// a `Strokes` leaves): far beyond any note, and every index fits the events' `u32`.
pub const MAX_PROJECTION_STROKES: usize = 1 << 20;
/// Most points the stroke in progress holds, as live's computer stroke: the points beyond are not
/// projected while it is written (the whole stroke is, once it joins the page).
pub const MAX_PROJECTED_STROKE_POINTS: usize = MAX_COMPUTER_POINTS;

/// The update builds on something the projection does not hold: the feed sends the page again.
pub const STALE: &str = "projectionStale";
/// The update holds numbers that make no sense: the feed logs it and sends the page again.
pub const INVALID: &str = "projectionInvalid";

/// What the editor tells the projection (`ProjectionUpdate` in `projection.ts`); numbers that are
/// indices or identifiers arrive as plain numbers and are checked by the projection
/// (`projectionInvalid`).
// `Page` carries a whole note and is rare (one call each time the layout changes): not boxed.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum ProjectionUpdate {
    /// The whole page (a note opened, its page, paper, image, layers or image key changed).
    Page {
        note: Note,
        image_key: Option<String>,
        view: ProjectionView,
    },
    /// The strokes from `keep` on are replaced by `tail`; `ended`: the stroke in progress `id` is now
    /// among them.
    Strokes {
        keep: f64,
        tail: Vec<Stroke>,
        ended: Option<f64>,
    },
    View {
        view: ProjectionView,
    },
    StrokeBegin {
        id: f64,
        tool: Tool,
        color: Rgba,
        width: f32,
        layer: f64,
        points: Vec<ProjectionPoint>,
    },
    StrokePoints {
        id: f64,
        points: Vec<ProjectionPoint>,
    },
    StrokeCancel {
        id: f64,
    },
}

/// The projected area: the zoom (1 = the whole page) and its centre, page units.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ProjectionView {
    pub zoom: f64,
    pub x: f64,
    pub y: f64,
}

/// A point of the editor's stroke in progress; `wall`: Unix ms of the pointer sample (the window's
/// latency figures count from it).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct ProjectionPoint {
    pub x: f64,
    pub y: f64,
    pub p: f64,
    pub t: f64,
    pub wall: f64,
}

/// Where the projection is open: what the editor's Projection tab shows.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectionStatus {
    pub open: Option<MonitorInfo>,
}

/// Where the editor's watch goes: its channel (or a test double). An error drops it.
pub trait StatusSink: Send + Sync {
    fn send(&self, status: ProjectionStatus) -> Result<(), String>;
}

impl StatusSink for Channel<ProjectionStatus> {
    fn send(&self, status: ProjectionStatus) -> Result<(), String> {
        Channel::send(self, status).map_err(|error| error.to_string())
    }
}

fn stale(why: impl ToString) -> CommandError {
    CommandError::new(STALE, why)
}

fn invalid(why: impl ToString) -> CommandError {
    CommandError::new(INVALID, why)
}

/// The page projected.
#[derive(Debug, Clone)]
struct Scene {
    /// The editor's note as last sent, its strokes kept up to date by the `strokes` updates.
    note: Note,
    /// The key and size of its staged image; `None` without an image, or while it is not staged.
    image: Option<ImagePixels>,
    view: LiveViewState,
}

/// The editor's stroke in progress, as sent to the window so far.
#[derive(Debug, Clone)]
struct Current {
    /// The editor's number for the stroke (the events' `id`).
    id: u32,
    stroke: LiveStroke,
}

/// The window's subscription.
struct Subscriber {
    /// Tells this subscription from a later one, when a send to it fails.
    id: u64,
    sink: Arc<dyn BatchSink>,
}

/// What the projection keeps. Pure: it makes the events, [`ProjectionRuntime`] sends them.
#[derive(Default)]
struct Projection {
    /// The screen the window is on; `None` while it is closed (updates are then refused).
    open: Option<MonitorInfo>,
    /// The number of the window open now (0: none).
    window: u64,
    /// The last number given to a window being opened (or moved).
    last_window: u64,
    /// The highest number of a window destroyed before its opening was recorded.
    gone: u64,
    scene: Option<Scene>,
    current: Option<Current>,
    /// What the snapshots and `strokesEdited` say; grows at every page and every edit.
    revision: u64,
    subscriber: Option<Subscriber>,
    next_subscriber: u64,
    /// The editor's watch on the window.
    watcher: Option<Arc<dyn StatusSink>>,
}

/// A number the events hold as a `u32` (an index, an identifier, a layer): whole, from 0.
fn whole(value: f64, what: &str) -> CommandResult<u32> {
    if value.is_finite() && value >= 0.0 && value.fract() == 0.0 && value <= f64::from(u32::MAX) {
        Ok(value as u32)
    } else {
        Err(invalid(format!("{what} is not a whole number: {value}")))
    }
}

/// The editor's points as the window draws them: every number finite (else `projectionInvalid`),
/// at most [`MAX_PROJECTION_POINTS`]; the coordinates within one page size around the page (as
/// the marks and live's computer pen), the pressure within 0..=1, the time a whole number of ms.
fn live_points(points: &[ProjectionPoint], page: PageSize) -> CommandResult<Vec<LivePoint>> {
    if points.len() > MAX_PROJECTION_POINTS {
        return Err(invalid(format!(
            "{} points in one update (at most {MAX_PROJECTION_POINTS})",
            points.len()
        )));
    }
    let (width, height) = (f64::from(page.width), f64::from(page.height));
    points
        .iter()
        .map(|point| {
            let ProjectionPoint { x, y, p, t, wall } = *point;
            if ![x, y, p, t, wall].iter().all(|value| value.is_finite()) {
                return Err(invalid(format!("a point is not finite: {point:?}")));
            }
            Ok(LivePoint {
                x: x.clamp(-width, 2.0 * width) as f32,
                y: y.clamp(-height, 2.0 * height) as f32,
                p: p.clamp(0.0, 1.0) as f32,
                t: t.floor().clamp(0.0, f64::from(u32::MAX)) as u32,
                recv: wall,
            })
        })
        .collect()
}

/// The projected area as live's view: zoom 1..=8, the centre on the page, no blackboard, no laser
/// (Q13: they are live's own).
fn live_view(view: ProjectionView, page: PageSize) -> LiveViewState {
    LiveViewState {
        zoom: view.zoom,
        x: view.x,
        y: view.y,
        board: false,
        laser: false,
    }
    .sanitized(page)
}

/// `count` strokes, in a tail or on the page: at most [`MAX_PROJECTION_STROKES`].
fn stroke_count(count: usize, what: &str) -> CommandResult<()> {
    if count > MAX_PROJECTION_STROKES {
        return Err(invalid(format!(
            "{count} strokes {what} (at most {MAX_PROJECTION_STROKES})"
        )));
    }
    Ok(())
}

/// A page the window can draw: a size, a layer, at most [`MAX_PROJECTION_STROKES`] strokes.
fn check_page(note: &Note) -> CommandResult<()> {
    if note.page.width == 0 || note.page.height == 0 {
        return Err(invalid(format!("the page has no size: {:?}", note.page)));
    }
    if note.layers.is_empty() {
        return Err(invalid("the page has no layer"));
    }
    stroke_count(note.strokes.len(), "on the page")
}

/// The key and size of the image staged under `key` for `note`; `None` when the note has no image,
/// or no key, or nothing is staged under it (the window then shows the paper's colour).
fn image_pixels(note: &Note, key: Option<&str>, staging: &Staging) -> Option<ImagePixels> {
    note.background_image.as_ref()?;
    let key = key?;
    let staged = staging.get(key)?;
    Some(ImagePixels {
        key: key.to_owned(),
        width: staged.width,
        height: staged.height,
    })
}

/// The window's first event for the stroke in progress (also after a snapshot).
fn stroke_start(current: &Current) -> LiveEvent {
    LiveEvent::StrokeStart {
        page_index: 0,
        stroke: current.stroke.clone(),
        pen: LivePen::Computer,
        id: Some(current.id),
        hidden: false,
    }
}

fn stroke_cancel(id: u32) -> LiveEvent {
    LiveEvent::StrokeCancel {
        page_index: 0,
        pen: LivePen::Computer,
        id: Some(id),
        hidden: false,
    }
}

impl Projection {
    fn status(&self) -> ProjectionStatus {
        ProjectionStatus {
            open: self.open.clone(),
        }
    }

    /// What a window draws from scratch: the page as a one-page live session (`state: live`, its
    /// paper, its image's pixels, the projected area, never an undo), or, without a page, an idle
    /// session on an empty A4 page (the window stays black). The stroke in progress is not in it.
    fn snapshot(&self) -> LiveSnapshot {
        let (state, page, view, image) = match &self.scene {
            Some(scene) => (
                LiveSessionState::Live,
                scene.note.clone(),
                scene.view,
                scene.image.clone(),
            ),
            None => {
                let mut page = blank_note("", "", 0);
                page.id.clear();
                let view = LiveViewState::whole(page.page);
                (LiveSessionState::Idle, page, view, None)
            }
        };
        LiveSnapshot {
            state,
            page_index: 0,
            page_count: 1,
            orientation: Orientation::default(),
            can_turn_page: false,
            layer: 0,
            computer_layer: 0,
            style: LiveStyle::default(),
            view,
            hover: None,
            presentation: self.open.clone(),
            saved: Vec::new(),
            revision: self.revision,
            can_undo: false,
            can_redo: false,
            page_paper: page.background.paper,
            image_pixels: image,
            tablet: None,
            page,
        }
    }

    /// A snapshot, then the stroke in progress.
    fn snapshot_events(&self) -> (Arc<LiveSnapshot>, Vec<LiveEvent>) {
        let snapshot = Arc::new(self.snapshot());
        let mut events = vec![LiveEvent::Snapshot {
            snapshot: snapshot.clone(),
        }];
        events.extend(self.current.as_ref().map(stroke_start));
        (snapshot, events)
    }

    /// The page size of the scene, or `projectionStale` without one.
    fn page_size(&self) -> CommandResult<PageSize> {
        self.scene
            .as_ref()
            .map(|scene| scene.note.page)
            .ok_or_else(|| stale("no page is projected"))
    }

    /// Applies `update` (with `image`, the staged pixels of a `Page`'s image) and returns the
    /// events that tell it to the window, in order.
    fn apply(
        &mut self,
        update: ProjectionUpdate,
        image: Option<ImagePixels>,
    ) -> CommandResult<Vec<LiveEvent>> {
        if self.open.is_none() {
            return Err(stale("the projection is closed"));
        }
        match update {
            ProjectionUpdate::Page { note, view, .. } => self.page(note, image, view),
            ProjectionUpdate::Strokes { keep, tail, ended } => self.strokes(keep, tail, ended),
            ProjectionUpdate::View { view } => self.view(view),
            ProjectionUpdate::StrokeBegin {
                id,
                tool,
                color,
                width,
                layer,
                points,
            } => self.begin(id, LiveStyle { tool, color, width }, layer, &points),
            ProjectionUpdate::StrokePoints { id, points } => self.points(id, &points),
            ProjectionUpdate::StrokeCancel { id } => {
                let id = whole(id, "id")?;
                Ok(self.drop_current(id).into_iter().collect())
            }
        }
    }

    /// A new page: its snapshot, then the stroke in progress (kept).
    fn page(
        &mut self,
        note: Note,
        image: Option<ImagePixels>,
        view: ProjectionView,
    ) -> CommandResult<Vec<LiveEvent>> {
        check_page(&note)?;
        let view = live_view(view, note.page);
        self.revision += 1;
        self.scene = Some(Scene { note, image, view });
        Ok(self.snapshot_events().1)
    }

    /// The strokes from `keep` on replaced by `tail`: a `strokesEdited` (none when nothing
    /// changes), then the `strokeCancel` of the stroke in progress when it is `ended`.
    fn strokes(
        &mut self,
        keep: f64,
        tail: Vec<Stroke>,
        ended: Option<f64>,
    ) -> CommandResult<Vec<LiveEvent>> {
        let keep = whole(keep, "keep")? as usize;
        let ended = ended.map(|id| whole(id, "ended")).transpose()?;
        stroke_count(tail.len(), "in one tail")?;
        let scene = self
            .scene
            .as_mut()
            .ok_or_else(|| stale("no page is projected"))?;
        let strokes = &mut scene.note.strokes;
        if keep > strokes.len() {
            return Err(stale(format!(
                "{keep} strokes kept, the page has {}",
                strokes.len()
            )));
        }
        stroke_count(keep + tail.len(), "on the page")?;
        let mut events = Vec::new();
        if keep < strokes.len() || !tail.is_empty() {
            // Every index fits a `u32`: the page holds at most `MAX_PROJECTION_STROKES`.
            let removed: Vec<u32> = (keep..strokes.len()).map(|i| i as u32).collect();
            let inserted: Vec<PlacedStroke> = tail
                .iter()
                .enumerate()
                .map(|(i, stroke)| PlacedStroke {
                    index: (keep + i) as u32,
                    stroke: stroke.clone(),
                })
                .collect();
            strokes.truncate(keep);
            strokes.extend(tail);
            self.revision += 1;
            events.push(LiveEvent::StrokesEdited {
                page_index: 0,
                revision: self.revision,
                removed,
                inserted: Arc::new(inserted),
                recoloured: Vec::new(),
                can_undo: false,
                can_redo: false,
                viewers: None,
            });
        }
        // After the edit, in the same batch: the stroke is on the page when it leaves the pen, no
        // frame without it.
        events.extend(ended.and_then(|id| self.drop_current(id)));
        Ok(events)
    }

    /// The projected area moved or zoomed (nothing when it is the same).
    fn view(&mut self, view: ProjectionView) -> CommandResult<Vec<LiveEvent>> {
        let scene = self
            .scene
            .as_mut()
            .ok_or_else(|| stale("no page is projected"))?;
        let view = live_view(view, scene.note.page);
        if view == scene.view {
            return Ok(Vec::new());
        }
        scene.view = view;
        Ok(vec![LiveEvent::View { view }])
    }

    /// A stroke begins; it replaces a stroke in progress, which the window drops first (kept, it
    /// would be among the window's strokes and not among the scene's).
    fn begin(
        &mut self,
        id: f64,
        style: LiveStyle,
        layer: f64,
        points: &[ProjectionPoint],
    ) -> CommandResult<Vec<LiveEvent>> {
        let id = whole(id, "id")?;
        let layer = whole(layer, "layer")?;
        let mut points = live_points(points, self.page_size()?)?;
        points.truncate(MAX_PROJECTED_STROKE_POINTS);
        let style = style.sanitized();
        let mut events: Vec<LiveEvent> = self
            .current
            .take()
            .map(|old| stroke_cancel(old.id))
            .into_iter()
            .collect();
        let current = Current {
            id,
            stroke: LiveStroke {
                tool: style.tool,
                color: style.color,
                width: style.width,
                layer,
                points,
            },
        };
        events.push(stroke_start(&current));
        self.current = Some(current);
        Ok(events)
    }

    /// More points of the stroke in progress `id` (ignored for another stroke, and beyond
    /// [`MAX_PROJECTED_STROKE_POINTS`]).
    fn points(&mut self, id: f64, points: &[ProjectionPoint]) -> CommandResult<Vec<LiveEvent>> {
        let id = whole(id, "id")?;
        let points = live_points(points, self.page_size()?)?;
        let Some(current) = self.current.as_mut().filter(|current| current.id == id) else {
            return Ok(Vec::new());
        };
        let room = MAX_PROJECTED_STROKE_POINTS.saturating_sub(current.stroke.points.len());
        let points: Vec<LivePoint> = points.into_iter().take(room).collect();
        if points.is_empty() {
            return Ok(Vec::new());
        }
        current.stroke.points.extend_from_slice(&points);
        Ok(vec![LiveEvent::Points {
            page_index: 0,
            points,
            pen: LivePen::Computer,
            id: Some(id),
            hidden: false,
        }])
    }

    /// The stroke in progress `id` leaves the window (dropped, or now on the page); nothing for
    /// another stroke.
    fn drop_current(&mut self, id: u32) -> Option<LiveEvent> {
        if self.current.as_ref()?.id != id {
            return None;
        }
        self.current = None;
        Some(stroke_cancel(id))
    }

    /// A number for a window about to be opened or moved.
    fn next_window(&mut self) -> u64 {
        self.last_window += 1;
        self.last_window
    }

    /// The window `window` is open on `monitor` (created, or moved there): `None` when it was
    /// destroyed meanwhile, else whether the screen changed. An older number than the window's
    /// (two openings at once) leaves the newer one's screen.
    fn opened(&mut self, window: u64, monitor: MonitorInfo) -> Option<bool> {
        if window <= self.gone {
            return None;
        }
        if window < self.window {
            return Some(false);
        }
        self.window = window;
        let changed = self.open.as_ref() != Some(&monitor);
        self.open = Some(monitor);
        Some(changed)
    }

    /// The window `window` was destroyed; true when that closed the projection. Another window's
    /// number is ignored: an older window's late report keeps the one open now; a newer one's
    /// (destroyed before its opening was recorded) is remembered.
    fn window_destroyed(&mut self, window: u64) -> bool {
        if window != 0 && window == self.window {
            return self.close();
        }
        if window > self.window {
            self.gone = self.gone.max(window);
        }
        false
    }

    /// The projection is closed: no window, no page, no stroke, no subscriber. True when it was
    /// open.
    fn close(&mut self) -> bool {
        self.window = 0;
        self.scene = None;
        self.current = None;
        self.subscriber = None;
        self.open.take().is_some()
    }
}

/// The editor's projection of the running app.
#[derive(Default)]
pub struct ProjectionRuntime {
    /// Held from the making of a batch to its sending (see the module's doc). Taken before
    /// `state`, never while holding it.
    sending: Mutex<()>,
    state: Mutex<Projection>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Tells the editor's watch where the projection is (dropped when that fails). Under the scene's
/// lock, so that two changes reach it in their order: a status is a few bytes.
fn tell(state: &mut Projection) {
    let status = state.status();
    if let Some(watcher) = &state.watcher
        && watcher.send(status).is_err()
    {
        state.watcher = None;
    }
}

impl ProjectionRuntime {
    /// Applies one update from the editor and sends its batch to the window, if one subscribed
    /// (else it is kept for the window's snapshot). `staging`: where a page's image is staged.
    pub fn update(&self, update: ProjectionUpdate, staging: &Staging) -> CommandResult<()> {
        // Outside the scene's lock (the staging has its own).
        let image = match &update {
            ProjectionUpdate::Page {
                note, image_key, ..
            } => image_pixels(note, image_key.as_deref(), staging),
            _ => None,
        };
        let _sending = lock(&self.sending);
        let mut state = lock(&self.state);
        let events = state.apply(update, image)?;
        let subscriber = state
            .subscriber
            .as_ref()
            .map(|subscriber| (subscriber.id, subscriber.sink.clone()));
        drop(state);
        if let Some((id, sink)) = subscriber {
            self.send(id, &sink, events);
        }
        Ok(())
    }

    /// The window's subscription, replacing the previous one: the snapshot returned, and the
    /// first batch starts with it and the stroke in progress (the window applies the batches in
    /// order whichever of the answer and the batch arrives first, as with the hub).
    pub fn subscribe(&self, sink: Arc<dyn BatchSink>) -> Arc<LiveSnapshot> {
        let _sending = lock(&self.sending);
        let mut state = lock(&self.state);
        let (snapshot, events) = state.snapshot_events();
        let id = state.next_subscriber;
        state.next_subscriber += 1;
        state.subscriber = Some(Subscriber {
            id,
            sink: sink.clone(),
        });
        drop(state);
        self.send(id, &sink, events);
        snapshot
    }

    /// Sends `events` as one batch to the subscription `id`, which is dropped when that fails.
    /// The caller holds `sending`, not `state`.
    fn send(&self, id: u64, sink: &Arc<dyn BatchSink>, events: Vec<LiveEvent>) {
        if events.is_empty() {
            return;
        }
        let batch = LiveBatch {
            events,
            sent_at: wall_now_ms(),
        };
        if sink.send(batch).is_err() {
            let mut state = lock(&self.state);
            if state.subscriber.as_ref().is_some_and(|s| s.id == id) {
                state.subscriber = None;
            }
        }
    }

    /// The editor's watch, replacing the previous one: the status now, then at every change.
    pub fn watch(&self, sink: Arc<dyn StatusSink>) -> ProjectionStatus {
        let mut state = lock(&self.state);
        state.watcher = Some(sink);
        state.status()
    }

    #[cfg(test)]
    pub fn status(&self) -> ProjectionStatus {
        lock(&self.state).status()
    }

    /// A number for the window about to be opened or moved (see [`opened`](Self::opened)).
    pub fn next_window(&self) -> u64 {
        lock(&self.state).next_window()
    }

    /// The window `window` is shown on `monitor`; false when it was destroyed meanwhile.
    pub fn opened(&self, window: u64, monitor: MonitorInfo) -> bool {
        let mut state = lock(&self.state);
        match state.opened(window, monitor) {
            None => false,
            Some(changed) => {
                if changed {
                    tell(&mut state);
                }
                true
            }
        }
    }

    /// The window `window` was destroyed (its own handler, on the main thread).
    pub fn window_destroyed(&self, window: u64) {
        let mut state = lock(&self.state);
        if state.window_destroyed(window) {
            tell(&mut state);
        }
    }

    /// The projection is closed, whatever its window (`projection_close`, live's presentation):
    /// the page, the stroke in progress and the subscriber go, the watch is told.
    pub fn closed(&self) {
        let mut state = lock(&self.state);
        if state.close() {
            tell(&mut state);
        }
    }
}

fn unexpected(error: tauri::Error) -> CommandError {
    CommandError::new("unexpected", error)
}

/// Destroys the projection window, if there is one, and closes the projection at once (the
/// window's own report of its destruction comes later, and is then ignored).
pub fn close(app: &AppHandle, projection: &ProjectionRuntime) -> CommandResult<()> {
    presentation::destroy(app, ScreenWindow::Projection).map_err(unexpected)?;
    projection.closed();
    Ok(())
}

/// Opens the projection window full screen on `monitor` (else the saved screen, else a secondary
/// one), or moves it there, and saves that choice for this PC (one projector per PC, shared with
/// live's presentation). Live's presentation window is destroyed first; the hub is told it closed,
/// never that this one opened (the live session's snapshots are not about the editor).
#[tauri::command]
pub async fn projection_open(
    app: AppHandle,
    context: State<'_, AppContext>,
    runtime: State<'_, LiveRuntime>,
    projection: State<'_, ProjectionRuntime>,
    monitor: Option<String>,
) -> CommandResult<MonitorInfo> {
    let chosen = presentation::screen_for(&app, &context, monitor.as_deref())?;
    presentation::destroy(&app, ScreenWindow::Presentation).map_err(unexpected)?;
    runtime.hub().presentation_closed(PRESENTATION);
    let window = projection.next_window();
    let handle = app.clone();
    let on_destroyed = Box::new(move || {
        if let Some(projection) = handle.try_state::<ProjectionRuntime>() {
            projection.window_destroyed(window);
        }
    });
    presentation::show_on(
        &app,
        ScreenWindow::Projection,
        &chosen,
        &runtime.hub().activity(),
        Some(on_destroyed),
    )
    .map_err(unexpected)?;
    presentation::remember_screen(&context, &chosen.key);
    if !projection.opened(window, chosen.clone()) {
        return Err(CommandError::new(
            "unexpected",
            "the projection window closed as it opened",
        ));
    }
    Ok(chosen)
}

/// Closes the projection window (Esc in it, « Arrêter », the editor going).
#[tauri::command]
pub async fn projection_close(
    app: AppHandle,
    projection: State<'_, ProjectionRuntime>,
) -> CommandResult<()> {
    close(&app, &projection)
}

/// The editor's watch on the window: the status now, then at every change.
#[tauri::command]
pub async fn projection_watch(
    projection: State<'_, ProjectionRuntime>,
    on_event: Channel<ProjectionStatus>,
) -> CommandResult<ProjectionStatus> {
    Ok(projection.watch(Arc::new(on_event)))
}

/// The window's subscription: the snapshot now (an idle, empty page while nothing is projected),
/// then live's batches. Written as the first batch holds it, not copied.
#[tauri::command]
pub async fn projection_subscribe(
    projection: State<'_, ProjectionRuntime>,
    on_event: Channel<LiveBatch>,
) -> CommandResult<SnapshotReply> {
    Ok(SnapshotReply(projection.subscribe(Arc::new(on_event))))
}

/// One update from the editor, applied and sent to the window.
#[tauri::command]
pub async fn projection_update(
    projection: State<'_, ProjectionRuntime>,
    update: ProjectionUpdate,
) -> CommandResult<()> {
    projection.update(update, background::staging())
}

#[cfg(test)]
mod tests;
