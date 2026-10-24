//! MP4 export behind the same [`VideoSink`] as the other formats: Windows Media Foundation by
//! default (H.264 + AAC, nothing to download), Cisco's OpenH264 2.6.0 as the fallback when
//! Media Foundation is missing or fails (H.264 by OpenH264, sound in Opus, muxed in pure Rust:
//! `openh264/`), SPEC §6.2 "Vidéo MP4".
//!
//! - [`create_mp4`] picks the back end ([`Mp4Backend`]). With `Auto`, Media Foundation is used
//!   when its DLLs load and it creates a writer; otherwise OpenH264, if the user allowed it
//!   (`Some(codec)`) and it is installed intact.
//! - [`media_foundation_available`]: whether `mfplat.dll` and `mfreadwrite.dll` load. The
//!   `ardoise` executable delay-loads them (so that it starts on Windows N editions): calling
//!   [`crate::Mp4Writer`], [`crate::probe_mp4`] or [`crate::available_encoders`] when this is
//!   false would end the process. [`create_mp4`] checks it; [`probe_mp4_animation`] does not
//!   use Media Foundation.
//! - [`OpenH264Codec`]: the library in the codec folder: [`status`](OpenH264Codec::status),
//!   [`download`](OpenH264Codec::download) from Cisco, [`remove`](OpenH264Codec::remove).
//!   Cisco's conditions for the interface: the user starts the download, can disable the
//!   fallback (pass `None`) and remove the library; [`CISCO_NOTICE`] is shown where it is
//!   controlled and [`OPENH264_LICENSE`] where licences are.
//! - [`probe_mp4_animation`] reads a file of either back end with Mozilla's `mp4parse`
//!   (Firefox's MP4 parser, pure Rust), independently of both writers and of Media Foundation:
//!   sample entry size, video samples counted (H.264 is not decoded), track and container
//!   durations from the sample tables and edit lists; an Opus track is decoded packet by packet
//!   to measure the sound really there, an AAC track is measured by its frames.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use mp4parse::{AudioCodecSpecific, CodecType, SampleEntry, Track, TrackType};

use crate::openh264::install;
pub use crate::openh264::install::{OPENH264_SHA256, OPENH264_URL};
use crate::opus::{OpusHead, OpusPacketDecoder};
use crate::{
    AnimationFormat, AnimationProbe, AudioProbe, AudioSettings, MediaError, Mp4Settings, Mp4Writer,
    NARRATION_SAMPLE_RATE, VideoSettings, VideoSink, VideoSummary,
};

/// The notice Cisco's licence requires wherever the user controls the codec (SPEC §6.2).
pub const CISCO_NOTICE: &str = "OpenH264 Video Codec provided by Cisco Systems, Inc.";

/// The only OpenH264 release the `openh264` crate accepts by hash (docs/research/media-stack.md §1).
pub const OPENH264_VERSION: &str = "2.6.0";

/// Cisco's licence for its OpenH264 binary (BSD text, AVC/H.264 patent portfolio notice and
/// Cisco's conditions), from <https://www.openh264.org/BINARY_LICENSE.txt>: to reproduce
/// where the app shows licensing information (condition 4).
pub const OPENH264_LICENSE: &str = include_str!("openh264/OPENH264-BINARY-LICENSE.txt");

/// Which H.264 encoder writes the MP4.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mp4Backend {
    /// Media Foundation when it is there, else OpenH264 when it is installed and allowed.
    #[default]
    Auto,
    MediaFoundation,
    /// Forces the fallback (tests, and PCs whose Media Foundation misbehaves).
    OpenH264,
}

/// MP4 options on top of [`VideoSettings`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp4Options {
    pub backend: Mp4Backend,
    /// Lets Media Foundation use the GPU's encoder (ignored by OpenH264).
    pub hardware_acceleration: bool,
    /// Average H.264 bitrate in bits per second; `None` for a default suited to the size.
    pub video_bitrate: Option<u32>,
}

impl Default for Mp4Options {
    fn default() -> Self {
        Self {
            backend: Mp4Backend::Auto,
            hardware_acceleration: true,
            video_bitrate: None,
        }
    }
}

