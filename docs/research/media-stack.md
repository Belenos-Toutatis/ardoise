# Media export stack: research and recommendation

Date: 2026-10-24. Target: portable Windows 11 x64 Tauri 2 app (single .exe, no installer, no admin), probably GPL (derives from Tuhi, which is GPLv2-or-later). Build machine: Rust 1.98.1 MSVC, VS Build Tools 2019 (cc crate), no cmake, no nasm.

Versions and licenses below were read from the crates.io API on 2026-10-24. Items marked **[built]** were compiled and/or run in `...\scratchpad\media-probe\probe` on this machine (cargo 1.98.1, no cmake, no nasm). Items marked **[not verified]** come from docs/READMEs or general knowledge only.

Licensing note: Apache-2.0 crates (cpal, shiguredo_mp4) are compatible with GPLv3 but not GPLv2-only. Since Tuhi is "GPLv2 or later", publish the app as GPL-3.0-or-later. MPL-2.0 (webm crate) is GPL-compatible through its secondary-license clause. MIT/BSD/ISC/Zlib are fine.

---

## 1. H.264 via OpenH264

Crate `openh264` **0.9.9** (2026-10-15), BSD-2-Clause, MSRV 1.85, ~1.7M downloads, actively maintained (Ralf Biedert). Companion `openh264-sys2` 0.9.9.

Two mutually exclusive cargo features:

- `source` (default): compiles Cisco's C++ source via `cc` in build.rs. **[built]** Compiles fine with VS 2019, no nasm, no cmake. nasm is optional (build.rs only uses it if found; ~3x faster). Measured without nasm: 60 frames of 1280x720 (trivial synthetic content) in ~0.6 s in release. Legal catch below.
- `libloading`: loads Cisco's prebuilt DLL at runtime. Yes, this is supported:
  - `OpenH264API::from_blob_path(path)`: loads the DLL only if its SHA-256 is in the crate's whitelist (`openh264-sys2/src/blobs/hashes.txt`).
  - `unsafe OpenH264API::from_blob_path_unchecked(path)`: no hash check.
  - Then `Encoder::with_api_config(api, EncoderConfig::new())` and `encoder.encode(&yuv)` / `encode_at(&yuv, Timestamp)`; output via `EncodedBitStream::to_vec()/write()` (Annex B). YUV input via `formats::YUVSlices` or the RGB-to-YUV helpers.
  - Use `default-features = false, features = ["libloading"]`. The build then needs no C++ compiler at all (pulls in `libloading` 0.9 ISC and `sha2`).

**Which DLL matches.** The crate's whitelist (master, which 0.9.9 matches) contains only OpenH264 **2.6.0** for Windows: `openh264-2.6.0-win64.dll`, SHA-256 `2076cb5675ec6c1a4c70e7a2a322552f547b6eeed649d6dfcd9e02a543b24691`. **[verified]** I downloaded the Cisco 2.6.0 bz2, decompressed it (978,520 bytes) and the hash matches the whitelist. The newest Cisco release is 2.7.0 (22 Oct 2026); its DLL hash is `0ef5e7f3b38753b15c13afa0418d4d7805fa228e6adfd3e425bbf758cff1c288` and is NOT in the whitelist, so `from_blob_path` rejects it (use the unchecked variant plus your own pinned hash if you want 2.7.0; I did not test 2.7.0 with the crate's bindings).

**Exact URL pattern (HTTP 200 confirmed for 2.4.1, 2.5.0, 2.5.1, 2.6.0, 2.7.0):**

`https://ciscobinary.openh264.org/openh264-<version>-win64.dll.bz2`

e.g. `https://ciscobinary.openh264.org/openh264-2.6.0-win64.dll.bz2` (452,053 bytes compressed). Checksum files exist next to it. Pin the SHA-256 of the decompressed DLL in the app; decompress with a bzip2 crate (`bzip2`, or the pure-Rust `libbz2-rs-sys`) and write to the app data folder, e.g. `%LOCALAPPDATA%\<App>\codecs\openh264-2.6.0-win64.dll`. `win32` and `win-arm64` variants also exist.

