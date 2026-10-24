//! Simulated tablet and capture replay, behind the same transport interface as ardoise-ble.
//!
//! The real Bamboo Slate is never on the development PC, so everything above the Bluetooth
//! layer is tested here (docs/design/device-stack.md, "ardoise-sim"):
//!
//! - [`SimTablet`]: a simulated Bamboo Slate (one stored identity, registration with the
//!   button, clock, name, battery, size, stored pages, live mode) with physical actions
//!   (button, drawing pages, pen movement), inspection and fault injection. Its
//!   [`SimConnector`] implements the device stack's `Connector` on a [`VirtualClock`], so a
//!   30 s live session runs in milliseconds and timeouts elapse instantly.
//! - [`SimulatedUser`]: the person in front of the simulated tablet for the guided test
//!   (`ardoise.exe --diagnostic --simulated`, the interface's simulation option, the tests).
//! - [`replay`]: a connector that replays a recorded `.jsonl` diagnostic log.
//! - [`drawings`]: reference pages and live handwriting.
//! - [`pen`]: pen movement for live mode.

pub mod clock;
mod connector;
pub mod drawings;
pub mod pen;
pub mod replay;
mod tablet;
mod user;

pub use clock::{SIM_EPOCH_UNIX, VirtualClock};
pub use connector::{SimConnector, SimTransport};
pub use pen::{PenScript, PenSegment};
pub use replay::{ReplayConnector, ReplayError, ReplayOptions};
pub use tablet::{CONNECT_SYSEVENT, SimConfig, SimStats, SimTablet, StoredPage};
pub use user::SimulatedUser;
