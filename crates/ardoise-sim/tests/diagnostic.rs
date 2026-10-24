//! The guided tablet test (`ardoise_device::diagnostic::run`) against the simulated tablet,
//! with the simulated user alone or wrapping another operator (as the interface does).
//!
//! `cargo test -p ardoise-sim --test diagnostic -- --nocapture` prints the reports.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod common;

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ardoise_device::diagnostic::{
    self, Decision, DiagnosticConfig, DiagnosticReport, Instruction, Operator, Progress, StepId,
    StepOutcome, StepStatus,
};
use ardoise_device::render_report;
use ardoise_sim::{SimConfig, SimTablet, SimulatedUser};

fn config(library: &Path) -> DiagnosticConfig {
    let mut config = DiagnosticConfig::new(library.to_owned(), "sim");
    config.language = "fr".into();
    config
}

fn run(tablet: &SimTablet, user: &mut SimulatedUser, library: &Path) -> DiagnosticReport {
    diagnostic::run(Box::new(tablet.connector()), user, &config(library), None)
}

fn step(report: &DiagnosticReport, id: StepId) -> &StepOutcome {
    report.steps.iter().find(|s| s.step == id).unwrap()
}

fn detail<'a>(outcome: &'a StepOutcome, key: &str) -> &'a str {
    outcome
        .details
        .iter()
        .find(|d| d.key == key)
        .map(|d| d.value.as_str())
        .unwrap_or_else(|| panic!("no {key} in {outcome:?}"))
}

fn say_report(report: &DiagnosticReport) {
    for line in render_report(report, "fr").lines() {
        say!("    {line}");
    }
}

/// Stands for the interface: answers prompts with `decide`, records every call.
struct Interface {
    decide: fn(StepId, &Instruction) -> Decision,
    events: Arc<Mutex<Vec<String>>>,
}

impl Interface {
    fn new(decide: fn(StepId, &Instruction) -> Decision) -> (Self, Arc<Mutex<Vec<String>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        (
            Self {
                decide,
                events: events.clone(),
            },
            events,
        )
    }

    fn push(&self, event: String) {
        self.events.lock().unwrap().push(event);
    }
}

impl Operator for Interface {
    fn started(&mut self, step: StepId) {
        self.push(format!("started {step:?}"));
    }

    fn prompt(&mut self, step: StepId, instruction: &Instruction) -> Decision {
        let decision = (self.decide)(step, instruction);
        self.push(format!("prompt {step:?} {instruction:?} -> {decision:?}"));
        decision
    }

    fn notify(&mut self, step: StepId, instruction: &Instruction) {
        self.push(format!("notify {step:?} {instruction:?}"));
    }

    fn progress(&mut self, _step: StepId, _progress: &Progress) {}

    fn finished(&mut self, outcome: &StepOutcome) {
        self.push(format!("finished {:?} {:?}", outcome.step, outcome.status));
    }

    fn cancelled(&self) -> bool {
        false
    }
}

#[test]
fn the_guided_test_passes_every_step_against_the_simulated_tablet() {
    let _report = common::report("the_guided_test_passes_every_step_against_the_simulated_tablet");
    let tablet = SimTablet::slate_large();
    let library = tempfile::tempdir().unwrap();
    let mut user = SimulatedUser::autonomous(tablet.clone());
    let report = run(&tablet, &mut user, library.path());
    say_report(&report);

    assert!(!report.cancelled);
    assert_eq!(report.failures(), 0, "{report:#?}");
    assert_eq!(report.count(StepStatus::Passed), 8);
    let ids: Vec<StepId> = report.steps.iter().map(|s| s.step).collect();
    assert_eq!(ids, StepId::ALL);
    let text = render_report(&report, "fr");
    assert!(text.contains("0 échec(s)"), "{text}");

    assert_eq!(
        detail(step(&report, StepId::Connect), "mode"),
        "@registeredNow"
    );
    assert_eq!(detail(step(&report, StepId::Find), "registerMode"), "@yes");
    let info = step(&report, StepId::Info);
    assert_eq!(detail(info, "firmware"), "B160805A-C170704A");
    assert_eq!(detail(info, "battery"), "87 %");
    assert_eq!(detail(info, "charging"), "@no");
    assert_eq!(detail(info, "pages"), "3");
    assert_eq!(detail(info, "clockDrift"), "+0 s");
    let sync = step(&report, StepId::Sync);
    assert_eq!(detail(sync, "pages"), "3 / 3");
    assert_eq!(detail(sync, "deleted"), "3");
    let live = step(&report, StepId::Live);
    let samples: u64 = detail(live, "samples").parse().unwrap();
    assert!((5_600..=6_000).contains(&samples), "{samples}");
    assert_eq!(detail(live, "rate"), "200 points/s");
    assert_eq!(detail(live, "reconnections"), "0");
    assert_eq!(tablet.stored_page_count(), 0);
    assert_eq!(
        user.actions(),
        [
            "drew 3 reference pages",
            "held the button",
            "pressed the button",
            "drew live for 30 s"
        ]
    );

    let log_path = report.log_path.clone().unwrap();
    let report_path = report.report_path.clone().unwrap();
    assert_eq!(log_path.extension().unwrap(), "jsonl");
    assert!(log_path.starts_with(library.path().join(".ardoise").join("diagnostics")));
    assert!(log_path.is_file());
    assert_eq!(report_path.extension().unwrap(), "txt");
    let written = std::fs::read_to_string(&report_path).unwrap();
    assert!(written.contains("0 échec(s)"), "{written}");
    let pages = log_path.with_extension("");
    for n in 1..=3 {
        for extension in ["bin", "svg"] {
            let file = pages.join(format!("page-{n}.{extension}"));
            assert!(file.is_file(), "{}", file.display());
        }
    }
    say!(
        "files: {} (+ .txt, page-1..3.bin, page-1..3.svg in {})",
        log_path.file_name().unwrap().to_string_lossy(),
        pages.file_name().unwrap().to_string_lossy()
    );
    say!("DIAGNOSTIC OK");
}

