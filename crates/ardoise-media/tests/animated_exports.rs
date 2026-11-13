//! The M6 end-of-milestone test (ROADMAP M6, "Critères de fin"): a reference note (synthetic
//! handwriting written with several tools, with pauses, a few seconds long) is replayed with
//! `replay::Timeline` and `replay::FrameRenderer` into the four animated formats, and every
//! file is read back by its own probe, independent of the writer:
//!
//! - MP4 through Media Foundation (H.264 + AAC) with a simulated narration: the AAC track must
//!   have the narration's length;
//! - MP4 through the OpenH264 fallback, forced (H.264 + Opus), with the same narration;
//! - GIF and WebP (no sound), WebM (AV1 + Opus) with the narration.
//!
//! For each file: duration, dimensions and frames read back, and the sound track. Run with
//! `cargo test -p ardoise-media --test animated_exports -- --nocapture` to see the table. The MP4
//! rows need Windows (Media Foundation; the OpenH264 library in the codec folder of the app,
//! `ARDOISE_CODECS_DIR` or `%LOCALAPPDATA%\Ardoise\codecs`, downloaded once by
//! `tests/openh264_download.rs`); they are skipped with a message otherwise. Set
//! `ARDOISE_SAMPLES_DIR` to keep the files.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::f32::consts::TAU;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use ardoise_core::note::{A4_LANDSCAPE, Background, Layer, Note, Origin, Paper, Template, Timing};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_media::microphone::simulated_narration;
use ardoise_media::replay::{FrameRenderer, FrameSettings, ReplayOptions, Timeline};
use ardoise_media::{
    AnimationFormat, AnimationProbe, AudioClip, CodecStatus, GifOptions, MediaError, Mp4Backend,
    Mp4Options, NARRATION_SAMPLE_RATE, OpenH264Codec, VideoSettings, VideoSink, VideoSummary,
    WebmOptions, WebpOptions, create_gif, create_mp4, create_webm, create_webp,
    media_foundation_available, probe_gif, probe_mp4_animation, probe_webm, probe_webp,
};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FPS: u32 = 15;
/// Pauses longer than this are trimmed ("supprimer les temps morts").
const MAX_PAUSE: Duration = Duration::from_millis(500);
/// The narration covers most of the replay, so its length differs from the video's, and the
/// last frames have no sound (a case the writers must handle).
const NARRATION: Duration = Duration::from_millis(4500);
/// Interval between two pen samples (the tablet reports about 200 per second).
const SAMPLE_MS: u32 = 5;

// --- The reference note -----------------------------------------------------------------

/// A cursive "word" of `letters` loops starting at (`x`, `y`) (baseline), `height` tall,
/// written from `t0` ms at a natural speed (about 180 ms per letter), pressure rising and
/// falling.
fn word(x: f32, y: f32, letters: u32, height: f32, t0: u32) -> Vec<Point> {
    let duration = letters * 180;
    let n = duration / SAMPLE_MS;
    let width = letters as f32 * height * 0.9;
    (0..=n)
        .map(|i| {
            let u = i as f32 / n as f32;
            let phase = u * letters as f32 * TAU;
            // Every third letter is tall (an l, a d...).
            let tall = if (u * letters as f32) as u32 % 3 == 1 {
                1.8
            } else {
                1.0
            };
            Point {
                x: x + width * u - 0.3 * height * phase.sin(),
                y: y - height * tall * 0.5 * (1.0 - phase.cos()),
                pressure: 0.35 + 0.55 * (u * std::f32::consts::PI).sin().sqrt(),
                t: t0 + i * SAMPLE_MS,
            }
        })
        .collect()
}

