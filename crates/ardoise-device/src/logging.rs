//! A transport decorator that records every exchange in the diagnostic log, whatever the
//! underlying link (Bluetooth, simulator, replay).

use std::time::Duration;

use crate::log::{SharedLog, lock};
use crate::transport::{Advertisement, Channel, Transport, TransportError, TransportEvent};

/// Records every write and every received event of `inner`, with its interpretation.
pub struct LoggingTransport {
    inner: Box<dyn Transport>,
    log: SharedLog,
    /// The end of the link is already in the log.
    closed: bool,
}

impl LoggingTransport {
    pub fn new(inner: Box<dyn Transport>, log: SharedLog) -> Self {
        Self {
            inner,
            log,
            closed: false,
        }
    }
}

/// One-line interpretation of bytes received on `channel`.
pub fn describe_received(channel: Channel, data: &[u8]) -> String {
    match channel {
        Channel::Command => ardoise_proto::describe::describe_reply(data),
        Channel::PageData => ardoise_proto::describe::describe_page_data(data),
        Channel::Live => ardoise_proto::describe::describe_live(data),
        Channel::SysEvent => ardoise_proto::describe::describe_sysevent(data),
    }
}

impl Transport for LoggingTransport {
    fn peer(&self) -> &Advertisement {
        self.inner.peer()
    }

    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        lock(&self.log).tx(frame, ardoise_proto::describe::describe_request(frame));
        let result = self.inner.write(frame);
        if let Err(error) = &result {
            lock(&self.log).event(format!("write failed: {error}"));
        }
        result
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
        let result = self.inner.recv(timeout);
        match &result {
            Ok(Some(TransportEvent::Notification { channel, data })) => {
                lock(&self.log).rx(*channel, data, describe_received(*channel, data));
            }
            Ok(Some(TransportEvent::Disconnected)) => {
                self.closed = true;
                lock(&self.log).event("disconnected");
            }
            Ok(None) => {}
            Err(error) => lock(&self.log).event(format!("receive failed: {error}")),
        }
        result
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        if !self.closed {
            self.closed = true;
            lock(&self.log).event("disconnect");
        }
        self.inner.disconnect()
    }
}

impl Drop for LoggingTransport {
    fn drop(&mut self) {
        // Dropping the transport closes the link: say so, so that connections stay delimited
        // in the log (replay relies on it).
        if !self.closed {
            lock(&self.log).event("disconnect");
        }
    }
}
