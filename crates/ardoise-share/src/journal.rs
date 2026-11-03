//! The share's journal (diagnostics): one line per connection event, so that a share that is
//! slow or refused on a real network can be understood afterwards (docs/design/lan-share.md,
//! "Journal"). It never holds the code, a cookie, a token, a query string or a header's value:
//! only what was asked for, from which address, and how long each step took.

use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

pub(crate) struct Journal {
    file: Mutex<File>,
    start: Instant,
}

impl Journal {
    /// A new journal at `path`; `None` when it cannot be written (the share runs without one).
    pub fn create(path: &Path) -> Option<Journal> {
        let file = File::create(path).ok()?;
        Some(Journal {
            file: Mutex::new(file),
            start: Instant::now(),
        })
    }

    /// Writes one line, after the time since the share started.
    pub fn note(&self, line: fmt::Arguments<'_>) {
        let elapsed = self.start.elapsed().as_secs_f64();
        let mut file = self.file.lock().unwrap_or_else(PoisonError::into_inner);
        // Best effort: a journal that cannot be written never stops the share.
        let _ = writeln!(file, "{elapsed:10.3} s  {line}");
    }
}
