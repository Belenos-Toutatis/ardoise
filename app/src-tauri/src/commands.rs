//! Commands called by the interface (see `app/ui/src/lib/api.ts`).

use std::path::{Path, PathBuf};

use ardoise_core::library;
use ardoise_core::settings::{Language, Settings, SettingsError, SettingsStore, Theme};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::AppContext;
use crate::selftest::{self, UiReport};

/// Error sent to the interface: `code` selects a translated message, `message` is the
/// technical detail shown when no translation exists.
#[derive(Debug, Serialize)]
pub struct CommandError {
    code: &'static str,
    message: String,
}

impl CommandError {
    fn new(code: &'static str, message: impl ToString) -> Self {
        CommandError {
            code,
            message: message.to_string(),
        }
    }
}

impl From<SettingsError> for CommandError {
    fn from(error: SettingsError) -> Self {
        CommandError::new("settings", error)
    }
}

type CommandResult<T> = Result<T, CommandError>;

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

    pub fn library(&self) -> Option<&str> {
        self.library.as_deref()
    }
}

fn store(context: &AppContext) -> CommandResult<&SettingsStore> {
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
        machine,
        settings_path,
        settings_error,
        language: settings.language,
        theme: settings.theme,
        system_locale: context.system_locale.clone(),
        self_test: context.self_test.is_some(),
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

fn update(context: &AppContext, change: impl FnOnce(&mut Settings)) -> CommandResult<AppState> {
    let updated = store(context)?.update(change)?;
    Ok(state_from(context, Ok(updated)))
}

#[tauri::command]
pub fn app_state(context: State<'_, AppContext>) -> AppState {
    current_state(&context)
}

#[tauri::command]
pub fn set_language(context: State<'_, AppContext>, language: Language) -> CommandResult<AppState> {
    update(&context, |settings| settings.language = language)
}

#[tauri::command]
pub fn set_theme(context: State<'_, AppContext>, theme: Theme) -> CommandResult<AppState> {
    update(&context, |settings| settings.theme = theme)
}

#[derive(Debug, Serialize)]
pub struct LibraryInspection {
    existing: bool,
}

fn existing_folder(path: &str) -> CommandResult<&Path> {
    let folder = Path::new(path);
    if folder.is_dir() {
        Ok(folder)
    } else {
        Err(CommandError::new("notAFolder", path))
    }
}

#[tauri::command]
pub fn inspect_library(path: String) -> CommandResult<LibraryInspection> {
    let folder = existing_folder(&path)?;
    Ok(LibraryInspection {
        existing: library::is_library(folder),
    })
}

/// Makes `path` this computer's library, creating `.ardoise/` there if needed.
#[tauri::command]
pub fn set_library(context: State<'_, AppContext>, path: String) -> CommandResult<AppState> {
    choose_library(&context, &path)
}

fn choose_library(context: &AppContext, path: &str) -> CommandResult<AppState> {
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
}
