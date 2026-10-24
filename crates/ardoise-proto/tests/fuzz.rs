//! Deterministic pseudo-random inputs for every decoder: none may panic, whatever the bytes.
//! Also a randomized encode/decode round trip of Slate page files and live samples.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use ardoise_proto::describe::{
    describe_live, describe_page, describe_page_data, describe_reply, describe_request,
};
use ardoise_proto::live::{self, LiveRecord};
use ardoise_proto::stroke::{self, Point, Stroke, StrokeTime, encode_slate, parse_packet};
use ardoise_proto::{BcdTime, Frame, Reply, Request, hex};

/// Knuth's MMIX linear congruential generator; good enough to spread test inputs.
struct Lcg(u64);

impl Lcg {
    fn next_u32(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 32) as u32
    }

    fn below(&mut self, bound: u32) -> u32 {
        self.next_u32() % bound
    }

    fn byte(&mut self) -> u8 {
        self.next_u32().to_le_bytes()[0]
    }

    /// A byte biased toward values that are meaningful to the decoders.
    fn interesting_byte(&mut self) -> u8 {
        const SPECIAL: [u8; 14] = [
            0x00, 0xff, 0xfc, 0xfa, 0xee, 0xdd, 0xbf, 0xa8, 0xe8, 0xc8, 0x0a, 0xa1, 0xb3, 0xc8,
        ];
        if self.below(3) == 0 {
            SPECIAL[self.below(SPECIAL.len() as u32) as usize]
        } else {
            self.byte()
        }
    }

    fn bytes(&mut self, max_len: u32, interesting: bool) -> Vec<u8> {
        let len = self.below(max_len + 1);
        (0..len)
            .map(|_| {
                if interesting {
                    self.interesting_byte()
                } else {
                    self.byte()
                }
            })
            .collect()
    }
}

/// Runs every decoder and describer on `data`; reaching the end means nothing panicked.
fn exercise(data: &[u8]) {
    if let Ok(frame) = Frame::decode(data) {
        assert_eq!(frame.encode().unwrap(), data);
        if let Ok(request) = Request::from_frame(frame.clone()) {
            // Decoding then encoding gives a canonical form that decodes to the same value.
            let canonical = request.encode().unwrap();
            assert_eq!(Request::decode(&canonical).unwrap(), request);
        }
        if let Ok(reply) = Reply::from_frame(frame) {
            let canonical = reply.encode().unwrap();
            assert_eq!(Reply::decode(&canonical).unwrap(), reply);
        }
    }
    let _ = Frame::decode_prefix(data);
    let _ = live::decode(data);
    let _ = parse_packet(data);
    let _ = stroke::decode(data);
    if let Ok(bcd) = <[u8; 6]>::try_from(data)
        && let Ok(time) = BcdTime::from_bcd(bcd)
    {
        assert_eq!(time.to_bcd(), bcd);
    }
    for text in [
        describe_request(data),
        describe_reply(data),
        describe_live(data),
        describe_page(data),
        describe_page_data(data),
    ] {
        assert!(!text.is_empty());
        assert!(!text.contains('\n'), "{text}");
    }
    assert_eq!(hex::decode(&hex::encode(data)).unwrap(), data);
}

#[test]
fn random_bytes_never_panic() {
    let mut rng = Lcg(0x0a7d_0153);
    for round in 0..20_000 {
        let data = rng.bytes(40, round % 2 == 0);
        exercise(&data);
    }
}

#[test]
fn random_frames_with_known_opcodes_never_panic() {
    const OPCODES: [u8; 26] = [
        0xe6, 0xe7, 0xbb, 0xb6, 0xb7, 0xb9, 0xea, 0xec, 0xb1, 0xc1, 0xcc, 0xc3, 0xca, 0xb3, 0xe4,
        0xbc, 0xbd, 0xb8, 0xba, 0xeb, 0xc2, 0xcf, 0xc8, 0xa1, 0xa2, 0x10,
    ];
    let mut rng = Lcg(0x5eed);
    for _ in 0..20_000 {
        let opcode = OPCODES[rng.below(OPCODES.len() as u32) as usize];
        let args = rng.bytes(19, true);
        let mut data = vec![opcode, u8::try_from(args.len()).unwrap()];
        data.extend_from_slice(&args);
        exercise(&data);
        // Wrong length byte.
        data[1] = data[1].wrapping_add(1);
        exercise(&data);
    }
}

