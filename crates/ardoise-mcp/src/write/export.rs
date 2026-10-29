//! `export_note`: the app's exports, to files that never replace another.
//!
//! This module checks the arguments, picks where the files go and gives each its first free
//! name (" (2)" added; for a Markdown file, a name whose `<name>_images` folder is free too);
//! the app writes them ([`WriteServices::export`](super::WriteServices::export)) without ever
//! replacing a file. The folders made for an export that fails are removed again.

use std::fs;
use std::path::{Path, PathBuf};

use ardoise_hwr::Language;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::error::{ToolError, ToolResult, json_result};
use crate::host::Host;
use crate::notes::find_note;
use crate::read::TextLanguage;

/// Resolution of the raster formats, dots per inch.
const MIN_DPI: u32 = 72;
const MAX_DPI: u32 = 600;
const DEFAULT_DPI: u32 = 150;
const DEFAULT_QUALITY: u8 = 90;
/// Highest " (n)" tried for a free name.
const MAX_SUFFIX: u32 = 999;
/// What the app adds to a Markdown file's name for the folder of its drawings
/// (`textdoc::write_markdown`): `<name>_images`.
pub const MARKDOWN_IMAGES_SUFFIX: &str = "_images";

/// Limits of a replay, which the assistant cannot cancel once asked for (the app's own limits
/// for the rate, the speed and the last frame: `video::check_options`).
pub const REPLAY_FRAME_RATES: [u32; 5] = [10, 15, 24, 30, 60];
pub const MIN_REPLAY_SPEED: f64 = 0.25;
pub const MAX_REPLAY_SPEED: f64 = 16.0;
pub const MAX_HOLD_END_SECONDS: f64 = 60.0;
/// `duration_seconds` at most (10 minutes).
pub const MAX_REPLAY_SECONDS: f64 = 600.0;
/// The longest replay, last frame and narration included (what `duration_seconds` and the
/// longest pause give).
pub const MAX_REPLAY_TOTAL_SECONDS: f64 = MAX_REPLAY_SECONDS + MAX_HOLD_END_SECONDS;
/// The most frames: the longest replay at the highest rate.
pub const MAX_REPLAY_FRAMES: u64 = 39_600;
/// The longest the app's estimate of the encoding may be (a 4K replay encodes slowly).
pub const MAX_REPLAY_ENCODING_SECONDS: u64 = 20 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Png,
    Jpg,
    Webp,
    Svg,
    Pdf,
    /// The recognised text.
    Txt,
    /// The recognised text with its structure (titles, lists), the drawings as PNG files in a folder next to it.
    Md,
    /// The recognised text with its structure, the drawings embedded.
    Docx,
    /// Animated replay of the writing (one note).
    Gif,
    Webm,
    Mp4,
}

/// How a format is exported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportKind {
    /// One image per note.
    Image,
    /// One file for all the notes, one page each.
    Pdf,
    /// One file for all the notes, one after the other.
    Text,
    /// The replay of one note.
    Video,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            ExportFormat::Png => "png",
            ExportFormat::Jpg => "jpg",
            ExportFormat::Webp => "webp",
            ExportFormat::Svg => "svg",
            ExportFormat::Pdf => "pdf",
            ExportFormat::Txt => "txt",
            ExportFormat::Md => "md",
            ExportFormat::Docx => "docx",
            ExportFormat::Gif => "gif",
            ExportFormat::Webm => "webm",
            ExportFormat::Mp4 => "mp4",
        }
    }

    pub fn kind(self) -> ExportKind {
        match self {
            ExportFormat::Png | ExportFormat::Jpg | ExportFormat::Webp | ExportFormat::Svg => {
                ExportKind::Image
            }
            ExportFormat::Pdf => ExportKind::Pdf,
            ExportFormat::Txt | ExportFormat::Md | ExportFormat::Docx => ExportKind::Text,
            ExportFormat::Gif | ExportFormat::Webm | ExportFormat::Mp4 => ExportKind::Video,
        }
    }

    /// Whether `extension` (case ignored) is one this format's files may end with.
    fn accepts(self, extension: &str) -> bool {
        let extension = extension.to_lowercase();
        extension == self.extension() || (self == ExportFormat::Jpg && extension == "jpeg")
    }
}

