//! The registered tablet, as the settings screen shows it (`_ardoise/device.json`, shared by
//! every computer of the library).

use ardoise_core::library::Library;
use ardoise_device::identity::{self, IdentityError};
use serde::Serialize;
use tauri::State;

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
