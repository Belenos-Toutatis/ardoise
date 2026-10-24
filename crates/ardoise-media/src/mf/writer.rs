//! MP4 (H.264 + AAC) writer on top of `IMFSinkWriter`.
//!
//! The sink writer does the heavy lifting: it loads an H.264 encoder MFT (hardware or
//! software) and Microsoft's AAC encoder MFT, and feeds the MPEG-4 file sink. We give it NV12
//! frames and 16-bit PCM, each with a timestamp and a duration in 100-ns units.

use std::collections::VecDeque;
use std::path::Path;
use std::time::Duration;

use windows::Win32::Media::MediaFoundation::{
    CODECAPI_AVEncMPVDefaultBPictureCount, CODECAPI_AVEncMPVGOPSize, IMFMediaBuffer, IMFMediaType,
    IMFSinkWriter, IMFSinkWriterEx, IMFTransform, MF_E_TOPO_CODEC_NOT_FOUND,
    MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, MF_MT_AAC_PAYLOAD_TYPE,
    MF_MT_ALL_SAMPLES_INDEPENDENT, MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MF_MT_AUDIO_BITS_PER_SAMPLE,
    MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS, MF_MT_AUDIO_SAMPLES_PER_SECOND,
    MF_MT_AVG_BITRATE, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_RATE, MF_MT_FRAME_SIZE,
    MF_MT_INTERLACE_MODE, MF_MT_MAJOR_TYPE, MF_MT_MPEG2_PROFILE, MF_MT_PIXEL_ASPECT_RATIO,
    MF_MT_SUBTYPE, MF_MT_TRANSFER_FUNCTION, MF_MT_VIDEO_NOMINAL_RANGE, MF_MT_VIDEO_PRIMARIES,
    MF_MT_YUV_MATRIX, MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, MF_TRANSCODE_CONTAINERTYPE,
    MFAudioFormat_AAC, MFAudioFormat_PCM, MFCreateMediaType, MFCreateMemoryBuffer, MFCreateSample,
    MFCreateSinkWriterFromURL, MFMediaType_Audio, MFMediaType_Video, MFNominalRange_16_235,
    MFT_CATEGORY_VIDEO_ENCODER, MFT_ENUM_HARDWARE_URL_Attribute, MFT_TRANSFORM_CLSID_Attribute,
    MFTranscodeContainerType_MPEG4, MFVideoFormat_H264, MFVideoFormat_NV12,
    MFVideoInterlace_Progressive, MFVideoPrimaries_BT709, MFVideoTransFunc_709,
    MFVideoTransferMatrix_BT709, eAVEncH264VProfile_High,
};
use windows::Win32::System::Com::IPersist;
use windows::core::{GUID, HSTRING, Interface};

use super::encoders::h264_encoders;
use super::{
    Attrs, LockedBuffer, MfSession, StepExt, create_attributes, get_string, mf_error, pack,
};
use crate::timing::{duration_to_hns, frame_period_hns, frames_to_hns, hns_to_duration};
use crate::{AAC_BITRATE, AudioSettings, EncoderInfo, MediaError, Mp4Settings, Mp4Summary, nv12};

/// AAC-LC, profile level 2 (up to 48 kHz stereo), as `MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION`.
const AAC_LC_LEVEL_2: u32 = 0x29;

/// When the written audio ends more than this (100-ns units: 1 s) before a frame, the audio
/// stream gets a stream tick (a gap) at that frame. Shorter gaps do not slow the sink writer.
const AUDIO_GAP_BEFORE_TICK: i64 = 10_000_000;

/// Writes an `.mp4` file holding H.264 video and optionally AAC audio.
///
/// Usage: [`create`](Self::create), then [`push_frame`](Self::push_frame) and
/// [`push_audio`](Self::push_audio) (order below), then [`finish`](Self::finish). Dropping the
/// writer without `finish` leaves an unplayable partial file: write to a temporary path and
/// rename it once `finish` succeeds (the path's extension does not matter).
///
/// The MPEG-4 sink interleaves the two tracks, so the order of the calls matters:
/// - audio may be pushed ahead of the video, even a whole narration at once: the writer keeps
///   it and hands it to Media Foundation in step with the frames;
/// - frames pushed long before the audio that covers them make the sink writer stall, waiting
///   for the audio stream (measured: 24 s instead of 2.4 s for 3 s of 720p when every frame
///   came first). Push each frame's audio before, or together with, the frame.
///
/// The writer is bound to the thread that created it (COM).
pub struct Mp4Writer {
    // COM objects first: fields drop in declaration order, and every COM object must be
    // released before `_session` shuts Media Foundation down.
    writer: IMFSinkWriter,
    video_stream: u32,
    audio: Option<AudioTrack>,
    width: usize,
    height: usize,
    frame_period: i64,
    last_timestamp: Option<Duration>,
    video_frames: u64,
    /// End of the last frame written, in 100-ns units.
    video_end: i64,
    encoder: EncoderInfo,
    _session: MfSession,
}

