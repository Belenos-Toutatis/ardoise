//! The simulated microphone of `--simulated-microphone` (M17, `docs/design/paper-projection-narration.md`
//! §3.3 and §3.8): the journeys record narrations without switching the PC's microphone on.
//!
//! The runtime it makes ([`runtime`]) lists and opens only this one device, whatever device is
//! asked for (a microphone remembered from an ordinary run included): the PC's own microphones
//! are never listed nor opened, `ardoise_media`'s `Recorder` is never started.
//!
//! The device behaves as a capture that delivers from the moment it opens: the audio received
//! ([`Source::elapsed`]) is the time since then, so the watcher's anchor is the wall clock at which
//! it opened; the capture is always `Receiving`; the level is that of the simulated voice at that
//! instant; and a recording stopped after `d` of capture is exactly
//! `simulated_narration(d, 1)` (deterministic, voice-like, never digital silence). A monitor
//! keeps no audio, as a real one.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use ardoise_media::microphone::{CaptureStatus, Level, simulated_narration};
use ardoise_media::{AudioClip, MediaError, NARRATION_SAMPLE_RATE};

use super::{Microphone, NarrationRuntime, Source};
use crate::commands::CommandResult;

/// The id of the simulated microphone (`Microphone.id`).
pub const SIMULATED_ID: &str = "simulated";
/// Its name, as the microphone picker shows it.
pub const SIMULATED_NAME: &str = "Simulated microphone";

/// The level meter's window, as the real capture's (`ardoise_media`'s `LEVEL_WINDOW_MS`).
const LEVEL_WINDOW: Duration = Duration::from_millis(50);
/// The span of the simulated voice whose levels loop: two of its words and their pauses
/// (a word of 0.9 s and a pause of 0.35 s in `ardoise_media`'s `simulated.rs`).
const LEVEL_LOOP: Duration = Duration::from_millis(2500);

/// The runtime of `--simulated-microphone`, backing up its narrations in `backup_dir`.
pub fn runtime(backup_dir: PathBuf) -> NarrationRuntime {
    NarrationRuntime::with_microphones(Box::new(open), Box::new(microphones), backup_dir)
}

/// The one microphone listed: the simulated one, the default.
pub fn microphones() -> CommandResult<Vec<Microphone>> {
    Ok(vec![Microphone {
        id: SIMULATED_ID.to_owned(),
        name: SIMULATED_NAME.to_owned(),
        is_default: true,
    }])
}

/// Opens the simulated microphone, whatever `device` names; `record` keeps its audio.
pub fn open(device: Option<&str>, record: bool) -> CommandResult<Box<dyn Source>> {
    let _ = device;
    let opened = Instant::now();
    Ok(Box::new(SimulatedMicrophone {
        record,
        since_open: Box::new(move || opened.elapsed()),
    }))
}

/// The time since the simulated microphone opened.
type Clock = Box<dyn Fn() -> Duration + Send + Sync>;

struct SimulatedMicrophone {
    record: bool,
    since_open: Clock,
}

impl Source for SimulatedMicrophone {
    fn device_name(&self) -> &str {
        SIMULATED_NAME
    }

    fn level(&self) -> Level {
        level_at((self.since_open)())
    }

    fn elapsed(&self) -> Duration {
        (self.since_open)()
    }

    fn status(&self) -> CaptureStatus {
        CaptureStatus::Receiving
    }

    fn stop(self: Box<Self>) -> Result<AudioClip, MediaError> {
        Ok(if self.record {
            simulated_narration((self.since_open)(), 1)
        } else {
            AudioClip::default()
        })
    }
}

/// The level of the simulated voice over the last whole window before `at` (none before the
/// first, as a real capture), its first [`LEVEL_LOOP`] repeating.
pub fn level_at(at: Duration) -> Level {
    let windows = at.as_nanos() / LEVEL_WINDOW.as_nanos();
    let levels = levels();
    if windows == 0 || levels.is_empty() {
        return Level::default();
    }
    let index = (windows - 1) % levels.len() as u128;
    usize::try_from(index)
        .ok()
        .and_then(|index| levels.get(index))
        .copied()
        .unwrap_or_default()
}

/// The levels of the windows of [`LEVEL_LOOP`] of the voice, computed once.
fn levels() -> &'static [Level] {
    static LEVELS: OnceLock<Vec<Level>> = OnceLock::new();
    LEVELS.get_or_init(|| {
        let clip = simulated_narration(LEVEL_LOOP, 1);
        let rate = u128::from(NARRATION_SAMPLE_RATE);
        let window = usize::try_from(LEVEL_WINDOW.as_nanos() * rate / 1_000_000_000)
            .unwrap_or(1)
            .max(1);
        clip.samples.chunks(window).map(level_of).collect()
    })
}

