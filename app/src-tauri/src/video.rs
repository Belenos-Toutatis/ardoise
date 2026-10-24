//! Animated exports of a note's replay to MP4, GIF, WebP and WebM (SPEC §4.5), with its
//! narration for the formats that have sound. Contract: `docs/design/media.md`, "Interface
//! package: contracts" (shapes of `app/ui/src/lib/video.ts`).
//!
//! - The source is loaded like the image exports (`export.rs`): the editor's note, or the note
//!   at its path. The narration always comes from the note file at `source.path` (its
//!   `audio.opus`), even when the note carries unsaved strokes.
//! - The interface's options become an [`AnimationExport`] here; the pipeline itself is
//!   `ardoise_media::export_animation`, which writes the file under a temporary name and puts
//!   it in place only once complete.
//! - The export runs off the interface thread, reports its progress at most 10 times a second
//!   (plus the end), and stops when `cancel_video_export` is called (`cancelled`).

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use ardoise_core::library::{Library, LibraryError};
use ardoise_core::note::Note;
use ardoise_media::replay::{MAX_SPEED, MIN_SPEED, ReplayOptions};
use ardoise_media::{
    AnimationExport, AnimationFormat, ExportProgress, FormatOptions, GifOptions, MediaError,
    Mp4Options, Narration, OpenH264Codec, WebmOptions, WebpOptions, decode_narration,
    estimate_export_time, export_animation, page_frame_size, plan_export,
};
use ardoise_render::Backdrop;
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;

use crate::AppContext;
use crate::codec::{codec_dir, openh264_allowed};
use crate::commands::{CommandError, CommandResult};
use crate::export::{CANCELLED, ExportSource};
use crate::library::{library_error, library_of, path_text};

/// Error code of a video export that failed while drawing, encoding or writing.
pub const VIDEO_EXPORT_FAILED: &str = "videoExportFailed";
/// Error code of options the export cannot use.
pub const INVALID_VIDEO_SETTINGS: &str = "invalidVideoSettings";
/// Error code of an MP4 export without any H.264 encoder (no Media Foundation, and the OpenH264
/// fallback not installed or not allowed).
pub const CODEC_UNAVAILABLE: &str = "codecUnavailable";

/// Frame rates offered by the interface (`FRAME_RATES` in `video.ts`).
pub const FRAME_RATES: [u32; 5] = [10, 15, 24, 30, 60];
/// Longest final frame, seconds.
pub const MAX_HOLD_END_SECONDS: f64 = 60.0;
/// Longest pause kept by "supprimer les temps morts" (`TRIMMED_PAUSE_MS` in `video.ts`).
pub const TRIMMED_PAUSE: Duration = Duration::from_millis(500);
/// Longer side of the frames of the "page" resolution.
pub const PAGE_LONG_SIDE: u32 = 1920;
/// Least time between two progress reports sent to the interface (10 per second).
pub const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// `VideoFormat` in `video.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VideoFormat {
    Mp4,
    Gif,
    Webp,
    Webm,
}

impl VideoFormat {
    pub fn animation(self) -> AnimationFormat {
        match self {
            VideoFormat::Mp4 => AnimationFormat::Mp4,
            VideoFormat::Gif => AnimationFormat::Gif,
            VideoFormat::Webp => AnimationFormat::WebP,
            VideoFormat::Webm => AnimationFormat::WebM,
        }
    }

    /// Each format with its default options (MP4: Media Foundation, else the OpenH264 fallback
    /// when the user allows it).
    pub fn options(self) -> FormatOptions {
        match self {
            VideoFormat::Mp4 => FormatOptions::Mp4(Mp4Options::default()),
            VideoFormat::Gif => FormatOptions::Gif(GifOptions::default()),
            VideoFormat::Webp => FormatOptions::WebP(WebpOptions::default()),
            VideoFormat::Webm => FormatOptions::WebM(WebmOptions::default()),
        }
    }

    /// GIF and WebP can leave the paper out (transparent pixels).
    pub fn has_transparency(self) -> bool {
        matches!(self, VideoFormat::Gif | VideoFormat::Webp)
    }
}

/// `VideoResolution` in `video.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoResolution {
    #[serde(rename = "720p")]
    Hd,
    #[serde(rename = "1080p")]
    FullHd,
    #[serde(rename = "4k")]
    Uhd,
    /// The page's proportions, the longer side [`PAGE_LONG_SIDE`] pixels.
    #[serde(rename = "page")]
    Page,
}

/// `VideoTiming` in `video.ts`: a speed, or a target duration.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum VideoTiming {
    Speed { speed: f64 },
    Duration { seconds: f64 },
}

