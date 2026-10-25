//! The live session (SPEC §4.2): pages that become notes, strokes, hover, layers, style and view,
//! driven by the tablet's live input and by the interface's commands.
//!
//! Pure: no I/O and no clock (times come with the input). The hub (`hub.rs`) shares it between
//! threads, sends what it emits to the windows and writes its pages to the library.

use std::path::PathBuf;
use std::time::Instant;

use ardoise_core::library::FileStamp;
use ardoise_core::note::{Background, Layer, Note, Origin, PageSize, Paper, Timing};
use ardoise_core::settings::LiveButton;
use ardoise_ink::{Point, Stroke};
use ardoise_proto::SLATE_MAX_PRESSURE;
use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::Point as PenPoint;

use super::model::{
    HoverPoint, LiveCommandKind, LiveEvent, LivePoint, LiveSessionState, LiveSnapshot, LiveStroke,
    LiveStyle, LiveViewState,
};
use crate::library::path_text;
use crate::presentation::MonitorInfo;

/// Interval between two records of the tablet (protocol.md §7.2: one record every 5 ms).
pub const SAMPLE_MS: f64 = 5.0;

/// Where a page is written, as last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedFile {
    pub path: PathBuf,
    pub stamp: FileStamp,
}

/// One page of the session: a note, and where it stands on disk.
#[derive(Debug, Clone)]
pub struct Page {
    pub note: Note,
    /// Bumped at every change of `note`.
    pub version: u64,
    /// `version` of what is on disk (0: nothing).
    pub saved_version: u64,
    pub file: Option<SavedFile>,
    /// Write it at the next pass, without waiting for the save interval (first stroke ended,
    /// page left, live mode stopped).
    pub urgent: bool,
    /// Last write attempt (set by the hub).
    pub last_attempt: Option<Instant>,
    /// Why the last write failed, once the windows were told (set by the hub): cleared by the
    /// next write that succeeds.
    pub save_error: Option<String>,
    /// The narration recorded on this page in live mode (note time), as the note's `audio.opus`
    /// member written with every version of the page. Only the encoded bytes stay in memory: a
    /// later recording on the page decodes them again to merge with them.
    pub audio: Option<Vec<u8>>,
    /// No stroke started yet: the note's time origin is still free.
    fresh: bool,
    /// Time of the page's last pen sample, ms since `note.created`.
    last_t: Option<u32>,
}

impl Page {
    fn new(note: Note) -> Page {
        Page {
            note,
            version: 0,
            saved_version: 0,
            file: None,
            urgent: false,
            last_attempt: None,
            save_error: None,
            audio: None,
            fresh: true,
            last_t: None,
        }
    }

    /// The page's time origin (`note.created`, the wall clock of its first stroke in Unix ms),
    /// or `None` while it has no stroke: a narration has nothing to follow there.
    pub fn time_origin(&self) -> Option<i64> {
        (!self.note.strokes.is_empty()).then_some(self.note.created)
    }

    /// Gives the page its narration, as `audio.opus` bytes: it is written again at the next
    /// pass. Refused (`false`) on a page without any stroke.
    pub fn set_audio(&mut self, audio: Vec<u8>) -> bool {
        if self.time_origin().is_none() {
            return false;
        }
        self.audio = Some(audio);
        self.touch();
        self.urgent = true;
        true
    }

    fn touch(&mut self) {
        self.version += 1;
    }

    /// Changed since it was written, and worth a file: a note is created only once a stroke
    /// ended on the page, then rewritten whenever it changes.
    pub fn needs_write(&self) -> bool {
        self.version != self.saved_version && (self.file.is_some() || !self.note.strokes.is_empty())
    }
}

/// The stroke being drawn.
#[derive(Debug, Clone)]
struct Drawing {
    page: usize,
    style: LiveStyle,
    layer: u32,
    points: Vec<LivePoint>,
    /// `strokeStart` was emitted.
    announced: bool,
    /// Trailing points not emitted yet.
    unsent: usize,
}

impl Drawing {
    fn live_stroke(&self, points: &[LivePoint]) -> LiveStroke {
        LiveStroke {
            tool: self.style.tool,
            color: self.style.color,
            width: self.style.width,
            layer: self.layer,
            points: points.to_vec(),
        }
    }
}

/// Name of the `n`th layer (1-based) in the interface language, as the editor names them.
pub fn layer_name(french: bool, n: usize) -> String {
    if french {
        format!("Calque {n}")
    } else {
        format!("Layer {n}")
    }
}

