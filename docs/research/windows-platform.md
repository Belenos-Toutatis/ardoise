# Windows platform research: portable Tauri 2 + BLE + pen injection + ink recognition

Date: 2026-10-24. Target: Windows 11, no admin rights, USB BLE dongle, single unsigned .exe.

Legend: **[V]** verified in a source I read this session; **[P]** verified by a read-only probe on this machine (Win 11 build 10.0.26300, fr-FR); **[U]** unverified, from memory or inference. Treat [U] items as things to test on the work PC before relying on them.

## Summary of verdicts

| # | Question | Verdict |
|---|----------|---------|
| 1 | BLE from Rust | Use **btleplug** behind our own trait; keep the raw `windows` crate as the escape hatch. No OS pairing or admin needed for plain GATT. Risk is the Slate protocol and the dongle, not the library. |
| 2 | Portable Tauri 2 | **Yes**, bare exe works on Win11 (WebView2 Evergreen is part of Win11). Set the WebView2 data dir explicitly to `%LOCALAPPDATA%`, never next to the exe in a synced folder. Unsigned exe: SmartScreen is click-through, but **Smart App Control or AppLocker/WDAC would block it with no override**. |
| 3 | Multi-window / projector | Feasible with standard Tauri 2 APIs. Use a **Channel** with per-frame batching, not `emit`. |
| 4 | System pen | `CreateSyntheticPointerDevice(PT_PEN)` works on Win10 1809+, no admin, no UIAccess. Cannot inject into elevated windows. Fallback `SendInput` mouse. |
| 5 | Handwriting recognition | `InkRecognizerContainer` **works from an unpackaged desktop process** [P]. French recognizer present on this FR machine, **English not**. OCR is a weak fallback for handwriting. |
| 6 | Other blockers | Smart App Control / AppLocker, AV heuristics on unsigned Rust exes, VC++ runtime (link statically), no TCP listeners, Nextcloud locking a running exe. |

---

## 1. BLE from Rust on Windows: btleplug vs `windows` crate

### What each is
- btleplug's Windows backend (`winrtble`) is itself a thin wrapper over the WinRT APIs through the `windows` crate. Its README lists scan, connect/disconnect, read/write, subscribe/notify events, descriptors, MTU retrieval and RSSI as supported on Windows [V: https://github.com/deviceplug/btleplug]. It is central/host mode only.
- Latest release tags seen: 0.13.x [V: `gh api repos/deviceplug/btleplug/tags`, 0.13.5].
- Using `windows` directly means `BluetoothLEAdvertisementWatcher` (scan), `BluetoothLEDevice::FromBluetoothAddressAsync`, `GattDeviceService`, `GattCharacteristic.WriteValueWithResultAsync`, `WriteClientCharacteristicConfigurationDescriptorAsync` and the `ValueChanged` event, `GattSession.MaintainConnection`, `GattSession.MaxPduSize`.

