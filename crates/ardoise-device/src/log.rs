//! Diagnostic log of every exchange with the tablet (SPEC §4.8).
//!
//! One JSON object per line in `<library>/.ardoise/diagnostics/YYYY-MM-DD_HHMMSS.jsonl`, so the
//! logs written at work reach the development PC through Nextcloud. The first line is a
//! [`LogHeader`], every other line a [`LogEntry`]. The same files are replayed by `ardoise-sim`
//! as test data.
//!
//! The library is a synced folder: the file is never appended to in place. Lines are kept in
//! memory and the whole file is rewritten atomically at most every [`FLUSH_INTERVAL`] while
//! there is something new, and when the log is flushed or dropped.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use ardoise_core::fsutil::write_atomic;
use ardoise_core::library::META_DIR;

use crate::transport::{Channel, Clock};

/// Sub-folder of `.ardoise/` holding the logs.
pub const DIAGNOSTICS_DIR: &str = "diagnostics";
/// Version of the log format, written in the header.
pub const LOG_FORMAT: u32 = 1;
/// Longest time new lines stay only in memory.
pub const FLUSH_INTERVAL: Duration = Duration::from_secs(2);

/// What a log line records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    /// Bytes written by the computer to the tablet.
    Tx,
    /// Bytes received from the tablet.
    Rx,
    /// Something that happened to the link (connected, disconnected…).
    Event,
    /// A remark of the application (diagnostic step, decision, error).
    Note,
}

/// First line of a log.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct LogHeader {
    /// Always `"header"`.
    #[serde(rename = "type")]
    pub kind: String,
    pub format: u32,
    /// Version of the application that wrote the log.
    pub app: String,
    /// `"ble"`, `"sim"` or `"replay"`.
    pub transport: String,
    /// Local time of creation, RFC 3339.
    pub created: String,
}

/// One recorded exchange or remark.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LogEntry {
    /// Seconds since the log was created, on the session clock.
    pub t: f64,
    pub dir: Direction,
    /// Channel of received bytes; `Command` for written bytes; absent for events and notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ch: Option<Channel>,
    /// The raw bytes, `e6 06 89 ea …`; absent for events and notes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hex: Option<String>,
    /// Human-readable interpretation.
    pub info: String,
}

impl LogEntry {
    /// The raw bytes of a `tx`/`rx` line.
    pub fn bytes(&self) -> Result<Vec<u8>, LogError> {
        parse_hex(self.hex.as_deref().unwrap_or_default())
    }
}

/// Errors reading a log back.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("line {line}: {message}")]
    Parse { line: usize, message: String },
    #[error("invalid hex byte {0:?}")]
    Hex(String),
}

/// A log being written.
pub struct DiagnosticLog {
    path: Option<PathBuf>,
    header: LogHeader,
    entries: Vec<LogEntry>,
    text: String,
    clock: Arc<dyn Clock>,
    origin: Duration,
    last_flush: Duration,
    dirty: bool,
}

/// A log shared between the transport decorator and the code driving the session.
pub type SharedLog = Arc<Mutex<DiagnosticLog>>;

/// Locks a shared log, even if a thread panicked while holding it (a log is never worth a
/// second panic).
pub fn lock(log: &SharedLog) -> MutexGuard<'_, DiagnosticLog> {
    log.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

impl DiagnosticLog {
    /// Starts a new log file in `<library>/.ardoise/diagnostics/`, named after the local time.
    pub fn create(library: &Path, transport: &str, clock: Arc<dyn Clock>) -> io::Result<Self> {
        let dir = library.join(META_DIR).join(DIAGNOSTICS_DIR);
        fs::create_dir_all(&dir)?;
        let local = chrono::DateTime::<chrono::Local>::from(clock.now());
        let stem = local.format("%Y-%m-%d_%H%M%S").to_string();
        let mut path = dir.join(format!("{stem}.jsonl"));
        let mut n = 2;
        while path.exists() {
            path = dir.join(format!("{stem}-{n}.jsonl"));
            n += 1;
        }
        let mut log = Self::new(Some(path), transport, clock);
        log.flush()?;
        Ok(log)
    }

    /// A log kept only in memory (tests, or no library chosen yet).
    pub fn in_memory(transport: &str, clock: Arc<dyn Clock>) -> Self {
        Self::new(None, transport, clock)
    }

