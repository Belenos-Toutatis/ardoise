//! Replay of a recorded diagnostic log (`.jsonl`, [`ardoise_device::log`]) behind the
//! transport interface, so a session recorded with the real tablet (or with the simulator)
//! runs again through the whole device stack without any tablet.
//!
//! Rules (docs/design/device-stack.md, "Replay rules"):
//!
//! - Entries are consumed in log order; `note` lines are ignored.
//! - On `write`, the next pending entry must be a `tx` with the same bytes. With
//!   [`ReplayOptions::lenient_set_time`] the 6 payload bytes of SET_TIME are exempt (a real
//!   capture was made at another time). Anything else is [`TransportError::Replay`], with the
//!   log line, the time, the expected entry and the bytes actually written. A mismatch is
//!   final: every later call returns the same error.
//! - On `recv`, a pending `rx` entry is delivered when the replay clock reaches its recorded
//!   `t` (the virtual-time rule of the simulator), an `event: disconnected` becomes
//!   [`TransportEvent::Disconnected`], and a pending `tx` means nothing to deliver (the clock
//!   advances by the timeout).
//! - `connected` / `disconnect` / `disconnected` events delimit successive connections:
//!   `connect` must meet a `connected` event, `disconnect` (or dropping the transport, which
//!   the logging transport records as `disconnect` too) a `disconnect` event.
//! - `recv(Duration::ZERO)` (the session drains what already arrived before each write)
//!   delivers only an entry recorded at or before the current replay time and never moves
//!   the clock.
//! - The replay clock starts at the log's creation time (its `t` = 0), so a log produced by
//!   the simulator replays with the same times, SET_TIME included.
//!
//! A recorded registration is replayed by giving the recorded identity to
//! `Manager::with_host_id` (or `DiagnosticConfig::host_id`). `find` returns the recorded
//! tablet, in registration mode when the next recorded connection starts with `e7`.

use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime};

use ardoise_device::log::{self, Direction, LogEntry, LogError, LogHeader};
use ardoise_device::transport::{
    Advertisement, Channel, Clock, Connector, REGISTER_MODE_DATA_LEN, Transport, TransportError,
    TransportEvent, WACOM_COMPANY_IDS,
};
use ardoise_proto::message::opcode;

use crate::clock::VirtualClock;

/// Length of the manufacturer data of a tablet in normal mode.
const NORMAL_MODE_DATA_LEN: usize = 7;

/// How strictly a log is replayed, and which tablet it pretends to be.
#[derive(Debug, Clone, Default)]
pub struct ReplayOptions {
    /// Accept any SET_TIME payload (for captures made with the real tablet at another time).
    pub lenient_set_time: bool,
    /// The tablet `find` and the transports report. Taken from the log's notes when `None`.
    pub peer: Option<Advertisement>,
}

