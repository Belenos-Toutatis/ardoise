//! Commands called by the interface (see `app/ui/src/lib/api.ts`).

use std::path::{Path, PathBuf};

use ardoise_core::library::{self, NOTE_EXTENSION};
use ardoise_core::settings::{
    AutoExport, AutoExportFormat, Language, Settings, SettingsError, SettingsStore, Theme,
};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::watcher::LibraryWatch;

use crate::AppContext;
use crate::library::{library_of, slash_path};
use crate::selftest::{self, UiReport};

/// Error sent to the interface: `code` selects a translated message, `message` is the
/// technical detail shown when no translation exists.
#[derive(Debug, Serialize)]
pub struct CommandError {
    code: &'static str,
    message: String,
}

impl CommandError {
    pub(crate) fn new(code: &'static str, message: impl ToString) -> Self {
        CommandError {
            code,
            message: message.to_string(),
        }
    }

    /// The error's stable identifier (`errorCodes.*`).
    #[cfg(test)]
    pub(crate) fn code(&self) -> &'static str {
        self.code
    }

    /// Whether this is the error `code`.
    pub(crate) fn is_code(&self, code: &str) -> bool {
        self.code == code
    }

    /// The identifier and the detail, for the errors of the MCP server's tools.
    pub(crate) fn parts(&self) -> (&'static str, &str) {
        (self.code, &self.message)
    }
}

impl From<SettingsError> for CommandError {
    fn from(error: SettingsError) -> Self {
        CommandError::new("settings", error)
    }
}

pub(crate) type CommandResult<T> = Result<T, CommandError>;

/// `AutoExportSettings` in `note.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct AutoExportState {
    /// Only off / pdf / png: a value written by a newer version is reported as `off`.
    pub format: AutoExportFormat,
    /// As stored, `/`-separated.
    pub folder: String,
}

/// Everything the interface needs to draw its first screen.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    version: &'static str,
    machine: String,
    settings_path: String,
    settings_error: Option<String>,
    language: Language,
    theme: Theme,
    library: Option<String>,
    system_locale: Option<String>,
    self_test: bool,
    open_diagnostic: bool,
    auto_export: AutoExportState,
    /// Microphone chosen on this computer for the narration (`None`: the system's default).
    microphone: Option<String>,
}

impl AppState {
    pub fn machine(&self) -> &str {
        &self.machine
    }

    pub fn settings_path(&self) -> &str {
        &self.settings_path
    }

    pub fn settings_error(&self) -> Option<&str> {
        self.settings_error.as_deref()
    }

    pub fn language(&self) -> Language {
        self.language
    }

    pub fn library(&self) -> Option<&str> {
        self.library.as_deref()
    }

    pub fn auto_export(&self) -> &AutoExportState {
        &self.auto_export
    }

    #[cfg(test)]
    pub fn microphone(&self) -> Option<&str> {
        self.microphone.as_deref()
    }
}

impl AutoExportState {
    /// Whether notes are exported automatically.
    pub fn is_active(&self) -> bool {
        self.format.is_active()
    }

    /// The folder relative to the library root as `a/b`, or `None` if the stored value is not
    /// acceptable (a hand-edited settings file).
    pub fn valid_folder(&self) -> Option<String> {
        let auto_export = AutoExport {
            folder: self.folder.clone(),
            ..AutoExport::default()
        };
        auto_export.relative_folder().map(|path| slash_path(&path))
    }
}

pub(crate) fn store(context: &AppContext) -> CommandResult<&SettingsStore> {
    context
        .store
        .as_ref()
        .map_err(|message| CommandError::new("settings", message))
}

fn state_from(context: &AppContext, loaded: Result<Settings, SettingsError>) -> AppState {
    let (machine, settings_path) = match &context.store {
        Ok(store) => (
            store.machine().to_owned(),
            store.path().display().to_string(),
        ),
        Err(_) => (ardoise_core::settings::machine_name(), String::new()),
    };
    let (settings, settings_error) = match (&context.store, loaded) {
        (Err(message), _) => (Settings::default(), Some(message.clone())),
        (Ok(_), Ok(settings)) => (settings, None),
        (Ok(_), Err(SettingsError::Parse { message, .. })) => (Settings::default(), Some(message)),
        (Ok(_), Err(error)) => (Settings::default(), Some(error.to_string())),
    };
    AppState {
        version: env!("CARGO_PKG_VERSION"),
        library: settings
            .library_for(&machine)
            .map(|p| p.display().to_string()),
        microphone: settings.microphone_for(&machine).map(str::to_owned),
        machine,
        settings_path,
        settings_error,
        language: settings.language,
        theme: settings.theme,
        system_locale: context.system_locale.clone(),
        self_test: context.self_test.is_some(),
        open_diagnostic: context.open_diagnostic,
        auto_export: AutoExportState {
            format: settings.auto_export.format.known(),
            folder: settings.auto_export.folder,
        },
    }
}

