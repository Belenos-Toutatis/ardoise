//! Every Slate request and reply against the bytes of protocol.md (real device traces quoted
//! from Tuhi issues) and of Tuhi's message tests.
//!
//! Byte pairs marked "Tuhi" are data copied from Tuhi's test suite
//! (https://github.com/tuhiproject/tuhi, test/test_messages.py, Copyright (c) 2019 Red Hat,
//! Inc., GPL-2.0-or-later). Pairs marked "issue #N" are real device traces quoted in
//! protocol.md.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use ardoise_proto::message::{MessageError, NameAssembler, opcode};
use ardoise_proto::{
    BcdTime, Dimension, FirmwarePart, Frame, FrameError, Mode, Reply, Request, StatusCode, crc32,
};
use common::bytes;

/// The request encodes to exactly `hex` and `hex` decodes back to it.
fn request(hex: &str, expected: Request) {
    let data = bytes(hex);
    assert_eq!(expected.encode().unwrap(), data, "{expected}");
    assert_eq!(Request::decode(&data).unwrap(), expected, "{hex}");
}

/// The reply decodes from exactly `hex` and encodes back to it.
fn reply(hex: &str, expected: Reply) {
    let data = bytes(hex);
    assert_eq!(Reply::decode(&data).unwrap(), expected, "{hex}");
    assert_eq!(expected.encode().unwrap(), data, "{expected}");
}

fn time_2019() -> BcdTime {
    BcdTime::new(2019, 3, 15, 3, 22, 16).unwrap()
}

const ISSUE_117_UUID: [u8; 6] = [0x89, 0xea, 0x78, 0x88, 0xff, 0xcb];

// ---- Requests ----------------------------------------------------------------------------

#[test]
fn request_connect() {
    request(
        "e6 06 89 ea 78 88 ff cb",
        Request::Connect {
            uuid: ISSUE_117_UUID,
        },
    );
    // issue #212 (answered b3 01 07).
    request(
        "e6 06 a0 bd 39 11 79 74",
        Request::Connect {
            uuid: [0xa0, 0xbd, 0x39, 0x11, 0x79, 0x74],
        },
    );
    // Tuhi: uuid 'abcdef123456'.
    request(
        "e6 06 ab cd ef 12 34 56",
        Request::Connect {
            uuid: [0xab, 0xcd, 0xef, 0x12, 0x34, 0x56],
        },
    );
}

#[test]
fn request_register_press_button() {
    // issue #117.
    request(
        "e7 06 89 ea 78 88 ff cb",
        Request::RegisterPressButton {
            uuid: ISSUE_117_UUID,
        },
    );
}

#[test]
fn request_get_name() {
    request("bb 01 00", Request::GetName);
}

#[test]
fn request_set_name() {
    // Tuhi: 'test dev name' plus the trailing line feed.
    request(
        "bb 0e 74 65 73 74 20 64 65 76 20 6e 61 6d 65 0a",
        Request::SetName("test dev name".to_owned()),
    );
}

#[test]
fn request_get_time() {
    request("b6 01 00", Request::GetTime);
}

#[test]
fn request_set_time() {
    // issue #117: 2019-03-15 03:22:16 UTC.
    request("b6 06 19 03 15 03 22 16", Request::SetTime(time_2019()));
}

#[test]
fn request_get_firmware() {
    request(
        "b7 01 00",
        Request::GetFirmware {
            part: FirmwarePart::High,
        },
    );
    request(
        "b7 01 01",
        Request::GetFirmware {
            part: FirmwarePart::Low,
        },
    );
}

#[test]
fn request_get_battery() {
    request("b9 01 00", Request::GetBattery);
}

#[test]
fn request_get_dimension() {
    request("ea 02 03 00", Request::GetDimension(Dimension::Width));
    request("ea 02 04 00", Request::GetDimension(Dimension::Height));
}

#[test]
fn request_set_file_transfer_reporting_type() {
    request(
        "ec 06 06 00 00 00 00 00",
        Request::SetFileTransferReportingType,
    );
}

#[test]
fn request_set_mode() {
    request("b1 01 00", Request::SetMode(Mode::Live));
    request("b1 01 01", Request::SetMode(Mode::Paper));
    request("b1 01 02", Request::SetMode(Mode::Idle));
}

#[test]
fn request_available_files_count() {
    request("c1 01 00", Request::AvailableFilesCount);
}

#[test]
fn request_get_file_info() {
    request("cc 01 00", Request::GetFileInfo);
}

#[test]
fn request_download_oldest_file() {
    request("c3 01 00", Request::DownloadOldestFile);
}

