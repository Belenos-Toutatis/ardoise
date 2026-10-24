//! Transport-level conformance of the simulated Slate: request frames go in, every reply is
//! checked byte for byte against protocol.md §2–§7, with the timing on the virtual clock.
//!
//! `cargo test -p ardoise-sim --test protocol -- --nocapture` prints each exchange.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod common;

use std::time::Duration;

use ardoise_device::transport::{
    Advertisement, Channel, Connector, Transport, TransportError, TransportEvent,
};
use ardoise_proto::live::{self, LivePacket, LiveRecord};
use ardoise_proto::stroke::{self, Point};
use ardoise_proto::{BcdTime, Mode, crc32};
use ardoise_sim::{CONNECT_SYSEVENT, PenScript, SimConfig, SimTablet, drawings};
use common::{bytes, hex, points};

const ID: [u8; 6] = [0x89, 0xea, 0x78, 0x88, 0xff, 0xcb];
const OTHER_ID: [u8; 6] = [0xa0, 0xbd, 0x39, 0x11, 0x79, 0x74];
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// One link to the simulated tablet, with byte-level helpers.
struct Link {
    tablet: SimTablet,
    transport: Box<dyn Transport>,
}

impl Link {
    /// Connects and checks the sysevent the tablet sends first.
    fn open(tablet: &SimTablet) -> Self {
        let address = tablet.config().address;
        let transport = tablet
            .connector()
            .connect(&address, CONNECT_TIMEOUT)
            .unwrap();
        let mut link = Self {
            tablet: tablet.clone(),
            transport,
        };
        link.expect(Channel::SysEvent, &hex(&CONNECT_SYSEVENT));
        link
    }

    /// Opens a link and authenticates with `ID` (registered beforehand).
    fn authenticated(tablet: &SimTablet) -> Self {
        let mut link = Self::open(tablet);
        link.ask("e6 06 89 ea 78 88 ff cb", "b3 01 00");
        link
    }

    fn now_ms(&self) -> u128 {
        self.tablet.clock().monotonic().as_millis()
    }

    fn send(&mut self, request: &str) {
        say!("  TX {request}");
        self.transport.write(&bytes(request)).unwrap();
    }

    /// The next event is due now and is exactly `reply` on `channel`.
    fn expect(&mut self, channel: Channel, reply: &str) {
        let before = self.now_ms();
        let event = self.transport.recv(Duration::ZERO).unwrap();
        assert_eq!(
            event,
            Some(TransportEvent::Notification {
                channel,
                data: bytes(reply)
            }),
            "expected {channel:?} {reply}"
        );
        assert_eq!(self.now_ms(), before, "replies are due at the request time");
        say!("  RX {channel:?} {reply}");
    }

    /// Nothing is pending now.
    fn silent(&mut self) {
        let before = self.now_ms();
        assert_eq!(self.transport.recv(Duration::ZERO).unwrap(), None);
        assert_eq!(self.now_ms(), before);
    }

    /// Sends `request`, checks that the only answer is the command reply `reply`.
    fn ask(&mut self, request: &str, reply: &str) {
        self.send(request);
        self.expect(Channel::Command, reply);
        self.silent();
    }

    /// Next event within `timeout` (virtual), with its time in ms.
    fn next(&mut self, timeout: Duration) -> (u128, Option<TransportEvent>) {
        let event = self.transport.recv(timeout).unwrap();
        (self.now_ms(), event)
    }
}

/// A tablet with `ID` registered through the button.
fn registered_tablet() -> SimTablet {
    let tablet = SimTablet::slate_large();
    let mut link = Link::open(&tablet);
    tablet.hold_button();
    link.send("e7 06 89 ea 78 88 ff cb");
    tablet.press_button();
    link.expect(Channel::Command, "e4 01 00");
    link.transport.disconnect().unwrap();
    tablet
}

fn bcd(unix: i64) -> String {
    hex(&BcdTime::from_unix(unix).unwrap().to_bcd())
}