/// Current settings for this computer, read fresh from disk.
pub fn current_state(context: &AppContext) -> AppState {
    let loaded = match &context.store {
        Ok(store) => store.load(),
        Err(_) => Ok(Settings::default()),
    };
    state_from(context, loaded)
}

pub(crate) fn update(
    context: &AppContext,
    change: impl FnOnce(&mut Settings),
) -> CommandResult<AppState> {
    let updated = store(context)?.update(change)?;
    Ok(state_from(context, Ok(updated)))
}

#[tauri::command]
pub fn app_state(context: State<'_, AppContext>) -> AppState {
    current_state(&context)
}

#[tauri::command]
pub fn set_language(
    app: AppHandle,
    context: State<'_, AppContext>,
    language: Language,
) -> CommandResult<AppState> {
    let state = update(&context, |settings| settings.language = language)?;
    // An `auto` handwriting language follows the interface's.
    crate::handwriting::refresh(&app);
    Ok(state)
}

#[tauri::command]
pub fn set_theme(context: State<'_, AppContext>, theme: Theme) -> CommandResult<AppState> {
    update(&context, |settings| settings.theme = theme)
}

/// Whether `dir` or a folder below it holds a note file.
fn holds_notes(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let path = entry.path();
        if path.is_dir() {
            holds_notes(&path)
        } else {
            path.extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(NOTE_EXTENSION))
        }
    })
}

/// Sets the automatic export after a sync (`[auto_export]`, shared by every PC). The folder
/// must be a sub-folder of the library (`invalidFolder` otherwise); it is stored as `a/b`.
/// With the export on, it must not be a binder that holds notes (`invalidFolder`), so that the
/// exports never overwrite files of a real binder. Keys of the section this version does not
/// know are kept, and so is a format written by a newer version while the interface says `off`
/// (the same thing for this version).
pub(crate) fn set_auto_export_in(
    context: &AppContext,
    auto_export: AutoExport,
) -> CommandResult<AppState> {
    let folder = auto_export
        .relative_folder()
        .map(|path| slash_path(&path))
        .ok_or_else(|| CommandError::new("invalidFolder", &auto_export.folder))?;
    let format = auto_export.format.known();
    if format.is_active()
        && let Ok(library) = library_of(context)
        && holds_notes(&library.root().join(&folder))
    {
        return Err(CommandError::new(
            "invalidFolder",
            format!("{folder} holds notes"),
        ));
    }
    update(context, |settings| {
        let stored = &mut settings.auto_export.format;
        if format != AutoExportFormat::Off || !matches!(stored, AutoExportFormat::Unknown(_)) {
            *stored = format;
        }
        settings.auto_export.folder = folder;
    })
}

#[tauri::command]
pub fn set_auto_export(
    context: State<'_, AppContext>,
    auto_export: AutoExport,
) -> CommandResult<AppState> {
    set_auto_export_in(&context, auto_export)
}

/// What a folder chosen as the library holds (`LibraryInspection` in `api.ts`).
#[derive(Debug, Serialize)]
pub struct LibraryInspection {
    /// `_ardoise/` (or the old `.ardoise/`) is there: an Ardoise library.
    existing: bool,
    /// Note files found, sub-folders included.
    notes: usize,
    /// False when the count stopped early (a very large folder): there may be more.
    complete: bool,
}

/// Folder entries looked at before the count of notes stops: picking a whole disk or a home
/// folder must not keep the dialog waiting.
const INSPECTION_ENTRIES: usize = 20_000;

fn existing_folder(path: &str) -> CommandResult<&Path> {
    let folder = Path::new(path);
    if folder.is_dir() {
        Ok(folder)
    } else {
        Err(CommandError::new("notAFolder", path))
    }
}

