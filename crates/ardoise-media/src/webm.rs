//! WebM export (SPEC §4.5, §6.2 row "Vidéo WebM"): AV1 video encoded by `rav1e` (BSD-2-Clause,
//! pure Rust, with x86-64 assembly when built with nasm), narration in Opus (`opus.rs`), muxed
//! by libwebm through the `webm` crate (MPL-2.0, C++ compiled by `cc`).
//!
//! - **Video**: RGBA converted to 8-bit 4:2:0 BT.709 limited range (the conversion of the MP4
//!   export, `nv12.rs`), declared both in the AV1 sequence header and in the Matroska `Colour`
//!   element. Constant quantizer (no bitrate target): a page of handwriting is mostly flat and
//!   costs little at any quantizer. Frames keep their own timestamps (Matroska blocks carry
//!   them; no fixed frame rate is assumed).
//! - **Speed**: AV1 is slow, so the defaults are rav1e's fastest preset (speed 10), low latency
//!   (no frame reordering: twice as fast, files about 30 % bigger) and tiles (about one per
//!   512 × 512 pixels, encoded in parallel: twice as fast at 1080p). Measured at 1080p, 30 fps,
//!   on the development PC (Core i5-1235U, rav1e built with nasm and optimised): 20 to 24
//!   frames per second, so a 2-minute replay takes 2.5 to 3 minutes, for about 100 kB per 10 s
//!   of writing (`speed_1080p` benchmark below). Still far slower than Media Foundation's
//!   H.264; SPEC §8 accepts it, the interface shows the estimated time first.
//! - **Interleaving**: libwebm needs blocks in time order across tracks, but rav1e returns
//!   packets some frames late. Audio and video packets wait in two queues and are written in
//!   time order as soon as no earlier packet can still come. If the narration stops before the
//!   video, the remaining video packets wait in memory until [`VideoSink::finish`] (a few
//!   kilobytes per frame of handwriting).
//! - **Sound**: Opus at 48 kHz, `OpusHead` as `CodecPrivate`. The `webm` crate cannot write
//!   `CodecDelay`/`SeekPreRoll`; players take the pre-skip from `OpusHead`. Without
//!   `DiscardPadding`, the track ends less than 2.5 ms after the narration.
//! - Windows reads these files too: Media Foundation's WebM source with its AV1 and Opus
//!   decoders gave the right size, duration and sound on the development PC (it resamples
//!   video to 30 fps for lack of a `DefaultDuration`, which the `webm` crate cannot write).
//!
//! [`probe_webm`] reads a file back with `matroska-demuxer` (pure Rust, independent of
//! libwebm): container duration, AV1 track size, video blocks counted (AV1 is not decoded), and
//! every Opus packet decoded to measure the sound's real length.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use matroska_demuxer::{Frame as MkvFrame, MatroskaFile, TrackType};
use rav1e::color::{
    ColorDescription, ColorPrimaries, MatrixCoefficients, PixelRange, TransferCharacteristics,
};
use rav1e::config::SpeedSettings;
use rav1e::data::FrameType;
use rav1e::{Config, Context, EncoderConfig, EncoderStatus};
use webm::mux::{
    AudioCodecId, AudioTrack, ColorRange, ColorSubsampling, Segment, SegmentBuilder, VideoCodecId,
    VideoTrack, Writer,
};

use crate::gif::validate_settings;
use crate::opus::{OpusEncoder, OpusHead, OpusPacketDecoder, default_opus_bitrate};
use crate::{
    AnimationFormat, AnimationProbe, AudioProbe, MediaError, NARRATION_SAMPLE_RATE, VideoSettings,
    VideoSink, VideoSummary, nv12,
};

/// Options of a WebM export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WebmOptions {
    /// rav1e speed preset, 0 (slowest, smallest) to 10 (fastest).
    pub speed: u8,
    /// AV1 quantizer, 0 (lossless) to 255 (worst). rav1e's default is 100.
    pub quantizer: u8,
    /// No frame reordering and a short lookahead: about twice as fast, files about 30 % bigger.
    pub low_latency: bool,
    /// Opus bitrate in bits per second; `None` for [`default_opus_bitrate`].
    pub audio_bitrate: Option<u32>,
}

