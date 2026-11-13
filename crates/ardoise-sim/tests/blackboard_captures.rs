//! Non-regression tests on the private captures of the Xiaomi blackboard study
//! (docs/research/xiaomi-blackboard.md §3, `testdata/private/xiaomi/`, ignored by git).
//!
//! The captures are the user's private data: every test below skips itself when the folder is
//! absent (cloud sessions, other PCs), and nothing taken from a capture is written in this file
//! (no address, no time, no page content): the tests compare the captures with each other and
//! with the study's own renderings, and only bare counts are printed. Failure messages too: they
//! name a capture or a page by its rank, never by its file name, and print counts only, never
//! bytes, frames, times or values (`assert!(a == b, "...")` where `assert_eq!` would).
//!
//! - Every frame of the vendor channel (`6e400002`, `6e400003`) in every capture decodes with
//!   `ardoise_proto::blackboard` and encodes back to the same bytes; none is unknown.
//! - Every live notification (`6e400004`, sessions without a download) is whole records of a
//!   known status.
//! - The bytes each download of Mi Home's import received are, byte for byte, the stored page
//!   kept from it, of the size the tablet announced.
//! - Every stored page passes the sync's record check and decodes completely (whole records,
//!   known statuses, inside the page size), into as many strokes and points as the study's renderings (`page-*.svg`) show, and
//!   becomes a note.
//! - The page stored during the captures holds the records received live, byte for byte.
//!
//! `cargo test -p ardoise-sim --test blackboard_captures -- --nocapture` prints the counts.

#![allow(clippy::unwrap_used, clippy::expect_used)]

#[macro_use]
mod common;

use std::path::{Path, PathBuf};

use ardoise_core::note::Timing;
use ardoise_core::settings::Orientation;
use ardoise_device::pages::{BLACKBOARD_PAGE, note_from_page, page_kind};
use ardoise_device::session::DownloadedPage;
use ardoise_device::transport::TabletKind;
use ardoise_proto::blackboard::{
    BOARD_HEIGHT, BOARD_WIDTH, BoardReply, BoardRequest, RecordStatus, check_page, decode_page,
    decode_records, record::RECORD_LEN,
};

const COMMAND: &str = "6e400002";
const REPLY: &str = "6e400003";
const STREAM: &str = "6e400004";

/// `testdata/private/xiaomi` of this checkout or of the main one it is a worktree of (walking up
/// from this crate), or `None` after printing why the test is skipped.
fn captures_or_skip(test: &str) -> Option<PathBuf> {
    let found = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|a| a.join("testdata").join("private").join("xiaomi"))
        .find(|candidate| candidate.is_dir());
    if found.is_none() {
        println!("{test}: SKIPPED - no private captures (testdata/private/xiaomi)");
    }
    found
}

/// One line of a capture: an ATT dump of Mi Home (`dir`, `op`) or a session of the survey tool
/// (`type`).
#[derive(Debug, Clone)]
struct Packet {
    /// The first 8 hex digits of the characteristic.
    uuid: String,
    /// Written by the host.
    tx: bool,
    bytes: Vec<u8>,
}

fn packets(path: &Path) -> Vec<Packet> {
    let text = std::fs::read_to_string(path).unwrap();
    text.lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .filter_map(|value| {
            let uuid = value.get("uuid")?.as_str()?.get(..8)?.to_owned();
            let hex = value.get("hex")?.as_str()?;
            let tx = value.get("dir").and_then(|d| d.as_str()) == Some("tx")
                || value.get("type").and_then(|t| t.as_str()) == Some("write");
            // Only values (writes and notifications), not reads of descriptors.
            let op = value.get("op").and_then(|o| o.as_str()).unwrap_or("");
            if !op.is_empty() && !matches!(op, "write-req" | "write-cmd" | "notify") {
                return None;
            }
            Some(Packet {
                uuid,
                tx,
                bytes: ardoise_proto::hex::decode(hex).ok()?,
            })
        })
        .filter(|p| [COMMAND, REPLY, STREAM].contains(&p.uuid.as_str()))
        .collect()
}

/// Every capture of the folder (`.jsonl`), by name.
fn captures(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "jsonl"))
        .collect();
    paths.sort();
    paths
}

/// The stored pages kept from the study: (time from the file name, bytes, rendering).
fn pages(dir: &Path) -> Vec<(u32, Vec<u8>, Option<String>)> {
    let pages = dir.join("pages");
    let mut found: Vec<(u32, Vec<u8>, Option<String>)> = std::fs::read_dir(&pages)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "bin"))
                .filter_map(|p| {
                    let stem = p.file_stem()?.to_str()?.strip_prefix("page-")?.to_owned();
                    let time = stem.parse().ok()?;
                    let svg = std::fs::read_to_string(p.with_extension("svg")).ok();
                    Some((time, std::fs::read(&p).ok()?, svg))
                })
                .collect()
        })
        .unwrap_or_default();
    found.sort_by_key(|(time, _, _)| *time);
    found
}

