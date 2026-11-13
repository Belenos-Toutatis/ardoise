# Replay, animated exports and narration (milestone M6)

Developer document (English). Requirements: SPEC §4.5, §4.6, §6.2 (rows "Vidéo MP4", "Vidéo WebM", "GIF / WebP",
"Micro"), §8; ROADMAP M6. Research: `docs/research/media-stack.md` (§7c: the Media Foundation spike, which works).

## Pieces

```
Note ──► replay::Timeline ──► replay::FrameRenderer ──► RGBA frames ─┐
          (speed, pauses)      (incremental drawing)                  ├─► dyn VideoSink ──► .mp4 / .gif / .webp / .webm
microphone::Recorder ──► AudioClip (48 kHz) ─────────────────────────┘    (one module per format)
```

All of it lives in `crates/ardoise-media`, except the incremental drawing primitives, which belong in
`crates/ardoise-render` (one drawing of the strokes: SPEC §6.2).

## Contracts (fixed; change only with a reason in your report)

`crates/ardoise-media/src/sink.rs`:

- `VideoSink` (`push_frame(rgba, timestamp)`, `push_audio(samples)`, `finish() -> VideoSummary`): every format
  implements it. Frames are opaque RGBA; timestamps start at zero and strictly increase; the last frame lasts
  `1 / frame_rate`. Audio is interleaved `i16` at `NARRATION_SAMPLE_RATE` (48 kHz), pushed with or before the
  frame it belongs to. Formats without sound (GIF, WebP) ignore audio.
- `VideoSettings { width, height, frame_rate, audio_channels }`, shared by every format; each format adds its own
  options struct (quality, palette, speed preset…).
