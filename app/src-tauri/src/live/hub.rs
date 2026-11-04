//! The live hub: one [`LiveSession`] shared by the tablet service's thread (live input), the
//! commands of every window, and two threads of its own:
//!
//! - the **flusher** sends the pending events to every subscribed window as soon as there are
//!   some, but not more often than every 4 ms (SPEC §6.2: grouped sending through a `Channel`);
//!   each window has its own queue, so a window that subscribes in the middle of a stroke gets
//!   exactly what follows its snapshot;
//! - the **saver** writes the pages to the library: a page becomes a note when its first stroke
//!   ends, then is rewritten atomically at most every 5 s while it changes, and at once when it
//!   is left or when live mode stops. A page's narration (recorded in live mode, cut per page
//!   by `narration.rs`) is attached to it here and written with it, as its `audio.opus`. A
//!   resumed sheet ([`LiveHub::resume`], `resume.rs`) is written through the file it came from.
//!
//! Neither thread does its slow work (sending, writing files) while holding the session lock,
//! so the tablet's input is never held up by a window or by the disk.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use ardoise_core::format::{FormatError, NoteFile};
use ardoise_core::fsutil::write_atomic_new;
use ardoise_core::library::{Library, LibraryError};
use ardoise_core::note::{BackgroundImage, Note, PageSize};
use ardoise_core::settings::{LiveButton, Orientation};
use ardoise_device::pages::oriented_size;
use ardoise_device::service::{LiveInput, LiveSink, LiveState};
use ardoise_media::replay::ReplayOptions;
use ardoise_media::{MediaError, Narration, decode_narration, encode_narration};
use ardoise_render::NoteView;
use tauri::ipc::Channel;

use super::activity::Activity;
use super::edits::{LiveEditError, Prepared, StrokesEdit};
use super::model::{
    ForkReason, LiveBatch, LiveCommandKind, LiveEvent, LiveMark, LiveSessionState, LiveSnapshot,
    LiveStyle, LiveViewState, wall_ms, wall_now_ms,
};
use super::resume::{ResumeBase, ResumeSeed, merged_keeping_both, placed_for_merge};
use super::session::{LiveSession, Page, SavedFile, SheetFork};
use crate::library::{now_ms, path_text};
use crate::original::keep_original;
use crate::presentation::MonitorInfo;
use crate::thumbnails::{thumbnail_png, thumbnail_view};

/// Timing of the hub (shortened by the tests).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HubConfig {
    /// Least time between two batches (SPEC §6.2; 4 ms leaves room for a 60 or 120 Hz screen).
    pub flush_interval: Duration,
    /// Least time between two writes of a page that keeps changing.
    pub save_interval: Duration,
}

impl Default for HubConfig {
    fn default() -> Self {
        Self {
            flush_interval: Duration::from_millis(4),
            save_interval: Duration::from_secs(5),
        }
    }
}

/// Where batches go: a window's channel (or a test double). An error drops the subscriber.
pub trait BatchSink: Send + Sync {
    fn send(&self, batch: LiveBatch) -> Result<(), String>;
}

impl BatchSink for Channel<LiveBatch> {
    fn send(&self, batch: LiveBatch) -> Result<(), String> {
        Channel::send(self, batch).map_err(|error| error.to_string())
    }
}

/// Reads what the tablet's button does (`[tablet] button`), when it is pressed.
pub type ButtonSetting = Arc<dyn Fn() -> LiveButton + Send + Sync>;

