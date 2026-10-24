//! Opus encoding of the narration, for WebM and for the MP4 written through the OpenH264
//! fallback (SPEC §6.2, row "Micro").
//!
//! The encoder is `ropus`, a pure-Rust port of libopus (BSD-3-Clause, bit-exact with libopus
//! 1.5.2): no C toolchain and no cmake are needed, unlike the `opus` crate.
//!
//! How the output maps onto a container (RFC 7845, Matroska and ISO BMFF Opus mappings):
//!
//! - Every packet holds 20 ms of sound (960 samples per channel at 48 kHz), except the last
//!   ones written by [`OpusEncoder::finish`] (10, 5 and 2.5 ms frames), so the end of the track
//!   is less than 2.5 ms past the end of the narration.
//! - The encoder adds [`OpusEncoder::pre_skip`] samples of delay: a decoder must drop that many
//!   samples at the start. [`OpusPacket::start`] counts samples on this encoded timeline:
//!   narration sample `n` is at encoded position `n + pre_skip`. In Matroska the first block is
//!   at time zero and `CodecDelay` (or the `OpusHead` pre-skip) tells players what to drop.
//! - [`OpusEncoder::head`] is the `OpusHead` header: Matroska's `CodecPrivate` as is, and the
//!   source of the fields of MP4's `dOps` box.

use std::time::Duration;

use ropus::{Application, Bitrate, Channels, DecodeMode, Decoder, Encoder};

use crate::{MediaError, NARRATION_SAMPLE_RATE};

/// Samples per channel in a regular packet: 20 ms at 48 kHz.
pub const OPUS_FRAME_SAMPLES: u32 = 960;

/// Shorter legal frame sizes, used only for the end of the stream (2.5, 5 and 10 ms).
const TAIL_FRAME_SAMPLES: [u32; 3] = [120, 240, 480];

/// Largest packet libopus produces for one frame is 1275 bytes; room for any frame size.
const MAX_PACKET_BYTES: usize = 4000;

/// Longest frame a decoder may return: 120 ms at 48 kHz, per channel.
const MAX_DECODED_SAMPLES: usize = 5760;

/// Default bitrate for narration: transparent for a voice (Opus is designed for this range),
/// about 0.5 MB per minute in mono.
pub fn default_opus_bitrate(channels: u16) -> u32 {
    if channels >= 2 { 96_000 } else { 64_000 }
}

/// One Opus packet and where it sits on the encoded timeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpusPacket {
    pub data: Vec<u8>,
    /// Position of the packet's first sample, in 48 kHz samples per channel, on the encoded
    /// timeline (which starts [`OpusEncoder::pre_skip`] samples before the narration).
    pub start: u64,
    /// Samples per channel decoded from the packet.
    pub samples: u32,
}

impl OpusPacket {
    /// Start of the packet on the encoded timeline (the container timestamp of the packet).
    pub fn timestamp(&self) -> Duration {
        samples_to_duration(self.start)
    }

    /// How long the packet plays.
    pub fn duration(&self) -> Duration {
        samples_to_duration(u64::from(self.samples))
    }
}

/// Encodes interleaved 16-bit narration at [`NARRATION_SAMPLE_RATE`] into Opus packets.
pub struct OpusEncoder {
    encoder: Encoder,
    channels: u16,
    pre_skip: u16,
    /// Interleaved samples not yet encoded (less than one 20 ms frame).
    pending: Vec<i16>,
    /// Samples per channel pushed by the caller (the narration's length).
    input_samples: u64,
    /// Encoded timeline position of the next packet.
    next_start: u64,
    output: Vec<u8>,
}

impl std::fmt::Debug for OpusEncoder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpusEncoder")
            .field("channels", &self.channels)
            .field("pre_skip", &self.pre_skip)
            .field("input_samples", &self.input_samples)
            .field("next_start", &self.next_start)
            .finish_non_exhaustive()
    }
}

