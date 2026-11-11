//! The live session (SPEC §4.2): pages that become notes, strokes, hover, layers, style and view,
//! driven by the tablet's live input and by the interface's commands.
//!
//! Pure: no I/O and no clock (times come with the input). The hub (`hub.rs`) shares it between
//! threads, sends what it emits to the windows and writes its pages to the library.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use ardoise_core::library::FileStamp;
use ardoise_core::note::{Background, Layer, Note, Origin, PageSize, Paper, Timing};
use ardoise_core::settings::{LiveButton, Orientation};
use ardoise_device::pages::{oriented_size, turn_point};
use ardoise_ink::{Point, Rgba, Stroke};
use ardoise_proto::SLATE_MAX_PRESSURE;
use ardoise_proto::live::LiveRecord;
use ardoise_proto::stroke::Point as PenPoint;

use super::edits::{
    CheckedEdit, ColourChange, EditAction, History, LiveEditError, Prepared, RedoStep, UndoStep,
    grouped, hidden_before, insert_at, layer_hidden, remove_at, viewer_place,
};
use super::layers::{
    CheckedLayerOp, MAX_LAYERS, existing, follow_merge, follow_move, merge_layer, move_layer,
};
use super::model::{
    ForkReason, ForkedSheet, HoverPoint, LiveCommandKind, LiveEvent, LiveMark, LivePen, LivePoint,
    LiveSessionState, LiveSnapshot, LiveStroke, LiveStyle, LiveViewState, PlacedStroke,
    RecolouredStrokes, ViewerIndices, sanitize_marks,
};
use super::pen::{CheckedPen, MAX_COMPUTER_POINTS, PenAction, PenSample, note_time, sample_wall};
use super::resume::{PenMapping, ResumeBase, ResumeSeed, hold_for};
use crate::library::path_text;
use crate::presentation::MonitorInfo;
use crate::share::page::is_drawn;

/// Interval between two records of the tablet (protocol.md §7.2: one record every 5 ms).
pub const SAMPLE_MS: f64 = 5.0;

/// Where a page is written, as last seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedFile {
    pub path: PathBuf,
    pub stamp: FileStamp,
    /// Fingerprint of what was written (`hub::file_fingerprint`: not the title, tags or favourite
    /// mark, which the library changes): tells a note only retitled elsewhere from one whose
    /// content was changed. `None` when it could not be computed.
    pub content: Option<u64>,
}

/// What became of a resumed sheet that changed elsewhere or was deleted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetFork {
    pub reason: ForkReason,
    /// The note left as it is, as last known.
    pub original: String,
    /// The new note in the same binder, where the page goes on.
    pub path: String,
}

/// What a page seeded from an existing note has that a page of the session does not.
#[derive(Debug, Clone)]
struct Resumed {
    /// The file as read at the resume: what every write of the page starts from.
    base: Arc<ResumeBase>,
    /// Wall clock (Unix ms) at note time 0 for what this session adds: the resume, less the note
    /// time at which it continues. `note.created` stays what it was (the note's date).
    origin: i64,
    /// This session wrote the file: until then the note is not among the session's notes.
    rewritten: bool,
    fork: Option<SheetFork>,
    /// Ink was added while the blackboard was on: the sheet then needs the slate it was written
    /// on, white chalk being invisible on most paper. Until then the blackboard is a matter of
    /// display, which the file does not hear of (`hub::written_sheet`).
    ink_on_board: bool,
    /// A deliberate change of the layers in the live window (add, rename, show or hide, move,
    /// merge; not where a pen acts): from then on the sheet's layers are written as they are, new
    /// ink or not, as in the editor (`hub::written_sheet`). The layer the session adds by itself
    /// (every layer hidden, the tablet's button) stays out of the file until ink goes on it.
    layers_edited: bool,
}

/// One page of the session: a note, and where it stands on disk.
#[derive(Debug, Clone)]
pub struct Page {
    pub note: Note,
    /// What the windows call this page, for as long as it lasts: the id it started with. The note's
    /// own id changes when a write has to make a new note (`hub::write_page`), and a window takes
    /// another id for another page: it would drop its marks and outlines, and a viewer the picture
    /// of the paper, while nothing on screen changed.
    shown_id: String,
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
    /// How long the last write that succeeded took (set by the hub): a slow one waits longer for
    /// the next (`hub::save_wait`).
    pub last_write: Option<Duration>,
    /// Why the last write failed, once the windows were told (set by the hub): cleared by the
    /// next write that succeeds: the library error's code (`errorCodes.*`) and its detail.
    pub save_error: Option<(&'static str, String)>,
    /// The narration recorded on this page in live mode (note time), as the note's `audio.opus`
    /// member written with every version of the page. Only the encoded bytes stay in memory: a
    /// later recording on the page decodes them again to merge with them. Shared with the writes
    /// under way: hours of sound (a resumed note may start with them) are not copied under the hub's
    /// lock at each pass.
    pub audio: Option<Arc<Vec<u8>>>,
    /// How the tablet was held for the page's ink: the page on screen follows the tablet
    /// ([`LiveSession::reorient`]). On a resumed sheet, how it is to be held: the tablet follows
    /// the page ([`hold_for`]).
    orientation: Orientation,
    /// No stroke started yet: the note's time origin is still free.
    fresh: bool,
    /// Time of the page's last pen sample, ms since `note.created`.
    last_t: Option<u32>,
    /// The paper the blackboard took the page from, given back when it is turned off (`None`:
    /// white, for a page made while the blackboard was on).
    paper_before_board: Option<Paper>,
    /// Seeded from an existing note.
    resumed: Option<Resumed>,
    /// What the indices of its strokes mean ([`LiveSnapshot::revision`]). Starts from the
    /// session's [`first_revision`], so that a command meant for a session that has ended cannot
    /// pass for one of the next. Changes when strokes leave the list or come back: a delete, a
    /// clearing that removed something, the undo or redo of either. Not when the tablet appends a
    /// stroke (no index moves), nor for a change of colour, a page turn, a new layer.
    revision: u64,
    /// Its edits and the computer's strokes, to undo and redo: kept when the page is left and when
    /// it turns, dropped when the session stops and by a merge of layers.
    history: History,
    /// The tablet's layer: where its ink goes (an index of `note.layers`; it may be hidden).
    tablet_layer: u32,
    /// The computer's layer: where its pen, its eraser end and the selection act (an index of
    /// `note.layers`). Both stay with the page when it is left.
    computer_layer: u32,
}

/// What one step of an edit did, for the event that tells the windows.
#[derive(Debug, Default)]
struct Edited {
    removed: Vec<u32>,
    inserted: Arc<Vec<PlacedStroke>>,
    recoloured: Vec<RecolouredStrokes>,
    viewers: Option<Box<ViewerIndices>>,
}

/// The first revision of the pages of session `id`. Sessions never run out of it (2^32 changes per
/// page) and stay within what a number holds exactly in the interface (2^21 sessions per run).
fn first_revision(id: u64) -> u64 {
    id << 32
}

impl Page {
    fn new(note: Note, orientation: Orientation, revision: u64) -> Page {
        Page {
            shown_id: note.id.clone(),
            note,
            version: 0,
            saved_version: 0,
            file: None,
            urgent: false,
            last_attempt: None,
            last_write: None,
            save_error: None,
            audio: None,
            orientation,
            fresh: true,
            last_t: None,
            paper_before_board: None,
            resumed: None,
            revision,
            history: History::default(),
            tablet_layer: 0,
            computer_layer: 0,
        }
    }

    /// The page of a note resumed at `now_ms`, for a tablet set to `setting`. A note whose layers
    /// are all hidden gets a visible layer to write on (hidden layers stay hidden: one may hold
    /// the answers); that alone is no change, so it is written only with what is written on it.
    fn resumed(
        seed: ResumeSeed,
        setting: Orientation,
        french: bool,
        now_ms: i64,
        revision: u64,
    ) -> Page {
        let ResumeSeed {
            base,
            saved,
            time_base,
        } = seed;
        let mut note = base.file.note.clone();
        if !note.layers.iter().any(|layer| layer.visible) {
            note.layers.push(Layer {
                name: layer_name(french, note.layers.len() + 1),
                visible: true,
            });
        }
        // The tablet writes on the last visible layer, and the computer starts there too.
        let layer = note
            .layers
            .iter()
            .rposition(|layer| layer.visible)
            .and_then(|layer| u32::try_from(layer).ok())
            .unwrap_or(0);
        let orientation = hold_for(note.page, setting);
        let mut page = Page::new(note, orientation, revision);
        page.tablet_layer = layer;
        page.computer_layer = layer;
        page.file = Some(saved);
        page.audio = base.file.audio.clone().map(Arc::new);
        page.fresh = false;
        // Its first new sample is at the time base at the earliest: the stamps of a notification
        // go back a little before it arrived, possibly to before the resume.
        page.last_t = time_base.checked_sub(SAMPLE_MS as u32);
        // Its own paper, whatever the blackboard does and from whichever page it is turned off: a
        // sheet with slate paper keeps it too.
        page.paper_before_board = Some(page.note.background.paper);
        page.resumed = Some(Resumed {
            base,
            origin: now_ms.saturating_sub(i64::from(time_base)),
            rewritten: false,
            fork: None,
            ink_on_board: false,
            layers_edited: false,
        });
        page
    }

