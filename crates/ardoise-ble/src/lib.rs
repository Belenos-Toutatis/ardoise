//! Windows Bluetooth LE transport for the tablet.
//!
//! - [`BleConnector`] / [`BleTransport`] implement the blocking [`Connector`] / [`Transport`]
//!   contract of `ardoise-device` over btleplug (WinRT backend). btleplug is asynchronous; a
//!   private tokio runtime inside this crate runs it, and nothing async leaks out.
//! - [`adapter_info`] reports the state of the default adapter (used by `--self-test`).
//! - [`gatt`] lists the only characteristics ever used. Writes go to Nordic UART TX and
//!   nowhere else; the firmware-update (DFU) service is never touched.
//!
//! On other platforms the connector finds no adapter.
//!
//! [`Connector`]: ardoise_device::transport::Connector
//! [`Transport`]: ardoise_device::transport::Transport

use std::fmt;

pub mod gatt;

#[cfg(windows)]
mod connector;
#[cfg(windows)]
mod convert;
#[cfg(windows)]
mod executor;
#[cfg(windows)]
mod link;
#[cfg(not(windows))]
mod unsupported;
// Keeps the placeholder compiling (and tested) on the Windows development machine.
#[cfg(all(windows, test))]
#[path = "unsupported.rs"]
mod unsupported;

#[cfg(windows)]
pub use connector::BleConnector;
#[cfg(windows)]
pub use link::BleTransport;
#[cfg(not(windows))]
pub use unsupported::{BleConnector, BleTransport};

// Compile-time check that the types fulfil the contract.
const _: () = {
    const fn connector<T: ardoise_device::transport::Connector + Send>() {}
    const fn transport<T: ardoise_device::transport::Transport + Send>() {}
    connector::<BleConnector>();
    transport::<BleTransport>();
};

/// State of the default Bluetooth adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AdapterStatus {
    /// No Bluetooth adapter at all.
    NotFound,
    /// An adapter exists but does not support Bluetooth Low Energy.
    NoLowEnergy,
    /// BLE-capable adapter whose radio is switched off.
    Off,
    /// BLE-capable adapter whose radio is on.
    On,
}

impl fmt::Display for AdapterStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::NotFound => "not found",
            Self::NoLowEnergy => "no Bluetooth LE support",
            Self::Off => "off",
            Self::On => "on",
        })
    }
}

/// Result of querying the default adapter.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct AdapterInfo {
    pub status: AdapterStatus,
    pub name: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum BleError {
    #[error("Windows Bluetooth API error: {0}")]
    Windows(String),
}

/// Blocking. Queries the default Bluetooth adapter.
pub fn adapter_info() -> Result<AdapterInfo, BleError> {
    imp::adapter_info()
}

