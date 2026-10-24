//! The narration stored with a note: its time base and its `audio.opus` member (an Ogg Opus
//! stream, `docs/file-format.md` §4 and `docs/design/media.md`, "Narration storage").
//!
//! Contract stub: the bodies are filled in by the media pipeline work package.

use std::time::Duration;

use crate::replay::ReplayOptions;
use crate::{AudioClip, MediaError};

/// The time base a narration was recorded in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrationBase {
    /// During a replay: replay time with the options of that replay.
    Replay,
    /// In live mode: note time (speed 1, no trimming).
    Note,
}

/// A narration and where it sits on the replay.
#[derive(Debug, Clone, PartialEq)]
pub struct Narration {
    /// 48 kHz.
    pub clip: AudioClip,
    pub base: NarrationBase,
    /// The time base: the replay options it was recorded with (for [`NarrationBase::Note`],
    /// speed 1 and no trimming).
    pub options: ReplayOptions,
    /// Replay instant, in that time base, of the first sample, in milliseconds; negative when
    /// the recording began before the note's time origin.
    pub start_ms: i64,
}

impl Narration {
    /// Length of the clip.
    pub fn duration(&self) -> Duration {
        self.clip.duration()
    }

    /// Replay instant just after the last sample, in milliseconds (rounded up).
    pub fn end_ms(&self) -> i64 {
        let ms = self.duration().as_nanos().div_ceil(1_000_000);
        self.start_ms
            .saturating_add(i64::try_from(ms).unwrap_or(i64::MAX))
    }

    /// One narration holding `self` and `later` (same base and options) at their instants,
    /// silence between them; where they overlap, `later` wins.
    pub fn merge(self, later: Narration) -> Narration {
        let _ = later;
        self
    }
}

/// The `audio.opus` member holding `narration`.
pub fn encode_narration(narration: &Narration) -> Result<Vec<u8>, MediaError> {
    let _ = narration;
    Err(MediaError::Encoding {
        codec: "Opus",
        message: "not implemented yet".into(),
    })
}

/// Reads an `audio.opus` member. [`MediaError::InvalidFile`] when it is not Ogg Opus.
pub fn decode_narration(bytes: &[u8]) -> Result<Narration, MediaError> {
    let _ = bytes;
    Err(MediaError::InvalidFile("not implemented yet".into()))
}

/// `clip` as a 16-bit PCM WAV file (the editor plays it during the replay).
pub fn wav_bytes(clip: &AudioClip) -> Vec<u8> {
    let _ = clip;
    Vec::new()
}
