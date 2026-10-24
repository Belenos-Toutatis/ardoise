//! The service's diagnostic log (docs/design/tablet-live-classroom.md, "Log"): one log in memory
//! per connection attempt, written to `<identity root>/_ardoise/diagnostics/` only when something
//! went wrong, or when live mode received a notification it could not classify (so that the log
//! of the hardware checkpoint H2 shows it). The files reach the development PC through Nextcloud.
//!
//! The library is a synced folder: a log is written atomically, and rewritten (same file) when
//! the same connection has more to say.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
        let reason = self.reason.clone()?;
        let root = root?;
        if self.path.is_none() && self.last_reason.as_ref() == Some(&reason) {
            return None;
        }
        let text = self.text();
        let path = match &self.path {
            Some(path) => path.clone(),
            None => self.new_path(root)?,
        };
        write_atomic(&path, text.as_bytes()).ok()?;
        self.path = Some(path.clone());
        self.last_reason = Some(reason);
        Some(path)
    }

    /// Starts again when the log grew too long (written first if it must be).
    pub(super) fn roll(&mut self, root: Option<&Path>) {
        if lock(&self.shared).entries().len() < MAX_ENTRIES {
            return;
        }
        self.write(root);
        self.begin();
        self.note("earlier lines of this connection were dropped (log too long)");
    }

    /// The `.jsonl` text of the log (docs/design/device-stack.md, "Diagnostic log").
    fn text(&self) -> String {
        let log = lock(&self.shared);
        let mut text = String::new();
        let lines = std::iter::once(serde_json::to_string(log.header()))
            .chain(log.entries().iter().map(serde_json::to_string));
        for line in lines.flatten() {
            text.push_str(&line);
            text.push('\n');
        }
        text
    }

    /// A free `YYYY-MM-DD_HHMMSS.jsonl` (local time) in the diagnostics folder.
    fn new_path(&self, root: &Path) -> Option<PathBuf> {
        let dir = root.join(META_DIR).join(DIAGNOSTICS_DIR);
        fs::create_dir_all(&dir).ok()?;
        let local = chrono::DateTime::<chrono::Local>::from(self.clock.now());
        let stem = local.format("%Y-%m-%d_%H%M%S").to_string();
        let mut path = dir.join(format!("{stem}.jsonl"));
        let mut n = 2;
        while path.exists() {
            path = dir.join(format!("{stem}-{n}.jsonl"));
            n += 1;
        }
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::read_log;
    use crate::testutil::TestClock;
    use std::sync::atomic::AtomicU64;

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
}