/// Pressure 0..=2047 as 0..=1 (SPEC §6.2).
pub fn normalized_pressure(pressure: u16) -> f32 {
    (f32::from(pressure) / f32::from(SLATE_MAX_PRESSURE)).clamp(0.0, 1.0)
}

/// A live session: its pages (at least one), the current page, style, view and state.
#[derive(Debug, Clone)]
pub struct LiveSession {
    id: u64,
    pages: Vec<Page>,
    current: usize,
    /// Drawing area of the tablet: the size of new pages.
    size: PageSize,
    style: LiveStyle,
    /// Style before the blackboard was turned on, given back when it is turned off.
    style_before_board: Option<LiveStyle>,
    view: LiveViewState,
    state: LiveSessionState,
    hover: Option<HoverPoint>,
    drawing: Option<Drawing>,
    /// Layer names in French.
    french: bool,
    /// The screen the presentation window is on (kept here so that every snapshot carries it).
    presentation: Option<MonitorInfo>,
}

impl LiveSession {
    /// A session with one blank page created at `now_ms`, idle.
    pub fn new(
        id: u64,
        size: PageSize,
        style: LiveStyle,
        view: LiveViewState,
        french: bool,
        now_ms: i64,
    ) -> LiveSession {
        let mut session = LiveSession {
            id,
            pages: Vec::new(),
            current: 0,
            size,
            style: style.sanitized(),
            style_before_board: None,
            view: view.sanitized(size),
            state: LiveSessionState::Idle,
            hover: None,
            drawing: None,
            french,
            presentation: None,
        };
        let page = session.new_page(now_ms);
        session.pages.push(page);
        session
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    #[cfg(test)]
    pub fn state(&self) -> LiveSessionState {
        self.state
    }

    /// Started and not stopped.
    pub fn is_running(&self) -> bool {
        self.state != LiveSessionState::Idle
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    pub fn page_mut(&mut self, index: usize) -> Option<&mut Page> {
        self.pages.get_mut(index)
    }

    /// Index of the current page.
    pub fn current(&self) -> usize {
        self.current
    }

    pub fn style(&self) -> LiveStyle {
        self.style
    }

    pub fn view(&self) -> LiveViewState {
        self.view
    }

    #[cfg(test)]
    pub fn size(&self) -> PageSize {
        self.size
    }

    /// A `saveFailed` event for every page whose last write failed.
    pub fn save_failures(&self) -> Vec<LiveEvent> {
        self.pages
            .iter()
            .enumerate()
            .filter_map(|(page_index, page)| {
                let message = page.save_error.clone()?;
                Some(LiveEvent::SaveFailed {
                    page_index,
                    message,
                })
            })
            .collect()
    }

    /// Paths of the notes written so far, in page order.
    pub fn saved(&self) -> Vec<String> {
        self.pages
            .iter()
            .filter_map(|page| page.file.as_ref())
            .map(|file| path_text(&file.path))
            .collect()
    }

    pub fn presentation(&self) -> Option<&MonitorInfo> {
        self.presentation.as_ref()
    }

    /// The presentation window opened on `presentation`, or closed (`None`).
    pub fn set_presentation(
        &mut self,
        presentation: Option<MonitorInfo>,
        out: &mut Vec<LiveEvent>,
    ) {
        if presentation != self.presentation {
            self.presentation = presentation;
            self.push_snapshot(out);
        }
    }

    /// Every page that changed is written at the next pass.
    pub fn mark_all_urgent(&mut self) {
        for page in &mut self.pages {
            if page.needs_write() {
                page.urgent = true;
            }
        }
    }

    fn current_page(&self) -> &Page {
        // `pages` is never empty and `current` always indexes it.
        &self.pages[self.current]
    }

    fn current_layer(&self) -> u32 {
        let layers = self.current_page().note.layers.len();
        u32::try_from(layers.saturating_sub(1)).unwrap_or(0)
    }

    fn new_page(&self, now_ms: i64) -> Page {
        let paper = if self.view.board {
            Paper::Slate
        } else {
            Paper::White
        };
        Page::new(Note {
            id: uuid::Uuid::new_v4().to_string(),
            title: String::new(),
            created: now_ms,
            modified: now_ms,
            page: self.size,
            background: Background {
                paper,
                ..Background::default()
            },
            background_image: None,
            tags: Vec::new(),
            favorite: false,
            layers: vec![Layer {
                name: layer_name(self.french, 1),
                visible: true,
            }],
            strokes: Vec::new(),
            timing: Timing::Recorded,
            origin: Origin::Live,
        })
    }

    /// The whole session, for a window that draws it from scratch.
    pub fn snapshot(&self) -> LiveSnapshot {
        LiveSnapshot {
            state: self.state,
            page: self.current_page().note.clone(),
            page_index: self.current,
            page_count: self.pages.len(),
            layer: self.current_layer(),
            style: self.style,
            view: self.view,
            hover: self.hover,
            presentation: self.presentation.clone(),
            saved: self.saved(),
        }
    }

    /// A snapshot, then the stroke in progress (it is not in the snapshot's page).
    fn push_snapshot(&self, out: &mut Vec<LiveEvent>) {
        out.push(LiveEvent::Snapshot {
            snapshot: Box::new(self.snapshot()),
        });
        out.extend(self.in_progress());
    }

    /// The stroke in progress as already sent, to follow a snapshot.
    pub fn in_progress(&self) -> Option<LiveEvent> {
        let drawing = self.drawing.as_ref().filter(|d| d.announced)?;
        let sent = drawing.points.len() - drawing.unsent;
        Some(LiveEvent::StrokeStart {
            page_index: drawing.page,
            stroke: drawing.live_stroke(&drawing.points[..sent]),
        })
    }

    /// State reported by the tablet service, or set by the hub (start, stop).
    pub fn set_state(&mut self, state: LiveSessionState, out: &mut Vec<LiveEvent>) {
        if state == self.state {
            return;
        }
        if state != LiveSessionState::Live {
            self.end_stroke(out);
            self.hover_end(out);
        }
        self.state = state;
        self.push_snapshot(out);
    }

    /// The tablet streams; its drawing area is `width` × `height`. Pages not drawn on yet take
    /// that size.
    pub fn started(&mut self, width: u32, height: u32, out: &mut Vec<LiveEvent>) {
        let size = PageSize { width, height };
        let resized = width > 0 && height > 0 && size != self.size;
        if resized {
            self.size = size;
            for page in &mut self.pages {
                if page.fresh && page.note.strokes.is_empty() && page.file.is_none() {
                    page.note.page = size;
                }
            }
            let page = self.current_page().note.page;
            self.view = if self.view.zoom <= 1.0 {
                LiveViewState {
                    board: self.view.board,
                    laser: self.view.laser,
                    ..LiveViewState::whole(page)
                }
            } else {
                self.view.sanitized(page)
            };
        }
        if self.state == LiveSessionState::Live {
            if resized {
                self.push_snapshot(out);
            }
        } else {
            self.set_state(LiveSessionState::Live, out);
        }
    }

    /// Pen records of one notification, 5 ms apart, the last one received at `received_ms`
    /// (wall clock, fractional ms). Already oriented by the tablet service.
    pub fn records(&mut self, records: &[LiveRecord], received_ms: f64, out: &mut Vec<LiveEvent>) {
        if !self.is_running() {
            return;
        }
        if self.state != LiveSessionState::Live {
            self.set_state(LiveSessionState::Live, out);
        }
        let last = records.len().saturating_sub(1);
        for (i, record) in records.iter().enumerate() {
            let at = received_ms - SAMPLE_MS * (last - i) as f64;
            match record {
                LiveRecord::Pen(point) if point.pressure > 0 => {
                    self.pen_down(point, at, received_ms, out);
                }
                LiveRecord::Pen(point) => {
                    self.end_stroke(out);
                    self.hover_at(point, out);
                }
                LiveRecord::ProximityOut => {
                    self.end_stroke(out);
                    self.hover_end(out);
                }
            }
        }
        self.flush_points(out);
    }

    fn pen_down(&mut self, point: &PenPoint, at: f64, received_ms: f64, out: &mut Vec<LiveEvent>) {
        if self.drawing.is_none() {
            self.hover_end(out);
            self.drawing = Some(Drawing {
                page: self.current,
                style: self.style,
                layer: self.current_layer(),
                points: Vec::new(),
                announced: false,
                unsent: 0,
            });
        }
        let t = self.point_time(at);
        if let Some(drawing) = &mut self.drawing {
            drawing.points.push(LivePoint {
                x: f32::from(point.x),
                y: f32::from(point.y),
                p: normalized_pressure(point.pressure),
                t,
                recv: received_ms,
            });
            drawing.unsent += 1;
        }
    }

    /// Time of a sample taken at `at` (wall clock, ms) on the current page, ms since the note's
    /// `created`. The first sample of a page sets `created`; then samples are at least 5 ms
    /// apart (the tablet's rate) and follow the wall clock across pauses, so times never go
    /// back even when notifications arrive in bursts.
    fn point_time(&mut self, at: f64) -> u32 {
        let page = &mut self.pages[self.current];
        if page.fresh {
            page.fresh = false;
            page.note.created = at.floor() as i64;
            page.note.modified = page.note.created;
        }
        let since = at - page.note.created as f64;
        let t = match page.last_t {
            Some(last) => since.max(f64::from(last) + SAMPLE_MS),
            None => since.max(0.0),
        };
        // Whole milliseconds, like `created`: the first sample is at 0.
        let t = t.floor().clamp(0.0, f64::from(u32::MAX)) as u32;
        page.last_t = Some(t);
        t
    }

    fn hover_at(&mut self, point: &PenPoint, out: &mut Vec<LiveEvent>) {
        let hover = HoverPoint {
            x: f32::from(point.x),
            y: f32::from(point.y),
        };
        self.hover = Some(hover);
        out.push(LiveEvent::Hover {
            x: hover.x,
            y: hover.y,
        });
    }

    fn hover_end(&mut self, out: &mut Vec<LiveEvent>) {
        if self.hover.take().is_some() {
            out.push(LiveEvent::HoverEnd);
        }
    }

    /// Sends the points drawn since the last call: `strokeStart` for a new stroke, else
    /// `points`.
    fn flush_points(&mut self, out: &mut Vec<LiveEvent>) {
        let Some(drawing) = &mut self.drawing else {
            return;
        };
        if !drawing.announced {
            drawing.announced = true;
            drawing.unsent = 0;
            out.push(LiveEvent::StrokeStart {
                page_index: drawing.page,
                stroke: drawing.live_stroke(&drawing.points),
            });
        } else if drawing.unsent > 0 {
            let from = drawing.points.len() - drawing.unsent;
            drawing.unsent = 0;
            out.push(LiveEvent::Points {
                page_index: drawing.page,
                points: drawing.points[from..].to_vec(),
            });
        }
    }

    /// Ends the stroke in progress: it joins its page.
    pub fn end_stroke(&mut self, out: &mut Vec<LiveEvent>) {
        self.flush_points(out);
        let Some(drawing) = self.drawing.take() else {
            return;
        };
        let Some(page) = self.pages.get_mut(drawing.page) else {
            return;
        };
        page.note.strokes.push(Stroke {
            tool: drawing.style.tool,
            color: drawing.style.color,
            width: drawing.style.width,
            layer: drawing.layer,
            points: drawing
                .points
                .iter()
                .map(|p| Point {
                    x: p.x,
                    y: p.y,
                    pressure: p.p,
                    t: p.t,
                })
                .collect(),
        });
        page.touch();
        if page.file.is_none() {
            page.urgent = true;
        }
        out.push(LiveEvent::StrokeEnd {
            page_index: drawing.page,
        });
    }

    /// Leaves page `index`: its changes are written now.
    fn leave(&mut self, index: usize) {
        if let Some(page) = self.pages.get_mut(index)
            && page.needs_write()
        {
            page.urgent = true;
        }
    }

    /// Runs a command (`button`: what the tablet's button does, from the settings; `now_ms`:
    /// creation time of a new page).
    pub fn command(
        &mut self,
        kind: LiveCommandKind,
        button: LiveButton,
        now_ms: i64,
        out: &mut Vec<LiveEvent>,
    ) {
        match kind {
            LiveCommandKind::NextPage => {
                self.end_stroke(out);
                let left = self.current;
                if self.current + 1 < self.pages.len() {
                    self.current += 1;
                } else {
                    let page = self.new_page(now_ms);
                    self.pages.push(page);
                    self.current = self.pages.len() - 1;
                }
                self.leave(left);
                self.push_snapshot(out);
            }
            LiveCommandKind::PreviousPage => {
                if self.current == 0 {
                    return;
                }
                self.end_stroke(out);
                let left = self.current;
                self.current -= 1;
                self.leave(left);
                self.push_snapshot(out);
            }
            LiveCommandKind::NewPage => {
                self.end_stroke(out);
                let left = self.current;
                let page = self.new_page(now_ms);
                self.pages.push(page);
                self.current = self.pages.len() - 1;
                self.leave(left);
                self.push_snapshot(out);
            }
            LiveCommandKind::NewLayer => {
                self.end_stroke(out);
                let french = self.french;
                let page = &mut self.pages[self.current];
                let n = page.note.layers.len() + 1;
                page.note.layers.push(Layer {
                    name: layer_name(french, n),
                    visible: true,
                });
                page.touch();
                self.push_snapshot(out);
            }
            LiveCommandKind::ClearPage => {
                // The stroke in progress goes too: the snapshot replaces everything.
                self.drawing = None;
                let page = &mut self.pages[self.current];
                page.note.strokes.clear();
                page.touch();
                self.push_snapshot(out);
            }
            LiveCommandKind::Button => {
                let kind = match button {
                    LiveButton::NewPage => LiveCommandKind::NewPage,
                    LiveButton::NewLayer => LiveCommandKind::NewLayer,
                };
                self.command(kind, button, now_ms, out);
            }
        }
    }

    /// Ink of the strokes to come (the stroke in progress keeps its own).
    pub fn set_style(&mut self, style: LiveStyle, out: &mut Vec<LiveEvent>) {
        let style = style.sanitized();
        if style == self.style {
            return;
        }
        self.style = style;
        self.push_snapshot(out);
    }

    /// The shared view. Turning the blackboard on gives the current page (and new pages) slate
    /// paper and switches to white chalk; turning it off gives white paper and the style used
    /// before back. Other pages keep their paper, so that ink written on one stays visible.
    pub fn set_view(&mut self, view: LiveViewState, out: &mut Vec<LiveEvent>) {
        let view = view.sanitized(self.current_page().note.page);
        if view == self.view {
            return;
        }
        let board_changed = view.board != self.view.board;
        self.view = view;
        out.push(LiveEvent::View { view });
        if !board_changed {
            return;
        }
        let paper = if view.board {
            self.style_before_board = Some(self.style);
            self.style = LiveStyle::chalk();
            Paper::Slate
        } else {
            self.style = self.style_before_board.take().unwrap_or_default();
            Paper::White
        };
        let page = &mut self.pages[self.current];
        if page.note.background.paper != paper {
            page.note.background.paper = paper;
            page.touch();
        }
        self.push_snapshot(out);
    }
}

#[cfg(test)]
mod tests {
    use ardoise_core::note::A4_LANDSCAPE;
    use ardoise_ink::{Rgba, Tool};

    use super::*;

    const T0: f64 = 1_792_843_200_000.0;

    fn pen(x: u16, y: u16, pressure: u16) -> LiveRecord {
        LiveRecord::Pen(PenPoint { x, y, pressure })
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
        session.started(29_700, 21_600, &mut out);
        assert_eq!(session.state(), LiveSessionState::Live);
        session
    }

    fn is_snapshot(event: &LiveEvent) -> bool {
        matches!(event, LiveEvent::Snapshot { .. })
    }

    /// The events other than snapshots.
    fn events(out: &[LiveEvent]) -> Vec<&LiveEvent> {
        out.iter().filter(|e| !is_snapshot(e)).collect()
    }

    fn snapshots(out: &[LiveEvent]) -> usize {
        out.iter().filter(|e| is_snapshot(e)).count()
    }

    #[test]
    fn pressure_starts_continues_and_ends_a_stroke() {
        let mut session = running();
        let mut out = Vec::new();
        session.records(
            &[pen(100, 200, 0), pen(110, 210, 1024), pen(120, 220, 2047)],
            T0,
            &mut out,
        );
        let sent = events(&out);
        assert!(matches!(sent[0], LiveEvent::Hover { x, y } if *x == 100.0 && *y == 200.0));
        assert_eq!(*sent[1], LiveEvent::HoverEnd);
        let LiveEvent::StrokeStart { page_index, stroke } = sent[2] else {
            panic!("{sent:?}");
        };
        assert_eq!(*page_index, 0);
        assert_eq!(stroke.tool, Tool::Ballpoint);
        assert_eq!(stroke.color, Rgba::BLACK);
        assert_eq!(stroke.layer, 0);
        assert_eq!(stroke.points.len(), 2);
        assert!((stroke.points[0].p - 1024.0 / 2047.0).abs() < 1e-6);
        assert_eq!(stroke.points[1].p, 1.0);
        assert_eq!(stroke.points[0].recv, T0);
        // The first sample sets the note's time origin; records are 5 ms apart.
        assert_eq!(session.pages()[0].note.created, (T0 - 5.0) as i64);
        assert_eq!((stroke.points[0].t, stroke.points[1].t), (0, 5));
        assert!(
            session.pages()[0].note.strokes.is_empty(),
            "still in progress"
        );

        out.clear();
        session.records(&[pen(130, 230, 900), pen(140, 240, 0)], T0 + 15.0, &mut out);
        let sent = events(&out);
        let LiveEvent::Points { points, .. } = sent[0] else {
            panic!("{sent:?}");
        };
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].t, 15);
        assert_eq!(*sent[1], LiveEvent::StrokeEnd { page_index: 0 });
        assert!(matches!(sent[2], LiveEvent::Hover { .. }));
        let page = &session.pages()[0];
        assert_eq!(page.note.strokes.len(), 1);
        let stroke = &page.note.strokes[0];
        assert_eq!(stroke.points.len(), 3);
        assert_eq!(stroke.points[2].x, 130.0);
        assert!(
            page.urgent,
            "the note is created when its first stroke ends"
        );
        assert!(page.needs_write());

        // The pen leaving ends a stroke too.
        out.clear();
        session.records(
            &[pen(10, 10, 500), LiveRecord::ProximityOut],
            T0 + 500.0,
            &mut out,
        );
        let sent = events(&out);
        assert!(matches!(sent[0], LiveEvent::HoverEnd));
        assert!(matches!(sent[1], LiveEvent::StrokeStart { .. }));
        assert_eq!(*sent[2], LiveEvent::StrokeEnd { page_index: 0 });
        assert_eq!(session.pages()[0].note.strokes.len(), 2);
        assert_eq!(session.pages()[0].note.strokes[1].points[0].t, 500);
    }

