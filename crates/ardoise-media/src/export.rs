//! The animated export pipeline: a note, its replay options, the format's options, an optional
//! narration and a final frame held N seconds give the frames of [`FrameRenderer`] pushed with
//! their sound into the format's [`VideoSink`] (`docs/design/media.md`, "Interface package").
//!
//! # Timing
//!
//! The video is, in order: a blank page while a narration that began before the note's time
//! origin plays (the lead-in, `-start_ms`), the replay, then the whole note held for
//! [`AnimationExport::hold_end`] or until the narration ends, whichever is longer. Frame `k` is
//! at [`frame_time`]`(k, frame_rate)` and shows replay instant `frame time - lead-in` (clamped
//! at zero). The frame count is the duration times the frame rate, rounded up, so the file ends
//! within one frame after [`ExportPlan::duration`]; the last frame always shows the whole note
//! (at most one frame early when the replay ends between two frames and nothing is held).
//!
//! A narration sets the time base: its replay options replace the export's (it was recorded
//! against that replay) and its first sample plays at video time `max(0, start_ms)`. Its
//! timing applies to every format; only MP4 and WebM carry its sound. The sound is pushed
//! with the frames, in exact sample counts: before frame `k`, everything up to the end of frame
//! `k` (`(k + 1) × 48000 / frame_rate` samples, rounded down, on the absolute timeline so
//! nothing drifts), silence before the narration starts, the rest before the sink finishes.
//!
//! # The file
//!
//! The sink writes a hidden temporary file next to the destination (`.<name>.<pid>.<n>.part`,
//! the naming of `ardoise_core::fsutil`: hidden from the library, never uploaded by Nextcloud),
//! which is synced and renamed over the destination only once the sink has finished. On an
//! error or a cancel the temporary file is removed and the destination is left as it was.

use std::fs::OpenOptions;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use ardoise_core::fsutil;
use ardoise_core::note::{Note, PageSize};
use ardoise_render::Backdrop;

use crate::narration::Narration;
use crate::opus::samples_to_duration;
use crate::replay::{FrameRenderer, FrameSettings, ReplayOptions, Timeline, frame_time};
use crate::{
    AnimationFormat, AudioClip, GifOptions, MediaError, Mp4Options, NARRATION_SAMPLE_RATE,
    OpenH264Codec, VideoSettings, VideoSink, VideoSummary, WebmOptions, WebpOptions, create_gif,
    create_mp4, create_webm, create_webp,
};

/// The format to write, with its own options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FormatOptions {
    Mp4(Mp4Options),
    Gif(GifOptions),
    WebP(WebpOptions),
    WebM(WebmOptions),
}

impl FormatOptions {
    /// The format these options are for.
    pub fn format(&self) -> AnimationFormat {
        match self {
            FormatOptions::Mp4(_) => AnimationFormat::Mp4,
            FormatOptions::Gif(_) => AnimationFormat::Gif,
            FormatOptions::WebP(_) => AnimationFormat::WebP,
            FormatOptions::WebM(_) => AnimationFormat::WebM,
        }
    }
}

/// One animated export.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationExport {
    /// Frame size in pixels; even for MP4 and WebM.
    pub width: u32,
    pub height: u32,
    pub frame_rate: u32,
    /// Replaced by the narration's options when a narration is given.
    pub replay: ReplayOptions,
    /// `Backdrop::Transparent` only for GIF and WebP.
    pub backdrop: Backdrop,
    /// How long the final frame stays (0 to 60 s).
    pub hold_end: Duration,
    pub format: FormatOptions,
}

/// What an export will be.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportPlan {
    /// The replay options actually used.
    pub replay: ReplayOptions,
    /// Blank page before the replay starts (a narration that begins before the note).
    pub lead_in: Duration,
    pub replay_duration: Duration,
    /// Whole video: lead-in, replay, then the longer of the held frame and the narration.
    pub duration: Duration,
    /// Frames pushed (at least 1).
    pub frames: u64,
    /// Video time of the narration's first sample; `None` without a sound track.
    pub audio_start: Option<Duration>,
}

/// Progress of [`export_animation`], reported after every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportProgress {
    pub frames_done: u64,
    pub frames_total: u64,
}