/// Failure of a live command.
#[derive(Debug, thiserror::Error)]
pub enum LiveError {
    #[error("live mode was never started")]
    NotRunning,
    /// The session was stopped: its notes are in the library, they are not changed any more.
    #[error("live mode is stopped: its pages are not changed any more")]
    Stopped,
    #[error(transparent)]
    Library(#[from] LibraryError),
    /// A narration could not be merged with the page's (decoded again) or encoded for it
    /// (nothing was changed).
    #[error("cannot store the narration of the page: {0}")]
    Narration(MediaError),
    /// An edit of strokes was refused (nothing was changed).
    #[error(transparent)]
    Edit(#[from] LiveEditError),
}

/// What a new session needs.
pub struct StartOptions {
    /// Where its pages go (the library root).
    pub library: Library,
    /// Layer names in French.
    pub french: bool,
    /// Drawing area of the tablet, if known (else the Slate A4's).
    pub size: PageSize,
    /// How the tablet is held (the page size and the points follow).
    pub orientation: Orientation,
    pub button: ButtonSetting,
}

/// Result of [`LiveHub::start`].
pub enum Start {
    /// A session is already running: its snapshot.
    Running(LiveSnapshot),
    /// A new session waits for the tablet: hand `sink` to the tablet service.
    New {
        snapshot: LiveSnapshot,
        sink: HubSink,
    },
}

/// One window receiving batches.
struct Subscriber {
    id: u64,
    /// Label of its window: a window has one subscription, the latest.
    label: String,
    sink: Arc<dyn BatchSink>,
    /// Events not sent yet.
    queue: Vec<LiveEvent>,
}

/// A page to write, taken from the session.
struct SaveJob {
    session: u64,
    page: usize,
    version: u64,
    note: Note,
    /// The page's narration (`audio.opus`), encoded once when it was attached.
    audio: Option<Arc<Vec<u8>>>,
    file: Option<SavedFile>,
    /// A resumed sheet: the file it came from.
    base: Option<Arc<ResumeBase>>,
}

/// Where live mode stands, for the narration's page watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PagePosition {
    /// Id of the session shown (the last one started).
    pub session: u64,
    /// Its current page, 0-based.
    pub page: usize,
    /// Started and not stopped.
    pub running: bool,
}

/// Where the session shown stands and the notes it wrote, for the assistant's bridge
/// (`app_bridge.rs`).
#[derive(Debug, Clone, PartialEq)]
pub struct LiveOverview {
    pub state: LiveSessionState,
    /// Started and not stopped.
    pub running: bool,
    /// The current page, 0-based, and the number of pages.
    pub page: usize,
    pub pages: usize,
    /// Name of the screen of the presentation window, when it is open.
    pub presentation: Option<String>,
    /// The library the session writes to.
    pub library: Option<PathBuf>,
    /// The pages written to the library: index (0-based), note id, path.
    pub notes: Vec<(usize, String, PathBuf)>,
}

/// What a write produced.
struct Written {
    file: SavedFile,
    /// The note's id as written: a new one when the note had to be written again next to a
    /// version changed elsewhere.
    id: String,
    /// Title, tags and favourite mark found on disk when the note was changed from outside
    /// (retitled, tagged) and the page was written into it all the same: the page takes them,
    /// so that its next writes keep them.
    outside: Option<Outside>,
    /// The file was written. Not when a resumed sheet came back to what its file holds.
    rewrote: bool,
    /// The resumed sheet had to become a new note: why, and the note left alone.
    fork: Option<(ForkReason, PathBuf)>,
}

/// What the library may change in a note live mode is writing.
struct Outside {
    title: String,
    tags: Vec<String>,
    favorite: bool,
}

struct HubState {
    session: LiveSession,
    /// Id of the session whose tablet input is accepted.
    accepting: Option<u64>,
    /// A session was started at least once since the app started.
    started: bool,
    next_session: u64,
    library: Option<Library>,
    button: ButtonSetting,
    subscribers: Vec<Subscriber>,
    next_subscriber: u64,
    last_flush: Option<Instant>,
    shutdown: bool,
}

struct Shared {
    config: HubConfig,
    state: Mutex<HubState>,
    /// Whether the session runs, readable without the lock (set by [`Shared::publish`]).
    activity: Activity,
    /// Wakes the flusher: events are pending, or shutdown.
    flush_wake: Condvar,
    /// Wakes the saver: a page changed, or shutdown.
    save_wake: Condvar,
    /// Held during a save pass, so that two passes never write the same page twice.
    saving: Mutex<()>,
    /// Held while a narration is merged and encoded for a page (outside the state lock), so
    /// that two narrations given to the same page never lose one another.
    narrating: Mutex<()>,
}

/// The hub of the running app (see the module documentation). Dropping it stops its threads.
pub struct LiveHub {
    shared: Arc<Shared>,
}

/// The live input of one session, for the tablet service. Input of a session that was stopped
/// or replaced is ignored.
pub struct HubSink {
    shared: Arc<Shared>,
    session: u64,
}

impl LiveSink for HubSink {
    fn input(&mut self, input: LiveInput) {
        self.shared.input(self.session, input);
    }
}

impl HubSink {
    /// Id of the session it feeds.
    pub fn session(&self) -> u64 {
        self.session
    }
}

fn default_size() -> PageSize {
    ardoise_core::note::A4_LANDSCAPE
}

/// The size of a new session's new pages: the tablet's area, as the tablet is held.
fn page_size_of(options: &StartOptions) -> PageSize {
    let size = if options.size.width > 0 && options.size.height > 0 {
        options.size
    } else {
        default_size()
    };
    oriented_size(size, options.orientation)
}

/// Adds `event` to a window's queue, merging what does not need to be sent twice: points join
/// the stroke they continue, only the latest hover position, view and list of marks are kept.
fn enqueue(queue: &mut Vec<LiveEvent>, event: LiveEvent) {
    match event {
        LiveEvent::Points { page_index, points } => match queue.last_mut() {
            Some(LiveEvent::Points {
                page_index: pending_page,
                points: pending,
            }) if *pending_page == page_index => pending.extend(points),
            Some(LiveEvent::StrokeStart {
                page_index: pending_page,
                stroke,
            }) if *pending_page == page_index => stroke.points.extend(points),
            _ => queue.push(LiveEvent::Points { page_index, points }),
        },
        LiveEvent::Hover { .. } | LiveEvent::HoverEnd => {
            queue.retain(|e| !matches!(e, LiveEvent::Hover { .. }));
            queue.push(event);
        }
        LiveEvent::View { .. } => {
            queue.retain(|e| !matches!(e, LiveEvent::View { .. }));
            queue.push(event);
        }
        LiveEvent::Marks { .. } => {
            queue.retain(|e| !matches!(e, LiveEvent::Marks { .. }));
            queue.push(event);
        }
        LiveEvent::Saved { .. } => {
            queue.retain(|e| !matches!(e, LiveEvent::Saved { .. }));
            queue.push(event);
        }
        event => queue.push(event),
    }
}

/// When `page` is due to be written: now when it is urgent or never tried, else one interval
/// after the last attempt; `None` when it has nothing to write.
fn page_due(page: &Page, now: Instant, interval: Duration) -> Option<Instant> {
    if !page.needs_write() {
        return None;
    }
    Some(match page.last_attempt {
        Some(last) if !page.urgent => last + interval,
        _ => now,
    })
}

/// Whether `events` may have made a page worth writing (a stroke ended, strokes were edited, a
/// page changed).
fn may_need_save(events: &[LiveEvent]) -> bool {
    events.iter().any(|e| {
        matches!(
            e,
            LiveEvent::StrokeEnd { .. }
                | LiveEvent::StrokesEdited { .. }
                | LiveEvent::Snapshot { .. }
        )
    })
}

/// Fingerprint of what live mode writes into a note: the ink, the layers, the paper, the page
/// (everything but the title, tags, favourite mark and modification time, which the library or
/// an assistant change while the lesson goes on).
fn content_fingerprint(note: &Note) -> Option<u64> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    // The ink as `strokes.bin` stores it, so that a note read back hashes as the page did.
    let strokes = ardoise_core::format::strokes::encode(&note.strokes).ok()?;
    let layout = serde_json::to_vec(&(
        note.created,
        &note.page,
        &note.background,
        &note.background_image,
        &note.layers,
        &note.timing,
        &note.origin,
    ))
    .ok()?;
    let mut hasher = DefaultHasher::new();
    strokes.hash(&mut hasher);
    layout.hash(&mut hasher);
    Some(hasher.finish())
}

/// Fingerprint of a note file as live mode leaves it: the note's [`content_fingerprint`], its
/// narration and everything it keeps as read (not its thumbnail, made again by whoever writes it,
/// nor what [`content_fingerprint`] leaves out). What differs from the fingerprint last written is
/// a change made elsewhere that a write must not overwrite.
pub(super) fn file_fingerprint(file: &NoteFile) -> Option<u64> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    content_fingerprint(&file.note)?.hash(&mut hasher);
    file.audio.hash(&mut hasher);
    file.original.hash(&mut hasher);
    file.original_note.hash(&mut hasher);
    file.text.hash(&mut hasher);
    if let Some(background) = &file.background {
        background.extension.hash(&mut hasher);
        background.bytes.hash(&mut hasher);
    }
    // Whatever order the JSON library keeps them in.
    let mut fields: Vec<_> = file.unknown_fields.iter().collect();
    fields.sort_by_key(|(name, _)| name.as_str());
    for (name, value) in fields {
        name.hash(&mut hasher);
        serde_json::to_vec(value).ok()?.hash(&mut hasher);
    }
    for member in &file.unknown_members {
        member.name.hash(&mut hasher);
        member.bytes.hash(&mut hasher);
    }
    Some(hasher.finish())
}