#[test]
fn request_delete_oldest_file() {
    request("ca 01 00", Request::DeleteOldestFile);
}

#[test]
fn request_unknown_opcodes_are_kept() {
    // UNKNOWN_E3 (Spark flow) and the Spark registration request.
    request("e3 01 00", Request::Unknown(Frame::new(0xe3, [0x00])));
    request("e3 01 01", Request::Unknown(Frame::new(0xe3, [0x01])));
    // Intuos Pro get-name.
    request("db 01 00", Request::Unknown(Frame::new(0xdb, [0x00])));
}

#[test]
fn malformed_requests_are_errors() {
    let cases = [
        "e6 05 89 ea 78 88 ff",
        "e7 07 89 ea 78 88 ff cb 00",
        "bb 00",
        "bb 02 41 42",
        "b6 01 01",
        "b6 02 19 03",
        "b6 06 19 13 15 03 22 16",
        "b6 06 1a 03 15 03 22 16",
        "b7 01 02",
        "b7 00",
        "b9 01 01",
        "b9 02 00 00",
        "ea 02 14 00",
        "ea 02 03 01",
        "ea 01 03",
        "ec 06 07 00 00 00 00 00",
        "ec 01 00",
        "b1 01 03",
        "b1 02 00 00",
        "c1 00",
        "cc 01 01",
        "c3 02 00 00",
        "ca 01 ff",
    ];
    for hex in cases {
        let result = Request::decode(&bytes(hex));
        assert!(
            matches!(
                result,
                Err(MessageError::Length { .. }
                    | MessageError::Argument { .. }
                    | MessageError::Time { .. })
            ),
            "{hex}: {result:?}"
        );
    }
    assert_eq!(
        Request::decode(&bytes("e6 06 89 ea")),
        Err(MessageError::Frame(FrameError::LengthMismatch {
            declared: 6,
            actual: 2
        }))
    );
}

// ---- Replies -----------------------------------------------------------------------------

#[test]
fn reply_status() {
    reply("b3 01 00", Reply::Status(StatusCode::Success));
    reply("b3 01 01", Reply::Status(StatusCode::GeneralError)); // Tuhi error test
    reply("b3 01 02", Reply::Status(StatusCode::InvalidState)); // issue #124
    reply("b3 01 03", Reply::Status(StatusCode::ReadOnlyParam));
    reply("b3 01 04", Reply::Status(StatusCode::CommandNotSupported));
    reply("b3 01 07", Reply::Status(StatusCode::AuthorizationError)); // issue #212
    reply("b3 01 05", Reply::Status(StatusCode::Other(5)));
}

#[test]
fn reply_register_button_pressed() {
    // issue #117.
    reply("e4 01 00", Reply::RegisterButtonPressed);
}

#[test]
fn reply_name_chunk() {
    // issue #117: "Bamboo Slate Small", 18 bytes, no trailing 0a.
    reply(
        "bc 12 42 61 6d 62 6f 6f 20 53 6c 61 74 65 20 53 6d 61 6c 6c",
        Reply::NameChunk(b"Bamboo Slate Small".to_vec()),
    );
    // Slate Large: "Bamboo Slate\n".
    reply(
        "bc 0d 42 61 6d 62 6f 6f 20 53 6c 61 74 65 0a",
        Reply::NameChunk(b"Bamboo Slate\n".to_vec()),
    );
    // issue #117: the stray frame.
    reply(
        "bc 09 00 42 61 6d 62 6f 6f 20 0a",
        Reply::NameChunk(b"\0Bamboo \n".to_vec()),
    );
    // Tuhi: 'test dev name\n'.
    let Reply::NameChunk(chunk) =
        Reply::decode(&bytes("bc 0e 74 65 73 74 20 64 65 76 20 6e 61 6d 65 0a")).unwrap()
    else {
        panic!("not a name chunk");
    };
    assert_eq!(
        NameAssembler::new().push(&chunk).as_deref(),
        Some("test dev name")
    );
}

#[test]
fn reply_time() {
    // issue #117.
    reply("bd 06 19 03 15 03 22 16", Reply::Time(time_2019()));
}

#[test]
fn reply_firmware() {
    // issue #256 (Intuos Pro, same reply format).
    reply(
        "b8 09 00 42 31 38 30 31 33 31 61",
        Reply::Firmware {
            part: FirmwarePart::High,
            text: "B180131a".to_owned(),
        },
    );
    // Slate A4 version B160805A-C170704A (issue #117), as the two ASCII halves.
    reply(
        "b8 09 00 42 31 36 30 38 30 35 41",
        Reply::Firmware {
            part: FirmwarePart::High,
            text: "B160805A".to_owned(),
        },
    );
    reply(
        "b8 09 01 43 31 37 30 37 30 34 41",
        Reply::Firmware {
            part: FirmwarePart::Low,
            text: "C170704A".to_owned(),
        },
    );
}