- `AnimationProbe` / `AudioProbe`: a file read back **independently** of its writer (a decoder or demuxer, never
  the writer's own counters), for the tests and the end-of-milestone criterion.
- `AudioClip { channels, samples }` at 48 kHz: recorded narration.

Each format module exposes `create_<format>(path, &VideoSettings, &<Format>Options) -> Result<Box<dyn VideoSink>,
MediaError>` and `probe_<format>(path) -> Result<AnimationProbe, MediaError>`. Errors are `MediaError` variants
(add variants; keep the existing ones). No `unwrap`/`expect` outside tests.

## Modules and owners (one agent per row, disjoint files)

| Module | Files | Content |
|---|---|---|
| Replay | `ardoise-media/src/replay.rs` (+ `replay/`), `ardoise-render` (new incremental API) | `Timeline` (speed, trimmed pauses, mapping replay time ↔ note time), `FrameRenderer` (frames at any size, drawn incrementally) |
| Animated formats | `ardoise-media/src/{gif,webp,webm,opus}.rs` (or folders) | GIF (`gif` + colour quantisation, never `gifski`: AGPL), WebP (`webp-animation`), WebM (AV1 by `rav1e` + `webm`, sound in Opus), the Opus encoder shared with the MP4 fallback |
| Microphone | `ardoise-media/src/microphone.rs` (+ folder) | list, choose, level meter, record to `AudioClip` (`cpal` WASAPI + `rubato`), silence detection (Windows privacy setting) |
| MP4 | `ardoise-media/src/mp4.rs`, `openh264/` | `create_mp4` choosing Media Foundation (default) or the OpenH264 2.6.0 fallback (Cisco download, SHA-256 checked, `shiguredo_mp4`, Opus sound); `VideoSink` for `Mp4Writer` |

`lib.rs` and `Cargo.toml` are shared: add only your own `mod`/`pub use` lines and dependencies; the main session
resolves the merge.

## Status (2026-10-24) and the remaining work

Done and merged: Media Foundation MP4 writer (`Mp4Writer`, `probe_mp4`), replay (`replay::Timeline`,
`replay::FrameRenderer`, `ardoise_render::PageCanvas`), GIF/WebP/WebM sinks and probes, `OpusEncoder`, microphone
(`microphone::Recorder`, `simulated_narration`).

Contract for the rest (`crates/ardoise-media/src/mp4.rs`): `create_mp4(path, &VideoSettings, &Mp4Options,
Option<&OpenH264Codec>)` (`Mp4Backend::{Auto, MediaFoundation, OpenH264}`; `None` = the user did not allow the
fallback), `probe_mp4_animation`, `OpenH264Codec::{new(dir), status, download(progress, cancel), remove}`,
`CISCO_NOTICE`, `OPENH264_VERSION`, `MediaError::CodecUnavailable`. The Media Foundation path works; the OpenH264
path and the download answer `CodecUnavailable` until the MP4 work package fills them in.

Two work packages run in parallel on disjoint files:

1. **MP4 (Windows PC)**: `mp4.rs` + an `openh264/` folder: the fallback (Cisco download over HTTPS from
   `ciscobinary.openh264.org`, bzip2, SHA-256 pinned, atomic install, `openh264` crate with `libloading`, MP4 muxing
   with Opus sound), `probe_mp4_animation` reading both back ends, the delay-loading of `mfplat.dll` and
   `mfreadwrite.dll` (`app/src-tauri/build.rs`, so that the exe starts on Windows N editions), and the M6
   end-of-milestone test (`crates/ardoise-media/tests/`): a reference note replayed into the 4 formats, each read
   back (duration, size, frames); an MP4 with `simulated_narration` whose sound track has the right length; the
   OpenH264 fallback forced.
2. **Interface (cloud session)**: everything else of M6 in `app/`:
   - `crates/ardoise-media/src/export.rs`: the export pipeline (note + `ReplayOptions` + format options +
     optional narration + final frame held N seconds → frames from `FrameRenderer` pushed with their audio into
     the format's sink; progress; cancel), and `estimate.rs` (estimated export time shown before exporting, from
     the speeds measured on the development PC at 1080p 30 fps: GIF ≈ 310 fps, WebP ≈ 24 fps, WebM ≈ 20 fps,
     MP4 Media Foundation ≈ 100 fps hardware / ≈ 270 fps software, scaled by the pixel count; drawing is negligible).
   - App commands (export with progress and cancel, estimate, microphones, narration recording with level and
     status, OpenH264 status/download/remove and the setting that allows it), narration storage (next to the note,
     atomic, Nextcloud-safe; documented in `docs/file-format.md` without breaking older readers).
   - Editor replay (timeline, play/pause, speeds 0.25×–16×, trim pauses), the video export dialog (formats with
     strengths, limits and a « Quel format choisir ? » table as in the image export dialog; resolution 720p /
     1080p / 4K / page size; frames per second; target duration or speed; background or transparent for GIF/WebP;
     final frame held N s; estimated time; progress and cancel), narration recording during replay and in live
     mode (microphone choice, level meter, the « Windows bloque le micro » explanation), the OpenH264 setting with
     `CISCO_NOTICE` and its licence. FR and EN texts; e2e steps for the replay and a GIF export.

## Replay semantics

- Note time: `Point::t`, milliseconds from `Note::created`. Replay time: what the viewer sees.
- `speed` (0.25 to 16, SPEC §4.5) divides durations. Trimming pauses ("suppression des temps morts") caps every gap between
  two strokes at `max_pause` (e.g. 0.5 s) before applying the speed; gaps inside a stroke are kept.
- The replay of a note with `Timing::Synthesized` is approximate (imported Inkspace notes): same rules.
- Hidden layers are not drawn. A stroke appears point by point; outlines come from `ardoise-ink` exactly as on
  screen and in the PNG export.
- Page objects (text frames, format 1.4): an object of a visible layer, of a kind that is drawn, counts as a stroke of one
  sample at its time `t` (duration, trimmed pauses) and appears all at once then, in the exports' drawing order (strokes of
  its layer under it): `docs/design/page-objects.md` §3.
- The mapping must stay simple and pure: the editor's on-screen replay (TypeScript, later) uses the same rules
  and will reuse it through the WebAssembly build if needed.
- Narration keeps the time base it was recorded in (replay time with its options, or note time in live mode);
  the export plays it back in that base. Wiring comes with the interface, after M4.

## Export dialog: help to choose a format

The user asked (2026-10-24) for help choosing between formats, images and videos alike. Each format in the export
dialogs shows what it is good for **and** its drawbacks, readable without hovering (the app is also used with a
pen or a touch screen): a short line under each choice, plus a "Quel format choisir ?" panel comparing them
(plays where, sound, transparency, file size, export time). FR and EN texts. Points to cover for the videos:

