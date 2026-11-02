//! Unit tests of the tablet service without a tablet (the simulator tests in
//! `crates/ardoise-sim/tests/service.rs` cover registration, sync and live mode).

use std::sync::mpsc;

use super::*;
use crate::testutil::ScriptConnector;
use crate::transport::TransportError;

/// A connector factory over a scripted tablet that is never found in registration mode.
struct Scripted;

impl ConnectorFactory for Scripted {
    fn connector(&mut self) -> Result<Box<dyn Connector>, TransportError> {
        Ok(Box::new(ScriptConnector::new(vec![], false)))
    }
}

/// An adapter that is switched off.
struct Off;

impl ConnectorFactory for Off {
    fn check(&mut self) -> Result<(), StatusError> {
        Err(StatusError::from(&DeviceError::Transport(
            TransportError::AdapterOff,
        )))
    }

    fn connector(&mut self) -> Result<Box<dyn Connector>, TransportError> {
        Err(TransportError::AdapterOff)
    }
}

fn start(
    library: Option<PathBuf>,
    factory: Box<dyn ConnectorFactory>,
) -> (TabletService, mpsc::Receiver<TabletStatus>) {
    let (sender, receiver) = mpsc::channel();
    let sender = std::sync::Mutex::new(sender);
    let config = ServiceConfig {
        identity_root: library.clone(),
        library,
        options: TabletOptions::default(),
        simulated: true,
    };
    let service = TabletService::start(
        config,
        factory,
        Arc::new(NoHooks),
        Box::new(move |status| {
            let _ = sender
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send(status.clone());
        }),
        Box::new(|_, _, _| Err("no store in this test".into())),
    );
    (service, receiver)
}

fn wait_for(
    receiver: &mpsc::Receiver<TabletStatus>,
    what: impl Fn(&TabletStatus) -> bool,
) -> TabletStatus {
    loop {
        let status = receiver
            .recv_timeout(Duration::from_secs(10))
            .expect("status in time");
        if what(&status) {
            return status;
        }
    }
}

#[test]
fn without_library_the_tablet_is_unavailable() {
    let (service, statuses) = start(None, Box::new(Scripted));
    let status = wait_for(&statuses, |s| s.error.is_some());
    assert_eq!(status.connection, Connection::Unavailable);
    assert_eq!(status.error.unwrap().code, "noLibrary");
    assert!(status.simulated);
    // Commands that need the tablet say why they cannot run.
    assert_eq!(service.set_time(), Err(DeviceError::NotConnected));
    assert_eq!(service.forget(), Err(DeviceError::NoLibrary));
    service.shutdown();
}

#[test]
fn a_library_without_registration_waits_for_the_wizard() {
    let library = tempfile::tempdir().unwrap();
    let (service, statuses) = start(Some(library.path().to_owned()), Box::new(Scripted));
    let status = wait_for(&statuses, |s| s.connection == Connection::NotRegistered);
    assert_eq!(status.registered, None);
    assert_eq!(
        service.rename("Classe"),
        Err(DeviceError::NoTabletRegistered)
    );
    assert_eq!(
        service.rename("Une tablette au nom trop long"),
        Err(DeviceError::InvalidName(
            "Une tablette au nom trop long".into()
        ))
    );
    // Sync without a registered tablet: an error, nothing pending.
    service.sync_now();
    let status = wait_for(&statuses, |s| s.error.is_some());
    assert_eq!(status.error.unwrap().code, "tabletNotRegistered");
    assert_eq!(service.status().pending, None);
    service.shutdown();
}

#[test]
fn a_switched_off_adapter_is_reported() {
    let library = tempfile::tempdir().unwrap();
    let (service, statuses) = start(Some(library.path().to_owned()), Box::new(Off));
    let status = wait_for(&statuses, |s| s.error.is_some());
    assert_eq!(status.connection, Connection::Unavailable);
    assert_eq!(status.error.unwrap().code, "bluetoothOff");
    service.shutdown();
}

