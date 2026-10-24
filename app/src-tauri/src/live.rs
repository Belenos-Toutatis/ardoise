//! Live mode and the classroom (SPEC §4.2): the live session (pages that become notes, layers,
//! style, blackboard, laser pointer, view of the projected area) shared by every window, fed by
//! the tablet service and sent to the windows in batches through Tauri channels.
//!
//! Contract: docs/design/tablet-live-classroom.md (work package B). The command names and
//! arguments are fixed; the placeholder JSON values become the typed shapes of the contract.

use serde_json::Value;
use tauri::State;
use tauri::ipc::Channel;

use crate::commands::{CommandError, CommandResult};

/// The live session of the running app.
#[derive(Default)]
pub struct LiveRuntime {}

fn not_ready<T>() -> CommandResult<T> {
    Err(CommandError::new("liveNotRunning", "not implemented yet"))
}

#[tauri::command]
pub async fn live_subscribe(
    runtime: State<'_, LiveRuntime>,
    on_event: Channel<Value>,
) -> CommandResult<Value> {
    let _ = (runtime, on_event);
    not_ready()
}

#[tauri::command]
pub async fn live_start(runtime: State<'_, LiveRuntime>) -> CommandResult<Value> {
    let _ = runtime;
    not_ready()
}

#[tauri::command]
pub async fn live_stop(runtime: State<'_, LiveRuntime>) -> CommandResult<Vec<String>> {
    let _ = runtime;
    not_ready()
}

#[tauri::command]
pub async fn live_set_style(runtime: State<'_, LiveRuntime>, style: Value) -> CommandResult<()> {
    let _ = (runtime, style);
    not_ready()
}

#[tauri::command]
pub async fn live_command(runtime: State<'_, LiveRuntime>, command: Value) -> CommandResult<()> {
    let _ = (runtime, command);
    not_ready()
}

#[tauri::command]
pub async fn live_set_view(runtime: State<'_, LiveRuntime>, view: Value) -> CommandResult<()> {
    let _ = (runtime, view);
    not_ready()
}
