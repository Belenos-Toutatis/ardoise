//! One authenticated conversation with a tablet: the flows of protocol.md §3–§7.
//!
//! A [`Session`] owns a [`Transport`] and turns requests into replies. Notifications are
//! sorted into per-channel queues as they arrive, so page bytes or live samples received while
//! waiting for a command reply are never lost, and unsolicited frames (a stray name chunk,
//! sysevents) are skipped instead of being taken for the expected answer.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ardoise_proto::message::opcode;
use ardoise_proto::{
    BcdTime, Dimension, FirmwarePart, Mode, NameAssembler, Reply, Request, StatusCode,
};

use crate::identity::HostId;
use crate::transport::{Advertisement, Channel, Clock, Transport, TransportError, TransportEvent};

/// Default wait for the reply to a command (protocol.md §2.3).
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// A download is abandoned when no page byte arrived for this long (protocol.md §5.1).
pub const DATA_IDLE_TIMEOUT: Duration = Duration::from_secs(5);
/// After the end-of-download marker, page bytes may still be in flight on their own
/// characteristic: they are awaited this long before the CRC is declared wrong.
pub const DATA_DRAIN_TIMEOUT: Duration = Duration::from_millis(1000);
/// Name chunks are collected until a line feed, or until this much silence after a chunk.
const NAME_CHUNK_TIMEOUT: Duration = Duration::from_millis(500);
/// A name never needs more chunks than this; a tablet repeating chunks cannot loop forever.
const MAX_NAME_CHUNKS: usize = 8;
/// Smallest cap on the size of a page download (the cap grows with the announced size).
const MAX_PAGE_BYTES: usize = 16 * 1024 * 1024;
/// Granularity of the waits, so that cancellation is noticed quickly.
const WAIT_SLICE: Duration = Duration::from_millis(250);
/// Longest name [`Session::set_name`] sends: 15 characters keep the frame (`bb`, length, name,
/// `0a`) within one 20-byte write (protocol.md §4.1 estimates about 17).
pub const MAX_NAME_LEN: usize = 15;
/// Notifications kept for [`Session::next_live_event`] while nobody reads them; older ones are
/// dropped so that a tablet streaming unasked cannot exhaust memory.
const MAX_LIVE_QUEUE: usize = 4096;

/// Errors of a session.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DeviceError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error("the tablet disconnected")]
    Disconnected,
    #[error("no answer from the tablet ({0})")]
    Timeout(&'static str),
    /// `b3 01 07`: the tablet is registered with another application (or another identity).
    #[error("the tablet refused our identity: it must be registered again")]
    NotRegistered,
    /// Any other error status, e.g. `InvalidState` when the tablet is not in the right mode.
    #[error("the tablet refused the command ({0:?})")]
    Refused(StatusCode),
    #[error("unexpected answer from the tablet: {0}")]
    Unexpected(String),
    #[error("page transfer corrupted (CRC {actual:08x} instead of {expected:08x})")]
    CrcMismatch { expected: u32, actual: u32 },
    #[error("cancelled")]
    Cancelled,
    /// The caller could not store a downloaded page; nothing was deleted from the tablet.
    #[error("cannot save the page: {0}")]
    Storage(String),
    /// The command needs a connected tablet (the tablet service).
    #[error("the tablet is not connected")]
    NotConnected,
    /// The tablet is syncing or in live mode (the tablet service).
    #[error("the tablet is busy (sync or live mode under way)")]
    Busy,
    /// No tablet is registered from this library (the tablet service).
    #[error("no tablet is registered")]
    NoTabletRegistered,
    /// No library is chosen, so there is no registration to use (the tablet service).
    #[error("no library is chosen")]
    NoLibrary,
    /// A tablet name must be 1 to 15 printable ASCII characters.
    #[error("invalid tablet name {0:?}: 1 to 15 printable ASCII characters")]
    InvalidName(String),
}