### Pairing / admin
- **No OS-level pairing is required for GATT on Windows.** Microsoft's own flow: scan with `BluetoothLEAdvertisementWatcher` (or `DeviceWatcher` with `GetDeviceSelectorFromPairingState(false)` for *unpaired* devices), then `FromBluetoothAddressAsync` returns the device if it is paired *or in the system cache populated by the scan*; connection is initiated by `GattSession.MaintainConnection = true`, an uncached service discovery, or a read/write [V: https://learn.microsoft.com/en-us/uwp/api/windows.devices.bluetooth.bluetoothledevice.frombluetoothaddressasync ; https://learn.microsoft.com/en-us/windows/apps/develop/devices-sensors/gatt-client].
- No admin is mentioned anywhere; the only "capability" is the UWP `bluetooth` capability, which only applies to packaged apps. Unpackaged desktop apps just call the API [U for the "no prompt" part, but this is how btleplug and all desktop BLE tools work].
- Pairing/bonding is handled by the OS, not by btleplug. A maintainer closed a "handle encryption/bonding" issue with "bonding happens from the OS, not from our library" [V: https://github.com/deviceplug/btleplug/issues/232]. So if a characteristic of the Slate needs an encrypted link, we would have to trigger Windows pairing ourselves (`DeviceInformation.Pairing.PairAsync` / `DeviceInformationCustomPairing`, which can show a system dialog and may need user consent). **Tuhi never calls Pair**: it only does BlueZ `Connect`, then `StartNotify`/`WriteValue`, and the "registration" is an application-level UUID handshake plus a physical button press, explicitly distinct from Bluetooth pairing [V: tuhi `ble.py`, `README.md` "Registering devices", `wacom.py` `register_device`]. This strongly suggests the Slate protocol works over an unbonded link [U: BlueZ could auto-bond silently; not confirmed either way].

### Does a device previously paired in Windows Settings need removing?
- **Recommendation: yes, remove it** (or never add it) [U]. Reasons: (a) the Slate accepts one registered central at a time (Tuhi README: registered with Inkspace/Tuhi, others cannot connect) [V]; (b) a Windows-paired device may be held "connected" by the OS or another app and hide from scans/connect (btleplug issue #459 describes the OS-connected-but-not-in-scan-cache case, and #260 shows connect failing unless the OS had connected first) [V: https://github.com/deviceplug/btleplug/issues/459, /260]; (c) bonded devices can change address resolution behaviour (RPA). If the Slate is simply left in Windows Settings it may work, but remove it to eliminate variables. Not verified with a real Slate.
- Also: if the teacher ever paired the Slate with the official Inkspace app or a phone, re-registration (hold the button ~6 s until the blue LED blinks) is required [V: Tuhi README].

### Known btleplug-on-Windows issues relevant to us [V: issue titles/bodies via `gh`]
- **Connection caching** (#260): `connect()` uses *uncached* service discovery; some devices fail (HRESULT 0x8000FFFF "catastrophic failure") unless already OS-connected; switching to `Cached` fixed it for that reporter but conflicts with #155 (stale cache on some devices). Takeaway: GATT cache mode may need to be configurable, which btleplug does not expose.
- **Stale peripherals** (#405): `peripherals()` keeps showing devices that stopped advertising until the scan is restarted. Handle by restarting the scan and by relying on the advertisement `last seen` logic of our own.
- **Duplicate notification streams** (#326): on Windows each `subscribe()` on the same characteristic adds a handler, producing duplicates; subscribe exactly once per characteristic per connection.
- **Connect/disconnect flakiness** (#429, #301, #300), WinRT exceptions (#250), memory leaks in the winrtble backend (#435 closed, #476 "possible leak in 0.13.1" closed). Keep the app on the latest 0.13.x and run a long-session soak test.
- **Notification length 20** (#341): reported on Windows/Mac/Linux, i.e. ATT MTU/negotiation or device-side, not Windows-only. btleplug lists MTU retrieval as supported on Windows [V]; in raw WinRT use `GattSession.MaxPduSize` [U: not fetched this session]. Windows negotiates MTU itself and cannot be forced from the app [U].
- **Scan flags**: `SetAllowExtendedAdvertisements` / coded PHY can hide legacy-only peripherals (#472, #473); keep defaults.
- Reconnects: after a drop you must re-create the peripheral/service objects and re-subscribe; plan a state machine with retry/backoff [U, design guidance]. Disposing all references triggers disconnect after a short timeout [V: gatt-client doc].
- Connection timing: GATT ops can wait about 7 s each when the device is unreachable and **cannot be cancelled** [V: FromBluetoothAddressAsync remarks]. Tuhi's Slate/Spark need the connection attempt pending *before* the button is pressed (Tuhi README: "you must run connect before pressing the button") [V]. On Windows the equivalent is `MaintainConnection = true` (waits indefinitely and connects when the device appears) [V], which btleplug does not expose; this is the main reason to keep the raw `windows` path ready.

### Dongle requirements
- Needs a **Bluetooth 4.0+ (LE capable) adapter**; classic-only dongles do not work for BLE [U, general knowledge; the WinRT BLE API needs LE support in the radio and stack].
- Driver: Windows' inbox generic Bluetooth USB driver (`BTHUSB`) works for most dongles with no admin. Dongles that need a vendor driver (some CSR8510 clones, older Broadcom) can be flaky on Win10/11 [U]. Plug-and-play driver installation from Windows Update for a standard user is normally allowed for inbox/WU drivers but a locked-down work PC may block new USB devices or Bluetooth entirely via policy [U]; test early.
- Bluetooth 5 dongles with Realtek/Intel chips are the safest buy [U]. Probe: this PC already exposes "Service d'attribut générique Bluetooth LE" and GATT profile devices, so a Bluetooth LE stack exists here [P].

### Recommendation
1. Build the BLE layer behind a small trait (`scan`, `connect`, `write`, `notifications`, `disconnect`).
2. Start with **btleplug** (less code, tokio streams, MTU/notifications working per README).
3. Keep a `windows`-crate implementation as plan B if we hit: cannot keep a pending connection before the button press, GATT cache issues, or MTU/PHY control needs. It is the same API underneath, so effort is moderate and not a new dependency family.
4. First thing to prototype: scan -> connect -> discover -> subscribe -> registration handshake with the real Slate without touching Windows Bluetooth settings.

---

## 2. Tauri 2 as a portable app

### Bare exe on another Win11 PC
- **WebView2 Evergreen Runtime is part of Windows 11** [V: "The Evergreen WebView2 Runtime will be included as part of the Windows 11 operating system"; https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution]. Probe: runtime 155.0.4283.62 registered here [P]. Detection keys: HKLM `...\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}` or HKCU equivalent [V].
- Build a bare exe with `cargo tauri build --no-bundle` or just take `target/release/<app>.exe` [U: standard Tauri behaviour; Tauri's Windows distribution page only documents MSI/NSIS installers and says nothing about portable exes (https://v2.tauri.app/distribute/windows-installer/) [V]].
- **Frontend assets are embedded in the exe** in release builds (`frontendDist` is compiled in via `generate_context!`; served through the custom protocol, no HTTP server, no socket) [U, standard Tauri behaviour; verify by running the exe on a clean user profile with the folder alone].
- Other dependencies: Rust MSVC builds link the dynamic VC++ runtime (`vcruntime140.dll`) by default. Win11 does not guarantee it is present. Use `RUSTFLAGS="-C target-feature=+crt-static"` (or `.cargo/config.toml`) so the exe is truly standalone [U, strongly recommended; test on a clean VM].
- Fixed Version WebView2 (bundling ~180-250 MB runtime) is the fallback for a PC without runtime, and it cannot run from a UNC path; on Win10 it needs icacls steps (admin) [V]. Not needed for Win11.

### WebView2 user data folder (UDF)
- Raw Win32 WebView2 default: `<exe path>.WebView2` next to the exe [V: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder]. Microsoft says to normally specify a custom location, and warns a non-writable exe folder causes a startup error.
- **Tauri 2 does not use that default.** Tauri's config docs say `localData` "on Windows and Linux ... is also the default data directory of the webviews" [V: https://v2.tauri.app/reference/config/]. In practice that is `%LOCALAPPDATA%\<bundle identifier>\EBWebView` [U: exact subfolder from memory of wry/Tauri behaviour; verify by running and checking].
- Override: `WebviewWindowBuilder::data_directory(PathBuf)` (config key `dataDirectory`); the `appDirectoriesOverride` config does *not* change a window's `dataDirectory` [V]. Windows with different browser args must use different data dirs [V: https://docs.rs/tauri/latest/tauri/webview/struct.WebviewBuilder.html]; give both windows the same dir and identical args.
- One UDF can host only one WebView2 session at a time across processes (second instance of the exe shares it) [V: UDF doc]. Use the single-instance plugin or accept sharing.
- **Nextcloud implications**: the UDF holds caches, lockfiles, LevelDB, GPU cache; Microsoft says avoid network drives (slowdowns, crashes, data loss) [V]. A sync client has the same effect (file churn, locked files, conflicts). **Keep the UDF in `%LOCALAPPDATA%` (not roamed, not synced).** Keep the .exe in the synced folder if you like, but note: (a) a running exe is locked, so Nextcloud cannot replace it during an update until the app closes (retries/conflict copy) [U]; (b) with Files On-Demand/virtual files the exe may be a placeholder that must hydrate first [U]. Safer: sync only the user data (notebooks, exports as plain files, atomic write-then-rename), and distribute the exe via a separate folder or a copy-to-local step.
- Fallback if `%LOCALAPPDATA%` is not writable (rare): `%TEMP%\<app>-webview`, and warn that settings in localStorage will not persist; store real settings in our own JSON file instead [U].

### SmartScreen and Mark-of-the-Web (unsigned exe)
- Files downloaded through a browser get a `Zone.Identifier` alternate data stream (ZoneId=3). SmartScreen does reputation checks on such files. Unsigned/unknown exe shows "Windows protected your PC" -> More info -> Run anyway. "Unblock" in Properties or `Unblock-File` removes the tag. None of these need admin in practice (user-owned file) [V for mechanism, U for the admin claim: https://www.winhelponline.com/blog/bulk-unblock-files-downloaded-internet/ ; https://textslashplain.com/2016/04/04/downloads-and-the-mark-of-the-web].
- Files copied by sync clients or USB sticks normally have no MOTW, so no SmartScreen prompt [U: Nextcloud desktop client behaviour not verified; a download via the Nextcloud *web UI* in a browser does set MOTW].
- **Hard blockers that no click can bypass**:
  - **Smart App Control (SAC)**: unsigned apps whose reputation the cloud cannot vouch for are blocked, and "there is currently no way to bypass Smart App Control protection for individual apps"; only turning SAC off or signing the app helps [V: https://support.microsoft.com/en-us/topic/what-is-smart-app-control-285ea03d-fa88-4d56-882e-6698afdb7003]. Probe: on this PC `VerifiedAndReputablePolicyState = 0` (off) [P; the registry value 0 = off from memory, U]. Check this value on the work PC. Turning SAC off may be impossible for a standard user on a managed PC [U].
  - **AppLocker / WDAC / school IT policy**: can block exes in user-writable paths (Downloads, Nextcloud, Desktop) [U]. Ask IT, or test.
- Code signing is the real fix (an OV/EV cert, or Azure Trusted Signing); it also reduces Defender false positives. Not available => document the "More info / Run anyway" step in the teacher's guide.

---

## 3. Multi-window / multi-monitor in Tauri 2

### Enumerate and place windows
- Tauri 2 `Window`/`WebviewWindow`/`AppHandle` expose `available_monitors()`, `primary_monitor()`, `current_monitor()`, `monitor_from_point(x, y)`, `set_fullscreen`, `set_position`, `set_decorations` [V: https://docs.rs/tauri/latest/tauri/window/struct.Window.html]. `Monitor` gives position, size and scale factor [U: from memory of the API].
- Plan: create the projector window with `WebviewWindowBuilder::new(...).decorations(false).position(PhysicalPosition of target monitor).inner_size(target monitor physical size)`, then optionally `set_fullscreen(true)`. Using physical coordinates avoids the mixed-DPI trap (each monitor can have a different scale factor; logical positions are relative to the primary scale) [U]. Fullscreen applies to the monitor the window currently lies on [U].
- Control window stays on the laptop screen; add a monitor chooser in the control window (list names/sizes from `available_monitors()`; persist the choice by monitor name + position, since order changes when the projector is unplugged). Handle hot-plug: re-check monitors on window focus or on a timer and fall back to the primary monitor if the saved one is gone [U].
- **Windows pitfall**: creating windows (and `add_child`/`builder`) from *synchronous* commands or event handlers can deadlock on Windows; use `async` commands or spawn a thread [V: Window docs note].
- Borderless projector window: use a plain borderless window, `always_on_top` optional, `skip_taskbar`, no shadow (`set_shadow(false)`; shadow on undecorated windows draws a 1 px border / rounded corners on Win11) [V: Window docs].
- Capabilities: both windows need to be listed in the capability file with the permissions for window creation and events.

### Performance for ~200 events/s Rust -> webview
- Tauri docs: events are "not designed for low latency or high throughput", always JSON strings, evaluated as JS in the webview, and may arrive out of order; **channels** are "designed to be fast and deliver ordered data" and are used internally for streaming [V: https://v2.tauri.app/develop/calling-frontend/].
- 200 small points/s is only tens of KB/s; either mechanism can carry it, but ordering and per-call overhead favour channels.
- **Recommendation**: `tauri::ipc::Channel<StrokeBatch>` per drawing window, and **batch on the Rust side** (coalesce everything that arrived in the last ~8-16 ms into one message, i.e. 60-120 msgs/s of 1-4 points). Draw in `requestAnimationFrame` with an `OffscreenCanvas`/2D context; predict/interpolate no further than necessary. Keep sending coordinates normalized (0..1 or tablet units) and let each window scale to its own size. Send events only for low-rate state (connection, battery, page change).
- Custom protocol (`register_asynchronous_uri_scheme_protocol`) is only worth it for bulk binary (e.g. exported bitmaps or full page snapshots); it is request/response so not suited to push streaming [U].
- Hidden/occluded webviews can be throttled by Chromium; the projector window is fully visible so fine, but do not start it minimized [U].
- Test: synthetic 500 pts/s sustained for 10 min on the actual laptop with both windows open; watch Rust queue depth and frame time.

---

## 4. Tablet as a system pen (synthetic pointer injection)

- `CreateSyntheticPointerDevice(PT_PEN, 1, mode)` and `InjectSyntheticPointerInput`: **Windows 10 version 1809+ desktop apps only**; for PT_PEN `maxCount` and `count` must be 1; pixel locations in `POINTER_TYPE_INFO` are **relative to the top-left of the virtual screen** [V: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-createsyntheticpointerdevice ; .../nf-winuser-injectsyntheticpointerinput].
- Pen fields: `pressure` 0..1024, `tiltX/Y` -90..90, `rotation` 0..359, with `penMask` flags saying which are valid [V: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-pointer_pen_info]. Hover: send `POINTER_FLAG_INRANGE | POINTER_FLAG_UPDATE` without `INCONTACT`; contact: `NEW | INRANGE | INCONTACT | DOWN`, then `UPDATE`, then `UP`; leave range with `UPDATE` without `INRANGE` (flag semantics from memory) [U].
- **Admin / UIAccess**: the documentation lists no admin or UIAccess requirement [V, absence only]. From general UIPI knowledge [U]: an unelevated process cannot inject input into windows of higher integrity (elevated apps, Task Manager, UAC prompt/secure desktop); UIAccess would lift this but needs a signed exe in a trusted location (Program Files), impossible for an unsigned portable app. Document this as a known limitation.
- **Multi-monitor mapping**: our responsibility. Map tablet coordinates (0..max) to a rectangle on the virtual desktop (a chosen monitor or "active window"), in physical pixels. Make the process per-monitor DPI aware (Tauri's manifest already is [U]) so coordinates are not scaled. Negative coordinates are possible if a monitor is left of / above primary.
- Practical limits [U]: apps that read Windows Ink / pointer pen messages (OneNote, Whiteboard, PowerPoint ink, Chromium pointer events) see real pressure/hover; legacy apps get mouse emulation. The synthetic device lives as long as its handle; call `DestroySyntheticPointerDevice` on exit. The injected input is also delivered to our own windows, so ignore or filter it in the projector canvas to avoid feedback. Behaviour on lock screen / secure desktop: not available.
- **Fallback `SendInput` mouse**: absolute move with `MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK` (normalized 0..65535 across the virtual desktop), left button down/up for contact; no pressure, no hover distinction, shares the user's cursor, and is subject to the same UIPI rules [U].
- Mode: expose "Draw in app only / Pen injection / Mouse fallback" in the control window, with an emergency hotkey to stop injection (a stuck injected contact can make the PC hard to use).

---

## 5. Offline handwriting recognition (French + English)

### `Windows.UI.Input.Inking.InkRecognizerContainer` from a non-packaged app
- The docs page lists it as UniversalApiContract v1.0, Windows 10 10240+, with no packaging requirement; the container "is null if a recognition engine is not available"; the class is **not agile** (create and use it on the same thread/apartment) [V: https://learn.microsoft.com/en-us/uwp/api/windows.ui.input.inking.inkrecognizercontainer]. The fetched page shows no `DualApiPartition` attribute (only Activatable, ContractVersion, MarshalingBehavior, Threading), so it does not appear to be UWP-partitioned [V for what the page shows; I could not fetch metadata beyond that].
- **Empirical probe on this machine** [P]: an unpackaged Windows PowerShell 5.1 process created `InkRecognizerContainer` successfully and `GetRecognizers()` returned **"Reconnaissance d'écriture Microsoft - Français"** and a Spanish recognizer. No English recognizer, because this install has only fr-FR (+ es handwriting) language features. (Probe script: `...\scratchpad\win-probe\p.ps1`; no system changes.)
- Rust: `windows` crate with feature `UI_Input_Inking` (+ `Foundation`, `Foundation_Collections`). Build `InkStroke`s with `InkStrokeBuilder::CreateStrokeFromInkPoints(points, transform)` (each `InkPoint` with position and pressure), add to an `InkStrokeContainer`, then `InkRecognizerContainer::RecognizeAsync(container, InkRecognitionTarget::All)` and read `GetTextCandidates()` per `InkRecognitionResult` [U: API shape from memory]. Initialize COM/WinRT apartment on the thread you use (e.g. a dedicated thread with `RoInitialize(RO_INIT_MULTITHREADED)` or STA) [U].
- Timing/density of points matters for the Microsoft engine; feed original stroke data (the Slate's 200 Hz points scaled to a sensible coordinate space) rather than a bitmap.

### Languages shipped
- Handwriting recognizers are per-language "Windows features on demand" (Language features -> Handwriting), installed with the language pack [U]. A French Windows 11 normally has the French recognizer; **English is only there if the English (US/UK) language is added** [P: absent here]. Adding a language / its handwriting feature through Settings typically needs elevation or Windows Update access, so on a locked work PC **we cannot count on English** [U].
- Enumerate at runtime with `GetRecognizers()` and show the available languages in the UI; degrade gracefully (disable "recognize EN") rather than failing.

### Alternatives
- `Windows.Media.Ocr.OcrEngine` on a rendered `SoftwareBitmap`: works unpackaged presumably (same WinRT), languages come from `AvailableRecognizerLanguages` [V: https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrengine]. Probe: **fr-FR and es-ES available, en not; MaxImageDimension = 10000 px** [P]. OCR is designed for printed text; quality on handwriting is poor to mediocre, especially cursive [U, widely reported; not tested]. It is a reasonable fallback for neat block capitals and mixed content (formulas excluded).
- Other offline options [U]: bundle Tesseract (fra+eng traineddata) - printed text only, weak on handwriting; an ONNX handwriting model (TrOCR-like) shipped in the exe folder - best accuracy but heavy (100s of MB), needs CPU time, and increases AV/size concerns; Windows App SDK `TextRecognizer` (Windows AI) works only on Copilot+ PCs with an NPU, so do not rely on it.
- Recommendation: primary = `InkRecognizerContainer` (FR now, EN if installed); fallback = OCR for the missing language; optional later = bundled model. Make recognition a swappable service with a language-availability probe at startup.

---

## 6. Other "portable, no admin" risks

1. **Smart App Control / AppLocker / WDAC**: see section 2; the single biggest unknown for a work PC. Test the actual exe on that PC first.
2. **Defender / third-party AV on unsigned Rust exes**: heuristic false positives are common for new unsigned binaries [U]. Mitigations: do not pack (no UPX), keep builds reproducible, submit to Microsoft for analysis, sign if possible. Exclusions need admin.
3. **Firewall prompts**: BLE and the Tauri custom protocol (`tauri.localhost`/`http://tauri.localhost` is virtual, no socket) do not open network ports, so no Windows Defender Firewall prompt [U]. **Do not add any TCP/UDP listener** (e.g. LAN sharing, local WebSocket, dev servers) in release: a standard user cannot approve the firewall rule without admin credentials. If one is ever needed, bind to `127.0.0.1` only.
4. **Per-user registry**: don't write HKLM. Avoid file associations, autostart and URL schemes. Tauri plugins used (window-state, single-instance, store) write to `%APPDATA%`/`%LOCALAPPDATA%` only [U]. WebView2 may write under HKCU/`%LOCALAPPDATA%`.
5. **Corporate policy on Bluetooth/USB**: GPO can disable Bluetooth or block new USB devices [U]. Verify the dongle enumerates and `BluetoothLEAdvertisementWatcher` starts on the work PC.
6. **Roaming / redirected profiles**: `%LOCALAPPDATA%` is machine-local; if `%APPDATA%` is roamed, don't put large or locked files there. Store Slate registration UUID in a small JSON (per-user, `%APPDATA%` or alongside notebooks) and **back it up**: losing it means re-registering the device.
7. **VC++ runtime and WebView2 loader**: statically link CRT (section 2). Tauri/wry load WebView2 via the OS, no extra DLL to ship [U].
8. **Power/USB selective suspend**: the BLE dongle may be suspended mid-class; changing power settings can need admin. Add reconnect logic and a visible connection state [U].
9. **Projector sessions**: disable app throttling with a keep-awake request (`SetThreadExecutionState`) to stop sleep during class; no admin needed [U].
10. **Antivirus on Nextcloud sync folder** can slow first launch of the exe (scan on open) [U].

---

## Sources
- btleplug repository and issues: https://github.com/deviceplug/btleplug (issues #232, #260, #326, #341, #405, #429, #459, #472, #473)
- Microsoft: BluetoothLEDevice.FromBluetoothAddressAsync: https://learn.microsoft.com/en-us/uwp/api/windows.devices.bluetooth.bluetoothledevice.frombluetoothaddressasync
- Microsoft: Bluetooth GATT client: https://learn.microsoft.com/en-us/windows/apps/develop/devices-sensors/gatt-client
- Tuhi: https://github.com/tuhiproject/tuhi (README "Registering devices", `tuhi/ble.py`, `tuhi/wacom.py`)
- WebView2 user data folder: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/user-data-folder
- WebView2 distribution: https://learn.microsoft.com/en-us/microsoft-edge/webview2/concepts/distribution
- Tauri Windows installer/WebView2 modes: https://v2.tauri.app/distribute/windows-installer/
- Tauri config reference: https://v2.tauri.app/reference/config/
- Tauri WebviewBuilder: https://docs.rs/tauri/latest/tauri/webview/struct.WebviewBuilder.html
- Tauri Window: https://docs.rs/tauri/latest/tauri/window/struct.Window.html
- Tauri events vs channels: https://v2.tauri.app/develop/calling-frontend/
- Smart App Control: https://support.microsoft.com/en-us/topic/what-is-smart-app-control-285ea03d-fa88-4d56-882e-6698afdb7003
- MOTW/SmartScreen background: https://textslashplain.com/2016/04/04/downloads-and-the-mark-of-the-web ; https://www.winhelponline.com/blog/bulk-unblock-files-downloaded-internet/
- CreateSyntheticPointerDevice: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-createsyntheticpointerdevice
- InjectSyntheticPointerInput: https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-injectsyntheticpointerinput
- POINTER_PEN_INFO: https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-pointer_pen_info
- InkRecognizerContainer: https://learn.microsoft.com/en-us/uwp/api/windows.ui.input.inking.inkrecognizercontainer
- OcrEngine: https://learn.microsoft.com/en-us/uwp/api/windows.media.ocr.ocrengine

## Open items to test on the real work PC (in order)
1. Does the unsigned exe launch (SAC, AppLocker, Defender)?
2. Does the dongle show up and does a BLE scan see the Slate in pairing mode (no Windows pairing)?
3. Registration handshake + pending connection before the button press (btleplug vs `MaintainConnection`).
4. Sustained 200 Hz notification stream: loss, duplicates, latency, over 30+ minutes.
5. `data_directory` location in `%LOCALAPPDATA%` and exe run from the Nextcloud folder.
6. Projector window placement with mixed DPI and hot-unplug.
7. Pen injection into OneNote/PowerPoint and behaviour over an elevated window.
8. `GetRecognizers()` on that PC (is English available?).