/// Errors building a replay.
#[derive(Debug, thiserror::Error)]
pub enum ReplayError {
    #[error(transparent)]
    Log(#[from] LogError),
    #[error("the log header has an invalid creation time {0:?}")]
    Created(String),
}

/// A [`Connector`] that replays a log. Clones share the same replay (one position in the
/// log), so a test can keep one to inspect [`ReplayConnector::remaining`].
#[derive(Clone)]
pub struct ReplayConnector {
    replay: Arc<Mutex<Replay>>,
    clock: Arc<VirtualClock>,
}

/// One connection of a replay.
pub struct ReplayTransport {
    replay: Arc<Mutex<Replay>>,
    clock: Arc<VirtualClock>,
    peer: Advertisement,
    connection: u64,
}

struct Replay {
    entries: Vec<LogEntry>,
    /// Next entry to consume.
    next: usize,
    lenient_set_time: bool,
    /// The tablet in normal mode.
    peer: Advertisement,
    /// Number of the open connection; 0 when none.
    connection: u64,
    connections: u64,
    failure: Option<String>,
}

/// What an entry means for the replay.
enum Kind<'a> {
    Tx,
    Rx,
    Connected,
    Disconnect,
    Disconnected,
    WriteFailed(&'a str),
    ReceiveFailed(&'a str),
    Ignored,
}

fn kind(entry: &LogEntry) -> Kind<'_> {
    match entry.dir {
        Direction::Tx => Kind::Tx,
        Direction::Rx => Kind::Rx,
        Direction::Note => Kind::Ignored,
        Direction::Event => match entry.info.as_str() {
            "connected" => Kind::Connected,
            "disconnect" => Kind::Disconnect,
            "disconnected" => Kind::Disconnected,
            other => {
                if let Some(error) = other.strip_prefix("write failed: ") {
                    Kind::WriteFailed(error)
                } else if let Some(error) = other.strip_prefix("receive failed: ") {
                    Kind::ReceiveFailed(error)
                } else {
                    Kind::Ignored
                }
            }
        },
    }
}

impl ReplayConnector {
    /// A replay of `entries`, whose clock starts at the creation time in `header`.
    pub fn new(
        header: &LogHeader,
        entries: Vec<LogEntry>,
        options: ReplayOptions,
    ) -> Result<Self, ReplayError> {
        let created = chrono::DateTime::parse_from_rfc3339(&header.created)
            .map_err(|_| ReplayError::Created(header.created.clone()))?;
        let clock = Arc::new(VirtualClock::with_origin(SystemTime::from(created)));
        let mut peer = options.peer.unwrap_or_else(|| peer_from_notes(&entries));
        if peer.manufacturer_data.len() == REGISTER_MODE_DATA_LEN {
            // Keep the tablet in normal mode; `find` reports registration mode when needed.
            peer.manufacturer_data.resize(NORMAL_MODE_DATA_LEN, 0);
        }
        Ok(Self {
            replay: Arc::new(Mutex::new(Replay {
                entries,
                next: 0,
                lenient_set_time: options.lenient_set_time,
                peer,
                connection: 0,
                connections: 0,
                failure: None,
            })),
            clock,
        })
    }

    /// A replay of the log file at `path`.
    pub fn from_file(path: &Path, options: ReplayOptions) -> Result<Self, ReplayError> {
        let (header, entries) = log::read_log(path)?;
        Self::new(&header, entries, options)
    }

    /// A replay of the content of a log file.
    pub fn from_text(text: &str, options: ReplayOptions) -> Result<Self, ReplayError> {
        let (header, entries) = log::parse_log(text)?;
        Self::new(&header, entries, options)
    }

    /// The replay clock (also returned by [`Connector::clock`]).
    pub fn replay_clock(&self) -> Arc<VirtualClock> {
        self.clock.clone()
    }

    /// Recorded exchanges and link events not replayed yet (notes are not counted).
    pub fn remaining(&self) -> usize {
        let replay = lock(&self.replay);
        replay.entries[replay.next.min(replay.entries.len())..]
            .iter()
            .filter(|e| !matches!(kind(e), Kind::Ignored))
            .count()
    }

