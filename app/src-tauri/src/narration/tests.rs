//! Tests of the narration commands: shapes, the page log and the live split, replay
//! recordings with a stand-in microphone (saving, pending narrations, backup copies).

use std::cell::{Cell, RefCell};
use std::fs;
use std::sync::atomic::AtomicUsize;

use ardoise_media::decode_narration;
use ardoise_media::microphone::simulated_narration;
use serde_json::json;

use super::*;
use crate::library::path_text;
use crate::library::tests::{context, library, write_sample};
use crate::live::HubConfig;

fn tone(frames: usize, channels: u16) -> AudioClip {
    AudioClip {
        channels,
        samples: (0..frames * usize::from(channels))
            .map(|i| i16::try_from(i % 30_000).unwrap() + 1)
            .collect(),
    }
}

fn change(at_ms: u64, page: usize) -> PageChange {
    PageChange {
        at: Duration::from_millis(at_ms),
        page,
    }
}

/// `narration_stop(true, …)`.
fn keep() -> StopRequest {
    StopRequest {
        save: true,
        ..StopRequest::default()
    }
}

/// `narration_stop(false, …)`.
fn discard() -> StopRequest {
    StopRequest::default()
}

/// Encodes `narration` and writes it into the note at `path`.
fn save_replay_narration(
    library: &Library,
    path: &Path,
    narration: &Narration,
    expected: Option<&FileStamp>,
) -> CommandResult<FileStamp> {
    save_replay_audio(
        library,
        path,
        &encode_narration(narration).unwrap(),
        expected,
    )
}

/// A microphone that has already received its whole clip.
struct Fake {
    clip: AudioClip,
}

impl Source for Fake {
    fn device_name(&self) -> &str {
        "Fake microphone"
    }

    fn level(&self) -> Level {
        Level::default()
    }

    fn elapsed(&self) -> Duration {
        self.clip.duration()
    }

    fn status(&self) -> CaptureStatus {
        CaptureStatus::Receiving
    }

    fn stop(self: Box<Self>) -> Result<AudioClip, MediaError> {
        Ok(self.clip)
    }
}

/// A runtime whose recordings get `clips` in turn (monitors get nothing), its backup copies in
/// `backup`.
fn fake_runtime(clips: Vec<AudioClip>, backup: &Path) -> NarrationRuntime {
    let next = AtomicUsize::new(0);
    NarrationRuntime::new(
        Box::new(move |_, record| {
            let clip = if record {
                let n = next.fetch_add(1, Ordering::Relaxed);
                clips[n % clips.len()].clone()
            } else {
                AudioClip::default()
            };
            Ok(Box::new(Fake { clip }) as Box<dyn Source>)
        }),
        backup.to_path_buf(),
    )
}

/// Starts recording a replay of the note at `path` (speed 2, from instant 250 ms).
fn start_replay(runtime: &NarrationRuntime, library: &Library, path: &Path, hub: &LiveHub) {
    let target = NarrationTarget::Replay {
        path: path_text(path),
        speed: 2.0,
        max_pause_ms: None,
        start_ms: 250.0,
    };
    start_in(
        runtime,
        None,
        target,
        || Ok(library.clone()),
        hub,
        Arc::new(|| None),
    )
    .unwrap();
}

/// The names in `dir`, sorted (none when it does not exist).
fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn shapes_are_those_of_video_ts() {
    let replay: NarrationTarget = serde_json::from_value(json!({
        "kind": "replay", "path": "C:\\N\\a.ardoise", "speed": 2,
        "maxPauseMs": 500, "startMs": -120.5
    }))
    .unwrap();
    assert_eq!(
        replay,
        NarrationTarget::Replay {
            path: "C:\\N\\a.ardoise".into(),
            speed: 2.0,
            max_pause_ms: Some(500.0),
            start_ms: -120.5,
        }
    );
    let untrimmed: NarrationTarget = serde_json::from_value(json!({
        "kind": "replay", "path": "a", "speed": 1, "maxPauseMs": null, "startMs": 0
    }))
    .unwrap();
    assert!(matches!(
        untrimmed,
        NarrationTarget::Replay {
            max_pause_ms: None,
            ..
        }
    ));
    let live: NarrationTarget = serde_json::from_value(json!({ "kind": "live" })).unwrap();
    assert_eq!(live, NarrationTarget::Live);

    assert_eq!(
        serde_json::to_value(NarrationStatus::idle()).unwrap(),
        json!({ "state": "idle", "capture": null, "level": { "peak": 0.0, "rms": 0.0 },
                "elapsedMs": 0, "device": null, "failure": null, "pending": false })
    );
    let recording = NarrationStatus {
        state: NarrationState::Recording,
        capture: Some(CaptureState::NoData),
        level: LevelState {
            peak: 0.5,
            rms: 0.25,
        },
        elapsed_ms: 1500,
        device: Some("Micro".into()),
        failure: None,
        pending: true,
    };
    let value = serde_json::to_value(recording).unwrap();
    assert_eq!(value["state"], "recording");
    assert_eq!(value["capture"], "noData");
    assert_eq!(value["level"], json!({ "peak": 0.5, "rms": 0.25 }));
    assert_eq!(value["elapsedMs"], 1500);
    assert_eq!(value["pending"], true);
    assert_eq!(
        serde_json::to_value(NarrationState::Monitoring).unwrap(),
        "monitoring"
    );

    let saved = NarrationSaved {
        stamp: None,
        duration_ms: 3000,
        pages: 2,
    };
    assert_eq!(
        serde_json::to_value(saved).unwrap(),
        json!({ "stamp": null, "durationMs": 3000, "pages": 2 })
    );
    let info = NarrationInfo::of(&NarrationHeader {
        channels: 1,
        base: NarrationBase::Replay,
        options: ReplayOptions {
            speed: 2.0,
            max_pause: Some(Duration::from_millis(500)),
        },
        start_ms: -40,
        duration: Duration::from_millis(1000),
    });
    assert_eq!(
        serde_json::to_value(info).unwrap(),
        json!({ "durationMs": 1000, "base": "replay", "speed": 2.0, "maxPauseMs": 500,
                "startMs": -40 })
    );
    let note = NarrationInfo::of(&NarrationHeader {
        channels: 2,
        base: NarrationBase::Note,
        options: ReplayOptions::default(),
        start_ms: 7,
        duration: Duration::from_millis(100),
    });
    assert_eq!(note.base, BaseName::Note);
    assert_eq!(
        serde_json::to_value(note).unwrap()["maxPauseMs"],
        json!(null)
    );
    let microphone = Microphone {
        id: "wasapi:1".into(),
        name: "Micro".into(),
        is_default: true,
    };
    assert_eq!(
        serde_json::to_value(microphone).unwrap(),
        json!({ "id": "wasapi:1", "name": "Micro", "isDefault": true })
    );
}

