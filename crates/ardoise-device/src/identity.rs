//! The tablet registration shared by every PC: `<library>/_ardoise/device.json` (SPEC §3.2).
//!
//! A Wacom smartpad remembers a single 6-byte identifier chosen by the host at registration
//! and refuses any other one. Storing it in the synced library lets the work PC and the
//! laptop use the tablet after registering it only once.
//!
//! The Xiaomi blackboard has no registration: "adding" it saves its address in its own file,
//! `<library>/_ardoise/blackboard.json` ([`BlackboardRecord`]), shared the same way. An older
//! Ardoise ignores that file and keeps working with the Slate's `device.json`, whose meaning
//! does not change.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use ardoise_core::fsutil::write_atomic;
use ardoise_core::library::META_DIR;

/// File name inside `_ardoise/`.
pub const DEVICE_FILE: &str = "device.json";
/// Version of the file format.
pub const DEVICE_FORMAT: u32 = 1;
/// File name of the known Xiaomi blackboard inside `_ardoise/`.
pub const BLACKBOARD_FILE: &str = "blackboard.json";
/// Version of the blackboard file's format.
pub const BLACKBOARD_FORMAT: u32 = 1;
/// The protocol written in the blackboard file.
pub const BLACKBOARD_PROTOCOL: &str = "xiaomi-blackboard";

/// The 6-byte identifier the tablet accepts (Tuhi calls it a "UUID"), shown as 12 hex digits.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct HostId(pub [u8; 6]);

impl HostId {
    /// A new random identifier, for a registration.
    pub fn generate() -> Result<Self, IdentityError> {
        let mut bytes = [0u8; 6];
        getrandom::fill(&mut bytes).map_err(|e| IdentityError::Random(e.to_string()))?;
        Ok(Self(bytes))
    }

    /// Parses 12 hex digits.
    pub fn from_hex(text: &str) -> Option<Self> {
        if text.len() != 12 || !text.is_ascii() {
            return None;
        }
        let mut bytes = [0u8; 6];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[2 * i..2 * i + 2], 16).ok()?;
        }
        Some(Self(bytes))
    }
}

impl fmt::Display for HostId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.iter().try_for_each(|b| write!(f, "{b:02x}"))
    }
}

impl fmt::Debug for HostId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HostId({self})")
    }
}

impl serde::Serialize for HostId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for HostId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_hex(&text)
            .ok_or_else(|| serde::de::Error::custom("expected 12 hexadecimal digits"))
    }
}

/// Content of `device.json`.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    pub format: u32,
    /// Identifier sent at every connection.
    pub uuid: HostId,
    /// Bluetooth address of the tablet.
    pub address: String,
    pub name: String,
    /// Protocol family; only `"slate"` so far.
    pub protocol: String,
    /// Drawing area in device units (10 µm).
    pub width: u32,
    pub height: u32,
    /// When the tablet was registered, RFC 3339.
    pub registered: String,
    /// Fields written by a newer version, kept on rewrite.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Content of `blackboard.json`: the Xiaomi blackboard this library knows. No secret: the
/// tablet accepts any client (docs/research/xiaomi-blackboard.md §3).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlackboardRecord {
    pub format: u32,
    /// Bluetooth address of the tablet.
    pub address: String,
    /// The name shown for it (the tablet's advertised name is a generic `xiaomi.dev.ble`).
    pub name: String,
    /// Always [`BLACKBOARD_PROTOCOL`].
    pub protocol: String,
    /// Mi Home's model name (`jiqid.sxb.v1`).
    pub model: String,
    /// When the tablet was added, RFC 3339.
    pub added: String,
    /// Fields written by a newer version, kept on rewrite.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

/// Errors loading or saving the registration.
#[derive(Debug, thiserror::Error)]
pub enum IdentityError {
    #[error("cannot access {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("{path} is damaged: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("no random source: {0}")]
    Random(String),
}

/// Path of `device.json` in a library.
pub fn device_file(library: &Path) -> PathBuf {
    library.join(META_DIR).join(DEVICE_FILE)
}

/// The registered tablet, or `None` if no tablet was registered from this library.
pub fn load(library: &Path) -> Result<Option<DeviceRecord>, IdentityError> {
    load_json(&device_file(library))
}

/// Saves the registration (atomic write).
pub fn save(library: &Path, record: &DeviceRecord) -> Result<(), IdentityError> {
    save_json(&device_file(library), record)
}

/// Path of `blackboard.json` in a library.
pub fn blackboard_file(library: &Path) -> PathBuf {
    library.join(META_DIR).join(BLACKBOARD_FILE)
}

/// The known blackboard, or `None` if none was added from this library.
pub fn load_blackboard(library: &Path) -> Result<Option<BlackboardRecord>, IdentityError> {
    load_json(&blackboard_file(library))
}

/// Saves the known blackboard (atomic write).
pub fn save_blackboard(library: &Path, record: &BlackboardRecord) -> Result<(), IdentityError> {
    save_json(&blackboard_file(library), record)
}

/// Forgets the blackboard. Not an error if none was added.
pub fn forget_blackboard(library: &Path) -> Result<(), IdentityError> {
    remove(&blackboard_file(library))
}

fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<Option<T>, IdentityError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => {
            return Err(IdentityError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| IdentityError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })
}

fn save_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), IdentityError> {
    let io_error = |source| IdentityError::Io {
        path: path.to_path_buf(),
        source,
    };
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(io_error)?;
    }
    let mut text = serde_json::to_string_pretty(value).map_err(|e| IdentityError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    text.push('\n');
    write_atomic(path, text.as_bytes()).map_err(io_error)
}

