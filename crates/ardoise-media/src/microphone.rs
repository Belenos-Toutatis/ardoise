//! Microphone capture for the narration (SPEC §4.5, §6.2 row "Micro").
//!
//! - [`list_input_devices`]: the input devices (WASAPI on Windows, through `cpal`), the default
//!   one marked.
//! - [`Recorder`]: records from a chosen device (or the default) and converts, whatever the
//!   device delivers, to an [`AudioClip`](crate::AudioClip) of **48 kHz mono 16-bit**
//!   ([`NARRATION_SAMPLE_RATE`](crate::NARRATION_SAMPLE_RATE)). `Recorder::monitor` does the
//!   same without keeping audio, for a level meter on the settings screen.
//! - [`Level`]: peak and RMS of the last 50 ms window, linear `0.0..=1.0` or in dBFS; poll
//!   [`Recorder::level`] about 20 times a second.
//! - [`CaptureStatus`] and [`MediaError::MicrophoneSilent`](crate::MediaError): the
//!   "Windows blocks microphone access for desktop apps" case. In that case the stream runs
//!   but delivers exact digital zeros; after one second of them the status becomes
//!   [`CaptureStatus::Silent`] and a recording made of nothing else ends with
//!   `MicrophoneSilent`. A quiet room (low but non-zero noise) is neither.
//! - [`simulated_narration`]: a deterministic voice-like clip for tests without a microphone.
//!
//! Structure: the `cpal` callback only converts samples to `f32` and sends them through a
//! channel (`device`); a worker thread feeds a `Pipeline` (level meter, silence detector,
//! downmix, resampling) that has no hardware in it and is tested with synthetic buffers.

mod device;
mod level;
mod pipeline;
mod recorder;
mod resample;
mod silence;
mod simulated;

pub use level::Level;
pub use recorder::{CaptureStatus, Recorder};
pub use simulated::simulated_narration;

use crate::MediaError;

/// One input device, as listed by [`list_input_devices`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputDevice {
    /// Stable identifier from the audio backend (for WASAPI the endpoint id, prefixed with the
    /// host name). Store it to select the same microphone later; pass it to
    /// [`Recorder::start`].
    pub id: String,
    /// Name to show. Two devices can share a name; the id is what tells them apart.
    pub name: String,
    /// True for the system's default input device.
    pub is_default: bool,
}

/// Lists the input devices. An empty list means the PC has no microphone.
pub fn list_input_devices() -> Result<Vec<InputDevice>, MediaError> {
    device::list()
}

#[cfg(test)]
mod hardware_probe {
    use std::time::{Duration, Instant};

    use super::*;

    /// Manual check on a PC: lists the microphones, records about two seconds from the
    /// default one and prints the level and whether it looked silent. Nothing is saved.
    ///
    /// `cargo test -p ardoise-media hardware_probe -- --ignored --nocapture`
    #[test]
    #[ignore = "needs a microphone"]
    fn list_and_record_two_seconds() {
        let devices = list_input_devices().expect("device list");
        println!("{} input device(s):", devices.len());
        for d in &devices {
            let mark = if d.is_default { " [default]" } else { "" };
            println!("  {}{mark}\n      id: {}", d.name, d.id);
        }
        if devices.is_empty() {
            println!("no microphone: nothing to record");
            return;
        }

        // Selecting by id, by name, and an unknown device.
        for wanted in [&devices[0].id, &devices[0].name] {
            let r = Recorder::monitor(Some(wanted)).expect("select by id or name");
            std::thread::sleep(Duration::from_millis(300));
            assert!(r.stop().expect("monitor stop").samples.is_empty());
        }
        assert!(matches!(
            Recorder::start(Some("no such microphone")),
            Err(MediaError::MicrophoneNotFound(_))
        ));
        println!("selection by id, by name and unknown name: ok");

        let recorder = Recorder::start(None).expect("start");
        println!("recording from: {}", recorder.device_name());
        let begin = Instant::now();
        let mut loudest = Level::default();
        while begin.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(50));
            let level = recorder.level();
            loudest.peak = loudest.peak.max(level.peak);
            loudest.rms = loudest.rms.max(level.rms);
        }
        let status = recorder.status();
        let audio_time = recorder.elapsed();
        match recorder.stop() {
            Ok(clip) => {
                let exact_zeros = clip.samples.iter().all(|&s| s == 0);
                println!(
                    "status {status:?}, audio received {audio_time:?}; clip: {} channel, {} samples ({:?}), all exact zeros: {exact_zeros}",
                    clip.channels,
                    clip.samples.len(),
                    clip.duration()
                );
                println!(
                    "loudest window: peak {:.4} ({:.1} dBFS), rms {:.4} ({:.1} dBFS)",
                    loudest.peak,
                    loudest.peak_dbfs(),
                    loudest.rms,
                    loudest.rms_dbfs()
                );
            }
            Err(MediaError::MicrophoneSilent) => {
                println!(
                    "status {status:?}: SILENT (exact digital zeros): Windows may block desktop apps from the microphone"
                );
            }
            Err(e) => panic!("recording failed: {e}"),
        }
    }
}
