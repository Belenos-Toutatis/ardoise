//! The device stack's view of the simulated tablet: a [`Connector`] and its [`Transport`]s.

use std::sync::Arc;
use std::time::Duration;

use ardoise_device::transport::{
    Advertisement, Clock, Connector, Transport, TransportError, TransportEvent,
};

use crate::tablet::SimTablet;

/// Finds and connects to one simulated tablet, on its virtual clock.
///
/// `find` returns at once when the tablet is visible and accepted by the filter (the
/// registration-mode filter only after [`SimTablet::hold_button`]); otherwise the virtual
/// clock jumps by the timeout and nothing is found.
#[derive(Debug, Clone)]
pub struct SimConnector {
    tablet: SimTablet,
}

impl SimConnector {
    pub(crate) fn new(tablet: SimTablet) -> Self {
        Self { tablet }
    }

    /// The tablet behind this connector.
    pub fn tablet(&self) -> &SimTablet {
        &self.tablet
    }
}

impl Connector for SimConnector {
    fn clock(&self) -> Arc<dyn Clock> {
        self.tablet.clock()
    }

    fn find(
        &mut self,
        timeout: Duration,
        filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError> {
        if let Some(advertisement) = self.tablet.advertisement()
            && filter(&advertisement)
        {
            return Ok(Some(advertisement));
        }
        self.tablet.clock().advance(timeout);
        Ok(None)
    }

    fn connect(
        &mut self,
        address: &str,
        timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        let (id, peer) = self.tablet.open_link(address, timeout)?;
        Ok(Box::new(SimTransport {
            tablet: self.tablet.clone(),
            id,
            peer,
            closed: false,
        }))
    }
}

/// One open link to the simulated tablet. Dropping it frees the tablet, like a host that
/// goes away.
#[derive(Debug)]
pub struct SimTransport {
    tablet: SimTablet,
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
        self.tablet.link_write(self.id, frame)
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
        if self.closed {
            return Err(TransportError::NotConnected);
        }
        let event = self.tablet.link_recv(self.id, timeout)?;
        if event == Some(TransportEvent::Disconnected) {
            self.closed = true;
        }
        Ok(event)
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        if !self.closed {
            self.closed = true;
            self.tablet.close_link(self.id);
        }
        Ok(())
    }
}

impl Drop for SimTransport {
    fn drop(&mut self) {
        if !self.closed {
            self.tablet.close_link(self.id);
        }
    }
}
