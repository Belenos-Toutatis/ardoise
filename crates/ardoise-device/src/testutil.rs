//! Test helpers: a scripted tablet that checks every host write byte for byte, and a clock
//! that only moves when a wait times out.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::transport::{Advertisement, Channel, Clock, Transport, TransportError, TransportEvent};

/// A clock that only moves when a wait times out.
pub(crate) struct TestClock(pub(crate) AtomicU64);

impl Clock for TestClock {
    fn monotonic(&self) -> Duration {
        Duration::from_millis(self.0.load(Ordering::SeqCst))
    }
    fn now(&self) -> SystemTime {
        // 2026-10-24 12:00:00 UTC.
        UNIX_EPOCH + Duration::from_secs(1_792_843_200) + self.monotonic()
    }
}

pub(crate) fn hex(text: &str) -> Vec<u8> {
    text.split_whitespace()
        .map(|b| u8::from_str_radix(b, 16).unwrap())
        .collect()
}

pub(crate) fn cmd(text: &str) -> TransportEvent {
    TransportEvent::Notification {
        channel: Channel::Command,
        data: hex(text),
    }
}

pub(crate) fn on(channel: Channel, data: Vec<u8>) -> TransportEvent {
    TransportEvent::Notification { channel, data }
}

/// Plays the tablet: each host write must equal the next expected frame, which releases the
/// scripted answers.
pub(crate) struct Script {
    peer: Advertisement,
    clock: Arc<TestClock>,
    steps: VecDeque<(Vec<u8>, Vec<TransportEvent>)>,
    pending: VecDeque<TransportEvent>,
    writes: Arc<Mutex<Vec<Vec<u8>>>>,
}

impl Transport for Script {
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
        self.clock.0.fetch_add(ms, Ordering::SeqCst);
        Ok(None)
    }
    fn disconnect(&mut self) -> Result<(), TransportError> {
        Ok(())
    }
}

pub(crate) struct Harness {
    pub(crate) clock: Arc<TestClock>,
    pub(crate) writes: Arc<Mutex<Vec<Vec<u8>>>>,
}

pub(crate) fn script(steps: Vec<(&str, Vec<TransportEvent>)>) -> (Box<dyn Transport>, Harness) {
    script_on(Arc::new(TestClock(AtomicU64::new(0))), steps)
}

/// A scripted transport using `clock`.
pub(crate) fn script_on(
    clock: Arc<TestClock>,
    steps: Vec<(&str, Vec<TransportEvent>)>,
) -> (Box<dyn Transport>, Harness) {
    let writes = Arc::new(Mutex::new(Vec::new()));
    let transport = Script {
        peer: Advertisement {
            address: "E2:43:03:67:0E:01".into(),
            name: Some("Bamboo Slate".into()),
            rssi: None,
            company_id: Some(0x4755),
            manufacturer_data: vec![0; 7],
        },
        clock: clock.clone(),
        steps: steps.into_iter().map(|(w, a)| (hex(w), a)).collect(),
        pending: VecDeque::new(),
        writes: writes.clone(),
    };
    (Box::new(transport), Harness { clock, writes })
}

/// Hands out scripted transports, one per connection, and always finds the scripted tablet.
pub(crate) struct ScriptConnector {
    pub(crate) clock: Arc<TestClock>,
    pub(crate) tablet: Advertisement,
    pub(crate) links: VecDeque<Box<dyn Transport>>,
}

impl ScriptConnector {
    /// One connection per script; all share the first script's clock.
    pub(crate) fn new(scripts: Vec<Vec<(&str, Vec<TransportEvent>)>>, register_mode: bool) -> Self {
        let clock = Arc::new(TestClock(AtomicU64::new(0)));
        let links = scripts
            .into_iter()
            .map(|steps| {
                let (transport, _) = script_on(clock.clone(), steps);
                transport
            })
            .collect();
        let tablet = Advertisement {
            address: "E2:43:03:67:0E:01".into(),
            name: Some("Bamboo Slate".into()),
            rssi: Some(-50),
            company_id: Some(0x4755),
            manufacturer_data: vec![0; if register_mode { 4 } else { 7 }],
        };
        Self {
            clock,
            tablet,
            links,
        }
    }
}

impl crate::transport::Connector for ScriptConnector {
    fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    fn find(
        &mut self,
        timeout: Duration,
        filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError> {
        if filter(&self.tablet) {
            return Ok(Some(self.tablet.clone()));
        }
        // Nothing else will show up: the whole scan elapses.
        let ms = u64::try_from(timeout.as_millis()).unwrap_or(u64::MAX);
        self.clock.0.fetch_add(ms, Ordering::SeqCst);
        Ok(None)
    }

    fn connect(
        &mut self,
        _address: &str,
        _timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        self.links.pop_front().ok_or(TransportError::Timeout)
    }
}