/// State of the OpenH264 library on this PC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecStatus {
    NotInstalled,
    /// Present, with the expected SHA-256.
    Installed {
        path: PathBuf,
        bytes: u64,
    },
    /// A file is there but does not match the expected hash (it is never loaded).
    Damaged(String),
}

/// The OpenH264 library, downloaded from Cisco on the user's request into `dir` (the app uses
/// `%LOCALAPPDATA%\Ardoise\codecs`). Whether the user allows its use is an app setting: pass
/// `None` to [`create_mp4`] when it is disabled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenH264Codec {
    dir: PathBuf,
}

impl OpenH264Codec {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The codec folder of the app: `ARDOISE_CODECS_DIR` when set (tests, diagnostics), else
    /// `%LOCALAPPDATA%\Ardoise\codecs` (local to the PC, never next to a portable exe that may
    /// sit in a synced folder). `None` when neither variable is set.
    pub fn default_location() -> Option<Self> {
        let var = |name: &str| std::env::var_os(name).filter(|v| !v.is_empty());
        var("ARDOISE_CODECS_DIR")
            .map(PathBuf::from)
            .or_else(|| {
                var("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join("Ardoise").join("codecs"))
            })
            .map(Self::new)
    }

    /// Folder holding the library.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Where the library is (or would be) installed.
    pub fn library_path(&self) -> PathBuf {
        install::library_path(&self.dir)
    }

    /// Whether the library is installed and intact (its SHA-256 is computed each time).
    pub fn status(&self) -> CodecStatus {
        install::status(&self.dir)
    }

    /// Downloads the library from Cisco's server ([`OPENH264_URL`]), checks its SHA-256
    /// ([`OPENH264_SHA256`]) and installs it atomically, replacing any previous file.
    /// `progress(received, total)` is called as the compressed bytes arrive (`total` when the
    /// server tells it); setting `cancel` stops it with [`MediaError::Cancelled`]. On any
    /// error nothing is left behind. Blocking: call it from a worker thread.
    pub fn download(
        &self,
        progress: &mut dyn FnMut(u64, Option<u64>),
        cancel: &AtomicBool,
    ) -> Result<(), MediaError> {
        install::download(&self.dir, progress, cancel)
    }

    /// Deletes the library (the user can remove it at any time), and any temporary file of an
    /// interrupted download. Succeeds when there is nothing to remove.
    pub fn remove(&self) -> Result<(), MediaError> {
        install::remove(&self.dir)
    }
}

/// True when Media Foundation's DLLs (`mfplat.dll`, `mfreadwrite.dll`) load from the system
/// folder; false on Windows N editions without the Media Feature Pack, and outside Windows.
/// Checked once per process.
pub fn media_foundation_available() -> bool {
    #[cfg(windows)]
    {
        static AVAILABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *AVAILABLE.get_or_init(|| {
            ["mfplat.dll", "mfreadwrite.dll"]
                .into_iter()
                .all(system_library_loads)
        })
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Loads `name` from System32 only (never from the app's folder or the current directory),
/// then releases it.
#[cfg(windows)]
fn system_library_loads(name: &str) -> bool {
    use windows::Win32::Foundation::FreeLibrary;
    use windows::Win32::System::LibraryLoader::{LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW};
    use windows::core::HSTRING;

    // SAFETY: the name is a valid null-terminated wide string for the duration of the call;
    // no file handle and no flag that would run code other than the library's own entry point
    // (system DLLs from System32).
    match unsafe { LoadLibraryExW(&HSTRING::from(name), None, LOAD_LIBRARY_SEARCH_SYSTEM32) } {
        Ok(module) => {
            // SAFETY: `module` was returned by the successful load just above and is released
            // once; nothing from it is used.
            let _ = unsafe { FreeLibrary(module) };
            true
        }
        Err(_) => false,
    }
}

/// Creates an MP4 writer. `openh264` is the fallback library when the user allows it (`None`
/// when the setting is off).
///
/// - `MediaFoundation`: Media Foundation only ([`MediaError::MediaFoundationUnavailable`] when
///   its DLLs are missing).
/// - `OpenH264`: the fallback only ([`MediaError::CodecUnavailable`] when it is not allowed,
///   not installed or damaged).
/// - `Auto`: Media Foundation, else the fallback when Media Foundation is missing or cannot
///   create a writer (no encoder, a broken installation) and the library is allowed and
///   installed; otherwise the Media Foundation error, or `CodecUnavailable`.
pub fn create_mp4(
    path: &Path,
    settings: &VideoSettings,
    options: &Mp4Options,
    openh264: Option<&OpenH264Codec>,
) -> Result<Box<dyn VideoSink>, MediaError> {
    match options.backend {
        Mp4Backend::MediaFoundation => create_media_foundation(path, settings, options),
        Mp4Backend::OpenH264 => create_openh264(path, settings, options, openh264),
        Mp4Backend::Auto => {
            let media_foundation = create_media_foundation(path, settings, options);
            match media_foundation {
                Ok(sink) => Ok(sink),
                Err(error) if falls_back(&error) => {
                    match openh264.map(|codec| (codec, codec.status())) {
                        Some((codec, CodecStatus::Installed { .. })) => {
                            create_openh264(path, settings, options, Some(codec))
                        }
                        _ => Err(error),
                    }
                }
                Err(error) => Err(error),
            }
        }
    }
}

/// Media Foundation errors that the fallback can work around (not bad settings or a file that
/// cannot be written: OpenH264 would fail the same way).
fn falls_back(error: &MediaError) -> bool {
    matches!(
        error,
        MediaError::Unsupported
            | MediaError::MediaFoundationUnavailable { .. }
            | MediaError::EncoderUnavailable { .. }
            | MediaError::MediaFoundation { .. }
    )
}

fn create_media_foundation(
    path: &Path,
    settings: &VideoSettings,
    options: &Mp4Options,
) -> Result<Box<dyn VideoSink>, MediaError> {
    if cfg!(windows) && !media_foundation_available() {
        return Err(MediaError::MediaFoundationUnavailable {
            // HRESULT_FROM_WIN32(ERROR_MOD_NOT_FOUND)
            code: 0x8007_007E,
            message: "mfplat.dll or mfreadwrite.dll is missing (Windows N edition without the \
                      Media Feature Pack?)"
                .into(),
        });
    }
    let mut mp4 = Mp4Settings::new(settings.width, settings.height, settings.frame_rate);
    if let Some(channels) = settings.audio_channels {
        mp4 = mp4.with_audio(AudioSettings::new(NARRATION_SAMPLE_RATE, channels));
    }
    if let Some(bitrate) = options.video_bitrate {
        mp4.video_bitrate = bitrate;
    }
    mp4.hardware_acceleration = options.hardware_acceleration;
    let writer = Mp4Writer::create(path, &mp4)?;
    Ok(Box::new(MediaFoundationSink { writer }))
}

fn create_openh264(
    path: &Path,
    settings: &VideoSettings,
    options: &Mp4Options,
    openh264: Option<&OpenH264Codec>,
) -> Result<Box<dyn VideoSink>, MediaError> {
    let Some(codec) = openh264 else {
        return Err(MediaError::CodecUnavailable(
            "the OpenH264 fallback is not allowed in the settings".into(),
        ));
    };
    match codec.status() {
        CodecStatus::Installed { path: library, .. } => {
            crate::openh264::create(path, &library, settings, options)
        }
        CodecStatus::NotInstalled => Err(MediaError::CodecUnavailable(
            "OpenH264 is not installed (download it from Cisco in the settings)".into(),
        )),
        CodecStatus::Damaged(why) => Err(MediaError::CodecUnavailable(format!(
            "the OpenH264 library is damaged and was not loaded: {why}"
        ))),
    }
}

/// Reads an MP4 back independently of its writer (either back end), without Media
/// Foundation: see the module documentation.
pub fn probe_mp4_animation(path: &Path) -> Result<AnimationProbe, MediaError> {
    let invalid = |what: String| MediaError::InvalidFile(format!("MP4: {what}"));
    let mut file = BufReader::new(File::open(path)?);
    let context = mp4parse::read_mp4(&mut file).map_err(|e| invalid(format!("{e:?}")))?;
    let movie_timescale = context
        .timescale
        .map(|t| t.0)
        .filter(|&t| t > 0)
        .ok_or_else(|| invalid("no movie timescale".into()))?;

    let video = context
        .tracks
        .iter()
        .find(|t| t.track_type == TrackType::Video)
        .ok_or_else(|| invalid("no video track".into()))?;
    let (width, height) = match first_entry(video) {
        Some(SampleEntry::Video(entry)) if entry.codec_type == CodecType::H264 => {
            (u32::from(entry.width), u32::from(entry.height))
        }
        Some(SampleEntry::Video(entry)) => {
            return Err(invalid(format!("video codec {:?}", entry.codec_type)));
        }
        _ => return Err(invalid("video track without a sample entry".into())),
    };
    let frames = sample_count(video);
    let video_duration = presented_duration(video, movie_timescale, None)?;

    let audio = match context
        .tracks
        .iter()
        .find(|t| t.track_type == TrackType::Audio)
    {
        Some(track) => Some(probe_audio(track, movie_timescale, &mut file)?),
        None => None,
    };
    let duration = video_duration.max(audio.as_ref().map_or(Duration::ZERO, |a| a.duration));
    Ok(AnimationProbe {
        format: AnimationFormat::Mp4,
        width,
        height,
        frames,
        duration,
        audio,
    })
}

/// The sound track: AAC measured by its frames, Opus decoded packet by packet.
fn probe_audio(
    track: &Track,
    movie_timescale: u64,
    file: &mut (impl Read + Seek),
) -> Result<AudioProbe, MediaError> {
    let invalid = |what: String| MediaError::InvalidFile(format!("MP4 sound: {what}"));
    let Some(SampleEntry::Audio(entry)) = first_entry(track) else {
        return Err(invalid("no audio sample entry".into()));
    };
    match (&entry.codec_type, &entry.codec_specific) {
        (CodecType::AAC, AudioCodecSpecific::ES_Descriptor(esds)) => {
            let channels = esds
                .audio_channel_count
                .unwrap_or(u16::try_from(entry.channelcount).unwrap_or(0));
            let sample_rate = esds
                .audio_sample_rate
                .unwrap_or(entry.samplerate.round() as u32);
            Ok(AudioProbe {
                codec: "AAC".into(),
                channels,
                sample_rate,
                duration: presented_duration(track, movie_timescale, None)?,
            })
        }
        (CodecType::Opus, AudioCodecSpecific::OpusSpecificBox(dops)) => {
            let mut head = Vec::new();
            mp4parse::serialize_opus_header(dops, &mut head)
                .map_err(|e| invalid(format!("dOps: {e:?}")))?;
            let head = OpusHead::parse(&head)?;
            let mut decoder = OpusPacketDecoder::new(head.channels)?;
            let mut decoded = 0u64;
            let mut packet = Vec::new();
            for (offset, size) in sample_ranges(track)? {
                packet.resize(size as usize, 0);
                file.seek(SeekFrom::Start(offset))?;
                file.read_exact(&mut packet)?;
                decoded += decoder.decode(&packet)? as u64;
            }
            // What a player outputs: the decoded samples after the pre-skip (or the edit
            // list's start), cut at the end of the edit.
            let skip = track.media_time.map_or(u64::from(head.pre_skip), |t| t.0);
            let decoded = decoded.saturating_sub(skip);
            Ok(AudioProbe {
                codec: "Opus".into(),
                channels: head.channels,
                sample_rate: NARRATION_SAMPLE_RATE,
                duration: presented_duration(track, movie_timescale, Some(decoded))?,
            })
        }
        (codec, _) => Err(invalid(format!("audio codec {codec:?}"))),
    }
}

fn first_entry(track: &Track) -> Option<&SampleEntry> {
    track.stsd.as_ref()?.descriptions.first()
}

/// Samples in the track (from the time-to-sample table).
fn sample_count(track: &Track) -> u64 {
    track.stts.as_ref().map_or(0, |stts| {
        stts.samples
            .iter()
            .map(|entry| u64::from(entry.sample_count))
            .sum()
    })
}

/// How long the track plays: its samples (or `decoded` media samples when given) after the
/// edit list's start, cut at the edit's end when there is an edit list.
fn presented_duration(
    track: &Track,
    movie_timescale: u64,
    decoded: Option<u64>,
) -> Result<Duration, MediaError> {
    let timescale = track
        .timescale
        .map(|t| t.0)
        .filter(|&t| t > 0)
        .ok_or_else(|| MediaError::InvalidFile("MP4: a track without a timescale".into()))?;
    let media = match decoded {
        Some(samples) => to_duration(samples, NARRATION_SAMPLE_RATE.into()),
        None => {
            let ticks: u64 = track.stts.as_ref().map_or(0, |stts| {
                stts.samples
                    .iter()
                    .map(|e| u64::from(e.sample_count) * u64::from(e.sample_delta))
                    .sum()
            });
            let skip = track.media_time.map_or(0, |t| t.0);
            to_duration(ticks.saturating_sub(skip), timescale)
        }
    };
    Ok(match track.edited_duration {
        Some(edit) => media.min(to_duration(edit.0, movie_timescale)),
        None => media,
    })
}

/// File offset and size of every sample, from the chunk tables.
fn sample_ranges(track: &Track) -> Result<Vec<(u64, u32)>, MediaError> {
    let invalid = |what: &str| MediaError::InvalidFile(format!("MP4 sample tables: {what}"));
    let (Some(stsc), Some(stco), Some(stsz)) = (&track.stsc, &track.stco, &track.stsz) else {
        return Err(invalid("missing"));
    };
    let count = usize::try_from(sample_count(track)).map_err(|_| invalid("too many samples"))?;
    let size_of = |index: usize| -> Result<u32, MediaError> {
        if stsz.sample_size > 0 {
            Ok(stsz.sample_size)
        } else {
            stsz.sample_sizes
                .get(index)
                .copied()
                .ok_or_else(|| invalid("fewer sizes than samples"))
        }
    };
    let mut ranges = Vec::with_capacity(count);
    for (chunk, &offset) in stco.offsets.iter().enumerate() {
        let number = u32::try_from(chunk + 1).map_err(|_| invalid("too many chunks"))?;
        let per_chunk = stsc
            .samples
            .iter()
            .take_while(|entry| entry.first_chunk <= number)
            .last()
            .map_or(0, |entry| entry.samples_per_chunk);
        let mut position = offset;
        for _ in 0..per_chunk {
            if ranges.len() == count {
                return Err(invalid("more samples in chunks than in the time table"));
            }
            let size = size_of(ranges.len())?;
            ranges.push((position, size));
            position += u64::from(size);
        }
    }
    if ranges.len() != count {
        return Err(invalid("fewer samples in chunks than in the time table"));
    }
    Ok(ranges)
}

fn to_duration(ticks: u64, timescale: u64) -> Duration {
    let nanos = u128::from(ticks) * 1_000_000_000 / u128::from(timescale.max(1));
    Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX))
}

/// [`Mp4Writer`] as a [`VideoSink`].
struct MediaFoundationSink {
    writer: Mp4Writer,
}

impl VideoSink for MediaFoundationSink {
    fn push_frame(&mut self, rgba: &[u8], timestamp: Duration) -> Result<(), MediaError> {
        self.writer.push_frame(rgba, timestamp)
    }

    fn push_audio(&mut self, samples: &[i16]) -> Result<(), MediaError> {
        self.writer.push_audio(samples)
    }

    fn finish(self: Box<Self>) -> Result<VideoSummary, MediaError> {
        let summary = self.writer.finish()?;
        let encoder = &summary.video_encoder;
        let name = encoder.name.as_deref().unwrap_or("software encoder");
        let kind = if encoder.hardware {
            "hardware"
        } else {
            "software"
        };
        Ok(VideoSummary {
            format: AnimationFormat::Mp4,
            frames: summary.video_frames,
            duration: summary.duration,
            audio: summary.audio_frames > 0,
            encoder: format!("Media Foundation: {name} ({kind})"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(audio_channels: Option<u16>) -> VideoSettings {
        VideoSettings {
            width: 320,
            height: 240,
            frame_rate: 10,
            audio_channels,
        }
    }

    #[test]
    fn the_fallback_needs_permission_and_an_intact_library() {
        let dir = tempfile::tempdir().unwrap();
        let forced = Mp4Options {
            backend: Mp4Backend::OpenH264,
            ..Mp4Options::default()
        };
        let path = dir.path().join("unused.mp4");
        let error = |codec: Option<&OpenH264Codec>| {
            create_mp4(&path, &settings(None), &forced, codec)
                .err()
                .unwrap()
        };
        // Not allowed.
        assert!(matches!(error(None), MediaError::CodecUnavailable(m) if m.contains("allowed")));
        // Allowed but not installed.
        let codec = OpenH264Codec::new(dir.path().join("codecs"));
        assert_eq!(codec.status(), CodecStatus::NotInstalled);
        let not_installed = error(Some(&codec));
        assert!(
            matches!(&not_installed, MediaError::CodecUnavailable(m) if m.contains("installed")),
            "{not_installed}"
        );
        // A damaged file is never loaded.
        std::fs::create_dir_all(codec.dir()).unwrap();
        std::fs::write(codec.library_path(), b"MZ damaged").unwrap();
        assert!(matches!(codec.status(), CodecStatus::Damaged(_)));
        assert!(
            matches!(error(Some(&codec)), MediaError::CodecUnavailable(m) if m.contains("damaged"))
        );
        codec.remove().unwrap();
        assert_eq!(codec.status(), CodecStatus::NotInstalled);
    }

    #[test]
    fn the_default_location_honours_the_override() {
        // Only read here: the variable is set by the test runner when wanted.
        if let Some(dir) = std::env::var_os("ARDOISE_CODECS_DIR").filter(|v| !v.is_empty()) {
            assert_eq!(
                OpenH264Codec::default_location().unwrap().dir(),
                Path::new(&dir)
            );
        } else if let Some(local) = std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()) {
            let expected = PathBuf::from(local).join("Ardoise").join("codecs");
            assert_eq!(OpenH264Codec::default_location().unwrap().dir(), expected);
        }
    }

    #[test]
    fn the_licence_text_carries_the_notice() {
        assert!(OPENH264_LICENSE.contains(CISCO_NOTICE));
        assert!(OPENH264_LICENSE.contains("AVC/H.264 Patent Portfolio"));
    }

    #[test]
    fn probing_rejects_what_is_not_an_mp4() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("not.mp4");
        std::fs::write(&path, b"GIF89a this is not an MP4 file").unwrap();
        assert!(probe_mp4_animation(&path).is_err());
        assert!(probe_mp4_animation(&dir.path().join("missing.mp4")).is_err());
    }

    /// The installed OpenH264 library of the app's codec folder, or `None` with a message
    /// (`tests/openh264_download.rs` downloads it).
    #[cfg(windows)]
    pub(crate) fn installed_codec() -> Option<OpenH264Codec> {
        let codec = OpenH264Codec::default_location()?;
        match codec.status() {
            CodecStatus::Installed { .. } => Some(codec),
            other => {
                println!(
                    "SKIPPED: OpenH264 is not installed in {} ({other:?}); run `cargo test -p \
                     ardoise-media --test openh264_download -- --ignored` once",
                    codec.dir().display()
                );
                None
            }
        }
    }

    /// Frame `index` of a 320 × 240 test animation: cream paper, a blue square moving right.
    #[cfg(windows)]
    fn moving_square(index: u32) -> Vec<u8> {
        let (width, height) = (320usize, 240usize);
        let mut frame = [250u8, 246, 235, 255].repeat(width * height);
        let x0 = 20 + 10 * index as usize;
        for y in 80..160 {
            for x in x0..(x0 + 80).min(width) {
                frame[(y * width + x) * 4..][..4].copy_from_slice(&[30, 60, 200, 255]);
            }
        }
        frame
    }

    /// The fallback forced: OpenH264 and Opus, read back by the probe and decoded by Media
    /// Foundation's H.264 decoder (frames, colours).
    #[cfg(windows)]
    #[test]
    fn openh264_writes_through_the_video_sink() {
        let Some(codec) = installed_codec() else {
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fallback.mp4");
        let forced = Mp4Options {
            backend: Mp4Backend::OpenH264,
            ..Mp4Options::default()
        };
        let mut sink = create_mp4(&path, &settings(Some(1)), &forced, Some(&codec)).unwrap();
        // 1.5 s of narration for 2 s of video, pushed with the frames.
        let tone: Vec<i16> = (0..72_000)
            .map(|i| ((i as f64 * 440.0 / 48_000.0 * std::f64::consts::TAU).sin() * 8000.0) as i16)
            .collect();
        let mut chunks = tone.chunks(4_800);
        for k in 0..20u32 {
            if let Some(chunk) = chunks.next() {
                sink.push_audio(chunk).unwrap();
            }
            sink.push_frame(&moving_square(k), Duration::from_millis(100 * u64::from(k)))
                .unwrap();
        }
        let summary = sink.finish().unwrap();
        assert_eq!(
            (summary.format, summary.frames, summary.audio),
            (AnimationFormat::Mp4, 20, true)
        );
        assert_eq!(summary.duration, Duration::from_secs(2));
        assert!(
            summary.encoder.starts_with("OpenH264 2.6.0"),
            "{}",
            summary.encoder
        );

        let probe = probe_mp4_animation(&path).unwrap();
        assert_eq!((probe.width, probe.height, probe.frames), (320, 240, 20));
        assert_eq!(probe.duration, Duration::from_secs(2));
        let audio = probe.audio.unwrap();
        assert_eq!(
            (audio.codec.as_str(), audio.channels, audio.sample_rate),
            ("Opus", 1, 48_000)
        );
        assert_eq!(audio.duration, Duration::from_millis(1500));

        // Media Foundation's decoder reads the H.264 track: every frame, and the colours.
        let decoded = crate::probe_mp4(
            &path,
            crate::ProbeOptions {
                capture_frame: Some(10),
                capture_audio: false,
            },
        );
        match decoded {
            Ok(decoded) => {
                let video = decoded.video.unwrap();
                println!(
                    "Media Foundation: {} frames, {:?} to {:?}, sound {:?}",
                    video.frames, video.first_timestamp, video.last_timestamp, decoded.audio
                );
                assert_eq!((video.width, video.height, video.frames), (320, 240, 20));
                let frame = decoded.captured_frame.unwrap();
                // Frame 10: the square spans x = 120..200.
                let close =
                    |a: [u8; 4], b: [u8; 3]| a.iter().zip(b).all(|(&x, y)| x.abs_diff(y) <= 6);
                let (paper, square) =
                    (frame.pixel(40, 40).unwrap(), frame.pixel(160, 120).unwrap());
                println!("decoded paper {paper:?}, square {square:?}");
                assert!(close(paper, [250, 246, 235]), "paper {paper:?}");
                assert!(close(square, [30, 60, 200]), "square {square:?}");
            }
            Err(e) => panic!("Media Foundation cannot read the fallback's MP4: {e}"),
        }
    }

    /// The Media Foundation path through the shared sink, read back by the probe.
    #[cfg(windows)]
    #[test]
    fn media_foundation_writes_through_the_video_sink() {
        assert!(media_foundation_available());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sink.mp4");
        let mut sink = create_mp4(&path, &settings(Some(1)), &Mp4Options::default(), None).unwrap();
        let frame = vec![200u8; 320 * 240 * 4];
        let audio = vec![0i16; 4_800];
        for k in 0..10u64 {
            sink.push_audio(&audio).unwrap();
            sink.push_frame(&frame, Duration::from_millis(100 * k))
                .unwrap();
        }
        let summary = sink.finish().unwrap();
        assert_eq!(
            (summary.format, summary.frames, summary.audio),
            (AnimationFormat::Mp4, 10, true)
        );
        assert!(summary.encoder.starts_with("Media Foundation: "));
        let probe = probe_mp4_animation(&path).unwrap();
        assert_eq!((probe.width, probe.height, probe.frames), (320, 240, 10));
        let audio = probe.audio.unwrap();
        assert_eq!((audio.codec.as_str(), audio.channels), ("AAC", 1));
        assert_eq!(audio.sample_rate, 48_000);
        // AAC pads to whole 1024-sample frames.
        let extra = audio.duration.saturating_sub(Duration::from_secs(1));
        assert!(
            audio.duration >= Duration::from_secs(1) && extra < Duration::from_micros(21_334),
            "{:?}",
            audio.duration
        );
    }
}
