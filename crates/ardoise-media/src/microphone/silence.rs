//! Detection of the "Windows blocks microphone access for desktop apps" case.
//!
//! An unpackaged app that is denied the microphone still gets a running stream, but every
//! sample is exactly zero. A quiet room gives low, non-zero noise (at least the converter's
//! own dither or the preamp's noise floor), so only an unbroken run of exact zeros counts.

/// How long the signal must stay exactly zero before it is reported as silent, in milliseconds.
pub(crate) const SILENCE_AFTER_MS: u32 = 1000;

#[derive(Debug)]
pub(crate) struct SilenceDetector {
    channels: usize,
    threshold_frames: u64,
    /// Zero frames in a row, up to now.
    run: u64,
    total: u64,
    non_zero_seen: bool,
}

impl SilenceDetector {
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        Self {
            channels: channels.max(1),
            threshold_frames: (u64::from(sample_rate) * u64::from(SILENCE_AFTER_MS) / 1000).max(1),
            run: 0,
            total: 0,
            non_zero_seen: false,
        }
    }

    /// Adds interleaved samples as the device gave them (before any processing). A frame is
    /// zero when every one of its channels is exactly `0.0`.
    pub fn push(&mut self, interleaved: &[f32]) {
        for frame in interleaved.chunks_exact(self.channels) {
            self.total += 1;
            if frame.iter().all(|&s| s == 0.0) {
                self.run += 1;
            } else {
                self.run = 0;
                self.non_zero_seen = true;
            }
        }
    }

    /// True while the last second or more has been exactly zero. Goes back to false as soon as
    /// a non-zero sample arrives.
    pub fn is_silent_now(&self) -> bool {
        self.run >= self.threshold_frames
    }

    /// True if nothing but zeros has arrived so far and that makes at least a second: the
    /// whole recording is silence.
    pub fn is_all_silent(&self) -> bool {
        !self.non_zero_seen && self.total >= self.threshold_frames
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_of_exact_zeros_is_silent() {
        let mut d = SilenceDetector::new(48_000, 2);
        d.push(&vec![0.0; 2 * 47_999]);
        assert!(!d.is_silent_now());
        d.push(&[0.0, -0.0]);
        assert!(d.is_silent_now());
        assert!(d.is_all_silent());
    }

    #[test]
    fn a_quiet_room_is_not_silent() {
        // One least-significant bit of a 16-bit converter, now and then.
        let lsb = 1.0 / 32_768.0;
        let mut d = SilenceDetector::new(48_000, 1);
        for second in 0..3 {
            let mut data = vec![0.0; 48_000];
            data[100 + second] = lsb;
            data[30_000] = -lsb;
            d.push(&data);
            assert!(!d.is_silent_now(), "second {second}");
            assert!(!d.is_all_silent());
        }
    }

    #[test]
    fn one_non_zero_channel_is_enough() {
        let mut d = SilenceDetector::new(16_000, 2);
        let data: Vec<f32> = (0..2 * 32_000)
            .map(|i| if i % 400 == 1 { 1e-6 } else { 0.0 })
            .collect();
        d.push(&data);
        assert!(!d.is_silent_now());
    }

    #[test]
    fn recovers_when_sound_arrives_but_remembers_the_start() {
        let mut d = SilenceDetector::new(44_100, 1);
        d.push(&vec![0.0; 44_100]);
        assert!(d.is_silent_now());
        d.push(&[0.01]);
        assert!(!d.is_silent_now());
        assert!(!d.is_all_silent());
        // Zeros again, but a shorter run than a second.
        d.push(&vec![0.0; 40_000]);
        assert!(!d.is_silent_now());
    }

    #[test]
    fn a_short_recording_is_not_judged() {
        let mut d = SilenceDetector::new(48_000, 1);
        d.push(&vec![0.0; 10_000]);
        assert!(!d.is_silent_now());
        assert!(!d.is_all_silent());
    }
}
