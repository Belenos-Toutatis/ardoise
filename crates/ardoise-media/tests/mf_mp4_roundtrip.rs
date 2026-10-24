//! Media Foundation spike (ROADMAP M6): writes a 3-second 1280×720 30 fps H.264 + AAC MP4 with
//! `Mp4Writer`, then reads it back independently through `IMFSourceReader` (`probe_mp4`):
//! container duration, codecs, dimensions, decoded frame count, audio format and duration,
//! tone frequency and a few decoded pixels. Run with `--nocapture` to see the tables.
//!
//! Set `ARDOISE_MF_SPIKE_KEEP` to a folder to keep the files written (for checking them in
//! Windows Explorer or a player).
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::time::{Duration, Instant};

use ardoise_media::{
    AudioSettings, Codec, MediaError, Mp4Probe, Mp4Settings, Mp4Summary, Mp4Writer, ProbeOptions,
    available_encoders, nv12, probe_mp4,
};

const WIDTH: u32 = 1280;
const HEIGHT: u32 = 720;
const FPS: u32 = 30;
const FRAMES: u32 = 90;
const SECONDS: f64 = 3.0;
const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: u16 = 2;
const TONE_HZ: f64 = 440.0;

const BACKGROUND: [u8; 3] = [250, 248, 240];
const INK: [u8; 3] = [20, 50, 160];
const MARKER: [u8; 3] = [200, 30, 30];
/// Frame decoded back and checked pixel by pixel.
const CHECKED_FRAME: u32 = 45;

/// Frame `index` of the animation: a horizontal stroke growing from left to right, and a red
/// square sliding below it.
fn draw_frame(canvas: &mut [u8], index: u32) {
    for px in canvas.as_chunks_mut::<4>().0 {
        *px = [BACKGROUND[0], BACKGROUND[1], BACKGROUND[2], 255];
    }
    let length = 1120 * (index + 1) / FRAMES;
    fill_rect(canvas, 80, 352, length, 16, INK);
    let marker_x = 80 + 1080 * index / (FRAMES - 1);
    fill_rect(canvas, marker_x, 540, 40, 40, MARKER);
}

fn fill_rect(canvas: &mut [u8], x: u32, y: u32, w: u32, h: u32, rgb: [u8; 3]) {
    for row in y..(y + h).min(HEIGHT) {
        for col in x..(x + w).min(WIDTH) {
            let i = ((row * WIDTH + col) * 4) as usize;
            canvas[i..i + 3].copy_from_slice(&rgb);
        }
    }
}

/// Audio frames `[first, first + count)` of a 440 Hz stereo tone (right channel a quarter
/// period behind), interleaved.
fn tone(first: u64, count: u64) -> Vec<i16> {
    let mut samples = Vec::with_capacity(count as usize * usize::from(CHANNELS));
    for n in first..first + count {
        let t = n as f64 / f64::from(SAMPLE_RATE);
        let phase = 2.0 * std::f64::consts::PI * TONE_HZ * t;
        samples.push((phase.sin() * 8000.0) as i16);
        samples.push((phase.cos() * 8000.0) as i16);
    }
    samples
}

/// Time spent in the writer (drawing excluded).
struct Timings {
    create: Duration,
    push: Duration,
    finish: Duration,
}

impl Timings {
    fn total(&self) -> Duration {
        self.create + self.push + self.finish
    }
}

/// Writes the animation and the tone, timing only the writer.
fn write_spike(path: &Path, hardware: bool) -> (Mp4Summary, Timings) {
    let mut settings =
        Mp4Settings::new(WIDTH, HEIGHT, FPS).with_audio(AudioSettings::new(SAMPLE_RATE, CHANNELS));
    settings.hardware_acceleration = hardware;
    let frames_per_video_frame = u64::from(SAMPLE_RATE / FPS);
    let mut canvas = vec![0u8; settings.rgba_frame_len()];

    let start = Instant::now();
    let mut writer = Mp4Writer::create(path, &settings).expect("create the MP4 writer");
    let create = start.elapsed();
    let mut push = Duration::ZERO;
    for index in 0..FRAMES {
        draw_frame(&mut canvas, index);
        let audio = tone(
            u64::from(index) * frames_per_video_frame,
            frames_per_video_frame,
        );
        let start = Instant::now();
        // Each frame's audio first, then the frame (see `Mp4Writer`).
        writer.push_audio(&audio).expect("push audio");
        let timestamp = Duration::from_nanos(u64::from(index) * 1_000_000_000 / u64::from(FPS));
        writer.push_frame(&canvas, timestamp).expect("push frame");
        push += start.elapsed();
    }
    let start = Instant::now();
    let summary = writer.finish().expect("finish the MP4 file");
    let finish = start.elapsed();
    (
        summary,
        Timings {
            create,
            push,
            finish,
        },
    )
}