#[test]
fn capture_statuses_and_levels_map_to_the_interface() {
    assert_eq!(
        capture_state(&CaptureStatus::Waiting),
        (CaptureState::Waiting, None)
    );
    assert_eq!(
        capture_state(&CaptureStatus::NoData),
        (CaptureState::NoData, None)
    );
    assert_eq!(
        capture_state(&CaptureStatus::Receiving),
        (CaptureState::Receiving, None)
    );
    assert_eq!(
        capture_state(&CaptureStatus::Silent),
        (CaptureState::Silent, None)
    );
    assert_eq!(
        capture_state(&CaptureStatus::Failed("unplugged".into())),
        (CaptureState::Failed, Some("unplugged".into()))
    );
    for (state, text) in [
        (CaptureState::Waiting, "waiting"),
        (CaptureState::NoData, "noData"),
        (CaptureState::Receiving, "receiving"),
        (CaptureState::Silent, "silent"),
        (CaptureState::Failed, "failed"),
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), text);
    }
    assert_eq!(
        level_state(Level {
            peak: 1.5,
            rms: f32::NAN
        }),
        LevelState {
            peak: 1.0,
            rms: 0.0
        }
    );
    assert_eq!(
        level_state(Level {
            peak: 0.3,
            rms: 0.1
        }),
        LevelState {
            peak: 0.3,
            rms: 0.1
        }
    );
}

#[test]
fn microphone_errors_get_the_interface_codes() {
    let code = |error, no_device| start_error(error, no_device).code();
    assert_eq!(
        code(MediaError::MicrophoneNotFound("Micro USB".into()), false),
        MICROPHONE_NOT_FOUND
    );
    assert_eq!(code(MediaError::MicrophoneSilent, false), MICROPHONE_SILENT);
    let failed = || MediaError::Microphone {
        step: "opening the microphone",
        message: "this PC has no microphone".into(),
    };
    assert_eq!(code(failed(), false), MICROPHONE_FAILED);
    assert_eq!(code(failed(), true), NO_MICROPHONE);
    // An unknown device is not "no microphone at all".
    assert_eq!(
        code(MediaError::MicrophoneNotFound("x".into()), true),
        MICROPHONE_NOT_FOUND
    );
    assert_eq!(
        narration_error(MediaError::Encoding {
            codec: "Opus",
            message: "x".into()
        })
        .code(),
        MICROPHONE_FAILED
    );
    assert_eq!(
        narration_error(MediaError::MicrophoneSilent).code(),
        MICROPHONE_SILENT
    );
}

