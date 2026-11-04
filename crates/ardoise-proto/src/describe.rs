//! One-line English interpretations of raw bytes, for the `info` field of the diagnostic log
//! (SPEC §4.8). These functions never fail: unknown or malformed input is described as such,
//! with the reason.

use crate::frame::Frame;
use crate::live;
use crate::message::{Reply, Request, opcode};
use crate::stroke::{self, FileFormat, INTUOS_PRO_MAGIC, SLATE_MAGIC};
use crate::sysevent;

/// Describes a frame written by the host, e.g. `CONNECT uuid=89ea7888ffcb`.
pub fn describe_request(bytes: &[u8]) -> String {
    match Frame::decode(bytes) {
        Err(error) => format!("malformed frame: {error}"),
        Ok(frame) => {
            let name = request_name(frame.opcode);
            match Request::from_frame(frame) {
                Ok(request) => request.to_string(),
                Err(error) => format!("malformed {name} request: {error}"),
            }
        }
    }
}

/// Describes a frame notified by the tablet, e.g. `STATUS success`, `BATTERY 62% discharging`.
pub fn describe_reply(bytes: &[u8]) -> String {
    match Frame::decode(bytes) {
        Err(error) => format!("malformed frame: {error}"),
        Ok(frame) => {
            let name = reply_name(frame.opcode);
            match Reply::from_frame(frame) {
                Ok(reply) => reply.to_string(),
                Err(error) => format!("malformed {name} reply: {error}"),
            }
        }
    }
}

/// Describes one chunk received on the offline characteristic during a download. Packets
/// span chunks, so only the size and a file header at the start are reported.
pub fn describe_page_data(chunk: &[u8]) -> String {
    let header = if chunk.starts_with(&SLATE_MAGIC) {
        ", Slate file header"
    } else if chunk.starts_with(&INTUOS_PRO_MAGIC) {
        ", Intuos Pro file header"
    } else {
        ""
    };
    let plural = if chunk.len() == 1 { "" } else { "s" };
    format!("PAGE_DATA {} byte{plural}{header}", chunk.len())
}

/// Describes a whole downloaded page, e.g. `PAGE Slate: 3 strokes, 412 points`.
pub fn describe_page(file: &[u8]) -> String {
    let decoded = match stroke::decode(file) {
        Ok(decoded) => decoded,
        Err(error) => return format!("malformed page of {} bytes: {error}", file.len()),
    };
    let format = match decoded.format {
        FileFormat::Slate => "Slate",
        FileFormat::IntuosPro { .. } => "Intuos Pro",
    };
    let mut text = format!(
        "PAGE {format}: {} strokes, {} points",
        decoded.strokes.len(),
        decoded.point_count()
    );
    let lost: u64 = decoded
        .strokes
        .iter()
        .map(|s| u64::from(s.lost_points))
        .sum();
    if lost > 0 {
        text.push_str(&format!(", {lost} lost points"));
    }
    if decoded.unknown_packets > 0 {
        text.push_str(&format!(", {} unknown packets", decoded.unknown_packets));
    }
    if decoded.eof {
        text.push_str(", EOF");
    }
    if decoded.trailing_bytes > 0 {
        text.push_str(&format!(", {} bytes after EOF", decoded.trailing_bytes));
    }
    text
}

/// Describes a live-mode notification, e.g. `LIVE 3 samples`.
pub fn describe_live(value: &[u8]) -> String {
    match live::decode(value) {
        Ok(packet) => packet.to_string(),
        Err(error) => format!("malformed live packet: {error}"),
    }
}

/// Describes a notification of the sysevent characteristic, e.g. `BATTERY_LEVEL 41% (flag 00)`.
pub fn describe_sysevent(value: &[u8]) -> String {
    sysevent::decode(value).to_string()
}