/// Frames per second accepted by every format.
const MAX_FRAME_RATE: u32 = 60;
/// Longest final frame held.
const MAX_HOLD_END: Duration = Duration::from_secs(60);
/// Longest video: far beyond any replay (a 3-hour lesson replayed at 0.25× lasts 12 hours), so
/// only absurd input (a damaged narration's start) is refused.
const MAX_VIDEO_DURATION: Duration = Duration::from_secs(24 * 3600);
/// Largest frame side the replay renderer draws (`replay::frames`).
const MAX_RENDER_SIDE: u32 = 16_384;
/// Samples per channel of silence pushed at a time before the narration starts.
const SILENCE_CHUNK: u64 = 4_800;

/// Frame sizes a format's sink accepts: `min..=max` a side, even or not.
#[derive(Debug, Clone, Copy)]
struct SizeLimits {
    min: u32,
    max: u32,
    even: bool,
}

/// The limits of each sink (`settings.rs` for MP4, `webm.rs`, `gif.rs`, `webp.rs`), within
/// what the renderer draws; at least 2 pixels a side.
fn size_limits(format: AnimationFormat) -> SizeLimits {
    let (min, max, even) = match format {
        AnimationFormat::Mp4 => (16, 4096, true),
        AnimationFormat::WebM => (16, 8192, true),
        AnimationFormat::Gif => (2, u32::from(u16::MAX), false),
        AnimationFormat::WebP => (2, 16_383, false),
    };
    SizeLimits {
        min,
        max: max.min(MAX_RENDER_SIDE),
        even,
    }
}

/// The plan of an export (no file is touched).
///
/// Fails with [`MediaError::InvalidSettings`] for a frame rate outside 1 to 60, a size outside
/// the format's limits (MP4: even, 16 to 4096; WebM: even, 16 to 8192; GIF: 2 to 16 384; WebP:
/// 2 to 16 383), a transparent background in MP4 or WebM, a held frame longer than 60 s, a
/// speed outside 0.25 to 16, a narration that MP4 or WebM cannot carry (not mono or stereo,
/// a partial frame), or a video longer than 24 hours.
pub fn plan_export(
    note: &Note,
    export: &AnimationExport,
    narration: Option<&Narration>,
) -> Result<ExportPlan, MediaError> {
    let format = export.format.format();
    validate(export, format)?;
    let replay = narration.map_or(export.replay, |n| n.options);
    let replay_duration = Timeline::new(note, &replay)?.duration();

    // Where the narration lies in video time: (start, length).
    let span = match narration {
        Some(narration) => {
            if format.has_sound() {
                validate_clip(&narration.clip)?;
            }
            let start = Duration::from_millis(u64::try_from(narration.start_ms).unwrap_or(0));
            Some((start, clip_duration(&narration.clip)))
        }
        None => None,
    };
    let lead_in = narration.map_or(Duration::ZERO, |n| {
        Duration::from_millis(n.start_ms.min(0).unsigned_abs())
    });
    let replay_end = lead_in.saturating_add(replay_duration);
    let narration_end = span.map_or(Duration::ZERO, |(start, length)| {
        start.saturating_add(length)
    });
    let tail = export
        .hold_end
        .max(narration_end.saturating_sub(replay_end));
    let duration = replay_end.saturating_add(tail);
    if duration > MAX_VIDEO_DURATION {
        return Err(MediaError::InvalidSettings(format!(
            "a video of {} s (24 hours at most)",
            duration.as_secs()
        )));
    }
    let audio_start = span
        .filter(|(_, length)| format.has_sound() && !length.is_zero())
        .map(|(start, _)| start);
    Ok(ExportPlan {
        replay,
        lead_in,
        replay_duration,
        duration,
        frames: frame_count(duration, export.frame_rate),
        audio_start,
    })
}

fn validate(export: &AnimationExport, format: AnimationFormat) -> Result<(), MediaError> {
    if !(1..=MAX_FRAME_RATE).contains(&export.frame_rate) {
        return Err(MediaError::InvalidSettings(format!(
            "frame rate {} (must be from 1 to {MAX_FRAME_RATE})",
            export.frame_rate
        )));
    }
    let SizeLimits { min, max, even } = size_limits(format);
    for (name, value) in [("width", export.width), ("height", export.height)] {
        if !(min..=max).contains(&value) || (even && value % 2 != 0) {
            let parity = if even { "even, " } else { "" };
            return Err(MediaError::InvalidSettings(format!(
                "{name} {value} for {} (must be {parity}from {min} to {max})",
                format.extension()
            )));
        }
    }
    if export.backdrop == Backdrop::Transparent
        && !matches!(format, AnimationFormat::Gif | AnimationFormat::WebP)
    {
        return Err(MediaError::InvalidSettings(format!(
            "a transparent background in {} (only GIF and WebP have one)",
            format.extension()
        )));
    }
    if export.hold_end > MAX_HOLD_END {
        return Err(MediaError::InvalidSettings(format!(
            "final frame held {} s (60 s at most)",
            export.hold_end.as_secs_f64()
        )));
    }
    Ok(())
}

