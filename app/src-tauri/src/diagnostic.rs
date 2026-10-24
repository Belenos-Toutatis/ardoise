//! The guided tablet test (SPEC §4.8) wired to the interface and to the command line.
//!
//! The run happens on its own thread. Its operator forwards every step event to the
//! interface through a Tauri channel and blocks on the user's answers, which arrive through
//! the `diagnostic_answer` command. `ardoise.exe --diagnostic --simulated` runs the same test
//! without a window, against the simulated tablet, and prints the report.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ardoise_core::library;
use ardoise_core::settings::Language;
use ardoise_device::diagnostic::{
    self, Decision, DiagnosticConfig, DiagnosticReport, Instruction, Operator, Progress, StepId,
    StepOutcome,
};
use ardoise_device::transport::{
    Advertisement, Clock, Connector, SystemClock, Transport, TransportError,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use tauri::ipc::Channel;

use crate::AppContext;
use crate::commands::{CommandError, current_state};

/// What the interface receives during a run (see `app/ui/src/lib/diagnostic.ts`).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DiagnosticEvent {
    Started {
        step: StepId,
    },
    Prompt {
        step: StepId,
        instruction: Instruction,
    },
    Notify {
        step: StepId,
        instruction: Instruction,
    },
    Progress {
        step: StepId,
        progress: Progress,
    },
    Finished {
        outcome: StepOutcome,
    },
    Done {
        report: DiagnosticReport,
    },
}

/// Options chosen in the interface before starting.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticOptions {
    pub simulated: bool,
    pub delete_after_sync: bool,
}

/// The run in progress, if any.
#[derive(Default)]
pub struct DiagnosticRuns {
    current: Arc<Mutex<Option<RunHandle>>>,
}

struct RunHandle {
    answers: Sender<Decision>,
    cancel: Arc<AtomicBool>,
}

/// Operator backed by the interface.
struct UiOperator {
    events: Channel<DiagnosticEvent>,
    answers: Receiver<Decision>,
    cancel: Arc<AtomicBool>,
}

impl UiOperator {
    fn send(&self, event: DiagnosticEvent) {
        // The window may have been closed: the run then simply continues to its end.
        let _ = self.events.send(event);
    }
}

impl Operator for UiOperator {
    fn started(&mut self, step: StepId) {
        self.send(DiagnosticEvent::Started { step });
    }

    fn prompt(&mut self, step: StepId, instruction: &Instruction) -> Decision {
        // Drop answers meant for an earlier prompt.
        while self.answers.try_recv().is_ok() {}
        self.send(DiagnosticEvent::Prompt {
            step,
            instruction: instruction.clone(),
        });
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Decision::Cancel;
            }
            match self.answers.recv_timeout(Duration::from_millis(200)) {
                Ok(decision) => return decision,
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return Decision::Cancel,
            }
        }
    }

    fn notify(&mut self, step: StepId, instruction: &Instruction) {
        self.send(DiagnosticEvent::Notify {
            step,
            instruction: instruction.clone(),
        });
    }

    fn progress(&mut self, step: StepId, progress: &Progress) {
        self.send(DiagnosticEvent::Progress {
            step,
            progress: progress.clone(),
        });
    }

    fn finished(&mut self, outcome: &StepOutcome) {
        self.send(DiagnosticEvent::Finished {
            outcome: outcome.clone(),
        });
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }
}

/// `"fr"` or `"en"`, following the interface language.
pub(crate) fn report_language(context: &AppContext) -> String {
    let french = match current_state(context).language() {
        Language::Fr => true,
        Language::En => false,
        Language::Auto => context
            .system_locale
            .as_deref()
            .is_some_and(|locale| locale.to_lowercase().starts_with("fr")),
    };
    if french { "fr" } else { "en" }.into()
}

/// Stands in for the Bluetooth connector when it cannot be created, so that the run still
/// produces a complete report saying why.
struct Unavailable {
    error: TransportError,
}

impl Connector for Unavailable {
    fn clock(&self) -> Arc<dyn Clock> {
        Arc::new(SystemClock::new())
    }

