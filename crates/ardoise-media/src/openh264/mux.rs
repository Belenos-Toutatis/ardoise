//! The MP4 container of the OpenH264 fallback: H.264 video and Opus narration, written with the
//! sans-I/O muxer of `shiguredo_mp4` (Apache-2.0, pure Rust, no dependencies).
//!
//! - **Streaming**: sample data goes to the file as it comes (`ftyp`, `mdat`, then `moov` at
//!   the end, the layout Media Foundation writes); only the sample tables stay in memory.
//! - **Video**: H.264 access units in Annex B (start codes, as OpenH264 delivers them) become
//!   length-prefixed samples. The parameter sets go into the `avcC` box (from the first key
//!   frame) and are left out of the samples; a change of parameter sets is an error. Timescale
//!   90 kHz. Each frame lasts until the next frame's timestamp, the last one for the frame
//!   period, as with Media Foundation. A `colr` box repeats the colour description of the
//!   SPS (BT.709, limited range).
//! - **Sound**: Opus as specified by "Encapsulation of Opus in ISO Base Media File Format"
//!   (the RFC 7845 mapping): an `Opus` sample entry whose `dOps` box carries the encoder's
//!   pre-skip, timescale 48 kHz, one packet per sample, and an edit list that starts after the
//!   pre-skip and ends with the narration (what FFmpeg writes), so the sound starts exactly at
//!   zero and stops with the narration.
//! - **Interleaving**: samples are written in windows of one second of replay, the video of a
//!   window then its sound, so a player reads both tracks from nearby places in the file.
//!   Sound pushed ahead of the video (the whole narration at once) waits in memory, a few
//!   kilobytes per second.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::num::NonZeroU32;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use shiguredo_mp4::boxes::{
    AudioSampleEntryFields, Avc1Box, AvccBox, DopsBox, EdtsBox, ElstBox, ElstEntry, HdlrBox,
    MoovBox, OpusBox, SampleEntry, UnknownBox, VisualSampleEntryFields,
};
use shiguredo_mp4::mux::{Mp4FileMuxer, Mp4FileMuxerOptions, Sample};
use shiguredo_mp4::{BoxSize, BoxType, Encode, FixedPointNumber, TrackKind, Uint};

use crate::opus::{OpusPacket, samples_to_duration};
use crate::{MediaError, NARRATION_SAMPLE_RATE};

/// Ticks per second of the video track.
const VIDEO_TIMESCALE: u32 = 90_000;
/// Length of an interleaving window.
const INTERLEAVE: Duration = Duration::from_secs(1);
/// H.264 NAL unit types handled here.
const NAL_IDR_SLICE: u8 = 5;
const NAL_SPS: u8 = 7;
const NAL_PPS: u8 = 8;
const NAL_ACCESS_UNIT_DELIMITER: u8 = 9;

/// The Opus narration track.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct OpusTrack {
    pub channels: u16,
    /// Samples (at 48 kHz, per channel) a decoder drops at the start
    /// ([`crate::OpusEncoder::pre_skip`]).
    pub pre_skip: u16,
}

/// What [`Mp4Muxer::finish`] wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MuxSummary {
    pub video_frames: u64,
    /// True if the file has a sound track.
    pub audio: bool,
    /// End of the last frame or of the narration, whichever is later.
    pub duration: Duration,
}

/// A video sample whose duration is known.
struct VideoSample {
    data: Vec<u8>,
    keyframe: bool,
    /// Start, in video ticks.
    start: u64,
    duration: u32,
}

/// The latest frame, waiting for the next one (its duration).
struct HeldFrame {
    data: Vec<u8>,
    keyframe: bool,
    start: u64,
    timestamp: Duration,
}

/// Writes an MP4 file of H.264 video and, optionally, Opus sound.
pub(crate) struct Mp4Muxer {
    file: BufWriter<File>,
    muxer: Mp4FileMuxer,
    /// Where the next sample goes in the file.
    position: u64,
    width: u16,
    height: u16,
    /// SPS and PPS of the stream (from the first key frame).
    parameter_sets: Option<(Vec<u8>, Vec<u8>)>,
    audio: Option<OpusTrack>,
    held: Option<HeldFrame>,
    video_queue: VecDeque<VideoSample>,
    audio_queue: VecDeque<OpusPacket>,
    /// Everything that starts before this replay instant is written.
    written_until: Duration,
    video_written: u64,
    audio_written: u64,
}