impl Default for WebmOptions {
    fn default() -> Self {
        Self {
            speed: 10,
            quantizer: 100,
            low_latency: true,
            audio_bitrate: None,
        }
    }
}

/// Largest frame side accepted (AV1 levels go to 8K).
const WEBM_MAX_DIMENSION: u32 = 8192;
/// Matroska timestamps are in milliseconds here (libwebm's default timecode scale).
const NANOS_PER_TICK: u64 = 1_000_000;
/// A key frame at least every 10 s, so players can seek.
const KEY_FRAME_INTERVAL_SECONDS: u64 = 10;
/// A Temporal Delimiter OBU (type 2, with an empty size field): rav1e starts every packet
/// with one; the Matroska AV1 mapping says blocks should not contain it.
const TEMPORAL_DELIMITER: [u8; 2] = [0x12, 0x00];

/// Starts a WebM export to `path` (created or replaced).
pub fn create_webm(
    path: &Path,
    settings: &VideoSettings,
    options: &WebmOptions,
) -> Result<Box<dyn VideoSink>, MediaError> {
    validate_settings(settings, 16, WEBM_MAX_DIMENSION, true)?;
    if options.speed > 10 {
        return Err(MediaError::InvalidSettings(format!(
            "AV1 speed {} (must be from 0 to 10)",
            options.speed
        )));
    }
    let (width, height) = (settings.width as usize, settings.height as usize);
    let context: Context<u8> = Config::new()
        .with_encoder_config(encoder_config(settings, options))
        .new_context()
        .map_err(|e| MediaError::Encoding {
            codec: "AV1",
            message: e.to_string(),
        })?;

    let opus = match settings.audio_channels {
        Some(channels) => Some(OpusEncoder::new(
            channels,
            options
                .audio_bitrate
                .unwrap_or_else(|| default_opus_bitrate(channels)),
        )?),
        None => None,
    };

    let file = BufWriter::new(File::create(path)?);
    let builder = SegmentBuilder::new(Writer::new(file))
        .and_then(|b| b.set_writing_app("Ardoise"))
        .map_err(webm_error)?;
    let (builder, video_track) = builder
        .add_video_track(settings.width, settings.height, VideoCodecId::AV1, None)
        .map_err(webm_error)?;
    let builder = builder
        .set_codec_private(video_track, &context.container_sequence_header())
        .and_then(|b| {
            b.set_color(
                video_track,
                8,
                ColorSubsampling {
                    chroma_horizontal: 1,
                    chroma_vertical: 1,
                },
                ColorRange::Broadcast,
            )
        })
        .and_then(|b| b.set_primaries(video_track, webm::mux::ColorPrimaries::Bt709))
        .and_then(|b| {
            b.set_transfer_characteristics(video_track, webm::mux::TransferCharacteristics::Bt709)
        })
        .and_then(|b| b.set_matrix_coefficients(video_track, webm::mux::MatrixCoefficients::Bt709))
        .map_err(webm_error)?;
    let (builder, audio_track) = match &opus {
        Some(opus) => {
            let (builder, track) = builder
                .add_audio_track(
                    NARRATION_SAMPLE_RATE,
                    u32::from(opus.channels()),
                    AudioCodecId::Opus,
                    None,
                )
                .map_err(webm_error)?;
            let builder = builder
                .set_codec_private(track, &opus.head())
                .map_err(webm_error)?;
            (builder, Some(track))
        }
        None => (builder, None),
    };

    Ok(Box::new(WebmSink {
        width,
        height,
        frame_period: Duration::from_secs(1) / settings.frame_rate,
        context,
        segment: builder.build(),
        video_track,
        audio_track,
        opus,
        nv12: vec![0; nv12::nv12_len(width, height)],
        frame_times: VecDeque::new(),
        frames_sent: 0,
        packets_received: 0,
        last_timestamp: None,
        video_queue: VecDeque::new(),
        audio_queue: VecDeque::new(),
        settings: *options,
    }))
}