    /// The tablet's layer and the computer's.
    #[cfg(test)]
    pub fn pen_layers(&self) -> (u32, u32) {
        (self.tablet_layer, self.computer_layer)
    }

    /// The resumed sheet's layers were changed on purpose: see the field of `Resumed`.
    pub fn layers_edited(&self) -> bool {
        self.resumed
            .as_ref()
            .is_some_and(|resumed| resumed.layers_edited)
    }

    /// A deliberate change of the page's layers.
    fn layers_changed(&mut self) {
        if let Some(resumed) = &mut self.resumed {
            resumed.layers_edited = true;
        }
        self.touch();
    }

    /// What the indices of the page's strokes mean: see the field.
    #[cfg(test)]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Seeded from an existing note: never turned, and written through its file.
    pub fn is_resumed(&self) -> bool {
        self.resumed.is_some()
    }

    /// The file a resumed sheet came from.
    pub fn base(&self) -> Option<&Arc<ResumeBase>> {
        self.resumed.as_ref().map(|resumed| &resumed.base)
    }

    /// The file of a note this session wrote. A resumed sheet has a file from the start, which
    /// counts only once the session changed it: a resume without any change wrote nothing.
    pub fn saved_file(&self) -> Option<&SavedFile> {
        match &self.resumed {
            Some(resumed) if !resumed.rewritten => None,
            _ => self.file.as_ref(),
        }
    }

    /// The hub wrote the page's file.
    pub fn mark_rewritten(&mut self) {
        if let Some(resumed) = &mut self.resumed {
            resumed.rewritten = true;
        }
    }

    /// The hub wrote the resumed sheet as a new note.
    pub fn set_fork(&mut self, fork: SheetFork) {
        if let Some(resumed) = &mut self.resumed {
            resumed.fork = Some(fork);
        }
    }

    /// What became of the resumed sheet, when it became a new note.
    pub fn fork(&self) -> Option<&SheetFork> {
        self.resumed
            .as_ref()
            .and_then(|resumed| resumed.fork.as_ref())
    }

    /// Ink was added to this resumed sheet under the blackboard: see the field.
    pub fn ink_on_board(&self) -> bool {
        self.resumed
            .as_ref()
            .is_some_and(|resumed| resumed.ink_on_board)
    }

    /// The wall clock (Unix ms) at note time 0 for what this session adds: `note.created`, the
    /// wall clock of the page's first stroke; on a resumed sheet, the origin shifted so that the
    /// time since the resume follows what the note holds. `None` until a stroke was started on a
    /// page of the session: a narration has nothing to follow there. Strokes that have left since
    /// (cleared, deleted: they can come back) change nothing to that.
    pub fn time_origin(&self) -> Option<i64> {
        match &self.resumed {
            Some(resumed) => Some(resumed.origin),
            None => (!self.fresh).then_some(self.note.created),
        }
    }

    /// Gives the page its narration, as `audio.opus` bytes: it is written again at the next
    /// pass. Refused (`false`) on a page where no stroke was ever started.
    pub fn set_audio(&mut self, audio: Vec<u8>) -> bool {
        if self.time_origin().is_none() {
            return false;
        }
        self.audio = Some(Arc::new(audio));
        self.touch();
        self.urgent = true;
        true
    }

    fn touch(&mut self) {
        self.version += 1;
    }

    /// Turns the page `turns` clockwise quarter turns, ink and all, so that it is held as
    /// `orientation` (the page size is swapped for an odd number of turns). Times, pressures,
    /// layers and narration are untouched. A resumed sheet is never turned (a turn knows nothing of
    /// its background image, and would rewrite a note for how the tablet happens to be held): the
    /// tablet follows it, as near to `orientation` as the sheet allows.
    fn turn(&mut self, turns: u8, orientation: Orientation) {
        if self.resumed.is_some() {
            self.orientation = hold_for(self.note.page, orientation);
            return;
        }
        if !turns.is_multiple_of(4) {
            let (width, height) = (self.note.page.width as f32, self.note.page.height as f32);
            for stroke in &mut self.note.strokes {
                for point in &mut stroke.points {
                    (point.x, point.y) = turn_point(point.x, point.y, width, height, turns);
                }
            }
            self.history.turn(width, height, turns);
            if turns % 2 == 1 {
                self.note.page = PageSize {
                    width: self.note.page.height,
                    height: self.note.page.width,
                };
            }
            self.touch();
        }
        self.orientation = orientation;
    }

    /// Changed since it was written, and worth a file: a note is created only once a stroke
    /// ended on the page, then rewritten whenever it changes.
    pub fn needs_write(&self) -> bool {
        self.version != self.saved_version && (self.file.is_some() || !self.note.strokes.is_empty())
    }

    /// Takes the strokes at `indices` (ascending, all on the page) off the page. A viewer is told of
    /// the visible ones only: it never had the others.
    fn take_strokes(&mut self, indices: Vec<u32>) -> (Vec<PlacedStroke>, Edited) {
        let (layers, list) = (&self.note.layers, &self.note.strokes);
        let viewers = hidden_before(layers, list).map(|hidden| {
            Box::new(ViewerIndices {
                removed: indices
                    .iter()
                    .filter_map(|&place| viewer_place(&hidden, layers, list, place))
                    .collect(),
                inserted: Vec::new(),
                recoloured: Vec::new(),
            })
        });
        let strokes = remove_at(&mut self.note.strokes, &indices);
        self.revision += 1;
        self.touch();
        let edited = Edited {
            removed: indices,
            viewers,
            ..Edited::default()
        };
        (strokes, edited)
    }

    /// Puts `stored` back at their places, through `copy`: the windows get `stored`, a viewer the
    /// visible ones only.
    fn give_back(&mut self, stored: Arc<Vec<PlacedStroke>>, copy: Vec<Stroke>) -> Edited {
        let places = stored.iter().map(|placed| placed.index);
        insert_at(&mut self.note.strokes, places.clone().zip(copy));
        self.revision += 1;
        self.touch();
        let (layers, list) = (&self.note.layers, &self.note.strokes);
        let viewers = hidden_before(layers, list).map(|hidden| {
            Box::new(ViewerIndices {
                removed: Vec::new(),
                inserted: places
                    .map(|place| viewer_place(&hidden, layers, list, place))
                    .collect(),
                recoloured: Vec::new(),
            })
        });
        Edited {
            inserted: stored,
            viewers,
            ..Edited::default()
        }
    }

    /// Gives strokes their colours (the indices are not checked again: they come from a step, or
    /// from a list just checked). A viewer is told of the visible ones only.
    fn colour_strokes(&mut self, colours: &[(u32, Rgba)]) -> Edited {
        for (index, color) in colours {
            if let Some(stroke) = self.note.strokes.get_mut(*index as usize) {
                stroke.color = *color;
            }
        }
        self.touch();
        let (layers, list) = (&self.note.layers, &self.note.strokes);
        let viewers = hidden_before(layers, list).map(|hidden| {
            let colours = colours.iter().filter_map(|&(place, color)| {
                Some((viewer_place(&hidden, layers, list, place)?, color))
            });
            Box::new(ViewerIndices {
                removed: Vec::new(),
                inserted: Vec::new(),
                recoloured: grouped(colours),
            })
        });
        Edited {
            recoloured: grouped(colours.iter().copied()),
            viewers,
            ..Edited::default()
        }
    }

    /// Time of a sample of the computer's pen taken at `wall` (Unix ms), ms since the note's
    /// `created`, from the origin the tablet's samples use ([`LiveSession::point_time`]): a fresh
    /// page takes it as its `created`; a resumed sheet counts from its shifted origin. Never below
    /// the stroke's `previous` time, and never through the tablet's spacing (`last_t`): neither
    /// pen pushes the other's times.
    fn computer_time(&mut self, wall: f64, previous: Option<u32>) -> u32 {
        if self.fresh {
            self.fresh = false;
            self.note.created = wall.floor() as i64;
            self.note.modified = self.note.created;
        }
        let origin = self
            .resumed
            .as_ref()
            .map_or(self.note.created, |resumed| resumed.origin);
        note_time(wall, origin, previous)
    }

    /// A stroke in progress ends on this page, whichever pen drew it: it joins the tail of the
    /// list (no place moves, the revision stays). Returns its place.
    fn finish(&mut self, drawing: &Drawing) -> u32 {
        let place = u32::try_from(self.note.strokes.len()).unwrap_or(u32::MAX);
        self.note.strokes.push(drawing.stroke());
        self.touch();
        // A page's first stroke creates its note, whichever pen.
        if self.file.is_none() {
            self.urgent = true;
        }
        // White chalk needs the slate it was written on, also on a sheet that was not on the
        // blackboard when it was turned on (windows draw slate either way).
        if drawing.on_board
            && let Some(resumed) = &mut self.resumed
        {
            resumed.ink_on_board = true;
            self.note.background.paper = Paper::Slate;
        }
        place
    }