/// Top-level MP4 boxes, in file order.
fn top_level_boxes(path: &Path) -> Vec<String> {
    let data = std::fs::read(path).expect("read the MP4 file");
    let mut boxes = Vec::new();
    let mut pos = 0usize;
    while pos + 8 <= data.len() {
        let size32 = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        let kind = String::from_utf8_lossy(&data[pos + 4..pos + 8]).into_owned();
        let size = match size32 {
            0 => data.len() - pos,
            1 => u64::from_be_bytes(data[pos + 8..pos + 16].try_into().unwrap()) as usize,
            n => n,
        };
        boxes.push(kind);
        if size < 8 {
            break;
        }
        pos += size;
    }
    boxes
}

/// Frequency of the left channel, from upward zero crossings away from the edges.
fn left_frequency(samples: &[i16], channels: usize, rate: u32) -> f64 {
    let left: Vec<i16> = samples.iter().step_by(channels).copied().collect();
    let margin = rate as usize / 10;
    let window = &left[margin..left.len() - margin];
    let crossings = window.windows(2).filter(|w| w[0] < 0 && w[1] >= 0).count();
    crossings as f64 * f64::from(rate) / window.len() as f64
}

fn colour_error(actual: [u8; 4], expected: [u8; 3]) -> u8 {
    (0..3)
        .map(|i| actual[i].abs_diff(expected[i]))
        .max()
        .unwrap_or(0)
}

fn secs(d: Duration) -> String {
    format!("{:.3} s", d.as_secs_f64())
}

