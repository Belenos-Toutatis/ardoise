//! The M1 milestone proof: the whole device stack (`Manager`, `Session`, the diagnostic log)
//! against the simulated Slate, from registration to live mode and reconnection.
//!
//! `cargo test -p ardoise-sim --test scenario -- --nocapture` reads like a report.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod common;

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use ardoise_device::identity;
use ardoise_device::log::{DiagnosticLog, lock};
use ardoise_device::manager::Manager;
use ardoise_device::session::{DownloadedPage, SyncSummary};
use ardoise_proto::live::{LivePacket, LiveRecord};
use ardoise_proto::stroke;
use ardoise_sim::{PenSegment, SimTablet, drawings};
use common::points;

const WAIT: Duration = Duration::from_secs(10);
const LIVE: Duration = Duration::from_secs(30);

/// What a live session delivered.
#[derive(Debug, Default)]
struct LiveCount {
    samples: u64,
    proximity_in: u32,
    proximity_out: u32,
    x: (u16, u16),
    y: (u16, u16),
    pressure_max: u16,
    /// Samples received after this virtual time (set by the test).
    samples_after: u64,
}

impl LiveCount {
    fn new() -> Self {
        Self {
            x: (u16::MAX, 0),
            y: (u16::MAX, 0),
            ..Self::default()
        }
    }

    fn add(&mut self, packet: &LivePacket, after: bool) {
        match packet {
            LivePacket::ProximityIn { .. } => self.proximity_in += 1,
            LivePacket::Samples(records) => {
                for record in records {
                    match record {
                        LiveRecord::Pen(p) => {
                            self.samples += 1;
                            if after {
                                self.samples_after += 1;
                            }
                            self.x = (self.x.0.min(p.x), self.x.1.max(p.x));
                            self.y = (self.y.0.min(p.y), self.y.1.max(p.y));
                            self.pressure_max = self.pressure_max.max(p.pressure);
                        }
                        LiveRecord::ProximityOut => self.proximity_out += 1,
                    }
                }
            }
            other => panic!("unexpected live packet {other:?}"),
        }
    }
}