/// Page size of a PDF export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum PdfPage {
    /// The note's own size (the default).
    Actual,
    /// 210 x 297 mm, in the note's orientation, the note fitted and centred.
    A4,
}

/// Frame size of a replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
pub enum VideoResolution {
    #[serde(rename = "720p")]
    Hd,
    #[serde(rename = "1080p")]
    FullHd,
    #[serde(rename = "4k")]
    Uhd,
    /// The page's proportions, longer side 1920 pixels.
    #[serde(rename = "page")]
    Page,
}

/// How long a replay lasts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VideoTiming {
    /// Times the speed of the writing.
    Speed(f64),
    /// Squeezed or stretched to this many seconds.
    Duration(f64),
}

/// The options of a replay (gif, webm, mp4), all resolved.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VideoOptions {
    pub resolution: VideoResolution,
    pub frame_rate: u32,
    pub timing: VideoTiming,
    pub trim_pauses: bool,
    pub hold_end_seconds: f64,
    pub narration: bool,
}

/// The options of an export, all resolved: the app validates the ones its format uses.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportOptions {
    pub dpi: u32,
    /// Draw the paper (colour, template, texture); without it PNG is transparent, JPG white.
    pub background: bool,
    /// JPG quality, 1 to 100.
    pub quality: u8,
    pub pdf_page: PdfPage,
    /// The language of the words the md and docx files add (date, tags, drawing).
    pub language: Language,
    pub video: VideoOptions,
}

/// Where the files go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportTarget {
    /// One file, whose name is free.
    File(PathBuf),
    /// A folder, for an image format with several notes: one file per note.
    Folder(PathBuf),
}

/// What the app is asked to export.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    /// The notes, absolute paths in the library, in the order asked.
    pub notes: Vec<PathBuf>,
    pub format: ExportFormat,
    pub target: ExportTarget,
    pub options: ExportOptions,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ExportNoteArgs {
    /// The notes to export, by `id` or path in the library. A replay (gif, webm, mp4) takes exactly one.
    pub notes: Vec<String>,
    /// png, jpg, webp, svg (one file per note), pdf (one file, one page per note), txt, md, docx (one file, the notes one after the other), gif, webm or mp4 (replay of one note).
    pub format: ExportFormat,
    /// Absolute path. A folder (it is created if missing): the files are named after the notes. Or a file name ending with the format's extension (not for several notes in an image format, which need a folder). Nothing is overwritten: " (2)" is added to a name that is taken.
    pub path: String,
    /// Resolution of png, jpg and webp, 72 to 600 dpi (default 150).
    #[schemars(range(min = 72, max = 600))]
    pub dpi: Option<u32>,
    /// Draw the paper and the background image (default `true`); without it PNG and WebP are transparent, JPG white, and a gif can be transparent.
    pub background: Option<bool>,
    /// jpg quality, 1 to 100 (default 90).
    #[schemars(range(min = 1, max = 100))]
    pub quality: Option<u8>,
    /// pdf page size: `actual` (default) or `a4`.
    pub pdf_page: Option<PdfPage>,
    /// md and docx: the language of the words the file adds (date, tags, drawing); default the app's.
    pub language: Option<TextLanguage>,
    /// Replay frame size: `720p` (default for gif), `1080p` (default), `4k`, `page`.
    pub resolution: Option<VideoResolution>,
    /// Replay frames per second: 10, 15, 24, 30 or 60 (default 15 for gif, 30 for webm and mp4).
    pub frame_rate: Option<u32>,
    /// Replay speed, times the writing speed, 0.25 to 16 (default 1). Not with `duration_seconds`. The whole replay may last 11 minutes at most: a long note needs a higher speed or `duration_seconds`.
    pub speed: Option<f64>,
    /// Replay length in seconds, the writing squeezed or stretched to fit, at most 600 (10 minutes). Not with `speed`.
    pub duration_seconds: Option<f64>,
    /// Replay: cut the long pauses (default `false`).
    pub trim_pauses: Option<bool>,
    /// Replay: seconds the last frame stays, 0 to 60 (default 3).
    pub hold_end_seconds: Option<f64>,
    /// Replay: play the note's recorded narration, webm and mp4 only (default `true`).
    pub narration: Option<bool>,
}