#[test]
fn replay_targets_are_checked() {
    assert_eq!(
        target_options(2.0, Some(500.0)).unwrap(),
        ReplayOptions {
            speed: 2.0,
            max_pause: Some(Duration::from_millis(500))
        }
    );
    assert_eq!(
        target_options(0.25, None).unwrap(),
        ReplayOptions {
            speed: 0.25,
            max_pause: None
        }
    );
    for (speed, pause) in [
        (0.2, None),
        (16.5, None),
        (f64::NAN, None),
        (1.0, Some(-1.0)),
        (1.0, Some(f64::INFINITY)),
    ] {
        assert_eq!(
            target_options(speed, pause).unwrap_err().code(),
            INVALID_VIDEO_SETTINGS,
            "{speed} {pause:?}"
        );
    }
    assert_eq!(target_start(-120.4).unwrap(), -120);
    assert!(target_start(f64::NAN).is_err());

    let (dir, library) = library();
    let saved = write_sample(&library, "", "Essai");
    let path = saved.path.to_string_lossy().into_owned();
    let target = replay_target(library.clone(), &path, 1.0, None, 250.0).unwrap();
    assert_eq!(target.start_ms, 250);
    let missing = dir.path().join("2026-10-24 Absente.ardoise");
    let error =
        replay_target(library.clone(), &missing.to_string_lossy(), 1.0, None, 0.0).unwrap_err();
    assert_eq!(error.code(), "noteNotFound");
    let elsewhere = tempfile::tempdir().unwrap();
    let outside = elsewhere.path().join("2026-10-24 X.ardoise");
    fs::write(&outside, "x").unwrap();
    let error = replay_target(library, &outside.to_string_lossy(), 1.0, None, 0.0).unwrap_err();
    assert_eq!(error.code(), "notInLibrary");
}

#[test]
fn the_page_log_keeps_the_changes_and_the_earliest_clock() {
    let mut log = PageLog::new(2);
    // No audio yet: no clock, but a page change counts from the start.
    log.observe(1_000.0, Duration::ZERO, Some(2));
    assert_eq!(log.anchor_ms, None);
    assert_eq!(log.wall_start_ms(990.0), 990.0);
    log.observe(1_010.0, Duration::ZERO, Some(3));
    assert_eq!(log.changes, [change(0, 3)]);
    // Audio arrives late (delivery delay): the smallest difference wins.
    log.observe(1_100.0, Duration::from_millis(60), Some(3));
    log.observe(1_150.0, Duration::from_millis(130), Some(3));
    log.observe(1_200.0, Duration::from_millis(150), None);
    assert_eq!(log.anchor_ms, Some(1_020.0));
    assert_eq!(log.wall_start_ms(0.0), 1_020.0);
    log.observe(1_250.0, Duration::from_millis(200), Some(4));
    log.observe(1_300.0, Duration::from_millis(250), Some(4));
    log.observe(1_310.0, Duration::from_millis(260), Some(5));
    // Changes before new audio: the last one counts, and back to page 4 is no change.
    log.observe(1_320.0, Duration::from_millis(260), Some(6));
    assert_eq!(log.changes, [change(0, 3), change(200, 4), change(260, 6)]);
    log.observe(1_330.0, Duration::from_millis(260), Some(4));
    log.observe(1_400.0, Duration::from_millis(330), Some(3));
    assert_eq!(log.changes, [change(0, 3), change(200, 4), change(330, 3)]);
    assert_eq!(log.anchor_ms, Some(1_020.0));
}

#[test]
fn a_live_recording_is_cut_per_page_in_note_time() {
    // 3 s of mono audio; pages 0, 1, 0 again, then 2 (no stroke) and 3.
    let clip = tone(3 * 48_000, 1);
    let changes = [
        change(0, 0),
        change(1000, 1),
        change(1500, 0),
        change(2000, 2),
        change(2500, 3),
    ];
    let wall_start = 1_792_843_200_000.0;
    let origins = [
        Some(1_792_843_200_400),
        Some(1_792_843_190_000),
        None,
        Some(1_792_843_203_000),
    ];
    let pieces = split_live_recording(clip.clone(), &changes, wall_start, &origins);
    let summary: Vec<(usize, i64, usize)> = pieces
        .iter()
        .map(|(page, n)| (*page, n.start_ms, n.clip.samples.len()))
        .collect();
    assert_eq!(
        summary,
        [
            // Begins 400 ms before the first stroke of page 0.
            (0, -400, 48_000),
            (1, 11_000, 24_000),
            (0, 1_100, 24_000),
            // Page 2 has no stroke: nothing. Page 3's first stroke comes after the end.
            (3, -500, 24_000),
        ]
    );
    for (_, narration) in &pieces {
        assert_eq!(narration.base, NarrationBase::Note);
        assert_eq!(narration.options, ReplayOptions::default());
        assert_eq!(narration.clip.channels, 1);
    }
    // The pieces put back together are the clip, minus what went to page 2.
    let kept: Vec<i16> = pieces
        .iter()
        .flat_map(|(_, n)| n.clip.samples.clone())
        .collect();
    let mut expected = clip.samples[..96_000].to_vec();
    expected.extend_from_slice(&clip.samples[120_000..]);
    assert_eq!(kept, expected);
}

#[test]
fn cuts_fall_on_whole_frames_and_out_of_range_changes_are_ignored() {
    // Stereo: a cut never separates the two channels of a frame.
    let clip = tone(4_800, 2);
    let changes = [change(0, 0), change(33, 1), change(500, 0)];
    let pieces = split_live_recording(clip.clone(), &changes, 1_000.0, &[Some(1_000), Some(900)]);
    assert_eq!(pieces.len(), 2);
    assert_eq!(pieces[0].1.clip.samples.len(), 2 * 1_584);
    assert_eq!(pieces[1].1.clip.samples.len(), 2 * (4_800 - 1_584));
    assert_eq!(pieces[1].1.start_ms, 133);
    assert_eq!(pieces[0].1.clip.channels, 2);
    // A page the hub does not know, an empty clip.
    assert!(split_live_recording(clip, &[change(0, 7)], 0.0, &[Some(0)]).is_empty());
    assert!(
        split_live_recording(AudioClip::default(), &[change(0, 0)], 0.0, &[Some(0)]).is_empty()
    );
}

