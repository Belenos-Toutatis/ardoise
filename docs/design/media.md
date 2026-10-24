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
