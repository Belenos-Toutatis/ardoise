//! Non-regression tests on the diagnostic journals of the real Bamboo Slate A4 (checkpoint H1,
//! docs/research/protocol.md §12).
//!
//! The journals are the user's private data: `testdata/private/captures/` is ignored by git, and
//! every test below skips itself when it is absent (cloud sessions, other PCs). Nothing taken
//! from a journal is written in this file: the tests read what they compare with (the report
//! `.txt`, the `page-N.bin` files) from the same folder, and only bare counts appear here.
//!
//! - Every journal decodes with the current `ardoise-proto`, with no frame left unclassified.
//! - The journal of the guided test recorded with the current command order
//!   (`2026-11-02_144819`, `ec` before paper mode) replays through the whole device stack, with
//!   the same report, the same pages and the same exchanges as the real run.
//! - The journals recorded with the former order (`ec` after paper mode, refused with
//!   `b3 01 02`) cannot replay against the current host, so only their decoding is checked, and
//!   the pages downloaded in one of them are decoded.
//!
//! `cargo test -p ardoise-sim --test real_captures -- --nocapture` prints the comparison.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ardoise_device::diagnostic::{
    self, Decision, DiagnosticConfig, Instruction, Operator, Progress, StepId, StepOutcome,
};
use ardoise_device::identity::{self, DEVICE_FORMAT, DeviceRecord, HostId};
use ardoise_device::log::{self, Direction, LOG_FORMAT, LogEntry};
use ardoise_device::render_report;
use ardoise_device::transport::{
    Advertisement, Channel, Clock, Connector, Transport, TransportError,
};
use ardoise_proto::live::{self, LivePacket};
use ardoise_proto::message::{Dimension, Reply, Request};
use ardoise_proto::{crc, stroke, sysevent};
use ardoise_sim::{ReplayConnector, ReplayOptions, VirtualClock};

/// The guided test recorded with the current command order.
const CURRENT: &str = "2026-11-02_144819";
/// The former-order session that downloaded pages.
const OLD_WITH_PAGES: &str = "2026-11-02_115821";

// ---------------------------------------------------------------------------------------
// The private captures (git-ignored, never committed).

/// `testdata/private/captures` of the main repository (found by walking up from this crate, so
/// it also works from a git worktree), or `ARDOISE_CAPTURES`.
fn private_captures() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("ARDOISE_CAPTURES").filter(|v| !v.is_empty()) {
        let dir = PathBuf::from(dir);
        return dir.is_dir().then_some(dir);
    }
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|a| a.join("testdata").join("private").join("captures"))
        .find(|candidate| candidate.is_dir())
}

/// The captures folder, or `None` after printing why the test is skipped.
fn captures_or_skip(test: &str) -> Option<PathBuf> {
    let found = private_captures();
    if found.is_none() {
        println!(
            "{test}: SKIPPED - no private captures (testdata/private/captures or ARDOISE_CAPTURES)"
        );
    }
    found
}

/// Every `.jsonl` journal of the folder, by name.
fn journals(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    paths.sort();
    paths
}

fn stem(path: &Path) -> String {
    path.file_stem().unwrap().to_string_lossy().into_owned()
}

// ---------------------------------------------------------------------------------------
// Decoding.

/// What the current protocol makes of a frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// Decoded and understood.
    Understood,
    /// Seen from a real Slate, named in protocol.md §12 as of unknown meaning, and ignored.
    Named,
    /// A chunk of a stored page.
    PageData,
}

