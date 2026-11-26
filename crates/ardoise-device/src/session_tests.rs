//! Unit tests of [`Session`] against a scripted tablet, byte for byte (protocol.md).

use super::*;
use crate::testutil::*;

const ID: HostId = HostId([0x89, 0xea, 0x78, 0x88, 0xff, 0xcb]);

fn connected(steps: Vec<(&str, Vec<TransportEvent>)>) -> (Session, Harness) {
    let mut all = vec![("e6 06 89 ea 78 88 ff cb", vec![cmd("b3 01 00")])];
    all.extend(steps);
    let (transport, harness) = script(all);
    let clock: Arc<dyn Clock> = harness.clock.clone();
    (Session::connect(transport, clock, ID).unwrap(), harness)
}

#[test]
fn connects_with_the_saved_identity() {
    let (session, harness) = connected(vec![]);
    assert!(session.is_connected());
    assert_eq!(harness.writes.lock().unwrap().len(), 1);
}

#[test]
fn a_refused_identity_means_registration_needed() {
    let (transport, harness) = script(vec![("e6 06 89 ea 78 88 ff cb", vec![cmd("b3 01 07")])]);
    let result = Session::connect(transport, harness.clock, ID);
    assert_eq!(result.err(), Some(DeviceError::NotRegistered));
}

#[test]
fn registers_when_the_button_is_pressed() {
    let (transport, harness) = script(vec![("e7 06 89 ea 78 88 ff cb", vec![cmd("e4 01 00")])]);
    let mut told = 0;
    let session = Session::register(
        transport,
        harness.clock,
        ID,
        Duration::from_secs(10),
        None,
        &mut || told += 1,
    );
    assert!(session.is_ok());
    assert_eq!(told, 1);
}

#[test]
fn registration_times_out_without_button_press() {
    let (transport, harness) = script(vec![("e7 06 89 ea 78 88 ff cb", vec![])]);
    let clock = harness.clock.clone();
    let result = Session::register(
        transport,
        harness.clock,
        ID,
        Duration::from_secs(10),
        None,
        &mut || {},
    );
    assert_eq!(
        result.err(),
        Some(DeviceError::Timeout("registration button"))
    );
    assert!(clock.monotonic() >= Duration::from_secs(10));
}

#[test]
fn cancellation_interrupts_the_button_wait() {
    let (transport, harness) = script(vec![("e7 06 89 ea 78 88 ff cb", vec![])]);
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = cancel.clone();
    let result = Session::register(
        transport,
        harness.clock,
        ID,
        Duration::from_secs(10),
        Some(cancel),
        &mut || flag.store(true, Ordering::SeqCst),
    );
    assert_eq!(result.err(), Some(DeviceError::Cancelled));
}

#[test]
fn reads_a_name_sent_in_chunks_among_unrelated_frames() {
    let (mut session, _) = connected(vec![(
        "bb 01 00",
        vec![
            on(Channel::SysEvent, hex("f1 02 13 00")),
            cmd("bc 07 42 61 6d 62 6f 6f 20"),
            cmd("99 01 00"),
            cmd("bc 06 53 6c 61 74 65 0a"),
        ],
    )]);
    assert_eq!(session.name().unwrap(), "Bamboo Slate");
}

#[test]
fn reads_a_name_without_final_line_feed() {
    let (mut session, _) = connected(vec![(
        "bb 01 00",
        vec![cmd(
            "bc 12 42 61 6d 62 6f 6f 20 53 6c 61 74 65 20 53 6d 61 6c 6c",
        )],
    )]);
    assert_eq!(session.name().unwrap(), "Bamboo Slate Small");
}

