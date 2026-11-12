//! A transport decorator that records every exchange in the diagnostic log, whatever the
//! underlying link (Bluetooth, simulator, replay).

use std::time::Duration;

use crate::log::{SharedLog, lock};
use crate::transport::{
    Advertisement, Channel, TabletKind, Transport, TransportError, TransportEvent,
};

/// Records every write and every received event of `inner`, with its interpretation in the
/// protocol of `kind`.
pub struct LoggingTransport {
    inner: Box<dyn Transport>,
    log: SharedLog,
    kind: TabletKind,
    /// The end of the link is already in the log.
    closed: bool,
}

impl LoggingTransport {
    /// A link to a Bamboo Slate.
    pub fn new(inner: Box<dyn Transport>, log: SharedLog) -> Self {
        Self::for_tablet(inner, log, TabletKind::Slate)
    }

    /// A link to a tablet of `kind`: its frames are described in its own protocol.
    pub fn for_tablet(inner: Box<dyn Transport>, log: SharedLog, kind: TabletKind) -> Self {
        Self {
            inner,
            log,
            kind,
            closed: false,
        }
    }
}

/// One-line interpretation of bytes received from a Bamboo Slate on `channel`.
pub fn describe_received(channel: Channel, data: &[u8]) -> String {
    describe_received_from(TabletKind::Slate, channel, data)
}

/// One-line interpretation of bytes received from a tablet of `kind` on `channel`.
pub fn describe_received_from(kind: TabletKind, channel: Channel, data: &[u8]) -> String {
    use ardoise_proto::{blackboard, describe};
    match (kind, channel) {
        (TabletKind::Blackboard, Channel::Command) => blackboard::describe_reply(data),
        (_, Channel::BoardData) => blackboard::describe_stream(data),
        (_, Channel::Command) => describe::describe_reply(data),
        (_, Channel::PageData) => describe::describe_page_data(data),
        (_, Channel::Live) => describe::describe_live(data),
        (_, Channel::SysEvent) => describe::describe_sysevent(data),
    }
}

/// One-line interpretation of a frame written to a tablet of `kind`.
pub fn describe_written(kind: TabletKind, frame: &[u8]) -> String {
    match kind {
        TabletKind::Slate => ardoise_proto::describe::describe_request(frame),
        TabletKind::Blackboard => ardoise_proto::blackboard::describe_request(frame),
    }
}

impl Transport for LoggingTransport {
    fn peer(&self) -> &Advertisement {
        self.inner.peer()
    }

    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        lock(&self.log).tx(frame, describe_written(self.kind, frame));
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
                let info = describe_received_from(self.kind, *channel, data);
                lock(&self.log).rx(*channel, data, info);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_are_described_in_the_protocol_of_their_tablet() {
        let battery = [0x11, 0x02, 0x01, 0x5a];
        assert_eq!(
            describe_received_from(TabletKind::Blackboard, Channel::Command, &battery),
            "BOARD BATTERY 90%"
        );
        assert!(
            describe_received(Channel::Command, &battery).contains("unknown"),
            "a Slate does not know it"
        );
        assert_eq!(
            describe_written(TabletKind::Blackboard, &[0x12, 0x01, 0x04]),
            "BOARD PAGE_COUNT"
        );
        assert!(
            describe_received_from(TabletKind::Blackboard, Channel::BoardData, &[0xa0; 7])
                .starts_with("BOARD_DATA 7 bytes: 1 records")
        );
    }
}