impl OpusEncoder {
    /// An encoder for `channels` (1 or 2) channels at `bitrate` bits per second (6 000 to
    /// 256 000 per channel), tuned for general audio (narration may contain music).
    pub fn new(channels: u16, bitrate: u32) -> Result<Self, MediaError> {
        let layout = match channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => {
                return Err(MediaError::InvalidSettings(format!(
                    "{channels} Opus channels (mono or stereo only)"
                )));
            }
        };
        let max_bitrate = 256_000 * u32::from(channels);
        if !(6_000..=max_bitrate).contains(&bitrate) {
            return Err(MediaError::InvalidSettings(format!(
                "Opus bitrate {bitrate} bit/s (must be from 6000 to {max_bitrate})"
            )));
        }
        let encoder = Encoder::builder(NARRATION_SAMPLE_RATE, layout, Application::Audio)
            .bitrate(Bitrate::Bits(bitrate))
            .build()
            .map_err(|e| opus_error(&e))?;
        let pre_skip = u16::try_from(encoder.lookahead()).map_err(|_| MediaError::Encoding {
            codec: "Opus",
            message: format!("encoder delay of {} samples", encoder.lookahead()),
        })?;
        Ok(Self {
            encoder,
            channels,
            pre_skip,
            pending: Vec::with_capacity(OPUS_FRAME_SAMPLES as usize * usize::from(channels)),
            input_samples: 0,
            next_start: 0,
            output: vec![0; MAX_PACKET_BYTES],
        })
    }

    pub fn channels(&self) -> u16 {
        self.channels
    }

    /// Samples per channel (at 48 kHz) a decoder drops at the start of the stream.
    pub fn pre_skip(&self) -> u16 {
        self.pre_skip
    }

    /// Length of the narration pushed so far.
    pub fn input_duration(&self) -> Duration {
        samples_to_duration(self.input_samples)
    }

    /// Encoded timeline position of the next packet this encoder will produce: every packet
    /// still to come starts there or later.
    pub fn next_packet_start(&self) -> u64 {
        self.next_start
    }

    /// The `OpusHead` identification header (RFC 7845 §5.1): 19 bytes, channel mapping
    /// family 0, no output gain.
    pub fn head(&self) -> Vec<u8> {
        OpusHead {
            channels: self.channels,
            pre_skip: self.pre_skip,
            input_sample_rate: NARRATION_SAMPLE_RATE,
        }
        .to_bytes()
    }

    /// Encodes interleaved samples and returns the packets completed so far (a packet every
    /// 20 ms; what does not fill a packet waits for the next call or [`Self::finish`]).
    pub fn push(&mut self, samples: &[i16]) -> Result<Vec<OpusPacket>, MediaError> {
        let channels = usize::from(self.channels);
        if !samples.len().is_multiple_of(channels) {
            return Err(MediaError::AudioLength {
                len: samples.len(),
                channels: self.channels,
            });
        }
        self.input_samples += (samples.len() / channels) as u64;
        self.pending.extend_from_slice(samples);
        self.encode_full_frames()
    }

    /// Encodes what is left: the pending samples followed by `pre_skip` samples of silence
    /// (the encoder's delay, so the end of the narration comes out of a decoder), in 20 ms
    /// packets, then 10, 5 and 2.5 ms ones, the last padded with silence to a whole 2.5 ms.
    pub fn finish(mut self) -> Result<Vec<OpusPacket>, MediaError> {
        let channels = usize::from(self.channels);
        self.pending.resize(
            self.pending.len() + usize::from(self.pre_skip) * channels,
            0,
        );
        let mut packets = self.encode_full_frames()?;
        let smallest = TAIL_FRAME_SAMPLES[0] as usize;
        let padded = (self.pending.len() / channels).div_ceil(smallest) * smallest;
        self.pending.resize(padded * channels, 0);
        let tail = std::mem::take(&mut self.pending);
        let mut rest = tail.as_slice();
        for size in TAIL_FRAME_SAMPLES.into_iter().rev() {
            let len = size as usize * channels;
            if rest.len() >= len {
                let (pcm, after) = rest.split_at(len);
                packets.push(self.encode_frame(pcm)?);
                rest = after;
            }
        }
        Ok(packets)
    }

    fn encode_full_frames(&mut self) -> Result<Vec<OpusPacket>, MediaError> {
        let frame_len = OPUS_FRAME_SAMPLES as usize * usize::from(self.channels);
        let frames = self.pending.len() / frame_len;
        let mut packets = Vec::with_capacity(frames);
        let pending = std::mem::take(&mut self.pending);
        let mut chunks = pending.chunks_exact(frame_len);
        for pcm in chunks.by_ref() {
            packets.push(self.encode_frame(pcm)?);
        }
        self.pending = chunks.remainder().to_vec();
        Ok(packets)
    }

    fn encode_frame(&mut self, pcm: &[i16]) -> Result<OpusPacket, MediaError> {
        let len = self
            .encoder
            .encode(pcm, &mut self.output)
            .map_err(|e| opus_error(&e))?;
        let samples = (pcm.len() / usize::from(self.channels)) as u32;
        let packet = OpusPacket {
            data: self.output[..len].to_vec(),
            start: self.next_start,
            samples,
        };
        self.next_start += u64::from(samples);
        Ok(packet)
    }
}

