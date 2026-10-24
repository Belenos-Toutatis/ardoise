//! Tests of the narration: `audio.opus` round trips and layout, foreign and hostile streams,
//! merging, WAV.

use std::time::Duration;

use super::*;
use crate::microphone::simulated_narration;
use crate::ogg::tests::{fix_all_crcs, fix_crc, raw_page};
use crate::ogg::{FLAG_CONTINUED, Page, pages};
use crate::opus::OpusPacketDecoder;

/// Deterministic pseudo-random numbers (the generator of `ardoise-core`'s format tests).
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Lcg(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 11
    }

    /// Uniform in `0..n` (n > 0).
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

fn narration(
    clip: AudioClip,
    base: NarrationBase,
    options: ReplayOptions,
    start_ms: i64,
) -> Narration {
    Narration {
        clip,
        base,
        options,
        start_ms,
    }
}

fn replay(clip: AudioClip, start_ms: i64) -> Narration {
    narration(
        clip,
        NarrationBase::Replay,
        ReplayOptions::default(),
        start_ms,
    )
}

fn rms(samples: &[i16]) -> f64 {
    let sum: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
    (sum / samples.len().max(1) as f64).sqrt()
}

/// Signal-to-noise ratio of `decoded` against `original`, in dB, after the first 20 ms (the
/// encoder's start-up).
fn snr(original: &[i16], decoded: &[i16], channels: usize) -> f64 {
    let (signal, noise) =
        original
            .iter()
            .zip(decoded)
            .skip(960 * channels)
            .fold((0f64, 0f64), |(s, n), (&a, &b)| {
                let (a, b) = (f64::from(a), f64::from(b));
                (s + a * a, n + (a - b) * (a - b))
            });
    10.0 * (signal / noise).log10()
}

/// Every page of a stream.
fn all_pages(bytes: &[u8]) -> Vec<Page<'_>> {
    pages(bytes).collect::<Result<_, _>>().unwrap()
}

#[test]
fn round_trips_mono_and_stereo() {
    for channels in [1u16, 2] {
        let clip = simulated_narration(Duration::from_secs(3), channels);
        let original = narration(
            clip.clone(),
            NarrationBase::Replay,
            ReplayOptions {
                speed: 2.5,
                max_pause: Some(ms(500)),
            },
            -1234,
        );
        let bytes = encode_narration(&original).unwrap();
        let decoded = decode_narration(&bytes).unwrap();

        assert_eq!(decoded.clip.channels, channels);
        assert_eq!(
            decoded.clip.samples.len(),
            clip.samples.len(),
            "{channels} ch"
        );
        assert_eq!(decoded.base, NarrationBase::Replay);
        assert_eq!(decoded.options, original.options);
        assert_eq!(decoded.start_ms, -1234);
        assert_eq!(decoded.duration(), Duration::from_secs(3));
        assert_eq!(decoded.end_ms(), 1766);

        let (before, after) = (rms(&clip.samples), rms(&decoded.clip.samples));
        assert!(
            (after / before - 1.0).abs() < 0.1,
            "{channels} ch: RMS {after:.0} for {before:.0}"
        );
        let snr = snr(&clip.samples, &decoded.clip.samples, usize::from(channels));
        println!(
            "{channels} ch: {} bytes, SNR {snr:.1} dB, RMS {after:.0} for {before:.0}",
            bytes.len()
        );
        assert!(snr > 12.0, "{channels} ch: SNR {snr:.1} dB");
        // About the bitrate (64 or 96 kbit/s) plus the pages: 3 s take less than 40 kB.
        let expected = default_opus_bitrate(channels) as usize / 8 * 3;
        assert!(bytes.len() < expected * 12 / 10, "{} bytes", bytes.len());
    }
}

