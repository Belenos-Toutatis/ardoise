//! One open link to a tablet: [`BleTransport`].
//!
//! A background task (the "link task") owns the notification stream of the peripheral and
//! forwards every notification, tagged with its [`Channel`], into a standard channel that
//! [`Transport::recv`] reads, so all channels arrive in one queue in arrival order. The same
//! task watches for the loss of the link: it wakes up on every `DeviceDisconnected` event of
//! the central and also polls the connection state, because btleplug's event stream is a
//! small broadcast buffer that drops events when it lags (a busy scan can flood it). On loss
//! it forwards the notifications already received, releases the Windows objects and then
//! queues [`TransportEvent::Disconnected`], exactly once.

use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use ardoise_device::transport::{Advertisement, Transport, TransportError, TransportEvent};
use btleplug::api::{
    BDAddr, Central as _, CentralEvent, Characteristic, Peripheral as _, ValueNotification,
    WriteType,
};
use btleplug::platform::{Adapter, Peripheral};
use futures::{FutureExt as _, Stream, StreamExt as _};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::{MissedTickBehavior, timeout};

use crate::convert::{GattLayout, layout_kind, locate_characteristics, map_error};
use crate::executor::Executor;
use crate::gatt;

/// How often the link task checks the connection state.
const LINK_POLL_INTERVAL: Duration = Duration::from_millis(200);
/// Upper bound for one write. WinRT GATT operations cannot be cancelled and may wait several
/// seconds when the tablet has just gone away.
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// Time allowed for unsubscribing from every characteristic when closing a live link.
const UNSUBSCRIBE_BUDGET: Duration = Duration::from_secs(2);
/// Time allowed for releasing the peripheral.
const DISCONNECT_TIMEOUT: Duration = Duration::from_secs(2);

type NotificationStream = Pin<Box<dyn Stream<Item = ValueNotification> + Send>>;
type CentralEventStream = Pin<Box<dyn Stream<Item = CentralEvent> + Send>>;

/// State of a link shared by its transport, its link task and the connector (which closes a
/// link still open to an address it is asked to connect to again).
pub(crate) struct Link {
    address: BDAddr,
    peripheral: Peripheral,
    /// Characteristics subscribed to, in subscription order.
    subscribed: Vec<Characteristic>,
    /// Set by the link task once the link is lost and the peripheral released.
    lost: Arc<AtomicBool>,
    control: Mutex<Control>,
}

struct Control {
    /// Closed by [`Transport::disconnect`] or `Drop`.
    closed: bool,
    stop: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl Link {
    pub(crate) fn address(&self) -> BDAddr {
        self.address
    }

    fn is_closed(&self) -> bool {
        self.control
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .closed
    }

    /// Neither closed by the host nor lost.
    pub(crate) fn is_open(&self) -> bool {
        !self.is_closed() && !self.lost.load(Ordering::SeqCst)
    }

    /// Stops the link task, unsubscribes (when still connected) and releases the peripheral.
    /// Idempotent; never fails, since closing a lost link is not an error.
    pub(crate) fn close(&self, executor: &Executor) {
        let (stop, task) = {
            let mut control = self.control.lock().unwrap_or_else(PoisonError::into_inner);
            if control.closed {
                return;
            }
            control.closed = true;
            (control.stop.take(), control.task.take())
        };
        if let Some(stop) = stop {
            let _ = stop.send(());
        }
        let peripheral = self.peripheral.clone();
        let subscribed = self.subscribed.clone();
        let lost = self.lost.clone();
        let _ = executor.run(async move {
            if let Some(task) = task {
                // Ends at once when stopped; after a loss it may be finishing its cleanup.
                let _ = task.await;
            }
            if lost.load(Ordering::SeqCst) {
                return; // Already released by the link task.
            }
            if is_connected(&peripheral).await {
                let _ = timeout(UNSUBSCRIBE_BUDGET, async {
                    for characteristic in subscribed.iter().rev() {
                        let _ = peripheral.unsubscribe(characteristic).await;
                    }
                })
                .await;
            }
            let _ = timeout(DISCONNECT_TIMEOUT, peripheral.disconnect()).await;
        });
    }
}

/// An open Bluetooth LE link to a tablet, created by [`crate::BleConnector`].
///
/// Writes go to the Nordic UART TX characteristic only, with response when the tablet
/// supports it. Dropping the transport disconnects it.
pub struct BleTransport {
    executor: Arc<Executor>,
    link: Arc<Link>,
    peer: Advertisement,
    command: Characteristic,
    write_type: WriteType,
    events: Receiver<TransportEvent>,
}

impl std::fmt::Debug for BleTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BleTransport")
            .field("peer", &self.peer)
            .field("write_type", &self.write_type)
            .field("open", &self.link.is_open())
            .finish_non_exhaustive()
    }
}

