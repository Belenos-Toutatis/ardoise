//! `export_note`: the app's exports, to files that never replace another.
//!
//! This module checks the arguments, picks where the files go and gives each its first free
//! name (" (2)" added); the app writes them ([`WriteServices::export`](super::WriteServices::export)).

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
    /// Replay speed, times the writing speed (default 1). Not with `duration_seconds`.
    pub speed: Option<f64>,
    /// Replay length in seconds, the writing squeezed or stretched to fit. Not with `speed`.
    pub duration_seconds: Option<f64>,
    /// Replay: cut the long pauses (default `false`).
    pub trim_pauses: Option<bool>,
    /// Replay: seconds the last frame stays (default 3, at most 60).
    pub hold_end_seconds: Option<f64>,
    /// Replay: play the note's recorded narration, webm and mp4 only (default `true`).
    pub narration: Option<bool>,
}

/// `path`, or `stem (2).ext`, `stem (3).ext`… next to it: the first name that is free.
pub fn free_file(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new(""));
    (2..=MAX_SUFFIX)
        .map(|n| dir.join(format!("{stem} ({n}){extension}")))
        .find(|candidate| !candidate.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

/// Whether `path` names a folder rather than a file: it is one, or ends with a separator, or
/// has no extension.
fn names_a_folder(text: &str, path: &Path) -> bool {
    text.ends_with(['/', '\\']) || path.is_dir() || path.extension().is_none()
}

/// Where the files of an export of `stems` (the notes' file names without extension) in
/// `format` to `text` go. Creates the folder they go in, never touches an existing file.
pub(super) fn resolve_target(
    text: &str,
    format: ExportFormat,
    stems: &[String],
) -> Result<ExportTarget, ToolError> {
    let path = Path::new(text.trim());
    if !path.is_absolute() {
        return Err(ToolError::invalid(format!(
            "`path` must be an absolute path, not `{text}`"
        )));
    }
    let several_images = stems.len() > 1 && format.kind() == ExportKind::Image;
    let create = |dir: &Path| {
        fs::create_dir_all(dir).map_err(|error| {
            ToolError::new(
                "exportFailed",
                format!("cannot create {}: {error}", dir.display()),
            )
        })
    };
    if names_a_folder(text.trim(), path) {
        create(path)?;
        if several_images {
            return Ok(ExportTarget::Folder(path.to_path_buf()));
        }
        let stem = match stems {
            [only] => only.clone(),
            _ => "Ardoise export".to_owned(),
        };
        return Ok(ExportTarget::File(free_file(
            &path.join(format!("{stem}.{}", format.extension())),
        )));
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
    if let Some(parent) = path.parent() {
        create(parent)?;
    }
    Ok(ExportTarget::File(free_file(path)))
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
        (Some(speed), None) => VideoTiming::Speed(speed),
        (None, Some(seconds)) => VideoTiming::Duration(seconds),
        (None, None) => VideoTiming::Speed(1.0),
    };
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
            frame_rate: args.frame_rate.unwrap_or(if gif { 15 } else { 30 }),
            timing,
            trim_pauses: args.trim_pauses.unwrap_or(false),
            hold_end_seconds: args.hold_end_seconds.unwrap_or(3.0),
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
    let target = resolve_target(&args.path, args.format, &stems)?;
    let request = ExportRequest {
        notes: entries.iter().map(|entry| entry.path.clone()).collect(),
        format: args.format,
        target,
        options,
    };
    let outcome = host.write.export(&library, &request)?;
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
        assert_eq!(free_file(&first), first);
        fs::write(&first, b"1").unwrap();
        let second = free_file(&first);
        assert_eq!(second, dir.path().join("a (2).png"));
        fs::write(&second, b"2").unwrap();
        assert_eq!(free_file(&first), dir.path().join("a (3).png"));
    }

    #[test]
    fn a_folder_gets_files_named_after_the_notes() {
        let dir = tempfile::tempdir().unwrap();
        let folder = text(dir.path(), "out");
        let one = resolve_target(&folder, ExportFormat::Png, &stems(&["2026-10-24 Optique"]));
        assert_eq!(
            one.unwrap(),
            ExportTarget::File(dir.path().join("out").join("2026-10-24 Optique.png"))
        );
        // Several images: a folder, one file each.
        let many = resolve_target(&folder, ExportFormat::Jpg, &stems(&["a", "b"])).unwrap();
        assert_eq!(many, ExportTarget::Folder(dir.path().join("out")));
        // One file for the notes together.
        let pdf = resolve_target(&folder, ExportFormat::Pdf, &stems(&["a", "b"])).unwrap();
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
        let target =
            resolve_target(&taken.to_string_lossy(), ExportFormat::Pdf, &stems(&["a"])).unwrap();
        assert_eq!(target, ExportTarget::File(dir.path().join("x (2).pdf")));
        assert_eq!(fs::read(&taken).unwrap(), b"keep");
    }
}