#[test]
fn full_scenario_register_sync_three_pages_delete_live_30s_disconnect() {
    let _report =
        common::report("full_scenario_register_sync_three_pages_delete_live_30s_disconnect");
    let real_start = Instant::now();
    let tablet = SimTablet::slate_large();
    let config = tablet.config();
    let clock = tablet.clock();
    let library = tempfile::tempdir().unwrap();
    let log = DiagnosticLog::create(library.path(), "sim", clock.clone())
        .unwrap()
        .shared();
    let log_path = lock(&log).path().unwrap().to_owned();
    let mut manager = Manager::new(
        Box::new(tablet.connector()),
        library.path().to_owned(),
        log.clone(),
    );
    say!(
        "== M1 scenario: simulated {} ({}) ==",
        config.name,
        config.address
    );

    // 1. Three pages on the tablet.
    let pages = drawings::reference_pages(config.width, config.height);
    for page in &pages {
        tablet.draw_page(page);
    }
    let stored = tablet.stored_pages();
    assert_eq!(stored.len(), 3);
    say!(
        "step 1  the tablet holds 3 drawn pages ({} strokes, {} points)",
        stored.iter().map(|p| p.strokes.len()).sum::<usize>(),
        stored
            .iter()
            .flat_map(|p| &p.strokes)
            .map(|s| s.points.len())
            .sum::<usize>()
    );

    // 2. Registration mode, then the scan.
    tablet.hold_button();
    let found = manager.find(true, WAIT).unwrap().expect("tablet found");
    assert!(found.in_register_mode());
    assert_eq!(found.address, config.address);
    say!(
        "step 2  hold_button -> find(register mode): {} at {} ({} bytes of manufacturer data)",
        found.name.as_deref().unwrap_or("?"),
        found.address,
        found.manufacturer_data.len()
    );

    // 3. Registration, confirmed with the button when the manager asks.
    let presses = AtomicU32::new(0);
    let (mut session, registration) = manager
        .register(&found, WAIT, &mut || {
            presses.fetch_add(1, Ordering::SeqCst);
            tablet.press_button();
        })
        .unwrap();
    assert_eq!(presses.load(Ordering::SeqCst), 1);
    let record = identity::load(library.path())
        .unwrap()
        .expect("device.json");
    assert_eq!(Some(record.uuid.0), tablet.registered_id());
    assert_eq!(record.address, config.address);
    assert_eq!(record.name, "Bamboo Slate");
    assert_eq!(record.protocol, "slate");
    assert_eq!((record.width, record.height), (29_700, 21_600));
    assert_eq!(registration.info.firmware, "B160805A-C170704A");
    assert_eq!(registration.info.battery_percent, 87);
    assert!(!registration.info.charging);
    assert_eq!(registration.clock_drift, 0);
    assert_eq!(tablet.tablet_time(), 1_788_249_600, "tablet clock set");
    say!(
        "step 3  register: button pressed once, identity {} saved in device.json and read back \
         ({}, firmware {}, battery {} %, {} x {}, clock drift {} s)",
        record.uuid,
        record.name,
        registration.info.firmware,
        registration.info.battery_percent,
        record.width,
        record.height,
        registration.clock_drift
    );

    // 4. Sync with deletion.
    let mut downloaded: Vec<DownloadedPage> = Vec::new();
    let summary = session
        .sync(true, &mut |page, index, total| {
            assert_eq!(usize::from(index), downloaded.len() + 1);
            assert_eq!(total, 3);
            downloaded.push(page);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        summary,
        SyncSummary {
            available: 3,
            downloaded: 3,
            deleted: 3
        }
    );
    for (index, ((page, drawn), stored)) in downloaded.iter().zip(&pages).zip(&stored).enumerate() {
        let decoded = stroke::decode(&page.bytes).unwrap();
        assert_eq!(
            points(&decoded.strokes),
            points(drawn),
            "page {}",
            index + 1
        );
        assert_eq!(decoded.strokes, stored.strokes);
        assert_eq!(page.bytes, stored.bytes);
        assert_eq!(page.timestamp, Some(stored.timestamp));
        assert_eq!(page.announced_size as usize, page.bytes.len());
        say!(
            "        page {}: {} bytes, CRC ok, {} strokes / {} points equal to the drawing",
            index + 1,
            page.bytes.len(),
            decoded.strokes.len(),
            decoded.point_count()
        );
    }
    say!("step 4  sync(delete): downloaded exactly 3 pages, decoded point for point");

    // 5. Deletion.
    assert_eq!(tablet.stored_page_count(), 0);
    assert_eq!(tablet.stats().downloads, 3);
    say!("step 5  the tablet now stores 0 pages");

    // 6. Live mode for 30 s of virtual time.
    let script = drawings::live_script(30, config.width, config.height);
    let expected = script.sample_count() as u64;
    let approaches = script.approach_count() as u32;
    let away: Duration = script
        .segments()
        .iter()
        .map(|segment| match segment {
            PenSegment::Away(pause) => *pause,
            PenSegment::Near(_) => Duration::ZERO,
        })
        .sum();
    // 30 s at one record per 5 ms is 6000 slots: every slot holds a sample, except the
    // "pen left" record of each approach and the slots while the pen is away.
    let away_slots = (away.as_millis() / 5) as u64;
    assert_eq!(expected, 6_000 - u64::from(approaches) - away_slots);
    tablet.draw_live(script);
    let live_start = clock.monotonic();
    let live_real = Instant::now();
    let mut count = LiveCount::new();
    let reconnections = manager
        .live(&mut session, &record, live_start + LIVE, &mut |packet| {
            count.add(packet, false);
        })
        .unwrap();
    let live_virtual = clock.monotonic() - live_start;
    let live_real = live_real.elapsed();
    assert_eq!(reconnections, 0);
    assert_eq!(count.samples, expected, "every sample of the 30 s script");
    assert!(
        (5_600..=6_000).contains(&count.samples),
        "{}",
        count.samples
    );
    assert_eq!(count.proximity_in, approaches);
    assert_eq!(count.proximity_out, approaches);
    assert_eq!(tablet.stats().live_samples, expected);
    assert!(count.x.0 >= 2_500 && u32::from(count.x.1) <= config.width - 1_000);
    assert!(count.y.0 >= 500 && u32::from(count.y.1) <= config.height - 500);
    assert!(count.pressure_max > 1_000 && count.pressure_max <= 2_047);
    assert_eq!(live_virtual, LIVE, "virtual clock advanced by exactly 30 s");
    assert!(live_real < Duration::from_secs(3), "{live_real:?}");
    say!(
        "step 6  live 30 s: {} samples = expected {expected} (6000 slots of 5 ms - {approaches} \
         pen-left records - {away_slots} slots with the pen away), {} approaches, x {}..{}, \
         y {}..{}, max pressure {}; virtual {:.3} s in {} ms of real time",
        count.samples,
        count.proximity_in,
        count.x.0,
        count.x.1,
        count.y.0,
        count.y.1,
        count.pressure_max,
        live_virtual.as_secs_f64(),
        live_real.as_millis()
    );

    // 7. Disconnection.
    session.disconnect().unwrap();
    assert!(!tablet.is_connected());
    say!("step 7  disconnect: the tablet is free");

    // 8. Reconnection with the saved identity.
    let saved = manager.registration().unwrap().unwrap();
    let (mut session, drift) = manager.connect(&saved, None).unwrap();
    let (battery, _) = session.battery().unwrap();
    assert_eq!(battery, 87);
    assert_eq!(drift, 0);
    session.disconnect().unwrap();
    assert_eq!(tablet.stats().connections, 2);
    say!("step 8  reconnect with the saved identity: accepted (battery {battery} %)");

    lock(&log).flush().unwrap();
    let (_, entries) = ardoise_device::log::read_log(&log_path).unwrap();
    assert!(entries.len() > 2_000);
    say!(
        "        diagnostic log: {} lines in {}",
        entries.len() + 1,
        log_path.file_name().unwrap().to_string_lossy()
    );
    say!(
        "SCENARIO OK ({:.1} s of virtual time in {} ms)",
        clock.monotonic().as_secs_f64(),
        real_start.elapsed().as_millis()
    );
}

#[test]
fn live_mode_survives_a_dropped_link_with_one_transparent_reconnection() {
    let _report =
        common::report("live_mode_survives_a_dropped_link_with_one_transparent_reconnection");
    let tablet = SimTablet::slate_large();
    let config = tablet.config();
    let clock = tablet.clock();
    let library = tempfile::tempdir().unwrap();
    let log = DiagnosticLog::in_memory("sim", clock.clone()).shared();
    let mut manager = Manager::new(
        Box::new(tablet.connector()),
        library.path().to_owned(),
        log.clone(),
    );
    tablet.hold_button();
    let found = manager.find(true, WAIT).unwrap().unwrap();
    let (mut session, _) = manager
        .register(&found, WAIT, &mut || tablet.press_button())
        .unwrap();
    let record = manager.registration().unwrap().unwrap();

    let script = drawings::live_script(30, config.width, config.height);
    let expected = script.sample_count() as u64;
    tablet.draw_live(script);
    let drop_after = Duration::from_millis(12_347);
    tablet.disconnect_after(drop_after);
    let start = clock.monotonic();
    let drop_at = start + drop_after;
    let mut count = LiveCount::new();
    let reconnections = manager
        .live(&mut session, &record, start + LIVE, &mut |packet| {
            count.add(packet, clock.monotonic() > drop_at);
        })
        .unwrap();
    let stats = tablet.stats();
    say!(
        "link dropped by the tablet at +{:.3} s; reconnections: {reconnections}; samples: {} \
         ({} after the drop), sent by the tablet: {}, script: {expected}",
        drop_after.as_secs_f64(),
        count.samples,
        count.samples_after,
        stats.live_samples
    );
    assert_eq!(reconnections, 1);
    assert_eq!(stats.links_lost, 1);
    assert_eq!(stats.connections, 2);
    assert!(count.samples_after > 3_000, "samples keep arriving");
    assert_eq!(
        count.samples, stats.live_samples,
        "nothing lost by the host"
    );
    // At most the records of the packet cut by the drop are lost.
    assert!(
        expected - count.samples <= 2,
        "{}",
        expected - count.samples
    );
    assert_eq!(clock.monotonic() - start, LIVE);
    let notes: Vec<String> = lock(&log)
        .entries()
        .iter()
        .map(|e| e.info.clone())
        .collect();
    assert!(
        notes
            .iter()
            .any(|n| n == "link lost during live mode, reconnecting")
    );
    assert_eq!(notes.iter().filter(|n| *n == "disconnected").count(), 1);
    session.disconnect().unwrap();
    say!("RECONNECTION OK");
}
