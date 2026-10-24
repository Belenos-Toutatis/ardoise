//! Placeholder for platforms other than Windows: there is never an adapter.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use ardoise_device::transport::{
    Advertisement, Clock, Connector, SystemClock, Transport, TransportError, TransportEvent,
};

/// Finds nothing: Bluetooth LE is only implemented on Windows.
#[derive(Debug)]
pub struct BleConnector {
    clock: Arc<SystemClock>,
}

impl BleConnector {
    pub fn new() -> Result<Self, TransportError> {
        Ok(Self {
            clock: Arc::new(SystemClock::new()),
        })
    }
}

impl Connector for BleConnector {
    fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    fn find(
        &mut self,
        _timeout: Duration,
        _filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError> {
        Err(TransportError::NoAdapter)
    }

    fn connect(
        &mut self,
        _address: &str,
        _timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        Err(TransportError::NoAdapter)
    }
}

/// Never constructed on this platform.
#[derive(Debug)]
#[cfg_attr(all(windows, test), expect(dead_code))]
pub struct BleTransport {
    never: Infallible,
}

impl Transport for BleTransport {
    fn peer(&self) -> &Advertisement {
        match self.never {}
    }

    fn write(&mut self, _frame: &[u8]) -> Result<(), TransportError> {
        match self.never {}
    }

    fn recv(&mut self, _timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
        match self.never {}
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        match self.never {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_no_adapter() {
        let mut connector = BleConnector::new().unwrap();
        assert_eq!(
            connector.find(Duration::from_secs(1), &mut |_| true),
            Err(TransportError::NoAdapter)
        );
        assert!(matches!(
            connector.connect("E2:43:03:67:0E:01", Duration::from_secs(1)),
            Err(TransportError::NoAdapter)
        ));
        let clock = connector.clock();
        let a = clock.monotonic();
        assert!(clock.monotonic() >= a);
    }
}
