//! Reads a media file back through `IMFSourceReader`, independently of the writer.
//!
//! Video is decoded and converted to RGB32 by Media Foundation itself (the same decoder and
//! video processor Windows players use), audio is decoded to 16-bit PCM, and both are counted.

use std::path::Path;
use std::time::Duration;

use windows::Win32::Media::MediaFoundation::{
    IMF2DBuffer, IMFMediaType, IMFSample, IMFSourceReader, MF_E_INVALIDSTREAMNUMBER,
    MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS,
    MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_DEFAULT_STRIDE, MF_MT_FRAME_SIZE, MF_MT_MAJOR_TYPE,
    MF_MT_SUBTYPE, MF_PD_DURATION, MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_ANY_STREAM,
    MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, MF_SOURCE_READER_MEDIASOURCE,
    MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READERF_ERROR, MFAudioFormat_AAC, MFAudioFormat_PCM, MFCreateMediaType,
    MFCreateSourceReaderFromURL, MFMediaType_Audio, MFMediaType_Video, MFVideoFormat_H264,
    MFVideoFormat_RGB32,
};
use windows::Win32::System::Com::StructuredStorage::PropVariantClear;
use windows::Win32::System::Variant::VT_UI8;
use windows::core::{GUID, HSTRING, Interface};

use super::{Attrs, LockedBuffer, MfSession, StepExt, create_attributes, mf_error, unpack};
use crate::timing::{frames_to_hns, hns_to_duration};
use crate::{
    AudioTrackProbe, Codec, MediaError, Mp4Probe, ProbeOptions, RgbaFrame, VideoTrackProbe,
};

/// Opens `path`, reads the container duration and track formats, then decodes every video
/// frame and audio sample to count them.
pub fn probe_mp4(path: &Path, options: ProbeOptions) -> Result<Mp4Probe, MediaError> {
    let _session = MfSession::start()?;

    let attributes = create_attributes(1)?;
    // Lets the reader convert decoded NV12 to RGB32.
    Attrs(&attributes).u32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;
    let url = HSTRING::from(path);
    // SAFETY: `url` is a NUL-terminated UTF-16 path that outlives the call; valid attributes.
    let reader = unsafe { MFCreateSourceReaderFromURL(&url, &attributes) }
        .step("MFCreateSourceReaderFromURL")?;

    let duration = container_duration(&reader)?;
    let (video_native, audio_native) = find_streams(&reader)?;

    // SAFETY: valid reader; ALL_STREAMS is an accepted pseudo-index.
    unsafe { reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false) }
        .step("IMFSourceReader::SetStreamSelection")?;

    let mut video = match &video_native {
        Some((index, native)) => Some(VideoState::open(&reader, *index, native)?),
        None => None,
    };
    let mut audio = match &audio_native {
        Some((index, native)) => Some(AudioState::open(&reader, *index, native, options)?),
        None => None,
    };

    let mut captured_frame = None;
    let mut video_done = video.is_none();
    let mut audio_done = audio.is_none();
    while !(video_done && audio_done) {
        let mut stream = 0u32;
        let mut flags = 0u32;
        let mut timestamp = 0i64;
        let mut sample: Option<IMFSample> = None;
        // SAFETY: valid reader; all out-pointers are valid locals.
        unsafe {
            reader.ReadSample(
                MF_SOURCE_READER_ANY_STREAM.0 as u32,
                0,
                Some(&mut stream),
                Some(&mut flags),
                Some(&mut timestamp),
                Some(&mut sample),
            )
        }
        .step("IMFSourceReader::ReadSample")?;
        if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
            return Err(MediaError::InvalidFile(format!(
                "read error on stream {stream}"
            )));
        }
        let end = flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0;
        let changed = flags & MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 as u32 != 0;

        if let Some(v) = video.as_mut().filter(|v| v.index == stream) {
            if changed {
                v.refresh_geometry(&reader)?;
            }
            if let Some(sample) = &sample {
                if options.capture_frame == Some(v.frames) {
                    captured_frame = Some(v.copy_frame(sample)?);
                }
                if v.frames == 0 {
                    v.first_timestamp = timestamp;
                }
                v.frames += 1;
                v.last_timestamp = timestamp;
            }
            video_done |= end;
        } else if let Some(a) = audio.as_mut().filter(|a| a.index == stream) {
            if changed {
                a.refresh_format(&reader)?;
            }
            if let Some(sample) = &sample {
                a.add(sample)?;
            }
            audio_done |= end;
        } else if end {
            // Should not happen (other streams are deselected); stop rather than loop.
            break;
        }
    }

    Ok(Mp4Probe {
        duration,
        video: video.as_ref().map(VideoState::result),
        audio: audio.as_ref().map(AudioState::result),
        captured_frame,
        audio_samples: audio.and_then(|a| a.samples),
    })
}