/// The fields of an `OpusHead` header that matter here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpusHead {
    pub channels: u16,
    pub pre_skip: u16,
    /// Sample rate of the original input (informative: Opus always decodes at 48 kHz here).
    pub input_sample_rate: u32,
}

impl OpusHead {
    const MAGIC: &'static [u8; 8] = b"OpusHead";
    const LEN: usize = 19;

    /// Serialises the header (version 1, no gain, channel mapping family 0).
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut head = Vec::with_capacity(Self::LEN);
        head.extend_from_slice(Self::MAGIC);
        head.push(1); // version
        head.push(self.channels as u8);
        head.extend_from_slice(&self.pre_skip.to_le_bytes());
        head.extend_from_slice(&self.input_sample_rate.to_le_bytes());
        head.extend_from_slice(&0i16.to_le_bytes()); // output gain
        head.push(0); // channel mapping family: mono or stereo
        head
    }

    /// Reads a header written by any Opus muxer (mapping family 0 or 1).
    pub fn parse(bytes: &[u8]) -> Result<Self, MediaError> {
        let invalid = |what: &str| MediaError::InvalidFile(format!("OpusHead: {what}"));
        if bytes.len() < Self::LEN || &bytes[..8] != Self::MAGIC {
            return Err(invalid("missing or truncated"));
        }
        if bytes[8] & 0xF0 != 0 {
            return Err(invalid("unsupported version"));
        }
        let channels = u16::from(bytes[9]);
        if channels == 0 {
            return Err(invalid("no channel"));
        }
        Ok(Self {
            channels,
            pre_skip: u16::from_le_bytes([bytes[10], bytes[11]]),
            input_sample_rate: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        })
    }
}

/// Decodes Opus packets only to count the samples they hold (probes of the files).
pub(crate) struct OpusPacketDecoder {
    decoder: Decoder,
    output: Vec<i16>,
}

impl OpusPacketDecoder {
    pub(crate) fn new(channels: u16) -> Result<Self, MediaError> {
        let layout = match channels {
            1 => Channels::Mono,
            2 => Channels::Stereo,
            _ => {
                return Err(MediaError::InvalidFile(format!(
                    "{channels} Opus channels (only mono and stereo are read)"
                )));
            }
        };
        let decoder = Decoder::new(NARRATION_SAMPLE_RATE, layout)
            .map_err(|e| MediaError::InvalidFile(format!("Opus decoder: {e}")))?;
        Ok(Self {
            decoder,
            output: vec![0; MAX_DECODED_SAMPLES * usize::from(channels)],
        })
    }

    /// Decodes one packet; returns its samples per channel.
    pub(crate) fn decode(&mut self, packet: &[u8]) -> Result<usize, MediaError> {
        if packet.is_empty() {
            return Err(MediaError::InvalidFile("empty Opus packet".into()));
        }
        self.decoder
            .decode(packet, &mut self.output, DecodeMode::Normal)
            .map_err(|e| MediaError::InvalidFile(format!("Opus packet: {e}")))
    }
}

