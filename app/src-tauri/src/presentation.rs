//! The presentation window (SPEC §4.2, classroom): full screen on the chosen screen (the
//! projector), no interface, showing the live session; the screens and the choice of one.
//!
//! Contract: docs/design/tablet-live-classroom.md (work package B).

use serde_json::Value;
use tauri::AppHandle;

use crate::commands::{CommandError, CommandResult};

fn not_ready<T>() -> CommandResult<T> {
    Err(CommandError::new("noMonitor", "not implemented yet"))
}

#[tauri::command]
pub async fn list_monitors(app: AppHandle) -> CommandResult<Vec<Value>> {
    let _ = app;
    not_ready()
}

#[tauri::command]
pub async fn open_presentation(app: AppHandle, monitor: Option<String>) -> CommandResult<Value> {
    let _ = (app, monitor);
    not_ready()
}

#[tauri::command]
pub async fn close_presentation(app: AppHandle) -> CommandResult<()> {
    let _ = app;
    not_ready()
}