- MP4: plays everywhere (Windows, phones, PowerPoint, ENT), sound; no transparency.
- GIF: plays without a player (e-mail, web page, slide), loops; no sound, 256 colours, large files.
- WebP: like GIF but smaller and sharper, transparency; no sound, some older programs do not open it.
- WebM: open format, sound, small files, plays in browsers and Moodle; slow export (AV1), not every Windows
  player opens it.

## Performance (SPEC §8)

MP4 1080p of a 2-minute replay in under 2 minutes: 3600 frames at 30 fps, so drawing plus encoding must stay
under ~30 ms per frame in release. Redrawing the whole note for every frame is too slow for long notes: draw the
paper once, then only what each frame adds (finished strokes stay on a cached canvas).

## Build tools

`rav1e` needs `nasm` on the build machine for a usable AV1 speed (the user agreed to its download on 2026-10-24,
for the development PC). The interface never downloads it. Cloud sessions get it from apt (`scripts/cloud-setup.sh`).

## Interface package: contracts (cloud session, 2026-10-24)

Fixed before the work was split; change only with a reason in your report. Owners in the last table.

### Narration storage: `audio.opus` inside the note

The narration is the optional `audio.opus` member of the `.ardoise` file (SPEC §3.3), so it travels with the note
(rename, move, trash, Nextcloud) and is written with it, atomically, through the library's usual checked save
(`Library::save` with the stamp: a version changed meanwhile is a conflict, never overwritten). The format code keeps
treating it as opaque bytes: no format version change, and 1.0/1.1 readers keep the member verbatim when they
rewrite the note. Its content is now specified (`docs/file-format.md` §4):

- an Ogg Opus stream (RFC 7845, RFC 3533): `OpusHead` page, `OpusTags` page, then audio pages; 48 kHz, mono (stereo
  read), 20 ms packets, granule positions per RFC 7845 (end trimming on the last page), so VLC or a browser plays it;
- `OpusTags` vendor `Ardoise`, user comments (all optional on read; defaults in brackets):
  `ARDOISE_BASE=replay|note` [`replay`], `ARDOISE_SPEED=<float>` [1], `ARDOISE_MAX_PAUSE_MS=<integer>|none` [none],
  `ARDOISE_START_MS=<signed integer>` [0].

Time base (see "Replay semantics"): a narration recorded during a replay is in the replay time of the options it was
recorded with (`base=replay`, its speed and trimming); a narration recorded in live mode is in note time (`base=note`,
speed 1, no trimming). `START_MS` is the replay instant, in that time base, of the first sample; negative when the
recording began before the note's time origin (live mode: the page's time origin is its first stroke).

### `ardoise-media` (Rust)

`narration.rs`:

```rust
pub enum NarrationBase { Replay, Note }
pub struct Narration {
    pub clip: AudioClip,          // 48 kHz
    pub base: NarrationBase,
    pub options: ReplayOptions,   // the time base (Note: speed 1, max_pause None)
    pub start_ms: i64,            // replay instant of the first sample in that base
}
impl Narration {
    pub fn duration(&self) -> Duration;
    pub fn end_ms(&self) -> i64;                       // start_ms + duration, rounded up
    pub fn merge(self, later: Narration) -> Narration;  // same base and options: one track, silence between
}
pub fn encode_narration(narration: &Narration) -> Result<Vec<u8>, MediaError>; // audio.opus bytes
pub fn decode_narration(bytes: &[u8]) -> Result<Narration, MediaError>;       // InvalidFile when not Ogg Opus
pub fn wav_bytes(clip: &AudioClip) -> Vec<u8>;          // 16-bit PCM WAV, for the editor to play the narration
```

`export.rs` (the pipeline; one call = one file):

