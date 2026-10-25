//! Tests of the export pipeline: the plan, the frames and sound pushed (through a recording
//! sink), real GIF, WebP and WebM files read back by their probes, cancel and file safety.

use std::fs::File;
use std::io::BufReader;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

use ardoise_core::note::{
    A4_LANDSCAPE, A4_PORTRAIT, Background, Layer, Note, Origin, PageSize, Paper, Template, Timing,
};
use ardoise_ink::{Point, Rgba, Stroke, Tool};
use ardoise_render::Backdrop;
use matroska_demuxer::{Frame as MkvFrame, MatroskaFile, TrackType};

use super::*;
use crate::microphone::simulated_narration;
use crate::narration::NarrationBase;
use crate::opus::OpusPacketDecoder;
use crate::{AnimationProbe, OpusHead, probe_gif, probe_webm, probe_webp};

const PAGE: PageSize = PageSize {
    width: 6000,
    height: 4000,
};

fn ms(value: u64) -> Duration {
    Duration::from_millis(value)
}

/// A straight stroke of 20 samples from `from` to `to`, drawn from `t0` to `t1` ms.
fn line(from: (f32, f32), to: (f32, f32), t0: u32, t1: u32) -> Stroke {
    let n = 20u32;
    Stroke {
        tool: Tool::Ballpoint,
        color: Rgba::rgb(30, 50, 140),
        width: 250.0,
        layer: 0,
        points: (0..n)
            .map(|i| {
                let u = i as f32 / (n - 1) as f32;
                Point {
                    x: from.0 + (to.0 - from.0) * u,
                    y: from.1 + (to.1 - from.1) * u,
                    pressure: 0.6,
                    t: t0 + (t1 - t0) * i / (n - 1),
                }
            })
            .collect(),
    }
}

fn note(strokes: Vec<Stroke>) -> Note {
    Note {
        id: "00000000-0000-4000-8000-000000000001".into(),
        title: "export".into(),
        created: 1_700_000_000_000,
        modified: 1_700_000_000_000,
        page: PAGE,
        background: Background {
            paper: Paper::Cream,
            template: Template::Lined,
            texture: false,
        },
        background_image: None,
        tags: vec![],
        favorite: false,
        layers: vec![Layer {
            name: "1".into(),
            visible: true,
        }],
        strokes,
        timing: Timing::Recorded,
        origin: Origin::Tablet,
    }
}

/// Two strokes, over [200, 1000] and [1500, 2500] ms: a replay of 2.5 s whose first instants
/// show a blank page.
fn sample_note() -> Note {
    note(vec![
        line((600.0, 800.0), (5400.0, 1400.0), 200, 1000),
        line((600.0, 2600.0), (5400.0, 3200.0), 1500, 2500),
    ])
}

fn gif() -> FormatOptions {
    FormatOptions::Gif(GifOptions::default())
}

fn webp() -> FormatOptions {
    FormatOptions::WebP(WebpOptions::default())
}

fn webm() -> FormatOptions {
    FormatOptions::WebM(WebmOptions::default())
}

fn mp4() -> FormatOptions {
    FormatOptions::Mp4(Mp4Options::default())
}

/// A small export at 10 frames per second: 64 × 48 (96 × 64 for the video codecs).
fn export(format: FormatOptions) -> AnimationExport {
    let (width, height) = if format.format().has_sound() {
        (96, 64)
    } else {
        (64, 48)
    };
    AnimationExport {
        width,
        height,
        frame_rate: 10,
        replay: ReplayOptions::default(),
        backdrop: Backdrop::Paper,
        hold_end: Duration::ZERO,
        format,
    }
}

fn narration(duration: Duration, channels: u16, start_ms: i64) -> Narration {
    Narration {
        clip: simulated_narration(duration, channels),
        base: NarrationBase::Replay,
        options: ReplayOptions::default(),
        start_ms,
    }
}

/// What the pipeline pushed.
#[derive(Debug, Default)]
struct Recording {
    timestamps: Vec<Duration>,
    frames: Vec<Vec<u8>>,
    /// Audio samples pushed before each frame.
    audio_before: Vec<usize>,
    audio: Vec<i16>,
}

impl VideoSink for Recording {
    fn push_frame(&mut self, rgba: &[u8], timestamp: Duration) -> Result<(), MediaError> {
        self.timestamps.push(timestamp);
        self.frames.push(rgba.to_vec());
        self.audio_before.push(self.audio.len());
        Ok(())
    }