/// Classifies an `rx` / `tx` entry; `Err` explains why it is not classified.
fn classify(entry: &LogEntry) -> Result<Class, String> {
    let bytes = entry.bytes().map_err(|e| e.to_string())?;
    match entry.dir {
        Direction::Tx => match Request::decode(&bytes) {
            Ok(Request::Unknown(_)) => Err("unknown request".into()),
            Ok(request) if request.encode().as_deref() != Ok(bytes.as_slice()) => {
                Err("does not encode back to the same bytes".into())
            }
            Ok(_) => Ok(Class::Understood),
            Err(error) => Err(format!("malformed request: {error}")),
        },
        Direction::Rx => match entry.ch.ok_or("received without a channel")? {
            Channel::Command => match Reply::decode(&bytes) {
                Ok(reply) if reply.is_live_button() => Ok(Class::Named),
                Ok(Reply::Unknown(_)) => Err("unknown reply".into()),
                // A Slate sends the firmware version one digit per byte: the decoded text
                // encodes back as ASCII, not as the same bytes.
                Ok(Reply::Firmware { .. }) => Ok(Class::Understood),
                Ok(reply) if reply.encode().as_deref() != Ok(bytes.as_slice()) => {
                    Err("does not encode back to the same bytes".into())
                }
                Ok(_) => Ok(Class::Understood),
                Err(error) => Err(format!("malformed reply: {error}")),
            },
            Channel::PageData => Ok(Class::PageData),
            Channel::Live => match live::decode(&bytes) {
                Ok(LivePacket::Unexplained(_)) => Ok(Class::Named),
                Ok(LivePacket::Unknown(_)) => Err("unknown live packet".into()),
                Ok(_) => Ok(Class::Understood),
                Err(error) => Err(format!("malformed live packet: {error}")),
            },
            Channel::SysEvent => match sysevent::decode(&bytes) {
                sysevent::SysEvent::BatteryLevel { .. } => Ok(Class::Understood),
                sysevent::SysEvent::Unexplained { .. } => Ok(Class::Named),
                sysevent::SysEvent::Unknown(_) => Err("unknown sysevent".into()),
            },
        },
        Direction::Event | Direction::Note => Ok(Class::Understood),
    }
}

/// The link events the replay and the log know.
fn known_event(info: &str) -> bool {
    matches!(info, "connected" | "disconnect" | "disconnected")
        || info.starts_with("write failed: ")
        || info.starts_with("receive failed: ")
}

/// A page downloaded in a journal: the bytes received on the page-data channel between the
/// `c8 .. be` and the `c8 05 ed <crc>` replies, and the CRC announced at the end.
struct Download {
    bytes: Vec<u8>,
    announced_crc: u32,
}

fn downloads(entries: &[LogEntry]) -> Vec<Download> {
    let mut pages = Vec::new();
    let mut current: Option<Vec<u8>> = None;
    for entry in entries.iter().filter(|e| e.dir == Direction::Rx) {
        let bytes = entry.bytes().unwrap();
        match entry.ch {
            Some(Channel::Command) => match Reply::decode(&bytes) {
                Ok(Reply::DownloadStart { .. }) => current = Some(Vec::new()),
                Ok(Reply::DownloadEnd { crc }) => {
                    if let Some(bytes) = current.take() {
                        pages.push(Download {
                            bytes,
                            announced_crc: crc,
                        });
                    }
                }
                _ => {}
            },
            Some(Channel::PageData) => {
                if let Some(page) = &mut current {
                    page.extend_from_slice(&bytes);
                }
            }
            _ => {}
        }
    }
    pages
}

#[test]
fn every_capture_decodes_with_the_current_protocol() {
    let _report = common::report("every_capture_decodes_with_the_current_protocol");
    let Some(dir) = captures_or_skip("every_capture_decodes_with_the_current_protocol") else {
        return;
    };
    let paths = journals(&dir);
    assert!(!paths.is_empty(), "no journal in the captures folder");
    let mut unclassified = Vec::new();
    let mut totals: BTreeMap<Class, usize> = BTreeMap::new();
    let mut frames = 0;
    for path in &paths {
        let name = stem(path);
        let (header, entries) =
            log::read_log(path).unwrap_or_else(|e| panic!("{name} does not parse: {e}"));
        assert_eq!(header.format, LOG_FORMAT, "{name}");
        assert!(!entries.is_empty(), "{name} is empty");
        let mut counts: BTreeMap<Class, usize> = BTreeMap::new();
        for (index, entry) in entries.iter().enumerate() {
            // `index + 2`: the header is the first line of the file.
            let at = format!("{name} line {} (t={:.3} s)", index + 2, entry.t);
            if entry.dir == Direction::Event && !known_event(&entry.info) {
                unclassified.push(format!("{at}: event {:?}", entry.info));
            }
            match classify(entry) {
                Ok(class) if matches!(entry.dir, Direction::Tx | Direction::Rx) => {
                    *counts.entry(class).or_default() += 1;
                }
                Ok(_) => {}
                Err(why) => unclassified.push(format!(
                    "{at}: {:?} {:?} {}: {why}",
                    entry.dir,
                    entry.ch,
                    entry.hex.as_deref().unwrap_or("")
                )),
            }
        }
        for (class, n) in &counts {
            *totals.entry(*class).or_default() += n;
            frames += n;
        }
        say!(
            "{name}: {} entries, {:?}",
            entries.len(),
            counts.iter().collect::<Vec<_>>()
        );
    }
    say!(
        "{} journals, {frames} frames in all: {:?}",
        paths.len(),
        totals
    );
    assert!(
        unclassified.is_empty(),
        "frames left unclassified:\n{}",
        unclassified.join("\n")
    );
    say!("every frame is classified");
}