    /// The mismatch that stopped the replay, if any.
    pub fn failure(&self) -> Option<String> {
        lock(&self.replay).failure.clone()
    }
}

impl Connector for ReplayConnector {
    fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    fn find(
        &mut self,
        timeout: Duration,
        filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError> {
        let advertisement = lock(&self.replay).advertisement();
        if filter(&advertisement) {
            return Ok(Some(advertisement));
        }
        self.clock.advance(timeout);
        Ok(None)
    }

    fn connect(
        &mut self,
        address: &str,
        _timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        let mut replay = lock(&self.replay);
        replay.check()?;
        replay.skip_ignored();
        match replay.current().map(kind) {
            Some(Kind::Connected) => {
                let peer = replay.advertisement();
                replay.next += 1;
                replay.connections += 1;
                replay.connection = replay.connections;
                Ok(Box::new(ReplayTransport {
                    replay: self.replay.clone(),
                    clock: self.clock.clone(),
                    peer,
                    connection: replay.connection,
                }))
            }
            _ => {
                let expected = replay.describe_current();
                Err(replay.fail(format!(
                    "{expected}: the host connected to {address} but the log expects something else"
                )))
            }
        }
    }
}

impl Transport for ReplayTransport {
    fn peer(&self) -> &Advertisement {
        &self.peer
    }

    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        let mut replay = lock(&self.replay);
        replay.check()?;
        if replay.connection != self.connection {
            return Err(TransportError::NotConnected);
        }
        replay.skip_ignored();
        let actual = log::to_hex(frame);
        let Some(entry) = replay.current() else {
            return Err(replay.fail(format!("the host wrote {actual} after the end of the log")));
        };
        if !matches!(kind(entry), Kind::Tx) {
            let expected = replay.describe_current();
            return Err(replay.fail(format!("{expected}: the host wrote tx {actual} instead")));
        }
        let expected = match entry.bytes() {
            Ok(bytes) => bytes,
            Err(error) => {
                let at = replay.describe_current();
                return Err(replay.fail(format!("{at}: unreadable bytes ({error})")));
            }
        };
        if expected != frame && !(replay.lenient_set_time && both_set_time(&expected, frame)) {
            let at = replay.describe_current();
            return Err(replay.fail(format!("{at}: the host wrote tx {actual} instead")));
        }
        replay.next += 1;
        // The original write may have failed.
        replay.skip_ignored();
        if let Some(Kind::WriteFailed(error)) = replay.current().map(kind) {
            let error = transport_error(error);
            replay.next += 1;
            return Err(error);
        }
        Ok(())
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
        let mut replay = lock(&self.replay);
        replay.check()?;
        if replay.connection != self.connection {
            return Err(TransportError::NotConnected);
        }
        replay.skip_ignored();
        let now = self.clock.monotonic();
        let horizon = now.saturating_add(timeout);
        let Some(entry) = replay.current() else {
            self.clock.advance_to(horizon);
            return Ok(None);
        };
        let due = recorded_time(entry);
        let deliverable = matches!(
            kind(entry),
            Kind::Rx | Kind::Disconnected | Kind::ReceiveFailed(_)
        );
        if !deliverable || due > horizon {
            self.clock.advance_to(horizon);
            return Ok(None);
        }
        self.clock.advance_to(due);
        match kind(entry) {
            Kind::Rx => {
                let channel = entry.ch;
                let bytes = entry.bytes();
                let at = replay.describe_current();
                let (Some(channel), Ok(data)) = (channel, bytes) else {
                    return Err(replay.fail(format!(
                        "{at}: received bytes without a channel or unreadable"
                    )));
                };
                replay.next += 1;
                Ok(Some(TransportEvent::Notification { channel, data }))
            }
            Kind::ReceiveFailed(error) => {
                let error = transport_error(error);
                replay.next += 1;
                Err(error)
            }
            _ => {
                replay.next += 1;
                replay.connection = 0;
                Ok(Some(TransportEvent::Disconnected))
            }
        }
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        let mut replay = lock(&self.replay);
        if replay.connection != self.connection {
            // Already lost (the log said `disconnected`), or replaced.
            return Ok(());
        }
        replay.connection = 0;
        replay.check()?;
        replay.skip_ignored();
        if let Some(Kind::Disconnect) = replay.current().map(kind) {
            replay.next += 1;
            return Ok(());
        }
        let expected = replay.describe_current();
        Err(replay.fail(format!("{expected}: the host disconnected instead")))
    }
}

impl Drop for ReplayTransport {
    /// A transport dropped without [`Transport::disconnect`] closes its link too; the logging
    /// transport records that as a `disconnect` event, consumed here. Nothing is reported if
    /// the log says otherwise: the next operation will meet the mismatch.
    fn drop(&mut self) {
        let mut replay = lock(&self.replay);
        if replay.connection != self.connection || replay.failure.is_some() {
            return;
        }
        replay.connection = 0;
        replay.skip_ignored();
        if let Some(Kind::Disconnect) = replay.current().map(kind) {
            replay.next += 1;
        }
    }
}

impl Replay {
    fn check(&self) -> Result<(), TransportError> {
        match &self.failure {
            Some(message) => Err(TransportError::Replay(message.clone())),
            None => Ok(()),
        }
    }