#[test]
fn a_quick_switch_while_live_mode_waits_for_the_tablet_reaches_the_sink() {
    // The registered tablet cannot be reached: live mode waits. The sink still hears the new way
    // of holding the tablet (the interface must show the switch), with the Slate Large's area as
    // none is known.
    let library = tempfile::tempdir().unwrap();
    crate::identity::save(library.path(), &scripted_record()).unwrap();
    let (service, statuses) = start(Some(library.path().to_owned()), Box::new(Handed(None)));
    let (inputs, received) = mpsc::channel();
    service.start_live(Box::new(ChannelSink(inputs)));
    let next = || received.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(next(), LiveInput::State(LiveState::Waiting));
    let options = TabletOptions {
        orientation: Orientation::PortraitLeft,
        ..TabletOptions::default()
    };
    service.set_options(options);
    assert_eq!(
        next(),
        LiveInput::Reoriented {
            width: ardoise_proto::SLATE_LARGE_WIDTH,
            height: ardoise_proto::SLATE_LARGE_HEIGHT,
            orientation: Orientation::PortraitLeft
        }
    );
    // Another option alone says nothing.
    service.set_options(TabletOptions {
        auto_sync: false,
        ..options
    });
    wait_for(&statuses, |s| !s.options.auto_sync);
    assert!(received.try_recv().is_err());
    service.shutdown();
}

#[test]
fn options_are_followed_and_shown() {
    let library = tempfile::tempdir().unwrap();
    let (service, statuses) = start(Some(library.path().to_owned()), Box::new(Scripted));
    let options = TabletOptions {
        auto_sync: false,
        delete_after_import: false,
        orientation: Orientation::LandscapeFlipped,
        button: LiveButton::NewLayer,
    };
    service.set_options(options);
    let status = wait_for(&statuses, |s| s.options == options);
    assert_eq!(
        serde_json::to_value(status.options).unwrap(),
        serde_json::json!({
            "autoSync": false,
            "deleteAfterImport": false,
            "orientation": "landscapeFlipped",
            "button": "newLayer",
        })
    );
    service.shutdown();
}

#[test]
fn the_status_has_the_shape_of_the_interface() {
    let mut status = TabletStatus::initial(true, TabletOptions::default());
    status.connection = Connection::Syncing;
    status.register_step = Some(RegisterStep::PressButton);
    status.sync = Some(SyncProgress {
        current: 2,
        total: 3,
        saved: 1,
    });
    status.pending = Some(Pending::Live);
    status.device = Some(DeviceDetails {
        name: "Bamboo Slate".into(),
        firmware: "B160805A-C170704A".into(),
        battery_percent: 87,
        charging: false,
        width: 29_700,
        height: 21_600,
        stored_pages: 3,
        model: model_name(29_700, 21_600),
        read_at: 1,
    });
    status.error = Some(StatusError::from(&DeviceError::Busy));
    let json = serde_json::to_value(&status).unwrap();
    assert_eq!(json["connection"], "syncing");
    assert_eq!(json["registerStep"], "pressButton");
    assert_eq!(
        json["sync"],
        serde_json::json!({"current": 2, "total": 3, "saved": 1})
    );
    assert_eq!(json["pending"], "live");
    assert_eq!(json["device"]["batteryPercent"], 87);
    assert_eq!(json["device"]["storedPages"], 3);
    assert_eq!(json["device"]["model"], "Bamboo Slate A4");
    assert_eq!(json["device"]["readAt"], 1);
    assert_eq!(json["error"]["code"], "tabletBusy");
    assert_eq!(json["lastSync"], serde_json::Value::Null);
    assert_eq!(json["simulated"], true);
    assert_eq!(model_name(21_600, 14_800), "Bamboo Slate A5");
    assert_eq!(model_name(44_800, 29_600), "Wacom (44800 × 29600)");
}

#[test]
fn device_errors_map_to_translated_codes() {
    let cases = [
        (
            DeviceError::Transport(TransportError::NoAdapter),
            "bluetoothMissing",
        ),
        (
            DeviceError::Transport(TransportError::AdapterOff),
            "bluetoothOff",
        ),
        (
            DeviceError::Transport(TransportError::Timeout),
            "tabletTimeout",
        ),
        (DeviceError::Timeout("name"), "tabletTimeout"),
        (DeviceError::Disconnected, "tabletNotConnected"),
        (DeviceError::NotConnected, "tabletNotConnected"),
        (DeviceError::NotRegistered, "tabletRefused"),
        (
            DeviceError::Refused(ardoise_proto::StatusCode::InvalidState),
            "tabletRefused",
        ),
        (DeviceError::Busy, "tabletBusy"),
        (DeviceError::NoTabletRegistered, "tabletNotRegistered"),
        (DeviceError::NoLibrary, "noLibrary"),
        (DeviceError::InvalidName("é".into()), "invalidName"),
        (DeviceError::Storage("disk full".into()), "libraryAccess"),
        (DeviceError::Unexpected("?".into()), "tablet"),
        (
            DeviceError::Transport(TransportError::Backend("x".into())),
            "tablet",
        ),
    ];
    for (error, code) in cases {
        assert_eq!(error_code(&error), code, "{error:?}");
        assert_eq!(StatusError::from(&error).message, error.to_string());
    }
}