pub(crate) fn inspect_folder(path: &str, max_entries: usize) -> CommandResult<LibraryInspection> {
    let folder = existing_folder(path)?;
    let count = library::count_notes(folder, max_entries);
    Ok(LibraryInspection {
        existing: library::is_library(folder),
        notes: count.notes,
        complete: count.complete,
    })
}

/// Async: counting the notes walks the folder, off the main thread.
#[tauri::command]
pub async fn inspect_library(path: String) -> CommandResult<LibraryInspection> {
    inspect_folder(&path, INSPECTION_ENTRIES)
}

/// Makes `path` this computer's library, creating `_ardoise/` there if needed, and watches
/// it from then on.
#[tauri::command]
pub fn set_library(
    app: AppHandle,
    context: State<'_, AppContext>,
    watch: State<'_, LibraryWatch>,
    path: String,
) -> CommandResult<AppState> {
    let state = choose_library(&context, &path)?;
    watch.follow(&app, state.library());
    crate::tablet::library_changed(&app);
    Ok(state)
}

pub(crate) fn choose_library(context: &AppContext, path: &str) -> CommandResult<AppState> {
    let folder = existing_folder(path)?;
    library::init(folder).map_err(|e| CommandError::new("libraryInit", e))?;
    let machine = store(context)?.machine().to_owned();
    update(context, |settings| {
        settings.set_library(&machine, PathBuf::from(folder))
    })
}

