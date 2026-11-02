# Wacom Bamboo Slate (Large / A4) BLE protocol reference, derived from Tuhi

Purpose: reference for reimplementing the Bluetooth LE communication with a Wacom Bamboo Slate
(Large = A4 model) in Rust on Windows.

Source studied (read-only, nothing executed):

- Repository `https://github.com/tuhiproject/tuhi`, shallow clone at commit `aea086fa321a9e6d93ebd013eb6743c175843acf`
  (2024-12-17, "CI: update the CI to current versions"), project version `0.6` (`meson.build`).
- Wiki `https://github.com/tuhiproject/tuhi/wiki` (pages Terminology, Live-Mode, Development, Log-files, FAQs, Home).
- Tuhi GitHub issues (read via `gh issue view`), used for real device traces. Cited as `issue #N`.
  These are user-supplied logs, not repo content, and are less authoritative than the code.

Conventions:

- Hex bytes are written `e6 06 ab cd ...`. All multi-byte integers are little-endian (LE) unless stated otherwise.
- "(inferred)" marks anything not read directly in code or in a quoted device trace.
- Citations are `file :: function/class`. Paths are relative to the Tuhi repo root.
- "Slate" = `ProtocolVersion.SLATE`. This is the protocol the Bamboo Slate uses (see section 3).
  Spark = Bamboo Spark (older), Intuos Pro = Intuos Pro Paper (newer).

Tuhi's README only lists "Bamboo Spark, Bamboo Slate, Intuos Pro Paper" as tested. The wiki has no protocol specification.
The protocol was reverse engineered. Issue #115 says the maintainers later got partial specs under NDA.
The code comments call the specs "ambiguous or outright wrong" in places (`tuhi/protocol.py :: StrokeFile._parse_data`).

---

## 0. Executive summary (the facts you most need)

1. The transport is a Nordic UART style service. Requests are written to one characteristic and replies arrive as
   notifications on another. Frame = `[opcode][length][args...]`. A generic status reply is `b3 01 <code>`.
2. The Slate authenticates with a 6-byte "UUID" chosen by the host at registration (`e7 06 <6 bytes>`, then a physical
   button press produces `e4 01 00`). Every later session starts with `e6 06 <same 6 bytes>` and expects `b3 01 00`.
   A wrong id gives `b3 01 07`.
3. Offline drawings are downloaded one at a time: `cc` (info), `c3` (start), raw bytes stream on a *separate*
   characteristic `ffee0003-...`, then `c8 05 ed <crc32 LE>` ends the transfer, then `ca` deletes the oldest file.
   The device only offers "oldest file", so deletion is how you reach the next one.
4. Stroke data is a compact packed stream. A header byte is a bitmask whose popcount gives the payload length.
   Points are second-order delta coded (a delta accumulates into a velocity). Slate file = 4-byte magic `62 38 62 74`
   followed directly by stroke packets. Intuos Pro uses a 16-byte file header and different stroke headers.
5. Slate Large (A4) reports 29700 x 21600 units of 10 micrometres (297 x 216 mm). The small A5 Slate reports 21600 x 14800.
   The Slate point size (10 um) and max pressure (2047) are hard-coded constants in Tuhi, not queried from the device.
6. Live mode is `b1 01 00` (Mode.LIVE = 0), then pen samples arrive as notifications on `00001524-1212-efde-1523-785feabcd123`:
   `a1 <len>` + repeated 6-byte records `x u16, y u16, pressure u16`. `ff ff ff ff ff ff` means the pen left proximity.
   Stop is mode 2 (IDLE). Normal mode is 1 (PAPER).
7. Tuhi's license is GPL-2.0-or-later (see section 10). It is copyleft, so do not copy code into the Rust port. This
   document records protocol facts, which is the intended use.

---

## 1. GATT services and characteristics

Defined in `tuhi/wacom.py` (module constants at the top) and `tools/parse_log.py` (extra UUIDs seen in btsnoop captures).