```rust
pub enum FormatOptions { Mp4(Mp4Options), Gif(GifOptions), WebP(WebpOptions), WebM(WebmOptions) }
pub struct AnimationExport {
    pub width: u32, pub height: u32,   // even for MP4/WebM (the pipeline refuses odd sizes: InvalidSettings)
    pub frame_rate: u32,
    pub replay: ReplayOptions,         // replaced by the narration's options when a narration is given
    pub backdrop: Backdrop,            // Transparent only for GIF and WebP (InvalidSettings otherwise)
    pub hold_end: Duration,            // the final frame stays this long (0..=60 s)
    pub format: FormatOptions,
}
pub struct ExportPlan {
    pub replay: ReplayOptions,         // what is actually used
    pub lead_in: Duration,             // blank page before the replay: -start_ms of a narration starting before 0
    pub replay_duration: Duration,
    pub duration: Duration,            // lead_in + replay + max(hold_end, narration past the replay's end)
    pub frames: u64,                   // frames pushed: duration × frame_rate, at least 1
    pub audio_start: Option<Duration>, // video time of the narration's first sample (None: no sound track)
}
pub struct ExportProgress { pub frames_done: u64, pub frames_total: u64 }
pub fn plan_export(note: &Note, export: &AnimationExport, narration: Option<&Narration>) -> Result<ExportPlan, MediaError>;
pub fn export_animation(
    note: &Note, export: &AnimationExport, narration: Option<&Narration>, path: &Path,
    openh264: Option<&OpenH264Codec>, progress: &mut dyn FnMut(ExportProgress), cancel: &AtomicBool,
) -> Result<VideoSummary, MediaError>;
pub fn page_frame_size(page: PageSize, long_side: u32) -> (u32, u32); // "taille de la page": even, page proportions
```

- Video time `v` of frame `k` is `frame_time(k, frame_rate)`; it shows replay instant `v - lead_in` (clamped at 0)
  through one `FrameRenderer` (opaque frames: `Backdrop::Paper`/`White`; straight alpha with `Transparent`).
- Narration is ignored by GIF and WebP (`audio_channels: None`, no sound pushed) and by a format without sound; for
  MP4/WebM it is pushed in chunks interleaved with the frames: before frame `k`, the samples up to the end of frame `k`
  (silence before `audio_start`), then whatever remains before `finish`.
- The file is written to a hidden temporary name next to `path` (`.<name>.<pid>.<n>.part`, like `fsutil`) and renamed
  over `path` only when the sink has finished; on error or cancel the temporary file is removed and `path` untouched.
- `cancel` is checked before every frame: `MediaError::Cancelled`. `progress` is called after every frame.

`estimate.rs`:

```rust
pub fn estimate_export_time(format: AnimationFormat, width: u32, height: u32, frames: u64) -> Duration;
```

Throughput measured on the development PC at 1080p (1920 × 1080), scaled by the pixel count: GIF 310 fps, WebP
24 fps, WebM 20 fps, MP4 100 fps (Media Foundation hardware: the slower of its two figures, as the encoder used is
not known in advance); plus a fixed 0.5 s to open and close the file. Drawing is negligible.

`MediaError::Cancelled` is added (the app maps it to `cancelled`).

### App commands (Rust, `app/src-tauri/src/`)

Errors keep the `{ code, message }` shape. New codes (translated in `errorCodes.*`): `cancelled` (exists),
`videoExportFailed`, `invalidVideoSettings`, `codecUnavailable` (MP4 without Media Foundation nor OpenH264),
`microphoneNotFound`, `microphoneSilent` (Windows blocks the microphone), `microphoneFailed`, `noMicrophone`,
`narrationBusy` (a recording or monitor already runs), `noRecording`, `openh264DownloadFailed`, `liveNotRunning`
(exists), `noteConflict` (exists).

`video.rs` (`VideoExportOptions` etc. are the TypeScript types of `app/ui/src/lib/video.ts`, camelCase):

| Command | Arguments | Returns |
|---|---|---|
| `estimate_video_export` | `source: ExportSource`, `options: VideoExportOptions` | `VideoEstimate` |
| `export_video` | `source`, `options`, `destination: string`, `onProgress: Channel<VideoProgress>` | `VideoExportResult` |
| `cancel_video_export` | — | — |

The narration of a source comes from the note file at `source.path` (its `audio.opus`), even when `source.note`
carries the editor's unsaved strokes. `options.narration` false, or a format without sound, exports without it.
Resolution `720p` = 1280 × 720, `1080p` = 1920 × 1080, `4k` = 3840 × 2160 (the page fitted, bands of paper colour
around it, `FrameRenderer`), `page` = `page_frame_size(page, 1920)`. MP4 uses `Mp4Backend::Auto` with the OpenH264
codec only when the setting allows it. Progress is sent at most 10 times a second plus once at the end.

`narration.rs` (one microphone capture at a time in the app):

