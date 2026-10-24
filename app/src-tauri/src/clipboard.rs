//! Copy of a note's image to the Windows clipboard ("PNG" and `CF_DIB`), to paste it into
//! Word or OneNote (SPEC §4.6). The data comes from `ardoise_render::clipboard_image`.

// Stub of the M5 contract: replaced by the implementation.
#![allow(dead_code)]

use tauri::State;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::export::ExportSource;

/// Copies the note's image at `dpi` to the clipboard. `clipboardUnavailable` outside Windows.
#[tauri::command]
pub async fn copy_note_image(
    context: State<'_, AppContext>,
    source: ExportSource,
    dpi: u32,
) -> CommandResult<()> {
    let _ = (context, source, dpi);
    Err(CommandError::new(
        "clipboardUnavailable",
        "not implemented yet",
    ))
}
