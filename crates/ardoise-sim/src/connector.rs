//! The device stack's view of the simulated tablets: a [`Connector`] and its [`Transport`]s.

use std::sync::Arc;
use std::time::Duration;

use ardoise_device::transport::{
    Advertisement, Clock, Connector, Transport, TransportError, TransportEvent,
};

use crate::board::SimBoard;
use crate::clock::VirtualClock;
use crate::tablet::SimTablet;

/// One simulated peripheral a connector can reach: a Bamboo Slate or the Xiaomi blackboard.
#[derive(Debug, Clone)]
pub enum SimDevice {
    Slate(SimTablet),
    Board(SimBoard),
}

impl SimDevice {
    pub fn clock(&self) -> Arc<VirtualClock> {
        match self {
            SimDevice::Slate(tablet) => tablet.clock(),
            SimDevice::Board(board) => board.clock(),
        }
    }

    pub fn address(&self) -> String {
        match self {
            SimDevice::Slate(tablet) => tablet.config().address,
            SimDevice::Board(board) => board.config().address,
        }
    }

    pub fn advertisement(&self) -> Option<Advertisement> {
        match self {
            SimDevice::Slate(tablet) => tablet.advertisement(),
            SimDevice::Board(board) => board.advertisement(),
        }
    }

    pub(crate) fn open_link(
        &self,
        address: &str,
        timeout: Duration,
    ) -> Result<(u64, Advertisement), TransportError> {
        match self {
            SimDevice::Slate(tablet) => tablet.open_link(address, timeout),
            SimDevice::Board(board) => board.open_link(address, timeout),
        }
    }

    pub(crate) fn link_write(&self, id: u64, bytes: &[u8]) -> Result<(), TransportError> {
        match self {
            SimDevice::Slate(tablet) => tablet.link_write(id, bytes),
            SimDevice::Board(board) => board.link_write(id, bytes),
        }
    }

    pub(crate) fn link_recv(
        &self,
        id: u64,
        timeout: Duration,
    ) -> Result<Option<TransportEvent>, TransportError> {
        match self {
            SimDevice::Slate(tablet) => tablet.link_recv(id, timeout),
            SimDevice::Board(board) => board.link_recv(id, timeout),
        }
    }

    pub(crate) fn link_next_due(&self, id: u64) -> Option<Duration> {
        match self {
            SimDevice::Slate(tablet) => tablet.link_next_due(id),
            SimDevice::Board(board) => board.link_next_due(id),
        }
    }

    pub(crate) fn close_link(&self, id: u64) {
        match self {
            SimDevice::Slate(tablet) => tablet.close_link(id),
            SimDevice::Board(board) => board.close_link(id),
        }
    }
}

/// The distinct clocks of `devices` (devices made on one clock share it).
pub(crate) fn clocks(devices: &[SimDevice]) -> Vec<Arc<VirtualClock>> {
    let mut clocks: Vec<Arc<VirtualClock>> = Vec::new();
    for device in devices {
        let clock = device.clock();
        if !clocks.iter().any(|known| Arc::ptr_eq(known, &clock)) {
            clocks.push(clock);
        }
    }
    clocks
}

/// Finds and connects to simulated tablets, on their virtual clock (one clock for all of them:
/// make the blackboard with [`SimBoard::with_clock`] on the Slate's).
///
/// `find` returns at once when a tablet is visible and accepted by the filter (the
/// registration-mode filter only after [`SimTablet::hold_button`]); otherwise the virtual
/// clock jumps by the timeout and nothing is found.
#[derive(Debug, Clone)]
pub struct SimConnector {
    devices: Vec<SimDevice>,
}

impl SimConnector {
    pub(crate) fn new(tablet: SimTablet) -> Self {
        Self::over(vec![SimDevice::Slate(tablet)])
    }

    /// A connector reaching every device of `devices` (the first one's clock is the
    /// connector's).
    pub fn over(devices: Vec<SimDevice>) -> Self {
        Self { devices }
    }

    /// The simulated Slate behind this connector, if any.
    pub fn tablet(&self) -> Option<&SimTablet> {
        self.devices.iter().find_map(|device| match device {
            SimDevice::Slate(tablet) => Some(tablet),
            SimDevice::Board(_) => None,
        })
    }

    /// The simulated blackboard behind this connector, if any.
    pub fn board(&self) -> Option<&SimBoard> {
        self.devices.iter().find_map(|device| match device {
            SimDevice::Board(board) => Some(board),
            SimDevice::Slate(_) => None,
        })
    }

    fn device(&self, address: &str) -> Option<&SimDevice> {
        self.devices
            .iter()
            .find(|device| device.address().eq_ignore_ascii_case(address))
    }
}

impl Connector for SimConnector {
    fn clock(&self) -> Arc<dyn Clock> {
        match self.devices.first() {
            Some(device) => device.clock(),
            None => Arc::new(VirtualClock::new()),
        }
    }

    fn find(
        &mut self,
        timeout: Duration,
        filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError> {
        for device in &self.devices {
            if let Some(advertisement) = device.advertisement()
                && filter(&advertisement)
            {
                return Ok(Some(advertisement));
            }
        }
        for clock in clocks(&self.devices) {
            clock.advance(timeout);
        }
        Ok(None)
    }

    fn connect(
        &mut self,
        address: &str,
        timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        let device = self
            .device(address)
            .cloned()
            .ok_or_else(|| TransportError::NotFound(address.to_owned()))?;
        let (id, peer) = device.open_link(address, timeout)?;
        Ok(Box::new(SimTransport {
            device,
            id,
            peer,
            closed: false,
        }))
    }
}

/// One open link to a simulated tablet. Dropping it frees the tablet, like a host that goes
/// away.
#[derive(Debug)]
pub struct SimTransport {
    device: SimDevice,
    id: u64,
    peer: Advertisement,
    /// Closed by the host, or lost (the `Disconnected` event was delivered).
    closed: bool,
}

impl Transport for SimTransport {
    fn peer(&self) -> &Advertisement {
        &self.peer
    }

    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if self.closed {
            return Err(TransportError::NotConnected);
        }
        self.device.link_write(self.id, frame)
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
        if self.closed {
            return Err(TransportError::NotConnected);
        }
        let event = self.device.link_recv(self.id, timeout)?;
        if event == Some(TransportEvent::Disconnected) {
            self.closed = true;
        }
        Ok(event)
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        if !self.closed {
            self.closed = true;
            self.device.close_link(self.id);
        }
        Ok(())
    }
}

impl Drop for SimTransport {
    fn drop(&mut self) {
        if !self.closed {
            self.device.close_link(self.id);
        }
    }
}