| Command | Arguments | Returns |
|---|---|---|
| `list_microphones` | — | `Microphone[]` |
| `set_microphone` | `device: string \| null` | `AppState` (remembered per PC: `[machines.X] microphone`) |
| `microphone_monitor` | `device: string \| null` | — (level only, for the settings screen and before recording) |
| `narration_start` | `device: string \| null`, `target: NarrationTarget` | — (stops a monitor first) |
| `narration_status` | — | `NarrationStatus` (poll about 10 times a second) |
| `narration_stop` | `save: boolean`, `stamp: FileStamp \| null` | `NarrationSaved \| null` |
| `narration_info` | `path: string` | `NarrationInfo \| null` |
| `narration_audio` | `path: string` | WAV bytes (`ArrayBuffer`, raw IPC response) |
| `delete_narration` | `path: string`, `stamp: FileStamp` | `FileStamp` |

- Replay target `{ kind: 'replay', path, speed, maxPauseMs, startMs }`: the clip is saved into that note file
  (`stamp` = the editor's stamp; the new stamp is returned for the editor's next save; a newer version on disk is a
  `noteConflict` and nothing is written). Recording again replaces the narration.
- Live target `{ kind: 'live' }`: a watcher records which page is current (every 50 ms) while recording; at stop
  the clip is cut per page and each piece, in note time (`start_ms` = wall clock of the piece's start − the page's
  `created`), is merged into that page's narration and handed to the live hub, which writes it with the page (the
  page's file is written by the hub only, so its stamp stays right). A page without any stroke gets nothing.
- `save: false` discards the recording; stopping a monitor uses `narration_stop` too (returns `null`).

`codec.rs` (OpenH264 fallback; the codec lives in `%LOCALAPPDATA%\Ardoise\codecs`, the temp folder elsewhere):

| Command | Arguments | Returns |
|---|---|---|
| `openh264_state` | — | `OpenH264State` |
| `openh264_download` | `onProgress: Channel<DownloadProgress>` | `OpenH264State` (allows the fallback) |
| `openh264_cancel_download` | — | — |
| `openh264_remove` | — | `OpenH264State` (and disallows it) |
| `set_openh264_allowed` | `allowed: boolean` | `OpenH264State` (`[machines.X] openh264`) |

`OpenH264State` carries `CISCO_NOTICE` and the full text of Cisco's binary licence
(`app/src-tauri/src/openh264-license.txt`, reproduced as Cisco requires), shown by the settings screen.

### Interface (TypeScript, `app/ui/src/`)

Types in `lib/video.ts`, wrappers in `lib/api.ts` (both written with the contracts). Browser stand-ins:
`lib/mock-video.ts` (export, estimate, OpenH264) and `lib/mock-narration.ts` (microphones, narration).

- `lib/replay.ts`: the replay mapping of `replay::Timeline` in TypeScript (same rules: visible layers, running
  maximum of sample times, pauses cut after `maxPause`, speed), `noteAt`, `forDuration`; tested against values
  computed by the Rust timeline.
- Editor: a replay mode (toolbar button and `R`): the page read-only, a bar with play/pause, a timeline slider,
  speeds 0.25×, 0.5×, 1×, 2×, 4×, 8×, 16×, "supprimer les temps morts" (max pause 0.5 s), the narration's sound
  played in sync when its options match, and "Enregistrer une narration" (`MicrophonePicker`, level, Windows block
  explanation; recording plays the replay from the start with the chosen options, stop saves).
- `components/VideoExportDialog.svelte` (`{ source: ExportSource; subject: string; onclose }`): format cards with
  strengths and limits, "Quel format choisir ?" table, resolution, frame rate, speed or target duration, trim
  pauses, background or transparent (GIF/WebP), final frame held, narration included (MP4/WebM when the note has
  one), estimated time, progress, cancel.
- `components/narration/MicrophonePicker.svelte` (`{ device: string | null; onchange; monitor?: boolean }`):
  microphone list, level meter, status line with the « Windows bloque le micro » explanation.
- Settings: microphone (picker), OpenH264 (state, download with progress, allow, remove, `CISCO_NOTICE`, licence).
- Live view: "Narration" button: record / stop, level, status.

### Owners (one agent per row, disjoint files)