/// `VideoExportOptions` in `video.ts`. Numbers are read as `f64` so that an odd value is refused
/// with `invalidVideoSettings` instead of failing to parse.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoExportOptions {
    pub format: VideoFormat,
    pub resolution: VideoResolution,
    pub frame_rate: f64,
    pub timing: VideoTiming,
    pub trim_pauses: bool,
    pub background: bool,
    pub hold_end_seconds: f64,
    pub narration: bool,
}

/// `VideoEstimate` in `video.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoEstimate {
    pub width: u32,
    pub height: u32,
    pub frames: u64,
    pub duration_ms: u64,
    pub export_ms: u64,
    pub speed: f64,
    pub narration_ms: Option<u64>,
}

/// `VideoProgress` in `video.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoProgress {
    pub frames_done: u64,
    pub frames_total: u64,
    pub elapsed_ms: u64,
}

/// `VideoExportResult` in `video.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VideoExportResult {
    pub path: String,
    pub frames: u64,
    pub duration_ms: u64,
    pub audio: bool,
    pub encoder: String,
}

/// The flag of the video export under way, set by `cancel_video_export`. A type of its own, so
/// that the app manages it apart from the image export's `ExportCancel` (Tauri keeps one state
/// per type): an image export may run meanwhile and is never stopped by it.
#[derive(Debug, Default, Clone)]
pub struct VideoCancel(pub Arc<AtomicBool>);

impl VideoCancel {
    /// Clears the flag for a new export and returns it, for the export's thread.
    pub fn arm(&self) -> Arc<AtomicBool> {
        self.0.store(false, Ordering::Relaxed);
        Arc::clone(&self.0)
    }

    /// Asks the export under way to stop.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// A note to replay and the narration that goes with it.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoSource {
    pub note: Note,
    pub narration: Option<Narration>,
}

fn invalid(message: impl ToString) -> CommandError {
    CommandError::new(INVALID_VIDEO_SETTINGS, message)
}

/// `duration` in whole milliseconds (saturating).
pub fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Error for the interface from the media pipeline, for an export to `format`.
pub(crate) fn video_error(error: MediaError, format: AnimationFormat) -> CommandError {
    let mp4 = format == AnimationFormat::Mp4;
    let code = match &error {
        MediaError::Cancelled => CANCELLED,
        MediaError::InvalidSettings(_) => INVALID_VIDEO_SETTINGS,
        MediaError::CodecUnavailable(_)
        | MediaError::MediaFoundationUnavailable { .. }
        | MediaError::EncoderUnavailable { .. }
        | MediaError::Unsupported
            if mp4 =>
        {
            CODEC_UNAVAILABLE
        }
        MediaError::MicrophoneNotFound(_) => crate::narration::MICROPHONE_NOT_FOUND,
        MediaError::MicrophoneSilent => crate::narration::MICROPHONE_SILENT,
        MediaError::Microphone { .. } => crate::narration::MICROPHONE_FAILED,
        _ => VIDEO_EXPORT_FAILED,
    };
    CommandError::new(code, error)
}

/// Frame size of `resolution` for a page of `note` (720p, 1080p and 4K fit the page inside,
/// with bands of paper around it).
pub fn frame_size(resolution: VideoResolution, note: &Note) -> (u32, u32) {
    match resolution {
        VideoResolution::Hd => (1280, 720),
        VideoResolution::FullHd => (1920, 1080),
        VideoResolution::Uhd => (3840, 2160),
        VideoResolution::Page => page_frame_size(note.page, PAGE_LONG_SIDE),
    }
}

/// Refuses options that no note makes valid (before any note is read).
pub fn check_options(options: &VideoExportOptions) -> CommandResult<()> {
    let rate = options.frame_rate;
    if !FRAME_RATES.iter().any(|&r| f64::from(r) == rate) {
        return Err(invalid(format!(
            "{rate} frames per second (offered: {FRAME_RATES:?})"
        )));
    }
    let hold = options.hold_end_seconds;
    if !(0.0..=MAX_HOLD_END_SECONDS).contains(&hold) {
        return Err(invalid(format!(
            "final frame held {hold} s (0 to {MAX_HOLD_END_SECONDS} s)"
        )));
    }
    match options.timing {
        VideoTiming::Speed { speed } if !(MIN_SPEED..=MAX_SPEED).contains(&speed) => {
            return Err(invalid(format!(
                "speed {speed} (from {MIN_SPEED} to {MAX_SPEED})"
            )));
        }
        VideoTiming::Duration { seconds }
            if !(seconds.is_finite() && seconds > 0.0)
                || Duration::try_from_secs_f64(seconds).is_err() =>
        {
            return Err(invalid(format!("target duration {seconds} s")));
        }
        _ => {}
    }
    if !options.background && !options.format.has_transparency() {
        return Err(invalid(format!(
            "{} cannot be transparent (GIF and WebP only)",
            options.format.animation().extension()
        )));
    }
    Ok(())
}

/// Whether the export plays the narration: asked for, and a format with sound.
pub fn wants_narration(options: &VideoExportOptions) -> bool {
    options.narration && options.format.animation().has_sound()
}

