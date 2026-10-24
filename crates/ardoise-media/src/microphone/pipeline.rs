//! The hardware-free core of the recorder: whatever the device delivers, as interleaved `f32`
//! at its own rate and channel count, goes through the level meter, the silence detector, a
//! mono downmix and the resampler, and ends up as 48 kHz mono `i16` (the narration format).

use std::time::Duration;

use cpal::{FromSample, Sample};

use super::level::{Level, LevelMeter};
use super::resample::NarrationResampler;
use super::silence::SilenceDetector;
use crate::{AudioClip, MediaError};

/// Highest device sample rate accepted (a sanity bound; real devices stop at 192 kHz).
const MAX_SAMPLE_RATE: u32 = 768_000;
/// Highest channel count accepted (audio interfaces go up to a few dozen).
const MAX_CHANNELS: u16 = 64;

/// Converts samples of any format the devices deliver (8, 16, 24, 32-bit integers, floats) to
/// `f32` in `-1.0..=1.0`, appending to `out`. A zero sample stays exactly `0.0` in every
/// format (unsigned formats are centred first), which the silence detector relies on.
pub(crate) fn convert_to_f32<T>(data: &[T], out: &mut Vec<f32>)
where
    T: Sample,
    f32: FromSample<T>,
{
    out.extend(data.iter().map(|s| s.to_sample::<f32>()));
}

pub(crate) struct Pipeline {
    channels: usize,
    sample_rate: u32,
    meter: LevelMeter,
    silence: SilenceDetector,
    resampler: NarrationResampler,
    /// False for a level-only monitor: nothing is kept.
    record: bool,
    level: Level,
    frames_in: u64,
    mono: Vec<f32>,
    converted: Vec<f32>,
    samples: Vec<i16>,
}

impl Pipeline {
    pub fn new(sample_rate: u32, channels: u16, record: bool) -> Result<Self, MediaError> {
        if sample_rate == 0
            || sample_rate > MAX_SAMPLE_RATE
            || channels == 0
            || channels > MAX_CHANNELS
        {
            return Err(MediaError::InvalidSettings(format!(
                "microphone format of {channels} channel(s) at {sample_rate} Hz"
            )));
        }
        let channels = usize::from(channels);
        Ok(Self {
            channels,
            sample_rate,
            meter: LevelMeter::new(sample_rate, channels),
            silence: SilenceDetector::new(sample_rate, channels),
            resampler: NarrationResampler::new(sample_rate)?,
            record,
            level: Level::default(),
            frames_in: 0,
            mono: Vec::new(),
            converted: Vec::new(),
            samples: Vec::new(),
        })
    }

    /// Adds interleaved samples as the device delivered them. A trailing partial frame is
    /// dropped.
    pub fn push(&mut self, interleaved: &[f32]) -> Result<(), MediaError> {
        let whole = interleaved.len() - interleaved.len() % self.channels;
        let interleaved = &interleaved[..whole];
        if interleaved.is_empty() {
            return Ok(());
        }
        self.frames_in += (whole / self.channels) as u64;
        if let Some(level) = self.meter.push(interleaved) {
            self.level = level;
        }
        self.silence.push(interleaved);
        if !self.record {
            return Ok(());
        }
        self.mono.clear();
        self.mono
            .extend(interleaved.chunks_exact(self.channels).map(|frame| {
                let sum: f32 = frame
                    .iter()
                    .map(|&s| if s.is_finite() { s } else { 0.0 })
                    .sum();
                sum / self.channels as f32
            }));
        self.converted.clear();
        self.resampler.push(&self.mono, &mut self.converted)?;
        self.store();
        Ok(())
    }

    fn store(&mut self) {
        self.samples
            .extend(self.converted.iter().map(|&s| to_i16(s)));
    }

    /// Level of the last completed 50 ms window (zero before the first one).
    pub fn level(&self) -> Level {
        self.level
    }

    /// Input time received so far.
    pub fn elapsed(&self) -> Duration {
        Duration::from_secs_f64(self.frames_in as f64 / f64::from(self.sample_rate))
    }

    pub fn frames_received(&self) -> u64 {
        self.frames_in
    }

    /// The last second or more was exactly zero.
    pub fn is_silent_now(&self) -> bool {
        self.silence.is_silent_now()
    }