/// Peak and RMS of `samples`, linear full scale.
fn level_of(samples: &[i16]) -> Level {
    let full = 32_768.0;
    let peak = samples
        .iter()
        .map(|&s| f64::from(s).abs() / full)
        .fold(0.0, f64::max);
    let sum: f64 = samples.iter().map(|&s| (f64::from(s) / full).powi(2)).sum();
    let rms = (sum / samples.len().max(1) as f64).sqrt();
    Level {
        peak: peak as f32,
        rms: rms as f32,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::live::{HubConfig, LiveHub};
    use crate::narration::{
        CaptureState, NarrationState, NarrationTarget, StopRequest, monitor_in, monitor_stop_in,
        start_in, stop_in,
    };

    /// A simulated microphone on a clock the test moves (milliseconds since it opened).
    fn on_clock(record: bool) -> (Box<SimulatedMicrophone>, Arc<AtomicU64>) {
        let now = Arc::new(AtomicU64::new(0));
        let clock = Arc::clone(&now);
        let microphone = SimulatedMicrophone {
            record,
            since_open: Box::new(move || Duration::from_millis(clock.load(Ordering::Relaxed))),
        };
        (Box::new(microphone), now)
    }

    #[test]
    fn it_records_the_simulated_voice_for_as_long_as_it_ran() {
        let (microphone, now) = on_clock(true);
        assert_eq!(microphone.device_name(), "Simulated microphone");
        assert_eq!(microphone.status(), CaptureStatus::Receiving);
        assert_eq!(microphone.elapsed(), Duration::ZERO);
        now.store(1_500, Ordering::Relaxed);
        assert_eq!(microphone.elapsed(), Duration::from_millis(1_500));
        now.store(2_250, Ordering::Relaxed);
        let clip = microphone.stop().unwrap();
        assert_eq!(clip.duration(), Duration::from_millis(2_250));
        // Exactly the simulated voice: the same every time.
        assert_eq!(clip, simulated_narration(Duration::from_millis(2_250), 1));

        // A monitor keeps nothing.
        let (monitor, now) = on_clock(false);
        now.store(1_000, Ordering::Relaxed);
        assert_eq!(monitor.elapsed(), Duration::from_secs(1));
        assert!(monitor.stop().unwrap().samples.is_empty());
    }

    #[test]
    fn its_level_follows_the_words_and_pauses_of_the_voice() {
        let (microphone, now) = on_clock(false);
        // Nothing before the first window, as a real capture.
        now.store(30, Ordering::Relaxed);
        assert_eq!(microphone.level(), Level::default());
        // A syllable of the first word (0 to 0.9 s, four syllables a second: 0.6 to 0.65 s), then
        // the pause after the word (0.9 to 1.25 s).
        now.store(650, Ordering::Relaxed);
        let word = microphone.level();
        assert!(word.rms > 0.05 && word.peak > word.rms, "{word:?}");
        assert!(word.peak <= 1.0);
        now.store(1_200, Ordering::Relaxed);
        let pause = microphone.level();
        assert!(pause.rms < 0.005 && pause.rms > 0.0, "{pause:?}");
        // It moves within a word (0.45 to 0.5 s, the end of a syllable), and loops after 2.5 s.
        now.store(500, Ordering::Relaxed);
        assert!(
            microphone.level().rms < word.rms / 2.0,
            "{:?}",
            microphone.level()
        );
        now.store(3_150, Ordering::Relaxed);
        assert_eq!(microphone.level(), word);
        assert_eq!(level_at(Duration::from_millis(650)), word);
    }

    #[test]
    fn the_simulated_runtime_lists_and_opens_only_the_simulated_microphone() {
        let listed = microphones().unwrap();
        assert_eq!(
            listed,
            [Microphone {
                id: "simulated".into(),
                name: "Simulated microphone".into(),
                is_default: true,
            }]
        );
        let backup = tempfile::tempdir().unwrap();
        let runtime = runtime(backup.path().to_path_buf());
        assert_eq!(runtime.microphones().unwrap(), listed);
        // Whatever device is asked for, even one remembered from an ordinary run.
        for device in [
            None,
            Some("simulated"),
            Some("wasapi:{0.0.1.00000000}.{abc}"),
        ] {
            monitor_in(&runtime, device).unwrap();
            let status = runtime.status();
            assert_eq!(status.state, NarrationState::Monitoring);
            assert_eq!(status.device.as_deref(), Some("Simulated microphone"));
            assert_eq!(status.capture, Some(CaptureState::Receiving));
            monitor_stop_in(&runtime);
        }
        // The app's runtime with the flag is this one.
        let app = NarrationRuntime::with_simulated_microphone(true);
        assert_eq!(app.microphones().unwrap(), listed);
        assert!(names(backup.path()).is_empty());
    }

    #[test]
    fn a_writing_recording_on_the_simulated_microphone_lasts_as_long_as_it_ran() {
        let (_dir, library) = crate::library::tests::library();
        let saved = crate::library::tests::write_sample(&library, "", "Essai");
        let backup = tempfile::tempdir().unwrap();
        let runtime = runtime(backup.path().to_path_buf());
        let hub = LiveHub::new(HubConfig::default());
        let target = NarrationTarget::Writing {
            path: crate::library::path_text(&saved.path),
        };
        start_in(
            &runtime,
            Some("simulated"),
            target,
            || Ok(library.clone()),
            &hub,
            Arc::new(|| None),
        )
        .unwrap();
        let begun = Instant::now();
        std::thread::sleep(Duration::from_millis(150));
        let status = runtime.status();
        assert_eq!(status.state, NarrationState::Recording);
        assert!(status.elapsed_ms >= 150, "{status:?}");
        let request = StopRequest {
            save: true,
            replay_started_at_ms: Some(crate::live::model::wall_now_ms()),
            ..StopRequest::default()
        };
        let saved = stop_in(&runtime, &request, &hub).unwrap().unwrap();
        let ran = u64::try_from(begun.elapsed().as_millis()).unwrap();
        assert!(
            (150..=ran + 50).contains(&saved.duration_ms),
            "{saved:?} after {ran} ms"
        );
        assert!(saved.staged.is_some());
        assert_eq!(names(backup.path()).len(), 1);
    }

    fn names(dir: &std::path::Path) -> Vec<String> {
        std::fs::read_dir(dir)
            .map(|entries| {
                entries
                    .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
    }
}