/// A clip that can go into a sound track: mono or stereo, whole frames.
fn validate_clip(clip: &AudioClip) -> Result<(), MediaError> {
    if !(1..=2).contains(&clip.channels) {
        return Err(MediaError::InvalidSettings(format!(
            "a narration with {} channels (mono or stereo only)",
            clip.channels
        )));
    }
    let len = clip.samples.len();
    if !len.is_multiple_of(usize::from(clip.channels)) {
        return Err(MediaError::InvalidSettings(format!(
            "a narration of {len} samples in {} channels (a partial frame)",
            clip.channels
        )));
    }
    Ok(())
}

/// Samples per channel of `clip`.
fn clip_frames(clip: &AudioClip) -> u64 {
    (clip.samples.len() / usize::from(clip.channels.max(1))) as u64
}

/// Exact length of `clip` (to the nanosecond, rounded down).
fn clip_duration(clip: &AudioClip) -> Duration {
    samples_to_duration(clip_frames(clip))
}

/// Frames covering `duration` at `frame_rate`: rounded up, at least one.
fn frame_count(duration: Duration, frame_rate: u32) -> u64 {
    let units = duration.as_nanos() * u128::from(frame_rate);
    u64::try_from(units.div_ceil(1_000_000_000))
        .unwrap_or(u64::MAX)
        .max(1)
}

/// Samples per channel of the sound track up to video time `frame_time(frame, frame_rate)`.
fn audio_frames_before(frame: u64, frame_rate: u32) -> u64 {
    let samples = u128::from(frame) * u128::from(NARRATION_SAMPLE_RATE) / u128::from(frame_rate);
    u64::try_from(samples).unwrap_or(u64::MAX)
}

/// Writes the animation of `note` to `path` (replaced only once complete).
///
/// Validates like [`plan_export`], prepares the frames (a page that cannot be drawn at that
/// size fails here, before any file is created), then writes to a temporary file next to
/// `path`, renamed over it once the sink has finished. `progress` is called after every frame;
/// `cancel` is checked before every frame, after the last one (before the sink's final flush)
/// and before the rename, and stops the export with [`MediaError::Cancelled`]. On any error
/// the temporary file is removed and `path` is left untouched. `openh264` is the MP4 fallback
/// when the user allows it.
pub fn export_animation(
    note: &Note,
    export: &AnimationExport,
    narration: Option<&Narration>,
    path: &Path,
    openh264: Option<&OpenH264Codec>,
    progress: &mut dyn FnMut(ExportProgress),
    cancel: &AtomicBool,
) -> Result<VideoSummary, MediaError> {
    let mut job = Job::new(note, export, narration)?;
    let format = export.format;
    let mut open = |temp: &Path, settings: &VideoSettings| match &format {
        FormatOptions::Mp4(options) => create_mp4(temp, settings, options, openh264),
        FormatOptions::Gif(options) => create_gif(temp, settings, options),
        FormatOptions::WebP(options) => create_webp(temp, settings, options),
        FormatOptions::WebM(options) => create_webm(temp, settings, options),
    };
    write_file(path, &mut job, &mut open, progress, cancel)
}

/// How [`write_file`] creates the sink: on the temporary file, with the job's settings.
type OpenSink<'a> = dyn FnMut(&Path, &VideoSettings) -> Result<Box<dyn VideoSink>, MediaError> + 'a;

/// Writes `job` through the sink `open` creates on a temporary file next to `path`, then puts
/// the file in place (see [`export_animation`]).
fn write_file(
    path: &Path,
    job: &mut Job<'_>,
    open: &mut OpenSink<'_>,
    progress: &mut dyn FnMut(ExportProgress),
    cancel: &AtomicBool,
) -> Result<VideoSummary, MediaError> {
    let temp = temp_path_for(path);
    let result = write_animation(&temp, job, open, progress, cancel).and_then(|summary| {
        sync_file(&temp)?;
        // A cancel during the final flush still leaves nothing.
        check_cancel(cancel)?;
        rename_replacing(&temp, path)?;
        Ok(summary)
    });
    if result.is_err() {
        // Best effort (the sink may not have created it): the original error is the one
        // worth reporting.
        let _ = fsutil::remove_file_with_retry(&temp);
    }
    result
}

