//! Live-mode notifications on the characteristic `00001524-1212-efde-1523-785feabcd123`
//! (protocol.md §7.2).
//!
//! - `a1 LL <records>`: pen samples, 6-byte records `x u16, y u16, pressure u16`; a record of
//!   six `ff` means the pen left proximity. One record every 5 ms.
//! - `a2 LL ?? ?? <ticks...>`: the pen entered proximity; the bytes from offset 4 are a
//!   little-endian count of 5 ms ticks (since power-up, probably). Bytes 2..4 are unknown.
//! - `10 ..`: status report with the pressure at bytes 2..4 and the buttons at byte 10.
//! - `a3 06 dd dd dd dd 01 00`: seen once from a real Slate A4, right after an `a2` (protocol.md
//!   §12). Meaning unknown (the `dd dd` bytes recall the lost-point packet of stored pages);
//!   decoded as [`LivePacket::Unexplained`] and ignored.

use std::fmt;

use crate::hex;
use crate::stroke::Point;

/// First byte of a pen-samples notification.
pub const SAMPLES: u8 = 0xa1;
/// First byte of a proximity-in notification.
pub const PROXIMITY_IN: u8 = 0xa2;
/// First byte of a status notification.
pub const STATUS: u8 = 0x10;
/// First byte of a notification seen from a real Slate whose meaning is unknown.
pub const UNEXPLAINED_A3: u8 = 0xa3;
/// Size of one pen record.
pub const RECORD_LEN: usize = 6;
/// Most records in one notification of at most 20 bytes.
pub const MAX_RECORDS_PER_NOTIFICATION: usize = (crate::MAX_NOTIFICATION_PAYLOAD - 2) / RECORD_LEN;
/// The record that means "pen out of proximity".
const PROXIMITY_OUT_RECORD: [u8; RECORD_LEN] = [0xff; RECORD_LEN];
/// Offset of the buttons byte in a status notification.
const STATUS_BUTTONS_OFFSET: usize = 10;
/// Resolution of the proximity timestamp.
const TICK_MS: u64 = 5;

/// Sysevent notifications taken for a press of the tablet's button during live mode.
///
/// **Hypothesis, to check at the hardware checkpoint H2:** protocol.md does not say what the
/// Slate sends when its button is pressed in live mode (§7.2 only knows the stylus button byte
/// of the `10` report, which Tuhi ignores; §11 lists the sysevent characteristic as unknown).
/// The working guess is a sysevent. For now the list holds only the simulator's invented
/// signature `f2 01 01`; the real one goes here once an H2 log shows it (every other
/// notification received during live mode is logged as "unclassified during live mode" for that
/// purpose). It must not look like the battery-level sysevent `f1 02 <percent> <flag>` (see
/// [`crate::sysevent`]), which a real Slate sends whenever its level changes.
pub const BUTTON_SYSEVENTS: &[&[u8]] = &[&[0xf2, 0x01, 0x01]];

/// Whether a sysevent notification is a press of the tablet's button (see
/// [`BUTTON_SYSEVENTS`]).
pub fn is_button_sysevent(data: &[u8]) -> bool {
    BUTTON_SYSEVENTS.contains(&data)
}

/// One record of a pen-samples notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LiveRecord {
    /// The pen is in proximity at this position (pressure 0 when hovering).
    Pen(Point),
    /// The pen left proximity (record of six `ff`).
    ProximityOut,
}

/// One decoded live notification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LivePacket {
    /// `a1`: records in order, 5 ms apart.
    Samples(Vec<LiveRecord>),
    /// `a2`: the pen entered proximity.
    ProximityIn {
        /// Tablet timestamp in milliseconds (5 ms ticks), origin unknown.
        timestamp_ms: u64,
    },
    /// `10`: status report.
    Status {
        /// Pressure at bytes 2..4.
        pressure: u16,
        /// Byte 10, when the notification is long enough.
        buttons: Option<u8>,
    },
    /// `a3`: seen from a real Slate, meaning unknown; kept verbatim and ignored.
    Unexplained(Vec<u8>),
    /// Any other first byte, kept verbatim.
    Unknown(Vec<u8>),
}