#[test]
fn random_page_files_never_panic() {
    let mut rng = Lcg(0xb8b7);
    for round in 0..5_000 {
        let mut data = if round % 2 == 0 {
            stroke::SLATE_MAGIC.to_vec()
        } else {
            let mut header = stroke::INTUOS_PRO_MAGIC.to_vec();
            header.extend((0..12).map(|_| rng.byte()));
            header
        };
        data.extend(rng.bytes(300, true));
        exercise(&data);
    }
}

#[test]
fn mutated_real_page_files_never_panic() {
    let mut rng = Lcg(0x7071);
    let blobs = common::tuhi_blobs();
    for round in 0..3_000 {
        let mut data = blobs[round % blobs.len()].clone();
        for _ in 0..=rng.below(4) {
            let index = rng.below(u32::try_from(data.len()).unwrap()) as usize;
            data[index] = rng.interesting_byte();
        }
        if rng.below(4) == 0 {
            data.truncate(rng.below(u32::try_from(data.len()).unwrap()) as usize);
        }
        exercise(&data);
    }
}

fn random_point(rng: &mut Lcg, previous: Option<Point>) -> Point {
    match previous {
        // Mostly small moves, sometimes a jump, to use every packet coding.
        Some(p) if rng.below(8) != 0 => {
            let step = |value: u16, rng: &mut Lcg, span: u32| {
                let offset = i64::from(rng.below(2 * span + 1)) - i64::from(span);
                u16::try_from((i64::from(value) + offset).clamp(0, 65_535)).unwrap()
            };
            Point {
                x: step(p.x, rng, 150),
                y: step(p.y, rng, 150),
                pressure: step(p.pressure, rng, 40).min(2047),
            }
        }
        _ => Point {
            x: u16::try_from(rng.below(65_536)).unwrap(),
            y: u16::try_from(rng.below(65_536)).unwrap(),
            pressure: u16::try_from(rng.below(2048)).unwrap(),
        },
    }
}

#[test]
fn random_slate_files_round_trip() {
    let mut rng = Lcg(0x51a7e);
    for _ in 0..300 {
        let strokes: Vec<Stroke> = (0..rng.below(12))
            .map(|_| {
                let mut points: Vec<Point> = Vec::new();
                for _ in 0..rng.below(200) {
                    let point = random_point(&mut rng, points.last().copied());
                    points.push(point);
                }
                Stroke {
                    time: StrokeTime::SincePowerUp {
                        millis: 5 * rng.below(65_536),
                    },
                    pen: None,
                    points,
                    lost_points: if rng.below(10) == 0 {
                        rng.below(100_000)
                    } else {
                        0
                    },
                }
            })
            .collect();
        let encoded = encode_slate(&strokes);
        let decoded = stroke::decode(&encoded).unwrap();
        assert_eq!(decoded.strokes, strokes);
        assert_eq!(decoded.unknown_packets, 0);
    }
}

#[test]
fn random_live_samples_round_trip() {
    let mut rng = Lcg(0x11fe);
    for _ in 0..1_000 {
        let records: Vec<LiveRecord> = (0..rng.below(40))
            .map(|_| {
                if rng.below(10) == 0 {
                    LiveRecord::ProximityOut
                } else {
                    LiveRecord::Pen(random_point(&mut rng, None))
                }
            })
            .collect();
        let mut decoded = Vec::new();
        for value in live::encode_samples(&records) {
            assert!(value.len() <= ardoise_proto::MAX_NOTIFICATION_PAYLOAD);
            let live::LivePacket::Samples(chunk) = live::decode(&value).unwrap() else {
                panic!("not samples");
            };
            decoded.extend(chunk);
        }
        assert_eq!(decoded, records);
    }
}
