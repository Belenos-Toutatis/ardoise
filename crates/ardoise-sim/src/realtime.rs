//! The simulated tablet in real time, for `ardoise.exe --simulated-tablet` and the latency
//! tests (docs/design/tablet-live-classroom.md, "Simulated tablet in the app").
//!
//! - [`RealTimeConnector`]: a `Connector` over a [`SimTablet`] whose clock follows the
//!   computer's: `recv` and `find` wait for real, so live mode streams 200 samples per second
//!   for real and the receipt stamps of the device layer measure a real latency. Best with
//!   [`SimTablet::real_time`]; a tablet on a plain virtual clock is moved forward by the
//!   connector as real time passes.
//! - [`SimFactory`]: hands the tablet service connectors over one simulated tablet, in real
//!   time or in virtual time (tests).
//! - [`SimHooks`]: the person in front of the simulated tablet for the service: holds the
//!   button when the wizard asks, presses it to confirm, writes when live mode starts.
//! - [`app_tablet`]: the tablet of `--simulated-tablet`: a Bamboo Slate Large holding the three
//!   reference pages. [`app_board`]: the blackboard of `--simulated-blackboard`, likewise.
//!
//! The connectors and the factory reach one or several simulated devices ([`SimDevice`]): the
//! Slate, the blackboard, or both (the service then connects to whichever it hears first).

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ardoise_device::service::{ConnectorFactory, ServiceHooks};
use ardoise_device::transport::{
    Advertisement, Clock, Connector, Transport, TransportError, TransportEvent,
};

use crate::board::{SimBoard, SimBoardConfig, reference_board_pages};
use crate::clock::VirtualClock;
use crate::connector::{SimDevice, clocks};
use crate::drawings;
use crate::tablet::{SimConfig, SimTablet};

/// How often `find` and `connect` look at the tablet again.
const SCAN_POLL: Duration = Duration::from_millis(20);
/// Longest sleep of `recv` before it looks at the tablet again (a button pressed meanwhile
/// adds a notification due at once).
const RECV_POLL: Duration = Duration::from_millis(2);
/// Shortest sleep of `recv`, so that it never spins.
const RECV_MIN_SLEEP: Duration = Duration::from_micros(200);
/// Pen movement queued when live mode starts.
pub const LIVE_SCRIPT_SECONDS: u32 = 600;
/// Delay of the simulated user's actions in the app (the wizard has time to show each step).
pub const APP_HOOK_DELAY: Duration = Duration::from_secs(1);

/// The tablet of `ardoise.exe --simulated-tablet`: a Bamboo Slate Large in real time (battery
/// 87 %), never registered, with the three reference pages stored
/// ([`drawings::reference_pages`]).
pub fn app_tablet() -> SimTablet {
    let tablet = SimTablet::real_time(SimConfig::slate_large());
    let config = tablet.config();
    for page in drawings::reference_pages(config.width, config.height) {
        tablet.draw_page(&page);
    }
    tablet
}

/// The blackboard of `ardoise.exe --simulated-blackboard`: in real time (on `clock` when
/// given, the simulated Slate's when both are simulated), battery 90 %, with the three reference
/// pages stored, a second apart.
pub fn app_board(clock: Option<Arc<VirtualClock>>) -> SimBoard {
    let config = SimBoardConfig::blackboard();
    let board = match clock {
        Some(clock) => SimBoard::with_clock(config, clock),
        None => SimBoard::real_time(config),
    };
    for page in reference_board_pages() {
        board.draw_page(&page);
    }
    board
}

/// `now + duration`, or a far future when it does not fit.
fn deadline(duration: Duration) -> Instant {
    let now = Instant::now();
    now.checked_add(duration)
        .unwrap_or_else(|| now + Duration::from_secs(365 * 24 * 3600))
}

/// Keeps the tablets' clocks at least at the real time elapsed since the connector was made
/// (nothing to do for a [`SimTablet::real_time`] clock, which follows by itself).
#[derive(Debug, Clone)]
struct Pace {
    clock: Arc<VirtualClock>,
    origin: Instant,
    base: Duration,
}

impl Pace {
    fn new(clock: Arc<VirtualClock>) -> Self {
        let base = clock.monotonic();
        Self {
            clock,
            origin: Instant::now(),
            base,
        }
    }

    fn sync(&self) {
        if !self.clock.is_real_time() {
            self.clock.advance_to(self.base + self.origin.elapsed());
        }
    }
}

/// Every clock of a connector kept in pace.
#[derive(Debug, Clone)]
struct Paces(Vec<Pace>);