#[test]
fn the_watcher_follows_the_pages_of_its_session() {
    let stop = AtomicBool::new(false);
    let tick = Cell::new(0u64);
    // Ticks of 50 ms of audio; the page changes at ticks 3 and 6, another session at 8.
    let position = || {
        let t = tick.get();
        let (session, page) = match t {
            0..=2 => (7, 0),
            3..=5 => (7, 1),
            6..=7 => (7, 2),
            _ => (8, 0),
        };
        Some(PagePosition {
            session,
            page,
            running: true,
        })
    };
    let audio = || Duration::from_millis(50 * tick.get());
    let seen = RefCell::new(0);
    let wall = || {
        let t = tick.get();
        *seen.borrow_mut() += 1;
        if t == 9 {
            stop.store(true, Ordering::Release);
        }
        tick.set(t + 1);
        5_000.0 + 50.0 * t as f64 + 3.0
    };
    let log = watch_pages(&stop, Duration::ZERO, 7, 0, &audio, &position, &wall);
    assert_eq!(log.changes, [change(0, 0), change(150, 1), change(300, 2)]);
    assert_eq!(log.anchor_ms, Some(5_003.0));
    assert_eq!(*seen.borrow(), 11, "one last look after the stop");
}

#[test]
fn nothing_to_stop_and_nothing_running() {
    let runtime = NarrationRuntime::default();
    let hub = LiveHub::new(HubConfig::default());
    assert_eq!(runtime.status(), NarrationStatus::idle());
    assert!(!runtime.is_recording_live());
    assert_eq!(stop_in(&runtime, &discard(), &hub).unwrap(), None);
    assert_eq!(
        stop_in(&runtime, &keep(), &hub).unwrap_err().code(),
        NO_RECORDING
    );
    assert!(finish_live(&runtime, &hub).is_none());
    // A live recording needs live mode (checked before the microphone is opened).
    let error = start_in(
        &runtime,
        None,
        NarrationTarget::Live,
        || Err(CommandError::new("noLibrary", "unused")),
        &hub,
        Arc::new(|| None),
    )
    .unwrap_err();
    assert_eq!(error.code(), "liveNotRunning");
    // A replay recording needs a library and a valid target.
    let error = start_in(
        &runtime,
        None,
        NarrationTarget::Replay {
            path: "a.ardoise".into(),
            speed: 1.0,
            max_pause_ms: None,
            start_ms: 0.0,
        },
        || Err(CommandError::new("noLibrary", "none")),
        &hub,
        Arc::new(|| None),
    )
    .unwrap_err();
    assert_eq!(error.code(), "noLibrary");
    assert_eq!(runtime.status(), NarrationStatus::idle());
}

#[test]
fn a_stale_stamp_is_a_conflict_and_writes_nothing() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Essai");
    // The note changes on disk after the editor opened it.
    let mut file = library.read(&saved.path).unwrap().file;
    file.note.title = "Changée ailleurs".into();
    let newer = library.save(&saved.path, &file, &saved.stamp).unwrap();
    let before = fs::read(&saved.path).unwrap();
    let narration = Narration {
        clip: tone(4_800, 1),
        base: NarrationBase::Replay,
        options: ReplayOptions::default(),
        start_ms: 0,
    };
    let error =
        save_replay_narration(&library, &saved.path, &narration, Some(&saved.stamp)).unwrap_err();
    assert_eq!(error.code(), "noteConflict");
    let error = delete_narration_in(&library, &saved.path, &saved.stamp).unwrap_err();
    assert_eq!(error.code(), "noteConflict");
    assert_eq!(fs::read(&saved.path).unwrap(), before);
    assert_eq!(FileStamp::of(&saved.path).unwrap(), newer);

    // Without narration, deleting it changes nothing.
    assert_eq!(
        delete_narration_in(&library, &saved.path, &newer).unwrap(),
        newer
    );
    assert_eq!(read_narration(&library, &saved.path).unwrap(), None);
    // A damaged member is reported as such.
    file.audio = Some(b"OggS but not really".to_vec());
    let damaged = library.save(&saved.path, &file, &newer).unwrap();
    assert_eq!(
        read_narration(&library, &saved.path).unwrap_err().code(),
        "noteDamaged"
    );
    // Deleting it works without decoding it.
    let stamp = delete_narration_in(&library, &saved.path, &damaged).unwrap();
    let after = library.read(&saved.path).unwrap();
    assert_eq!(after.stamp, stamp);
    assert_eq!(after.file.audio, None);
    assert_eq!(after.file.note.title, "Changée ailleurs");
    assert_eq!(after.file.note.strokes, file.note.strokes);
}