    fn fail(&mut self, message: String) -> TransportError {
        self.failure = Some(message.clone());
        TransportError::Replay(message)
    }

    fn skip_ignored(&mut self) {
        while self
            .entries
            .get(self.next)
            .is_some_and(|e| matches!(kind(e), Kind::Ignored))
        {
            self.next += 1;
        }
    }

    fn current(&self) -> Option<&LogEntry> {
        self.entries.get(self.next)
    }

    /// `log line 12 (entry #10, t=0.250 s): expected rx command b3 01 00`.
    fn describe_current(&self) -> String {
        match self.current() {
            None => "end of the log: expected nothing more".into(),
            Some(entry) => {
                let what = match entry.dir {
                    Direction::Tx => format!("tx {}", entry.hex.as_deref().unwrap_or("")),
                    Direction::Rx => format!(
                        "rx {} {}",
                        entry.ch.map_or("?", channel_name),
                        entry.hex.as_deref().unwrap_or("")
                    ),
                    Direction::Event => format!("event {}", entry.info),
                    Direction::Note => format!("note {}", entry.info),
                };
                format!(
                    "log line {} (entry #{}, t={:.3} s): expected {what}",
                    self.next + 2,
                    self.next,
                    entry.t
                )
            }
        }
    }

    /// The recorded tablet, in registration mode when the next connection registers.
    fn advertisement(&self) -> Advertisement {
        let mut advertisement = self.peer.clone();
        if self.next_connection_registers() {
            advertisement
                .manufacturer_data
                .truncate(REGISTER_MODE_DATA_LEN);
            advertisement
                .manufacturer_data
                .resize(REGISTER_MODE_DATA_LEN, 0);
        }
        advertisement
    }

    fn next_connection_registers(&self) -> bool {
        let rest = self.entries.get(self.next..).unwrap_or_default();
        let mut after_connect = rest
            .iter()
            .skip_while(|e| !matches!(kind(e), Kind::Connected))
            .skip(1);
        after_connect
            .find(|e| matches!(kind(e), Kind::Tx | Kind::Connected))
            .filter(|e| matches!(kind(e), Kind::Tx))
            .and_then(|e| e.bytes().ok())
            .is_some_and(|bytes| bytes.first() == Some(&opcode::REGISTER))
    }
}

fn lock(replay: &Arc<Mutex<Replay>>) -> MutexGuard<'_, Replay> {
    replay.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Both frames are SET_TIME with a 6-byte payload.
fn both_set_time(a: &[u8], b: &[u8]) -> bool {
    let set_time = |f: &[u8]| f.len() == 8 && f[..2] == [opcode::TIME, 0x06];
    set_time(a) && set_time(b)
}

/// Time of an entry on the replay clock (`t` is rounded to the millisecond in the log).
fn recorded_time(entry: &LogEntry) -> Duration {
    Duration::from_millis((entry.t * 1000.0).round().max(0.0) as u64)
}

fn channel_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Command => "command",
        Channel::PageData => "page_data",
        Channel::Live => "live",
        Channel::SysEvent => "sys_event",
        Channel::BoardData => "board_data",
    }
}

/// The error a `write failed: …` / `receive failed: …` event recorded.
fn transport_error(text: &str) -> TransportError {
    match text {
        "not connected" => TransportError::NotConnected,
        "the connection timed out" => TransportError::Timeout,
        "no Bluetooth LE adapter" => TransportError::NoAdapter,
        "Bluetooth is switched off" => TransportError::AdapterOff,
        other => match (
            other.strip_prefix("Bluetooth error: "),
            other.strip_prefix("replay mismatch: "),
        ) {
            (Some(message), _) => TransportError::Backend(message.to_owned()),
            (_, Some(message)) => TransportError::Replay(message.to_owned()),
            _ => TransportError::Backend(other.to_owned()),
        },
    }
}