    fn push_audio(&mut self, samples: &[i16]) -> Result<(), MediaError> {
        self.audio.extend_from_slice(samples);
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<VideoSummary, MediaError> {
        Err(MediaError::InvalidSettings(
            "a recording is not a file".into(),
        ))
    }
}

/// The plan and what the pipeline pushes for it.
fn record(
    note: &Note,
    export: &AnimationExport,
    narration: Option<&Narration>,
) -> (ExportPlan, Recording) {
    let mut job = Job::new(note, export, narration).unwrap();
    let mut recording = Recording::default();
    job.push_frames(&mut recording, &mut |_| {}, &AtomicBool::new(false))
        .unwrap();
    (job.plan, recording)
}

/// The frame showing the whole note.
fn whole_note(note: &Note, export: &AnimationExport, options: &ReplayOptions) -> Vec<u8> {
    let timeline = Timeline::new(note, options).unwrap();
    let end = timeline.duration();
    let settings = FrameSettings {
        width: export.width,
        height: export.height,
        frame_rate: export.frame_rate,
        backdrop: export.backdrop,
    };
    let mut renderer = FrameRenderer::new(timeline, &settings).unwrap();
    renderer.render_at(end).unwrap().to_vec()
}

/// Runs of identical consecutive frames: what GIF and WebP store as frames.
fn runs(frames: &[Vec<u8>]) -> u64 {
    1 + frames.windows(2).filter(|w| w[0] != w[1]).count() as u64
}

fn run_export(
    note: &Note,
    export: &AnimationExport,
    narration: Option<&Narration>,
    path: &Path,
) -> Result<VideoSummary, MediaError> {
    export_animation(
        note,
        export,
        narration,
        path,
        None,
        &mut |_| {},
        &AtomicBool::new(false),
    )
}

/// Asserts that `actual` is `expected` or at most one frame later.
fn assert_within_a_frame(actual: Duration, expected: Duration, frame_rate: u32) {
    let frame = Duration::from_secs(1) / frame_rate;
    assert!(
        actual + Duration::from_millis(1) >= expected && actual <= expected + frame,
        "{actual:?} for {expected:?}"
    );
}

/// The names in `dir`.
fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn invalid_settings(result: Result<ExportPlan, MediaError>) {
    assert!(
        matches!(result, Err(MediaError::InvalidSettings(_))),
        "{result:?}"
    );
}

// --- The plan ---

#[test]
fn plan_of_a_plain_replay() {
    let note = sample_note();
    let plan = plan_export(&note, &export(gif()), None).unwrap();
    assert_eq!(
        plan,
        ExportPlan {
            replay: ReplayOptions::default(),
            lead_in: Duration::ZERO,
            replay_duration: ms(2500),
            duration: ms(2500),
            frames: 25,
            audio_start: None,
        }
    );

    let held = AnimationExport {
        hold_end: ms(2000),
        ..export(gif())
    };
    let plan = plan_export(&note, &held, None).unwrap();
    assert_eq!((plan.duration, plan.frames), (ms(4500), 45));

    // Rounded up to whole frames.
    let seven = AnimationExport {
        frame_rate: 7,
        ..export(webm())
    };
    assert_eq!(plan_export(&note, &seven, None).unwrap().frames, 18);

    // A note without strokes still makes one frame.
    let plan = plan_export(&note_without_strokes(), &export(webp()), None).unwrap();
    assert_eq!((plan.duration, plan.frames), (Duration::ZERO, 1));

    // Speed and trimmed pauses.
    let fast = AnimationExport {
        replay: ReplayOptions {
            speed: 2.0,
            max_pause: Some(ms(100)),
        },
        ..export(mp4())
    };
    // Kept: 100 ms of the 200 before the first stroke, 800, 100 of the 500 between, 1000.
    let plan = plan_export(&note, &fast, None).unwrap();
    assert_eq!(plan.replay, fast.replay);
    assert_eq!(plan.replay_duration, ms(1000));
}

fn note_without_strokes() -> Note {
    note(vec![])
}

#[test]
fn a_target_duration_sets_the_speed() {
    let note = sample_note();
    for target in [ms(1000), ms(2000), ms(5000)] {
        let replay = ReplayOptions::for_duration(&note, Some(ms(500)), target);
        let plan = plan_export(
            &note,
            &AnimationExport {
                replay,
                ..export(gif())
            },
            None,
        )
        .unwrap();
        let error = plan.replay_duration.abs_diff(target);
        assert!(error < ms(1), "{:?} for {target:?}", plan.replay_duration);
    }
}

#[test]
fn a_narration_sets_the_time_base() {
    let note = sample_note();
    // Recorded at 2× from 1.5 s before the note's origin: 1.5 s of blank page first.
    let mut early = narration(ms(3000), 1, -1500);
    early.options.speed = 2.0;
    let plan = plan_export(&note, &export(webm()), Some(&early)).unwrap();
    assert_eq!(plan.replay, early.options);
    assert_eq!(plan.lead_in, ms(1500));
    assert_eq!(plan.replay_duration, ms(1250));
    // The narration (3 s from video time 0) ends after the replay (2.75 s).
    assert_eq!(plan.duration, ms(3000));
    assert_eq!(plan.frames, 30);
    assert_eq!(plan.audio_start, Some(Duration::ZERO));

    // Starting 0.5 s into the replay, ending 1 s after it.
    let late = narration(ms(3000), 2, 500);
    let plan = plan_export(&note, &export(webm()), Some(&late)).unwrap();
    assert_eq!(plan.lead_in, Duration::ZERO);
    assert_eq!(plan.duration, ms(3500));
    assert_eq!(plan.audio_start, Some(ms(500)));
    // The held frame counts from the replay's end, the narration may outlast it.
    for (hold, duration) in [(ms(500), ms(3500)), (ms(2000), ms(4500))] {
        let held = AnimationExport {
            hold_end: hold,
            ..export(mp4())
        };
        assert_eq!(
            plan_export(&note, &held, Some(&late)).unwrap().duration,
            duration
        );
    }
    // Formats without sound keep the timing, without a sound track.
    let plan = plan_export(&note, &export(gif()), Some(&late)).unwrap();
    assert_eq!((plan.duration, plan.audio_start), (ms(3500), None));
    // An empty clip gives no sound track.
    let empty = Narration {
        clip: AudioClip {
            channels: 1,
            samples: vec![],
        },
        ..late.clone()
    };
    let plan = plan_export(&note, &export(webm()), Some(&empty)).unwrap();
    assert_eq!((plan.duration, plan.audio_start), (ms(2500), None));
}

#[test]
fn invalid_settings_are_refused() {
    let note = sample_note();
    let plan = |export: AnimationExport| plan_export(&note, &export, None);
    for rate in [0, 61] {
        invalid_settings(plan(AnimationExport {
            frame_rate: rate,
            ..export(gif())
        }));
    }
    plan(AnimationExport {
        frame_rate: 60,
        ..export(gif())
    })
    .unwrap();
    for format in [mp4(), webm()] {
        invalid_settings(plan(AnimationExport {
            width: 97,
            ..export(format)
        }));
        invalid_settings(plan(AnimationExport {
            height: 14,
            ..export(format)
        }));
        invalid_settings(plan(AnimationExport {
            backdrop: Backdrop::Transparent,
            ..export(format)
        }));
    }
    for (format, max) in [
        (mp4(), 4096),
        (webm(), 8192),
        (gif(), 16_384),
        (webp(), 16_383),
    ] {
        let at = |width| AnimationExport {
            width,
            ..export(format)
        };
        plan(at(max)).unwrap();
        invalid_settings(plan(at(max + 2)));
    }
    for format in [gif(), webp()] {
        invalid_settings(plan(AnimationExport {
            width: 1,
            ..export(format)
        }));
        plan(AnimationExport {
            width: 3,
            height: 2,
            backdrop: Backdrop::Transparent,
            ..export(format)
        })
        .unwrap();
    }
    plan(AnimationExport {
        hold_end: ms(60_000),
        ..export(gif())
    })
    .unwrap();
    invalid_settings(plan(AnimationExport {
        hold_end: ms(60_001),
        ..export(gif())
    }));
    invalid_settings(plan(AnimationExport {
        replay: ReplayOptions {
            speed: 20.0,
            max_pause: None,
        },
        ..export(gif())
    }));

    // The narration's options are checked, and its clip when it goes into a sound track.
    let mut slow = narration(ms(100), 1, 0);
    slow.options.speed = 0.1;
    invalid_settings(plan_export(&note, &export(gif()), Some(&slow)));
    let three = Narration {
        clip: AudioClip {
            channels: 3,
            samples: vec![0; 30],
        },
        ..narration(ms(100), 1, 0)
    };
    invalid_settings(plan_export(&note, &export(webm()), Some(&three)));
    plan_export(&note, &export(gif()), Some(&three)).unwrap();
    // A damaged start far in the past is not a day-long video.
    let ancient = narration(ms(100), 1, i64::MIN);
    invalid_settings(plan_export(&note, &export(webm()), Some(&ancient)));
}

// --- Frames and sound ---

#[test]
fn frames_follow_the_plan_with_a_lead_in() {
    let note = sample_note();
    let early = narration(ms(3000), 1, -1500);
    let options = export(webm());
    let (plan, recording) = record(&note, &options, Some(&early));
    assert_eq!(plan.frames, 40); // 1.5 s + 2.5 s
    assert_eq!(recording.frames.len(), 40);
    for (k, timestamp) in recording.timestamps.iter().enumerate() {
        assert_eq!(*timestamp, frame_time(k as u64, 10));
    }
    // Blank page until the first sample appears 0.2 s into the replay.
    let blank = &recording.frames[0];
    assert!(recording.frames[..17].iter().all(|f| f == blank));
    assert_ne!(&recording.frames[17], blank);
    assert_eq!(
        recording.frames.last().unwrap(),
        &whole_note(&note, &options, &early.options)
    );

    // The narration from video time 0, frame by frame: before frame k, everything up to its
    // end.
    assert_eq!(recording.audio, early.clip.samples);
    for (k, &before) in recording.audio_before.iter().enumerate() {
        assert_eq!(
            before,
            ((k + 1) * 4800).min(early.clip.samples.len()),
            "frame {k}"
        );
    }
}

#[test]
fn sound_starts_after_silence_and_never_drifts() {
    let note = sample_note();
    let late = narration(ms(1000), 2, 700);
    // 7 frames per second: 48000 / 7 samples per frame is not a whole number.
    let options = AnimationExport {
        frame_rate: 7,
        ..export(webm())
    };
    let (plan, recording) = record(&note, &options, Some(&late));
    assert_eq!(plan.audio_start, Some(ms(700)));
    let silence = 700 * 48 * 2;
    assert_eq!(recording.audio.len(), silence + late.clip.samples.len());
    assert!(recording.audio[..silence].iter().all(|&s| s == 0));
    assert_eq!(&recording.audio[silence..], &late.clip.samples[..]);
    for (k, &before) in recording.audio_before.iter().enumerate() {
        let expected = (k as u64 + 1) * 48_000 / 7 * 2;
        assert_eq!(
            before as u64,
            expected.min(recording.audio.len() as u64),
            "frame {k}"
        );
    }
    // Without sound in the format: nothing pushed, the same frames.
    let (gif_plan, silent) = record(
        &note,
        &AnimationExport {
            format: gif(),
            ..options
        },
        Some(&late),
    );
    assert_eq!(gif_plan.frames, plan.frames);
    assert!(silent.audio.is_empty());
}

#[test]
fn the_last_frame_shows_the_whole_note() {
    let note = sample_note();
    // 2.5 s at 7 fps: 18 frames, the last at 2.43 s.
    let options = AnimationExport {
        frame_rate: 7,
        ..export(gif())
    };
    let (plan, recording) = record(&note, &options, None);
    assert_eq!(plan.frames, 18);
    let whole = whole_note(&note, &options, &ReplayOptions::default());
    assert_eq!(recording.frames.last().unwrap(), &whole);
    assert_ne!(&recording.frames[16], &whole);

    // Held: the whole note from the replay's end on.
    let held = AnimationExport {
        hold_end: ms(1000),
        ..export(gif())
    };
    let (_, recording) = record(&note, &held, None);
    assert_eq!(recording.frames.len(), 35);
    assert!(
        recording.frames[25..]
            .iter()
            .all(|f| f == &whole_frame_at(&note, &held))
    );
    assert_ne!(&recording.frames[24], &whole_frame_at(&note, &held));
}

fn whole_frame_at(note: &Note, export: &AnimationExport) -> Vec<u8> {
    whole_note(note, export, &export.replay)
}

#[test]
fn progress_counts_every_frame_and_cancel_stops() {
    let note = sample_note();
    let options = export(gif());
    let mut job = Job::new(&note, &options, None).unwrap();
    let mut seen = Vec::new();
    job.push_frames(
        &mut Recording::default(),
        &mut |p| seen.push(p),
        &AtomicBool::new(false),
    )
    .unwrap();
    let expected: Vec<ExportProgress> = (1..=25)
        .map(|frames_done| ExportProgress {
            frames_done,
            frames_total: 25,
        })
        .collect();
    assert_eq!(seen, expected);

    let mut job = Job::new(&note, &options, None).unwrap();
    let cancel = AtomicBool::new(true);
    let mut recording = Recording::default();
    let result = job.push_frames(
        &mut recording,
        &mut |_| panic!("no frame expected"),
        &cancel,
    );
    assert!(matches!(result, Err(MediaError::Cancelled)));
    assert!(recording.frames.is_empty());
}

// --- Files ---

#[test]
fn gif_export_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let note = sample_note();
    let options = AnimationExport {
        hold_end: ms(1000),
        ..export(gif())
    };
    let (plan, recording) = record(&note, &options, None);
    let path = dir.path().join("note.gif");
    let summary = run_export(&note, &options, None, &path).unwrap();
    assert_eq!(summary.frames, plan.frames);
    assert!(!summary.audio);
    let probe = probe_gif(&path).unwrap();
    assert_eq!((probe.width, probe.height), (64, 48));
    // Identical frames are merged into one longer frame.
    assert_eq!(probe.frames, runs(&recording.frames));
    assert!(probe.frames < plan.frames);
    assert_within_a_frame(probe.duration, plan.duration, 10);
    assert_eq!(entries(dir.path()), ["note.gif"]);

