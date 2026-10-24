//! The animated export pipeline: a note, its replay options, the format's options, an optional
//! narration and a final frame held N seconds give the frames of [`FrameRenderer`] pushed with
//! their sound into the format's [`VideoSink`] (`docs/design/media.md`, "Interface package").
//!
//! Contract stub: the bodies are filled in by the media pipeline work package.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ardoise_core::note::{Note, PageSize};
use ardoise_render::Backdrop;

use crate::narration::Narration;
use crate::replay::ReplayOptions;
use crate::{
    GifOptions, MediaError, Mp4Options, OpenH264Codec, VideoSummary, WebmOptions, WebpOptions,
};

/// The format to write, with its own options.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FormatOptions {
    Mp4(Mp4Options),
    Gif(GifOptions),
    WebP(WebpOptions),
    WebM(WebmOptions),
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

/// The plan of an export (no file is touched).
pub fn plan_export(
    note: &Note,
    export: &AnimationExport,
    narration: Option<&Narration>,
) -> Result<ExportPlan, MediaError> {
    let _ = (note, export, narration);
    Err(MediaError::InvalidSettings("not implemented yet".into()))
}

/// Writes the animation of `note` to `path` (replaced only once complete).
pub fn export_animation(
    note: &Note,
    export: &AnimationExport,
    narration: Option<&Narration>,
    path: &Path,
    openh264: Option<&OpenH264Codec>,
    progress: &mut dyn FnMut(ExportProgress),
    cancel: &AtomicBool,
) -> Result<VideoSummary, MediaError> {
    let _ = (note, export, narration, path, openh264, progress, cancel);
    Err(MediaError::InvalidSettings("not implemented yet".into()))
}

/// Frame size with the page's proportions, the longer side `long_side` pixels; both sides even
/// and at least 2.
pub fn page_frame_size(page: PageSize, long_side: u32) -> (u32, u32) {
    let _ = page;
    (long_side, long_side)
}