/// A straight stroke from `from` to `to`, drawn in `duration` ms from `t0`.
fn line(from: (f32, f32), to: (f32, f32), t0: u32, duration: u32) -> Vec<Point> {
    let n = (duration / SAMPLE_MS).max(1);
    (0..=n)
        .map(|i| {
            let u = i as f32 / n as f32;
            Point {
                x: from.0 + (to.0 - from.0) * u,
                y: from.1 + (to.1 - from.1) * u + 40.0 * (u * TAU).sin(),
                pressure: 0.8,
                t: t0 + i * SAMPLE_MS,
            }
        })
        .collect()
}

fn stroke(tool: Tool, color: Rgba, points: Vec<Point>) -> Stroke {
    Stroke {
        tool,
        color,
        width: tool.default_width(),
        layer: 0,
        points,
    }
}

/// The reference note: a title in fountain pen, two lines of ballpoint words, a pencil
/// sketch, a felt-tip underline, a highlighter over a word and a brush mark, with short pauses
/// between strokes and one long pause (trimmed in the replay).
fn reference_note() -> Note {
    let ink = Rgba::rgb(0x1a, 0x2a, 0x80);
    let mut strokes = Vec::new();
    let mut t = 300; // the pen comes down after 0.3 s
    let mut add = |tool: Tool, color: Rgba, points: Vec<Point>, pause: u32| {
        let end = points.last().map_or(0, |p| p.t);
        strokes.push(stroke(tool, color, points));
        end + pause
    };
    t = add(
        Tool::FountainPen,
        Rgba::BLACK,
        word(4_000.0, 4_500.0, 4, 1_400.0, t),
        150,
    );
    // A long pause (the writer thinks), trimmed to `MAX_PAUSE` in the replay.
    t = add(
        Tool::FountainPen,
        Rgba::BLACK,
        word(11_000.0, 4_500.0, 2, 1_400.0, t),
        2_000,
    );
    t = add(
        Tool::Ballpoint,
        ink,
        word(4_000.0, 8_500.0, 3, 900.0, t),
        150,
    );
    t = add(
        Tool::Ballpoint,
        ink,
        word(8_000.0, 8_500.0, 2, 900.0, t),
        150,
    );
    t = add(
        Tool::Ballpoint,
        ink,
        word(4_000.0, 11_500.0, 3, 900.0, t),
        150,
    );
    t = add(
        Tool::FeltTip,
        Rgba::rgb(0xd8, 0x00, 0x00),
        line((4_000.0, 12_200.0), (7_500.0, 12_200.0), t, 250),
        150,
    );
    t = add(
        Tool::Highlighter,
        Rgba::rgb(0xff, 0xe6, 0x2e),
        line((7_800.0, 8_000.0), (10_200.0, 8_000.0), t, 250),
        150,
    );
    t = add(
        Tool::Pencil,
        Rgba::rgb(0x3a, 0x3a, 0x3c),
        word(18_000.0, 14_000.0, 3, 2_000.0, t),
        150,
    );
    add(
        Tool::Brush,
        Rgba::rgb(0x51, 0xa7, 0xf9),
        line((17_000.0, 16_500.0), (25_000.0, 15_500.0), t, 300),
        0,
    );
    Note {
        id: "4f0d6b52-9a1e-4c3d-8b7f-2e5a9c1d3b60".into(),
        title: "Note de référence M6".into(),
        created: 1_792_843_200_000,
        modified: 1_792_843_200_000,
        page: A4_LANDSCAPE,
        background: Background {
            paper: Paper::Cream,
            template: Template::Seyes,
            texture: true,
        },
        background_image: None,
        tags: vec![],
        favorite: false,
        layers: vec![Layer {
            name: "Calque 1".into(),
            visible: true,
        }],
        strokes,
        objects: Vec::new(),
        timing: Timing::Recorded,
        origin: Origin::Ardoise,
    }
}

// --- Writing ------------------------------------------------------------------------------

/// Opens the sink of one export at a path.
type CreateSink<'a> = dyn Fn(&Path) -> Result<Box<dyn VideoSink>, MediaError> + 'a;