#[test]
fn the_old_session_that_downloaded_pages_decodes_its_strokes() {
    let _report = common::report("the_old_session_that_downloaded_pages_decodes_its_strokes");
    let Some(dir) = captures_or_skip("the_old_session_that_downloaded_pages_decodes_its_strokes")
    else {
        return;
    };
    let path = dir.join(format!("{OLD_WITH_PAGES}.jsonl"));
    if !path.is_file() {
        println!(
            "the_old_session_that_downloaded_pages_decodes_its_strokes: SKIPPED - no such journal"
        );
        return;
    }
    let (_, entries) = log::read_log(&path).unwrap();
    let pages = downloads(&entries);
    assert_eq!(pages.len(), 4, "the session downloaded 4 pages");
    for (index, page) in pages.iter().enumerate() {
        assert_eq!(
            crc::crc32(&page.bytes),
            page.announced_crc,
            "page {}: the CRC announced by the tablet",
            index + 1
        );
        let file = stroke::decode(&page.bytes)
            .unwrap_or_else(|e| panic!("page {} does not decode: {e}", index + 1));
        say!(
            "page {}: {} bytes, {} strokes, {} points, {} unknown packets, eof {}",
            index + 1,
            page.bytes.len(),
            file.strokes.len(),
            file.point_count(),
            file.unknown_packets,
            file.eof
        );
        assert!(!file.strokes.is_empty(), "page {} has strokes", index + 1);
        assert!(file.point_count() > 0, "page {} has points", index + 1);
        assert_eq!(file.unknown_packets, 0, "page {}", index + 1);
        assert!(
            file.strokes
                .iter()
                .flat_map(|s| &s.points)
                .all(|p| p.x != 0 || p.y != 0),
            "page {}: points at the origin",
            index + 1
        );
    }
}

// ---------------------------------------------------------------------------------------
// Replay of the guided test recorded with the current code.

/// Time of an entry on the replay clock (the journal rounds `t` to the millisecond).
fn at(entry: &LogEntry) -> Duration {
    Duration::from_millis((entry.t * 1000.0).round().max(0.0) as u64)
}

/// The person in front of the tablet, replayed: answers every prompt with "continue", and lets
/// the time pass that the person took (the journal shows it as a gap before the step's first
/// entry) so that the next recorded reply is due when the host asks for it.
struct RecordedUser {
    clock: Arc<VirtualClock>,
    /// For each step that prompts, when its first entry after the prompt was recorded.
    resumes: BTreeMap<String, Duration>,
}

impl RecordedUser {
    fn new(clock: Arc<VirtualClock>, entries: &[LogEntry]) -> Self {
        let mut resumes = BTreeMap::new();
        for step in StepId::ALL {
            let marker = format!("step {step:?}");
            let started = entries
                .iter()
                .position(|e| e.dir == Direction::Note && e.info == marker);
            if let Some(next) = started.and_then(|i| entries.get(i + 1)) {
                resumes.insert(format!("{step:?}"), at(next));
            }
        }
        Self { clock, resumes }
    }
}

impl Operator for RecordedUser {
    fn prompt(&mut self, step: StepId, _instruction: &Instruction) -> Decision {
        if let Some(resume) = self.resumes.get(&format!("{step:?}")) {
            self.clock.advance_to(*resume);
        }
        Decision::Continue
    }
    fn notify(&mut self, _step: StepId, _instruction: &Instruction) {}
    fn progress(&mut self, _step: StepId, _progress: &Progress) {}
    fn finished(&mut self, _outcome: &StepOutcome) {}
    fn cancelled(&self) -> bool {
        false
    }
}

/// How one connection attempt of the journal ended (read from its notes and events).
#[derive(Debug, Clone, Copy)]
enum Attempt {
    Pending,
    /// The link opened (the `connected` event) at this time.
    Connected(Duration),
    /// The attempt failed, as the runner's note says, at this time.
    Failed(Duration),
}