/// What is written for a page, its thumbnail apart. A page of the session holds only its note and
/// its narration. A resumed sheet is the file it came from with the note and the narration
/// replaced, so that nothing else of it is lost; the version before the resume is kept for "back
/// to the original" as soon as the ink differs from it (`original::keep_original`: never over an
/// original the file already has). Recomputed from that file at each write: the first write may
/// change only the narration, and a fork needs it as much as the note's own file does.
fn page_file(
    note: &Note,
    audio: Option<&Arc<Vec<u8>>>,
    base: Option<&ResumeBase>,
) -> Result<NoteFile, FormatError> {
    let mut file = match base {
        Some(base) => {
            let mut file = base.file.clone();
            keep_original(&mut file, note)?;
            file.note = note.clone();
            file.note.modified = base.file.note.modified;
            file
        }
        None => NoteFile::new(note.clone()),
    };
    file.note.modified = now_ms().max(file.note.modified).max(file.note.created);
    file.audio = audio.map(|audio| audio.as_ref().clone());
    Ok(file)
}

/// The thumbnail of a page: with its background image on a resumed sheet (a page of the session has
/// none); the one the file had when it cannot be drawn.
fn page_thumbnail(file: &mut NoteFile, base: Option<&ResumeBase>) {
    let drawn = match base {
        Some(base) => thumbnail_view(NoteView::new(&file.note, base.picture())),
        None => thumbnail_png(&file.note),
    };
    if drawn.is_some() {
        file.thumbnail = drawn;
    }
}

/// Writes a page: a new note in the library root the first time, then over its file. A file
/// moved meanwhile (same note, untouched) is followed; one whose title or tags were changed
/// elsewhere keeps them and receives the page ([`merge_outside`]); one whose content was
/// changed elsewhere, or that was deleted, is left alone and the page becomes a new note next
/// to it, so that nothing is ever overwritten: in the library root for a page of the session, in
/// the note's binder for a resumed sheet, which says so ([`Written::fork`]). A resumed sheet that
/// holds what its file holds (the blackboard turned on and off) is not written at all.
fn write_page(library: &Library, job: &SaveJob) -> Result<Written, LibraryError> {
    let base = job.base.as_deref();
    let mut file =
        page_file(&job.note, job.audio.as_ref(), base).map_err(|source| LibraryError::Format {
            path: job
                .file
                .as_ref()
                .map_or_else(|| library.root().to_path_buf(), |file| file.path.clone()),
            source,
        })?;
    let content = file_fingerprint(&file);
    if let (Some(_), Some(previous)) = (base, &job.file)
        && content.is_some()
        && previous.content == content
    {
        return Ok(Written {
            file: previous.clone(),
            id: file.note.id,
            outside: None,
            rewrote: false,
            fork: None,
        });
    }
    page_thumbnail(&mut file, base);
    let Some(previous) = &job.file else {
        let saved = library.create(Path::new(""), &file)?;
        return Ok(Written {
            file: SavedFile {
                path: saved.path,
                stamp: saved.stamp,
                content,
            },
            id: file.note.id,
            outside: None,
            rewrote: true,
            fork: None,
        });
    };
    // The page can no longer go in its note.
    let fork = |file: NoteFile, from: &Path, reason: ForkReason| {
        if base.is_some() {
            write_beside(library, file, content, from, reason)
        } else {
            write_as_new(library, file, content)
        }
    };
    match library.save(&previous.path, &file, &previous.stamp) {
        Ok(stamp) => Ok(Written {
            file: SavedFile {
                path: previous.path.clone(),
                stamp,
                content,
            },
            id: file.note.id,
            outside: None,
            rewrote: true,
            fork: None,
        }),
        Err(LibraryError::NotFound { .. }) => {
            let moved = library
                .list()
                .notes
                .into_iter()
                .find(|entry| entry.id == file.note.id);
            match moved {
                Some(entry) if entry.stamp == previous.stamp => {
                    let stamp = library.save(&entry.path, &file, &entry.stamp)?;
                    Ok(Written {
                        file: SavedFile {
                            path: entry.path,
                            stamp,
                            content,
                        },
                        id: file.note.id,
                        outside: None,
                        rewrote: true,
                        fork: None,
                    })
                }
                // Retitled elsewhere (the file was renamed and rewritten).
                Some(entry) => {
                    match merge_outside(library, &file, previous, content, &entry.path) {
                        Some(written) => Ok(written),
                        None => fork(file, &entry.path, ForkReason::Changed),
                    }
                }
                None => fork(file, &previous.path, ForkReason::Deleted),
            }
        }
        Err(LibraryError::Conflict { .. }) => {
            match merge_outside(library, &file, previous, content, &previous.path) {
                Some(written) => Ok(written),
                None => fork(file, &previous.path, ForkReason::Changed),
            }
        }
        Err(error) => Err(error),
    }
}

/// The note at `path` was rewritten since this page last wrote it. When only what the library
/// changes differs (title, tags, favourite mark: the retitle or tags of the library grid, the
/// assistant's `update_note`), the page is written into it with those kept; the page then goes
/// on in the same note instead of leaving a duplicate per page. `None` when the note holds
/// other ink, layout, narration or members than the page last wrote, is another note, or cannot
/// be written: the caller then writes a new note.
fn merge_outside(
    library: &Library,
    file: &NoteFile,
    previous: &SavedFile,
    content: Option<u64>,
    path: &Path,
) -> Option<Written> {
    let known = previous.content?;
    let loaded = library.read(path).ok()?;
    let mut merged = loaded.file;
    if merged.note.id != file.note.id || file_fingerprint(&merged)? != known {
        return None;
    }
    let outside = Outside {
        title: merged.note.title.clone(),
        tags: merged.note.tags.clone(),
        favorite: merged.note.favorite,
    };
    let note = &mut merged.note;
    note.created = file.note.created;
    note.modified = file.note.modified.max(note.modified);
    note.page = file.note.page;
    note.background = file.note.background;
    note.background_image = file.note.background_image.clone();
    note.layers = file.note.layers.clone();
    note.strokes = file.note.strokes.clone();
    note.timing = file.note.timing;
    note.origin = file.note.origin.clone();
    merged.thumbnail = file.thumbnail.clone();
    merged.audio = file.audio.clone();
    // The version before a resume, kept by this very write when it is the first to change the ink.
    merged.original_note = file.original_note.clone();
    let stamp = library.save(&loaded.path, &merged, &loaded.stamp).ok()?;
    Some(Written {
        file: SavedFile {
            path: loaded.path,
            stamp,
            content,
        },
        id: file.note.id.clone(),
        outside: Some(outside),
        rewrote: true,
        fork: None,
    })
}

