//! Streaming conversion of mono audio from the device rate to the 48 kHz narration rate.
//!
//! Uses `rubato`'s FFT resampler (fixed ratio, the best quality it has) in fixed-size input
//! chunks. The resampler delays its output by half an FFT block: that delay is dropped at the
//! start and the tail is flushed at the end, so the output has exactly
//! `round(input_frames * 48000 / rate)` frames and lines up with the input in time.

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};

use crate::{MediaError, NARRATION_SAMPLE_RATE};

/// Input chunk of the resampler, in frames (about 21 ms at 48 kHz).
const CHUNK_FRAMES: usize = 1024;

/// Safety bound on flush iterations (each one feeds a chunk of zeros).
const MAX_FLUSH_CHUNKS: usize = 64;

pub(crate) struct NarrationResampler {
    in_rate: u32,
    frames_in: u64,
    /// Frames handed out so far, after the startup delay was dropped.
    emitted: u64,
    /// `None` when the device already runs at 48 kHz.
    inner: Option<Inner>,
}

struct Inner {
    fft: Fft<f32>,
    pending: Vec<f32>,
    out_buf: Vec<f32>,
    delay_left: usize,
}

fn failed(message: impl ToString) -> MediaError {
    MediaError::Microphone {
        step: "resampling",
        message: message.to_string(),
    }
}

impl NarrationResampler {
    pub fn new(in_rate: u32) -> Result<Self, MediaError> {
        if in_rate == 0 {
            return Err(failed("the sample rate is zero"));
        }
        let inner = if in_rate == NARRATION_SAMPLE_RATE {
            None
        } else {
            let fft = Fft::<f32>::new(
                in_rate as usize,
                NARRATION_SAMPLE_RATE as usize,
                CHUNK_FRAMES,
                1,
                FixedSync::Input,
            )
            .map_err(failed)?;
            let delay_left = fft.output_delay();
            let out_buf = vec![0.0; fft.output_frames_max()];
            Some(Inner {
                fft,
                pending: Vec::new(),
                out_buf,
                delay_left,
            })
        };
        Ok(Self {
            in_rate,
            frames_in: 0,
            emitted: 0,
            inner,
        })
    }

    /// Adds mono samples at the device rate; appends the converted samples to `out`.
    pub fn push(&mut self, mono: &[f32], out: &mut Vec<f32>) -> Result<(), MediaError> {
        self.frames_in += mono.len() as u64;
        let Some(inner) = self.inner.as_mut() else {
            self.emitted += mono.len() as u64;
            out.extend_from_slice(mono);
            return Ok(());
        };
        inner.pending.extend_from_slice(mono);
        while inner.pending.len() >= inner.fft.input_frames_next() {
            self.emitted += inner.run_chunk(out)? as u64;
        }
        Ok(())
    }

    /// Flushes what the resampler still holds; appends it to `out`. Call once, after the last
    /// `push`. The total output now matches the total input exactly.
    pub fn finish(&mut self, out: &mut Vec<f32>) -> Result<(), MediaError> {
        let expected = (u128::from(self.frames_in) * u128::from(NARRATION_SAMPLE_RATE)
            + u128::from(self.in_rate) / 2)
            / u128::from(self.in_rate);
        let expected = expected as u64;
        let Some(inner) = self.inner.as_mut() else {
            return Ok(());
        };
        let tail_start = out.len();
        let mut chunks = 0;
        while self.emitted < expected && chunks < MAX_FLUSH_CHUNKS {
            // The first chunk completes the partial one with zeros, the next ones are zeros.
            let need = inner.fft.input_frames_next();
            if inner.pending.len() < need {
                inner.pending.resize(need, 0.0);
            }
            self.emitted += inner.run_chunk(out)? as u64;
            chunks += 1;
        }
        let excess = self.emitted.saturating_sub(expected) as usize;
        out.truncate(out.len().saturating_sub(excess).max(tail_start));
        self.emitted -= excess as u64;
        Ok(())
    }
}