impl Mp4Muxer {
    /// Creates (or replaces) `path`. `width` and `height` are the frame size.
    pub(crate) fn create(
        path: &Path,
        width: u32,
        height: u32,
        audio: Option<OpusTrack>,
    ) -> Result<Self, MediaError> {
        let size = |value: u32| {
            u16::try_from(value)
                .map_err(|_| MediaError::InvalidSettings(format!("frame side of {value} px")))
        };
        let (width, height) = (size(width)?, size(height)?);
        let options = Mp4FileMuxerOptions {
            creation_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default(),
            ..Mp4FileMuxerOptions::default()
        };
        let muxer = Mp4FileMuxer::with_options(options).map_err(mux_error)?;
        let mut file = BufWriter::new(File::create(path)?);
        let initial = muxer.initial_boxes_bytes();
        file.write_all(initial)?;
        Ok(Self {
            file,
            position: initial.len() as u64,
            muxer,
            width,
            height,
            parameter_sets: None,
            audio,
            held: None,
            video_queue: VecDeque::new(),
            audio_queue: VecDeque::new(),
            written_until: Duration::ZERO,
            video_written: 0,
            audio_written: 0,
        })
    }

    /// Adds one H.264 access unit (Annex B) shown from `timestamp`. Timestamps strictly
    /// increase (checked by the sink); the first access unit must be a key frame with its
    /// parameter sets.
    pub(crate) fn push_video(
        &mut self,
        access_unit: &[u8],
        timestamp: Duration,
    ) -> Result<(), MediaError> {
        let mut data = Vec::with_capacity(access_unit.len() + 16);
        let (mut keyframe, mut sps, mut pps) = (false, None, None);
        for nal in annex_b_nal_units(access_unit) {
            match nal[0] & 0x1F {
                NAL_SPS => sps = Some(nal),
                NAL_PPS => pps = Some(nal),
                NAL_ACCESS_UNIT_DELIMITER => {}
                kind => {
                    keyframe |= kind == NAL_IDR_SLICE;
                    let len = u32::try_from(nal.len())
                        .map_err(|_| h264_error("NAL unit larger than 4 GB"))?;
                    data.extend_from_slice(&len.to_be_bytes());
                    data.extend_from_slice(nal);
                }
            }
        }
        if data.is_empty() {
            return Err(h264_error("an access unit without a picture"));
        }
        match (&self.parameter_sets, sps, pps) {
            (None, Some(sps), Some(pps)) if keyframe && sps.len() >= 4 => {
                self.parameter_sets = Some((sps.to_vec(), pps.to_vec()));
            }
            (None, ..) => {
                return Err(h264_error(
                    "the first frame is not a key frame with its parameter sets",
                ));
            }
            (Some((known_sps, known_pps)), sps, pps) => {
                if sps.is_some_and(|s| s != known_sps.as_slice())
                    || pps.is_some_and(|p| p != known_pps.as_slice())
                {
                    return Err(h264_error("the parameter sets changed during the video"));
                }
            }
        }

        let mut start = video_ticks(timestamp);
        if let Some(held) = self.held.take() {
            start = start.max(held.start + 1);
            self.video_queue.push_back(VideoSample {
                duration: duration_ticks(start - held.start)?,
                data: held.data,
                keyframe: held.keyframe,
                start: held.start,
            });
        }
        self.held = Some(HeldFrame {
            data,
            keyframe,
            start,
            timestamp,
        });
        self.interleave(false)
    }

    /// Adds Opus packets, in order and contiguous ([`crate::OpusEncoder`] output).
    pub(crate) fn push_audio(&mut self, packets: Vec<OpusPacket>) -> Result<(), MediaError> {
        if self.audio.is_none() {
            return Err(MediaError::NoAudioTrack);
        }
        self.audio_queue.extend(packets);
        self.interleave(false)
    }

