//! One conversation with the Xiaomi blackboard (docs/design/blackboard.md): the flows of
//! docs/research/xiaomi-blackboard.md §3 over a [`Transport`].
//!
//! There is no authentication and no registration: the tablet answers any client, and the
//! Xiaomi service (`fe95`) is never written to. A [`BoardSession`] only ever sends the
//! requests of [`BoardRequest`], the frames seen in Mi Home's captures.
//!
//! Like the Slate's [`Session`](crate::session::Session), it sorts what arrives as it comes:
//! replies wait in a queue for the request that expects them, the bytes of the stream
//! characteristic go to the page being downloaded or, in live mode, to the caller; anything
//! else (stream bytes nobody asked for, unknown or malformed frames) is dropped and counted
//! ([`BoardSession::take_oddities`]), never an error. The diagnostic log has every frame
//! anyway (the [`LoggingTransport`](crate::logging::LoggingTransport)).

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime};

use ardoise_proto::blackboard::{BoardReply, BoardRequest, PageInfo};

use crate::session::{DeviceError, DownloadedPage, LiveNotification, SyncSummary, unix_seconds};
use crate::transport::{
    Advertisement, Channel, Clock, TabletKind, Transport, TransportError, TransportEvent,
};

/// Wait for the reply to a command. The tablet answered in under 50 ms in every capture.
pub const REPLY_TIMEOUT: Duration = Duration::from_secs(5);
/// A download is abandoned when no byte of the page arrived for this long. A page of 52 185
/// bytes took 2.1 s in Mi Home's capture (research note §3).
pub const DATA_IDLE_TIMEOUT: Duration = Duration::from_secs(5);
/// Most pages one sync reads, whatever the tablet says: about 400 fit in its memory (research
/// note §1). Guards against a tablet that never runs out of pages.
pub const MAX_SYNC_PAGES: u32 = 1000;
/// Granularity of the waits, so that cancellation is noticed quickly.
const WAIT_SLICE: Duration = Duration::from_millis(250);
/// Live notifications kept while nobody reads them (oldest dropped first).
const MAX_LIVE_QUEUE: usize = 4096;

/// What the tablet says about itself (`11 01 01`, `11 01 02`, `12 01 04`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BoardInfo {
    /// Per cent (inferred from the values 100 and 90 seen).
    pub battery_percent: u8,
    /// Memory in bytes.
    pub memory_total: u32,
    pub memory_free: u32,
    pub stored_pages: u16,
    /// Bytes of the stored pages.
    pub stored_bytes: u32,
}

/// What a session dropped since it was last asked ([`BoardSession::take_oddities`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Oddities {
    /// Frames of the reply characteristic that are unknown or malformed, as hex.
    pub frames: Vec<String>,
    /// Bytes of the stream characteristic received outside a download and outside live mode.
    pub stray_bytes: usize,
    /// Notifications of a channel the blackboard does not have.
    pub other_channels: usize,
}

impl Oddities {
    pub fn is_empty(&self) -> bool {
        self.frames.is_empty() && self.stray_bytes == 0 && self.other_channels == 0
    }
}

/// A page being downloaded.
struct Download {
    expected: usize,
    bytes: Vec<u8>,
    /// When the last byte came (session clock), or when the request was sent.
    last: Duration,
}

/// An open link to the blackboard.
pub struct BoardSession {
    transport: Box<dyn Transport>,
    clock: Arc<dyn Clock>,
    cancel: Option<Arc<AtomicBool>>,
    commands: VecDeque<Vec<u8>>,
    download: Option<Download>,
    live: VecDeque<LiveNotification>,
    live_mode: bool,
    /// The tablet said a page was stored with its button (`12 01 09`) and nobody asked yet.
    page_stored: bool,
    oddities: Oddities,
    connected: bool,
    reply_timeout: Duration,
    data_timeout: Duration,
}

