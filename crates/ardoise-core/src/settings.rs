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

/// File format of the automatic export after a sync (SPEC §3.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AutoExportFormat {
    /// No automatic export.
    #[default]
    Off,
    /// One PDF per synced note.
    Pdf,
    /// One PNG per synced note.
    Png,
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

/// Settings that only make sense on one computer.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct MachineSettings {
    /// Library folder on this computer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub library: Option<PathBuf>,
    /// Keys this version does not know about, kept verbatim.
    #[serde(flatten)]
    pub extra: toml::Table,
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
        // A format this version does not know is an error, never silently "off".
        fs::write(store.path(), "[auto_export]\nformat = \"docx\"\n").unwrap();
        assert!(matches!(store.load(), Err(SettingsError::Parse { .. })));
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
}
