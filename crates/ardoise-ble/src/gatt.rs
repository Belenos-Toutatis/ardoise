//! GATT layout of the tablet (docs/research/protocol.md §1).
//!
//! Only the characteristics listed here are ever touched. The Nordic DFU service
//! (`00001530-…`, firmware update) is named only so that it can be excluded explicitly:
//! writing to it could brick the tablet.

use ardoise_device::transport::Channel;
use uuid::Uuid;

/// Nordic UART service.
pub const NORDIC_UART_SERVICE: Uuid = Uuid::from_u128(0x6e400001_b5a3_f393_e0a9_e50e24dcca9e);
/// Nordic UART TX: the host writes every command here. The only characteristic ever written.
pub const COMMAND_TX: Uuid = Uuid::from_u128(0x6e400002_b5a3_f393_e0a9_e50e24dcca9e);
/// Nordic UART RX: replies to commands ([`Channel::Command`]).
pub const COMMAND_RX: Uuid = Uuid::from_u128(0x6e400003_b5a3_f393_e0a9_e50e24dcca9e);
/// Live pen samples ([`Channel::Live`]).
pub const LIVE: Uuid = Uuid::from_u128(0x00001524_1212_efde_1523_785feabcd123);
/// Raw bytes of a stored page during a download ([`Channel::PageData`]).
pub const PAGE_DATA: Uuid = Uuid::from_u128(0xffee0003_bbaa_9988_7766_554433221100);
/// "Sysevent" notifications ([`Channel::SysEvent`]); optional.
pub const SYS_EVENT: Uuid = Uuid::from_u128(0x3a340721_c572_11e5_86c5_0002a5d5c51b);
/// Nordic DFU (firmware update) service. Never used; its characteristics are ignored.
pub const DFU_SERVICE: Uuid = Uuid::from_u128(0x00001530_1212_efde_1523_785feabcd123);

/// Notification channels in subscription order (Nordic RX, live, offline, sysevent),
/// with whether the tablet must offer them.
pub const SUBSCRIPTIONS: [(Channel, bool); 4] = [
    (Channel::Command, true),
    (Channel::Live, true),
    (Channel::PageData, true),
    (Channel::SysEvent, false),
];

/// The characteristic that notifies on `channel`.
pub fn uuid_for_channel(channel: Channel) -> Uuid {
    match channel {
        Channel::Command => COMMAND_RX,
        Channel::PageData => PAGE_DATA,
        Channel::Live => LIVE,
        Channel::SysEvent => SYS_EVENT,
    }
}

/// The channel a notifying characteristic belongs to; `None` for any other characteristic.
pub fn channel_for_uuid(uuid: Uuid) -> Option<Channel> {
    match uuid {
        COMMAND_RX => Some(Channel::Command),
        PAGE_DATA => Some(Channel::PageData),
        LIVE => Some(Channel::Live),
        SYS_EVENT => Some(Channel::SysEvent),
        _ => None,
    }
}

/// Human-readable name of a characteristic, for error messages.
pub fn describe(uuid: Uuid) -> String {
    let role = match uuid {
        COMMAND_TX => "command",
        COMMAND_RX => "replies",
        PAGE_DATA => "page data",
        LIVE => "live",
        SYS_EVENT => "sysevent",
        _ => "unknown",
    };
    format!("{uuid} ({role})")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Channel; 4] = [
        Channel::Command,
        Channel::PageData,
        Channel::Live,
        Channel::SysEvent,
    ];

    #[test]
    fn uuids_match_the_protocol_reference() {
        assert_eq!(
            COMMAND_TX.to_string(),
            "6e400002-b5a3-f393-e0a9-e50e24dcca9e"
        );
        assert_eq!(
            COMMAND_RX.to_string(),
            "6e400003-b5a3-f393-e0a9-e50e24dcca9e"
        );
        assert_eq!(LIVE.to_string(), "00001524-1212-efde-1523-785feabcd123");
        assert_eq!(
            PAGE_DATA.to_string(),
            "ffee0003-bbaa-9988-7766-554433221100"
        );
        assert_eq!(
            SYS_EVENT.to_string(),
            "3a340721-c572-11e5-86c5-0002a5d5c51b"
        );
        assert_eq!(
            DFU_SERVICE.to_string(),
            "00001530-1212-efde-1523-785feabcd123"
        );
        assert_eq!(
            NORDIC_UART_SERVICE.to_string(),
            "6e400001-b5a3-f393-e0a9-e50e24dcca9e"
        );
    }

    #[test]
    fn channel_and_uuid_round_trip() {
        for channel in ALL {
            assert_eq!(channel_for_uuid(uuid_for_channel(channel)), Some(channel));
        }
    }

    #[test]
    fn other_characteristics_have_no_channel() {
        assert_eq!(channel_for_uuid(COMMAND_TX), None);
        assert_eq!(channel_for_uuid(DFU_SERVICE), None);
        assert_eq!(
            channel_for_uuid(Uuid::from_u128(0x00001531_1212_efde_1523_785feabcd123)),
            None
        );
        assert_eq!(channel_for_uuid(Uuid::nil()), None);
    }

    #[test]
    fn subscription_order_follows_the_protocol() {
        let order: Vec<Channel> = SUBSCRIPTIONS.iter().map(|(c, _)| *c).collect();
        assert_eq!(
            order,
            [
                Channel::Command,
                Channel::Live,
                Channel::PageData,
                Channel::SysEvent
            ]
        );
        assert!(
            SUBSCRIPTIONS
                .iter()
                .all(|(c, required)| *required != (*c == Channel::SysEvent))
        );
    }

    #[test]
    fn describe_names_the_role() {
        assert_eq!(
            describe(COMMAND_TX),
            "6e400002-b5a3-f393-e0a9-e50e24dcca9e (command)"
        );
        assert!(describe(Uuid::nil()).ends_with("(unknown)"));
    }
}