#[test]
fn the_microphone_is_remembered_per_machine() {
    let settings = tempfile::tempdir().unwrap();
    let context = context(settings.path());
    let state = set_microphone_in(&context, Some("wasapi:{abc}".into())).unwrap();
    assert_eq!(state.microphone(), Some("wasapi:{abc}"));
    let state = set_microphone_in(&context, Some("  ".into())).unwrap();
    assert_eq!(state.microphone(), None);
    let state = set_microphone_in(&context, None).unwrap();
    assert_eq!(state.microphone(), None);
}

// --- With the media pipeline (narration.rs of ardoise-media: Opus encoding) ---

#[test]
fn a_replay_narration_is_saved_into_its_note_and_read_back() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Essai");
    let before = library.read(&saved.path).unwrap().file;
    let narration = Narration {
        clip: simulated_narration(Duration::from_secs(2), 1),
        base: NarrationBase::Replay,
        options: ReplayOptions {
            speed: 2.0,
            max_pause: Some(Duration::from_millis(500)),
        },
        start_ms: 300,
    };
    let stamp =
        save_replay_narration(&library, &saved.path, &narration, Some(&saved.stamp)).unwrap();
    let after = library.read(&saved.path).unwrap();
    assert_eq!(after.stamp, stamp);
    assert_eq!(after.file.note.strokes, before.note.strokes);
    assert_eq!(after.file.thumbnail, before.thumbnail);
    let info = narration_info_in(&library, &saved.path).unwrap().unwrap();
    assert_eq!(info.base, BaseName::Replay);
    assert_eq!(
        (info.speed, info.max_pause_ms, info.start_ms),
        (2.0, Some(500), 300)
    );
    assert!((1990..=2010).contains(&info.duration_ms), "{info:?}");
    // The interface gets the stream as stored (the WebView plays Ogg Opus), never a WAV file.
    let opus = narration_opus(&library, &saved.path).unwrap();
    assert_eq!(&opus[..4], b"OggS");
    assert_eq!(Some(&opus), after.file.audio.as_ref());
    // The header says what decoding gives.
    let decoded = read_narration(&library, &saved.path).unwrap().unwrap();
    assert_eq!(info.duration_ms, duration_ms(decoded.duration()));

    // Recording again replaces it; deleting it removes it.
    let shorter = Narration {
        clip: simulated_narration(Duration::from_secs(1), 1),
        ..narration
    };
    let stamp = save_replay_narration(&library, &saved.path, &shorter, Some(&stamp)).unwrap();
    let info = narration_info_in(&library, &saved.path).unwrap().unwrap();
    assert!(info.duration_ms < 1100, "{info:?}");
    let stamp = delete_narration_in(&library, &saved.path, &stamp).unwrap();
    assert_eq!(read_narration(&library, &saved.path).unwrap(), None);
    assert_eq!(narration_info_in(&library, &saved.path).unwrap(), None);
    assert_eq!(
        narration_opus(&library, &saved.path).unwrap_err().code(),
        NO_NARRATION
    );

    // A damaged member: the editor's line says so, without decoding anything.
    let mut file = library.read(&saved.path).unwrap().file;
    file.audio = Some(b"OggS but not really".to_vec());
    library.save(&saved.path, &file, &stamp).unwrap();
    assert_eq!(
        narration_info_in(&library, &saved.path).unwrap_err().code(),
        "noteDamaged"
    );
    assert_eq!(
        narration_opus(&library, &saved.path).unwrap_err().code(),
        "noteDamaged"
    );
}

#[test]
fn the_first_sample_is_placed_on_the_replay_by_the_wall_clock() {
    // First sample 5.25 s after the replay was at instant 0.
    assert_eq!(
        replay_start_ms(1_792_843_205_250.0, 1_792_843_200_000.0),
        5_250
    );
    // The recording began before the replay: a negative instant.
    assert_eq!(
        replay_start_ms(1_792_843_199_000.0, 1_792_843_200_000.0),
        -1_000
    );
    // Rounded to the nearest millisecond.
    assert_eq!(
        replay_start_ms(1_792_843_200_012.6, 1_792_843_200_000.0),
        13
    );
    assert_eq!(
        replay_start_ms(1_792_843_200_012.4, 1_792_843_200_000.0),
        12
    );
    assert_eq!(replay_start_ms(1_000.5, 0.0), 1_001);
    // The interface's clock is checked.
    assert_eq!(
        replay_clock(1_792_843_200_000.0).unwrap(),
        1_792_843_200_000.0
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 1e16] {
        assert_eq!(
            replay_clock(bad).unwrap_err().code(),
            INVALID_VIDEO_SETTINGS,
            "{bad}"
        );
    }
}

