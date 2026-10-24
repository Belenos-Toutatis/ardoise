//! The guided test itself: runs the steps of [`crate::diagnostic`] against a connector.
//!
//! Detail values that are fixed phrases are written as `@key`: the interface and the text
//! report translate them (`diagnostic.values.key`). Numbers, names and technical error
//! messages are written as they are.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::time::Duration;

use ardoise_core::fsutil::write_atomic;
use ardoise_proto::live::{LivePacket, LiveRecord};
use ardoise_proto::stroke::Point;

use crate::diagnostic::{
    Decision, Detail, DiagnosticConfig, DiagnosticReport, Instruction, Operator, Progress, StepId,
    StepOutcome, StepStatus,
};
use crate::identity::DeviceRecord;
use crate::log::{DiagnosticLog, SharedLog, lock};
use crate::manager::{CLOCK_DRIFT_WARNING, Manager};
use crate::session::{DeviceError, DeviceInfo, DownloadedPage, Session};
use crate::transport::{Advertisement, Connector};
use crate::{preview, report};

/// Checks the Bluetooth adapter: `Ok(description)` or `Err(reason)`.
pub type AdapterCheck<'a> = &'a dyn Fn() -> Result<String, String>;

/// How often live-mode progress is reported.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(500);

/// Runs the whole guided test and returns its report. The log, the downloaded pages and the
/// text report are written in `<library>/.ardoise/diagnostics/`. Never panics on tablet
/// errors: they become failed steps.
pub fn run(
    connector: Box<dyn Connector>,
    operator: &mut dyn Operator,
    config: &DiagnosticConfig,
    adapter_check: Option<AdapterCheck<'_>>,
) -> DiagnosticReport {
    let clock = connector.clock();
    let started = chrono::DateTime::<chrono::Local>::from(clock.now())
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, false);
    let (log, log_error) =
        match DiagnosticLog::create(&config.library, &config.transport, clock.clone()) {
            Ok(log) => (log, None),
            Err(error) => (
                DiagnosticLog::in_memory(&config.transport, clock.clone()),
                Some(error.to_string()),
            ),
        };
    let log_path = log.path().map(Path::to_path_buf);
    let log = log.shared();
    if let Some(error) = log_error {
        lock(&log).note(format!("cannot write the log in the library: {error}"));
    }
    lock(&log).note(format!(
        "diagnostic started: {} reference pages, live {} s, delete after sync: {}",
        config.reference_pages,
        config.live_duration.as_secs(),
        config.delete_after_sync
    ));

    let manager = Manager::new(connector, config.library.clone(), log.clone())
        .with_cancel_flag(config.cancel.clone());
    let manager = match config.host_id {
        Some(id) => manager.with_host_id(id),
        None => manager,
    };
    let mut run = Run {
        manager,
        operator,
        config,
        log: log.clone(),
        pages_dir: log_path.as_deref().map(pages_dir_for),
        record: None,
        found: None,
        session: None,
        info: None,
        clock_drift: 0,
        reference_pages_drawn: false,
        adapter_check,
    };

    let mut steps = Vec::new();
    let mut cancelled = false;
    for step in StepId::ALL {
        if run.is_cancelled() {
            cancelled = true;
            break;
        }
        run.operator.started(step);
        lock(&log).note(format!("step {step:?}"));
        let flow = run.step(step);
        let (status, details) = match flow {
            Flow::Passed(details) => (StepStatus::Passed, details),
            Flow::Failed(details) => (StepStatus::Failed, details),
            Flow::Skipped(details) => (StepStatus::Skipped, details),
            Flow::Cancelled => {
                cancelled = true;
                break;
            }
        };
        let outcome = StepOutcome {
            step,
            status,
            details,
        };
        {
            let mut log = lock(&log);
            log.note(format!("step {step:?}: {status:?}"));
            // A step boundary is a good moment to put the log on disk.
            let _ = log.flush();
        }
        run.operator.finished(&outcome);
        steps.push(outcome);
    }
    if let Some(session) = run.session.take() {
        let _ = session.disconnect();
    }
    if cancelled {
        lock(&log).note("diagnostic cancelled by the user");
    }

    let mut result = DiagnosticReport {
        app_version: env!("CARGO_PKG_VERSION").into(),
        transport: config.transport.clone(),
        started,
        steps,
        log_path: log_path.clone(),
        report_path: None,
        cancelled,
    };
    if let Some(path) = &log_path {
        let report_path = path.with_extension("txt");
        let text = report::render(&result, &config.language);
        match write_atomic(&report_path, text.as_bytes()) {
            Ok(()) => result.report_path = Some(report_path),
            Err(error) => lock(&log).note(format!("cannot write the report: {error}")),
        }
    }
    lock(&log).note(format!(
        "diagnostic finished: {} passed, {} failed, {} skipped",
        result.count(StepStatus::Passed),
        result.count(StepStatus::Failed),
        result.count(StepStatus::Skipped)
    ));
    let _ = lock(&log).flush();
    result
}

