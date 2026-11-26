//! Recording: a worker thread owns the `cpal` stream and consumes what its callback sends
//! through a channel. The consuming loop ([`run_capture`]) knows nothing about `cpal`, so it
//! is tested with synthetic events.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::device;
use super::level::Level;
use super::pipeline::Pipeline;
use crate::{AudioClip, MediaError};

/// How often the worker wakes up to look at the stop flag when no audio arrives.
const POLL: Duration = Duration::from_millis(50);

/// A stream that has delivered nothing for this long (since its start, or since its last frame)
/// is reported as [`CaptureStatus::NoData`]. A silence is no such thing: it delivers frames.
const NO_DATA_AFTER: Duration = Duration::from_secs(3);

/// What the `cpal` callbacks send to the worker.
pub(crate) enum CaptureEvent {
    /// Interleaved samples, converted to `f32`.
    Samples(Vec<f32>),
    /// The stream failed for good (device unplugged, driver reset).
    Failed(String),
}

/// What the device delivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CaptureFormat {
    pub sample_rate: u32,
    pub channels: u16,
}

/// What a recorder is doing, for the interface to poll (about 20 times a second).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureStatus {
    /// The stream was started and no audio has arrived yet.
    Waiting,
    /// No audio for several seconds, after the start or since the last audio: the device is
    /// stuck.
    NoData,
    /// Audio is arriving.
    Receiving,
    /// Audio arrives but the last second or more is exact digital silence. On Windows this
    /// is the microphone privacy setting blocking desktop apps (or a hardware mute): the
    /// interface should explain Settings > Privacy & security > Microphone > "Let desktop
    /// apps access your microphone". A quiet room is not this state. It clears by itself as
    /// soon as a non-zero sample arrives.
    Silent,
    /// The stream failed (device unplugged...); the message is for the log.
    Failed(String),
    /// The recording reached the longest narration
    /// ([`MAX_RECORDING_FRAMES`](super::MAX_RECORDING_FRAMES)): the capture stopped by itself and
    /// [`Recorder::stop`] returns what it recorded, as after any stop.
    Full,
}

#[derive(Default)]
struct State {
    level: Level,
    elapsed: Duration,
    frames: u64,
    /// When the last frame arrived.
    last_frame: Option<Instant>,
    silent: bool,
    full: bool,
    failure: Option<String>,
}

impl State {
    /// See [`CaptureStatus`]: what a capture started at `started` is doing at `now`.
    fn status(&self, started: Instant, now: Instant) -> CaptureStatus {
        if let Some(failure) = &self.failure {
            CaptureStatus::Failed(failure.clone())
        } else if self.full {
            CaptureStatus::Full
        } else if self.stalled(started, now) {
            CaptureStatus::NoData
        } else if self.frames == 0 {
            CaptureStatus::Waiting
        } else if self.silent {
            CaptureStatus::Silent
        } else {
            CaptureStatus::Receiving
        }
    }

    /// The level shown: zero once nothing arrives any more.
    fn level(&self, started: Instant, now: Instant) -> Level {
        if self.stalled(started, now) {
            Level::default()
        } else {
            self.level
        }
    }

    /// Nothing arrived for [`NO_DATA_AFTER`], since the start or since the last frame.
    fn stalled(&self, started: Instant, now: Instant) -> bool {
        now.saturating_duration_since(self.last_frame.unwrap_or(started)) > NO_DATA_AFTER
    }
}

/// State shared between the worker and the [`Recorder`] handle.
#[derive(Default)]
pub(crate) struct Shared {
    state: Mutex<State>,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn update(&self, pipeline: &Pipeline) {
        let mut state = self.lock();
        let frames = pipeline.frames_received();
        if frames != state.frames {
            state.last_frame = Some(Instant::now());
        }
        state.full = pipeline.is_full();
        // The meter falls to zero with a capture that ended.
        state.level = if state.full {
            Level::default()
        } else {
            pipeline.level()
        };
        state.elapsed = pipeline.elapsed();
        state.frames = frames;
        state.silent = pipeline.is_silent_now();
    }

