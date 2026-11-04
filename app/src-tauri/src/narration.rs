//! Narration at the microphone (SPEC §4.5): choosing the microphone, its level, recording
//! during a replay or in live mode, and the narration stored in a note (`audio.opus`).
//! Contract: `docs/design/media.md`, "Interface package: contracts" and the decisions after
//! it (shapes of `app/ui/src/lib/video.ts`).
//!
//! - One capture at a time in the app ([`NarrationRuntime`]): a level-only monitor (settings,
//!   before recording) or a recording. Starting a recording stops a monitor first;
//!   `microphone_monitor_stop` stops only a monitor.
//! - Every recording has a watcher that notes, every 50 ms, the smallest "wall clock − audio
//!   received" ([`PageLog`]): the wall clock of the first sample.
//! - A recording during a replay is in replay time (the replay's speed and trimming): at stop
//!   it replaces the narration of its note (at the path given at stop when the editor renamed
//!   it meanwhile), through the library's checked save with the editor's stamp (a version
//!   changed on disk is a `noteConflict`, nothing is written). Its first sample is placed on
//!   the replay from the wall clock at which the replay was at instant 0 ([`replay_start_ms`]).
//! - A replay recording that cannot be saved is not lost: it stays pending in the runtime for
//!   `narration_retry` (into the note's current path) or `narration_discard`, and a copy of
//!   its `audio.opus` is written to [`backup_dir`] (local to this PC, outside the library),
//!   removed once the narration is saved.
//! - A recording in live mode is in note time: the watcher also notes which page is shown; at
//!   stop the clip is cut per page ([`split_live_recording`]), each piece placed from its
//!   page's first stroke, merged with the page's narration and handed to the live hub, which
//!   writes it with the page (the hub is the only writer of live pages).

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use ardoise_core::fsutil;
use ardoise_core::library::{FileStamp, Library, LibraryError};
use ardoise_media::microphone::{CaptureStatus, Level, Recorder, list_input_devices};
use ardoise_media::replay::{MAX_SPEED, MIN_SPEED, ReplayOptions};
use ardoise_media::{
    AudioClip, MediaError, NARRATION_SAMPLE_RATE, Narration, NarrationBase, NarrationHeader,
    encode_narration, narration_header,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use crate::AppContext;
use crate::commands::{AppState, CommandError, CommandResult, store, update};
use crate::library::{library_error, library_of, now_ms};
use crate::live::model::wall_now_ms;
use crate::live::{LiveHub, LiveRuntime, PagePosition};
use crate::video::{INVALID_VIDEO_SETTINGS, duration_ms};

/// No microphone matches the one asked for (unplugged).
pub const MICROPHONE_NOT_FOUND: &str = "microphoneNotFound";
/// The microphone delivers only digital silence: Windows blocks desktop apps from using it
/// (Settings > Privacy & security > Microphone), or a hardware mute.
pub const MICROPHONE_SILENT: &str = "microphoneSilent";
/// Opening, reading or encoding the recording failed.
pub const MICROPHONE_FAILED: &str = "microphoneFailed";
/// This PC has no microphone at all.
pub const NO_MICROPHONE: &str = "noMicrophone";
/// A recording already runs.
pub const NARRATION_BUSY: &str = "narrationBusy";
/// Nothing is being recorded.
pub const NO_RECORDING: &str = "noRecording";
/// The recording captured nothing.
pub const EMPTY_RECORDING: &str = "emptyRecording";
/// The note has no narration.
pub const NO_NARRATION: &str = "noNarration";
/// No replay narration waits to be saved again.
pub const NO_PENDING_NARRATION: &str = "noPendingNarration";

/// How often the watcher of a recording looks at the audio received and the page shown.
pub const WATCH_INTERVAL: Duration = Duration::from_millis(50);

/// Longest note file stem kept in the name of a backup copy, in characters.
const BACKUP_STEM_CHARS: usize = 150;

/// `Microphone` in `video.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Microphone {
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

/// `NarrationTarget` in `video.ts`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum NarrationTarget {
    /// The note at `path` replayed in the editor with these options, from `start_ms`.
    Replay {
        path: String,
        speed: f64,
        max_pause_ms: Option<f64>,
        start_ms: f64,
    },
    /// The pages of live mode.
    Live,
}

/// `NarrationStatus.state` in `video.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum NarrationState {
    Idle,
    Monitoring,
    Recording,
}

/// `CaptureState` in `video.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CaptureState {
    Waiting,
    NoData,
    Receiving,
    Silent,
    Failed,
}

/// Peak and RMS of the last 50 ms, linear 0..=1.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize)]
pub struct LevelState {
    pub peak: f32,
    pub rms: f32,
}

/// `NarrationStatus` in `video.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NarrationStatus {
    pub state: NarrationState,
    pub capture: Option<CaptureState>,
    pub level: LevelState,
    pub elapsed_ms: u64,
    pub device: Option<String>,
    pub failure: Option<String>,
    /// A replay narration that could not be saved waits for `narration_retry`.
    pub pending: bool,
}

impl NarrationStatus {
    /// Nothing is captured.
    pub fn idle() -> Self {
        Self {
            state: NarrationState::Idle,
            capture: None,
            level: LevelState::default(),
            elapsed_ms: 0,
            device: None,
            failure: None,
            pending: false,
        }
    }
}

/// `NarrationSaved` in `video.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NarrationSaved {
    /// Replay target: the note's new stamp. Live target: `None`.
    pub stamp: Option<FileStamp>,
    pub duration_ms: u64,
    /// Notes or pages that got the narration.
    pub pages: usize,
}

/// `NarrationInfo.base` in `video.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BaseName {
    Replay,
    Note,
}

/// `NarrationInfo` in `video.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NarrationInfo {
    pub duration_ms: u64,
    pub base: BaseName,
    pub speed: f64,
    pub max_pause_ms: Option<u64>,
    pub start_ms: i64,
}