    fn new(path: Option<PathBuf>, transport: &str, clock: Arc<dyn Clock>) -> Self {
        let header = LogHeader {
            kind: "header".into(),
            format: LOG_FORMAT,
            app: env!("CARGO_PKG_VERSION").into(),
            transport: transport.into(),
            created: chrono::DateTime::<chrono::Local>::from(clock.now())
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        };
        let mut text = json_line(&header);
        text.push('\n');
        let origin = clock.monotonic();
        Self {
            path,
            header,
            entries: Vec::new(),
            text,
            clock,
            origin,
            last_flush: origin,
            dirty: false,
        }
    }

    /// Wraps the log for sharing.
    pub fn shared(self) -> SharedLog {
        Arc::new(Mutex::new(self))
    }

    /// File being written, if any.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn header(&self) -> &LogHeader {
        &self.header
    }

    pub fn entries(&self) -> &[LogEntry] {
        &self.entries
    }

    /// Records bytes written to the tablet.
    pub fn tx(&mut self, data: &[u8], info: impl Into<String>) {
        self.push(
            Direction::Tx,
            Some(Channel::Command),
            Some(data),
            info.into(),
        );
    }

    /// Records bytes received from the tablet.
    pub fn rx(&mut self, channel: Channel, data: &[u8], info: impl Into<String>) {
        self.push(Direction::Rx, Some(channel), Some(data), info.into());
    }

    /// Records something that happened to the link.
    pub fn event(&mut self, info: impl Into<String>) {
        self.push(Direction::Event, None, None, info.into());
    }

    /// Records a remark of the application.
    pub fn note(&mut self, info: impl Into<String>) {
        self.push(Direction::Note, None, None, info.into());
    }

    fn push(&mut self, dir: Direction, ch: Option<Channel>, data: Option<&[u8]>, info: String) {
        let now = self.clock.monotonic();
        let t = now.saturating_sub(self.origin).as_secs_f64();
        let entry = LogEntry {
            t: (t * 1000.0).round() / 1000.0,
            dir,
            ch,
            hex: data.map(to_hex),
            info,
        };
        self.text.push_str(&json_line(&entry));
        self.text.push('\n');
        self.entries.push(entry);
        self.dirty = true;
        if now.saturating_sub(self.last_flush) >= FLUSH_INTERVAL {
            // A failed periodic write is retried at the next line or at the final flush.
            let _ = self.flush();
        }
    }

    /// Writes everything recorded so far to the file (atomically).
    pub fn flush(&mut self) -> io::Result<()> {
        self.last_flush = self.clock.monotonic();
        let Some(path) = &self.path else {
            self.dirty = false;
            return Ok(());
        };
        write_atomic(path, self.text.as_bytes())?;
        self.dirty = false;
        Ok(())
    }
}

impl Drop for DiagnosticLog {
    fn drop(&mut self) {
        if self.dirty {
            let _ = self.flush();
        }
    }
}

fn json_line<T: serde::Serialize>(value: &T) -> String {
    // Serializing these plain structs cannot fail; an empty object keeps the file valid anyway.
    serde_json::to_string(value).unwrap_or_else(|_| "{}".into())
}

/// Reads a log file back.
pub fn read_log(path: &Path) -> Result<(LogHeader, Vec<LogEntry>), LogError> {
    let text = fs::read_to_string(path).map_err(|source| LogError::Io {
        path: path.to_owned(),
        source,
    })?;
    parse_log(&text)
}

/// Parses the content of a log file.
pub fn parse_log(text: &str) -> Result<(LogHeader, Vec<LogEntry>), LogError> {
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty());
    let Some((_, first)) = lines.next() else {
        return Err(LogError::Parse {
            line: 1,
            message: "empty log".into(),
        });
    };
    let header: LogHeader = serde_json::from_str(first).map_err(|e| LogError::Parse {
        line: 1,
        message: e.to_string(),
    })?;
    if header.kind != "header" {
        return Err(LogError::Parse {
            line: 1,
            message: "the first line is not a header".into(),
        });
    }
    let entries = lines
        .map(|(index, line)| {
            serde_json::from_str(line).map_err(|e| LogError::Parse {
                line: index + 1,
                message: e.to_string(),
            })
        })
        .collect::<Result<Vec<LogEntry>, _>>()?;
    Ok((header, entries))
}