struct AudioTrack {
    stream: u32,
    settings: AudioSettings,
    /// Interleaved samples not yet given to the sink writer.
    pending: VecDeque<i16>,
    /// Audio frames already given to the sink writer.
    written_frames: u64,
}

impl Mp4Writer {
    /// Creates (or overwrites) the file at `path` and prepares the encoders.
    pub fn create(path: &Path, settings: &Mp4Settings) -> Result<Self, MediaError> {
        settings.validate()?;
        let session = MfSession::start()?;

        // MF_MPEG4SINK_MOOV_BEFORE_MDAT ("fast start") stays off: with the sink writer's own
        // write-only file `Finalize` fails with E_ACCESSDENIED, and with a read-write byte
        // stream from MFCreateFile the sink leaves a corrupt file (index pointing at the wrong
        // place, data duplicated). The index ("moov") therefore ends the file.
        let attributes = create_attributes(2)?;
        Attrs(&attributes)
            // MPEG-4 whatever the file extension (atomic writes use a temporary name).
            .guid(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MPEG4)?
            // Without this the sink writer never loads hardware encoders.
            .u32(
                &MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS,
                u32::from(settings.hardware_acceleration),
            )?;

        let url = HSTRING::from(path);
        // SAFETY: `url` is a NUL-terminated UTF-16 path that outlives the call; no byte stream
        // is given (the sink writer creates the file); `attributes` is a valid attribute store.
        let writer = unsafe { MFCreateSinkWriterFromURL(&url, None, &attributes) }
            .step("MFCreateSinkWriterFromURL")?;

        let video_stream = add_video_stream(&writer, settings)?;
        let audio = match settings.audio {
            Some(audio) => Some(AudioTrack {
                stream: add_audio_stream(&writer, &audio)?,
                settings: audio,
                pending: VecDeque::new(),
                written_frames: 0,
            }),
            None => None,
        };

        // SAFETY: valid sink writer whose streams all have input and output types.
        unsafe { writer.BeginWriting() }.step("IMFSinkWriter::BeginWriting")?;
        let encoder = video_encoder_info(&writer, video_stream);

        Ok(Self {
            writer,
            video_stream,
            audio,
            width: settings.width as usize,
            height: settings.height as usize,
            frame_period: frame_period_hns(settings.frame_rate),
            last_timestamp: None,
            video_frames: 0,
            video_end: 0,
            encoder,
            _session: session,
        })
    }

    /// The H.264 encoder the sink writer loaded.
    pub fn video_encoder(&self) -> &EncoderInfo {
        &self.encoder
    }

    /// Adds one frame: `width × height` RGBA pixels, top row first, alpha ignored. Timestamps
    /// start at zero and must strictly increase; each frame lasts one nominal frame period.
    pub fn push_frame(&mut self, rgba: &[u8], timestamp: Duration) -> Result<(), MediaError> {
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

        let len = nv12::nv12_len(self.width, self.height);
        let len_u32 = u32::try_from(len)
            .map_err(|_| MediaError::InvalidSettings(format!("frame of {len} bytes")))?;
        // A fresh buffer per frame: the sink writer may still hold earlier samples.
        // SAFETY: plain allocation call.
        let buffer = unsafe { MFCreateMemoryBuffer(len_u32) }.step("MFCreateMemoryBuffer")?;
        {
            let mut locked = LockedBuffer::lock(&buffer)?;
            nv12::rgba_to_nv12(rgba, self.width, self.height, locked.as_mut_slice())?;
        }
        // SAFETY: valid buffer; `len` bytes were just written and fit in its allocation.
        unsafe { buffer.SetCurrentLength(len_u32) }.step("IMFMediaBuffer::SetCurrentLength")?;

        let start = duration_to_hns(timestamp);
        write_sample(
            &self.writer,
            self.video_stream,
            &buffer,
            start,
            self.frame_period,
        )?;
        self.last_timestamp = Some(timestamp);
        self.video_frames += 1;
        self.video_end = start.saturating_add(self.frame_period);
        self.release_audio(Some(self.video_end))?;
        // The narration may stop long before the video ends: tell the sink writer the sound
        // track has a gap, or it keeps waiting for audio and the export gets tens of times
        // slower (measured: 42 s instead of 0.6 s for 5 s of video after the narration).
        if let Some(audio) = &self.audio
            && audio.pending.is_empty()
        {
            let audio_end = frames_to_hns(audio.written_frames, audio.settings.sample_rate);
            if audio_end.saturating_add(AUDIO_GAP_BEFORE_TICK) < start {
                // SAFETY: valid sink writer and audio stream index; writing has begun.
                unsafe { self.writer.SendStreamTick(audio.stream, start) }
                    .step("IMFSinkWriter::SendStreamTick")?;
            }
        }
        Ok(())
    }

