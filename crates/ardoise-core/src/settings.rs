//! Portable settings stored in `ardoise.toml`.
//!
//! The file lives next to the executable so it travels with the app, falling back to
//! `%APPDATA%\Ardoise` when that folder is read-only. Settings that depend on the computer
//! (such as the library folder, which differs per Nextcloud install) are stored in a
//! `[machines."<NAME>"]` section so several PCs can share one file.
//!
//! Unknown keys are preserved on every write, so a newer or older version of the app on another
//! PC never loses data it does not understand.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsutil::write_atomic;

/// Interface language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// Follow the system language.
    #[default]
    Auto,
    /// French.
    Fr,
    /// English.
    En,
}

/// Interface theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    /// Follow the system theme.
    #[default]
    System,
    /// Light theme.
    Light,
    /// Dark theme.
    Dark,
}

/// File format of the automatic export after a sync (SPEC §3.2). Stored as `"off"`, `"pdf"` or
/// `"png"`; any other value (written by a newer version on another PC) loads as
/// [`Unknown`](Self::Unknown), which exports nothing and is written back verbatim, so the
/// settings file shared by every PC stays readable and keeps what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum AutoExportFormat {
    /// No automatic export.
    #[default]
    Off,
    /// One PDF per synced note.
    Pdf,
    /// One PNG per synced note.
    Png,
    /// A value this version does not know, kept as written.
    Unknown(String),
}

impl AutoExportFormat {
    /// Whether notes are exported (not for `Off`, nor for a value this version does not know).
    pub fn is_active(&self) -> bool {
        matches!(self, AutoExportFormat::Pdf | AutoExportFormat::Png)
    }

    /// The format for a program that only knows off / pdf / png: `Off` for an unknown value.
    pub fn known(&self) -> AutoExportFormat {
        if matches!(self, AutoExportFormat::Unknown(_)) {
            AutoExportFormat::Off
        } else {
            self.clone()
        }
    }
}

impl From<String> for AutoExportFormat {
    fn from(text: String) -> Self {
        match text.as_str() {
            "off" => AutoExportFormat::Off,
            "pdf" => AutoExportFormat::Pdf,
            "png" => AutoExportFormat::Png,
            _ => AutoExportFormat::Unknown(text),
        }
    }
}

impl From<AutoExportFormat> for String {
    fn from(format: AutoExportFormat) -> Self {
        match format {
            AutoExportFormat::Off => "off".to_owned(),
            AutoExportFormat::Pdf => "pdf".to_owned(),
            AutoExportFormat::Png => "png".to_owned(),
            AutoExportFormat::Unknown(text) => text,
        }
    }
}

/// Default folder of the automatic exports, relative to the library root.
pub const DEFAULT_AUTO_EXPORT_FOLDER: &str = "Exports";

fn default_auto_export_folder() -> String {
    DEFAULT_AUTO_EXPORT_FOLDER.to_owned()
}

/// Automatic export of each synced note, to read the notes from a phone (SPEC §3.2). Shared by
/// every computer, like the library.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AutoExport {
    #[serde(default)]
    pub format: AutoExportFormat,
    /// Sub-folder of the library, `/`-separated.
    #[serde(default = "default_auto_export_folder")]
    pub folder: String,
    /// Keys this version does not know about, kept verbatim.
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Default for AutoExport {
    fn default() -> Self {
        Self {
            format: AutoExportFormat::Off,
            folder: default_auto_export_folder(),
            extra: toml::Table::new(),
        }
    }
}

impl AutoExport {
    /// The folder as a path relative to the library root, or `None` if it is not acceptable:
    /// empty, with a `.` or `..` part, a drive letter or stream (`:`), or inside the library's
    /// own `_ardoise/` folder. Leading, doubled and trailing separators are ignored.
    pub fn relative_folder(&self) -> Option<PathBuf> {
        let mut path = PathBuf::new();
        for part in self.folder.split(['/', '\\']) {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if part == "." || part == ".." || part.contains(':') {
                return None;
            }
            if path.as_os_str().is_empty()
                && (part.eq_ignore_ascii_case(crate::library::META_DIR)
                    || part.eq_ignore_ascii_case(crate::library::LEGACY_META_DIR))
            {
                return None;
            }
            path.push(part);
        }
        (!path.as_os_str().is_empty()).then_some(path)
    }
}

/// How the tablet is held: the four positions of Wacom Inkspace. The tablet's button and light
/// are on one edge of the body; the position is named after where that edge ends up for the
/// user. Every point the tablet sends is turned by whole quarter turns back to the way up of
/// the page (clockwise, [`quarter_turns`](Self::quarter_turns) times), and a portrait position
/// makes portrait pages (width and height swapped).
///
/// Stored as `"landscape"`, `"landscapeFlipped"`, `"portraitLeft"` or `"portraitRight"`. Files
/// written before the four positions have `hand = "right"` (read as landscape) or
/// `hand = "left"` (the tablet turned half a turn: landscape flipped); any other value loads as
/// landscape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum Orientation {
    /// Held as drawn on the tablet, long side across, button at the bottom. Right-handed.
    #[default]
    Landscape,
    /// Half a turn from landscape, button at the top. Left-handed.
    LandscapeFlipped,
    /// A quarter turn clockwise from landscape: long side up, button on the left.
    PortraitLeft,
    /// A quarter turn anticlockwise from landscape: long side up, button on the right.
    PortraitRight,
}

impl Orientation {
    /// Every position, in the order the interface lists them.
    pub const ALL: [Orientation; 4] = [
        Orientation::Landscape,
        Orientation::LandscapeFlipped,
        Orientation::PortraitLeft,
        Orientation::PortraitRight,
    ];

    /// How many clockwise quarter turns bring the tablet's own coordinates to the page's.
    pub fn quarter_turns(self) -> u8 {
        match self {
            Orientation::Landscape => 0,
            Orientation::PortraitLeft => 1,
            Orientation::LandscapeFlipped => 2,
            Orientation::PortraitRight => 3,
        }
    }