/// `path`, or `stem (2).ext`, `stem (3).ext`… next to it: the first name that is free.
/// `exportFailed` when the 999 first ones are all taken.
pub fn free_file(path: &Path) -> Result<PathBuf, ToolError> {
    free_file_with(path, None)
}

/// [`free_file`], where `companion` (a suffix of the stem, e.g. `_images`) names a file or folder
/// that goes with the file and must be free too.
fn free_file_with(path: &Path, companion: Option<&str>) -> Result<PathBuf, ToolError> {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new(""));
    (1..=MAX_SUFFIX)
        .map(|n| match n {
            1 => stem.clone(),
            n => format!("{stem} ({n})"),
        })
        .find(|stem| {
            !dir.join(format!("{stem}{extension}")).exists()
                && companion.is_none_or(|suffix| !dir.join(format!("{stem}{suffix}")).exists())
        })
        .map(|stem| dir.join(format!("{stem}{extension}")))
        .ok_or_else(|| {
            ToolError::new(
                "exportFailed",
                format!(
                    "no free name for {}: {MAX_SUFFIX} names are taken",
                    path.display()
                ),
            )
        })
}

/// The first free name for a file of `format` at `path`: for Markdown, its drawings' folder
/// must be free too (the app would otherwise put them among another export's).
fn free_name(path: &Path, format: ExportFormat) -> Result<PathBuf, ToolError> {
    let companion = (format == ExportFormat::Md).then_some(MARKDOWN_IMAGES_SUFFIX);
    free_file_with(path, companion)
}

/// Whether `path` names a folder rather than a file: it is one, or ends with a separator, or
/// has no extension.
fn names_a_folder(text: &str, path: &Path) -> bool {
    text.ends_with(['/', '\\']) || path.is_dir() || path.extension().is_none()
}

/// The folders made for an export, shallowest first: removed again if it fails.
#[derive(Debug, Default)]
pub(super) struct MadeFolders(Vec<PathBuf>);

impl MadeFolders {
    /// Creates `dir` and its missing parents, remembering which were missing.
    fn make(dir: &Path) -> Result<MadeFolders, ToolError> {
        let mut missing = Vec::new();
        let mut at = Some(dir);
        while let Some(folder) = at.filter(|folder| !folder.as_os_str().is_empty()) {
            if folder.exists() {
                break;
            }
            missing.push(folder.to_path_buf());
            at = folder.parent();
        }
        missing.reverse();
        let made = MadeFolders(missing);
        fs::create_dir_all(dir).map_err(|error| {
            made.undo();
            ToolError::new(
                "exportFailed",
                format!("cannot create {}: {error}", dir.display()),
            )
        })?;
        Ok(made)
    }

    /// Removes the folders made, deepest first, if they are still empty (best effort).
    pub(super) fn undo(&self) {
        for folder in self.0.iter().rev() {
            let _ = fs::remove_dir(folder);
        }
    }
}