fn remove(path: &Path) -> Result<(), IdentityError> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(IdentityError::Io {
            path: path.to_path_buf(),
            source: e,
        }),
        _ => Ok(()),
    }
}

/// Forgets the registered tablet. Not an error if none was registered.
pub fn forget(library: &Path) -> Result<(), IdentityError> {
    remove(&device_file(library))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> DeviceRecord {
        DeviceRecord {
            format: DEVICE_FORMAT,
            uuid: HostId([0x89, 0xea, 0x78, 0x88, 0xff, 0xcb]),
            address: "E2:43:03:67:0E:01".into(),
            name: "Bamboo Slate".into(),
            protocol: "slate".into(),
            width: 29700,
            height: 21600,
            registered: "2026-10-24T14:03:12+02:00".into(),
            extra: Default::default(),
        }
    }

    #[test]
    fn host_id_hex_round_trip() {
        let id = HostId([0x89, 0xea, 0x78, 0x88, 0xff, 0xcb]);
        assert_eq!(id.to_string(), "89ea7888ffcb");
        assert_eq!(HostId::from_hex("89ea7888ffcb"), Some(id));
        assert_eq!(HostId::from_hex("89EA7888FFCB"), Some(id));
        for bad in [
            "",
            "89ea7888ffc",
            "89ea7888ffcbb",
            "89ea7888ffcz",
            "é9ea7888ffc",
        ] {
            assert_eq!(HostId::from_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn generated_ids_differ() {
        assert_ne!(HostId::generate().unwrap(), HostId::generate().unwrap());
    }

    #[test]
    fn missing_file_means_not_registered() {
        let library = tempfile::tempdir().unwrap();
        assert!(load(library.path()).unwrap().is_none());
        forget(library.path()).unwrap();
    }

    #[test]
    fn saves_loads_and_forgets() {
        let library = tempfile::tempdir().unwrap();
        save(library.path(), &record()).unwrap();
        let text = fs::read_to_string(device_file(library.path())).unwrap();
        assert!(text.contains(r#""uuid": "89ea7888ffcb""#), "{text}");
        assert_eq!(load(library.path()).unwrap(), Some(record()));

        forget(library.path()).unwrap();
        assert!(load(library.path()).unwrap().is_none());
    }

    #[test]
    fn keeps_fields_from_newer_versions() {
        let library = tempfile::tempdir().unwrap();
        let mut newer = serde_json::to_value(record()).unwrap();
        newer["orientation"] = "left-handed".into();
        fs::create_dir_all(library.path().join(META_DIR)).unwrap();
        fs::write(device_file(library.path()), newer.to_string()).unwrap();

        let loaded = load(library.path()).unwrap().unwrap();
        assert_eq!(loaded.extra["orientation"], "left-handed");
        save(library.path(), &loaded).unwrap();
        let again = load(library.path()).unwrap().unwrap();
        assert_eq!(again.extra["orientation"], "left-handed");
    }

    fn blackboard() -> BlackboardRecord {
        BlackboardRecord {
            format: BLACKBOARD_FORMAT,
            address: "00:00:5E:00:53:B1".into(),
            name: "Mijia LCD blackboard".into(),
            protocol: BLACKBOARD_PROTOCOL.into(),
            model: "jiqid.sxb.v1".into(),
            added: "2026-11-12T20:00:00+01:00".into(),
            extra: Default::default(),
        }
    }

    #[test]
    fn the_blackboard_has_its_own_file_next_to_the_slates() {
        let library = tempfile::tempdir().unwrap();
        assert!(load_blackboard(library.path()).unwrap().is_none());
        forget_blackboard(library.path()).unwrap();
        save(library.path(), &record()).unwrap();
        save_blackboard(library.path(), &blackboard()).unwrap();
        assert_eq!(load_blackboard(library.path()).unwrap(), Some(blackboard()));
        // `device.json` is untouched: an older Ardoise still finds the Slate.
        assert_eq!(load(library.path()).unwrap(), Some(record()));
        let text = fs::read_to_string(blackboard_file(library.path())).unwrap();
        assert!(
            text.contains(r#""protocol": "xiaomi-blackboard""#),
            "{text}"
        );
        // Forgetting one keeps the other.
        forget_blackboard(library.path()).unwrap();
        assert!(load_blackboard(library.path()).unwrap().is_none());
        assert_eq!(load(library.path()).unwrap(), Some(record()));
        // Fields of a newer version survive a rewrite; a damaged file is reported.
        let mut newer = serde_json::to_value(blackboard()).unwrap();
        newer["calibration"] = 1.02.into();
        fs::write(blackboard_file(library.path()), newer.to_string()).unwrap();
        let loaded = load_blackboard(library.path()).unwrap().unwrap();
        save_blackboard(library.path(), &loaded).unwrap();
        let again = load_blackboard(library.path()).unwrap().unwrap();
        assert_eq!(again.extra["calibration"], 1.02);
        fs::write(blackboard_file(library.path()), "{").unwrap();
        assert!(matches!(
            load_blackboard(library.path()),
            Err(IdentityError::Parse { .. })
        ));
    }

    #[test]
    fn damaged_file_is_reported() {
        let library = tempfile::tempdir().unwrap();
        fs::create_dir_all(library.path().join(META_DIR)).unwrap();
        fs::write(device_file(library.path()), "{ not json").unwrap();
        assert!(matches!(
            load(library.path()),
            Err(IdentityError::Parse { .. })
        ));
        let mut bad_id = serde_json::to_value(record()).unwrap();
        bad_id["uuid"] = "xyz".into();
        fs::write(device_file(library.path()), bad_id.to_string()).unwrap();
        assert!(matches!(
            load(library.path()),
            Err(IdentityError::Parse { .. })
        ));
    }
}
