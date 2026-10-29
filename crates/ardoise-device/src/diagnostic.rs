//! Guided tablet test (SPEC §4.8): a fixed sequence of steps, each preceded by an
//! instruction for the user, producing a report.
//!
//! The runner knows nothing about windows or consoles: it talks to an [`Operator`]. The
//! application implements it with its interface; `ardoise-sim` implements it with a
//! simulated user who presses the simulated tablet's button at the right moments.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ardoise_core::settings::Orientation;

use crate::service::PageContext;
use crate::session::DownloadedPage;

/// The steps, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepId {
    /// A Bluetooth LE adapter is present and switched on.
    Adapter,
    /// The user draws reference pages that the sync step will download.
    ReferencePages,
    /// The tablet is found while scanning.
    Find,
    /// Connection with the shared identity, or registration if there is none (or it was refused).
    Connect,
    /// Name, firmware, battery, size, clock and number of stored pages.
    Info,
    /// Download of every stored page, CRC and decoding checks, a note in the library for each,
    /// deletion after saving.
    Sync,
    /// Live mode for a fixed time: sample rate, coordinates and pressure ranges.
    Live,
    /// Disconnection, then a new connection with the saved identity.
    Reconnect,
}

impl StepId {
    pub const ALL: [StepId; 8] = [
        StepId::Adapter,
        StepId::ReferencePages,
        StepId::Find,
        StepId::Connect,
        StepId::Info,
        StepId::Sync,
        StepId::Live,
        StepId::Reconnect,
    ];
}

/// What the user is asked to do. The interface turns each variant into a translated sentence.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Instruction {
    /// Draw `count` reference pages on paper, pressing the tablet's button after each one.
    DrawReferencePages { count: u32 },
    /// Press the tablet's button briefly so that it shows up.
    PressButton,
    /// Registration: hold the button about 6 s until the LED blinks blue. The interface warns
    /// that the phone app and Inkspace will lose access to the tablet.
    HoldButtonToRegister,
    /// Registration started: press the button briefly now to confirm it.
    PressButtonToConfirm,
    /// Draw continuously on the tablet for `seconds` seconds (live mode).
    DrawLive { seconds: u32 },
}

/// The user's answer to a prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Decision {
    /// Done, run the step.
    Continue,
    /// Do not run this step (reported as skipped).
    Skip,
    /// Stop the whole test (the report is still written).
    Cancel,
}

/// Live information while a step runs.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Progress {
    /// Waiting for the tablet (scan, button press…).
    Waiting,
    /// Downloading stored page `page` of `total` (1-based).
    Downloading { page: u32, total: u32 },
    /// Live mode running.
    Live { samples: u64, elapsed_ms: u64 },
}

/// Result of a step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StepStatus {
    Passed,
    Failed,
    Skipped,
}

/// One fact measured by a step, e.g. `("battery", "87 %, not charging")`. The key is a
/// stable identifier the interface translates; the value is already formatted.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Detail {
    pub key: String,
    pub value: String,
}

/// What a step did.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct StepOutcome {
    pub step: StepId,
    pub status: StepStatus,
    pub details: Vec<Detail>,
}

/// The person (or the simulation) in front of the tablet.
pub trait Operator {
    /// A step begins (before its prompt, if it has one).
    fn started(&mut self, _step: StepId) {}

    /// Shows `instruction` before `step` and blocks until the user answers.
    fn prompt(&mut self, step: StepId, instruction: &Instruction) -> Decision;

    /// Shows `instruction` while the runner keeps working (e.g. "press the button to confirm"
    /// while it waits for the tablet's answer). Must not block.
    fn notify(&mut self, step: StepId, instruction: &Instruction);

    /// Live information about the running step. Must not block.
    fn progress(&mut self, step: StepId, progress: &Progress);

    /// A step finished.
    fn finished(&mut self, outcome: &StepOutcome);

    /// Whether the user asked to stop; polled during long waits.
    fn cancelled(&self) -> bool;
}

/// Settings of a test run.
#[derive(Debug, Clone)]
pub struct DiagnosticConfig {
    /// Library holding `_ardoise/device.json` and receiving the log, the downloaded pages
    /// (`_ardoise/diagnostics/<log name>/page-N.bin`) and the report.
    pub library: PathBuf,
    /// `"ble"`, `"sim"` or `"replay"`, written in the log header.
    pub transport: String,
    /// Number of reference pages asked for.
    pub reference_pages: u32,
    /// Duration of the live-mode step.
    pub live_duration: Duration,
    /// Delete each page from the tablet once it is saved and verified (and added to the
    /// library as a note: [`PageImport`]).
    pub delete_after_sync: bool,
    /// How the tablet is held, for the notes made from the pages.
    pub orientation: Orientation,
    /// Makes the note of a downloaded page as the normal synchronisation does (thumbnail,
    /// automatic export). `None`: a plain note without them.
    pub page_import: Option<PageImport>,
    /// How long the tablet is looked for, and how long a button press is awaited.
    pub wait_timeout: Duration,
    /// Language of the text report: `"fr"` or `"en"`.
    pub language: String,
    /// Set from another thread to stop the run as soon as possible.
    pub cancel: Arc<AtomicBool>,
    /// Identity for a registration (random when `None`); set to replay a recorded run.
    pub host_id: Option<crate::identity::HostId>,
}