fn find(tablet: &SimTablet, register_mode: bool) -> Option<Advertisement> {
    tablet
        .connector()
        .find(Duration::from_secs(2), &mut |ad: &Advertisement| {
            ad.is_wacom() && (!register_mode || ad.in_register_mode())
        })
        .unwrap()
}

#[test]
fn find_honours_the_registration_filter() {
    let _report = common::report("find_honours_the_registration_filter");
    let tablet = SimTablet::slate_large();
    let clock = tablet.clock();
    say!("scan for a tablet in registration mode before the button is held");
    assert_eq!(find(&tablet, true), None);
    assert_eq!(
        clock.monotonic(),
        Duration::from_secs(2),
        "the scan timed out"
    );

    let normal = find(&tablet, false).unwrap();
    say!("normal scan: {normal:?}");
    assert_eq!(normal.name.as_deref(), Some("Bamboo Slate"));
    assert_eq!(normal.company_id, Some(0x4755));
    assert_eq!(normal.manufacturer_data.len(), 7);
    assert_eq!(clock.monotonic(), Duration::from_secs(2), "found at once");

    tablet.hold_button();
    let register = find(&tablet, true).unwrap();
    say!("after hold_button: {register:?}");
    assert!(register.in_register_mode());
    assert_eq!(register.manufacturer_data.len(), 4);

    let _link = Link::open(&tablet);
    assert_eq!(
        find(&tablet, false),
        None,
        "not advertising while connected"
    );
}

#[test]
fn one_connection_at_a_time() {
    let _report = common::report("one_connection_at_a_time");
    let tablet = SimTablet::slate_large();
    let clock = tablet.clock();
    let address = tablet.config().address;
    let mut connector = tablet.connector();

    assert!(matches!(
        connector.connect("00:11:22:33:44:55", CONNECT_TIMEOUT),
        Err(TransportError::NotFound(_))
    ));
    let mut first = Link::open(&tablet);
    say!("second connection while the first is open: times out");
    assert!(matches!(
        connector.connect(&address, CONNECT_TIMEOUT),
        Err(TransportError::Timeout)
    ));
    assert_eq!(clock.monotonic(), CONNECT_TIMEOUT);
    assert!(tablet.is_connected());

    first.transport.disconnect().unwrap();
    assert!(!tablet.is_connected());
    assert!(matches!(
        first.transport.write(&bytes("b9 01 00")),
        Err(TransportError::NotConnected)
    ));
    say!("after disconnect: a new connection works");
    let second = Link::open(&tablet);
    drop(second);
    say!("a dropped transport frees the tablet too");
    let _third = Link::open(&tablet);
    assert_eq!(tablet.stats().connections, 3);
}

#[test]
fn refuses_commands_before_authentication() {
    let _report = common::report("refuses_commands_before_authentication");
    let tablet = SimTablet::slate_large();
    let mut link = Link::open(&tablet);
    say!("assumption: known commands before authentication get b3 01 02");
    link.ask("b9 01 00", "b3 01 02");
    link.ask("bb 01 00", "b3 01 02");
    link.ask("c1 01 00", "b3 01 02");
    link.ask("b1 01 00", "b3 01 02");
    say!("e7 outside registration mode: general error");
    link.ask("e7 06 89 ea 78 88 ff cb", "b3 01 01");
    say!("unknown opcode: command not supported (assumption: even before authentication)");
    link.ask("f0 01 00", "b3 01 04");
    say!("malformed known opcode: general error (assumption)");
    link.ask("b9 02 00 00", "b3 01 01");
    link.ask("b9 01", "b3 01 01");
    say!("e6 when no identity is stored: authorization error");
    link.ask("e6 06 89 ea 78 88 ff cb", "b3 01 07");
    link.ask("b9 01 00", "b3 01 02");
    assert_eq!(tablet.registered_id(), None);
    assert_eq!(tablet.stats().requests, 10);
}