#[test]
fn reads_device_information() {
    let (mut session, _) = connected(vec![
        (
            "bb 01 00",
            vec![cmd("bc 0d 42 61 6d 62 6f 6f 20 53 6c 61 74 65 0a")],
        ),
        ("b7 01 00", vec![cmd("b8 09 00 42 31 36 30 38 30 35 41")]),
        ("b7 01 01", vec![cmd("b8 09 01 43 31 37 30 37 30 34 41")]),
        ("b9 01 00", vec![cmd("ba 02 57 01")]),
        ("ea 02 03 00", vec![cmd("eb 06 03 00 04 74 00 00")]),
        ("ea 02 04 00", vec![cmd("eb 06 04 00 60 54 00 00")]),
    ]);
    let info = session.info().unwrap();
    assert_eq!(
        info,
        DeviceInfo {
            name: "Bamboo Slate".into(),
            firmware: "B160805A-C170704A".into(),
            battery_percent: 87,
            charging: true,
            width: 29700,
            height: 21600,
        }
    );
}

#[test]
fn an_error_status_is_the_answer() {
    let (mut session, _) = connected(vec![("ea 02 03 00", vec![cmd("b3 01 02")])]);
    assert_eq!(
        session.dimensions().err(),
        Some(DeviceError::Refused(StatusCode::InvalidState))
    );
}

#[test]
fn sets_and_reads_the_clock() {
    // 2026-10-24 12:00:00 UTC in BCD.
    let (mut session, _) = connected(vec![
        ("b6 06 26 10 24 12 00 00", vec![cmd("b3 01 00")]),
        ("b6 01 00", vec![cmd("bd 06 26 10 24 12 00 05")]),
    ]);
    session.set_time().unwrap();
    assert_eq!(session.read_time().unwrap(), 1_792_843_205);
}

fn page(bytes: &[u8]) -> Vec<TransportEvent> {
    bytes
        .chunks(20)
        .map(|chunk| on(Channel::PageData, chunk.to_vec()))
        .collect()
}

fn end_marker(bytes: &[u8]) -> TransportEvent {
    let crc = ardoise_proto::crc32(bytes).to_le_bytes();
    TransportEvent::Notification {
        channel: Channel::Command,
        data: vec![0xc8, 0x05, 0xed, crc[0], crc[1], crc[2], crc[3]],
    }
}

#[test]
fn syncs_every_page_and_deletes_each_once_saved() {
    let first: Vec<u8> = (0..45u8).collect();
    let second: Vec<u8> = (100..130u8).collect();
    let mut download_first = vec![cmd("c8 01 be")];
    download_first.extend(page(&first));
    download_first.push(end_marker(&first));
    // For the second page, the end marker overtakes the last chunk.
    let mut download_second = vec![cmd("c8 01 be")];
    let mut chunks = page(&second);
    let last = chunks.pop().unwrap();
    download_second.extend(chunks);
    download_second.push(end_marker(&second));
    download_second.push(last);

    let (mut session, harness) = connected(vec![
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 02 00")]),
        ("cc 01 00", vec![cmd("cf 0a 2d 00 00 00 26 10 24 09 30 00")]),
        ("c3 01 00", download_first),
        ("ca 01 00", vec![cmd("b3 01 00")]),
        ("cc 01 00", vec![cmd("cf 0a 1e 00 00 00 26 10 24 09 31 00")]),
        ("c3 01 00", download_second),
        ("ca 01 00", vec![cmd("b3 01 00")]),
    ]);
    let mut received = Vec::new();
    let summary = session
        .sync(true, &mut |page, index, total| {
            received.push((page, index, total));
            Ok(())
        })
        .unwrap();
    assert_eq!(
        summary,
        SyncSummary {
            available: 2,
            downloaded: 2,
            deleted: 2
        }
    );
    assert_eq!(received[0].0.bytes, first);
    assert_eq!(received[0].0.announced_size, 45);
    // 2026-10-24 09:30:00 UTC.
    assert_eq!(received[0].0.timestamp, Some(1_792_834_200));
    assert_eq!((received[0].1, received[0].2), (1, 2));
    assert_eq!(received[1].0.bytes, second);
    assert_eq!(harness.writes.lock().unwrap().len(), 10);
}