    /// The position `turns` clockwise quarter turns (modulo 4) from landscape.
    pub fn from_quarter_turns(turns: u8) -> Self {
        match turns % 4 {
            0 => Orientation::Landscape,
            1 => Orientation::PortraitLeft,
            2 => Orientation::LandscapeFlipped,
            _ => Orientation::PortraitRight,
        }
    }

    /// Quarter turns (clockwise, 0 to 3) that bring a page held in `from` to this position.
    pub fn turns_from(self, from: Orientation) -> u8 {
        (self.quarter_turns() + 4 - from.quarter_turns()) % 4
    }

    /// The tablet is held long side up.
    pub fn is_portrait(self) -> bool {
        self.quarter_turns() % 2 == 1
    }

    /// Size of the page for a tablet drawing area of `width` × `height` (swapped in portrait).
    pub fn page_size(self, width: u32, height: u32) -> (u32, u32) {
        if self.is_portrait() {
            (height, width)
        } else {
            (width, height)
        }
    }
}

impl From<String> for Orientation {
    fn from(text: String) -> Self {
        match text.as_str() {
            "landscapeFlipped" | "left" => Orientation::LandscapeFlipped,
            "portraitLeft" => Orientation::PortraitLeft,
            "portraitRight" => Orientation::PortraitRight,
            _ => Orientation::Landscape,
        }
    }
}

impl From<Orientation> for String {
    fn from(orientation: Orientation) -> Self {
        match orientation {
            Orientation::Landscape => "landscape",
            Orientation::LandscapeFlipped => "landscapeFlipped",
            Orientation::PortraitLeft => "portraitLeft",
            Orientation::PortraitRight => "portraitRight",
        }
        .to_owned()
    }
}

/// What the tablet's button does in live mode (SPEC §4.2). Stored as `"newPage"` or
/// `"newLayer"`; any other value loads as a new page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum LiveButton {
    #[default]
    NewPage,
    NewLayer,
}

impl From<String> for LiveButton {
    fn from(text: String) -> Self {
        if text == "newLayer" {
            LiveButton::NewLayer
        } else {
            LiveButton::NewPage
        }
    }
}

impl From<LiveButton> for String {
    fn from(button: LiveButton) -> Self {
        match button {
            LiveButton::NewPage => "newPage".to_owned(),
            LiveButton::NewLayer => "newLayer".to_owned(),
        }
    }
}

fn yes() -> bool {
    true
}

/// The tablet's options (SPEC §4.1, §4.2), shared by every computer like the library.
///
/// The orientation is stored under `orientation`, and also under the legacy `hand` key so that an
/// older Ardoise sharing the file keeps the half turn (it writes `hand` next to the `orientation`
/// it does not know). When both keys are present, `orientation` wins; `hand` alone is read as the
/// older files meant it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawTabletSettings", into = "RawTabletSettings")]
pub struct TabletSettings {
    /// Sync the stored pages as soon as the tablet connects.
    pub auto_sync: bool,
    /// Delete each page from the tablet once its note is written and checked.
    pub delete_after_import: bool,
    /// How the tablet is held.
    pub orientation: Orientation,
    /// What the tablet's button does in live mode.
    pub button: LiveButton,
    /// Keys this version does not know about, kept verbatim.
    pub extra: toml::Table,
}

/// The tablet table as stored: the legacy `hand` key next to `orientation`.
#[derive(Serialize, Deserialize)]
struct RawTabletSettings {
    #[serde(default = "yes")]
    auto_sync: bool,
    #[serde(default = "yes")]
    delete_after_import: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    orientation: Option<Orientation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    hand: Option<String>,
    #[serde(default)]
    button: LiveButton,
    #[serde(flatten)]
    extra: toml::Table,
}

impl From<RawTabletSettings> for TabletSettings {
    fn from(raw: RawTabletSettings) -> Self {
        Self {
            auto_sync: raw.auto_sync,
            delete_after_import: raw.delete_after_import,
            orientation: raw
                .orientation
                .or_else(|| raw.hand.map(Orientation::from))
                .unwrap_or_default(),
            button: raw.button,
            extra: raw.extra,
        }
    }
}

impl From<TabletSettings> for RawTabletSettings {
    fn from(settings: TabletSettings) -> Self {
        // An older Ardoise only knows the half turn.
        let hand = if settings.orientation == Orientation::LandscapeFlipped {
            "left"
        } else {
            "right"
        };
        Self {
            auto_sync: settings.auto_sync,
            delete_after_import: settings.delete_after_import,
            orientation: Some(settings.orientation),
            hand: Some(hand.to_owned()),
            button: settings.button,
            extra: settings.extra,
        }
    }
}

impl Default for TabletSettings {
    fn default() -> Self {
        Self {
            auto_sync: true,
            delete_after_import: true,
            orientation: Orientation::Landscape,
            button: LiveButton::NewPage,
            extra: toml::Table::new(),
        }
    }
}

/// Language of the handwriting recognition (SPEC §4.7). Stored as `"auto"`, `"fr"` or `"en"`;
/// any other value (written by a newer version on another PC) loads as
/// [`Unknown`](Self::Unknown), which works as `auto` and is written back verbatim.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum HandwritingLanguage {
    /// The interface language when its recognizer is installed on the PC, else the first
    /// installed one (each PC decides with its own recognizers).
    #[default]
    Auto,
    Fr,
    En,
    /// A value this version does not know, kept as written.
    Unknown(String),
}

impl HandwritingLanguage {
    /// The choice for a program that only knows auto / fr / en: `Auto` for an unknown value.
    pub fn known(&self) -> HandwritingLanguage {
        if matches!(self, HandwritingLanguage::Unknown(_)) {
            HandwritingLanguage::Auto
        } else {
            self.clone()
        }
    }
}