/// The replay connector, plus what a journal only shows in its timestamps and notes: the time
/// a scan or a connection attempt took, and that an attempt failed (`TransportError::NotFound`,
/// "direct reconnection failed" in the Reconnect step). The replay rules themselves, the
/// strict matching of every written frame included, are the replay connector's, unchanged.
struct Recorded {
    inner: ReplayConnector,
    clock: Arc<VirtualClock>,
    attempts: Vec<Attempt>,
    attempt: usize,
    found: Vec<Duration>,
    seen: usize,
}

impl Recorded {
    fn new(inner: ReplayConnector, entries: &[LogEntry]) -> Self {
        let mut attempts: Vec<Attempt> = Vec::new();
        let mut found = Vec::new();
        for entry in entries {
            match entry.dir {
                Direction::Note if entry.info.starts_with("connecting to ") => {
                    attempts.push(Attempt::Pending);
                }
                Direction::Note if entry.info.starts_with("direct reconnection failed: ") => {
                    if let Some(last @ Attempt::Pending) = attempts.last_mut() {
                        *last = Attempt::Failed(at(entry));
                    }
                }
                Direction::Note if entry.info.starts_with("found ") => found.push(at(entry)),
                Direction::Event if entry.info == "connected" => {
                    if let Some(last @ Attempt::Pending) = attempts.last_mut() {
                        *last = Attempt::Connected(at(entry));
                    }
                }
                _ => {}
            }
        }
        let clock = inner.replay_clock();
        Self {
            inner,
            clock,
            attempts,
            attempt: 0,
            found,
            seen: 0,
        }
    }
}

impl Connector for Recorded {
    fn clock(&self) -> Arc<dyn Clock> {
        self.inner.clock()
    }

    fn find(
        &mut self,
        timeout: Duration,
        filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError> {
        let found = self.inner.find(timeout, filter)?;
        if found.is_some() {
            if let Some(time) = self.found.get(self.seen) {
                self.clock.advance_to(*time);
            }
            self.seen += 1;
        }
        Ok(found)
    }

    fn connect(
        &mut self,
        address: &str,
        timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        let attempt = self.attempts.get(self.attempt).copied();
        self.attempt += 1;
        match attempt {
            Some(Attempt::Failed(time)) => {
                self.clock.advance_to(time);
                Err(TransportError::NotFound(address.to_owned()))
            }
            Some(Attempt::Connected(time)) => {
                self.clock.advance_to(time);
                self.inner.connect(address, timeout)
            }
            _ => self.inner.connect(address, timeout),
        }
    }
}

/// The registration of the library the journal was recorded with, rebuilt from the journal:
/// the identity of its first CONNECT, the address and name of the tablet found, its size.
fn registration_of(header: &log::LogHeader, entries: &[LogEntry]) -> DeviceRecord {
    let uuid = entries
        .iter()
        .filter(|e| e.dir == Direction::Tx)
        .find_map(|e| match Request::decode(&e.bytes().ok()?) {
            Ok(Request::Connect { uuid }) => Some(uuid),
            _ => None,
        })
        .expect("a CONNECT in the journal");
    let (name, address) = entries
        .iter()
        .filter(|e| e.dir == Direction::Note)
        .find_map(|e| {
            let found = e.info.strip_prefix("found ")?;
            let (tablet, _) = found.split_once(" (rssi ")?;
            let (name, address) = tablet.rsplit_once(" at ")?;
            Some((name.to_owned(), address.to_owned()))
        })
        .expect("a tablet found in the journal");
    let dimension = |which| {
        entries
            .iter()
            .filter(|e| e.dir == Direction::Rx && e.ch == Some(Channel::Command))
            .find_map(|e| match Reply::decode(&e.bytes().ok()?) {
                Ok(Reply::Dimension { which: w, value }) if w == which => Some(value),
                _ => None,
            })
            .expect("a dimension in the journal")
    };
    DeviceRecord {
        format: DEVICE_FORMAT,
        uuid: HostId(uuid),
        address,
        name,
        protocol: "slate".into(),
        width: dimension(Dimension::Width),
        height: dimension(Dimension::Height),
        registered: header.created.clone(),
        extra: Default::default(),
    }
}

/// One step of a text report: its status tag and the values of its detail lines.
#[derive(Debug, PartialEq, Eq)]
struct Block {
    status: String,
    values: Vec<String>,
}

/// The steps of a report written by `render_report`, and its last line (the summary). Compared
/// by status and values, not by wording. Left out: the adapter description (the replay has no
/// Bluetooth adapter) and the folder of the saved pages (another library).
fn blocks(text: &str) -> (Vec<Block>, String) {
    let mut blocks: Vec<Block> = Vec::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix('[') {
            let status = rest.split(']').next().unwrap_or_default().to_owned();
            blocks.push(Block {
                status,
                values: Vec::new(),
            });
        } else if line.starts_with("    ")
            && let Some(block) = blocks.last_mut()
            && let Some((_, value)) = line.trim().split_once(" : ")
            && !value.contains("diagnostics")
        {
            block.values.push(value.to_owned());
        }
    }
    // The adapter step is the first one.
    if let Some(first) = blocks.first_mut() {
        first.values.clear();
    }
    let summary = text
        .lines()
        .rev()
        .find(|l| !l.is_empty())
        .unwrap_or_default();
    (blocks, summary.to_owned())
}

