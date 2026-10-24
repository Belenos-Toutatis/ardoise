//! Scanning and connecting: [`BleConnector`].

use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use ardoise_device::transport::{
    Advertisement, Clock, Connector, SystemClock, Transport, TransportError,
};
use btleplug::api::{
    BDAddr, Central as _, CentralEvent, Manager as _, Peripheral as _, ScanFilter,
};
use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
use futures::StreamExt as _;
use tokio::sync::oneshot;
use tokio::time::{MissedTickBehavior, timeout};

use crate::convert::{advertisement_from_properties, check_adapter_status, map_error};
use crate::executor::Executor;
use crate::link::{BleTransport, Link};

/// Longest wait honoured; longer timeouts are clamped (avoids `Instant` overflow).
const MAX_WAIT: Duration = Duration::from_secs(24 * 3600);
/// While scanning, every known peripheral is also reported at this interval, in case
/// advertisement events were dropped by btleplug's small event buffer.
const SWEEP_INTERVAL: Duration = Duration::from_secs(1);
/// Time allowed for the scan to stop after `find` returns.
const SCAN_STOP_TIMEOUT: Duration = Duration::from_secs(2);

/// Finds tablets and opens links to them over the default Bluetooth LE adapter (btleplug,
/// WinRT backend). Blocking; a private tokio runtime runs the Bluetooth work.
pub struct BleConnector {
    executor: Arc<Executor>,
    clock: Arc<SystemClock>,
    adapter: Option<Adapter>,
    /// Links this connector opened, to close one still open before reconnecting to the
    /// same address.
    links: Vec<Weak<Link>>,
}

impl std::fmt::Debug for BleConnector {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BleConnector")
            .field("adapter_ready", &self.adapter.is_some())
            .finish_non_exhaustive()
    }
}

impl BleConnector {
    /// Starts the private runtime. The adapter is looked up on first use, so this works even
    /// when no dongle is plugged in yet.
    pub fn new() -> Result<Self, TransportError> {
        Ok(Self {
            executor: Arc::new(Executor::new()?),
            clock: Arc::new(SystemClock::new()),
            adapter: None,
            links: Vec::new(),
        })
    }

    /// `NoAdapter` / `AdapterOff` from the state of the default adapter.
    /// Runs [`crate::adapter_info`] on a blocking thread of the runtime, so the caller's
    /// thread keeps its own COM apartment. Forgets the cached adapter when it is gone or
    /// off, so an unplugged and replugged dongle is picked up again.
    fn check_adapter(&mut self) -> Result<(), TransportError> {
        let info = self
            .executor
            .run(async { tokio::task::spawn_blocking(crate::adapter_info).await })?
            .map_err(|e| TransportError::Backend(format!("adapter check failed: {e}")))?
            .map_err(|e| TransportError::Backend(e.to_string()))?;
        let status = check_adapter_status(info.status);
        if status.is_err() {
            self.adapter = None;
        }
        status
    }

    fn adapter(&mut self) -> Result<Adapter, TransportError> {
        if let Some(adapter) = &self.adapter {
            return Ok(adapter.clone());
        }
        let adapters = self
            .executor
            .run(async {
                let manager = Manager::new().await?;
                manager.adapters().await
            })?
            .map_err(map_error)?;
        let adapter = adapters
            .into_iter()
            .next()
            .ok_or(TransportError::NoAdapter)?;
        self.adapter = Some(adapter.clone());
        Ok(adapter)
    }

    /// Scans for up to `limit`, handing every advertisement received to `filter`.
    fn scan(
        &mut self,
        limit: Duration,
        filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError> {
        let deadline = Instant::now() + limit.min(MAX_WAIT);
        let adapter = self.adapter()?;
        let (found, received) = mpsc::channel();
        let (stop, stop_receiver) = oneshot::channel();
        let task = self
            .executor
            .spawn(scan_task(adapter, found, stop_receiver))?;
        let outcome = loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break Ok(None);
            }
            match received.recv_timeout(remaining) {
                Ok(Ok(advertisement)) => {
                    if filter(&advertisement) {
                        break Ok(Some(advertisement));
                    }
                }
                Ok(Err(error)) => break Err(error),
                Err(RecvTimeoutError::Timeout) => break Ok(None),
                Err(RecvTimeoutError::Disconnected) => {
                    break Err(TransportError::Backend(
                        "the scan stopped unexpectedly".into(),
                    ));
                }
            }
        };
        let _ = stop.send(());
        let _ = self.executor.run(async move {
            let _ = timeout(SCAN_STOP_TIMEOUT, task).await;
        });
        outcome
    }

    fn lookup(
        &self,
        adapter: &Adapter,
        id: &PeripheralId,
    ) -> Result<Option<Peripheral>, TransportError> {
        let adapter = adapter.clone();
        let id = id.clone();
        match self
            .executor
            .run(async move { adapter.peripheral(&id).await })?
        {
            Ok(peripheral) => Ok(Some(peripheral)),
            Err(btleplug::Error::DeviceNotFound) => Ok(None),
            Err(error) => Err(map_error(error)),
        }
    }

    /// Closes the links still open to `address`, so their notifications cannot be duplicated
    /// into a new link and their Windows objects are released; forgets the dead ones.
    fn close_links_to(&mut self, address: BDAddr) {
        let mut to_close = Vec::new();
        self.links.retain(|weak| match weak.upgrade() {
            Some(link) if link.is_open() => {
                if link.address() == address {
                    to_close.push(link);
                    false
                } else {
                    true
                }
            }
            _ => false,
        });
        for link in to_close {
            link.close(&self.executor);
        }
    }
}