/// `MF_PD_DURATION` of the media source: the duration declared by the container.
fn container_duration(reader: &IMFSourceReader) -> Result<Duration, MediaError> {
    // SAFETY: valid reader; MEDIASOURCE is an accepted pseudo-index and the key a valid GUID.
    let mut value = unsafe {
        reader.GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
    }
    .step("IMFSourceReader::GetPresentationAttribute (duration)")?;
    // SAFETY: reading the tag and, when it says VT_UI8, the matching union member of an
    // initialised PROPVARIANT.
    let hns = unsafe {
        let inner = &value.Anonymous.Anonymous;
        (inner.vt == VT_UI8).then(|| inner.Anonymous.uhVal)
    };
    // SAFETY: frees whatever the PROPVARIANT owns (nothing for VT_UI8); it is not used after.
    let _ = unsafe { PropVariantClear(&mut value) };
    let hns = hns.ok_or_else(|| MediaError::InvalidFile("duration is not a VT_UI8".into()))?;
    Ok(hns_to_duration(i64::try_from(hns).unwrap_or(i64::MAX)))
}

type NativeStream = Option<(u32, IMFMediaType)>;

/// First video and first audio stream, with their native (compressed) media types.
fn find_streams(reader: &IMFSourceReader) -> Result<(NativeStream, NativeStream), MediaError> {
    let mut video = None;
    let mut audio = None;
    for index in 0u32.. {
        // SAFETY: valid reader; out-of-range indices fail with MF_E_INVALIDSTREAMNUMBER.
        let native = match unsafe { reader.GetNativeMediaType(index, 0) } {
            Ok(native) => native,
            Err(e) if e.code() == MF_E_INVALIDSTREAMNUMBER => break,
            Err(e) => return Err(mf_error("IMFSourceReader::GetNativeMediaType", &e)),
        };
        // SAFETY: valid media type; constant key.
        let major = unsafe { native.GetGUID(&MF_MT_MAJOR_TYPE) }.step("GetGUID (major type)")?;
        if major == MFMediaType_Video && video.is_none() {
            video = Some((index, native));
        } else if major == MFMediaType_Audio && audio.is_none() {
            audio = Some((index, native));
        }
    }
    Ok((video, audio))
}

fn codec(media_type: &IMFMediaType) -> Result<Codec, MediaError> {
    // SAFETY: valid media type; constant key.
    let subtype = unsafe { media_type.GetGUID(&MF_MT_SUBTYPE) }.step("GetGUID (subtype)")?;
    Ok(if subtype == MFVideoFormat_H264 {
        Codec::H264
    } else if subtype == MFAudioFormat_AAC {
        Codec::Aac
    } else {
        Codec::Other(format!("{subtype:?}"))
    })
}

fn get_u32(media_type: &IMFMediaType, key: &GUID, step: &'static str) -> Result<u32, MediaError> {
    // SAFETY: valid media type; the key reference lives for the call.
    unsafe { media_type.GetUINT32(key) }.step(step)
}

fn frame_size(media_type: &IMFMediaType) -> Result<(u32, u32), MediaError> {
    // SAFETY: valid media type; constant key.
    let packed =
        unsafe { media_type.GetUINT64(&MF_MT_FRAME_SIZE) }.step("GetUINT64 (frame size)")?;
    Ok(unpack(packed))
}

struct VideoState {
    index: u32,
    codec: Codec,
    /// Size declared by the file.
    width: u32,
    height: u32,
    /// Size and stride of the decoded RGB32 frames (may differ: decoders align to 16 rows).
    out_width: u32,
    out_height: u32,
    out_stride: Option<i32>,
    frames: u64,
    first_timestamp: i64,
    last_timestamp: i64,
}