/// Async so the checks run off the main thread (the Bluetooth query blocks).
#[tauri::command]
pub async fn self_test_report(
    app: AppHandle,
    context: State<'_, AppContext>,
    report: UiReport,
) -> Result<(), ()> {
    selftest::finish(&app, &context, report);
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn context(settings_dir: &Path, machine: &str) -> AppContext {
        AppContext {
            store: Ok(SettingsStore::locate(settings_dir, settings_dir, machine)),
            system_locale: Some("fr-FR".to_owned()),
            self_test: None,
            open_diagnostic: false,
        }
    }

    #[test]
    fn fresh_install_has_no_library() {
        let settings = tempfile::tempdir().unwrap();
        let state = current_state(&context(settings.path(), "PC-A"));
        assert_eq!(state.library(), None);
        assert_eq!(state.settings_error(), None);
        assert_eq!(state.machine(), "PC-A");
    }

    #[test]
    fn choosing_a_library_creates_it_and_remembers_it_per_machine() {
        let settings = tempfile::tempdir().unwrap();
        let notes = tempfile::tempdir().unwrap();
        let path = notes.path().display().to_string();

        let state = choose_library(&context(settings.path(), "PC-A"), &path).unwrap();
        assert_eq!(state.library(), Some(path.as_str()));
        assert!(library::is_library(notes.path()));

        // Another computer sharing the same settings file keeps its own choice.
        let other = context(settings.path(), "PC-B");
        assert_eq!(current_state(&other).library(), None);
        choose_library(&other, &path).unwrap();
        assert_eq!(
            current_state(&context(settings.path(), "PC-A")).library(),
            Some(path.as_str())
        );
    }

    #[test]
    fn inspecting_a_folder_tells_what_it_holds() {
        let dir = tempfile::tempdir().unwrap();
        let text = |path: &Path| path.to_string_lossy().into_owned();
        let empty = inspect_folder(&text(dir.path()), 1000).unwrap();
        assert!(!empty.existing && empty.notes == 0 && empty.complete);

        fs::create_dir_all(dir.path().join("Classe")).unwrap();
        fs::write(dir.path().join("2026-10-24 Un.ardoise"), b"x").unwrap();
        fs::write(
            dir.path().join("Classe").join("2026-10-24 Deux.ardoise"),
            b"x",
        )
        .unwrap();
        fs::write(dir.path().join("Classe").join("sujet.pdf"), b"x").unwrap();
        let found = inspect_folder(&text(dir.path()), 1000).unwrap();
        assert!(!found.existing);
        assert_eq!((found.notes, found.complete), (2, true));

        library::init(dir.path()).unwrap();
        let json = serde_json::to_value(inspect_folder(&text(dir.path()), 1000).unwrap()).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "existing": true, "notes": 2, "complete": true })
        );
        // A very large folder: the count stops early and says so.
        assert!(!inspect_folder(&text(dir.path()), 1).unwrap().complete);
        assert_eq!(
            inspect_folder(&text(&dir.path().join("absent")), 1000)
                .unwrap_err()
                .code(),
            "notAFolder"
        );
    }

    #[test]
    fn missing_folder_is_rejected_without_touching_settings() {
        let settings = tempfile::tempdir().unwrap();
        let missing = settings.path().join("does-not-exist").display().to_string();
        let error = choose_library(&context(settings.path(), "PC-A"), &missing).unwrap_err();
        assert_eq!(error.code, "notAFolder");
        assert!(!settings.path().join(SettingsStore::FILE_NAME).exists());
    }

    #[test]
    fn unreadable_settings_are_reported_not_overwritten() {
        let settings = tempfile::tempdir().unwrap();
        let file = settings.path().join(SettingsStore::FILE_NAME);
        fs::write(&file, "language = [broken").unwrap();
        let context = context(settings.path(), "PC-A");

        assert!(current_state(&context).settings_error().is_some());
        let notes = tempfile::tempdir().unwrap();
        let error = choose_library(&context, &notes.path().display().to_string()).unwrap_err();
        assert_eq!(error.code, "settings");
        assert_eq!(fs::read_to_string(&file).unwrap(), "language = [broken");
    }

    fn auto_export(format: AutoExportFormat, folder: &str) -> AutoExport {
        AutoExport {
            format,
            folder: folder.to_owned(),
            ..AutoExport::default()
        }
    }

    #[test]
    fn auto_export_is_off_in_a_fresh_state() {
        let settings = tempfile::tempdir().unwrap();
        let state = current_state(&context(settings.path(), "PC-A"));
        assert_eq!(state.auto_export().format, AutoExportFormat::Off);
        assert_eq!(state.auto_export().folder, "Exports");
        assert_eq!(
            state.auto_export().valid_folder().as_deref(),
            Some("Exports")
        );
    }

    #[test]
    fn auto_export_is_validated_normalised_and_stored() {
        let settings = tempfile::tempdir().unwrap();
        let context = context(settings.path(), "PC-A");
        let state = set_auto_export_in(
            &context,
            auto_export(AutoExportFormat::Pdf, "/Lecture\\Tel/"),
        )
        .unwrap();
        assert_eq!(state.auto_export().format, AutoExportFormat::Pdf);
        assert_eq!(state.auto_export().folder, "Lecture/Tel");
        // Read back from disk, shared by every PC (not a machine section).
        let other = self::context(settings.path(), "PC-B");
        let state = current_state(&other);
        assert_eq!(state.auto_export().format, AutoExportFormat::Pdf);
        assert_eq!(state.auto_export().folder, "Lecture/Tel");
        let text = fs::read_to_string(settings.path().join(SettingsStore::FILE_NAME)).unwrap();
        assert!(text.contains("[auto_export]"), "{text}");
        assert!(!text.contains("library"), "{text}");

        let state =
            set_auto_export_in(&context, auto_export(AutoExportFormat::Off, "Exports")).unwrap();
        assert_eq!(state.auto_export().format, AutoExportFormat::Off);
    }

    #[test]
    fn invalid_auto_export_folders_are_refused_without_touching_settings() {
        let settings = tempfile::tempdir().unwrap();
        let context = context(settings.path(), "PC-A");
        for bad in [
            "",
            " ",
            "..",
            "a/../b",
            "C:/Exports",
            "_ardoise",
            "_ardoise/x",
        ] {
            let error =
                set_auto_export_in(&context, auto_export(AutoExportFormat::Png, bad)).unwrap_err();
            assert_eq!(error.code(), "invalidFolder", "{bad:?}");
        }
        assert!(!settings.path().join(SettingsStore::FILE_NAME).exists());
    }

    #[test]
    fn a_binder_with_notes_is_refused_as_the_export_folder() {
        let settings = tempfile::tempdir().unwrap();
        let (dir, library) = crate::library::tests::library();
        let context = context(settings.path(), "PC-A");
        // Without a library there is nothing to check.
        set_auto_export_in(&context, auto_export(AutoExportFormat::Pdf, "Classe")).unwrap();
        choose_library(&context, &dir.path().display().to_string()).unwrap();
        crate::library::tests::write_sample(&library, "Classe/3e B", "Cours");
        library.create_binder(Path::new("Vide")).unwrap();

        // The binder itself, or one above a note, with the export on.
        for folder in ["Classe", "Classe/3e B", "Classe/3e B/"] {
            let error = set_auto_export_in(&context, auto_export(AutoExportFormat::Png, folder))
                .unwrap_err();
            assert_eq!(error.code(), "invalidFolder", "{folder}");
        }
        assert_eq!(
            current_state(&context).auto_export().format,
            AutoExportFormat::Pdf
        );
        // An empty binder, a new folder, and the export off are fine.
        for folder in ["Vide", "Nouveau"] {
            set_auto_export_in(&context, auto_export(AutoExportFormat::Png, folder)).unwrap();
        }
        set_auto_export_in(&context, auto_export(AutoExportFormat::Off, "Classe")).unwrap();
    }

    #[test]
    fn a_format_from_a_newer_version_is_off_for_the_interface_and_kept() {
        let settings = tempfile::tempdir().unwrap();
        let file = settings.path().join(SettingsStore::FILE_NAME);
        fs::write(&file, "[auto_export]\nformat = \"jpg\"\nfolder = \"Tel\"\n").unwrap();
        let context = context(settings.path(), "PC-A");
        let state = current_state(&context);
        assert_eq!(state.settings_error(), None);
        assert_eq!(state.auto_export().format, AutoExportFormat::Off);
        assert!(!state.auto_export().is_active());
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["autoExport"]["format"], "off");

        // Saving "off" (another folder) keeps the value; a real choice replaces it.
        set_auto_export_in(&context, auto_export(AutoExportFormat::Off, "Lecture")).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("format = \"jpg\""), "{text}");
        assert!(text.contains("folder = \"Lecture\""), "{text}");
        set_auto_export_in(&context, auto_export(AutoExportFormat::Pdf, "Lecture")).unwrap();
        let text = fs::read_to_string(&file).unwrap();
        assert!(text.contains("format = \"pdf\""), "{text}");
    }

    #[test]
    fn setting_the_auto_export_keeps_unknown_keys() {
        let settings = tempfile::tempdir().unwrap();
        fs::write(
            settings.path().join(SettingsStore::FILE_NAME),
            "[auto_export]\nformat = \"png\"\nfuture = 3\n\n[other]\nkept = true\n",
        )
        .unwrap();
        let context = context(settings.path(), "PC-A");
        set_auto_export_in(&context, auto_export(AutoExportFormat::Pdf, "Lecture")).unwrap();
        let text = fs::read_to_string(settings.path().join(SettingsStore::FILE_NAME)).unwrap();
        assert!(text.contains("future = 3"), "{text}");
        assert!(text.contains("kept = true"), "{text}");
        assert!(text.contains("format = \"pdf\""), "{text}");
    }

    #[test]
    fn app_state_carries_this_machine_s_microphone() {
        let settings = tempfile::tempdir().unwrap();
        let context = context(settings.path(), "PC-A");
        let json = serde_json::to_value(current_state(&context)).unwrap();
        assert_eq!(json["microphone"], serde_json::Value::Null);

        let state = update(&context, |s| {
            s.set_microphone("PC-A", Some("wasapi:micro".into()));
            s.set_microphone("PC-B", Some("wasapi:other".into()));
        })
        .unwrap();
        assert_eq!(state.microphone(), Some("wasapi:micro"));
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(json["microphone"], "wasapi:micro");
        // Read back from disk, per machine.
        let other = self::context(settings.path(), "PC-B");
        assert_eq!(current_state(&other).microphone(), Some("wasapi:other"));
        let third = self::context(settings.path(), "PC-C");
        assert_eq!(current_state(&third).microphone(), None);
    }

    #[test]
    fn app_state_json_has_the_auto_export_shape_of_the_interface() {
        let settings = tempfile::tempdir().unwrap();
        let context = context(settings.path(), "PC-A");
        let state =
            set_auto_export_in(&context, auto_export(AutoExportFormat::Png, "Lecture")).unwrap();
        let json = serde_json::to_value(&state).unwrap();
        assert_eq!(
            json["autoExport"],
            serde_json::json!({ "format": "png", "folder": "Lecture" })
        );
        // The interface sends the same shape back.
        let parsed: AutoExport = serde_json::from_value(json["autoExport"].clone()).unwrap();
        assert_eq!(parsed.format, AutoExportFormat::Png);
        assert_eq!(parsed.folder, "Lecture");
    }
}