#[test]
fn registers_with_the_button_then_answers_every_command() {
    let _report = common::report("registers_with_the_button_then_answers_every_command");
    let tablet = SimTablet::slate_large();
    let clock = tablet.clock();
    let mut link = Link::open(&tablet);

    say!("registration: hold the button, e7, no reply until the button is pressed");
    tablet.hold_button();
    link.send("e7 06 89 ea 78 88 ff cb");
    let (at, event) = link.next(Duration::from_secs(1));
    assert_eq!(event, None);
    assert_eq!(at, 1000, "the wait elapsed on the virtual clock");
    assert_eq!(tablet.registered_id(), None);
    tablet.press_button();
    link.expect(Channel::Command, "e4 01 00");
    assert_eq!(tablet.registered_id(), Some(ID));
    assert!(!tablet.in_register_mode());

    say!("clock: tablet time follows the virtual clock (2026-09-01 08:00:01 UTC)");
    link.ask("b6 01 00", "bd 06 26 09 01 08 00 01");
    link.ask("b6 06 26 10 24 12 00 00", "b3 01 00");
    link.ask("b6 01 00", "bd 06 26 10 24 12 00 00");
    let (_, event) = link.next(Duration::from_secs(61));
    assert_eq!(event, None);
    link.ask("b6 01 00", "bd 06 26 10 24 12 01 01");
    assert_eq!(
        tablet.tablet_time(),
        BcdTime::from_bcd(bytes("26 10 24 12 01 01").try_into().unwrap())
            .unwrap()
            .to_unix()
    );
    link.ask("b6 06 26 13 01 00 00 00", "b3 01 01");

    say!("name, in one chunk ending with 0a");
    link.ask("bb 01 00", "bc 0d 42 61 6d 62 6f 6f 20 53 6c 61 74 65 0a");
    link.ask("bb 08 41 72 64 6f 69 73 65 0a", "b3 01 00");
    link.ask("bb 01 00", "bc 08 41 72 64 6f 69 73 65 0a");
    assert_eq!(tablet.config().name, "Ardoise");

    say!("firmware, battery, dimensions");
    link.ask("b7 01 00", "b8 09 00 42 31 36 30 38 30 35 41");
    link.ask("b7 01 01", "b8 09 01 43 31 37 30 37 30 34 41");
    link.ask("b9 01 00", "ba 02 57 00");
    tablet.set_battery(100, true);
    link.ask("b9 01 00", "ba 02 64 01");
    link.ask("ea 02 03 00", "eb 06 03 00 04 74 00 00");
    link.ask("ea 02 04 00", "eb 06 04 00 60 54 00 00");

    say!("transfer reporting type and modes");
    link.ask("ec 06 06 00 00 00 00 00", "b3 01 00");
    link.ask("b1 01 02", "b3 01 00");
    assert_eq!(tablet.mode(), Mode::Idle);
    link.ask("b1 01 01", "b3 01 00");
    assert_eq!(tablet.mode(), Mode::Paper);

    say!("no stored page: count 0; file info, download and delete refused (assumption)");
    link.ask("c1 01 00", "c2 02 00 00");
    link.ask("cc 01 00", "b3 01 02");
    link.ask("c3 01 00", "b3 01 02");
    link.ask("ca 01 00", "b3 01 02");
    assert_eq!(clock.monotonic(), Duration::from_secs(62));
}

#[test]
fn a_wrong_identity_is_refused_on_every_connection() {
    let _report = common::report("a_wrong_identity_is_refused_on_every_connection");
    let tablet = registered_tablet();
    let mut link = Link::open(&tablet);
    say!("e6 with another identity");
    link.ask("e6 06 a0 bd 39 11 79 74", "b3 01 07");
    link.ask("b9 01 00", "b3 01 02");
    say!("e6 with the registered identity");
    link.ask("e6 06 89 ea 78 88 ff cb", "b3 01 00");
    link.ask("b9 01 00", "ba 02 57 00");
    link.transport.disconnect().unwrap();

    say!("another application registers the tablet: our identity is refused");
    tablet.set_registered_id(Some(OTHER_ID));
    let mut link = Link::open(&tablet);
    link.ask("e6 06 89 ea 78 88 ff cb", "b3 01 07");
    link.ask("e6 06 a0 bd 39 11 79 74", "b3 01 00");
}