/// Where the files of an export of `stems` (the notes' file names without extension) in
/// `format` to `text` go. Every check comes first; then the folder they go in is created (the
/// folders made are returned, to be removed again if the export fails). Never touches an
/// existing file.
pub(super) fn resolve_target(
    text: &str,
    format: ExportFormat,
    stems: &[String],
) -> Result<(ExportTarget, MadeFolders), ToolError> {
    let path = Path::new(text.trim());
    if !path.is_absolute() {
        return Err(ToolError::invalid(format!(
            "`path` must be an absolute path, not `{text}`"
        )));
    }
    let several_images = stems.len() > 1 && format.kind() == ExportKind::Image;
    if names_a_folder(text.trim(), path) {
        if several_images {
            let made = MadeFolders::make(path)?;
            return Ok((ExportTarget::Folder(path.to_path_buf()), made));
        }
        let stem = match stems {
            [only] => only.clone(),
            _ => "Ardoise export".to_owned(),
        };
        let file = free_name(&path.join(format!("{stem}.{}", format.extension())), format)?;
        let made = MadeFolders::make(path)?;
        return Ok((ExportTarget::File(file), made));
    }
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_default();
    if !format.accepts(&extension) {
        return Err(ToolError::invalid(format!(
            "a {} file name ends with .{}, not .{extension} (to export into a folder, give the folder's path)",
            format.extension(),
            format.extension(),
        )));
    }
    if several_images {
        return Err(ToolError::invalid(format!(
            "{} notes give one {} file each: `path` must be a folder",
            stems.len(),
            format.extension(),
        )));
    }
    let file = free_name(path, format)?;
    let made = match path.parent() {
        Some(parent) => MadeFolders::make(parent)?,
        None => MadeFolders::default(),
    };
    Ok((ExportTarget::File(file), made))
}

/// Checks the plan of a replay (the app's estimate: its frames, its length and how long its
/// encoding takes) against the limits of an export the assistant cannot cancel.
pub fn check_replay_plan(frames: u64, duration_ms: u64, encoding_ms: u64) -> Result<(), ToolError> {
    let minutes = |ms: u64| format!("{} min {:02} s", ms / 60_000, ms / 1000 % 60);
    if duration_ms as f64 > MAX_REPLAY_TOTAL_SECONDS * 1000.0 {
        return Err(ToolError::invalid(format!(
            "this replay would last {}: a replay asked by the assistant lasts at most {} min (10 min of writing and up to 60 s on the last frame). Give `duration_seconds` (at most {MAX_REPLAY_SECONDS}) or a higher `speed` (at most {MAX_REPLAY_SPEED}); a long narration makes it longer too (`narration: false`)",
            minutes(duration_ms),
            MAX_REPLAY_TOTAL_SECONDS / 60.0,
        )));
    }
    if frames > MAX_REPLAY_FRAMES {
        return Err(ToolError::invalid(format!(
            "this replay would have {frames} frames, at most {MAX_REPLAY_FRAMES}: lower `frame_rate` or make it shorter"
        )));
    }
    if encoding_ms > MAX_REPLAY_ENCODING_SECONDS * 1000 {
        return Err(ToolError::invalid(format!(
            "encoding this replay would take about {}, at most {} min for an export the assistant cannot stop: lower `resolution` or `frame_rate`, or make it shorter",
            minutes(encoding_ms),
            MAX_REPLAY_ENCODING_SECONDS / 60,
        )));
    }
    Ok(())
}

fn check_range(name: &str, value: f64, low: f64, high: f64) -> Result<(), ToolError> {
    if value.is_finite() && (low..=high).contains(&value) {
        Ok(())
    } else {
        Err(ToolError::invalid(format!(
            "`{name}` is {value}: it must be between {low} and {high}"
        )))
    }
}