    /// Everything received so far was exactly zero and that makes at least a second.
    pub fn is_all_silent(&self) -> bool {
        self.silence.is_all_silent()
    }

    /// Flushes the resampler and returns the narration (empty for a monitor).
    pub fn finish(mut self) -> Result<AudioClip, MediaError> {
        if self.record {
            self.converted.clear();
            self.resampler.finish(&mut self.converted)?;
            self.store();
        }
        Ok(AudioClip {
            channels: 1,
            samples: self.samples,
        })
    }
}

fn to_i16(sample: f32) -> i16 {
    // `as` saturates and maps NaN to zero.
    (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::NARRATION_SAMPLE_RATE;

    /// `frames` frames of a sine at `freq` Hz, `amplitude` of full scale, the same on every
    /// channel, as interleaved f32.
    fn tone(freq: f64, amplitude: f64, rate: u32, channels: usize, frames: usize) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let v = (amplitude
                    * (2.0 * std::f64::consts::PI * freq * n as f64 / f64::from(rate)).sin())
                    as f32;
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    /// Feeds `data` in chunks of `chunk_frames` frames.
    fn record(rate: u32, channels: u16, data: &[f32], chunk_frames: usize) -> AudioClip {
        let mut p = Pipeline::new(rate, channels, true).expect("pipeline");
        for chunk in data.chunks(chunk_frames * usize::from(channels)) {
            p.push(chunk).expect("push");
        }
        p.finish().expect("finish")
    }

    /// Magnitude of the `freq` component of a 48 kHz mono clip (Goertzel), normalised so a
    /// full-scale sine gives about 1.0.
    fn component(clip: &AudioClip, freq: f64) -> f64 {
        let n = clip.samples.len();
        let w = 2.0 * std::f64::consts::PI * freq / f64::from(NARRATION_SAMPLE_RATE);
        let coeff = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0f64, 0.0f64);
        for &x in &clip.samples {
            let s = f64::from(x) / 32768.0 + coeff * s1 - s2;
            s2 = s1;
            s1 = s;
        }
        let power = s1 * s1 + s2 * s2 - coeff * s1 * s2;
        2.0 * power.max(0.0).sqrt() / n as f64
    }

    fn expected_frames(rate: u32, frames: usize) -> usize {
        (frames as f64 * 48_000.0 / f64::from(rate)).round() as usize
    }

    #[test]
    fn every_rate_and_channel_count_gives_48k_mono_of_the_right_length() {
        for rate in [48_000u32, 44_100, 16_000] {
            for channels in [1u16, 2, 3, 5, 6] {
                let frames = rate as usize; // one second
                let data = tone(440.0, 0.5, rate, usize::from(channels), frames);
                // An awkward chunk size, as real drivers deliver.
                let clip = record(rate, channels, &data, 441);
                assert_eq!(clip.channels, 1);
                assert_eq!(
                    clip.samples.len(),
                    expected_frames(rate, frames),
                    "{rate} Hz, {channels} channel(s)"
                );
                let tone_level = component(&clip, 440.0);
                assert!(
                    (tone_level - 0.5).abs() < 0.02,
                    "{rate} Hz, {channels} ch: 440 Hz at {tone_level}"
                );
                assert!(component(&clip, 1_000.0) < 0.01);
                assert!(component(&clip, 7_000.0) < 0.01);
            }
        }
    }

    #[test]
    fn channels_are_averaged() {
        // Left at +0.4, right at +0.2 -> 0.3.
        let data: Vec<f32> = (0..4800).flat_map(|_| [0.4f32, 0.2]).collect();
        let clip = record(48_000, 2, &data, 480);
        assert_eq!(clip.samples.len(), 4800);
        let expected = (0.3f32 * 32767.0).round() as i16;
        assert!(clip.samples.iter().all(|&s| s == expected));
    }

    #[test]
    fn samples_are_clamped_not_wrapped() {
        let clip = record(48_000, 1, &[2.0, -2.0, f32::NAN, 0.0], 4);
        assert_eq!(clip.samples, vec![32767, -32767, 0, 0]);
    }

    #[test]
    fn a_trailing_partial_frame_is_dropped() {
        let mut p = Pipeline::new(48_000, 2, true).expect("pipeline");
        p.push(&[0.1, 0.1, 0.1]).expect("push");
        let clip = p.finish().expect("finish");
        assert_eq!(clip.samples.len(), 1);
    }

    #[test]
    fn integer_formats_convert_to_float() {
        let mut out = Vec::new();
        convert_to_f32::<i16>(&[0, i16::MIN, 16384, -16384], &mut out);
        assert_eq!(out, vec![0.0, -1.0, 0.5, -0.5]);
        out.clear();
        convert_to_f32::<u16>(&[32768, 0, 49152], &mut out);
        assert_eq!(out, vec![0.0, -1.0, 0.5]);
        out.clear();
        convert_to_f32::<i32>(&[0, i32::MIN, 1 << 30], &mut out);
        assert_eq!(out, vec![0.0, -1.0, 0.5]);
        out.clear();
        convert_to_f32::<u8>(&[128, 0, 192], &mut out);
        assert_eq!(out, vec![0.0, -1.0, 0.5]);
        out.clear();
        convert_to_f32::<f32>(&[0.0, 0.25], &mut out);
        assert_eq!(out, vec![0.0, 0.25]);
    }

    #[test]
    fn i16_input_round_trips_at_48k() {
        let source: Vec<i16> = (0..9600)
            .map(|n| ((n as f64 * 0.05).sin() * 20_000.0) as i16)
            .collect();
        let mut converted = Vec::new();
        convert_to_f32(&source, &mut converted);
        let clip = record(48_000, 1, &converted, 480);
        let worst = source
            .iter()
            .zip(&clip.samples)
            .map(|(&a, &b)| (i32::from(a) - i32::from(b)).abs())
            .max()
            .unwrap_or(0);
        assert!(worst <= 1, "worst difference {worst}");
    }

    #[test]
    fn digital_silence_is_detected_after_a_second_in_the_pipeline() {
        let mut p = Pipeline::new(44_100, 2, true).expect("pipeline");
        p.push(&vec![0.0; 2 * 22_050]).expect("push");
        assert!(!p.is_silent_now());
        p.push(&vec![0.0; 2 * 22_050]).expect("push");
        assert!(p.is_silent_now());
        assert!(p.is_all_silent());
        assert_eq!(p.level(), Level::default());
        assert_eq!(p.elapsed(), Duration::from_secs(1));
    }

    #[test]
    fn a_quiet_room_is_not_silence() {
        // A few least-significant bits of noise from a 16-bit converter.
        let mut state = 12345u32;
        let noise: Vec<f32> = (0..3 * 16_000)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                f32::from(((state >> 16) % 5) as i16 - 2) / 32768.0
            })
            .collect();
        let mut p = Pipeline::new(16_000, 1, true).expect("pipeline");
        p.push(&noise).expect("push");
        assert!(!p.is_silent_now());
        assert!(!p.is_all_silent());
        let level = p.level();
        assert!(level.peak > 0.0 && level.peak_dbfs() < -80.0, "{level:?}");
    }

    #[test]
    fn the_level_follows_the_signal() {
        let mut p = Pipeline::new(44_100, 2, true).expect("pipeline");
        assert_eq!(p.level(), Level::default());
        p.push(&tone(440.0, 0.25, 44_100, 2, 4410)).expect("push");
        let level = p.level();
        assert!((level.peak - 0.25).abs() < 0.01, "{level:?}");
        assert!((level.peak_dbfs() + 12.04).abs() < 0.2, "{level:?}");
        p.push(&vec![0.0; 2 * 4410]).expect("push");
        assert_eq!(p.level().peak, 0.0);
    }

    #[test]
    fn a_monitor_keeps_nothing() {
        let mut p = Pipeline::new(44_100, 1, false).expect("pipeline");
        p.push(&tone(440.0, 0.5, 44_100, 1, 44_100)).expect("push");
        assert!(p.level().peak > 0.4);
        let clip = p.finish().expect("finish");
        assert!(clip.samples.is_empty());
    }

    #[test]
    fn rejects_impossible_formats() {
        assert!(Pipeline::new(0, 1, true).is_err());
        assert!(Pipeline::new(48_000, 0, true).is_err());
        assert!(Pipeline::new(48_000, 1000, true).is_err());
        assert!(Pipeline::new(10_000_000, 1, true).is_err());
    }
}