#[test]
fn a_replay_recording_is_placed_by_the_replay_clock() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Essai");
    let backup = tempfile::tempdir().unwrap();
    let runtime = fake_runtime(vec![tone(48_000, 1)], backup.path());
    let hub = LiveHub::new(HubConfig::default());

    // Without the replay's clock: the target's instant.
    start_replay(&runtime, &library, &saved.path, &hub);
    assert_eq!(runtime.status().state, NarrationState::Recording);
    let request = StopRequest {
        stamp: Some(saved.stamp),
        ..keep()
    };
    let result = stop_in(&runtime, &request, &hub).unwrap().unwrap();
    assert_eq!((result.duration_ms, result.pages), (1000, 1));
    let info = narration_info_in(&library, &saved.path).unwrap().unwrap();
    assert_eq!(
        (info.base, info.speed, info.start_ms, info.duration_ms),
        (BaseName::Replay, 2.0, 250, 1000)
    );

    // With it: the wall clock of the first sample (the stand-in had received its second of
    // sound at the watcher's first look) less the wall clock of the replay's instant 0.
    let replay_started_at = wall_now_ms() - 5_000.0;
    start_replay(&runtime, &library, &saved.path, &hub);
    let request = StopRequest {
        stamp: result.stamp,
        replay_started_at_ms: Some(replay_started_at),
        ..keep()
    };
    stop_in(&runtime, &request, &hub).unwrap().unwrap();
    let info = narration_info_in(&library, &saved.path).unwrap().unwrap();
    assert!((3_990..=4_500).contains(&info.start_ms), "{info:?}");
    assert_eq!(info.speed, 2.0);
    assert!(!runtime.has_pending());
    assert!(names(backup.path()).is_empty());

    // A bad clock is refused before anything stops.
    start_replay(&runtime, &library, &saved.path, &hub);
    let request = StopRequest {
        replay_started_at_ms: Some(f64::NAN),
        ..keep()
    };
    assert_eq!(
        stop_in(&runtime, &request, &hub).unwrap_err().code(),
        INVALID_VIDEO_SETTINGS
    );
    assert_eq!(runtime.status().state, NarrationState::Recording);
    // Discarding ignores it.
    let request = StopRequest {
        replay_started_at_ms: Some(f64::NAN),
        ..discard()
    };
    assert_eq!(stop_in(&runtime, &request, &hub).unwrap(), None);
    assert_eq!(runtime.status(), NarrationStatus::idle());
}

#[test]
fn a_replay_recording_goes_to_the_path_given_at_stop() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Essai");
    let backup = tempfile::tempdir().unwrap();
    let runtime = fake_runtime(vec![tone(4_800, 1)], backup.path());
    let hub = LiveHub::new(HubConfig::default());

    // The editor renamed the note while recording.
    start_replay(&runtime, &library, &saved.path, &hub);
    let renamed = saved.path.with_file_name("2026-10-24 Renommée.ardoise");
    fs::rename(&saved.path, &renamed).unwrap();
    let request = StopRequest {
        stamp: Some(saved.stamp),
        path: Some(path_text(&renamed)),
        ..keep()
    };
    let result = stop_in(&runtime, &request, &hub).unwrap().unwrap();
    let after = library.read(&renamed).unwrap();
    assert_eq!(result.stamp, Some(after.stamp));
    assert!(after.file.audio.is_some());
    assert!(!saved.path.exists());

    // A path outside the library: refused, and the recording is kept.
    start_replay(&runtime, &library, &renamed, &hub);
    let elsewhere = tempfile::tempdir().unwrap();
    let request = StopRequest {
        path: Some(path_text(&elsewhere.path().join("2026-10-24 X.ardoise"))),
        ..keep()
    };
    assert_eq!(
        stop_in(&runtime, &request, &hub).unwrap_err().code(),
        "notInLibrary"
    );
    assert!(runtime.has_pending());
    // Its copy is named after the note it was recorded for.
    let copies = names(backup.path());
    assert_eq!(copies.len(), 1);
    assert!(copies[0].starts_with("2026-10-24 Renommée 2"), "{copies:?}");
}