/// What a successful connection produced, before the link task starts.
struct Established {
    layout: GattLayout,
    notifications: NotificationStream,
    central_events: CentralEventStream,
}

impl BleTransport {
    /// Connects to `peripheral`, discovers its services, subscribes to every notification
    /// channel once and starts the link task. On failure the peripheral is released.
    pub(crate) fn open(
        executor: Arc<Executor>,
        adapter: Adapter,
        peripheral: Peripheral,
        peer: Advertisement,
        limit: Duration,
    ) -> Result<Self, TransportError> {
        let target = peer.clone();
        let p = peripheral.clone();
        let established = executor.run(async move {
            let outcome = match timeout(limit, establish(&p, &adapter, &target)).await {
                Ok(outcome) => outcome,
                Err(_) => Err(TransportError::Timeout),
            };
            if outcome.is_err() {
                let _ = timeout(DISCONNECT_TIMEOUT, p.disconnect()).await;
            }
            outcome
        })??;

        let Established {
            layout,
            notifications,
            central_events,
        } = established;
        let (sender, events) = mpsc::channel();
        let (stop, stop_receiver) = oneshot::channel();
        let lost = Arc::new(AtomicBool::new(false));
        let task = LinkTask {
            peripheral: peripheral.clone(),
            notifications,
            central_events,
            stop: stop_receiver,
            events: sender,
            lost: lost.clone(),
        };
        let task = match executor.spawn(task.run()) {
            Ok(task) => task,
            Err(error) => {
                let p = peripheral.clone();
                let _ = executor.run(async move {
                    let _ = timeout(DISCONNECT_TIMEOUT, p.disconnect()).await;
                });
                return Err(error);
            }
        };
        let link = Arc::new(Link {
            address: peripheral.address(),
            peripheral,
            subscribed: layout
                .subscriptions
                .iter()
                .map(|(_, c)| c.clone())
                .collect(),
            lost,
            control: Mutex::new(Control {
                closed: false,
                stop: Some(stop),
                task: Some(task),
            }),
        });
        Ok(Self {
            executor,
            link,
            peer,
            command: layout.command,
            write_type: layout.write_type,
            events,
        })
    }

    pub(crate) fn link(&self) -> &Arc<Link> {
        &self.link
    }

    /// Whether writes use write-with-response (otherwise write-without-response).
    pub fn writes_with_response(&self) -> bool {
        self.write_type == WriteType::WithResponse
    }

    /// Whether the link is still usable (not disconnected by the host, not lost).
    pub fn is_open(&self) -> bool {
        self.link.is_open()
    }
}

async fn establish(
    peripheral: &Peripheral,
    adapter: &Adapter,
    peer: &Advertisement,
) -> Result<Established, TransportError> {
    // Subscribe to the central's events before connecting so no disconnection is missed.
    let central_events = adapter.events().await.map_err(map_error)?;
    peripheral.connect().await.map_err(|error| match error {
        btleplug::Error::DeviceNotFound => TransportError::NotFound(peer.address.clone()),
        other => map_error(other),
    })?;
    peripheral.discover_services().await.map_err(map_error)?;
    // The Slate's layout, or the blackboard's (never anything of its Xiaomi service).
    let characteristics = peripheral.characteristics();
    let kind = layout_kind(peer, &characteristics);
    let mut layout = locate_characteristics(&characteristics, kind)?;
    // Take the notification stream before enabling notifications, so the first ones (the
    // sysevent sent on connect) are not lost.
    let notifications = peripheral.notifications().await.map_err(map_error)?;
    let mut subscribed = Vec::with_capacity(layout.subscriptions.len());
    for (channel, characteristic) in layout.subscriptions {
        let required = gatt::subscriptions(kind)
            .iter()
            .any(|(c, required)| *c == channel && *required);
        match peripheral.subscribe(&characteristic).await {
            Ok(()) => subscribed.push((channel, characteristic)),
            Err(error) if required => return Err(map_error(error)),
            // An optional channel (sysevent) that cannot be enabled is simply not used.
            Err(_) => {}
        }
    }
    layout.subscriptions = subscribed;
    Ok(Established {
        layout,
        notifications,
        central_events,
    })
}