/// `[0xe6, 0x06]` -> `"e6 06"`.
pub fn to_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 3);
    for (i, byte) in bytes.iter().enumerate() {
        if i > 0 {
            text.push(' ');
        }
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// `"e6 06"` -> `[0xe6, 0x06]`.
pub fn parse_hex(text: &str) -> Result<Vec<u8>, LogError> {
    text.split_whitespace()
        .map(|pair| u8::from_str_radix(pair, 16).map_err(|_| LogError::Hex(pair.to_owned())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    /// A clock the test moves by hand.
    struct ManualClock(AtomicU64);

    impl ManualClock {
        fn advance(&self, ms: u64) {
            self.0.fetch_add(ms, Ordering::SeqCst);
        }
    }

    impl Clock for ManualClock {
        fn monotonic(&self) -> Duration {
            Duration::from_millis(self.0.load(Ordering::SeqCst))
        }
        fn now(&self) -> SystemTime {
            UNIX_EPOCH + Duration::from_secs(1_790_000_000) + self.monotonic()
        }
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(to_hex(&[0xe6, 0x06, 0x00, 0xff]), "e6 06 00 ff");
        assert_eq!(parse_hex("e6 06 00 ff").unwrap(), [0xe6, 0x06, 0x00, 0xff]);
        assert_eq!(parse_hex("").unwrap(), Vec::<u8>::new());
        assert!(matches!(parse_hex("e6 zz"), Err(LogError::Hex(_))));
    }

    #[test]
    fn writes_a_jsonl_file_that_reads_back() {
        let library = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock(AtomicU64::new(0)));
        let mut log = DiagnosticLog::create(library.path(), "sim", clock.clone()).unwrap();
        let path = log.path().unwrap().to_owned();
        assert!(path.starts_with(library.path().join(".ardoise").join("diagnostics")));
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(name.len(), "2026-10-24_140312.jsonl".len(), "{name}");

        log.tx(&[0xe6, 0x06, 1, 2, 3, 4, 5, 6], "CONNECT uuid=010203040506");
        clock.advance(1234);
        log.rx(Channel::Command, &[0xb3, 0x01, 0x00], "STATUS success");
        log.event("disconnected");
        log.note("step: info");
        log.flush().unwrap();

        let (header, entries) = read_log(&path).unwrap();
        assert_eq!(header.kind, "header");
        assert_eq!(header.format, LOG_FORMAT);
        assert_eq!(header.transport, "sim");
        assert_eq!(entries, log.entries());
        assert_eq!(entries[1].t, 1.234);
        assert_eq!(entries[1].ch, Some(Channel::Command));
        assert_eq!(entries[1].bytes().unwrap(), [0xb3, 0x01, 0x00]);
        assert_eq!(entries[2].ch, None);
        assert_eq!(entries[2].hex, None);

        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.lines()
                .nth(2)
                .unwrap()
                .contains(r#""dir":"rx","ch":"command","hex":"b3 01 00""#)
        );
    }

    #[test]
    fn flushes_periodically_and_on_drop() {
        let library = tempfile::tempdir().unwrap();
        let clock = Arc::new(ManualClock(AtomicU64::new(0)));
        let mut log = DiagnosticLog::create(library.path(), "ble", clock.clone()).unwrap();
        let path = log.path().unwrap().to_owned();
        let lines = || fs::read_to_string(&path).unwrap().lines().count();

        log.note("first");
        assert_eq!(lines(), 1, "only the header before the flush interval");
        clock.advance(2_000);
        log.note("second");
        assert_eq!(lines(), 3, "flushed once the interval elapsed");
        log.note("third");
        drop(log);
        assert_eq!(lines(), 4, "flushed when dropped");
    }

    #[test]
    fn two_logs_in_the_same_second_get_distinct_files() {
        let library = tempfile::tempdir().unwrap();
        let clock: Arc<dyn Clock> = Arc::new(ManualClock(AtomicU64::new(0)));
        let a = DiagnosticLog::create(library.path(), "sim", clock.clone()).unwrap();
        let b = DiagnosticLog::create(library.path(), "sim", clock).unwrap();
        assert_ne!(a.path(), b.path());
    }

    #[test]
    fn rejects_malformed_logs() {
        assert!(matches!(
            parse_log(""),
            Err(LogError::Parse { line: 1, .. })
        ));
        assert!(matches!(
            parse_log("{\"t\":0,\"dir\":\"note\",\"info\":\"x\"}"),
            Err(LogError::Parse { line: 1, .. })
        ));
        let header =
            r#"{"type":"header","format":1,"app":"0.1.0","transport":"sim","created":"x"}"#;
        assert!(matches!(
            parse_log(&format!("{header}\nnot json\n")),
            Err(LogError::Parse { line: 2, .. })
        ));
        assert!(parse_log(&format!("{header}\n\n")).unwrap().1.is_empty());
    }
}
