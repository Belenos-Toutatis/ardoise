//! GATT layouts of the tablets (docs/research/protocol.md §1, docs/research/xiaomi-blackboard.md
//! §2).
//!
//! Only the characteristics listed here are ever touched. The Nordic DFU service
//! (`00001530-…`, firmware update) is named only so that it can be excluded explicitly:
//! writing to it could brick the tablet. The Xiaomi service of the blackboard (`fe95`, its
//! login and registration) is named for the same reason: nothing in it is ever subscribed to or
//! written, so the tablet stays bound to the user's Mi Home.
//!
//! Both tablets take their commands on Nordic UART TX and answer on Nordic UART RX; the Slate
//! has three more notifying characteristics ([`SUBSCRIPTIONS`]), the blackboard one, `6e400004`
//! ([`BOARD_SUBSCRIPTIONS`]).

use ardoise_device::transport::{Channel, TabletKind};
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
/// The blackboard's live records and page bytes ([`Channel::BoardData`]): a fourth
/// characteristic of its Nordic UART service.
pub const BOARD_DATA: Uuid = Uuid::from_u128(0x6e400004_b5a3_f393_e0a9_e50e24dcca9e);
/// Xiaomi's service (16-bit `fe95`): its service data in advertisements is the MiBeacon that
/// identifies the blackboard; the service itself is never used and its characteristics are
/// ignored.
pub const XIAOMI_SERVICE: Uuid = Uuid::from_u128(0x0000_fe95_0000_1000_8000_0080_5f9b_34fb);

/// The Slate's notification channels in subscription order (Nordic RX, live, offline,
/// sysevent), with whether the tablet must offer them.
pub const SUBSCRIPTIONS: [(Channel, bool); 4] = [
    (Channel::Command, true),
    (Channel::Live, true),
    (Channel::PageData, true),
    (Channel::SysEvent, false),
];

/// The blackboard's notification channels in subscription order: replies, then records. The
/// order of the PC's sessions that worked (research note §3; Mi Home enables them the other way
/// round).
pub const BOARD_SUBSCRIPTIONS: [(Channel, bool); 2] =
    [(Channel::Command, true), (Channel::BoardData, true)];

/// The notification channels of a tablet of `kind`, in subscription order.
pub fn subscriptions(kind: TabletKind) -> &'static [(Channel, bool)] {
    match kind {
        TabletKind::Slate => &SUBSCRIPTIONS,
        TabletKind::Blackboard => &BOARD_SUBSCRIPTIONS,
    }
}

/// Services whose characteristics are never used, whatever their UUIDs.
pub fn is_forbidden_service(service: Uuid) -> bool {
    service == DFU_SERVICE || service == XIAOMI_SERVICE
}

/// The characteristic that notifies on `channel`.
pub fn uuid_for_channel(channel: Channel) -> Uuid {
    match channel {
        Channel::Command => COMMAND_RX,
        Channel::PageData => PAGE_DATA,
        Channel::Live => LIVE,
        Channel::SysEvent => SYS_EVENT,
        Channel::BoardData => BOARD_DATA,
    }
}

/// The channel a notifying characteristic belongs to; `None` for any other characteristic.
pub fn channel_for_uuid(uuid: Uuid) -> Option<Channel> {
    match uuid {
        COMMAND_RX => Some(Channel::Command),
        PAGE_DATA => Some(Channel::PageData),
        LIVE => Some(Channel::Live),
        SYS_EVENT => Some(Channel::SysEvent),
        BOARD_DATA => Some(Channel::BoardData),
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
        BOARD_DATA => "blackboard data",
        XIAOMI_SERVICE => "Xiaomi service, never used",
        _ => "unknown",
    };
    format!("{uuid} ({role})")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: [Channel; 5] = [
        Channel::Command,
        Channel::PageData,
        Channel::Live,
        Channel::SysEvent,
        Channel::BoardData,
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
        assert_eq!(
            BOARD_DATA.to_string(),
            "6e400004-b5a3-f393-e0a9-e50e24dcca9e"
        );
        // The 16-bit `fe95` on the Bluetooth base UUID.
        assert_eq!(XIAOMI_SERVICE.as_u128() >> 96, 0xfe95);
        assert_eq!(
            XIAOMI_SERVICE.as_u128() & ((1 << 96) - 1),
            0x0000_1000_8000_0080_5f9b_34fb
        );
    }

    #[test]
    fn the_blackboard_layout_has_its_own_channels() {
        assert_eq!(
            subscriptions(TabletKind::Blackboard),
            [(Channel::Command, true), (Channel::BoardData, true)]
        );
        assert_eq!(subscriptions(TabletKind::Slate), SUBSCRIPTIONS);
        assert!(is_forbidden_service(XIAOMI_SERVICE));
        assert!(is_forbidden_service(DFU_SERVICE));
        assert!(!is_forbidden_service(NORDIC_UART_SERVICE));
        assert!(describe(XIAOMI_SERVICE).ends_with("(Xiaomi service, never used)"));
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