#[test]
fn a_page_is_not_deleted_when_it_cannot_be_saved() {
    let bytes = vec![1u8, 2, 3];
    let mut download = vec![cmd("c8 01 be")];
    download.extend(page(&bytes));
    download.push(end_marker(&bytes));
    // No `ca` in the script: a delete request would fail the test.
    let (mut session, _) = connected(vec![
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 01 00")]),
        ("cc 01 00", vec![cmd("cf 0a 03 00 00 00 26 10 24 09 30 00")]),
        ("c3 01 00", download),
    ]);
    let result = session.sync(true, &mut |_, _, _| Err("disk full".into()));
    assert_eq!(result.err(), Some(DeviceError::Storage("disk full".into())));
}

#[test]
fn a_corrupted_transfer_is_retried_once_then_reported_and_nothing_is_deleted() {
    let bytes = vec![1u8, 2, 3, 4];
    let corrupted = || {
        let mut download = vec![cmd("c8 01 be")];
        download.extend(page(&bytes[..3]));
        download.push(end_marker(&bytes));
        download
    };
    let info = || vec![cmd("cf 0a 04 00 00 00 26 10 24 09 30 00")];
    // No `ca` in the script: a delete request would fail the test.
    let (mut session, _) = connected(vec![
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 01 00")]),
        ("cc 01 00", info()),
        ("c3 01 00", corrupted()),
        ("cc 01 00", info()),
        ("c3 01 00", corrupted()),
    ]);
    let result = session.sync(true, &mut |_, _, _| Ok(()));
    assert!(
        matches!(result, Err(DeviceError::CrcMismatch { .. })),
        "{result:?}"
    );
}

/// A transfer the tablet cut short, with the right CRC of what it sent, as a real tablet's CRC is
/// its own (the CRC of nothing is 0): asked once more, then refused, never a page to delete.
#[test]
fn a_page_cut_short_by_the_tablet_is_asked_again_then_refused_and_nothing_is_deleted() {
    let whole = slate_page();
    let info = || page_info(whole.len());
    let sent = |bytes: &[u8]| {
        let mut download = vec![cmd("c8 01 be")];
        download.extend(page(bytes));
        download.push(end_marker(bytes));
        download
    };
    let start = || {
        vec![
            ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
            ("b1 01 01", vec![cmd("b3 01 00")]),
            ("c1 01 00", vec![cmd("c2 02 01 00")]),
        ]
    };
    for cut in [0, 2, whole.len() - 1] {
        let cut_short = &whole[..cut];
        assert!(ardoise_proto::stroke::decode(cut_short).is_err());

        // Twice. No `ca` in the script: a delete request would fail the test.
        let mut steps = start();
        steps.extend([
            ("cc 01 00", vec![info()]),
            ("c3 01 00", sent(cut_short)),
            ("cc 01 00", vec![info()]),
            ("c3 01 00", sent(cut_short)),
        ]);
        let (mut session, _) = connected(steps);
        let mut kept = 0;
        let result = session.sync(true, &mut |_, _, _| {
            kept += 1;
            Ok(())
        });
        assert_eq!(
            result.err(),
            Some(DeviceError::PageCutShort {
                announced: u32::try_from(whole.len()).unwrap(),
                received: cut,
            })
        );
        assert_eq!(kept, 0);

        // Once: the second transfer is the page.
        let mut steps = start();
        steps.extend([
            ("cc 01 00", vec![info()]),
            ("c3 01 00", sent(cut_short)),
            ("cc 01 00", vec![info()]),
            ("c3 01 00", sent(&whole)),
            ("ca 01 00", vec![cmd("b3 01 00")]),
        ]);
        let (mut session, _) = connected(steps);
        let mut saved = Vec::new();
        let summary = session
            .sync(true, &mut |page, _, _| {
                saved.push(page.bytes);
                Ok(())
            })
            .unwrap();
        assert_eq!((summary.downloaded, summary.deleted), (1, 1));
        assert_eq!(saved, std::slice::from_ref(&whole));
    }

    // Shorter than announced but read to its end (what is missing is at most its last stroke's
    // end mark here): asked once more, as it may have been cut between two packets. The same
    // again: a page. Whole this time: the whole page is the one kept.
    let readable = &whole[..whole.len() - ardoise_proto::stroke::STROKE_END_PACKET.len()];
    assert!(ardoise_proto::stroke::decode(readable).is_ok());
    for second in [readable, &whole] {
        let mut steps = start();
        steps.extend([
            ("cc 01 00", vec![info()]),
            ("c3 01 00", sent(readable)),
            ("cc 01 00", vec![info()]),
            ("c3 01 00", sent(second)),
            ("ca 01 00", vec![cmd("b3 01 00")]),
        ]);
        let (mut session, _) = connected(steps);
        let mut saved = Vec::new();
        let summary = session
            .sync(true, &mut |page, _, _| {
                saved.push(page.bytes);
                Ok(())
            })
            .unwrap();
        assert_eq!((summary.downloaded, summary.deleted), (1, 1));
        assert_eq!(saved, [second.to_vec()]);
    }

    // As long as announced and undecodable: not a failed transfer, a page the decoder does not
    // know (what is made of it is the sink's affair).
    let mut unreadable = whole.clone();
    unreadable.truncate(readable.len());
    unreadable.extend([0x04, 0x01, 0, 0, 0, 0, 0]);
    assert_eq!(unreadable.len(), whole.len());
    assert!(ardoise_proto::stroke::decode(&unreadable).is_err());
    let mut steps = start();
    steps.extend([
        ("cc 01 00", vec![info()]),
        ("c3 01 00", sent(&unreadable)),
        ("ca 01 00", vec![cmd("b3 01 00")]),
    ]);
    let (mut session, _) = connected(steps);
    assert_eq!(
        session.sync(true, &mut |_, _, _| Ok(())).unwrap().deleted,
        1
    );
}

