//! Ardoise desktop application: window setup, command-line modes and the commands the
//! interface calls.

// A release build without `custom-protocol` would load its interface from the dev server
// instead of embedding it: refuse to produce such a non-portable executable.
#[cfg(all(dev, not(debug_assertions)))]
compile_error!("release builds need `--features custom-protocol` (use `cargo xtask check`)");

mod autoexport;
mod clipboard;
mod commands;
mod console;
mod demo;
mod diagnostic;
mod export;
mod import;
mod library;
mod notes;
mod operations;
mod selftest;
mod tablet;
mod thumbnails;
mod watcher;

use std::path::PathBuf;

use ardoise_core::settings::SettingsStore;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

/// Command-line options. Normal launches have none.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cli {
    /// `--self-test`: load the interface in a hidden window, check the environment,
    /// print a report and exit with 0 (ok) or 1 (failure).
    pub self_test: bool,
    /// `--diagnostic`: open the guided tablet test right away.
    pub diagnostic: bool,
    /// With `--diagnostic`: run the test without a window against the simulated tablet,
    /// print the report and exit with 0 (no failure) or 1.
    pub simulated: bool,
    /// `--import-inkspace [LocalState] [--library DIR] [--compare]`: import Wacom Inkspace's
    /// notes without a window, print a table and exit with 0 (all imported) or 1.
    pub import_inkspace: Option<import::ImportArgs>,
    /// `--create-demo-library DIR`: make DIR a library of synthetic notes without a window,
    /// print what was created and exit (an empty path when DIR is missing).
    pub create_demo_library: Option<PathBuf>,
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Cli {
        let mut cli = Cli::default();
        let mut import_inkspace = false;
        let mut import_args = import::ImportArgs::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--self-test" => cli.self_test = true,
                "--diagnostic" => cli.diagnostic = true,
                "--simulated" => cli.simulated = true,
                "--import-inkspace" => import_inkspace = true,
                "--compare" => import_args.compare = true,
                "--library" => import_args.library = args.next().map(PathBuf::from),
                "--create-demo-library" => {
                    cli.create_demo_library =
                        Some(args.next().map(PathBuf::from).unwrap_or_default());
                }
                other if !other.starts_with("--") && import_args.local_state.is_none() => {
                    import_args.local_state = Some(PathBuf::from(other));
                }
                _ => {}
            }
        }
        if import_inkspace {
            cli.import_inkspace = Some(import_args);
        }
        cli
    }
}

/// Shared state of the running app.
pub struct AppContext {
    /// `Err` holds a description when the settings location could not be determined.
    pub store: Result<SettingsStore, String>,
    pub system_locale: Option<String>,
    pub self_test: Option<selftest::SelfTest>,
    /// Started with `--diagnostic`: the interface opens the tablet test.
    pub open_diagnostic: bool,
}

/// WebView2 keeps caches in this folder: always local to the PC, never next to a portable
/// exe that may sit in a synced folder.
fn webview_data_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .filter(|v| !v.is_empty())
        .map(|dir| PathBuf::from(dir).join("Ardoise").join("webview"))
}