#[test]
fn reply_battery() {
    // protocol.md §4.3: 62 %, discharging.
    reply(
        "ba 02 3e 00",
        Reply::Battery {
            percent: 62,
            charging: false,
        },
    );
    // Tuhi: 78 %, charging.
    reply(
        "ba 02 4e 01",
        Reply::Battery {
            percent: 78,
            charging: true,
        },
    );
}

#[test]
fn reply_dimension() {
    // issue #266 (small Slate).
    reply(
        "eb 06 03 00 60 54 00 00",
        Reply::Dimension {
            which: Dimension::Width,
            value: 21_600,
        },
    );
    reply(
        "eb 06 04 00 d0 39 00 00",
        Reply::Dimension {
            which: Dimension::Height,
            value: 14_800,
        },
    );
    // Slate Large (protocol.md §11).
    reply(
        "eb 06 03 00 04 74 00 00",
        Reply::Dimension {
            which: Dimension::Width,
            value: ardoise_proto::SLATE_LARGE_WIDTH,
        },
    );
    reply(
        "eb 06 04 00 60 54 00 00",
        Reply::Dimension {
            which: Dimension::Height,
            value: ardoise_proto::SLATE_LARGE_HEIGHT,
        },
    );
    // Tuhi: 1234 and 4321.
    reply(
        "eb 06 03 00 d2 04 00 00",
        Reply::Dimension {
            which: Dimension::Width,
            value: 1234,
        },
    );
    reply(
        "eb 06 04 00 e1 10 00 00",
        Reply::Dimension {
            which: Dimension::Height,
            value: 4321,
        },
    );
}

#[test]
fn reply_files_count() {
    // Tuhi (Slate, little-endian): 1234.
    reply("c2 02 d2 04", Reply::FilesCount(1234));
}

#[test]
fn reply_file_info() {
    // Tuhi: count 1024; the size 3452 of the issue #256 trace.
    reply(
        "cf 0a 00 04 00 00 19 03 15 03 22 16",
        Reply::FileInfo {
            size: 1024,
            time: time_2019(),
        },
    );
    reply(
        "cf 0a 7c 0d 00 00 24 02 29 23 59 59",
        Reply::FileInfo {
            size: 3452,
            time: BcdTime::new(2024, 2, 29, 23, 59, 59).unwrap(),
        },
    );
}

#[test]
fn reply_download_start() {
    // Tuhi.
    reply("c8 01 be", Reply::DownloadStart { extra: Vec::new() });
    // Shape of the issue #256 Intuos Pro reply: size 0x0d70, unix time, 00 00.
    reply(
        "c8 0b be 70 0d 00 00 cc ce 53 5d 00 00",
        Reply::DownloadStart {
            extra: bytes("70 0d 00 00 cc ce 53 5d 00 00"),
        },
    );
}

#[test]
fn reply_download_end() {
    reply(
        "c8 05 ed 26 39 f4 cb",
        Reply::DownloadEnd {
            crc: crc32(b"123456789"),
        },
    );
    // The CRC covers the whole file as received.
    let blob = &common::tuhi_blobs()[0];
    let end = Reply::DownloadEnd { crc: crc32(blob) }.encode().unwrap();
    assert_eq!(end[..3], [opcode::DOWNLOAD, 0x05, 0xed]);
    assert_eq!(end[3..], crc32(blob).to_le_bytes());
}

#[test]
fn reply_unknown_opcodes_are_kept() {
    // Sysevent seen on connect (issue #212), Intuos Pro connect reply and button reply.
    reply(
        "f1 02 13 00",
        Reply::Unknown(Frame::new(0xf1, [0x13, 0x00])),
    );
    reply(
        "50 06 ab cd ef 12 34 56",
        Reply::Unknown(Frame::new(0x50, bytes("ab cd ef 12 34 56"))),
    );
    reply("53 00", Reply::Unknown(Frame::new(0x53, [])));
    assert!(
        !Reply::decode(&bytes("f1 02 13 00"))
            .unwrap()
            .is_live_button()
    );
}

#[test]
fn the_unasked_cb_frame_of_a_real_slate_is_taken_for_its_button() {
    // Our capture (Slate A4, 2026-11-02), on the command channel during live mode.
    let frame = Reply::decode(&bytes("cb 01 00")).unwrap();
    assert_eq!(
        frame,
        Reply::Unknown(Frame::new(opcode::LIVE_BUTTON, [0x00]))
    );
    assert!(frame.is_live_button());
    assert_eq!(frame.to_string(), "cb 01 00, live-mode button (hypothesis)");
}

