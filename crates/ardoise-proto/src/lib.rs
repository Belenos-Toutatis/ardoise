//! Wacom smartpad protocol as spoken by the Bamboo Slate: message encoding and decoding,
//! stored-page decoding and live-mode decoding; and, in [`blackboard`], the vendor protocol of the
//! second tablet, the Xiaomi Mijia LCD blackboard (storage edition).
//!
//! The crate does no I/O, keeps no clock and spawns nothing: every function works on byte
//! slices, so the device layer (host side), the simulator (tablet side) and the tests share the
//! same code. It is a clean-room implementation of `docs/research/protocol.md` (SPEC §6.2).
//! Multi-byte integers are little-endian unless stated otherwise.
//!
//! Modules:
//!
//! - [`frame`]: the `opcode, length, arguments` framing of the Nordic UART characteristics.
//! - [`message`]: typed [`Request`]s (host to tablet) and [`Reply`]s (tablet to host), each
//!   encoded and decoded in both directions, plus reassembly of the tablet name.
//! - [`time`]: [`BcdTime`], the tablet clock format (`yymmddhhmmss` BCD, UTC).
//! - [`stroke`]: stored page files (Slate and Intuos Pro formats): decoder, and an encoder for
//!   Slate files used by the simulator.
//! - [`live`]: live-mode pen notifications: decoder, and encoders used by the simulator.
//! - [`sysevent`]: notifications of the sysevent characteristic (battery level, and frames seen
//!   from a real Slate whose meaning is unknown).
//! - [`crc`]: the CRC-32 that checks a downloaded page.
//! - [`describe`]: one-line English interpretation of any frame or packet, for the diagnostic
//!   log. It never fails.
//! - [`hex`]: `e6 06 ab ...` hex dumps, as written in the diagnostic log.
//! - [`blackboard`]: the Xiaomi blackboard (docs/research/xiaomi-blackboard.md): its frames, the
//!   7-byte records of its live stream and stored pages, and its advertisement (MiBeacon).

pub mod blackboard;
pub mod crc;
pub mod describe;
pub mod frame;
pub mod hex;
pub mod live;
pub mod message;
pub mod stroke;
pub mod sysevent;
pub mod time;

pub use crc::crc32;
pub use frame::{Frame, FrameError};
pub use message::{
    Dimension, FirmwarePart, HostId, MessageError, Mode, NameAssembler, Reply, Request, StatusCode,
    firmware_digits_frame,
};
pub use time::{BcdTime, TimeError};

/// Width of the Bamboo Slate Large (A4), in device units of 10 µm (297 mm).
pub const SLATE_LARGE_WIDTH: u32 = 29_700;
/// Height of the Bamboo Slate Large (A4), in device units of 10 µm (216 mm).
pub const SLATE_LARGE_HEIGHT: u32 = 21_600;
/// Width of the small Bamboo Slate (A5), in device units of 10 µm (216 mm).
pub const SLATE_SMALL_WIDTH: u32 = 21_600;
/// Height of the small Bamboo Slate (A5), in device units of 10 µm (148 mm).
pub const SLATE_SMALL_HEIGHT: u32 = 14_800;
/// Size of one device unit in micrometres (fixed on the Slate, not queryable).
pub const POINT_SIZE_UM: u32 = 10;
/// Highest pressure value reported by a Slate (fixed, not queryable).
pub const SLATE_MAX_PRESSURE: u16 = 2047;
/// Time between two pen samples, both in stored pages and in live mode.
pub const SAMPLE_INTERVAL_MS: u32 = 5;
/// Largest payload of one notification or write with the default ATT MTU.
pub const MAX_NOTIFICATION_PAYLOAD: usize = 20;
