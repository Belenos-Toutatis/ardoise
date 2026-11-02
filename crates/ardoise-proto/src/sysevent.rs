//! Notifications of the "sysevent" characteristic `3a340721-c572-11e5-86c5-0002a5d5c51b`
//! (protocol.md §1, §12). They use the command framing (`opcode, length, arguments`) and come
//! unasked, at any time of a connection.
//!
//! - `f1 02 <percent> <flag>`: the battery level changed. Seen on a real Slate A4 (2026-11-02)
//!   once per percent lost, about 55 s apart, each one 1 below the last `ba` battery reading;
//!   issue #212 saw `f1 02 13 00` after a connection. `flag` was always `00` while discharging:
//!   probably the charging state, like the second byte of `ba` (**supposed**, not used).
//! - `ef 01 00`: sent once, right after a registration request (`e7`). Meaning unknown; ignored.
//!
//! The live-mode button of the simulator also comes here (`live::BUTTON_SYSEVENTS`).

use std::fmt;

use crate::hex;

/// Opcode of the battery-level notification.
pub const BATTERY_LEVEL: u8 = 0xf1;
/// Opcode of the notification seen right after a registration request, meaning unknown.
pub const AFTER_REGISTER: u8 = 0xef;

/// One decoded sysevent notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SysEvent {
    /// The tablet's battery level, in percent.
    BatteryLevel {
        percent: u8,
        /// Last byte, `00` in every capture (probably "charging", supposed).
        flag: u8,
    },
    /// A notification seen from a real Slate whose meaning is unknown: named, and ignored.
    Unexplained {
        /// What it was seen with.
        name: &'static str,
        data: Vec<u8>,
    },
    /// Anything else, kept verbatim.
    Unknown(Vec<u8>),
}

/// Decodes one sysevent notification. Never fails: what is not recognised exactly is
/// [`SysEvent::Unknown`].
pub fn decode(data: &[u8]) -> SysEvent {
    match data {
        [BATTERY_LEVEL, 0x02, percent, flag] if *percent <= 100 => SysEvent::BatteryLevel {
            percent: *percent,
            flag: *flag,
        },
        [AFTER_REGISTER, ..] => SysEvent::Unexplained {
            name: "seen right after REGISTER",
            data: data.to_vec(),
        },
        _ => SysEvent::Unknown(data.to_vec()),
    }
}

/// The battery-level notification for `percent` (used by the simulator; the last byte is the
/// charging state, as supposed above).
pub fn encode_battery_level(percent: u8, charging: bool) -> [u8; 4] {
    [BATTERY_LEVEL, 0x02, percent.min(100), u8::from(charging)]
}

impl fmt::Display for SysEvent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BatteryLevel { percent, flag } => {
                write!(f, "BATTERY_LEVEL {percent}% (flag {flag:02x})")
            }
            Self::Unexplained { name, data } => write!(
                f,
                "SYSEVENT {}, {name}, meaning unknown (ignored)",
                hex::encode(data)
            ),
            Self::Unknown(data) => write!(f, "unknown sysevent {}", hex::encode(data)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(text: &str) -> Vec<u8> {
        hex::decode(text).unwrap()
    }

    #[test]
    fn battery_level_notifications_of_a_real_slate() {
        // Our captures (Slate A4, 2026-11-02): 47 %, 46 %, 45 % during a session read at 48 %,
        // and 41 % right after a reading of 42 %.
        for (frame, percent) in [
            ("f1 02 2f 00", 47),
            ("f1 02 2e 00", 46),
            ("f1 02 2d 00", 45),
            ("f1 02 29 00", 41),
            // issue #212, after a connection.
            ("f1 02 13 00", 19),
        ] {
            assert_eq!(
                decode(&bytes(frame)),
                SysEvent::BatteryLevel { percent, flag: 0 },
                "{frame}"
            );
        }
        assert_eq!(
            decode(&bytes("f1 02 29 00")).to_string(),
            "BATTERY_LEVEL 41% (flag 00)"
        );
    }

    #[test]
    fn what_is_not_exactly_a_battery_level_stays_unknown() {
        for frame in ["f1 02 99 00", "f1 02 29", "f1 03 29 00 00", "f2 01 01", ""] {
            assert!(
                matches!(decode(&bytes(frame)), SysEvent::Unknown(_)),
                "{frame}"
            );
        }
        assert_eq!(
            decode(&bytes("f1 02 99 00")).to_string(),
            "unknown sysevent f1 02 99 00"
        );
    }

    #[test]
    fn the_frame_seen_after_a_registration_request_is_named_and_ignored() {
        let event = decode(&bytes("ef 01 00"));
        assert!(matches!(event, SysEvent::Unexplained { .. }));
        assert_eq!(
            event.to_string(),
            "SYSEVENT ef 01 00, seen right after REGISTER, meaning unknown (ignored)"
        );
    }

    #[test]
    fn the_simulator_encodes_what_the_decoder_reads() {
        assert_eq!(encode_battery_level(87, false), [0xf1, 0x02, 0x57, 0x00]);
        assert_eq!(
            decode(&encode_battery_level(20, true)),
            SysEvent::BatteryLevel {
                percent: 20,
                flag: 1
            }
        );
        assert_eq!(encode_battery_level(250, false)[2], 100);
    }
}