/// Hands every live input to the test.
struct ChannelSink(mpsc::Sender<LiveInput>);

impl LiveSink for ChannelSink {
    fn input(&mut self, input: LiveInput) {
        let _ = self.0.send(input);
    }
}

/// The scripted Slate of `crate::testutil`, registered with this identity.
fn scripted_record() -> crate::identity::DeviceRecord {
    crate::identity::DeviceRecord {
        format: crate::identity::DEVICE_FORMAT,
        uuid: crate::identity::HostId([0x89, 0xea, 0x78, 0x88, 0xff, 0xcb]),
        address: "E2:43:03:67:0E:01".into(),
        name: "Bamboo Slate".into(),
        protocol: "slate".into(),
        width: 29_700,
        height: 21_600,
        registered: "2026-10-24T14:00:00+02:00".into(),
        extra: Default::default(),
    }
}

type Script = Vec<(&'static str, Vec<crate::transport::TransportEvent>)>;

/// One connection of the scripted tablet.
struct OneLink(Option<Script>);

impl ConnectorFactory for OneLink {
    fn connector(&mut self) -> Result<Box<dyn Connector>, TransportError> {
        Ok(Box::new(ScriptConnector::new(
            self.0.take().into_iter().collect(),
            false,
        )))
    }
}

#[test]
fn the_interrupt_flag_stays_up_while_an_interrupting_request_waits() {
    let interrupts = Interrupts::new();
    let flag = interrupts.flag();
    assert!(!flag.load(Ordering::SeqCst));
    // Two requests sent: the worker takes the first, the second still waits.
    interrupts.raise();
    interrupts.raise();
    interrupts.taken();
    assert!(flag.load(Ordering::SeqCst), "a request still waits");
    interrupts.taken();
    assert!(!flag.load(Ordering::SeqCst));
    // A request counted while the worker takes another one keeps the flag up, whatever the
    // order of the two threads.
    interrupts.raise();
    interrupts.raise();
    interrupts.taken();
    interrupts.raise();
    interrupts.taken();
    assert!(flag.load(Ordering::SeqCst));
    interrupts.taken();
    assert!(!flag.load(Ordering::SeqCst));
    // Never below zero.
    interrupts.taken();
    interrupts.raise();
    assert!(flag.load(Ordering::SeqCst));
}

/// The scripted Slate's connection: authentication, clock, name, firmware, battery, size, page
/// count (`pages`, two hex bytes) in paper mode, `ec` before it (protocol.md §3.3, §5.3).
fn connection_script(pages: &'static str) -> Script {
    use crate::testutil::cmd;
    vec![
        ("e6 06 89 ea 78 88 ff cb", vec![cmd("b3 01 00")]),
        // 2026-10-24 12:00:00 UTC, the scripted clock.
        ("b6 06 26 10 24 12 00 00", vec![cmd("b3 01 00")]),
        ("b6 01 00", vec![cmd("bd 06 26 10 24 12 00 00")]),
        (
            "bb 01 00",
            vec![cmd("bc 0d 42 61 6d 62 6f 6f 20 53 6c 61 74 65 0a")],
        ),
        ("b7 01 00", vec![cmd("b8 09 00 42 31 36 30 38 30 35 41")]),
        ("b7 01 01", vec![cmd("b8 09 01 43 31 37 30 37 30 34 41")]),
        ("b9 01 00", vec![cmd("ba 02 57 00")]),
        ("ea 02 03 00", vec![cmd("eb 06 03 00 04 74 00 00")]),
        ("ea 02 04 00", vec![cmd("eb 06 04 00 60 54 00 00")]),
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd(pages)]),
    ]
}

/// Hands out one scripted connection whose clock the test moves.
struct Handed(Option<ScriptConnector>);

impl ConnectorFactory for Handed {
    fn connector(&mut self) -> Result<Box<dyn Connector>, TransportError> {
        match self.0.take() {
            Some(connector) => Ok(Box::new(connector)),
            None => Err(TransportError::Timeout),
        }
    }
}