impl BoardSession {
    /// Starts a conversation on an open link. Nothing is sent: the tablet needs no login.
    pub fn open(transport: Box<dyn Transport>, clock: Arc<dyn Clock>) -> Self {
        Self {
            transport,
            clock,
            cancel: None,
            commands: VecDeque::new(),
            download: None,
            live: VecDeque::new(),
            live_mode: false,
            page_stored: false,
            oddities: Oddities::default(),
            connected: true,
            reply_timeout: REPLY_TIMEOUT,
            data_timeout: DATA_IDLE_TIMEOUT,
        }
    }

    /// Lets the caller interrupt long waits (download, live mode).
    pub fn set_cancel_flag(&mut self, cancel: Arc<AtomicBool>) {
        self.cancel = Some(cancel);
    }

    /// Shorter waits (tests of a stalled transfer).
    pub fn with_timeouts(mut self, reply: Duration, data: Duration) -> Self {
        self.reply_timeout = reply;
        self.data_timeout = data;
        self
    }

    pub fn peer(&self) -> &Advertisement {
        self.transport.peer()
    }

    pub fn is_connected(&self) -> bool {
        self.connected
    }

    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// Sets the tablet's clock to the computer's (Unix seconds, UTC): `11 05 07`, no reply.
    pub fn set_time(&mut self) -> Result<(), DeviceError> {
        let now = unix_seconds(&*self.clock);
        let unix = u32::try_from(now)
            .map_err(|_| DeviceError::Unexpected(format!("cannot send the time {now}")))?;
        self.send(BoardRequest::SetTime { unix })
    }

    /// Battery level in per cent (`11 01 01`).
    pub fn battery(&mut self) -> Result<u8, DeviceError> {
        match self.exchange(BoardRequest::Battery, "battery", |r| {
            matches!(r, BoardReply::Battery { .. })
        })? {
            BoardReply::Battery { percent } => Ok(percent),
            other => Err(unexpected(&other)),
        }
    }

    /// Total and free memory in bytes (`11 01 02`).
    pub fn memory(&mut self) -> Result<(u32, u32), DeviceError> {
        match self.exchange(BoardRequest::Memory, "memory", |r| {
            matches!(r, BoardReply::Memory { .. })
        })? {
            BoardReply::Memory { total, free } => Ok((total, free)),
            other => Err(unexpected(&other)),
        }
    }

    /// Pages stored and their total size in bytes (`12 01 04`).
    pub fn page_count(&mut self) -> Result<(u16, u32), DeviceError> {
        match self.exchange(BoardRequest::PageCount, "page count", |r| {
            matches!(r, BoardReply::PageCount { .. })
        })? {
            BoardReply::PageCount { count, bytes } => Ok((count, bytes)),
            other => Err(unexpected(&other)),
        }
    }

    /// Battery, memory and stored pages, in Mi Home's order.
    pub fn info(&mut self) -> Result<BoardInfo, DeviceError> {
        let battery_percent = self.battery()?;
        let (memory_total, memory_free) = self.memory()?;
        let (stored_pages, stored_bytes) = self.page_count()?;
        Ok(BoardInfo {
            battery_percent,
            memory_total,
            memory_free,
            stored_pages,
            stored_bytes,
        })
    }

    /// The oldest stored page (`12 01 05`), `None` when the tablet holds none: it only ever
    /// describes its oldest page.
    pub fn oldest_page(&mut self) -> Result<Option<PageInfo>, DeviceError> {
        match self.exchange(BoardRequest::OldestPage, "oldest page", |r| {
            matches!(r, BoardReply::OldestPage(_))
        })? {
            BoardReply::OldestPage(info) => Ok(info),
            other => Err(unexpected(&other)),
        }
    }