    #[test]
    fn point_times_never_go_back_in_bursts() {
        let mut session = running();
        let mut out = Vec::new();
        // Two notifications delivered 1 ms apart (a BLE burst), then one late.
        session.records(&[pen(1, 1, 9), pen(2, 2, 9), pen(3, 3, 9)], T0, &mut out);
        session.records(
            &[pen(4, 4, 9), pen(5, 5, 9), pen(6, 6, 9)],
            T0 + 1.0,
            &mut out,
        );
        session.records(&[pen(7, 7, 9)], T0 + 80.0, &mut out);
        // Stamps with a fraction: whole milliseconds from the first sample.
        session.records(&[pen(8, 8, 9)], T0 + 85.7, &mut out);
        session.end_stroke(&mut out);
        let times: Vec<u32> = session.pages()[0].note.strokes[0]
            .points
            .iter()
            .map(|p| p.t)
            .collect();
        assert_eq!(times, [0, 5, 10, 15, 20, 25, 90, 95]);
        let mut session = running();
        session.records(&[pen(1, 1, 9), pen(2, 2, 9)], T0 + 0.6, &mut out);
        session.end_stroke(&mut out);
        let points = &session.pages()[0].note.strokes[0].points;
        assert_eq!((points[0].t, points[1].t), (0, 5));
        assert_eq!(session.pages()[0].note.created, T0 as i64 - 5);
    }