/// The pipeline's settings for `options` and `note` (see the module documentation).
pub fn animation_export(
    note: &Note,
    options: &VideoExportOptions,
) -> CommandResult<AnimationExport> {
    check_options(options)?;
    let (width, height) = frame_size(options.resolution, note);
    let max_pause = options.trim_pauses.then_some(TRIMMED_PAUSE);
    let replay = match options.timing {
        VideoTiming::Speed { speed } => ReplayOptions { speed, max_pause },
        VideoTiming::Duration { seconds } => ReplayOptions::for_duration(
            note,
            max_pause,
            Duration::try_from_secs_f64(seconds).map_err(invalid)?,
        ),
    };
    let backdrop = if options.background {
        Backdrop::Paper
    } else {
        Backdrop::Transparent
    };
    Ok(AnimationExport {
        width,
        height,
        // Checked against `FRAME_RATES`: a small whole number.
        frame_rate: options.frame_rate as u32,
        replay,
        backdrop,
        hold_end: Duration::from_secs_f64(options.hold_end_seconds),
        format: options.format.options(),
    })
}

/// Whether loading `source` reads the library: the note is not given, or its narration is
/// wanted from its file.
pub fn needs_library(source: &ExportSource, options: &VideoExportOptions) -> bool {
    source.note.is_none() || (wants_narration(options) && !source.path.is_empty())
}

/// The note to replay (the one given, else the one at `source.path`) and, when `narration`,
/// the narration stored in the file at `source.path`. A note without narration, or an unsaved
/// note whose file does not exist, has none; one that cannot be read is an error
/// (`videoExportFailed`).
pub fn load_video_source(
    library: Option<&Library>,
    source: ExportSource,
    narration: bool,
) -> CommandResult<VideoSource> {
    let path = Path::new(&source.path);
    let wants_file = source.note.is_none() || (narration && !source.path.is_empty());
    let (note, audio) = match (source.note, wants_file) {
        (Some(note), false) => (note, None),
        (given, _) => {
            let library = library.ok_or_else(|| {
                CommandError::new("noLibrary", "no library is chosen on this computer")
            })?;
            match (library.read(path), given) {
                (Ok(loaded), given) => (given.unwrap_or(loaded.file.note), loaded.file.audio),
                // The editor's note, not saved yet: no narration either.
                (Err(LibraryError::NotFound { .. }), Some(note)) => (note, None),
                (Err(error), _) => return Err(library_error(error)),
            }
        }
    };
    let narration = match audio {
        Some(bytes) if narration => Some(decode_narration(&bytes).map_err(|error| {
            CommandError::new(
                VIDEO_EXPORT_FAILED,
                format!("the narration stored in the note cannot be read: {error}"),
            )
        })?),
        _ => None,
    };
    Ok(VideoSource { note, narration })
}

/// What the export of `source` with `options` would be (nothing is written).
pub fn estimate_in(
    library: Option<&Library>,
    source: ExportSource,
    options: &VideoExportOptions,
) -> CommandResult<VideoEstimate> {
    check_options(options)?;
    let format = options.format.animation();
    let source = load_video_source(library, source, wants_narration(options))?;
    let export = animation_export(&source.note, options)?;
    let plan = plan_export(&source.note, &export, source.narration.as_ref())
        .map_err(|error| video_error(error, format))?;
    let narration_ms = plan
        .audio_start
        .and(source.narration.as_ref())
        .map(|narration| duration_ms(narration.duration()));
    Ok(VideoEstimate {
        width: export.width,
        height: export.height,
        frames: plan.frames,
        duration_ms: duration_ms(plan.duration),
        export_ms: duration_ms(estimate_export_time(
            format,
            export.width,
            export.height,
            plan.frames,
        )),
        speed: plan.replay.speed,
        narration_ms,
    })
}

/// Lets a report through at most once per interval, and the last one always.
#[derive(Debug, Clone)]
pub struct Throttle {
    interval: Duration,
    last: Option<Instant>,
    finished: bool,
}

impl Throttle {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            last: None,
            finished: false,
        }
    }

    /// Whether a report made at `now` goes out (`last`: the final report).
    pub fn ready(&mut self, now: Instant, last: bool) -> bool {
        let due = last
            || self
                .last
                .is_none_or(|sent| now.saturating_duration_since(sent) >= self.interval);
        if due {
            self.last = Some(now);
            self.finished |= last;
        }
        due
    }

    /// The final report went out.
    pub fn finished(&self) -> bool {
        self.finished
    }
}