/// The tablet as described by the manager's notes (`found <name> at <address> (rssi …`,
/// `connecting to <address>`), in normal mode.
fn peer_from_notes(entries: &[LogEntry]) -> Advertisement {
    let mut peer = Advertisement {
        address: String::new(),
        name: Some("Bamboo Slate".into()),
        rssi: None,
        company_id: Some(WACOM_COMPANY_IDS[0]),
        manufacturer_data: vec![0; NORMAL_MODE_DATA_LEN],
        xiaomi_service_data: Vec::new(),
    };
    let notes = entries.iter().filter(|e| e.dir == Direction::Note);
    for note in notes {
        if let Some(found) = note.info.strip_prefix("found ")
            && let Some((tablet, details)) = found.split_once(" (rssi ")
            && let Some((name, address)) = tablet.rsplit_once(" at ")
        {
            peer.name = Some(name.to_owned());
            if peer.address.is_empty() {
                peer.address = address.to_owned();
            }
            peer.rssi = details
                .split_once(',')
                .and_then(|(rssi, _)| rssi.strip_prefix("Some(")?.strip_suffix(')'))
                .and_then(|rssi| rssi.parse().ok());
            break;
        }
        if let Some(address) = note.info.strip_prefix("connecting to ")
            && peer.address.is_empty()
        {
            peer.address = address.trim().to_owned();
        }
    }
    if peer.address.is_empty() {
        peer.address = "00:00:00:00:00:00".into();
    }
    peer
}