/// What the tablet says about itself.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub name: String,
    pub firmware: String,
    pub battery_percent: u8,
    pub charging: bool,
    /// Drawing area in device units (10 µm).
    pub width: u32,
    pub height: u32,
}

/// A stored page as received, before any interpretation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedPage {
    /// The file bytes, CRC verified.
    pub bytes: Vec<u8>,
    /// When the page was stored on the tablet (unix seconds, from the file-info reply).
    pub timestamp: Option<i64>,
    /// Size announced by the tablet before the transfer (informative only).
    pub announced_size: u32,
}

/// A notification received during live mode, stamped when it came out of the transport.
///
/// `received` is read from the system clock, not from the session [`Clock`]: it measures the
/// real latency of live mode (docs/design/tablet-live-classroom.md, "Latency"), the one
/// documented exception to "time comes from a `Clock`".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveNotification {
    pub channel: Channel,
    pub data: Vec<u8>,
    pub received: SystemTime,
}

/// Outcome of a sync.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSummary {
    /// Pages stored on the tablet when the sync started.
    pub available: u16,
    /// Pages downloaded and accepted by the sink.
    pub downloaded: u16,
    /// Pages deleted from the tablet after being saved.
    pub deleted: u16,
}

/// An authenticated link to a tablet.
pub struct Session {
    transport: Box<dyn Transport>,
    clock: Arc<dyn Clock>,
    cancel: Option<Arc<AtomicBool>>,
    commands: VecDeque<Vec<u8>>,
    page_data: Vec<u8>,
    last_page_data: Option<Duration>,
    /// Live-mode notifications: pen data always, every other notification too while
    /// `live_mode` is set (so that the caller can classify them).
    live: VecDeque<LiveNotification>,
    /// Live mode was entered with [`Session::start_live`] and not left.
    live_mode: bool,
    connected: bool,
    reply_timeout: Duration,
}

impl Session {
    fn new(transport: Box<dyn Transport>, clock: Arc<dyn Clock>) -> Self {
        Self {
            transport,
            clock,
            cancel: None,
            commands: VecDeque::new(),
            page_data: Vec::new(),
            last_page_data: None,
            live: VecDeque::new(),
            live_mode: false,
            connected: true,
            reply_timeout: REPLY_TIMEOUT,
        }
    }

    /// Authenticates with an identity registered earlier (`e6`).
    pub fn connect(
        transport: Box<dyn Transport>,
        clock: Arc<dyn Clock>,
        id: HostId,
    ) -> Result<Self, DeviceError> {
        let mut session = Self::new(transport, clock);
        session
            .expect_success(&Request::Connect { uuid: id.0 })
            .map_err(|error| match error {
                DeviceError::Refused(StatusCode::AuthorizationError) => DeviceError::NotRegistered,
                other => other,
            })?;
        Ok(session)
    }

    /// Registers `id` with a tablet in registration mode (`e7`), then waits up to
    /// `button_timeout` for the user to confirm with the button. `waiting_for_button` is called
    /// once the request is sent, to tell the user to press it.
    pub fn register(
        transport: Box<dyn Transport>,
        clock: Arc<dyn Clock>,
        id: HostId,
        button_timeout: Duration,
        cancel: Option<Arc<AtomicBool>>,
        waiting_for_button: &mut dyn FnMut(),
    ) -> Result<Self, DeviceError> {
        let mut session = Self::new(transport, clock);
        session.cancel = cancel;
        session.send(&Request::RegisterPressButton { uuid: id.0 })?;
        waiting_for_button();
        session.wait_reply(button_timeout, "registration button", |reply| {
            matches!(reply, Reply::RegisterButtonPressed)
        })?;
        Ok(session)
    }

    /// Lets the caller interrupt long waits (button, download, live mode).
    pub fn set_cancel_flag(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = Some(cancel);
    }

    /// The tablet at the other end.
    pub fn peer(&self) -> &Advertisement {
        self.transport.peer()
    }