impl VideoState {
    fn open(
        reader: &IMFSourceReader,
        index: u32,
        native: &IMFMediaType,
    ) -> Result<Self, MediaError> {
        let (width, height) = frame_size(native)?;
        // SAFETY: plain creation call.
        let output = unsafe { MFCreateMediaType() }.step("MFCreateMediaType")?;
        Attrs(&output)
            .guid(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?
            .guid(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        // SAFETY: valid reader, stream index from enumeration, valid media type.
        unsafe { reader.SetCurrentMediaType(index, None, &output) }
            .step("IMFSourceReader::SetCurrentMediaType (RGB32)")?;
        // SAFETY: valid reader and stream index.
        unsafe { reader.SetStreamSelection(index, true) }
            .step("IMFSourceReader::SetStreamSelection (video)")?;
        let mut state = Self {
            index,
            codec: codec(native)?,
            width,
            height,
            out_width: width,
            out_height: height,
            out_stride: None,
            frames: 0,
            first_timestamp: 0,
            last_timestamp: 0,
        };
        state.refresh_geometry(reader)?;
        Ok(state)
    }

    fn refresh_geometry(&mut self, reader: &IMFSourceReader) -> Result<(), MediaError> {
        // SAFETY: valid reader and stream index.
        let current = unsafe { reader.GetCurrentMediaType(self.index) }
            .step("IMFSourceReader::GetCurrentMediaType (video)")?;
        (self.out_width, self.out_height) = frame_size(&current)?;
        // The stride attribute holds a signed value in a UINT32 (negative: bottom-up).
        // SAFETY: valid media type; constant key.
        self.out_stride = unsafe { current.GetUINT32(&MF_MT_DEFAULT_STRIDE) }
            .ok()
            .map(|s| s as i32);
        Ok(())
    }

    /// Copies the visible `width × height` part of a decoded RGB32 (B, G, R, X) frame.
    fn copy_frame(&self, sample: &IMFSample) -> Result<RgbaFrame, MediaError> {
        let width = self.width.min(self.out_width) as usize;
        let height = self.height.min(self.out_height) as usize;
        let mut pixels = Vec::with_capacity(width * height * 4);

        // SAFETY: valid sample.
        let buffer = unsafe { sample.GetBufferByIndex(0) }.step("IMFSample::GetBufferByIndex")?;
        if let Ok(buffer_2d) = buffer.cast::<IMF2DBuffer>() {
            let mut scanline0 = std::ptr::null_mut();
            let mut pitch = 0i32;
            // SAFETY: valid buffer; out-pointers are valid locals. Balanced by Unlock2D below.
            unsafe { buffer_2d.Lock2D(&mut scanline0, &mut pitch) }.step("IMF2DBuffer::Lock2D")?;
            let row_len = self.out_width as usize * 4;
            let rows = self.out_height as usize;
            let result = if scanline0.is_null() || (pitch.unsigned_abs() as usize) < row_len {
                Err(MediaError::InvalidFile("bad 2D buffer layout".into()))
            } else {
                copy_rows(&mut pixels, width, height, |y| {
                    if y >= rows {
                        return None;
                    }
                    // SAFETY: Lock2D guarantees `rows` rows of at least `row_len` bytes, the
                    // first at `scanline0` and each next one `pitch` bytes further (pitch is
                    // negative for bottom-up images); the memory stays valid until Unlock2D.
                    Some(unsafe {
                        std::slice::from_raw_parts(
                            scanline0.offset(y as isize * pitch as isize),
                            row_len,
                        )
                    })
                })
            };
            // SAFETY: balances the successful Lock2D above; no row slice outlives this point.
            let _ = unsafe { buffer_2d.Unlock2D() };
            result?;
        } else {
            let locked = LockedBuffer::lock(&buffer)?;
            let data = locked.as_slice();
            let row_len = self.out_width as usize * 4;
            let stride = self.out_stride.unwrap_or(row_len as i32);
            let abs = stride.unsigned_abs() as usize;
            let rows = self.out_height as usize;
            copy_rows(&mut pixels, width, height, |y| {
                let line = if stride < 0 {
                    rows.checked_sub(y + 1)?
                } else {
                    y
                };
                data.get(line * abs..line * abs + row_len)
            })?;
        }
        Ok(RgbaFrame {
            width: width as u32,
            height: height as u32,
            pixels,
        })
    }

    fn result(&self) -> VideoTrackProbe {
        VideoTrackProbe {
            codec: self.codec.clone(),
            width: self.width,
            height: self.height,
            frames: self.frames,
            first_timestamp: hns_to_duration(self.first_timestamp),
            last_timestamp: hns_to_duration(self.last_timestamp),
        }
    }
}

/// Appends `height` rows of `width` B, G, R, X pixels, as RGBA, to `pixels`.
fn copy_rows<'d>(
    pixels: &mut Vec<u8>,
    width: usize,
    height: usize,
    row: impl Fn(usize) -> Option<&'d [u8]>,
) -> Result<(), MediaError> {
    for y in 0..height {
        let line = row(y)
            .and_then(|line| line.get(..width * 4))
            .ok_or_else(|| MediaError::InvalidFile("decoded frame too small".into()))?;
        for bgrx in line.as_chunks::<4>().0 {
            pixels.extend_from_slice(&[bgrx[2], bgrx[1], bgrx[0], 255]);
        }
    }
    Ok(())
}

struct AudioState {
    index: u32,
    codec: Codec,
    channels: u16,
    sample_rate: u32,
    block_align: usize,
    frames: u64,
    samples: Option<Vec<i16>>,
}

impl AudioState {
    fn open(
        reader: &IMFSourceReader,
        index: u32,
        native: &IMFMediaType,
        options: ProbeOptions,
    ) -> Result<Self, MediaError> {
        // SAFETY: plain creation call.
        let output = unsafe { MFCreateMediaType() }.step("MFCreateMediaType")?;
        Attrs(&output)
            .guid(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?
            .guid(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?
            .u32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
        // SAFETY: valid reader, stream index from enumeration, valid media type.
        unsafe { reader.SetCurrentMediaType(index, None, &output) }
            .step("IMFSourceReader::SetCurrentMediaType (PCM)")?;
        // SAFETY: valid reader and stream index.
        unsafe { reader.SetStreamSelection(index, true) }
            .step("IMFSourceReader::SetStreamSelection (audio)")?;
        let channels = get_u32(native, &MF_MT_AUDIO_NUM_CHANNELS, "GetUINT32 (channels)")?;
        let sample_rate = get_u32(
            native,
            &MF_MT_AUDIO_SAMPLES_PER_SECOND,
            "GetUINT32 (sample rate)",
        )?;
        let mut state = Self {
            index,
            codec: codec(native)?,
            channels: u16::try_from(channels).unwrap_or(u16::MAX),
            sample_rate,
            block_align: 2,
            frames: 0,
            samples: options.capture_audio.then(Vec::new),
        };
        state.refresh_format(reader)?;
        Ok(state)
    }

    fn refresh_format(&mut self, reader: &IMFSourceReader) -> Result<(), MediaError> {
        // SAFETY: valid reader and stream index.
        let current = unsafe { reader.GetCurrentMediaType(self.index) }
            .step("IMFSourceReader::GetCurrentMediaType (audio)")?;
        let align = get_u32(
            &current,
            &MF_MT_AUDIO_BLOCK_ALIGNMENT,
            "GetUINT32 (block align)",
        )?;
        self.block_align = (align as usize).max(1);
        Ok(())
    }

    fn add(&mut self, sample: &IMFSample) -> Result<(), MediaError> {
        // SAFETY: valid sample.
        let buffer =
            unsafe { sample.ConvertToContiguousBuffer() }.step("ConvertToContiguousBuffer")?;
        let locked = LockedBuffer::lock(&buffer)?;
        let data = locked.as_slice();
        self.frames += (data.len() / self.block_align) as u64;
        if let Some(samples) = self.samples.as_mut() {
            samples.extend(
                data.as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| i16::from_le_bytes(*b)),
            );
        }
        Ok(())
    }

    fn result(&self) -> AudioTrackProbe {
        AudioTrackProbe {
            codec: self.codec.clone(),
            channels: self.channels,
            sample_rate: self.sample_rate,
            frames: self.frames,
            duration: hns_to_duration(frames_to_hns(self.frames, self.sample_rate)),
        }
    }
}