/// rav1e's settings for an export.
fn encoder_config(settings: &VideoSettings, options: &WebmOptions) -> EncoderConfig {
    let (width, height) = (settings.width as usize, settings.height as usize);
    let frame_rate = u64::from(settings.frame_rate);
    EncoderConfig {
        width,
        height,
        time_base: rav1e::data::Rational::new(1, frame_rate),
        bit_depth: 8,
        pixel_range: PixelRange::Limited,
        color_description: Some(ColorDescription {
            color_primaries: ColorPrimaries::BT709,
            transfer_characteristics: TransferCharacteristics::BT709,
            matrix_coefficients: MatrixCoefficients::BT709,
        }),
        min_key_frame_interval: frame_rate,
        max_key_frame_interval: frame_rate * KEY_FRAME_INTERVAL_SECONDS,
        low_latency: options.low_latency,
        quantizer: usize::from(options.quantizer),
        bitrate: 0,
        tiles: tile_count(width, height),
        speed_settings: SpeedSettings::from_preset(options.speed),
        ..EncoderConfig::default()
    }
}

/// Tiles let rav1e encode parts of a frame in parallel: about one per 512 × 512 pixels, at
/// most the number of processors (rav1e rounds to a valid tiling).
fn tile_count(width: usize, height: usize) -> usize {
    let cpus = std::thread::available_parallelism().map_or(1, usize::from);
    let by_area = (width * height).div_ceil(512 * 512);
    by_area.clamp(1, cpus.clamp(1, 16))
}

/// A block waiting to be muxed.
struct Block {
    /// Nanoseconds.
    timestamp: u64,
    data: Vec<u8>,
    keyframe: bool,
}

struct WebmSink {
    width: usize,
    height: usize,
    frame_period: Duration,
    context: Context<u8>,
    segment: Segment<BufWriter<File>>,
    video_track: VideoTrack,
    audio_track: Option<AudioTrack>,
    opus: Option<OpusEncoder>,
    nv12: Vec<u8>,
    /// Timestamps (ns) of the frames given to rav1e whose packet has not come out yet.
    frame_times: VecDeque<u64>,
    frames_sent: u64,
    packets_received: u64,
    last_timestamp: Option<Duration>,
    video_queue: VecDeque<Block>,
    audio_queue: VecDeque<Block>,
    settings: WebmOptions,
}

impl WebmSink {
    /// Copies the frame into a rav1e frame (Y, U, V planes).
    fn rav1e_frame(&mut self, rgba: &[u8]) -> Result<rav1e::Frame<u8>, MediaError> {
        nv12::rgba_to_nv12(rgba, self.width, self.height, &mut self.nv12)?;
        let (y, uv) = self.nv12.split_at(self.width * self.height);
        let chroma_len = uv.len() / 2;
        let mut u = Vec::with_capacity(chroma_len);
        let mut v = Vec::with_capacity(chroma_len);
        for &[cb, cr] in uv.as_chunks::<2>().0 {
            u.push(cb);
            v.push(cr);
        }
        let mut frame = self.context.new_frame();
        frame.planes[0].copy_from_raw_u8(y, self.width, 1);
        frame.planes[1].copy_from_raw_u8(&u, self.width / 2, 1);
        frame.planes[2].copy_from_raw_u8(&v, self.width / 2, 1);
        // rav1e pads a frame it owns alone; this one is shared (kept for a retry), so pad it
        // here or `send_frame` panics.
        for plane in &mut frame.planes {
            plane.pad(self.width, self.height);
        }
        Ok(frame)
    }

    /// Moves every packet rav1e has ready into the video queue.
    fn receive_packets(&mut self) -> Result<(), MediaError> {
        loop {
            match self.context.receive_packet() {
                Ok(packet) => {
                    if packet.input_frameno != self.packets_received {
                        return Err(av1_error(format!(
                            "packet for frame {} where frame {} was expected",
                            packet.input_frameno, self.packets_received
                        )));
                    }
                    let timestamp = self
                        .frame_times
                        .pop_front()
                        .ok_or_else(|| av1_error("packet without a frame".into()))?;
                    let data = match packet.data.strip_prefix(&TEMPORAL_DELIMITER) {
                        Some(rest) => rest.to_vec(),
                        None => packet.data,
                    };
                    self.video_queue.push_back(Block {
                        timestamp,
                        data,
                        keyframe: packet.frame_type == FrameType::KEY,
                    });
                    self.packets_received += 1;
                }
                Err(EncoderStatus::Encoded) => {}
                Err(EncoderStatus::NeedMoreData | EncoderStatus::LimitReached) => return Ok(()),
                Err(e) => return Err(av1_error(e.to_string())),
            }
        }
    }