    #[test]
    fn hovering_writes_nothing_even_with_the_laser() {
        let mut session = running();
        let mut out = Vec::new();
        let mut view = session.view();
        view.laser = true;
        session.set_view(view, &mut out);
        assert_eq!(events(&out), [&LiveEvent::View { view }]);
        assert_eq!(snapshots(&out), 0);
        out.clear();
        session.records(&[pen(5, 6, 0), pen(7, 8, 0)], T0, &mut out);
        session.records(&[LiveRecord::ProximityOut], T0 + 5.0, &mut out);
        let sent = events(&out);
        assert_eq!(sent.len(), 3);
        assert!(matches!(sent[1], LiveEvent::Hover { x, .. } if *x == 7.0));
        assert_eq!(*sent[2], LiveEvent::HoverEnd);
        let page = &session.pages()[0];
        assert!(page.note.strokes.is_empty());
        assert!(!page.needs_write(), "nothing to write while hovering");
        assert!(page.fresh, "hovering does not set the time origin");
        assert_eq!(session.snapshot().hover, None);
    }

    #[test]
    fn pages_follow_the_commands() {
        let mut session = running();
        let mut out = Vec::new();
        session.records(&[pen(1, 1, 50)], T0, &mut out);
        out.clear();
        // Next page while drawing: the stroke ends on page 1, page 2 is created.
        session.command(LiveCommandKind::NextPage, LiveButton::NewPage, 7, &mut out);
        assert_eq!(*events(&out)[0], LiveEvent::StrokeEnd { page_index: 0 });
        assert_eq!(snapshots(&out), 1);
        assert_eq!((session.current(), session.pages().len()), (1, 2));
        assert_eq!(session.pages()[0].note.strokes.len(), 1);
        assert!(session.pages()[0].urgent);
        assert_eq!(session.pages()[1].note.created, 7);
        assert_ne!(session.pages()[0].note.id, session.pages()[1].note.id);

        session.command(
            LiveCommandKind::PreviousPage,
            LiveButton::NewPage,
            8,
            &mut out,
        );
        assert_eq!(session.current(), 0);
        out.clear();
        session.command(
            LiveCommandKind::PreviousPage,
            LiveButton::NewPage,
            8,
            &mut out,
        );
        assert!(out.is_empty(), "no page before the first");
        session.command(LiveCommandKind::NextPage, LiveButton::NewPage, 9, &mut out);
        assert_eq!((session.current(), session.pages().len()), (1, 2));
        session.command(LiveCommandKind::NewPage, LiveButton::NewPage, 10, &mut out);
        assert_eq!((session.current(), session.pages().len()), (2, 3));
        let snapshot = session.snapshot();
        assert_eq!((snapshot.page_index, snapshot.page_count), (2, 3));
        assert_eq!(snapshot.page.origin, Origin::Live);
        assert_eq!(snapshot.page.title, "");
        assert_eq!(snapshot.page.page, A4_LANDSCAPE);
        assert_eq!(snapshot.page.background.paper, Paper::White);
    }