/// One short stroke, as the tablet stores it.
fn slate_page() -> Vec<u8> {
    use ardoise_proto::stroke::{Point, Stroke, StrokeTime, encode_slate};
    encode_slate(&[Stroke {
        time: StrokeTime::SincePowerUp { millis: 5 },
        points: (0..4u16)
            .map(|i| Point {
                x: 1000 + i * 300,
                y: 2000,
                pressure: 700,
            })
            .collect(),
        ..Stroke::default()
    }])
}

/// The `cf` reply announcing `size` bytes, stored on 2026-10-24 09:30:00 UTC.
fn page_info(size: usize) -> TransportEvent {
    let [a, b, c, d] = u32::try_from(size).unwrap().to_le_bytes();
    TransportEvent::Notification {
        channel: Channel::Command,
        data: vec![0xcf, 0x0a, a, b, c, d, 0x26, 0x10, 0x24, 0x09, 0x30, 0x00],
    }
}

/// A corrupted transfer then one cut short, in either order: one retry in all.
#[test]
fn a_transfer_is_asked_again_once_whatever_was_wrong() {
    let whole = slate_page();
    let corrupted = || {
        let mut download = vec![cmd("c8 01 be")];
        download.extend(page(&whole[..3]));
        download.push(end_marker(&whole));
        download
    };
    let cut = || {
        let mut download = vec![cmd("c8 01 be")];
        download.extend(page(&whole[..2]));
        download.push(end_marker(&whole[..2]));
        download
    };
    for (first, second, crc) in [(corrupted(), cut(), false), (cut(), corrupted(), true)] {
        let (mut session, _) = connected(vec![
            ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
            ("b1 01 01", vec![cmd("b3 01 00")]),
            ("c1 01 00", vec![cmd("c2 02 01 00")]),
            ("cc 01 00", vec![page_info(whole.len())]),
            ("c3 01 00", first),
            ("cc 01 00", vec![page_info(whole.len())]),
            ("c3 01 00", second),
        ]);
        let error = session.sync(true, &mut |_, _, _| Ok(())).unwrap_err();
        assert_eq!(
            matches!(error, DeviceError::CrcMismatch { .. }),
            crc,
            "{error:?}"
        );
        assert_eq!(
            matches!(error, DeviceError::PageCutShort { .. }),
            !crc,
            "{error:?}"
        );
    }
}