/// Exports `source` with `options` to `destination` (replaced only once complete); `progress`
/// gets at most 10 reports a second plus the last one; `cancel` stops it (`cancelled`, nothing
/// written). `openh264`: the fallback library for MP4, when the user allows it.
pub fn export_video_in(
    library: Option<&Library>,
    source: ExportSource,
    options: &VideoExportOptions,
    destination: &Path,
    openh264: Option<&OpenH264Codec>,
    progress: &mut dyn FnMut(VideoProgress),
    cancel: &AtomicBool,
) -> CommandResult<VideoExportResult> {
    check_options(options)?;
    if destination.as_os_str().is_empty() {
        return Err(CommandError::new(
            VIDEO_EXPORT_FAILED,
            "no destination file",
        ));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(video_error(
            MediaError::Cancelled,
            options.format.animation(),
        ));
    }
    let format = options.format.animation();
    let source = load_video_source(library, source, wants_narration(options))?;
    let export = animation_export(&source.note, options)?;
    let started = Instant::now();
    let mut throttle = Throttle::new(PROGRESS_INTERVAL);
    let elapsed = |started: Instant| duration_ms(started.elapsed());
    let mut report = |done: ExportProgress| {
        if throttle.ready(Instant::now(), done.frames_done >= done.frames_total) {
            progress(VideoProgress {
                frames_done: done.frames_done,
                frames_total: done.frames_total,
                elapsed_ms: elapsed(started),
            });
        }
    };
    let summary = export_animation(
        &source.note,
        &export,
        source.narration.as_ref(),
        destination,
        openh264,
        &mut report,
        cancel,
    )
    .map_err(|error| video_error(error, format))?;
    if !throttle.finished() {
        progress(VideoProgress {
            frames_done: summary.frames,
            frames_total: summary.frames,
            elapsed_ms: elapsed(started),
        });
    }
    Ok(VideoExportResult {
        path: path_text(destination),
        frames: summary.frames,
        duration_ms: duration_ms(summary.duration),
        audio: summary.audio,
        encoder: summary.encoder,
    })
}

/// The library, if loading `source` needs it.
fn library_for(
    context: &AppContext,
    source: &ExportSource,
    options: &VideoExportOptions,
) -> CommandResult<Option<Library>> {
    if needs_library(source, options) {
        library_of(context).map(Some)
    } else {
        Ok(None)
    }
}

/// Size, length and expected export time of a video export (no file written).
#[tauri::command]
pub async fn estimate_video_export(
    context: State<'_, AppContext>,
    source: ExportSource,
    options: VideoExportOptions,
) -> CommandResult<VideoEstimate> {
    check_options(&options)?;
    let library = library_for(&context, &source, &options)?;
    tauri::async_runtime::spawn_blocking(move || estimate_in(library.as_ref(), source, &options))
        .await
        .map_err(|error| CommandError::new(VIDEO_EXPORT_FAILED, error))?
}

/// Exports the replay of `source` to `destination`; progress through `on_progress`.
#[tauri::command]
pub async fn export_video(
    context: State<'_, AppContext>,
    cancel: State<'_, VideoCancel>,
    source: ExportSource,
    options: VideoExportOptions,
    destination: String,
    on_progress: Channel<VideoProgress>,
) -> CommandResult<VideoExportResult> {
    check_options(&options)?;
    let library = library_for(&context, &source, &options)?;
    let openh264 = (options.format == VideoFormat::Mp4 && openh264_allowed(&context))
        .then(|| OpenH264Codec::new(codec_dir()));
    let flag = cancel.arm();
    // Encoding takes seconds to minutes: keep it off the async runtime's threads.
    tauri::async_runtime::spawn_blocking(move || {
        export_video_in(
            library.as_ref(),
            source,
            &options,
            Path::new(&destination),
            openh264.as_ref(),
            // The window may be gone: the export still ends normally.
            &mut |progress| {
                let _ = on_progress.send(progress);
            },
            &flag,
        )
    })
    .await
    .map_err(|error| CommandError::new(VIDEO_EXPORT_FAILED, error))?
}

/// Asks the video export under way to stop (`export_video` then fails with `cancelled`).
#[tauri::command]
pub fn cancel_video_export(cancel: State<'_, VideoCancel>) {
    cancel.cancel();
}

#[cfg(test)]
mod tests {
    use std::fs;

    use ardoise_core::format::read_note;
    use ardoise_core::note::A4_PORTRAIT;
    use ardoise_media::microphone::simulated_narration;
    use ardoise_media::{NarrationBase, encode_narration, probe_gif};
    use serde_json::json;

    use super::*;
    use crate::library::tests::{library, sample_note, write_sample};

    static NOT_CANCELLED: AtomicBool = AtomicBool::new(false);

    fn options(format: VideoFormat) -> VideoExportOptions {
        VideoExportOptions {
            format,
            resolution: VideoResolution::Hd,
            frame_rate: 10.0,
            timing: VideoTiming::Speed { speed: 1.0 },
            trim_pauses: false,
            background: true,
            hold_end_seconds: 0.5,
            narration: true,
        }
    }