    /// The event saying what a step did to this page, the `index`th.
    fn event(&self, index: usize, edited: Edited) -> LiveEvent {
        LiveEvent::StrokesEdited {
            page_index: index,
            revision: self.revision,
            removed: edited.removed,
            inserted: edited.inserted,
            recoloured: edited.recoloured,
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
            viewers: edited.viewers,
        }
    }
}

/// The stroke being drawn.
#[derive(Debug, Clone)]
struct Drawing {
    page: usize,
    style: LiveStyle,
    layer: u32,
    /// The blackboard was on when it began.
    on_board: bool,
    points: Vec<LivePoint>,
    /// `strokeStart` was emitted.
    announced: bool,
    /// Trailing points not emitted yet.
    unsent: usize,
}

impl Drawing {
    fn new(page: usize, style: LiveStyle, layer: u32, on_board: bool) -> Drawing {
        Drawing {
            page,
            style,
            layer,
            on_board,
            points: Vec::new(),
            announced: false,
            unsent: 0,
        }
    }

    fn live_stroke(&self, points: &[LivePoint]) -> LiveStroke {
        LiveStroke {
            tool: self.style.tool,
            color: self.style.color,
            width: self.style.width,
            layer: self.layer,
            points: points.to_vec(),
        }
    }

    /// The points drawn since the last call, as `pen` (`id`) sends them: `strokeStart` for a new
    /// stroke, else `points`; `None` when there is nothing new. `hidden`: the stroke's layer is
    /// hidden now.
    fn flush(&mut self, pen: LivePen, id: Option<u32>, hidden: bool) -> Option<LiveEvent> {
        if !self.announced {
            self.announced = true;
            self.unsent = 0;
            return Some(LiveEvent::StrokeStart {
                page_index: self.page,
                stroke: self.live_stroke(&self.points),
                pen,
                id,
                hidden,
            });
        }
        if self.unsent == 0 {
            return None;
        }
        let from = self.points.len() - self.unsent;
        self.unsent = 0;
        Some(LiveEvent::Points {
            page_index: self.page,
            points: self.points[from..].to_vec(),
            pen,
            id,
            hidden,
        })
    }

    /// The stroke so far, as already sent, to follow a snapshot (`None` before it was announced).
    fn so_far(&self, pen: LivePen, id: Option<u32>, hidden: bool) -> Option<LiveEvent> {
        if !self.announced {
            return None;
        }
        let sent = self.points.len() - self.unsent;
        Some(LiveEvent::StrokeStart {
            page_index: self.page,
            stroke: self.live_stroke(&self.points[..sent]),
            pen,
            id,
            hidden,
        })
    }