    #[test]
    fn layers_receive_the_new_strokes() {
        let mut session = running();
        let mut out = Vec::new();
        session.command(LiveCommandKind::NewLayer, LiveButton::NewPage, 0, &mut out);
        assert_eq!(snapshots(&out), 1);
        let snapshot = session.snapshot();
        assert_eq!(snapshot.layer, 1);
        assert_eq!(snapshot.page.layers[1].name, "Calque 2");
        session.records(&[pen(1, 1, 50), pen(2, 2, 0)], T0, &mut out);
        assert_eq!(session.pages()[0].note.strokes[0].layer, 1);
        assert_eq!(layer_name(false, 3), "Layer 3");
    }

    #[test]
    fn clearing_removes_the_strokes_and_the_stroke_in_progress() {
        let mut session = running();
        let mut out = Vec::new();
        session.records(&[pen(1, 1, 50), pen(2, 2, 0)], T0, &mut out);
        session.records(&[pen(3, 3, 50)], T0 + 100.0, &mut out);
        let before = session.pages()[0].version;
        out.clear();
        session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
        assert_eq!((out.len(), snapshots(&out)), (1, 1));
        assert!(session.pages()[0].note.strokes.is_empty());
        assert!(session.in_progress().is_none());
        assert!(session.pages()[0].version > before);
        // Still pressing: a new stroke starts.
        out.clear();
        session.records(&[pen(4, 4, 50)], T0 + 105.0, &mut out);
        assert!(matches!(events(&out)[0], LiveEvent::StrokeStart { .. }));
    }

