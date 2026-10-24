//! The service's diagnostic log (docs/design/tablet-live-classroom.md, "Log"): one log in memory
//! per connection attempt, written to `<identity root>/_ardoise/diagnostics/` only when something
//! went wrong, or when live mode received a notification it could not classify (so that the log
//! of the hardware checkpoint H2 shows it). The files reach the development PC through Nextcloud.
//!
//! The library is a synced folder: a log is written atomically, and rewritten (same file) when
//! the same connection has more to say. A log that grew too long is written by a thread of its
//! own ([`Journal::roll`]), so that live mode is never held up by the disk.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ardoise_core::fsutil::write_atomic;
use ardoise_core::library::META_DIR;

use crate::log::{DIAGNOSTICS_DIR, DiagnosticLog, SharedLog, lock};
use crate::transport::Clock;

/// Lines kept in memory for one connection. Beyond, the log starts again (written first when
/// it must be), so that hours of live mode in class do not fill the memory.
pub(super) const MAX_ENTRIES: usize = 20_000;

pub(super) struct Journal {
    shared: SharedLog,
    transport: &'static str,
    clock: Arc<dyn Clock>,
    /// Why this log must be written, if it must.
    reason: Option<String>,
    /// The file this log was written to.
    path: Option<PathBuf>,
    /// Reason of the last log written: a failure repeated at every retry (every 5 s) is
    /// written once, until a success.
    last_reason: Option<String>,
    /// Files that a background write ([`Journal::roll`]) has not finished yet: never chosen for
    /// another log meanwhile.
    writing: Arc<Mutex<HashSet<PathBuf>>>,
}

impl Journal {
    pub(super) fn new(transport: &'static str, clock: Arc<dyn Clock>) -> Self {
        let shared = DiagnosticLog::in_memory(transport, clock.clone()).shared();
        Self {
            shared,
            transport,
            clock,
            reason: None,
            path: None,
            last_reason: None,
            writing: Arc::default(),
        }
    }

    /// The log the `Manager` and its transports write to.
    pub(super) fn shared(&self) -> SharedLog {
        self.shared.clone()
    }

    /// Starts a fresh log: what was recorded so far is dropped (or was written).
    pub(super) fn begin(&mut self) {
        *lock(&self.shared) = DiagnosticLog::in_memory(self.transport, self.clock.clone());
        self.reason = None;
        self.path = None;
    }

    pub(super) fn note(&self, text: impl Into<String>) {
        lock(&self.shared).note(text);
    }

    /// The log must be written, because of `reason`.
    pub(super) fn keep(&mut self, reason: impl Into<String>) {
        let reason = reason.into();
        self.note(format!("problem: {reason}"));
        if self.reason.is_none() {
            self.reason = Some(reason);
        }
    }

    /// A success: the next failure is written even if it repeats the last one.
    pub(super) fn success(&mut self) {
        self.last_reason = None;
    }

    /// Writes the log into `root`'s diagnostics folder if it must be. Best effort: a log that
    /// cannot be written is not worth failing anything.
    pub(super) fn write(&mut self, root: Option<&Path>) -> Option<PathBuf> {
        let path = self.target(root)?;
        let text = text_of(&lock(&self.shared));
        write_atomic(&path, text.as_bytes()).ok()?;
        self.path = Some(path.clone());
        self.last_reason.clone_from(&self.reason);
        Some(path)
    }

    /// Starts again when the log grew too long. The lines dropped are written first if the log
    /// must be, on a thread of its own: thousands of lines to encode and write must not delay
    /// the notifications that follow (live mode).
    pub(super) fn roll(&mut self, root: Option<&Path>) {
        if lock(&self.shared).entries().len() < MAX_ENTRIES {
            return;
        }
        let target = self.target(root);
        let old = std::mem::replace(
            &mut *lock(&self.shared),
            DiagnosticLog::in_memory(self.transport, self.clock.clone()),
        );
        if let Some(path) = target {
            self.last_reason.clone_from(&self.reason);
            let writing = self.writing.clone();
            reserved(&writing).insert(path.clone());
            let spawned = std::thread::Builder::new()
                .name("tablet-log".into())
                .spawn(move || {
                    // Best effort, like `write`.
                    let _ = write_atomic(&path, text_of(&old).as_bytes());
                    reserved(&writing).remove(&path);
                });
            if spawned.is_err() {
                // The lines are lost (no thread to write them): their file name is free again.
                reserved(&self.writing).clear();
            }
        }
        self.reason = None;
        self.path = None;
        self.note("earlier lines of this connection were dropped (log too long)");
    }