#[test]
fn status_is_a_legal_reply_to_any_request() {
    // issue #124 / #202: GET_DIMENSION answered with INVALID_STATE.
    assert_eq!(
        Reply::decode(&bytes("b3 01 02")).unwrap(),
        Reply::Status(StatusCode::InvalidState)
    );
}

#[test]
fn malformed_replies_are_errors() {
    let cases = [
        "b3 00",
        "b3 02 00 00",
        "bd 05 19 03 15 03 22",
        "bd 06 19 03 15 03 22 60",
        "bd 06 19 03 32 03 22 16",
        "b8 00",
        "b8 03 02 41 42",
        "ba 01 3e",
        "ba 03 3e 00 00",
        "eb 06 05 00 60 54 00 00",
        "eb 06 03 01 60 54 00 00",
        "eb 04 03 00 60 54",
        "c2 01 03",
        "c2 04 03 00 00 00",
        "cf 06 7c 0d 00 00 19 03",
        "cf 0a 7c 0d 00 00 19 03 15 03 22 6a",
        "c8 00",
        "c8 01 aa",
        "c8 01 ed",
        "c8 04 ed 26 39 f4",
        "c8 06 ed 26 39 f4 cb 00",
    ];
    for hex in cases {
        let result = Reply::decode(&bytes(hex));
        assert!(
            matches!(
                result,
                Err(MessageError::Length { .. }
                    | MessageError::Argument { .. }
                    | MessageError::Time { .. })
            ),
            "{hex}: {result:?}"
        );
    }
    for hex in ["", "b3", "b3 01", "b3 01 00 00", "c8 05 ed 26"] {
        assert!(
            matches!(Reply::decode(&bytes(hex)), Err(MessageError::Frame(_))),
            "{hex}"
        );
    }
}

// ---- Whole exchanges ---------------------------------------------------------------------

/// One host write, its expected decoding, and the tablet answer if any.
type Exchange = (&'static str, Request, Option<(&'static str, Reply)>);

#[test]
fn issue_117_registration_trace() {
    // Start of a real Slate Small registration (protocol.md §3.2).
    let exchange: [Exchange; 6] = [
        (
            "e7 06 89 ea 78 88 ff cb",
            Request::RegisterPressButton {
                uuid: ISSUE_117_UUID,
            },
            Some(("e4 01 00", Reply::RegisterButtonPressed)),
        ),
        (
            "b6 06 19 03 15 03 22 16",
            Request::SetTime(time_2019()),
            Some(("b3 01 00", Reply::Status(StatusCode::Success))),
        ),
        (
            "b6 01 00",
            Request::GetTime,
            Some(("bd 06 19 03 15 03 22 16", Reply::Time(time_2019()))),
        ),
        (
            "ec 06 06 00 00 00 00 00",
            Request::SetFileTransferReportingType,
            Some(("b3 01 00", Reply::Status(StatusCode::Success))),
        ),
        (
            "bb 01 00",
            Request::GetName,
            Some((
                "bc 12 42 61 6d 62 6f 6f 20 53 6c 61 74 65 20 53 6d 61 6c 6c",
                Reply::NameChunk(b"Bamboo Slate Small".to_vec()),
            )),
        ),
        (
            "e6 06 89 ea 78 88 ff cb",
            Request::Connect {
                uuid: ISSUE_117_UUID,
            },
            None,
        ),
    ];
    for (tx, expected_request, rx) in exchange {
        request(tx, expected_request.clone());
        assert_eq!(expected_request.expects_reply(), !tx.starts_with("e7"));
        if let Some((rx, expected_reply)) = rx {
            reply(rx, expected_reply);
        }
    }
    assert_eq!(time_2019().to_unix(), 1_552_620_136);
}

#[test]
fn firmware_halves_make_the_version() {
    let high = Reply::decode(&bytes("b8 09 00 42 31 36 30 38 30 35 41")).unwrap();
    let low = Reply::decode(&bytes("b8 09 01 43 31 37 30 37 30 34 41")).unwrap();
    let (
        Reply::Firmware {
            part: FirmwarePart::High,
            text: high,
        },
        Reply::Firmware {
            part: FirmwarePart::Low,
            text: low,
        },
    ) = (high, low)
    else {
        panic!("unexpected replies");
    };
    assert_eq!(format!("{high}-{low}"), "B160805A-C170704A");
}