#[test]
fn a_replay_narration_that_cannot_be_saved_is_kept() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Essai");
    let backup = tempfile::tempdir().unwrap();
    let runtime = fake_runtime(vec![tone(24_000, 1)], backup.path());
    let hub = LiveHub::new(HubConfig::default());

    // The note changes on disk while recording: the editor's stamp is stale.
    start_replay(&runtime, &library, &saved.path, &hub);
    let mut file = library.read(&saved.path).unwrap().file;
    file.note.title = "Changée ailleurs".into();
    let newer = library.save(&saved.path, &file, &saved.stamp).unwrap();
    let before = fs::read(&saved.path).unwrap();
    let request = StopRequest {
        stamp: Some(saved.stamp),
        ..keep()
    };
    assert_eq!(
        stop_in(&runtime, &request, &hub).unwrap_err().code(),
        "noteConflict"
    );
    assert_eq!(fs::read(&saved.path).unwrap(), before);
    let status = runtime.status();
    assert_eq!(status.state, NarrationState::Idle);
    assert!(status.pending);
    assert_eq!(serde_json::to_value(&status).unwrap()["pending"], true);

    // A copy of its audio.opus, named after the note, outside the library.
    let copies = names(backup.path());
    assert_eq!(copies.len(), 1);
    let stem = saved
        .path
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(
        copies[0].starts_with(&format!("{stem} 2")) && copies[0].ends_with(".opus"),
        "{copies:?}"
    );
    let copy = fs::read(backup.path().join(&copies[0])).unwrap();
    let narration = decode_narration(&copy).unwrap();
    assert_eq!((narration.start_ms, narration.options.speed), (250, 2.0));
    assert_eq!(narration.clip.samples.len(), 24_000);

    // The editor renamed the note since: a retry at the old path fails and keeps it...
    let renamed = saved.path.with_file_name("2026-10-24 Renommée.ardoise");
    fs::rename(&saved.path, &renamed).unwrap();
    let old = path_text(&saved.path);
    assert_eq!(
        retry_in(&runtime, &library, &old, Some(&newer))
            .unwrap_err()
            .code(),
        "noteNotFound"
    );
    assert!(runtime.has_pending());
    // ...one with a stale stamp too...
    let at = path_text(&renamed);
    assert_eq!(
        retry_in(&runtime, &library, &at, Some(&saved.stamp))
            .unwrap_err()
            .code(),
        "noteConflict"
    );
    assert!(runtime.has_pending());
    // ...and one at its new path saves it, then removes the copy.
    let result = retry_in(&runtime, &library, &at, Some(&newer)).unwrap();
    assert_eq!((result.duration_ms, result.pages), (500, 1));
    let after = library.read(&renamed).unwrap();
    assert_eq!(result.stamp, Some(after.stamp));
    assert_eq!(after.file.note.title, "Changée ailleurs");
    assert_eq!(after.file.audio.as_deref(), Some(&copy[..]));
    assert!(!runtime.has_pending());
    assert!(names(backup.path()).is_empty());
    assert_eq!(
        retry_in(&runtime, &library, &at, None).unwrap_err().code(),
        NO_PENDING_NARRATION
    );
}

#[test]
fn a_pending_narration_can_be_discarded_and_a_newer_one_replaces_it() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Essai");
    let backup = tempfile::tempdir().unwrap();
    let runtime = fake_runtime(vec![tone(4_800, 1), tone(9_600, 1)], backup.path());
    let hub = LiveHub::new(HubConfig::default());

    // The note goes away while recording, twice: the newer recording is the one kept.
    start_replay(&runtime, &library, &saved.path, &hub);
    let moved = saved.path.with_file_name("moved.bin");
    fs::rename(&saved.path, &moved).unwrap();
    assert_eq!(
        stop_in(&runtime, &keep(), &hub).unwrap_err().code(),
        "noteNotFound"
    );
    fs::rename(&moved, &saved.path).unwrap();
    start_replay(&runtime, &library, &saved.path, &hub);
    // Recording again while one is pending is allowed.
    assert!(runtime.status().pending);
    fs::rename(&saved.path, &moved).unwrap();
    assert_eq!(
        stop_in(&runtime, &keep(), &hub).unwrap_err().code(),
        "noteNotFound"
    );
    assert_eq!(names(backup.path()).len(), 2);
    fs::rename(&moved, &saved.path).unwrap();
    let at = path_text(&saved.path);
    let result = retry_in(&runtime, &library, &at, None).unwrap();
    assert_eq!(result.duration_ms, 200);
    // The older copy stays, as a last resort.
    assert_eq!(names(backup.path()).len(), 1);

    // Discarded: nothing pending any more, its copy stays.
    let runtime = fake_runtime(vec![tone(4_800, 1)], backup.path());
    start_replay(&runtime, &library, &saved.path, &hub);
    fs::rename(&saved.path, &moved).unwrap();
    assert!(stop_in(&runtime, &keep(), &hub).is_err());
    assert!(runtime.has_pending());
    assert_eq!(names(backup.path()).len(), 2);
    discard_in(&runtime);
    assert!(!runtime.has_pending());
    assert_eq!(names(backup.path()).len(), 2);
    assert_eq!(
        retry_in(&runtime, &library, &at, None).unwrap_err().code(),
        NO_PENDING_NARRATION
    );
    // Discarding nothing is harmless.
    discard_in(&runtime);
}

#[test]
fn stopping_the_monitor_never_stops_a_recording() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Essai");
    let backup = tempfile::tempdir().unwrap();
    let runtime = fake_runtime(vec![tone(4_800, 1)], backup.path());
    let hub = LiveHub::new(HubConfig::default());

    // Nothing running: nothing happens.
    monitor_stop_in(&runtime);
    assert_eq!(runtime.status(), NarrationStatus::idle());
    // A monitor stops.
    monitor_in(&runtime, None).unwrap();
    let status = runtime.status();
    assert_eq!(status.state, NarrationState::Monitoring);
    assert_eq!(status.device.as_deref(), Some("Fake microphone"));
    monitor_stop_in(&runtime);
    assert_eq!(runtime.status().state, NarrationState::Idle);
    // A recording goes on, and is saved whole.
    start_replay(&runtime, &library, &saved.path, &hub);
    monitor_stop_in(&runtime);
    assert_eq!(runtime.status().state, NarrationState::Recording);
    assert_eq!(
        monitor_in(&runtime, None).unwrap_err().code(),
        NARRATION_BUSY
    );
    assert_eq!(runtime.status().state, NarrationState::Recording);
    let result = stop_in(&runtime, &keep(), &hub).unwrap().unwrap();
    assert_eq!(result.duration_ms, 100);
}