    /// Where this log goes if it must be written now: the file it was already written to, or a
    /// new one in `root`'s diagnostics folder. `None` when it need not be written: nothing went
    /// wrong, no folder, or the same failure as the last log written.
    fn target(&self, root: Option<&Path>) -> Option<PathBuf> {
        let reason = self.reason.as_ref()?;
        let root = root?;
        match &self.path {
            Some(path) => Some(path.clone()),
            None if self.last_reason.as_ref() == Some(reason) => None,
            None => self.new_path(root),
        }
    }

    /// A free `YYYY-MM-DD_HHMMSS.jsonl` (local time) in the diagnostics folder.
    fn new_path(&self, root: &Path) -> Option<PathBuf> {
        let dir = root.join(META_DIR).join(DIAGNOSTICS_DIR);
        fs::create_dir_all(&dir).ok()?;
        let local = chrono::DateTime::<chrono::Local>::from(self.clock.now());
        let stem = local.format("%Y-%m-%d_%H%M%S").to_string();
        let mut path = dir.join(format!("{stem}.jsonl"));
        let mut n = 2;
        let writing = reserved(&self.writing);
        while path.exists() || writing.contains(&path) {
            path = dir.join(format!("{stem}-{n}.jsonl"));
            n += 1;
        }
        Some(path)
    }
}

/// The `.jsonl` text of `log` (docs/design/device-stack.md, "Diagnostic log").
fn text_of(log: &DiagnosticLog) -> String {
    let mut text = String::new();
    let lines = std::iter::once(serde_json::to_string(log.header()))
        .chain(log.entries().iter().map(serde_json::to_string));
    for line in lines.flatten() {
        text.push_str(&line);
        text.push('\n');
    }
    text
}

fn reserved(writing: &Mutex<HashSet<PathBuf>>) -> MutexGuard<'_, HashSet<PathBuf>> {
    writing.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::read_log;
    use crate::testutil::TestClock;
    use std::sync::atomic::AtomicU64;
    use std::time::{Duration, Instant};

    fn journal() -> Journal {
        Journal::new("sim", Arc::new(TestClock(AtomicU64::new(0))))
    }

    #[test]
    fn only_logs_that_must_be_kept_are_written_and_repeats_once() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = journal();
        journal.note("all is well");
        assert_eq!(journal.write(Some(root.path())), None);

        journal.keep("tabletRefused");
        let first = journal.write(Some(root.path())).unwrap();
        let (header, entries) = read_log(&first).unwrap();
        assert_eq!(header.transport, "sim");
        assert_eq!(entries.len(), 2);
        // The same connection says more: same file.
        journal.note("more");
        assert_eq!(journal.write(Some(root.path())), Some(first.clone()));
        assert_eq!(read_log(&first).unwrap().1.len(), 3);

        // The same failure at the next retry is not written again...
        journal.begin();
        journal.keep("tabletRefused");
        assert_eq!(journal.write(Some(root.path())), None);
        // ...until a success.
        journal.success();
        journal.begin();
        journal.keep("tabletRefused");
        let second = journal.write(Some(root.path())).unwrap();
        assert_ne!(first, second);
        // Without a folder nothing is written.
        assert_eq!(journal.write(None), None);
    }

    #[test]
    fn long_logs_start_again() {
        let mut journal = journal();
        for i in 0..MAX_ENTRIES {
            journal.note(format!("line {i}"));
        }
        journal.roll(None);
        assert_eq!(lock(&journal.shared()).entries().len(), 1);
    }

    #[test]
    fn a_long_log_that_must_be_kept_is_written_by_another_thread() {
        let root = tempfile::tempdir().unwrap();
        let mut journal = journal();
        journal.keep("unclassified notification during live mode");
        for i in 1..MAX_ENTRIES {
            journal.note(format!("line {i}"));
        }
        journal.roll(Some(root.path()));
        // Started again at once.
        assert_eq!(lock(&journal.shared()).entries().len(), 1);
        // A log written meanwhile (the same second on this clock) never takes the same file.
        journal.keep("tabletRefused");
        let next = journal.write(Some(root.path())).unwrap();
        assert_eq!(read_log(&next).unwrap().1.len(), 2);

        // The background write ends by giving its file name back.
        let deadline = Instant::now() + Duration::from_secs(10);
        while !reserved(&journal.writing).is_empty() {
            assert!(Instant::now() < deadline, "the long log is not written");
            std::thread::sleep(Duration::from_millis(10));
        }
        let dir = root.path().join(META_DIR).join(DIAGNOSTICS_DIR);
        let others: Vec<PathBuf> = fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| *path != next)
            .collect();
        assert_eq!(others.len(), 1, "{others:?}");
        assert_eq!(read_log(&others[0]).unwrap().1.len(), MAX_ENTRIES);
    }
}