    // The held frame lengthens the file, not its frame count.
    let longer = AnimationExport {
        hold_end: ms(3000),
        ..options
    };
    let path = dir.path().join("longer.gif");
    run_export(&note, &longer, None, &path).unwrap();
    let held = probe_gif(&path).unwrap();
    assert_eq!(held.duration, probe.duration + ms(2000));
    assert_eq!(held.frames, probe.frames);
}

#[test]
fn webp_export_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let note = sample_note();
    let options = export(webp());
    let (plan, recording) = record(&note, &options, None);
    let path = dir.path().join("note.webp");
    let summary = run_export(&note, &options, None, &path).unwrap();
    assert_eq!(summary.frames, plan.frames);
    let probe = probe_webp(&path).unwrap();
    assert_eq!((probe.width, probe.height), (64, 48));
    assert_eq!(probe.frames, runs(&recording.frames));
    assert_within_a_frame(probe.duration, plan.duration, 10);

    // A target duration.
    let replay = ReplayOptions::for_duration(&note, Some(ms(500)), ms(1000));
    let short = AnimationExport {
        replay,
        hold_end: ms(500),
        ..options
    };
    let path = dir.path().join("short.webp");
    run_export(&note, &short, None, &path).unwrap();
    let probe = probe_webp(&path).unwrap();
    assert_within_a_frame(probe.duration, ms(1500), 10);
}