#[test]
fn downloads_a_page_in_20_byte_chunks_with_its_crc_then_deletes_it() {
    let _report = common::report("downloads_a_page_in_20_byte_chunks_with_its_crc_then_deletes_it");
    let tablet = registered_tablet();
    let drawn = drawings::square_and_circle(29_700, 21_600);
    let stored_at = tablet.tablet_time();
    tablet.draw_page(&drawn);
    let page = tablet.stored_pages().remove(0);
    assert_eq!(page.timestamp, stored_at);
    let size = u32::try_from(page.bytes.len()).unwrap();
    say!(
        "page: {} strokes, {} bytes, CRC {:08x}",
        page.strokes.len(),
        size,
        crc32(&page.bytes)
    );

    let mut link = Link::authenticated(&tablet);
    link.ask("ec 06 06 00 00 00 00 00", "b3 01 00");
    link.ask("b1 01 01", "b3 01 00");
    link.ask("c1 01 00", "c2 02 01 00");
    link.ask(
        "cc 01 00",
        &format!("cf 0a {} {}", hex(&size.to_le_bytes()), bcd(stored_at)),
    );
    link.send("c3 01 00");
    link.expect(Channel::Command, "c8 01 be");
    let mut received = Vec::new();
    let mut chunks = 0;
    let before = link.now_ms();
    let crc = loop {
        match link.transport.recv(Duration::ZERO).unwrap() {
            Some(TransportEvent::Notification {
                channel: Channel::PageData,
                data,
            }) => {
                assert!(data.len() <= 20, "chunk of {} bytes", data.len());
                received.extend(data);
                chunks += 1;
            }
            Some(TransportEvent::Notification {
                channel: Channel::Command,
                data,
            }) => {
                assert_eq!(&data[..3], &[0xc8, 0x05, 0xed], "{}", hex(&data));
                break u32::from_le_bytes(data[3..].try_into().unwrap());
            }
            other => panic!("unexpected {other:?}"),
        }
    };
    link.silent();
    assert_eq!(
        link.now_ms(),
        before,
        "the transfer is due at the request time"
    );
    say!(
        "  RX {chunks} page-data chunks, then c8 05 ed {}",
        hex(&crc.to_le_bytes())
    );
    assert_eq!(chunks, page.bytes.len().div_ceil(20));
    assert_eq!(received, page.bytes);
    assert_eq!(crc, crc32(&received));
    let decoded = stroke::decode(&received).unwrap();
    assert_eq!(points(&decoded.strokes), points(&drawn));
    assert_eq!(decoded.strokes, page.strokes, "stroke times included");

    say!("delete the oldest page");
    link.ask("ca 01 00", "b3 01 00");
    link.ask("c1 01 00", "c2 02 00 00");
    assert_eq!(tablet.stored_page_count(), 0);
}

#[test]
fn pages_are_served_oldest_first() {
    let _report = common::report("pages_are_served_oldest_first");
    let tablet = registered_tablet();
    let pages = drawings::reference_pages(29_700, 21_600);
    for page in &pages {
        tablet.draw_page(page);
    }
    let mut link = Link::authenticated(&tablet);
    link.ask("c1 01 00", "c2 02 03 00");
    for page in &pages {
        link.send("c3 01 00");
        let mut data = Vec::new();
        while let Some(TransportEvent::Notification { channel, data: d }) =
            link.transport.recv(Duration::ZERO).unwrap()
        {
            if channel == Channel::PageData {
                data.extend(d);
            }
        }
        assert_eq!(
            points(&stroke::decode(&data).unwrap().strokes),
            points(page)
        );
        link.ask("ca 01 00", "b3 01 00");
    }
    link.ask("c1 01 00", "c2 02 00 00");
}

