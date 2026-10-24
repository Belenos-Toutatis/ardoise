//! Narration at the microphone (SPEC §4.5): choosing the microphone, its level, recording
//! during a replay or in live mode, and the narration stored in a note (`audio.opus`).
//! Contract: `docs/design/media.md`, "Interface package: contracts" (shapes of
//! `app/ui/src/lib/video.ts`).
//!
//! - One capture at a time in the app ([`NarrationRuntime`]): a level-only monitor (settings,
//!   before recording) or a recording. Starting a recording stops a monitor first.
//! - A recording during a replay is in replay time (the replay's speed and trimming): at stop
//!   it replaces the narration of its note, through the library's checked save with the
//!   editor's stamp (a version changed on disk is a `noteConflict`, nothing is written).
//! - A recording in live mode is in note time: a watcher notes which page is shown every 50 ms;
//!   at stop the clip is cut per page ([`split_live_recording`]), each piece placed from its
//!   page's first stroke, merged with the page's narration and handed to the live hub, which
//!   writes it with the page (the hub is the only writer of live pages).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::Duration;

use ardoise_core::library::{FileStamp, Library, LibraryError};
use ardoise_media::microphone::{CaptureStatus, Level, Recorder, list_input_devices};
use ardoise_media::replay::{MAX_SPEED, MIN_SPEED, ReplayOptions};
use ardoise_media::{
    AudioClip, MediaError, NARRATION_SAMPLE_RATE, Narration, NarrationBase, decode_narration,
    encode_narration, wav_bytes,
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
/// Nothing was recorded, or the note has no narration.
pub const NO_RECORDING: &str = "noRecording";

/// How often the live watcher looks at the page shown.
pub const WATCH_INTERVAL: Duration = Duration::from_millis(50);

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
    pub fn of(narration: &Narration) -> Self {
        Self {
            duration_ms: duration_ms(narration.duration()),
            base: match narration.base {
                NarrationBase::Replay => BaseName::Replay,
                NarrationBase::Note => BaseName::Note,
            },
            speed: narration.options.speed,
            max_pause_ms: narration.options.max_pause.map(duration_ms),
            start_ms: narration.start_ms,
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

// --- Live recordings: which page was shown when -------------------------------------------------

/// From audio position `at` on, page `page` (0-based) was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageChange {
    pub at: Duration,
    pub page: usize,
}

/// What the live watcher saw during a recording.
#[derive(Debug, Clone, PartialEq)]
pub struct PageLog {
    /// The page shown from the start, then each change; positions strictly increase.
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
/// its first sample − the page's time origin (`origins[page]`: its first stroke, Unix ms).
/// `wall_start_ms` is the wall clock of the clip's first sample. Pieces of pages without any
/// stroke (`None`, or unknown pages) and empty pieces are left out. A page shown several times
/// gets several pieces, in order.
pub fn split_live_recording(
    clip: &AudioClip,
    changes: &[PageChange],
    wall_start_ms: f64,
    origins: &[Option<i64>],
) -> Vec<(usize, Narration)> {
    let channels = usize::from(clip.channels.max(1));
    let frames = clip.samples.len() / channels;
    let mut pieces = Vec::new();
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
        let Some(samples) = clip.samples.get(start * channels..end * channels) else {
            continue;
        };
        pieces.push((
            change.page,
            Narration {
                clip: AudioClip {
                    channels: clip.channels,
                    samples: samples.to_vec(),
                },
                base: NarrationBase::Note,
                options: ReplayOptions::default(),
                start_ms,
            },
        ));
    }
    pieces
}

/// The pieces of each page merged into one narration, pages in the order they first appear.
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

/// The page watcher of a live recording.
struct LiveWatch {
    session: u64,
    first_page: usize,
    /// Wall clock when the recording started (Unix ms), if no audio is ever seen.
    started_ms: f64,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<PageLog>>,
}

impl LiveWatch {
    /// Stops the watcher and returns what it saw.
    fn finish(&mut self) -> PageLog {
        self.stop.store(true, Ordering::Release);
        self.thread
            .take()
            .and_then(|thread| thread.join().ok())
            .unwrap_or_else(|| PageLog::new(self.first_page))
    }
}

impl Drop for LiveWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

// --- The capture of the app ---------------------------------------------------------------------

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
    Replay(ReplayTarget),
    Live(LiveWatch),
}

struct Capture {
    /// Shared with the live watcher, which reads the audio position.
    recorder: Arc<Recorder>,
    purpose: Purpose,
}

/// What a stopped capture was for.
enum Stopped {
    Monitor,
    Replay(ReplayTarget),
    Live {
        session: u64,
        log: PageLog,
        started_ms: f64,
    },
}

impl Capture {
    fn is_recording(&self) -> bool {
        !matches!(self.purpose, Purpose::Monitor)
    }

    fn is_live(&self) -> bool {
        matches!(self.purpose, Purpose::Live(_))
    }

    /// Stops the watcher, then the recorder.
    fn stop(self) -> (Stopped, Result<AudioClip, MediaError>) {
        let Capture { recorder, purpose } = self;
        let stopped = match purpose {
            Purpose::Monitor => Stopped::Monitor,
            Purpose::Replay(target) => Stopped::Replay(target),
            Purpose::Live(mut watch) => {
                let log = watch.finish();
                Stopped::Live {
                    session: watch.session,
                    log,
                    started_ms: watch.started_ms,
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

/// The microphone capture of the running app: one at a time.
#[derive(Default)]
pub struct NarrationRuntime {
    capture: Mutex<Option<Capture>>,
}

impl NarrationRuntime {
    fn lock(&self) -> MutexGuard<'_, Option<Capture>> {
        self.capture.lock().unwrap_or_else(PoisonError::into_inner)
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

    /// What the capture under way is doing.
    pub fn status(&self) -> NarrationStatus {
        let slot = self.lock();
        let Some(capture) = slot.as_ref() else {
            return NarrationStatus::idle();
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
        }
    }
}

/// A device id from the interface: `None` (or empty) for the system's default.
fn device_of(device: Option<&str>) -> Option<&str> {
    device.map(str::trim).filter(|d| !d.is_empty())
}

/// Opens the microphone (`record`: keep the audio, else only the level).
fn open(device: Option<&str>, record: bool) -> CommandResult<Recorder> {
    let device = device_of(device);
    let opened = if record {
        Recorder::start(device)
    } else {
        Recorder::monitor(device)
    };
    opened.map_err(|error| start_error(error, device.is_none() && has_no_microphone()))
}

/// Starts the level meter (replacing a monitor; refused while recording).
pub fn monitor_in(runtime: &NarrationRuntime, device: Option<&str>) -> CommandResult<()> {
    let mut slot = runtime.lock();
    if slot.as_ref().is_some_and(Capture::is_recording) {
        return Err(busy());
    }
    // The previous monitor stops before the device is opened again.
    *slot = None;
    let recorder = open(device, false)?;
    *slot = Some(Capture {
        recorder: Arc::new(recorder),
        purpose: Purpose::Monitor,
    });
    Ok(())
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

/// A replay instant from the interface, in whole milliseconds.
fn target_start(start_ms: f64) -> CommandResult<i64> {
    if start_ms.is_finite() && start_ms.abs() < 1e15 {
        Ok(start_ms.round() as i64)
    } else {
        Err(CommandError::new(
            INVALID_VIDEO_SETTINGS,
            format!("replay instant {start_ms} ms"),
        ))
    }
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
    position: Arc<dyn Fn() -> Option<PagePosition> + Send + Sync>,
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
    let recorder = Arc::new(open(device, true)?);
    let purpose = match planned {
        Planned::Replay(target) => Purpose::Replay(target),
        Planned::Live(start) => {
            let stop = Arc::new(AtomicBool::new(false));
            let started_ms = wall_now_ms();
            let thread = {
                let (stop, recorder) = (Arc::clone(&stop), Arc::clone(&recorder));
                std::thread::Builder::new()
                    .name("narration-pages".into())
                    .spawn(move || {
                        watch_pages(
                            &stop,
                            WATCH_INTERVAL,
                            start.session,
                            start.page,
                            &|| recorder.elapsed(),
                            &*position,
                            &wall_now_ms,
                        )
                    })
                    .map_err(|error| CommandError::new(MICROPHONE_FAILED, error))?
            };
            Purpose::Live(LiveWatch {
                session: start.session,
                first_page: start.page,
                started_ms,
                stop,
                thread: Some(thread),
            })
        }
    };
    *slot = Some(Capture { recorder, purpose });
    Ok(())
}

/// Writes `narration` as the `audio.opus` of the note at `path` (replacing any), unless the
/// file is no longer the version of `expected` (`noteConflict`, nothing written). Everything
/// else in the file is kept. Returns the new stamp.
pub fn save_replay_narration(
    library: &Library,
    path: &Path,
    narration: &Narration,
    expected: Option<&FileStamp>,
) -> CommandResult<FileStamp> {
    let loaded = library.read(path).map_err(library_error)?;
    if expected.is_some_and(|expected| *expected != loaded.stamp) {
        return Err(library_error(LibraryError::Conflict { path: loaded.path }));
    }
    let audio = encode_narration(narration).map_err(narration_error)?;
    let mut file = loaded.file;
    file.audio = Some(audio);
    file.note.modified = now_ms().max(file.note.modified);
    library
        .save(&loaded.path, &file, &loaded.stamp)
        .map_err(library_error)
}

/// Removes the narration of the note at `path` (same checks as [`save_replay_narration`]).
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

/// The narration stored in the note at `path`, if any.
pub fn read_narration(library: &Library, path: &Path) -> CommandResult<Option<Narration>> {
    let loaded = library.read(path).map_err(library_error)?;
    loaded
        .file
        .audio
        .map(|bytes| decode_narration(&bytes).map_err(damaged))
        .transpose()
}

/// Hands the pieces of a live recording to the hub; returns how many pages got narration.
pub fn give_to_hub(
    hub: &LiveHub,
    session: u64,
    log: &PageLog,
    wall_start_ms: f64,
    clip: &AudioClip,
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

/// Saves (or, without `save`, discards) what a stopped capture recorded.
fn save_stopped(
    stopped: Stopped,
    clip: Result<AudioClip, MediaError>,
    save: bool,
    stamp: Option<&FileStamp>,
    hub: &LiveHub,
) -> CommandResult<Option<NarrationSaved>> {
    if matches!(stopped, Stopped::Monitor) || !save {
        return Ok(None);
    }
    let clip = clip.map_err(narration_error)?;
    if clip.samples.is_empty() {
        return Err(CommandError::new(NO_RECORDING, "nothing was recorded"));
    }
    let duration = duration_ms(clip.duration());
    match stopped {
        Stopped::Monitor => Ok(None),
        Stopped::Replay(target) => {
            let narration = Narration {
                clip,
                base: NarrationBase::Replay,
                options: target.options,
                start_ms: target.start_ms,
            };
            let stamp = save_replay_narration(&target.library, &target.path, &narration, stamp)?;
            Ok(Some(NarrationSaved {
                stamp: Some(stamp),
                duration_ms: duration,
                pages: 1,
            }))
        }
        Stopped::Live {
            session,
            log,
            started_ms,
        } => {
            let wall_start = log.wall_start_ms(started_ms);
            let pages = give_to_hub(hub, session, &log, wall_start, &clip)?;
            Ok(Some(NarrationSaved {
                stamp: None,
                duration_ms: duration,
                pages,
            }))
        }
    }
}

/// Stops the capture under way and, with `save`, keeps what it recorded (see the module
/// documentation). Nothing under way: `noRecording` with `save`, else nothing.
pub fn stop_in(
    runtime: &NarrationRuntime,
    save: bool,
    stamp: Option<&FileStamp>,
    hub: &LiveHub,
) -> CommandResult<Option<NarrationSaved>> {
    // The capture leaves the runtime first: stopping it (joining threads) holds no lock.
    let Some(capture) = runtime.take(false) else {
        return if save {
            Err(CommandError::new(
                NO_RECORDING,
                "no narration is being recorded",
            ))
        } else {
            Ok(None)
        };
    };
    let (stopped, clip) = capture.stop();
    save_stopped(stopped, clip, save, stamp, hub)
}

/// Stops a live recording under way and gives it to its pages (before live mode stops, or when
/// the app closes); `None` when no live recording runs.
pub fn finish_live(
    runtime: &NarrationRuntime,
    hub: &LiveHub,
) -> Option<CommandResult<NarrationSaved>> {
    let capture = runtime.take(true)?;
    let (stopped, clip) = capture.stop();
    save_stopped(stopped, clip, true, None, hub).transpose()
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
) -> CommandResult<Option<NarrationSaved>> {
    // Encoding a long recording takes a while: off the async runtime's threads.
    tauri::async_runtime::spawn_blocking(move || {
        let runtime = app.state::<NarrationRuntime>();
        let live = app.state::<LiveRuntime>();
        stop_in(&runtime, save, stamp.as_ref(), live.hub())
    })
    .await
    .map_err(|error| CommandError::new(MICROPHONE_FAILED, error))?
}

#[tauri::command]
pub async fn narration_info(
    context: State<'_, AppContext>,
    path: String,
) -> CommandResult<Option<NarrationInfo>> {
    let library = library_of(&context)?;
    Ok(read_narration(&library, Path::new(&path))?
        .as_ref()
        .map(NarrationInfo::of))
}

/// The narration of a note as a WAV file (raw bytes: an `ArrayBuffer` in the interface).
#[tauri::command]
pub async fn narration_audio(
    context: State<'_, AppContext>,
    path: String,
) -> CommandResult<tauri::ipc::Response> {
    let library = library_of(&context)?;
    let narration = read_narration(&library, Path::new(&path))?
        .ok_or_else(|| CommandError::new(NO_RECORDING, "this note has no narration"))?;
    Ok(tauri::ipc::Response::new(wav_bytes(&narration.clip)))
}

#[tauri::command]
pub async fn delete_narration(
    context: State<'_, AppContext>,
    path: String,
    stamp: FileStamp,
) -> CommandResult<FileStamp> {
    delete_narration_in(&library_of(&context)?, Path::new(&path), &stamp)
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::fs;

    use ardoise_media::microphone::simulated_narration;
    use serde_json::json;

    use super::*;
    use crate::library::tests::{context, library, write_sample};
    use crate::live::HubConfig;

    fn tone(frames: usize, channels: u16) -> AudioClip {
        AudioClip {
            channels,
            samples: (0..frames * usize::from(channels))
                .map(|i| i16::try_from(i % 30_000).unwrap() + 1)
                .collect(),
        }
    }

    fn change(at_ms: u64, page: usize) -> PageChange {
        PageChange {
            at: Duration::from_millis(at_ms),
            page,
        }
    }

    #[test]
    fn shapes_are_those_of_video_ts() {
        let replay: NarrationTarget = serde_json::from_value(json!({
            "kind": "replay", "path": "C:\\N\\a.ardoise", "speed": 2,
            "maxPauseMs": 500, "startMs": -120.5
        }))
        .unwrap();
        assert_eq!(
            replay,
            NarrationTarget::Replay {
                path: "C:\\N\\a.ardoise".into(),
                speed: 2.0,
                max_pause_ms: Some(500.0),
                start_ms: -120.5,
            }
        );
        let untrimmed: NarrationTarget = serde_json::from_value(json!({
            "kind": "replay", "path": "a", "speed": 1, "maxPauseMs": null, "startMs": 0
        }))
        .unwrap();
        assert!(matches!(
            untrimmed,
            NarrationTarget::Replay {
                max_pause_ms: None,
                ..
            }
        ));
        let live: NarrationTarget = serde_json::from_value(json!({ "kind": "live" })).unwrap();
        assert_eq!(live, NarrationTarget::Live);

        assert_eq!(
            serde_json::to_value(NarrationStatus::idle()).unwrap(),
            json!({ "state": "idle", "capture": null, "level": { "peak": 0.0, "rms": 0.0 },
                    "elapsedMs": 0, "device": null, "failure": null })
        );
        let recording = NarrationStatus {
            state: NarrationState::Recording,
            capture: Some(CaptureState::NoData),
            level: LevelState {
                peak: 0.5,
                rms: 0.25,
            },
            elapsed_ms: 1500,
            device: Some("Micro".into()),
            failure: None,
        };
        let value = serde_json::to_value(recording).unwrap();
        assert_eq!(value["state"], "recording");
        assert_eq!(value["capture"], "noData");
        assert_eq!(value["level"], json!({ "peak": 0.5, "rms": 0.25 }));
        assert_eq!(value["elapsedMs"], 1500);
        assert_eq!(
            serde_json::to_value(NarrationState::Monitoring).unwrap(),
            "monitoring"
        );

        let saved = NarrationSaved {
            stamp: None,
            duration_ms: 3000,
            pages: 2,
        };
        assert_eq!(
            serde_json::to_value(saved).unwrap(),
            json!({ "stamp": null, "durationMs": 3000, "pages": 2 })
        );
        let info = NarrationInfo::of(&Narration {
            clip: tone(48_000, 1),
            base: NarrationBase::Replay,
            options: ReplayOptions {
                speed: 2.0,
                max_pause: Some(Duration::from_millis(500)),
            },
            start_ms: -40,
        });
        assert_eq!(
            serde_json::to_value(info).unwrap(),
            json!({ "durationMs": 1000, "base": "replay", "speed": 2.0, "maxPauseMs": 500,
                    "startMs": -40 })
        );
        let note = NarrationInfo::of(&Narration {
            clip: tone(4_800, 1),
            base: NarrationBase::Note,
            options: ReplayOptions::default(),
            start_ms: 7,
        });
        assert_eq!(note.base, BaseName::Note);
        assert_eq!(
            serde_json::to_value(note).unwrap()["maxPauseMs"],
            json!(null)
        );
        let microphone = Microphone {
            id: "wasapi:1".into(),
            name: "Micro".into(),
            is_default: true,
        };
        assert_eq!(
            serde_json::to_value(microphone).unwrap(),
            json!({ "id": "wasapi:1", "name": "Micro", "isDefault": true })
        );
    }

    #[test]
    fn capture_statuses_and_levels_map_to_the_interface() {
        assert_eq!(
            capture_state(&CaptureStatus::Waiting),
            (CaptureState::Waiting, None)
        );
        assert_eq!(
            capture_state(&CaptureStatus::NoData),
            (CaptureState::NoData, None)
        );
        assert_eq!(
            capture_state(&CaptureStatus::Receiving),
            (CaptureState::Receiving, None)
        );
        assert_eq!(
            capture_state(&CaptureStatus::Silent),
            (CaptureState::Silent, None)
        );
        assert_eq!(
            capture_state(&CaptureStatus::Failed("unplugged".into())),
            (CaptureState::Failed, Some("unplugged".into()))
        );
        for (state, text) in [
            (CaptureState::Waiting, "waiting"),
            (CaptureState::NoData, "noData"),
            (CaptureState::Receiving, "receiving"),
            (CaptureState::Silent, "silent"),
            (CaptureState::Failed, "failed"),
        ] {
            assert_eq!(serde_json::to_value(state).unwrap(), text);
        }
        assert_eq!(
            level_state(Level {
                peak: 1.5,
                rms: f32::NAN
            }),
            LevelState {
                peak: 1.0,
                rms: 0.0
            }
        );
        assert_eq!(
            level_state(Level {
                peak: 0.3,
                rms: 0.1
            }),
            LevelState {
                peak: 0.3,
                rms: 0.1
            }
        );
    }

    #[test]
    fn microphone_errors_get_the_interface_codes() {
        let code = |error, no_device| start_error(error, no_device).code();
        assert_eq!(
            code(MediaError::MicrophoneNotFound("Micro USB".into()), false),
            MICROPHONE_NOT_FOUND
        );
        assert_eq!(code(MediaError::MicrophoneSilent, false), MICROPHONE_SILENT);
        let failed = || MediaError::Microphone {
            step: "opening the microphone",
            message: "this PC has no microphone".into(),
        };
        assert_eq!(code(failed(), false), MICROPHONE_FAILED);
        assert_eq!(code(failed(), true), NO_MICROPHONE);
        // An unknown device is not "no microphone at all".
        assert_eq!(
            code(MediaError::MicrophoneNotFound("x".into()), true),
            MICROPHONE_NOT_FOUND
        );
        assert_eq!(
            narration_error(MediaError::Encoding {
                codec: "Opus",
                message: "x".into()
            })
            .code(),
            MICROPHONE_FAILED
        );
        assert_eq!(
            narration_error(MediaError::MicrophoneSilent).code(),
            MICROPHONE_SILENT
        );
    }

    #[test]
    fn replay_targets_are_checked() {
        assert_eq!(
            target_options(2.0, Some(500.0)).unwrap(),
            ReplayOptions {
                speed: 2.0,
                max_pause: Some(Duration::from_millis(500))
            }
        );
        assert_eq!(
            target_options(0.25, None).unwrap(),
            ReplayOptions {
                speed: 0.25,
                max_pause: None
            }
        );
        for (speed, pause) in [
            (0.2, None),
            (16.5, None),
            (f64::NAN, None),
            (1.0, Some(-1.0)),
            (1.0, Some(f64::INFINITY)),
        ] {
            assert_eq!(
                target_options(speed, pause).unwrap_err().code(),
                INVALID_VIDEO_SETTINGS,
                "{speed} {pause:?}"
            );
        }
        assert_eq!(target_start(-120.4).unwrap(), -120);
        assert!(target_start(f64::NAN).is_err());

        let (dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let path = saved.path.to_string_lossy().into_owned();
        let target = replay_target(library.clone(), &path, 1.0, None, 250.0).unwrap();
        assert_eq!(target.start_ms, 250);
        let missing = dir.path().join("2026-10-24 Absente.ardoise");
        let error =
            replay_target(library.clone(), &missing.to_string_lossy(), 1.0, None, 0.0).unwrap_err();
        assert_eq!(error.code(), "noteNotFound");
        let elsewhere = tempfile::tempdir().unwrap();
        let outside = elsewhere.path().join("2026-10-24 X.ardoise");
        fs::write(&outside, "x").unwrap();
        let error = replay_target(library, &outside.to_string_lossy(), 1.0, None, 0.0).unwrap_err();
        assert_eq!(error.code(), "notInLibrary");
    }

    #[test]
    fn the_page_log_keeps_the_changes_and_the_earliest_clock() {
        let mut log = PageLog::new(2);
        // No audio yet: no clock, but a page change counts from the start.
        log.observe(1_000.0, Duration::ZERO, Some(2));
        assert_eq!(log.anchor_ms, None);
        assert_eq!(log.wall_start_ms(990.0), 990.0);
        log.observe(1_010.0, Duration::ZERO, Some(3));
        assert_eq!(log.changes, [change(0, 3)]);
        // Audio arrives late (delivery delay): the smallest difference wins.
        log.observe(1_100.0, Duration::from_millis(60), Some(3));
        log.observe(1_150.0, Duration::from_millis(130), Some(3));
        log.observe(1_200.0, Duration::from_millis(150), None);
        assert_eq!(log.anchor_ms, Some(1_020.0));
        assert_eq!(log.wall_start_ms(0.0), 1_020.0);
        log.observe(1_250.0, Duration::from_millis(200), Some(4));
        log.observe(1_300.0, Duration::from_millis(250), Some(4));
        log.observe(1_310.0, Duration::from_millis(260), Some(5));
        // Changes before new audio: the last one counts, and back to page 4 is no change.
        log.observe(1_320.0, Duration::from_millis(260), Some(6));
        assert_eq!(log.changes, [change(0, 3), change(200, 4), change(260, 6)]);
        log.observe(1_330.0, Duration::from_millis(260), Some(4));
        log.observe(1_400.0, Duration::from_millis(330), Some(3));
        assert_eq!(log.changes, [change(0, 3), change(200, 4), change(330, 3)]);
        assert_eq!(log.anchor_ms, Some(1_020.0));
    }

    #[test]
    fn a_live_recording_is_cut_per_page_in_note_time() {
        // 3 s of mono audio; pages 0, 1, 0 again, then 2 (no stroke) and 3.
        let clip = tone(3 * 48_000, 1);
        let changes = [
            change(0, 0),
            change(1000, 1),
            change(1500, 0),
            change(2000, 2),
            change(2500, 3),
        ];
        let wall_start = 1_792_843_200_000.0;
        let origins = [
            Some(1_792_843_200_400),
            Some(1_792_843_190_000),
            None,
            Some(1_792_843_203_000),
        ];
        let pieces = split_live_recording(&clip, &changes, wall_start, &origins);
        let summary: Vec<(usize, i64, usize)> = pieces
            .iter()
            .map(|(page, n)| (*page, n.start_ms, n.clip.samples.len()))
            .collect();
        assert_eq!(
            summary,
            [
                // Begins 400 ms before the first stroke of page 0.
                (0, -400, 48_000),
                (1, 11_000, 24_000),
                (0, 1_100, 24_000),
                // Page 2 has no stroke: nothing. Page 3's first stroke comes after the end.
                (3, -500, 24_000),
            ]
        );
        for (_, narration) in &pieces {
            assert_eq!(narration.base, NarrationBase::Note);
            assert_eq!(narration.options, ReplayOptions::default());
            assert_eq!(narration.clip.channels, 1);
        }
        // The pieces put back together are the clip, minus what went to page 2.
        let kept: Vec<i16> = pieces
            .iter()
            .flat_map(|(_, n)| n.clip.samples.clone())
            .collect();
        let mut expected = clip.samples[..96_000].to_vec();
        expected.extend_from_slice(&clip.samples[120_000..]);
        assert_eq!(kept, expected);
    }

    #[test]
    fn cuts_fall_on_whole_frames_and_out_of_range_changes_are_ignored() {
        // Stereo: a cut never separates the two channels of a frame.
        let clip = tone(4_800, 2);
        let changes = [change(0, 0), change(33, 1), change(500, 0)];
        let pieces = split_live_recording(&clip, &changes, 1_000.0, &[Some(1_000), Some(900)]);
        assert_eq!(pieces.len(), 2);
        assert_eq!(pieces[0].1.clip.samples.len(), 2 * 1_584);
        assert_eq!(pieces[1].1.clip.samples.len(), 2 * (4_800 - 1_584));
        assert_eq!(pieces[1].1.start_ms, 133);
        assert_eq!(pieces[0].1.clip.channels, 2);
        // A page the hub does not know, an empty clip.
        assert!(split_live_recording(&clip, &[change(0, 7)], 0.0, &[Some(0)]).is_empty());
        assert!(
            split_live_recording(&AudioClip::default(), &[change(0, 0)], 0.0, &[Some(0)])
                .is_empty()
        );
    }

    #[test]
    fn the_watcher_follows_the_pages_of_its_session() {
        let stop = AtomicBool::new(false);
        let tick = Cell::new(0u64);
        // Ticks of 50 ms of audio; the page changes at ticks 3 and 6, another session at 8.
        let position = || {
            let t = tick.get();
            let (session, page) = match t {
                0..=2 => (7, 0),
                3..=5 => (7, 1),
                6..=7 => (7, 2),
                _ => (8, 0),
            };
            Some(PagePosition {
                session,
                page,
                running: true,
            })
        };
        let audio = || Duration::from_millis(50 * tick.get());
        let seen = RefCell::new(0);
        let wall = || {
            let t = tick.get();
            *seen.borrow_mut() += 1;
            if t == 9 {
                stop.store(true, Ordering::Release);
            }
            tick.set(t + 1);
            5_000.0 + 50.0 * t as f64 + 3.0
        };
        let log = watch_pages(&stop, Duration::ZERO, 7, 0, &audio, &position, &wall);
        assert_eq!(log.changes, [change(0, 0), change(150, 1), change(300, 2)]);
        assert_eq!(log.anchor_ms, Some(5_003.0));
        assert_eq!(*seen.borrow(), 11, "one last look after the stop");
    }

    #[test]
    fn nothing_to_stop_and_nothing_running() {
        let runtime = NarrationRuntime::default();
        let hub = LiveHub::new(HubConfig::default());
        assert_eq!(runtime.status(), NarrationStatus::idle());
        assert!(!runtime.is_recording_live());
        assert_eq!(stop_in(&runtime, false, None, &hub).unwrap(), None);
        assert_eq!(
            stop_in(&runtime, true, None, &hub).unwrap_err().code(),
            NO_RECORDING
        );
        assert!(finish_live(&runtime, &hub).is_none());
        // A live recording needs live mode (checked before the microphone is opened).
        let error = start_in(
            &runtime,
            None,
            NarrationTarget::Live,
            || Err(CommandError::new("noLibrary", "unused")),
            &hub,
            Arc::new(|| None),
        )
        .unwrap_err();
        assert_eq!(error.code(), "liveNotRunning");
        // A replay recording needs a library and a valid target.
        let error = start_in(
            &runtime,
            None,
            NarrationTarget::Replay {
                path: "a.ardoise".into(),
                speed: 1.0,
                max_pause_ms: None,
                start_ms: 0.0,
            },
            || Err(CommandError::new("noLibrary", "none")),
            &hub,
            Arc::new(|| None),
        )
        .unwrap_err();
        assert_eq!(error.code(), "noLibrary");
        assert_eq!(runtime.status(), NarrationStatus::idle());
    }

    #[test]
    fn a_stale_stamp_is_a_conflict_and_writes_nothing() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        // The note changes on disk after the editor opened it.
        let mut file = library.read(&saved.path).unwrap().file;
        file.note.title = "Changée ailleurs".into();
        let newer = library.save(&saved.path, &file, &saved.stamp).unwrap();
        let before = fs::read(&saved.path).unwrap();
        let narration = Narration {
            clip: tone(4_800, 1),
            base: NarrationBase::Replay,
            options: ReplayOptions::default(),
            start_ms: 0,
        };
        let error = save_replay_narration(&library, &saved.path, &narration, Some(&saved.stamp))
            .unwrap_err();
        assert_eq!(error.code(), "noteConflict");
        let error = delete_narration_in(&library, &saved.path, &saved.stamp).unwrap_err();
        assert_eq!(error.code(), "noteConflict");
        assert_eq!(fs::read(&saved.path).unwrap(), before);
        assert_eq!(FileStamp::of(&saved.path).unwrap(), newer);

        // Without narration, deleting it changes nothing.
        assert_eq!(
            delete_narration_in(&library, &saved.path, &newer).unwrap(),
            newer
        );
        assert_eq!(read_narration(&library, &saved.path).unwrap(), None);
        // A damaged member is reported as such.
        file.audio = Some(b"OggS but not really".to_vec());
        let damaged = library.save(&saved.path, &file, &newer).unwrap();
        assert_eq!(
            read_narration(&library, &saved.path).unwrap_err().code(),
            "noteDamaged"
        );
        // Deleting it works without decoding it.
        let stamp = delete_narration_in(&library, &saved.path, &damaged).unwrap();
        let after = library.read(&saved.path).unwrap();
        assert_eq!(after.stamp, stamp);
        assert_eq!(after.file.audio, None);
        assert_eq!(after.file.note.title, "Changée ailleurs");
        assert_eq!(after.file.note.strokes, file.note.strokes);
    }

    #[test]
    fn the_microphone_is_remembered_per_machine() {
        let settings = tempfile::tempdir().unwrap();
        let context = context(settings.path());
        let state = set_microphone_in(&context, Some("wasapi:{abc}".into())).unwrap();
        assert_eq!(state.microphone(), Some("wasapi:{abc}"));
        let state = set_microphone_in(&context, Some("  ".into())).unwrap();
        assert_eq!(state.microphone(), None);
        let state = set_microphone_in(&context, None).unwrap();
        assert_eq!(state.microphone(), None);
    }

    // --- With the media pipeline (narration.rs of ardoise-media: Opus encoding) ---

    #[test]
    fn a_replay_narration_is_saved_into_its_note_and_read_back() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let before = library.read(&saved.path).unwrap().file;
        let narration = Narration {
            clip: simulated_narration(Duration::from_secs(2), 1),
            base: NarrationBase::Replay,
            options: ReplayOptions {
                speed: 2.0,
                max_pause: Some(Duration::from_millis(500)),
            },
            start_ms: 300,
        };
        let stamp =
            save_replay_narration(&library, &saved.path, &narration, Some(&saved.stamp)).unwrap();
        let after = library.read(&saved.path).unwrap();
        assert_eq!(after.stamp, stamp);
        assert_eq!(after.file.note.strokes, before.note.strokes);
        assert_eq!(after.file.thumbnail, before.thumbnail);
        let info = NarrationInfo::of(&read_narration(&library, &saved.path).unwrap().unwrap());
        assert_eq!(info.base, BaseName::Replay);
        assert_eq!(
            (info.speed, info.max_pause_ms, info.start_ms),
            (2.0, Some(500), 300)
        );
        assert!((1990..=2010).contains(&info.duration_ms), "{info:?}");
        let wav = wav_bytes(&read_narration(&library, &saved.path).unwrap().unwrap().clip);
        assert_eq!(&wav[..4], b"RIFF");

        // Recording again replaces it; deleting it removes it.
        let shorter = Narration {
            clip: simulated_narration(Duration::from_secs(1), 1),
            ..narration
        };
        let stamp = save_replay_narration(&library, &saved.path, &shorter, Some(&stamp)).unwrap();
        let info = NarrationInfo::of(&read_narration(&library, &saved.path).unwrap().unwrap());
        assert!(info.duration_ms < 1100, "{info:?}");
        delete_narration_in(&library, &saved.path, &stamp).unwrap();
        assert_eq!(read_narration(&library, &saved.path).unwrap(), None);
    }
}