#[test]
fn every_frame_of_the_captures_decodes_and_encodes_back() {
    let _report = common::report("captures: every frame of the vendor channel");
    let Some(dir) = captures_or_skip("every_frame_of_the_captures_decodes_and_encodes_back") else {
        return;
    };
    let (mut requests, mut replies) = (0, 0);
    for (rank, path) in captures(&dir).iter().enumerate() {
        for packet in packets(path) {
            match (packet.uuid.as_str(), packet.tx) {
                (COMMAND, true) => {
                    let Ok(request) = BoardRequest::decode(&packet.bytes) else {
                        panic!("capture {rank}: a request does not decode");
                    };
                    assert!(
                        request.encode() == packet.bytes,
                        "capture {rank}: a request does not encode back"
                    );
                    requests += 1;
                }
                (REPLY, false) => {
                    let Ok(reply) = BoardReply::decode(&packet.bytes) else {
                        panic!("capture {rank}: a reply does not decode");
                    };
                    assert!(
                        !matches!(reply, BoardReply::Unknown(_)),
                        "capture {rank}: an unknown reply"
                    );
                    assert!(
                        reply.encode().ok() == Some(packet.bytes.clone()),
                        "capture {rank}: a reply does not encode back"
                    );
                    replies += 1;
                }
                _ => {}
            }
        }
    }
    say!("{requests} requests and {replies} replies decoded and encoded back");
    assert!(requests > 0 && replies > 0);
}

#[test]
fn every_live_notification_is_whole_records_of_a_known_status() {
    let _report = common::report("captures: live notifications");
    let Some(dir) = captures_or_skip("every_live_notification_is_whole_records_of_a_known_status")
    else {
        return;
    };
    let mut notifications = 0;
    let mut records = 0;
    for path in captures(&dir) {
        let packets = packets(&path);
        // A capture with a download holds page chunks on the stream, cut anywhere.
        let downloads = packets.iter().any(|p| {
            p.tx && matches!(
                BoardRequest::decode(&p.bytes),
                Ok(BoardRequest::Download { .. })
            )
        });
        if downloads {
            continue;
        }
        for packet in packets.iter().filter(|p| p.uuid == STREAM) {
            assert!(
                (RECORD_LEN..=RECORD_LEN * 6).contains(&packet.bytes.len())
                    && packet.bytes.len() % RECORD_LEN == 0,
                "{} bytes",
                packet.bytes.len()
            );
            let decoded = decode_records(&packet.bytes);
            assert_eq!(decoded.trailing, 0);
            for record in &decoded.records {
                assert!(
                    !matches!(record.status, RecordStatus::Unknown(_)),
                    "a live record of an unknown status"
                );
                assert!(u32::from(record.x) < BOARD_WIDTH && u32::from(record.y) < BOARD_HEIGHT);
                assert!(record.to_live().is_some());
            }
            records += decoded.records.len();
            notifications += 1;
        }
    }
    say!("{notifications} live notifications, {records} records");
    assert!(notifications > 0);
}

#[test]
fn the_downloads_of_the_import_are_the_stored_pages() {
    let _report = common::report("captures: downloads of Mi Home's import");
    let Some(dir) = captures_or_skip("the_downloads_of_the_import_are_the_stored_pages") else {
        return;
    };
    let pages = pages(&dir);
    let mut checked = 0;
    for path in captures(&dir) {
        let mut announced: Vec<(u32, u32)> = Vec::new();
        let mut current: Option<(u32, Vec<u8>)> = None;
        let mut downloads: Vec<(u32, Vec<u8>)> = Vec::new();
        for packet in packets(&path) {
            if packet.uuid == REPLY
                && let Ok(BoardReply::OldestPage(Some(info))) = BoardReply::decode(&packet.bytes)
            {
                announced.push((info.time, info.size));
            }
            if packet.tx {
                if let Some(done) = current.take() {
                    downloads.push(done);
                }
                if let Ok(BoardRequest::Download { time }) = BoardRequest::decode(&packet.bytes) {
                    current = Some((time, Vec::new()));
                }
            } else if packet.uuid == STREAM
                && let Some((_, bytes)) = current.as_mut()
            {
                bytes.extend_from_slice(&packet.bytes);
            }
        }
        downloads.extend(current);
        for (time, bytes) in downloads {
            let size = announced
                .iter()
                .find(|(t, _)| *t == time)
                .map(|(_, size)| *size)
                .expect("each download follows its description");
            assert_eq!(bytes.len(), size as usize, "the announced size");
            let (_, kept, _) = pages
                .iter()
                .find(|(t, _, _)| *t == time)
                .expect("each page downloaded was kept");
            assert!(
                &bytes == kept,
                "download {checked}: not the stored page, byte for byte"
            );
            checked += 1;
        }
    }
    say!("{checked} downloads match the stored pages");
    assert_eq!(
        checked,
        pages.len(),
        "every page kept comes from a download"
    );
}