    /// Adds interleaved 16-bit PCM in the format given at creation. Audio is contiguous: its
    /// timestamps come from the number of samples pushed so far, starting at zero.
    pub fn push_audio(&mut self, samples: &[i16]) -> Result<(), MediaError> {
        let Some(audio) = self.audio.as_mut() else {
            return Err(MediaError::NoAudioTrack);
        };
        let channels = audio.settings.channels;
        if !samples.len().is_multiple_of(usize::from(channels)) {
            return Err(MediaError::AudioLength {
                len: samples.len(),
                channels,
            });
        }
        audio.pending.extend(samples.iter().copied());
        self.release_audio(Some(self.video_end))
    }

    /// Writes what is left, finalises the file and reports what was written.
    pub fn finish(mut self) -> Result<Mp4Summary, MediaError> {
        if self.video_frames == 0 {
            return Err(MediaError::InvalidSettings(
                "no video frame was pushed".into(),
            ));
        }
        self.release_audio(None)?;
        // SAFETY: valid sink writer; `BeginWriting` succeeded in `create`.
        unsafe { self.writer.Finalize() }.step("IMFSinkWriter::Finalize")?;

        let audio_frames = self.audio.as_ref().map_or(0, |a| a.written_frames);
        let audio_end = self.audio.as_ref().map_or(0, |a| {
            frames_to_hns(a.written_frames, a.settings.sample_rate)
        });
        Ok(Mp4Summary {
            video_frames: self.video_frames,
            audio_frames,
            duration: hns_to_duration(self.video_end.max(audio_end)),
            video_encoder: self.encoder.clone(),
        })
    }

    /// Gives pending audio to the sink writer, in chunks of at most 100 ms, until the next
    /// chunk would start at or after `until` (100-ns units); `None` writes everything.
    fn release_audio(&mut self, until: Option<i64>) -> Result<(), MediaError> {
        let Some(audio) = self.audio.as_mut() else {
            return Ok(());
        };
        let channels = usize::from(audio.settings.channels);
        let rate = audio.settings.sample_rate;
        let chunk_frames = (rate / 10) as usize;
        loop {
            let pending_frames = audio.pending.len() / channels;
            if pending_frames == 0 {
                return Ok(());
            }
            let start = frames_to_hns(audio.written_frames, rate);
            if until.is_some_and(|until| start >= until) {
                return Ok(());
            }
            let frames = pending_frames.min(chunk_frames);
            let samples = frames * channels;
            let bytes = samples * 2;
            let bytes_u32 = u32::try_from(bytes).map_err(|_| {
                MediaError::InvalidSettings(format!("audio chunk of {bytes} bytes"))
            })?;

            // SAFETY: plain allocation call.
            let buffer = unsafe { MFCreateMemoryBuffer(bytes_u32) }.step("MFCreateMemoryBuffer")?;
            {
                let mut locked = LockedBuffer::lock(&buffer)?;
                let dest = locked.as_mut_slice();
                if dest.len() < bytes {
                    return Err(MediaError::MediaFoundation {
                        step: "MFCreateMemoryBuffer",
                        code: 0,
                        message: "buffer smaller than requested".into(),
                    });
                }
                for (out, sample) in dest
                    .as_chunks_mut::<2>()
                    .0
                    .iter_mut()
                    .zip(audio.pending.drain(..samples))
                {
                    *out = sample.to_le_bytes();
                }
            }
            // SAFETY: valid buffer; `bytes` bytes were just written and fit in its allocation.
            unsafe { buffer.SetCurrentLength(bytes_u32) }
                .step("IMFMediaBuffer::SetCurrentLength")?;

            let end = frames_to_hns(audio.written_frames + frames as u64, rate);
            write_sample(&self.writer, audio.stream, &buffer, start, end - start)?;
            audio.written_frames += frames as u64;
        }
    }
}

