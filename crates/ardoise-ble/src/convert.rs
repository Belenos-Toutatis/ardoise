//! Pure conversions between btleplug types and the transport contract.

use std::collections::{BTreeSet, HashMap};

use ardoise_device::transport::{Advertisement, Channel, TransportError, WACOM_COMPANY_IDS};
use btleplug::api::{CharPropFlags, Characteristic, PeripheralProperties, WriteType};

use crate::AdapterStatus;
use crate::gatt;

/// Converts what btleplug knows about an advertising peripheral.
///
/// The name is the GAP local name, else the advertised name. Manufacturer data: a Wacom
/// company identifier wins if several are present, otherwise the lowest identifier (in
/// practice a peripheral sends a single entry).
pub(crate) fn advertisement_from_properties(properties: &PeripheralProperties) -> Advertisement {
    let name = [&properties.local_name, &properties.advertisement_name]
        .into_iter()
        .flatten()
        .map(|name| name.trim_end_matches('\0'))
        .find(|name| !name.is_empty())
        .map(str::to_owned);
    let (company_id, manufacturer_data) =
        match pick_manufacturer_data(&properties.manufacturer_data) {
            Some((id, data)) => (Some(id), data.clone()),
            None => (None, Vec::new()),
        };
    Advertisement {
        // `BDAddr` displays as `E2:43:03:67:0E:01`.
        address: properties.address.to_string(),
        name,
        rssi: properties.rssi,
        company_id,
        manufacturer_data,
    }
}

fn pick_manufacturer_data(data: &HashMap<u16, Vec<u8>>) -> Option<(u16, &Vec<u8>)> {
    WACOM_COMPANY_IDS
        .iter()
        .find_map(|id| data.get_key_value(id))
        .or_else(|| data.iter().min_by_key(|(id, _)| **id))
        .map(|(id, bytes)| (*id, bytes))
}

/// Write with response when the characteristic supports it (what BlueZ, hence Tuhi, does),
/// else without response. `None` when it cannot be written at all.
pub(crate) fn choose_write_type(properties: CharPropFlags) -> Option<WriteType> {
    if properties.contains(CharPropFlags::WRITE) {
        Some(WriteType::WithResponse)
    } else if properties.contains(CharPropFlags::WRITE_WITHOUT_RESPONSE) {
        Some(WriteType::WithoutResponse)
    } else {
        None
    }
}

fn can_notify(properties: CharPropFlags) -> bool {
    properties.intersects(CharPropFlags::NOTIFY | CharPropFlags::INDICATE)
}

/// The characteristics a link uses, found by UUID among the discovered ones.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GattLayout {
    /// Nordic UART TX, the only characteristic ever written.
    pub command: Characteristic,
    pub write_type: WriteType,
    /// Notifying characteristics in subscription order (protocol.md §1).
    pub subscriptions: Vec<(Channel, Characteristic)>,
}

/// Locates the tablet's characteristics. Anything in the DFU service is ignored.
pub(crate) fn locate_characteristics(
    characteristics: &BTreeSet<Characteristic>,
) -> Result<GattLayout, TransportError> {
    let find = |uuid| {
        characteristics
            .iter()
            .find(|c| c.uuid == uuid && c.service_uuid != gatt::DFU_SERVICE)
    };
    let command = find(gatt::COMMAND_TX)
        .ok_or_else(|| TransportError::MissingCharacteristic(gatt::describe(gatt::COMMAND_TX)))?;
    let write_type = choose_write_type(command.properties).ok_or_else(|| {
        TransportError::MissingCharacteristic(format!(
            "{} is not writable",
            gatt::describe(gatt::COMMAND_TX)
        ))
    })?;
    let mut subscriptions = Vec::new();
    for (channel, required) in gatt::SUBSCRIPTIONS {
        let uuid = gatt::uuid_for_channel(channel);
        match find(uuid) {
            Some(c) if can_notify(c.properties) => subscriptions.push((channel, c.clone())),
            Some(_) if required => {
                return Err(TransportError::MissingCharacteristic(format!(
                    "{} cannot notify",
                    gatt::describe(uuid)
                )));
            }
            None if required => {
                return Err(TransportError::MissingCharacteristic(gatt::describe(uuid)));
            }
            _ => {}
        }
    }
    Ok(GattLayout {
        command: command.clone(),
        write_type,
        subscriptions,
    })
}

