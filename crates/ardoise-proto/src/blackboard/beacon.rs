//! How the blackboard is recognised while scanning: the "MiBeacon" Xiaomi devices put in the
//! service data of the 16-bit service `fe95` (research note §2, "Advertisement").
//!
//! Layout, as read by the open-source parsers of Home Assistant (`xiaomi-ble`): frame control
//! (u16 little-endian), product identifier (u16 little-endian), frame counter (u8), then the
//! device address (6 bytes, reversed) when bit 4 of the frame control is set. The blackboard
//! sends `10 59 d8 0f 00` + its address: frame control `0x5910` (version 5, authentication
//! mode 2, registered, address included, not encrypted), product `0x0fd8` (4056, Mi Home's
//! `jiqid.sxb.v1`), counter 0. Only the product identifier is used to recognise it.
//!
//! Reading an advertisement writes nothing: the Xiaomi service itself is never used.

/// The 16-bit UUID of Xiaomi's service, whose service data holds the MiBeacon.
pub const XIAOMI_SERVICE: u16 = 0xfe95;
/// Mi Home's product identifier of the blackboard (`jiqid.sxb.v1`, product 4056).
pub const BLACKBOARD_PRODUCT_ID: u16 = 0x0fd8;
/// The frame control the blackboard sends (see the module documentation).
pub const BLACKBOARD_FRAME_CONTROL: u16 = 0x5910;
/// Mi Home's model name of the blackboard.
pub const BLACKBOARD_MODEL: &str = "jiqid.sxb.v1";

/// Bit of the frame control: the frame is encrypted.
const ENCRYPTED: u16 = 1 << 3;
/// Bit of the frame control: the address follows the counter.
const ADDRESS: u16 = 1 << 4;
/// Bit of the frame control: bound to an account.
const REGISTERED: u16 = 1 << 8;

/// The header of a MiBeacon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MiBeacon {
    pub frame_control: u16,
    pub product_id: u16,
    pub counter: u8,
}

impl MiBeacon {
    /// The header of `data` (the service data of `fe95`), `None` when it is too short.
    pub fn parse(data: &[u8]) -> Option<Self> {
        let [f0, f1, p0, p1, counter, ..] = data else {
            return None;
        };
        Some(Self {
            frame_control: u16::from_le_bytes([*f0, *f1]),
            product_id: u16::from_le_bytes([*p0, *p1]),
            counter: *counter,
        })
    }

    /// Whether this is the beacon of the Xiaomi blackboard (product 4056).
    pub fn is_blackboard(&self) -> bool {
        self.product_id == BLACKBOARD_PRODUCT_ID
    }

    /// Bound to a Mi Home account.
    pub fn registered(&self) -> bool {
        self.frame_control & REGISTERED != 0
    }

    pub fn encrypted(&self) -> bool {
        self.frame_control & ENCRYPTED != 0
    }

    /// MiBeacon version (bits 12 to 15).
    pub fn version(&self) -> u8 {
        u8::try_from(self.frame_control >> 12).unwrap_or(0)
    }
}

/// The service data of the blackboard's beacon for the address `address` (`AA:BB:…` order),
/// as the real tablet sends it (the simulator).
pub fn blackboard_service_data(address: [u8; 6], counter: u8) -> Vec<u8> {
    let mut data = Vec::with_capacity(11);
    data.extend_from_slice(&BLACKBOARD_FRAME_CONTROL.to_le_bytes());
    data.extend_from_slice(&BLACKBOARD_PRODUCT_ID.to_le_bytes());
    data.push(counter);
    if BLACKBOARD_FRAME_CONTROL & ADDRESS != 0 {
        data.extend(address.iter().rev());
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blackboard_beacon_is_recognised() {
        // The research note's layout, with a documentation address (RFC 7042).
        let address = [0x00, 0x00, 0x5e, 0x00, 0x53, 0xb1];
        let data = blackboard_service_data(address, 0);
        assert_eq!(
            data,
            [
                0x10, 0x59, 0xd8, 0x0f, 0x00, 0xb1, 0x53, 0x00, 0x5e, 0x00, 0x00
            ]
        );
        let beacon = MiBeacon::parse(&data).unwrap();
        assert!(beacon.is_blackboard());
        assert!(beacon.registered());
        assert!(!beacon.encrypted());
        assert_eq!(beacon.version(), 5);
        assert_eq!(beacon.counter, 0);
    }

    #[test]
    fn other_beacons_and_short_data_are_not_the_blackboard() {
        let other = MiBeacon::parse(&[0x10, 0x59, 0x5b, 0x04, 0x07]).unwrap();
        assert!(!other.is_blackboard());
        assert_eq!(MiBeacon::parse(&[0x10, 0x59, 0xd8, 0x0f]), None);
        assert_eq!(MiBeacon::parse(&[]), None);
    }
}