#[test]
fn the_page_count_is_read_again_while_the_tablet_stays_connected() {
    use crate::testutil::cmd;
    let library = tempfile::tempdir().unwrap();
    crate::identity::save(library.path(), &scripted_record()).unwrap();
    let mut script = connection_script("c2 02 00 00");
    // 30 s later: a page was finished on the tablet; the automatic sync starts, and finds the
    // page gone (the script keeps it short). `ec` was sent on this link: not again (a real
    // Slate A4 refuses it in paper mode).
    script.extend([
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 00 00")]),
    ]);
    let connector = ScriptConnector::new(vec![script], false);
    let clock = connector.clock.clone();
    let (sender, statuses) = mpsc::channel();
    let sender = std::sync::Mutex::new(sender);
    let config = ServiceConfig {
        library: Some(library.path().to_owned()),
        identity_root: Some(library.path().to_owned()),
        options: TabletOptions::default(),
        simulated: false,
    };
    let service = TabletService::start(
        config,
        Box::new(Handed(Some(connector))),
        Arc::new(NoHooks),
        Box::new(move |status| {
            let _ = sender
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send(status.clone());
        }),
        Box::new(|_, _, _| Err("no page expected".into())),
    );
    let status = wait_for(&statuses, |s| s.connection == Connection::Connected);
    assert_eq!(status.device.unwrap().stored_pages, 0);
    assert!(status.last_sync.is_none(), "nothing to sync");
    // Connected and idle: nothing is asked before the interval...
    std::thread::sleep(Duration::from_millis(200));
    assert!(service.status().last_sync.is_none());
    // ...then the count is read again.
    clock.0.fetch_add(
        u64::try_from(worker::PAGE_COUNT_INTERVAL.as_millis()).unwrap(),
        Ordering::SeqCst,
    );
    let status = wait_for(&statuses, |s| {
        s.device.as_ref().is_some_and(|d| d.stored_pages == 1)
    });
    assert_eq!(status.connection, Connection::Connected);
    let status = wait_for(&statuses, |s| s.last_sync.is_some());
    assert_eq!(status.connection, Connection::Connected);
    assert_eq!(status.device.unwrap().stored_pages, 0);
    assert_eq!(status.error, None);
    service.shutdown();
}