impl From<String> for HandwritingLanguage {
    fn from(text: String) -> Self {
        match text.as_str() {
            "auto" => HandwritingLanguage::Auto,
            "fr" => HandwritingLanguage::Fr,
            "en" => HandwritingLanguage::En,
            _ => HandwritingLanguage::Unknown(text),
        }
    }
}

impl From<HandwritingLanguage> for String {
    fn from(language: HandwritingLanguage) -> Self {
        match language {
            HandwritingLanguage::Auto => "auto".to_owned(),
            HandwritingLanguage::Fr => "fr".to_owned(),
            HandwritingLanguage::En => "en".to_owned(),
            HandwritingLanguage::Unknown(text) => text,
        }
    }
}

/// Handwriting recognition (SPEC §4.7), shared by every computer. Its results are a cache on
/// each PC, never in this file nor in the library.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct HandwritingSettings {
    #[serde(default)]
    pub language: HandwritingLanguage,
    /// Keys this version does not know about, kept verbatim.
    #[serde(flatten)]
    pub extra: toml::Table,
}

/// Port of the share on the local network when `ardoise.toml` does not choose one.
pub const DEFAULT_SHARE_PORT: u16 = 8765;

/// Sharing the sheet on the local network (SPEC §4.10), shared by every computer. The port is
/// fixed so that the firewall's permission given the first time stays valid. A missing, zero or
/// invalid value loads as [`DEFAULT_SHARE_PORT`] (and is written back as such).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "RawShareSettings", into = "RawShareSettings")]
pub struct ShareSettings {
    /// TCP port listened on while sharing.
    pub port: u16,
    /// Keys this version does not know about, kept verbatim.
    pub extra: toml::Table,
}

impl Default for ShareSettings {
    fn default() -> Self {
        Self {
            port: DEFAULT_SHARE_PORT,
            extra: toml::Table::new(),
        }
    }
}

/// The share table as stored: any value under `port`, so that a typing error in the file does
/// not make the whole file unreadable.
#[derive(Serialize, Deserialize)]
struct RawShareSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    port: Option<toml::Value>,
    #[serde(flatten)]
    extra: toml::Table,
}

impl From<RawShareSettings> for ShareSettings {
    fn from(raw: RawShareSettings) -> Self {
        let port = raw
            .port
            .and_then(|value| value.as_integer())
            .and_then(|port| u16::try_from(port).ok())
            .filter(|port| *port != 0)
            .unwrap_or(DEFAULT_SHARE_PORT);
        Self {
            port,
            extra: raw.extra,
        }
    }
}

impl From<ShareSettings> for RawShareSettings {
    fn from(settings: ShareSettings) -> Self {
        Self {
            port: Some(toml::Value::Integer(i64::from(settings.port))),
            extra: settings.extra,
        }
    }
}

/// Settings that only make sense on one computer.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MachineSettings {
    /// Library folder on this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<PathBuf>,
    /// Key of the screen last chosen for the presentation window (`name@x,y`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projector: Option<String>,
    /// Microphone chosen for the narration (the stable id of the input device); the system's
    /// default when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microphone: Option<String>,
    /// The user allows the OpenH264 fallback for MP4 on this computer (SPEC §4.5, §6.2: it is
    /// downloaded on request, and can be turned off and removed).
    #[serde(default, skip_serializing_if = "is_false")]
    pub openh264: bool,
    /// Keys this version does not know about, kept verbatim.
    #[serde(flatten)]
    pub extra: toml::Table,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Contents of `ardoise.toml`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Settings {
    /// Interface language.
    #[serde(default)]
    pub language: Language,
    /// Interface theme.
    #[serde(default)]
    pub theme: Theme,
    /// Automatic export after a sync.
    #[serde(default)]
    pub auto_export: AutoExport,
    /// The tablet's options.
    #[serde(default)]
    pub tablet: TabletSettings,
    /// Handwriting recognition.
    #[serde(default)]
    pub handwriting: HandwritingSettings,
    /// Sharing the sheet on the local network.
    #[serde(default)]
    pub share: ShareSettings,
    /// Per-computer settings, keyed by machine name.
    #[serde(default)]
    pub machines: BTreeMap<String, MachineSettings>,
    /// Keys this version does not know about, kept verbatim.
    #[serde(flatten)]
    pub extra: toml::Table,
}

impl Settings {
    /// Library folder configured for `machine`, if any.
    pub fn library_for(&self, machine: &str) -> Option<&Path> {
        self.machines.get(machine)?.library.as_deref()
    }

    /// Sets the library folder of `machine`, creating its section if needed.
    pub fn set_library(&mut self, machine: &str, path: PathBuf) {
        self.machines.entry(machine.to_owned()).or_default().library = Some(path);
    }

    /// Microphone chosen on `machine`, if any.
    pub fn microphone_for(&self, machine: &str) -> Option<&str> {
        self.machines.get(machine)?.microphone.as_deref()
    }

    /// Sets the microphone of `machine` (`None`: the system's default). A machine without a
    /// section gets one only for a microphone.
    pub fn set_microphone(&mut self, machine: &str, microphone: Option<String>) {
        match self.machines.get_mut(machine) {
            Some(settings) => settings.microphone = microphone,
            None if microphone.is_some() => {
                self.machines
                    .entry(machine.to_owned())
                    .or_default()
                    .microphone = microphone;
            }
            None => {}
        }
    }

    /// Whether the OpenH264 fallback is allowed on `machine` (not by default).
    pub fn openh264_allowed(&self, machine: &str) -> bool {
        self.machines.get(machine).is_some_and(|m| m.openh264)
    }

    /// Allows or disallows the OpenH264 fallback on `machine`. A machine without a section gets
    /// one only to allow it.
    pub fn set_openh264_allowed(&mut self, machine: &str, allowed: bool) {
        match self.machines.get_mut(machine) {
            Some(settings) => settings.openh264 = allowed,
            None if allowed => {
                self.machines
                    .entry(machine.to_owned())
                    .or_default()
                    .openh264 = true;
            }
            None => {}
        }
    }
}