impl Paces {
    fn new(devices: &[SimDevice]) -> Self {
        Self(clocks(devices).into_iter().map(Pace::new).collect())
    }

    fn sync(&self) {
        for pace in &self.0 {
            pace.sync();
        }
    }
}

/// Finds and connects to simulated tablets, their clocks following the real time.
#[derive(Debug, Clone)]
pub struct RealTimeConnector {
    devices: Vec<SimDevice>,
    pace: Paces,
}

impl RealTimeConnector {
    /// A connector over `tablet`, preferably made with [`SimTablet::real_time`] (a virtual
    /// clock is moved forward only while the connector or one of its links is used).
    pub fn new(tablet: SimTablet) -> Self {
        Self::over(vec![SimDevice::Slate(tablet)])
    }

    /// A connector over every device of `devices`.
    pub fn over(devices: Vec<SimDevice>) -> Self {
        let pace = Paces::new(&devices);
        Self { devices, pace }
    }

    /// The simulated Slate, if any.
    pub fn tablet(&self) -> Option<&SimTablet> {
        self.devices.iter().find_map(|device| match device {
            SimDevice::Slate(tablet) => Some(tablet),
            SimDevice::Board(_) => None,
        })
    }
}

impl Connector for RealTimeConnector {
    fn clock(&self) -> Arc<dyn Clock> {
        match self.devices.first() {
            Some(device) => device.clock(),
            None => Arc::new(VirtualClock::following_real_time()),
        }
    }

    fn find(
        &mut self,
        timeout: Duration,
        filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError> {
        let deadline = deadline(timeout);
        loop {
            self.pace.sync();
            for device in &self.devices {
                if let Some(advertisement) = device.advertisement()
                    && filter(&advertisement)
                {
                    return Ok(Some(advertisement));
                }
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            std::thread::sleep((deadline - now).min(SCAN_POLL));
        }
    }

    fn connect(
        &mut self,
        address: &str,
        timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        let Some(device) = self
            .devices
            .iter()
            .find(|device| device.address().eq_ignore_ascii_case(address))
            .cloned()
        else {
            return Err(TransportError::NotFound(address.to_owned()));
        };
        // Switched off or busy: nothing answers until the timeout, waited for real.
        let deadline = deadline(timeout);
        self.pace.sync();
        while device.advertisement().is_none() {
            self.pace.sync();
            let now = Instant::now();
            if now >= deadline {
                return Err(TransportError::Timeout);
            }
            std::thread::sleep((deadline - now).min(SCAN_POLL));
        }
        // A zero timeout: opening the link never moves the clock ahead of the real time.
        let (id, peer) = device.open_link(address, Duration::ZERO)?;
        Ok(Box::new(RealTimeTransport {
            device,
            pace: self.pace.clone(),
            id,
            peer,
            closed: false,
        }))
    }
}

/// One open link to a simulated tablet in real time. Dropping it frees the tablet.
#[derive(Debug)]
pub struct RealTimeTransport {
    device: SimDevice,
    pace: Paces,
    id: u64,
    peer: Advertisement,
    closed: bool,
}

impl Transport for RealTimeTransport {
    fn peer(&self) -> &Advertisement {
        &self.peer
    }

    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::NotConnected);
        }
        self.pace.sync();
        self.device.link_write(self.id, frame)
    }

    /// Delivers what is due now on the real clock, else sleeps until the next planned event
    /// (at most [`RECV_POLL`] at a time) or the timeout.
    fn recv(&mut self, timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
        if self.closed {
            return Err(TransportError::NotConnected);
        }
        let deadline = deadline(timeout);
        loop {
            self.pace.sync();
            // A zero timeout delivers only what is due and never moves the clock.
            if let Some(event) = self.device.link_recv(self.id, Duration::ZERO)? {
                if event == TransportEvent::Disconnected {
                    self.closed = true;
                }
                return Ok(Some(event));
            }
            let now = Instant::now();
            if now >= deadline {
                return Ok(None);
            }
            let mut wait = (deadline - now).min(RECV_POLL);
            if let Some(due) = self.device.link_next_due(self.id) {
                wait = wait.min(due.saturating_sub(self.device.clock().monotonic()));
            }
            std::thread::sleep(wait.max(RECV_MIN_SLEEP).min(deadline - now));
        }
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        if !self.closed {
            self.closed = true;
            self.device.close_link(self.id);
        }
        Ok(())
    }
}

impl Drop for RealTimeTransport {
    fn drop(&mut self) {
        if !self.closed {
            self.device.close_link(self.id);
        }
    }
}

