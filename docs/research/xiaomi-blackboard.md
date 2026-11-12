# Xiaomi Mijia LCD blackboard, storage edition: Bluetooth study

Feasibility study, started 2026-11-12 at the user's request: can Ardoise fetch the pages stored in this second
tablet and receive its pen strokes live, as it does with the Bamboo Slate? Nothing in the application depends on
this document yet. SPEC.md covers the Bamboo Slate only (see `docs/questions.md`, Q14).

State on 2026-11-12: the tablet's own commands, its live pen samples and its stored pages are decoded
(section 3). The tablet obeys a client that skips the Xiaomi login: **the live mode works from the PC**. The
download of stored pages is known from a capture of Mi Home and has not been replayed from the PC yet
(section 5).

Conventions: hex bytes are written `12 01 09`. "(measured)" was observed on the user's own tablet with
`cargo run -p ardoise-ble --example survey`. "(reported)" comes from a public source and was not verified by us.
The raw captures hold the tablet's Bluetooth address and stay in `testdata/private/xiaomi/` (never in git); the
address is left out of this document on purpose.

## 1. The device

- 米家液晶小黑板 存储版 13.5", model XMXHB05JQD, sold in China from December 2021: electromagnetic pen with 2048
  pressure levels, about 121 MB of memory (some 400 pages), Bluetooth 5.0, a USB-C port described for charging
  only (reported: [IT之家](https://www.ithome.com/0/595/259.htm)).
- In the Mi Home (米家) application it is the model `jiqid.sxb.v1`, product number 4056, made by 机器岛 (Jiqid);
  no MIoT specification is published for it (reported:
  [home.miot-spec.com](https://home.miot-spec.com/s/jiqid.sxb.v1); the `/spec/jiqid.sxb.v1` page answers "Not
  Found").
- Mi Home offers: backup of the stored pages to the Xiaomi cloud, a real-time mirror of the writing, replay of the
  strokes, export as image or video (reported). So the tablet does send strokes, with their order, to the phone.
- The plain 13.5" LCD tablets (XMXHB02WC, BHR4245GL, the Color Edition) have no memory and no data link: they are
  out of scope.
- No public description of its protocol and no open-source client were found (searches in English and Chinese,
  2026-11-12). Nothing plays the part Tuhi plays for the Slate.

## 2. What the tablet shows over Bluetooth (measured, 2026-11-12)

### Advertisement

- Name `xiaomi.dev.ble`.
- Service data of the 16-bit service `fe95` (a Xiaomi "MiBeacon"): `10 59 d8 0f 00` followed by the six bytes of
  the address, reversed. Read with the layout of the open-source parsers
  ([xiaomi-ble](https://github.com/Bluetooth-Devices/xiaomi-ble)): frame control `0x5910` = MiBeacon version 5,
  authentication mode 2 ("standard"), registered (bound to an account), address included, no capability byte, no
  object, not encrypted; product identifier `0x0fd8` = 4056; frame counter 0.
- The user's tablet is bound to their Mi Home account, on an Android phone.

### GATT table

| Service | Characteristic | Properties | Value read |
|---|---|---|---|
| `1800` | `2a00` | read, write | `xiaomi.dev.ble` |
| `1800` | `2a01` | read | `00 00` |
| `1800` | `2a04` | read | `0c 00 18 00 00 00 5a 00` |
| `1800` | `2aa6` | read | `01` |
| `1801` | `2a05` | indicate | (Windows refuses the subscription) |
| `fe95` | `0004` | read | `2.0.2_0016` (padded with zeros to 20 bytes) |
| `fe95` | `0005` | read, notify | 4 bytes |
| `fe95` | `0010` | write without response, notify | |
| `fe95` | `0017` | write, notify | |
| `fe95` | `0018`, `0019`, `001a`, `001b`, `001c` | write without response, notify | |
| `6e400001-b5a3-f393-e0a9-e50e24dcca9e` | `6e400002-…` | write | |
| `6e400001-…` | `6e400003-…` | notify | |
| `6e400001-…` | `6e400004-…` | notify | |

The last service has the UUIDs of the Nordic UART service (the same base as the Slate's command channel), with a
third characteristic, `6e400004`, that the Nordic service does not have.

### Behaviour without any login

- Windows connects without pairing. With every notification enabled and nothing written, the tablet kept the
  connection open for 80 s (it does not drop a client that has not logged in).
- While the user wrote on the tablet and pressed its buttons, four notifications arrived on `6e400003`, all
  `12 01 09` (the "page stored" event of section 3).
- Nothing arrived on `6e400004` nor on the `fe95` characteristics: pen strokes are not sent unasked.

## 3. Two sessions of Mi Home (measured, captures of 2026-11-12)

From Bluetooth HCI logs of the user's Android phone (section 6). In the first, Mi Home connected to the tablet
and mirrored the writing, and the user stored the page with the tablet's button. In the second, Mi Home imported
the nine pages stored in the tablet. The tablet asks for an ATT MTU of 247.

### Sequence

1. Service discovery, then on `fe95`: notifications of `001c` enabled and five one-byte queries on it (`00`,
   `02` to `05`), answered with short texts, among them `nRF52832.DK.1` (the tablet is built on a Nordic
   nRF52832); then `a4` on `0010` and an exchange of filler frames on `0019`.
2. The Xiaomi login: `24 00 00 00` on `0010`, random numbers and proofs exchanged in fragments on `0019`, and
   the tablet's `21 00 00 00` on `0010` (success). It matches the "standard" login of section 4.
3. Notifications of `6e400004` and `6e400003` enabled, then **everything else in clear** on the Nordic-UART
   service: commands written (with response) to `6e400002`, replies and events on `6e400003`, pen samples on
   `6e400004`. Nothing more on `fe95`.

### Frames of the vendor channel

A frame is `group, length, code, payload`: `length` counts the code and the payload. Integers in frames are
**big-endian**.

| Direction | Frame | Meaning |
|---|---|---|
| host to tablet | `11 01 01` | Read the battery. Reply `11 02 01 64`: 100 (per cent, inferred). |
| host to tablet | `11 01 02` | Read the memory. Reply `11 09 02` + total bytes (u32) + free bytes (u32): 126 909 952 (the advertised 121 MB) and 126 794 240. |
| host to tablet | `11 05 07` + u32 | Set the clock: Unix seconds (the value was the time of the capture, UTC). No reply. |
| host to tablet | `12 01 04` | Count the stored pages. Reply `12 07 04` + count (u16) + bytes (u32): 8 pages and 111 608 bytes, then 9 pages and 118 111 bytes after the user stored one. |
| host to tablet | `12 01 05` | Describe the oldest stored page. Reply `12 09 05` + the page's time (u32, Unix seconds: when it was stored, by the tablet's clock) + its size in bytes (u32). The time identifies the page in the next two commands. |
| host to tablet | `12 06 03 01` + time (u32) | Download that page. No reply on `6e400003`; its bytes follow on `6e400004`. The meaning of the `01` is not known. |
| host to tablet | `12 05 06` + time (u32) | Delete that page. No reply. The next `12 01 05` describes the following page. |
| host to tablet | `12 02 08 01` | Start the live stream. No reply; samples follow on `6e400004`. |
| host to tablet | `12 02 08 02` | Stop the live stream. No reply. |
| tablet to host | `12 01 09` | Event: a page was stored with the tablet's button (inferred: it came when the user pressed it, and the count went from 8 to 9). Also sent to a client that has not logged in. |

In the first session Mi Home sent, in order: `11 01 01`, `11 01 02`, `11 05 07 …`, `12 01 04`,
`12 02 08 01`, `12 01 05`, then, when the user left the mirror, `12 02 08 02` and `12 01 04`.

### Import of the stored pages (second session)

After `11 01 01`, `11 01 02`, `11 05 07 …` and `12 01 04` (9 pages, 144 123 bytes), Mi Home repeated nine times:
`12 01 05` (time and size of the oldest page), `12 06 03 01 <time>` (download), then, once all the bytes had
arrived, `12 05 06 <time>` (delete). It did not ask for a tenth page: what `12 01 05` answers when the tablet is
empty is not known.

- The bytes of a page arrive on `6e400004` as a plain stream cut into notifications of at most 244 bytes; the
  host counts them against the size announced. No header, no end mark, no checksum. A page of 52 185 bytes took
  2.1 s; the nine pages took 13 s in all.
- **A stored page is the same 7-byte records as the live stream**, in the order they were written, hovering
  records included: every size is a multiple of 7. Checked on the test page: it contains, byte for byte, the 886
  records Mi Home received live in the first session and the 3 176 records the PC received live afterwards.
- A page has no times inside either: only the moment it was stored. The oldest pages were dated June 2025.
- Storing again without erasing the screen replaces the stored page by a longer one (the count stayed at 9, the
  size went from 6 503 to 32 515 bytes, with a new time): a stored page is the whole content of the screen.
- **Mi Home deletes each page from the tablet once it has it.** The pages go to whichever application imports
  them first.

### Live samples (`6e400004`)

- A notification holds one to six records of 7 bytes: `status, x (u16), y (u16), pressure (u16)`, here
  **little-endian**.
- `status`: `a1` the pen touches (pressure 1 to 2047), `a0` the pen hovers (pressure 0), `c0` the last record
  before the pen leaves the tablet's range (pressure 0). A stroke is a run of `a1` records.
- Coordinates: the diagonal the user drew from the top left corner to the bottom right one runs from (136, 349)
  to (20350, 27024); all samples lie within 117..20401 by 250..27067. So the origin is the top left corner of the
  tablet held upright, x to the right, y downwards, over about 20 400 by 27 100 units. With a 13.5" panel of
  3:4 (about 206 by 274 mm) one unit is close to 0.01 mm, the Slate's unit (inferred; to be calibrated with a
  ruler).
- No time in a record: about 120 records per second by their arrival (886 records in the capture, 7 strokes).

### The same commands from the PC, without the Xiaomi login (measured, 2026-11-12)

With `survey -- send`, on a fresh connection where only the notifications of `6e400003` and `6e400004` are
enabled and nothing is written to `fe95`:

- `11 01 01`, `11 01 02`, `12 01 04` and `12 01 05` are answered at once, in the same form as to Mi Home
  (battery `5a`, 90; 9 pages; the same page description).
- `12 02 08 01` starts the live stream: 3 176 records in 75 s while the user wrote, which redraw the handwriting;
  `12 02 08 02` stops it. With nobody writing, nothing is sent (40 s without a record).
- The tablet stays bound to Mi Home: nothing was registered, nothing changed on the Xiaomi side.

So the Xiaomi layer can be ignored altogether for these commands.

### Not seen yet

What the tablet's erase button sends (if anything), what `12 01 05` answers on an empty tablet, whether a
download can be interrupted, and the codes of groups `11` and `12` that Mi Home did not use.

## 4. The Xiaomi layer (`fe95`), from public sources (reported)

- Mi Home devices authenticate the application on the `fe95` service. Mode 2 ("standard") is an ECDH P-256
  registration that creates a long-term token, then, at each connection, a login that derives session keys
  (HKDF-SHA256) for AES-CCM. `0010` carries the commands and `0019` the fragmented data of that exchange. The
  clearest public description is [miauth](https://github.com/dnandha/miauth), written from captures of Xiaomi
  scooters; Xiaomi's own SDK is published without the wire details
  ([MiEcosystem](https://github.com/MiEcosystem/mijia_ble_standard)).
- On those scooters the Nordic UART pair `6e400002`/`6e400003` then carries frames encrypted with the session
  keys. On this tablet the notifications seen on `6e400003` before any login are in clear, so its vendor channel
  may not be covered by the session at all. Not known yet.
- The "BLE KEY" that tools read from the Xiaomi cloud is the key of the encrypted advertisements, not the login
  token ([ble_monitor FAQ](https://custom-components.github.io/ble_monitor/faq)). Whether the login token of a
  bound device can be obtained is not established.
- A new registration from another client would replace the token: Mi Home would have to bind the tablet again.
  To be avoided unless the user accepts it.

## 5. Open questions, in the order that decides feasibility

1. Do the download and the deletion of a page work from the PC without the Xiaomi login, as the other commands
   do? To be tried on a test page: download first, compare with the live records, delete only with the user's
   agreement.
2. What does the tablet's erase button send, and what does `12 01 05` answer on an empty tablet?
3. The exact unit of the coordinates (a ruler), and whether the live stream goes on when a page is stored.
4. Does the tablet accept two clients, or does a PC connection keep Mi Home out (and the reverse)? While the
   phone was connected, the PC did not hear the tablet.

Answered: the commands, the live samples and the stored pages are in clear; the tablet accepts the commands
tried so far from a client that has not logged in on `fe95`; a stored page is a run of live records (section 3).
Both the live mode and the import of stored pages are feasible, the second subject to question 1.

## 6. Method

- **Never write a guessed command.** An unknown opcode could erase the stored pages or start a firmware update.
  Only commands seen in a capture of Mi Home are replayed, and the destructive ones (delete, format, reset,
  update) only with the user's agreement, on test pages.
- **Capture of Mi Home on the user's Android phone**: developer options, "Bluetooth HCI snoop log" enabled (not
  "filtered"), Bluetooth switched off and on, then a scripted session (connect, live mirror with known strokes: a
  diagonal from the top left corner to the bottom right one, a cross in the middle, a hard and a light stroke;
  save the page with the tablet's button; open the saved page in the application; delete it), then a bug report,
  from which only `FS/data/misc/bluetooth/logs/btsnoop_hci.log` is kept. That log records every Bluetooth device
  of the phone and can hold pairing keys: private, never in git.
- **From the PC**: `cargo run -p ardoise-ble --example survey -- scan [SECONDS]` lists what every nearby device
  advertises (MiBeacon headers decoded); `… survey -- gatt ADDRESS [SECONDS] [--out FILE]` connects, lists the
  GATT table, reads what is readable and prints the notifications. It never writes a characteristic value.
  `… survey -- send ADDRESS CHARACTERISTIC STEP...` writes the frames given on its command line, one by one,
  and prints the replies.
- The tablet is with the user at the development PC, unlike the Bamboo Slate: experiments can be run directly,
  and whatever is learnt must still end up in `ardoise-sim` and in replayable captures.

## 7. What Ardoise would need

- A second protocol next to the Slate's (`ardoise-proto`), its flows in `ardoise-device` (`device.json` already
  records a `protocol`), its simulated tablet in `ardoise-sim`, and in `ardoise-ble` a GATT layout chosen by the
  kind of tablet (the transport contract names the Slate's channels today).
- A choice of tablet in the interface, and the conversion of the tablet's units into the notes' own.
- A change of SPEC.md, to be agreed with the user once the study says it can be done.