impl NarrationInfo {
    pub fn of(header: &NarrationHeader) -> Self {
        Self {
            duration_ms: duration_ms(header.duration),
            base: match header.base {
                NarrationBase::Replay => BaseName::Replay,
                NarrationBase::Note => BaseName::Note,
            },
            speed: header.options.speed,
            max_pause_ms: header.options.max_pause.map(duration_ms),
            start_ms: header.start_ms,
        }
    }
}

// --- Errors -------------------------------------------------------------------------------------

/// Error for the interface from the microphone or the narration's encoding.
pub(crate) fn narration_error(error: MediaError) -> CommandError {
    let code = match &error {
        MediaError::MicrophoneNotFound(_) => MICROPHONE_NOT_FOUND,
        MediaError::MicrophoneSilent => MICROPHONE_SILENT,
        _ => MICROPHONE_FAILED,
    };
    CommandError::new(code, error)
}

/// Error of a capture that could not start; `no_device` says that the PC has no microphone at
/// all (the system's default was asked for and none exists).
pub fn start_error(error: MediaError, no_device: bool) -> CommandError {
    match error {
        MediaError::Microphone { .. } if no_device => CommandError::new(NO_MICROPHONE, error),
        other => narration_error(other),
    }
}

/// Whether the PC has no input device at all (a listing failure says nothing).
fn has_no_microphone() -> bool {
    list_input_devices().is_ok_and(|devices| devices.is_empty())
}

fn busy() -> CommandError {
    CommandError::new(NARRATION_BUSY, "a narration is being recorded")
}

/// A note's narration that cannot be read: the file is damaged.
fn damaged(error: MediaError) -> CommandError {
    CommandError::new(
        "noteDamaged",
        format!("its narration (audio.opus) cannot be read: {error}"),
    )
}

// --- Status -------------------------------------------------------------------------------------

/// The interface's state of a capture, with the detail of a failure.
pub fn capture_state(status: &CaptureStatus) -> (CaptureState, Option<String>) {
    match status {
        CaptureStatus::Waiting => (CaptureState::Waiting, None),
        CaptureStatus::NoData => (CaptureState::NoData, None),
        CaptureStatus::Receiving => (CaptureState::Receiving, None),
        CaptureStatus::Silent => (CaptureState::Silent, None),
        CaptureStatus::Failed(message) => (CaptureState::Failed, Some(message.clone())),
    }
}

/// A level within 0..=1 (0 for anything else).
pub fn level_state(level: Level) -> LevelState {
    let clean = |value: f32| {
        if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        }
    };
    LevelState {
        peak: clean(level.peak),
        rms: clean(level.rms),
    }
}

// --- What the watcher sees: the clock of the first sample, the pages shown ----------------------

/// From audio position `at` on, page `page` (0-based) was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageChange {
    pub at: Duration,
    pub page: usize,
}

/// What the watcher saw during a recording.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLog {
    /// The page shown from the start, then each change; positions strictly increase (live
    /// mode only).
    pub changes: Vec<PageChange>,
    /// Smallest "wall clock − audio received" seen (Unix ms): the wall clock of the first
    /// sample, give or take the delivery delay of the audio (the smallest is the best bound).
    pub anchor_ms: Option<f64>,
}

impl PageLog {
    /// A recording that begins on page `page`.
    pub fn new(page: usize) -> Self {
        Self {
            changes: vec![PageChange {
                at: Duration::ZERO,
                page,
            }],
            anchor_ms: None,
        }
    }

    /// At wall clock `wall_ms` (Unix ms), `audio` had been received and page `page` was shown
    /// (`None`: not known, nothing changes).
    pub fn observe(&mut self, wall_ms: f64, audio: Duration, page: Option<usize>) {
        if !audio.is_zero() && wall_ms.is_finite() {
            let anchor = wall_ms - audio.as_secs_f64() * 1000.0;
            self.anchor_ms = Some(self.anchor_ms.map_or(anchor, |a| a.min(anchor)));
        }
        let Some(page) = page else {
            return;
        };
        match self.changes.last_mut() {
            Some(last) if last.page == page => {}
            // Several changes before any new audio: the last one counts.
            Some(last) if last.at >= audio => {
                last.page = page;
                // Back to the page shown before: no change at all.
                let len = self.changes.len();
                if len >= 2 && self.changes[len - 2].page == page {
                    self.changes.pop();
                }
            }
            _ => self.changes.push(PageChange { at: audio, page }),
        }
    }

    /// Wall clock of the first sample: the anchor, else `fallback_ms` (no audio was seen).
    pub fn wall_start_ms(&self, fallback_ms: f64) -> f64 {
        self.anchor_ms.unwrap_or(fallback_ms)
    }
}

/// Frame of a 48 kHz clip at `at`, at most `frames`.
fn frame_at(at: Duration, frames: usize) -> usize {
    let rate = u128::from(NARRATION_SAMPLE_RATE);
    let frame = (at.as_nanos() * rate + 500_000_000) / 1_000_000_000;
    usize::try_from(frame).unwrap_or(usize::MAX).min(frames)
}