    /// Downloads the page `info` describes (`12 06 03 01 <time>`): its bytes come on the stream
    /// characteristic, with no header, no end mark and no checksum, so they are counted against
    /// the size announced. No byte for [`DATA_IDLE_TIMEOUT`], or more bytes than announced, is
    /// [`DeviceError::TransferIncomplete`]. Nothing is deleted here.
    pub fn download(&mut self, info: PageInfo) -> Result<DownloadedPage, DeviceError> {
        let expected = usize::try_from(info.size).unwrap_or(usize::MAX);
        let incomplete = |received| DeviceError::TransferIncomplete {
            expected: info.size,
            received,
        };
        // Stream bytes still in flight (the end of a live stream) must not join the page, nor
        // an earlier reply pass for anything.
        self.drain()?;
        self.commands.clear();
        self.download = Some(Download {
            expected,
            bytes: Vec::with_capacity(expected.min(1 << 20)),
            last: self.clock.monotonic(),
        });
        let result = self.receive_page(BoardRequest::Download { time: info.time }, &incomplete);
        let download = self.download.take();
        result?;
        let bytes = download.map(|d| d.bytes).unwrap_or_default();
        if bytes.len() != expected {
            return Err(incomplete(bytes.len()));
        }
        Ok(DownloadedPage {
            bytes,
            timestamp: Some(i64::from(info.time)),
            announced_size: info.size,
            kind: TabletKind::Blackboard,
        })
    }

    fn receive_page(
        &mut self,
        request: BoardRequest,
        incomplete: &dyn Fn(usize) -> DeviceError,
    ) -> Result<(), DeviceError> {
        self.write(request)?;
        loop {
            let (received, expected, last) = match &self.download {
                Some(d) => (d.bytes.len(), d.expected, d.last),
                None => return Ok(()),
            };
            if received > expected {
                return Err(incomplete(received));
            }
            if received == expected {
                return Ok(());
            }
            self.check_cancel()?;
            let now = self.clock.monotonic();
            let idle = now.saturating_sub(last);
            if idle >= self.data_timeout {
                return Err(incomplete(received));
            }
            self.pump((self.data_timeout - idle).min(WAIT_SLICE))?;
        }
    }

    /// Deletes the page stored at `time` (`12 05 06 <time>`): no reply. Only ever called once
    /// the page is safe in the library (see [`BoardSession::sync`]).
    pub fn delete(&mut self, time: u32) -> Result<(), DeviceError> {
        self.send(BoardRequest::Delete { time })
    }

    /// Reads every stored page, oldest first: describe, download, hand it to `sink`, and only
    /// once the sink returned `Ok` (the note written and verified) delete it, then describe the
    /// next one. The tablet only describes its oldest page, so reading the next one needs the
    /// deletion: a sync always deletes. Stops when the tablet holds no page, after
    /// [`MAX_SYNC_PAGES`], or with [`DeviceError::PageNotDeleted`] when the tablet still
    /// describes the page it was asked to delete.
    pub fn sync(
        &mut self,
        sink: &mut dyn FnMut(DownloadedPage, u16) -> Result<(), String>,
    ) -> Result<SyncSummary, DeviceError> {
        let (available, _) = self.page_count()?;
        let mut summary = SyncSummary {
            available,
            ..SyncSummary::default()
        };
        let mut deleted: Option<u32> = None;
        for index in 1..=MAX_SYNC_PAGES {
            self.check_cancel()?;
            let Some(info) = self.oldest_page()? else {
                break;
            };
            if deleted == Some(info.time) {
                return Err(DeviceError::PageNotDeleted(info.time));
            }
            let page = match self.download(info) {
                // Notifications can be lost on the way (BLE stack buffers): try once more.
                Err(DeviceError::TransferIncomplete { .. }) => self.download(info)?,
                other => other?,
            };
            sink(page, u16::try_from(index).unwrap_or(u16::MAX)).map_err(DeviceError::Storage)?;
            summary.downloaded = summary.downloaded.saturating_add(1);
            self.delete(info.time)?;
            summary.deleted = summary.deleted.saturating_add(1);
            deleted = Some(info.time);
        }
        Ok(summary)
    }