#[cfg(windows)]
mod imp {
    use super::{AdapterInfo, AdapterStatus, BleError};
    use windows::Devices::Bluetooth::BluetoothAdapter;
    use windows::Devices::Radios::RadioState;
    use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};

    fn err(e: windows::core::Error) -> BleError {
        BleError::Windows(e.to_string())
    }

    pub fn adapter_info() -> Result<AdapterInfo, BleError> {
        // Initialise the WinRT apartment for this thread. If the thread is
        // already initialised (S_FALSE) or uses another apartment model
        // (RPC_E_CHANGED_MODE), WinRT calls still work, so ignore the result.
        // SAFETY: plain FFI call with a valid constant; no pointers involved.
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };

        // `join()` blocks the calling thread until the async operation completes.
        let adapter = match BluetoothAdapter::GetDefaultAsync().map_err(err)?.join() {
            Ok(a) => a,
            // A null result (no adapter) can surface as an error on some builds.
            Err(e) if e.code().is_ok() => {
                return Ok(AdapterInfo {
                    status: AdapterStatus::NotFound,
                    name: None,
                });
            }
            Err(e) => return Err(err(e)),
        };
        if !adapter.IsLowEnergySupported().map_err(err)? {
            return Ok(AdapterInfo {
                status: AdapterStatus::NoLowEnergy,
                name: None,
            });
        }
        let radio = adapter.GetRadioAsync().map_err(err)?.join().map_err(err)?;
        let status = if radio.State().map_err(err)? == RadioState::On {
            AdapterStatus::On
        } else {
            AdapterStatus::Off
        };
        let name = radio
            .Name()
            .ok()
            .map(|n| n.to_string())
            .filter(|n| !n.is_empty());
        Ok(AdapterInfo { status, name })
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{AdapterInfo, AdapterStatus, BleError};

    pub fn adapter_info() -> Result<AdapterInfo, BleError> {
        Ok(AdapterInfo {
            status: AdapterStatus::NotFound,
            name: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_texts() {
        assert_eq!(AdapterStatus::On.to_string(), "on");
        assert_eq!(AdapterStatus::Off.to_string(), "off");
        assert_eq!(AdapterStatus::NotFound.to_string(), "not found");
        assert_eq!(
            AdapterStatus::NoLowEnergy.to_string(),
            "no Bluetooth LE support"
        );
    }

    #[test]
    fn serde_names() {
        let j = |s| serde_json::to_string(&s).unwrap_or_default();
        assert_eq!(j(AdapterStatus::NotFound), "\"not_found\"");
        assert_eq!(j(AdapterStatus::NoLowEnergy), "\"no_low_energy\"");
        assert_eq!(j(AdapterStatus::Off), "\"off\"");
        assert_eq!(j(AdapterStatus::On), "\"on\"");
    }

    #[test]
    fn smoke_adapter_info() {
        let r = adapter_info();
        println!("adapter_info = {r:?}");
        assert!(r.is_ok());
    }

    mod transport {
        use super::super::{BleConnector, BleTransport};
        use ardoise_device::transport::{Connector, Transport, TransportError};
        use std::sync::{Mutex, PoisonError};
        use std::time::{Duration, Instant};

        /// The radio tests share the adapter: run them one at a time.
        static RADIO: Mutex<()> = Mutex::new(());

        #[test]
        fn types_implement_the_contract() {
            fn connector<T: Connector + Send>() {}
            fn transport<T: Transport + Send>() {}
            connector::<BleConnector>();
            transport::<BleTransport>();
        }

        #[test]
        fn clock_is_monotonic() {
            let connector = BleConnector::new().unwrap();
            let clock = connector.clock();
            let a = clock.monotonic();
            assert!(clock.monotonic() >= a);
        }

        /// Passes without an adapter or with the radio off; with one, nothing may match.
        #[test]
        fn smoke_find_nothing() {
            let _radio = RADIO.lock().unwrap_or_else(PoisonError::into_inner);
            let mut connector = BleConnector::new().unwrap();
            let mut seen = 0usize;
            let start = Instant::now();
            let result = connector.find(Duration::from_secs(3), &mut |_| {
                seen += 1;
                false
            });
            let elapsed = start.elapsed();
            println!("find = {result:?} after {elapsed:?}, {seen} advertisement(s) offered");
            match result {
                Ok(None) => assert!(elapsed >= Duration::from_secs(3)),
                Err(TransportError::NoAdapter | TransportError::AdapterOff) => {}
                other => panic!("unexpected scan result {other:?}"),
            }
            assert!(
                elapsed < Duration::from_secs(8),
                "the scan did not stop in time"
            );
        }

        /// No device has this address: the connector scans for it, then gives up.
        #[test]
        fn smoke_connect_unknown_address() {
            let _radio = RADIO.lock().unwrap_or_else(PoisonError::into_inner);
            let mut connector = BleConnector::new().unwrap();
            let start = Instant::now();
            let result = connector.connect("00:00:5E:00:53:00", Duration::from_secs(2));
            println!(
                "connect = {:?} after {:?}",
                result.as_ref().map(|t| t.peer().clone()),
                start.elapsed()
            );
            match result {
                Err(
                    TransportError::NotFound(_)
                    | TransportError::NoAdapter
                    | TransportError::AdapterOff,
                ) => {}
                other => panic!(
                    "unexpected connect result {:?}",
                    other.map(|t| t.peer().clone())
                ),
            }
            assert!(start.elapsed() < Duration::from_secs(8));
        }

        #[cfg(windows)]
        #[test]
        fn invalid_address_is_rejected() {
            let mut connector = BleConnector::new().unwrap();
            match connector.connect("not an address", Duration::from_secs(1)) {
                Err(TransportError::Backend(text)) => assert!(text.contains("invalid")),
                other => panic!(
                    "unexpected connect result {:?}",
                    other.map(|t| t.peer().clone())
                ),
            }
        }
    }
}