#[test]
fn transparent_backgrounds_only_in_gif_and_webp() {
    let dir = tempfile::tempdir().unwrap();
    let note = sample_note();
    for (format, name) in [(gif(), "clear.gif"), (webp(), "clear.webp")] {
        let options = AnimationExport {
            backdrop: Backdrop::Transparent,
            ..export(format)
        };
        let path = dir.path().join(name);
        run_export(&note, &options, None, &path).unwrap();
        let probe: AnimationProbe = if name.ends_with("gif") {
            probe_gif(&path).unwrap()
        } else {
            probe_webp(&path).unwrap()
        };
        assert_eq!((probe.width, probe.height), (64, 48));
        assert_within_a_frame(probe.duration, ms(2500), 10);
    }
    let options = AnimationExport {
        backdrop: Backdrop::Transparent,
        ..export(webm())
    };
    let path = dir.path().join("clear.webm");
    assert!(matches!(
        run_export(&note, &options, None, &path),
        Err(MediaError::InvalidSettings(_))
    ));
    assert_eq!(entries(dir.path()), ["clear.gif", "clear.webp"]);
}

/// The Opus sound of a WebM file, decoded, pre-skip dropped: (channels, samples).
fn webm_sound(path: &Path) -> (u16, Vec<i16>) {
    let mut mkv = MatroskaFile::open(BufReader::new(File::open(path).unwrap())).unwrap();
    let track = mkv
        .tracks()
        .iter()
        .find(|t| t.track_type() == TrackType::Audio)
        .unwrap();
    let number = track.track_number().get();
    let head = OpusHead::parse(track.codec_private().unwrap()).unwrap();
    let mut decoder = OpusPacketDecoder::new(head.channels).unwrap();
    let mut samples = Vec::new();
    let mut frame = MkvFrame::default();
    while mkv.next_frame(&mut frame).unwrap() {
        if frame.track == number {
            samples.extend_from_slice(decoder.decode_samples(&frame.data).unwrap());
        }
    }
    let skip = usize::from(head.pre_skip) * usize::from(head.channels);
    (head.channels, samples.split_off(skip))
}