#[test]
fn every_stored_page_decodes_completely_and_becomes_a_note() {
    let _report = common::report("captures: the stored pages");
    let Some(dir) = captures_or_skip("every_stored_page_decodes_completely_and_becomes_a_note")
    else {
        return;
    };
    let pages = pages(&dir);
    assert!(
        !pages.is_empty(),
        "no page in testdata/private/xiaomi/pages"
    );
    let mut counts = Vec::new();
    for (rank, (time, bytes, svg)) in pages.iter().enumerate() {
        assert_eq!(bytes.len() % RECORD_LEN, 0, "a multiple of 7");
        assert_eq!(page_kind(bytes), TabletKind::Blackboard);
        // The check every page passes before it is kept, then deleted from the tablet.
        assert!(
            check_page(bytes).is_whole(),
            "page {rank}: refused by the sync's check"
        );
        let page = decode_page(bytes);
        assert_eq!(page.trailing_bytes, 0);
        assert_eq!(page.unknown_records, 0, "every status is known");
        assert_eq!(page.records * RECORD_LEN, bytes.len());
        assert!(!page.strokes.is_empty());
        for point in page.strokes.iter().flat_map(|s| &s.points) {
            assert!(u32::from(point.x) < BLACKBOARD_PAGE.width);
            assert!(u32::from(point.y) < BLACKBOARD_PAGE.height);
            assert!(
                (1..=2047).contains(&point.pressure),
                "page {rank}: a pressure out of range"
            );
        }
        // As many strokes and points as the study's rendering (one polyline per stroke).
        if let Some(svg) = svg {
            let polylines = svg.matches("<polyline").count();
            let points: usize = svg
                .split("points=\"")
                .skip(1)
                .map(|rest| rest.split('"').next().unwrap_or("").split(' ').count())
                .sum();
            assert_eq!(page.strokes.len(), polylines, "strokes as rendered");
            assert_eq!(page.point_count(), points, "points as rendered");
        }
        // The note, as the sync makes it.
        let downloaded = DownloadedPage {
            bytes: bytes.clone(),
            timestamp: Some(i64::from(*time)),
            announced_size: u32::try_from(bytes.len()).unwrap(),
            kind: TabletKind::Blackboard,
        };
        let file = note_from_page(
            &downloaded,
            BLACKBOARD_PAGE,
            Orientation::Landscape,
            "id".into(),
            "Calque 1",
            0,
        )
        .unwrap();
        assert_eq!(file.note.strokes.len(), page.strokes.len());
        assert!(
            file.note.created == i64::from(*time) * 1000,
            "page {rank}: not dated by its time"
        );
        assert_eq!(file.note.timing, Timing::Synthesized);
        let last = file
            .note
            .strokes
            .last()
            .and_then(|s| s.points.last())
            .unwrap();
        assert!(last.t <= page.duration_ms);
        counts.push(format!(
            "{} records, {} strokes, {:.0} s",
            page.records,
            page.strokes.len(),
            f64::from(page.duration_ms) / 1000.0
        ));
    }
    say!("{} pages: {}", pages.len(), counts.join("; "));
}

#[test]
fn the_page_stored_during_the_captures_holds_the_live_records() {
    let _report = common::report("captures: a stored page is the live records");
    let Some(dir) = captures_or_skip("the_page_stored_during_the_captures_holds_the_live_records")
    else {
        return;
    };
    let pages = pages(&dir);
    let mut found = 0;
    for (rank, path) in captures(&dir).iter().enumerate() {
        let packets = packets(path);
        let live: Vec<u8> = packets
            .iter()
            .filter(|p| p.uuid == STREAM)
            .flat_map(|p| p.bytes.iter().copied())
            .collect();
        let starts_live = packets
            .iter()
            .any(|p| p.tx && BoardRequest::decode(&p.bytes) == Ok(BoardRequest::StartLive));
        if live.len() < 100 * RECORD_LEN || !starts_live {
            continue;
        }
        let holder = pages
            .iter()
            .find(|(_, bytes, _)| bytes.windows(live.len()).any(|w| w == live.as_slice()));
        assert!(
            holder.is_some(),
            "capture {rank}: the live records are in no stored page"
        );
        found += 1;
    }
    say!("{found} live sessions found byte for byte in a stored page");
    assert!(found > 0);
}