| Role | Service UUID | Characteristic UUID | Direction / usage |
|---|---|---|---|
| Nordic UART service | `6e400001-b5a3-f393-e0a9-e50e24dcca9e` | | |
| Nordic UART TX (host to device) | | `6e400002-b5a3-f393-e0a9-e50e24dcca9e` | **Written** by the host: every command (`WacomProtocolLowLevelComm.send_nordic_command` calls `chrc.write_value`). |
| Nordic UART RX (device to host) | | `6e400003-b5a3-f393-e0a9-e50e24dcca9e` | **Notified**: every reply (`device.connect_gatt_value(NORDIC_UART_CHRC_RX_UUID, ...)` which calls StartNotify). |
| Wacom "live" service | `00001523-1212-efde-1523-785feabcd123` | | |
| Live pen data | | `00001524-1212-efde-1523-785feabcd123` | **Notified**: real-time pen samples (section 7). Subscribed in `WacomProtocolBase.__init__`. |
| Wacom "offline" service | `ffee0001-bbaa-9988-7766-554433221100` | | |
| Offline pen data | | `ffee0003-bbaa-9988-7766-554433221100` | **Notified**: raw stored-drawing bytes during a download (section 5). Subscribed in `WacomProtocolBase.__init__`. |
| Offline "FW data" | | `ffee0002-bbaa-9988-7766-554433221100` | Named `WACOM_OFFLINE_FW_DATA_UUID` only in `tools/parse_log.py` ("Sending FW Data"). Not used by Tuhi. |
| "Sysevent" service (named "mysterious" in `parse_log.py`) | `3a340720-c572-11e5-86c5-0002a5d5c51b` | | |
| Sysevent notification | | `3a340721-c572-11e5-86c5-0002a5d5c51b` | **Notified**, only subscribed by Slate and later (`WacomProtocolSlate.__init__`). Tuhi just logs it. Our Slate A4 sends the battery level there (`f1 02 <percent> <flag>`), see section 12. |
| Nordic DFU (firmware update) | `00001530-1212-efde-1523-785feabcd123` | `00001531-...` control point, `00001532-...` packet, `00001534-...` unknown | Listed in `tools/parse_log.py` only. **Never touch it** (firmware update, risk of bricking). |
| Standard services | `1800` GAP, `1801` GATT, `180f` Battery, `180a` Device Info | | Present on a Slate (gatttool dump in issue #212), not used by Tuhi. Battery comes from opcode `b9`. |

Observed service list of a real Bamboo Slate (issue #212, `gatttool primary`): `1800`, `1801`, `00001523-...`,
`ffee0001-...`, `180f`, `180a`, `6e400001-...`, `3a340720-...`, `00001530-...`. Per-characteristic properties
(write vs write-without-response, notify vs indicate) are **not** in the repo. Tuhi calls BlueZ `WriteValue(data, {})`
with no write-type option (`tuhi/ble.py :: BlueZCharacteristic.write_value`). On Windows, read the characteristic
properties and use whichever write type is supported (inferred).

Subscription order in Tuhi: Nordic RX (in `WacomProtocolLowLevelComm.__init__`), then live `00001524`, then offline
`ffee0003` (in `WacomProtocolBase.__init__`), then sysevent `3a340721` (Slate only). All happen right after the BLE
connection is established and before the first command. Sysevent traffic seen on connect: `f1 02 13 00` (issue #212),
which our captures read as a battery level of 19 % (section 12).

Advertising and scan filter (`tuhi/base.py :: Tuhi._add_device`, `WACOM_COMPANY_IDS`):

- Tuhi only talks to devices whose BLE manufacturer-specific data company id is one of `0x4755`, `0x4157`, `0x424d`.
  A device with no manufacturer data is accepted only if it is already registered.
- Manufacturer data is **7 bytes in normal mode and 4 bytes while the device is in register mode** (blue LED blinking).
  See wiki "Development" > "BlueZ ManufacturerData" and `Tuhi._add_device` (`len(manufacturer_data) == 4` means REGISTER mode).
  The BlueZ property update lags, so it can stay 4 bytes for one connection after a successful registration.
- Device local names seen in issues: `Bamboo Slate`, `Bamboo Slate Small`, `Bamboo Spark`, `LE Paper` (Intuos). Tuhi deliberately
  does not detect the protocol by name (issue #42).
- The device is not BLE-bonded or paired by Tuhi. The wiki says "Tuhi-compatible devices do not need to be paired", and
  authentication is purely the app-level UUID (section 3).

---

## 2. Message framing

Source: `tuhi/protocol.py :: NordicData`, `Msg.execute`, `DeviceError`;
`tuhi/wacom.py :: WacomProtocolLowLevelComm.send_nordic_command / pop_next_message / nordic_data_exchange`.

### 2.1 Frame format (both directions, Nordic UART characteristics)

```
byte 0      opcode
byte 1      L = number of argument bytes that follow (0..255; practically <= 18 because a frame must fit one 20-byte ATT payload)
byte 2..    L argument bytes
```

- Every request has at least one argument byte. An "empty" request is sent as `opcode 01 00` (`Msg.args` defaults to `[0x00]`:
  "Empty messages don't exist").
- Replies use their own opcodes (usually not the request opcode, see the table in 2.3).
- `NordicData.__init__` rejects a frame whose length byte does not equal the number of bytes present
  (`UnexpectedDataError`). Tuhi does **not** reassemble a frame split across several notifications
  (`pop_next_message` raises `Invalid answer message length`) (inferred consequence: all known replies fit one notification).
- Tuhi appends every notification to a byte buffer, releases a semaphore once per notification, and parses one frame per wait
  (so concurrent notifications such as `c8 ed` followed immediately by `c9` do not overwrite each other, the fix for issue #90).

### 2.2 Generic status reply `b3`

```
b3 01 <code>
```

`Msg.execute` treats opcode `b3` specially for every message: `code == 00` is success, anything else raises `DeviceError(code)`.

| code | name (`DeviceError.ErrorCode`) | notes |
|---|---|---|
| `00` | SUCCESS | |
| `01` | GENERAL_ERROR | Spark answers `e6` (connect) with this while in pairing mode; also seen when the device is in the wrong state (issue #202) |
| `02` | INVALID_STATE | e.g. device not in the right mode. Tuhi logs "make sure the LED is blue and the button is pressed to switch it back to green" |
| `03` | READ_ONLY_PARAM | |
| `04` | COMMAND_NOT_SUPPORTED | |
| `07` | AUTHORIZATION_ERROR | wrong/unknown host UUID (Slate). Seen verbatim `e6 06 a0 bd 39 11 79 74` then `b3 01 07` in issue #212 |

Any other code value raises `ValueError` inside `DeviceError.ErrorCode(...)` (not handled) (inferred risk).

Note: `MsgConnectSlate._handle_reply` contains code to convert `AUTHORIZATION_ERROR` into `AuthorizationError`, but `b3` replies are
intercepted earlier in `Msg.execute`, so in practice a Slate auth failure surfaces as `DeviceError.AUTHORIZATION_ERROR`
(matches the logs in issues #212 and #266). Treat `b3 01 07` after `e6` as "re-register needed".

### 2.3 Timeouts and flow control at the framing level

- Default reply wait: 5 s (`nordic_data_exchange`: `timeout or 5`). No reply raises `MissingReplyError` (errno ETIME). There is no automatic retry.
- Requests that expect no reply (`requires_reply = False`): `e7`/`e3` registration press-button on the Spark and Slate,
  `ca` delete on Spark.
- Some "requests" are pure waits (`requires_request = False`): wait-for-button (10 s) and wait-for-end-of-read (5 s).
- A reply can sometimes be delivered as several frames of the same opcode: `bb` get-name loops on `bc` until the last byte is `0a`.

### 2.4 Opcode catalogue per protocol (all classes in `tuhi/protocol.py`)

The lookup `_get_protocol_dictionary` picks, for each interaction, the message class with the highest `protocol` that is `<=` the
requested protocol. So Slate inherits every ANY (Spark-era) message unless a SLATE-specific class exists.

Requests (host to device) and their replies, **as used for the Slate**:

| Interaction | Request `opcode len args` | Expected reply | Class | Section |
|---|---|---|---|---|
| CONNECT (auth) | `e6 06 <uuid6>` | `b3 01 00`, or `b3 01 07` | `MsgConnectSlate` | 3 |
| REGISTER_PRESS_BUTTON | `e7 06 <uuid6>` | none immediately | `MsgRegisterPressButtonSlateOrIntuosPro` | 3 |
| REGISTER_WAIT_FOR_BUTTON | (no request) | `e4 01 00` (Slate) | `MsgRegisterWaitForButtonSlateOrIntuosPro` | 3 |
| REGISTER_COMPLETE | (noop on Slate) | | `MsgRegisterCompleteSlate` | 3 |
| GET_NAME | `bb 01 00` | `bc n <name bytes>` | `MsgGetName` | 4 |
| SET_NAME | `bb n+1 <name bytes> 0a` | `b3 01 00` | `MsgSetName` | 4 |
| GET_TIME | `b6 01 00` | `bd 06 <6 BCD bytes>` | `MsgGetTime` | 4 |
| SET_TIME | `b6 06 <6 BCD bytes>` | `b3 01 00` | `MsgSetTime` | 4 |
| GET_FIRMWARE | `b7 01 00` then `b7 01 01` | `b8 n <idx> <digits>` (twice; one byte per hex digit on a Slate) | `MsgGetFirmwareVersion` | 4 |
| GET_BATTERY | `b9 01 00` | `ba 02 <pct> <charging>` | `MsgGetBattery` | 4 |
| GET_WIDTH | `ea 02 03 00` | `eb 06 03 00 <u32>` | `MsgGetWidthSlate` | 4 |
| GET_HEIGHT | `ea 02 04 00` | `eb 06 04 00 <u32>` | `MsgGetHeightSlate` | 4 |
| GET_POINT_SIZE | (noop, constant 10 um) | | `MsgGetPointSizeSpark` | 4 |
| SET_FILE_TRANSFER_REPORTING_TYPE | `ec 06 06 00 00 00 00 00` | `b3 01 00` | `MsgSetFileTransferReportingType` | 5 |
| SET_MODE | `b1 01 <mode>` | `b3 01 00` | `MsgSetMode` | 5, 7 |
| AVAILABLE_FILES_COUNT | `c1 01 00` | `c2 02 <u16 LE>` | `MsgAvailableFilesCountSlate` | 5 |
| GET_STROKES (file info) | `cc 01 00` | `cf 0a <u32 LE> <6 BCD bytes>` | `MsgGetStrokesSlate` | 5 |
| DOWNLOAD_OLDEST_FILE | `c3 01 00` | `c8 .. be ..` | `MsgDownloadOldestFile` | 5 |
| WAIT_FOR_END_READ | (no request) | `c8 05 ed <crc32 LE>` | `MsgWaitForEndReadSlate` | 5 |
| DELETE_OLDEST_FILE | `ca 01 00` | `b3 01 00` | `MsgDeleteOldestFileSlate` | 5 |
| UNKNOWN_E3 | `e3 01 00` | `b3 01 00` | `MsgUnknownE3Command` | 3 (not used in the Slate flow) |

Differences for the other devices are listed inline in the sections below and summarised in the table in section 3.4.

---

## 3. Registration, authentication, and protocol versions

Sources: `tuhi/wacom.py :: WacomRegisterHelper.register_device`, `WacomDevice.register_device`, `WacomProtocolSlate.register_device_finish`,
`WacomProtocolSlate.retrieve_data`, `WacomProtocolBase.check_connection`; `tuhi/protocol.py :: MsgConnect*`, `MsgRegister*`;
`tuhi/config.py`; README "Registering devices"; wiki Terminology.

### 3.1 The "UUID" (host identifier)

- It is not a standard 128-bit UUID. It is **6 random bytes** chosen by the host: `uuid.uuid4().hex[:12]` (12 hex characters),
  `WacomDevice.register_device`. It is sent as 6 raw bytes (`binascii.unhexlify`) and stored as the 12-char hex string in
  `settings.ini` (`tuhi/config.py :: TuhiConfig.new_device`, asserts `len(uuid) == 12`).
- Semantics (README): the device stores one id. Registering with another app (Wacom Inkspace) overwrites it, and the previous
  app is locked out until it re-registers. Nothing is cloud related.
- Persisted per device in `$XDG_DATA_HOME/tuhi/<BT address>/settings.ini`:
  ```
  [Device]
  Address=E2:43:03:67:0E:01
  UUID=dead00beef00
  Protocol=slate
  ```
  `Protocol` values: `spark`, `slate`, `intuos_pro` (`ProtocolVersion.from_string`; matching is exact, so `Slate` fails).
  (`example.ini`, `tuhi/config.py`)

### 3.2 Registration flow (Slate)

1. User **holds the device button 6+ seconds** until the blue LED blinks (README, wiki Terminology). The device advertises with
   4-byte manufacturer data.
2. Host scans, picks the device, connects (plain GATT connect, no bonding), discovers services, subscribes to notifications.
3. Host chooses `uuid6` and sends `e7 06 <uuid6>` (REGISTER_PRESS_BUTTON, no reply). Tuhi then tells the user to press the button.
   Slate/Intuos: `WacomRegisterHelper.register_device`, branch `else:` (it first creates a `ProtocolVersion.SLATE` protocol object).
4. User presses the physical button. The device sends `e4 01 00` on Nordic RX (Slate). Wait timeout is **10 s**
   (`MsgRegisterWaitForButton*`, `kwargs['timeout'] = 10`). Intuos Pro would send opcode `53` here instead.
5. The reply opcode decides the stored protocol: `e4` = SLATE, `53` = INTUOS_PRO. Any other opcode raises `UnexpectedReply`.
6. Post-registration commands (`WacomProtocolSlate.register_device_finish`), in this order:
   `b6` SET_TIME, `b6` GET_TIME (log a warning if off by more than 300 s), `ec` SET_FILE_TRANSFER_REPORTING_TYPE, `bb` GET_NAME,
   `ea 03`/`ea 04` dimensions, `b7` x2 firmware, `b9` battery. No `e5` REGISTER_COMPLETE on Slate (it is a noop; `e5 01 00` exists
   only for the Spark path), and no `e3`.
   Real trace of the start of a Slate registration (issue #117, Slate Small):
   ```
   TX e7 06 89 ea 78 88 ff cb          (registration, uuid = 89ea7888ffcb)
   RX e4 01 00                          (button pressed)
   TX b6 06 19 03 15 03 22 16  RX b3 01 00        (set time 2019-03-15 03:22:16 UTC)
   TX b6 01 00                 RX bd 06 19 03 15 03 22 16
   TX ec 06 06 00 00 00 00 00  RX b3 01 00
   TX bb 01 00                 RX bc 12 "Bamboo Slate Small"
   ```
7. Tuhi saves `Address`, `UUID`, `Protocol=slate`, then disconnects.

### 3.3 Normal (authenticated) connect flow for a Slate

`WacomProtocolSlate.retrieve_data`:

1. User presses the device button briefly (device advertises with 7-byte manufacturer data, host connects). On the Spark you must
   connect *before/while* pressing, and the Spark only connects when drawings are pending (README "Device notes"). For the Slate
   the code makes no such statement (inferred: similar behaviour possible).
2. `e6 06 <uuid6>` CONNECT, expects `b3 01 00`. If `b3 01 07`: authorization error, device must be re-registered.
3. `b6` SET_TIME (current UTC BCD, section 4).
4. `b9` GET_BATTERY, then `ea 03`, `ea 04` dimensions.
5. `b7` x2 firmware, `ec` transfer reporting type (once per session).
6. Offline sync (section 5): `b1 01 01` PAPER mode, `c1` count, loop of `cc`, `c3`, data, `c8 ed`, `ca`.
   **The order of 5 and 6 matters on a real Slate A4: `ec` sent after `b1 01 01` is refused (`b3 01 02`), see 5.3.**
7. Tuhi disconnects (`_on_fetching_finished`). The device also disconnects by itself after a few seconds idle (README says about 10 s on the Spark; issue #97 says the device "disconnects itself" even in live mode).

Warning for the port: on any error (`DeviceError.INVALID_STATE`) Tuhi just logs "no data, please make sure the LED is blue and the button is pressed to switch
it back to green" (`retrieve_data`). The meaning of the LED colours is not defined in code beyond this message (blue blinking = register mode per the README).

### 3.4 Protocol versions and how they are detected

Enum `ProtocolVersion` in `tuhi/protocol.py`: `ANY=0, SPARK=1, SLATE=2, INTUOS_PRO=3` (numeric values are only for ordering).

Detection (done only once, at registration, then stored in `settings.ini`):

1. `WacomRegisterHelper.is_spark(device)`: **Spark = the device does not expose characteristic `3a340721-c572-11e5-86c5-0002a5d5c51b`**.
   If it is a Spark, protocol SPARK is used for registration: `e6` (expects a GENERAL_ERROR `b3 01 01` "as expected"), then `e3 01 01`
   (press button), then wait for `e4`.
2. Otherwise start as SLATE (`e7 06 <uuid6>`) and decide by the button-press reply opcode: `e4` -> SLATE, `53` -> INTUOS_PRO
   (`MsgRegisterWaitForButtonSlateOrIntuosPro._handle_reply`).
3. Afterwards `WacomDevice._init_protocol` instantiates `WacomProtocolSpark` / `WacomProtocolSlate` / `WacomProtocolIntuosPro` from the stored value.

**The Bamboo Slate (small and large) uses `ProtocolVersion.SLATE`** (README lists it; issue #117 trace "is using protocol Protocol.SLATE"; issue #257 "protocol: slate").

Per-protocol differences (the table to keep for the port):

| Feature | Spark | **Slate** | Intuos Pro |
|---|---|---|---|
| Auth request | `e6` (+ `b3 01 01` during pairing) | `e6 06 uuid`, `b3 01 00` or `b3 01 07` | `e6 06 uuid`, reply `50 06 <uuid>` ok or `51 07 <uuid> <reason>` (reason 0,3 = invalid state; 1,2 = wrong uuid) |
| Register request | `e3 01 01` | `e7 06 uuid` | `e7 06 uuid` |
| Register button reply | `e4` | `e4 01 00` | `53` |
| Register complete | `e5` | noop | noop |
| Get/set name | `bb` (+ trailing `0a`) | same as Spark | `db` (no `0a`) |
| Time get/set | `b6` BCD | same (BCD) | set `b6` / get `d6`, u32 LE seconds + 2 bytes |
| Files count `c1`/`c2` | u16 big-endian | u16 LE | u16 LE |
| File info | `c5` -> `c7`(u32 BE) and `cd`(BCD time) | `cc` -> `cf` (u32 LE + BCD time) | `cc` -> `cf` (u32 LE + u32 LE unix seconds) |
| Download end | `c8 ed` then `c9 <crc BE>` | `c8 ed <crc LE>` | same as Slate |
| Delete | `ca` (no reply) | `ca` -> `b3` | same as Slate |
| Width/height | not queryable | `ea 03 00` / `ea 04 00` | same as Slate |
| Point size | const 10 um | const 10 um | `ea 14 00`, value minus 1 (reply 6, real size 5) |
| File header | `62 38 62 74` | `62 38 62 74` | `67 82 69 65` + 16-byte header |
| Stroke header | none | `ff ee ee ..` | `ff fa ..` |
| Live mode | not implemented (issue #254) | yes | yes |
| Max pressure (Tuhi const) | 1023 | 2047 | 8192 |

---

## 4. Device information commands

All via `tuhi/protocol.py` message classes; high-level wrappers in `tuhi/wacom.py :: WacomProtocolBase` (`get_name`, `get_firmware_version`,
`get_battery_info`, `read_time`, `set_time`, `update_dimensions`).

### 4.1 Name (get / set)

- Get: `bb 01 00` -> `bc n <n bytes>`. Reassembly rule (`MsgGetName._handle_reply`): append the bytes, and **if the last byte is not `0a`
  wait for another `bc` frame** (re-read without sending anything; 5 s timeout). So the Slate/Spark name carries a trailing LF (`Bamboo Slate\n`
  seen in issue #117).
- Set: `bb n+1 <ASCII name> 0a`. The code comment says the Spark firmware gets confused without the trailing linebreak. Reply `b3 01 00`.
  Only `ord(c)` bytes, i.e. effectively ASCII/Latin-1, no length limit checked in code (the 20-byte frame limit implies about 17 chars, inferred).
- Quirk: in the Slate Small trace (issue #117) the 18-byte name `Bamboo Slate Small` came back with **no trailing 0a**, and a later stray frame
  `bc 09 00 "Bamboo " 0a` arrived. The current Tuhi loop would wait for another frame (probably to cope with this), but exactly how the device chunks long names is
  not documented. For the Slate Large the name is a single frame ending in `0a`. Port should: collect frames until `0a`, or until a short timeout, then trim `\n`/`\0`
  (inferred).
- Intuos Pro: opcode `db`, no trailing `0a`, single frame.

### 4.2 Firmware version

`b7 01 00` -> `b8 n 00 <digits...>` ("hi" part) and `b7 01 01` -> `b8 n 01 <digits...>` ("lo" part). Result string = `hi-lo`.
Real values: Slate A4 `B160805A-C170704A` (issue #117), Slate (issue #115) `B160805A-C160909B`, Intuos Pro `B180131a-C325.003` (issue #256,
trace `b8 09 00 42 31 38 30 31 33 31 61`).
**Encoding, settled by our own capture (real Bamboo Slate A4, 2026-11-02):** the Slate sends **one byte per hexadecimal digit**, not ASCII:
`b8 09 00 0b 01 06 00 09 02 09 0a` and `b8 09 01 0c 01 07 00 07 00 04 0a` are `B160929A` and `C170704A`. This is what Tuhi's `hex(d)[2:]`
(`MsgGetFirmwareVersion`, ANY/SPARK/SLATE) and its Slate unit test do; the versions quoted in issues #115/#117 are that hex rendering. The Intuos
Pro sends ASCII (`chr(d)`, trace of issue #256). Ardoise reads a reply whose bytes are all below `10` as digits, anything else as ASCII. Note that
the digit `0a` is not a line feed here: trimming whitespace from an ASCII reading loses the last digit.

### 4.3 Battery

`b9 01 00` -> `ba 02 <percent 0..100> <charging: 01 = charging, else 00>`. Example Intuos: `ba 02 3e 00` = 62 %, discharging.
Tuhi refreshes every connect; the GUI marks the value unknown after 300 s (`base.py :: BATTERY_UPDATE_MIN_INTERVAL`).

### 4.4 Time

- Spark and Slate (`MsgSetTime`, `MsgGetTime`, protocol ANY): 6 bytes, each byte is a **BCD pair** equal to `strftime('%y%m%d%H%M%S')` read as hex.
  Example `19 03 15 03 22 16` = 2019-03-15 03:22:16. Assumed **UTC** (code uses `gmtime`/`timegm`). Set: `b6 06 <6 BCD>` -> `b3 01 00`.
  Get: `b6 01 00` -> `bd 06 <6 BCD>`. Year is 2-digit (`%y`, so 2000-2068 with Python's strptime).
- Intuos Pro: set `b6 06 <u32 LE unix seconds> 00 00`; get uses opcode `d6` -> `bd 06 <u32 LE> <2 bytes ms>`. Not used by Slate.
- Tuhi sets the time on every sync, and logs an error if the device time differs by more than 300 s from the host (`read_time`).

### 4.5 Dimensions, pressure range, point size / resolution

Slate:

- `ea 02 03 00` -> `eb 06 03 00 <u32 LE width>`; `ea 02 04 00` -> `eb 06 04 00 <u32 LE height>`. Both checked (`reply[0:2] == 03 00` / `04 00`, length 6).
- Units are "points" of **10 micrometres** (0.01 mm). There is **no query for the point size on Spark/Slate**: `MsgGetPointSizeSpark` is a noop returning 10.
  (`update_dimensions` computes `width_um = w * point_size`; `Drawing.dimensions` is stored in micrometres.)
- Maximum pressure is **not queried**. It is a class constant: Spark 1023, **Slate 2047**, Intuos Pro 8192 (`WacomProtocolSpark/Slate/IntuosPro.pressure`).
  Tuhi normalises to 0..65535 with `65536 * p / pressure_max`.
- The wire reply `eb 06 03 00 60 54 00 00` = width 0x5460 = 21600; `eb 06 04 00 d0 39 00 00` = height 0x39d0 = 14800 (issue #266, a "Bamboo Slate").
- The reply `b3 01 02` instead of `eb` happens when the device is in the wrong state (issue #124, #202), so treat `b3` as a legal reply to `ea`.

Concrete values (units of 10 um for Spark/Slate, 5 um for Intuos Pro):

| Device | Width x height (units) | Physical | Pressure max | Source |
|---|---|---|---|---|
| Bamboo Spark | 21000 x 14800 (hard-coded, no query) | 210 x 148 mm | 1023 | `WacomProtocolSpark`, `MsgGetWidthSpark/HeightSpark`; live x range starts at 2100 |
| Bamboo Slate small (A5-ish; advertised as `Bamboo Slate` in issue #266) | 21600 x 14800 | 216 x 148 mm | 2047 | `WacomProtocolSlate` class defaults; trace `eb 06 03 00 60 54 ..` in issue #266 |
| **Bamboo Slate Large (A4)** | **29700 x 21600** | **297 x 216 mm** | 2047 (same constant, device not queried) | issues #115, #117, #118: "incompatible dimensions: 29700x21600"; `WacomProtocolSlate.live_mode` comment "Slate tablet has two models A5 and A4" |
| Intuos Pro Paper (M) | 44800 x 29600 | 224 x 148 mm | 8192 | `WacomProtocolIntuosPro`; trace `eb 06 03 00 00 af ..` (0xaf00) and `eb 06 04 00 a0 73 ..` (0x73a0) in issue #256 |

So **yes, the Slate Large differs from the small Slate** and its size can only be learned by asking the device (`ea 03`/`ea 04`). The A4 values come from user-supplied
logs in issues, not from a repo fixture; the width/height pair arrives as (29700, 21600), i.e. width > height, matching a landscape coordinate frame of the sensor
(see orientation in section 6).

Live-mode logical range used for the virtual tablet (`WacomProtocolSlate.live_mode`): after reading the real dimensions,
`x_min = 2500`, `x_max = width_units - 1000`, `y_min = 500`, `y_max = height_units - 500` (margin for the paper-pad frame, issue #88).
For the small Slate defaults: x 2500..20600, y 500..14300.

Intuos Pro point size: `ea 02 14 00` -> `eb 06 14 00 06 00 00 00`; code subtracts 1 ("the IntuosPro returns 6 but a point size of 5 matches the physical dimensions"). Not applicable to the Slate.

---

## 5. Listing, downloading and deleting stored drawings

Sources: `tuhi/wacom.py :: WacomProtocolBase.read_offline_data / set_paper_mode / count_available_files / get_stroke_data /
start_downloading_oldest_file / wait_for_end_read / delete_oldest_file / _on_pen_data_received`;
`tuhi/protocol.py :: MsgSetMode, MsgAvailableFilesCountSlate, MsgGetStrokesSlate, MsgDownloadOldestFile, MsgWaitForEndReadSlate, MsgDeleteOldestFileSlate`;
wiki FAQs ("Why does Tuhi delete drawings from the device?").

### 5.1 Sequence for the Slate

```
(connected + authenticated + time set + ec sent, see section 3.3)
TX b1 01 01                 mode PAPER                  RX b3 01 00      (Tuhi actually sends this twice, see quirk)
TX c1 01 00                 files available?            RX c2 02 NN NN   (u16 LE)
repeat while remaining > 0:
  TX cc 01 00               info about the oldest file  RX cf 0a SS SS SS SS yy mm dd hh mm ss   (u32 LE, BCD time)
  TX c3 01 00               start download              RX c8 .. be ...  (start marker)
  ... N notifications on characteristic ffee0003-bbaa-9988-7766-554433221100 carry the raw file bytes ...
                                                        RX c8 05 ed C0 C1 C2 C3   (end marker + CRC-32, LE)
  host: crc32(all ffee0003 bytes received) must equal the CRC; else abort ("CRCs do not match")
  host: parse the stroke data (section 6)
  TX ca 01 00               delete the oldest file      RX b3 01 00
(next iteration now sees the next-oldest file)
```

Details:

- Only two file-management commands exist: "download the oldest" and "delete the oldest" (FAQ). Tuhi cannot address files individually or download without
  deleting except with its `--peek` option, which downloads the oldest drawing and stops after it without deleting (`TuhiConfig().peek_at_drawing`).
- `c1` -> `c2`: the count. For Spark it is big-endian (`MsgAvailableFilesCount`), for Slate/Intuos little-endian (issue #156 discusses the Spark endianness doubt).
- `cc` -> `cf` (Slate): bytes 0..3 = u32 LE "count" which Tuhi logs as "receiving {count} bytes". Its meaning is therefore the size in bytes of the file
  (inferred; the Intuos trace shows `cf 0a 7c 0d 00 00 ...` = 3452 while the following `c8` start reply carries 0x0d70 = 3440, so the two numbers do not match exactly; Tuhi does not use either to validate length).
  Bytes 4..9 = BCD `yymmddhhmmss` UTC = the drawing's timestamp (the **only** timestamp for a Slate drawing, since the Slate file header has none).
- `c3` -> `c8`: `MsgDownloadOldestFile._handle_reply` requires opcode `c8` and first argument byte `be`. In the test suite the reply is `c8 01 be`; in a real
  Intuos Pro trace it is `c8 0b be <u32 LE size> <u32 LE unix time> 00 00` (issue #256; Slate shape not captured, inferred to be at least `c8 01 be`).
- Data phase: **no framing, no sequence numbers, no per-chunk acknowledgement from the host.** Every notification payload on `ffee0003` is appended verbatim to
  a buffer (`_on_pen_data_received`). Observed chunk size is 20 bytes per notification in the Intuos trace (issue #256), i.e. the default 20-byte ATT payload; the last chunk is shorter (inferred).
- End phase: Slate sends `c8` with first byte `ed` followed by 4 CRC bytes. Tuhi reverses them (`crc.reverse()`) so they are **little-endian**, and compares with
  `binascii.crc32(pen_data)` (the standard IEEE CRC-32, as in zlib, unsigned) over the whole file as received. (Spark instead sends `c8 ed` then a separate `c9 <crc big-endian>`.)
- Waiting for the end marker: `wait_for_end_read` repeatedly waits for the `c8 ed` frame with a 5 s timeout per attempt and keeps retrying as long as pen data was
  received within the last 5 s (issue #91: a large drawing otherwise "times out" erroneously). If nothing arrives for 5 s, it raises `MissingReplyError`.
- Order hazard (inferred): the `c8 ed` frame (Nordic RX) and the last data chunks (`ffee0003`) are separate characteristics. Tuhi reads its buffer immediately after
  the end marker; a robust port should keep draining `ffee0003` for a short moment, or verify the CRC and retry once, before declaring a mismatch.
- On success Tuhi parses the buffer, emits the drawing, and **only then** sends `ca`. If anything fails earlier (timeout, CRC mismatch, parse error) it does **not**
  delete, so the drawing remains on the device (wiki FAQ "Tuhi crashed while downloading").
- `ca 01 00` on Slate replies `b3 01 00`. On Spark there is no reply.
- `ec 06 06 00 00 00 00 00` (SET_FILE_TRANSFER_REPORTING_TYPE) must be sent first on Slate/Intuos. The docstring says "Changes where the device needs to send the data to. 0x00 is on the
  FFEE0003 GATT". The meaning of the first byte `06` is unknown (it looks like a sub-command or length); the fixed 6 argument bytes are `06 00 00 00 00 00` in every trace.
  The Spark flow does not send it. A real Slate A4 refuses it in paper mode: section 5.3.
- Quirk: `set_paper_mode` calls `.execute()` on an already executed message, so `b1 01 01` is sent **twice** (visible in the issue #256 log). Harmless; one is enough (inferred).

### 5.2 Loop control in Tuhi

`read_offline_data`: `set_paper_mode` -> `n = count` -> `while n > 0: info; download; wait_end_read(crc); parse; emit; n -= 1; delete`. Returns whether any file was available.

### 5.3 `ec` and the modes on a real Slate A4 (our captures, 2026-11-02)

Source: Ardoise's diagnostic journals of the user's Bamboo Slate A4 (firmware `B160929A-C170704A`), 23 sessions on 2026-11-02.
The journals are private; only the command frames below are quoted.

- `ec 06 06 00 00 00 00 00` sent right after `b1 01 01` (paper mode, answered `b3 01 00`) is answered **`b3 01 02` (INVALID_STATE)**, every
  time: 19 automatic connections and the guided test. Nothing can be downloaded after that.
- In the one session where it was sent right after `b1 01 02` (idle mode, which followed `b1 01 00` live mode), it was answered `b3 01 00`;
  `b1 01 01`, `c1` (4), then four `cc` / `c3` / data / `c8 05 ed <crc>` / `ca` cycles followed, every CRC matched and every `ca` was answered
  `b3 01 00`.
- Later on that same link, `b1 01 01` then `ec` again: refused again (`b3 01 02`).
- This agrees with Tuhi's order (3.3: `ec` before `b1 01 01`, "once per session"), where `ec` comes right after the connection and the
  information commands, and is accepted (issue #117, Slate Small, during the registration).

What Ardoise does since (and the simulator models): `ec` once per link before the first paper mode; again after live mode, from idle mode;
never in paper or live mode; a refusal is retried once after `b1 01 02`. **Not observed yet** (to check at H1): `ec` right after the
connection on this tablet (Tuhi's order); whether a second sync on the same link works without a new `ec` (assumed, as Tuhi: once per session);
whether a download without `ec` is refused (the simulator assumes `b3 01 02`); `ec` in live mode (the simulator assumes a refusal); the mode of a
fresh connection (the simulator assumes idle, since Tuhi's `ec` right after connecting is accepted).

---

## 6. Stroke data encoding

Sources: `tuhi/protocol.py :: StrokeDataType.identify, StrokeFile, StrokeFileHeader, StrokeHeader, StrokeDelta, StrokePoint, StrokeEOF, StrokeEndOfStroke,
StrokeLostPoint, StrokePacketUnknown`; `tuhi/drawing.py`; `tuhi/export.py`; `test/test_strokes.py`.

The decoder in Tuhi is shared by all devices; the data buffer for **one file** (the bytes collected on `ffee0003`) is fed to `StrokeFile(data)`.

### 6.1 Overall file layout

```
FILE_HEADER
  repeat:
    STROKE_HEADER
    POINT      (first point of the stroke, absolute)
    DELTA ...  (any number, mixed with further POINT / LOST_POINT packets)
    STROKE_END
  [EOF marker]   (optional; a file may simply end after the last STROKE_END)
```

File header variants (`StrokeFileHeader`, identified by the first 4 bytes read as a u32 LE):

| Magic bytes | Device | Total header size | Fields |
|---|---|---|---|
| `62 38 62 74` (ASCII `b8bt`) | Spark and **Slate** | **4 bytes** | magic only. No timestamp, no stroke count. |
| `67 82 69 65` | Intuos Pro | 16 bytes | `[4..8)` u32 LE unix time (end of drawing, i.e. when the button was pressed), `[8..10)` ms (always 0), `[10..14)` u32 LE stroke count (only roughly reliable), `[14..16)` zero |

Any other magic raises `StrokeParsingError('Unknown file format')`. For the Slate, take the timestamp from the `cf` file-info reply (section 5).

### 6.2 Packet framing inside the file: the bitmask header byte

Every packet begins with a **header byte `H`**. `n = popcount(H)` is the number of payload bytes that follow, so the packet size is `1 + n`.
`StrokeDataType.identify` classifies the next packet (order matters):

1. File header magic (see above).
2. `fc ff ff ff ff ff ff` -> **STROKE_END** (7 bytes: header `fc` plus six `ff`). "Can sometimes mean end of file too."
3. Payload (`n` bytes) is all `ff` and `n == 8` (i.e. `ff` x 9) -> **EOF** (9 bytes). Parsing stops.
4. `H & 0b11 == 0` -> **DELTA** (a coordinate packet, see 6.4). "All special headers have the lowest two bits set."
5. no payload -> UNKNOWN.
6. `payload[0] == fa` or `payload[0:3] == ff ee ee` -> **STROKE_HEADER** (`fa` = Intuos Pro style, `ff ee ee` = Slate style).
7. `payload[0:2] == ff ff` -> **POINT**.
8. `payload[0:2] == dd dd` -> **LOST_POINT**.
9. anything else -> UNKNOWN (skipped using the same `1 + popcount(H)` size; the parser logs and continues).

`H` is nominally `ff` for the special packets, but the real device sometimes sends other values (STROKE_END can be `fc`; POINT can be `bf`; Intuos stroke headers can be
`bf`/`ff`), hence everything is driven by `popcount`.

### 6.3 Special packets

**STROKE_HEADER, Slate style** (`StrokeHeader._parse_slate`): `H ff ee ee ee tt tt 00 00` (the 4th byte after `H` is shown as `ee` following the issue #111 note; the code does not check it).
The code comment says: "On the first stroke after the file header, this packet is 6 bytes only. Other strokes have 8 bytes but the last two bytes are always zero."
Whether those counts include `H` is unclear; the parser simply uses size = `1 + popcount(H)`.

- `payload[4:6]` (u16 LE) times 5 ms = a **time offset in milliseconds since device power-up** (comment says "since powerup", 5 ms resolution) -> `time_offset`.
  This is a relative stamp, there is no absolute time per stroke on a Slate. Tuhi ignores it. A u16 of 5 ms ticks wraps at 327.68 s (inferred), so it may only be a coarse ordering aid.
- Pen id: 0, pen type: 0, new layer: false (the Slate has none of these).
- Issue #111 (open design note): "WacomPacketHandlerStrokePrefixSlate seems always starts with ?f,ff,ee,ee,ee"; the trailing 4 bytes may act as an implied previous Y/pressure
  for a first delta that has no absolute coordinate. Tuhi's answer: default the previous point to (0, 0, 0).

**STROKE_HEADER, Intuos Pro style** (`StrokeHeader._parse_intuos_pro`): `H fa FL t0 t1 t2 t3 m0 m1` where

- `FL` (flags): bit `0x80` = a pen-id packet follows, bit `0x40` = new layer, low 6 bits (`0x3f`) = pen type.
- `t0..t3` u32 LE absolute unix seconds for this stroke. `m0 m1` = sub-second, probably milliseconds (the fixture values 302, 424, 109, 528 are all < 1000)
  (inferred; Tuhi code has `FIXME: plus two bytes for milis`, and a maintainer says the spec calls them 100 us units).
- If the pen-id flag is set, a second packet `ff` + 8 bytes (u64 LE pen serial) follows immediately. Its header byte **must be `ff`**; otherwise `StrokeParsingError`
  (this is the open issue #256 on a Folio/Intuos).
- Example: `ff fa c3 1f 73 53 5d 2e 01` then `ff 0a 87 75 80 28 42 00 10` -> new layer, pen type 3, time 1565750047, pen id `0x100042288075870a` (test_parse_stroke_header).

**POINT** (`StrokePoint`): `H ff ff <body>`. It is a DELTA packet with the two bytes `ff ff` inserted after the header byte. Parse by clearing the low two bits of `H`
and decoding `H' <body>` as a DELTA (6.4). With `H = ff`, body = x u16, y u16, pressure u16 (9 bytes total). With `H = bf` the pressure is an 8-bit delta instead
(8 bytes total), see test `0xbf, 0xff, 0xff, 1, 2, 3, 4, 5, 6`.

**LOST_POINT** (`StrokeLostPoint`): `H dd dd LL LL ...` (nominally `ff dd dd LL LL ...`, total size `1 + popcount(H)`). `LL LL` is `nlost`, u16 LE, the number of samples the firmware could not record. Tuhi does not interpolate ("We don't yet handle lost points").

**STROKE_END**: `fc ff ff ff ff ff ff` (or any `H` with all payload bytes `ff`). Closes the current stroke.

**EOF**: `ff` x 9. Closes the current stroke and ends the file.

### 6.4 Coordinate packets (DELTA)

`H` has its two low bits 00. The remaining 6 bits are three 2-bit masks:

```
bits 7..6 : pressure mask
bits 5..4 : y mask
bits 3..2 : x mask
bits 1..0 : 00
```

Mask meaning per axis (`StrokeDelta.extract`):

| mask | meaning | extra bytes |
|---|---|---|
| `00` | axis not present | 0 |
| `01` | not implemented ("device not supposed to exist"; raises) | n/a |
| `10` | signed 8-bit delta (a delta of 0 is invalid) | 1 |
| `11` | absolute value, u16 LE | 2 |

Bytes follow in the order x, y, pressure, each only if its mask is non-zero. Packet size = 1 + sum of extra bytes.
Examples from `test_parse_delta`: `c8 01 cb 04` (x delta +1, pressure absolute 0x04cb), `e8 ff 01 0f 06` (x delta -1, y delta +1, pressure absolute 0x060f),
`a8 03 ff 75` (x delta +3, y delta -1, pressure delta +0x75).

(`popcount(H)` for DELTA packets is *not* the packet length. The length-by-popcount rule applies to the special packets; DELTA length comes from the masks.
`StrokePacketUnknown` falls back to popcount only for unrecognised headers.)

### 6.5 Reconstruction algorithm: cumulative (second-order) delta

From `StrokeFile._parse_data`; the code comment gives the model

```
P0 = absolute
P1 = P0 + d1
P2 = P0 + 2*d1 + d2
P3 = P0 + 3*d1 + 2*d2 + d3
```

State per file: `last_point = (x, y, p)` (initially 0,0,0, **not** reset at stroke boundaries) and `last_delta = (dx, dy, dp)` (reset to 0 at every STROKE_HEADER).
For each POINT or DELTA packet, per axis independently:

1. if the packet has a delta for the axis: `d += delta`; else if it has an absolute value: `value = absolute; d = 0`; else (axis absent): `value` and `d` unchanged.
2. `last_delta = d`; `current = value`; `new_point = current + d`; `last_point = new_point`; emit it.

Consequences: a delta is added to a running velocity, and an absent axis keeps moving with the previous velocity; an absolute value cancels the velocity.
Pressure is treated the same way as x and y. Each emitted `(x, y, p)` is one pen sample, **one packet = one sample**; the samples are at a fixed rate with no per-point timestamp.
Sampling interval is 5 ms (a Tuhi user measured it, issue #231; consistent with the live-mode `+= 5` ms).

Per-point time (inferred, from #231): `t_point = stroke_timestamp + index * 5 ms` (Intuos Pro, absolute stroke time) or `power_up_offset + index * 5 ms` (Slate, relative stroke time).

### 6.6 Worked example (real device data from the repo, Intuos Pro format)

First stroke of `test/test_strokes.py :: TestStrokes.test_single_stroke`. I decoded it with an independent re-implementation of the rules above, and the
three fixture blobs in that file all parse to exactly the end of data with 2, 2 and 5 strokes (matching the stroke count in each file header).

```
67 82 69 65  22 73 53 5d  00 00  02 00 00 00  00 00      FILE_HEADER: magic, time 0x5d537322 = 1565750050 (2019-08-14 02:34:10 UTC),
                                                           ms=0, 2 strokes, 2 zero bytes (16 bytes)
ff fa c3 1f 73 53 5d 2e 01                               STROKE_HEADER: flags c3 = pen-id(80) | new-layer(40) | pen type 3,
                                                           time 0x5d53731f = 1565750047, sub-second bytes 2e 01 (=302, ms?)
ff 0a 87 75 80 28 42 00 10                               pen id packet: 0x100042288075870a
ff ff ff 41 3c 13 30 72 03                               POINT  : x=0x3c41=15425, y=0x3013=12307, p=0x0372=882       -> (15425, 12307, 882)
c8 01 cb 04                                              DELTA  : mask x=10 (+1), y absent, p=11 abs 0x04cb=1227      -> (15426, 12307, 1227)
e8 ff 01 0f 06                                           DELTA  : x -1 (cum. dx 0), y +1 (dy 1), p abs 0x060f=1551    -> (15426, 12308, 1551)
e0 ff 5f 07                                              DELTA  : y -1 (dy 0), p abs 0x075f=1887                      -> (15426, 12308, 1887)
e0 ff 91 08                                              DELTA  : y -1 (dy -1), p abs 0x0891=2193                     -> (15426, 12307, 2193)
c0 78 09                                                 DELTA  : x,y absent (y keeps dy=-1), p abs 0x0978=2424       -> (15426, 12306, 2424)
e8 02 01 2f 0a                                           DELTA  : dx +2, dy +1 (dy 0), p abs 0x0a2f=2607              -> (15428, 12306, 2607)
...                                                      (36 points in this stroke)
ff fa 03 21 73 53 5d 46 02                               2nd STROKE_HEADER: flags 03 (no new layer, no pen id), time 1565750049, 46 02 (=582)
ff ff ff d3 6f 5a 38 c0 03 ...                           POINT: x=0x6fd3 ... (90 points)
...
fc ff ff ff ff ff ff                                     STROKE_END (also the last 7 bytes of the file)
```

Result of decoding all of blob 0: 2 strokes with 36 and 90 points, x range 15398..28627, y range 12306..20549, pressure 404..4001.

There is **no Slate-format fixture in the repo** (no `ff ee ee` packets, no `62 38 62 74` file). The only real Slate/Spark bytes are in issues:
issue #111 shows the beginning of a Spark file `62 38 62 74 bf ff ff 4a 14 29 31 6a` (magic; `bf ff ff` = POINT with 8-bit pressure delta; x=0x144a, y=0x3129, dp=0x6a). The port must capture its own Slate sample.

### 6.7 From device units to drawings and orientation

`WacomProtocolBase.parse_pen_data`: for each point, `position = (x * point_size_um, y * point_size_um)` (micrometres), `pressure = 65536 * p / pressure_max`
(normalised 0..65535). `Drawing(name, (width_um, height_um), timestamp)`. JSON format `Drawing.to_json` (version 1: `devicename, sessionid, dimensions, timestamp, strokes[points[position, pressure]]`).

**The device sends no orientation/rotation information.** The coordinate frame is the sensor's own frame (x along the width reported by `ea 03`, y along the height).
Orientation is a user/export setting in Tuhi: model defaults `orientation = 'portrait'` (Spark and Slate) and `'landscape'` (Intuos Pro) in `wacom.py`,
`raw-log-converter.py` uses it; `export.py :: ImageExportBase.output_strokes` rotates after the fact:

- `landscape`: output (x, y) unchanged, output size (W, H).
- `reverse-landscape`: `x' = W - x`, `y' = H - y`.
- `portrait` / `reverse-portrait`: output size is swapped (H, W); `portrait`: `x' = Wout - y`, `y' = x`; `reverse-portrait`: `x' = y`, `y' = Hout - x`
  (with `Wout, Hout` the already swapped output width and height).

Whether a Slate drawn in portrait ends up the right way up therefore depends on this default (`portrait`), not on data from the device (inferred: the Slate is used with the long edge vertical in A4 portrait).
Pen type, pen id, and layer flags exist only in the Intuos Pro stroke header; the Slate has pressure only (no tilt, no hover, no button state in offline strokes;
a maintainer says "we only get x/y and pressure from the device", issue #231).

---

## 7. Live mode

Sources: `tuhi/wacom.py :: WacomProtocolBase.start_live / stop_live / _on_pen_data_changed / live_mode`, `WacomProtocolSlate.live_mode`,
`wacom_live_rdesc_template`; `tuhi/protocol.py :: Mode, MsgSetMode`; `tools/tuhi-live.py`; wiki Live-Mode; `tuhi/uhid.py` (Linux-only HID injection).

### 7.1 Enter / exit

- `Mode` enum: `LIVE = 0x00`, `PAPER = 0x01`, `IDLE = 0x02` (docstring: LIVE reports pen strokes immediately, IDLE is live mode without reporting, PAPER is the normal mode where drawings are downloaded).
- Command: `b1 01 <mode>` -> `b3 01 00`.
- Start (Slate), `WacomProtocolSlate.live_mode(True)`: connect; **first** read `ea 03`/`ea 04` (the model can be A4 or A5; computes the logical range, section 4.5); then
  `e6 06 <uuid6>` (auth); then `b1 01 00` (LIVE). Live pen notifications then start flowing on `00001524-...`. The user must have the device connected (button press) for this to work (tuhi-live prints
  "please press button on the device").
- Stop: `b1 01 02` (IDLE) on the same connection (`stop_live`; no re-authentication in that path). Afterwards the next normal sync sends PAPER mode (`b1 01 01`) again.
- Not implemented for the Spark (issue #254, maintainer reply).
- The device disconnects itself after idle time even in live mode (issue #97, open). A port has to reconnect/re-auth and re-send LIVE.
- Starting live mode while the device is already connected/busy can yield `b3 01 02` (INVALID_STATE) replies to `ea` (issues #124, #202).

### 7.2 Real-time packet format (notifications on `00001524-1212-efde-1523-785feabcd123`)

`_on_pen_data_changed` dispatches on `value[0]`:

| `value[0]` | Meaning (Tuhi) | Layout |
|---|---|---|
| `a1` | pen data | `a1 LL <records>`; `LL` must be a multiple of 6 (else logged as "wrong data" and dropped); each record 6 bytes: `x u16 LE`, `y u16 LE`, `pressure u16 LE`. A record of six `ff` bytes (`ff ff ff ff ff ff`) means **pen left proximity**. Each record advances the internal clock by 5 ms. |
| `a2` | pen entered proximity | `a2 LL xx xx T0 T1 ..`: `value[4:]` is read as an LE integer and multiplied by 5 to give a millisecond timestamp (so ticks are 5 ms since some epoch, probably power-up, inferred). `value[2:4]` is not interpreted (unknown). |
| `10` | status / hover report (inferred) | `pressure = u16 LE at value[2:4]`, `buttons = value[10]`. Tuhi only logs these; it does not use them for input. |

- Proximity, touch and button: proximity in = `a2`, proximity out = the all-`ff` record in `a1`. There is no explicit tip-switch bit. Touch is inferred from pressure > 0 (inferred).
  The stylus side button appears only as `value[10]` of the `10` report and Tuhi does not forward it. Hover and tilt are not carried.
- Tuhi turns each record into a HID input report for a virtual tablet: report id 1, one byte with bit 0 = "in range", then x, y, pressure as three u16 LE
  (`[1, 1, x_lo, x_hi, y_lo, y_hi, p_lo, p_hi]`); leaving proximity is `[1, 0, 0, 0, 0, 0, 0, 0]`. Report descriptor template in `wacom_live_rdesc_template`
  (Digitizer, Stylus, In Range, X, Y with unit exponent -3 centimetre (i.e. 10 um units), Tip Pressure). The USB ids used for the virtual device are bus 5 (Bluetooth), vendor `0x056a`, product `0x0001`. (`tuhi/uhid.py` and `/dev/uhid` are Linux-only.
  On Windows the equivalent has to be done with a different mechanism, e.g. pointer/pen injection or a virtual HID driver: out of scope here.)
- Live range for the HID descriptor on the Slate: x from 2500 to (width - 1000), y from 500 to (height - 500), pressure 0..2047.
- Sample rate: 5 ms per sample (200 Hz), from the `+= 5` timestamp increments (`_timestamp`).

### 7.3 Are pages stored on the device during live mode?

Not answered by Tuhi's code or wiki. Tuhi never queries the file count while live and the modes are distinct (LIVE vs PAPER), so the natural reading is that live mode streams without
storing (inferred), but this is **unverified**. Test on the real device: count (`c1`) before and after a live session.

Our captures (Slate A4, 2026-11-02) partly answer: in one session the count read 4 right before and right after about 9 s of drawing in live mode
(no file appeared at once). About 30 s after the 4 pages had been downloaded and deleted, the count read 1: a page was finished meanwhile, but whether
it held the live strokes (stored when the button was pressed?) is not known. To check at H2: draw in live mode, leave live mode, press the button, count.

---

## 8. Known quirks, timeouts, retries, firmware differences, workarounds

Timeouts and retries (all from `tuhi/wacom.py` and `tuhi/protocol.py`):

- Reply wait 5 s; button-press wait 10 s; end-of-read wait 5 s with retry-while-data-flows. There are no other retries. Any exception ends the session and disconnects.
- Registration: wiki and issue #195/#257: re-registering is flaky, because the device may stay in 4-byte manufacturer-data mode for the next connection, or the host library caches
  stale GATT state. Fix used by users: delete the host-side device cache/config and re-register.
- Only one host identity at a time: use with Inkspace overwrites the UUID, so Tuhi then gets `b3 01 07` (issues #212, #257, #266; FAQ).
- Time: device clock assumed UTC; Tuhi sets it on every sync.
- Slate Small vs Large: Tuhi originally hard-coded the A5 size and mis-handled A4 live mode (issue #118, fixed by reading `ea 03`/`ea 04` first). Device names do not identify the size: an A5 unit reports the name `Bamboo Slate` (issue #266), the A4 unit also `Bamboo Slate` (issue #117), a third unit `Bamboo Slate Small`. Always query `ea 03`/`ea 04`.
- The CRC is computed on the exact concatenation of all `ffee0003` payloads for the file, including the file header and the final EOF/STROKE_END packet.
- Stroke parser tolerance: header byte values other than `ff` for POINT/STROKE_END; DELTA with a zero delta is a hard error; unknown packets are skipped by popcount;
  a "lost point" packet is parsed but not interpolated; point counts in the Intuos file header are unreliable.
- Spark first point may be a delta (no absolute): default previous point to (0, 0, 0) (issue #111).
- Unsolicited/other frames: Slate sends sysevent notifications (`f1 02 13 00` on connect: a battery level, section 12) and a stray extra `bc` frame was seen in a Slate Small trace; the Nordic buffer logic must tolerate them. Our Slate A4 also sent `ef 01 00`, `a3 …` and `cb 01 00` (section 12).
- Name reading quirk (4.1), firmware decoding quirk (4.2), double `b1 01 01` (5.1).
- `UNKNOWN_E3` (`e3 01 00`) is sent only by the Spark/base `retrieve_data` and `check`; the Slate path skips it.
- Spark specifics (not needed for Slate): only connects when drawings are pending, device disconnects after about 10 s, "A successful communication with the controller deletes the drawings" (README).
- Do not send anything to the DFU service `00001530-...` and be conservative with unknown opcodes: Tuhi's own comment on why it filters by company id is "to avoid potentially bricking a device" (`base.py`).
- Tuhi stores a raw YAML log of every session precisely because downloads are destructive (wiki Log-files). A port should write the raw bytes of each file to disk **before** sending `ca`.
- Hardware access/permissions: BlueZ-specific code in `tuhi/ble.py` (D-Bus Device1/GattCharacteristic1) has nothing to reuse on Windows. Only the GATT UUIDs and the byte-level protocol matter.

---

## 9. Test fixtures useful for a device simulator

Everything in the repo (there are no binary fixtures, recordings or sample drawings checked in):

| Path | Content / format | Reuse |
|---|---|---|
| `test/test_messages.py` | pytest. Classes `TestProtocolAny`, `TestProtocolSpark`, `TestProtocolSlate`, `TestProtocolIntuosPro`: each test defines a mock device callback that **asserts the exact request bytes and returns the exact reply frame** for every interaction (connect, name, time, firmware, battery, width/height, point size, mode, strokes info, files count, download, delete, register, error `b3 01 01`). | Best source for a request-to-reply table for a Slate simulator. Use the `TestProtocolSlate` overrides. Note the synthetic firmware reply there is not realistic (see 4.2). |
| `test/test_strokes.py` | `TestStrokeParsers`: byte-level unit tests of every packet type (file header, stroke header with/without pen id, point `ff`/`bf`, lost point, EOF, delta masks). `TestStrokes`: three embedded hex dumps `test_single_stroke` (505 bytes, 2 strokes), `test_double_stroke` (345 bytes, 2 strokes), `test_quint_stroke` (981 bytes, 5 strokes) in Intuos Pro format. | Ground truth for the decoder. The three blobs are the only real stroke data in the repo (all Intuos Pro, none Slate). I extracted them to a scratch file for my own verification. |
| `test/test_strokes.py :: test_log_files` | Parametrised over `$XDG_DATA_HOME/tuhi/**/raw/log-*.yaml` on the *user's* machine. Not shipped; skipped when absent. | Defines the raw log format below. |
| Raw log YAML (written by `wacom.py :: DataLogger`, read by `tools/raw-log-converter.py`, `test_strokes.py`) | Header keys `sessionid`, `name`, `bluetooth`, `time`, then `data:` = list of items. Command items: `- send: [0xb9, 0x01, 0x00]` followed by `recv: [0xba, 0x02, 0x44, 0x00]`. Pen items: `- recv: [0x62, ...]` with `source: PEN`. Sysevent items have `source: SYSEVENT`. Comment lines `#         GET_BATTERY` name each request. Bytes are written as `0x..` lists. | Convenient format for recording sessions from the real device and replaying them in the simulator. |
| `tools/raw-log-converter.py` | Converts a raw log's PEN bytes to SVG/PNG/JSON using the tablet defaults (`--tablet-model intuos-pro|slate|spark`). | Reference for how point units map to output. |
| `tools/parse_log.py` | Python 2 tool that parses an Android `btsnoop_hci.log` (BLE ATT traffic) into rows with the characteristic names, for capturing the *official* Wacom Inkspace app protocol. Lists all UUIDs in section 1. | Instructions for capturing Inkspace traffic (Android "Bluetooth HCI snoop log"). |
| `tools/kete.py` | Interactive D-Bus client for debugging the Tuhi daemon. | Not protocol relevant. |
| `tools/tuhi-live.py` | Launcher for live mode (needs root, `/dev/uhid`, Linux). | Flow reference only. |
| `example.ini` | Example `settings.ini`. | Config format. |

Real device wire traces useful as simulator data (all from GitHub issues, not the repo): #117 (Slate Small registration), #266 (Slate dimensions and auth failure), #212
(Slate auth failure, GATT services), #256 (Intuos Pro full sync including `cc`/`c3`/`c8`), #115/#118 (Slate A4 dimensions and firmware strings).

---

## 10. License

- Main license: **GNU General Public License, version 2 or (at your option) any later version, i.e. GPL-2.0-or-later (SPDX `GPL-2.0-or-later`)**.
  Evidence: every `.py` source file header says "either version 2 of the License, or (at your option) any later version" (checked on `tuhi/*.py`, `tuhi/gui/*.py`,
  `tools/*.py`, `test/*.py`, `tuhi-server.py`); `README.md` says "Tuhi is licensed under the GPLv2 or later"; `data/org.freedesktop.Tuhi.appdata.xml.in.in`
  has `<project_license>GPL-2.0+</project_license>`; `COPYING` is the GPL v2 text (June 1991). Caveat: `meson.build` says only `license: 'GPLv2'` and the
  About dialog says `gpl-2-0`, which is less precise than the file headers; the headers and README control ("or later").
- Copyright holders: "Copyright (c) 2019 Red Hat, Inc." appears in `test/test_messages.py`, `test/test_strokes.py`, `tools/raw-log-converter.py`; the About dialog says
  "Copyright (C) 2020 Tuhi Developers"; most other files carry only the licence boilerplate with no named holder.
- Separately licensed files: **none found.** All other files either have the same GPL header or no header at all (build files, `.ui` files, desktop/appdata, SVG logos, translations
  `po/*.po`, `DBusInterface.md`, `example.ini`). The appdata file's *metadata* license is `FSFAP` (`<metadata_license>FSFAP</metadata_license>`).
  I found no Creative Commons or other licence statements in the logo SVGs (inferred: they are covered by the project licence). The wiki repository has no licence file that I read.
- Practical note for the port (not legal advice): the licence covers Tuhi's code. Protocol facts (UUIDs, opcodes, byte layouts) written in this document are descriptions, but
  copying or closely translating Tuhi's Python (e.g. porting `protocol.py` line by line to Rust) would create a derivative work that has to be released under GPL-2.0-or-later. A clean-room
  implementation from this document and from your own captures avoids that. Decide this before committing to the architecture.

---

## 11. Open questions and things to verify on the real Slate Large

1. Exact characteristic properties (write vs write-without-response) of `6e400002-...`, CCCD requirements, and the usable ATT MTU. Dump with a GATT explorer.
2. Confirm the Slate Large's `ea 03`/`ea 04` replies (expected width 29700 = `04 74 00 00` and height 21600 = `60 54 00 00`) and its name string and firmware string format.
3. Whether the Slate `c8 be` start reply carries the size and timestamp like the Intuos Pro (`c8 0b be ...`), and what exactly the `cf` "count" field is (bytes vs something else).
4. Real Slate stroke data: confirm `62 38 62 74` magic, `ff ee ee` stroke header layout, whether pressure max is really 2047 on the Large, how the time offset behaves, and whether `dd dd` lost-point packets occur.
5. The meaning of the sysevent characteristic and of the `ec 06 06 00 00 00 00 00` argument bytes. Partly answered (section 12): `f1` is the battery
   level; `ef 01 00` is still unknown. When `ec` is accepted: section 5.3.
6. Whether live mode stores pages on the device (section 7.3).
7. The `10 ..` live report (button state) and `a2` bytes 2..3.
8. Timing: how soon after connect the first command must be sent, and the exact idle disconnect time of the Slate.
9. Windows specifics (inferred, not from Tuhi): bonding is not needed according to Tuhi, but Windows may cache an old GATT database after firmware changes or after another app re-registered the device.

---

## 12. Frames seen from our Slate A4 (2026-11-02)

Source: Ardoise's diagnostic journals of the user's Bamboo Slate A4 (section 5.3). Ardoise decodes the battery level and names the others
"meaning unknown (ignored)" (`ardoise_proto::sysevent`, `live::LivePacket::Unexplained`, `Reply::is_unexplained`), so that they no longer show as
unexpected in the logs.

| Characteristic | Frame | When | Reading | How sure |
|---|---|---|---|---|
| sysevent | `f1 02 <pp> 00` | Unasked, at any moment of a connection: `2f`, `2e`, `2d` about 55 s apart during a long session (the third in live mode); once 30 ms after a `ba` reply | **Battery level changed**, `pp` = percent. The values follow the `ba` readings: 47, 46, 45 % after a reading of 48 %; 41 % right after a reading of 42 %, and the next session read 41 %. Issue #212's `f1 02 13 00` on connect fits (19 %). The last byte was always `00` while discharging: probably the charging state, like `ba` (supposed, not used) | High for `pp`; low for the last byte |
| sysevent | `ef 01 00` | Once, right after `e7` (registration request), before the button press | Unknown | — |
| live `00001524` | `a3 06 dd dd dd dd 01 00` | Once, in the same notification batch as an `a2` proximity-in, before the samples of a stroke | Unknown. The `dd dd` bytes recall the stroke-file lost-point packet (`dd dd` + u16 count), so perhaps "1 sample lost" (supposed) | Low |
| Nordic RX (command) | `cb 01 00` | Once, unasked, during live mode (about 2.4 s after the pen came near) | Unknown (a button press? a page event?) | — |

Also seen: the `a2` proximity-in carries `ee ee` in bytes 2..3 (`a2 06 ee ee 00 02 00 00`), the same marker as the Slate stroke header `ff ee ee`.