fn options_of(args: &ExportNoteArgs, language: Language) -> Result<ExportOptions, ToolError> {
    let dpi = args.dpi.unwrap_or(DEFAULT_DPI);
    check_range(
        "dpi",
        f64::from(dpi),
        f64::from(MIN_DPI),
        f64::from(MAX_DPI),
    )?;
    let quality = args.quality.unwrap_or(DEFAULT_QUALITY);
    check_range("quality", f64::from(quality), 1.0, 100.0)?;
    let gif = args.format == ExportFormat::Gif;
    let timing = match (args.speed, args.duration_seconds) {
        (Some(_), Some(_)) => {
            return Err(ToolError::invalid(
                "give `speed` or `duration_seconds`, not both",
            ));
        }
        (Some(speed), None) => {
            check_range("speed", speed, MIN_REPLAY_SPEED, MAX_REPLAY_SPEED)?;
            VideoTiming::Speed(speed)
        }
        (None, Some(seconds)) => {
            if !(seconds.is_finite() && seconds > 0.0 && seconds <= MAX_REPLAY_SECONDS) {
                return Err(ToolError::invalid(format!(
                    "`duration_seconds` is {seconds}: it must be more than 0 and at most {MAX_REPLAY_SECONDS} (10 minutes)"
                )));
            }
            VideoTiming::Duration(seconds)
        }
        (None, None) => VideoTiming::Speed(1.0),
    };
    let frame_rate = args.frame_rate.unwrap_or(if gif { 15 } else { 30 });
    if !REPLAY_FRAME_RATES.contains(&frame_rate) {
        return Err(ToolError::invalid(format!(
            "`frame_rate` is {frame_rate}: it must be one of {REPLAY_FRAME_RATES:?}"
        )));
    }
    let hold_end_seconds = args.hold_end_seconds.unwrap_or(3.0);
    check_range(
        "hold_end_seconds",
        hold_end_seconds,
        0.0,
        MAX_HOLD_END_SECONDS,
    )?;
    Ok(ExportOptions {
        dpi,
        background: args.background.unwrap_or(true),
        quality,
        pdf_page: args.pdf_page.unwrap_or(PdfPage::Actual),
        language,
        video: VideoOptions {
            resolution: args.resolution.unwrap_or(if gif {
                VideoResolution::Hd
            } else {
                VideoResolution::FullHd
            }),
            frame_rate,
            timing,
            trim_pauses: args.trim_pauses.unwrap_or(false),
            hold_end_seconds,
            narration: args.narration.unwrap_or(true),
        },
    })
}