impl DiagnosticConfig {
    pub fn new(library: PathBuf, transport: &str) -> Self {
        Self {
            library,
            transport: transport.into(),
            reference_pages: 3,
            live_duration: Duration::from_secs(30),
            delete_after_sync: true,
            orientation: Orientation::Landscape,
            page_import: None,
            wait_timeout: Duration::from_secs(60),
            language: "en".into(),
            cancel: Arc::new(AtomicBool::new(false)),
            host_id: None,
        }
    }
}

/// Writes a downloaded page as a note of the library (`&Path`: its root) and returns the note's
/// path, like [`crate::service::PageStore`] but shareable. `Err` keeps the page on the tablet.
#[derive(Clone)]
pub struct PageImport(Arc<PageImportFn>);

type PageImportFn =
    dyn Fn(&Path, DownloadedPage, &PageContext) -> Result<PathBuf, String> + Send + Sync;

impl PageImport {
    pub fn new(
        import: impl Fn(&Path, DownloadedPage, &PageContext) -> Result<PathBuf, String>
        + Send
        + Sync
        + 'static,
    ) -> Self {
        Self(Arc::new(import))
    }

    pub(crate) fn import(
        &self,
        root: &Path,
        page: DownloadedPage,
        context: &PageContext,
    ) -> Result<PathBuf, String> {
        (self.0)(root, page, context)
    }
}

impl fmt::Debug for PageImport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PageImport")
    }
}

pub use crate::runner::run;

/// Everything a test run produced.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub app_version: String,
    pub transport: String,
    /// Local start time, RFC 3339.
    pub started: String,
    pub steps: Vec<StepOutcome>,
    /// The `.jsonl` log of the run.
    pub log_path: Option<PathBuf>,
    /// The text report written next to the log.
    pub report_path: Option<PathBuf>,
    /// The test was stopped by the user before the end.
    pub cancelled: bool,
}

impl DiagnosticReport {
    pub fn count(&self, status: StepStatus) -> usize {
        self.steps.iter().filter(|s| s.status == status).count()
    }

    pub fn failures(&self) -> usize {
        self.count(StepStatus::Failed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The interface reads these shapes (`app/ui/src/lib/diagnostic.ts`).
    #[test]
    fn serializes_as_the_interface_expects() {
        let value = |v: &dyn erased::Serialize| v.to_json();
        assert_eq!(value(&StepId::ReferencePages), json!("referencePages"));
        assert_eq!(
            value(&Instruction::DrawLive { seconds: 30 }),
            json!({"kind": "drawLive", "seconds": 30})
        );
        assert_eq!(
            value(&Instruction::HoldButtonToRegister),
            json!({"kind": "holdButtonToRegister"})
        );
        assert_eq!(
            value(&Progress::Live {
                samples: 12,
                elapsed_ms: 500
            }),
            json!({"kind": "live", "samples": 12, "elapsedMs": 500})
        );
        assert_eq!(
            value(&Progress::Downloading { page: 1, total: 3 }),
            json!({"kind": "downloading", "page": 1, "total": 3})
        );
        assert_eq!(value(&StepStatus::Skipped), json!("skipped"));
        assert_eq!(
            serde_json::from_value::<Decision>(json!("continue")).unwrap(),
            Decision::Continue
        );
        let report = DiagnosticReport {
            app_version: "0.1.0".into(),
            transport: "sim".into(),
            started: "x".into(),
            steps: vec![],
            log_path: None,
            report_path: None,
            cancelled: false,
        };
        let keys: Vec<String> = value(&report)
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        for key in [
            "appVersion",
            "transport",
            "started",
            "steps",
            "logPath",
            "reportPath",
            "cancelled",
        ] {
            assert!(keys.contains(&key.to_owned()), "{key}");
        }
    }

    /// Lets the test above take any serializable value.
    mod erased {
        pub trait Serialize {
            fn to_json(&self) -> serde_json::Value;
        }
        impl<T: serde::Serialize> Serialize for T {
            fn to_json(&self) -> serde_json::Value {
                serde_json::to_value(self).unwrap()
            }
        }
    }
}