/// Wraps `buffer` in a sample stamped `start` (100-ns units) lasting `duration` and writes it.
fn write_sample(
    writer: &IMFSinkWriter,
    stream: u32,
    buffer: &IMFMediaBuffer,
    start: i64,
    duration: i64,
) -> Result<(), MediaError> {
    // SAFETY: plain creation call.
    let sample = unsafe { MFCreateSample() }.step("MFCreateSample")?;
    // SAFETY: valid sample and buffer COM objects; the sample takes its own reference.
    unsafe { sample.AddBuffer(buffer) }.step("IMFSample::AddBuffer")?;
    // SAFETY: valid sample.
    unsafe { sample.SetSampleTime(start) }.step("IMFSample::SetSampleTime")?;
    // SAFETY: valid sample.
    unsafe { sample.SetSampleDuration(duration) }.step("IMFSample::SetSampleDuration")?;
    // SAFETY: valid writer, stream index returned by AddStream, valid sample. May block while
    // the encoder catches up (sink writer throttling).
    unsafe { writer.WriteSample(stream, &sample) }.step("IMFSinkWriter::WriteSample")
}

/// A video media type with the fields shared by the H.264 output and the NV12 input.
fn video_type(settings: &Mp4Settings, subtype: &GUID) -> Result<IMFMediaType, MediaError> {
    // SAFETY: plain creation call.
    let media_type = unsafe { MFCreateMediaType() }.step("MFCreateMediaType")?;
    Attrs(&media_type)
        .guid(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?
        .guid(&MF_MT_SUBTYPE, subtype)?
        .u32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)?
        .u64(&MF_MT_FRAME_SIZE, pack(settings.width, settings.height))?
        .u64(&MF_MT_FRAME_RATE, pack(settings.frame_rate, 1))?
        .u64(&MF_MT_PIXEL_ASPECT_RATIO, pack(1, 1))?
        // Colour description matching `nv12::rgba_to_nv12` (BT.709, limited range).
        .u32(&MF_MT_YUV_MATRIX, MFVideoTransferMatrix_BT709.0 as u32)?
        .u32(&MF_MT_VIDEO_NOMINAL_RANGE, MFNominalRange_16_235.0 as u32)?
        .u32(&MF_MT_VIDEO_PRIMARIES, MFVideoPrimaries_BT709.0 as u32)?
        .u32(&MF_MT_TRANSFER_FUNCTION, MFVideoTransFunc_709.0 as u32)?;
    Ok(media_type)
}

fn add_video_stream(writer: &IMFSinkWriter, settings: &Mp4Settings) -> Result<u32, MediaError> {
    let output = video_type(settings, &MFVideoFormat_H264)?;
    Attrs(&output)
        .u32(&MF_MT_AVG_BITRATE, settings.video_bitrate)?
        .u32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_High.0 as u32)?;
    // SAFETY: valid writer and media type.
    let stream = unsafe { writer.AddStream(&output) }.step("IMFSinkWriter::AddStream (H.264)")?;

    let input = video_type(settings, &MFVideoFormat_NV12)?;
    Attrs(&input)
        // Rows are packed: the stride of both NV12 planes is the width.
        .u32(&MF_MT_DEFAULT_STRIDE, settings.width)?
        .u32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
    // Encoder settings (ICodecAPI properties, applied by the sink writer):
    let parameters = create_attributes(2)?;
    Attrs(&parameters)
        // No B-frames: with them Microsoft's software encoder shifts every frame one period
        // later (the MP4 sink writes no edit list), so video would lag the audio.
        .u32(&CODECAPI_AVEncMPVDefaultBPictureCount, 0)?
        // A key frame every 2 s keeps seeking quick in long replays.
        .u32(&CODECAPI_AVEncMPVGOPSize, settings.frame_rate * 2)?;
    // SAFETY: valid writer, stream index from AddStream, valid media type and parameters.
    unsafe { writer.SetInputMediaType(stream, &input, &parameters) }
        .map_err(|e| codec_error("H.264", "IMFSinkWriter::SetInputMediaType (NV12)", &e))?;
    Ok(stream)
}