#[test]
fn a_corrupted_transfer_fails_its_crc_once() {
    let _report = common::report("a_corrupted_transfer_fails_its_crc_once");
    let tablet = registered_tablet();
    tablet.draw_page(&drawings::pressure_lines(29_700, 21_600));
    tablet.corrupt_next_transfer();
    let mut link = Link::authenticated(&tablet);
    let download = |link: &mut Link| {
        link.send("c3 01 00");
        let mut data = Vec::new();
        let mut crc = None;
        while let Some(TransportEvent::Notification { channel, data: d }) =
            link.transport.recv(Duration::ZERO).unwrap()
        {
            match channel {
                Channel::PageData => data.extend(d),
                _ if d[..2] == [0xc8, 0x05] => {
                    crc = Some(u32::from_le_bytes(d[3..].try_into().unwrap()));
                }
                _ => {}
            }
        }
        (crc32(&data), crc.unwrap())
    };
    let (actual, announced) = download(&mut link);
    say!("corrupted: received CRC {actual:08x}, announced {announced:08x}");
    assert_ne!(actual, announced);
    let (actual, announced) = download(&mut link);
    say!("next transfer: received CRC {actual:08x}, announced {announced:08x}");
    assert_eq!(actual, announced);
    assert_eq!(tablet.stored_page_count(), 1, "nothing deleted");
}

#[test]
fn page_commands_are_refused_in_live_mode() {
    let _report = common::report("page_commands_are_refused_in_live_mode");
    let tablet = registered_tablet();
    tablet.draw_page(&drawings::pressure_lines(29_700, 21_600));
    let mut link = Link::authenticated(&tablet);
    link.ask("b1 01 00", "b3 01 00");
    say!("assumption: page commands in live mode get b3 01 02");
    link.ask("c1 01 00", "b3 01 02");
    link.ask("c3 01 00", "b3 01 02");
    link.ask("b1 01 02", "b3 01 00");
    link.ask("c1 01 00", "c2 02 01 00");
}

fn pt(x: u16, y: u16, pressure: u16) -> Point {
    Point { x, y, pressure }
}

fn record_hex(p: Point) -> String {
    let mut data = Vec::new();
    for value in [p.x, p.y, p.pressure] {
        data.extend(value.to_le_bytes());
    }
    hex(&data)
}

fn proximity_hex(at_ms: u128) -> String {
    let ticks = u32::try_from(at_ms / 5).unwrap();
    format!("a2 06 00 00 {}", hex(&ticks.to_le_bytes()))
}

#[test]
fn live_mode_streams_proximity_then_three_records_per_packet_5_ms_apart() {
    let _report =
        common::report("live_mode_streams_proximity_then_three_records_per_packet_5_ms_apart");
    let tablet = registered_tablet();
    let first: Vec<Point> = (0..5)
        .map(|k| pt(5000 + k, 8000 + k, 300 + 100 * k))
        .collect();
    let second = vec![pt(9000, 9000, 0), pt(9001, 9002, 0)];
    let script = PenScript::new()
        .near(first.clone())
        .away(Duration::from_millis(100))
        .near(second.clone());
    say!(
        "pen script queued before live mode: {} samples",
        script.sample_count()
    );
    tablet.draw_live(script);

    let mut link = Link::authenticated(&tablet);
    // The pen is near from the start: the a2 is due with the reply, after it.
    link.send("b1 01 00");
    link.expect(Channel::Command, "b3 01 00");
    assert_eq!(tablet.mode(), Mode::Live);
    let t0 = link.now_ms();
    let out = "ff ff ff ff ff ff";
    let expected = [
        (t0, proximity_hex(t0)),
        (
            t0 + 10,
            format!(
                "a1 12 {} {} {}",
                record_hex(first[0]),
                record_hex(first[1]),
                record_hex(first[2])
            ),
        ),
        (
            t0 + 25,
            format!(
                "a1 12 {} {} {out}",
                record_hex(first[3]),
                record_hex(first[4])
            ),
        ),
        (t0 + 130, proximity_hex(t0 + 130)),
        (
            t0 + 140,
            format!(
                "a1 12 {} {} {out}",
                record_hex(second[0]),
                record_hex(second[1])
            ),
        ),
    ];
    for (due, packet) in &expected {
        let (at, event) = link.next(Duration::from_secs(1));
        assert_eq!(
            event,
            Some(TransportEvent::Notification {
                channel: Channel::Live,
                data: bytes(packet)
            })
        );
        assert_eq!(at, *due, "{packet}");
        say!("  RX live at t0+{:>3} ms: {packet}", at - t0);
    }
    let decoded = live::decode(&bytes(&expected[2].1)).unwrap();
    assert_eq!(
        decoded,
        LivePacket::Samples(vec![
            LiveRecord::Pen(first[3]),
            LiveRecord::Pen(first[4]),
            LiveRecord::ProximityOut
        ])
    );
    let (at, event) = link.next(Duration::from_secs(1));
    assert_eq!(event, None);
    assert_eq!(at, t0 + 140 + 1000);
    link.ask("b1 01 02", "b3 01 00");
    assert_eq!(tablet.stats().live_samples, 7);
    assert_eq!(tablet.stats().proximity_in, 2);
}