    fn fail(&self, message: String) {
        let mut state = self.lock();
        state.failure = Some(message);
        state.level = Level::default();
    }
}

/// What the worker returns when it stops.
pub(crate) struct CaptureOutcome {
    pub clip: AudioClip,
    /// Everything received was exact digital silence, for at least a second.
    pub all_silent: bool,
    /// The stream failed during the recording.
    pub failure: Option<String>,
}

/// The consuming loop: reads events until `stop` is set (then drains what is already queued),
/// the channel closes, the stream fails, or a recording reaches its limit.
pub(crate) fn run_capture(
    rx: &Receiver<CaptureEvent>,
    format: CaptureFormat,
    record: bool,
    stop: &AtomicBool,
    shared: &Shared,
) -> Result<CaptureOutcome, MediaError> {
    let pipeline = Pipeline::new(format.sample_rate, format.channels, record)?;
    consume(rx, pipeline, stop, shared)
}

/// [`run_capture`] into `pipeline`.
fn consume(
    rx: &Receiver<CaptureEvent>,
    mut pipeline: Pipeline,
    stop: &AtomicBool,
    shared: &Shared,
) -> Result<CaptureOutcome, MediaError> {
    let mut failure = None;
    loop {
        let event = if stop.load(Ordering::Acquire) {
            match rx.try_recv() {
                Ok(event) => Some(event),
                Err(_) => break,
            }
        } else {
            match rx.recv_timeout(POLL) {
                Ok(event) => Some(event),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        };
        match event {
            Some(CaptureEvent::Samples(samples)) => {
                if let Err(e) = pipeline.push(&samples) {
                    shared.fail(e.to_string());
                    return Err(e);
                }
                shared.update(&pipeline);
                // The longest narration: the capture ends (the device closes) and keeps what
                // it has, as at a stop.
                if pipeline.is_full() {
                    break;
                }
            }
            Some(CaptureEvent::Failed(message)) => {
                shared.fail(message.clone());
                failure = Some(message);
                break;
            }
            None => {}
        }
    }
    let all_silent = pipeline.is_all_silent();
    let clip = pipeline.finish()?;
    Ok(CaptureOutcome {
        clip,
        all_silent,
        failure,
    })
}

/// A running capture from a microphone: either a recording ([`Recorder::start`]) or a
/// level-only monitor ([`Recorder::monitor`]) for the settings screen.
///
/// The recording is converted on the fly to what narration needs: **48 kHz, mono, 16-bit**
/// (a voice does not need stereo, and a stereo microphone is averaged to one channel).
/// Dropping the recorder stops the capture and discards the audio.
///
/// Memory: 48 kHz mono 16-bit is 96 kB per second, so an hour is about 350 MB. A recording stops
/// by itself at [`MAX_RECORDING_FRAMES`](super::MAX_RECORDING_FRAMES) (just under 4 hours).
pub struct Recorder {
    shared: Arc<Shared>,
    stop: Arc<AtomicBool>,
    worker: Option<JoinHandle<Result<CaptureOutcome, MediaError>>>,
    device_name: String,
    record: bool,
    started: Instant,
}

impl Recorder {
    /// Starts recording from the device with this id or name (see
    /// [`list_input_devices`](super::list_input_devices)), or from the default input device if
    /// `device` is `None`. Returns once the stream runs; fails with
    /// [`MediaError::MicrophoneNotFound`] for an unknown device and
    /// [`MediaError::Microphone`] if it cannot be opened.
    pub fn start(device: Option<&str>) -> Result<Self, MediaError> {
        Self::spawn(device, true)
    }

    /// Like [`Recorder::start`] but keeps no audio: only the level and the status are
    /// updated, so the interface can show a meter while the user picks a microphone.
    /// [`Recorder::stop`] then returns an empty clip.
    pub fn monitor(device: Option<&str>) -> Result<Self, MediaError> {
        Self::spawn(device, false)
    }

    fn spawn(device: Option<&str>, record: bool) -> Result<Self, MediaError> {
        let shared = Arc::new(Shared::default());
        let stop = Arc::new(AtomicBool::new(false));
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<String, MediaError>>(1);
        let selector = device.map(str::to_owned);

        let worker = {
            let shared = Arc::clone(&shared);
            let stop = Arc::clone(&stop);
            std::thread::Builder::new()
                .name("ardoise-microphone".into())
                .spawn(move || {
                    let (tx, rx) = mpsc::channel();
                    // The stream is created, played and dropped on this thread: on some
                    // platforms it must not move between threads.
                    let opened = match device::open_input(selector.as_deref(), tx) {
                        Ok(opened) => opened,
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                            return Ok(CaptureOutcome {
                                clip: AudioClip::default(),
                                all_silent: false,
                                failure: None,
                            });
                        }
                    };
                    let _ = ready_tx.send(Ok(opened.name.clone()));
                    let outcome = run_capture(&rx, opened.format, record, &stop, &shared);
                    drop(opened.stream);
                    outcome
                })
                .map_err(|e| MediaError::Microphone {
                    step: "starting the capture thread",
                    message: e.to_string(),
                })?
        };

        match ready_rx.recv() {
            Ok(Ok(device_name)) => Ok(Self {
                shared,
                stop,
                worker: Some(worker),
                device_name,
                record,
                started: Instant::now(),
            }),
            Ok(Err(e)) => {
                let _ = worker.join();
                Err(e)
            }
            Err(_) => Err(MediaError::Microphone {
                step: "opening the microphone",
                message: "the capture thread ended unexpectedly".into(),
            }),
        }
    }

    /// Name of the device being recorded.
    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Loudness over the last completed 50 ms window (see [`Level`]); zero until the first, and
    /// once the capture failed, ended at its limit or no longer receives anything.
    pub fn level(&self) -> Level {
        self.shared.lock().level(self.started, Instant::now())
    }

    /// Audio time received so far (counted in samples, not wall-clock).
    pub fn elapsed(&self) -> Duration {
        self.shared.lock().elapsed
    }

    /// See [`CaptureStatus`].
    pub fn status(&self) -> CaptureStatus {
        self.shared.lock().status(self.started, Instant::now())
    }

    /// Stops the capture and returns the narration (48 kHz, mono).
    ///
    /// - [`MediaError::MicrophoneSilent`] if the whole recording was digital silence (and at
    ///   least a second long): there is nothing to keep, and the cause is almost surely the
    ///   Windows microphone privacy setting.
    /// - If the device failed in the middle (unplugged), what was recorded before is returned;
    ///   the interface saw [`CaptureStatus::Failed`] meanwhile. With nothing recorded, the
    ///   failure is returned as [`MediaError::Microphone`].
    /// - A recording that reached its limit ([`CaptureStatus::Full`]) returns what it recorded.
    pub fn stop(mut self) -> Result<AudioClip, MediaError> {
        self.stop.store(true, Ordering::Release);
        let Some(worker) = self.worker.take() else {
            return Ok(AudioClip::default());
        };
        let outcome = worker.join().map_err(|_| MediaError::Microphone {
            step: "recording",
            message: "the capture thread panicked".into(),
        })??;
        if self.record {
            if outcome.all_silent {
                return Err(MediaError::MicrophoneSilent);
            }
            if let Some(message) = outcome.failure
                && outcome.clip.samples.is_empty()
            {
                return Err(MediaError::Microphone {
                    step: "recording",
                    message,
                });
            }
        }
        Ok(outcome.clip)
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Sender;

    fn tone(rate: u32, channels: usize, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let v = (0.3
                    * (2.0 * std::f64::consts::PI * 300.0 * n as f64 / f64::from(rate)).sin())
                    as f32;
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    fn run_with(
        format: CaptureFormat,
        record: bool,
        events: impl FnOnce(&Sender<CaptureEvent>),
    ) -> (Result<CaptureOutcome, MediaError>, Shared) {
        let (tx, rx) = mpsc::channel();
        events(&tx);
        // Everything is queued: ask to stop; the loop drains the queue then ends.
        let stop = AtomicBool::new(true);
        let shared = Shared::default();
        let outcome = run_capture(&rx, format, record, &stop, &shared);
        (outcome, shared)
    }

    const MONO_44K: CaptureFormat = CaptureFormat {
        sample_rate: 44_100,
        channels: 1,
    };

    #[test]
    fn drains_the_queue_after_stop_and_converts() {
        let (outcome, shared) = run_with(MONO_44K, true, |tx| {
            for _ in 0..10 {
                tx.send(CaptureEvent::Samples(tone(44_100, 1, 4_410))).ok();
            }
        });
        let outcome = outcome.expect("outcome");
        // 44 100 frames at 44.1 kHz -> 48 000 frames.
        assert_eq!(outcome.clip.channels, 1);
        assert_eq!(outcome.clip.samples.len(), 48_000);
        assert!(!outcome.all_silent);
        assert!(outcome.failure.is_none());
        let state = shared.lock();
        assert_eq!(state.frames, 44_100);
        assert_eq!(state.elapsed, Duration::from_secs(1));
        assert!((state.level.peak - 0.3).abs() < 0.02);
        assert!(!state.silent);
    }

    #[test]
    fn exact_zeros_for_a_second_are_reported() {
        let (outcome, shared) = run_with(MONO_44K, true, |tx| {
            tx.send(CaptureEvent::Samples(vec![0.0; 22_050])).ok();
            tx.send(CaptureEvent::Samples(vec![0.0; 22_050])).ok();
        });
        let outcome = outcome.expect("outcome");
        assert!(outcome.all_silent);
        assert!(shared.lock().silent);
    }

    #[test]
    fn a_quiet_start_followed_by_sound_is_not_all_silent() {
        let (outcome, shared) = run_with(MONO_44K, true, |tx| {
            tx.send(CaptureEvent::Samples(vec![0.0; 44_100])).ok();
            tx.send(CaptureEvent::Samples(tone(44_100, 1, 4_410))).ok();
        });
        let outcome = outcome.expect("outcome");
        assert!(!outcome.all_silent);
        assert!(!shared.lock().silent, "cleared by the sound");
    }

    #[test]
    fn a_failure_ends_the_loop_and_keeps_the_audio() {
        let (outcome, shared) = run_with(MONO_44K, true, |tx| {
            tx.send(CaptureEvent::Samples(tone(44_100, 1, 4_410))).ok();
            tx.send(CaptureEvent::Failed("device unplugged".into()))
                .ok();
            // Never read: the loop stopped at the failure.
            tx.send(CaptureEvent::Samples(tone(44_100, 1, 4_410))).ok();
        });
        let outcome = outcome.expect("outcome");
        assert_eq!(outcome.failure.as_deref(), Some("device unplugged"));
        assert_eq!(outcome.clip.samples.len(), 4_800);
        let state = shared.lock();
        assert_eq!(state.failure.as_deref(), Some("device unplugged"));
        // The meter falls with the microphone instead of staying at the last tone.
        assert_eq!(state.level, Level::default());
    }

    #[test]
    fn a_recording_stops_by_itself_at_its_limit_and_keeps_what_came_before() {
        let (tx, rx) = mpsc::channel();
        let shared = Arc::new(Shared::default());
        // 0.1 s at 48 kHz is 4410 frames at 44.1 kHz.
        let pipeline = Pipeline::with_limit(44_100, 1, true, 4_800).expect("pipeline");
        let (done, outcome) = mpsc::channel();
        let worker = {
            let shared = Arc::clone(&shared);
            std::thread::spawn(move || {
                // Neither the stop flag nor a closed channel ends this loop.
                let stop = AtomicBool::new(false);
                let _ = done.send(consume(&rx, pipeline, &stop, &shared));
            })
        };
        for _ in 0..3 {
            tx.send(CaptureEvent::Samples(tone(44_100, 1, 2_205))).ok();
        }
        let outcome = outcome
            .recv_timeout(Duration::from_secs(10))
            .expect("the capture ended by itself")
            .expect("outcome");
        worker.join().expect("no panic");
        assert_eq!(outcome.clip.samples.len(), 4_800);
        assert!(outcome.failure.is_none());
        let state = shared.lock();
        let now = Instant::now();
        assert_eq!(state.status(now, now), CaptureStatus::Full);
        assert_eq!(state.frames, 4_410);
        assert_eq!(state.level, Level::default());
        drop(tx);
    }

    #[test]
    fn a_monitor_has_no_limit() {
        // It keeps nothing, whatever its length.
        let mut monitor = Pipeline::with_limit(48_000, 1, false, 480).expect("pipeline");
        monitor.push(&tone(48_000, 1, 4_800)).expect("push");
        assert!(!monitor.is_full());
    }

    #[test]
    fn a_stream_that_stops_delivering_is_reported_and_its_level_falls() {
        let started = Instant::now();
        let at = |ms: u64| started + Duration::from_millis(ms);
        let mut state = State::default();
        assert_eq!(state.status(started, at(1_000)), CaptureStatus::Waiting);
        assert_eq!(state.status(started, at(3_500)), CaptureStatus::NoData);
        // Audio arrived at 4 s, then nothing more.
        state.frames = 4_410;
        state.last_frame = Some(at(4_000));
        state.level = Level {
            peak: 0.3,
            rms: 0.2,
        };
        assert_eq!(state.status(started, at(6_000)), CaptureStatus::Receiving);
        assert_eq!(state.level(started, at(6_000)).peak, 0.3);
        assert_eq!(state.status(started, at(7_500)), CaptureStatus::NoData);
        assert_eq!(state.level(started, at(7_500)), Level::default());
        // A silence delivers frames: it is reported as such, not as a stuck stream.
        state.silent = true;
        state.last_frame = Some(at(7_400));
        assert_eq!(state.status(started, at(7_500)), CaptureStatus::Silent);
    }

    #[test]
    fn a_monitor_reports_levels_but_keeps_no_audio() {
        let (outcome, shared) = run_with(MONO_44K, false, |tx| {
            tx.send(CaptureEvent::Samples(tone(44_100, 1, 22_050))).ok();
        });
        let outcome = outcome.expect("outcome");
        assert!(outcome.clip.samples.is_empty());
        assert!(shared.lock().level.peak > 0.25);
    }

    #[test]
    fn a_closed_channel_ends_the_loop() {
        let (tx, rx) = mpsc::channel();
        tx.send(CaptureEvent::Samples(tone(48_000, 2, 480))).ok();
        drop(tx);
        let stop = AtomicBool::new(false);
        let shared = Shared::default();
        let format = CaptureFormat {
            sample_rate: 48_000,
            channels: 2,
        };
        let outcome = run_capture(&rx, format, true, &stop, &shared).expect("outcome");
        assert_eq!(outcome.clip.samples.len(), 480);
    }

    #[test]
    fn the_stop_flag_ends_a_running_loop() {
        let (tx, rx) = mpsc::channel::<CaptureEvent>();
        let stop = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared::default());
        let worker = {
            let (stop, shared) = (Arc::clone(&stop), Arc::clone(&shared));
            std::thread::spawn(move || {
                let format = CaptureFormat {
                    sample_rate: 16_000,
                    channels: 1,
                };
                run_capture(&rx, format, true, &stop, &shared)
            })
        };
        tx.send(CaptureEvent::Samples(tone(16_000, 1, 1_600))).ok();
        std::thread::sleep(Duration::from_millis(120));
        // The sender is still alive: only the flag can end the loop.
        stop.store(true, Ordering::Release);
        let outcome = worker.join().expect("no panic").expect("outcome");
        assert_eq!(outcome.clip.samples.len(), 4_800);
        drop(tx);
    }

    #[test]
    fn an_impossible_format_is_an_error() {
        let format = CaptureFormat {
            sample_rate: 0,
            channels: 1,
        };
        let (outcome, _) = run_with(format, true, |_| {});
        assert!(matches!(outcome, Err(MediaError::InvalidSettings(_))));
    }
}