/// One export of the replay.
struct Export {
    name: &'static str,
    path: PathBuf,
    sink: Box<dyn VideoSink>,
    /// Time spent in the sink (encoding and writing), drawing excluded.
    spent: Duration,
    probe: fn(&Path) -> Result<AnimationProbe, MediaError>,
}

/// What the replay pushed.
struct Pushed {
    frames: u64,
    /// Frames that differ from the one before (what GIF and WebP keep).
    distinct_frames: u64,
    video_end: Duration,
    replay: Duration,
}

/// Replays `note` at `speed` into every sink: frames from the renderer, each frame's
/// narration pushed just before it, the rest of the narration after the last frame.
fn replay_into(
    note: &Note,
    speed: f64,
    settings: &VideoSettings,
    exports: &mut [Export],
    narration: &AudioClip,
) -> Pushed {
    let timeline = Timeline::new(
        note,
        &ReplayOptions {
            speed,
            max_pause: Some(MAX_PAUSE),
        },
    )
    .unwrap();
    let replay = timeline.duration();
    let mut renderer = FrameRenderer::new(timeline, &FrameSettings::for_video(settings)).unwrap();
    let frames = renderer.frame_count();
    let channels = usize::from(narration.channels);
    let sample_at = |time: Duration| {
        let samples = time.as_nanos() * u128::from(NARRATION_SAMPLE_RATE) / 1_000_000_000;
        (samples as usize * channels).min(narration.samples.len())
    };
    let (mut pushed_audio, mut distinct_frames, mut previous) = (0usize, 0u64, None::<Vec<u8>>);
    for index in 0..frames {
        let timestamp = renderer.frame_time(index);
        let next = sample_at(renderer.frame_time(index + 1));
        let audio = &narration.samples[pushed_audio..next];
        pushed_audio = next;
        let frame = renderer.render_frame(index).unwrap();
        if previous.as_deref() != Some(frame) {
            distinct_frames += 1;
            previous = Some(frame.to_vec());
        }
        for export in exports.iter_mut() {
            let started = Instant::now();
            if !audio.is_empty() {
                export.sink.push_audio(audio).unwrap();
            }
            export.sink.push_frame(frame, timestamp).unwrap();
            export.spent += started.elapsed();
        }
    }
    let rest = &narration.samples[pushed_audio..];
    for export in exports.iter_mut() {
        let started = Instant::now();
        if !rest.is_empty() {
            export.sink.push_audio(rest).unwrap();
        }
        export.spent += started.elapsed();
    }
    Pushed {
        frames,
        distinct_frames,
        video_end: renderer.frame_time(frames - 1) + Duration::from_secs(1) / settings.frame_rate,
        replay,
    }
}

fn video_settings(audio_channels: Option<u16>) -> VideoSettings {
    VideoSettings {
        width: WIDTH,
        height: HEIGHT,
        frame_rate: FPS,
        audio_channels,
    }
}

/// The OpenH264 library of the app's codec folder, if installed intact.
fn installed_openh264() -> Option<OpenH264Codec> {
    let codec = OpenH264Codec::default_location()?;
    match codec.status() {
        CodecStatus::Installed { .. } => Some(codec),
        other => {
            println!(
                "SKIPPED: MP4 through OpenH264: the library is not installed in {} ({other:?}); \
                 run `cargo test -p ardoise-media --test openh264_download -- --ignored` once",
                codec.dir().display()
            );
            None
        }
    }
}

// --- Checking -----------------------------------------------------------------------------

fn secs(duration: Duration) -> String {
    format!("{:.3} s", duration.as_secs_f64())
}

/// Asserts `actual` is within `[expected - below, expected + above]`.
fn assert_close(
    what: &str,
    actual: Duration,
    expected: Duration,
    below: Duration,
    above: Duration,
) {
    assert!(
        actual + below >= expected && actual <= expected + above,
        "{what}: {actual:?}, expected {expected:?} (-{below:?} / +{above:?})"
    );
}