fn request_name(op: u8) -> &'static str {
    match op {
        opcode::CONNECT => "CONNECT",
        opcode::REGISTER => "REGISTER",
        opcode::NAME => "GET_NAME/SET_NAME",
        opcode::TIME => "GET_TIME/SET_TIME",
        opcode::FIRMWARE => "GET_FIRMWARE",
        opcode::BATTERY => "GET_BATTERY",
        opcode::DIMENSION => "GET_DIMENSION",
        opcode::FILE_TRANSFER_REPORTING_TYPE => "SET_FILE_TRANSFER_REPORTING_TYPE",
        opcode::SET_MODE => "SET_MODE",
        opcode::FILES_COUNT => "AVAILABLE_FILES_COUNT",
        opcode::FILE_INFO => "GET_FILE_INFO",
        opcode::DOWNLOAD_OLDEST => "DOWNLOAD_OLDEST_FILE",
        opcode::DELETE_OLDEST => "DELETE_OLDEST_FILE",
        _ => "unknown",
    }
}

fn reply_name(op: u8) -> &'static str {
    match op {
        opcode::STATUS => "STATUS",
        opcode::REGISTER_BUTTON_PRESSED => "REGISTER_BUTTON_PRESSED",
        opcode::NAME_CHUNK => "NAME_CHUNK",
        opcode::TIME_VALUE => "TIME",
        opcode::FIRMWARE_VALUE => "FIRMWARE",
        opcode::BATTERY_VALUE => "BATTERY",
        opcode::DIMENSION_VALUE => "DIMENSION",
        opcode::FILES_COUNT_VALUE => "FILES_COUNT",
        opcode::FILE_INFO_VALUE => "FILE_INFO",
        opcode::DOWNLOAD => "DOWNLOAD_START/DOWNLOAD_END",
        _ => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex;
    use crate::stroke::{Point, Stroke, StrokeTime, encode_slate};

    fn bytes(text: &str) -> Vec<u8> {
        hex::decode(text).unwrap()
    }

    #[test]
    fn requests() {
        let cases = [
            ("e6 06 89 ea 78 88 ff cb", "CONNECT uuid=89ea7888ffcb"),
            ("e7 06 89 ea 78 88 ff cb", "REGISTER uuid=89ea7888ffcb"),
            ("bb 01 00", "GET_NAME"),
            ("bb 05 41 72 64 6f 0a", "SET_NAME \"Ardo\""),
            ("b6 01 00", "GET_TIME"),
            ("b6 06 19 03 15 03 22 16", "SET_TIME 2019-03-15T03:22:16Z"),
            ("b7 01 01", "GET_FIRMWARE low"),
            ("b9 01 00", "GET_BATTERY"),
            ("ea 02 03 00", "GET_DIMENSION width"),
            (
                "ec 06 06 00 00 00 00 00",
                "SET_FILE_TRANSFER_REPORTING_TYPE",
            ),
            ("b1 01 01", "SET_MODE paper"),
            ("c1 01 00", "AVAILABLE_FILES_COUNT"),
            ("cc 01 00", "GET_FILE_INFO"),
            ("c3 01 00", "DOWNLOAD_OLDEST_FILE"),
            ("ca 01 00", "DELETE_OLDEST_FILE"),
            ("e3 01 00", "unknown request e3 01 00"),
            (
                "b1 01 07",
                "malformed SET_MODE request: opcode b1: unknown mode 07",
            ),
            (
                "b6 06 19 13 15 03 22 16",
                "malformed GET_TIME/SET_TIME request: opcode b6: \
                 2019-13-15 03:22:16 is not a valid date and time",
            ),
            (
                "e6 02 00",
                "malformed frame: frame length byte says 2 argument bytes but 1 are present",
            ),
            (
                "",
                "malformed frame: frame of 0 bytes is too short (opcode and length need 2)",
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(describe_request(&bytes(text)), expected, "{text}");
        }
    }

    #[test]
    fn replies() {
        let cases = [
            ("b3 01 00", "STATUS success"),
            ("b3 01 07", "STATUS authorization error"),
            ("b3 01 09", "STATUS unknown code 09"),
            ("e4 01 00", "REGISTER_BUTTON_PRESSED"),
            (
                "bc 0d 42 61 6d 62 6f 6f 20 53 6c 61 74 65 0a",
                "NAME_CHUNK \"Bamboo Slate\\n\"",
            ),
            ("bd 06 19 03 15 03 22 16", "TIME 2019-03-15T03:22:16Z"),
            (
                "b8 09 00 42 31 38 30 31 33 31 61",
                "FIRMWARE high \"B180131a\"",
            ),
            ("ba 02 3e 00", "BATTERY 62% discharging"),
            ("ba 02 64 01", "BATTERY 100% charging"),
            ("eb 06 03 00 60 54 00 00", "DIMENSION width=21600"),
            ("c2 02 03 00", "FILES_COUNT 3"),
            (
                "cf 0a 7c 0d 00 00 19 03 15 03 22 16",
                "FILE_INFO size=3452 time=2019-03-15T03:22:16Z",
            ),
            ("c8 01 be", "DOWNLOAD_START"),
            ("c8 03 be 70 0d", "DOWNLOAD_START extra=70 0d"),
            ("c8 05 ed 26 39 f4 cb", "DOWNLOAD_END crc=cbf43926"),
            ("f1 02 13 00", "unknown reply f1 02 13 00"),
            ("cb 01 00", "cb 01 00, live-mode button (hypothesis)"),
            (
                "c8 01 aa",
                "malformed DOWNLOAD_START/DOWNLOAD_END reply: \
                 opcode c8: first argument aa is neither be (start) nor ed (end)",
            ),
            (
                "ba 01 3e",
                "malformed BATTERY reply: opcode ba: 1 argument bytes, expected 2",
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(describe_reply(&bytes(text)), expected, "{text}");
        }
    }

    #[test]
    fn sysevents() {
        assert_eq!(
            describe_sysevent(&bytes("f1 02 2f 00")),
            "BATTERY_LEVEL 47% (flag 00)"
        );
        assert_eq!(
            describe_sysevent(&bytes("ef 01 00")),
            "SYSEVENT ef 01 00, seen right after REGISTER, meaning unknown (ignored)"
        );
        assert_eq!(
            describe_sysevent(&bytes("f1 02 99 00")),
            "unknown sysevent f1 02 99 00"
        );
        assert_eq!(describe_sysevent(&[]), "unknown sysevent ");
    }

    #[test]
    fn live_packets() {
        assert_eq!(
            describe_live(&bytes(
                "a1 12 c4 09 f4 01 00 00 c5 09 f5 01 10 00 c6 09 f6 01 ff 07"
            )),
            "LIVE 3 samples"
        );
        assert_eq!(
            describe_live(&bytes("a1 05 01 02 03 04 05")),
            "malformed live packet: pen samples length 5 is not a multiple of 6"
        );
        assert_eq!(
            describe_live(&[]),
            "malformed live packet: empty live notification"
        );
        assert_eq!(describe_live(&[0x42]), "unknown live packet 42");
    }

    #[test]
    fn page_data_and_pages() {
        assert_eq!(
            describe_page_data(&bytes("62 38 62 74 ff ff ee")),
            "PAGE_DATA 7 bytes, Slate file header"
        );
        assert_eq!(describe_page_data(&[0x00]), "PAGE_DATA 1 byte");
        assert_eq!(
            describe_page_data(&INTUOS_PRO_MAGIC),
            "PAGE_DATA 4 bytes, Intuos Pro file header"
        );

        let stroke = Stroke {
            time: StrokeTime::SincePowerUp { millis: 0 },
            points: vec![
                Point {
                    x: 1,
                    y: 2,
                    pressure: 3,
                },
                Point {
                    x: 2,
                    y: 2,
                    pressure: 3,
                },
            ],
            ..Stroke::default()
        };
        let file = encode_slate(&[stroke.clone(), stroke]);
        assert_eq!(describe_page(&file), "PAGE Slate: 2 strokes, 4 points");
        assert_eq!(
            describe_page(&file[..file.len() - 1]),
            format!(
                "malformed page of {} bytes: packet at offset {} is cut short by the end of the data",
                file.len() - 1,
                file.len() - 7
            )
        );
    }
}