#[test]
fn idle_mode_stops_the_stream() {
    let _report = common::report("idle_mode_stops_the_stream");
    let tablet = registered_tablet();
    let points: Vec<Point> = (0..100).map(|k| pt(4000 + k, 6000, 800)).collect();
    let mut link = Link::authenticated(&tablet);
    link.ask("b1 01 00", "b3 01 00");
    let t0 = link.now_ms();
    say!("pen movement queued while live mode streams starts at once");
    tablet.draw_live(PenScript::new().near(points));
    let (at, _) = link.next(Duration::from_secs(1));
    assert_eq!(at, t0, "a2");
    let (at, _) = link.next(Duration::from_secs(1));
    assert_eq!(at, t0 + 10, "first a1");
    // Wait 50 ms of virtual time without reading: the packets stay queued.
    let clock = tablet.clock();
    clock.advance(Duration::from_millis(50));
    say!("leave live mode 60 ms in: packets already sent still arrive, then the reply");
    link.send("b1 01 02");
    let mut live_packets = 0;
    loop {
        match link.transport.recv(Duration::ZERO).unwrap() {
            Some(TransportEvent::Notification {
                channel: Channel::Live,
                ..
            }) => live_packets += 1,
            Some(TransportEvent::Notification {
                channel: Channel::Command,
                data,
            }) => {
                assert_eq!(hex(&data), "b3 01 00");
                break;
            }
            other => panic!("{other:?}"),
        }
    }
    // Records at 15..=60 ms: packets due at 25, 40, 55 ms.
    assert_eq!(live_packets, 3);
    let (_, event) = link.next(Duration::from_secs(2));
    assert_eq!(event, None, "nothing after idle mode");
    assert_eq!(tablet.stats().live_samples, 12);
}

#[test]
fn the_link_drops_at_the_injected_time() {
    let _report = common::report("the_link_drops_at_the_injected_time");
    let tablet = registered_tablet();
    let points: Vec<Point> = (0..100).map(|k| pt(4000 + k, 6000, 800)).collect();
    tablet.draw_live(PenScript::new().near(points));
    let mut link = Link::authenticated(&tablet);
    link.send("b1 01 00");
    link.expect(Channel::Command, "b3 01 00");
    let t0 = link.now_ms();
    tablet.disconnect_after(Duration::from_millis(52));
    let mut last = 0;
    let lost_at = loop {
        match link.next(Duration::from_secs(1)) {
            (at, Some(TransportEvent::Notification { .. })) => {
                assert!(at < t0 + 52);
                last = at;
            }
            (at, Some(TransportEvent::Disconnected)) => break at,
            other => panic!("{other:?}"),
        }
    };
    say!(
        "last live packet at t0+{} ms, link lost at t0+{} ms",
        last - t0,
        lost_at - t0
    );
    assert_eq!(lost_at, t0 + 52);
    assert!(!tablet.is_connected());
    assert_eq!(tablet.mode(), Mode::Paper);
    assert!(matches!(
        link.transport.write(&bytes("b9 01 00")),
        Err(TransportError::NotConnected)
    ));
    assert_eq!(tablet.stats().links_lost, 1);
    say!("the tablet is free again");
    let _again = Link::authenticated(&tablet);
}