fn write_as_new(
    library: &Library,
    mut file: NoteFile,
    content: Option<u64>,
) -> Result<Written, LibraryError> {
    file.note.id = uuid::Uuid::new_v4().to_string();
    let saved = library.create(Path::new(""), &file)?;
    Ok(Written {
        file: SavedFile {
            path: saved.path,
            stamp: saved.stamp,
            content,
        },
        id: file.note.id,
        outside: None,
        rewrote: true,
        fork: None,
    })
}

/// A resumed sheet that can no longer go in its note at `from` becomes a new note in the same
/// binder (created again if it went with the note; the library root for a folder whose name the
/// library would not make).
fn write_beside(
    library: &Library,
    mut file: NoteFile,
    content: Option<u64>,
    from: &Path,
    reason: ForkReason,
) -> Result<Written, LibraryError> {
    file.note.id = uuid::Uuid::new_v4().to_string();
    let binder = from
        .parent()
        .and_then(|dir| dir.strip_prefix(library.root()).ok())
        .unwrap_or_else(|| Path::new(""));
    let saved = match library.create(binder, &file) {
        Err(LibraryError::InvalidName { .. }) => library.create(Path::new(""), &file)?,
        saved => saved?,
    };
    Ok(Written {
        file: SavedFile {
            path: saved.path,
            stamp: saved.stamp,
            content,
        },
        id: file.note.id,
        outside: None,
        rewrote: true,
        fork: Some((reason, from.to_path_buf())),
    })
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, HubState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues `events` for every window and wakes the threads that have work. Every change of
    /// the session ends here (even one without events), which keeps [`Activity::live`] up to
    /// date.
    fn publish(&self, state: &mut HubState, events: Vec<LiveEvent>) {
        self.activity.set_live(state.session.is_running());
        if events.is_empty() {
            return;
        }
        if may_need_save(&events) {
            self.save_wake.notify_all();
        }
        if state.subscribers.is_empty() {
            return;
        }
        if let Some((last, others)) = state.subscribers.split_last_mut() {
            for subscriber in others {
                for event in &events {
                    enqueue(&mut subscriber.queue, event.clone());
                }
            }
            for event in events {
                enqueue(&mut last.queue, event);
            }
        }
        self.flush_wake.notify_all();
    }

    fn input(&self, session: u64, input: LiveInput) {
        // The setting is read (a small file) without holding the lock.
        let button = matches!(input, LiveInput::Button).then(|| {
            let setting = self.lock().button.clone();
            setting()
        });
        let mut state = self.lock();
        if state.accepting != Some(session) || state.session.id() != session {
            return;
        }
        let mut out = Vec::new();
        let session = &mut state.session;
        match input {
            LiveInput::Started {
                width,
                height,
                orientation,
            } => session.started(width, height, orientation, &mut out),
            LiveInput::Reoriented {
                width,
                height,
                orientation,
            } => session.reorient(width, height, orientation, &mut out),
            LiveInput::Records { records, received } => {
                session.records(&records, wall_ms(received), &mut out);
            }
            LiveInput::ProximityIn { .. } => {}
            LiveInput::Button => session.command(
                LiveCommandKind::Button,
                button.unwrap_or_default(),
                now_ms(),
                &mut out,
            ),
            LiveInput::State(LiveState::Waiting) => {
                session.set_state(LiveSessionState::Waiting, &mut out);
            }
            LiveInput::State(LiveState::Live) => {
                session.set_state(LiveSessionState::Live, &mut out);
            }
            LiveInput::State(LiveState::Reconnecting) => {
                session.set_state(LiveSessionState::Reconnecting, &mut out);
            }
            LiveInput::State(LiveState::Stopped) => {
                session.set_state(LiveSessionState::Idle, &mut out);
                session.mark_all_urgent();
            }
        }
        self.publish(&mut state, out);
    }

    /// Sends what is pending, at most every `flush_interval`, until shutdown.
    fn flush_loop(&self) {
        let mut state = self.lock();
        loop {
            if state.shutdown {
                return;
            }
            if state.subscribers.iter().all(|s| s.queue.is_empty()) {
                state = self
                    .flush_wake
                    .wait(state)
                    .unwrap_or_else(PoisonError::into_inner);
                continue;
            }
            let now = Instant::now();
            if let Some(last) = state.last_flush {
                let due = last + self.config.flush_interval;
                if now < due {
                    // Not a timed wait on the condition variable: on Windows it follows the
                    // system timer (15.6 ms by default), while `sleep` uses a high-resolution
                    // timer (to be confirmed at H2). Events keep arriving in the queues meanwhile.
                    drop(state);
                    std::thread::sleep(due - now);
                    state = self.lock();
                    continue;
                }
            }
            let batches: Vec<(u64, Arc<dyn BatchSink>, Vec<LiveEvent>)> = state
                .subscribers
                .iter_mut()
                .filter(|s| !s.queue.is_empty())
                .map(|s| (s.id, s.sink.clone(), std::mem::take(&mut s.queue)))
                .collect();
            state.last_flush = Some(now);
            drop(state);
            let mut failed = Vec::new();
            for (id, sink, events) in batches {
                let batch = LiveBatch {
                    events,
                    sent_at: wall_now_ms(),
                };
                if sink.send(batch).is_err() {
                    failed.push(id);
                }
            }
            state = self.lock();
            if !failed.is_empty() {
                state.subscribers.retain(|s| !failed.contains(&s.id));
            }
        }
    }

    /// When the next page is due to be written (`None`: nothing to write).
    fn next_save(&self, state: &HubState, now: Instant) -> Option<Instant> {
        state.library.as_ref()?;
        state
            .session
            .pages()
            .iter()
            .filter_map(|page| page_due(page, now, self.config.save_interval))
            .min()
    }

    /// Writes the pages that are due, until shutdown.
    fn save_loop(&self) {
        let mut state = self.lock();
        loop {
            if state.shutdown {
                return;
            }
            let now = Instant::now();
            match self.next_save(&state, now) {
                Some(due) if due <= now => {
                    drop(state);
                    // A failure is logged and retried after the save interval.
                    let _ = self.save_pass(false);
                    state = self.lock();
                }
                Some(due) => {
                    state = self
                        .save_wake
                        .wait_timeout(state, due - now)
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                }
                None => {
                    state = self
                        .save_wake
                        .wait(state)
                        .unwrap_or_else(PoisonError::into_inner);
                }
            }
        }
    }

    /// Writes the pages that are due (`force`: every page with changes). Returns the first
    /// failure; the other pages are written anyway.
    fn save_pass(&self, force: bool) -> Result<(), LibraryError> {
        let _saving = self.saving.lock().unwrap_or_else(PoisonError::into_inner);
        let (library, jobs) = {
            let mut state = self.lock();
            let now = Instant::now();
            let Some(library) = state.library.clone() else {
                return Ok(());
            };
            let session_id = state.session.id();
            let mut jobs = Vec::new();
            for index in 0..state.session.pages().len() {
                let Some(page) = state.session.page_mut(index) else {
                    continue;
                };
                let Some(due) = page_due(page, now, self.config.save_interval) else {
                    continue;
                };
                if !force && due > now {
                    continue;
                }
                page.urgent = false;
                page.last_attempt = Some(now);
                jobs.push(SaveJob {
                    session: session_id,
                    page: index,
                    version: page.version,
                    note: page.note.clone(),
                    audio: page.audio.clone(),
                    file: page.file.clone(),
                    base: page.base().cloned(),
                });
            }
            (library, jobs)
        };
        let mut first_error = None;
        for job in jobs {
            let result = write_page(&library, &job);
            let mut state = self.lock();
            match result {
                Ok(written) => self.written(&mut state, &job, written),
                Err(error) => {
                    self.write_failed(&mut state, &job, &error);
                    drop(state);
                    eprintln!("live mode: cannot write page {}: {error}", job.page + 1);
                    first_error.get_or_insert(error);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    /// Records a page written; tells the windows when a note appeared, or when a page whose
    /// write had failed is safe again (then the pages still failing are announced again).
    fn written(&self, state: &mut HubState, job: &SaveJob, written: Written) {
        if state.session.id() != job.session {
            return;
        }
        let before = state.session.saved();
        let Some(page) = state.session.page_mut(job.page) else {
            return;
        };
        let fork = written.fork.map(|(reason, original)| SheetFork {
            reason,
            original: path_text(&original),
            path: path_text(&written.file.path),
        });
        page.file = Some(written.file);
        page.note.id = written.id;
        if let Some(outside) = written.outside {
            page.note.title = outside.title;
            page.note.tags = outside.tags;
            page.note.favorite = outside.favorite;
        }
        page.saved_version = job.version;
        if written.rewrote {
            page.mark_rewritten();
        }
        let forked = fork.is_some();
        if let Some(fork) = fork {
            page.set_fork(fork);
        }
        let recovered = page.save_error.take().is_some();
        let after = state.session.saved();
        let mut events = Vec::new();
        if after != before || recovered {
            events.push(LiveEvent::Saved { paths: after });
            events.extend(state.session.save_failures());
        }
        if forked {
            events.extend(state.session.sheet_forks().into_iter().filter(
                |event| matches!(event, LiveEvent::SheetForked { page_index, .. } if *page_index == job.page),
            ));
        }
        self.publish(state, events);
    }

    /// A page could not be written: the windows are told once, until a write of it succeeds.
    fn write_failed(&self, state: &mut HubState, job: &SaveJob, error: &LibraryError) {
        if state.session.id() != job.session {
            return;
        }
        let Some(page) = state.session.page_mut(job.page) else {
            return;
        };
        if page.save_error.is_some() {
            return;
        }
        let message = error.to_string();
        let code = error.code();
        page.save_error = Some((code, message.clone()));
        self.publish(
            state,
            vec![LiveEvent::SaveFailed {
                page_index: job.page,
                code: code.to_string(),
                message,
            }],
        );
    }
}

/// A free `<time> page <n>.ardoise` in `dir` (`-2`, `-3`... when taken), written atomically.
fn write_backup(dir: &Path, page_index: usize, bytes: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stem = format!(
        "{} page {}",
        chrono::Local::now().format("%Y-%m-%d_%H%M%S"),
        page_index + 1
    );
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{stem}.ardoise")
        } else {
            format!("{stem}-{n}.ardoise")
        };
        let path = dir.join(name);
        match write_atomic_new(&path, bytes) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
            Err(error) => return Err(error),
        }
    }
}

impl LiveHub {
    /// A hub with an idle session, and its two threads.
    pub fn new(config: HubConfig) -> LiveHub {
        let size = default_size();
        let shared = Arc::new(Shared {
            config,
            activity: Activity::default(),
            state: Mutex::new(HubState {
                session: LiveSession::new(
                    0,
                    size,
                    LiveStyle::default(),
                    LiveViewState::whole(size),
                    false,
                    now_ms(),
                ),
                accepting: None,
                started: false,
                next_session: 1,
                library: None,
                button: Arc::new(LiveButton::default),
                subscribers: Vec::new(),
                next_subscriber: 1,
                last_flush: None,
                shutdown: false,
            }),
            flush_wake: Condvar::new(),
            save_wake: Condvar::new(),
            saving: Mutex::new(()),
            narrating: Mutex::new(()),
        });
        for (name, work) in [
            ("live-flush", Shared::flush_loop as fn(&Shared)),
            ("live-save", Shared::save_loop as fn(&Shared)),
        ] {
            let shared = shared.clone();
            if let Err(error) = std::thread::Builder::new()
                .name(name.to_owned())
                .spawn(move || work(&shared))
            {
                eprintln!("live mode: cannot start the {name} thread: {error}");
            }
        }
        LiveHub { shared }
    }

    /// The session as it is now.
    #[cfg(test)]
    pub fn snapshot(&self) -> LiveSnapshot {
        self.shared.lock().session.snapshot()
    }

    /// Several changes of the session under one hold of the lock (no pass of the saver between
    /// them), and what they emitted sent.
    #[cfg(test)]
    pub fn change_at_once(&self, change: impl FnOnce(&mut LiveSession, &mut Vec<LiveEvent>)) {
        self.change(|_| true, change).unwrap();
    }

    /// The session as it is now, as the first events of a subscription give it: the snapshot,
    /// then the stroke in progress and the marks written over the presentation. The frozen
    /// image of a paused share on the network.
    pub fn current_events(&self) -> Vec<LiveEvent> {
        let state = self.shared.lock();
        let mut events = vec![LiveEvent::Snapshot {
            snapshot: Arc::new(state.session.snapshot()),
        }];
        events.extend(state.session.in_progress());
        events.extend(state.session.marks_event());
        events
    }

    /// The background image of page `index` and the file holding its pixels: a resumed sheet is
    /// the only page with one. Only a look under the lock: decoding and rendering are the caller's,
    /// after it (`share::live_paper`, `live_page_background`).
    pub fn page_image(&self, index: usize) -> Option<(BackgroundImage, Arc<ResumeBase>)> {
        let state = self.shared.lock();
        let page = state.session.pages().get(index)?;
        Some((page.note.background_image.clone()?, page.base()?.clone()))
    }

    /// Sends the session's batches to `sink` from now on, replacing the window's previous
    /// subscription. The first batch starts with the returned snapshot (and the stroke in
    /// progress), so a window may apply the batches in order whatever the order in which the
    /// command's answer and the first batch arrive. A sheet that went on in a new note is told to a
    /// late window while its session runs; a stopped session's are not, or the view of the next
    /// lesson would open on a notice about this one (`live_stop` gave them back).
    pub fn subscribe(&self, label: &str, sink: Arc<dyn BatchSink>) -> Arc<LiveSnapshot> {
        let mut state = self.shared.lock();
        let snapshot = Arc::new(state.session.snapshot());
        let mut queue = vec![LiveEvent::Snapshot {
            snapshot: snapshot.clone(),
        }];
        queue.extend(state.session.in_progress());
        queue.extend(state.session.marks_event());
        queue.extend(state.session.save_failures());
        queue.extend(state.session.sheet_forks());
        let id = state.next_subscriber;
        state.next_subscriber += 1;
        state.subscribers.retain(|s| s.label != label);
        state.subscribers.push(Subscriber {
            id,
            label: label.to_owned(),
            sink,
            queue,
        });
        self.shared.flush_wake.notify_all();
        snapshot
    }

    /// The window `label` went away.
    pub fn unsubscribe(&self, label: &str) {
        self.shared.lock().subscribers.retain(|s| s.label != label);
    }

    /// Number of windows subscribed.
    #[cfg(test)]
    pub fn subscriber_count(&self) -> usize {
        self.shared.lock().subscribers.len()
    }

    /// Labels of the subscriptions (windows, viewers of the share).
    #[cfg(test)]
    pub fn subscriber_labels(&self) -> Vec<String> {
        self.shared
            .lock()
            .subscribers
            .iter()
            .map(|s| s.label.clone())
            .collect()
    }

    /// Starts a session (or returns the running one). The previous session's changes are
    /// written first; if that fails nothing starts. The style and the view (blackboard, laser)
    /// carry over; the view shows the whole page again.
    pub fn start(&self, options: StartOptions) -> Result<Start, LiveError> {
        // The previous session is replaced only once everything in it is written (a command may
        // change it between the write and the lock: a few attempts, then it is replaced).
        let mut attempts = 0;
        let mut state = loop {
            {
                let state = self.shared.lock();
                if state.session.is_running() {
                    return Ok(Start::Running(state.session.snapshot()));
                }
            }
            self.shared.save_pass(true)?;
            let state = self.shared.lock();
            attempts += 1;
            let unsaved = state.library.is_some()
                && state.session.pages().iter().any(|page| page.needs_write());
            if !unsaved || attempts >= 3 {
                break state;
            }
        };
        if state.session.is_running() {
            return Ok(Start::Running(state.session.snapshot()));
        }
        let id = state.next_session;
        state.next_session += 1;
        let size = page_size_of(&options);
        let old = &state.session;
        let view = LiveViewState {
            board: old.view().board,
            laser: old.view().laser,
            ..LiveViewState::whole(size)
        };
        let session = LiveSession::new(id, size, old.style(), view, options.french, now_ms())
            .with_orientation(options.orientation);
        Ok(self.install(state, session, options))
    }

    /// Starts a session on the note at `path` (SPEC §4.2): its first page is that note, whose
    /// file every write of the page goes through, and the next page is a new note. What was
    /// shown goes first, as for a new session: a session still running is stopped (its tablet
    /// input is ignored from now on), and if anything in it cannot be written nothing starts. A
    /// note that cannot be read leaves all of that as it is.
    pub fn resume(&self, options: StartOptions, path: &Path) -> Result<Start, LiveError> {
        let mut loaded = options.library.read(path)?;
        // The same few attempts as `start`: a command may change the old session meanwhile.
        for _ in 0..3 {
            let unwritten = {
                let state = self.shared.lock();
                state.session.is_running()
                    || (state.library.is_some()
                        && state.session.pages().iter().any(Page::needs_write))
            };
            if !unwritten {
                break;
            }
            self.stop()?;
            // The old session may just have written that very note.
            loaded = options.library.read(path)?;
        }
        let seed = ResumeSeed::of(loaded);
        let mut state = self.shared.lock();
        if state.session.is_running() {
            return Ok(Start::Running(state.session.snapshot()));
        }
        let id = state.next_session;
        state.next_session += 1;
        let size = page_size_of(&options);
        let old = &state.session;
        let view = LiveViewState {
            laser: old.view().laser,
            ..LiveViewState::whole(size)
        };
        let now = now_ms();
        let session = LiveSession::new(id, size, old.style_off_board(), view, options.french, now)
            .with_orientation(options.orientation)
            .resuming(seed, now);
        Ok(self.install(state, session, options))
    }

    /// `session` replaces the session shown and waits for the tablet.
    fn install(
        &self,
        mut state: MutexGuard<'_, HubState>,
        mut session: LiveSession,
        options: StartOptions,
    ) -> Start {
        let id = session.id();
        session.set_presentation(state.session.presentation().cloned(), &mut Vec::new());
        let mut out = Vec::new();
        // The old session's marks are not the new one's.
        state.session.clear_marks(&mut out);
        session.set_state(LiveSessionState::Waiting, &mut out);
        state.session = session;
        state.accepting = Some(id);
        state.started = true;
        state.library = Some(options.library);
        state.button = options.button;
        let snapshot = state.session.snapshot();
        self.shared.publish(&mut state, out);
        Start::New {
            snapshot,
            sink: HubSink {
                shared: self.shared.clone(),
                session: id,
            },
        }
    }

    /// The tablet service could not start session `id`: it goes back to idle.
    pub fn abort_start(&self, id: u64) {
        let mut state = self.shared.lock();
        if state.session.id() != id {
            return;
        }
        state.accepting = None;
        let mut out = Vec::new();
        state.session.set_state(LiveSessionState::Idle, &mut out);
        self.shared.publish(&mut state, out);
    }

    /// Stops the session: the stroke in progress ends, the tablet's input is ignored from now
    /// on, and every page with changes is written. Returns the paths of the notes of the
    /// session. The stopped session stays shown until the next start.
    pub fn stop(&self) -> Result<Vec<String>, LiveError> {
        {
            let mut state = self.shared.lock();
            state.accepting = None;
            let mut out = Vec::new();
            state.session.set_state(LiveSessionState::Idle, &mut out);
            state.session.mark_all_urgent();
            self.shared.publish(&mut state, out);
        }
        self.shared.save_pass(true)?;
        Ok(self.shared.lock().session.saved())
    }

    /// Runs `change` on the session and sends what it emitted; [`LiveError::NotRunning`] before
    /// the first start. Once the session is stopped its notes are in the library: a change that
    /// would rewrite one (`rewrites` says so) is refused ([`LiveError::Stopped`]), while paging
    /// through the pages and moving the view are still allowed.
    fn change(
        &self,
        rewrites: impl FnOnce(&LiveSession) -> bool,
        change: impl FnOnce(&mut LiveSession, &mut Vec<LiveEvent>),
    ) -> Result<(), LiveError> {
        let mut state = self.shared.lock();
        if !state.started {
            return Err(LiveError::NotRunning);
        }
        if !state.session.is_running() && rewrites(&state.session) {
            return Err(LiveError::Stopped);
        }
        let mut out = Vec::new();
        change(&mut state.session, &mut out);
        self.shared.publish(&mut state, out);
        Ok(())
    }

    pub fn command(&self, kind: LiveCommandKind) -> Result<(), LiveError> {
        let button = (kind == LiveCommandKind::Button).then(|| {
            let setting = self.shared.lock().button.clone();
            setting()
        });
        // What the button does depends on the settings.
        let effective = match (kind, button) {
            (LiveCommandKind::Button, Some(LiveButton::NewLayer)) => LiveCommandKind::NewLayer,
            (LiveCommandKind::Button, _) => LiveCommandKind::NewPage,
            (kind, _) => kind,
        };
        self.change(
            |_| {
                matches!(
                    effective,
                    LiveCommandKind::NewLayer | LiveCommandKind::ClearPage
                )
            },
            |session, out| {
                session.command(kind, button.unwrap_or_default(), now_ms(), out);
            },
        )
    }

    /// « Undo the clearing »: gives the current page the ink of the last « Clear the page »
    /// back, through its history. Returns whether anything was restored (the page was left
    /// meanwhile).
    pub fn restore_cleared(&self) -> Result<bool, LiveError> {
        let prepared = self.prepared_undo();
        let mut restored = false;
        self.change(
            |_| true,
            |session, out| restored = session.restore_cleared_with(prepared, out),
        )?;
        Ok(restored)
    }

    /// Deletes or recolours strokes a window chose on page `page_index` at `revision` (see
    /// [`LiveSession::edit_strokes`]). The list is checked and sorted before the lock is taken.
    /// Returns the page's revision after the edit.
    pub fn edit_strokes(
        &self,
        page_index: usize,
        revision: u64,
        edit: StrokesEdit,
    ) -> Result<u64, LiveError> {
        let edit = edit.checked()?;
        let mut outcome = Err(LiveEditError::Stale);
        self.change(
            |_| true,
            |session, out| outcome = session.edit_strokes(page_index, revision, edit, out),
        )?;
        Ok(outcome?)
    }

    /// Undoes the last edit of page `page_index` (the one on screen). Returns whether there was
    /// anything to undo.
    pub fn undo_edit(&self, page_index: usize) -> Result<bool, LiveError> {
        let prepared = self.prepared_undo();
        let mut outcome = Ok(false);
        self.change(
            |_| true,
            |session, out| outcome = session.undo_edit(page_index, prepared, out),
        )?;
        Ok(outcome?)
    }

    /// Does again the last edit undone on page `page_index` (the one on screen): the strokes move
    /// from the page to its history, nothing is copied. Returns whether there was anything to redo.
    pub fn redo_edit(&self, page_index: usize) -> Result<bool, LiveError> {
        let mut outcome = Ok(false);
        self.change(
            |_| true,
            |session, out| outcome = session.redo_edit(page_index, out),
        )?;
        Ok(outcome?)
    }

    /// A copy of the strokes the next undo gives back, made without holding the lock: those points
    /// can be many, and the tablet's input waits for that lock.
    fn prepared_undo(&self) -> Option<Prepared> {
        let stored = self.shared.lock().session.next_restored()?;
        Some(Prepared::of(stored))
    }

    pub fn set_style(&self, style: LiveStyle) -> Result<(), LiveError> {
        self.change(|_| false, |session, out| session.set_style(style, out))
    }

    /// The shared view; turning the blackboard on or off changes the current page's paper.
    pub fn set_view(&self, view: LiveViewState) -> Result<(), LiveError> {
        self.change(
            |session| view.board != session.view().board,
            |session, out| session.set_view(view, out),
        )
    }

    /// The marks the presentation window wrote over page `page_index`, the whole list (see
    /// [`LiveMark`]); sent to the viewers of the share. Allowed once the session is stopped, like
    /// the view: the presentation stays up.
    pub fn set_marks(&self, page_index: usize, marks: Vec<LiveMark>) -> Result<(), LiveError> {
        self.change(
            |_| false,
            |session, out| session.set_marks(page_index, marks, out),
        )
    }

    /// Writes into `dir` a complete copy (`.ardoise`, atomic, a new note id) of every page whose
    /// changes are not in the library: the end of the app when the library cannot be written,
    /// so that nothing drawn is lost. Returns each page (0-based) with its copy or the error.
    pub fn back_up_unsaved(&self, dir: &Path) -> Vec<(usize, std::io::Result<PathBuf>)> {
        type Unsaved = (usize, Note, Option<Arc<Vec<u8>>>, Option<Arc<ResumeBase>>);
        let pages: Vec<Unsaved> = {
            let state = self.shared.lock();
            state
                .session
                .pages()
                .iter()
                .enumerate()
                .filter(|(_, page)| page.needs_write())
                .map(|(index, page)| {
                    (
                        index,
                        page.note.clone(),
                        page.audio.clone(),
                        page.base().cloned(),
                    )
                })
                .collect()
        };
        pages
            .into_iter()
            .map(|(index, note, audio, base)| {
                let copy = page_file(&note, audio.as_ref(), base.as_deref())
                    .and_then(|mut file| {
                        file.note.id = uuid::Uuid::new_v4().to_string();
                        page_thumbnail(&mut file, base.as_deref());
                        ardoise_core::format::to_bytes(&file)
                    })
                    .map_err(std::io::Error::other)
                    .and_then(|bytes| write_backup(dir, index, &bytes));
                (index, copy)
            })
            .collect()
    }

    /// Whether live mode runs or the presentation window is open, readable without any lock
    /// (shared: the presentation window sets its part).
    pub fn activity(&self) -> Activity {
        self.shared.activity.clone()
    }

    /// The session shown and its current page; `None` before the first start.
    pub fn page_position(&self) -> Option<PagePosition> {
        let state = self.shared.lock();
        state.started.then(|| PagePosition {
            session: state.session.id(),
            page: state.session.current(),
            running: state.session.is_running(),
        })
    }

    /// The session shown and the notes it wrote; `None` before the first start.
    pub fn overview(&self) -> Option<LiveOverview> {
        let state = self.shared.lock();
        let session = &state.session;
        state.started.then(|| LiveOverview {
            state: session.state(),
            running: session.is_running(),
            page: session.current(),
            pages: session.pages().len(),
            presentation: session.presentation().map(|monitor| monitor.name.clone()),
            library: state
                .library
                .as_ref()
                .map(|library| library.root().to_path_buf()),
            notes: session
                .pages()
                .iter()
                .enumerate()
                .filter_map(|(index, page)| {
                    let file = page.saved_file()?;
                    Some((index, page.note.id.clone(), file.path.clone()))
                })
                .collect(),
        })
    }

    /// The time origin of every page of session `session` (`Page::time_origin`: `None` for a
    /// page without any stroke); `None` when another session replaced it.
    pub fn page_origins(&self, session: u64) -> Option<Vec<Option<i64>>> {
        let state = self.shared.lock();
        (state.session.id() == session).then(|| {
            state
                .session
                .pages()
                .iter()
                .map(Page::time_origin)
                .collect()
        })
    }

    /// Merges `narration` (recorded in live mode: note time) into the narration of page `page`
    /// of session `session`, encodes the result to `audio.opus` once (outside the session
    /// lock), and writes it with the page at the next pass. The hub stays the only writer of
    /// live pages, so their stamps stay right. Also after a stop: the narration of a session
    /// is saved even when live mode was left first. Returns `false` when nothing changed: a
    /// page without any stroke, or a session or page that no longer exists.
    ///
    /// The page keeps only the encoded narration: a second recording on it decodes the first
    /// one again (rare) rather than holding its sound for the rest of the session. A resumed
    /// sheet starts with the narration of its note: nothing of it is replaced, whatever its time
    /// base ([`placed_for_merge`]) or the time since it was recorded ([`merged_keeping_both`]).
    pub fn attach_narration(
        &self,
        session: u64,
        page: usize,
        narration: Narration,
    ) -> Result<bool, LiveError> {
        let _attaching = self
            .shared
            .narrating
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let existing = {
            let state = self.shared.lock();
            if state.session.id() != session {
                return Ok(false);
            }
            match state.session.pages().get(page) {
                Some(target) if target.time_origin().is_some() => target.audio.clone(),
                _ => return Ok(false),
            }
        };
        let merged = match existing {
            Some(bytes) => {
                let earlier = decode_narration(&bytes).map_err(LiveError::Narration)?;
                let later = if earlier.options == ReplayOptions::default() {
                    narration
                } else {
                    let note = {
                        let state = self.shared.lock();
                        state.session.pages().get(page).map(|p| p.note.clone())
                    };
                    match note {
                        Some(note) => placed_for_merge(narration, &earlier, &note),
                        None => narration,
                    }
                };
                merged_keeping_both(earlier, later)
            }
            None => narration,
        };
        let audio = encode_narration(&merged).map_err(LiveError::Narration)?;
        drop(merged);
        let mut state = self.shared.lock();
        if state.session.id() != session {
            return Ok(false);
        }
        let attached = state
            .session
            .page_mut(page)
            .is_some_and(|target| target.set_audio(audio));
        if attached {
            self.shared.save_wake.notify_all();
        }
        Ok(attached)
    }

    /// Gives page `page` of the current session a narration with ready-made `audio` bytes (no
    /// encoding, no merge): the tests of what the hub writes.
    #[cfg(test)]
    pub fn set_page_audio(&self, page: usize, audio: Vec<u8>) -> bool {
        let mut state = self.shared.lock();
        let attached = state
            .session
            .page_mut(page)
            .is_some_and(|target| target.set_audio(audio));
        if attached {
            self.shared.save_wake.notify_all();
        }
        attached
    }

    /// The presentation window opened on `monitor`, or closed (`None`).
    pub fn set_presentation(&self, monitor: Option<MonitorInfo>) {
        let mut state = self.shared.lock();
        let mut out = Vec::new();
        state.session.set_presentation(monitor, &mut out);
        self.shared.publish(&mut state, out);
    }

    /// The presentation window `label` was closed.
    pub fn presentation_closed(&self, label: &str) {
        self.unsubscribe(label);
        self.set_presentation(None);
    }
}

impl Drop for LiveHub {
    fn drop(&mut self) {
        self.shared.lock().shutdown = true;
        self.shared.flush_wake.notify_all();
        self.shared.save_wake.notify_all();
    }
}
