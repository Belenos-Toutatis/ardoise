//! Ardoise desktop application: window setup, command-line modes and the commands the
//! interface calls.

// A release build without `custom-protocol` would load its interface from the dev server
// instead of embedding it: refuse to produce such a non-portable executable.
#[cfg(all(dev, not(debug_assertions)))]
compile_error!("release builds need `--features custom-protocol` (use `cargo xtask check`)");

mod commands;
mod console;
mod diagnostic;
mod selftest;

use std::path::PathBuf;

use ardoise_core::settings::SettingsStore;
use tauri::{WebviewUrl, WebviewWindowBuilder};

/// Command-line options. Normal launches have none.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Cli {
    /// `--self-test`: load the interface in a hidden window, check the environment,
    /// print a report and exit with 0 (ok) or 1 (failure).
    pub self_test: bool,
    /// `--diagnostic`: open the guided tablet test right away.
    pub diagnostic: bool,
    /// With `--diagnostic`: run the test without a window against the simulated tablet,
    /// print the report and exit with 0 (no failure) or 1.
    pub simulated: bool,
}

impl Cli {
    pub fn parse(args: impl IntoIterator<Item = String>) -> Cli {
        let mut cli = Cli::default();
        for arg in args {
            match arg.as_str() {
                "--self-test" => cli.self_test = true,
                "--diagnostic" => cli.diagnostic = true,
                "--simulated" => cli.simulated = true,
                _ => {}
            }
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
    if cli.self_test || headless_diagnostic {
        console::attach_parent_console();
    }

    let context = AppContext {
        store: SettingsStore::open_default().map_err(|e| e.to_string()),
        system_locale: sys_locale::get_locale(),
        self_test: cli.self_test.then(selftest::SelfTest::start),
        open_diagnostic: cli.diagnostic,
    };

    if headless_diagnostic {
        std::process::exit(diagnostic::run_simulated_cli(&context));
    }

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(context)
        .manage(diagnostic::DiagnosticRuns::default())
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
    }
}