#[test]
fn live_mode_forwards_oriented_pen_data_and_the_button_and_logs_the_rest() {
    use crate::testutil::{cmd, hex, on};
    use crate::transport::Channel;
    use ardoise_proto::live::LiveRecord;
    use ardoise_proto::stroke::Point;

    let library = tempfile::tempdir().unwrap();
    crate::identity::save(library.path(), &scripted_record()).unwrap();
    let script: Script = vec![
        ("e6 06 89 ea 78 88 ff cb", vec![cmd("b3 01 00")]),
        // 2026-10-24 12:00:00 UTC, the scripted clock.
        ("b6 06 26 10 24 12 00 00", vec![cmd("b3 01 00")]),
        ("b6 01 00", vec![cmd("bd 06 26 10 24 12 00 00")]),
        (
            "bb 01 00",
            vec![cmd("bc 0d 42 61 6d 62 6f 6f 20 53 6c 61 74 65 0a")],
        ),
        ("b7 01 00", vec![cmd("b8 09 00 42 31 36 30 38 30 35 41")]),
        ("b7 01 01", vec![cmd("b8 09 01 43 31 37 30 37 30 34 41")]),
        ("b9 01 00", vec![cmd("ba 02 57 00")]),
        ("ea 02 03 00", vec![cmd("eb 06 03 00 04 74 00 00")]),
        ("ea 02 04 00", vec![cmd("eb 06 04 00 60 54 00 00")]),
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 00 00")]),
        (
            "b1 01 00",
            vec![
                cmd("b3 01 00"),
                on(Channel::Live, hex("a2 06 00 00 e8 03 00 00")),
                // Seen from a real Slate A4: named, ignored.
                on(Channel::Live, hex("a3 06 dd dd dd dd 01 00")),
                on(
                    Channel::Live,
                    hex("a1 0c c4 09 f4 01 00 04 ff ff ff ff ff ff"),
                ),
                // The hypothetical button signature, the battery level and an unasked frame
                // of a real Slate A4 (ignored), then two unknown notifications.
                on(Channel::SysEvent, hex("f2 01 01")),
                on(Channel::SysEvent, hex("f1 02 2a 00")),
                cmd("cb 01 00"),
                on(Channel::SysEvent, hex("f1 02 99 00")),
                on(Channel::Live, hex("10 02 2a 00")),
            ],
        ),
        ("b1 01 02", vec![cmd("b3 01 00")]),
    ];
    let (sender, statuses) = mpsc::channel();
    let sender = std::sync::Mutex::new(sender);
    let config = ServiceConfig {
        library: Some(library.path().to_owned()),
        identity_root: Some(library.path().to_owned()),
        options: TabletOptions {
            orientation: Orientation::LandscapeFlipped,
            ..TabletOptions::default()
        },
        simulated: false,
    };
    let service = TabletService::start(
        config,
        Box::new(OneLink(Some(script))),
        Arc::new(NoHooks),
        Box::new(move |status| {
            let _ = sender
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send(status.clone());
        }),
        Box::new(|_, _, _| Err("no page expected".into())),
    );
    let status = wait_for(&statuses, |s| s.connection == Connection::Connected);
    let device = status.device.unwrap();
    assert_eq!(device.name, "Bamboo Slate");
    assert_eq!(device.firmware, "B160805A-C170704A");
    assert_eq!(device.model, "Bamboo Slate A4");
    assert_eq!(device.stored_pages, 0);
    assert_eq!(status.registered.unwrap().address, "E2:43:03:67:0E:01");

    let (inputs, received) = mpsc::channel();
    service.start_live(Box::new(ChannelSink(inputs)));
    let next = || received.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(
        next(),
        LiveInput::Started {
            width: 29_700,
            height: 21_600,
            orientation: Orientation::LandscapeFlipped
        }
    );
    assert_eq!(next(), LiveInput::State(LiveState::Live));
    assert!(matches!(next(), LiveInput::ProximityIn { .. }));
    let LiveInput::Records {
        records,
        received: stamp,
    } = next()
    else {
        panic!("records expected");
    };
    assert!(stamp <= SystemTime::now());
    // Left orientation: (2500, 500) on a 29 700 × 21 600 tablet becomes (27 200, 21 100).
    assert_eq!(
        records,
        [
            LiveRecord::Pen(Point {
                x: 27_200,
                y: 21_100,
                pressure: 1024
            }),
            LiveRecord::ProximityOut
        ]
    );
    assert_eq!(next(), LiveInput::Button);
    wait_for(&statuses, |s| s.connection == Connection::Live);
    // The battery level announced during live mode is shown (it was 87 % at the connection).
    wait_for(&statuses, |s| {
        s.device.as_ref().is_some_and(|d| d.battery_percent == 42)
    });
    service.stop_live();
    assert_eq!(next(), LiveInput::State(LiveState::Stopped));
    wait_for(&statuses, |s| s.connection == Connection::Connected);
    service.shutdown();

    // The live session saw notifications it could not classify: its log is in the library.
    let dir = library.path().join("_ardoise").join("diagnostics");
    let logs: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().collect();
    assert_eq!(logs.len(), 1, "one log for the live session");
    let text = std::fs::read_to_string(logs[0].path()).unwrap();
    assert!(
        text.contains("unclassified during live mode: sys_event f1 02 99 00"),
        "{text}"
    );
    assert!(
        text.contains("unclassified during live mode: live 10 02 2a 00"),
        "{text}"
    );
    assert!(
        text.contains("button pressed (sysevent f2 01 01)"),
        "{text}"
    );
    assert!(!text.contains("unclassified during live mode: sys_event f2 01 01"));
    // Known frames are described in their `rx` lines, and not flagged.
    for known in ["f1 02 2a 00", "a3 06 dd dd dd dd 01 00", "cb 01 00"] {
        assert!(
            !text.contains(&format!("unclassified during live mode: {known}"))
                && !text.contains(&format!(": sys_event {known}"))
                && !text.contains(&format!(": live {known}"))
                && !text.contains(&format!(": command {known}")),
            "{known} flagged: {text}"
        );
    }
    assert!(text.contains("BATTERY_LEVEL 42% (flag 00)"), "{text}");
    assert!(text.contains("meaning unknown (ignored)"), "{text}");
    let (header, entries) = crate::log::parse_log(&text).unwrap();
    assert_eq!(header.transport, "ble");
    // The log ends with the disconnection, then why.
    let last: Vec<&str> = entries
        .iter()
        .rev()
        .take(2)
        .map(|e| e.info.as_str())
        .collect();
    assert_eq!(
        last,
        ["end of this log: the tablet service stopped", "disconnect"],
        "{text}"
    );
}