    /// Starts the live stream (`12 02 08 01`, no reply): records follow on the stream
    /// characteristic, see [`BoardSession::next_live_event`].
    pub fn start_live(&mut self) -> Result<(), DeviceError> {
        self.live.clear();
        self.live_mode = false;
        self.send(BoardRequest::StartLive)?;
        self.live_mode = true;
        Ok(())
    }

    /// Stops the live stream (`12 02 08 02`, no reply).
    pub fn stop_live(&mut self) -> Result<(), DeviceError> {
        let result = self.send(BoardRequest::StopLive);
        self.live_mode = false;
        self.live.clear();
        result
    }

    pub fn in_live_mode(&self) -> bool {
        self.live_mode
    }

    /// Next notification of live mode, waiting up to `timeout`: records
    /// ([`Channel::BoardData`]) and the frames of the reply characteristic
    /// ([`Channel::Command`]: the "page stored" event, unknown frames), in arrival order, each
    /// stamped when it came out of the transport. With a zero `timeout` it takes only what
    /// already arrived.
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

    /// Keeps the link while nothing is asked, for up to `timeout`; a lost link is
    /// [`DeviceError::Disconnected`]. With a zero `timeout` it only takes in what arrived.
    pub fn idle(&mut self, timeout: Duration) -> Result<(), DeviceError> {
        let deadline = self.clock.monotonic() + timeout;
        loop {
            self.drain()?;
            self.commands.clear();
            self.live.clear();
            let now = self.clock.monotonic();
            if now >= deadline {
                return Ok(());
            }
            self.pump((deadline - now).min(WAIT_SLICE))?;
        }
    }

    /// Whether the tablet said a page was stored with its button since the last call.
    pub fn take_page_stored(&mut self) -> bool {
        std::mem::take(&mut self.page_stored)
    }

    /// What was dropped since the last call (for the diagnostic log).
    pub fn take_oddities(&mut self) -> Oddities {
        std::mem::take(&mut self.oddities)
    }

    /// Closes the link.
    pub fn disconnect(mut self) -> Result<(), DeviceError> {
        self.connected = false;
        self.transport.disconnect().map_err(DeviceError::from)
    }

    // --- plumbing -------------------------------------------------------------------------

    /// Takes in everything already arrived.
    fn drain(&mut self) -> Result<(), DeviceError> {
        while self.pump(Duration::ZERO)? {}
        Ok(())
    }

    /// Writes `request` after taking in what already arrived (a lost link is noticed before
    /// writing) and forgetting earlier replies (a late answer must not pass for this one's).
    fn send(&mut self, request: BoardRequest) -> Result<(), DeviceError> {
        self.drain()?;
        self.commands.clear();
        self.write(request)
    }

    fn write(&mut self, request: BoardRequest) -> Result<(), DeviceError> {
        if !self.connected {
            return Err(DeviceError::Disconnected);
        }
        let written = self.transport.write(&request.encode());
        self.link_result(written)
    }

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