/// Creates the sink, pushes every frame and finishes the file. The sink is dropped (its file
/// closed) before this returns, whatever happens.
fn write_animation(
    temp: &Path,
    job: &mut Job<'_>,
    open: &mut OpenSink<'_>,
    progress: &mut dyn FnMut(ExportProgress),
    cancel: &AtomicBool,
) -> Result<VideoSummary, MediaError> {
    let mut sink = open(temp, &job.settings)?;
    job.push_frames(sink.as_mut(), progress, cancel)?;
    // After the last frame: a cancel then does not wait for the final flush.
    check_cancel(cancel)?;
    sink.finish()
}

/// [`MediaError::Cancelled`] once `cancel` is set.
fn check_cancel(cancel: &AtomicBool) -> Result<(), MediaError> {
    if cancel.load(Ordering::Relaxed) {
        Err(MediaError::Cancelled)
    } else {
        Ok(())
    }
}

/// One export, ready to push into a sink.
struct Job<'n> {
    plan: ExportPlan,
    renderer: FrameRenderer<'n>,
    sound: Option<SoundTrack<'n>>,
    /// The sink's settings: sound only when the plan has a sound track.
    settings: VideoSettings,
}

/// The sound track: silence, then the narration's clip.
#[derive(Debug, Clone, Copy)]
struct SoundTrack<'a> {
    clip: &'a AudioClip,
    /// Samples per channel of silence before the clip.
    silence: u64,
}

impl<'n> Job<'n> {
    /// Plans the export and prepares its frames (a page that cannot be drawn at that size
    /// fails here).
    fn new(
        note: &'n Note,
        export: &AnimationExport,
        narration: Option<&'n Narration>,
    ) -> Result<Self, MediaError> {
        let plan = plan_export(note, export, narration)?;
        let timeline = Timeline::new(note, &plan.replay)?;
        let renderer = FrameRenderer::new(
            timeline,
            &FrameSettings {
                width: export.width,
                height: export.height,
                frame_rate: export.frame_rate,
                backdrop: export.backdrop,
            },
        )?;
        let sound = match (plan.audio_start, narration) {
            (Some(start), Some(narration)) => Some(SoundTrack {
                clip: &narration.clip,
                silence: audio_frames_at(start),
            }),
            _ => None,
        };
        let settings = VideoSettings {
            width: export.width,
            height: export.height,
            frame_rate: export.frame_rate,
            audio_channels: sound.map(|s| s.clip.channels),
        };
        Ok(Self {
            plan,
            renderer,
            sound,
            settings,
        })
    }

    /// Pushes the frames of the plan and their sound into `sink`.
    fn push_frames(
        &mut self,
        sink: &mut dyn VideoSink,
        progress: &mut dyn FnMut(ExportProgress),
        cancel: &AtomicBool,
    ) -> Result<(), MediaError> {
        let plan = self.plan;
        let frame_rate = self.settings.frame_rate;
        let mut sound = self.sound.map(SoundFeed::new);
        for index in 0..plan.frames {
            check_cancel(cancel)?;
            if let Some(sound) = sound.as_mut() {
                sound.push_until(sink, audio_frames_before(index + 1, frame_rate))?;
            }
            let timestamp = frame_time(index, frame_rate);
            let mut instant = timestamp.saturating_sub(plan.lead_in);
            if index + 1 == plan.frames {
                // The last frame shows the whole note, even when the replay ends after it.
                instant = instant.max(plan.replay_duration);
            }
            let rgba = self.renderer.render_at(instant)?;
            sink.push_frame(rgba, timestamp)?;
            progress(ExportProgress {
                frames_done: index + 1,
                frames_total: plan.frames,
            });
        }
        if let Some(sound) = sound.as_mut() {
            sound.push_until(sink, u64::MAX)?;
        }
        Ok(())
    }
}

/// Samples per channel at 48 kHz in `duration` (rounded to the nearest).
fn audio_frames_at(duration: Duration) -> u64 {
    let samples =
        (duration.as_nanos() * u128::from(NARRATION_SAMPLE_RATE) + 500_000_000) / 1_000_000_000;
    u64::try_from(samples).unwrap_or(u64::MAX)
}

/// Pushes a [`SoundTrack`] in pieces.
struct SoundFeed<'a> {
    track: SoundTrack<'a>,
    channels: usize,
    /// Samples per channel of the whole track.
    total: u64,
    /// Samples per channel pushed so far.
    pushed: u64,
    /// A chunk of silence, allocated when needed.
    zeros: Vec<i16>,
}