    /// Writes what is left and the index, and closes the file. The last frame lasts
    /// `last_frame`; `narration_samples` is the narration's length (48 kHz samples per
    /// channel): the edit list ends the sound there. Without any narration the file has no
    /// sound track.
    pub(crate) fn finish(
        mut self,
        last_frame: Duration,
        narration_samples: u64,
    ) -> Result<MuxSummary, MediaError> {
        let Some(held) = self.held.take() else {
            return Err(MediaError::InvalidSettings(
                "no video frame was pushed".into(),
            ));
        };
        let video_end = held.timestamp.saturating_add(last_frame);
        let end = video_ticks(video_end).max(held.start + 1);
        self.video_queue.push_back(VideoSample {
            duration: duration_ticks(end - held.start)?,
            data: held.data,
            keyframe: held.keyframe,
            start: held.start,
        });
        if narration_samples == 0 {
            // Only the encoder's padding: no sound track rather than one of silence.
            self.audio_queue.clear();
        }
        self.interleave(true)?;
        let audio = match self.audio {
            Some(track) if self.audio_written > 0 => Some(track),
            _ => None,
        };

        self.file.flush()?;
        let finalized = self.muxer.finalize().map_err(mux_error)?;
        if finalized.is_faststart_enabled() {
            return Err(mp4_error("unexpected layout with the index first"));
        }
        let mut moov = finalized.moov_box().clone();
        if let Some(track) = audio {
            add_narration_edit(&mut moov, track.pre_skip, narration_samples)?;
        }
        let moov_bytes = moov.encode_to_vec().map_err(|e| mp4_error(e.to_string()))?;
        for (offset, bytes) in finalized.offset_and_bytes_pairs() {
            self.file.seek(SeekFrom::Start(offset))?;
            if bytes.get(4..8) == Some(b"moov".as_slice()) {
                self.file.write_all(&moov_bytes)?;
            } else {
                self.file.write_all(bytes)?;
            }
        }
        self.file.flush()?;
        let file = self
            .file
            .into_inner()
            .map_err(|e| MediaError::Io(e.into_error()))?;
        file.sync_all()?;

        let narration = samples_to_duration(narration_samples);
        Ok(MuxSummary {
            video_frames: self.video_written,
            audio: audio.is_some(),
            duration: if audio.is_some() {
                video_end.max(narration)
            } else {
                video_end
            },
        })
    }

    /// Writes the queued samples window by window, once the video has gone past a window's
    /// end (everything when `flushing`).
    fn interleave(&mut self, flushing: bool) -> Result<(), MediaError> {
        loop {
            let window_end = self.written_until + INTERLEAVE;
            let video_past = self
                .held
                .as_ref()
                .is_some_and(|held| held.timestamp >= window_end);
            if !flushing && !video_past {
                return Ok(());
            }
            while self
                .video_queue
                .front()
                .is_some_and(|s| ticks_to_duration(s.start) < window_end)
            {
                if let Some(sample) = self.video_queue.pop_front() {
                    self.write_video(&sample)?;
                }
            }
            while self
                .audio_queue
                .front()
                .is_some_and(|p| samples_to_duration(p.start) < window_end)
            {
                if let Some(packet) = self.audio_queue.pop_front() {
                    self.write_audio(&packet)?;
                }
            }
            self.written_until = window_end;
            if flushing && self.video_queue.is_empty() && self.audio_queue.is_empty() {
                return Ok(());
            }
        }
    }

    fn write_video(&mut self, sample: &VideoSample) -> Result<(), MediaError> {
        let entry = match self.video_written {
            0 => Some(self.video_entry()?),
            _ => None,
        };
        self.append(
            TrackKind::Video,
            entry,
            sample.keyframe,
            VIDEO_TIMESCALE,
            (sample.duration, &sample.data),
        )?;
        self.video_written += 1;
        Ok(())
    }

    fn write_audio(&mut self, packet: &OpusPacket) -> Result<(), MediaError> {
        let Some(track) = self.audio else {
            return Err(MediaError::NoAudioTrack);
        };
        let entry = match self.audio_written {
            0 => Some(opus_entry(track)),
            _ => None,
        };
        self.append(
            TrackKind::Audio,
            entry,
            true,
            NARRATION_SAMPLE_RATE,
            (packet.samples, &packet.data),
        )?;
        self.audio_written += 1;
        Ok(())
    }