#[test]
fn replays_the_reference_note_into_the_four_animated_formats() {
    let note = reference_note();
    let narration = simulated_narration(NARRATION, 1);
    assert_eq!(narration.duration(), NARRATION);
    let keep = std::env::var_os("ARDOISE_SAMPLES_DIR").map(PathBuf::from);
    let temporary = tempfile::tempdir().unwrap();
    let dir = keep.as_deref().unwrap_or(temporary.path());

    let with_sound = video_settings(Some(narration.channels));
    let silent = video_settings(None);
    let mut exports = Vec::new();
    let mut add = |name, file: &str, create: &CreateSink, probe| {
        let path = dir.join(file);
        let started = Instant::now();
        let sink = create(&path).unwrap();
        exports.push(Export {
            name,
            path,
            sink,
            spent: started.elapsed(),
            probe,
        });
    };

    let media_foundation = cfg!(windows) && media_foundation_available();
    if media_foundation {
        let options = Mp4Options {
            backend: Mp4Backend::MediaFoundation,
            ..Mp4Options::default()
        };
        add(
            "MP4",
            "reference-mf.mp4",
            &|p| create_mp4(p, &with_sound, &options, None),
            probe_mp4_animation,
        );
    } else {
        println!("SKIPPED: MP4 through Media Foundation (not available on this system)");
    }
    let openh264 = if cfg!(windows) {
        installed_openh264()
    } else {
        None
    };
    if let Some(codec) = &openh264 {
        let options = Mp4Options {
            backend: Mp4Backend::OpenH264,
            ..Mp4Options::default()
        };
        add(
            "MP4",
            "reference-openh264.mp4",
            &|p| create_mp4(p, &with_sound, &options, Some(codec)),
            probe_mp4_animation,
        );
    }
    add(
        "GIF",
        "reference.gif",
        &|p| create_gif(p, &silent, &GifOptions::default()),
        probe_gif,
    );
    add(
        "WebP",
        "reference.webp",
        &|p| create_webp(p, &silent, &WebpOptions::default()),
        probe_webp,
    );
    add(
        "WebM",
        "reference.webm",
        &|p| create_webm(p, &with_sound, &WebmOptions::default()),
        probe_webm,
    );

    let started = Instant::now();
    let pushed = replay_into(&note, 1.0, &silent, &mut exports, &narration);
    let replay_time = started.elapsed();

    println!();
    println!(
        "Reference note: {} strokes, {} tools; replay {} (pauses trimmed to {} ms), {} frames \
         at {FPS} fps ({} distinct), video {}; narration {} mono 48 kHz; frames {WIDTH}x{HEIGHT}",
        note.strokes.len(),
        {
            let mut tools: Vec<u8> = note.strokes.iter().map(|s| s.tool as u8).collect();
            tools.sort_unstable();
            tools.dedup();
            tools.len()
        },
        secs(pushed.replay),
        MAX_PAUSE.as_millis(),
        pushed.frames,
        pushed.distinct_frames,
        secs(pushed.video_end),
        secs(NARRATION),
    );
    println!(
        "| Format | Writer | Size read | Frames pushed / read | Duration expected / read | \
         Sound expected / read | File | Encoding |"
    );
    println!("|---|---|---|---|---|---|---|---|");

    let mut rows = 0;
    for export in exports {
        let Export {
            name,
            path,
            sink,
            mut spent,
            probe,
        } = export;
        let started = Instant::now();
        let summary: VideoSummary = sink.finish().unwrap();
        spent += started.elapsed();
        let read = probe(&path).unwrap();
        let bytes = std::fs::metadata(&path).unwrap().len();
        let format = summary.format;
        let sound_expected = format.has_sound();
        let expected_frames = match format {
            // Identical consecutive frames are merged into one (longer) frame.
            AnimationFormat::Gif | AnimationFormat::WebP => pushed.distinct_frames,
            AnimationFormat::Mp4 | AnimationFormat::WebM => pushed.frames,
        };
        let expected_duration = if sound_expected {
            pushed.video_end.max(NARRATION)
        } else {
            pushed.video_end
        };
        let sound = read.audio.as_ref().map_or_else(
            || "none".to_string(),
            |a| {
                format!(
                    "{} {} ch {} Hz {}",
                    a.codec,
                    a.channels,
                    a.sample_rate,
                    secs(a.duration)
                )
            },
        );
        println!(
            "| {name} | {} | {}x{} | {} / {} | {} / {} | {} / {sound} | {} kB | {:.2} s |",
            summary.encoder,
            read.width,
            read.height,
            pushed.frames,
            read.frames,
            secs(expected_duration),
            secs(read.duration),
            if sound_expected {
                secs(NARRATION)
            } else {
                "none".into()
            },
            bytes.div_ceil(1000),
            spent.as_secs_f64(),
        );

        // Every file, read back on its own.
        assert_eq!(read.format, format, "{name}");
        assert_eq!((read.width, read.height), (WIDTH, HEIGHT), "{name} size");
        assert_eq!(read.frames, expected_frames, "{name} frames");
        assert_eq!(summary.frames, pushed.frames, "{name} frames pushed");
        // Tolerances: GIF stores centiseconds and WebM milliseconds (each frame's end rounded
        // on the absolute timeline: at most half a tick off); WebP milliseconds; MP4 keeps
        // 100 ns (Media Foundation) or 1/90000 s (OpenH264), and AAC adds up to one frame of
        // 1024 samples (21.3 ms) of padding at the end.
        let (below, above) = match (format, read.audio.as_ref().map(|a| a.codec.as_str())) {
            (AnimationFormat::Gif, _) => (Duration::from_millis(5), Duration::from_millis(5)),
            (AnimationFormat::WebP | AnimationFormat::WebM, _) => {
                (Duration::from_micros(500), Duration::from_micros(500))
            }
            (AnimationFormat::Mp4, Some("AAC")) => {
                (Duration::from_millis(1), Duration::from_micros(21_334))
            }
            (AnimationFormat::Mp4, _) => (Duration::from_micros(20), Duration::from_micros(20)),
        };
        assert_close(
            &format!("{name} duration"),
            read.duration,
            expected_duration,
            below,
            above,
        );
        match (sound_expected, &read.audio) {
            (false, None) => {}
            (true, Some(audio)) => {
                assert_eq!(
                    (audio.channels, audio.sample_rate),
                    (1, 48_000),
                    "{name} sound"
                );
                assert!(summary.audio, "{name}: the writer says it wrote sound");
                // The narration's length: AAC pads to whole 1024-sample frames, Opus to whole
                // 2.5 ms frames (WebM, no end trimming); the OpenH264 MP4's edit list ends the
                // sound with the narration (to a tick of the movie timescale).
                let above = match (format, audio.codec.as_str()) {
                    (AnimationFormat::Mp4, "AAC") => Duration::from_micros(21_334),
                    (AnimationFormat::Mp4, "Opus") => Duration::from_micros(21),
                    (AnimationFormat::WebM, "Opus") => Duration::from_micros(2_500),
                    other => panic!("{name}: unexpected sound track {other:?}"),
                };
                assert_close(
                    &format!("{name} sound"),
                    audio.duration,
                    NARRATION,
                    Duration::ZERO,
                    above,
                );
            }
            (expected, found) => panic!("{name}: sound expected {expected}, found {found:?}"),
        }
        rows += 1;
    }
    println!(
        "(drawing and encoding of all formats together: {:.2} s)",
        replay_time.as_secs_f64()
    );
    // GIF, WebP and WebM always; the MP4 rows on Windows.
    assert!(rows >= 3);
    if cfg!(windows) {
        assert!(
            media_foundation,
            "Media Foundation is expected on the development PC"
        );
    }
}