**Cisco binary license conditions** (full text: https://www.openh264.org/BINARY_LICENSE.txt, FAQ https://www.openh264.org/faq.html). Cisco pays the MPEG LA AVC/H.264 royalties for its own binary only if ALL of these hold:

1. The Cisco binary is separately downloaded to the end user's device and not integrated or combined with third-party software before that download. So: do not bundle the DLL in the .exe or the release zip; download at first use. The FAQ says the product must download it "at the time the product or project is installed on the user's computer". For a portable app, a user-triggered first-use download is the closest equivalent. Download directly from Cisco's server (not a mirror you host).
2. The end user must be able to control (enable, disable, re-enable) use of the binary. Provide a setting, e.g. "Enable H.264 export (downloads OpenH264 from Cisco)" plus a "remove" button.
3. Where the user controls it, the UI must display exactly: **"OpenH264 Video Codec provided by Cisco Systems, Inc."**
4. The full license notice (BSD text, AVC/H.264 Patent Portfolio License Notice and these conditions) must be reproduced in the EULA and/or wherever licensing information is shown (About / Third-party licenses screen, plus the GitHub repo).

Cisco only covers its binary. Using the `source` feature gives BSD code but NO Cisco patent coverage (the crate docs say this explicitly). Cisco also does not guarantee coverage against other patent holders. The MPEG LA notice is aimed at personal / non-remunerated use, which matches a free tool.

Alternative with no download and no patent question for you: the Windows built-in H.264 encoder through Media Foundation (section 7b).

---

## 2. MP4 muxing (H.264 + audio) in pure Rust, and the audio codec question

| Crate | Version | License | Notes |
|---|---|---|---|
| `muxide` | 0.5.0 (2026-10-19) | MIT OR Apache-2.0 | Write-only, pure Rust, MSRV 1.86. H.264/H.265 Annex B, AV1, VP9; audio AAC and Opus (48 kHz mono/stereo). API: `write_video(pts_seconds, annexb, is_keyframe)`, `write_audio(...)`, `finish()`. First keyframe must carry SPS+PPS. Young (33k downloads, single author) but builds **[built]**. Best fit. |
| `shiguredo_mp4` | 2026.5.0 | Apache-2.0 | Sans-I/O, zero deps, no_std, H.264/AAC/Opus. Company-backed. Lower level: you build sample entries and write mdat yourself. MSRV 1.93. Good fallback. |
| `mp4-atom` | 0.16.2 (2026-10) | MIT OR Apache-2.0 | Low-level box read/write, very active. Fallback for custom boxes. |
| `mp4` (alfg) | 0.14.0 (2023-08) | MIT | Popular (14M dl) but unmaintained since 2023. Avoid for new work. |
| `minimp4` | 0.1.2 | MPL-2.0 | C binding (minimp4.h); little maintained. |

**AAC encoders**: there is no usable pure-Rust AAC-LC encoder (symphonia's AAC is decode-only). Options:

- `fdk-aac` 0.8.0 / `fdk-aac-sys` 0.5.0 (MIT wrappers around Fraunhofer FDK AAC). The FDK AAC library license is considered **incompatible with the GPL** (FSF/Fedora: it denies any patent license), so it cannot be linked into a GPL binary. Do not use it.
- `xaac-rs` (bindings to libxaac): obscure, very young, **[not verified]**.
- **Windows Media Foundation AAC encoder MFT** (built into Windows non-N editions) via the `windows` crate (0.62.2, MIT/Apache-2.0). This is the practical AAC route. N/KN editions need the Media Feature Pack **[not verified for the encoder MFT specifically]**. Windows 11 Home (this machine) is a normal edition.
- ffmpeg's native `aac` encoder (LGPL) if you go the ffmpeg route.

**Opus in MP4 playback** (web search results plus general knowledge, **not tested here**):

- Chrome / Edge / Firefox / Safari: play MP4 with Opus audio. VLC: plays it.
- Windows 11 Media Player / Films & TV / legacy WMP: **unreliable.** Microsoft media-foundation GitHub issue #36 reports Windows 11 Media Player failing to play the Opus track of MP4 files while MKV works; community answers say the Store "Web Media Extensions" Opus decoder is needed. Many Windows users may get silent video. Treat Opus-in-MP4 as "works in browsers and VLC only".

Conclusion: for MP4 use AAC-LC if audio must work in Windows-native players; use Opus only inside WebM/Matroska.

---

## 3. WebM

- **VP8/VP9 via libvpx**: `vpx-encode` 0.6.2 (MIT, 2022, abandoned), `env-libvpx-sys` 5.1.3 (MPL-2.0, 2023), `libvpx-native-sys` 5.0.17 (MPL-2.0, active), `vpx-rs` 0.2.1 (BSD-2). **[built]** `libvpx-native-sys` fails on this machine: build.rs requires a system libvpx found through `pkg-config` (no bundled source). On Windows that means vcpkg (which bootstraps its own yasm/nasm and msys) or a manual MSYS build of libvpx. Impractical here. **Not recommended.**
- **AV1 via rav1e**: `rav1e` 0.8.1 (2025-06), BSD-2-Clause, pure Rust. The `asm` feature (default) needs nasm (build.rs panics "NASM build failed ... or disable the asm feature"). With `default-features = false, features = ["threading"]` it builds with no nasm **[built]** but is slow: measured in release, 1280x720, 60 frames, speed 10, low-latency, auto threads: **25.8 s (about 2.3 fps)**; speed 8: 30 s. With asm it is typically several times faster (not measured; nasm is a portable zip, needed only on the build machine, no admin). A 30 s clip at 30 fps would take roughly 6-7 minutes at 720p without asm. Lower resolution/fps helps. AV1 playback: Chrome, Edge, Firefox, VLC yes; Windows Media Player / Films & TV need the Store "AV1 Video Extension".
  - `libaom-sys` (BSD-2) and SVT-AV1 need cmake. No.
- **Pure-Rust VP8/VP9 encoders**: none production-ready (`image-webp` is a lossless still-image WebP encoder, not VP8 video).
- **WebM/Matroska muxer**:
  - `webm` 2.2.1 (2026-08-25), MPL-2.0, wraps Google's libwebm C++ (`webm-sys`, built with cc). **[built]** compiles without cmake. Muxes VP8/VP9/AV1 video and **Opus/Vorbis** audio (`VideoCodecId`, `AudioCodecId::Opus`). Maintenance badge: "passively-maintained". API: `add_video_track`, `add_audio_track(sample_rate, channels, AudioCodecId::Opus, ..)`, `add_frame(track, data, timestamp_ns, is_keyframe)`. How it handles Opus CodecPrivate (OpusHead) and pre-skip: **[not verified]**; test playback in Chrome and VLC.
  - `mkv-element` 0.4.0 (MIT), `matroska` 0.30 (MIT/Apache), `webm-iterable` 0.7 (MIT, EBML) are pure-Rust building blocks if you prefer to hand-write the muxer.

Recommended WebM profile: AV1 (rav1e, asm off by default; optionally build with nasm for speed) + Opus. Label WebM as a slow export in the UI.

---

## 4. Opus encoding

- `opus` 0.4.0 (2026-08, MIT/Apache): safe wrapper over `audiopus_sys` 0.2.2 (ISC) / `libopus_sys` 0.4.0 (MIT). **[built]** `libopus_sys` fails here with "failed to execute command: program not found, is cmake not installed?" (it builds Opus with CMake and the VS 2019 generator). `audiopus_sys` also uses cmake. **Needs cmake.** A portable cmake zip on the build machine only would fix it.
- **`ropus`** 0.12.18 (2026-05, BSD-3-Clause): pure-Rust port of libopus (SILK+CELT+hybrid encoder and decoder, VBR/CBR, FEC, DTX, repacketizer). **[built]** compiles with no C toolchain. Claims bit-exact output vs libopus 1.5.2 and encode speed about 1.0x C. Caveats: young 0.x, ~40% doc coverage, its docs say the published binary assumes an x86-64-v3 (AVX2) baseline (check target-cpu settings for old CPUs), `wide` as only dependency. Good fit for voice narration.
- `unsafe-libopus` 0.2.0 (BSD-3, c2rust translation of libopus 1.3.1, encoder included): older no-C option.
- `opus-rs` 0.1.39 (BSD-3, active): another pure-Rust effort, not evaluated.
- `ogg` 0.9.2 (BSD-3) if you ever want .opus files.

Recommendation: `ropus` first, `opus` + cmake as fallback. Use 20 ms frames, 48 kHz mono, 32-64 kbps for narration.

---

## 5. Animated WebP and GIF

- **Animated WebP**: `webp-animation` 0.10.0 (2026-04, MIT OR Apache-2.0) wrapping libwebp's `WebPAnimEncoder` via `libwebp-sys`. **[built]** compiles with cc only. Lossy and lossless, per-frame timestamps, loop count. libwebp is BSD-3. Alternatives: `libwebp-sys` 0.14.4 raw; `webp` 0.3.1 and `image` 0.25.10 (via `image-webp` 0.2.4) only do stills / lossless, so `webp-animation` is the one.
- **GIF**: `gif` 0.14.2 (MIT OR Apache-2.0, 158M downloads, image-rs, maintained). **[built]** Pure Rust. Quantization:
  - `color_quant` 2.0.0 (MIT) NeuQuant: good enough, pure Rust.
  - `imagequant` 4.4.1 is **GPL-3.0-or-later**: fine for a GPLv3 app, better quality (dithering/remap).
  - `gifski` 1.34.0 is **AGPL-3.0-or-later** (confirmed on crates.io). Avoid unless you license the app AGPL; use `gif` + `imagequant`/`color_quant`.
  - Handwriting on a plain background: a fixed or sampled global palette plus frame differencing (transparent unchanged pixels, `DisposalMethod::Keep`) gives small files.

---

## 6. Microphone capture and resampling

- `cpal` **0.18.2** (2026-08-16), Apache-2.0, MSRV 1.85, 25M downloads, RustAudio, active. **[built]** Windows backend is WASAPI (shared mode), no admin required. API: `cpal::default_host()`, `host.input_devices()`, device name, `default_input_config()` / `supported_input_configs()`, `build_input_stream(...)`. Select by stored device name (names can collide; also store index). Capture format is whatever the device offers (f32/i16, 16/44.1/48 kHz, mono/stereo), so convert to f32 mono.
  - Windows privacy: Settings > Privacy & security > Microphone must allow "Let desktop apps access your microphone". Otherwise the stream opens but delivers silence (no prompt for unpackaged apps). Detect all-zero input and offer a hint / link to `ms-settings:privacy-microphone`.
  - Alternative inside Tauri: WebView2 `getUserMedia` + `MediaRecorder` (audio/webm;codecs=opus) would give WebM/Opus without native audio code, but needs permission handling, device labels only appear after permission, and MP4/AAC still needs transcoding. cpal is preferred because the Rust pipeline gets raw PCM for any container.
- **Resampling**: `rubato` **5.0.1** (2026-10-01), MIT OR Apache-2.0, pure Rust, **[built]**. Convert device rate to 48 kHz for Opus/AAC.
- `hound` 3.5.1 (Apache-2.0) if you want a scratch WAV during recording (recommended: record raw PCM to a temp file, encode at export).

---

## 7. ffmpeg alternatives (honest comparison)

### 7a. Bundling or downloading ffmpeg

BtbN FFmpeg-Builds `latest` assets (Win64 zips, sizes read from the GitHub API on 2026-10-24):

| Build | n8.1 | n9.0 |
|---|---|---|
| GPL static | 193 MB | 194 MB |
| GPL shared | 90 MB | 87 MB |
| LGPL static | 170 MB | 172 MB |
| LGPL shared | 81 MB | 77 MB |

`ffmpeg-sidecar` 2.7.0 (MIT, active) can auto-download ffmpeg at runtime and drive it through pipes.

- Pros: every format (H.264, VP9, AV1, AAC, Opus, GIF with palettegen, animated WebP, muxing, A/V sync) from one tool, fast (asm everywhere), small Rust code, VP9 WebM and high-quality GIF/WebP are trivial.
- Cons: 80-190 MB download, child process (no admin needed), license burden. **GPL builds** include libx264/libx265: fine for a GPL app if the user downloads it separately, but H.264 patent exposure then sits with you/the user (no Cisco-style coverage). **LGPL builds** exclude libx264, so H.264 comes only from Media Foundation (`h264_mf`) or hardware encoders (nvenc/qsv/amf), or libopenh264 if enabled (**[not verified]**). I believe LGPL BtbN builds include native AAC, libopus, libvpx, libwebp and AV1 encoders (**[not verified]**).
- `ffmpeg-next` 9.0.0 / `ffmpeg-sys-next` 9.0.1 (WTFPL) need ffmpeg dev libs plus clang/bindgen or vcpkg at build time: poor fit for this build machine. Not recommended.

### 7b. Windows Media Foundation (built into Windows 11)

Via `windows` 0.62.2 you can drive `IMFSinkWriter` with the MPEG-4 sink: H.264 encoder MFT (hardware if available, else Microsoft's software encoder) plus AAC encoder MFT, producing a normal .mp4 with proper AAC, no download, no Cisco obligations, plays everywhere. Cons: large unsafe COM surface, less rate-control tuning than OpenH264, N editions need the Media Feature Pack, Windows-only (acceptable here). **Verified by a spike on 2026-10-24: see section 7c.**

### Comparison

| Concern | Native crates | Media Foundation hybrid | ffmpeg download |
|---|---|---|---|
| Single exe, no big download | Yes, except OpenH264 DLL (~0.45 MB) | Yes (OS codecs), MP4 only | No (80-190 MB) |
| H.264 licensing | Cisco-covered if DLL downloaded + notices | OS-provided | x264 not covered (GPL build) |
| MP4 + AAC for Windows players | Needs MF AAC, or Opus (flaky) | Yes | Yes |
| WebM | AV1 only, slow without nasm | No | VP9/AV1 fast |
| GIF / WebP | Yes | n/a | Yes |
| Build complexity | Medium (many crates) | Medium (COM) | Low code, heavy distribution |

---

## 7c. Media Foundation spike: result (2026-10-24)

**Verdict: it works.** On this PC (Windows 11 Home 10.0.26300, Intel Core i5-1235U, Intel Iris Xe, driver 32.0.101.7085) Rust writes a standard `.mp4` holding H.264 video and AAC-LC audio through `IMFSinkWriter`, with nothing downloaded, and the file reads back correctly through Media Foundation and in Windows Explorer. It is fit to be the default MP4 path planned by SPEC §6.2, provided the points under "What M6 must still do" are handled. The most important one is delay-loading the Media Foundation DLLs: without it `ardoise.exe` would not even start on a Windows "N" edition, so the OpenH264 fallback could never run.

### What was built

Reusable code in `crates/ardoise-media`, using `windows` 0.62.2 as already in `Cargo.lock` (new features `Win32_Foundation`, `Win32_Media_MediaFoundation`, `Win32_System_Com`, `Win32_System_Com_StructuredStorage`, `Win32_System_Variant`; no new crate, no download):

- `Mp4Writer::create(path, &Mp4Settings)`, `push_frame(&[u8] RGBA, Duration)`, `push_audio(&[i16])`, `finish() -> Mp4Summary`. `Mp4Settings::new(width, height, frame_rate)`, plus `with_audio(AudioSettings::new(48_000, 2))`, `video_bitrate`, `hardware_acceleration`. H.264 High profile, no B-frames, a key frame every 2 s, default bitrate 0.08 bit per pixel and frame (2.2 Mbit/s at 720p30); AAC-LC at 128 kbit/s from 16-bit PCM, 44.1 or 48 kHz, mono or stereo.
- `nv12::rgba_to_nv12`: the colour conversion, done in Rust on purpose: BT.709 matrix, limited range (Y 16-235, U/V 16-240), chroma = average of each 2×2 block, alpha ignored, packed planes (stride = width, UV plane right after the Y plane). The NV12 and H.264 media types carry the same colour description (`MF_MT_YUV_MATRIX`, `MF_MT_VIDEO_NOMINAL_RANGE`, primaries, transfer function), so encoder and players agree. Unit tests check reference colours against the BT.709 equations; they also run on Linux.
- `probe_mp4(path, ProbeOptions)`: independent read-back through `IMFSourceReader`: container duration (`MF_PD_DURATION`), native subtypes, frame size, every video frame decoded to RGB32 by Media Foundation's own decoder and video processor, every audio sample decoded to PCM, optional capture of one frame and of the audio. Reusable for the M6 end criteria ("durée, dimensions et nombre d'images relus depuis le fichier").
- `available_encoders()`: the H.264 and AAC encoders registered with Media Foundation (`MFTEnumEx`), for diagnostics and for deciding on the OpenH264 fallback.
- RAII guards: `CoInitializeEx(COINIT_MULTITHREADED)` / `CoUninitialize` (left alone when the thread is already a single-threaded apartment, `RPC_E_CHANGED_MODE`), `MFStartup(MFSTARTUP_LITE)` / `MFShutdown`, buffer `Lock` / `Unlock`; COM objects are released before shutdown (field order). Typed errors (`MediaError`, with `EncoderUnavailable` for `MF_E_TOPO_CODEC_NOT_FOUND` and `MediaFoundationUnavailable` when `MFStartup` fails). Outside Windows every entry point returns `MediaError::Unsupported` (checked by building and linting for `wasm32-unknown-unknown`).
- Test `crates/ardoise-media/tests/mf_mp4_roundtrip.rs` (Windows only): a 3 s 1280×720 30 fps animation (a stroke growing from left to right, a red square sliding below it) with a 440 Hz stereo tone, written under a temporary name (`.partial`) then renamed, once with hardware encoders allowed and once with the software encoder forced; each file is then read back with `probe_mp4` and asserted.

### Measured

Encoders registered on this PC. H.264: Intel Quick Sync Video H.264 Encoder MFT (hardware, listed twice), Microsoft AVC DX12 Encoder (hardware), H264 Encoder MFT (software). AAC: Microsoft AAC Audio Encoder MFT (software). With hardware allowed, the sink writer picked Intel Quick Sync.

Read back with `IMFSourceReader` (debug build, `cargo test -p ardoise-media -- --nocapture`):

| Property | Expected | Hardware (Quick Sync) | Software (H264 Encoder MFT) |
|---|---|---|---|
| Container duration | 3.000 s | 3.008 s | 3.008 s |
| Video codec, size | H.264, 1280×720 | H.264, 1280×720 | H.264, 1280×720 |
| Video frames decoded | 90 | 90 | 90 |
| First / last frame time | 0.000 / 2.967 s | 0.000 / 2.967 s | 0.000 / 2.967 s |
| Audio codec, channels, rate | AAC, 2, 48000 Hz | AAC, 2, 48000 Hz | AAC, 2, 48000 Hz |
| Audio decoded | 3.000 s | 3.008 s (144 384 frames) | 3.008 s (144 384 frames) |
| Tone frequency (zero crossings) | 440 Hz | 439.8 Hz | 439.8 Hz |
| 5 decoded pixels of frame 45 | exact colours | max error 1/255 | max error 1/255 |
| File size | | 602 536 bytes | 76 527 bytes |

The pixels were chosen so that they identify frame 45 exactly (the stroke end moves 12 px per frame): frame order, timestamps, colour matrix and range all survive the round trip.

Speed (writer only: `create` + pushes + `finish`, RGBA→NV12 included, drawing excluded; 90 frames and 3 s of audio):

| Build | Hardware path | Software path |
|---|---|---|
| Release (4 runs) | 0.67-0.93 s = 97-134 frames/s overall. `create` 0.42-0.57 s (GPU encoder start-up), the 90 pushes 0.18-0.21 s (about 450 frames/s), `finish` 0.08-0.16 s | 0.26-0.33 s = 274-343 frames/s overall; `create` 0.06 s |
| Debug (2 runs) | 2.34-2.40 s = 37-38 frames/s | 2.36-2.45 s = 37-38 frames/s |

RGBA→NV12 alone takes 1.4-1.7 ms per 720p frame in release and 21.5-22.8 ms in debug, where it dominates. On this simple content the software encoder is as fast as the hardware one once started; the hardware encoder's half-second start-up only pays off on long exports. Both are far faster than real time.

What Windows reports for both files (PowerShell `Shell.Application` `GetDetailsOf`, French UI): "Durée 00:00:03", "Largeur de trame 1280", "Hauteur de trame 720", "Fréquence d'images 30.00 trames/s", "Compression vidéo {34363248-0000-0010-8000-00AA00389B71}" (the H.264 subtype), audio "Débit binaire 129 Kbits/s", type "MPEG-4 Video"; video "Débit de données" 1471 kbit/s (hardware) and 68 kbit/s (software).

### Pitfalls met

- **Media types.** H.264 output type: major type, subtype, `MF_MT_AVG_BITRATE`, `MF_MT_FRAME_SIZE` and `MF_MT_FRAME_RATE` (two u32 packed in a u64), `MF_MT_INTERLACE_MODE` = progressive, `MF_MT_PIXEL_ASPECT_RATIO`, `MF_MT_MPEG2_PROFILE`. The NV12 input type repeats size, rate, interlace mode and aspect ratio, plus `MF_MT_DEFAULT_STRIDE`. The sink writer looks for the encoder in `SetInputMediaType`; a missing codec shows up there (`MF_E_TOPO_CODEC_NOT_FOUND`).
- **Attributes.** Hardware encoders are only considered with `MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS` = TRUE; FALSE forces the software encoder (a useful user-facing fallback switch). `MF_TRANSCODE_CONTAINERTYPE` = MPEG-4 makes the container independent of the file extension, which atomic writes need (the test writes `*.mp4.partial`, then renames).
- **Timestamps** are `i64` counts of 100 ns. Video: from the caller's `Duration`, each frame lasting one nominal period. Audio: the start of each chunk is computed from the total number of frames written (`frames × 10^7 / rate`), never by adding chunk durations, so there is no drift whatever the chunk sizes.
- **NV12 stride.** With `MFCreateMemoryBuffer` the planes are packed: stride = width, and the UV plane (height/2 rows of width bytes, U and V interleaved) follows the Y plane directly. Width and height must be even. A fresh buffer per frame, because the sink writer may still hold earlier samples.
- **AAC input constraints.** Microsoft's encoder takes 16-bit PCM at 44.1 or 48 kHz with 1, 2 or 6 channels. The output type's `MF_MT_AUDIO_AVG_BYTES_PER_SECOND` must be exactly 12000, 16000, 20000 or 24000 (96-192 kbit/s), and `MF_MT_AAC_PAYLOAD_TYPE` = 0 (raw AAC, as MP4 stores it). AAC pads to whole 1024-sample frames: 3 s became 141 frames = 3.008 s, which also sets the container duration. Any chunk size is accepted (the writer sends at most 100 ms per sample).
- **Sink writer quirks.**
  1. *Fast start (`MF_MPEG4SINK_MOOV_BEFORE_MDAT`) is unusable as is.* With the file the sink writer opens itself from the URL, `Finalize` fails with `E_ACCESSDENIED` (0x80070005): the file is write-only. With a read-write byte stream from `MFCreateFile(MF_ACCESSMODE_READWRITE, MF_OPENMODE_DELETE_IF_EXIST, MF_FILEFLAGS_NONE, ...)`, `Finalize` succeeds but the file is corrupt: `moov` is written at the front with chunk offsets for the final layout, but the media data stays duplicated at its old place and at the end (file twice as large, 0 decodable frames), with software and hardware encoders, with and without audio. `MF_FILEFLAGS_NOBUFFERING` made the writer hang. The writer therefore leaves the index at the end of the file (`ftyp, uuid, mdat, moov`), which every local player handles; browsers then need HTTP range requests to start before the download ends. If fast start is wanted, a small pure-Rust post-pass (move `moov` forward, add its size to every `stco`/`co64` offset) is the safe way.
  2. *Interleaving.* The MPEG-4 sink interleaves the tracks and the sink writer stalls a stream that runs ahead: pushing all 90 frames before any audio took 24 s instead of 2.4 s (no deadlock, but ten times slower). Audio pushed ahead is fine: the writer buffers it and releases it in step with the frames. Rule for the export loop: push each frame's audio before (or with) the frame, or the whole narration up front.
  3. *B-frames shift the video.* With default settings Microsoft's software encoder used B-frames, and since the MP4 sink writes no edit list every decoded frame came out one period late (first frame at 33 ms, last at 3.000 s): the video lagged the audio by one frame. Passing `CODECAPI_AVEncMPVDefaultBPictureCount` = 0 (and `CODECAPI_AVEncMPVGOPSize` = 2 s) as encoding parameters of `SetInputMediaType` fixed it, for both encoders.
  4. *Rate control differs between encoders.* For the same 2.2 Mbit/s target, Quick Sync spent 1.47 Mbit/s on a nearly static picture (602 536 bytes for 3 s), the software encoder 68 kbit/s (76 527 bytes). For handwriting, quality-based VBR (`CODECAPI_AVEncCommonRateControlMode`, `CODECAPI_AVEncCommonQuality`) would make hardware files much smaller: to tune in M6.
  5. *Identifying the encoder.* `IMFSinkWriterEx::GetTransformForStream` returns the encoder MFT. Hardware MFTs have an attribute store with `MFT_ENUM_HARDWARE_URL_Attribute`, a vendor id ("VEN_8086") and `MFT_TRANSFORM_CLSID_Attribute` but no friendly name (looked up by CLSID in the `MFTEnumEx` list). Microsoft's software encoder has no attribute store at all, so it is reported as "unnamed software encoder" (on this PC it can only be "H264 Encoder MFT").
  6. The MPEG-4 sink closes the file itself in `Finalize`: the file can be renamed as soon as `finish` returns.
- **DLL imports (important).** The `windows` crate links `mfplat.dll` and `mfreadwrite.dll` as ordinary load-time imports (`dumpbin /dependents` on the test binary). On a Windows N/KN edition without the Media Feature Pack these DLLs do not exist, so the whole executable would fail to start, not just the export. Delay-loading works with the crate's `raw-dylib` imports: built with `/DELAYLOAD:mfplat.dll /DELAYLOAD:mfreadwrite.dll delayimp.lib`, the test binary lists both DLLs under "delay load dependencies" and the tests still pass.
- **Debug builds** convert colours about 15 times slower (about 22 ms per frame): add `ardoise-media` to the `opt-level` overrides of the root `Cargo.toml`, as for `ardoise-render`.

### What M6 must still do

1. In `app/src-tauri/build.rs`: `cargo:rustc-link-arg-bins=/DELAYLOAD:mfplat.dll`, the same for `mfreadwrite.dll`, and `cargo:rustc-link-arg-bins=delayimp.lib` (not `RUSTFLAGS`, which would replace the `+crt-static` flag of `.cargo/config.toml`). Before the first Media Foundation call, check that `mfplat.dll` loads (`LoadLibraryExW` with `LOAD_LIBRARY_SEARCH_SYSTEM32`); if it does not, offer the OpenH264 fallback.
2. Run the export on a worker thread (COM multithreaded apartment); the single-threaded-apartment branch of the COM guard is untested.
3. Follow the interleaving rule in the export loop; keep `hardware_acceleration` as a setting (software forced if a GPU driver misbehaves).
4. Tune rate control for handwriting; decide whether fast start is worth a Rust post-pass.
5. Not verified here: Windows N with the Media Feature Pack, other GPUs (NVIDIA, AMD), playback in the Media Player and Films & TV apps (decoding was verified through the same Media Foundation decoder they use), very long exports.

---

## Recommended stack

| Output / function | Crate (version) | License | Build needs | Status | Notes |
|---|---|---|---|---|---|
| H.264 | `openh264` 0.9.9, `default-features=false, features=["libloading"]` | BSD-2 | none | active (2026-10) | Download `openh264-2.6.0-win64.dll.bz2` from `ciscobinary.openh264.org`, verify SHA-256, bzip2-decompress, `OpenH264API::from_blob_path`. Show Cisco notice, enable/disable control |
| MP4 mux | `muxide` 0.5.0 (fallbacks `shiguredo_mp4`, `mp4-atom`) | MIT/Apache-2.0 | none | active, young | H.264 Annex B + AAC (or Opus) |
| AAC for MP4 | Windows Media Foundation AAC MFT via `windows` 0.62.2 (not fdk-aac) | MIT/Apache-2.0 (crate); OS codec | none | OS | spike passed (section 7c); fallback Opus-in-MP4 (browser/VLC only), or ffmpeg download |
| WebM video | `rav1e` 0.8.1, `default-features=false, features=["threading"]` | BSD-2 | none (nasm optional for big speedup) | stable, slow cadence | measured ~2.3 fps at 720p without asm |
| WebM mux | `webm` 2.2.1 | MPL-2.0 | cc only (builds) | passively maintained | VP8/VP9/AV1 + Opus/Vorbis |
| Opus | `ropus` 0.12.18 (fallback `opus` 0.4.0 + cmake) | BSD-3 | none | young, active | pure Rust |
| Animated WebP | `webp-animation` 0.10.0 | MIT/Apache-2.0 | cc only (builds) | active | libwebp BSD-3 |
| GIF | `gif` 0.14.2 + `color_quant` 2.0.0 (or `imagequant` 4.4.1, GPL-3+) | MIT/Apache-2.0 | none | active | NOT gifski (AGPL) |
| Mic capture | `cpal` 0.18.2 | Apache-2.0 | none | active | WASAPI, no admin |
| Resampling | `rubato` 5.0.1 | MIT/Apache-2.0 | none | active | to 48 kHz |
| Dynamic loading | `libloading` 0.9.0 (via openh264-sys2) | ISC | none | active | |

Project license: GPL-3.0-or-later (Tuhi is GPLv2+; Apache-2.0 cpal requires v3).

Fallback decision: if the Media Foundation AAC spike fails or Opus-in-MP4 on stock Windows is unacceptable, offer an optional "Download ffmpeg" path (BtbN LGPL-shared, ~80 MB, via `ffmpeg-sidecar`) used only for AAC mux and/or VP9 WebM.

## Risks

1. **Cisco license compliance.** The DLL must be downloaded by the user's machine from Cisco, never bundled; need an enable/disable control, the exact notice string, and the license text. Breaking this voids patent coverage. Coverage is for Cisco's binary only, not other patent holders.
2. **Crate whitelist pins OpenH264 2.6.0.** Newer Cisco releases (2.7.0 exists) fail `from_blob_path` until the crate updates; old DLL URLs could vanish. Pin version and hash; consider the unchecked loader with your own hash.
3. **No Rust AAC encoder with a GPL-compatible license.** fdk-aac is GPL-incompatible. Must rely on Media Foundation (spike passed, section 7c; N editions need the Media Feature Pack, and the MF DLLs must be delay-loaded) or accept Opus-in-MP4, which reportedly fails in Windows 11 Media Player / Films & TV but works in browsers and VLC.
4. **WebM is slow without nasm** (AV1 rav1e about 2.3 fps at 720p measured). VP8/VP9 via libvpx is not buildable here (needs system libvpx via pkg-config). Options: install nasm on the build machine only, lower resolution/fps for WebM, or use ffmpeg for VP9.
5. **cmake needed for the `opus`/`audiopus_sys`/`libopus_sys` route** (confirmed failure here). Avoided by `ropus`, but it is a young 0.x port: validate by decoding in Chrome/VLC, and check the AVX2 baseline note.
6. **`muxide` is young** (single author). Test output in Chrome, Edge, VLC, Windows Media Player (A/V sync, moov placement / fast-start, seeking); keep `shiguredo_mp4` or `mp4-atom` as fallbacks.
7. **`webm` crate is passively maintained** (MPL-2.0, C++ libwebm); verify Opus CodecPrivate / pre-skip handling and audio playback in Chrome.
8. **Microphone privacy setting** on Windows 11 can silently yield silence for desktop apps; detect and guide. Device names are not unique; persist name plus index.
9. **Licensing hygiene**: cpal (Apache-2.0) requires GPLv3 rather than v2-only; `imagequant` is GPL-3+; gifski (AGPL) excluded; ship a third-party licenses file including the Cisco text and libwebp/libwebm/OpenH264 notices.
10. **Not verified in this research**: actual playback of produced files (no players were run, no end-to-end mux test was built), Media Foundation encoder behavior on this PC, contents of BtbN LGPL builds, OpenH264 2.7.0 compatibility with the crate bindings, muxide/webm Opus handling.