pub fn run() {
    let cli = Cli::parse(std::env::args().skip(1));
    let headless_diagnostic = cli.diagnostic && cli.simulated;
    if cli.self_test
        || headless_diagnostic
        || cli.import_inkspace.is_some()
        || cli.create_demo_library.is_some()
    {
        console::attach_parent_console();
    }

    if let Some(import_args) = &cli.import_inkspace {
        std::process::exit(import::run_cli(import_args));
    }
    if let Some(dir) = &cli.create_demo_library {
        std::process::exit(demo::run_cli(dir));
    }

    let context = AppContext {
        store: SettingsStore::open_default().map_err(|e| e.to_string()),
        system_locale: sys_locale::get_locale(),
        self_test: cli.self_test.then(selftest::SelfTest::start),
        open_diagnostic: cli.diagnostic,
    };

    // A library made before `.ardoise/` became `_ardoise/` is moved over once (best effort:
    // opening the library retries later).
    if let Some(library) = commands::current_state(&context).library() {
        let library = std::path::Path::new(library);
        let _ = ardoise_core::library::migrate_legacy_meta_dir(library);
        // SPEC §3.2: the trash keeps notes 30 days (best effort, never blocks the start).
        if let Ok(opened) = ardoise_core::library::Library::open(library) {
            let _ = opened.purge_expired();
        }
    }

    if headless_diagnostic {
        std::process::exit(diagnostic::run_simulated_cli(&context));
    }

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(context)
        .manage(diagnostic::DiagnosticRuns::default())
        .manage(thumbnails::ThumbnailCache::default_location())
        .manage(watcher::LibraryWatch::default())
        .manage(export::ExportCancel::default())
        .invoke_handler(tauri::generate_handler![
            commands::app_state,
            commands::set_language,
            commands::set_theme,
            commands::inspect_library,
            commands::set_library,
            commands::self_test_report,
            diagnostic::diagnostic_start,
            diagnostic::diagnostic_answer,
            diagnostic::diagnostic_cancel,
            notes::note_outlines,
            library::library_list,
            thumbnails::note_thumbnail,
            library::create_note,
            library::create_binder,
            library::move_notes,
            library::trash_notes,
            library::trash_list,
            library::trash_restore,
            library::trash_purge,
            library::trash_empty,
            library::set_note_tags,
            library::set_note_favorite,
            library::retitle_note,
            notes::open_note,
            notes::save_note,
            notes::render_background,
            tablet::tablet_info,
            tablet::forget_tablet,
            commands::set_auto_export,
            export::export_notes,
            export::cancel_export,
            clipboard::copy_note_image,
            operations::split_note,
            operations::merge_notes,
        ])
        .setup(move |app| {
            if cli.self_test {
                selftest::arm_timeout(app.handle().clone());
            }
            let mut window =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("Ardoise")
                    .inner_size(1200.0, 800.0)
                    .min_inner_size(800.0, 560.0)
                    .center()
                    .visible(!cli.self_test);
            if let Some(dir) = webview_data_dir() {
                window = window.data_directory(dir);
            }
            window.build()?;
            // Live refresh of the library; a failure is logged, the app works without it.
            let library = commands::current_state(&app.state::<AppContext>())
                .library()
                .map(str::to_owned);
            app.state::<watcher::LibraryWatch>()
                .follow(app.handle(), library.as_deref());
            Ok(())
        })
        .run(tauri::generate_context!());

    if let Err(error) = result {
        if cli.self_test {
            println!("SELF-TEST FAILED: {error}");
        }
        eprintln!("Ardoise could not start: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Cli {
        Cli::parse(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn parses_command_line_modes() {
        assert_eq!(parse(&[]), Cli::default());
        assert!(parse(&["--self-test"]).self_test);
        let diagnostic = parse(&["--diagnostic"]);
        assert!(diagnostic.diagnostic && !diagnostic.simulated);
        let simulated = parse(&["--diagnostic", "--simulated"]);
        assert!(simulated.diagnostic && simulated.simulated && !simulated.self_test);
        assert_eq!(parse(&["--unknown"]), Cli::default());
        assert_eq!(parse(&["C:\\somewhere"]).import_inkspace, None);
    }

    #[test]
    fn parses_the_demo_library_mode() {
        let demo = parse(&["--create-demo-library", "D:\\Démo"]);
        assert_eq!(demo.create_demo_library, Some(PathBuf::from("D:\\Démo")));
        assert_eq!(demo.import_inkspace, None);
        assert_eq!(
            parse(&["--create-demo-library"]).create_demo_library,
            Some(PathBuf::new())
        );
    }

    #[test]
    fn parses_the_inkspace_import() {
        let default = parse(&["--import-inkspace"]);
        assert_eq!(default.import_inkspace, Some(import::ImportArgs::default()));
        let full = parse(&[
            "--import-inkspace",
            "D:\\copy\\LocalState",
            "--library",
            "E:\\Notes",
            "--compare",
        ]);
        assert_eq!(
            full.import_inkspace,
            Some(import::ImportArgs {
                local_state: Some(PathBuf::from("D:\\copy\\LocalState")),
                library: Some(PathBuf::from("E:\\Notes")),
                compare: true,
            })
        );
        let reordered = parse(&["--compare", "--import-inkspace", "X"]);
        let args = reordered.import_inkspace.unwrap();
        assert!(args.compare && args.local_state == Some(PathBuf::from("X")));
        assert!(!reordered.self_test && !reordered.diagnostic);
    }
}