    fn queue_audio(&mut self, packets: Vec<crate::opus::OpusPacket>) {
        self.audio_queue
            .extend(packets.into_iter().map(|packet| Block {
                timestamp: samples_to_nanos(packet.start),
                data: packet.data,
                keyframe: true,
            }));
    }

    /// Writes queued blocks in time order, as long as no packet still to come can be earlier
    /// (everything when `flushing`).
    fn mux(&mut self, flushing: bool) -> Result<(), MediaError> {
        // Earliest timestamp a video packet not yet queued can have.
        let video_bound = if flushing {
            u64::MAX
        } else {
            self.frame_times
                .front()
                .copied()
                .or_else(|| self.last_timestamp.map(duration_nanos))
                .unwrap_or(0)
        };
        // The same for audio.
        let audio_bound = match (&self.opus, flushing) {
            (Some(opus), false) => samples_to_nanos(opus.next_packet_start()),
            _ => u64::MAX,
        };
        loop {
            let take_video = match (self.video_queue.front(), self.audio_queue.front()) {
                (Some(v), Some(a)) => v.timestamp <= a.timestamp,
                (Some(v), None) if v.timestamp <= audio_bound => true,
                (None, Some(a)) if a.timestamp <= video_bound => false,
                _ => return Ok(()),
            };
            let (track, block) = if take_video {
                (u64::from(self.video_track), self.video_queue.pop_front())
            } else {
                let track = self.audio_track.map(u64::from).unwrap_or(0);
                (track, self.audio_queue.pop_front())
            };
            if let Some(block) = block {
                self.segment
                    .add_frame(track, &block.data, block.timestamp, block.keyframe)
                    .map_err(webm_error)?;
            }
        }
    }
}

impl VideoSink for WebmSink {
    fn push_frame(&mut self, rgba: &[u8], timestamp: Duration) -> Result<(), MediaError> {
        let expected = self.width * self.height * 4;
        if rgba.len() != expected {
            return Err(MediaError::FrameSize {
                expected,
                actual: rgba.len(),
            });
        }
        if let Some(previous) = self.last_timestamp
            && timestamp <= previous
        {
            return Err(MediaError::TimestampOrder {
                previous,
                timestamp,
            });
        }
        let frame = Arc::new(self.rav1e_frame(rgba)?);
        self.frame_times.push_back(duration_nanos(timestamp));
        self.last_timestamp = Some(timestamp);
        match self.context.send_frame(Arc::clone(&frame)) {
            Ok(()) => {}
            Err(EncoderStatus::EnoughData) => {
                self.receive_packets()?;
                self.context
                    .send_frame(frame)
                    .map_err(|e| av1_error(e.to_string()))?;
            }
            Err(e) => return Err(av1_error(e.to_string())),
        }
        self.frames_sent += 1;
        self.receive_packets()?;
        self.mux(false)
    }

    fn push_audio(&mut self, samples: &[i16]) -> Result<(), MediaError> {
        let Some(opus) = self.opus.as_mut() else {
            return Err(MediaError::NoAudioTrack);
        };
        let packets = opus.push(samples)?;
        self.queue_audio(packets);
        self.mux(false)
    }

    fn finish(mut self: Box<Self>) -> Result<VideoSummary, MediaError> {
        let Some(last) = self.last_timestamp else {
            return Err(MediaError::InvalidSettings(
                "no video frame was pushed".into(),
            ));
        };
        self.context.flush();
        self.receive_packets()?;
        if self.packets_received != self.frames_sent {
            return Err(av1_error(format!(
                "{} packets for {} frames",
                self.packets_received, self.frames_sent
            )));
        }
        let audio_end = match self.opus.take() {
            Some(opus) => {
                let end = opus.input_duration();
                let packets = opus.finish()?;
                self.queue_audio(packets);
                Some(end)
            }
            None => None,
        };
        self.mux(true)?;

        // The container's duration, in its millisecond ticks (what players and the probe read).
        let end = last
            .saturating_add(self.frame_period)
            .max(audio_end.unwrap_or_default());
        let ticks = ((duration_nanos(end) + NANOS_PER_TICK / 2) / NANOS_PER_TICK).max(1);
        let duration = Duration::from_nanos(ticks * NANOS_PER_TICK);
        let WebmSink {
            segment,
            frames_sent,
            settings,
            ..
        } = *self;
        let writer = segment
            .finalize(Some(ticks))
            .map_err(|_| webm_error(webm::mux::Error::Unknown))?;
        let mut file = writer.into_inner();
        file.flush()?;
        file.into_inner()
            .map_err(|e| MediaError::Io(e.into_error()))?
            .sync_all()?;

        let asm = if cfg!(feature = "asm") {
            ""
        } else {
            ", no assembly"
        };
        Ok(VideoSummary {
            format: AnimationFormat::WebM,
            frames: frames_sent,
            duration,
            audio: audio_end.is_some(),
            encoder: format!(
                "rav1e (AV1, speed {}, quantizer {}{}{asm})",
                settings.speed,
                settings.quantizer,
                if settings.low_latency {
                    ", low latency"
                } else {
                    ""
                }
            ),
        })
    }
}