    /// Whether the link is still up.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// The clock of this session.
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// Sets the tablet's clock to the current UTC time.
    pub fn set_time(&mut self) -> Result<(), DeviceError> {
        let now = unix_seconds(&*self.clock);
        let time = BcdTime::from_unix(now)
            .map_err(|e| DeviceError::Unexpected(format!("cannot encode the time: {e}")))?;
        self.expect_success(&Request::SetTime(time))
    }

    /// Reads the tablet's clock, in unix seconds.
    pub fn read_time(&mut self) -> Result<i64, DeviceError> {
        match self.exchange(&Request::GetTime, "time", |r| matches!(r, Reply::Time(_)))? {
            Reply::Time(time) => Ok(time.to_unix()),
            other => Err(unexpected(&other)),
        }
    }

    /// Tells the tablet to send page bytes on the page-data characteristic (once per session,
    /// before any download).
    pub fn set_file_transfer_reporting(&mut self) -> Result<(), DeviceError> {
        self.expect_success(&Request::SetFileTransferReportingType)
    }

    /// The tablet's name. Long names may come in several chunks ending with a line feed.
    pub fn name(&mut self) -> Result<String, DeviceError> {
        let mut name = NameAssembler::new();
        let mut chunk = match self.exchange(&Request::GetName, "name", is_name_chunk)? {
            Reply::NameChunk(chunk) => chunk,
            other => return Err(unexpected(&other)),
        };
        for _ in 0..MAX_NAME_CHUNKS {
            if let Some(complete) = name.push(&chunk) {
                return Ok(complete);
            }
            chunk = match self.wait_reply(NAME_CHUNK_TIMEOUT, "name", is_name_chunk) {
                Ok(Reply::NameChunk(chunk)) => chunk,
                Ok(other) => return Err(unexpected(&other)),
                // Some tablets send the whole name without the final line feed.
                Err(DeviceError::Timeout(_)) => break,
                Err(error) => return Err(error),
            };
        }
        Ok(name.finish())
    }

    /// Renames the tablet: `bb n+1 <ASCII name> 0a`, answered by `b3 01 00` (protocol.md
    /// §4.1). The name must be 1 to [`MAX_NAME_LEN`] printable ASCII characters.
    pub fn set_name(&mut self, name: &str) -> Result<(), DeviceError> {
        if !valid_name(name) {
            return Err(DeviceError::InvalidName(name.to_owned()));
        }
        self.expect_success(&Request::SetName(name.to_owned()))
    }

    /// Firmware version, `B160805A-C170704A` style.
    pub fn firmware(&mut self) -> Result<String, DeviceError> {
        let mut parts = Vec::with_capacity(2);
        for part in [FirmwarePart::High, FirmwarePart::Low] {
            let reply = self.exchange(
                &Request::GetFirmware { part },
                "firmware",
                |r| matches!(r, Reply::Firmware { part: p, .. } if *p == part),
            )?;
            if let Reply::Firmware { text, .. } = reply {
                parts.push(text.trim_end_matches('\0').trim().to_owned());
            }
        }
        Ok(parts.join("-"))
    }

    /// Battery level in percent and whether it is charging.
    pub fn battery(&mut self) -> Result<(u8, bool), DeviceError> {
        match self.exchange(&Request::GetBattery, "battery", |r| {
            matches!(r, Reply::Battery { .. })
        })? {
            Reply::Battery { percent, charging } => Ok((percent, charging)),
            other => Err(unexpected(&other)),
        }
    }

    /// Width and height of the drawing area, in device units. The Slate exists in two sizes
    /// that report the same name, so this must always be asked.
    pub fn dimensions(&mut self) -> Result<(u32, u32), DeviceError> {
        let mut read = |dimension: Dimension| -> Result<u32, DeviceError> {
            match self.exchange(
                &Request::GetDimension(dimension),
                "dimensions",
                |r| matches!(r, Reply::Dimension { which, .. } if *which == dimension),
            )? {
                Reply::Dimension { value, .. } => Ok(value),
                other => Err(unexpected(&other)),
            }
        };
        Ok((read(Dimension::Width)?, read(Dimension::Height)?))
    }