fn rms(samples: &[i16]) -> f64 {
    let sum: f64 = samples.iter().map(|&s| f64::from(s).powi(2)).sum();
    (sum / samples.len().max(1) as f64).sqrt()
}

#[test]
fn webm_export_carries_the_narration() {
    let dir = tempfile::tempdir().unwrap();
    let note = sample_note();
    let late = narration(ms(3000), 1, 500);
    let options = export(webm());
    let plan = plan_export(&note, &options, Some(&late)).unwrap();
    let path = dir.path().join("narrated.webm");
    let summary = run_export(&note, &options, Some(&late), &path).unwrap();
    assert!(summary.audio);
    assert_eq!(summary.frames, plan.frames);

    let probe = probe_webm(&path).unwrap();
    assert_eq!((probe.width, probe.height), (96, 64));
    assert_eq!(probe.frames, plan.frames);
    assert_within_a_frame(probe.duration, plan.duration, 10);
    let audio = probe.audio.unwrap();
    assert_eq!(
        (audio.codec.as_str(), audio.channels, audio.sample_rate),
        ("Opus", 1, 48_000)
    );
    // The track: 0.5 s of silence then the 3 s narration (Opus pads less than 2.5 ms).
    let track = audio.duration.saturating_sub(ms(3500));
    assert!(
        audio.duration >= ms(3500) && track < Duration::from_micros(2500),
        "{:?}",
        audio.duration
    );

    // The narration starts at `audio_start`: silence before, its sound after.
    let (channels, sound) = webm_sound(&path);
    assert_eq!(channels, 1);
    let start = 500 * 48;
    let before = rms(&sound[..start - 960]);
    let after = rms(&sound[start..start + 48_000]);
    let original = rms(&late.clip.samples[..48_000]);
    assert!(before < 20.0, "RMS {before:.1} before the narration");
    assert!(
        (after / original - 1.0).abs() < 0.2,
        "RMS {after:.0} for {original:.0}"
    );

    // Stereo, starting before the note: lead-in, sound from the start of the video.
    let early = narration(ms(2000), 2, -1000);
    let plan = plan_export(&note, &options, Some(&early)).unwrap();
    assert_eq!(
        (plan.lead_in, plan.audio_start),
        (ms(1000), Some(Duration::ZERO))
    );
    let path = dir.path().join("early.webm");
    run_export(&note, &options, Some(&early), &path).unwrap();
    let probe = probe_webm(&path).unwrap();
    assert_eq!(probe.frames, 35);
    let audio = probe.audio.unwrap();
    assert_eq!(audio.channels, 2);
    assert!(audio.duration >= ms(2000) && audio.duration < ms(2003));
}