/// `…/2026-10-24_140312.jsonl` -> `…/2026-10-24_140312/`.
fn pages_dir_for(log_path: &Path) -> PathBuf {
    log_path.with_extension("")
}

enum Flow {
    Passed(Vec<Detail>),
    Failed(Vec<Detail>),
    Skipped(Vec<Detail>),
    Cancelled,
}

fn detail(key: &str, value: impl Into<String>) -> Detail {
    Detail {
        key: key.into(),
        value: value.into(),
    }
}

fn yes_no(value: bool) -> &'static str {
    if value { "@yes" } else { "@no" }
}

fn error_flow(error: DeviceError) -> Flow {
    match error {
        DeviceError::Cancelled => Flow::Cancelled,
        error => Flow::Failed(vec![detail("error", error.to_string())]),
    }
}

struct Run<'a> {
    manager: Manager,
    operator: &'a mut dyn Operator,
    config: &'a DiagnosticConfig,
    log: SharedLog,
    pages_dir: Option<PathBuf>,
    record: Option<DeviceRecord>,
    found: Option<Advertisement>,
    session: Option<Session>,
    info: Option<DeviceInfo>,
    clock_drift: i64,
    reference_pages_drawn: bool,
    adapter_check: Option<AdapterCheck<'a>>,
}