/// Failure while reading or writing the settings file.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    /// The file or its folder could not be read or written.
    #[error("cannot access settings at {}: {source}", path.display())]
    Io {
        /// Path involved in the failure.
        path: PathBuf,
        /// Underlying error.
        source: io::Error,
    },
    /// The file is not valid settings TOML. It is left untouched.
    #[error("cannot parse settings at {}: {message}", path.display())]
    Parse {
        /// Path of the unreadable file.
        path: PathBuf,
        /// Parser diagnostic.
        message: String,
    },
    /// The settings could not be turned into TOML.
    #[error("cannot serialize settings: {0}")]
    Serialize(String),
}

/// Handle on the settings file of the current computer.
#[derive(Debug, Clone)]
pub struct SettingsStore {
    path: PathBuf,
    machine: String,
}

impl SettingsStore {
    /// Name of the settings file.
    pub const FILE_NAME: &'static str = "ardoise.toml";

    /// Chooses where the settings file lives. Rules, in order:
    ///
    /// 1. `exe_dir/ardoise.toml` if it exists;
    /// 2. `appdata_dir/Ardoise/ardoise.toml` if it exists;
    /// 3. `exe_dir/ardoise.toml` if `exe_dir` is writable;
    /// 4. `appdata_dir/Ardoise/ardoise.toml`.
    ///
    /// Nothing is created: the file only appears on the first [`update`](Self::update).
    pub fn locate(exe_dir: &Path, appdata_dir: &Path, machine: impl Into<String>) -> SettingsStore {
        let portable = exe_dir.join(Self::FILE_NAME);
        let roaming = appdata_dir.join("Ardoise").join(Self::FILE_NAME);
        let path = if portable.is_file() {
            portable
        } else if roaming.is_file() || !is_writable_dir(exe_dir) {
            roaming
        } else {
            portable
        };
        SettingsStore {
            path,
            machine: machine.into(),
        }
    }

    /// Locates the settings for the running app: the folder of the current executable,
    /// `%APPDATA%`, and the computer name.
    pub fn open_default() -> Result<SettingsStore, SettingsError> {
        let exe = std::env::current_exe().map_err(|source| SettingsError::Io {
            path: PathBuf::from("<current executable>"),
            source,
        })?;
        let exe_dir = exe.parent().ok_or_else(|| SettingsError::Io {
            path: exe.clone(),
            source: io::Error::new(io::ErrorKind::NotFound, "executable has no parent folder"),
        })?;
        let appdata = std::env::var_os("APPDATA")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| SettingsError::Io {
                path: PathBuf::from("%APPDATA%"),
                source: io::Error::new(io::ErrorKind::NotFound, "APPDATA is not set"),
            })?;
        Ok(Self::locate(exe_dir, &appdata, machine_name()))
    }

    /// Path of the settings file (which may not exist yet).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Name of the current computer, the key of its `[machines]` section.
    pub fn machine(&self) -> &str {
        &self.machine
    }

    /// Reads the settings. A missing file yields the defaults; an invalid one is a
    /// [`SettingsError::Parse`] and is never overwritten.
    pub fn load(&self) -> Result<Settings, SettingsError> {
        let text = match fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Settings::default()),
            Err(source) => {
                return Err(SettingsError::Io {
                    path: self.path.clone(),
                    source,
                });
            }
        };
        toml::from_str(&text).map_err(|e| SettingsError::Parse {
            path: self.path.clone(),
            message: e.to_string(),
        })
    }

    /// Applies `f` to the settings and saves them atomically, returning the new value.
    ///
    /// The file is re-read first, so sections written meanwhile by another PC (through
    /// Nextcloud) are kept. The parent folder is created if needed.
    pub fn update(&self, f: impl FnOnce(&mut Settings)) -> Result<Settings, SettingsError> {
        let mut settings = self.load()?;
        f(&mut settings);
        // Going through `toml::Table` puts plain values before sub-tables, whatever the
        // order of the flattened fields.
        let table = toml::Table::try_from(&settings)
            .map_err(|e| SettingsError::Serialize(e.to_string()))?;
        let text =
            toml::to_string_pretty(&table).map_err(|e| SettingsError::Serialize(e.to_string()))?;
        self.write(text.as_bytes())?;
        Ok(settings)
    }

    fn write(&self, bytes: &[u8]) -> Result<(), SettingsError> {
        let io_err = |source| SettingsError::Io {
            path: self.path.clone(),
            source,
        };
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(io_err)?;
        }
        write_atomic(&self.path, bytes).map_err(io_err)
    }
}