#[test]
fn an_idle_link_is_dropped_by_the_tablet() {
    let _report = common::report("an_idle_link_is_dropped_by_the_tablet");
    let config = SimConfig {
        idle_timeout: Some(Duration::from_secs(10)),
        ..SimConfig::slate_large()
    };
    let tablet = SimTablet::new(config);
    let mut link = Link::open(&tablet);
    link.ask("b9 01 00", "b3 01 02");
    let (at, event) = link.next(Duration::from_secs(4));
    assert_eq!((at, event), (4000, None));
    link.ask("b9 01 00", "b3 01 02");
    let (at, event) = link.next(Duration::from_secs(60));
    say!("idle link lost at {at} ms (10 s after the last write)");
    assert_eq!(event, Some(TransportEvent::Disconnected));
    assert_eq!(at, 14_000);
}

#[test]
fn power_off_loses_the_link_and_hides_the_tablet() {
    let _report = common::report("power_off_loses_the_link_and_hides_the_tablet");
    let tablet = registered_tablet();
    let mut link = Link::authenticated(&tablet);
    tablet.power_off();
    assert_eq!(
        link.transport.recv(Duration::from_secs(1)).unwrap(),
        Some(TransportEvent::Disconnected)
    );
    assert_eq!(find(&tablet, false), None);
    let address = tablet.config().address;
    assert!(matches!(
        tablet.connector().connect(&address, CONNECT_TIMEOUT),
        Err(TransportError::Timeout)
    ));
    tablet.power_on();
    assert!(find(&tablet, false).is_some());
    say!("switched on again: registration kept");
    let _link = Link::authenticated(&tablet);
}

#[test]
fn recv_zero_delivers_only_what_is_due_and_never_moves_the_clock() {
    let _report = common::report("recv_zero_delivers_only_what_is_due_and_never_moves_the_clock");
    let tablet = registered_tablet();
    tablet.draw_live(
        PenScript::new()
            .away(Duration::from_millis(100))
            .near(vec![pt(5000, 5000, 400)]),
    );
    let mut link = Link::authenticated(&tablet);
    link.send("b1 01 00");
    link.expect(Channel::Command, "b3 01 00");
    let t0 = link.now_ms();
    assert_eq!(link.next(Duration::ZERO), (t0, None));
    assert_eq!(link.next(Duration::from_millis(60)), (t0 + 60, None));
    assert_eq!(link.next(Duration::ZERO), (t0 + 60, None));
    let (at, event) = link.next(Duration::from_millis(100));
    assert_eq!(at, t0 + 100, "the a2 is due 100 ms after live mode started");
    assert!(event.is_some());
    say!("recv(0) before the pen comes near: nothing, clock unchanged; a2 delivered at t0+100 ms");

    tablet.disconnect_after(Duration::from_millis(500));
    // The a1 (sample + pen left) is due at t0+105.
    assert_eq!(link.next(Duration::ZERO), (t0 + 100, None));
    tablet.clock().advance(Duration::from_secs(1));
    say!("1 s later (the link was lost at t0+600 ms): the a1 sent before, then the loss");
    let (at, packet) = link.next(Duration::ZERO);
    assert_eq!(at, t0 + 1100);
    assert!(matches!(
        packet,
        Some(TransportEvent::Notification {
            channel: Channel::Live,
            ..
        })
    ));
    assert_eq!(
        link.next(Duration::ZERO),
        (t0 + 1100, Some(TransportEvent::Disconnected))
    );
}