    #[test]
    fn the_button_follows_the_setting() {
        let mut session = running();
        let mut out = Vec::new();
        session.command(LiveCommandKind::Button, LiveButton::NewPage, 0, &mut out);
        assert_eq!(session.pages().len(), 2);
        assert_eq!(session.current(), 1);
        session.command(LiveCommandKind::Button, LiveButton::NewLayer, 0, &mut out);
        assert_eq!(session.pages().len(), 2);
        assert_eq!(session.pages()[1].note.layers.len(), 2);
    }

    #[test]
    fn the_blackboard_gives_slate_and_chalk_then_gives_them_back() {
        let mut session = running();
        let mut out = Vec::new();
        let red = LiveStyle {
            tool: Tool::FeltTip,
            color: Rgba::rgb(200, 0, 0),
            width: 110.0,
        };
        session.set_style(red, &mut out);
        assert_eq!((out.len(), snapshots(&out)), (1, 1));
        out.clear();
        let mut view = session.view();
        view.board = true;
        session.set_view(view, &mut out);
        assert_eq!(snapshots(&out), 1);
        assert_eq!(session.style(), LiveStyle::chalk());
        assert_eq!(session.pages()[0].note.background.paper, Paper::Slate);
        session.command(LiveCommandKind::NewPage, LiveButton::NewPage, 0, &mut out);
        assert_eq!(session.pages()[1].note.background.paper, Paper::Slate);
        session.records(&[pen(1, 1, 50), pen(2, 2, 0)], T0, &mut out);
        let stroke = &session.pages()[1].note.strokes[0];
        assert_eq!((stroke.tool, stroke.color), (Tool::Chalk, Rgba::WHITE));

        view.board = false;
        session.set_view(view, &mut out);
        assert_eq!(session.style(), red);
        assert_eq!(session.pages()[1].note.background.paper, Paper::White);
        assert_eq!(
            session.pages()[0].note.background.paper,
            Paper::Slate,
            "other pages keep their paper"
        );
    }