#[test]
fn time_bases_round_trip() {
    let clip = simulated_narration(ms(300), 1);
    for (base, options, start) in [
        (NarrationBase::Replay, ReplayOptions::default(), 0),
        (
            NarrationBase::Replay,
            ReplayOptions {
                speed: 0.25,
                max_pause: Some(ms(1)),
            },
            i64::MAX,
        ),
        (
            NarrationBase::Replay,
            ReplayOptions {
                speed: 16.0,
                max_pause: Some(ms(86_400_000)),
            },
            i64::MIN,
        ),
        (
            NarrationBase::Replay,
            ReplayOptions {
                speed: 1.0 / 3.0,
                max_pause: None,
            },
            42,
        ),
        (NarrationBase::Note, ReplayOptions::default(), -800),
    ] {
        let decoded = decode_narration(
            &encode_narration(&narration(clip.clone(), base, options, start)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            (decoded.base, decoded.options, decoded.start_ms),
            (base, options, start)
        );
    }

    // Note time has no speed and no trimming, whatever the struct says.
    let note = narration(
        clip,
        NarrationBase::Note,
        ReplayOptions {
            speed: 3.0,
            max_pause: Some(ms(500)),
        },
        5,
    );
    let decoded = decode_narration(&encode_narration(&note).unwrap()).unwrap();
    assert_eq!(decoded.base, NarrationBase::Note);
    assert_eq!(decoded.options, ReplayOptions::default());
    // A pause is stored in whole milliseconds.
    let precise = narration(
        AudioClip {
            channels: 1,
            samples: vec![0; 480],
        },
        NarrationBase::Replay,
        ReplayOptions {
            speed: 1.0,
            max_pause: Some(Duration::from_micros(1_600)),
        },
        0,
    );
    let decoded = decode_narration(&encode_narration(&precise).unwrap()).unwrap();
    assert_eq!(decoded.options.max_pause, Some(ms(2)));
}

#[test]
fn empty_and_tiny_clips_round_trip() {
    for frames in [0usize, 1, 119, 120, 959, 960, 961] {
        for channels in [1u16, 2] {
            let clip = AudioClip {
                channels,
                samples: (0..frames * usize::from(channels))
                    .map(|i| (i as i16).wrapping_mul(37))
                    .collect(),
            };
            let decoded = decode_narration(&encode_narration(&replay(clip, 7)).unwrap()).unwrap();
            assert_eq!(decoded.clip.channels, channels);
            assert_eq!(
                decoded.clip.samples.len(),
                frames * usize::from(channels),
                "{frames} frames, {channels} ch"
            );
        }
    }
}

#[test]
fn bad_clips_are_refused() {
    let three = AudioClip {
        channels: 3,
        samples: vec![0; 3],
    };
    assert!(matches!(
        encode_narration(&replay(three, 0)),
        Err(MediaError::InvalidSettings(_))
    ));
    let partial = AudioClip {
        channels: 2,
        samples: vec![0; 3],
    };
    assert!(matches!(
        encode_narration(&replay(partial, 0)),
        Err(MediaError::AudioLength { .. })
    ));
}

#[test]
fn stream_layout_follows_rfc_7845() {
    let clip = simulated_narration(ms(2_345), 1);
    let length = clip.samples.len() as u64;
    let bytes = encode_narration(&replay(clip, 250)).unwrap();
    let pages = all_pages(&bytes);
    assert!(pages.len() >= 5, "{} pages", pages.len());
    for (index, page) in pages.iter().enumerate() {
        assert_eq!(page.sequence, index as u32);
        assert_eq!(page.serial, pages[0].serial);
    }

    // OpusHead alone on the first page, beginning of stream.
    assert_eq!(pages[0].flags, FLAG_BOS);
    assert_eq!(pages[0].granule, 0);
    assert_eq!(pages[0].lacing, &[19]);
    let head = OpusHead::parse(pages[0].body).unwrap();
    assert_eq!((head.channels, head.input_sample_rate), (1, 48_000));
    assert_eq!(pages[0].body[HEAD_MAPPING], 0);
    // OpusTags alone on the second page.
    assert_eq!((pages[1].flags, pages[1].granule), (0, 0));
    assert_eq!(pages[1].lacing.len(), 1);
    let tags = pages[1].body;
    assert_eq!(&tags[..8], b"OpusTags");
    assert_eq!(&tags[8..12], &7u32.to_le_bytes());
    assert_eq!(&tags[12..19], b"Ardoise");
    assert_eq!(
        comments(tags),
        [
            "ARDOISE_BASE=replay",
            "ARDOISE_SPEED=1",
            "ARDOISE_MAX_PAUSE_MS=none",
            "ARDOISE_START_MS=250"
        ]
    );

    // Audio: each page's granule is the end of its last packet, about a second per page; the
    // last page ends the stream and trims the encoder's padding.
    let mut decoder = OpusPacketDecoder::new(1).unwrap();
    let mut decoded = 0u64;
    let (audio, last) = pages[2..].split_at(pages.len() - 3);
    for page in audio {
        assert_eq!(page.flags, 0);
        // 50 packets of 20 ms (a packet over 254 bytes takes two lacing values).
        let mut reader = PacketReader::default();
        assert_eq!(
            reader
                .push(&Page {
                    sequence: 0,
                    ..*page
                })
                .unwrap(),
            PACKETS_PER_PAGE
        );
        for index in 0..reader.len() {
            decoded += decoder.decode(reader.packet(index).unwrap()).unwrap() as u64;
        }
        assert_eq!(page.granule, decoded);
    }
    let last = &last[0];
    assert_eq!(last.flags, FLAG_EOS);
    let mut reader = PacketReader::default();
    reader
        .push(&Page {
            sequence: 0,
            ..*last
        })
        .unwrap();
    for index in 0..reader.len() {
        decoded += decoder.decode(reader.packet(index).unwrap()).unwrap() as u64;
    }
    let pre_skip = u64::from(head.pre_skip);
    assert_eq!(last.granule, pre_skip + length);
    assert!(decoded > last.granule && decoded - last.granule < 120);
}

#[test]
fn pages_hold_a_second_or_255_lacing_values() {
    let packet = |len: usize, index: u64| OpusPacket {
        data: vec![0; len],
        start: index * 960,
        samples: 960,
    };
    let small: Vec<OpusPacket> = (0..120).map(|i| packet(100, i)).collect();
    let sizes: Vec<usize> = group_packets(&small).iter().map(Vec::len).collect();
    assert_eq!(sizes, [50, 50, 20]);
    // 4000-byte packets take 16 lacing values: 15 fit on a page.
    let large: Vec<OpusPacket> = (0..40).map(|i| packet(4000, i)).collect();
    let sizes: Vec<usize> = group_packets(&large).iter().map(Vec::len).collect();
    assert_eq!(sizes, [15, 15, 10]);
    assert!(group_packets(&[]).is_empty());
}

/// An Ogg file as another muxer could write it: a second logical stream around the Opus one,
/// `OpusTags` from another vendor continued over two pages, small audio pages, and the given
/// output gain.
fn foreign_stream(clip: &AudioClip, gain: i16) -> Vec<u8> {
    const OTHER: u32 = 7;
    const OPUS: u32 = 0xBEEF;
    let mut encoder = OpusEncoder::new(clip.channels, 96_000).unwrap();
    let mut head = encoder.head();
    head[HEAD_GAIN..HEAD_GAIN + 2].copy_from_slice(&gain.to_le_bytes());
    let pre_skip = u64::from(encoder.pre_skip());
    let mut packets = encoder.push(&clip.samples).unwrap();
    packets.extend(encoder.finish().unwrap());

    let mut tags = OPUS_TAGS.to_vec();
    let vendor = "libopus 1.5.2";
    tags.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
    tags.extend_from_slice(vendor.as_bytes());
    let description = format!("DESCRIPTION={}", "é".repeat(300));
    let comments = ["TITLE=Leçon", &description, "ARTIST=Someone"];
    tags.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for comment in comments {
        tags.extend_from_slice(&(comment.len() as u32).to_le_bytes());
        tags.extend_from_slice(comment.as_bytes());
    }
    assert!((510..765).contains(&tags.len()), "{}", tags.len());

    let mut file = Vec::new();
    file.extend(raw_page(FLAG_BOS, 0, OTHER, 0, &[8], b"fishead\0"));
    file.extend(raw_page(FLAG_BOS, 0, OPUS, 0, &[19], &head));
    file.extend(raw_page(0, NO_GRANULE, OPUS, 1, &[255, 255], &tags[..510]));
    file.extend(raw_page(0, 0, OTHER, 1, &[3], b"abc"));
    let rest = (tags.len() - 510) as u8;
    file.extend(raw_page(FLAG_CONTINUED, 0, OPUS, 2, &[rest], &tags[510..]));
    let chunks: Vec<&[OpusPacket]> = packets.chunks(7).collect();
    for (index, chunk) in chunks.iter().enumerate() {
        let sequence = 3 + index as u32;
        let last = index + 1 == chunks.len();
        let lacing: Vec<u8> = chunk
            .iter()
            .flat_map(|p| {
                let mut values = vec![255u8; p.data.len() / 255];
                values.push((p.data.len() % 255) as u8);
                values
            })
            .collect();
        let body: Vec<u8> = chunk.iter().flat_map(|p| p.data.clone()).collect();
        let granule = if last {
            pre_skip + (clip.samples.len() / usize::from(clip.channels)) as u64
        } else {
            chunk
                .last()
                .map(|p| p.start + u64::from(p.samples))
                .unwrap()
        };
        let flags = if last { FLAG_EOS } else { 0 };
        if index == 1 {
            file.extend(raw_page(FLAG_EOS, 0, OTHER, 2, &[1], b"z"));
        }
        file.extend(raw_page(flags, granule, OPUS, sequence, &lacing, &body));
    }
    file
}

#[test]
fn a_foreign_ogg_opus_file_decodes_with_defaults() {
    let clip = simulated_narration(ms(700), 2);
    let decoded = decode_narration(&foreign_stream(&clip, 0)).unwrap();
    assert_eq!(decoded.clip.channels, 2);
    assert_eq!(decoded.clip.samples.len(), clip.samples.len());
    assert_eq!(decoded.base, NarrationBase::Replay);
    assert_eq!(decoded.options, ReplayOptions::default());
    assert_eq!(decoded.start_ms, 0);
    let snr = snr(&clip.samples, &decoded.clip.samples, 2);
    assert!(snr > 12.0, "SNR {snr:.1} dB");

    // The output gain of OpusHead is applied: -6.02 dB halves the amplitude.
    let quieter = decode_narration(&foreign_stream(&clip, -1541)).unwrap();
    let ratio = rms(&quieter.clip.samples) / rms(&decoded.clip.samples);
    assert!((ratio - 0.5).abs() < 0.01, "gain ratio {ratio}");
}

#[test]
fn tags_fall_back_to_their_defaults() {
    let read = |comments: &[&str]| parse_tags(&opus_tags(comments));
    let defaults = Tags {
        base: NarrationBase::Replay,
        options: ReplayOptions::default(),
        start_ms: 0,
    };
    assert_eq!(read(&[]), defaults);
    assert_eq!(
        read(&[
            "ARDOISE_BASE=other",
            "ARDOISE_SPEED=100",
            "ARDOISE_MAX_PAUSE_MS=-5",
            "ARDOISE_START_MS=x",
        ]),
        defaults
    );
    for speed in ["NaN", "inf", "-1", "0.2", "16.5", "", "fast"] {
        assert_eq!(
            read(&[&format!("ARDOISE_SPEED={speed}")]),
            defaults,
            "{speed}"
        );
    }
    // Keys are case-insensitive (Vorbis comments); the first of a key counts.
    assert_eq!(
        read(&[
            "ardoise_base=Note",
            "Ardoise_Start_Ms=-12",
            "ARDOISE_START_MS=99",
            "ARDOISE_SPEED=2",
        ]),
        Tags {
            base: NarrationBase::Note,
            options: ReplayOptions::default(),
            start_ms: -12,
        }
    );
    assert_eq!(
        read(&[
            "ARDOISE_SPEED= 0.25 ",
            "ARDOISE_MAX_PAUSE_MS=750",
            "NOEQUALSIGN",
            "=x"
        ]),
        Tags {
            base: NarrationBase::Replay,
            options: ReplayOptions {
                speed: 0.25,
                max_pause: Some(ms(750)),
            },
            start_ms: 0,
        }
    );

    // A comment list cut short keeps what precedes the cut; a huge count is not trusted.
    let mut cut = opus_tags(&["ARDOISE_SPEED=4", "ARDOISE_START_MS=10"]);
    cut.truncate(cut.len() - 3);
    assert_eq!(parse_tags(&cut).options.speed, 4.0);
    assert_eq!(parse_tags(&cut).start_ms, 0);
    let mut huge = opus_tags(&["ARDOISE_SPEED=8"]);
    let count_at = 8 + 4 + VENDOR.len();
    huge[count_at..count_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(parse_tags(&huge).options.speed, 8.0);
    for len in 0..cut.len() {
        let _ = parse_tags(&cut[..len]);
    }
    // Invalid UTF-8 is skipped.
    let mut latin = opus_tags(&["ARDOISE_START_MS=5", "X=\u{e9}", "ARDOISE_SPEED=2"]);
    let at = latin
        .windows(2)
        .position(|w| w == "\u{e9}".as_bytes())
        .unwrap();
    latin[at] = 0xFF;
    let tags = parse_tags(&latin);
    assert_eq!((tags.start_ms, tags.options.speed), (5, 2.0));

    // Through a whole stream.
    let clip = simulated_narration(ms(100), 1);
    let bytes = encode_stream(&clip, &opus_tags(&["ARDOISE_SPEED=3", "TITLE=x"])).unwrap();
    let decoded = decode_narration(&bytes).unwrap();
    assert_eq!(decoded.options.speed, 3.0);
    assert_eq!(decoded.start_ms, 0);
}

fn invalid_file(result: Result<Narration, MediaError>) -> String {
    match result {
        Err(MediaError::InvalidFile(message)) => message,
        other => panic!("expected InvalidFile, got {other:?}"),
    }
}

#[test]
fn what_is_not_ogg_opus_is_refused() {
    invalid_file(decode_narration(b""));
    invalid_file(decode_narration(b"RIFF\0\0\0\0WAVEfmt "));
    let vorbis = raw_page(FLAG_BOS | FLAG_EOS, 0, 1, 0, &[7], b"\x01vorbis");
    assert!(invalid_file(decode_narration(&vorbis)).contains("not an Ogg Opus"));

    let clip = simulated_narration(ms(200), 1);
    let valid = encode_narration(&replay(clip.clone(), 0)).unwrap();
    let head_page_len = 27 + 1 + 19;
    let with_head = |change: &dyn Fn(&mut [u8])| {
        let mut bytes = valid.clone();
        change(&mut bytes[27 + 1..head_page_len]);
        fix_crc(&mut bytes[..head_page_len]);
        decode_narration(&bytes)
    };
    assert!(invalid_file(with_head(&|head| head[9] = 3)).contains("mono and stereo"));
    assert!(invalid_file(with_head(&|head| head[18] = 1)).contains("mapping family"));
    invalid_file(with_head(&|head| head[9] = 0));
    invalid_file(with_head(&|head| head[8] = 0x10));

    // The end-of-stream page is required.
    let pages = all_pages(&valid);
    let last_len = 27 + pages.last().unwrap().lacing.len() + pages.last().unwrap().body.len();
    assert!(invalid_file(decode_narration(&valid[..valid.len() - last_len])).contains("truncated"));

    // The second packet must be OpusTags.
    let no_tags = encode_stream(&clip, b"OpusTagz").unwrap();
    assert!(invalid_file(decode_narration(&no_tags)).contains("OpusTags"));

    // A file without any audio page.
    let mut writer = PageWriter::new(3);
    writer.page(&[&encoder_head(1, 312)], 0, FLAG_BOS).unwrap();
    writer.page(&[&opus_tags(&[])], 0, FLAG_EOS).unwrap();
    invalid_file(decode_narration(&writer.into_bytes()));

    // Zero-length packets are refused (a decoder would conceal 120 ms of loss for each).
    let mut writer = PageWriter::new(3);
    writer.page(&[&encoder_head(1, 312)], 0, FLAG_BOS).unwrap();
    writer.page(&[&opus_tags(&[])], 0, 0).unwrap();
    writer.page(&[b"", b""], 1000, FLAG_EOS).unwrap();
    invalid_file(decode_narration(&writer.into_bytes()));
}

#[test]
fn the_crc_is_checked() {
    let valid = encode_narration(&replay(simulated_narration(ms(500), 1), 0)).unwrap();
    let mut damaged = valid.clone();
    let at = damaged.len() - 10;
    damaged[at] ^= 0x01;
    assert!(invalid_file(decode_narration(&damaged)).contains("CRC"));
    // With the CRC fixed, the page is read again (its sound is simply a little different).
    assert!(fix_all_crcs(&mut damaged));
    let _ = decode_narration(&damaged);
}

#[test]
fn every_truncation_is_an_error() {
    let bytes = encode_narration(&replay(simulated_narration(ms(150), 2), -3)).unwrap();
    for len in 0..bytes.len() {
        assert!(
            decode_narration(&bytes[..len]).is_err(),
            "prefix of {len} bytes"
        );
    }
    assert!(decode_narration(&bytes).is_ok());
}

#[test]
fn damaged_bytes_never_panic() {
    let mut rng = Lcg::new(2026);
    let valid = encode_narration(&replay(simulated_narration(ms(200), 1), 0)).unwrap();
    let mut errors = 0;
    for _ in 0..3000 {
        let mut bytes = valid.clone();
        for _ in 0..1 + rng.below(3) {
            let at = rng.below(bytes.len() as u64) as usize;
            bytes[at] ^= 1 << rng.below(8);
        }
        if decode_narration(&bytes).is_err() {
            errors += 1;
        }
    }
    // The CRC catches every damaged page.
    assert_eq!(errors, 3000);

    // Damage behind a right CRC reaches the parser, the tags and the Opus decoder.
    let mut decoded = 0;
    for _ in 0..400 {
        let mut bytes = valid.clone();
        for _ in 0..1 + rng.below(4) {
            let at = rng.below(bytes.len() as u64) as usize;
            bytes[at] = rng.below(256) as u8;
        }
        if fix_all_crcs(&mut bytes) && decode_narration(&bytes).is_ok() {
            decoded += 1;
        }
    }
    assert!(decoded > 0, "no damaged file decoded at all");

    // Random bytes, some of them shaped like pages.
    for round in 0..300 {
        let len = rng.below(300) as usize;
        let mut bytes: Vec<u8> = (0..len).map(|_| rng.below(256) as u8).collect();
        if round % 2 == 0 && bytes.len() >= 27 {
            bytes[..4].copy_from_slice(b"OggS");
            bytes[4] = 0;
            fix_all_crcs(&mut bytes);
        }
        let _ = decode_narration(&bytes);
    }
}

/// Speed of `audio.opus` for a minute of narration:
/// `cargo test --release -p ardoise-media narration_speed -- --ignored --nocapture`.
#[test]
#[ignore = "benchmark: run in release"]
fn narration_speed() {
    let clip = simulated_narration(Duration::from_secs(60), 1);
    let started = std::time::Instant::now();
    let bytes = encode_narration(&replay(clip, 0)).unwrap();
    let encoding = started.elapsed();
    let started = std::time::Instant::now();
    let decoded = decode_narration(&bytes).unwrap();
    let decoding = started.elapsed();
    let started = std::time::Instant::now();
    let wav = wav_bytes(&decoded.clip);
    println!(
        "1 min mono: {} kB, encoded in {:.2} s, decoded in {:.2} s, WAV ({} kB) in {:.3} s",
        bytes.len() / 1000,
        encoding.as_secs_f64(),
        decoding.as_secs_f64(),
        wav.len() / 1000,
        started.elapsed().as_secs_f64()
    );
}

#[test]
fn random_opus_packets_never_panic() {
    // Well-formed pages around random packets: everything reaches the Opus decoder. Most
    // packets have a one-frame table of contents (code 0), which any length satisfies, so
    // the SILK and CELT decoders run on random bits.
    let mut rng = Lcg::new(77);
    let mut decoded = 0;
    for round in 0..200 {
        let channels = 1 + (round % 2) as u16;
        let mut writer = PageWriter::new(1);
        writer
            .page(&[&encoder_head(channels, 312)], 0, FLAG_BOS)
            .unwrap();
        writer.page(&[&opus_tags(&[])], 0, 0).unwrap();
        let packets: Vec<Vec<u8>> = (0..10)
            .map(|_| {
                let len = 1 + rng.below(if round % 4 == 0 { 8 } else { 600 }) as usize;
                let mut packet: Vec<u8> = (0..len).map(|_| rng.below(256) as u8).collect();
                if rng.below(8) != 0 {
                    packet[0] &= !3;
                }
                packet
            })
            .collect();
        let refs: Vec<&[u8]> = packets.iter().map(Vec::as_slice).collect();
        let granule = rng.next() >> rng.below(64);
        writer.page(&refs, granule, FLAG_EOS).unwrap();
        let bytes = writer.into_bytes();
        if let Ok(narration) = decode_narration(&bytes) {
            decoded += 1;
            // 10 packets decode to 1.2 s at most.
            assert!(narration.clip.samples.len() <= 10 * 5760 * usize::from(channels));
        }
    }
    assert!(
        decoded > 50,
        "only {decoded} streams of random packets decoded"
    );
}

fn constant(channels: u16, frames: usize, value: i16) -> AudioClip {
    AudioClip {
        channels,
        samples: (0..frames * usize::from(channels))
            .map(|i| value + (i % usize::from(channels)) as i16)
            .collect(),
    }
}

#[test]
fn merge_places_both_clips_on_one_track() {
    // 10 ms at 0 ms and 10 ms at 20 ms: 10 ms of silence between.
    let merged = replay(constant(1, 480, 100), 0).merge(replay(constant(1, 480, 200), 20));
    assert_eq!(merged.start_ms, 0);
    assert_eq!(merged.clip.channels, 1);
    let s = &merged.clip.samples;
    assert_eq!(s.len(), 1440);
    assert!(s[..480].iter().all(|&v| v == 100));
    assert!(s[480..960].iter().all(|&v| v == 0));
    assert!(s[960..].iter().all(|&v| v == 200));
    assert_eq!(merged.end_ms(), 30);

    // Overlap: the later recording wins where both have sound.
    let merged = replay(constant(1, 960, 100), 0).merge(replay(constant(1, 240, 200), 5));
    let s = &merged.clip.samples;
    assert_eq!(s.len(), 960);
    assert!(s[..240].iter().all(|&v| v == 100));
    assert!(s[240..480].iter().all(|&v| v == 200));
    assert!(s[480..].iter().all(|&v| v == 100));

    // A later recording placed before the first: the track starts with it.
    let merged = replay(constant(1, 480, 100), 50).merge(replay(constant(1, 480, 200), -10));
    assert_eq!(merged.start_ms, -10);
    assert_eq!(merged.clip.samples.len(), 60 * 48 + 480);
    assert_eq!(merged.clip.samples[0], 200);
    assert_eq!(merged.clip.samples[60 * 48], 100);

    // Mono and stereo: stereo, the mono clip on both sides.
    let merged = replay(constant(1, 48, 100), 0).merge(replay(constant(2, 48, 200), 1));
    assert_eq!(merged.clip.channels, 2);
    assert_eq!(&merged.clip.samples[..4], &[100, 100, 100, 100]);
    assert_eq!(&merged.clip.samples[96..100], &[200, 201, 200, 201]);
    assert_eq!(merged.clip.samples.len(), 2 * 96);
    let merged = replay(constant(2, 48, 200), 0).merge(replay(constant(1, 48, 100), 1));
    assert_eq!(&merged.clip.samples[..2], &[200, 201]);
    assert_eq!(&merged.clip.samples[96..98], &[100, 100]);

    // Base and options come from the first; empty clips add nothing.
    let first = narration(
        constant(1, 48, 1),
        NarrationBase::Note,
        ReplayOptions::default(),
        3,
    );
    let empty = replay(
        AudioClip {
            channels: 2,
            samples: vec![],
        },
        -1000,
    );
    assert_eq!(first.clone().merge(empty.clone()), first);
    assert_eq!(empty.merge(first.clone()), first);
    assert_eq!(
        first.clone().merge(replay(constant(1, 48, 2), 3)).base,
        NarrationBase::Note
    );

    // Hours apart: only the later one.
    let later = replay(constant(1, 48, 7), 5 * 3600 * 1000);
    assert_eq!(first.merge(later.clone()), later);
}

#[test]
fn wav_header_and_data() {
    let stereo = AudioClip {
        channels: 2,
        samples: vec![1, -2, 3, -4, 5],
    };
    let wav = wav_bytes(&stereo);
    let le16 = |at: usize| u16::from_le_bytes([wav[at], wav[at + 1]]);
    let le32 = |at: usize| u32::from_le_bytes([wav[at], wav[at + 1], wav[at + 2], wav[at + 3]]);
    assert_eq!(&wav[0..4], b"RIFF");
    assert_eq!(le32(4), 36 + 8);
    assert_eq!(&wav[8..16], b"WAVEfmt ");
    assert_eq!(le32(16), 16);
    assert_eq!(le16(20), 1); // PCM
    assert_eq!(le16(22), 2);
    assert_eq!(le32(24), 48_000);
    assert_eq!(le32(28), 192_000);
    assert_eq!(le16(32), 4);
    assert_eq!(le16(34), 16);
    assert_eq!(&wav[36..40], b"data");
    assert_eq!(le32(40), 8); // the partial last frame is left out
    assert_eq!(&wav[44..], &[1, 0, 0xFE, 0xFF, 3, 0, 0xFC, 0xFF]);

    let mono = wav_bytes(&simulated_narration(ms(1000), 1));
    assert_eq!(mono.len(), 44 + 96_000);
    assert_eq!(u16::from_le_bytes([mono[22], mono[23]]), 1);
    assert_eq!(
        u32::from_le_bytes([mono[28], mono[29], mono[30], mono[31]]),
        96_000
    );
    assert_eq!(wav_bytes(&AudioClip::default()).len(), 44);
}