    fn exchange(
        &mut self,
        request: BoardRequest,
        what: &'static str,
        accept: impl Fn(&BoardReply) -> bool,
    ) -> Result<BoardReply, DeviceError> {
        self.send(request)?;
        let deadline = self.clock.monotonic() + self.reply_timeout;
        loop {
            while let Some(bytes) = self.commands.pop_front() {
                if let Ok(reply) = BoardReply::decode(&bytes)
                    && accept(&reply)
                {
                    return Ok(reply);
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

    /// Receives at most one event and files it.
    fn pump(&mut self, timeout: Duration) -> Result<bool, DeviceError> {
        if !self.connected {
            return Err(DeviceError::Disconnected);
        }
        let received = self.transport.recv(timeout);
        match self.link_result(received)? {
            None => Ok(false),
            Some(TransportEvent::Disconnected) => {
                self.connected = false;
                Err(DeviceError::Disconnected)
            }
            Some(TransportEvent::Notification { channel, data }) => {
                // The latency stamp (system clock, see `LiveNotification`).
                let received = SystemTime::now();
                self.file(channel, data, received);
                Ok(true)
            }
        }
    }

    fn file(&mut self, channel: Channel, data: Vec<u8>, received: SystemTime) {
        match channel {
            Channel::Command => {
                match BoardReply::decode(&data) {
                    Ok(BoardReply::PageStored) => self.page_stored = true,
                    Ok(BoardReply::Unknown(_)) | Err(_) => {
                        self.oddities.frames.push(ardoise_proto::hex::encode(&data));
                    }
                    Ok(_) => {}
                }
                if self.live_mode {
                    self.push_live(channel, data.clone(), received);
                }
                self.commands.push_back(data);
            }
            Channel::BoardData => {
                if let Some(download) = self.download.as_mut() {
                    download.bytes.extend_from_slice(&data);
                    download.last = self.clock.monotonic();
                } else if self.live_mode {
                    self.push_live(channel, data, received);
                } else {
                    self.oddities.stray_bytes += data.len();
                }
            }
            Channel::PageData | Channel::Live | Channel::SysEvent => {
                self.oddities.other_channels += 1;
            }
        }
    }

    fn push_live(&mut self, channel: Channel, data: Vec<u8>, received: SystemTime) {
        if self.live.len() >= MAX_LIVE_QUEUE {
            self.live.pop_front();
        }
        self.live.push_back(LiveNotification {
            channel,
            data,
            received,
        });
    }

    fn check_cancel(&self) -> Result<(), DeviceError> {
        match &self.cancel {
            Some(flag) if flag.load(Ordering::SeqCst) => Err(DeviceError::Cancelled),
            _ => Ok(()),
        }
    }
}

fn unexpected(reply: &BoardReply) -> DeviceError {
    DeviceError::Unexpected(format!("{reply:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{TestClock, hex, on};
    use std::sync::Mutex;
    use std::sync::atomic::AtomicU64;

    /// Plays the blackboard: each write must be the next expected frame; its answers follow.
    struct Board {
        peer: Advertisement,
        clock: Arc<TestClock>,
        steps: VecDeque<(Vec<u8>, Vec<TransportEvent>)>,
        pending: VecDeque<TransportEvent>,
        writes: Arc<Mutex<Vec<Vec<u8>>>>,
    }

    impl Transport for Board {
        fn peer(&self) -> &Advertisement {
            &self.peer
        }
        fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
            self.writes.lock().unwrap().push(frame.to_vec());
            let (expected, answers) = self
                .steps
                .pop_front()
                .unwrap_or_else(|| panic!("unexpected write {frame:02x?}"));
            assert_eq!(frame, expected.as_slice(), "write");
            self.pending.extend(answers);
            Ok(())
        }
        fn recv(&mut self, timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
            if let Some(event) = self.pending.pop_front() {
                return Ok(Some(event));
            }
            let ms = u64::try_from(timeout.as_millis()).unwrap();
            self.clock
                .0
                .fetch_add(ms, std::sync::atomic::Ordering::SeqCst);
            Ok(None)
        }
        fn disconnect(&mut self) -> Result<(), TransportError> {
            Ok(())
        }
    }

    fn reply(text: &str) -> TransportEvent {
        on(Channel::Command, hex(text))
    }

    fn data(bytes: &[u8]) -> TransportEvent {
        on(Channel::BoardData, bytes.to_vec())
    }

    fn session(
        steps: Vec<(&str, Vec<TransportEvent>)>,
    ) -> (BoardSession, Arc<Mutex<Vec<Vec<u8>>>>) {
        let clock = Arc::new(TestClock(AtomicU64::new(0)));
        let writes = Arc::new(Mutex::new(Vec::new()));
        let board = Board {
            peer: Advertisement {
                address: "00:00:5E:00:53:B1".into(),
                name: Some("xiaomi.dev.ble".into()),
                rssi: None,
                company_id: None,
                manufacturer_data: Vec::new(),
                xiaomi_service_data: Vec::new(),
            },
            clock: clock.clone(),
            steps: steps.into_iter().map(|(w, a)| (hex(w), a)).collect(),
            pending: VecDeque::new(),
            writes: writes.clone(),
        };
        (BoardSession::open(Box::new(board), clock), writes)
    }

    /// A page of `n` records (7 bytes each).
    fn page_bytes(n: usize) -> Vec<u8> {
        (0..n)
            .flat_map(|i| [0xa1, i as u8, 0, 0, 1, 0, 4])
            .collect()
    }

    #[test]
    fn information_and_clock() {
        let (mut board, writes) = session(vec![
            // 2026-10-24 12:00:00 UTC, the test clock's start.
            ("11 05 07 6a dc 9d c0", vec![]),
            ("11 01 01", vec![reply("11 02 01 5a")]),
            ("11 01 02", vec![reply("11 09 02 07 90 7e 00 07 8e ba 00")]),
            ("12 01 04", vec![reply("12 07 04 00 02 00 00 00 1c")]),
        ]);
        board.set_time().unwrap();
        let info = board.info().unwrap();
        assert_eq!(
            info,
            BoardInfo {
                battery_percent: 90,
                memory_total: 126_909_952,
                memory_free: 126_794_240,
                stored_pages: 2,
                stored_bytes: 28,
            }
        );
        assert_eq!(writes.lock().unwrap().len(), 4);
    }

    #[test]
    fn a_sync_deletes_each_page_after_the_sink_kept_it() {
        let first = page_bytes(3);
        let second = page_bytes(40);
        let (mut board, writes) = session(vec![
            ("12 01 04", vec![reply("12 07 04 00 02 00 00 01 2d")]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 06 03 01 68 00 00 01", vec![data(&first)]),
            ("12 05 06 68 00 00 01", vec![]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 02 00 00 01 18")]),
            // 280 bytes in two chunks, as the tablet cuts them (244 at most).
            (
                "12 06 03 01 68 00 00 02",
                vec![data(&second[..244]), data(&second[244..])],
            ),
            ("12 05 06 68 00 00 02", vec![]),
            ("12 01 05", vec![reply("12 01 05")]),
        ]);
        let mut kept = Vec::new();
        let summary = board
            .sync(&mut |page, index| {
                assert_eq!(page.kind, TabletKind::Blackboard);
                kept.push((index, page.timestamp, page.bytes.len()));
                Ok(())
            })
            .unwrap();
        assert_eq!(
            kept,
            [(1, Some(0x6800_0001), 21), (2, Some(0x6800_0002), 280)]
        );
        assert_eq!(
            (summary.available, summary.downloaded, summary.deleted),
            (2, 2, 2)
        );
        assert_eq!(writes.lock().unwrap().len(), 8);
    }

    #[test]
    fn a_page_that_cannot_be_kept_is_not_deleted() {
        let (mut board, writes) = session(vec![
            ("12 01 04", vec![reply("12 07 04 00 01 00 00 00 15")]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page_bytes(3))]),
        ]);
        let result = board.sync(&mut |_, _| Err("disk full".into()));
        assert_eq!(result, Err(DeviceError::Storage("disk full".into())));
        // No `12 05 06` was written.
        assert!(
            writes
                .lock()
                .unwrap()
                .iter()
                .all(|w| w[..3] != [0x12, 0x05, 0x06])
        );
    }

    #[test]
    fn a_short_or_stalled_transfer_is_a_typed_error_and_nothing_is_deleted() {
        let page = page_bytes(10);
        let (board, writes) = session(vec![
            ("12 01 04", vec![reply("12 07 04 00 01 00 00 00 46")]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 46")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page[..30])]),
            // Tried once more, stalled again.
            ("12 06 03 01 68 00 00 01", vec![data(&page[..44])]),
        ]);
        let mut board = board.with_timeouts(REPLY_TIMEOUT, Duration::from_secs(2));
        let result = board.sync(&mut |_, _| panic!("nothing to keep"));
        assert_eq!(
            result,
            Err(DeviceError::TransferIncomplete {
                expected: 70,
                received: 44
            })
        );
        assert_eq!(writes.lock().unwrap().len(), 4);

        // More bytes than announced: refused too.
        let (mut board, _) = session(vec![(
            "12 06 03 01 68 00 00 01",
            vec![data(&page_bytes(4))],
        )]);
        let info = PageInfo {
            time: 0x6800_0001,
            size: 21,
        };
        assert_eq!(
            board.download(info),
            Err(DeviceError::TransferIncomplete {
                expected: 21,
                received: 28
            })
        );
    }

    #[test]
    fn a_page_still_described_after_its_deletion_stops_the_sync() {
        let (mut board, _) = session(vec![
            ("12 01 04", vec![reply("12 07 04 00 01 00 00 00 15")]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
            ("12 06 03 01 68 00 00 01", vec![data(&page_bytes(3))]),
            ("12 05 06 68 00 00 01", vec![]),
            ("12 01 05", vec![reply("12 09 05 68 00 00 01 00 00 00 15")]),
        ]);
        let mut kept = 0;
        let result = board.sync(&mut |_, _| {
            kept += 1;
            Ok(())
        });
        assert_eq!(result, Err(DeviceError::PageNotDeleted(0x6800_0001)));
        assert_eq!(kept, 1);
    }

    #[test]
    fn an_empty_tablet_syncs_nothing() {
        let (mut board, _) = session(vec![
            ("12 01 04", vec![reply("12 07 04 00 00 00 00 00 00")]),
            ("12 01 05", vec![reply("12 01 05")]),
        ]);
        let summary = board.sync(&mut |_, _| panic!("no page")).unwrap();
        assert_eq!(summary, SyncSummary::default());
    }

    #[test]
    fn unknown_input_is_ignored_and_counted() {
        let (mut board, _) = session(vec![(
            "11 01 01",
            vec![
                reply("12 01 0b"),
                reply("13"),
                data(&[0xa1; 7]),
                on(Channel::SysEvent, vec![1]),
                reply("12 01 09"),
                reply("11 02 01 64"),
            ],
        )]);
        assert_eq!(board.battery().unwrap(), 100);
        let oddities = board.take_oddities();
        assert_eq!(oddities.frames, ["12 01 0b", "13"]);
        assert_eq!(oddities.stray_bytes, 7);
        assert_eq!(oddities.other_channels, 1);
        assert!(board.take_page_stored());
        assert!(!board.take_page_stored());
        assert!(board.take_oddities().is_empty());
    }

    #[test]
    fn live_mode_hands_over_records_and_events_in_order() {
        let records = page_bytes(6);
        let (mut board, _) = session(vec![
            (
                "12 02 08 01",
                vec![data(&records), reply("12 01 09"), data(&records[..7])],
            ),
            ("12 02 08 02", vec![]),
        ]);
        board.start_live().unwrap();
        assert!(board.in_live_mode());
        let mut seen = Vec::new();
        while let Some(n) = board.next_live_event(Duration::from_millis(100)).unwrap() {
            seen.push((n.channel, n.data.len()));
        }
        assert_eq!(
            seen,
            [
                (Channel::BoardData, 42),
                (Channel::Command, 3),
                (Channel::BoardData, 7)
            ]
        );
        assert!(
            board.take_page_stored(),
            "live mode goes on, the event is noted"
        );
        board.stop_live().unwrap();
        assert!(!board.in_live_mode());
    }

    #[test]
    fn a_lost_link_is_disconnected() {
        let (mut board, _) = session(vec![("12 01 04", vec![TransportEvent::Disconnected])]);
        assert_eq!(board.page_count(), Err(DeviceError::Disconnected));
        assert!(!board.is_connected());
        assert_eq!(board.battery(), Err(DeviceError::Disconnected));
    }
}
