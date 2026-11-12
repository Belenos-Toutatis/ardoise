# Xiaomi Mijia LCD blackboard, storage edition: Bluetooth study

Feasibility study, started 2026-11-12 at the user's request: can Ardoise fetch the pages stored in this second
tablet and receive its pen strokes live, as it does with the Bamboo Slate? Nothing in the application depends on
this document yet. SPEC.md covers the Bamboo Slate only (see `docs/questions.md`, Q14).

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
  `12 01 09`. Their cause is not identified. The shape (one byte, a length of 1, one byte) suggests frames of the
  form opcode, length, payload, sent in clear.
- Nothing arrived on `6e400004` nor on the `fe95` characteristics. Pen strokes are not sent unasked: something
  must be written first (a login, a "live" command, or both).

## 3. The Xiaomi layer (`fe95`), from public sources (reported)

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

## 4. Open questions, in the order that decides feasibility

1. Does the tablet accept commands on `6e400002` from a client that has not logged in on `fe95`?
2. Are the commands and the data (stored pages, live samples) in clear, as the `12 01 09` events are?
3. What are the commands: list the pages, download one, delete one, start and stop the live stream, read the
   battery and the time?
4. What is the format of a stored page and of a live sample (coordinates, pressure, timing, pen up and down,
   erasing with the button)? What are the units and the size of the writing area?
5. What do `12 01 09` and the characteristic `6e400004` mean?

If the answer to 1 is no and the session keys cannot be obtained without unbinding the tablet from Mi Home, the
study stops there, unless the user accepts that Ardoise replaces Mi Home for this tablet.

## 5. Method

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
- The tablet is with the user at the development PC, unlike the Bamboo Slate: experiments can be run directly,
  and whatever is learnt must still end up in `ardoise-sim` and in replayable captures.

## 6. What Ardoise would need

- A second protocol next to the Slate's (`ardoise-proto`), its flows in `ardoise-device` (`device.json` already
  records a `protocol`), its simulated tablet in `ardoise-sim`, and in `ardoise-ble` a GATT layout chosen by the
  kind of tablet (the transport contract names the Slate's channels today).
- A choice of tablet in the interface, and the conversion of the tablet's units into the notes' own.
- A change of SPEC.md, to be agreed with the user once the study says it can be done.