/// Two report values are the same, except that two measures are only reproduced approximately:
/// the clock drift (`-1 s`; the journal gives its creation time to the second, so the replay
/// clock may be up to a second off the real one) and the sample rate (`230 points/s`; measured
/// on real time, the journal rounds its times to the millisecond): 1 s and 2 % apart at most.
fn same_value(replayed: &str, real: &str) -> bool {
    let number =
        |text: &str, unit: &str| -> Option<f64> { text.strip_suffix(unit)?.trim().parse().ok() };
    if let (Some(a), Some(b)) = (number(replayed, " points/s"), number(real, " points/s")) {
        return (a - b).abs() <= 0.02 * b;
    }
    if let (Some(a), Some(b)) = (number(replayed, " s"), number(real, " s")) {
        return (a - b).abs() <= 1.0;
    }
    replayed == real
}

/// SET_TIME carries the time of the capture: only its opcode and length are compared.
fn comparable(entries: &[LogEntry]) -> Vec<(Direction, Option<Channel>, Option<String>, String)> {
    entries
        .iter()
        .filter(|e| e.dir != Direction::Note)
        .map(|e| {
            let hex = e.hex.clone().map(|hex| {
                if e.dir == Direction::Tx && hex.starts_with("b6 06 ") {
                    "b6 06".to_owned()
                } else {
                    hex
                }
            });
            let info = if e.dir == Direction::Event {
                e.info.clone()
            } else {
                String::new()
            };
            (e.dir, e.ch, hex, info)
        })
        .collect()
}

/// The notes of the application, without the paths of the notes saved in the library and
/// without the signal strength of a tablet found (the replay reports the first one it finds
/// every time, while the real one varied between scans).
fn notes(entries: &[LogEntry]) -> Vec<String> {
    entries
        .iter()
        .filter(|e| e.dir == Direction::Note)
        .map(|e| {
            if let Some((page, _)) = e.info.split_once(" saved as ") {
                format!("{page} saved")
            } else if e.info.starts_with("found ")
                && let Some((tablet, _)) = e.info.split_once(" (rssi ")
            {
                tablet.to_owned()
            } else {
                e.info.clone()
            }
        })
        .collect()
}