#[test]
fn webm_export_without_narration() {
    let dir = tempfile::tempdir().unwrap();
    let note = sample_note();
    let path = dir.path().join("silent.webm");
    let options = AnimationExport {
        hold_end: ms(500),
        ..export(webm())
    };
    let summary = run_export(&note, &options, None, &path).unwrap();
    assert!(!summary.audio);
    let probe = probe_webm(&path).unwrap();
    assert_eq!(probe.frames, 30);
    assert_eq!(probe.duration, ms(3000));
    assert_eq!(probe.audio, None);
}

#[test]
fn cancel_leaves_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let note = sample_note();
    for format in [gif(), webp(), webm()] {
        let options = export(format);
        let path = dir
            .path()
            .join(format!("cancelled.{}", format.format().extension()));
        let cancel = AtomicBool::new(false);
        let mut calls = 0;
        let result = export_animation(
            &note,
            &options,
            None,
            &path,
            None,
            &mut |p| {
                calls += 1;
                if p.frames_done == 5 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        );
        assert!(matches!(result, Err(MediaError::Cancelled)), "{result:?}");
        assert_eq!(calls, 5);
        assert!(entries(dir.path()).is_empty(), "{:?}", entries(dir.path()));
    }

    // An existing file at the destination survives a cancelled export, and is replaced by a
    // finished one.
    let path = dir.path().join("existing.gif");
    std::fs::write(&path, b"previous export").unwrap();
    let cancel = AtomicBool::new(true);
    let result = export_animation(
        &note,
        &export(gif()),
        None,
        &path,
        None,
        &mut |_| {},
        &cancel,
    );
    assert!(matches!(result, Err(MediaError::Cancelled)));
    assert_eq!(std::fs::read(&path).unwrap(), b"previous export");
    assert_eq!(entries(dir.path()), ["existing.gif"]);
    run_export(&note, &export(gif()), None, &path).unwrap();
    assert_eq!(probe_gif(&path).unwrap().width, 64);
    assert_eq!(entries(dir.path()), ["existing.gif"]);
}

/// A sink that writes its file only when it finishes, and sets `cancel_on_finish` then.
struct Flushing {
    path: PathBuf,
    cancel_on_finish: Option<std::sync::Arc<AtomicBool>>,
}

impl VideoSink for Flushing {
    fn push_frame(&mut self, _: &[u8], _: Duration) -> Result<(), MediaError> {
        Ok(())
    }

    fn push_audio(&mut self, _: &[i16]) -> Result<(), MediaError> {
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<VideoSummary, MediaError> {
        std::fs::write(&self.path, b"finished")?;
        if let Some(cancel) = &self.cancel_on_finish {
            cancel.store(true, Ordering::Relaxed);
        }
        Ok(VideoSummary {
            format: AnimationFormat::Gif,
            frames: 1,
            duration: ms(100),
            audio: false,
            encoder: "test".into(),
        })
    }
}

#[test]
fn a_cancel_after_the_last_frame_leaves_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let note = sample_note();
    let path = dir.path().join("late.gif");
    std::fs::write(&path, b"previous export").unwrap();