fn print_and_check(
    label: &str,
    path: &Path,
    summary: &Mp4Summary,
    timings: &Timings,
    probe: &Mp4Probe,
) {
    let video = probe.video.as_ref().expect("a video track");
    let audio = probe.audio.as_ref().expect("an audio track");
    let samples = probe.audio_samples.as_ref().expect("decoded audio");
    let frame = probe.captured_frame.as_ref().expect("decoded frame");
    let frequency = left_frequency(samples, usize::from(audio.channels), audio.sample_rate);
    let size = std::fs::metadata(path).expect("file metadata").len();
    let boxes = top_level_boxes(path);
    let fps = f64::from(FRAMES) / timings.total().as_secs_f64();
    // The stroke ends at x = 652 in frame 45 (640 in frame 44, 664 in frame 46) and the
    // square covers x = 626..666, so these pixels identify frame 45 exactly.
    let pixel_checks = [
        ("background (640,200)", (640, 200), BACKGROUND),
        ("stroke (600,360)", (600, 360), INK),
        ("stroke end (646,360)", (646, 360), INK),
        ("past stroke (658,360)", (658, 360), BACKGROUND),
        ("square (646,560)", (646, 560), MARKER),
    ];

    println!();
    println!("=== {label} ===");
    println!("video encoder used : {}", summary.video_encoder);
    println!(
        "writer time        : {} for {FRAMES} frames + 3 s of audio = {fps:.0} frames/s",
        secs(timings.total()),
    );
    println!(
        "                     (create {}, push {}, finish {}; drawing excluded)",
        secs(timings.create),
        secs(timings.push),
        secs(timings.finish),
    );
    println!("file               : {size} bytes, top-level boxes {boxes:?}");
    println!(
        "{:<32}| {:<16}| read back (IMFSourceReader)",
        "property", "expected"
    );
    println!("{:-<32}+{:-<17}+{:-<28}", "", "", "");
    let row = |name: &str, expected: String, read: String| {
        println!("{name:<32}| {expected:<16}| {read}");
    };
    row("container duration", "3.000 s".into(), secs(probe.duration));
    row("video codec", "H.264".into(), video.codec.to_string());
    row(
        "dimensions",
        format!("{WIDTH}x{HEIGHT}"),
        format!("{}x{}", video.width, video.height),
    );
    row(
        "video frames decoded",
        FRAMES.to_string(),
        video.frames.to_string(),
    );
    row(
        "first frame timestamp",
        "0.000 s".into(),
        secs(video.first_timestamp),
    );
    row(
        "last frame timestamp",
        "2.967 s".into(),
        secs(video.last_timestamp),
    );
    row("audio codec", "AAC".into(), audio.codec.to_string());
    row(
        "audio channels",
        CHANNELS.to_string(),
        audio.channels.to_string(),
    );
    row(
        "audio sample rate",
        format!("{SAMPLE_RATE} Hz"),
        format!("{} Hz", audio.sample_rate),
    );
    row(
        "audio duration (decoded)",
        "3.000 s".into(),
        format!("{} ({} frames)", secs(audio.duration), audio.frames),
    );
    row(
        "tone frequency (left)",
        "440 Hz".into(),
        format!("{frequency:.1} Hz"),
    );
    for (name, (x, y), expected) in pixel_checks {
        let px = frame.pixel(x, y).expect("pixel inside the frame");
        row(
            &format!("frame {CHECKED_FRAME} {name}"),
            format!("{expected:?}"),
            format!("{:?} (max error {})", &px[..3], colour_error(px, expected)),
        );
    }

    // Writer's own counters (not the proof, just consistency).
    assert_eq!(summary.video_frames, u64::from(FRAMES));
    assert_eq!(summary.audio_frames, u64::from(SAMPLE_RATE) * 3);

    // Independent read-back.
    assert!(
        (probe.duration.as_secs_f64() - SECONDS).abs() < 0.05,
        "container duration"
    );
    assert_eq!(video.codec, Codec::H264);
    assert_eq!((video.width, video.height), (WIDTH, HEIGHT));
    assert!(
        video.frames.abs_diff(u64::from(FRAMES)) <= 1,
        "decoded frames"
    );
    // Frames start at 0 and keep their timestamps (no B-frame shift against the audio).
    assert!(
        video.first_timestamp < Duration::from_millis(1),
        "first frame time"
    );
    let last = video.last_timestamp.as_secs_f64();
    assert!((last - 89.0 / 30.0).abs() < 0.005, "last frame time {last}");
    assert_eq!(audio.codec, Codec::Aac);
    assert_eq!(audio.channels, CHANNELS);
    assert_eq!(audio.sample_rate, SAMPLE_RATE);
    assert!(
        (audio.duration.as_secs_f64() - SECONDS).abs() < 0.05,
        "audio duration"
    );
    assert!(
        (frequency - TONE_HZ).abs() < 2.0,
        "tone frequency {frequency}"
    );
    assert_eq!((frame.width, frame.height), (WIDTH, HEIGHT));
    for (name, (x, y), expected) in pixel_checks {
        let px = frame.pixel(x, y).expect("pixel inside the frame");
        assert!(
            colour_error(px, expected) <= 12,
            "{name}: {px:?} vs {expected:?}"
        );
    }
    let moov = boxes.iter().position(|b| b == "moov");
    let mdat = boxes.iter().position(|b| b == "mdat");
    assert!(moov.is_some() && mdat.is_some(), "boxes {boxes:?}");
}

#[test]
fn writes_h264_aac_mp4_and_reads_it_back() {
    let encoders = available_encoders().expect("list Media Foundation encoders");
    println!();
    println!("H.264 encoders registered with Media Foundation:");
    for encoder in &encoders.h264 {
        println!("  - {encoder}");
    }
    println!("AAC encoders registered with Media Foundation:");
    for encoder in &encoders.aac {
        println!("  - {encoder}");
    }
    assert!(!encoders.h264.is_empty(), "no H.264 encoder");
    assert!(!encoders.aac.is_empty(), "no AAC encoder");

    // Share of the colour conversion in the writer time.
    let mut canvas = vec![0u8; (WIDTH * HEIGHT * 4) as usize];
    draw_frame(&mut canvas, CHECKED_FRAME);
    let mut nv12 = vec![0u8; nv12::nv12_len(WIDTH as usize, HEIGHT as usize)];
    let start = Instant::now();
    for _ in 0..30 {
        nv12::rgba_to_nv12(&canvas, WIDTH as usize, HEIGHT as usize, &mut nv12).unwrap();
    }
    println!(
        "RGBA->NV12 conversion alone: {:.2} ms per 1280x720 frame",
        start.elapsed().as_secs_f64() * 1000.0 / 30.0
    );

    let dir = tempfile::tempdir().expect("temporary folder");
    let keep = std::env::var_os("ARDOISE_MF_SPIKE_KEEP");
    for (hardware, label, name) in [
        (true, "hardware encoder allowed", "spike-hardware.mp4"),
        (false, "software encoder forced", "spike-software.mp4"),
    ] {
        // Written under a temporary name, then renamed, as an atomic export would do: the
        // container type must not depend on the extension.
        let partial = dir.path().join(format!("{name}.partial"));
        let path = dir.path().join(name);
        let (summary, timings) = write_spike(&partial, hardware);
        std::fs::rename(&partial, &path).expect("rename the finished file");
        if let Some(keep) = &keep {
            std::fs::copy(&path, Path::new(keep).join(name)).expect("keep a copy");
        }

        let probe = probe_mp4(
            &path,
            ProbeOptions {
                capture_frame: Some(u64::from(CHECKED_FRAME)),
                capture_audio: true,
            },
        )
        .expect("read the MP4 back");
        print_and_check(label, &path, &summary, &timings, &probe);
        if !hardware {
            assert!(!summary.video_encoder.hardware, "software encoder expected");
        }
    }
}

