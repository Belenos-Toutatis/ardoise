//! The abstract link to a tablet.
//!
//! The device manager only talks to a [`Connector`] (find tablets, open a link) and a
//! [`Transport`] (one open link). `ardoise-ble` implements them over Windows Bluetooth LE,
//! `ardoise-sim` over a simulated tablet or a replayed capture, so everything above this
//! module runs unchanged against the real device, the simulator and recorded sessions.
//!
//! The interface is blocking on purpose: the manager runs on its own thread and waits with
//! timeouts, which keeps the protocol code linear and the tests deterministic.
//!
//! Two kinds of tablet share it ([`TabletKind`]): the Wacom Bamboo Slate and the Xiaomi Mijia
//! LCD blackboard (docs/design/blackboard.md). Both take their commands on Nordic UART TX and
//! answer on Nordic UART RX ([`Channel::Command`]); the blackboard sends its pen records and
//! page bytes on a channel of its own ([`Channel::BoardData`]).

use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

/// Bluetooth company identifiers used by Wacom smartpads in their manufacturer data.
/// Other devices are ignored, so the application never writes to an unrelated peripheral.
pub const WACOM_COMPANY_IDS: [u16; 3] = [0x4755, 0x4157, 0x424d];

/// Length of the manufacturer data while the tablet waits to be registered (blue LED blinking).
/// It is 7 bytes in normal mode.
pub const REGISTER_MODE_DATA_LEN: usize = 4;

/// The kinds of tablet Ardoise talks to. Nothing else is ever connected to or written to.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Default, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub enum TabletKind {
    /// A Wacom smartpad (Bamboo Slate), recognised by its company identifier.
    #[default]
    Slate,
    /// The Xiaomi Mijia LCD blackboard, storage edition, recognised by its MiBeacon (product
    /// 4056 in the service data of `fe95`).
    Blackboard,
}

impl TabletKind {
    /// How the diagnostic log and the status name it.
    pub fn name(self) -> &'static str {
        match self {
            TabletKind::Slate => "slate",
            TabletKind::Blackboard => "blackboard",
        }
    }
}

/// Where a notification came from. Host writes always go to the command characteristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// Replies to commands (Nordic UART RX, `6e400003-…`).
    Command,
    /// Raw bytes of a stored page during a download (`ffee0003-…`).
    PageData,
    /// Live-mode pen samples (`00001524-…`).
    Live,
    /// "Sysevent" notifications (`3a340721-…`), meaning unknown, only logged.
    SysEvent,
    /// The Xiaomi blackboard's live pen records and stored-page bytes (`6e400004-…`).
    BoardData,
}

/// Something received on an open link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportEvent {
    /// One notification payload, exactly as received.
    Notification { channel: Channel, data: Vec<u8> },
    /// The link was lost (tablet switched off, out of range, idle timeout…).
    /// The transport is unusable afterwards; reconnect through the [`Connector`].
    Disconnected,
}

/// A tablet seen while scanning.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Advertisement {
    /// Bluetooth address, `E2:43:03:67:0E:01` style. Used to connect.
    pub address: String,
    pub name: Option<String>,
    /// Signal strength in dBm, when known.
    pub rssi: Option<i16>,
    /// Company identifier of the manufacturer-specific data, when present.
    pub company_id: Option<u16>,
    /// Manufacturer-specific data, without the company identifier.
    pub manufacturer_data: Vec<u8>,
    /// Service data of Xiaomi's 16-bit service `fe95` (a MiBeacon), when present. Absent from
    /// the logs and identities written before the blackboard was known (serde default).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub xiaomi_service_data: Vec<u8>,
}

impl Advertisement {
    /// Whether this looks like a Wacom smartpad.
    pub fn is_wacom(&self) -> bool {
        self.company_id
            .is_some_and(|id| WACOM_COMPANY_IDS.contains(&id))
    }

    /// Whether the tablet is waiting to be registered (button held until the LED blinks blue).
    pub fn in_register_mode(&self) -> bool {
        self.is_wacom() && self.manufacturer_data.len() == REGISTER_MODE_DATA_LEN
    }

    /// Whether this is the Xiaomi blackboard: its MiBeacon names product 4056
    /// (`ardoise_proto::blackboard::beacon`).
    pub fn is_blackboard(&self) -> bool {
        ardoise_proto::blackboard::beacon::MiBeacon::parse(&self.xiaomi_service_data)
            .is_some_and(|beacon| beacon.is_blackboard())
    }

    /// The kind of tablet this is, `None` for any other device (never connected to).
    pub fn kind(&self) -> Option<TabletKind> {
        if self.is_wacom() {
            Some(TabletKind::Slate)
        } else if self.is_blackboard() {
            Some(TabletKind::Blackboard)
        } else {
            None
        }
    }
}

