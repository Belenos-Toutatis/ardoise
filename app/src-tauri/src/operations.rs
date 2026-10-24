//! Split and merge commands (SPEC §4.4), over `ardoise_core::ops`.
//! Contract: `docs/design/exports-and-operations.md`.

// Stub of the M5 contract: replaced by the implementation.
#![allow(dead_code)]

use ardoise_core::library::FileStamp;
use ardoise_core::note::Note;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::library::NoteSummary;

/// `SplitSpec` in `note.ts`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SplitSpec {
    /// Everything written from `at` (ms since `created`) on goes to the new note.
    Time { at: u32 },
    /// Strokes with at least half of their samples inside the polygon (page coordinates).
    Zone { polygon: Vec<[f32; 2]> },
}

/// `MergeLayout` in `note.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MergeLayout {
    Overlay,
    EndToEnd,
}

/// `SplitResult` in `note.ts`: the kept part as saved over the note, and the new note.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SplitResult {
    pub note: Note,
    pub stamp: FileStamp,
    pub created: NoteSummary,
}

#[tauri::command]
pub async fn split_note(
    context: State<'_, AppContext>,
    path: String,
    note: Note,
    stamp: FileStamp,
    split: SplitSpec,
    title: String,
) -> CommandResult<SplitResult> {
    let _ = (context, path, note, stamp, split, title);
    Err(CommandError::new("nothingToSplit", "not implemented yet"))
}

#[tauri::command]
pub async fn merge_notes(
    context: State<'_, AppContext>,
    paths: Vec<String>,
    layout: MergeLayout,
    title: String,
) -> CommandResult<NoteSummary> {
    let _ = (context, paths, layout, title);
    Err(CommandError::new("nothingToMerge", "not implemented yet"))
}