impl<'a> SoundFeed<'a> {
    fn new(track: SoundTrack<'a>) -> Self {
        Self {
            channels: usize::from(track.clip.channels.max(1)),
            total: track.silence.saturating_add(clip_frames(track.clip)),
            pushed: 0,
            zeros: Vec::new(),
            track,
        }
    }

    /// Pushes the track up to sample `target` (per channel; clamped to its end).
    fn push_until(&mut self, sink: &mut dyn VideoSink, target: u64) -> Result<(), MediaError> {
        let target = target.min(self.total);
        let silence = self.track.silence;
        while self.pushed < target {
            if self.pushed < silence {
                let count = (silence.min(target) - self.pushed).min(SILENCE_CHUNK);
                // At most SILENCE_CHUNK × 2 samples.
                let len = count as usize * self.channels;
                if self.zeros.len() < len {
                    self.zeros.resize(SILENCE_CHUNK as usize * self.channels, 0);
                }
                sink.push_audio(self.zeros.get(..len).unwrap_or_default())?;
                self.pushed += count;
            } else {
                // Within the clip: both offsets are at most its length.
                let from = (self.pushed - silence) as usize * self.channels;
                let to = (target - silence) as usize * self.channels;
                sink.push_audio(self.track.clip.samples.get(from..to).unwrap_or_default())?;
                self.pushed = target;
            }
        }
        Ok(())
    }
}

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Longest file name NTFS accepts, in UTF-16 code units.
const MAX_FILE_NAME_UNITS: usize = 255;
/// How many times a rename refused because another process holds the target open is tried
/// (a player showing the previous export, a sync client, an antivirus).
const RENAME_ATTEMPTS: u32 = 10;
const RENAME_RETRY_DELAY: Duration = Duration::from_millis(50);

/// A unique temporary path next to `path`: `.{name}.{pid}.{n}.part`, the naming of
/// `ardoise_core::fsutil` (same directory, so the rename is atomic; the leading dot hides it
/// from the library and Nextcloud ignores `.part` files). A long name is shortened so that the
/// temporary name still fits in one file name.
fn temp_path_for(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tail = format!(".{}.{unique}.part", std::process::id());
    let room = MAX_FILE_NAME_UNITS
        .saturating_sub(1)
        .saturating_sub(tail.encode_utf16().count());
    let mut used = 0;
    let short: String = name
        .chars()
        .take_while(|c| {
            used += c.len_utf16();
            used <= room
        })
        .collect();
    path.with_file_name(format!(".{short}{tail}"))
}

/// Flushes the finished file to disk before it replaces the destination.
fn sync_file(path: &Path) -> io::Result<()> {
    // Write access: Windows refuses to flush a file opened for reading only.
    with_retry(|| OpenOptions::new().write(true).open(path)?.sync_all())
}

/// Renames `from` over `to` (replacing it).
fn rename_replacing(from: &Path, to: &Path) -> io::Result<()> {
    with_retry(|| std::fs::rename(from, to))
}

/// Runs `operation` again while another process (or an encoder still releasing the file)
/// briefly holds a file open.
fn with_retry(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    let mut attempt = 1;
    loop {
        match operation() {
            Err(error) if attempt < RENAME_ATTEMPTS && is_transient_lock(&error) => {
                attempt += 1;
                std::thread::sleep(RENAME_RETRY_DELAY);
            }
            result => return result,
        }
    }
}

/// Windows reports a file held open by another process as "access denied" (5) or "sharing
/// violation" (32).
fn is_transient_lock(error: &io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32))
}

/// Frame size with the page's proportions, the longer side `long_side` pixels; both sides even
/// and at least 2 (an odd `long_side` is rounded down to even).
pub fn page_frame_size(page: PageSize, long_side: u32) -> (u32, u32) {
    let long = (long_side & !1).max(2);
    let (width, height) = (u64::from(page.width), u64::from(page.height));
    let (large, small) = (width.max(height), width.min(height));
    let short = if large == 0 {
        long
    } else {
        // The nearest even number to long × small / large.
        let even = 2 * ((u64::from(long) * small + large) / (2 * large));
        u32::try_from(even).unwrap_or(long).clamp(2, long)
    };
    if width >= height {
        (long, short)
    } else {
        (short, long)
    }
}

#[cfg(test)]
mod tests;