/// Failure while decoding a live notification.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LiveError {
    /// An empty notification.
    #[error("empty live notification")]
    Empty,
    /// A known first byte with fewer bytes than its fixed fields.
    #[error("live packet {kind:02x} of {len} bytes is too short")]
    TooShort {
        /// First byte.
        kind: u8,
        /// Notification length.
        len: usize,
    },
    /// The length byte of a samples notification announces more bytes than present.
    #[error("pen samples announce {declared} bytes but {actual} are present")]
    LengthMismatch {
        /// Value of the length byte.
        declared: usize,
        /// Bytes present after the length byte.
        actual: usize,
    },
    /// The samples length is not a whole number of 6-byte records.
    #[error("pen samples length {0} is not a multiple of 6")]
    PartialRecord(usize),
    /// A proximity timestamp wider than 64 bits.
    #[error("proximity timestamp of {0} bytes is too wide")]
    TimestampTooWide(usize),
}

/// Decodes one live notification.
///
/// For `a1` the length byte is authoritative: it must be a multiple of 6 and not exceed the
/// bytes present; bytes after it are ignored. The length bytes of `a2` and `10` are not
/// checked (their layouts are only partly known).
pub fn decode(value: &[u8]) -> Result<LivePacket, LiveError> {
    let Some(&kind) = value.first() else {
        return Err(LiveError::Empty);
    };
    let too_short = LiveError::TooShort {
        kind,
        len: value.len(),
    };
    match kind {
        SAMPLES => {
            let [_, declared, rest @ ..] = value else {
                return Err(too_short);
            };
            let declared = usize::from(*declared);
            let data = rest.get(..declared).ok_or(LiveError::LengthMismatch {
                declared,
                actual: rest.len(),
            })?;
            if declared % RECORD_LEN != 0 {
                return Err(LiveError::PartialRecord(declared));
            }
            let (records, _) = data.as_chunks::<RECORD_LEN>();
            let records = records.iter().map(record).collect();
            Ok(LivePacket::Samples(records))
        }
        PROXIMITY_IN => {
            let ticks = value.get(4..).ok_or(too_short)?;
            if ticks.len() > 8 {
                return Err(LiveError::TimestampTooWide(ticks.len()));
            }
            let mut wide = [0u8; 8];
            wide[..ticks.len()].copy_from_slice(ticks);
            Ok(LivePacket::ProximityIn {
                timestamp_ms: u64::from_le_bytes(wide).saturating_mul(TICK_MS),
            })
        }
        STATUS => {
            let [_, _, low, high, ..] = value else {
                return Err(too_short);
            };
            Ok(LivePacket::Status {
                pressure: u16::from_le_bytes([*low, *high]),
                buttons: value.get(STATUS_BUTTONS_OFFSET).copied(),
            })
        }
        UNEXPLAINED_A3 => Ok(LivePacket::Unexplained(value.to_vec())),
        _ => Ok(LivePacket::Unknown(value.to_vec())),
    }
}

/// Encodes records as `a1` notifications of at most 20 bytes (3 records each), as the tablet
/// streams them (used by the simulator).
///
/// A pen record at (`0xffff`, `0xffff`, `0xffff`) would read back as proximity out; real
/// tablets never report it.
pub fn encode_samples(records: &[LiveRecord]) -> Vec<Vec<u8>> {
    records
        .chunks(MAX_RECORDS_PER_NOTIFICATION)
        .map(|chunk| {
            let mut value = Vec::with_capacity(2 + chunk.len() * RECORD_LEN);
            value.push(SAMPLES);
            // At most 18 bytes by construction.
            value.push(u8::try_from(chunk.len() * RECORD_LEN).unwrap_or(u8::MAX));
            for record in chunk {
                match record {
                    LiveRecord::Pen(point) => {
                        value.extend_from_slice(&point.x.to_le_bytes());
                        value.extend_from_slice(&point.y.to_le_bytes());
                        value.extend_from_slice(&point.pressure.to_le_bytes());
                    }
                    LiveRecord::ProximityOut => value.extend_from_slice(&PROXIMITY_OUT_RECORD),
                }
            }
            value
        })
        .collect()
}

/// Encodes a proximity-in notification `a2 06 00 00 <u32 ticks>` (used by the simulator).
///
/// The width of the tick counter is not known; this encoder uses 32 bits, so the timestamp
/// reads back exactly when it is a multiple of 5 ms below 2³² ticks.
pub fn encode_proximity_in(timestamp_ms: u64) -> Vec<u8> {
    // Truncation wraps the counter, as a 32-bit device counter would.
    let ticks = (timestamp_ms / TICK_MS) as u32;
    let mut value = vec![PROXIMITY_IN, 0x06, 0x00, 0x00];
    value.extend_from_slice(&ticks.to_le_bytes());
    value
}