fn add_audio_stream(writer: &IMFSinkWriter, audio: &AudioSettings) -> Result<u32, MediaError> {
    let channels = u32::from(audio.channels);

    // SAFETY: plain creation call.
    let output = unsafe { MFCreateMediaType() }.step("MFCreateMediaType")?;
    Attrs(&output)
        .guid(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?
        .guid(&MF_MT_SUBTYPE, &MFAudioFormat_AAC)?
        .u32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?
        .u32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, audio.sample_rate)?
        .u32(&MF_MT_AUDIO_NUM_CHANNELS, channels)?
        // Must be exactly 12000, 16000, 20000 or 24000 bytes/s for Microsoft's encoder.
        .u32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, AAC_BITRATE / 8)?
        // Raw AAC frames, as the MP4 container stores them (no ADTS headers).
        .u32(&MF_MT_AAC_PAYLOAD_TYPE, 0)?
        .u32(&MF_MT_AAC_AUDIO_PROFILE_LEVEL_INDICATION, AAC_LC_LEVEL_2)?;
    // SAFETY: valid writer and media type.
    let stream = unsafe { writer.AddStream(&output) }.step("IMFSinkWriter::AddStream (AAC)")?;

    // SAFETY: plain creation call.
    let input = unsafe { MFCreateMediaType() }.step("MFCreateMediaType")?;
    Attrs(&input)
        .guid(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?
        .guid(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?
        .u32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?
        .u32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, audio.sample_rate)?
        .u32(&MF_MT_AUDIO_NUM_CHANNELS, channels)?
        .u32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, channels * 2)?
        .u32(
            &MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
            audio.sample_rate * channels * 2,
        )?
        .u32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;
    // SAFETY: valid writer, stream index from AddStream, valid media type, no parameters.
    unsafe { writer.SetInputMediaType(stream, &input, None) }
        .map_err(|e| codec_error("AAC", "IMFSinkWriter::SetInputMediaType (PCM)", &e))?;
    Ok(stream)
}

/// `SetInputMediaType` is where the sink writer looks for an encoder.
fn codec_error(
    codec: &'static str,
    step: &'static str,
    error: &windows::core::Error,
) -> MediaError {
    if error.code() == MF_E_TOPO_CODEC_NOT_FOUND {
        MediaError::EncoderUnavailable {
            codec,
            code: error.code().0 as u32,
        }
    } else {
        mf_error(step, error)
    }
}

/// Finds the encoder MFT the sink writer inserted for `stream` and describes it.
fn video_encoder_info(writer: &IMFSinkWriter, stream: u32) -> EncoderInfo {
    let mut info = EncoderInfo {
        name: None,
        hardware: false,
    };
    let Some(transform) = video_encoder(writer, stream) else {
        return info;
    };
    // Hardware MFTs have an attribute store holding their hardware URL, vendor and class
    // identifier, but no friendly name; Microsoft's software encoder has no store at all.
    // SAFETY: valid transform.
    let attributes = unsafe { transform.GetAttributes() }.ok();
    info.hardware = attributes
        .as_ref()
        .is_some_and(|a| get_string(a, &MFT_ENUM_HARDWARE_URL_Attribute).is_some());
    let clsid = attributes
        .as_ref()
        // SAFETY: valid attribute store; constant key.
        .and_then(|a| unsafe { a.GetGUID(&MFT_TRANSFORM_CLSID_Attribute) }.ok())
        .or_else(|| {
            let persist = transform.cast::<IPersist>().ok()?;
            // SAFETY: valid COM object.
            unsafe { persist.GetClassID() }.ok()
        });
    // The friendly name is registered with the class: look it up in the encoder list.
    info.name = clsid.and_then(|clsid| {
        h264_encoders()
            .ok()?
            .into_iter()
            .find(|r| r.clsid == Some(clsid))?
            .info
            .name
    });
    info
}

/// The encoder MFT of the sink writer's chain for `stream`.
fn video_encoder(writer: &IMFSinkWriter, stream: u32) -> Option<IMFTransform> {
    let writer_ex = writer.cast::<IMFSinkWriterEx>().ok()?;
    // The chain is short (converter, encoder); the bound only guards against surprises.
    for index in 0..8 {
        let mut category = GUID::zeroed();
        let mut transform: Option<IMFTransform> = None;
        // SAFETY: valid writer; the out-pointers are valid locals filled on success.
        let found = unsafe {
            writer_ex.GetTransformForStream(stream, index, Some(&mut category), &mut transform)
        };
        if found.is_err() {
            return None; // MF_E_INVALIDINDEX: end of the chain
        }
        if category == MFT_CATEGORY_VIDEO_ENCODER && transform.is_some() {
            return transform;
        }
    }
    None
}