#[test]
fn a_recording_of_nothing_is_an_empty_recording() {
    let (_dir, library) = library();
    let saved = write_sample(&library, "", "Essai");
    let backup = tempfile::tempdir().unwrap();
    let runtime = fake_runtime(vec![AudioClip::default()], backup.path());
    let hub = LiveHub::new(HubConfig::default());
    start_replay(&runtime, &library, &saved.path, &hub);
    assert_eq!(
        stop_in(&runtime, &keep(), &hub).unwrap_err().code(),
        EMPTY_RECORDING
    );
    // Nothing to keep, and nothing recording any more.
    assert!(!runtime.has_pending());
    assert!(names(backup.path()).is_empty());
    assert_eq!(
        stop_in(&runtime, &keep(), &hub).unwrap_err().code(),
        NO_RECORDING
    );
    assert_eq!(library.read(&saved.path).unwrap().file.audio, None);
}

#[test]
fn backup_copies_live_outside_the_library() {
    let local = backup_dir_in(Some(OsString::from(r"C:\Users\prof\AppData\Local")));
    assert_eq!(
        local,
        Path::new(r"C:\Users\prof\AppData\Local")
            .join("Ardoise")
            .join("narration-backup")
    );
    let elsewhere = std::env::temp_dir()
        .join("Ardoise")
        .join("narration-backup");
    assert_eq!(backup_dir_in(None), elsewhere);
    assert_eq!(backup_dir_in(Some(OsString::new())), elsewhere);
    assert!(backup_dir().ends_with(Path::new("Ardoise").join("narration-backup")));

    let note = Path::new("Notes").join("2026-10-24 Leçon.ardoise");
    assert_eq!(
        backup_name(&note, "2026-10-24 153012", 1),
        "2026-10-24 Leçon 2026-10-24 153012.opus"
    );
    assert_eq!(
        backup_name(&note, "2026-10-24 153012", 3),
        "2026-10-24 Leçon 2026-10-24 153012-3.opus"
    );
    let long = Path::new(&format!("{}.ardoise", "é".repeat(400))).to_path_buf();
    let name = backup_name(&long, "2026-10-24 153012", 1);
    assert_eq!(name.chars().count(), 150 + " 2026-10-24 153012.opus".len());
    assert_eq!(
        backup_name(Path::new(""), "2026-10-24 153012", 1),
        "narration 2026-10-24 153012.opus"
    );

    // Two copies in the same second get two names.
    let dir = tempfile::tempdir().unwrap();
    let first = write_narration_backup(&dir.path().join("sub"), &note, b"one").unwrap();
    let second = write_narration_backup(&dir.path().join("sub"), &note, b"two").unwrap();
    assert_ne!(first, second);
    assert_eq!(fs::read(&first).unwrap(), b"one");
    assert_eq!(fs::read(&second).unwrap(), b"two");
}

#[test]
fn a_recording_on_one_page_keeps_its_buffer() {
    let clip = tone(48_000, 1);
    let buffer = clip.samples.as_ptr();
    let pieces = split_live_recording(clip, &[change(0, 0)], 1_000.0, &[Some(1_000)]);
    assert_eq!(pieces.len(), 1);
    assert_eq!(pieces[0].1.clip.samples.as_ptr(), buffer);
    assert_eq!(pieces[0].1.clip.samples.len(), 48_000);
    let merged = merge_by_page(pieces);
    assert_eq!(merged[0].1.clip.samples.as_ptr(), buffer);
}

/// Not a test: puts a long synthetic narration into an existing note, for the manual check of
/// the replay's sound (M10, `docs/design/media.md`). Run it in release, the encoder is slow
/// otherwise:
/// `ARDOISE_LONG_NARRATION_NOTE=<note.ardoise> [ARDOISE_LONG_NARRATION_SECS=3600]
/// cargo test -p ardoise --release --lib -- --ignored writes_a_long_narration`.
#[test]
#[ignore = "manual check helper"]
fn writes_a_long_narration_into_a_note() {
    let Some(path) = std::env::var_os("ARDOISE_LONG_NARRATION_NOTE") else {
        return;
    };
    let path = PathBuf::from(path);
    let secs: u64 = std::env::var("ARDOISE_LONG_NARRATION_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3600);
    let started = std::time::Instant::now();
    let narration = Narration {
        clip: simulated_narration(Duration::from_secs(secs), 1),
        base: NarrationBase::Replay,
        options: ReplayOptions::default(),
        start_ms: 0,
    };
    let bytes = encode_narration(&narration).unwrap();
    let mut file = ardoise_core::format::read_note(&path).unwrap();
    file.audio = Some(bytes);
    ardoise_core::format::write_note(&path, &file).unwrap();
    println!(
        "{secs} s of narration ({} bytes) written into {} in {:?}",
        file.audio.as_ref().map_or(0, Vec::len),
        path.display(),
        started.elapsed()
    );
}