fn record(bytes: &[u8; RECORD_LEN]) -> LiveRecord {
    if *bytes == PROXIMITY_OUT_RECORD {
        return LiveRecord::ProximityOut;
    }
    let [x0, x1, y0, y1, p0, p1] = *bytes;
    LiveRecord::Pen(Point {
        x: u16::from_le_bytes([x0, x1]),
        y: u16::from_le_bytes([y0, y1]),
        pressure: u16::from_le_bytes([p0, p1]),
    })
}

/// `LIVE 3 samples`, `LIVE 1 sample, proximity out`, `LIVE proximity in t=1234 ms`, ...
impl fmt::Display for LivePacket {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Samples(records) => {
                let pens = records
                    .iter()
                    .filter(|r| matches!(r, LiveRecord::Pen(_)))
                    .count();
                let outs = records.len() - pens;
                f.write_str("LIVE")?;
                if pens > 0 || outs == 0 {
                    write!(f, " {pens} sample{}", if pens == 1 { "" } else { "s" })?;
                }
                if outs > 0 {
                    write!(f, "{} proximity out", if pens > 0 { "," } else { "" })?;
                    if outs > 1 {
                        write!(f, " x{outs}")?;
                    }
                }
                Ok(())
            }
            Self::ProximityIn { timestamp_ms } => {
                write!(f, "LIVE proximity in t={timestamp_ms} ms")
            }
            Self::Status { pressure, buttons } => {
                write!(f, "LIVE status pressure={pressure}")?;
                if let Some(buttons) = buttons {
                    write!(f, " buttons={buttons:02x}")?;
                }
                Ok(())
            }
            Self::Unexplained(bytes) => write!(
                f,
                "LIVE {}, seen once after a proximity in, meaning unknown (ignored)",
                hex::encode(bytes)
            ),
            Self::Unknown(bytes) => write!(f, "unknown live packet {}", hex::encode(bytes)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(text: &str) -> Vec<u8> {
        hex::decode(text).unwrap()
    }

    fn pen(x: u16, y: u16, pressure: u16) -> LiveRecord {
        LiveRecord::Pen(Point { x, y, pressure })
    }

    #[test]
    fn decodes_samples() {
        let packet = decode(&bytes(
            "a1 12 c4 09 f4 01 00 00 c5 09 f5 01 10 00 c6 09 f6 01 ff 07",
        ))
        .unwrap();
        assert_eq!(
            packet,
            LivePacket::Samples(vec![
                pen(2500, 500, 0),
                pen(2501, 501, 16),
                pen(2502, 502, 2047)
            ])
        );
        assert_eq!(packet.to_string(), "LIVE 3 samples");
    }

    #[test]
    fn decodes_proximity_out() {
        let packet = decode(&bytes("a1 0c c4 09 f4 01 00 00 ff ff ff ff ff ff")).unwrap();
        assert_eq!(
            packet,
            LivePacket::Samples(vec![pen(2500, 500, 0), LiveRecord::ProximityOut])
        );
        assert_eq!(packet.to_string(), "LIVE 1 sample, proximity out");
        let packet = decode(&bytes("a1 06 ff ff ff ff ff ff")).unwrap();
        assert_eq!(packet.to_string(), "LIVE proximity out");
        assert_eq!(
            decode(&bytes("a1 00")).unwrap().to_string(),
            "LIVE 0 samples"
        );
    }

    #[test]
    fn rejects_malformed_samples() {
        assert_eq!(decode(&[]), Err(LiveError::Empty));
        assert_eq!(
            decode(&[0xa1]),
            Err(LiveError::TooShort { kind: 0xa1, len: 1 })
        );
        assert_eq!(
            decode(&bytes("a1 05 01 02 03 04 05")),
            Err(LiveError::PartialRecord(5))
        );
        assert_eq!(
            decode(&bytes("a1 0c 01 02 03 04 05 06")),
            Err(LiveError::LengthMismatch {
                declared: 12,
                actual: 6
            })
        );
        // Bytes after the announced records are ignored.
        assert_eq!(
            decode(&bytes("a1 06 01 00 02 00 03 00 00 00")).unwrap(),
            LivePacket::Samples(vec![pen(1, 2, 3)])
        );
    }

    #[test]
    fn decodes_proximity_in_and_status() {
        let packet = decode(&bytes("a2 06 00 00 e8 03 00 00")).unwrap();
        assert_eq!(packet, LivePacket::ProximityIn { timestamp_ms: 5000 });
        assert_eq!(packet.to_string(), "LIVE proximity in t=5000 ms");
        assert_eq!(
            decode(&bytes("a2 02 00 00")).unwrap(),
            LivePacket::ProximityIn { timestamp_ms: 0 }
        );
        assert_eq!(
            decode(&bytes("a2 01 00")),
            Err(LiveError::TooShort { kind: 0xa2, len: 3 })
        );
        assert_eq!(
            decode(&bytes("a2 0b 00 00 01 02 03 04 05 06 07 08 09")),
            Err(LiveError::TimestampTooWide(9))
        );

        let status = decode(&bytes("10 09 2a 00 00 00 00 00 00 00 02")).unwrap();
        assert_eq!(
            status,
            LivePacket::Status {
                pressure: 42,
                buttons: Some(2)
            }
        );
        assert_eq!(status.to_string(), "LIVE status pressure=42 buttons=02");
        assert_eq!(
            decode(&bytes("10 02 2a 00")).unwrap(),
            LivePacket::Status {
                pressure: 42,
                buttons: None
            }
        );
        assert_eq!(
            decode(&bytes("10 01 2a")),
            Err(LiveError::TooShort { kind: 0x10, len: 3 })
        );
    }

    #[test]
    fn unknown_packets_are_kept() {
        assert_eq!(
            decode(&bytes("55 01 02")).unwrap(),
            LivePacket::Unknown(vec![0x55, 0x01, 0x02])
        );
    }

    #[test]
    fn encoded_samples_fit_notifications_and_round_trip() {
        let records: Vec<LiveRecord> = (0..7u16)
            .map(|i| pen(2500 + i, 500 + i * 2, i * 300))
            .chain([LiveRecord::ProximityOut])
            .collect();
        let notifications = encode_samples(&records);
        assert_eq!(notifications.len(), 3);
        assert_eq!(notifications[0].len(), crate::MAX_NOTIFICATION_PAYLOAD);
        let mut decoded = Vec::new();
        for value in &notifications {
            assert!(value.len() <= crate::MAX_NOTIFICATION_PAYLOAD);
            let LivePacket::Samples(chunk) = decode(value).unwrap() else {
                panic!("not samples");
            };
            assert!(chunk.len() <= MAX_RECORDS_PER_NOTIFICATION);
            decoded.extend(chunk);
        }
        assert_eq!(decoded, records);
        assert_eq!(MAX_RECORDS_PER_NOTIFICATION, 3);
        assert!(encode_samples(&[]).is_empty());
    }

    #[test]
    fn button_sysevents_are_recognised_exactly() {
        assert!(is_button_sysevent(&bytes("f2 01 01")));
        // Battery levels (issue #212, our captures) are not the button, charging or not.
        assert!(!is_button_sysevent(&bytes("f1 02 13 00")));
        assert!(!is_button_sysevent(&bytes("f1 02 14 01")));
        assert!(!is_button_sysevent(&bytes("f2 01")));
        assert!(!is_button_sysevent(&bytes("f2 01 01 00")));
        assert!(!is_button_sysevent(&[]));
        for signature in BUTTON_SYSEVENTS {
            assert!(
                matches!(
                    crate::sysevent::decode(signature),
                    crate::sysevent::SysEvent::Unknown(_)
                ),
                "{signature:02x?} looks like a known sysevent"
            );
        }
    }

    #[test]
    fn the_a3_packet_of_a_real_slate_is_named_and_ignored() {
        // Our capture (Slate A4, 2026-11-02), right after `a2 06 ee ee 00 02 00 00`.
        let packet = decode(&bytes("a3 06 dd dd dd dd 01 00")).unwrap();
        assert_eq!(
            packet,
            LivePacket::Unexplained(bytes("a3 06 dd dd dd dd 01 00"))
        );
        assert_eq!(
            packet.to_string(),
            "LIVE a3 06 dd dd dd dd 01 00, seen once after a proximity in, meaning unknown (ignored)"
        );
    }

    #[test]
    fn proximity_in_round_trips() {
        for timestamp_ms in [0, 5, 1_234_565, 5 * u64::from(u32::MAX)] {
            let value = encode_proximity_in(timestamp_ms);
            assert_eq!(value.len(), 8);
            assert_eq!(
                decode(&value).unwrap(),
                LivePacket::ProximityIn { timestamp_ms }
            );
        }
    }
}