| Work | Files |
|---|---|
| Media pipeline | `crates/ardoise-media/src/{export,estimate,narration}.rs` (+ folders), `ogg.rs`; `error.rs` (`Cancelled` only) |
| App commands | `app/src-tauri/src/{video,narration,codec}.rs`, `lib.rs`, `commands.rs` (AppState), `live/`, `Cargo.toml`; `crates/ardoise-core/src/settings.rs` |
| Editor replay and narration | `lib/replay.ts`, `lib/mock-narration.ts`, `components/editor/*`, `components/narration/*`, `components/live/*`; locale sections `replay`, `narration` |
| Video export and settings | `lib/video.ts` helpers, `lib/mock-video.ts`, `components/VideoExportDialog.svelte`, `components/Settings.svelte`, `app/e2e/journeys.mjs`; locale sections `videoExport`, `openh264`, and every new `errorCodes.*` |

### Decisions taken during the work (2026-10-24)

Deviations from the contract above, kept after review of each agent's report:

- `plan_export`: MP4 and WebM frames are at least 16 px a side (what their writers accept); GIF at most 16 384 px (the
  frame renderer's limit). A video longer than 24 hours is refused (a damaged `ARDOISE_START_MS` cannot start an
  endless export). An empty narration clip gives no sound track. `FormatOptions::format()` was added.