    #[test]
    fn view_and_style_are_kept_within_bounds() {
        let mut session = running();
        let mut out = Vec::new();
        session.set_view(
            LiveViewState {
                zoom: 40.0,
                x: f64::NAN,
                y: -5.0,
                board: false,
                laser: false,
            },
            &mut out,
        );
        let view = session.view();
        assert_eq!(view.zoom, 8.0);
        assert_eq!(view.x, 29_700.0 / 2.0);
        assert_eq!(view.y, 0.0);
        session.set_style(
            LiveStyle {
                tool: Tool::Brush,
                color: Rgba::BLACK,
                width: f32::INFINITY,
            },
            &mut out,
        );
        assert_eq!(session.style().width, Tool::Brush.default_width());
    }

    #[test]
    fn a_snapshot_mid_stroke_is_followed_by_the_stroke_so_far() {
        let mut session = running();
        let mut out = Vec::new();
        session.records(&[pen(1, 1, 50), pen(2, 2, 60)], T0, &mut out);
        let Some(LiveEvent::StrokeStart { stroke, .. }) = session.in_progress() else {
            panic!("no stroke in progress");
        };
        assert_eq!(stroke.points.len(), 2);
        assert!(session.snapshot().page.strokes.is_empty());
        // A style change does not touch the stroke in progress; its snapshot is followed by the
        // stroke so far, which the next points continue.
        out.clear();
        session.set_style(LiveStyle::chalk(), &mut out);
        assert!(matches!(&out[..], [
            LiveEvent::Snapshot { snapshot },
            LiveEvent::StrokeStart { stroke, .. },
        ] if snapshot.page.strokes.is_empty() && stroke.points.len() == 2
            && stroke.tool == Tool::Ballpoint));
        out.clear();
        session.records(&[pen(3, 3, 60), pen(3, 3, 0)], T0 + 10.0, &mut out);
        assert!(matches!(&out[0], LiveEvent::Points { points, .. } if points.len() == 1));
        let stroke = &session.pages()[0].note.strokes[0];
        assert_eq!(stroke.tool, Tool::Ballpoint);
        assert_eq!(stroke.points.len(), 3);
    }

