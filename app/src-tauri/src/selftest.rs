//! `ardoise.exe --self-test`: proves on a given PC that the executable starts, the interface
//! loads, the settings are readable and Bluetooth LE is usable — without changing any data.
//! The report is printed and also saved next to the settings file, so a test run on another
//! PC comes back through the synced folder.

use std::fmt::Write as _;
use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use ardoise_ble::{AdapterInfo, AdapterStatus};
use serde::Deserialize;
use tauri::{AppHandle, Manager};

use crate::AppContext;
use crate::commands::current_state;

/// How long the interface may take to load and report back.
const UI_TIMEOUT: Duration = Duration::from_secs(30);
const BLUETOOTH_TIMEOUT: Duration = Duration::from_secs(10);

pub struct SelfTest {
    started: Instant,
    finished: AtomicBool,
}

impl SelfTest {
    pub fn start() -> SelfTest {
        SelfTest {
            started: Instant::now(),
            finished: AtomicBool::new(false),
        }
    }

    /// True for the first caller only, so the report is produced exactly once.
    fn claim(&self) -> bool {
        !self.finished.swap(true, Ordering::SeqCst)
    }
}

/// Checks performed inside the webview (`app/ui/src/lib/selftest.ts`).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UiReport {
    rendered: bool,
    i18n_consistent: bool,
    locale: String,
    key_count: usize,
}

/// Fails the self-test if the interface never reports back.
pub fn arm_timeout(app: AppHandle) {
    std::thread::spawn(move || {
        std::thread::sleep(UI_TIMEOUT);
        let context = app.state::<AppContext>();
        if let Some(test) = &context.self_test
            && test.claim()
        {
            conclude(&app, &context, None, test.started.elapsed());
        }
    });
}

/// Called once the interface has run its own checks.
pub fn finish(app: &AppHandle, context: &AppContext, ui: UiReport) {
    if let Some(test) = &context.self_test
        && test.claim()
    {
        conclude(app, context, Some(&ui), test.started.elapsed());
    }
}

fn conclude(app: &AppHandle, context: &AppContext, ui: Option<&UiReport>, elapsed: Duration) {
    let share = app.try_state::<crate::share::ShareRuntime>().map_or_else(
        || Err("not set up".to_owned()),
        |share| share.viewer_check(),
    );
    let (mut text, ok) = report(context, ui, elapsed, bluetooth(), share);
    let verdict = if ok {
        "SELF-TEST OK"
    } else {
        "SELF-TEST FAILED"
    };

    if let Ok(store) = &context.store
        && let Some(dir) = store.path().parent()
    {
        let file = dir.join(format!(
            "ardoise-selftest-{}.txt",
            sanitize(store.machine())
        ));
        match std::fs::write(&file, format!("{text}{verdict}\n")) {
            Ok(()) => {
                let _ = writeln!(text, "  report:    saved to {}", file.display());
            }
            Err(error) => {
                let _ = writeln!(text, "  report:    not saved ({error})");
            }
        }
    }

    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{text}{verdict}");
    let _ = out.flush();
    // Closing the webview before exiting keeps WebView2 from printing shutdown noise.
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.destroy();
    }
    app.exit(if ok { 0 } else { 1 });
}

/// Queries the adapter on its own thread: the WinRT call blocks and must not hang the test.
fn bluetooth() -> Result<AdapterInfo, String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(ardoise_ble::adapter_info().map_err(|e| e.to_string()));
    });
    receiver
        .recv_timeout(BLUETOOTH_TIMEOUT)
        .unwrap_or_else(|_| Err(format!("no answer after {} s", BLUETOOTH_TIMEOUT.as_secs())))
}

/// Builds the report lines (without the verdict) and whether every required check passed.
/// Bluetooth problems are warnings: the rest of the app works without a tablet.
fn report(
    context: &AppContext,
    ui: Option<&UiReport>,
    elapsed: Duration,
    bluetooth: Result<AdapterInfo, String>,
    share: Result<usize, String>,
) -> (String, bool) {
    let state = current_state(context);
    let mut ok = true;
    let mut text = String::new();
    let mut line = |label: &str, value: String| {
        let _ = writeln!(text, "  {:<10} {value}", format!("{label}:"));
    };

    line("version", format!("Ardoise {}", env!("CARGO_PKG_VERSION")));
    line("machine", state.machine().to_owned());

    match state.settings_error() {
        None => {
            let status = if std::path::Path::new(state.settings_path()).is_file() {
                "ok"
            } else {
                "ok, not created yet"
            };
            line("settings", format!("{} ({status})", state.settings_path()));
        }
        Some(error) => {
            ok = false;
            line(
                "settings",
                format!("{} (error: {error})", state.settings_path()),
            );
        }
    }
    line(
        "library",
        state
            .library()
            .map_or_else(|| "not chosen yet".to_owned(), str::to_owned),
    );

    match tauri::webview_version() {
        Ok(version) => line("webview", format!("WebView2 {version}")),
        Err(error) => {
            ok = false;
            line("webview", format!("error: {error}"));
        }
    }

    match ui {
        None => {
            ok = false;
            line(
                "interface",
                format!("no answer after {} s", UI_TIMEOUT.as_secs()),
            );
        }
        Some(ui) => {
            if !ui.rendered || !ui.i18n_consistent {
                ok = false;
            }
            line(
                "interface",
                format!(
                    "{} in {} ms, language {}, {} texts, FR/EN {}",
                    if ui.rendered {
                        "loaded"
                    } else {
                        "NOT RENDERED"
                    },
                    elapsed.as_millis(),
                    ui.locale,
                    ui.key_count,
                    if ui.i18n_consistent {
                        "consistent"
                    } else {
                        "INCONSISTENT"
                    },
                ),
            );
        }
    }

    // The page another PC opens when the sheet is shared on the network (M11).
    match share {
        Ok(files) => line("share", format!("viewer embedded ({files} files)")),
        Err(error) => {
            ok = false;
            line("share", format!("error: {error}"));
        }
    }

    line(
        "bluetooth",
        match bluetooth {
            Ok(AdapterInfo {
                status: AdapterStatus::On,
                name,
            }) => format!("on ({})", name.as_deref().unwrap_or("unnamed adapter")),
            Ok(AdapterInfo {
                status: AdapterStatus::Off,
                ..
            }) => "off — turn Bluetooth on in Windows settings (warning)".to_owned(),
            Ok(AdapterInfo {
                status: AdapterStatus::NotFound,
                ..
            }) => "no adapter found — plug in a Bluetooth 4.0+ dongle (warning)".to_owned(),
            Ok(AdapterInfo {
                status: AdapterStatus::NoLowEnergy,
                ..
            }) => "adapter without Bluetooth LE — a Bluetooth 4.0+ dongle is needed (warning)"
                .to_owned(),
            Err(error) => format!("error: {error} (warning)"),
        },
    );

    (text, ok)
}

/// Keeps machine names usable in a file name.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn sanitize_keeps_simple_names() {
        assert_eq!(sanitize("PC-MAISON_2"), "PC-MAISON_2");
        assert_eq!(sanitize("pc/é:1"), "pc___1");
    }
}