/// Maps the state of the default adapter to the errors of the contract.
pub(crate) fn check_adapter_status(status: AdapterStatus) -> Result<(), TransportError> {
    match status {
        AdapterStatus::On => Ok(()),
        AdapterStatus::Off => Err(TransportError::AdapterOff),
        AdapterStatus::NotFound | AdapterStatus::NoLowEnergy => Err(TransportError::NoAdapter),
    }
}

/// Maps a btleplug error. "Device not found" is mapped by the caller, which knows the address.
pub(crate) fn map_error(error: btleplug::Error) -> TransportError {
    match error {
        btleplug::Error::NoAdapterAvailable => TransportError::NoAdapter,
        btleplug::Error::TimedOut(_) => TransportError::Timeout,
        btleplug::Error::NotConnected => TransportError::NotConnected,
        other => TransportError::Backend(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use btleplug::api::BDAddr;
    use std::time::Duration;
    use uuid::Uuid;

    fn properties(manufacturer_data: &[(u16, &[u8])]) -> PeripheralProperties {
        PeripheralProperties {
            address: BDAddr::from([0xE2, 0x43, 0x03, 0x67, 0x0E, 0x01]),
            local_name: Some("Bamboo Slate".into()),
            rssi: Some(-61),
            manufacturer_data: manufacturer_data
                .iter()
                .map(|(id, data)| (*id, data.to_vec()))
                .collect(),
            ..PeripheralProperties::default()
        }
    }

    #[test]
    fn converts_a_wacom_advertisement() {
        let ad = advertisement_from_properties(&properties(&[(0x4755, &[1, 2, 3, 4, 5, 6, 7])]));
        assert_eq!(ad.address, "E2:43:03:67:0E:01");
        assert_eq!(ad.name.as_deref(), Some("Bamboo Slate"));
        assert_eq!(ad.rssi, Some(-61));
        assert_eq!(ad.company_id, Some(0x4755));
        assert_eq!(ad.manufacturer_data, [1, 2, 3, 4, 5, 6, 7]);
        assert!(ad.is_wacom());
        assert!(!ad.in_register_mode());
    }

    #[test]
    fn register_mode_has_four_bytes() {
        let ad = advertisement_from_properties(&properties(&[(0x424d, &[9, 8, 7, 6])]));
        assert!(ad.in_register_mode());
    }

    #[test]
    fn device_without_manufacturer_data_or_name() {
        let props = PeripheralProperties {
            address: BDAddr::from([0, 0x1a, 0x7d, 0xda, 0x71, 0x13]),
            ..PeripheralProperties::default()
        };
        let ad = advertisement_from_properties(&props);
        assert_eq!(ad.address, "00:1A:7D:DA:71:13");
        assert_eq!(ad.name, None);
        assert_eq!(ad.rssi, None);
        assert_eq!(ad.company_id, None);
        assert!(ad.manufacturer_data.is_empty());
        assert!(!ad.is_wacom());
    }

    #[test]
    fn name_falls_back_to_the_advertised_name() {
        let mut props = properties(&[]);
        props.local_name = Some(String::new());
        props.advertisement_name = Some("Bamboo Slate Small".into());
        assert_eq!(
            advertisement_from_properties(&props).name.as_deref(),
            Some("Bamboo Slate Small")
        );
        props.advertisement_name = Some("\0".into());
        assert_eq!(advertisement_from_properties(&props).name, None);
    }

    #[test]
    fn wacom_entry_wins_over_others() {
        let ad = advertisement_from_properties(&properties(&[
            (0x0006, &[0xaa]),
            (0x4157, &[1, 2, 3, 4]),
            (0x004c, &[0xbb]),
        ]));
        assert_eq!(ad.company_id, Some(0x4157));
        assert_eq!(ad.manufacturer_data, [1, 2, 3, 4]);
    }

    #[test]
    fn lowest_company_id_otherwise() {
        let ad =
            advertisement_from_properties(&properties(&[(0x004c, &[0xbb]), (0x0006, &[0xaa])]));
        assert_eq!(ad.company_id, Some(0x0006));
        assert_eq!(ad.manufacturer_data, [0xaa]);
    }

    #[test]
    fn write_type_prefers_with_response() {
        use CharPropFlags as F;
        assert_eq!(
            choose_write_type(F::WRITE | F::WRITE_WITHOUT_RESPONSE),
            Some(WriteType::WithResponse)
        );
        assert_eq!(choose_write_type(F::WRITE), Some(WriteType::WithResponse));
        assert_eq!(
            choose_write_type(F::WRITE_WITHOUT_RESPONSE | F::NOTIFY),
            Some(WriteType::WithoutResponse)
        );
        assert_eq!(choose_write_type(F::READ | F::NOTIFY), None);
    }

    fn chr(service: Uuid, uuid: Uuid, properties: CharPropFlags) -> Characteristic {
        Characteristic {
            uuid,
            service_uuid: service,
            properties,
            descriptors: BTreeSet::new(),
        }
    }

    const LIVE_SERVICE: Uuid = Uuid::from_u128(0x00001523_1212_efde_1523_785feabcd123);
    const OFFLINE_SERVICE: Uuid = Uuid::from_u128(0xffee0001_bbaa_9988_7766_554433221100);
    const SYS_SERVICE: Uuid = Uuid::from_u128(0x3a340720_c572_11e5_86c5_0002a5d5c51b);
    const DFU_CONTROL: Uuid = Uuid::from_u128(0x00001531_1212_efde_1523_785feabcd123);

    fn slate(with_sysevent: bool) -> BTreeSet<Characteristic> {
        use CharPropFlags as F;
        let mut set: BTreeSet<_> = [
            chr(gatt::DFU_SERVICE, DFU_CONTROL, F::WRITE | F::NOTIFY),
            chr(
                gatt::NORDIC_UART_SERVICE,
                gatt::COMMAND_TX,
                F::WRITE | F::WRITE_WITHOUT_RESPONSE,
            ),
            chr(gatt::NORDIC_UART_SERVICE, gatt::COMMAND_RX, F::NOTIFY),
            chr(LIVE_SERVICE, gatt::LIVE, F::NOTIFY),
            chr(OFFLINE_SERVICE, gatt::PAGE_DATA, F::NOTIFY),
            chr(
                OFFLINE_SERVICE,
                Uuid::from_u128(0xffee0002_bbaa_9988_7766_554433221100),
                F::WRITE,
            ),
        ]
        .into_iter()
        .collect();
        if with_sysevent {
            set.insert(chr(SYS_SERVICE, gatt::SYS_EVENT, F::NOTIFY));
        }
        set
    }

    #[test]
    fn locates_the_slate_characteristics_in_subscription_order() {
        let layout = locate_characteristics(&slate(true)).unwrap();
        assert_eq!(layout.command.uuid, gatt::COMMAND_TX);
        assert_eq!(layout.write_type, WriteType::WithResponse);
        let order: Vec<(Channel, Uuid)> = layout
            .subscriptions
            .iter()
            .map(|(channel, c)| (*channel, c.uuid))
            .collect();
        assert_eq!(
            order,
            [
                (Channel::Command, gatt::COMMAND_RX),
                (Channel::Live, gatt::LIVE),
                (Channel::PageData, gatt::PAGE_DATA),
                (Channel::SysEvent, gatt::SYS_EVENT),
            ]
        );
        assert!(
            layout
                .subscriptions
                .iter()
                .all(|(_, c)| c.service_uuid != gatt::DFU_SERVICE)
        );
    }

    #[test]
    fn sysevent_is_optional() {
        let layout = locate_characteristics(&slate(false)).unwrap();
        assert_eq!(layout.subscriptions.len(), 3);
        assert!(
            layout
                .subscriptions
                .iter()
                .all(|(channel, _)| *channel != Channel::SysEvent)
        );
    }

    #[test]
    fn missing_required_characteristics_are_reported() {
        for missing in [
            gatt::COMMAND_TX,
            gatt::COMMAND_RX,
            gatt::LIVE,
            gatt::PAGE_DATA,
        ] {
            let set: BTreeSet<_> = slate(true)
                .into_iter()
                .filter(|c| c.uuid != missing)
                .collect();
            match locate_characteristics(&set) {
                Err(TransportError::MissingCharacteristic(text)) => {
                    assert!(text.contains(&missing.to_string()), "{text}");
                }
                other => panic!("expected a missing characteristic, got {other:?}"),
            }
        }
    }

    #[test]
    fn unusable_characteristics_are_reported() {
        use CharPropFlags as F;
        let mut set = slate(true);
        set.retain(|c| c.uuid != gatt::COMMAND_TX);
        set.insert(chr(gatt::NORDIC_UART_SERVICE, gatt::COMMAND_TX, F::READ));
        assert!(matches!(
            locate_characteristics(&set),
            Err(TransportError::MissingCharacteristic(text)) if text.contains("not writable")
        ));

        let mut set = slate(true);
        set.retain(|c| c.uuid != gatt::LIVE);
        set.insert(chr(LIVE_SERVICE, gatt::LIVE, F::READ));
        assert!(matches!(
            locate_characteristics(&set),
            Err(TransportError::MissingCharacteristic(text)) if text.contains("cannot notify")
        ));
    }

    #[test]
    fn characteristics_inside_the_dfu_service_are_never_used() {
        use CharPropFlags as F;
        let set: BTreeSet<_> = slate(true)
            .into_iter()
            .filter(|c| c.uuid != gatt::COMMAND_TX)
            .chain([chr(gatt::DFU_SERVICE, gatt::COMMAND_TX, F::WRITE)])
            .collect();
        assert!(matches!(
            locate_characteristics(&set),
            Err(TransportError::MissingCharacteristic(_))
        ));
    }

    #[test]
    fn indicate_counts_as_notify_and_write_without_response_is_used() {
        use CharPropFlags as F;
        let mut set = slate(false);
        set.retain(|c| c.uuid != gatt::COMMAND_TX && c.uuid != gatt::COMMAND_RX);
        set.insert(chr(
            gatt::NORDIC_UART_SERVICE,
            gatt::COMMAND_TX,
            F::WRITE_WITHOUT_RESPONSE,
        ));
        set.insert(chr(
            gatt::NORDIC_UART_SERVICE,
            gatt::COMMAND_RX,
            F::INDICATE,
        ));
        let layout = locate_characteristics(&set).unwrap();
        assert_eq!(layout.write_type, WriteType::WithoutResponse);
        assert_eq!(layout.subscriptions[0].1.uuid, gatt::COMMAND_RX);
    }

    #[test]
    fn adapter_status_maps_to_contract_errors() {
        assert_eq!(check_adapter_status(AdapterStatus::On), Ok(()));
        assert_eq!(
            check_adapter_status(AdapterStatus::Off),
            Err(TransportError::AdapterOff)
        );
        assert_eq!(
            check_adapter_status(AdapterStatus::NotFound),
            Err(TransportError::NoAdapter)
        );
        assert_eq!(
            check_adapter_status(AdapterStatus::NoLowEnergy),
            Err(TransportError::NoAdapter)
        );
    }

    #[test]
    fn btleplug_errors_map_to_contract_errors() {
        assert_eq!(
            map_error(btleplug::Error::NoAdapterAvailable),
            TransportError::NoAdapter
        );
        assert_eq!(
            map_error(btleplug::Error::TimedOut(Duration::from_secs(1))),
            TransportError::Timeout
        );
        assert_eq!(
            map_error(btleplug::Error::NotConnected),
            TransportError::NotConnected
        );
        assert!(matches!(
            map_error(btleplug::Error::NotSupported("x".into())),
            TransportError::Backend(text) if text.contains("not supported")
        ));
    }
}