    // Cancelled from the last frame's progress report: the file is not finished.
    let cancel = AtomicBool::new(false);
    let mut last = None;
    let result = export_animation(
        &note,
        &export(gif()),
        None,
        &path,
        None,
        &mut |p| {
            last = Some(p);
            if p.frames_done == p.frames_total {
                cancel.store(true, Ordering::Relaxed);
            }
        },
        &cancel,
    );
    assert!(matches!(result, Err(MediaError::Cancelled)), "{result:?}");
    assert_eq!(last.map(|p| p.frames_done), Some(25));
    assert_eq!(std::fs::read(&path).unwrap(), b"previous export");
    assert_eq!(entries(dir.path()), ["late.gif"]);

    // Cancelled while the sink finishes its file: the file is not put in place.
    let cancel = std::sync::Arc::new(AtomicBool::new(false));
    let mut job = Job::new(&note, &export(gif()), None).unwrap();
    let mut open = |temp: &Path, _: &VideoSettings| -> Result<Box<dyn VideoSink>, MediaError> {
        Ok(Box::new(Flushing {
            path: temp.to_path_buf(),
            cancel_on_finish: Some(std::sync::Arc::clone(&cancel)),
        }))
    };
    let result = write_file(&path, &mut job, &mut open, &mut |_| {}, &cancel);
    assert!(matches!(result, Err(MediaError::Cancelled)), "{result:?}");
    assert_eq!(std::fs::read(&path).unwrap(), b"previous export");
    assert_eq!(entries(dir.path()), ["late.gif"]);

    // Not cancelled: the same sink's file replaces the destination.
    let mut job = Job::new(&note, &export(gif()), None).unwrap();
    let mut open = |temp: &Path, _: &VideoSettings| -> Result<Box<dyn VideoSink>, MediaError> {
        Ok(Box::new(Flushing {
            path: temp.to_path_buf(),
            cancel_on_finish: None,
        }))
    };
    let flag = AtomicBool::new(false);
    write_file(&path, &mut job, &mut open, &mut |_| {}, &flag).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"finished");
    assert_eq!(entries(dir.path()), ["late.gif"]);
}

#[test]
fn errors_leave_nothing_behind() {
    let dir = tempfile::tempdir().unwrap();
    let note = sample_note();
    // Invalid settings: refused before any file is created.
    let path = dir.path().join("odd.webm");
    let odd = AnimationExport {
        width: 95,
        ..export(webm())
    };
    assert!(run_export(&note, &odd, None, &path).is_err());
    // A destination in a missing folder: the sink cannot create its file.
    let missing = dir.path().join("missing").join("note.gif");
    assert!(matches!(
        run_export(&note, &export(gif()), None, &missing),
        Err(MediaError::Io(_))
    ));
    // A destination that is a folder: the final rename fails.
    let folder = dir.path().join("folder.gif");
    std::fs::create_dir(&folder).unwrap();
    assert!(run_export(&note, &export(gif()), None, &folder).is_err());
    assert_eq!(entries(dir.path()), ["folder.gif"]);
    assert!(entries(&folder).is_empty());
}

/// Media Foundation does not exist outside Windows: the MP4 export fails cleanly.
#[cfg(not(windows))]
#[test]
fn mp4_export_fails_without_media_foundation() {
    let dir = tempfile::tempdir().unwrap();
    let note = sample_note();
    let path = dir.path().join("note.mp4");
    std::fs::write(&path, b"previous").unwrap();
    let late = narration(ms(1000), 1, 0);
    let result = run_export(&note, &export(mp4()), Some(&late), &path);
    assert!(matches!(result, Err(MediaError::Unsupported)), "{result:?}");
    assert_eq!(std::fs::read(&path).unwrap(), b"previous");
    assert_eq!(entries(dir.path()), ["note.mp4"]);
}

/// The MP4 export with a narration, read back by Media Foundation (Windows only).
#[cfg(windows)]
#[test]
fn mp4_export_reads_back() {
    let dir = tempfile::tempdir().unwrap();
    let note = sample_note();
    let late = narration(ms(3000), 1, 500);
    let options = AnimationExport {
        width: 320,
        height: 240,
        ..export(mp4())
    };
    let plan = plan_export(&note, &options, Some(&late)).unwrap();
    let path = dir.path().join("note.mp4");
    let summary = run_export(&note, &options, Some(&late), &path).unwrap();
    assert!(summary.audio);
    let probe = crate::probe_mp4_animation(&path).unwrap();
    assert_eq!((probe.width, probe.height), (320, 240));
    assert_eq!(probe.frames, plan.frames);
    let audio = probe.audio.unwrap();
    assert_eq!(audio.channels, 1);
    // AAC frames are 1024 samples: the track may end up to one frame later.
    assert!(
        audio.duration >= ms(3450) && audio.duration < ms(3600),
        "{:?}",
        audio.duration
    );
    assert_eq!(entries(dir.path()), ["note.mp4"]);
}