#[test]
fn rejects_misuse() {
    let dir = tempfile::tempdir().expect("temporary folder");
    let path = dir.path().join("misuse.mp4");
    let settings = Mp4Settings::new(64, 64, 10);
    let mut writer = Mp4Writer::create(&path, &settings).expect("create");
    let frame = vec![255u8; settings.rgba_frame_len()];

    assert!(matches!(
        writer.push_frame(&frame[1..], Duration::ZERO),
        Err(MediaError::FrameSize { .. })
    ));
    assert!(matches!(
        writer.push_audio(&[0; 4]),
        Err(MediaError::NoAudioTrack)
    ));
    writer
        .push_frame(&frame, Duration::from_millis(100))
        .expect("first frame");
    assert!(matches!(
        writer.push_frame(&frame, Duration::from_millis(100)),
        Err(MediaError::TimestampOrder { .. })
    ));
    writer
        .push_frame(&frame, Duration::from_millis(200))
        .expect("second frame");
    let summary = writer.finish().expect("finish");
    assert_eq!(summary.video_frames, 2);
    assert_eq!(summary.audio_frames, 0);

    let empty = Mp4Writer::create(&dir.path().join("empty.mp4"), &settings).expect("create");
    assert!(matches!(
        empty.finish(),
        Err(MediaError::InvalidSettings(_))
    ));

    let stereo = Mp4Settings::new(64, 64, 10).with_audio(AudioSettings::new(48_000, 2));
    let mut writer = Mp4Writer::create(&dir.path().join("odd.mp4"), &stereo).expect("create");
    assert!(matches!(
        writer.push_audio(&[0; 3]),
        Err(MediaError::AudioLength {
            len: 3,
            channels: 2
        })
    ));
}

/// A narration that stops seconds before the video: without a stream tick on the audio
/// stream, the sink writer waits for audio and this took about 40 s instead of under 1 s.
#[test]
fn a_narration_shorter_than_the_video_does_not_stall_the_writer() {
    let dir = tempfile::tempdir().expect("temporary folder");
    let path = dir.path().join("short-narration.mp4");
    let settings = Mp4Settings::new(640, 360, 30).with_audio(AudioSettings::new(SAMPLE_RATE, 1));
    let mut writer = Mp4Writer::create(&path, &settings).expect("create");
    let frame = vec![200u8; settings.rgba_frame_len()];
    let started = Instant::now();
    // 3 s of narration, pushed with the first frames; then 8 s of video.
    writer
        .push_audio(&vec![1000i16; 3 * SAMPLE_RATE as usize])
        .expect("audio");
    for k in 0..240u64 {
        writer
            .push_frame(&frame, Duration::from_micros(k * 1_000_000 / 30))
            .expect("frame");
    }
    let summary = writer.finish().expect("finish");
    let elapsed = started.elapsed();
    println!("short narration: {elapsed:?} for 8 s of 640x360 video after 3 s of narration");
    assert_eq!(summary.video_frames, 240);
    assert_eq!(summary.audio_frames, 3 * u64::from(SAMPLE_RATE));
    assert!(
        elapsed < Duration::from_secs(15),
        "the sink writer stalled: {elapsed:?}"
    );
    let probe = probe_mp4(&path, ProbeOptions::default()).expect("probe");
    assert_eq!(probe.video.expect("video").frames, 240);
    let audio = probe.audio.expect("audio");
    assert!(
        (audio.duration.as_secs_f64() - 3.0).abs() < 0.05,
        "audio {:?}",
        audio.duration
    );
}