    #[test]
    fn state_changes_end_the_stroke_and_resize_fresh_pages() {
        let mut session = LiveSession::new(
            1,
            A4_LANDSCAPE,
            LiveStyle::default(),
            LiveViewState::whole(A4_LANDSCAPE),
            false,
            0,
        );
        let mut out = Vec::new();
        // Idle: records are ignored.
        session.records(&[pen(1, 1, 50)], T0, &mut out);
        assert!(out.is_empty());
        session.set_state(LiveSessionState::Waiting, &mut out);
        session.started(21_600, 14_800, &mut out);
        assert_eq!(session.pages()[0].note.page.width, 21_600);
        assert_eq!(session.size().height, 14_800);
        assert_eq!((session.view().x, session.view().y), (10_800.0, 7_400.0));
        session.records(&[pen(1, 1, 50)], T0, &mut out);
        out.clear();
        session.set_state(LiveSessionState::Reconnecting, &mut out);
        assert_eq!(*events(&out)[0], LiveEvent::StrokeEnd { page_index: 0 });
        assert_eq!(snapshots(&out), 1);
        // Records after the link came back: live again.
        out.clear();
        session.records(&[pen(1, 1, 0)], T0 + 900.0, &mut out);
        assert_eq!(session.state(), LiveSessionState::Live);
        assert_eq!(snapshots(&out), 1);
    }
}