    fn find(
        &mut self,
        _timeout: Duration,
        _filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError> {
        Err(self.error.clone())
    }

    fn connect(
        &mut self,
        _address: &str,
        _timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        Err(self.error.clone())
    }
}

/// A throwaway library for simulated runs, so that a simulated registration never replaces
/// the real tablet's identity in the user's library.
fn simulation_library() -> std::io::Result<PathBuf> {
    remove_old_simulations();
    let dir = std::env::temp_dir().join(format!(
        "ardoise-simulation-{}-{}",
        std::process::id(),
        chrono_stamp()
    ));
    std::fs::create_dir_all(&dir)?;
    library::init(&dir)?;
    Ok(dir)
}

/// Simulated libraries are kept a day for inspection, then removed (best effort).
fn remove_old_simulations() {
    let Ok(entries) = std::fs::read_dir(std::env::temp_dir()) else {
        return;
    };
    for entry in entries.flatten() {
        let old = entry
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|modified| modified.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(24 * 3600));
        if old
            && entry
                .file_name()
                .to_string_lossy()
                .starts_with("ardoise-simulation-")
        {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

fn chrono_stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default()
}

/// Checks the Bluetooth adapter before a real run.
fn adapter_check() -> Result<String, String> {
    match ardoise_ble::adapter_info() {
        Ok(info) => match info.status {
            ardoise_ble::AdapterStatus::On => Ok(info
                .name
                .map_or_else(|| "on".to_owned(), |name| format!("{name}, on"))),
            ardoise_ble::AdapterStatus::Off => {
                Err("Bluetooth is switched off in Windows settings".into())
            }
            ardoise_ble::AdapterStatus::NoLowEnergy => {
                Err("the Bluetooth adapter does not support Bluetooth Low Energy".into())
            }
            ardoise_ble::AdapterStatus::NotFound => Err("no Bluetooth adapter found".into()),
        },
        Err(error) => Err(error.to_string()),
    }
}

/// Runs the test against the simulated tablet; `inner` is the person answering prompts, or
/// `None` to let the simulated user answer everything.
fn run_simulated(
    library: &Path,
    options: DiagnosticOptions,
    language: String,
    cancel: Arc<AtomicBool>,
    inner: Option<Box<dyn Operator + Send>>,
) -> DiagnosticReport {
    let tablet = ardoise_sim::SimTablet::slate_large();
    let connector: Box<dyn Connector> = Box::new(tablet.connector());
    let mut operator = match inner {
        Some(inner) => ardoise_sim::SimulatedUser::wrapping(tablet.clone(), inner),
        None => ardoise_sim::SimulatedUser::autonomous(tablet.clone()),
    };
    let mut config = DiagnosticConfig::new(library.to_path_buf(), "sim");
    config.delete_after_sync = options.delete_after_sync;
    config.language = language;
    config.cancel = cancel;
    diagnostic::run(connector, &mut operator, &config, None)
}

/// `ardoise.exe --diagnostic --simulated`: prints the report; returns the process exit code
/// (0 when no step failed).
pub fn run_simulated_cli(context: &AppContext) -> i32 {
    let library = match simulation_library() {
        Ok(dir) => dir,
        Err(error) => {
            println!("DIAGNOSTIC FAILED: cannot create a temporary library: {error}");
            return 1;
        }
    };
    let options = DiagnosticOptions {
        simulated: true,
        delete_after_sync: true,
    };
    let report = run_simulated(
        &library,
        options,
        report_language(context),
        Arc::new(AtomicBool::new(false)),
        None,
    );
    print!(
        "{}",
        ardoise_device::render_report(&report, &report_language(context))
    );
    if let Some(path) = &report.report_path {
        println!("Report: {}", path.display());
    }
    if report.failures() == 0 && !report.cancelled {
        println!("DIAGNOSTIC OK");
        0
    } else {
        println!("DIAGNOSTIC FAILED");
        1
    }
}

#[tauri::command]
pub fn diagnostic_start(
    context: State<'_, AppContext>,
    runs: State<'_, DiagnosticRuns>,
    options: DiagnosticOptions,
    on_event: Channel<DiagnosticEvent>,
) -> Result<(), CommandError> {
    let mut current = runs
        .current
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if current.is_some() {
        return Err(CommandError::new(
            "diagnosticRunning",
            "a test is already running",
        ));
    }
    let library = if options.simulated {
        simulation_library().map_err(|e| CommandError::new("libraryInit", e))?
    } else {
        let state = current_state(&context);
        PathBuf::from(
            state
                .library()
                .ok_or_else(|| CommandError::new("noLibrary", "no library chosen"))?,
        )
    };
    let language = report_language(&context);
    let (answers_tx, answers_rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    *current = Some(RunHandle {
        answers: answers_tx,
        cancel: cancel.clone(),
    });
    drop(current);

    let slot = runs.current.clone();
    let spawned = std::thread::Builder::new()
        .name("diagnostic".into())
        .spawn(move || {
            let operator = UiOperator {
                events: on_event.clone(),
                answers: answers_rx,
                cancel: cancel.clone(),
            };
            let report = if options.simulated {
                run_simulated(
                    &library,
                    options,
                    language,
                    cancel,
                    Some(Box::new(operator)),
                )
            } else {
                run_ble(&library, options, language, cancel, operator)
            };
            *slot.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
            let _ = on_event.send(DiagnosticEvent::Done { report });
        });
    if let Err(error) = spawned {
        *runs
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
        return Err(CommandError::new("diagnostic", error));
    }
    Ok(())
}

fn run_ble(
    library: &Path,
    options: DiagnosticOptions,
    language: String,
    cancel: Arc<AtomicBool>,
    mut operator: UiOperator,
) -> DiagnosticReport {
    let mut config = DiagnosticConfig::new(library.to_path_buf(), "ble");
    config.delete_after_sync = options.delete_after_sync;
    config.language = language;
    config.cancel = cancel;
    let connector: Box<dyn Connector> = match ardoise_ble::BleConnector::new() {
        Ok(connector) => Box::new(connector),
        Err(error) => Box::new(Unavailable { error }),
    };
    diagnostic::run(connector, &mut operator, &config, Some(&adapter_check))
}

#[tauri::command]
pub fn diagnostic_answer(runs: State<'_, DiagnosticRuns>, decision: Decision) {
    let current = runs
        .current
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(run) = current.as_ref() {
        let _ = run.answers.send(decision);
    }
}

#[tauri::command]
pub fn diagnostic_cancel(runs: State<'_, DiagnosticRuns>) {
    let current = runs
        .current
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(run) = current.as_ref() {
        run.cancel.store(true, Ordering::SeqCst);
        let _ = run.answers.send(Decision::Cancel);
    }
}