/// A page announced empty is what it says: nothing is missing, it is deleted like any other.
#[test]
fn a_page_announced_without_any_byte_is_an_empty_page() {
    let empty = vec![cmd("c8 01 be"), end_marker(&[])];
    let (mut session, _) = connected(vec![
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 01 00")]),
        ("cc 01 00", vec![cmd("cf 0a 00 00 00 00 26 10 24 09 30 00")]),
        ("c3 01 00", empty),
        ("ca 01 00", vec![cmd("b3 01 00")]),
    ]);
    let mut received = Vec::new();
    let summary = session
        .sync(true, &mut |page, _, _| {
            received.push(page.bytes);
            Ok(())
        })
        .unwrap();
    assert_eq!(summary.deleted, 1);
    assert_eq!(received, [Vec::<u8>::new()]);
}

#[test]
fn a_transfer_corrupted_once_succeeds_on_the_retry() {
    let bytes = vec![1u8, 2, 3, 4];
    let mut corrupted = vec![cmd("c8 01 be")];
    corrupted.extend(page(&bytes[..3]));
    corrupted.push(end_marker(&bytes));
    let mut intact = vec![cmd("c8 01 be")];
    intact.extend(page(&bytes));
    intact.push(end_marker(&bytes));
    let info = || vec![cmd("cf 0a 04 00 00 00 26 10 24 09 30 00")];
    let (mut session, _) = connected(vec![
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 01 00")]),
        ("cc 01 00", info()),
        ("c3 01 00", corrupted),
        ("cc 01 00", info()),
        ("c3 01 00", intact),
        ("ca 01 00", vec![cmd("b3 01 00")]),
    ]);
    let mut saved = Vec::new();
    let summary = session
        .sync(true, &mut |page, _, _| {
            saved.push(page.bytes);
            Ok(())
        })
        .unwrap();
    assert_eq!(summary.deleted, 1);
    assert_eq!(saved, vec![bytes]);
}

/// A scripted tablet whose notifications come `pace` apart, the clock moving in between: a long
/// transfer outlasts a wait for its end marker.
struct Paced {
    script: Box<dyn Transport>,
    clock: Arc<TestClock>,
    pace: Duration,
    /// The next notification of the script and when it arrives.
    next: Option<(TransportEvent, Duration)>,
}

impl Paced {
    fn set_clock(&self, at: Duration) {
        let ms = u64::try_from(at.as_millis()).unwrap();
        self.clock.0.store(ms, std::sync::atomic::Ordering::SeqCst);
    }
}

impl Transport for Paced {
    fn peer(&self) -> &Advertisement {
        self.script.peer()
    }
    fn write(&mut self, frame: &[u8]) -> Result<(), TransportError> {
        self.script.write(frame)
    }
    fn recv(&mut self, timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
        let now = self.clock.monotonic();
        if self.next.is_none() {
            // What the script holds ready (a wait of zero moves no clock).
            self.next = self
                .script
                .recv(Duration::ZERO)?
                .map(|e| (e, now + self.pace));
        }
        match self.next.take() {
            Some((event, due)) if due <= now + timeout => {
                self.set_clock(due.max(now));
                Ok(Some(event))
            }
            later => {
                self.next = later;
                self.set_clock(now + timeout);
                Ok(None)
            }
        }
    }
    fn disconnect(&mut self) -> Result<(), TransportError> {
        self.script.disconnect()
    }
}