/// Cuts a live recording per page: each stretch between two page changes goes to the page
/// shown then, in note time (base `Note`, speed 1, no trimming) with `start_ms` = wall clock of
/// its first sample − the page's time origin (`origins[page]`: its first stroke, Unix ms; on a
/// resumed sheet, the origin that makes the time since the resume follow what the note holds).
/// `wall_start_ms` is the wall clock of the clip's first sample. Pieces of pages without any
/// stroke (`None`, or unknown pages) and empty pieces are left out. A page shown several times
/// gets several pieces, in order.
///
/// The clip is taken apart, not copied: the pieces are cut off its end one by one (its buffer
/// shrinking as they go) and the first piece keeps the buffer itself.
pub fn split_live_recording(
    clip: AudioClip,
    changes: &[PageChange],
    wall_start_ms: f64,
    origins: &[Option<i64>],
) -> Vec<(usize, Narration)> {
    let channels = usize::from(clip.channels.max(1));
    let frames = clip.samples.len() / channels;
    // The pieces kept: page, first and end frames, start in note time.
    let mut cuts = Vec::new();
    for (i, change) in changes.iter().enumerate() {
        let start = frame_at(change.at, frames);
        let end = changes
            .get(i + 1)
            .map_or(frames, |next| frame_at(next.at, frames));
        if end <= start {
            continue;
        }
        let Some(Some(origin)) = origins.get(change.page) else {
            continue;
        };
        let offset_ms = start as f64 * 1000.0 / f64::from(NARRATION_SAMPLE_RATE);
        let wall = (wall_start_ms + offset_ms).round();
        // Unix milliseconds: well within `i64`.
        let start_ms = (wall as i64).saturating_sub(*origin);
        cuts.push((change.page, start, end, start_ms));
    }
    let mut samples = clip.samples;
    let mut pieces = Vec::with_capacity(cuts.len());
    for &(page, start, end, start_ms) in cuts.iter().rev() {
        // The cuts are in order and within the clip: `start < end <= samples.len()`.
        samples.truncate(end * channels);
        let piece = if start == 0 {
            std::mem::take(&mut samples)
        } else {
            let piece = samples.split_off(start * channels);
            // Gives the piece's room back (usually in place, without copying).
            samples.shrink_to_fit();
            piece
        };
        pieces.push((
            page,
            Narration {
                clip: AudioClip {
                    channels: clip.channels,
                    samples: piece,
                },
                base: NarrationBase::Note,
                options: ReplayOptions::default(),
                start_ms,
            },
        ));
    }
    pieces.reverse();
    pieces
}

/// The pieces of each page merged into one narration, pages in the order they first appear
/// (the pieces are moved, and a page's later piece is appended to its earlier one in place).
pub fn merge_by_page(pieces: Vec<(usize, Narration)>) -> Vec<(usize, Narration)> {
    let mut merged: Vec<(usize, Narration)> = Vec::new();
    for (page, piece) in pieces {
        match merged.iter().position(|(p, _)| *p == page) {
            Some(index) => {
                let (_, earlier) = merged.remove(index);
                merged.insert(index, (page, earlier.merge(piece)));
            }
            None => merged.push((page, piece)),
        }
    }
    merged
}

/// The watcher's loop: every `interval` until `stop`, notes the audio received and the page of
/// `session` shown (one last look after `stop`).
pub fn watch_pages(
    stop: &AtomicBool,
    interval: Duration,
    session: u64,
    first_page: usize,
    audio: &dyn Fn() -> Duration,
    position: &dyn Fn() -> Option<PagePosition>,
    wall_ms: &dyn Fn() -> f64,
) -> PageLog {
    let mut log = PageLog::new(first_page);
    loop {
        let stopping = stop.load(Ordering::Acquire);
        let page = position().filter(|p| p.session == session).map(|p| p.page);
        let received = audio();
        log.observe(wall_ms(), received, page);
        if stopping {
            return log;
        }
        std::thread::sleep(interval);
    }
}

/// How the watcher reads the page shown (`None` outside live mode).
type PositionFn = Arc<dyn Fn() -> Option<PagePosition> + Send + Sync>;

/// The watcher of a recording (see [`watch_pages`]).
struct Watch {
    first_page: usize,
    /// Wall clock when the recording started (Unix ms), if no audio is ever seen.
    started_ms: f64,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<PageLog>>,
}

impl Watch {
    /// Starts watching `recorder` and the page of `session` that `position` reads.
    fn spawn(
        recorder: &Arc<Box<dyn Source>>,
        session: u64,
        first_page: usize,
        position: PositionFn,
    ) -> CommandResult<Watch> {
        let stop = Arc::new(AtomicBool::new(false));
        let started_ms = wall_now_ms();
        let thread = {
            let (stop, recorder) = (Arc::clone(&stop), Arc::clone(recorder));
            std::thread::Builder::new()
                .name("narration-watch".into())
                .spawn(move || {
                    watch_pages(
                        &stop,
                        WATCH_INTERVAL,
                        session,
                        first_page,
                        &|| recorder.elapsed(),
                        &*position,
                        &wall_now_ms,
                    )
                })
                .map_err(|error| CommandError::new(MICROPHONE_FAILED, error))?
        };
        Ok(Watch {
            first_page,
            started_ms,
            stop,
            thread: Some(thread),
        })
    }