#[test]
fn the_guided_test_recorded_with_the_current_code_replays_as_the_real_run() {
    let _report =
        common::report("the_guided_test_recorded_with_the_current_code_replays_as_the_real_run");
    let Some(dir) =
        captures_or_skip("the_guided_test_recorded_with_the_current_code_replays_as_the_real_run")
    else {
        return;
    };
    let path = dir.join(format!("{CURRENT}.jsonl"));
    let report_text = dir.join(format!("{CURRENT}.txt"));
    let pages_dir = dir.join(CURRENT);
    if !path.is_file() || !report_text.is_file() || !pages_dir.is_dir() {
        println!(
            "the_guided_test_recorded_with_the_current_code_replays_as_the_real_run: SKIPPED - \
             the capture of the guided test is incomplete"
        );
        return;
    }
    let (header, entries) = log::read_log(&path).unwrap();
    let real_report = std::fs::read_to_string(&report_text).unwrap();
    let (real_blocks, real_summary) = blocks(&real_report);
    assert_eq!(real_blocks.len(), StepId::ALL.len());
    assert!(real_blocks.iter().all(|b| b.status == "RÉUSSI"));

    // The pages the real run saved are the ones the journal carries, and hold the counts of the
    // report.
    let mut saved_pages = Vec::new();
    for number in 1.. {
        match std::fs::read(pages_dir.join(format!("page-{number}.bin"))) {
            Ok(bytes) => saved_pages.push(bytes),
            Err(_) => break,
        }
    }
    let received = downloads(&entries);
    assert_eq!(saved_pages.len(), 3);
    assert_eq!(received.len(), saved_pages.len());
    for (index, (saved, received)) in saved_pages.iter().zip(&received).enumerate() {
        assert_eq!(saved, &received.bytes, "page {} of the journal", index + 1);
        assert_eq!(crc::crc32(saved), received.announced_crc);
    }
    let decoded: Vec<_> = saved_pages
        .iter()
        .map(|bytes| stroke::decode(bytes).unwrap())
        .collect();
    let strokes: usize = decoded.iter().map(|f| f.strokes.len()).sum();
    let points: usize = decoded.iter().map(|f| f.point_count()).sum();
    say!(
        "real run: {} pages, {strokes} strokes, {points} points; {}",
        saved_pages.len(),
        real_summary
    );
    assert_eq!((strokes, points), (26, 7_834));

    // Replay through the whole device stack, in a library holding the registration the real run
    // had.
    let library = tempfile::tempdir().unwrap();
    identity::save(library.path(), &registration_of(&header, &entries)).unwrap();
    let replay = ReplayConnector::from_file(
        &path,
        ReplayOptions {
            lenient_set_time: true,
            peer: None,
        },
    )
    .unwrap();
    let mut user = RecordedUser::new(replay.replay_clock(), &entries);
    let connector = Recorded::new(replay.clone(), &entries);
    let mut config = DiagnosticConfig::new(library.path().to_owned(), "replay");
    config.language = "fr".into();
    let replayed = diagnostic::run(Box::new(connector), &mut user, &config, None);
    for line in render_report(&replayed, "fr").lines() {
        say!("    {line}");
    }

    assert_eq!(replay.failure(), None, "the host followed the journal");
    assert_eq!(replay.remaining(), 0, "the whole journal was replayed");
    assert_eq!(replayed.failures(), 0, "{replayed:#?}");
    assert_eq!(replayed.steps.len(), StepId::ALL.len());

    // The same report as the real run, step by step.
    let replayed_text = std::fs::read_to_string(replayed.report_path.as_ref().unwrap()).unwrap();
    let (replayed_blocks, replayed_summary) = blocks(&replayed_text);
    assert_eq!(replayed_blocks.len(), real_blocks.len());
    for (replayed, real) in replayed_blocks.iter().zip(&real_blocks) {
        assert_eq!(replayed.status, real.status);
        assert_eq!(replayed.values.len(), real.values.len(), "{replayed:?}");
        for (replayed, real) in replayed.values.iter().zip(&real.values) {
            assert!(
                same_value(replayed, real),
                "{replayed:?} instead of {real:?}"
            );
        }
    }
    assert_eq!(replayed_summary, real_summary);

    // The same pages, byte for byte.
    let replayed_log = replayed.log_path.clone().unwrap();
    for (index, saved) in saved_pages.iter().enumerate() {
        let name = format!("page-{}.bin", index + 1);
        let again = std::fs::read(replayed_log.with_extension("").join(&name)).unwrap();
        assert_eq!(&again, saved, "{name}");
    }
    assert!(!replayed_log.with_extension("").join("page-4.bin").exists());

    // The same exchanges: every frame in the same order (SET_TIME aside), the same link
    // events, the same notes, and each reply at its recorded time.
    let (_, replayed_entries) = log::read_log(&replayed_log).unwrap();
    assert_eq!(comparable(&replayed_entries), comparable(&entries));
    assert_eq!(notes(&replayed_entries), notes(&entries));
    let reply_times = |entries: &[LogEntry]| {
        entries
            .iter()
            .filter(|e| e.dir == Direction::Rx)
            .map(|e| e.t.to_bits())
            .collect::<Vec<_>>()
    };
    assert_eq!(reply_times(&replayed_entries), reply_times(&entries));
    say!(
        "replayed: {} entries, same report, same pages, same exchanges and notes",
        replayed_entries.len()
    );
}