/// Probes whether files can be created in `dir` by creating and deleting a temporary one.
fn is_writable_dir(dir: &Path) -> bool {
    let probe = dir.join(format!(".ardoise-write-test-{}.part", std::process::id()));
    match OpenOptions::new().write(true).create_new(true).open(&probe) {
        Ok(file) => {
            drop(file);
            let _ = fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

/// Name of this computer: `%COMPUTERNAME%` on Windows, else `HOSTNAME`, else `"unknown"`.
pub fn machine_name() -> String {
    ["COMPUTERNAME", "HOSTNAME"]
        .iter()
        .filter_map(|var| std::env::var(var).ok())
        .map(|v| v.trim().to_owned())
        .find(|v| !v.is_empty())
        .unwrap_or_else(|| "unknown".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_in(dir: &Path) -> SettingsStore {
        SettingsStore::locate(dir, dir, "PC-A")
    }

    #[test]
    fn missing_file_yields_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        assert_eq!(store.load().unwrap(), Settings::default());
        assert!(!store.path().exists(), "load must not create the file");
    }

    #[test]
    fn round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let saved = store
            .update(|s| {
                s.language = Language::Fr;
                s.theme = Theme::Dark;
                s.set_library("PC-A", PathBuf::from(r"C:\Users\prof\Nextcloud\Ardoise"));
                s.set_library("PC-B", PathBuf::from(r"D:\Nextcloud\Ardoise"));
            })
            .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded, saved);
        assert_eq!(loaded.language, Language::Fr);
        assert_eq!(loaded.theme, Theme::Dark);
        assert_eq!(
            loaded.library_for("PC-B"),
            Some(Path::new(r"D:\Nextcloud\Ardoise"))
        );
    }

    #[test]
    fn parses_documented_format() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        fs::write(
            store.path(),
            "language = \"auto\"\ntheme = \"system\"\n\n[machines.\"PC-MAISON\"]\nlibrary = 'C:\\Users\\prof\\Nextcloud\\Ardoise'\n",
        )
        .unwrap();
        let settings = store.load().unwrap();
        assert_eq!(settings.language, Language::Auto);
        assert_eq!(
            settings.library_for("PC-MAISON"),
            Some(Path::new(r"C:\Users\prof\Nextcloud\Ardoise"))
        );
        assert_eq!(settings.library_for("PC-BOULOT"), None);
    }

    #[test]
    fn partial_file_uses_defaults_for_missing_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        fs::write(store.path(), "theme = \"light\"\n").unwrap();
        let settings = store.load().unwrap();
        assert_eq!(settings.theme, Theme::Light);
        assert_eq!(settings.language, Language::Auto);
        assert!(settings.machines.is_empty());
    }

    #[test]
    fn unknown_keys_survive_update() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        fs::write(
            store.path(),
            "future_flag = true\n\n[future_section]\nanswer = 42\n\n[machines.\"PC-A\"]\nlibrary = 'C:\\Lib'\nmonitor = 2\n\n[machines.\"PC-A\".pen]\nsmoothing = 0.5\n",
        )
        .unwrap();

        let updated = store
            .update(|s| {
                s.theme = Theme::Dark;
                s.set_library("PC-A", PathBuf::from(r"E:\Lib"));
            })
            .unwrap();
        assert_eq!(updated.theme, Theme::Dark);

        let reloaded = store.load().unwrap();
        assert_eq!(reloaded.extra["future_flag"], toml::Value::Boolean(true));
        assert_eq!(
            reloaded.extra["future_section"]["answer"].as_integer(),
            Some(42)
        );
        let machine = &reloaded.machines["PC-A"];
        assert_eq!(machine.library.as_deref(), Some(Path::new(r"E:\Lib")));
        assert_eq!(machine.extra["monitor"].as_integer(), Some(2));
        assert_eq!(machine.extra["pen"]["smoothing"].as_float(), Some(0.5));
    }

    #[test]
    fn update_keeps_section_written_by_another_pc_after_load() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        store
            .update(|s| s.set_library("PC-A", PathBuf::from(r"C:\A")))
            .unwrap();
        let stale = store.load().unwrap();
        assert_eq!(stale.machines.len(), 1);

        // Another PC writes its section and Nextcloud syncs it down.
        let other = SettingsStore::locate(dir.path(), dir.path(), "PC-B");
        other
            .update(|s| s.set_library("PC-B", PathBuf::from(r"D:\B")))
            .unwrap();

        let result = store.update(|s| s.theme = Theme::Light).unwrap();
        assert_eq!(result.library_for("PC-A"), Some(Path::new(r"C:\A")));
        assert_eq!(result.library_for("PC-B"), Some(Path::new(r"D:\B")));
        assert_eq!(store.load().unwrap(), result);
    }

    #[test]
    fn invalid_toml_is_a_parse_error_and_file_is_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let garbage = "language = [this is not toml";
        fs::write(store.path(), garbage).unwrap();

        assert!(matches!(
            store.load(),
            Err(SettingsError::Parse { ref path, .. }) if path == store.path()
        ));
        assert!(matches!(
            store.update(|s| s.theme = Theme::Dark),
            Err(SettingsError::Parse { .. })
        ));
        assert_eq!(fs::read_to_string(store.path()).unwrap(), garbage);
    }

    #[test]
    fn wrong_value_type_is_a_parse_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        fs::write(store.path(), "language = \"klingon\"\n").unwrap();
        assert!(matches!(store.load(), Err(SettingsError::Parse { .. })));
    }

    #[test]
    fn update_creates_missing_parent_folder() {
        let dir = tempfile::tempdir().unwrap();
        let appdata = dir.path().join("Roaming");
        let blocked = dir.path().join("file");
        fs::write(&blocked, b"").unwrap();
        // exe_dir is not writable, so the file goes under appdata/Ardoise, which does not exist.
        let store = SettingsStore::locate(&blocked.join("exe"), &appdata, "PC-A");
        store.update(|s| s.language = Language::En).unwrap();
        assert!(appdata.join("Ardoise").join("ardoise.toml").is_file());
    }

    #[test]
    fn locate_prefers_existing_file_next_to_exe() {
        let exe = tempfile::tempdir().unwrap();
        let appdata = tempfile::tempdir().unwrap();
        fs::write(exe.path().join("ardoise.toml"), "").unwrap();
        fs::create_dir(appdata.path().join("Ardoise")).unwrap();
        fs::write(appdata.path().join("Ardoise").join("ardoise.toml"), "").unwrap();
        let store = SettingsStore::locate(exe.path(), appdata.path(), "PC-A");
        assert_eq!(store.path(), exe.path().join("ardoise.toml"));
        assert_eq!(store.machine(), "PC-A");
    }

    #[test]
    fn locate_uses_existing_appdata_file_when_none_next_to_exe() {
        let exe = tempfile::tempdir().unwrap();
        let appdata = tempfile::tempdir().unwrap();
        fs::create_dir(appdata.path().join("Ardoise")).unwrap();
        fs::write(appdata.path().join("Ardoise").join("ardoise.toml"), "").unwrap();
        // The exe folder is writable, but an existing roaming file still wins.
        let store = SettingsStore::locate(exe.path(), appdata.path(), "PC-A");
        assert_eq!(
            store.path(),
            appdata.path().join("Ardoise").join("ardoise.toml")
        );
    }

    #[test]
    fn locate_uses_writable_exe_dir_when_no_file_exists() {
        let exe = tempfile::tempdir().unwrap();
        let appdata = tempfile::tempdir().unwrap();
        let store = SettingsStore::locate(exe.path(), appdata.path(), "PC-A");
        assert_eq!(store.path(), exe.path().join("ardoise.toml"));
        // The writability probe leaves nothing behind.
        assert_eq!(fs::read_dir(exe.path()).unwrap().count(), 0);
    }

    #[test]
    fn locate_falls_back_to_appdata_when_exe_dir_not_writable() {
        let tmp = tempfile::tempdir().unwrap();
        let appdata = tmp.path().join("appdata");
        let blocker = tmp.path().join("blocker");
        fs::write(&blocker, b"not a directory").unwrap();
        let exe = blocker.join("bin");
        let store = SettingsStore::locate(&exe, &appdata, "PC-A");
        assert_eq!(store.path(), appdata.join("Ardoise").join("ardoise.toml"));
    }

    #[test]
    fn library_accessors() {
        let mut settings = Settings::default();
        assert_eq!(settings.library_for("PC-A"), None);
        settings.set_library("PC-A", PathBuf::from(r"C:\One"));
        settings.set_library("PC-A", PathBuf::from(r"C:\Two"));
        settings.set_library("PC-B", PathBuf::from(r"D:\Three"));
        assert_eq!(settings.library_for("PC-A"), Some(Path::new(r"C:\Two")));
        assert_eq!(settings.library_for("PC-B"), Some(Path::new(r"D:\Three")));
        assert_eq!(settings.library_for("PC-C"), None);
    }

    #[test]
    fn machine_name_is_never_empty() {
        assert!(!machine_name().is_empty());
    }

    #[test]
    fn auto_export_folders_stay_inside_the_library() {
        let folder = |text: &str| {
            AutoExport {
                folder: text.into(),
                ..AutoExport::default()
            }
            .relative_folder()
        };
        assert_eq!(folder("Exports"), Some(PathBuf::from("Exports")));
        assert_eq!(
            folder("/Exports\\PDF/"),
            Some(PathBuf::from("Exports").join("PDF"))
        );
        for bad in [
            "",
            " / ",
            "..",
            "a/../b",
            "C:/x",
            "_ardoise",
            "_Ardoise/x",
            ".ardoise",
        ] {
            assert_eq!(folder(bad), None, "{bad}");
        }
        assert_eq!(AutoExport::default().format, AutoExportFormat::Off);
    }

    #[test]
    fn auto_export_is_off_by_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        for text in [None, Some("theme = \"light\"\n"), Some("[auto_export]\n")] {
            if let Some(text) = text {
                fs::write(store.path(), text).unwrap();
            }
            let settings = store.load().unwrap();
            assert_eq!(settings.auto_export.format, AutoExportFormat::Off);
            assert_eq!(settings.auto_export.folder, DEFAULT_AUTO_EXPORT_FOLDER);
            assert_eq!(settings.auto_export, AutoExport::default());
        }
    }

    #[test]
    fn auto_export_values_are_read() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        for (text, format) in [
            ("off", AutoExportFormat::Off),
            ("pdf", AutoExportFormat::Pdf),
            ("png", AutoExportFormat::Png),
        ] {
            fs::write(
                store.path(),
                format!("[auto_export]\nformat = \"{text}\"\nfolder = \"Lecture/Tel\"\n"),
            )
            .unwrap();
            let auto = store.load().unwrap().auto_export;
            assert_eq!(auto.format, format);
            assert_eq!(auto.folder, "Lecture/Tel");
            assert_eq!(
                auto.relative_folder(),
                Some(Path::new("Lecture").join("Tel"))
            );
        }
        // Only the format given: the folder keeps its default.
        fs::write(store.path(), "[auto_export]\nformat = \"pdf\"\n").unwrap();
        let auto = store.load().unwrap().auto_export;
        assert_eq!(auto.format, AutoExportFormat::Pdf);
        assert_eq!(auto.folder, DEFAULT_AUTO_EXPORT_FOLDER);
    }

    #[test]
    fn an_unknown_auto_export_format_loads_as_off_and_is_written_back_verbatim() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        // A newer version on another PC wrote a format this one does not know.
        fs::write(
            store.path(),
            "[auto_export]\nformat = \"jpg\"\nfolder = \"Tel\"\n",
        )
        .unwrap();
        let auto = store.load().unwrap().auto_export;
        assert_eq!(auto.format, AutoExportFormat::Unknown("jpg".into()));
        assert!(!auto.format.is_active());
        assert_eq!(auto.format.known(), AutoExportFormat::Off);
        assert_eq!(auto.folder, "Tel");

        // Another setting changed: the value is still there.
        store.update(|s| s.theme = Theme::Dark).unwrap();
        let text = fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("format = \"jpg\""), "{text}");
        // Known formats are active.
        assert!(AutoExportFormat::Pdf.is_active() && AutoExportFormat::Png.is_active());
        assert!(!AutoExportFormat::Off.is_active());
    }

    #[test]
    fn microphone_and_openh264_are_per_machine_and_kept_with_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        fs::write(
            store.path(),
            "[machines.\"PC-A\"]\nlibrary = 'C:\\Lib'\nfuture = 3\n\n[machines.\"PC-B\"]\nmicrophone = \"wasapi:B\"\n",
        )
        .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(loaded.microphone_for("PC-A"), None);
        assert!(!loaded.openh264_allowed("PC-A"));
        assert_eq!(loaded.microphone_for("PC-B"), Some("wasapi:B"));

        let updated = store
            .update(|s| {
                s.set_microphone("PC-A", Some("wasapi:{0.0.1.00000000}.{abc}".into()));
                s.set_openh264_allowed("PC-A", true);
            })
            .unwrap();
        let reloaded = store.load().unwrap();
        assert_eq!(reloaded, updated);
        assert_eq!(
            reloaded.microphone_for("PC-A"),
            Some("wasapi:{0.0.1.00000000}.{abc}")
        );
        assert!(reloaded.openh264_allowed("PC-A"));
        let machine = &reloaded.machines["PC-A"];
        assert_eq!(machine.library.as_deref(), Some(Path::new(r"C:\Lib")));
        assert_eq!(machine.extra["future"].as_integer(), Some(3));
        // The other PC keeps its own choice and does not get the fallback.
        assert_eq!(reloaded.microphone_for("PC-B"), Some("wasapi:B"));
        assert!(!reloaded.openh264_allowed("PC-B"));
        let text = fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("openh264 = true"), "{text}");

        // Back to the defaults: the keys go away, the rest stays.
        store
            .update(|s| {
                s.set_microphone("PC-A", None);
                s.set_openh264_allowed("PC-A", false);
            })
            .unwrap();
        let text = fs::read_to_string(store.path()).unwrap();
        assert!(!text.contains("openh264"), "{text}");
        assert!(!text.contains("wasapi:{"), "{text}");
        assert!(text.contains("future = 3"), "{text}");
        let reloaded = store.load().unwrap();
        assert_eq!(reloaded.microphone_for("PC-A"), None);
        assert!(!reloaded.openh264_allowed("PC-A"));
    }

    #[test]
    fn defaults_do_not_create_a_machine_section() {
        let mut settings = Settings::default();
        settings.set_microphone("PC-A", None);
        settings.set_openh264_allowed("PC-A", false);
        assert!(settings.machines.is_empty());
        settings.set_openh264_allowed("PC-A", true);
        assert!(settings.openh264_allowed("PC-A"));
        settings.set_microphone("PC-C", Some("Micro USB".into()));
        assert_eq!(settings.microphone_for("PC-C"), Some("Micro USB"));
        assert_eq!(settings.machines.len(), 2);
    }

    #[test]
    fn the_handwriting_language_is_shared_and_keeps_unknown_values() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        assert_eq!(
            store.load().unwrap().handwriting.language,
            HandwritingLanguage::Auto
        );
        store
            .update(|s| s.handwriting.language = HandwritingLanguage::En)
            .unwrap();
        let text = fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("[handwriting]"), "{text}");
        assert!(text.contains("language = \"en\""), "{text}");
        // Not per machine: another PC reads the same choice.
        let other = SettingsStore::locate(dir.path(), dir.path(), "PC-B");
        assert_eq!(
            other.load().unwrap().handwriting.language,
            HandwritingLanguage::En
        );

        // A value of a newer version works as auto and survives another change.
        fs::write(
            store.path(),
            "[handwriting]\nlanguage = \"de\"\nengine = \"x\"\n",
        )
        .unwrap();
        let loaded = store.load().unwrap();
        assert_eq!(
            loaded.handwriting.language,
            HandwritingLanguage::Unknown("de".into())
        );
        assert_eq!(
            loaded.handwriting.language.known(),
            HandwritingLanguage::Auto
        );
        store.update(|s| s.theme = Theme::Dark).unwrap();
        let text = fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("language = \"de\""), "{text}");
        assert!(text.contains("engine = \"x\""), "{text}");
        assert_eq!(HandwritingLanguage::Fr.known(), HandwritingLanguage::Fr);
    }

    #[test]
    fn the_share_port_is_shared_defaults_to_8765_and_keeps_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        assert_eq!(store.load().unwrap().share.port, DEFAULT_SHARE_PORT);
        assert_eq!(DEFAULT_SHARE_PORT, 8765);

        fs::write(store.path(), "[share]\nport = 18765\nfuture = \"x\"\n").unwrap();
        // Not per machine: another PC reads the same port.
        let other = SettingsStore::locate(dir.path(), dir.path(), "PC-B");
        assert_eq!(other.load().unwrap().share.port, 18765);
        store.update(|s| s.theme = Theme::Dark).unwrap();
        let text = fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("[share]"), "{text}");
        assert!(text.contains("port = 18765"), "{text}");
        assert!(text.contains("future = \"x\""), "{text}");

        // A zero, out of range or mistyped port loads as the default, and the rest of the file
        // is still read.
        for wrong in ["0", "70000", "-1", "\"huit\"", "87.65"] {
            fs::write(
                store.path(),
                format!("theme = \"dark\"\n\n[share]\nport = {wrong}\n"),
            )
            .unwrap();
            let loaded = store.load().unwrap();
            assert_eq!(loaded.share.port, DEFAULT_SHARE_PORT, "{wrong}");
            assert_eq!(loaded.theme, Theme::Dark, "{wrong}");
        }
    }

    #[test]
    fn auto_export_round_trip_keeps_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        fs::write(
            store.path(),
            "future_flag = true\n\n[auto_export]\nformat = \"png\"\nfolder = \"Exports\"\nquality = 90\n\n[auto_export.future]\nanswer = 42\n",
        )
        .unwrap();

        let updated = store
            .update(|s| {
                s.auto_export.format = AutoExportFormat::Pdf;
                s.auto_export.folder = "Lecture".to_owned();
            })
            .unwrap();
        assert_eq!(updated.auto_export.format, AutoExportFormat::Pdf);

        let reloaded = store.load().unwrap();
        assert_eq!(reloaded, updated);
        assert_eq!(reloaded.auto_export.format, AutoExportFormat::Pdf);
        assert_eq!(reloaded.auto_export.folder, "Lecture");
        assert_eq!(reloaded.auto_export.extra["quality"].as_integer(), Some(90));
        assert_eq!(
            reloaded.auto_export.extra["future"]["answer"].as_integer(),
            Some(42)
        );
        assert_eq!(reloaded.extra["future_flag"], toml::Value::Boolean(true));

        // Written as the documented table, with lowercase values.
        let text = fs::read_to_string(store.path()).unwrap();
        assert!(text.contains("[auto_export]"), "{text}");
        assert!(text.contains("format = \"pdf\""), "{text}");
        assert!(text.contains("folder = \"Lecture\""), "{text}");

        // Another setting changed later leaves the automatic export alone.
        let later = store.update(|s| s.theme = Theme::Dark).unwrap();
        assert_eq!(later.auto_export, reloaded.auto_export);
    }

    #[test]
    fn the_hand_of_older_files_becomes_an_orientation() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let orientation = |toml: &str| {
            fs::write(store.path(), toml).unwrap();
            store.load().unwrap().tablet.orientation
        };
        assert_eq!(orientation(""), Orientation::Landscape);
        assert_eq!(
            orientation(
                "[tablet]
hand = \"right\"
"
            ),
            Orientation::Landscape
        );
        assert_eq!(
            orientation(
                "[tablet]
hand = \"left\"
"
            ),
            Orientation::LandscapeFlipped
        );
        assert_eq!(
            orientation(
                "[tablet]
hand = \"both\"
"
            ),
            Orientation::Landscape
        );
        for known in Orientation::ALL {
            let text = format!(
                "[tablet]
orientation = \"{}\"
",
                String::from(known)
            );
            assert_eq!(orientation(&text), known);
        }

        // An older file is read, then written with both keys; the other keys stay.
        fs::write(
            store.path(),
            "[tablet]
hand = \"left\"
button = \"newLayer\"
future = 1
",
        )
        .unwrap();
        let updated = store.update(|s| s.theme = Theme::Dark).unwrap();
        assert_eq!(updated.tablet.orientation, Orientation::LandscapeFlipped);
        let text = fs::read_to_string(store.path()).unwrap();
        assert!(
            text.contains("orientation = \"landscapeFlipped\""),
            "{text}"
        );
        assert!(text.contains("hand = \"left\""), "{text}");
        assert!(text.contains("future = 1"), "{text}");
        assert_eq!(
            store.load().unwrap().tablet.orientation,
            Orientation::LandscapeFlipped
        );
    }

    #[test]
    fn both_orientation_keys_load_and_an_older_reader_still_accepts_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let store = store_in(dir.path());
        let orientation = |toml: &str| {
            fs::write(store.path(), toml).unwrap();
            store.load().unwrap().tablet.orientation
        };
        // An older exe shared the file: its `hand` sits next to our `orientation`; ours wins.
        assert_eq!(
            orientation(
                "[tablet]
orientation = \"portraitLeft\"
hand = \"right\"
"
            ),
            Orientation::PortraitLeft
        );
        assert_eq!(
            orientation(
                "[tablet]
hand = \"left\"
orientation = \"landscape\"
"
            ),
            Orientation::Landscape
        );
        assert_eq!(
            orientation(
                "[tablet]
hand = \"left\"
"
            ),
            Orientation::LandscapeFlipped
        );
        assert_eq!(
            orientation(
                "[tablet]
orientation = \"portraitRight\"
"
            ),
            Orientation::PortraitRight
        );

        // A file with both keys keeps the other settings (the library path of `[machines]`).
        fs::write(
            store.path(),
            "[tablet]
orientation = \"portraitLeft\"
hand = \"right\"
button = \"newLayer\"
",
        )
        .unwrap();
        let updated = store.update(|s| s.theme = Theme::Dark).unwrap();
        assert_eq!(updated.tablet.orientation, Orientation::PortraitLeft);
        assert_eq!(updated.tablet.button, LiveButton::NewLayer);

        // What we write is read by a struct that only knows `hand` and keeps the rest.
        #[derive(Deserialize)]
        struct OlderTablet {
            hand: String,
            #[serde(flatten)]
            rest: toml::Table,
        }
        #[derive(Deserialize)]
        struct OlderFile {
            tablet: OlderTablet,
        }
        for (held, hand) in [
            (Orientation::Landscape, "right"),
            (Orientation::LandscapeFlipped, "left"),
            (Orientation::PortraitLeft, "right"),
            (Orientation::PortraitRight, "right"),
        ] {
            store.update(|s| s.tablet.orientation = held).unwrap();
            let text = fs::read_to_string(store.path()).unwrap();
            let older: OlderFile = toml::from_str(&text).unwrap();
            assert_eq!(older.tablet.hand, hand, "{text}");
            assert!(older.tablet.rest.contains_key("orientation"), "{text}");
            assert_eq!(store.load().unwrap().tablet.orientation, held);
        }
    }

    #[test]
    fn orientations_turn_pages_by_quarter_turns() {
        assert_eq!(Orientation::default(), Orientation::Landscape);
        for (i, orientation) in Orientation::ALL.into_iter().enumerate() {
            assert_eq!(
                Orientation::from_quarter_turns(orientation.quarter_turns()),
                orientation,
                "{i}"
            );
            assert_eq!(orientation.turns_from(orientation), 0);
            assert_eq!(
                orientation.is_portrait(),
                orientation.quarter_turns() % 2 == 1
            );
        }
        assert_eq!(
            Orientation::Landscape.page_size(29_700, 21_600),
            (29_700, 21_600)
        );
        assert_eq!(
            Orientation::LandscapeFlipped.page_size(29_700, 21_600),
            (29_700, 21_600)
        );
        assert_eq!(
            Orientation::PortraitLeft.page_size(29_700, 21_600),
            (21_600, 29_700)
        );
        assert_eq!(
            Orientation::PortraitRight.page_size(29_700, 21_600),
            (21_600, 29_700)
        );
        assert_eq!(
            Orientation::PortraitRight.turns_from(Orientation::Landscape),
            3
        );
        assert_eq!(
            Orientation::Landscape.turns_from(Orientation::PortraitRight),
            1
        );
        assert_eq!(
            Orientation::LandscapeFlipped.turns_from(Orientation::PortraitLeft),
            1
        );
    }
}