/// `duration_ms` of writing on an A4 page: a cursive word every 200 ms, drawn in 150 ms.
fn handwriting(duration_ms: u32) -> Note {
    let strokes = (0..duration_ms / 200)
        .map(|i| {
            let (row, column) = (i / 10, i % 10);
            let (x0, y0) = (
                1500.0 + column as f32 * 2600.0,
                2000.0 + row as f32 * 1800.0,
            );
            let n = 40u32;
            Stroke {
                points: (0..n)
                    .map(|k| {
                        let u = k as f32 / (n - 1) as f32;
                        Point {
                            x: x0 + u * 2200.0 + (u * 12.0).sin() * 300.0,
                            y: y0 + (u * 12.0).cos() * 500.0,
                            pressure: 0.5 + 0.3 * (u * 5.0).sin(),
                            t: i * 200 + 150 * k / (n - 1),
                        }
                    })
                    .collect(),
                ..line((0.0, 0.0), (1.0, 1.0), 0, 1)
            }
        })
        .collect();
    Note {
        page: A4_LANDSCAPE,
        ..note(strokes)
    }
}

/// Export speed at 1080p and 30 frames per second of 20 s of writing, compared with the
/// estimate: `cargo test --release -p ardoise-media export_speed -- --ignored --nocapture`.
#[test]
#[ignore = "benchmark: run in release"]
fn export_speed_1080p() {
    let dir = tempfile::tempdir().unwrap();
    let note = handwriting(20_000);
    for format in [gif(), webp(), webm()] {
        let options = AnimationExport {
            width: 1920,
            height: 1080,
            frame_rate: 30,
            ..export(format)
        };
        let path = dir
            .path()
            .join(format!("speed.{}", format.format().extension()));
        let started = std::time::Instant::now();
        let summary = run_export(&note, &options, None, &path).unwrap();
        let elapsed = started.elapsed();
        let estimate = crate::estimate_export_time(format.format(), 1920, 1080, summary.frames);
        println!(
            "{}: {} frames in {:.1} s ({:.0} fps), {} kB; estimated {:.1} s",
            format.format().extension(),
            summary.frames,
            elapsed.as_secs_f64(),
            summary.frames as f64 / elapsed.as_secs_f64(),
            std::fs::metadata(&path).unwrap().len() / 1000,
            estimate.as_secs_f64()
        );
    }
}

// --- Helpers ---

#[test]
fn frame_size_of_the_page() {
    // The Slate's A4 area is 297 × 216 mm: 1920 × 1396.4, rounded to the nearest even height.
    assert_eq!(page_frame_size(A4_LANDSCAPE, 1920), (1920, 1396));
    assert_eq!(page_frame_size(A4_PORTRAIT, 1920), (1396, 1920));
    assert_eq!(page_frame_size(PAGE, 1920), (1920, 1280));
    assert_eq!(page_frame_size(A4_LANDSCAPE, 1921), (1920, 1396));
    let square = PageSize {
        width: 100,
        height: 100,
    };
    assert_eq!(page_frame_size(square, 64), (64, 64));
    // Extreme pages and sizes stay even and at least 2.
    let thin = PageSize {
        width: 100_000,
        height: 1,
    };
    assert_eq!(page_frame_size(thin, 1920), (1920, 2));
    let empty = PageSize {
        width: 0,
        height: 0,
    };
    assert_eq!(page_frame_size(empty, 100), (100, 100));
    assert_eq!(page_frame_size(A4_PORTRAIT, 0), (2, 2));
    assert_eq!(
        page_frame_size(A4_PORTRAIT, u32::MAX),
        (3_123_612_578, 4_294_967_294)
    );
    for long in [2u32, 3, 16, 720, 1080, 3840] {
        for page in [A4_LANDSCAPE, A4_PORTRAIT, PAGE, thin] {
            let (w, h) = page_frame_size(page, long);
            assert!(w % 2 == 0 && h % 2 == 0 && w >= 2 && h >= 2, "{w} x {h}");
            assert_eq!(w.max(h), (long & !1).max(2));
        }
    }
}

#[test]
fn temporary_files_are_hidden_next_to_the_destination() {
    let dir = Path::new("/some/folder");
    let temp = temp_path_for(&dir.join("lesson.mp4"));
    assert_eq!(temp.parent(), Some(dir));
    let name = temp.file_name().unwrap().to_string_lossy().into_owned();
    assert!(name.starts_with(".lesson.mp4."), "{name}");
    assert!(name.ends_with(".part"), "{name}");
    assert_ne!(temp_path_for(&dir.join("lesson.mp4")), temp);
    // A long name is shortened to fit in 255 UTF-16 units.
    let long = "é".repeat(300) + ".gif";
    let name = temp_path_for(&dir.join(&long))
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(name.encode_utf16().count() <= 255);
    assert!(name.ends_with(".part"));
}