impl Run<'_> {
    fn is_cancelled(&self) -> bool {
        if self.operator.cancelled() {
            self.config.cancel.store(true, Ordering::SeqCst);
        }
        self.config.cancel.load(Ordering::SeqCst)
    }

    /// Asks the user; `Some(flow)` when the step must not run.
    fn ask(&mut self, step: StepId, instruction: Instruction) -> Option<Flow> {
        // The user may take minutes: what happened so far must already be on disk.
        let _ = lock(&self.log).flush();
        match self.operator.prompt(step, &instruction) {
            Decision::Continue if self.is_cancelled() => Some(Flow::Cancelled),
            Decision::Continue => None,
            Decision::Skip => {
                lock(&self.log).note(format!("step {step:?} skipped by the user"));
                Some(Flow::Skipped(Vec::new()))
            }
            Decision::Cancel => {
                self.config.cancel.store(true, Ordering::SeqCst);
                Some(Flow::Cancelled)
            }
        }
    }

    fn step(&mut self, step: StepId) -> Flow {
        match step {
            StepId::Adapter => self.adapter(),
            StepId::ReferencePages => self.reference_pages(),
            StepId::Find => self.find(),
            StepId::Connect => self.connect(),
            StepId::Info => self.info(),
            StepId::Sync => self.sync(),
            StepId::Live => self.live(),
            StepId::Reconnect => self.reconnect(),
        }
    }

    fn adapter(&mut self) -> Flow {
        match self.adapter_check {
            None => Flow::Passed(vec![detail("adapter", "@simulatedAdapter")]),
            Some(check) => match check() {
                Ok(description) => Flow::Passed(vec![detail("adapter", description)]),
                Err(reason) => Flow::Failed(vec![detail("error", reason)]),
            },
        }
    }

    fn reference_pages(&mut self) -> Flow {
        let count = self.config.reference_pages;
        if let Some(flow) = self.ask(
            StepId::ReferencePages,
            Instruction::DrawReferencePages { count },
        ) {
            return flow;
        }
        self.reference_pages_drawn = true;
        Flow::Passed(vec![detail("pages", count.to_string())])
    }

    fn find(&mut self) -> Flow {
        self.record = match self.manager.registration() {
            Ok(record) => record,
            Err(error) => return error_flow(error),
        };
        let instruction = if self.record.is_some() {
            Instruction::PressButton
        } else {
            Instruction::HoldButtonToRegister
        };
        if let Some(flow) = self.ask(StepId::Find, instruction) {
            return flow;
        }
        self.operator.progress(StepId::Find, &Progress::Waiting);
        let timeout = self.config.wait_timeout;
        let found = match self.record.clone() {
            Some(record) => self.manager.find_registered(&record, timeout),
            None => self.manager.find(true, timeout),
        };
        match found {
            Ok(Some(found)) => {
                let details = advertisement_details(&found);
                self.found = Some(found);
                Flow::Passed(details)
            }
            Ok(None) => Flow::Failed(vec![detail("error", "@noTabletFound")]),
            Err(error) => error_flow(error),
        }
    }

    fn connect(&mut self) -> Flow {
        let Some(found) = self.found.clone() else {
            return Flow::Skipped(vec![detail("error", "@needsPreviousStep")]);
        };
        let Some(record) = self.record.clone() else {
            return self.register(&found, false);
        };
        match self.manager.connect(&record, Some(&found.address)) {
            Ok((session, drift)) => {
                self.session = Some(session);
                self.clock_drift = drift;
                Flow::Passed(vec![
                    detail("mode", "@existingRegistration"),
                    detail("name", record.name),
                ])
            }
            Err(DeviceError::NotRegistered) => {
                lock(&self.log).note("saved identity refused, registering again");
                if let Some(flow) = self.ask(StepId::Connect, Instruction::HoldButtonToRegister) {
                    return flow;
                }
                self.operator.progress(StepId::Connect, &Progress::Waiting);
                match self.manager.find(true, self.config.wait_timeout) {
                    Ok(Some(found)) => self.register(&found, true),
                    Ok(None) => Flow::Failed(vec![detail("error", "@registrationRefused")]),
                    Err(error) => error_flow(error),
                }
            }
            Err(error) => error_flow(error),
        }
    }

    fn register(&mut self, found: &Advertisement, again: bool) -> Flow {
        let mode = detail(
            "mode",
            if again {
                "@registeredAgain"
            } else {
                "@registeredNow"
            },
        );
        let previous = self.record.as_ref().map(|r| r.uuid);
        let operator = &mut *self.operator;
        operator.progress(StepId::Connect, &Progress::Waiting);
        let result = self
            .manager
            .register(found, self.config.wait_timeout, &mut || {
                operator.notify(StepId::Connect, &Instruction::PressButtonToConfirm);
            });
        match result {
            Ok((session, registration)) => {
                self.session = Some(session);
                self.clock_drift = registration.clock_drift;
                let name = registration.info.name.clone();
                self.info = Some(registration.info);
                self.record = self.manager.registration().ok().flatten();
                Flow::Passed(vec![
                    mode,
                    detail("name", name),
                    detail("identity", "@savedInDeviceJson"),
                ])
            }
            Err(DeviceError::Cancelled) => Flow::Cancelled,
            Err(error) => {
                // If the tablet confirmed the registration, the new identity is already saved:
                // continue with it rather than losing the rest of the test.
                let saved = self.manager.registration().ok().flatten();
                match saved {
                    Some(record) if Some(record.uuid) != previous => {
                        lock(&self.log).note(format!(
                            "registered, but reading the tablet failed ({error}); reconnecting"
                        ));
                        self.record = Some(record.clone());
                        match self.manager.connect(&record, Some(&found.address)) {
                            Ok((session, drift)) => {
                                self.session = Some(session);
                                self.clock_drift = drift;
                                Flow::Passed(vec![
                                    mode,
                                    detail("identity", "@savedInDeviceJson"),
                                    detail("result", "@reconnected"),
                                ])
                            }
                            Err(second) => Flow::Failed(vec![
                                mode,
                                detail("identity", "@savedInDeviceJson"),
                                detail("error", format!("{error}; {second}")),
                            ]),
                        }
                    }
                    _ => error_flow(error),
                }
            }
        }
    }

    fn info(&mut self) -> Flow {
        let Some(session) = self.session.as_mut() else {
            return Flow::Skipped(vec![detail("error", "@notConnected")]);
        };
        let info = match self.info.take() {
            Some(info) => info,
            None => match session.info() {
                Ok(info) => info,
                Err(error) => return error_flow(error),
            },
        };
        let pages = match session.stored_pages() {
            Ok(pages) => pages,
            Err(error) => return error_flow(error),
        };
        let mut details = vec![
            detail("name", info.name.clone()),
            detail("firmware", info.firmware.clone()),
            detail("battery", format!("{} %", info.battery_percent)),
            detail("charging", yes_no(info.charging)),
            detail(
                "size",
                format!(
                    "{} × {} mm ({} × {})",
                    info.width / 100,
                    info.height / 100,
                    info.width,
                    info.height
                ),
            ),
            detail("pages", pages.to_string()),
            detail("clockDrift", format!("{:+} s", self.clock_drift)),
        ];
        self.info = Some(info);
        if self.clock_drift.abs() > CLOCK_DRIFT_WARNING {
            details.push(detail("error", "@clockNotSet"));
            return Flow::Failed(details);
        }
        Flow::Passed(details)
    }

    fn sync(&mut self) -> Flow {
        let Some(session) = self.session.as_mut() else {
            return Flow::Skipped(vec![detail("error", "@notConnected")]);
        };
        let dir = self.pages_dir.clone();
        let (width, height) = self
            .info
            .as_ref()
            .map_or((29_700, 21_600), |info| (info.width, info.height));
        let mut strokes = 0usize;
        let mut points = 0usize;
        let mut decode_errors = Vec::new();
        let operator = &mut *self.operator;
        let log = self.log.clone();
        let mut sink = |page: DownloadedPage, index: u16, total: u16| -> Result<(), String> {
            operator.progress(
                StepId::Sync,
                &Progress::Downloading {
                    page: u32::from(index),
                    total: u32::from(total),
                },
            );
            // Without a place to keep the page, it must stay on the tablet.
            let Some(dir) = &dir else {
                return Err("@noPagesFolder".into());
            };
            save_verified(&dir.join(format!("page-{index}.bin")), &page.bytes)?;
            match ardoise_proto::stroke::decode(&page.bytes) {
                Ok(file) => {
                    strokes += file.strokes.len();
                    points += file.point_count();
                    let svg = preview::svg(&file, width, height);
                    // A missing preview is not worth losing the page for.
                    let _ = write_atomic(&dir.join(format!("page-{index}.svg")), svg.as_bytes());
                }
                Err(error) => {
                    // The raw bytes are saved: a better decoder can read them later.
                    lock(&log).note(format!("page {index} cannot be decoded: {error}"));
                    decode_errors.push(format!("page {index}: {error}"));
                }
            }
            Ok(())
        };
        let summary = match session.sync(self.config.delete_after_sync, &mut sink) {
            Ok(summary) => summary,
            Err(DeviceError::Storage(reason)) if reason.starts_with('@') => {
                return Flow::Failed(vec![detail("error", reason)]);
            }
            Err(error) => return error_flow(error),
        };
        let mut details = vec![
            detail(
                "pages",
                format!("{} / {}", summary.downloaded, summary.available),
            ),
            detail("strokes", strokes.to_string()),
            detail("points", points.to_string()),
            detail("deleted", summary.deleted.to_string()),
        ];
        if let Some(dir) = &self.pages_dir {
            details.push(detail("folder", dir.display().to_string()));
        }
        if !decode_errors.is_empty() {
            details.push(detail("error", decode_errors.join("; ")));
            return Flow::Failed(details);
        }
        let expected = if self.reference_pages_drawn && self.config.delete_after_sync {
            u16::try_from(self.config.reference_pages).unwrap_or(u16::MAX)
        } else {
            0
        };
        if summary.downloaded < expected {
            details.push(detail("error", "@missingReferencePages"));
            return Flow::Failed(details);
        }
        Flow::Passed(details)
    }

    fn live(&mut self) -> Flow {
        if self.session.is_none() || self.record.is_none() {
            return Flow::Skipped(vec![detail("error", "@notConnected")]);
        }
        let seconds = u32::try_from(self.config.live_duration.as_secs()).unwrap_or(u32::MAX);
        if let Some(flow) = self.ask(StepId::Live, Instruction::DrawLive { seconds }) {
            return flow;
        }
        let (Some(session), Some(record)) = (self.session.as_mut(), self.record.clone()) else {
            return Flow::Skipped(vec![detail("error", "@notConnected")]);
        };
        // The tablet drops idle links by itself, typically while the user read the prompt:
        // ask for a button press, the manager reconnects.
        if matches!(session.battery(), Err(DeviceError::Disconnected)) {
            lock(&self.log).note("the tablet disconnected while idle");
            self.operator
                .notify(StepId::Live, &Instruction::PressButton);
        }
        let clock = self.manager.clock();
        let start = clock.monotonic();
        let mut stats = LiveStats::new(record.width, record.height);
        let mut last_progress = start;
        let operator = &mut *self.operator;
        let result = self.manager.live(
            session,
            &record,
            start + self.config.live_duration,
            &mut |packet: &LivePacket| {
                let now = clock.monotonic();
                stats.add(packet, now);
                if now.saturating_sub(last_progress) >= PROGRESS_INTERVAL {
                    last_progress = now;
                    operator.progress(
                        StepId::Live,
                        &Progress::Live {
                            samples: stats.samples,
                            elapsed_ms: millis(now.saturating_sub(start)),
                        },
                    );
                }
            },
        );
        stats.finish(clock.monotonic());
        let mut details = stats.details();
        match result {
            Ok(reconnections) => details.push(detail("reconnections", reconnections.to_string())),
            Err(DeviceError::Cancelled) => return Flow::Cancelled,
            // What was measured before the failure is still worth reporting.
            Err(error) => {
                details.push(detail("error", error.to_string()));
                return Flow::Failed(details);
            }
        }
        if stats.samples == 0 {
            details.push(detail("error", "@noPenData"));
            return Flow::Failed(details);
        }
        if stats.out_of_range > 0 {
            details.push(detail("error", "@pointsOutOfRange"));
            return Flow::Failed(details);
        }
        Flow::Passed(details)
    }

    fn reconnect(&mut self) -> Flow {
        let (Some(session), Some(record)) = (self.session.take(), self.record.clone()) else {
            return Flow::Skipped(vec![detail("error", "@notConnected")]);
        };
        let address = session.peer().address.clone();
        if let Err(error) = session.disconnect() {
            lock(&self.log).note(format!("disconnect failed: {error}"));
        }
        // The tablet may stop advertising once disconnected: retry after a button press.
        let result = match self.manager.connect(&record, Some(&address)) {
            Ok(connected) => Ok(connected),
            Err(DeviceError::Cancelled) => return Flow::Cancelled,
            Err(error) => {
                lock(&self.log).note(format!("direct reconnection failed: {error}"));
                self.operator
                    .notify(StepId::Reconnect, &Instruction::PressButton);
                self.operator
                    .progress(StepId::Reconnect, &Progress::Waiting);
                match self
                    .manager
                    .find_registered(&record, self.config.wait_timeout)
                {
                    Ok(Some(found)) => self.manager.connect(&record, Some(&found.address)),
                    Ok(None) => Err(DeviceError::Timeout("tablet advertisement")),
                    Err(error) => Err(error),
                }
            }
        };
        match result {
            Ok((mut session, _)) => {
                let battery = session.battery();
                let _ = session.disconnect();
                match battery {
                    Ok(_) => Flow::Passed(vec![detail("result", "@reconnected")]),
                    Err(error) => error_flow(error),
                }
            }
            Err(error) => error_flow(error),
        }
    }
}