/// Errors of a connector or a transport.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransportError {
    #[error("no Bluetooth LE adapter")]
    NoAdapter,
    #[error("Bluetooth is switched off")]
    AdapterOff,
    #[error("tablet {0} not found")]
    NotFound(String),
    #[error("the connection timed out")]
    Timeout,
    #[error("not connected")]
    NotConnected,
    #[error("the tablet does not offer the expected Bluetooth service ({0})")]
    MissingCharacteristic(String),
    /// A recorded session no longer matches what the host sends (replay only).
    #[error("replay mismatch: {0}")]
    Replay(String),
    /// Anything else reported by the Bluetooth stack, with its message.
    #[error("Bluetooth error: {0}")]
    Backend(String),
}

/// One open link to a tablet.
pub trait Transport: Send {
    /// The tablet this link is connected to.
    fn peer(&self) -> &Advertisement;

    /// Writes one request frame to the command characteristic (Nordic UART TX).
    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError>;

    /// Waits up to `timeout` for the next event; `Ok(None)` when nothing arrived in time.
    /// Events from all channels come through here, in arrival order.
    fn recv(&mut self, timeout: Duration) -> Result<Option<TransportEvent>, TransportError>;

    /// Closes the link. Calling it on a lost link is not an error.
    fn disconnect(&mut self) -> Result<(), TransportError>;
}

/// Finds tablets and opens links to them.
pub trait Connector: Send {
    /// Time source matching this connector: the real clock for Bluetooth, a virtual clock
    /// for the simulator (so a 30 s live session runs instantly in tests).
    fn clock(&self) -> Arc<dyn Clock>;

    /// Scans until `timeout` or until `filter` accepts an advertisement, which is returned.
    /// `Ok(None)` when nothing matched in time.
    fn find(
        &mut self,
        timeout: Duration,
        filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError>;

    /// Connects to the tablet, discovers its services and subscribes to every notification
    /// channel it offers, so the first [`Transport::write`] can follow immediately.
    fn connect(
        &mut self,
        address: &str,
        timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError>;
}

/// Time as seen by the device manager and the diagnostic log.
pub trait Clock: Send + Sync {
    /// Monotonic time since an arbitrary origin (durations, timeouts, log timestamps).
    fn monotonic(&self) -> Duration;
    /// Current wall-clock time (sent to the tablet, file names).
    fn now(&self) -> SystemTime;
}

/// The computer's clock.
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn monotonic(&self) -> Duration {
        self.origin.elapsed()
    }

    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn advertisement(company_id: Option<u16>, data_len: usize) -> Advertisement {
        Advertisement {
            address: "E2:43:03:67:0E:01".into(),
            name: Some("Bamboo Slate".into()),
            rssi: Some(-60),
            company_id,
            manufacturer_data: vec![0; data_len],
            xiaomi_service_data: Vec::new(),
        }
    }

    #[test]
    fn recognises_wacom_tablets_and_register_mode() {
        assert!(advertisement(Some(0x4755), 7).is_wacom());
        assert!(!advertisement(Some(0x4755), 7).in_register_mode());
        assert!(advertisement(Some(0x424d), 4).in_register_mode());
        assert!(!advertisement(Some(0x0006), 4).is_wacom());
        assert!(!advertisement(None, 4).in_register_mode());
        assert_eq!(
            advertisement(Some(0x4755), 7).kind(),
            Some(TabletKind::Slate)
        );
        assert_eq!(advertisement(Some(0x0006), 4).kind(), None);
    }

    #[test]
    fn recognises_the_blackboard_by_its_beacon_only() {
        let mut board = advertisement(None, 0);
        board.name = Some("xiaomi.dev.ble".into());
        assert_eq!(board.kind(), None, "the name alone is not enough");
        board.xiaomi_service_data =
            ardoise_proto::blackboard::beacon::blackboard_service_data([0, 0, 0x5e, 0, 0x53, 1], 0);
        assert!(board.is_blackboard());
        assert_eq!(board.kind(), Some(TabletKind::Blackboard));
        // Another Xiaomi product.
        board.xiaomi_service_data = vec![0x10, 0x59, 0x5b, 0x04, 0x00];
        assert_eq!(board.kind(), None);
        // Older JSON without the field still loads.
        let old = r#"{"address":"E2:43:03:67:0E:01","name":null,"rssi":null,"companyId":18261,"manufacturerData":[0,0,0,0,0,0,0]}"#;
        let parsed: Advertisement = serde_json::from_str(old).unwrap();
        assert!(parsed.xiaomi_service_data.is_empty());
        assert!(!serde_json::to_string(&parsed).unwrap().contains("xiaomi"));
    }

    #[test]
    fn system_clock_is_monotonic() {
        let clock = SystemClock::new();
        let a = clock.monotonic();
        let b = clock.monotonic();
        assert!(b >= a);
    }
}