#[test]
fn a_second_run_reuses_the_saved_registration() {
    let _report = common::report("a_second_run_reuses_the_saved_registration");
    let tablet = SimTablet::slate_large();
    let library = tempfile::tempdir().unwrap();
    let first = run(
        &tablet,
        &mut SimulatedUser::autonomous(tablet.clone()),
        library.path(),
    );
    assert_eq!(first.failures(), 0);
    let mut user = SimulatedUser::autonomous(tablet.clone());
    let second = run(&tablet, &mut user, library.path());
    say_report(&second);
    assert_eq!(second.failures(), 0, "{second:#?}");
    assert_eq!(second.count(StepStatus::Passed), 8);
    assert_eq!(
        detail(step(&second, StepId::Connect), "mode"),
        "@existingRegistration"
    );
    assert_eq!(detail(step(&second, StepId::Find), "registerMode"), "@no");
    assert_ne!(first.log_path, second.log_path);
    assert_eq!(
        user.actions(),
        [
            "drew 3 reference pages",
            "pressed the button",
            "drew live for 30 s"
        ]
    );
    say!("second run: Connect step reports @existingRegistration");
}

#[test]
fn a_skip_answer_at_the_live_step_is_reported_as_skipped() {
    let _report = common::report("a_skip_answer_at_the_live_step_is_reported_as_skipped");
    let tablet = SimTablet::slate_large();
    let library = tempfile::tempdir().unwrap();
    let (interface, events) = Interface::new(|step, _| {
        if step == StepId::Live {
            Decision::Skip
        } else {
            Decision::Continue
        }
    });
    let mut user = SimulatedUser::wrapping(tablet.clone(), Box::new(interface));
    let report = run(&tablet, &mut user, library.path());
    say_report(&report);
    assert!(!report.cancelled);
    assert_eq!(report.failures(), 0, "{report:#?}");
    assert_eq!(step(&report, StepId::Live).status, StepStatus::Skipped);
    assert_eq!(step(&report, StepId::Reconnect).status, StepStatus::Passed);
    assert_eq!(report.count(StepStatus::Passed), 7);
    assert!(!user.actions().iter().any(|a| a.starts_with("drew live")));
    assert_eq!(tablet.stats().live_samples, 0);
    let events = events.lock().unwrap();
    assert!(events.contains(&"prompt Live DrawLive { seconds: 30 } -> Skip".to_owned()));
    assert!(events.contains(&"notify Connect PressButtonToConfirm".to_owned()));
    assert!(events.contains(&"finished Live Skipped".to_owned()));
    say!("the wrapped operator saw {} events", events.len());
}

#[test]
fn a_cancel_answer_stops_the_run() {
    let _report = common::report("a_cancel_answer_stops_the_run");
    let tablet = SimTablet::slate_large();
    let library = tempfile::tempdir().unwrap();
    let (interface, events) = Interface::new(|step, _| {
        if step == StepId::Find {
            Decision::Cancel
        } else {
            Decision::Continue
        }
    });
    let mut user = SimulatedUser::wrapping(tablet.clone(), Box::new(interface));
    let report = run(&tablet, &mut user, library.path());
    say_report(&report);
    assert!(report.cancelled);
    let ids: Vec<StepId> = report.steps.iter().map(|s| s.step).collect();
    assert_eq!(ids, [StepId::Adapter, StepId::ReferencePages]);
    assert!(!tablet.in_register_mode(), "the button was not held");
    assert_eq!(tablet.stats().connections, 0);
    assert!(render_report(&report, "fr").contains("Test arrêté avant la fin."));
    assert!(
        events
            .lock()
            .unwrap()
            .contains(&"prompt Find HoldButtonToRegister -> Cancel".to_owned())
    );
    assert!(report.report_path.unwrap().is_file());
}

#[test]
fn the_guided_test_survives_a_tablet_that_drops_idle_links() {
    let _report = common::report("the_guided_test_survives_a_tablet_that_drops_idle_links");
    let config = SimConfig {
        idle_timeout: Some(Duration::from_secs(10)),
        ..SimConfig::slate_large()
    };
    let tablet = SimTablet::new(config);
    let library = tempfile::tempdir().unwrap();
    let mut user = SimulatedUser::autonomous(tablet.clone());
    let report = run(&tablet, &mut user, library.path());
    say_report(&report);
    assert_eq!(report.failures(), 0, "{report:#?}");
    let live = step(&report, StepId::Live);
    // No write for 10 s during live mode: the tablet drops the link at 10 s and 20 s and the
    // manager reconnects each time; the drop at 30 s comes when live mode is over.
    assert_eq!(detail(live, "reconnections"), "2");
    let samples: u64 = detail(live, "samples").parse().unwrap();
    assert!((5_600..=6_000).contains(&samples), "{samples}");
    say!(
        "links lost by the tablet: {}; live mode reconnected twice; 0 failures",
        tablet.stats().links_lost
    );
}