pub fn export_note(host: &Host, args: ExportNoteArgs) -> ToolResult {
    if args.notes.is_empty() {
        return Err(ToolError::invalid(
            "`notes` is empty: give at least one note",
        ));
    }
    if args.format.kind() == ExportKind::Video && args.notes.len() != 1 {
        return Err(ToolError::invalid(format!(
            "a {} replay takes exactly one note, not {}",
            args.format.extension(),
            args.notes.len()
        )));
    }
    let language = args
        .language
        .map_or_else(|| host.environment.language(), Language::from);
    let options = options_of(&args, language)?;
    let library = host.library()?;
    let entries = args
        .notes
        .iter()
        .map(|reference| find_note(&library, reference))
        .collect::<Result<Vec<_>, _>>()?;
    let stems: Vec<String> = entries
        .iter()
        .map(|entry| {
            entry
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
        .collect();
    let (target, made) = resolve_target(&args.path, args.format, &stems)?;
    let request = ExportRequest {
        notes: entries.iter().map(|entry| entry.path.clone()).collect(),
        format: args.format,
        target,
        options,
    };
    let outcome = host.write.export(&library, &request).inspect_err(|_| {
        // A failed export leaves no empty folder behind.
        made.undo();
    })?;
    json_result(json!({
        "format": args.format.extension(),
        "files": outcome.files.iter().map(|p| p.to_string_lossy()).collect::<Vec<_>>(),
        "notes": entries.len(),
        "details": outcome.details,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stems(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| (*s).to_owned()).collect()
    }

    fn text(dir: &Path, tail: &str) -> String {
        format!("{}{}{tail}", dir.display(), std::path::MAIN_SEPARATOR)
    }

    #[test]
    fn a_taken_name_gets_a_number_and_nothing_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("a.png");
        assert_eq!(free_file(&first).unwrap(), first);
        fs::write(&first, b"1").unwrap();
        let second = free_file(&first).unwrap();
        assert_eq!(second, dir.path().join("a (2).png"));
        fs::write(&second, b"2").unwrap();
        assert_eq!(free_file(&first).unwrap(), dir.path().join("a (3).png"));
        // Every name taken: an error, never a taken name.
        fs::write(dir.path().join("b.png"), b"").unwrap();
        for n in 2..=MAX_SUFFIX {
            fs::write(dir.path().join(format!("b ({n}).png")), b"").unwrap();
        }
        let error = free_file(&dir.path().join("b.png")).unwrap_err();
        assert_eq!(error.code, "exportFailed");
    }

    #[test]
    fn a_markdown_file_needs_its_drawings_folder_free_too() {
        let dir = tempfile::tempdir().unwrap();
        // A folder of drawings left by an export whose `.md` was moved or deleted.
        fs::create_dir(dir.path().join("cours_images")).unwrap();
        fs::write(dir.path().join("cours_images").join("cours-1.png"), b"x").unwrap();
        let (target, _) = resolve_target(
            &text(dir.path(), "cours.md"),
            ExportFormat::Md,
            &stems(&["a"]),
        )
        .unwrap();
        assert_eq!(target, ExportTarget::File(dir.path().join("cours (2).md")));
        // Into a folder, the same: the note's name, then the next one.
        fs::write(dir.path().join("Optique_images"), b"a file of the user").unwrap();
        let (target, _) = resolve_target(
            &text(dir.path(), ""),
            ExportFormat::Md,
            &stems(&["Optique"]),
        )
        .unwrap();
        assert_eq!(
            target,
            ExportTarget::File(dir.path().join("Optique (2).md"))
        );
        // Other formats have no companion.
        let (target, _) = resolve_target(
            &text(dir.path(), "cours.docx"),
            ExportFormat::Docx,
            &stems(&["a"]),
        )
        .unwrap();
        assert_eq!(target, ExportTarget::File(dir.path().join("cours.docx")));
    }

    #[test]
    fn folders_are_made_after_every_check_and_removed_again_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        // A refused request makes no folder.
        let wrong = text(dir.path(), "new/deeper/x.png");
        assert!(resolve_target(&wrong, ExportFormat::Jpg, &stems(&["a"])).is_err());
        let crowd = text(dir.path(), "new/deeper/x.png");
        assert!(resolve_target(&crowd, ExportFormat::Png, &stems(&["a", "b"])).is_err());
        assert!(!dir.path().join("new").exists());
        // An accepted one makes them, and gives them back to undo.
        let (target, made) = resolve_target(
            &text(dir.path(), "new/deeper/x.png"),
            ExportFormat::Png,
            &stems(&["a"]),
        )
        .unwrap();
        assert_eq!(
            target,
            ExportTarget::File(dir.path().join("new").join("deeper").join("x.png"))
        );
        assert!(dir.path().join("new").join("deeper").is_dir());
        made.undo();
        assert!(!dir.path().join("new").exists());
        assert!(dir.path().exists(), "a folder that was there stays");
        // A folder that holds something by then is left alone.
        let (_, made) = resolve_target(
            &text(dir.path(), "kept/"),
            ExportFormat::Png,
            &stems(&["a", "b"]),
        )
        .unwrap();
        fs::write(dir.path().join("kept").join("a.png"), b"x").unwrap();
        made.undo();
        assert!(dir.path().join("kept").join("a.png").exists());
    }

    fn args(format: ExportFormat) -> ExportNoteArgs {
        ExportNoteArgs {
            notes: vec!["a".into()],
            format,
            path: String::new(),
            dpi: None,
            background: None,
            quality: None,
            pdf_page: None,
            language: None,
            resolution: None,
            frame_rate: None,
            speed: None,
            duration_seconds: None,
            trim_pauses: None,
            hold_end_seconds: None,
            narration: None,
        }
    }

    #[test]
    fn replay_options_are_bounded() {
        let ok = |args: &ExportNoteArgs| options_of(args, Language::Fr).is_ok();
        let refused = |args: &ExportNoteArgs| {
            let error = options_of(args, Language::Fr).unwrap_err();
            assert_eq!(error.code, "invalidArguments");
            error.message
        };
        let mut a = args(ExportFormat::Webm);
        assert!(ok(&a));
        a.duration_seconds = Some(600.0);
        assert!(ok(&a));
        a.duration_seconds = Some(600.5);
        assert!(refused(&a).contains("600"));
        a.duration_seconds = Some(f64::INFINITY);
        refused(&a);
        a.duration_seconds = Some(0.0);
        refused(&a);
        a.duration_seconds = None;
        a.speed = Some(16.0);
        assert!(ok(&a));
        a.speed = Some(1e9);
        assert!(refused(&a).contains("16"));
        a.speed = Some(0.1);
        refused(&a);
        a.speed = None;
        a.frame_rate = Some(25);
        assert!(refused(&a).contains("[10, 15, 24, 30, 60]"));
        a.frame_rate = Some(60);
        a.hold_end_seconds = Some(61.0);
        assert!(refused(&a).contains("60"));
        a.hold_end_seconds = Some(-1.0);
        refused(&a);
        a.hold_end_seconds = Some(60.0);
        assert!(ok(&a));
    }

    #[test]
    fn a_replay_plan_beyond_the_limits_is_refused_with_them() {
        assert!(check_replay_plan(36_000, 600_000, 60_000).is_ok());
        let long = check_replay_plan(30_000, 3_600_000, 60_000).unwrap_err();
        assert_eq!(long.code, "invalidArguments");
        assert!(long.message.contains("60 min 00 s"), "{}", long.message);
        assert!(long.message.contains("`duration_seconds`"));
        let frames = check_replay_plan(MAX_REPLAY_FRAMES + 1, 600_000, 60_000).unwrap_err();
        assert!(frames.message.contains("39600"), "{}", frames.message);
        let slow = check_replay_plan(36_000, 600_000, 2 * 3_600_000).unwrap_err();
        assert!(slow.message.contains("20 min"), "{}", slow.message);
        assert_eq!(
            MAX_REPLAY_FRAMES,
            (MAX_REPLAY_TOTAL_SECONDS * 60.0) as u64,
            "the longest replay at 60 fps"
        );
    }

    #[test]
    fn a_folder_gets_files_named_after_the_notes() {
        let dir = tempfile::tempdir().unwrap();
        let folder = text(dir.path(), "out");
        let one = resolve_target(&folder, ExportFormat::Png, &stems(&["2026-10-24 Optique"]));
        assert_eq!(
            one.unwrap().0,
            ExportTarget::File(dir.path().join("out").join("2026-10-24 Optique.png"))
        );
        // Several images: a folder, one file each.
        let many = resolve_target(&folder, ExportFormat::Jpg, &stems(&["a", "b"])).unwrap();
        assert_eq!(many.0, ExportTarget::Folder(dir.path().join("out")));
        // One file for the notes together.
        let pdf = resolve_target(&folder, ExportFormat::Pdf, &stems(&["a", "b"]))
            .unwrap()
            .0;
        assert_eq!(
            pdf,
            ExportTarget::File(dir.path().join("out").join("Ardoise export.pdf"))
        );
    }

    #[test]
    fn a_file_name_must_fit_the_format_and_the_count() {
        let dir = tempfile::tempdir().unwrap();
        let file = text(dir.path(), "x.JPEG");
        assert!(resolve_target(&file, ExportFormat::Jpg, &stems(&["a"])).is_ok());
        let wrong = resolve_target(&file, ExportFormat::Png, &stems(&["a"])).unwrap_err();
        assert_eq!(wrong.code, "invalidArguments");
        let crowd = resolve_target(
            &text(dir.path(), "x.png"),
            ExportFormat::Png,
            &stems(&["a", "b"]),
        )
        .unwrap_err();
        assert_eq!(crowd.code, "invalidArguments");
        let relative =
            resolve_target("exports/x.png", ExportFormat::Png, &stems(&["a"])).unwrap_err();
        assert_eq!(relative.code, "invalidArguments");
    }

    #[test]
    fn an_existing_file_is_never_the_target() {
        let dir = tempfile::tempdir().unwrap();
        let taken = dir.path().join("x.pdf");
        fs::write(&taken, b"keep").unwrap();
        let (target, _) =
            resolve_target(&taken.to_string_lossy(), ExportFormat::Pdf, &stems(&["a"])).unwrap();
        assert_eq!(target, ExportTarget::File(dir.path().join("x (2).pdf")));
        assert_eq!(fs::read(&taken).unwrap(), b"keep");
    }
}