/// A frame the decoder refuses, met during a long download, is no reason to give up while the
/// page's bytes still flow: the download waits on as after a wait with nothing at all.
#[test]
fn a_frame_that_does_not_decode_does_not_stop_a_download_whose_bytes_flow() {
    // 60 chunks of 20 bytes, 200 ms apart: 12 s, more than two waits for the end marker.
    let bytes: Vec<u8> = (0..1200u32).map(|i| (i * 7 % 251) as u8).collect();
    let mut download = vec![cmd("c8 01 be")];
    let mut chunks = page(&bytes);
    let later = chunks.split_off(10);
    download.extend(chunks);
    // Declares two bytes, holds one.
    download.push(cmd("b3 02 00"));
    download.extend(later);
    download.push(end_marker(&bytes));
    let (script, harness) = script(vec![
        ("e6 06 89 ea 78 88 ff cb", vec![cmd("b3 01 00")]),
        ("cc 01 00", vec![page_info(bytes.len())]),
        ("c3 01 00", download),
    ]);
    let paced = Paced {
        script,
        clock: harness.clock.clone(),
        pace: Duration::from_millis(200),
        next: None,
    };
    let clock: Arc<dyn Clock> = harness.clock.clone();
    let mut session = Session::connect(Box::new(paced), clock, ID).unwrap();
    let page = session.download_oldest().unwrap();
    assert_eq!(page.bytes, bytes);
    let took = harness.clock.monotonic();
    assert!(took > DATA_IDLE_TIMEOUT * 2, "{took:?}");
}

#[test]
fn without_deletion_only_the_oldest_page_is_read() {
    let bytes = vec![9u8; 5];
    let mut download = vec![cmd("c8 01 be")];
    download.extend(page(&bytes));
    download.push(end_marker(&bytes));
    let (mut session, _) = connected(vec![
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 03 00")]),
        ("cc 01 00", vec![cmd("cf 0a 05 00 00 00 26 10 24 09 30 00")]),
        ("c3 01 00", download),
    ]);
    let summary = session.sync(false, &mut |_, _, _| Ok(())).unwrap();
    assert_eq!(
        summary,
        SyncSummary {
            available: 3,
            downloaded: 1,
            deleted: 0
        }
    );
}

#[test]
fn live_mode_delivers_samples_received_before_the_acknowledgement() {
    let sample = hex("a1 06 10 27 20 4e 00 04");
    let (mut session, _) = connected(vec![
        (
            "b1 01 00",
            vec![on(Channel::Live, sample.clone()), cmd("b3 01 00")],
        ),
        ("b1 01 02", vec![cmd("b3 01 00")]),
    ]);
    session.start_live().unwrap();
    assert_eq!(
        session.next_live(Duration::from_secs(1)).unwrap(),
        Some(sample)
    );
    assert_eq!(session.next_live(Duration::from_secs(1)).unwrap(), None);
    session.stop_live().unwrap();
}

#[test]
fn live_mode_keeps_every_notification_in_order_with_its_receipt_stamp() {
    let before = SystemTime::now();
    let sample = hex("a1 06 10 27 20 4e 00 04");
    let (mut session, _) = connected(vec![
        (
            "b1 01 00",
            vec![
                cmd("b3 01 00"),
                on(Channel::SysEvent, hex("f1 02 14 01")),
                on(Channel::Live, sample.clone()),
                cmd("99 01 00"),
                on(Channel::Live, hex("10 02 2a 00")),
            ],
        ),
        ("b1 01 02", vec![cmd("b3 01 00")]),
    ]);
    session.start_live().unwrap();
    assert!(session.in_live_mode());
    let mut seen = Vec::new();
    while let Some(n) = session.next_live_event(Duration::from_millis(100)).unwrap() {
        assert!(n.received >= before);
        seen.push((n.channel, ardoise_proto::hex::encode(&n.data)));
    }
    assert_eq!(
        seen,
        [
            (Channel::SysEvent, "f1 02 14 01".to_owned()),
            (Channel::Live, "a1 06 10 27 20 4e 00 04".to_owned()),
            (Channel::Command, "99 01 00".to_owned()),
            (Channel::Live, "10 02 2a 00".to_owned()),
        ]
    );
    session.stop_live().unwrap();
    assert!(!session.in_live_mode());
}

