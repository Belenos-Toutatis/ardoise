//! The tablet in the interface (SPEC §4.1): the registered tablet (`_ardoise/device.json`,
//! shared by every computer of the library), and the commands of the tablet service
//! (`ardoise_device::service`): status, registration, sync, settings, the simulated tablet.
//!
//! Contract: docs/design/tablet-live-classroom.md (work package A).

use std::sync::{Mutex, PoisonError};

use ardoise_core::library::Library;
use ardoise_device::identity::{self, IdentityError};
use ardoise_device::service::{TabletOptions, TabletService, TabletStatus};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, State};

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::library::library_of;

/// `TabletInfo` in `note.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TabletInfo {
    pub name: String,
    pub address: String,
    /// RFC 3339.
    pub registered: String,
    pub width: u32,
    pub height: u32,
}

fn identity_error(error: IdentityError) -> CommandError {
    CommandError::new("libraryAccess", error)
}

pub fn tablet_info_in(library: &Library) -> CommandResult<Option<TabletInfo>> {
    let record = identity::load(library.root()).map_err(identity_error)?;
    Ok(record.map(|record| TabletInfo {
        name: record.name,
        address: record.address,
        registered: record.registered,
        width: record.width,
        height: record.height,
    }))
}

pub fn forget_tablet_in(library: &Library) -> CommandResult<()> {
    identity::forget(library.root()).map_err(identity_error)
}

/// What `tablet_subscribe` sends (`TabletEvent` in `tablet.ts`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum TabletEvent {
    Status { status: TabletStatus },
}

/// The tablet service of the running app (see [`start_service`]).
#[derive(Default)]
pub struct TabletRuntime {
    service: Mutex<Option<TabletService>>,
}

impl TabletRuntime {
    /// Runs `f` with the service; `tablet` error when it is not running.
    pub fn with_service<T>(&self, f: impl FnOnce(&TabletService) -> T) -> CommandResult<T> {
        let guard = self.service.lock().unwrap_or_else(PoisonError::into_inner);
        match guard.as_ref() {
            Some(service) => Ok(f(service)),
            None => Err(CommandError::new(
                "tablet",
                "the tablet service is not running",
            )),
        }
    }
}

/// Starts the tablet service for this PC's library (once, from `setup`). With `simulated`, the
/// simulated tablet in real time replaces Bluetooth.
pub fn start_service(app: &AppHandle, simulated: bool) {
    let _ = (app, simulated);
}

/// The library changed (`set_library`): the service follows it.
pub fn library_changed(app: &AppHandle) {
    let _ = app;
}

fn not_ready<T>() -> CommandResult<T> {
    Err(CommandError::new("tablet", "not implemented yet"))
}

#[tauri::command]
pub async fn tablet_status(runtime: State<'_, TabletRuntime>) -> CommandResult<TabletStatus> {
    runtime.with_service(TabletService::status)
}

#[tauri::command]
pub async fn tablet_subscribe(
    runtime: State<'_, TabletRuntime>,
    on_event: Channel<TabletEvent>,
) -> CommandResult<TabletStatus> {
    let _ = on_event;
    runtime.with_service(TabletService::status)
}

#[tauri::command]
pub async fn tablet_register(runtime: State<'_, TabletRuntime>) -> CommandResult<()> {
    runtime.with_service(TabletService::register)
}

#[tauri::command]
pub async fn tablet_cancel(runtime: State<'_, TabletRuntime>) -> CommandResult<()> {
    runtime.with_service(TabletService::cancel)
}

#[tauri::command]
pub async fn tablet_sync(runtime: State<'_, TabletRuntime>) -> CommandResult<()> {
    runtime.with_service(TabletService::sync_now)
}

#[tauri::command]
pub async fn tablet_rename(
    runtime: State<'_, TabletRuntime>,
    name: String,
) -> CommandResult<TabletStatus> {
    let _ = (runtime, name);
    not_ready()
}

#[tauri::command]
pub async fn tablet_set_time(runtime: State<'_, TabletRuntime>) -> CommandResult<i64> {
    let _ = runtime;
    not_ready()
}

#[tauri::command]
pub async fn tablet_set_options(
    runtime: State<'_, TabletRuntime>,
    options: TabletOptions,
) -> CommandResult<TabletStatus> {
    let _ = (runtime, options);
    not_ready()
}

/// Presses the simulated tablet's button (`--simulated-tablet` only, for the e2e journeys).
#[tauri::command]
pub async fn sim_press_button(runtime: State<'_, TabletRuntime>) -> CommandResult<()> {
    let _ = runtime;
    Err(CommandError::new("simulationOnly", "no simulated tablet"))
}

#[tauri::command]
pub async fn tablet_info(context: State<'_, AppContext>) -> CommandResult<Option<TabletInfo>> {
    tablet_info_in(&library_of(&context)?)
}

#[tauri::command]
pub async fn forget_tablet(context: State<'_, AppContext>) -> CommandResult<()> {
    forget_tablet_in(&library_of(&context)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::tests::library;
    use ardoise_device::identity::{DEVICE_FORMAT, DeviceRecord, HostId};

    #[test]
    fn shows_and_forgets_the_registered_tablet() {
        let (_dir, library) = library();
        assert_eq!(tablet_info_in(&library).unwrap(), None);
        // Forgetting when nothing is registered is fine.
        forget_tablet_in(&library).unwrap();

        let record = DeviceRecord {
            format: DEVICE_FORMAT,
            uuid: HostId([0x89, 0xea, 0x78, 0x88, 0xff, 0xcb]),
            address: "E2:43:03:67:0E:01".into(),
            name: "Bamboo Slate".into(),
            protocol: "slate".into(),
            width: 29700,
            height: 21600,
            registered: "2026-10-24T14:03:12+02:00".into(),
            extra: Default::default(),
        };
        identity::save(library.root(), &record).unwrap();
        let info = tablet_info_in(&library).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(&info).unwrap(),
            serde_json::json!({
                "name": "Bamboo Slate",
                "address": "E2:43:03:67:0E:01",
                "registered": "2026-10-24T14:03:12+02:00",
                "width": 29700,
                "height": 21600,
            })
        );

        forget_tablet_in(&library).unwrap();
        assert_eq!(tablet_info_in(&library).unwrap(), None);

        std::fs::write(identity::device_file(library.root()), "{broken").unwrap();
        assert_eq!(
            tablet_info_in(&library).unwrap_err().code(),
            "libraryAccess"
        );
    }
}