fn duration_nanos(duration: Duration) -> u64 {
    u64::try_from(duration.as_nanos()).unwrap_or(u64::MAX)
}

fn samples_to_nanos(samples: u64) -> u64 {
    duration_nanos(crate::opus::samples_to_duration(samples))
}

fn av1_error(message: String) -> MediaError {
    MediaError::Encoding {
        codec: "AV1",
        message,
    }
}

fn webm_error(error: webm::mux::Error) -> MediaError {
    MediaError::Encoding {
        codec: "WebM",
        message: format!("libwebm: {error}"),
    }
}

/// Reads a WebM file back with `matroska-demuxer`: container duration, the AV1 track's size
/// and block count, and the Opus track decoded packet by packet.
pub fn probe_webm(path: &Path) -> Result<AnimationProbe, MediaError> {
    let invalid = |what: String| MediaError::InvalidFile(format!("WebM: {what}"));
    let mut mkv = MatroskaFile::open(BufReader::new(File::open(path)?))
        .map_err(|e| invalid(e.to_string()))?;

    let scale = mkv.info().timestamp_scale().get();
    let video = mkv
        .tracks()
        .iter()
        .find(|t| t.track_type() == TrackType::Video)
        .ok_or_else(|| invalid("no video track".into()))?;
    if video.codec_id() != "V_AV1" {
        return Err(invalid(format!("video codec {}", video.codec_id())));
    }
    let video_number = video.track_number().get();
    let (width, height) = video
        .video()
        .map(|v| (v.pixel_width().get(), v.pixel_height().get()))
        .ok_or_else(|| invalid("video track without size".into()))?;

    struct AudioTrackInfo {
        number: u64,
        channels: u16,
        sample_rate: u32,
        pre_skip: u64,
        decoder: OpusPacketDecoder,
        samples: u64,
    }
    let mut audio = match mkv
        .tracks()
        .iter()
        .find(|t| t.track_type() == TrackType::Audio)
    {
        Some(track) => {
            if track.codec_id() != "A_OPUS" {
                return Err(invalid(format!("audio codec {}", track.codec_id())));
            }
            let settings = track
                .audio()
                .ok_or_else(|| invalid("audio track without settings".into()))?;
            let channels = u16::try_from(settings.channels().get())
                .map_err(|_| invalid("too many audio channels".into()))?;
            let head = OpusHead::parse(track.codec_private().unwrap_or_default())?;
            Some(AudioTrackInfo {
                number: track.track_number().get(),
                channels,
                sample_rate: settings.sampling_frequency().round() as u32,
                pre_skip: u64::from(head.pre_skip),
                decoder: OpusPacketDecoder::new(channels)?,
                samples: 0,
            })
        }
        None => None,
    };

    let mut frames = 0u64;
    let mut last_video = 0u64;
    let mut frame = MkvFrame::default();
    while mkv
        .next_frame(&mut frame)
        .map_err(|e| invalid(e.to_string()))?
    {
        if frame.track == video_number {
            frames += 1;
            last_video = last_video.max(frame.timestamp);
        } else if let Some(audio) = audio.as_mut().filter(|a| a.number == frame.track) {
            audio.samples += audio.decoder.decode(&frame.data)? as u64;
        }
    }

    let nanos = |ticks: f64| Duration::from_nanos((ticks * scale as f64).round() as u64);
    let duration = match mkv.info().duration() {
        Some(ticks) => nanos(ticks),
        None => nanos(last_video as f64),
    };
    Ok(AnimationProbe {
        format: AnimationFormat::WebM,
        width: u32::try_from(width).map_err(|_| invalid("huge width".into()))?,
        height: u32::try_from(height).map_err(|_| invalid("huge height".into()))?,
        frames,
        duration,
        audio: audio.map(|a| AudioProbe {
            codec: "Opus".into(),
            channels: a.channels,
            sample_rate: a.sample_rate,
            duration: crate::opus::samples_to_duration(a.samples.saturating_sub(a.pre_skip)),
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gif::tests::{Handwriting, growing_line, print_probe_table, settings};

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

    /// Writes a growing line, `frames` frames at 15 fps, with `narration` (channels, seconds)
    /// pushed frame by frame and what remains pushed after the last frame.
    fn write_line(
        path: &Path,
        (width, height): (u32, u32),
        frames: u32,
        narration: Option<(u16, f64)>,
    ) -> (VideoSummary, Duration) {
        let fps = 15u32;
        let settings = VideoSettings {
            audio_channels: narration.map(|(channels, _)| channels),
            ..settings(width, height, fps)
        };
        let started = std::time::Instant::now();
        let mut sink = create_webm(path, &settings, &WebmOptions::default()).unwrap();
        let audio = narration.map_or_else(Vec::new, |(channels, seconds)| tone(channels, seconds));
        let per_frame = 48_000 / fps as usize * usize::from(settings.audio_channels.unwrap_or(1));
        let mut chunks = audio.chunks(per_frame);
        for i in 0..frames {
            // Each frame's audio goes in with (here: just before) the frame.
            if let Some(chunk) = chunks.next() {
                sink.push_audio(chunk).unwrap();
            }
            let frame = growing_line(width, height, i, frames);
            sink.push_frame(&frame, Duration::from_secs(u64::from(i)) / fps)
                .unwrap();
        }
        for chunk in chunks {
            sink.push_audio(chunk).unwrap();
        }
        (sink.finish().unwrap(), started.elapsed())
    }

    /// Asserts that `audio` lasts `seconds`, plus less than one 2.5 ms Opus frame of padding.
    fn assert_audio_duration(audio: &AudioProbe, seconds: f64) {
        let expected = Duration::from_secs_f64(seconds);
        let extra = audio.duration.saturating_sub(expected);
        assert!(
            audio.duration >= expected && extra < Duration::from_micros(2500),
            "{:?} for {expected:?}",
            audio.duration
        );
    }

    #[test]
    fn writes_and_probes_a_webm_with_sound() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("line.webm");
        let (summary, elapsed) = write_line(&path, (640, 360), 30, Some((2, 2.0)));
        let probe = probe_webm(&path).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        print_probe_table("WebM", &probe, size, elapsed);

        assert_eq!(summary.frames, 30);
        assert_eq!(summary.duration, Duration::from_secs(2));
        assert!(summary.audio);
        assert_eq!((probe.width, probe.height), (640, 360));
        assert_eq!(probe.frames, 30);
        assert_eq!(probe.duration, Duration::from_secs(2));
        let audio = probe.audio.expect("an Opus track");
        assert_eq!(audio.codec, "Opus");
        assert_eq!(audio.channels, 2);
        assert_eq!(audio.sample_rate, 48_000);
        assert_audio_duration(&audio, 2.0);
    }

    #[test]
    fn narration_shorter_or_longer_than_the_video() {
        let dir = tempfile::tempdir().unwrap();
        for (seconds, duration) in [(0.7, 2.0), (3.25, 3.25)] {
            let path = dir.path().join(format!("{seconds}.webm"));
            let (summary, _) = write_line(&path, (160, 96), 30, Some((1, seconds)));
            let probe = probe_webm(&path).unwrap();
            assert_eq!(summary.duration, Duration::from_secs_f64(duration));
            assert_eq!(probe.duration, Duration::from_secs_f64(duration));
            assert_eq!(probe.frames, 30);
            let audio = probe.audio.expect("an Opus track");
            assert_eq!(audio.channels, 1);
            assert_audio_duration(&audio, seconds);
        }
    }

    #[test]
    fn writes_and_probes_a_silent_webm() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("silent.webm");
        let (summary, _) = write_line(&path, (160, 96), 30, None);
        assert!(!summary.audio);
        let probe = probe_webm(&path).unwrap();
        assert_eq!((probe.width, probe.height), (160, 96));
        assert_eq!(probe.frames, 30);
        assert_eq!(probe.duration, Duration::from_secs(2));
        assert_eq!(probe.audio, None);
    }

    /// Writes `line.gif`, `line.webp` and `line.webm` (with a stereo tone) into the folder
    /// `ARDOISE_SAMPLES_DIR`, to try them in browsers and players.
    #[test]
    #[ignore = "writes sample files for manual checks"]
    fn write_samples() {
        let Some(dir) = std::env::var_os("ARDOISE_SAMPLES_DIR") else {
            panic!("set ARDOISE_SAMPLES_DIR to the folder to write the samples into");
        };
        let dir = std::path::PathBuf::from(dir);
        let (width, height, fps, frames) = (640, 360, 15, 30);
        let mut sinks = vec![
            crate::create_gif(
                &dir.join("line.gif"),
                &settings(width, height, fps),
                &crate::GifOptions::default(),
            )
            .unwrap(),
            crate::create_webp(
                &dir.join("line.webp"),
                &settings(width, height, fps),
                &crate::WebpOptions::default(),
            )
            .unwrap(),
        ];
        write_line(
            &dir.join("line.webm"),
            (width, height),
            frames,
            Some((2, 2.0)),
        );
        for i in 0..frames {
            let frame = growing_line(width, height, i, frames);
            for sink in &mut sinks {
                sink.push_frame(&frame, Duration::from_secs(u64::from(i)) / fps)
                    .unwrap();
            }
        }
        for sink in sinks {
            sink.finish().unwrap();
        }
    }

    #[test]
    fn rejects_bad_input() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.webm");
        assert!(matches!(
            create_webm(&path, &settings(641, 360, 15), &WebmOptions::default()),
            Err(MediaError::InvalidSettings(_))
        ));
        let mut sink = create_webm(&path, &settings(64, 32, 15), &WebmOptions::default()).unwrap();
        assert!(matches!(
            sink.push_audio(&[0; 2]),
            Err(MediaError::NoAudioTrack)
        ));
        assert!(matches!(
            sink.push_frame(&[0; 3], Duration::ZERO),
            Err(MediaError::FrameSize { .. })
        ));
        let frame = growing_line(64, 32, 0, 1);
        sink.push_frame(&frame, Duration::from_millis(10)).unwrap();
        assert!(matches!(
            sink.push_frame(&frame, Duration::from_millis(10)),
            Err(MediaError::TimestampOrder { .. })
        ));
    }

    /// PSNR of the luma of a reconstructed frame against its source.
    fn luma_psnr(packet: &rav1e::Packet<u8>) -> Option<f64> {
        let (rec, source) = (packet.rec.as_ref()?, packet.source.as_ref()?);
        let (a, b) = (&rec.planes[0], &source.planes[0]);
        let (width, height) = (a.cfg.width, a.cfg.height);
        let mut squared = 0f64;
        for y in 0..height {
            let row_a = &a.data_origin()[y * a.cfg.stride..][..width];
            let row_b = &b.data_origin()[y * b.cfg.stride..][..width];
            for (p, q) in row_a.iter().zip(row_b) {
                squared += f64::from(p.abs_diff(*q)).powi(2);
            }
        }
        let mse = squared / (width * height) as f64;
        Some(if mse == 0.0 {
            99.0
        } else {
            10.0 * (255.0f64.powi(2) / mse).log10()
        })
    }

    /// Encoding speed at 1080p, 30 fps, for several presets, then the whole sink with
    /// narration (`cargo test --release -p ardoise-media speed_1080p -- --ignored
    /// --nocapture`; `ARDOISE_BENCH_FRAMES` frames, 300 by default).
    #[test]
    #[ignore = "benchmark: minutes in release"]
    fn speed_1080p() {
        let frames: usize = std::env::var("ARDOISE_BENCH_FRAMES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);
        let (width, height, fps) = (1920usize, 1080usize, 30u32);
        let settings = settings(width as u32, height as u32, fps);
        println!(
            "rav1e, {width}x{height}, {frames} frames at {fps} fps, {} threads, assembly: {}",
            std::thread::available_parallelism().map_or(1, usize::from),
            cfg!(feature = "asm")
        );
        println!(
            "| speed | low latency | quantizer | fps   | 2 min (3600 frames) | size per 10 s | luma PSNR |"
        );
        // (speed, low latency, quantizer): the default first.
        let presets = [
            (10, true, 100),
            (10, false, 100),
            (8, true, 100),
            (6, true, 100),
            (10, true, 130),
        ];
        for (speed, low_latency, quantizer) in presets {
            let options = WebmOptions {
                speed,
                quantizer,
                low_latency,
                audio_bitrate: None,
            };
            let mut context: Context<u8> = Config::new()
                .with_encoder_config(encoder_config(&settings, &options))
                .new_context()
                .unwrap();
            let mut page = Handwriting::new(width, height);
            let mut nv12_frame = vec![0; nv12::nv12_len(width, height)];
            let mut encoding = Duration::ZERO;
            let (mut bytes, mut psnr_sum, mut psnr_count) = (0usize, 0f64, 0usize);
            let mut collect = |context: &mut Context<u8>| loop {
                match context.receive_packet() {
                    Ok(packet) => {
                        bytes += packet.data.len();
                        if let Some(psnr) = luma_psnr(&packet) {
                            psnr_sum += psnr;
                            psnr_count += 1;
                        }
                    }
                    Err(EncoderStatus::Encoded) => {}
                    Err(_) => break,
                }
            };
            for _ in 0..frames {
                page.write(40);
                let started = std::time::Instant::now();
                nv12::rgba_to_nv12(&page.canvas, width, height, &mut nv12_frame).unwrap();
                let (y, uv) = nv12_frame.split_at(width * height);
                let u: Vec<u8> = uv.as_chunks::<2>().0.iter().map(|p| p[0]).collect();
                let v: Vec<u8> = uv.as_chunks::<2>().0.iter().map(|p| p[1]).collect();
                let mut frame = context.new_frame();
                frame.planes[0].copy_from_raw_u8(y, width, 1);
                frame.planes[1].copy_from_raw_u8(&u, width / 2, 1);
                frame.planes[2].copy_from_raw_u8(&v, width / 2, 1);
                context.send_frame(frame).unwrap();
                collect(&mut context);
                encoding += started.elapsed();
            }
            let started = std::time::Instant::now();
            context.flush();
            collect(&mut context);
            encoding += started.elapsed();
            let fps_measured = frames as f64 / encoding.as_secs_f64();
            println!(
                "| {speed:>5} | {low_latency:>11} | {quantizer:>9} | {fps_measured:>5.1} | {:>17.0} s | {:>10.0} kB | {:>6.2} dB |",
                3600.0 / fps_measured,
                bytes as f64 / 1000.0 / (frames as f64 / f64::from(fps)) * 10.0,
                psnr_sum / psnr_count.max(1) as f64,
            );
        }

        // The whole export path with the default options and stereo narration.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bench.webm");
        let settings = VideoSettings {
            audio_channels: Some(2),
            ..settings
        };
        let mut sink = create_webm(&path, &settings, &WebmOptions::default()).unwrap();
        let narration = tone(2, frames as f64 / f64::from(fps) + 1.0);
        let per_frame = 2 * 48_000 / fps as usize;
        let mut page = Handwriting::new(width, height);
        let mut spent = Duration::ZERO;
        for i in 0..frames {
            page.write(40);
            let started = std::time::Instant::now();
            sink.push_audio(&narration[i * per_frame..(i + 1) * per_frame])
                .unwrap();
            sink.push_frame(&page.canvas, Duration::from_secs(i as u64) / fps)
                .unwrap();
            spent += started.elapsed();
        }
        let started = std::time::Instant::now();
        let summary = sink.finish().unwrap();
        spent += started.elapsed();
        let probe = probe_webm(&path).unwrap();
        let size = std::fs::metadata(&path).unwrap().len();
        let fps_measured = frames as f64 / spent.as_secs_f64();
        println!(
            "Sink with defaults ({}): {fps_measured:.1} fps, 2 min replay ~{:.0} s, {size} B",
            summary.encoder,
            3600.0 / fps_measured
        );
        print_probe_table("WebM", &probe, size, spent);
        assert_eq!(probe.frames, frames as u64);
    }
}