/// The first difference between two logs in their sequence of (direction, channel, bytes),
/// or `None` when they record the same exchanges. Times and texts are not compared.
pub fn first_difference(expected: &[LogEntry], actual: &[LogEntry]) -> Option<String> {
    let key = |e: &LogEntry| (e.dir, e.ch, e.hex.clone());
    for (index, (a, b)) in expected.iter().zip(actual).enumerate() {
        if key(a) != key(b) {
            return Some(format!(
                "entry #{index}: expected {:?} {:?} {:?} ({}), got {:?} {:?} {:?} ({})",
                a.dir,
                a.ch,
                a.hex.as_deref().unwrap_or(""),
                a.info,
                b.dir,
                b.ch,
                b.hex.as_deref().unwrap_or(""),
                b.info
            ));
        }
    }
    (expected.len() != actual.len()).then(|| {
        format!(
            "{} entries expected, {} recorded",
            expected.len(),
            actual.len()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str = r#"{"type":"header","format":1,"app":"0.1.0","transport":"ble","created":"2026-09-01T10:00:00+02:00"}"#;

    fn log(lines: &[&str]) -> String {
        let mut text = String::from(HEADER);
        for line in lines {
            text.push('\n');
            text.push_str(line);
        }
        text
    }

    fn connect(connector: &mut ReplayConnector) -> Box<dyn Transport> {
        connector
            .connect("D4:5A:2E:7B:3C:91", Duration::from_secs(1))
            .unwrap()
    }

    #[test]
    fn delivers_recorded_replies_at_their_time() {
        let text = log(&[
            r#"{"t":0.0,"dir":"note","info":"found Bamboo Slate at D4:5A:2E:7B:3C:91 (rssi Some(-58), manufacturer data 7 bytes)"}"#,
            r#"{"t":0.0,"dir":"event","info":"connected"}"#,
            r#"{"t":0.0,"dir":"tx","ch":"command","hex":"b9 01 00","info":"GET_BATTERY"}"#,
            r#"{"t":0.25,"dir":"rx","ch":"command","hex":"ba 02 57 00","info":"BATTERY"}"#,
            r#"{"t":0.25,"dir":"event","info":"disconnect"}"#,
        ]);
        let mut connector = ReplayConnector::from_text(&text, ReplayOptions::default()).unwrap();
        let clock = connector.replay_clock();
        assert_eq!(
            chrono::DateTime::<chrono::Utc>::from(clock.now()).to_rfc3339(),
            "2026-09-01T08:00:00+00:00"
        );
        let mut found = None;
        connector
            .find(Duration::from_secs(1), &mut |ad| {
                found = Some(ad.clone());
                true
            })
            .unwrap();
        let found = found.unwrap();
        assert_eq!(found.address, "D4:5A:2E:7B:3C:91");
        assert_eq!(found.rssi, Some(-58));
        assert!(!found.in_register_mode());

        let mut link = connect(&mut connector);
        link.write(&[0xb9, 0x01, 0x00]).unwrap();
        assert_eq!(link.recv(Duration::from_millis(100)).unwrap(), None);
        assert_eq!(clock.monotonic(), Duration::from_millis(100));
        assert_eq!(
            link.recv(Duration::from_secs(5)).unwrap(),
            Some(TransportEvent::Notification {
                channel: Channel::Command,
                data: vec![0xba, 0x02, 0x57, 0x00]
            })
        );
        assert_eq!(clock.monotonic(), Duration::from_millis(250));
        link.disconnect().unwrap();
        assert_eq!(connector.remaining(), 0);
    }

    #[test]
    fn a_different_write_is_a_precise_mismatch_and_stays_one() {
        let text = log(&[
            r#"{"t":0.0,"dir":"event","info":"connected"}"#,
            r#"{"t":0.0,"dir":"tx","ch":"command","hex":"b9 01 00","info":"GET_BATTERY"}"#,
        ]);
        let mut connector = ReplayConnector::from_text(&text, ReplayOptions::default()).unwrap();
        let mut link = connect(&mut connector);
        let TransportError::Replay(message) = link.write(&[0xbb, 0x01, 0x00]).unwrap_err() else {
            panic!("not a replay error");
        };
        assert_eq!(
            message,
            "log line 3 (entry #1, t=0.000 s): expected tx b9 01 00: the host wrote tx bb 01 00 instead"
        );
        assert!(matches!(
            link.recv(Duration::ZERO),
            Err(TransportError::Replay(_))
        ));
        assert_eq!(connector.failure(), Some(message));
    }

    #[test]
    fn set_time_payload_is_exempt_only_in_lenient_mode() {
        let text = log(&[
            r#"{"t":0.0,"dir":"event","info":"connected"}"#,
            r#"{"t":0.0,"dir":"tx","ch":"command","hex":"b6 06 19 03 15 03 22 16","info":"SET_TIME"}"#,
            r#"{"t":0.0,"dir":"rx","ch":"command","hex":"b3 01 00","info":"STATUS success"}"#,
        ]);
        let now = [0xb6, 0x06, 0x26, 0x09, 0x01, 0x08, 0x00, 0x00];
        let mut strict = ReplayConnector::from_text(&text, ReplayOptions::default()).unwrap();
        assert!(matches!(
            connect(&mut strict).write(&now),
            Err(TransportError::Replay(_))
        ));
        let lenient_options = ReplayOptions {
            lenient_set_time: true,
            ..ReplayOptions::default()
        };
        let mut lenient = ReplayConnector::from_text(&text, lenient_options).unwrap();
        let mut link = connect(&mut lenient);
        link.write(&now).unwrap();
        assert!(link.recv(Duration::ZERO).unwrap().is_some());
        // Only SET_TIME is exempt.
        let mut other = ReplayConnector::from_text(
            &log(&[
                r#"{"t":0.0,"dir":"event","info":"connected"}"#,
                r#"{"t":0.0,"dir":"tx","ch":"command","hex":"b6 01 00","info":"GET_TIME"}"#,
            ]),
            ReplayOptions {
                lenient_set_time: true,
                ..ReplayOptions::default()
            },
        )
        .unwrap();
        assert!(connect(&mut other).write(&[0xb6, 0x01, 0x01]).is_err());
    }

    #[test]
    fn replays_disconnections_and_successive_connections() {
        let text = log(&[
            r#"{"t":0.0,"dir":"event","info":"connected"}"#,
            r#"{"t":1.0,"dir":"event","info":"disconnected"}"#,
            r#"{"t":1.0,"dir":"note","info":"reconnecting"}"#,
            r#"{"t":1.0,"dir":"event","info":"connected"}"#,
            r#"{"t":1.0,"dir":"tx","ch":"command","hex":"b9 01 00","info":"GET_BATTERY"}"#,
            r#"{"t":1.0,"dir":"event","info":"write failed: Bluetooth error: busy"}"#,
            r#"{"t":1.2,"dir":"event","info":"disconnect"}"#,
        ]);
        let mut connector = ReplayConnector::from_text(&text, ReplayOptions::default()).unwrap();
        let mut first = connect(&mut connector);
        assert_eq!(
            first.recv(Duration::from_secs(5)).unwrap(),
            Some(TransportEvent::Disconnected)
        );
        assert_eq!(connector.replay_clock().monotonic(), Duration::from_secs(1));
        let mut second = connect(&mut connector);
        assert!(matches!(
            first.write(&[0xb9, 0x01, 0x00]),
            Err(TransportError::NotConnected)
        ));
        assert_eq!(
            second.write(&[0xb9, 0x01, 0x00]),
            Err(TransportError::Backend("busy".into()))
        );
        second.disconnect().unwrap();
        assert_eq!(connector.remaining(), 0);
    }

    #[test]
    fn a_dropped_transport_consumes_its_disconnect_event() {
        let text = log(&[
            r#"{"t":0.0,"dir":"event","info":"connected"}"#,
            r#"{"t":0.0,"dir":"tx","ch":"command","hex":"b9 01 00","info":"GET_BATTERY"}"#,
            r#"{"t":0.0,"dir":"rx","ch":"command","hex":"ba 02 57 00","info":"BATTERY"}"#,
            r#"{"t":0.0,"dir":"event","info":"disconnect"}"#,
            r#"{"t":0.5,"dir":"event","info":"connected"}"#,
        ]);
        let mut connector = ReplayConnector::from_text(&text, ReplayOptions::default()).unwrap();
        let mut link = connect(&mut connector);
        link.write(&[0xb9, 0x01, 0x00]).unwrap();
        assert!(link.recv(Duration::ZERO).unwrap().is_some());
        // recv(0) never moves the clock, even with nothing to deliver.
        assert_eq!(link.recv(Duration::ZERO).unwrap(), None);
        assert_eq!(connector.replay_clock().monotonic(), Duration::ZERO);
        drop(link);
        let _second = connect(&mut connector);
        assert_eq!(connector.remaining(), 0);
    }

    #[test]
    fn find_reports_registration_mode_before_a_recorded_registration() {
        let text = log(&[
            r#"{"t":0.0,"dir":"note","info":"connecting to D4:5A:2E:7B:3C:91"}"#,
            r#"{"t":0.0,"dir":"event","info":"connected"}"#,
            r#"{"t":0.0,"dir":"tx","ch":"command","hex":"e7 06 01 02 03 04 05 06","info":"REGISTER"}"#,
        ]);
        let mut connector = ReplayConnector::from_text(&text, ReplayOptions::default()).unwrap();
        let found = connector
            .find(Duration::from_secs(1), &mut |ad| ad.in_register_mode())
            .unwrap()
            .unwrap();
        assert_eq!(found.address, "D4:5A:2E:7B:3C:91");
        assert_eq!(found.manufacturer_data.len(), 4);
    }

    #[test]
    fn compares_logs_by_direction_channel_and_bytes() {
        let (_, a) = log::parse_log(&log(&[
            r#"{"t":0.0,"dir":"tx","ch":"command","hex":"b9 01 00","info":"x"}"#,
        ]))
        .unwrap();
        let (_, b) = log::parse_log(&log(&[
            r#"{"t":3.0,"dir":"tx","ch":"command","hex":"b9 01 00","info":"y"}"#,
        ]))
        .unwrap();
        assert_eq!(first_difference(&a, &b), None);
        assert!(first_difference(&a, &[]).is_some());
    }
}