#[test]
fn next_live_skips_what_is_not_pen_data() {
    let sample = hex("a1 06 10 27 20 4e 00 04");
    let (mut session, _) = connected(vec![(
        "b1 01 00",
        vec![
            cmd("b3 01 00"),
            on(Channel::SysEvent, hex("f1 02 13 00")),
            on(Channel::Live, sample.clone()),
        ],
    )]);
    session.start_live().unwrap();
    assert_eq!(
        session.next_live(Duration::from_secs(1)).unwrap(),
        Some(sample)
    );
}

#[test]
fn sysevents_outside_live_mode_are_not_kept() {
    let (mut session, _) = connected(vec![(
        "b9 01 00",
        vec![
            on(Channel::SysEvent, hex("f1 02 13 00")),
            cmd("ba 02 3e 00"),
        ],
    )]);
    assert_eq!(session.battery().unwrap(), (62, false));
    assert_eq!(session.next_live_event(Duration::ZERO).unwrap(), None);
}

#[test]
fn renames_the_tablet_with_a_line_feed() {
    // protocol.md §4.1: `bb n+1 <ASCII name> 0a` -> `b3 01 00`.
    let (mut session, harness) = connected(vec![(
        "bb 0b 43 6c 61 73 73 65 20 33 65 42 0a",
        vec![cmd("b3 01 00")],
    )]);
    session.set_name("Classe 3eB").unwrap();
    assert_eq!(harness.writes.lock().unwrap().len(), 2);
    for bad in ["", "Ardoise de la classe", "Tablette é", "tab\tle"] {
        assert_eq!(
            session.set_name(bad).err(),
            Some(DeviceError::InvalidName(bad.to_owned())),
            "{bad:?}"
        );
    }
    assert_eq!(harness.writes.lock().unwrap().len(), 2, "nothing sent");
    assert!(valid_name("Bamboo Slate A4"));
    assert!(!valid_name("Bamboo Slate A4!!"));
}

#[test]
fn idle_drops_stray_frames_and_notices_a_lost_link() {
    let (mut session, _) = connected(vec![(
        "b9 01 00",
        vec![
            cmd("ba 02 3e 00"),
            cmd("bc 06 53 6c 61 74 65 0a"),
            TransportEvent::Disconnected,
        ],
    )]);
    assert_eq!(session.battery().unwrap(), (62, false));
    assert_eq!(
        session.idle(Duration::from_secs(1)).err(),
        Some(DeviceError::Disconnected)
    );
    assert!(!session.is_connected());
}

#[test]
fn a_lost_link_is_reported() {
    let (mut session, _) = connected(vec![("b9 01 00", vec![TransportEvent::Disconnected])]);
    assert_eq!(session.battery().err(), Some(DeviceError::Disconnected));
    assert!(!session.is_connected());
    assert_eq!(session.battery().err(), Some(DeviceError::Disconnected));
}

#[test]
fn ignores_the_stray_chunk_after_a_name_without_line_feed() {
    // Issue #117: the whole name without `0a`, then a stray `bc 09 00 "Bamboo " 0a`.
    let (mut session, _) = connected(vec![(
        "bb 01 00",
        vec![
            cmd("bc 12 42 61 6d 62 6f 6f 20 53 6c 61 74 65 20 53 6d 61 6c 6c"),
            cmd("bc 09 00 42 61 6d 62 6f 6f 20 0a"),
        ],
    )]);
    assert_eq!(session.name().unwrap(), "Bamboo Slate Small");
}

/// A link that is already gone: every write fails like the Bluetooth transport does.
struct Gone(Advertisement);

impl Transport for Gone {
    fn peer(&self) -> &Advertisement {
        &self.0
    }
    fn write(&mut self, _frame: &[u8]) -> Result<(), TransportError> {
        Err(TransportError::NotConnected)
    }
    fn recv(&mut self, _timeout: Duration) -> Result<Option<TransportEvent>, TransportError> {
        Ok(None)
    }
    fn disconnect(&mut self) -> Result<(), TransportError> {
        Ok(())
    }
}

