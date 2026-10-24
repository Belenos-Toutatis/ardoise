//! The level meter: peak and RMS of the microphone signal over windows of about 50 ms.

/// Length of one measurement window.
pub(crate) const LEVEL_WINDOW_MS: u32 = 50;

/// Loudness over one window, in linear full-scale units (`0.0` silence, `1.0` digital full scale).
///
/// Both values are measured on the device signal before any resampling or downmix, over all
/// channels: `peak` is the largest absolute sample (a clipping channel shows up), `rms` the
/// root mean square of every sample.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Level {
    /// Largest absolute sample of the window, `0.0..=1.0`.
    pub peak: f32,
    /// Root mean square of the window, `0.0..=1.0`.
    pub rms: f32,
}

impl Level {
    /// What [`Level::peak_dbfs`] and [`Level::rms_dbfs`] return for silence.
    pub const FLOOR_DBFS: f32 = -90.0;

    /// Peak in dBFS (`0.0` at full scale), never below [`Level::FLOOR_DBFS`].
    pub fn peak_dbfs(self) -> f32 {
        to_dbfs(self.peak)
    }

    /// RMS in dBFS (`0.0` for a full-scale square wave), never below [`Level::FLOOR_DBFS`].
    pub fn rms_dbfs(self) -> f32 {
        to_dbfs(self.rms)
    }
}

fn to_dbfs(linear: f32) -> f32 {
    if linear <= 0.0 {
        Level::FLOOR_DBFS
    } else {
        (20.0 * linear.log10()).max(Level::FLOOR_DBFS)
    }
}

/// Accumulates interleaved samples and yields a [`Level`] each time a window is complete.
#[derive(Debug)]
pub(crate) struct LevelMeter {
    channels: usize,
    window_frames: u64,
    frames: u64,
    sum_squares: f64,
    samples: u64,
    peak: f32,
}

impl LevelMeter {
    /// A meter for a device with `channels` channels at `sample_rate` Hz.
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let window_frames = (u64::from(sample_rate) * u64::from(LEVEL_WINDOW_MS) / 1000).max(1);
        Self {
            channels: channels.max(1),
            window_frames,
            frames: 0,
            sum_squares: 0.0,
            samples: 0,
            peak: 0.0,
        }
    }

    /// Adds interleaved samples (whole frames; a trailing partial frame is ignored). Returns
    /// the level of the last window completed by this call, if any.
    pub fn push(&mut self, interleaved: &[f32]) -> Option<Level> {
        let mut last = None;
        for frame in interleaved.chunks_exact(self.channels) {
            for &sample in frame {
                // A NaN or infinite sample from a broken driver counts as silence.
                let value = if sample.is_finite() { sample } else { 0.0 };
                self.peak = self.peak.max(value.abs());
                self.sum_squares += f64::from(value) * f64::from(value);
            }
            self.samples += self.channels as u64;
            self.frames += 1;
            if self.frames >= self.window_frames {
                last = Some(self.finish_window());
            }
        }
        last
    }

    fn finish_window(&mut self) -> Level {
        let rms = (self.sum_squares / self.samples.max(1) as f64).sqrt();
        let level = Level {
            peak: self.peak.min(1.0),
            rms: (rms as f32).min(1.0),
        };
        self.frames = 0;
        self.samples = 0;
        self.sum_squares = 0.0;
        self.peak = 0.0;
        level
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(amplitude: f32, frames: usize, channels: usize, rate: u32) -> Vec<f32> {
        (0..frames)
            .flat_map(|n| {
                let v =
                    amplitude * (2.0 * std::f32::consts::PI * 440.0 * n as f32 / rate as f32).sin();
                std::iter::repeat_n(v, channels)
            })
            .collect()
    }

    #[test]
    fn sine_peak_and_rms() {
        let mut meter = LevelMeter::new(48_000, 1);
        let level = meter.push(&sine(0.5, 2400, 1, 48_000)).expect("one window");
        assert!((level.peak - 0.5).abs() < 0.01, "{level:?}");
        assert!((level.rms - 0.5 / 2f32.sqrt()).abs() < 0.01, "{level:?}");
        assert!((level.peak_dbfs() + 6.02).abs() < 0.2, "{level:?}");
    }

    #[test]
    fn a_window_is_fifty_milliseconds() {
        let mut meter = LevelMeter::new(44_100, 2);
        // 50 ms at 44.1 kHz = 2205 frames.
        assert_eq!(meter.push(&sine(0.3, 2204, 2, 44_100)), None);
        assert!(meter.push(&sine(0.3, 1, 2, 44_100)).is_some());
        // The next window starts from zero.
        assert_eq!(meter.push(&sine(0.3, 100, 2, 44_100)), None);
    }

    #[test]
    fn several_windows_in_one_call_report_the_last() {
        let mut meter = LevelMeter::new(48_000, 1);
        let mut data = vec![0.9; 2400];
        data.extend(vec![0.1; 2400]);
        let level = meter.push(&data).expect("two windows");
        assert!((level.peak - 0.1).abs() < 1e-6);
    }

    #[test]
    fn silence_is_at_the_floor() {
        let mut meter = LevelMeter::new(48_000, 1);
        let level = meter.push(&vec![0.0; 2400]).expect("window");
        assert_eq!(level, Level::default());
        assert_eq!(level.peak_dbfs(), Level::FLOOR_DBFS);
        assert_eq!(level.rms_dbfs(), Level::FLOOR_DBFS);
    }

    #[test]
    fn peak_sees_a_single_loud_channel_and_clamps() {
        let mut meter = LevelMeter::new(48_000, 2);
        let mut data = vec![0.0; 4800];
        data[1001] = -1.5;
        data[7] = f32::NAN;
        let level = meter.push(&data).expect("window");
        assert_eq!(level.peak, 1.0);
        assert!(level.rms.is_finite());
    }
}