    /// Stops the watcher and returns what it saw, with the wall clock of the first sample.
    fn finish(&mut self) -> (PageLog, f64) {
        self.stop.store(true, Ordering::Release);
        let log = self
            .thread
            .take()
            .and_then(|thread| thread.join().ok())
            .unwrap_or_else(|| PageLog::new(self.first_page));
        let wall_start_ms = log.wall_start_ms(self.started_ms);
        (log, wall_start_ms)
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// --- The capture of the app ---------------------------------------------------------------------

/// A microphone capture as the runtime uses it: a [`Recorder`], or a stand-in in the tests.
pub trait Source: Send + Sync {
    fn device_name(&self) -> &str;
    fn level(&self) -> Level;
    /// Audio received so far.
    fn elapsed(&self) -> Duration;
    fn status(&self) -> CaptureStatus;
    /// Stops the capture and returns what it recorded.
    fn stop(self: Box<Self>) -> Result<AudioClip, MediaError>;
}

impl Source for Recorder {
    fn device_name(&self) -> &str {
        Recorder::device_name(self)
    }

    fn level(&self) -> Level {
        Recorder::level(self)
    }

    fn elapsed(&self) -> Duration {
        Recorder::elapsed(self)
    }

    fn status(&self) -> CaptureStatus {
        Recorder::status(self)
    }

    fn stop(self: Box<Self>) -> Result<AudioClip, MediaError> {
        Recorder::stop(*self)
    }
}

/// Opens a microphone (`None`: the system's default); `record` keeps the audio, else only the
/// level is measured.
pub type Opener = dyn Fn(Option<&str>, bool) -> CommandResult<Box<dyn Source>> + Send + Sync;

/// Opens a microphone of this PC.
fn open_microphone(device: Option<&str>, record: bool) -> CommandResult<Box<dyn Source>> {
    let opened = if record {
        Recorder::start(device)
    } else {
        Recorder::monitor(device)
    };
    match opened {
        Ok(recorder) => Ok(Box::new(recorder)),
        Err(error) => Err(start_error(error, device.is_none() && has_no_microphone())),
    }
}

/// Where a replay recording goes.
#[derive(Debug, Clone)]
struct ReplayTarget {
    library: Library,
    path: PathBuf,
    options: ReplayOptions,
    start_ms: i64,
}

enum Purpose {
    Monitor,
    Replay { target: ReplayTarget, watch: Watch },
    Live { session: u64, watch: Watch },
}

struct Capture {
    /// Shared with the watcher, which reads the audio position.
    recorder: Arc<Box<dyn Source>>,
    purpose: Purpose,
}

/// What a stopped capture was for, with the wall clock (Unix ms) of its first sample.
enum Stopped {
    Monitor,
    Replay {
        target: ReplayTarget,
        wall_start_ms: f64,
    },
    Live {
        session: u64,
        log: PageLog,
        wall_start_ms: f64,
    },
}

impl Capture {
    fn is_recording(&self) -> bool {
        !matches!(self.purpose, Purpose::Monitor)
    }

    fn is_live(&self) -> bool {
        matches!(self.purpose, Purpose::Live { .. })
    }

    /// Stops the watcher, then the recorder.
    fn stop(self) -> (Stopped, Result<AudioClip, MediaError>) {
        let Capture { recorder, purpose } = self;
        let stopped = match purpose {
            Purpose::Monitor => Stopped::Monitor,
            Purpose::Replay { target, mut watch } => Stopped::Replay {
                target,
                wall_start_ms: watch.finish().1,
            },
            Purpose::Live { session, mut watch } => {
                let (log, wall_start_ms) = watch.finish();
                Stopped::Live {
                    session,
                    log,
                    wall_start_ms,
                }
            }
        };
        // The watcher has ended: this is the last reference.
        let clip = match Arc::try_unwrap(recorder) {
            Ok(recorder) => recorder.stop(),
            Err(_) => Err(MediaError::Microphone {
                step: "stopping the recording",
                message: "the recording is still in use".into(),
            }),
        };
        (stopped, clip)
    }
}

/// A replay recording that could not be saved: kept until `narration_retry` saves it,
/// `narration_discard` drops it, or a newer one replaces it.
struct Pending {
    /// Its `audio.opus` (also what the backup copy holds).
    audio: Vec<u8>,
    duration_ms: u64,
    /// The backup copy, removed once the narration is saved; `None` when it could not be
    /// written.
    backup: Option<PathBuf>,
}

/// The microphone capture of the running app (one at a time), and the replay narration that
/// waits to be saved again.
pub struct NarrationRuntime {
    capture: Mutex<Option<Capture>>,
    pending: Mutex<Option<Pending>>,
    open: Box<Opener>,
    /// Where the copies of pending narrations go.
    backup_dir: PathBuf,
}

impl Default for NarrationRuntime {
    fn default() -> Self {
        Self::new(Box::new(open_microphone), backup_dir())
    }
}

impl NarrationRuntime {
    /// A runtime opening its microphones with `open` and backing up pending narrations in
    /// `backup_dir`.
    pub fn new(open: Box<Opener>, backup_dir: PathBuf) -> Self {
        Self {
            capture: Mutex::new(None),
            pending: Mutex::new(None),
            open,
            backup_dir,
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<Capture>> {
        self.capture.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_pending(&self) -> MutexGuard<'_, Option<Pending>> {
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes the capture under way (`only_live`: only a live recording).
    fn take(&self, only_live: bool) -> Option<Capture> {
        let mut slot = self.lock();
        if only_live && !slot.as_ref().is_some_and(Capture::is_live) {
            return None;
        }
        slot.take()
    }

    /// Whether a live recording runs.
    #[cfg(test)]
    pub fn is_recording_live(&self) -> bool {
        self.lock().as_ref().is_some_and(Capture::is_live)
    }

    /// Whether a replay narration waits to be saved again.
    pub fn has_pending(&self) -> bool {
        self.lock_pending().is_some()
    }

    /// Keeps a replay narration that could not be saved into the note at `note` (replacing
    /// any older one, whose backup copy stays on disk), after writing its backup copy.
    fn keep_pending(&self, note: &Path, audio: Vec<u8>, duration_ms: u64) {
        let backup = match write_narration_backup(&self.backup_dir, note, &audio) {
            Ok(path) => {
                eprintln!("narration: kept unsaved, copy in {}", path.display());
                Some(path)
            }
            Err(error) => {
                eprintln!("narration: kept unsaved, no copy could be written: {error}");
                None
            }
        };
        *self.lock_pending() = Some(Pending {
            audio,
            duration_ms,
            backup,
        });
    }

    fn take_pending(&self) -> Option<Pending> {
        self.lock_pending().take()
    }

    /// Puts back a pending narration that could not be saved again, unless a newer one took
    /// its place meanwhile.
    fn restore_pending(&self, pending: Pending) {
        let mut slot = self.lock_pending();
        if slot.is_none() {
            *slot = Some(pending);
        }
    }

    /// What the capture under way is doing.
    pub fn status(&self) -> NarrationStatus {
        let pending = self.has_pending();
        let slot = self.lock();
        let Some(capture) = slot.as_ref() else {
            return NarrationStatus {
                pending,
                ..NarrationStatus::idle()
            };
        };
        let recorder = &capture.recorder;
        let (state, failure) = capture_state(&recorder.status());
        NarrationStatus {
            state: if capture.is_recording() {
                NarrationState::Recording
            } else {
                NarrationState::Monitoring
            },
            capture: Some(state),
            level: level_state(recorder.level()),
            elapsed_ms: duration_ms(recorder.elapsed()),
            device: Some(recorder.device_name().to_owned()),
            failure,
            pending,
        }
    }
}

/// A device id from the interface: `None` (or empty) for the system's default.
fn device_of(device: Option<&str>) -> Option<&str> {
    device.map(str::trim).filter(|d| !d.is_empty())
}

/// Starts the level meter (replacing a monitor; refused while recording).
pub fn monitor_in(runtime: &NarrationRuntime, device: Option<&str>) -> CommandResult<()> {
    let mut slot = runtime.lock();
    if slot.as_ref().is_some_and(Capture::is_recording) {
        return Err(busy());
    }
    // The previous monitor stops before the device is opened again.
    *slot = None;
    let recorder = (runtime.open)(device_of(device), false)?;
    *slot = Some(Capture {
        recorder: Arc::new(recorder),
        purpose: Purpose::Monitor,
    });
    Ok(())
}

/// Stops the level meter; a recording under way goes on untouched.
pub fn monitor_stop_in(runtime: &NarrationRuntime) {
    let monitor = {
        let mut slot = runtime.lock();
        if slot.as_ref().is_some_and(|capture| !capture.is_recording()) {
            slot.take()
        } else {
            None
        }
    };
    // The device closes outside the lock.
    drop(monitor);
}

/// The replay options of a target from the interface (`invalidVideoSettings` when out of
/// range).
pub fn target_options(speed: f64, max_pause_ms: Option<f64>) -> CommandResult<ReplayOptions> {
    if !(MIN_SPEED..=MAX_SPEED).contains(&speed) {
        return Err(CommandError::new(
            INVALID_VIDEO_SETTINGS,
            format!("speed {speed} (from {MIN_SPEED} to {MAX_SPEED})"),
        ));
    }
    let max_pause = match max_pause_ms {
        None => None,
        Some(ms) if ms.is_finite() && ms >= 0.0 => Some(
            Duration::try_from_secs_f64(ms / 1000.0)
                .map_err(|error| CommandError::new(INVALID_VIDEO_SETTINGS, error))?,
        ),
        Some(ms) => {
            return Err(CommandError::new(
                INVALID_VIDEO_SETTINGS,
                format!("longest pause {ms} ms"),
            ));
        }
    };
    Ok(ReplayOptions { speed, max_pause })
}

/// Largest time in milliseconds taken from the interface (an instant or a Unix time).
const MAX_INTERFACE_MS: f64 = 1e15;

/// A replay instant from the interface, in whole milliseconds.
fn target_start(start_ms: f64) -> CommandResult<i64> {
    if start_ms.is_finite() && start_ms.abs() < MAX_INTERFACE_MS {
        Ok(start_ms.round() as i64)
    } else {
        Err(CommandError::new(
            INVALID_VIDEO_SETTINGS,
            format!("replay instant {start_ms} ms"),
        ))
    }
}

/// The wall clock (Unix ms) at which the replay was at instant 0, from the interface.
fn replay_clock(started_at_ms: f64) -> CommandResult<f64> {
    if started_at_ms.is_finite() && started_at_ms.abs() < MAX_INTERFACE_MS {
        Ok(started_at_ms)
    } else {
        Err(CommandError::new(
            INVALID_VIDEO_SETTINGS,
            format!("replay start {started_at_ms} ms"),
        ))
    }
}

/// Replay instant (ms) of the first sample of a replay recording: its wall clock less the wall
/// clock at which the replay was at instant 0 and playing (replay time runs at wall-clock
/// speed, whatever the replay's speed). Both are Unix milliseconds checked by
/// [`replay_clock`]: the difference fits in `i64`.
pub fn replay_start_ms(first_sample_wall_ms: f64, replay_started_at_ms: f64) -> i64 {
    (first_sample_wall_ms - replay_started_at_ms).round() as i64
}

/// The note file a replay recording goes to: a note of `library` that exists.
fn replay_target(
    library: Library,
    path: &str,
    speed: f64,
    max_pause_ms: Option<f64>,
    start_ms: f64,
) -> CommandResult<ReplayTarget> {
    let options = target_options(speed, max_pause_ms)?;
    let start_ms = target_start(start_ms)?;
    let path = library.note_path(Path::new(path)).map_err(library_error)?;
    if !path.is_file() {
        return Err(library_error(LibraryError::NotFound { path }));
    }
    Ok(ReplayTarget {
        library,
        path,
        options,
        start_ms,
    })
}

/// Starts recording for `target` (a monitor under way stops first; refused while recording).
/// `library` is asked for only by a replay target; `hub` and `position` (how the watcher reads
/// the page shown) only by a live one.
pub fn start_in(
    runtime: &NarrationRuntime,
    device: Option<&str>,
    target: NarrationTarget,
    library: impl FnOnce() -> CommandResult<Library>,
    hub: &LiveHub,
    position: PositionFn,
) -> CommandResult<()> {
    let mut slot = runtime.lock();
    if slot.as_ref().is_some_and(Capture::is_recording) {
        return Err(busy());
    }
    enum Planned {
        Replay(ReplayTarget),
        Live(PagePosition),
    }
    let planned = match target {
        NarrationTarget::Replay {
            path,
            speed,
            max_pause_ms,
            start_ms,
        } => Planned::Replay(replay_target(
            library()?,
            &path,
            speed,
            max_pause_ms,
            start_ms,
        )?),
        NarrationTarget::Live => match hub.page_position() {
            Some(position) if position.running => Planned::Live(position),
            _ => {
                return Err(CommandError::new(
                    "liveNotRunning",
                    "live mode is not running",
                ));
            }
        },
    };
    *slot = None;
    let recorder = Arc::new((runtime.open)(device_of(device), true)?);
    let purpose = match planned {
        Planned::Replay(target) => Purpose::Replay {
            target,
            watch: Watch::spawn(&recorder, 0, 0, Arc::new(|| None::<PagePosition>))?,
        },
        Planned::Live(start) => Purpose::Live {
            session: start.session,
            watch: Watch::spawn(&recorder, start.session, start.page, position)?,
        },
    };
    *slot = Some(Capture { recorder, purpose });
    Ok(())
}

/// Writes `audio` (an `audio.opus` member) into the note at `path` (replacing any narration),
/// unless the file is no longer the version of `expected` (`noteConflict`, nothing written; no
/// `expected`: checked against the version just read). Everything else in the file is kept.
/// Returns the new stamp.
pub fn save_replay_audio(
    library: &Library,
    path: &Path,
    audio: &[u8],
    expected: Option<&FileStamp>,
) -> CommandResult<FileStamp> {
    let loaded = library.read(path).map_err(library_error)?;
    if expected.is_some_and(|expected| *expected != loaded.stamp) {
        return Err(library_error(LibraryError::Conflict { path: loaded.path }));
    }
    let mut file = loaded.file;
    file.audio = Some(audio.to_vec());
    file.note.modified = now_ms().max(file.note.modified);
    library
        .save(&loaded.path, &file, &loaded.stamp)
        .map_err(library_error)
}

/// Removes the narration of the note at `path` (same checks as [`save_replay_audio`]).
/// A note without narration is left as it is.
pub fn delete_narration_in(
    library: &Library,
    path: &Path,
    expected: &FileStamp,
) -> CommandResult<FileStamp> {
    let loaded = library.read(path).map_err(library_error)?;
    if *expected != loaded.stamp {
        return Err(library_error(LibraryError::Conflict { path: loaded.path }));
    }
    if loaded.file.audio.is_none() {
        return Ok(loaded.stamp);
    }
    let mut file = loaded.file;
    file.audio = None;
    file.note.modified = now_ms().max(file.note.modified);
    library
        .save(&loaded.path, &file, &loaded.stamp)
        .map_err(library_error)
}

/// The narration stored in the note at `path`, if any, decoded (the tests compare it with what
/// the interface gets).
#[cfg(test)]
pub fn read_narration(library: &Library, path: &Path) -> CommandResult<Option<Narration>> {
    let loaded = library.read(path).map_err(library_error)?;
    loaded
        .file
        .audio
        .map(|bytes| ardoise_media::decode_narration(&bytes).map_err(damaged))
        .transpose()
}

/// What the narration of the note at `path` is, if it has one, read without decoding its
/// sound.
pub fn narration_info_in(library: &Library, path: &Path) -> CommandResult<Option<NarrationInfo>> {
    let loaded = library.read(path).map_err(library_error)?;
    loaded
        .file
        .audio
        .map(|bytes| {
            narration_header(&bytes)
                .map(|header| NarrationInfo::of(&header))
                .map_err(damaged)
        })
        .transpose()
}

/// The narration of the note at `path` as stored (`noNarration` when it has none): its Ogg Opus
/// stream, which the WebView plays itself (M10: no longer decoded to a WAV file, which took
/// 345 MB of memory per hour of sound). Its headers are checked (`noteDamaged`), its sound is
/// not decoded.
pub fn narration_opus(library: &Library, path: &Path) -> CommandResult<Vec<u8>> {
    let loaded = library.read(path).map_err(library_error)?;
    let bytes = loaded
        .file
        .audio
        .ok_or_else(|| CommandError::new(NO_NARRATION, "this note has no narration"))?;
    narration_header(&bytes).map_err(damaged)?;
    Ok(bytes)
}

/// Hands the pieces of a live recording to the hub; returns how many pages got narration.
pub fn give_to_hub(
    hub: &LiveHub,
    session: u64,
    log: &PageLog,
    wall_start_ms: f64,
    clip: AudioClip,
) -> CommandResult<usize> {
    let Some(origins) = hub.page_origins(session) else {
        return Err(CommandError::new(
            "liveNotRunning",
            "live mode was started again since the recording began",
        ));
    };
    let pieces = split_live_recording(clip, &log.changes, wall_start_ms, &origins);
    let mut pages = 0;
    for (page, narration) in merge_by_page(pieces) {
        if hub
            .attach_narration(session, page, narration)
            .map_err(crate::live::LiveError::command_error)?
        {
            pages += 1;
        }
    }
    Ok(pages)
}

/// Where the copies of the replay narrations that could not be saved go:
/// `%LOCALAPPDATA%\Ardoise\narration-backup` (local to this PC, never in the library); under
/// the temporary folder when `LOCALAPPDATA` is not set (Linux).
pub fn backup_dir() -> PathBuf {
    backup_dir_in(std::env::var_os("LOCALAPPDATA"))
}

fn backup_dir_in(local_app_data: Option<OsString>) -> PathBuf {
    local_app_data
        .filter(|dir| !dir.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("Ardoise")
        .join("narration-backup")
}

/// Name of the backup copy of a narration for the note at `note` made at `time`
/// (`yyyy-mm-dd hhmmss`): `<note file stem> <time>.opus`, `<…>-<n>.opus` for the `n`-th one
/// (`n` > 1). A very long stem is shortened.
pub fn backup_name(note: &Path, time: &str, n: u32) -> String {
    let stem: String = note
        .file_stem()
        .map(|stem| {
            stem.to_string_lossy()
                .chars()
                .take(BACKUP_STEM_CHARS)
                .collect()
        })
        .unwrap_or_default();
    let stem = stem.trim();
    let stem = if stem.is_empty() { "narration" } else { stem };
    if n <= 1 {
        format!("{stem} {time}.opus")
    } else {
        format!("{stem} {time}-{n}.opus")
    }
}

/// Writes `audio` atomically into a free [`backup_name`] in `dir` (created if needed).
fn write_narration_backup(dir: &Path, note: &Path, audio: &[u8]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let time = chrono::Local::now().format("%Y-%m-%d %H%M%S").to_string();
    let mut n = 1;
    loop {
        let path = dir.join(backup_name(note, &time, n));
        match fsutil::write_atomic_new(&path, audio) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists && n < 1000 => n += 1,
            Err(error) => return Err(error),
        }
    }
}

/// What `narration_stop` asks for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StopRequest {
    /// Keep what was recorded (else it is discarded).
    pub save: bool,
    /// Replay target: the editor's stamp of the note (`None`: checked against the version
    /// read).
    pub stamp: Option<FileStamp>,
    /// Replay target: the note's path now, instead of the one given at the start (the editor
    /// may have renamed the note meanwhile).
    pub path: Option<String>,
    /// Replay target: Unix time (ms) at which the replay was at instant 0 and playing; without
    /// it, the narration starts at the target's `startMs`.
    pub replay_started_at_ms: Option<f64>,
}

/// Saves what a stopped replay recording captured into its note. A narration that cannot be
/// saved stays pending in `runtime` (with its backup copy) and the error is returned as is.
fn save_replay(
    runtime: &NarrationRuntime,
    target: ReplayTarget,
    clip: AudioClip,
    start_ms: i64,
    request: &StopRequest,
) -> CommandResult<NarrationSaved> {
    let duration = duration_ms(clip.duration());
    let narration = Narration {
        clip,
        base: NarrationBase::Replay,
        options: target.options,
        start_ms,
    };
    // Encoded once: what is saved, kept pending and backed up. A clip the encoder refuses
    // (never expected from the recorder) could not be saved by a retry either.
    let audio = encode_narration(&narration).map_err(narration_error)?;
    drop(narration);
    let (note, saved) = match request.path.as_deref() {
        Some(path) => match target.library.note_path(Path::new(path)) {
            Ok(note) => {
                let saved =
                    save_replay_audio(&target.library, &note, &audio, request.stamp.as_ref());
                (note, saved)
            }
            Err(error) => (target.path.clone(), Err(library_error(error))),
        },
        None => {
            let saved = save_replay_audio(
                &target.library,
                &target.path,
                &audio,
                request.stamp.as_ref(),
            );
            (target.path.clone(), saved)
        }
    };
    match saved {
        Ok(stamp) => Ok(NarrationSaved {
            stamp: Some(stamp),
            duration_ms: duration,
            pages: 1,
        }),
        Err(error) => {
            runtime.keep_pending(&note, audio, duration);
            Err(error)
        }
    }
}

/// Saves (or, without `request.save`, discards) what a stopped capture recorded;
/// `replay_started_at` is the checked `request.replay_started_at_ms`.
fn save_stopped(
    runtime: &NarrationRuntime,
    stopped: Stopped,
    clip: Result<AudioClip, MediaError>,
    request: &StopRequest,
    replay_started_at: Option<f64>,
    hub: &LiveHub,
) -> CommandResult<Option<NarrationSaved>> {
    if matches!(stopped, Stopped::Monitor) || !request.save {
        return Ok(None);
    }
    let clip = clip.map_err(narration_error)?;
    if clip.samples.is_empty() {
        return Err(CommandError::new(EMPTY_RECORDING, "nothing was recorded"));
    }
    match stopped {
        Stopped::Monitor => Ok(None),
        Stopped::Replay {
            target,
            wall_start_ms,
        } => {
            let start_ms =
                replay_started_at.map_or(target.start_ms, |at| replay_start_ms(wall_start_ms, at));
            save_replay(runtime, target, clip, start_ms, request).map(Some)
        }
        Stopped::Live {
            session,
            log,
            wall_start_ms,
        } => {
            let duration = duration_ms(clip.duration());
            let pages = give_to_hub(hub, session, &log, wall_start_ms, clip)?;
            Ok(Some(NarrationSaved {
                stamp: None,
                duration_ms: duration,
                pages,
            }))
        }
    }
}

/// Stops the capture under way and, with `request.save`, keeps what it recorded (see the
/// module documentation). Nothing under way: `noRecording` with `save`, else nothing. A bad
/// `replay_started_at_ms` is refused before anything stops.
pub fn stop_in(
    runtime: &NarrationRuntime,
    request: &StopRequest,
    hub: &LiveHub,
) -> CommandResult<Option<NarrationSaved>> {
    let replay_started_at = match request.replay_started_at_ms {
        Some(at) if request.save => Some(replay_clock(at)?),
        _ => None,
    };
    // The capture leaves the runtime first: stopping it (joining threads) holds no lock.
    let Some(capture) = runtime.take(false) else {
        return if request.save {
            Err(CommandError::new(
                NO_RECORDING,
                "no narration is being recorded",
            ))
        } else {
            Ok(None)
        };
    };
    let (stopped, clip) = capture.stop();
    save_stopped(runtime, stopped, clip, request, replay_started_at, hub)
}

/// Stops a live recording under way and gives it to its pages (before live mode stops, or when
/// the app closes); `None` when no live recording runs.
pub fn finish_live(
    runtime: &NarrationRuntime,
    hub: &LiveHub,
) -> Option<CommandResult<NarrationSaved>> {
    let capture = runtime.take(true)?;
    let (stopped, clip) = capture.stop();
    let request = StopRequest {
        save: true,
        ..StopRequest::default()
    };
    save_stopped(runtime, stopped, clip, &request, None, hub).transpose()
}

/// Saves the pending replay narration into the note at `path` (checked like
/// [`save_replay_audio`]), then forgets it and removes its backup copy. On failure it stays
/// pending. `noPendingNarration` when there is none.
pub fn retry_in(
    runtime: &NarrationRuntime,
    library: &Library,
    path: &str,
    stamp: Option<&FileStamp>,
) -> CommandResult<NarrationSaved> {
    let Some(pending) = runtime.take_pending() else {
        return Err(CommandError::new(
            NO_PENDING_NARRATION,
            "no narration waits to be saved",
        ));
    };
    let saved = library
        .note_path(Path::new(path))
        .map_err(library_error)
        .and_then(|note| save_replay_audio(library, &note, &pending.audio, stamp));
    match saved {
        Ok(stamp) => {
            if let Some(backup) = &pending.backup {
                // Best effort: an orphan copy only takes a little room.
                let _ = fsutil::remove_file_with_retry(backup);
            }
            Ok(NarrationSaved {
                stamp: Some(stamp),
                duration_ms: pending.duration_ms,
                pages: 1,
            })
        }
        Err(error) => {
            runtime.restore_pending(pending);
            Err(error)
        }
    }
}

/// Drops the pending replay narration (its backup copy stays on disk, as a last resort).
pub fn discard_in(runtime: &NarrationRuntime) {
    drop(runtime.take_pending());
}

// --- Commands -----------------------------------------------------------------------------------

#[tauri::command]
pub async fn list_microphones() -> CommandResult<Vec<Microphone>> {
    let devices = list_input_devices().map_err(narration_error)?;
    Ok(devices
        .into_iter()
        .map(|device| Microphone {
            id: device.id,
            name: device.name,
            is_default: device.is_default,
        })
        .collect())
}

/// Remembers the microphone on this PC (`None`: the system's default).
pub fn set_microphone_in(context: &AppContext, device: Option<String>) -> CommandResult<AppState> {
    let machine = store(context)?.machine().to_owned();
    let device = device_of(device.as_deref()).map(str::to_owned);
    update(context, |settings| {
        settings.set_microphone(&machine, device)
    })
}

#[tauri::command]
pub fn set_microphone(
    context: State<'_, AppContext>,
    device: Option<String>,
) -> CommandResult<AppState> {
    set_microphone_in(&context, device)
}

#[tauri::command]
pub async fn microphone_monitor(
    runtime: State<'_, NarrationRuntime>,
    device: Option<String>,
) -> CommandResult<()> {
    monitor_in(&runtime, device.as_deref())
}

/// Stops the level meter, never a recording.
#[tauri::command]
pub async fn microphone_monitor_stop(runtime: State<'_, NarrationRuntime>) -> CommandResult<()> {
    monitor_stop_in(&runtime);
    Ok(())
}

#[tauri::command]
pub async fn narration_start(
    app: AppHandle,
    context: State<'_, AppContext>,
    runtime: State<'_, NarrationRuntime>,
    live: State<'_, LiveRuntime>,
    device: Option<String>,
    target: NarrationTarget,
) -> CommandResult<()> {
    let watched = app.clone();
    let position = Arc::new(move || {
        watched
            .try_state::<LiveRuntime>()
            .and_then(|live| live.hub().page_position())
    });
    start_in(
        &runtime,
        device.as_deref(),
        target,
        || library_of(&context),
        live.hub(),
        position,
    )
}

#[tauri::command]
pub async fn narration_status(
    runtime: State<'_, NarrationRuntime>,
) -> CommandResult<NarrationStatus> {
    Ok(runtime.status())
}

#[tauri::command]
pub async fn narration_stop(
    app: AppHandle,
    save: bool,
    stamp: Option<FileStamp>,
    path: Option<String>,
    replay_started_at_ms: Option<f64>,
) -> CommandResult<Option<NarrationSaved>> {
    let request = StopRequest {
        save,
        stamp,
        path,
        replay_started_at_ms,
    };
    // Encoding a long recording takes a while: off the async runtime's threads.
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = app.state::<NarrationRuntime>();
        let live = app.state::<LiveRuntime>();
        stop_in(&runtime, &request, live.hub())
    })
    .await
    .map_err(|error| CommandError::new(MICROPHONE_FAILED, error))?
}

/// Saves the replay narration that could not be saved at stop into the note at `path`.
#[tauri::command]
pub async fn narration_retry(
    context: State<'_, AppContext>,
    runtime: State<'_, NarrationRuntime>,
    path: String,
    stamp: Option<FileStamp>,
) -> CommandResult<NarrationSaved> {
    let library = library_of(&context)?;
    retry_in(&runtime, &library, &path, stamp.as_ref())
}

/// Forgets the replay narration that could not be saved (its backup copy stays).
#[tauri::command]
pub async fn narration_discard(runtime: State<'_, NarrationRuntime>) -> CommandResult<()> {
    discard_in(&runtime);
    Ok(())
}

#[tauri::command]
pub async fn narration_info(
    context: State<'_, AppContext>,
    path: String,
) -> CommandResult<Option<NarrationInfo>> {
    narration_info_in(&library_of(&context)?, Path::new(&path))
}

/// The narration of a note as stored, an Ogg Opus stream (raw bytes: an `ArrayBuffer` in the
/// interface), read on a blocking thread.
#[tauri::command]
pub async fn narration_audio(
    context: State<'_, AppContext>,
    path: String,
) -> CommandResult<tauri::ipc::Response> {
    let library = library_of(&context)?;
    let opus = crate::notes::blocking(move || narration_opus(&library, Path::new(&path))).await?;
    Ok(tauri::ipc::Response::new(opus))
}

#[tauri::command]
pub async fn delete_narration(
    context: State<'_, AppContext>,
    path: String,
    stamp: FileStamp,
) -> CommandResult<FileStamp> {
    let library = library_of(&context)?;
    crate::notes::blocking(move || delete_narration_in(&library, Path::new(&path), &stamp)).await
}

#[cfg(test)]
mod tests;