    /// Writes a sample's data at the end of `mdat` and records it.
    fn append(
        &mut self,
        track_kind: TrackKind,
        sample_entry: Option<SampleEntry>,
        keyframe: bool,
        timescale: u32,
        (duration, data): (u32, &[u8]),
    ) -> Result<(), MediaError> {
        let timescale = NonZeroU32::new(timescale).ok_or_else(|| mp4_error("zero timescale"))?;
        self.file.write_all(data)?;
        self.muxer
            .append_sample(&Sample {
                track_kind,
                sample_entry,
                keyframe,
                timescale,
                duration,
                composition_time_offset: None,
                data_offset: self.position,
                data_size: data.len(),
            })
            .map_err(mux_error)?;
        self.position += data.len() as u64;
        Ok(())
    }

    /// The `avc1` sample entry, from the stream's parameter sets.
    fn video_entry(&self) -> Result<SampleEntry, MediaError> {
        let Some((sps, pps)) = &self.parameter_sets else {
            return Err(h264_error("no parameter sets"));
        };
        let (profile, compatibility, level) = match sps.as_slice() {
            [_, profile, compatibility, level, ..] => (*profile, *compatibility, *level),
            _ => return Err(h264_error("SPS too short")),
        };
        // The High profiles add the chroma format and bit depths (4:2:0, 8 bits here).
        let high = matches!(profile, 100 | 110 | 122 | 244 | 44 | 83 | 86 | 118 | 128);
        let mut compressor = [0u8; 32];
        let name = b"OpenH264";
        compressor[0] = name.len() as u8;
        compressor[1..=name.len()].copy_from_slice(name);
        Ok(SampleEntry::Avc1(Avc1Box {
            visual: VisualSampleEntryFields {
                data_reference_index: VisualSampleEntryFields::DEFAULT_DATA_REFERENCE_INDEX,
                width: self.width,
                height: self.height,
                horizresolution: VisualSampleEntryFields::DEFAULT_HORIZRESOLUTION,
                vertresolution: VisualSampleEntryFields::DEFAULT_VERTRESOLUTION,
                frame_count: VisualSampleEntryFields::DEFAULT_FRAME_COUNT,
                compressorname: compressor,
                depth: VisualSampleEntryFields::DEFAULT_DEPTH,
            },
            avcc_box: AvccBox {
                avc_profile_indication: profile,
                profile_compatibility: compatibility,
                avc_level_indication: level,
                length_size_minus_one: Uint::new(3),
                sps_list: vec![sps.clone()],
                pps_list: vec![pps.clone()],
                chroma_format: high.then(|| Uint::new(1)),
                bit_depth_luma_minus8: high.then(|| Uint::new(0)),
                bit_depth_chroma_minus8: high.then(|| Uint::new(0)),
                sps_ext_list: Vec::new(),
            },
            unknown_boxes: vec![bt709_colour_box()],
        }))
    }
}

/// The `Opus` sample entry.
fn opus_entry(track: OpusTrack) -> SampleEntry {
    SampleEntry::Opus(OpusBox {
        audio: AudioSampleEntryFields {
            data_reference_index: AudioSampleEntryFields::DEFAULT_DATA_REFERENCE_INDEX,
            channelcount: track.channels,
            samplesize: AudioSampleEntryFields::DEFAULT_SAMPLESIZE,
            // 48 000 fits the 16-bit integer part.
            samplerate: FixedPointNumber::new(NARRATION_SAMPLE_RATE as u16, 0),
        },
        dops_box: DopsBox {
            output_channel_count: track.channels as u8,
            pre_skip: track.pre_skip,
            input_sample_rate: NARRATION_SAMPLE_RATE,
            output_gain: 0,
        },
        unknown_boxes: Vec::new(),
    })
}