fn advertisement_details(found: &Advertisement) -> Vec<Detail> {
    let mut details = vec![
        detail("name", found.name.clone().unwrap_or_default()),
        detail("address", found.address.clone()),
    ];
    if let Some(rssi) = found.rssi {
        details.push(detail("signal", format!("{rssi} dBm")));
    }
    details.push(detail("registerMode", yes_no(found.in_register_mode())));
    details
}

/// Writes a page atomically and reads it back: the tablet copy is only deleted once the
/// file on disk is known to be intact.
fn save_verified(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    write_atomic(path, bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let written = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if written != bytes {
        return Err(format!("{}: content differs after writing", path.display()));
    }
    Ok(())
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// Measurements of the live-mode step.
struct LiveStats {
    width: u32,
    height: u32,
    samples: u64,
    proximity_in: u32,
    proximity_out: u32,
    x: Option<(u16, u16)>,
    y: Option<(u16, u16)>,
    pressure_max: u16,
    out_of_range: u64,
    /// Time with the pen near the surface, to compute the sample rate.
    active: Duration,
    near_since: Option<Duration>,
}

impl LiveStats {
    fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            samples: 0,
            proximity_in: 0,
            proximity_out: 0,
            x: None,
            y: None,
            pressure_max: 0,
            out_of_range: 0,
            active: Duration::ZERO,
            near_since: None,
        }
    }

    fn add(&mut self, packet: &LivePacket, now: Duration) {
        match packet {
            LivePacket::ProximityIn { .. } => {
                self.proximity_in += 1;
                self.near_since.get_or_insert(now);
            }
            LivePacket::Samples(records) => {
                for record in records {
                    match *record {
                        LiveRecord::Pen(Point { x, y, pressure }) => {
                            self.samples += 1;
                            self.near_since.get_or_insert(now);
                            self.x = Some(widen(self.x, x));
                            self.y = Some(widen(self.y, y));
                            self.pressure_max = self.pressure_max.max(pressure);
                            if u32::from(x) > self.width || u32::from(y) > self.height {
                                self.out_of_range += 1;
                            }
                        }
                        LiveRecord::ProximityOut => {
                            self.proximity_out += 1;
                            if let Some(since) = self.near_since.take() {
                                self.active += now.saturating_sub(since);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn finish(&mut self, now: Duration) {
        if let Some(since) = self.near_since.take() {
            self.active += now.saturating_sub(since);
        }
    }

    fn details(&self) -> Vec<Detail> {
        let range = |r: Option<(u16, u16)>| r.map_or("-".to_owned(), |(a, b)| format!("{a}–{b}"));
        let rate = if self.active.is_zero() {
            "-".to_owned()
        } else {
            format!(
                "{:.0} points/s",
                self.samples as f64 / self.active.as_secs_f64()
            )
        };
        vec![
            detail("samples", self.samples.to_string()),
            detail("rate", rate),
            detail("xRange", range(self.x)),
            detail("yRange", range(self.y)),
            detail("pressureMax", self.pressure_max.to_string()),
            detail(
                "proximity",
                format!("{} / {}", self.proximity_in, self.proximity_out),
            ),
        ]
    }
}

fn widen(range: Option<(u16, u16)>, value: u16) -> (u16, u16) {
    range.map_or((value, value), |(lo, hi)| (lo.min(value), hi.max(value)))
}