/// Connectors over simulated tablets for the tablet service. The adapter check always passes.
#[derive(Debug, Clone)]
pub struct SimFactory {
    devices: Vec<SimDevice>,
    real_time: bool,
}

impl SimFactory {
    /// [`RealTimeConnector`]s (the app's simulated tablet, latency tests).
    pub fn real_time(tablet: SimTablet) -> Self {
        Self::of(vec![SimDevice::Slate(tablet)], true)
    }

    /// [`SimConnector`](crate::SimConnector)s on the tablet's virtual clock (fast tests).
    pub fn virtual_time(tablet: SimTablet) -> Self {
        Self::of(vec![SimDevice::Slate(tablet)], false)
    }

    /// Connectors over `devices` (in real time or on their virtual clock, which they must
    /// share).
    pub fn of(devices: Vec<SimDevice>, real_time: bool) -> Self {
        Self { devices, real_time }
    }

    /// The same factory with the blackboard `board` in range too.
    pub fn with_board(mut self, board: SimBoard) -> Self {
        self.devices.push(SimDevice::Board(board));
        self
    }

    /// The simulated Slate, if any.
    pub fn tablet(&self) -> Option<&SimTablet> {
        self.devices.iter().find_map(|device| match device {
            SimDevice::Slate(tablet) => Some(tablet),
            SimDevice::Board(_) => None,
        })
    }
}

impl ConnectorFactory for SimFactory {
    fn connector(&mut self) -> Result<Box<dyn Connector>, TransportError> {
        Ok(if self.real_time {
            Box::new(RealTimeConnector::over(self.devices.clone()))
        } else {
            Box::new(crate::SimConnector::over(self.devices.clone()))
        })
    }
}

/// The person in front of the simulated tablets, for the tablet service: holds the Slate's
/// button [`APP_HOOK_DELAY`] after the wizard asks, presses it [`APP_HOOK_DELAY`] after the
/// wizard asks to confirm, and queues [`LIVE_SCRIPT_SECONDS`] of handwriting (hovering
/// included) on the tablet whose live mode starts.
#[derive(Debug)]
pub struct SimHooks {
    tablet: Option<SimTablet>,
    board: Option<SimBoard>,
    delay: Duration,
    /// Seed of the next live handwriting, so that each live session writes something new.
    seed: AtomicU64,
}

impl SimHooks {
    /// The app's simulated user (actions after [`APP_HOOK_DELAY`], on a thread of their own).
    pub fn new(tablet: SimTablet) -> Self {
        Self::with_delay(tablet, APP_HOOK_DELAY)
    }

    /// Actions after `delay`; with a zero delay they happen at once, on the caller's thread
    /// (the right choice with a virtual clock, which a sleeping thread would not follow).
    pub fn with_delay(tablet: SimTablet, delay: Duration) -> Self {
        Self {
            tablet: Some(tablet),
            board: None,
            delay,
            seed: AtomicU64::new(0x5eed_a2d0_1520_2026),
        }
    }

    /// The user of a simulated blackboard alone (it has no registration to act on).
    pub fn for_board(board: SimBoard) -> Self {
        Self {
            tablet: None,
            board: Some(board),
            delay: APP_HOOK_DELAY,
            seed: AtomicU64::new(0x5eed_a2d0_1520_2026),
        }
    }

    /// The same user, with the blackboard `board` too.
    pub fn with_board(mut self, board: SimBoard) -> Self {
        self.board = Some(board);
        self
    }

    fn later(&self, action: fn(&SimTablet)) {
        let Some(tablet) = &self.tablet else {
            return;
        };
        if self.delay.is_zero() {
            action(tablet);
            return;
        }
        let tablet = tablet.clone();
        let delay = self.delay;
        let spawned = std::thread::Builder::new()
            .name("simulated-user".into())
            .spawn(move || {
                std::thread::sleep(delay);
                action(&tablet);
            });
        if spawned.is_err()
            && let Some(tablet) = &self.tablet
        {
            // No thread: act at once rather than never.
            action(tablet);
        }
    }

    fn next_seed(&self) -> u64 {
        self.seed.fetch_add(1, Ordering::SeqCst)
    }
}

impl ServiceHooks for SimHooks {
    fn registration_started(&self) {
        self.later(SimTablet::hold_button);
    }

    fn waiting_for_button(&self) {
        self.later(SimTablet::press_button);
    }

    fn live_started(&self) {
        let Some(tablet) = &self.tablet else {
            return;
        };
        let config = tablet.config();
        tablet.draw_live(drawings::live_script_seeded(
            LIVE_SCRIPT_SECONDS,
            config.width,
            config.height,
            self.next_seed(),
        ));
    }