    /// The finished stroke, as its page keeps it.
    fn stroke(&self) -> Stroke {
        Stroke {
            tool: self.style.tool,
            color: self.style.color,
            width: self.style.width,
            layer: self.layer,
            points: self
                .points
                .iter()
                .map(|p| Point {
                    x: p.x,
                    y: p.y,
                    pressure: p.p,
                    t: p.t,
                })
                .collect(),
        }
    }
}

/// The computer's stroke being drawn (M14): what the tablet's has, plus the control window's number
/// for the gesture and the wall time of its last sample.
#[derive(Debug, Clone)]
struct ComputerDrawing {
    drawing: Drawing,
    /// The window's number for the gesture: a call naming another is not for this stroke.
    id: u32,
    /// Wall clock (Unix ms) of the last sample: the next is never before it.
    last_wall: Option<f64>,
}

/// Whether the stroke of `drawing` is on a hidden layer of its page.
fn drawn_hidden(pages: &[Page], drawing: &Drawing) -> bool {
    pages
        .get(drawing.page)
        .is_some_and(|page| layer_hidden(&page.note.layers, drawing.layer))
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
    /// The size of new pages: the tablet's drawing area, as the tablet is held.
    size: PageSize,
    /// How the tablet is held: the orientation of new pages.
    orientation: Orientation,
    style: LiveStyle,
    /// Style before the blackboard was turned on, given back when it is turned off.
    style_before_board: Option<LiveStyle>,
    /// The blackboard is on because the resumed sheet is slate, not because it was asked for: the
    /// next session does not inherit it.
    board_imposed: bool,
    view: LiveViewState,
    state: LiveSessionState,
    hover: Option<HoverPoint>,
    /// The tablet's stroke being drawn.
    drawing: Option<Drawing>,
    /// The computer's stroke being drawn, beside the tablet's (M14).
    computer: Option<ComputerDrawing>,
    /// Layer names in French.
    french: bool,
    /// The screen the presentation window is on (kept here so that every snapshot carries it).
    presentation: Option<MonitorInfo>,
    /// The marks written over the presentation, and the page they were written on (as it is
    /// held). Temporary: not part of any page, never written, gone when the page changes.
    marks: Vec<LiveMark>,
    marks_page: Option<(usize, Orientation, PageSize)>,
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
            orientation: Orientation::Landscape,
            style: style.sanitized(),
            style_before_board: None,
            board_imposed: false,
            view: view.sanitized(size),
            state: LiveSessionState::Idle,
            hover: None,
            drawing: None,
            computer: None,
            french,
            presentation: None,
            marks: Vec::new(),
            marks_page: None,
        };
        let page = session.new_page(now_ms);
        session.pages.push(page);
        session
    }

    pub fn id(&self) -> u64 {
        self.id
    }

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

    #[cfg(test)]
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

    #[cfg(test)]
    pub fn orientation(&self) -> Orientation {
        self.orientation
    }

    /// The session starts held as `orientation` (before the tablet streams): `size` is the
    /// page size it implies.
    pub fn with_orientation(mut self, orientation: Orientation) -> Self {
        self.orientation = orientation;
        for page in &mut self.pages {
            page.orientation = orientation;
        }
        self
    }

    /// The session's one page becomes the note of `seed`, resumed at `now_ms`; the next page is a
    /// new note, as always. The view shows the whole sheet, and the blackboard is on only for a
    /// sheet with slate paper, whatever it was before: it never changes the paper of a sheet by
    /// itself. On slate paper the ink is chalk, as when the blackboard is turned on.
    pub fn resuming(mut self, seed: ResumeSeed, now_ms: i64) -> Self {
        let revision = first_revision(self.id);
        let page = Page::resumed(seed, self.orientation, self.french, now_ms, revision);
        let board = page.note.background.paper == Paper::Slate;
        self.view = LiveViewState {
            board,
            laser: self.view.laser,
            ..LiveViewState::whole(page.note.page)
        };
        if board {
            self.style_before_board = Some(self.style);
            self.style = LiveStyle::chalk();
            self.board_imposed = true;
        }
        self.pages = vec![page];
        self.current = 0;
        self
    }

    /// The style in use, or the one the blackboard replaced.
    pub fn style_off_board(&self) -> LiveStyle {
        self.style_before_board.unwrap_or(self.style)
    }

    /// The session starts on the blackboard the session before it left on: `before` is the ink to
    /// give back when it is turned off.
    pub fn with_style_before_board(mut self, before: Option<LiveStyle>) -> Self {
        self.style_before_board = before;
        self
    }

    /// What a new session takes from this one: the ink in use, the ink the blackboard replaced, and
    /// whether the blackboard stays on: only if someone turned it on. One that came on by itself
    /// with a slate sheet stays with that sheet, and the ink from before it comes back.
    pub fn carried_ink(&self) -> (LiveStyle, Option<LiveStyle>, bool) {
        if self.view.board && !self.board_imposed {
            (self.style, self.style_before_board, true)
        } else {
            (self.style_off_board(), None, false)
        }
    }

    /// A `saveFailed` event for every page whose last write failed.
    pub fn save_failures(&self) -> Vec<LiveEvent> {
        self.pages
            .iter()
            .enumerate()
            .filter_map(|(page_index, page)| {
                let (code, message) = page.save_error.clone()?;
                Some(LiveEvent::SaveFailed {
                    page_index,
                    code: code.to_string(),
                    message,
                })
            })
            .collect()
    }

    /// Paths of the notes written so far, in page order.
    pub fn saved(&self) -> Vec<String> {
        self.pages
            .iter()
            .filter_map(Page::saved_file)
            .map(|file| path_text(&file.path))
            .collect()
    }

    /// Every resumed sheet that became a new note.
    pub fn forked_sheets(&self) -> Vec<ForkedSheet> {
        self.pages
            .iter()
            .enumerate()
            .filter_map(|(page_index, page)| {
                let fork = page.fork()?;
                Some(ForkedSheet {
                    page_index,
                    reason: fork.reason,
                    original: fork.original.clone(),
                    path: fork.path.clone(),
                })
            })
            .collect()
    }

    /// A `sheetForked` event for every resumed sheet that became a new note.
    pub fn sheet_forks(&self) -> Vec<LiveEvent> {
        self.forked_sheets()
            .into_iter()
            .map(|fork| LiveEvent::SheetForked {
                page_index: fork.page_index,
                reason: fork.reason,
                original: fork.original,
                path: fork.path,
            })
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
            // The window that wrote the marks is gone: they would stay on the viewers for good.
            if self.presentation.is_none() {
                self.clear_marks(out);
            }
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

    fn new_page(&self, now_ms: i64) -> Page {
        let paper = if self.view.board {
            Paper::Slate
        } else {
            Paper::White
        };
        let note = Note {
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
        };
        Page::new(note, self.orientation, first_revision(self.id))
    }

    /// The whole session, for a window that draws it from scratch.
    pub fn snapshot(&self) -> LiveSnapshot {
        let mut page = self.current_page().note.clone();
        page.id.clone_from(&self.current_page().shown_id);
        LiveSnapshot {
            state: self.state,
            page,
            page_index: self.current,
            page_count: self.pages.len(),
            orientation: self.current_page().orientation,
            can_turn_page: !self.current_page().is_resumed(),
            layer: self.current_page().tablet_layer,
            computer_layer: self.current_page().computer_layer,
            style: self.style,
            view: self.view,
            hover: self.hover,
            presentation: self.presentation.clone(),
            saved: self.saved(),
            revision: self.current_page().revision,
            can_undo: self.current_page().history.can_undo(),
            can_redo: self.current_page().history.can_redo(),
        }
    }

    /// A snapshot, then the strokes in progress (they are not in the snapshot's page), each told
    /// again whether its layer is hidden. If the page is no longer the one the marks were written
    /// on, they go (an empty list follows).
    fn push_snapshot(&mut self, out: &mut Vec<LiveEvent>) {
        out.push(LiveEvent::Snapshot {
            snapshot: Arc::new(self.snapshot()),
        });
        out.extend(self.in_progress());
        if !self.marks.is_empty() && self.marks_page != Some(self.held_page()) {
            self.clear_marks(out);
        }
    }

    /// The page on screen as the marks see it: another page, or the same one turned (the quick
    /// switch of the orientation), is another page for them.
    fn held_page(&self) -> (usize, Orientation, PageSize) {
        (
            self.current,
            self.orientation,
            self.current_page().note.page,
        )
    }

    /// The marks written over the presentation, as a late window starts from them (`None`
    /// when there are none).
    pub fn marks_event(&self) -> Option<LiveEvent> {
        (!self.marks.is_empty()).then(|| LiveEvent::Marks {
            marks: self.marks.clone(),
        })
    }

    /// The presentation window's marks for page `page_index`, whole (see
    /// [`sanitize_marks`] for what is kept). Ignored when the page on screen is another one:
    /// the list was written before a page change the window had not seen yet.
    pub fn set_marks(&mut self, page_index: usize, marks: Vec<LiveMark>, out: &mut Vec<LiveEvent>) {
        if page_index != self.current {
            return;
        }
        let marks = sanitize_marks(marks, self.current_page().note.page);
        if marks == self.marks {
            return;
        }
        self.marks_page = Some(self.held_page());
        self.marks = marks;
        out.push(LiveEvent::Marks {
            marks: self.marks.clone(),
        });
    }

    /// Removes every mark (an event only when there were some).
    pub fn clear_marks(&mut self, out: &mut Vec<LiveEvent>) {
        self.marks_page = None;
        if !self.marks.is_empty() {
            self.marks.clear();
            out.push(LiveEvent::Marks { marks: Vec::new() });
        }
    }

    /// The strokes in progress as already sent, to follow a snapshot: none, one or two, the
    /// tablet's first.
    pub fn in_progress(&self) -> Vec<LiveEvent> {
        let tablet = self.drawing.as_ref().and_then(|drawing| {
            let hidden = drawn_hidden(&self.pages, drawing);
            drawing.so_far(LivePen::Tablet, None, hidden)
        });
        let computer = self.computer.as_ref().and_then(|computer| {
            let hidden = drawn_hidden(&self.pages, &computer.drawing);
            computer
                .drawing
                .so_far(LivePen::Computer, Some(computer.id), hidden)
        });
        tablet.into_iter().chain(computer).collect()
    }

    /// State reported by the tablet service, or set by the hub (start, stop). A lost link ends
    /// the tablet's stroke; the computer's goes on while the session runs, and ends at the stop.
    pub fn set_state(&mut self, state: LiveSessionState, out: &mut Vec<LiveEvent>) {
        if state == self.state {
            return;
        }
        if state != LiveSessionState::Live {
            self.end_stroke(out);
            self.hover_end(out);
        }
        // Stopped: its notes are in the library, and no edit comes any more.
        if state == LiveSessionState::Idle {
            self.end_computer(out);
            for page in &mut self.pages {
                page.history.clear();
            }
        }
        self.state = state;
        self.push_snapshot(out);
    }

    /// The tablet streams; its drawing area is `width` × `height` and it is held as
    /// `orientation`. Pages not drawn on yet take the page size that implies.
    pub fn started(
        &mut self,
        width: u32,
        height: u32,
        orientation: Orientation,
        out: &mut Vec<LiveEvent>,
    ) {
        let changed = self.hold(width, height, orientation, out);
        if self.state == LiveSessionState::Live {
            if changed {
                self.push_snapshot(out);
            }
        } else {
            self.set_state(LiveSessionState::Live, out);
        }
    }

    /// The tablet is held another way while live mode runs (the quick switch of the live
    /// window). The stroke in progress ends, then the page on screen is turned with its ink
    /// (see [`hold`](Self::hold)) and every window gets a snapshot.
    pub fn reorient(
        &mut self,
        width: u32,
        height: u32,
        orientation: Orientation,
        out: &mut Vec<LiveEvent>,
    ) {
        if self.hold(width, height, orientation, out) {
            self.push_snapshot(out);
        }
    }

    /// Takes the tablet's drawing area and way of being held. What happens to the pages:
    /// - the current page is turned with its ink, by the quarter turns from the orientation it
    ///   was written in (the writing stays upright; the page swaps its width and height for a
    ///   quarter turn); the shared view follows (zoom kept, centre turned);
    /// - blank pages never written to disk just take the new size (a page whose strokes all went
    ///   is not blank while they can come back);
    /// - the other pages keep their ink and orientation, until they are shown again
    ///   ([`follow`](Self::follow)): the pen's points are always in the current orientation;
    /// - a resumed sheet stays as it is, whichever of these it is: only the way it is to be held
    ///   follows ([`Page::turn`]), and [`pen_mapping`](Self::pen_mapping) takes the pen's points
    ///   to it from the new orientation.
    ///
    /// Returns whether anything changed.
    fn hold(
        &mut self,
        width: u32,
        height: u32,
        orientation: Orientation,
        out: &mut Vec<LiveEvent>,
    ) -> bool {
        let size = oriented_size(PageSize { width, height }, orientation);
        if width == 0 || height == 0 || (size == self.size && orientation == self.orientation) {
            return false;
        }
        // Both strokes end before the page turns, so that they turn with its ink.
        self.end_stroke(out);
        self.end_computer(out);
        self.hover_end(out);
        self.size = size;
        self.orientation = orientation;
        let shown = self.current_page();
        let before = shown.note.page;
        let turns = if shown.is_resumed() {
            0
        } else {
            orientation.turns_from(shown.orientation)
        };
        for index in 0..self.pages.len() {
            let page = &mut self.pages[index];
            if page.note.strokes.is_empty() && page.file.is_none() && page.history.is_empty() {
                page.note.page = size;
                page.orientation = orientation;
            } else if index == self.current {
                page.turn(turns, orientation);
                self.leave(index);
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
            let (x, y) = turn_point(
                self.view.x as f32,
                self.view.y as f32,
                before.width as f32,
                before.height as f32,
                turns,
            );
            LiveViewState {
                x: f64::from(x),
                y: f64::from(y),
                ..self.view
            }
            .sanitized(page)
        };
        true
    }

    /// The current page is shown: it is turned to the way the tablet is held now, if it was
    /// written another way, so that the pen's points land where they should.
    fn follow(&mut self) {
        let orientation = self.orientation;
        let index = self.current;
        let page = &mut self.pages[index];
        if page.orientation != orientation {
            page.turn(orientation.turns_from(page.orientation), orientation);
            self.leave(index);
        }
    }

    /// How the pen's points land on the current page: as they come, on a page of the session; on a
    /// resumed sheet, turned to the way it is held and fitted in it.
    fn pen_mapping(&self) -> PenMapping {
        let page = self.current_page();
        if page.is_resumed() {
            PenMapping::for_sheet(
                self.size,
                page.orientation.turns_from(self.orientation),
                page.note.page,
            )
        } else {
            PenMapping::direct()
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
        let mapping = self.pen_mapping();
        let last = records.len().saturating_sub(1);
        for (i, record) in records.iter().enumerate() {
            let at = received_ms - SAMPLE_MS * (last - i) as f64;
            match record {
                LiveRecord::Pen(point) if point.pressure > 0 => {
                    self.pen_down(point, mapping, at, received_ms, out);
                }
                LiveRecord::Pen(point) => {
                    self.end_stroke(out);
                    self.hover_at(point, mapping, out);
                }
                LiveRecord::ProximityOut => {
                    self.end_stroke(out);
                    self.hover_end(out);
                }
            }
        }
        self.flush_points(out);
    }

    fn pen_down(
        &mut self,
        point: &PenPoint,
        mapping: PenMapping,
        at: f64,
        received_ms: f64,
        out: &mut Vec<LiveEvent>,
    ) {
        if self.drawing.is_none() {
            self.hover_end(out);
            self.drawing = Some(Drawing::new(
                self.current,
                self.style,
                self.current_page().tablet_layer,
                self.view.board,
            ));
        }
        let t = self.point_time(at);
        if let Some(drawing) = &mut self.drawing {
            let (x, y) = mapping.map(point.x, point.y);
            drawing.points.push(LivePoint {
                x,
                y,
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
    /// back even when notifications arrive in bursts. On a resumed sheet `created` stays and the
    /// wall clock counts from the page's shifted origin ([`Page::time_origin`]), whatever the age
    /// of the note.
    fn point_time(&mut self, at: f64) -> u32 {
        let page = &mut self.pages[self.current];
        if page.fresh {
            page.fresh = false;
            page.note.created = at.floor() as i64;
            page.note.modified = page.note.created;
        }
        let origin = page
            .resumed
            .as_ref()
            .map_or(page.note.created, |resumed| resumed.origin);
        let since = at - origin as f64;
        let t = match page.last_t {
            Some(last) => since.max(f64::from(last) + SAMPLE_MS),
            None => since.max(0.0),
        };
        // Whole milliseconds, like `created`: the first sample is at 0.
        let t = t.floor().clamp(0.0, f64::from(u32::MAX)) as u32;
        page.last_t = Some(t);
        t
    }

    fn hover_at(&mut self, point: &PenPoint, mapping: PenMapping, out: &mut Vec<LiveEvent>) {
        let (x, y) = mapping.map(point.x, point.y);
        let hover = HoverPoint { x, y };
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

    /// Sends the tablet's points drawn since the last call: `strokeStart` for a new stroke, else
    /// `points`.
    fn flush_points(&mut self, out: &mut Vec<LiveEvent>) {
        let Some(drawing) = &mut self.drawing else {
            return;
        };
        let hidden = drawn_hidden(&self.pages, drawing);
        out.extend(drawing.flush(LivePen::Tablet, None, hidden));
    }

    /// Ends the tablet's stroke in progress: it joins its page.
    pub fn end_stroke(&mut self, out: &mut Vec<LiveEvent>) {
        self.flush_points(out);
        let Some(drawing) = self.drawing.take() else {
            return;
        };
        let Some(page) = self.pages.get_mut(drawing.page) else {
            return;
        };
        page.finish(&drawing);
        out.push(LiveEvent::StrokeEnd {
            page_index: drawing.page,
            pen: LivePen::Tablet,
            id: None,
            hidden: layer_hidden(&page.note.layers, drawing.layer),
        });
    }

    /// Ends both strokes in progress (a page change): they join their page.
    fn end_strokes(&mut self, out: &mut Vec<LiveEvent>) {
        self.end_stroke(out);
        self.end_computer(out);
    }

    // --- The computer's pen (M14) ---------------------------------------------------------------

    /// A call of the computer's pen from the control window (docs/design/tablet-live-classroom.md,
    /// "The computer's strokes"), received at `wall_now` (Unix ms). `begin`: `Ok(true)`, or refused
    /// as stale (another page on screen) or on a hidden layer, nothing changed; a stroke in
    /// progress ends first (kept). `points`, `end`: whether the stroke went on (`false` once the
    /// session ended or dropped it, or for a number that is not the stroke in progress). `cancel`:
    /// whether there was a stroke to drop. A stopped session takes nothing (the hub refuses the
    /// `begin` before).
    pub fn computer_pen(
        &mut self,
        pen: CheckedPen,
        wall_now: f64,
        out: &mut Vec<LiveEvent>,
    ) -> Result<bool, LiveEditError> {
        let CheckedPen {
            stroke,
            action,
            samples,
        } = pen;
        if !self.is_running() {
            return Ok(false);
        }
        let ours = self.computer.as_ref().is_some_and(|c| c.id == stroke);
        match action {
            PenAction::Begin { page_index, style } => {
                if page_index != self.current {
                    return Err(LiveEditError::Stale);
                }
                let page = self.current_page();
                let layer = page.computer_layer;
                if layer_hidden(&page.note.layers, layer) {
                    return Err(LiveEditError::LayerHidden);
                }
                self.end_computer(out);
                self.computer = Some(ComputerDrawing {
                    drawing: Drawing::new(self.current, style, layer, self.view.board),
                    id: stroke,
                    last_wall: None,
                });
                self.computer_samples(&samples, wall_now, out);
                Ok(true)
            }
            _ if !ours => Ok(false),
            PenAction::Points => Ok(self.computer_samples(&samples, wall_now, out)),
            PenAction::End => {
                self.computer_samples(&samples, wall_now, out);
                self.end_computer(out);
                Ok(true)
            }
            PenAction::Cancel => {
                self.cancel_computer(out);
                Ok(true)
            }
        }
    }

    /// Appends `samples` to the computer's stroke and sends them at once (`strokeStart` with the
    /// first ones, then `points`). The stroke ends there (kept) once it holds
    /// [`MAX_COMPUTER_POINTS`]: then `false`, and the window stops sending for that gesture.
    fn computer_samples(
        &mut self,
        samples: &[PenSample],
        wall_now: f64,
        out: &mut Vec<LiveEvent>,
    ) -> bool {
        let Some(computer) = &mut self.computer else {
            return false;
        };
        let Some(page) = self.pages.get_mut(computer.drawing.page) else {
            return false;
        };
        let (width, height) = (page.note.page.width as f32, page.note.page.height as f32);
        let room = MAX_COMPUTER_POINTS.saturating_sub(computer.drawing.points.len());
        for sample in samples.iter().take(room) {
            let wall = sample_wall(wall_now, sample.age, computer.last_wall);
            computer.last_wall = Some(wall);
            let previous = computer.drawing.points.last().map(|point| point.t);
            let t = page.computer_time(wall, previous);
            let (x, y) = sample.within(width, height);
            computer.drawing.points.push(LivePoint {
                x,
                y,
                p: sample.p,
                t,
                recv: wall,
            });
            computer.drawing.unsent += 1;
        }
        let hidden = layer_hidden(&page.note.layers, computer.drawing.layer);
        out.extend(
            computer
                .drawing
                .flush(LivePen::Computer, Some(computer.id), hidden),
        );
        if computer.drawing.points.len() >= MAX_COMPUTER_POINTS {
            self.end_computer(out);
            return false;
        }
        true
    }

    /// Ends the computer's stroke in progress: it joins the tail of its page's list, as a step of
    /// the page's history (`strokeEnd`, then `history`).
    fn end_computer(&mut self, out: &mut Vec<LiveEvent>) {
        let Some(ComputerDrawing {
            mut drawing, id, ..
        }) = self.computer.take()
        else {
            return;
        };
        let Some(page) = self.pages.get_mut(drawing.page) else {
            return;
        };
        let hidden = layer_hidden(&page.note.layers, drawing.layer);
        out.extend(drawing.flush(LivePen::Computer, Some(id), hidden));
        let place = page.finish(&drawing);
        page.history.record(
            UndoStep::Added {
                indices: vec![place],
            },
            None,
        );
        out.push(LiveEvent::StrokeEnd {
            page_index: drawing.page,
            pen: LivePen::Computer,
            id: Some(id),
            hidden,
        });
        out.push(LiveEvent::History {
            page_index: drawing.page,
            can_undo: page.history.can_undo(),
            can_redo: page.history.can_redo(),
        });
    }

    /// Drops the computer's stroke in progress: nothing of it joins the page (`strokeCancel`).
    fn cancel_computer(&mut self, out: &mut Vec<LiveEvent>) {
        let Some(ComputerDrawing { drawing, id, .. }) = self.computer.take() else {
            return;
        };
        out.push(LiveEvent::StrokeCancel {
            page_index: drawing.page,
            pen: LivePen::Computer,
            id: Some(id),
            hidden: drawn_hidden(&self.pages, &drawing),
        });
    }

    // --- Layers (M14) ---------------------------------------------------------------------------

    /// A layer operation of the live window on page `page_index` (the one on screen, or it is
    /// stale), refused as a whole when it makes no sense there (docs/design/tablet-live-classroom.md,
    /// "Layer operations during live"). One snapshot when something changed; no stroke in progress
    /// ends.
    pub fn layers(
        &mut self,
        page_index: usize,
        op: CheckedLayerOp,
        out: &mut Vec<LiveEvent>,
    ) -> Result<(), LiveEditError> {
        if page_index != self.current {
            return Err(LiveEditError::Stale);
        }
        let french = self.french;
        let page = &mut self.pages[page_index];
        let layers = &page.note.layers;
        let changed = match op {
            CheckedLayerOp::Add => {
                if layers.len() >= MAX_LAYERS {
                    return Err(LiveEditError::TooManyLayers);
                }
                let n = layers.len() + 1;
                page.note.layers.push(Layer {
                    name: layer_name(french, n),
                    visible: true,
                });
                page.computer_layer = u32::try_from(n - 1).unwrap_or(0);
                page.layers_changed();
                true
            }
            CheckedLayerOp::Rename { layer, name } => {
                let layer = existing(layers, layer)?;
                let same = page.note.layers[layer].name == name;
                if !same {
                    page.note.layers[layer].name = name;
                    page.layers_changed();
                }
                !same
            }
            CheckedLayerOp::Visible { layer, visible } => {
                let layer = existing(layers, layer)?;
                let same = page.note.layers[layer].visible == visible;
                if !same {
                    page.note.layers[layer].visible = visible;
                    page.layers_changed();
                }
                !same
            }
            CheckedLayerOp::Move { from, to } => {
                existing(layers, from)?;
                existing(layers, to)?;
                if from == to {
                    return Err(LiveEditError::BadLayer);
                }
                let count = layers.len();
                let follow = |layer: u32| {
                    if (layer as usize) < count {
                        follow_move(layer, from, to)
                    } else {
                        layer
                    }
                };
                move_layer(&mut page.note, from, to);
                page.history.remap_layers(follow);
                page.tablet_layer = follow(page.tablet_layer);
                page.computer_layer = follow(page.computer_layer);
                page.layers_changed();
                self.follow_drawings(page_index, follow);
                true
            }
            CheckedLayerOp::Merge { layer } => {
                existing(layers, layer)?;
                if layer == 0 {
                    return Err(LiveEditError::BadLayer);
                }
                merge_layer(&mut page.note, layer);
                // Places moved: their steps would name other strokes.
                page.history.clear();
                page.revision += 1;
                page.tablet_layer = follow_merge(page.tablet_layer, layer);
                page.computer_layer = follow_merge(page.computer_layer, layer);
                page.layers_changed();
                self.follow_drawings(page_index, |index| follow_merge(index, layer));
                true
            }
            CheckedLayerOp::Computer { layer } => {
                let layer = existing(layers, layer)? as u32;
                let same = page.computer_layer == layer;
                page.computer_layer = layer;
                !same
            }
            CheckedLayerOp::Tablet { layer } => {
                let layer = existing(layers, layer)? as u32;
                let same = page.tablet_layer == layer;
                page.tablet_layer = layer;
                !same
            }
        };
        if changed {
            self.push_snapshot(out);
        }
        Ok(())
    }

    /// The layers of page `page_index` were renumbered: the strokes in progress on it follow.
    fn follow_drawings(&mut self, page_index: usize, follow: impl Fn(u32) -> u32) {
        let computer = self.computer.as_mut().map(|computer| &mut computer.drawing);
        for drawing in self.drawing.iter_mut().chain(computer) {
            if drawing.page == page_index {
                drawing.layer = follow(drawing.layer);
            }
        }
    }

    /// Leaves page `index`: its changes are written now.
    fn leave(&mut self, index: usize) {
        if let Some(page) = self.pages.get_mut(index)
            && page.needs_write()
        {
            page.urgent = true;
        }
    }

    /// A resumed sheet shown again with the blackboard off (it was turned off on another page)
    /// gets its paper back, unless chalk was written on it.
    fn settle_paper(&mut self) {
        if self.view.board {
            return;
        }
        let page = &mut self.pages[self.current];
        if page.is_resumed()
            && !page.ink_on_board()
            && let Some(paper) = page.paper_before_board
        {
            page.note.background.paper = paper;
        }
    }

    /// The page on screen changed from one of size `before`: the shared view stays on the page
    /// (a view of the whole page goes on showing the whole page).
    fn fit_view(&mut self, before: PageSize) {
        let page = self.current_page().note.page;
        if page == before {
            return;
        }
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
                self.end_strokes(out);
                let left = self.current;
                let before = self.current_page().note.page;
                if self.current + 1 < self.pages.len() {
                    self.current += 1;
                } else {
                    let page = self.new_page(now_ms);
                    self.pages.push(page);
                    self.current = self.pages.len() - 1;
                }
                self.leave(left);
                self.follow();
                self.settle_paper();
                self.fit_view(before);
                self.push_snapshot(out);
            }
            LiveCommandKind::PreviousPage => {
                if self.current == 0 {
                    return;
                }
                self.end_strokes(out);
                let left = self.current;
                let before = self.current_page().note.page;
                self.current -= 1;
                self.leave(left);
                self.follow();
                self.settle_paper();
                self.fit_view(before);
                self.push_snapshot(out);
            }
            LiveCommandKind::NewPage => {
                self.end_strokes(out);
                let left = self.current;
                let before = self.current_page().note.page;
                let page = self.new_page(now_ms);
                self.pages.push(page);
                self.current = self.pages.len() - 1;
                self.leave(left);
                self.fit_view(before);
                self.push_snapshot(out);
            }
            LiveCommandKind::NewLayer => {
                // The tablet's button (or `newLayer`): the tablet goes to a new layer on top; the
                // computer stays where it acts. Nothing beyond the most layers a page may get.
                let french = self.french;
                if self.current_page().note.layers.len() >= MAX_LAYERS {
                    return;
                }
                self.end_stroke(out);
                let page = &mut self.pages[self.current];
                let n = page.note.layers.len() + 1;
                page.note.layers.push(Layer {
                    name: layer_name(french, n),
                    visible: true,
                });
                page.tablet_layer = u32::try_from(n - 1).unwrap_or(0);
                page.touch();
                self.push_snapshot(out);
            }
            LiveCommandKind::ClearPage => {
                // The strokes in progress go too: the snapshot replaces everything. The computer's
                // window is told which of its gestures went.
                self.drawing = None;
                self.cancel_computer(out);
                let page = &mut self.pages[self.current];
                // What shows: nobody can tell that the ink of a hidden layer is going.
                let shown: Vec<u32> = page
                    .note
                    .strokes
                    .iter()
                    .enumerate()
                    .filter(|(_, stroke)| is_drawn(&page.note.layers, stroke))
                    .map(|(index, _)| index as u32)
                    .collect();
                page.touch();
                // An empty page is no step (a second press is harmless).
                if !shown.is_empty() {
                    let (strokes, _) = page.take_strokes(shown);
                    page.history.record(UndoStep::removed(strokes, true), None);
                }
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

    /// Deletes or recolours strokes of the page on screen (`docs/design/tablet-live-classroom.md`,
    /// "M12"), for a window that chose them on page `page_index` at `revision`. Refused as a whole
    /// when anything is wrong. The stroke in progress is not concerned: it is not in the list yet,
    /// and joins at the tail when it ends. Returns the page's revision: a change of colour keeps
    /// it, as does an edit that changes nothing (no step, no event).
    pub fn edit_strokes(
        &mut self,
        page_index: usize,
        revision: u64,
        edit: CheckedEdit,
        out: &mut Vec<LiveEvent>,
    ) -> Result<u64, LiveEditError> {
        if page_index != self.current {
            return Err(LiveEditError::Stale);
        }
        let page = &mut self.pages[page_index];
        if revision != page.revision {
            return Err(LiveEditError::Stale);
        }
        let strokes = &page.note.strokes;
        for &index in &edit.indices {
            let Some(stroke) = strokes.get(index as usize) else {
                return Err(LiveEditError::OutOfRange {
                    index,
                    strokes: strokes.len(),
                });
            };
            if !is_drawn(&page.note.layers, stroke) {
                return Err(LiveEditError::Hidden { index });
            }
        }
        let edited = match edit.action {
            EditAction::Delete => {
                let (strokes, edited) = page.take_strokes(edit.indices);
                page.history
                    .record(UndoStep::removed(strokes, false), edit.group);
                edited
            }
            EditAction::Recolour(after) => {
                let changes: Vec<ColourChange> = edit
                    .indices
                    .iter()
                    .filter_map(|&index| {
                        let before = strokes.get(index as usize)?.color;
                        (before != after).then_some(ColourChange {
                            index,
                            before,
                            after,
                        })
                    })
                    .collect();
                if changes.is_empty() {
                    return Ok(page.revision);
                }
                let colours: Vec<(u32, Rgba)> = changes.iter().map(|c| (c.index, after)).collect();
                let edited = page.colour_strokes(&colours);
                page.history.record(UndoStep::Recoloured(changes), None);
                edited
            }
        };
        out.push(page.event(page_index, edited));
        Ok(page.revision)
    }

    /// The strokes the next undo on the page on screen gives back (every step of its group), to
    /// copy before the lock is taken ([`Prepared`]).
    pub fn next_restored(&self) -> Vec<Arc<Vec<PlacedStroke>>> {
        self.current_page().history.next_restored()
    }

    /// The computer's strokes the next redo on the page on screen gives back, likewise.
    pub fn next_redone(&self) -> Vec<Arc<Vec<PlacedStroke>>> {
        self.current_page().history.next_redone()
    }

    /// Undoes the last step of page `page_index` (the one on screen, or the call is stale), with
    /// every step of its group: a stroke comes back at its place, whatever the pens wrote since,
    /// which stays after it; a computer stroke goes. Returns whether there was anything to undo.
    /// `prepared`: see [`next_restored`](Self::next_restored).
    pub fn undo_edit(
        &mut self,
        page_index: usize,
        prepared: Option<Prepared>,
        out: &mut Vec<LiveEvent>,
    ) -> Result<bool, LiveEditError> {
        if page_index != self.current {
            return Err(LiveEditError::Stale);
        }
        Ok(self.undo_step(prepared, out))
    }

    /// Undoes the group on top of the current page's history, top down: one `strokesEdited` per
    /// step.
    fn undo_step(&mut self, prepared: Option<Prepared>, out: &mut Vec<LiveEvent>) -> bool {
        let index = self.current;
        let page = &mut self.pages[index];
        let mut prepared = prepared;
        let Some(mut next) = page.history.pop_undo() else {
            return false;
        };
        loop {
            let (step, group) = next;
            let edited = match step {
                UndoStep::Removed {
                    strokes, cleared, ..
                } => {
                    let copy = Prepared::copy_of(&mut prepared, &strokes);
                    let indices = strokes.iter().map(|placed| placed.index).collect();
                    page.history
                        .push_redo(RedoStep::Remove { indices, cleared }, group);
                    page.give_back(strokes, copy)
                }
                UndoStep::Recoloured(changes) => {
                    let colours: Vec<(u32, Rgba)> =
                        changes.iter().map(|c| (c.index, c.before)).collect();
                    page.history.push_redo(RedoStep::Recolour(changes), group);
                    page.colour_strokes(&colours)
                }
                UndoStep::Added { indices } => {
                    let (strokes, edited) = page.take_strokes(indices);
                    let strokes = Arc::new(strokes);
                    page.history.push_redo(RedoStep::Add { strokes }, group);
                    edited
                }
            };
            out.push(page.event(index, edited));
            if !page.history.undo_joins(group) {
                return true;
            }
            let Some(step) = page.history.pop_undo() else {
                return true;
            };
            next = step;
        }
    }

    /// Does again the last group undone on page `page_index` (the one on screen, or the call is
    /// stale), in the order it was done. Returns whether there was anything to redo. `prepared`:
    /// a copy of the computer's strokes it gives back ([`next_redone`](Self::next_redone)).
    pub fn redo_edit(
        &mut self,
        page_index: usize,
        prepared: Option<Prepared>,
        out: &mut Vec<LiveEvent>,
    ) -> Result<bool, LiveEditError> {
        if page_index != self.current {
            return Err(LiveEditError::Stale);
        }
        let page = &mut self.pages[page_index];
        let mut prepared = prepared;
        let Some(mut next) = page.history.pop_redo() else {
            return Ok(false);
        };
        loop {
            let (step, group) = next;
            let edited = match step {
                RedoStep::Remove { indices, cleared } => {
                    let (strokes, edited) = page.take_strokes(indices);
                    page.history
                        .push_undo(UndoStep::removed(strokes, cleared), group);
                    edited
                }
                RedoStep::Recolour(changes) => {
                    let colours: Vec<(u32, Rgba)> =
                        changes.iter().map(|c| (c.index, c.after)).collect();
                    let edited = page.colour_strokes(&colours);
                    page.history.push_undo(UndoStep::Recoloured(changes), group);
                    edited
                }
                RedoStep::Add { strokes } => {
                    let copy = Prepared::copy_of(&mut prepared, &strokes);
                    let indices = strokes.iter().map(|placed| placed.index).collect();
                    let edited = page.give_back(strokes, copy);
                    page.history.push_undo(UndoStep::Added { indices }, group);
                    edited
                }
            };
            out.push(page.event(page_index, edited));
            if !page.history.redo_joins(group) {
                return Ok(true);
            }
            let Some(step) = page.history.pop_redo() else {
                return Ok(true);
            };
            next = step;
        }
    }

    /// « Undo the clearing »: gives back the ink of the last « Clear the page » of the page on
    /// screen, under what was written since. It takes its place in the page's history: edits made
    /// after the clearing are undone first (a redo brings them back). Returns whether anything was
    /// restored: not when that page has no clearing to undo.
    pub fn restore_cleared_with(
        &mut self,
        prepared: Option<Prepared>,
        out: &mut Vec<LiveEvent>,
    ) -> bool {
        if !self.current_page().history.holds_clearing() {
            return false;
        }
        let mut prepared = prepared;
        loop {
            let last = self.current_page().history.next_is_clearing();
            if !self.undo_step(prepared.take(), out) || last {
                return true;
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
    /// paper and switches to white chalk; turning it off gives back the paper the page had
    /// (white for a page made while the blackboard was on) and the style used before. Other pages
    /// keep their paper, so that ink written on one stays visible. On a resumed sheet that slate
    /// reaches the file only with ink written under it (`hub::written_sheet`): pressing B in front
    /// of a sheet is not a change of the sheet.
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
        self.board_imposed = false;
        let page = &mut self.pages[self.current];
        let paper = if view.board {
            self.style_before_board = Some(self.style);
            self.style = LiveStyle::chalk();
            if page.note.background.paper != Paper::Slate {
                page.paper_before_board = Some(page.note.background.paper);
            }
            Paper::Slate
        } else {
            self.style = self.style_before_board.take().unwrap_or_default();
            page.paper_before_board.unwrap_or(Paper::White)
        };
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
        session.started(29_700, 21_600, Orientation::Landscape, &mut out);
        assert_eq!(session.state(), LiveSessionState::Live);
        session
    }

    impl LiveSession {
        /// `command` with the button on its default and a fixed clock.
        fn command_for_test(&mut self, kind: LiveCommandKind, out: &mut Vec<LiveEvent>) {
            self.command(kind, LiveButton::NewPage, T0 as i64, out);
        }

        /// `restore_cleared_with`, the copy of the strokes made on the spot.
        fn restore_cleared(&mut self, out: &mut Vec<LiveEvent>) -> bool {
            self.restore_cleared_with(None, out)
        }
    }

    fn is_snapshot(event: &LiveEvent) -> bool {
        matches!(event, LiveEvent::Snapshot { .. })
    }

    /// The end of a tablet's stroke on a visible layer of page `page_index`.
    fn tablet_end(page_index: usize) -> LiveEvent {
        LiveEvent::StrokeEnd {
            page_index,
            pen: LivePen::Tablet,
            id: None,
            hidden: false,
        }
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
        let LiveEvent::StrokeStart {
            page_index, stroke, ..
        } = sent[2]
        else {
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
        assert_eq!(*sent[1], tablet_end(0));
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
        assert_eq!(*sent[2], tablet_end(0));
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
        assert_eq!(*events(&out)[0], tablet_end(0));
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
        assert!(session.in_progress().is_empty());
        assert!(session.pages()[0].version > before);
        // Still pressing: a new stroke starts.
        out.clear();
        session.records(&[pen(4, 4, 50)], T0 + 105.0, &mut out);
        assert!(matches!(events(&out)[0], LiveEvent::StrokeStart { .. }));
    }

    #[test]
    fn undoing_the_clearing_gives_the_ink_back_under_what_was_written_since() {
        let mut session = running();
        let mut out = Vec::new();
        session.records(&[pen(1, 1, 50), pen(2, 2, 0)], T0, &mut out);
        let kept = session.pages()[0].note.strokes.clone();
        assert_eq!(kept.len(), 1);
        session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
        assert!(session.pages()[0].note.strokes.is_empty());
        // A second clearing of the empty page keeps the ink of the first one.
        session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
        // Something is written, then the clearing is undone: the old ink goes first.
        session.records(&[pen(5, 5, 50), pen(6, 6, 0)], T0 + 200.0, &mut out);
        let version = session.pages()[0].version;
        out.clear();
        assert!(session.restore_cleared(&mut out));
        // Through the page's history: the strokes that come back, not the whole page again.
        assert!(
            matches!(&out[..], [LiveEvent::StrokesEdited { inserted, .. }] if inserted.len() == 1)
        );
        let strokes = &session.pages()[0].note.strokes;
        assert_eq!(strokes.len(), 2);
        assert_eq!(strokes[0], kept[0]);
        assert!(session.pages()[0].version > version);
        // Only once.
        assert!(!session.restore_cleared(&mut out));
    }

    #[test]
    fn the_clearing_cannot_be_undone_from_another_page() {
        let mut session = running();
        let mut out = Vec::new();
        session.records(&[pen(1, 1, 50), pen(2, 2, 0)], T0, &mut out);
        session.command(LiveCommandKind::ClearPage, LiveButton::NewPage, 0, &mut out);
        session.command(
            LiveCommandKind::NextPage,
            LiveButton::NewPage,
            T0 as i64,
            &mut out,
        );
        out.clear();
        assert!(!session.restore_cleared(&mut out));
        assert!(out.is_empty());
        assert!(session.pages().iter().all(|p| p.note.strokes.is_empty()));
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
        let [LiveEvent::StrokeStart { stroke, .. }] = &session.in_progress()[..] else {
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

    /// A running session held as `orientation` (the tablet is A4: 29 700 x 21 600).
    fn running_as(orientation: Orientation) -> LiveSession {
        let mut session = running();
        let mut out = Vec::new();
        session.started(29_700, 21_600, orientation, &mut out);
        session
    }

    fn first_point(session: &LiveSession, page: usize) -> (f32, f32) {
        let p = session.pages()[page].note.strokes[0].points[0];
        (p.x, p.y)
    }

    #[test]
    fn a_portrait_start_makes_portrait_pages() {
        let mut session = running_as(Orientation::PortraitLeft);
        assert_eq!(session.orientation(), Orientation::PortraitLeft);
        assert_eq!(
            session.size(),
            PageSize {
                width: 21_600,
                height: 29_700
            }
        );
        assert_eq!(session.pages()[0].note.page, session.size());
        assert_eq!(session.snapshot().orientation, Orientation::PortraitLeft);
        // The view shows the whole new page.
        assert_eq!((session.view().x, session.view().y), (10_800.0, 14_850.0));
        // New pages are portrait too.
        let mut out = Vec::new();
        session.command_for_test(LiveCommandKind::NewPage, &mut out);
        assert_eq!(session.pages()[1].note.page, session.size());
    }

    #[test]
    fn switching_turns_the_current_page_with_its_ink_and_ends_the_stroke() {
        let mut session = running();
        let mut out = Vec::new();
        // A finished stroke at (1000, 2000), then a stroke still in progress at (5000, 6000).
        session.records(
            &[
                pen(1000, 2000, 400),
                pen(1100, 2000, 400),
                pen(1100, 2000, 0),
            ],
            T0,
            &mut out,
        );
        session.records(
            &[pen(5000, 6000, 400), pen(5100, 6000, 400)],
            T0 + 100.0,
            &mut out,
        );
        out.clear();

        session.reorient(29_700, 21_600, Orientation::PortraitLeft, &mut out);
        // The stroke in progress ended (and is kept), one snapshot follows.
        assert!(out.iter().any(|e| matches!(e, LiveEvent::StrokeEnd { .. })));
        assert_eq!(snapshots(&out), 1);
        assert_eq!(session.orientation(), Orientation::PortraitLeft);
        let page = &session.pages()[0];
        assert_eq!(
            page.note.page,
            PageSize {
                width: 21_600,
                height: 29_700
            }
        );
        assert_eq!(page.note.strokes.len(), 2);
        // A quarter turn clockwise: (x, y) -> (21 600 - y, x).
        assert_eq!(first_point(&session, 0), (19_600.0, 1000.0));
        let second = page.note.strokes[1].points[0];
        assert_eq!((second.x, second.y), (15_600.0, 5000.0));
        // Times are untouched, and the page is to be written again.
        let stroke = &page.note.strokes[0];
        assert_eq!(stroke.points[1].t - stroke.points[0].t, 5);
        assert!(page.needs_write());

        // Another switch, to flipped landscape: net effect half a turn from the start,
        // (x, y) -> (29 700 - x, 21 600 - y).
        out.clear();
        session.reorient(29_700, 21_600, Orientation::LandscapeFlipped, &mut out);
        assert_eq!(session.pages()[0].note.page, A4_LANDSCAPE);
        assert_eq!(first_point(&session, 0), (28_700.0, 19_600.0));
        // Nothing changes when the position is the same.
        out.clear();
        session.reorient(29_700, 21_600, Orientation::LandscapeFlipped, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn a_switch_while_waiting_for_the_tablet_shows_at_once_and_is_not_turned_twice() {
        // Never started: the snapshot takes the new way, pages are blank and just take the size.
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
        out.clear();
        session.reorient(29_700, 21_600, Orientation::PortraitLeft, &mut out);
        assert_eq!(snapshots(&out), 1);
        assert_eq!(session.state(), LiveSessionState::Waiting);
        assert_eq!(session.orientation(), Orientation::PortraitLeft);
        assert_eq!(session.pages()[0].note.page, session.size());
        // The tablet then streams, held the same way: nothing more to turn.
        out.clear();
        session.started(29_700, 21_600, Orientation::PortraitLeft, &mut out);
        assert_eq!(session.state(), LiveSessionState::Live);
        assert_eq!(session.orientation(), Orientation::PortraitLeft);

        // The link drops after some ink; the switch during the wait turns the page once.
        session.records(&[pen(1000, 2000, 400), pen(1100, 2000, 0)], T0, &mut out);
        session.set_state(LiveSessionState::Reconnecting, &mut out);
        out.clear();
        session.reorient(29_700, 21_600, Orientation::LandscapeFlipped, &mut out);
        assert_eq!(snapshots(&out), 1);
        assert_eq!(session.state(), LiveSessionState::Reconnecting);
        assert_eq!(session.pages()[0].note.page, A4_LANDSCAPE);
        let turned = first_point(&session, 0);
        out.clear();
        session.started(29_700, 21_600, Orientation::LandscapeFlipped, &mut out);
        assert_eq!(session.state(), LiveSessionState::Live);
        assert_eq!(first_point(&session, 0), turned);
        assert_eq!(session.pages()[0].note.page, A4_LANDSCAPE);
    }

    #[test]
    fn switching_keeps_the_zoom_and_turns_its_centre() {
        let mut session = running();
        let mut out = Vec::new();
        let zoomed = LiveViewState {
            zoom: 2.0,
            x: 5000.0,
            y: 4000.0,
            board: false,
            laser: false,
        };
        session.set_view(zoomed, &mut out);
        session.records(&[pen(1000, 2000, 400), pen(1100, 2000, 0)], T0, &mut out);
        session.reorient(29_700, 21_600, Orientation::PortraitLeft, &mut out);
        let view = session.view();
        assert_eq!(view.zoom, 2.0);
        // (5000, 4000) on 29 700 x 21 600 turned a quarter clockwise: (21 600 - 4000, 5000).
        assert_eq!((view.x, view.y), (17_600.0, 5000.0));
    }

    #[test]
    fn other_pages_keep_their_ink_until_shown_again_and_blank_ones_follow() {
        let mut session = running();
        let mut out = Vec::new();
        session.records(&[pen(1000, 2000, 400), pen(1100, 2000, 0)], T0, &mut out);
        session.command_for_test(LiveCommandKind::NewPage, &mut out);
        assert_eq!(session.current, 1);
        session.reorient(29_700, 21_600, Orientation::PortraitRight, &mut out);
        // Page 2 was blank: it follows at once. Page 1 is untouched, still landscape.
        assert_eq!(session.pages()[1].note.page, session.size());
        assert_eq!(session.pages()[0].note.page, A4_LANDSCAPE);
        assert_eq!(first_point(&session, 0), (1000.0, 2000.0));
        // Shown again, page 1 turns three quarters: (x, y) -> (y, 29 700 - x).
        session.command_for_test(LiveCommandKind::PreviousPage, &mut out);
        assert_eq!(session.pages()[0].note.page, session.size());
        assert_eq!(first_point(&session, 0), (2000.0, 28_700.0));
        // New pages from now on are portrait.
        session.command_for_test(LiveCommandKind::NewPage, &mut out);
        let newest = session.pages().last().unwrap();
        assert_eq!(newest.note.page, session.size());
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
        session.started(21_600, 14_800, Orientation::Landscape, &mut out);
        assert_eq!(session.pages()[0].note.page.width, 21_600);
        assert_eq!(session.size().height, 14_800);
        assert_eq!((session.view().x, session.view().y), (10_800.0, 7_400.0));
        session.records(&[pen(1, 1, 50)], T0, &mut out);
        out.clear();
        session.set_state(LiveSessionState::Reconnecting, &mut out);
        assert_eq!(*events(&out)[0], tablet_end(0));
        assert_eq!(snapshots(&out), 1);
        // Records after the link came back: live again.
        out.clear();
        session.records(&[pen(1, 1, 0)], T0 + 900.0, &mut out);
        assert_eq!(session.state(), LiveSessionState::Live);
        assert_eq!(snapshots(&out), 1);
    }
}
