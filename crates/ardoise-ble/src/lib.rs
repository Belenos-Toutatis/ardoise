//! Windows Bluetooth LE transport for the tablet.
//!
//! Currently exposes adapter detection ([`adapter_info`]), used by `--self-test`.

use std::fmt;

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
}