    /// Name, firmware, battery and size.
    pub fn info(&mut self) -> Result<DeviceInfo, DeviceError> {
        let name = self.name()?;
        let firmware = self.firmware()?;
        let (battery_percent, charging) = self.battery()?;
        let (width, height) = self.dimensions()?;
        Ok(DeviceInfo {
            name,
            firmware,
            battery_percent,
            charging,
            width,
            height,
        })
    }

    /// Switches the tablet to a mode (`b1`).
    pub fn set_mode(&mut self, mode: Mode) -> Result<(), DeviceError> {
        self.expect_success(&Request::SetMode(mode))
    }

    /// Number of pages stored on the tablet. Switches it to paper mode first.
    pub fn stored_pages(&mut self) -> Result<u16, DeviceError> {
        self.set_mode(Mode::Paper)?;
        match self.exchange(&Request::AvailableFilesCount, "page count", |r| {
            matches!(r, Reply::FilesCount(_))
        })? {
            Reply::FilesCount(count) => Ok(count),
            other => Err(unexpected(&other)),
        }
    }

    /// Downloads the oldest stored page without deleting it. Requires paper mode and the
    /// file-transfer reporting type (see [`Session::stored_pages`] and
    /// [`Session::set_file_transfer_reporting`]).
    pub fn download_oldest(&mut self) -> Result<DownloadedPage, DeviceError> {
        let (announced_size, timestamp) = self.file_info()?;
        // A tablet streaming bytes without ever ending the transfer must not exhaust memory.
        let limit = usize::try_from(announced_size)
            .unwrap_or(usize::MAX)
            .saturating_mul(4)
            .max(MAX_PAGE_BYTES);

        self.page_data.clear();
        self.last_page_data = None;
        self.exchange(&Request::DownloadOldestFile, "download start", |r| {
            matches!(r, Reply::DownloadStart { .. })
        })?;

        // The end marker can take long for a big page: keep waiting while bytes flow.
        let crc = loop {
            if self.page_data.len() > limit {
                return Err(DeviceError::Unexpected(format!(
                    "page larger than {limit} bytes ({announced_size} announced)"
                )));
            }
            match self.wait_reply(DATA_IDLE_TIMEOUT, "end of download", |r| {
                matches!(r, Reply::DownloadEnd { .. })
            }) {
                Ok(Reply::DownloadEnd { crc }) => break crc,
                Ok(other) => return Err(unexpected(&other)),
                Err(DeviceError::Timeout(what)) => {
                    let flowing = self.last_page_data.is_some_and(|last| {
                        self.clock.monotonic().saturating_sub(last) < DATA_IDLE_TIMEOUT
                    });
                    if !flowing {
                        return Err(DeviceError::Timeout(what));
                    }
                }
                Err(error) => return Err(error),
            }
        };

        // The last chunks travel on another characteristic than the end marker.
        let deadline = self.clock.monotonic() + DATA_DRAIN_TIMEOUT;
        let mut actual = ardoise_proto::crc32(&self.page_data);
        while actual != crc {
            let now = self.clock.monotonic();
            if now >= deadline {
                return Err(DeviceError::CrcMismatch {
                    expected: crc,
                    actual,
                });
            }
            self.pump((deadline - now).min(WAIT_SLICE))?;
            actual = ardoise_proto::crc32(&self.page_data);
        }
        Ok(DownloadedPage {
            bytes: std::mem::take(&mut self.page_data),
            timestamp,
            announced_size,
        })
    }

