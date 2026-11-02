//! Ties a connector, the library's shared registration (`device.json`) and the diagnostic log
//! together: find a tablet, register it once for every PC, connect with the saved identity,
//! keep live mode running across disconnections.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use ardoise_proto::live::{self, LivePacket};

use crate::identity::{self, DEVICE_FORMAT, DeviceRecord, HostId};
use crate::log::{SharedLog, lock};
use crate::logging::LoggingTransport;
use crate::session::{DeviceError, DeviceInfo, Session, unix_seconds};
use crate::transport::{Advertisement, Clock, Connector, Transport};

/// How long opening a link may take.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Tablet clocks further off than this are reported (protocol.md §4.4).
pub const CLOCK_DRIFT_WARNING: i64 = 300;
/// Length of one scan; between scans, cancellation is checked.
const SCAN_SLICE: Duration = Duration::from_secs(5);

/// A freshly registered tablet.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registration {
    pub info: DeviceInfo,
    /// Tablet clock minus computer clock right after setting it, in seconds.
    pub clock_drift: i64,
}

/// Finds, registers and connects tablets for one library.
pub struct Manager {
    connector: Box<dyn Connector>,
    clock: Arc<dyn Clock>,
    library: PathBuf,
    log: SharedLog,
    cancel: Arc<AtomicBool>,
    /// Identity used for the next registration; random when `None`.
    host_id: Option<HostId>,
}

impl Manager {
    pub fn new(connector: Box<dyn Connector>, library: PathBuf, log: SharedLog) -> Self {
        let clock = connector.clock();
        Self {
            connector,
            clock,
            library,
            log,
            cancel: Arc::new(AtomicBool::new(false)),
            host_id: None,
        }
    }

    /// Registers with `id` instead of a random identity. Used to replay a recorded
    /// registration byte for byte.
    pub fn with_host_id(mut self, id: HostId) -> Self {
        self.host_id = Some(id);
        self
    }

    /// Uses `cancel` as the cancellation flag (see [`Manager::cancel_flag`]).
    pub fn with_cancel_flag(mut self, cancel: Arc<AtomicBool>) -> Self {
        self.cancel = cancel;
        self
    }

    pub fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    pub fn log(&self) -> &SharedLog {
        &self.log
    }

    pub fn library(&self) -> &Path {
        &self.library
    }