/// `colr` box of type `nclx`: BT.709 primaries, transfer and matrix, limited range (what
/// `nv12.rs` produces and the SPS also says).
fn bt709_colour_box() -> UnknownBox {
    let mut payload = b"nclx".to_vec();
    for code in [1u16, 1, 1] {
        payload.extend_from_slice(&code.to_be_bytes());
    }
    payload.push(0); // full_range_flag = 0, reserved bits
    UnknownBox {
        box_type: BoxType::Normal(*b"colr"),
        box_size: BoxSize::U32(8 + payload.len() as u32),
        payload,
    }
}

/// Gives the sound track an edit list that skips the Opus pre-skip and lasts as long as the
/// narration, and updates the track and movie durations accordingly.
fn add_narration_edit(
    moov: &mut MoovBox,
    pre_skip: u16,
    narration_samples: u64,
) -> Result<(), MediaError> {
    let movie_timescale = u128::from(moov.mvhd_box.timescale.get());
    let edit = u128::from(narration_samples) * movie_timescale;
    let edit = u64::try_from(edit.div_ceil(u128::from(NARRATION_SAMPLE_RATE)))
        .map_err(|_| mp4_error("narration too long"))?;
    let mut found = false;
    for trak in &mut moov.trak_boxes {
        if trak.mdia_box.hdlr_box.handler_type == HdlrBox::HANDLER_TYPE_SOUN {
            trak.edts_box = Some(EdtsBox {
                elst_box: Some(ElstBox {
                    entries: vec![ElstEntry {
                        edit_duration: edit,
                        media_time: i64::from(pre_skip),
                        media_rate: FixedPointNumber::new(1, 0),
                    }],
                }),
                unknown_boxes: Vec::new(),
            });
            trak.tkhd_box.duration = edit;
            found = true;
        }
    }
    if !found {
        return Err(mp4_error("no sound track to edit"));
    }
    moov.mvhd_box.duration = moov
        .trak_boxes
        .iter()
        .map(|t| t.tkhd_box.duration)
        .max()
        .unwrap_or(0);
    Ok(())
}

