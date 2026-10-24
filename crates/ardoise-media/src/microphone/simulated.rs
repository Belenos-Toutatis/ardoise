//! A synthetic, voice-like narration for tests that need sound without a microphone (for
//! example the end-of-milestone check of an MP4 with a narration track).

use std::f64::consts::PI;
use std::time::Duration;

use crate::{AudioClip, NARRATION_SAMPLE_RATE};

/// Length of one "word" and of the pause after it, in seconds.
const WORD: f64 = 0.9;
const PAUSE: f64 = 0.35;
/// Syllables per second inside a word.
const SYLLABLE_RATE: f64 = 4.0;
/// Peak level of the signal, as a fraction of full scale (about -8 dBFS: a normal voice).
const PEAK: f64 = 0.4;

/// A synthetic narration of `duration` at 48 kHz: words of about a second made of modulated
/// harmonic tones (a vowel-like spectrum on a gliding pitch of 100 to 180 Hz) separated by
/// short pauses of near silence. It never reaches digital silence, as a real room does not.
///
/// `channels` is 1 or 2 (anything else is clamped into that range); in stereo the right
/// channel is slightly quieter. The result is deterministic.
pub fn simulated_narration(duration: Duration, channels: u16) -> AudioClip {
    let channels = channels.clamp(1, 2);
    let rate = f64::from(NARRATION_SAMPLE_RATE);
    let frames = (duration.as_secs_f64() * rate).round() as usize;
    let mut samples = Vec::with_capacity(frames * usize::from(channels));
    let mut noise_state = 0x1234_5678u32;
    let mut phase = 0.0f64;
    let period = WORD + PAUSE;

    for n in 0..frames {
        let t = n as f64 / rate;
        let word_index = (t / period).floor();
        let in_word = t - word_index * period;

        // Pitch glides up and down differently in every word.
        let glide = (word_index * 1.7).sin();
        let f0 = 140.0 + 40.0 * glide * (PI * (in_word / WORD).min(1.0)).sin();
        phase += 2.0 * PI * f0 / rate;

        let voiced = if in_word < WORD {
            // Syllable envelope: raised cosine bumps, with fades at both ends of the word.
            let syllable = 0.5 - 0.5 * (2.0 * PI * SYLLABLE_RATE * in_word).cos();
            let edge = (in_word / 0.05).min(1.0) * ((WORD - in_word) / 0.05).clamp(0.0, 1.0);
            // The vowel changes with every syllable: formant-like emphasis of harmonics.
            let vowel = (in_word * SYLLABLE_RATE).floor() + word_index;
            let centre = [3.0, 5.0, 8.0, 11.0][(vowel as usize) % 4];
            let mut sum = 0.0;
            for h in 1..=20 {
                let h = f64::from(h);
                let weight = (-((h - centre) / 3.0).powi(2)).exp() + 0.5 / h;
                sum += weight * (h * phase).sin();
            }
            sum * syllable * edge
        } else {
            0.0
        };

        noise_state = noise_state
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        let noise = (f64::from(noise_state >> 8) / f64::from(1u32 << 24) - 0.5) * 0.004;

        let value = voiced * 0.12 + noise;
        let left = value.clamp(-1.0, 1.0);
        samples.push(to_i16(left));
        if channels == 2 {
            samples.push(to_i16(left * 0.85));
        }
    }

    // Scale so the loudest sample sits at PEAK of full scale, whatever the synthesis gives.
    let loudest = samples
        .iter()
        .map(|&s| i32::from(s).abs())
        .max()
        .unwrap_or(0);
    if loudest > 0 {
        let gain = PEAK * f64::from(i16::MAX) / f64::from(loudest);
        for s in &mut samples {
            *s = (f64::from(*s) * gain).round() as i16;
        }
    }
    AudioClip { channels, samples }
}

fn to_i16(v: f64) -> i16 {
    (v * f64::from(i16::MAX)).round() as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rms(samples: &[i16]) -> f64 {
        let sum: f64 = samples
            .iter()
            .map(|&s| (f64::from(s) / 32768.0).powi(2))
            .sum();
        (sum / samples.len().max(1) as f64).sqrt()
    }

    #[test]
    fn length_and_channels_follow_the_request() {
        let mono = simulated_narration(Duration::from_millis(2500), 1);
        assert_eq!(mono.channels, 1);
        assert_eq!(mono.samples.len(), 120_000);
        assert_eq!(mono.duration(), Duration::from_millis(2500));

        let stereo = simulated_narration(Duration::from_secs(1), 2);
        assert_eq!(stereo.channels, 2);
        assert_eq!(stereo.samples.len(), 96_000);
        assert_eq!(stereo.duration(), Duration::from_secs(1));

        assert_eq!(simulated_narration(Duration::from_secs(1), 0).channels, 1);
        assert_eq!(simulated_narration(Duration::from_secs(1), 6).channels, 2);
        assert!(simulated_narration(Duration::ZERO, 1).samples.is_empty());
    }

    #[test]
    fn sounds_like_speech_with_pauses() {
        let clip = simulated_narration(Duration::from_secs(5), 1);
        let peak = clip
            .samples
            .iter()
            .map(|&s| i32::from(s).abs())
            .max()
            .unwrap_or(0);
        assert!((12_000..=14_000).contains(&peak), "peak {peak}");

        // Loud inside the first word, quiet in the pause that follows it (0.9 s to 1.25 s).
        let word = &clip.samples[4_800..40_000];
        let pause = &clip.samples[44_000 + 2_000..44_000 + 12_000];
        assert!(rms(word) > 0.05, "word rms {}", rms(word));
        assert!(rms(pause) < 0.005, "pause rms {}", rms(pause));
        // The pause is still not digital silence.
        assert!(pause.iter().any(|&s| s != 0));
        // The speech has dynamics: the syllables are not a steady tone.
        let windows: Vec<f64> = clip.samples[4_800..40_000].chunks(2_400).map(rms).collect();
        let (lo, hi) = windows
            .iter()
            .fold((f64::MAX, 0.0f64), |(lo, hi), &r| (lo.min(r), hi.max(r)));
        assert!(hi > 3.0 * lo, "levels {lo} .. {hi}");
    }

    #[test]
    fn is_deterministic() {
        let a = simulated_narration(Duration::from_secs(2), 2);
        let b = simulated_narration(Duration::from_secs(2), 2);
        assert_eq!(a, b);
    }

    #[test]
    fn stereo_right_channel_is_quieter() {
        let clip = simulated_narration(Duration::from_secs(1), 2);
        let left: Vec<i16> = clip.samples.iter().step_by(2).copied().collect();
        let right: Vec<i16> = clip.samples.iter().skip(1).step_by(2).copied().collect();
        let ratio = rms(&right) / rms(&left);
        assert!((ratio - 0.85).abs() < 0.02, "ratio {ratio}");
    }
}