/// Encoding speed of the MP4 back ends at 1080p, 30 fps, on the reference note replayed at half
/// speed with a narration all along; only the time spent in the sinks counts (drawing
/// excluded). `cargo test --release -p ardoise-media --test animated_exports mp4_speed_1080p --
/// --ignored --nocapture`
#[test]
#[ignore = "benchmark: run it in release"]
fn mp4_speed_1080p() {
    let note = reference_note();
    let settings = VideoSettings {
        width: 1920,
        height: 1080,
        frame_rate: 30,
        audio_channels: Some(1),
    };
    let narration = simulated_narration(Duration::from_secs(12), 1);
    let dir = tempfile::tempdir().unwrap();
    let mut exports = Vec::new();
    let mut add = |name, backend, codec: Option<&OpenH264Codec>| {
        let path = dir.path().join(format!("{name}.mp4"));
        let options = Mp4Options {
            backend,
            ..Mp4Options::default()
        };
        let started = Instant::now();
        let sink = create_mp4(&path, &settings, &options, codec).unwrap();
        exports.push(Export {
            name,
            path,
            sink,
            spent: started.elapsed(),
            probe: probe_mp4_animation,
        });
    };
    if cfg!(windows) && media_foundation_available() {
        add("Media Foundation", Mp4Backend::MediaFoundation, None);
    }
    let openh264 = installed_openh264();
    if let Some(codec) = &openh264 {
        add("OpenH264", Mp4Backend::OpenH264, Some(codec));
    }
    let pushed = replay_into(&note, 0.5, &settings, &mut exports, &narration);
    println!(
        "1920x1080, 30 fps, {} frames ({}), narration {}",
        pushed.frames,
        secs(pushed.video_end),
        secs(narration.duration())
    );
    // The last frame (the whole note), to compare with what Media Foundation decodes.
    #[cfg(windows)]
    let last = {
        let timeline = Timeline::new(
            &note,
            &ReplayOptions {
                speed: 0.5,
                max_pause: Some(MAX_PAUSE),
            },
        )
        .unwrap();
        let mut renderer =
            FrameRenderer::new(timeline, &FrameSettings::for_video(&settings)).unwrap();
        renderer.render_frame(pushed.frames - 1).unwrap().to_vec()
    };
    for export in exports {
        let Export {
            name,
            path,
            sink,
            mut spent,
            probe,
        } = export;
        let started = Instant::now();
        let summary = sink.finish().unwrap();
        spent += started.elapsed();
        let read = probe(&path).unwrap();
        assert_eq!(read.frames, pushed.frames);
        println!(
            "{name}: {:.2} s = {:.0} frames/s, {} kB ({})",
            spent.as_secs_f64(),
            pushed.frames as f64 / spent.as_secs_f64(),
            std::fs::metadata(&path).unwrap().len().div_ceil(1000),
            summary.encoder
        );
        #[cfg(windows)]
        if media_foundation_available() {
            // The last frame decoded by Media Foundation against the drawing.
            let decoded = ardoise_media::probe_mp4(
                &path,
                ardoise_media::ProbeOptions {
                    capture_frame: Some(pushed.frames - 1),
                    capture_audio: false,
                },
            )
            .unwrap();
            let frame = decoded.captured_frame.unwrap();
            let (mut squared, mut worst, mut count) = (0f64, 0u8, 0usize);
            for (a, b) in frame
                .pixels
                .as_chunks::<4>()
                .0
                .iter()
                .zip(last.as_chunks::<4>().0)
            {
                for c in 0..3 {
                    let d = a[c].abs_diff(b[c]);
                    worst = worst.max(d);
                    squared += f64::from(d) * f64::from(d);
                    count += 1;
                }
            }
            let psnr = 10.0 * (255.0f64.powi(2) / (squared / count as f64)).log10();
            println!(
                "  last frame decoded by Media Foundation: PSNR {psnr:.1} dB, worst error {worst}"
            );
        }
    }
}