- `Narration::merge`: two pieces more than 4 hours apart keep only the later one (no gigabytes of silence).
- `export_animation` retries the final sync and rename while Windows reports the destination busy.
- Live narration: `LiveHub::attach_narration(session, page, narration)` takes the session id (a recording never lands
  on another session's page) and still accepts a narration after its session stopped; the hub also exposes
  `page_position()` and `page_origins(session)`. A running live narration is saved by `live_stop`, when the main
  window closes, and by `live_start` when the previous session ended on its own. The clock of the first audio sample
  is the smallest "wall clock − audio received" seen by the 50 ms watcher (sound-card drift, about 0.2 s an hour, is
  not corrected).
- Error codes beyond the contract: a bad replay target → `invalidVideoSettings`; a recording that cannot be encoded →
  `microphoneFailed`; an unreadable `audio.opus` → `noteDamaged`; `narration_audio` without narration and
  `narration_stop` with nothing to save → `noRecording`. A replay narration saved with `stamp: null` is checked against
  the version just read; saving or deleting a narration updates the note's `modified` time.
- Interface: the replay controls carry `data-action` attributes (`replay-toggle`, `replay-play`, `replay-seek`,
  `replay-speed`, `replay-trim`, `replay-narration`, `video-export`) used by the end-to-end journey; the dialogs'
  "save as" answer can be scripted by the journey through `window.__ardoiseE2eSavePath`. The CSP allows `blob:` media
  so the narration's WAV plays in an `<audio>` element.

After the review of the package (contract changes; the interface codes against them):

- `narration_stop(save, stamp, path, replayStartedAtMs)` (`path`, `replayStartedAtMs`: `string | null`,
  `number | null`). Replay target: `path`, when given, replaces the path fixed at `narration_start` (the editor may
  have renamed the note). `replayStartedAtMs` is the Unix time in ms (`Date.now()`) at which the replay was at instant
  0 and playing; then `start_ms = round(wall clock of the first sample − replayStartedAtMs)` (replay time runs at
  wall-clock speed whatever the replay speed). The wall clock of the first sample is estimated as in live mode: every
  recording, replay or live, has the 50 ms watcher that keeps the smallest "wall clock − audio received" (the wall
  clock at the start when no audio was seen). Without `replayStartedAtMs`, the target's `startMs`. A
  `replayStartedAtMs` that is not finite or beyond ±10¹⁵ is refused (`invalidVideoSettings`) before anything stops,
  so the recording goes on.
- A replay recording that cannot be saved (any error once the clip was captured and encoded: `noteConflict`,
  `noteNotFound`, `notInLibrary`, I/O…) is not lost: the runtime keeps it as the pending narration (its `audio.opus`
  bytes and its length: the PCM is not kept) and writes a best-effort copy `<note file stem> <yyyy-mm-dd hhmmss>.opus`
  (`-2`, `-3`… when taken; stem cut to 150 characters) into `%LOCALAPPDATA%\Ardoise\narration-backup\` (temp folder +
  `Ardoise/narration-backup` when `LOCALAPPDATA` is unset; never in the library). The error returned is unchanged.
  `NarrationStatus.pending: boolean` says that one exists.
  - `narration_retry(path, stamp)` → `NarrationSaved`: saves the pending narration into the note at `path` of the
    current library (checked save, like `narration_stop`; `stamp: null` checks against the version read), then forgets
    it and deletes its copy. On failure it stays pending (unless a newer one replaced it meanwhile).
    `noPendingNarration` when there is none.
  - `narration_discard()`: forgets the pending narration; its copy stays on disk as a last resort.
  - Recording again while one is pending is allowed; the pending one stays until retried or discarded, or until a
    newer recording fails to save (one pending narration at a time; the older copy stays on disk).
  - A clip the Opus encoder refuses (never expected from the recorder: mono, 48 kHz) is `microphoneFailed` and is not
    kept: a retry could not save it either.
- `microphone_monitor_stop()`: stops the capture only if it is a level-only monitor; a recording goes on.
- Error codes: `noNarration` (`narration_audio` on a note without narration), `emptyRecording` (a recording that
  captured nothing), `noPendingNarration`, `openh264RemoveFailed` (removing the codec failed, e.g. Windows still has
  the DLL loaded: the interface says to restart Ardoise). `noRecording` now only means "nothing is recording" (this
  replaces the `noRecording` uses listed above).
- `narration_info` reads `ardoise_media::narration_header` (headers and granule positions, no decoding): the length
  comes from the last granule position, the origin from the first audio page's packet durations (table of contents,
  RFC 6716 §3.1).
- Hostile `audio.opus`: a stream whose granule positions give more than 4 hours is refused before decoding
  (`InvalidFile`), and decoding stops with `InvalidFile` as soon as it yields more than that length plus one packet
  (120 ms) of end trimming.
- `export_animation` checks `cancel` again after the last frame (before the sink's final flush) and before the
  rename: a cancel during the final flush leaves nothing.
- Live narration memory: the clip is cut into its pages without copying it (pieces split off its end, the first one
  keeps its buffer), a page's pieces are appended in place (`Narration::merge` grows the earlier track when the later
  one follows it with as many channels), and a live page keeps only its `audio.opus` bytes; a second recording on the
  page decodes them again to merge (re-encoding the earlier narration once more).


## M10: narration playback without a WAV in memory (agent `m10-fix-hard`)

Review finding, confirmed: `narration_audio` decoded the whole `audio.opus` to a 16-bit 48 kHz WAV (about 345 MB per
hour, 1.4 GB at the 4-hour limit) inside an async command (holding a runtime worker that live mode and the tablet
need), then sent it through IPC in one response and the editor wrapped it in a Blob.

- `narration_audio(path)` now returns the member as stored, the Ogg Opus stream (about 29 MB per hour at 64 kbit/s
  mono), read on a blocking thread (`notes::blocking`); its headers are checked like `narration_info` (`noteDamaged`),
  the sound is not decoded. `narration_wav` is gone; `ardoise_media::wav_bytes` stays for its own tests.
- The editor makes the Blob with the type sniffed from the bytes (`narrationType`: `OggS` → `audio/ogg; codecs=opus`,
  else `audio/wav`, which the browser stand-in still returns). WebView2 (Chromium) plays and seeks Ogg Opus itself; the
  pre-skip and the end trimming of RFC 7845 are applied by its demuxer, so `currentTime` 0 is the first sample, as the
  WAV's was, and `NarrationPlayback` (start offset, drift correction, seeking from the replay's playhead) is
  unchanged. A narration made of several live pieces is one stream already (`Narration::merge`), so pages and clips
  play the same way.
- Manual check of a long narration: `ARDOISE_LONG_NARRATION_NOTE=<note.ardoise> cargo test -p ardoise --release --lib
  -- --ignored writes_a_long_narration` puts an hour of synthetic voice into a note.
- Measured on the development PC with a one-hour narration (29 MB note), from "Lire avec la narration" to a sound
  ready to play, and the peak private memory of the app and its WebView2 processes: before (release exe of main
  f3841d4) 25.3 s and 3273 MB (398 MB before, 785 MB after); after (debug build) 2.7 s and 382 MB (266 MB before, 360 MB
  after). Both seek right: the replay put at 30:00 and 58:20 puts the sound at 1802.5 s and 3502.4 s.
