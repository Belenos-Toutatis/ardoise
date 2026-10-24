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
        let cancel = self.cancel.clone();
        let deadline = self.clock.monotonic() + timeout;
        // Short scans in a loop, so that cancellation is noticed.
        loop {
            if cancel.load(Ordering::SeqCst) {
                return Err(DeviceError::Cancelled);
            }
            let now = self.clock.monotonic();
            if now >= deadline {
                self.note("no tablet found");
                return Ok(None);
            }
            let slice = (deadline - now).min(Duration::from_secs(2));
            let found = self.connector.find(slice, &mut |ad: &Advertisement| {
                ad.is_wacom() && (!register_mode || ad.in_register_mode())
            })?;
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
        let clock_drift = self.prepare(&mut session)?;
        let info = session.info()?;
        let record = DeviceRecord {
            format: DEVICE_FORMAT,
            uuid: id,
            address: tablet.address.clone(),
            name: info.name.clone(),
            protocol: "slate".into(),
            width: info.width,
            height: info.height,
            registered: chrono::DateTime::<chrono::Local>::from(self.clock.now())
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
            extra: Default::default(),
        };
        identity::save(&self.library, &record).map_err(|e| DeviceError::Storage(e.to_string()))?;
        self.note("registration saved in device.json");
        Ok((session, Registration { info, clock_drift }))
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
    /// `on_packet`. A lost link is reopened with the saved identity and live mode resumed;
    /// returns the number of reconnections.
    pub fn live(
        &mut self,
        session: &mut Session,
        record: &DeviceRecord,
        until: Duration,
        on_packet: &mut dyn FnMut(&LivePacket),
    ) -> Result<u32, DeviceError> {
        let mut reconnections = 0;
        session.start_live()?;
        loop {
            let now = self.clock.monotonic();
            if now >= until {
                break;
            }
            match session.next_live((until - now).min(Duration::from_millis(250))) {
                Ok(Some(raw)) => match live::decode(&raw) {
                    Ok(packet) => on_packet(&packet),
                    Err(error) => self.note(format!("malformed live packet: {error}")),
                },
                Ok(None) => {}
                Err(DeviceError::Disconnected) => {
                    self.note("link lost during live mode, reconnecting");
                    let address = session.peer().address.clone();
                    let (mut fresh, _) = self.connect(record, Some(&address))?;
                    fresh.start_live()?;
                    *session = fresh;
                    reconnections += 1;
                }
                Err(error) => return Err(error),
            }
        }
        session.stop_live()?;
        Ok(reconnections)
    }
}