    /// Size and timestamp of the oldest page (`cc` → `cf`). A timestamp the tablet cannot
    /// express correctly (e.g. its clock was reset to zeros) must not block the sync forever,
    /// since only the oldest page is reachable: the size is then read from the raw frame.
    fn file_info(&mut self) -> Result<(u32, Option<i64>), DeviceError> {
        self.send(&Request::GetFileInfo)?;
        let bytes = self.wait_raw(self.reply_timeout, "page info", opcode::FILE_INFO_VALUE)?;
        match Reply::decode(&bytes) {
            Ok(Reply::FileInfo { size, time }) => Ok((size, Some(time.to_unix()))),
            _ => match bytes.get(2..6) {
                Some(size) => Ok((
                    u32::from_le_bytes([size[0], size[1], size[2], size[3]]),
                    None,
                )),
                None => Err(DeviceError::Unexpected(format!(
                    "malformed page info {}",
                    ardoise_proto::hex::encode(&bytes)
                ))),
            },
        }
    }

    /// Deletes the oldest stored page (the one [`Session::download_oldest`] returns).
    pub fn delete_oldest(&mut self) -> Result<(), DeviceError> {
        self.expect_success(&Request::DeleteOldestFile)
    }

    /// Downloads every stored page, oldest first. Each page goes to `sink` with its 1-based
    /// index and the total; only once the sink returned `Ok` (page safely stored) is it deleted
    /// from the tablet, when `delete` is set. Without deletion only the oldest page can be read
    /// (the tablet has no other way to reach the next one).
    pub fn sync(
        &mut self,
        delete: bool,
        sink: &mut dyn FnMut(DownloadedPage, u16, u16) -> Result<(), String>,
    ) -> Result<SyncSummary, DeviceError> {
        self.set_file_transfer_reporting()?;
        let available = self.stored_pages()?;
        let mut summary = SyncSummary {
            available,
            ..SyncSummary::default()
        };
        let to_read = if delete { available } else { available.min(1) };
        for index in 1..=to_read {
            self.check_cancel()?;
            let page = match self.download_oldest() {
                // Notifications can be lost on the way (BLE stack buffers): try once more.
                Err(DeviceError::CrcMismatch { .. }) => self.download_oldest()?,
                other => other?,
            };
            sink(page, index, available).map_err(DeviceError::Storage)?;
            summary.downloaded += 1;
            if delete {
                self.delete_oldest()?;
                summary.deleted += 1;
            }
        }
        Ok(summary)
    }

    /// Enters live mode: pen samples start flowing (see [`Session::next_live_event`]).
    pub fn start_live(&mut self) -> Result<(), DeviceError> {
        self.live.clear();
        self.live_mode = false;
        self.set_mode(Mode::Live)?;
        // From now on every notification is kept for the caller, in arrival order.
        self.live_mode = true;
        Ok(())
    }

    /// Leaves live mode (idle mode, as the tablet expects).
    pub fn stop_live(&mut self) -> Result<(), DeviceError> {
        let result = self.set_mode(Mode::Idle);
        self.live_mode = false;
        self.live.clear();
        result
    }

    /// Whether live mode was entered and not left.
    pub fn in_live_mode(&self) -> bool {
        self.live_mode
    }