    /// Setting this flag interrupts waits as soon as possible with [`DeviceError::Cancelled`].
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel.clone()
    }

    /// The tablet registered from this library (on any PC), if any.
    pub fn registration(&self) -> Result<Option<DeviceRecord>, DeviceError> {
        identity::load(&self.library).map_err(|e| DeviceError::Storage(e.to_string()))
    }

    fn note(&self, text: impl Into<String>) {
        lock(&self.log).note(text);
    }

    /// Waits up to `timeout` for a Wacom tablet. With `register_mode`, only a tablet waiting to
    /// be registered (LED blinking blue) is accepted.
    pub fn find(
        &mut self,
        register_mode: bool,
        timeout: Duration,
    ) -> Result<Option<Advertisement>, DeviceError> {
        self.note(if register_mode {
            "looking for a tablet in registration mode"
        } else {
            "looking for a tablet"
        });
        self.scan(timeout, &mut |ad: &Advertisement| {
            ad.is_wacom() && (!register_mode || ad.in_register_mode())
        })
    }

    /// Waits up to `timeout` for the registered tablet, at its saved address only: another
    /// tablet in range (a school may hold several Slates) is never connected to without the
    /// user (a new registration). Another Wacom tablet seen meanwhile is noted once per call.
    ///
    /// The real Slate A4 keeps its address (a static random one, `DD:…`): in the first hardware
    /// session every connection used it. Its advertisements come at any moment of a scan, which
    /// is why the scan is not cut short (the former "any tablet after two thirds of the time"
    /// fallback accepted the registered tablet itself, seen late).
    pub fn find_registered(
        &mut self,
        record: &DeviceRecord,
        timeout: Duration,
    ) -> Result<Option<Advertisement>, DeviceError> {
        self.note(format!(
            "looking for the registered tablet {}",
            record.address
        ));
        let address = record.address.clone();
        let mut others = Vec::<String>::new();
        let found = self.scan(timeout, &mut |ad: &Advertisement| {
            if ad.address.eq_ignore_ascii_case(&address) {
                return ad.is_wacom();
            }
            if ad.is_wacom() && !others.contains(&ad.address) {
                others.push(ad.address.clone());
            }
            false
        });
        for other in others {
            self.note(format!(
                "another tablet ({other}) is in range: ignored, it is not the registered one"
            ));
        }
        found
    }

    /// Scans in slices (so that cancellation is noticed) until `filter` accepts a tablet.
    /// Within a slice, cancellation ends the scan at the next advertisement received.
    fn scan(
        &mut self,
        timeout: Duration,
        filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, DeviceError> {
        let cancel = self.cancel.clone();
        let deadline = self.clock.monotonic() + timeout;
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(DeviceError::Cancelled);
            }
            let now = self.clock.monotonic();
            if now >= deadline {
                self.note("no tablet found");
                return Ok(None);
            }
            let slice = (deadline - now).min(SCAN_SLICE);
            let found = self.connector.find(slice, &mut |ad: &Advertisement| {
                cancel.load(Ordering::SeqCst) || filter(ad)
            })?;
            if cancel.load(Ordering::SeqCst) {
                return Err(DeviceError::Cancelled);
            }
            if let Some(ad) = found {
                self.note(format!(
                    "found {} at {} (rssi {:?}, manufacturer data {} bytes)",
                    ad.name.as_deref().unwrap_or("unnamed tablet"),
                    ad.address,
                    ad.rssi,
                    ad.manufacturer_data.len()
                ));
                return Ok(Some(ad));
            }
        }
    }

    fn open(&mut self, address: &str) -> Result<Box<dyn Transport>, DeviceError> {
        self.note(format!("connecting to {address}"));
        let transport = self.connector.connect(address, CONNECT_TIMEOUT)?;
        lock(&self.log).event("connected");
        Ok(Box::new(LoggingTransport::new(transport, self.log.clone())))
    }

    /// Registers a tablet found in registration mode with a new identity, reads its
    /// information and saves the registration in the library for every PC.
    /// `waiting_for_button` is called when the user must press the button to confirm.
    pub fn register(
        &mut self,
        tablet: &Advertisement,
        button_timeout: Duration,
        waiting_for_button: &mut dyn FnMut(),
    ) -> Result<(Session, Registration), DeviceError> {
        let transport = self.open(&tablet.address)?;
        let id = match self.host_id {
            Some(id) => id,
            None => HostId::generate().map_err(|e| DeviceError::Storage(e.to_string()))?,
        };
        self.note("registering");
        let mut session = Session::register(
            transport,
            self.clock.clone(),
            id,
            button_timeout,
            Some(self.cancel.clone()),
            waiting_for_button,
        )?;
        // From now on the tablet accepts only `id`, and the previous registration is dead:
        // save it before anything else can fail, with provisional details.
        let mut record = DeviceRecord {
            format: DEVICE_FORMAT,
            uuid: id,
            address: tablet.address.clone(),
            name: tablet.name.clone().unwrap_or_else(|| "Bamboo Slate".into()),
            protocol: "slate".into(),
            width: ardoise_proto::SLATE_LARGE_WIDTH,
            height: ardoise_proto::SLATE_LARGE_HEIGHT,
            registered: chrono::DateTime::<chrono::Local>::from(self.clock.now())
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            extra: Default::default(),
        };
        self.save(&record)?;
        self.note("registration saved in device.json");
        let clock_drift = self.prepare(&mut session)?;
        let info = session.info()?;
        record.name = info.name.clone();
        record.width = info.width;
        record.height = info.height;
        self.save(&record)?;
        Ok((session, Registration { info, clock_drift }))
    }

    fn save(&self, record: &DeviceRecord) -> Result<(), DeviceError> {
        identity::save(&self.library, record).map_err(|e| DeviceError::Storage(e.to_string()))
    }

    /// Connects with the saved identity, to `address` or to the registered address.
    /// Returns the session and the tablet clock drift in seconds.
    pub fn connect(
        &mut self,
        record: &DeviceRecord,
        address: Option<&str>,
    ) -> Result<(Session, i64), DeviceError> {
        let transport = self.open(address.unwrap_or(&record.address))?;
        let mut session = Session::connect(transport, self.clock.clone(), record.uuid)?;
        session.set_cancel_flag(self.cancel.clone());
        let drift = self.prepare(&mut session)?;
        Ok((session, drift))
    }

    /// Sets the tablet clock and reads it back; returns the remaining drift.
    fn prepare(&mut self, session: &mut Session) -> Result<i64, DeviceError> {
        session.set_cancel_flag(self.cancel.clone());
        session.set_time()?;
        let drift = session.read_time()? - unix_seconds(&*self.clock);
        if drift.abs() > CLOCK_DRIFT_WARNING {
            self.note(format!("tablet clock is {drift} s off after setting it"));
        }
        Ok(drift)
    }

    /// Runs live mode until the session clock reaches `until`, handing every decoded packet to
    /// `on_packet`. The tablet drops idle links by itself: a lost link (even before live mode
    /// started) is reopened with the saved identity, retrying until `until`, and live mode
    /// resumed. Returns the number of reconnections.
    pub fn live(
        &mut self,
        session: &mut Session,
        record: &DeviceRecord,
        until: Duration,
        on_packet: &mut dyn FnMut(&LivePacket),
    ) -> Result<u32, DeviceError> {
        let mut reconnections = 0;
        let mut live = false;
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Err(DeviceError::Cancelled);
            }
            let now = self.clock.monotonic();
            if now >= until {
                break;
            }
            let result = if live {
                session
                    .next_live((until - now).min(Duration::from_millis(250)))
                    .map(|raw| {
                        if let Some(raw) = raw {
                            match live::decode(&raw) {
                                Ok(packet) => on_packet(&packet),
                                Err(error) => self.note(format!("malformed live packet: {error}")),
                            }
                        }
                    })
            } else {
                session.start_live().map(|()| live = true)
            };
            match result {
                Ok(()) => {}
                Err(DeviceError::Disconnected) => {
                    live = false;
                    self.note("link lost during live mode, reconnecting");
                    if !self.reconnect_until(session, record, until)? {
                        break;
                    }
                    reconnections += 1;
                }
                Err(error) => return Err(error),
            }
        }
        if live && session.is_connected() {
            // The samples are what matters: failing to leave live mode is only noted.
            if let Err(error) = session.stop_live() {
                self.note(format!("could not leave live mode: {error}"));
            }
        }
        Ok(reconnections)
    }

    /// Reopens `session` with the saved identity, retrying until `until`. Between attempts it
    /// waits for the tablet to advertise again. `Ok(false)` when time ran out.
    fn reconnect_until(
        &mut self,
        session: &mut Session,
        record: &DeviceRecord,
        until: Duration,
    ) -> Result<bool, DeviceError> {
        let address = session.peer().address.clone();
        loop {
            if self.cancel.load(Ordering::SeqCst) {
                return Err(DeviceError::Cancelled);
            }
            let now = self.clock.monotonic();
            if now >= until {
                self.note("could not reconnect before the end of live mode");
                return Ok(false);
            }
            match self.connect(record, Some(&address)) {
                Ok((fresh, _)) => {
                    *session = fresh;
                    return Ok(true);
                }
                Err(error @ (DeviceError::Cancelled | DeviceError::NotRegistered)) => {
                    return Err(error);
                }
                Err(error) => {
                    self.note(format!("reconnection failed: {error}"));
                    let wait = until.saturating_sub(self.clock.monotonic()).min(SCAN_SLICE);
                    let wanted = address.clone();
                    self.scan(wait, &mut |ad: &Advertisement| {
                        ad.address.eq_ignore_ascii_case(&wanted)
                    })?;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::DiagnosticLog;
    use crate::testutil::{ScriptConnector, cmd};
    use crate::transport::TransportEvent;

    const ID: HostId = HostId([0x89, 0xea, 0x78, 0x88, 0xff, 0xcb]);
    // 2026-10-24 12:00:00 UTC, the scripted clock's start.
    const SET_TIME: &str = "b6 06 26 10 24 12 00 00";
    const TIME: &str = "bd 06 26 10 24 12 00 00";

    fn manager(connector: ScriptConnector, library: &Path) -> Manager {
        let clock = connector.clock();
        let log = DiagnosticLog::in_memory("test", clock).shared();
        Manager::new(Box::new(connector), library.to_path_buf(), log).with_host_id(ID)
    }

    fn record() -> DeviceRecord {
        DeviceRecord {
            format: DEVICE_FORMAT,
            uuid: ID,
            address: "E2:43:03:67:0E:01".into(),
            name: "Bamboo Slate".into(),
            protocol: "slate".into(),
            width: 29700,
            height: 21600,
            registered: "2026-10-24T14:00:00+02:00".into(),
            extra: Default::default(),
        }
    }

    #[test]
    fn registration_is_saved_as_soon_as_the_tablet_confirms_it() {
        let library = tempfile::tempdir().unwrap();
        let connector = ScriptConnector::new(
            vec![vec![
                ("e7 06 89 ea 78 88 ff cb", vec![cmd("e4 01 00")]),
                // The tablet then refuses to set its clock: the registration must survive.
                (SET_TIME, vec![cmd("b3 01 02")]),
            ]],
            true,
        );
        let mut manager = manager(connector, library.path());
        let tablet = manager.find(true, Duration::from_secs(5)).unwrap().unwrap();
        let result = manager.register(&tablet, Duration::from_secs(10), &mut || {});
        assert_eq!(
            result.err(),
            Some(DeviceError::Refused(
                ardoise_proto::StatusCode::InvalidState
            ))
        );
        let saved = manager.registration().unwrap().unwrap();
        assert_eq!(saved.uuid, ID);
        assert_eq!(saved.address, "E2:43:03:67:0E:01");
    }

    #[test]
    fn live_mode_reconnects_when_the_idle_link_is_already_gone() {
        let library = tempfile::tempdir().unwrap();
        let sample = crate::testutil::hex("a1 06 10 27 20 4e 00 04");
        let connector = ScriptConnector::new(
            vec![
                vec![
                    ("e6 06 89 ea 78 88 ff cb", vec![cmd("b3 01 00")]),
                    (SET_TIME, vec![cmd("b3 01 00")]),
                    // The tablet drops the link right after: the loss is noticed later.
                    ("b6 01 00", vec![cmd(TIME), TransportEvent::Disconnected]),
                ],
                vec![
                    ("e6 06 89 ea 78 88 ff cb", vec![cmd("b3 01 00")]),
                    (SET_TIME, vec![cmd("b3 01 00")]),
                    ("b6 01 00", vec![cmd(TIME)]),
                    (
                        "b1 01 00",
                        vec![
                            cmd("b3 01 00"),
                            TransportEvent::Notification {
                                channel: crate::transport::Channel::Live,
                                data: sample,
                            },
                        ],
                    ),
                    ("b1 01 02", vec![cmd("b3 01 00")]),
                ],
            ],
            false,
        );
        let mut manager = manager(connector, library.path());
        let record = record();
        let (mut session, drift) = manager.connect(&record, None).unwrap();
        assert_eq!(drift, 0);
        let until = manager.clock().monotonic() + Duration::from_secs(1);
        let mut packets = 0;
        let reconnections = manager
            .live(&mut session, &record, until, &mut |_| packets += 1)
            .unwrap();
        assert_eq!(reconnections, 1);
        assert_eq!(packets, 1);
    }

    #[test]
    fn only_the_registered_address_is_accepted() {
        let library = tempfile::tempdir().unwrap();
        let connector = ScriptConnector::new(vec![], false);
        let mut manager = manager(connector, library.path());
        let mut other = record();
        other.address = "AA:BB:CC:DD:EE:FF".into();
        // The scripted tablet has another address: never accepted, only noted.
        let found = manager
            .find_registered(&other, Duration::from_secs(3))
            .unwrap();
        assert_eq!(found, None);
        assert_eq!(manager.clock().monotonic(), Duration::from_secs(3));
        let notes: Vec<String> = lock(manager.log())
            .entries()
            .iter()
            .map(|e| e.info.clone())
            .collect();
        assert!(
            notes.contains(
                &"another tablet (E2:43:03:67:0E:01) is in range: ignored, it is not the registered one"
                    .to_owned()
            ),
            "{notes:?}"
        );
        assert!(!notes.iter().any(|n| n.contains("accepting any tablet")));
        // Its own address: found at once, whatever the case of the letters.
        let mut lower = record();
        lower.address = "e2:43:03:67:0e:01".into();
        let found = manager
            .find_registered(&lower, Duration::from_secs(3))
            .unwrap()
            .unwrap();
        assert_eq!(found.address, "E2:43:03:67:0E:01");
    }
}