impl Connector for BleConnector {
    fn clock(&self) -> Arc<dyn Clock> {
        self.clock.clone()
    }

    fn find(
        &mut self,
        timeout: Duration,
        filter: &mut dyn FnMut(&Advertisement) -> bool,
    ) -> Result<Option<Advertisement>, TransportError> {
        self.check_adapter()?;
        self.scan(timeout, filter)
    }

    fn connect(
        &mut self,
        address: &str,
        timeout: Duration,
    ) -> Result<Box<dyn Transport>, TransportError> {
        let deadline = Instant::now() + timeout.min(MAX_WAIT);
        let parsed: BDAddr = address.trim().parse().map_err(|_| {
            TransportError::Backend(format!("invalid Bluetooth address {address:?}"))
        })?;
        self.check_adapter()?;
        self.close_links_to(parsed);
        let adapter = self.adapter()?;
        let id = PeripheralId::from(parsed);

        let peripheral = match self.lookup(&adapter, &id)? {
            Some(peripheral) => peripheral,
            None => {
                // Not seen by the last scan: look for it until the deadline.
                let wanted = parsed.to_string();
                let remaining = deadline.saturating_duration_since(Instant::now());
                if self
                    .scan(remaining, &mut |ad| ad.address == wanted)?
                    .is_none()
                {
                    return Err(TransportError::NotFound(address.to_owned()));
                }
                self.lookup(&adapter, &id)?
                    .ok_or_else(|| TransportError::NotFound(address.to_owned()))?
            }
        };

        let p = peripheral.clone();
        let peer = match self.executor.run(async move { p.properties().await })? {
            Ok(Some(properties)) => advertisement_from_properties(&properties),
            _ => Advertisement {
                address: parsed.to_string(),
                name: None,
                rssi: None,
                company_id: None,
                manufacturer_data: Vec::new(),
            },
        };

        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(TransportError::Timeout);
        }
        let transport =
            BleTransport::open(self.executor.clone(), adapter, peripheral, peer, remaining)?;
        self.links.push(Arc::downgrade(transport.link()));
        Ok(Box::new(transport))
    }
}

/// Restarts the scan, then reports every advertisement received until stopped.
async fn scan_task(
    adapter: Adapter,
    found: Sender<Result<Advertisement, TransportError>>,
    mut stop: oneshot::Receiver<()>,
) {
    if let Err(error) = scan_loop(&adapter, &found, &mut stop).await {
        let _ = found.send(Err(error));
    }
    let _ = adapter.stop_scan().await;
}

async fn scan_loop(
    adapter: &Adapter,
    found: &Sender<Result<Advertisement, TransportError>>,
    stop: &mut oneshot::Receiver<()>,
) -> Result<(), TransportError> {
    let mut events = adapter.events().await.map_err(map_error)?;
    // A fresh scan with an empty peripheral list: peripherals that stopped advertising
    // (btleplug #405) are forgotten, so only devices heard during this call are reported.
    let _ = adapter.stop_scan().await;
    adapter.clear_peripherals().await.map_err(map_error)?;
    adapter
        .start_scan(ScanFilter::default())
        .await
        .map_err(map_error)?;
    let mut sweep =
        tokio::time::interval_at(tokio::time::Instant::now() + SWEEP_INTERVAL, SWEEP_INTERVAL);
    sweep.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = &mut *stop => return Ok(()),
            event = events.next() => match event {
                Some(CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id)) => {
                    if let Ok(peripheral) = adapter.peripheral(&id).await
                        && !report(&peripheral, found).await
                    {
                        return Ok(());
                    }
                }
                Some(_) => {}
                None => {
                    return Err(TransportError::Backend("the scan event stream ended".into()));
                }
            },
            _ = sweep.tick() => {
                for peripheral in adapter.peripherals().await.map_err(map_error)? {
                    if !report(&peripheral, found).await {
                        return Ok(());
                    }
                }
            }
        }
    }
}

/// Sends the peripheral's advertisement; `false` once nobody listens any more.
async fn report(
    peripheral: &Peripheral,
    found: &Sender<Result<Advertisement, TransportError>>,
) -> bool {
    match peripheral.properties().await {
        Ok(Some(properties)) => found
            .send(Ok(advertisement_from_properties(&properties)))
            .is_ok(),
        _ => true,
    }
}
