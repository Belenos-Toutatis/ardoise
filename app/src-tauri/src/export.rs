//! Exports of notes to PNG, JPG, SVG and PDF (SPEC §4.6), for the editor and the library.
//! Contract: `docs/design/exports-and-operations.md`.

// Stub of the M5 contract: replaced by the implementation.
#![allow(dead_code)]

use ardoise_core::note::Note;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};

/// `ExportFormat` in `note.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExportFormat {
    Png,
    Jpg,
    Svg,
    Pdf,
}

/// Page size of a PDF export.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PdfPageSize {
    /// The note's own size.
    Actual,
    /// 210 × 297 mm, in the note's orientation, the note fitted and centred.
    A4,
}

/// `ExportOptions` in `note.ts`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportOptions {
    /// Resolution of raster formats (72..=600).
    pub dpi: u32,
    /// Draw the paper (colour, template, texture); without it PNG is transparent, JPG white.
    pub background: bool,
    /// JPG quality, 1..=100.
    pub quality: u8,
    pub pdf_page: PdfPageSize,
}

/// A note to export: the note at `path`, or the editor's current `note` (unsaved changes
/// included). `path` also names the output file.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportSource {
    pub path: String,
    #[serde(default)]
    pub note: Option<Note>,
}

/// Exports `sources` to `destination` (a file, or a folder for several notes in an image
/// format); returns the paths written.
#[tauri::command]
pub async fn export_notes(
    context: State<'_, AppContext>,
    sources: Vec<ExportSource>,
    format: ExportFormat,
    options: ExportOptions,
    destination: String,
) -> CommandResult<Vec<String>> {
    let _ = (context, sources, format, options, destination);
    Err(CommandError::new("exportFailed", "not implemented yet"))
}