    fn from_library(path: &Path) -> ExportSource {
        ExportSource {
            path: path_text(path),
            note: None,
        }
    }

    fn code(result: CommandResult<impl std::fmt::Debug>) -> &'static str {
        result.unwrap_err().code()
    }

    #[test]
    fn options_have_the_interface_shape() {
        let options: VideoExportOptions = serde_json::from_value(json!({
            "format": "webm", "resolution": "4k", "frameRate": 24,
            "timing": { "kind": "duration", "seconds": 12.5 },
            "trimPauses": true, "background": false, "holdEndSeconds": 3, "narration": true
        }))
        .unwrap();
        assert_eq!(options.format, VideoFormat::Webm);
        assert_eq!(options.resolution, VideoResolution::Uhd);
        assert_eq!(options.frame_rate, 24.0);
        assert_eq!(options.timing, VideoTiming::Duration { seconds: 12.5 });
        assert!(options.trim_pauses && !options.background && options.narration);
        for (text, resolution) in [
            ("720p", VideoResolution::Hd),
            ("1080p", VideoResolution::FullHd),
            ("4k", VideoResolution::Uhd),
            ("page", VideoResolution::Page),
        ] {
            assert_eq!(
                serde_json::from_value::<VideoResolution>(json!(text)).unwrap(),
                resolution
            );
        }
        for (text, format) in [
            ("mp4", VideoFormat::Mp4),
            ("gif", VideoFormat::Gif),
            ("webp", VideoFormat::Webp),
            ("webm", VideoFormat::Webm),
        ] {
            assert_eq!(
                serde_json::from_value::<VideoFormat>(json!(text)).unwrap(),
                format
            );
        }
        let speed: VideoTiming =
            serde_json::from_value(json!({ "kind": "speed", "speed": 2 })).unwrap();
        assert_eq!(speed, VideoTiming::Speed { speed: 2.0 });

        let estimate = VideoEstimate {
            width: 1280,
            height: 720,
            frames: 10,
            duration_ms: 1000,
            export_ms: 600,
            speed: 1.0,
            narration_ms: None,
        };
        assert_eq!(
            serde_json::to_value(estimate).unwrap(),
            json!({ "width": 1280, "height": 720, "frames": 10, "durationMs": 1000,
                    "exportMs": 600, "speed": 1.0, "narrationMs": null })
        );
        let progress = VideoProgress {
            frames_done: 3,
            frames_total: 9,
            elapsed_ms: 40,
        };
        assert_eq!(
            serde_json::to_value(progress).unwrap(),
            json!({ "framesDone": 3, "framesTotal": 9, "elapsedMs": 40 })
        );
        let result = VideoExportResult {
            path: "a.gif".into(),
            frames: 9,
            duration_ms: 900,
            audio: false,
            encoder: "gif".into(),
        };
        assert_eq!(
            serde_json::to_value(result).unwrap(),
            json!({ "path": "a.gif", "frames": 9, "durationMs": 900, "audio": false,
                    "encoder": "gif" })
        );
    }

    #[test]
    fn presets_and_options_map_to_the_pipeline() {
        let note = sample_note("Essai");
        let mut gif = options(VideoFormat::Gif);
        for (resolution, size) in [
            (VideoResolution::Hd, (1280, 720)),
            (VideoResolution::FullHd, (1920, 1080)),
            (VideoResolution::Uhd, (3840, 2160)),
            (
                VideoResolution::Page,
                page_frame_size(note.page, PAGE_LONG_SIDE),
            ),
        ] {
            gif.resolution = resolution;
            let export = animation_export(&note, &gif).unwrap();
            assert_eq!((export.width, export.height), size, "{resolution:?}");
        }
        let mut portrait = note.clone();
        portrait.page = A4_PORTRAIT;
        assert_eq!(
            frame_size(VideoResolution::Page, &portrait),
            page_frame_size(A4_PORTRAIT, PAGE_LONG_SIDE)
        );

        let mut webm = options(VideoFormat::Webm);
        webm.frame_rate = 60.0;
        webm.hold_end_seconds = 2.5;
        webm.trim_pauses = true;
        webm.timing = VideoTiming::Speed { speed: 4.0 };
        let export = animation_export(&note, &webm).unwrap();
        assert_eq!(export.frame_rate, 60);
        assert_eq!(export.hold_end, Duration::from_millis(2500));
        assert_eq!(
            export.replay,
            ReplayOptions {
                speed: 4.0,
                max_pause: Some(TRIMMED_PAUSE)
            }
        );
        assert_eq!(export.backdrop, Backdrop::Paper);
        assert!(matches!(export.format, FormatOptions::WebM(o) if o == WebmOptions::default()));

        // A target duration: the timeline's speed for it, clamped by the timeline.
        webm.timing = VideoTiming::Duration { seconds: 0.116 };
        let export = animation_export(&note, &webm).unwrap();
        assert_eq!(
            export.replay,
            ReplayOptions::for_duration(&note, Some(TRIMMED_PAUSE), Duration::from_millis(116))
        );

        // Transparent GIF and WebP; every format with its default options.
        gif.background = false;
        assert_eq!(
            animation_export(&note, &gif).unwrap().backdrop,
            Backdrop::Transparent
        );
        let mut webp = options(VideoFormat::Webp);
        webp.background = false;
        let export = animation_export(&note, &webp).unwrap();
        assert!(matches!(export.format, FormatOptions::WebP(o) if o == WebpOptions::default()));
        let export = animation_export(&note, &options(VideoFormat::Mp4)).unwrap();
        assert!(matches!(export.format, FormatOptions::Mp4(o) if o == Mp4Options::default()));
        assert!(
            matches!(export.format, FormatOptions::Mp4(o) if o.backend == ardoise_media::Mp4Backend::Auto)
        );
        let export = animation_export(&note, &options(VideoFormat::Gif)).unwrap();
        assert!(matches!(export.format, FormatOptions::Gif(o) if o == GifOptions::default()));
    }

    #[test]
    fn bad_options_are_refused() {
        let note = sample_note("Essai");
        let refused = |change: &dyn Fn(&mut VideoExportOptions)| {
            let mut bad = options(VideoFormat::Mp4);
            change(&mut bad);
            assert_eq!(code(check_options(&bad)), INVALID_VIDEO_SETTINGS, "{bad:?}");
            assert_eq!(
                code(animation_export(&note, &bad)),
                INVALID_VIDEO_SETTINGS,
                "{bad:?}"
            );
        };
        refused(&|o| o.frame_rate = 25.0);
        refused(&|o| o.frame_rate = 30.5);
        refused(&|o| o.frame_rate = f64::NAN);
        refused(&|o| o.hold_end_seconds = -1.0);
        refused(&|o| o.hold_end_seconds = 60.5);
        refused(&|o| o.hold_end_seconds = f64::INFINITY);
        refused(&|o| o.timing = VideoTiming::Speed { speed: 0.2 });
        refused(&|o| o.timing = VideoTiming::Speed { speed: 17.0 });
        refused(&|o| o.timing = VideoTiming::Speed { speed: f64::NAN });
        refused(&|o| o.timing = VideoTiming::Duration { seconds: 0.0 });
        refused(&|o| o.timing = VideoTiming::Duration { seconds: -3.0 });
        refused(&|o| o.timing = VideoTiming::Duration { seconds: f64::NAN });
        refused(&|o| o.timing = VideoTiming::Duration { seconds: 1e300 });
        // Only GIF and WebP can be transparent.
        refused(&|o| o.background = false);
        refused(&|o| {
            o.format = VideoFormat::Webm;
            o.background = false;
        });
        // The limits themselves are fine.
        for rate in FRAME_RATES {
            let mut good = options(VideoFormat::Gif);
            good.frame_rate = f64::from(rate);
            good.hold_end_seconds = 60.0;
            good.timing = VideoTiming::Speed { speed: 0.25 };
            check_options(&good).unwrap();
            good.timing = VideoTiming::Speed { speed: 16.0 };
            good.hold_end_seconds = 0.0;
            check_options(&good).unwrap();
        }
    }

    #[test]
    fn media_errors_get_the_interface_codes() {
        let mp4 = AnimationFormat::Mp4;
        let gif = AnimationFormat::Gif;
        let code_of = |error: MediaError, format| video_error(error, format).code();
        assert_eq!(code_of(MediaError::Cancelled, gif), CANCELLED);
        assert_eq!(
            code_of(MediaError::InvalidSettings("odd".into()), mp4),
            INVALID_VIDEO_SETTINGS
        );
        for error in [
            || MediaError::CodecUnavailable("none".into()),
            || MediaError::Unsupported,
            || MediaError::MediaFoundationUnavailable {
                code: 1,
                message: "N edition".into(),
            },
            || MediaError::EncoderUnavailable {
                codec: "H.264",
                code: 2,
            },
        ] {
            assert_eq!(code_of(error(), mp4), CODEC_UNAVAILABLE);
            // Not an MP4: a plain failure.
            assert_eq!(code_of(error(), gif), VIDEO_EXPORT_FAILED);
        }
        assert_eq!(
            code_of(MediaError::MicrophoneNotFound("x".into()), mp4),
            "microphoneNotFound"
        );
        assert_eq!(
            code_of(MediaError::MicrophoneSilent, mp4),
            "microphoneSilent"
        );
        assert_eq!(
            code_of(
                MediaError::Microphone {
                    step: "opening",
                    message: "x".into()
                },
                mp4
            ),
            "microphoneFailed"
        );
        assert_eq!(
            code_of(
                MediaError::Encoding {
                    codec: "AV1",
                    message: "x".into()
                },
                AnimationFormat::WebM
            ),
            VIDEO_EXPORT_FAILED
        );
        assert_eq!(
            code_of(MediaError::Io(std::io::Error::other("disk full")), gif),
            VIDEO_EXPORT_FAILED
        );
    }

    #[test]
    fn progress_is_throttled_but_the_end_always_goes_out() {
        let mut throttle = Throttle::new(Duration::from_millis(100));
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        assert!(throttle.ready(at(0), false), "the first report");
        assert!(!throttle.ready(at(30), false));
        assert!(!throttle.ready(at(99), false));
        assert!(throttle.ready(at(100), false));
        assert!(!throttle.ready(at(150), false));
        assert!(!throttle.finished());
        assert!(throttle.ready(at(160), true), "the last report");
        assert!(throttle.finished());
        // 1000 reports over one second: about ten go out.
        let mut throttle = Throttle::new(PROGRESS_INTERVAL);
        let sent = (0..1000u64)
            .filter(|&k| throttle.ready(at(k), k == 999))
            .count();
        assert!((10..=11).contains(&sent), "{sent}");
    }

    #[test]
    fn the_narration_is_read_from_the_note_file_only_when_wanted() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let mut file = library.read(&saved.path).unwrap().file;
        file.audio = Some(b"not an Ogg stream".to_vec());
        library.save(&saved.path, &file, &saved.stamp).unwrap();

        // Formats without sound, or the narration turned off, do not read it.
        let mut gif = options(VideoFormat::Gif);
        assert!(!wants_narration(&gif));
        let source = load_video_source(Some(&library), from_library(&saved.path), false).unwrap();
        assert_eq!(source.narration, None);
        assert_eq!(source.note, file.note);
        let mut mp4 = options(VideoFormat::Mp4);
        assert!(wants_narration(&mp4));
        mp4.narration = false;
        assert!(!wants_narration(&mp4));
        gif.narration = true;
        assert!(!wants_narration(&gif));

        // An unreadable narration is an error, not a silent video.
        let error = load_video_source(Some(&library), from_library(&saved.path), true).unwrap_err();
        assert_eq!(error.code(), VIDEO_EXPORT_FAILED);

        // The editor's note with its unsaved strokes, and the narration of its file.
        let mut edited = file.note.clone();
        edited.strokes.push(edited.strokes[0].clone());
        let with_note = ExportSource {
            path: path_text(&saved.path),
            note: Some(edited.clone()),
        };
        assert!(needs_library(&with_note, &options(VideoFormat::Webm)));
        assert!(!needs_library(&with_note, &options(VideoFormat::Gif)));
        let loaded = load_video_source(Some(&library), with_note.clone(), false).unwrap();
        assert_eq!(loaded.note, edited);
        assert_eq!(
            load_video_source(Some(&library), with_note, true)
                .unwrap_err()
                .code(),
            VIDEO_EXPORT_FAILED
        );

        // A note without narration, an unsaved note, and a note that is not in the library.
        let plain = write_sample(&library, "", "Sans son");
        let source = load_video_source(Some(&library), from_library(&plain.path), true).unwrap();
        assert_eq!(source.narration, None);
        let unsaved = ExportSource {
            path: path_text(&library.root().join("2026-10-24 Pas encore.ardoise")),
            note: Some(sample_note("Pas encore")),
        };
        assert_eq!(
            load_video_source(Some(&library), unsaved, true)
                .unwrap()
                .narration,
            None
        );
        let missing = library.root().join("2026-10-24 Absente.ardoise");
        assert_eq!(
            load_video_source(Some(&library), from_library(&missing), true)
                .unwrap_err()
                .code(),
            "noteNotFound"
        );
        assert_eq!(
            load_video_source(None, from_library(&saved.path), false)
                .unwrap_err()
                .code(),
            "noLibrary"
        );
        // Bad options are refused before any note is read.
        let mut bad = options(VideoFormat::Mp4);
        bad.frame_rate = 7.0;
        assert_eq!(
            code(estimate_in(None, from_library(&missing), &bad)),
            INVALID_VIDEO_SETTINGS
        );
        let out = tempfile::tempdir().unwrap();
        assert_eq!(
            code(export_video_in(
                None,
                from_library(&missing),
                &bad,
                &out.path().join("x.mp4"),
                None,
                &mut |_| {},
                &NOT_CANCELLED,
            )),
            INVALID_VIDEO_SETTINGS
        );
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_cancelled_export_writes_nothing() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let out = tempfile::tempdir().unwrap();
        let cancelled = AtomicBool::new(true);
        let error = export_video_in(
            Some(&library),
            from_library(&saved.path),
            &options(VideoFormat::Gif),
            &out.path().join("essai.gif"),
            None,
            &mut |_| {},
            &cancelled,
        )
        .unwrap_err();
        assert_eq!(error.code(), CANCELLED);
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);

        // The commands' flag: a new export clears it (an earlier cancel does not stop it), and
        // `cancel_video_export` reaches the export's thread through it.
        let managed = VideoCancel::default();
        managed.cancel();
        let flag = managed.arm();
        assert!(!flag.load(Ordering::Relaxed));
        managed.cancel();
        let error = export_video_in(
            Some(&library),
            from_library(&saved.path),
            &options(VideoFormat::Gif),
            &out.path().join("essai.gif"),
            None,
            &mut |_| {},
            &flag,
        )
        .unwrap_err();
        assert_eq!(error.code(), CANCELLED);
        // Tauri keeps one managed state per type: the video flag is a type of its own, so
        // cancelling a video export never reaches the image export's `ExportCancel`.
        assert_ne!(
            std::any::TypeId::of::<VideoCancel>(),
            std::any::TypeId::of::<crate::export::ExportCancel>()
        );
    }

    // --- With the media pipeline (export.rs, estimate.rs, narration.rs of ardoise-media) ---

    #[test]
    fn a_library_note_exports_to_gif_with_progress() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let out = tempfile::tempdir().unwrap();
        let destination = out.path().join("essai.gif");
        let mut reports = Vec::new();
        let result = export_video_in(
            Some(&library),
            from_library(&saved.path),
            &options(VideoFormat::Gif),
            &destination,
            None,
            &mut |progress| reports.push(progress),
            &NOT_CANCELLED,
        )
        .unwrap();
        assert_eq!(result.path, path_text(&destination));
        assert!(!result.audio);
        let last = reports.last().copied().unwrap();
        assert_eq!(last.frames_done, last.frames_total);
        assert_eq!(last.frames_total, result.frames);
        assert!(
            reports
                .windows(2)
                .all(|w| w[0].frames_done < w[1].frames_done)
        );
        let probe = probe_gif(&destination).unwrap();
        assert_eq!((probe.width, probe.height), (1280, 720));
        // GIF merges a frame identical to the previous one (the held final frame).
        assert!((1..=result.frames).contains(&probe.frames), "{probe:?}");
        let probed = duration_ms(probe.duration);
        assert!(
            probed.abs_diff(result.duration_ms) <= 50,
            "{probe:?} {result:?}"
        );
        // Only the file asked for: no temporary file left.
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 1);
        // The note itself is not touched.
        assert_eq!(
            read_note(&saved.path).unwrap().note,
            library.read(&saved.path).unwrap().file.note
        );
    }

    #[test]
    fn a_cancel_during_the_export_leaves_no_file() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let out = tempfile::tempdir().unwrap();
        let destination = out.path().join("essai.webp");
        let cancel = AtomicBool::new(false);
        let mut slow = options(VideoFormat::Webp);
        slow.hold_end_seconds = 10.0;
        let error = export_video_in(
            Some(&library),
            from_library(&saved.path),
            &slow,
            &destination,
            None,
            &mut |progress| {
                if progress.frames_done >= 1 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        )
        .unwrap_err();
        assert_eq!(error.code(), CANCELLED);
        assert_eq!(fs::read_dir(out.path()).unwrap().count(), 0);
    }

    #[test]
    fn the_estimate_follows_the_options_and_the_narration() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let gif = estimate_in(
            Some(&library),
            from_library(&saved.path),
            &options(VideoFormat::Gif),
        )
        .unwrap();
        assert_eq!((gif.width, gif.height), (1280, 720));
        assert_eq!(gif.speed, 1.0);
        assert_eq!(gif.narration_ms, None);
        assert!(gif.frames >= 1 && gif.duration_ms >= 500);
        assert!(gif.export_ms >= 500, "{gif:?}");

        // A narration of 3 s recorded during a replay at 2×: it imposes its options.
        let narration = Narration {
            clip: simulated_narration(Duration::from_secs(3), 1),
            base: NarrationBase::Replay,
            options: ReplayOptions {
                speed: 2.0,
                max_pause: None,
            },
            start_ms: 0,
        };
        let mut file = library.read(&saved.path).unwrap().file;
        file.audio = Some(encode_narration(&narration).unwrap());
        library.save(&saved.path, &file, &saved.stamp).unwrap();
        let webm = estimate_in(
            Some(&library),
            from_library(&saved.path),
            &options(VideoFormat::Webm),
        )
        .unwrap();
        assert_eq!(webm.speed, 2.0);
        let narration_ms = webm.narration_ms.unwrap();
        assert!((2990..=3010).contains(&narration_ms), "{narration_ms}");
        assert!(webm.duration_ms >= narration_ms);
        // The same note to GIF: no sound.
        let gif = estimate_in(
            Some(&library),
            from_library(&saved.path),
            &options(VideoFormat::Gif),
        )
        .unwrap();
        assert_eq!(gif.narration_ms, None);
        assert_eq!(gif.speed, 1.0);
    }
}