/// The NAL units of an Annex B byte stream (start codes of 3 or 4 bytes), without their start
/// codes and trailing zero bytes; empty units are skipped.
pub(crate) fn annex_b_nal_units(stream: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= stream.len() {
        if stream[i] == 0 && stream[i + 1] == 0 && stream[i + 2] == 1 {
            starts.push(i + 3);
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut units = Vec::with_capacity(starts.len());
    for (k, &start) in starts.iter().enumerate() {
        let end = starts.get(k + 1).map_or(stream.len(), |&next| next - 3);
        let mut unit = &stream[start..end.max(start)];
        while let [rest @ .., 0] = unit {
            unit = rest;
        }
        if !unit.is_empty() {
            units.push(unit);
        }
    }
    units
}

/// A replay instant in video ticks, rounded to the nearest.
fn video_ticks(timestamp: Duration) -> u64 {
    let ticks = (timestamp.as_nanos() * u128::from(VIDEO_TIMESCALE) + 500_000_000) / 1_000_000_000;
    u64::try_from(ticks).unwrap_or(u64::MAX)
}

fn ticks_to_duration(ticks: u64) -> Duration {
    let nanos = u128::from(ticks) * 1_000_000_000 / u128::from(VIDEO_TIMESCALE);
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

fn duration_ticks(ticks: u64) -> Result<u32, MediaError> {
    u32::try_from(ticks).map_err(|_| mp4_error("a frame lasting more than 13 hours"))
}

fn h264_error(message: impl Into<String>) -> MediaError {
    MediaError::Encoding {
        codec: "H.264",
        message: message.into(),
    }
}

fn mp4_error(message: impl Into<String>) -> MediaError {
    MediaError::Encoding {
        codec: "MP4",
        message: message.into(),
    }
}

fn mux_error(error: shiguredo_mp4::mux::MuxError) -> MediaError {
    mp4_error(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnimationFormat, OpusEncoder, probe_mp4_animation};

    // Synthetic H.264 NAL units: real headers, made-up payloads (the muxer and the probe read
    // the structure, they do not decode pictures).
    const SPS: &[u8] = &[0x67, 0x42, 0xC0, 0x1E, 0xDA, 0x02, 0x80, 0xBF, 0xE5, 0x84];
    const PPS: &[u8] = &[0x68, 0xCE, 0x3C, 0x80];

    fn access_unit(key: bool, index: u32) -> Vec<u8> {
        let mut au = Vec::new();
        if key {
            for nal in [SPS, PPS] {
                au.extend_from_slice(&[0, 0, 0, 1]);
                au.extend_from_slice(nal);
            }
        }
        au.extend_from_slice(&[0, 0, 1]);
        au.push(if key { 0x65 } else { 0x41 });
        // No zero bytes in the payload: no false start code (real streams use emulation
        // prevention bytes).
        au.extend_from_slice(&[0x88, 0x80 | (index % 128) as u8, 0x84]);
        au
    }

    #[test]
    fn splits_annex_b_streams() {
        let stream = [
            0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 0, 1, 0x65, 4, 0,
        ];
        let units = annex_b_nal_units(&stream);
        assert_eq!(
            units,
            [&[0x67, 1, 2][..], &[0x68, 3][..], &[0x65, 4][..]],
            "start codes and trailing zeros removed"
        );
        assert!(annex_b_nal_units(&[0, 0, 0]).is_empty());
        assert!(annex_b_nal_units(&[0x65, 1, 2]).is_empty());
    }

    /// Writes `frames` frames at 10 fps (a key frame every 10) and `narration` seconds of a
    /// tone, the sound pushed with the frames as the sinks receive it.
    fn write(path: &Path, frames: u32, narration: Option<(u16, f64)>) -> (MuxSummary, Option<u16>) {
        let mut opus = narration.map(|(channels, _)| OpusEncoder::new(channels, 64_000).unwrap());
        let track = opus.as_ref().map(|o| OpusTrack {
            channels: o.channels(),
            pre_skip: o.pre_skip(),
        });
        let mut mux = Mp4Muxer::create(path, 320, 240, track).unwrap();
        let (channels, seconds) = narration.unwrap_or((1, 0.0));
        let total = (seconds * 48_000.0).round() as usize * usize::from(channels);
        let audio: Vec<i16> = (0..total).map(|i| ((i % 100) as i16 - 50) * 100).collect();
        let mut chunks = audio.chunks(4_800 * usize::from(channels));
        for i in 0..frames {
            if let (Some(opus), Some(chunk)) = (opus.as_mut(), chunks.next()) {
                mux.push_audio(opus.push(chunk).unwrap()).unwrap();
            }
            mux.push_video(
                &access_unit(i % 10 == 0, i),
                Duration::from_millis(100 * u64::from(i)),
            )
            .unwrap();
        }
        let mut samples = 0;
        if let Some(mut opus) = opus {
            for chunk in chunks {
                mux.push_audio(opus.push(chunk).unwrap()).unwrap();
            }
            samples = (audio.len() / usize::from(channels)) as u64;
            // As the sink: the encoder's tail only when there was a narration.
            if samples > 0 {
                mux.push_audio(opus.finish().unwrap()).unwrap();
            }
        }
        let summary = mux.finish(Duration::from_millis(100), samples).unwrap();
        (summary, track.map(|t| t.pre_skip))
    }

    #[test]
    fn video_and_opus_read_back_by_an_independent_parser() {
        let dir = tempfile::tempdir().unwrap();
        for (frames, channels, seconds) in [(25u32, 1u16, 2.0f64), (25, 2, 3.3), (30, 1, 1.25)] {
            let path = dir
                .path()
                .join(format!("{frames}-{channels}-{seconds}.mp4"));
            let (summary, _) = write(&path, frames, Some((channels, seconds)));
            let video_end = Duration::from_millis(100 * u64::from(frames));
            let narration = Duration::from_secs_f64(seconds);
            assert_eq!(summary.video_frames, u64::from(frames));
            assert!(summary.audio);
            assert_eq!(summary.duration, video_end.max(narration));

            let probe = probe_mp4_animation(&path).unwrap();
            assert_eq!(probe.format, AnimationFormat::Mp4);
            assert_eq!((probe.width, probe.height), (320, 240));
            assert_eq!(probe.frames, u64::from(frames));
            let audio = probe.audio.expect("an Opus track");
            assert_eq!(
                (audio.codec.as_str(), audio.channels, audio.sample_rate),
                ("Opus", channels, 48_000)
            );
            // The edit list ends the sound with the narration (rounded up to a tick of the
            // movie timescale, at most 1/48000 s here).
            let extra = audio.duration.saturating_sub(narration);
            assert!(
                audio.duration >= narration && extra <= Duration::from_micros(21),
                "{:?} of sound for {narration:?}",
                audio.duration
            );
            let expected = video_end.max(narration);
            assert!(
                probe.duration >= expected
                    && probe.duration - expected <= Duration::from_micros(21),
                "{:?} for {expected:?}",
                probe.duration
            );
        }
    }

    #[test]
    fn silent_video_has_no_sound_track() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("silent.mp4");
        let (summary, _) = write(&path, 12, None);
        assert_eq!(
            summary,
            MuxSummary {
                video_frames: 12,
                audio: false,
                duration: Duration::from_millis(1200)
            }
        );
        let probe = probe_mp4_animation(&path).unwrap();
        assert_eq!((probe.frames, probe.audio), (12, None));
        assert_eq!(probe.duration, Duration::from_millis(1200));

        // An audio track declared but no narration pushed: no sound track either.
        let path = dir.path().join("declared.mp4");
        let (summary, _) = write(&path, 12, Some((1, 0.0)));
        assert!(!summary.audio);
        assert_eq!(probe_mp4_animation(&path).unwrap().audio, None);
    }

    #[test]
    fn the_file_layout_and_boxes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("layout.mp4");
        let (_, pre_skip) = write(&path, 25, Some((1, 2.0)));
        let bytes = std::fs::read(&path).unwrap();
        // Top-level boxes: ftyp, free, mdat (64-bit size), moov at the end.
        let mut top = Vec::new();
        let mut offset = 0usize;
        while offset + 8 <= bytes.len() {
            let mut size = u64::from(u32::from_be_bytes(
                bytes[offset..offset + 4].try_into().unwrap(),
            ));
            let kind = String::from_utf8_lossy(&bytes[offset + 4..offset + 8]).into_owned();
            if size == 1 {
                size = u64::from_be_bytes(bytes[offset + 8..offset + 16].try_into().unwrap());
            }
            top.push(kind);
            offset += size as usize;
        }
        assert_eq!(offset, bytes.len(), "boxes cover the file exactly");
        assert_eq!(top, ["ftyp", "free", "mdat", "moov"]);
        let find = |needle: &[u8]| bytes.windows(needle.len()).position(|w| w == needle);
        // The colour box, and the dOps pre-skip followed by the edit list's media time.
        assert!(find(b"colrnclx\x00\x01\x00\x01\x00\x01\x00").is_some());
        let dops = find(b"dOps").unwrap();
        let skip = u16::from_be_bytes([bytes[dops + 6], bytes[dops + 7]]);
        assert_eq!(Some(skip), pre_skip);
        assert!(find(b"elst").is_some());
    }

    #[test]
    fn rejects_streams_it_cannot_store() {
        let dir = tempfile::tempdir().unwrap();
        let mut mux = Mp4Muxer::create(&dir.path().join("bad.mp4"), 64, 64, None).unwrap();
        // A P frame first.
        assert!(
            mux.push_video(&access_unit(false, 0), Duration::ZERO)
                .is_err()
        );
        assert!(
            mux.push_video(&[0, 0, 1, 0x67, 1, 2, 3], Duration::ZERO)
                .is_err()
        );
        mux.push_video(&access_unit(true, 0), Duration::ZERO)
            .unwrap();
        // Other parameter sets later.
        let mut changed = access_unit(true, 1);
        changed[8] ^= 1;
        assert!(mux.push_video(&changed, Duration::from_millis(40)).is_err());
        assert!(matches!(
            mux.push_audio(Vec::new()),
            Err(MediaError::NoAudioTrack)
        ));
        assert!(Mp4Muxer::create(&dir.path().join("big.mp4"), 70_000, 64, None).is_err());
    }
}
