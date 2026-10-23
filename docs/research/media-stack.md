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

Via `windows` 0.62.2 you can drive `IMFSinkWriter` with the MPEG-4 sink: H.264 encoder MFT (hardware if available, else Microsoft's software encoder) plus AAC encoder MFT, producing a normal .mp4 with proper AAC, no download, no Cisco obligations, plays everywhere. Cons: large unsafe COM surface, less rate-control tuning than OpenH264, N editions need the Media Feature Pack, Windows-only (acceptable here). **[not verified here]**; worth a short spike because it also solves AAC-in-MP4.

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

## Recommended stack

| Output / function | Crate (version) | License | Build needs | Status | Notes |
|---|---|---|---|---|---|
| H.264 | `openh264` 0.9.9, `default-features=false, features=["libloading"]` | BSD-2 | none | active (2026-10) | Download `openh264-2.6.0-win64.dll.bz2` from `ciscobinary.openh264.org`, verify SHA-256, bzip2-decompress, `OpenH264API::from_blob_path`. Show Cisco notice, enable/disable control |
| MP4 mux | `muxide` 0.5.0 (fallbacks `shiguredo_mp4`, `mp4-atom`) | MIT/Apache-2.0 | none | active, young | H.264 Annex B + AAC (or Opus) |
| AAC for MP4 | Windows Media Foundation AAC MFT via `windows` 0.62.2 (not fdk-aac) | MIT/Apache-2.0 (crate); OS codec | none | OS | spike needed; fallback Opus-in-MP4 (browser/VLC only), or ffmpeg download |
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
3. **No Rust AAC encoder with a GPL-compatible license.** fdk-aac is GPL-incompatible. Must rely on Media Foundation (unverified spike; N editions need Media Feature Pack) or accept Opus-in-MP4, which reportedly fails in Windows 11 Media Player / Films & TV but works in browsers and VLC.
4. **WebM is slow without nasm** (AV1 rav1e about 2.3 fps at 720p measured). VP8/VP9 via libvpx is not buildable here (needs system libvpx via pkg-config). Options: install nasm on the build machine only, lower resolution/fps for WebM, or use ffmpeg for VP9.
5. **cmake needed for the `opus`/`audiopus_sys`/`libopus_sys` route** (confirmed failure here). Avoided by `ropus`, but it is a young 0.x port: validate by decoding in Chrome/VLC, and check the AVX2 baseline note.
6. **`muxide` is young** (single author). Test output in Chrome, Edge, VLC, Windows Media Player (A/V sync, moov placement / fast-start, seeking); keep `shiguredo_mp4` or `mp4-atom` as fallbacks.
7. **`webm` crate is passively maintained** (MPL-2.0, C++ libwebm); verify Opus CodecPrivate / pre-skip handling and audio playback in Chrome.
8. **Microphone privacy setting** on Windows 11 can silently yield silence for desktop apps; detect and guide. Device names are not unique; persist name plus index.
9. **Licensing hygiene**: cpal (Apache-2.0) requires GPLv3 rather than v2-only; `imagequant` is GPL-3+; gifski (AGPL) excluded; ship a third-party licenses file including the Cisco text and libwebp/libwebm/OpenH264 notices.
10. **Not verified in this research**: actual playback of produced files (no players were run, no end-to-end mux test was built), Media Foundation encoder behavior on this PC, contents of BtbN LGPL builds, OpenH264 2.7.0 compatibility with the crate bindings, muxide/webm Opus handling.