async fn is_connected(peripheral: &Peripheral) -> bool {
    peripheral.is_connected().await.unwrap_or(false)
}

struct LinkTask {
    peripheral: Peripheral,
    notifications: NotificationStream,
    central_events: CentralEventStream,
    stop: oneshot::Receiver<()>,
    events: Sender<TransportEvent>,
    lost: Arc<AtomicBool>,
}

impl LinkTask {
    async fn run(mut self) {
        let id = self.peripheral.id();
        let mut poll = tokio::time::interval(LINK_POLL_INTERVAL);
        poll.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let mut central_open = true;
        loop {
            tokio::select! {
                biased;
                // Closed by the host: no `Disconnected` event.
                _ = &mut self.stop => return,
                notification = self.notifications.next() => match notification {
                    Some(notification) => self.forward(notification),
                    None => break,
                },
                event = self.central_events.next(), if central_open => match event {
                    Some(CentralEvent::DeviceDisconnected(event_id)) if event_id == id => {
                        // The event is keyed by address: confirm with this link's own state
                        // (another link to the same address may have been released).
                        if !is_connected(&self.peripheral).await {
                            break;
                        }
                    }
                    Some(_) => {}
                    None => central_open = false,
                },
                _ = poll.tick() => {
                    if !is_connected(&self.peripheral).await {
                        break;
                    }
                }
            }
        }
        // Link lost: deliver what was already received, release the Windows objects (so a
        // later connection starts clean, without stale notification handlers), then report.
        while let Some(Some(notification)) = self.notifications.next().now_or_never() {
            self.forward(notification);
        }
        let _ = timeout(DISCONNECT_TIMEOUT, self.peripheral.disconnect()).await;
        self.lost.store(true, Ordering::SeqCst);
        let _ = self.events.send(TransportEvent::Disconnected);
    }

    fn forward(&self, notification: ValueNotification) {
        if let Some(channel) = gatt::channel_for_uuid(notification.uuid) {
            let _ = self.events.send(TransportEvent::Notification {
                channel,
                data: notification.value,
            });
        }
    }
}

impl Transport for BleTransport {
    fn peer(&self) -> &Advertisement {
        &self.peer
    }

    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        if !self.link.is_open() {
            return Err(TransportError::NotConnected);
        }
        // Defensive: the layout only ever selects Nordic UART TX, never in a service that must
        // not be touched (firmware update, Xiaomi's login).
        if self.command.uuid != gatt::COMMAND_TX
            || gatt::is_forbidden_service(self.command.service_uuid)
        {
            return Err(TransportError::Backend(format!(
                "refusing to write to {}",
                gatt::describe(self.command.uuid)
            )));
        }
        let peripheral = self.link.peripheral.clone();
        let characteristic = self.command.clone();
        let write_type = self.write_type;
        let data = frame.to_vec();
        self.executor.run(async move {
            match timeout(
                WRITE_TIMEOUT,
                peripheral.write(&characteristic, &data, write_type),
            )
            .await
            {
                Ok(Ok(())) => Ok(()),
                Ok(Err(error)) => {
                    if is_connected(&peripheral).await {
                        Err(map_error(error))
                    } else {
                        Err(TransportError::NotConnected)
                    }
                }
                Err(_) => Err(TransportError::Timeout),
            }
        })?
    }

    fn recv(&mut self, timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
        if self.link.is_closed() {
            return Err(TransportError::NotConnected);
        }
        match self.events.recv_timeout(timeout) {
            Ok(event) => Ok(Some(event)),
            Err(RecvTimeoutError::Timeout) => Ok(None),
            // The link task ended after reporting the loss (or was stopped).
            Err(RecvTimeoutError::Disconnected) => Err(TransportError::NotConnected),
        }
    }

    fn disconnect(&mut self) -> Result<(), TransportError> {
        self.link.close(&self.executor);
        Ok(())
    }
}

impl Drop for BleTransport {
    fn drop(&mut self) {
        self.link.close(&self.executor);
    }
}