/// 48 kHz samples to a duration.
pub(crate) fn samples_to_duration(samples: u64) -> Duration {
    let nanos = u128::from(samples) * 1_000_000_000 / u128::from(NARRATION_SAMPLE_RATE);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

fn opus_error(error: &dyn std::fmt::Display) -> MediaError {
    MediaError::Encoding {
        codec: "Opus",
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `seconds` of a 440 Hz tone, interleaved, quarter of full scale.
    fn tone(channels: u16, seconds: f64) -> Vec<i16> {
        let frames = (seconds * f64::from(NARRATION_SAMPLE_RATE)).round() as usize;
        (0..frames)
            .flat_map(|i| {
                let t = i as f64 / f64::from(NARRATION_SAMPLE_RATE);
                let s = (t * 440.0 * std::f64::consts::TAU).sin() * 8000.0;
                std::iter::repeat_n(s as i16, usize::from(channels))
            })
            .collect()
    }

    #[test]
    fn head_round_trips() {
        let encoder = OpusEncoder::new(2, 96_000).unwrap();
        let head = encoder.head();
        assert_eq!(head.len(), 19);
        assert_eq!(&head[..8], b"OpusHead");
        let parsed = OpusHead::parse(&head).unwrap();
        assert_eq!(parsed.channels, 2);
        assert_eq!(parsed.pre_skip, encoder.pre_skip());
        assert_eq!(parsed.input_sample_rate, 48_000);
        assert!(OpusHead::parse(&head[..10]).is_err());
        assert!(OpusHead::parse(b"OpusTags...........").is_err());
    }

    #[test]
    fn rejects_bad_settings() {
        assert!(matches!(
            OpusEncoder::new(3, 64_000),
            Err(MediaError::InvalidSettings(_))
        ));
        assert!(matches!(
            OpusEncoder::new(1, 1_000),
            Err(MediaError::InvalidSettings(_))
        ));
        let mut encoder = OpusEncoder::new(2, 96_000).unwrap();
        assert!(matches!(
            encoder.push(&[0; 3]),
            Err(MediaError::AudioLength {
                len: 3,
                channels: 2
            })
        ));
    }

    #[test]
    fn packets_are_contiguous_and_decode_to_the_input_length() {
        for channels in [1u16, 2] {
            let mut encoder = OpusEncoder::new(channels, default_opus_bitrate(channels)).unwrap();
            let pre_skip = u64::from(encoder.pre_skip());
            let pcm = tone(channels, 1.234);
            let mut packets = Vec::new();
            // Odd chunk sizes: packets must not depend on how the input was split.
            for chunk in pcm.chunks(usize::from(channels) * 777) {
                packets.extend(encoder.push(chunk).unwrap());
            }
            assert_eq!(encoder.input_duration(), Duration::from_millis(1234));
            packets.extend(encoder.finish().unwrap());

            let mut decoder = OpusPacketDecoder::new(channels).unwrap();
            let mut position = 0u64;
            let mut decoded = 0u64;
            for packet in &packets {
                assert_eq!(packet.start, position, "packets must be contiguous");
                position += u64::from(packet.samples);
                decoded += decoder.decode(&packet.data).unwrap() as u64;
            }
            assert_eq!(decoded, position);
            let input = (pcm.len() / usize::from(channels)) as u64;
            let output = decoded - pre_skip;
            // Everything comes out, padded by less than one 2.5 ms frame.
            assert!(
                output >= input && output < input + 120,
                "{channels} ch: {output} samples decoded for {input}"
            );
        }
    }

    #[test]
    fn the_tone_survives_the_round_trip() {
        let mut encoder = OpusEncoder::new(1, 64_000).unwrap();
        let pre_skip = usize::from(encoder.pre_skip());
        let pcm = tone(1, 0.5);
        let mut packets = encoder.push(&pcm).unwrap();
        packets.extend(encoder.finish().unwrap());
        let mut decoder = Decoder::new(NARRATION_SAMPLE_RATE, Channels::Mono).unwrap();
        let mut decoded = Vec::new();
        let mut buffer = vec![0i16; MAX_DECODED_SAMPLES];
        for packet in &packets {
            let n = decoder
                .decode(&packet.data, &mut buffer, DecodeMode::Normal)
                .unwrap();
            decoded.extend_from_slice(&buffer[..n]);
        }
        // Signal-to-noise ratio of the decoded tone, aligned on the pre-skip, after the first
        // 20 ms (the encoder's start-up).
        let (signal, noise) = pcm.iter().zip(&decoded[pre_skip..]).skip(960).fold(
            (0f64, 0f64),
            |(s, n), (&a, &b)| {
                let (a, b) = (f64::from(a), f64::from(b));
                (s + a * a, n + (a - b) * (a - b))
            },
        );
        let snr = 10.0 * (signal / noise).log10();
        assert!(snr > 20.0, "SNR {snr:.1} dB");
    }

    #[test]
    fn tail_uses_short_frames() {
        let mut encoder = OpusEncoder::new(1, 64_000).unwrap();
        let pre_skip = usize::from(encoder.pre_skip());
        // Exactly one 20 ms frame of input: the tail is only the encoder delay.
        let packets = encoder.push(&vec![0; 960]).unwrap();
        assert_eq!(packets.len(), 1);
        let tail = encoder.finish().unwrap();
        let tail_samples: u32 = tail.iter().map(|p| p.samples).sum();
        assert!(tail_samples as usize >= pre_skip);
        assert!((tail_samples as usize) < pre_skip + 120);
        assert!(tail.iter().all(|p| p.samples < 960));
    }

    #[test]
    fn durations() {
        assert_eq!(samples_to_duration(48_000), Duration::from_secs(1));
        let packet = OpusPacket {
            data: vec![],
            start: 960,
            samples: 480,
        };
        assert_eq!(packet.timestamp(), Duration::from_millis(20));
        assert_eq!(packet.duration(), Duration::from_millis(10));
    }
}