impl Inner {
    /// Resamples one chunk from the front of `pending`; returns the frames appended to `out`
    /// (after the startup delay was dropped).
    fn run_chunk(&mut self, out: &mut Vec<f32>) -> Result<usize, MediaError> {
        let need = self.fft.input_frames_next();
        let room = self.fft.output_frames_next();
        if self.out_buf.len() < room {
            self.out_buf.resize(room, 0.0);
        }
        let input = InterleavedSlice::new(&self.pending[..need], 1, need).map_err(failed)?;
        let mut output =
            InterleavedSlice::new_mut(&mut self.out_buf[..room], 1, room).map_err(failed)?;
        let (_, produced) = self
            .fft
            .process_into_buffer(&input, &mut output, None)
            .map_err(failed)?;
        let skip = self.delay_left.min(produced);
        self.delay_left -= skip;
        out.extend_from_slice(&self.out_buf[skip..produced]);
        self.pending.drain(..need);
        Ok(produced - skip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(freq: f64, rate: u32, frames: usize) -> Vec<f32> {
        (0..frames)
            .map(|n| {
                (0.5 * (2.0 * std::f64::consts::PI * freq * n as f64 / f64::from(rate)).sin())
                    as f32
            })
            .collect()
    }

    fn resample(rate: u32, input: &[f32], chunk: usize) -> Vec<f32> {
        let mut r = NarrationResampler::new(rate).expect("resampler");
        let mut out = Vec::new();
        for c in input.chunks(chunk) {
            r.push(c, &mut out).expect("push");
        }
        r.finish(&mut out).expect("finish");
        out
    }

    /// Normalised correlation between `a` and `b` over their common central part.
    fn correlation(a: &[f32], b: &[f32]) -> f64 {
        let n = a.len().min(b.len());
        let (lo, hi) = (n / 10, n - n / 10);
        let (mut ab, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
        for i in lo..hi {
            let (x, y) = (f64::from(a[i]), f64::from(b[i]));
            ab += x * y;
            aa += x * x;
            bb += y * y;
        }
        ab / (aa.sqrt() * bb.sqrt())
    }

    #[test]
    fn output_length_is_exact_for_common_rates() {
        for (rate, frames) in [
            (44_100u32, 44_100usize),
            (44_100, 12_345),
            (16_000, 16_000),
            (16_000, 1),
            (8_000, 9_999),
            (22_050, 22_050),
            (32_000, 100_000),
            (96_000, 96_000),
            (88_200, 5_000),
            (48_000, 777),
        ] {
            let out = resample(rate, &tone(440.0, rate, frames), 441);
            let expected = (frames as f64 * 48_000.0 / f64::from(rate)).round() as usize;
            assert_eq!(out.len(), expected, "{rate} Hz, {frames} frames");
        }
    }

    #[test]
    fn empty_input_gives_empty_output() {
        assert!(resample(44_100, &[], 100).is_empty());
        assert!(resample(48_000, &[], 100).is_empty());
    }

    #[test]
    fn pitch_and_timing_are_preserved() {
        for rate in [44_100u32, 16_000, 96_000, 22_050] {
            let out = resample(rate, &tone(440.0, rate, rate as usize), 1000);
            let ideal = tone(440.0, 48_000, out.len());
            let c = correlation(&out, &ideal);
            assert!(c > 0.99, "{rate} Hz: correlation {c}");
        }
    }

    #[test]
    fn chunking_does_not_change_the_result() {
        let input = tone(300.0, 44_100, 30_000);
        let a = resample(44_100, &input, 30_000);
        let b = resample(44_100, &input, 137);
        assert_eq!(a.len(), b.len());
        let worst = a
            .iter()
            .zip(&b)
            .map(|(x, y)| (x - y).abs())
            .fold(0.0f32, f32::max);
        assert!(worst < 1e-4, "worst difference {worst}");
    }

    #[test]
    fn the_48k_path_is_a_passthrough() {
        let input = tone(440.0, 48_000, 5_000);
        assert_eq!(resample(48_000, &input, 333), input);
    }

    #[test]
    fn rejects_a_zero_rate() {
        assert!(NarrationResampler::new(0).is_err());
    }
}