#[test]
fn a_link_lost_before_writing_is_a_disconnection() {
    let (_, harness) = script(vec![]);
    let peer = Advertisement {
        address: "E2:43:03:67:0E:01".into(),
        name: None,
        rssi: None,
        company_id: Some(0x4755),
        manufacturer_data: vec![0; 7],
        xiaomi_service_data: Vec::new(),
    };
    let mut session = Session::new(Box::new(Gone(peer)), harness.clock);
    assert_eq!(session.start_live().err(), Some(DeviceError::Disconnected));
    assert!(!session.is_connected());
}

/// `ec` and paper mode, as a real Slate A4 needs them (protocol.md §5.3): `ec` once per link,
/// before the first paper mode, never in paper mode.
#[test]
fn ec_comes_once_per_link_before_the_first_paper_mode() {
    let (mut session, harness) = connected(vec![
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 01 00")]),
        // The next count (and the sync after it): paper mode again, no `ec`.
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 00 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 00 00")]),
    ]);
    assert_eq!(session.stored_pages(), Ok(1));
    assert_eq!(session.stored_pages(), Ok(0));
    let summary = session.sync(true, &mut |_, _, _| Ok(())).unwrap();
    assert_eq!(summary.available, 0);
    assert_eq!(harness.writes.lock().unwrap().len(), 8);
}

#[test]
fn after_live_mode_ec_is_sent_again_from_idle_mode() {
    // The order of the only sync that worked in the first hardware session.
    let (mut session, _) = connected(vec![
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 00 00")]),
        ("b1 01 00", vec![cmd("b3 01 00")]),
        ("b1 01 02", vec![cmd("b3 01 00")]),
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 00 00")]),
        // Live mode left straight for paper mode: idle mode first, for `ec`.
        ("b1 01 00", vec![cmd("b3 01 00")]),
        ("b1 01 02", vec![cmd("b3 01 00")]),
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 00 00")]),
    ]);
    assert_eq!(session.stored_pages(), Ok(0));
    session.start_live().unwrap();
    session.stop_live().unwrap();
    assert_eq!(session.stored_pages(), Ok(0));
    session.start_live().unwrap();
    assert_eq!(session.stored_pages(), Ok(0));
}

#[test]
fn a_refused_ec_is_retried_once_from_idle_mode() {
    let (mut session, _) = connected(vec![
        // The tablet answers as in paper mode (the 2026-11-02 captures): idle mode, then again.
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 02")]),
        ("b1 01 02", vec![cmd("b3 01 00")]),
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 00")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 03 00")]),
    ]);
    assert_eq!(session.stored_pages(), Ok(3));

    // Refused twice: paper mode and the count still work (the connection goes on); the sync
    // asks again, from idle mode, and fails with the tablet's answer.
    let (mut session, harness) = connected(vec![
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 02")]),
        ("b1 01 02", vec![cmd("b3 01 00")]),
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 02")]),
        ("b1 01 01", vec![cmd("b3 01 00")]),
        ("c1 01 00", vec![cmd("c2 02 03 00")]),
        ("b1 01 02", vec![cmd("b3 01 00")]),
        ("ec 06 06 00 00 00 00 00", vec![cmd("b3 01 02")]),
    ]);
    assert_eq!(session.stored_pages(), Ok(3));
    assert_eq!(
        session.sync(true, &mut |_, _, _| Ok(())).err(),
        Some(DeviceError::Refused(StatusCode::InvalidState))
    );
    assert_eq!(harness.writes.lock().unwrap().len(), 8);
}

#[test]
fn battery_levels_announced_by_the_tablet_are_kept_for_the_caller() {
    // Our capture: `f1 02 29 00` (41 %) right after a reading of 42 %.
    let (mut session, _) = connected(vec![(
        "b9 01 00",
        vec![
            cmd("ba 02 2a 00"),
            on(Channel::SysEvent, hex("f1 02 29 00")),
        ],
    )]);
    assert_eq!(session.battery().unwrap(), (42, false));
    session.idle(Duration::ZERO).unwrap();
    assert_eq!(session.take_battery_event(), Some(41));
    assert_eq!(session.take_battery_event(), None);
}