    /// Next raw live-mode pen notification, waiting up to `timeout`. Other notifications
    /// received meanwhile (sysevents…) are skipped: see [`Session::next_live_event`].
    pub fn next_live(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, DeviceError> {
        let deadline = self.clock.monotonic() + timeout;
        loop {
            let now = self.clock.monotonic();
            let left = deadline.saturating_sub(now);
            match self.next_live_event(left)? {
                Some(notification) if notification.channel == Channel::Live => {
                    return Ok(Some(notification.data));
                }
                Some(_) => {}
                None => return Ok(None),
            }
        }
    }

    /// Next notification received during live mode, waiting up to `timeout`: pen data
    /// ([`Channel::Live`]) and, between [`Session::start_live`] and [`Session::stop_live`],
    /// anything else the tablet sends (sysevents, unsolicited command frames…), in arrival
    /// order, each stamped when it came out of the transport. With a zero `timeout` it takes
    /// only what already arrived.
    pub fn next_live_event(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<LiveNotification>, DeviceError> {
        let deadline = self.clock.monotonic() + timeout;
        loop {
            if let Some(notification) = self.live.pop_front() {
                return Ok(Some(notification));
            }
            self.check_cancel()?;
            let now = self.clock.monotonic();
            let received = self.pump(deadline.saturating_sub(now).min(WAIT_SLICE))?;
            if !received && now >= deadline {
                return Ok(None);
            }
        }
    }

    /// Keeps the link while nothing is asked, for up to `timeout`: frames received meanwhile
    /// are dropped (nothing waits for them) and a lost link is reported as
    /// [`DeviceError::Disconnected`]. With a zero `timeout` it only takes in what already
    /// arrived.
    pub fn idle(&mut self, timeout: Duration) -> Result<(), DeviceError> {
        let deadline = self.clock.monotonic() + timeout;
        loop {
            while self.pump(Duration::ZERO)? {}
            self.commands.clear();
            self.live.clear();
            let now = self.clock.monotonic();
            if now >= deadline {
                return Ok(());
            }
            self.pump((deadline - now).min(WAIT_SLICE))?;
        }
    }

    /// Closes the link.
    pub fn disconnect(mut self) -> Result<(), DeviceError> {
        self.connected = false;
        self.transport.disconnect().map_err(DeviceError::from)
    }

    // --- plumbing -------------------------------------------------------------------------

    fn send(&mut self, request: &Request) -> Result<(), DeviceError> {
        if !self.connected {
            return Err(DeviceError::Disconnected);
        }
        let bytes = request
            .encode()
            .map_err(|e| DeviceError::Unexpected(format!("cannot encode {request:?}: {e}")))?;
        // Take in what already arrived (a lost link is noticed before writing), then forget
        // command frames: a late answer to an earlier, timed-out request must not pass for
        // the answer to this one.
        while self.pump(Duration::ZERO)? {}
        self.commands.clear();
        let written = self.transport.write(&bytes);
        self.link_result(written)
    }

    /// Turns "the link is gone" transport errors into [`DeviceError::Disconnected`], so that
    /// callers can reconnect whatever noticed the loss first (a write or a receive).
    fn link_result<T>(&mut self, result: Result<T, TransportError>) -> Result<T, DeviceError> {
        match result {
            Ok(value) => Ok(value),
            Err(TransportError::NotConnected) => {
                self.connected = false;
                Err(DeviceError::Disconnected)
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Waits for the raw bytes of the next command frame with `opcode` (or an error status).
    fn wait_raw(
        &mut self,
        timeout: Duration,
        what: &'static str,
        opcode: u8,
    ) -> Result<Vec<u8>, DeviceError> {
        let deadline = self.clock.monotonic() + timeout;
        loop {
            while let Some(bytes) = self.commands.pop_front() {
                if bytes.first() == Some(&opcode) {
                    return Ok(bytes);
                }
                if let Ok(Reply::Status(code)) = Reply::decode(&bytes)
                    && code != StatusCode::Success
                {
                    return Err(DeviceError::Refused(code));
                }
            }
            self.check_cancel()?;
            let now = self.clock.monotonic();
            if now >= deadline {
                return Err(DeviceError::Timeout(what));
            }
            self.pump((deadline - now).min(WAIT_SLICE))?;
        }
    }

    fn exchange(
        &mut self,
        request: &Request,
        what: &'static str,
        accept: impl Fn(&Reply) -> bool,
    ) -> Result<Reply, DeviceError> {
        self.send(request)?;
        self.wait_reply(self.reply_timeout, what, accept)
    }

    fn expect_success(&mut self, request: &Request) -> Result<(), DeviceError> {
        self.send(request)?;
        self.wait_reply(self.reply_timeout, "acknowledgement", |r| {
            matches!(r, Reply::Status(StatusCode::Success))
        })
        .map(|_| ())
    }

    /// Waits for a command reply accepted by `accept`. An error status is the tablet's
    /// answer to the request and ends the wait; other unexpected frames are skipped.
    fn wait_reply(
        &mut self,
        timeout: Duration,
        what: &'static str,
        accept: impl Fn(&Reply) -> bool,
    ) -> Result<Reply, DeviceError> {
        let deadline = self.clock.monotonic() + timeout;
        let mut malformed = None;
        loop {
            while let Some(bytes) = self.commands.pop_front() {
                // Malformed frames are in the log; they cannot be the expected answer, but they
                // explain a timeout better than silence.
                let reply = match Reply::decode(&bytes) {
                    Ok(reply) => reply,
                    Err(error) => {
                        malformed = Some(format!(
                            "malformed answer {} ({error})",
                            ardoise_proto::hex::encode(&bytes)
                        ));
                        continue;
                    }
                };
                if accept(&reply) {
                    return Ok(reply);
                }
                if let Reply::Status(code) = reply
                    && code != StatusCode::Success
                {
                    return Err(DeviceError::Refused(code));
                }
            }
            self.check_cancel()?;
            let now = self.clock.monotonic();
            if now >= deadline {
                return Err(match malformed {
                    Some(message) => DeviceError::Unexpected(message),
                    None => DeviceError::Timeout(what),
                });
            }
            self.pump((deadline - now).min(WAIT_SLICE))?;
        }
    }

    /// Receives at most one event and files it in its queue.
    fn pump(&mut self, timeout: Duration) -> Result<bool, DeviceError> {
        if !self.connected {
            return Err(DeviceError::Disconnected);
        }
        let received = self.transport.recv(timeout);
        match self.link_result(received)? {
            None => return Ok(false),
            Some(TransportEvent::Notification { channel, data }) => {
                // The latency stamp (system clock, see `LiveNotification`).
                let received = SystemTime::now();
                match channel {
                    Channel::Command => self.commands.push_back(data.clone()),
                    Channel::PageData => {
                        self.page_data.extend_from_slice(&data);
                        self.last_page_data = Some(self.clock.monotonic());
                    }
                    Channel::Live | Channel::SysEvent => {}
                }
                if channel == Channel::Live || self.live_mode {
                    if self.live.len() >= MAX_LIVE_QUEUE {
                        self.live.pop_front();
                    }
                    self.live.push_back(LiveNotification {
                        channel,
                        data,
                        received,
                    });
                }
            }
            Some(TransportEvent::Disconnected) => {
                self.connected = false;
                return Err(DeviceError::Disconnected);
            }
        }
        Ok(true)
    }

    fn check_cancel(&self) -> Result<(), DeviceError> {
        match &self.cancel {
            Some(flag) if flag.load(Ordering::SeqCst) => Err(DeviceError::Cancelled),
            _ => Ok(()),
        }
    }
}

/// 1 to [`MAX_NAME_LEN`] printable ASCII characters (space included).
pub fn valid_name(name: &str) -> bool {
    (1..=MAX_NAME_LEN).contains(&name.len()) && name.bytes().all(|b| (0x20..=0x7e).contains(&b))
}

fn is_name_chunk(reply: &Reply) -> bool {
    matches!(reply, Reply::NameChunk(_))
}

fn unexpected(reply: &Reply) -> DeviceError {
    DeviceError::Unexpected(format!("{reply:?}"))
}

/// Current time of `clock` in unix seconds.
pub fn unix_seconds(clock: &dyn Clock) -> i64 {
    match clock.now().duration_since(UNIX_EPOCH) {
        Ok(elapsed) => i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX),
        Err(before) => -i64::try_from(before.duration().as_secs()).unwrap_or(i64::MAX),
    }
}

#[cfg(test)]
#[path = "session_tests.rs"]
mod tests;