    fn blackboard_live_started(&self) {
        let Some(board) = &self.board else {
            return;
        };
        board.draw_live(drawings::live_script_seeded(
            LIVE_SCRIPT_SECONDS,
            ardoise_proto::blackboard::BOARD_WIDTH,
            ardoise_proto::blackboard::BOARD_HEIGHT,
            self.next_seed(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ardoise_proto::{Reply, Request};

    #[test]
    fn the_app_tablet_holds_the_reference_pages() {
        let tablet = app_tablet();
        assert_eq!(tablet.stored_page_count(), 3);
        assert_eq!(tablet.config().battery_percent, 87);
        assert!(tablet.clock().is_real_time());
    }

    #[test]
    fn replies_and_waits_follow_the_real_clock() {
        let tablet = SimTablet::real_time(SimConfig::slate_large());
        tablet.set_registered_id(Some([1, 2, 3, 4, 5, 6]));
        let mut connector = RealTimeConnector::new(tablet.clone());
        let found = connector
            .find(Duration::from_secs(1), &mut |ad| ad.is_wacom())
            .unwrap()
            .unwrap();
        let mut link = connector
            .connect(&found.address, Duration::from_secs(1))
            .unwrap();
        // The connection sysevent, then the answer to CONNECT.
        assert!(matches!(
            link.recv(Duration::from_millis(100)).unwrap(),
            Some(TransportEvent::Notification { .. })
        ));
        let connect = Request::Connect {
            uuid: [1, 2, 3, 4, 5, 6],
        };
        link.write(&connect.encode().unwrap()).unwrap();
        let Some(TransportEvent::Notification { data, .. }) =
            link.recv(Duration::from_millis(100)).unwrap()
        else {
            panic!("no answer");
        };
        assert_eq!(
            Reply::decode(&data).unwrap(),
            Reply::Status(ardoise_proto::StatusCode::Success)
        );
        // Nothing more: the wait lasts for real.
        let started = Instant::now();
        assert_eq!(link.recv(Duration::from_millis(30)).unwrap(), None);
        assert!(started.elapsed() >= Duration::from_millis(30));
        // Busy: a second connection times out for real.
        let started = Instant::now();
        assert!(matches!(
            connector.connect(&found.address, Duration::from_millis(40)),
            Err(TransportError::Timeout)
        ));
        assert!(started.elapsed() >= Duration::from_millis(40));
        // The clock never ran ahead of the real time.
        assert!(tablet.clock().monotonic() < Duration::from_secs(5));
    }

    #[test]
    fn a_virtual_clock_is_moved_with_the_real_time() {
        let tablet = SimTablet::slate_large();
        tablet.set_registered_id(Some([1, 2, 3, 4, 5, 6]));
        let mut connector = RealTimeConnector::new(tablet.clone());
        let address = tablet.config().address;
        let mut link = connector.connect(&address, Duration::from_secs(1)).unwrap();
        let connect = Request::Connect {
            uuid: [1, 2, 3, 4, 5, 6],
        };
        link.write(&connect.encode().unwrap()).unwrap();
        link.write(
            &Request::SetMode(ardoise_proto::Mode::Live)
                .encode()
                .unwrap(),
        )
        .unwrap();
        tablet.draw_live(drawings::live_script(1, 29_700, 21_600));
        // Pen data arrives over about a second of real time, not at once.
        let started = Instant::now();
        let mut live = 0;
        while started.elapsed() < Duration::from_millis(300) {
            if let Some(TransportEvent::Notification {
                channel: ardoise_device::transport::Channel::Live,
                ..
            }) = link.recv(Duration::from_millis(10)).unwrap()
            {
                live += 1;
            }
        }
        // 300 ms of a script streaming about 67 notifications per second.
        assert!((5..=40).contains(&live), "{live} live notifications");
        assert!(tablet.clock().monotonic() >= Duration::from_millis(300));
    }

    #[test]
    fn hooks_act_on_the_tablet() {
        let tablet = SimTablet::slate_large();
        let hooks = SimHooks::with_delay(tablet.clone(), Duration::ZERO);
        hooks.registration_started();
        assert!(tablet.in_register_mode());
        let delayed = SimTablet::real_time(SimConfig::slate_large());
        let hooks = SimHooks::with_delay(delayed.clone(), Duration::from_millis(20));
        hooks.registration_started();
        assert!(!delayed.in_register_mode(), "not yet");
        let deadline = Instant::now() + Duration::from_secs(5);
        while !delayed.in_register_mode() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(delayed.in_register_mode());
    }
}
