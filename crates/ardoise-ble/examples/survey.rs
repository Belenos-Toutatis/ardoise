//! Surveys a Bluetooth LE device Ardoise does not know yet (a study tool, not part of the app).
//!
//! ```text
//! cargo run -p ardoise-ble --example survey -- scan [SECONDS] [--out FILE]
//! cargo run -p ardoise-ble --example survey -- gatt ADDRESS [SECONDS] [--out FILE]
//! cargo run -p ardoise-ble --example survey -- send ADDRESS CHARACTERISTIC STEP... [--out FILE]
//! ```
//!
//! - `scan` (default 20 s) listens and prints everything each device advertises: name, services,
//!   service data, manufacturer data. The header of a Xiaomi MiBeacon (service data `fe95`) is decoded.
//!   Nothing is connected to.
//! - `gatt` connects to `ADDRESS` (`00:00:5E:00:53:01` style), lists its services and characteristics,
//!   reads the readable ones, then enables every notification and prints what arrives for `SECONDS`
//!   (default 30). It never writes a characteristic value: its only writes are the standard "enable
//!   notifications" descriptors.
//! - `send` connects to `ADDRESS`, enables the notifications of the service that holds `CHARACTERISTIC` (a
//!   UUID, or its 16-bit form), then runs the steps in order and prints every notification received
//!   meanwhile. A step is a frame to write to the characteristic, in hexadecimal (`120104`), or a pause in
//!   seconds (`wait:20`). It writes exactly the frames given, nothing else: only frames whose meaning is known
//!   (seen in a capture of the vendor's application) belong on its command line.
//!
//! `--out FILE` also records one JSON object per line. That file holds Bluetooth addresses: keep it
//! out of git (`testdata/private/`).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match Command::parse(&args) {
        Ok(command) => run(command),
        Err(message) => {
            eprintln!("{message}");
            eprintln!("usage: survey scan [SECONDS] [--out FILE]");
            eprintln!("       survey gatt ADDRESS [SECONDS] [--out FILE]");
            eprintln!("       survey send ADDRESS CHARACTERISTIC STEP... [--out FILE]");
            eprintln!("       (STEP: a frame in hexadecimal, or wait:SECONDS)");
            ExitCode::FAILURE
        }
    }
}

/// What the command line asks for.
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Scan {
        seconds: u64,
        out: Option<String>,
    },
    Gatt {
        address: String,
        seconds: u64,
        out: Option<String>,
    },
    Send {
        address: String,
        characteristic: String,
        steps: Vec<Step>,
        out: Option<String>,
    },
}

/// One step of `send`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Write this frame to the characteristic.
    Write(Vec<u8>),
    /// Only listen, for this many seconds.
    Wait(u64),
}

impl Step {
    fn parse(word: &str) -> Result<Self, String> {
        if let Some(seconds) = word.strip_prefix("wait:") {
            return seconds
                .parse::<u64>()
                .map(Self::Wait)
                .map_err(|_| format!("not a number of seconds: {seconds}"));
        }
        let digits: Vec<u8> = word.bytes().filter(|b| *b != b'_').collect();
        if digits.is_empty() || !digits.len().is_multiple_of(2) {
            return Err(format!("not a frame in hexadecimal: {word}"));
        }
        digits
            .chunks(2)
            .map(|pair| {
                std::str::from_utf8(pair)
                    .ok()
                    .and_then(|text| u8::from_str_radix(text, 16).ok())
            })
            .collect::<Option<Vec<u8>>>()
            .map(Self::Write)
            .ok_or_else(|| format!("not a frame in hexadecimal: {word}"))
    }
}

impl Command {
    fn parse(args: &[String]) -> Result<Self, String> {
        let mut out = None;
        let mut rest = Vec::new();
        let mut words = args.iter();
        while let Some(word) = words.next() {
            if word == "--out" {
                out = Some(words.next().ok_or("--out needs a file name")?.clone());
            } else {
                rest.push(word.as_str());
            }
        }
        let seconds = |word: Option<&&str>, default: u64| match word {
            None => Ok(default),
            Some(text) => text
                .parse::<u64>()
                .map_err(|_| format!("not a number of seconds: {text}")),
        };
        match rest.as_slice() {
            ["scan", tail @ ..] if tail.len() <= 1 => Ok(Self::Scan {
                seconds: seconds(tail.first(), 20)?,
                out,
            }),
            ["gatt", address, tail @ ..] if tail.len() <= 1 => Ok(Self::Gatt {
                address: (*address).to_owned(),
                seconds: seconds(tail.first(), 30)?,
                out,
            }),
            ["send", address, characteristic, steps @ ..] if !steps.is_empty() => Ok(Self::Send {
                address: (*address).to_owned(),
                characteristic: (*characteristic).to_owned(),
                steps: steps
                    .iter()
                    .map(|word| Step::parse(word))
                    .collect::<Result<_, _>>()?,
                out,
            }),
            _ => Err(
                "expected `scan`, `gatt ADDRESS` or `send ADDRESS CHARACTERISTIC STEP...`"
                    .to_owned(),
            ),
        }
    }
}

/// Bytes as `0a 1b 2c`.
fn hex(bytes: &[u8]) -> String {
    let text: Vec<String> = bytes.iter().map(|b| format!("{b:02x}")).collect();
    text.join(" ")
}

/// The printable ASCII of `bytes`, when all of it is printable and there is some.
fn ascii(bytes: &[u8]) -> Option<String> {
    let trimmed: &[u8] = match bytes.iter().rposition(|b| *b != 0) {
        Some(last) => &bytes[..=last],
        None => &[],
    };
    (!trimmed.is_empty() && trimmed.iter().all(|b| (0x20..0x7f).contains(b)))
        .then(|| String::from_utf8_lossy(trimmed).into_owned())
}

/// The header of a Xiaomi MiBeacon (the service data of the 16-bit service `fe95`), in words.
///
/// Layout (as read by the open-source parsers of Home Assistant): frame control (2 bytes, little-endian),
/// product identifier (2 bytes, little-endian), frame counter (1 byte), then the device address (6 bytes,
/// reversed) when the frame control says so, then a capability byte when it says so. In the frame control:
/// bit 3 encrypted, bit 4 address, bit 5 capability, bit 6 object, bit 7 mesh, bit 8 registered (bound to an
/// account), bit 9 solicited (asks to be bound), bits 10-11 authentication mode, bits 12-15 version.
fn describe_mibeacon(data: &[u8]) -> String {
    let [f0, f1, p0, p1, counter, rest @ ..] = data else {
        return format!("MiBeacon too short ({} bytes)", data.len());
    };
    let control = u16::from_le_bytes([*f0, *f1]);
    let product = u16::from_le_bytes([*p0, *p1]);
    let bit = |n: u16| control >> n & 1 == 1;
    let mut text = format!(
        "MiBeacon v{} product {product:#06x} counter {counter}",
        control >> 12
    );
    text.push_str(if bit(8) {
        ", registered"
    } else {
        ", not registered"
    });
    text.push_str(match control >> 10 & 3 {
        0 => ", auth mode 0 (old)",
        1 => ", auth mode 1 (safe)",
        2 => ", auth mode 2 (standard)",
        _ => ", auth mode 3",
    });
    for (n, name) in [
        (3, "encrypted"),
        (6, "object"),
        (7, "mesh"),
        (9, "solicited"),
    ] {
        if bit(n) {
            text.push_str(", ");
            text.push_str(name);
        }
    }
    let mut rest = rest;
    if bit(4) {
        match rest {
            [a, b, c, d, e, f, tail @ ..] => {
                text.push_str(&format!(
                    ", address {f:02X}:{e:02X}:{d:02X}:{c:02X}:{b:02X}:{a:02X}"
                ));
                rest = tail;
            }
            _ => return text + ", address cut short",
        }
    }
    if bit(5) {
        match rest {
            [capability, ..] => text.push_str(&format!(", capability {capability:#04x}")),
            [] => text.push_str(", capability cut short"),
        }
    }
    text
}

#[cfg(not(windows))]
fn run(command: Command) -> ExitCode {
    // Mentions every helper so that the build stays free of warnings where Bluetooth is not available.
    let _ = (hex(&[]), ascii(&[]), describe_mibeacon(&[]));
    eprintln!("the survey needs Windows Bluetooth ({command:?})");
    ExitCode::FAILURE
}

#[cfg(windows)]
fn run(command: Command) -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_time()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("error: cannot start the runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let outcome = runtime.block_on(async {
        match command {
            Command::Scan { seconds, out } => {
                windows_impl::scan(seconds, windows_impl::Record::new(out)?).await
            }
            Command::Gatt {
                address,
                seconds,
                out,
            } => windows_impl::gatt(&address, seconds, windows_impl::Record::new(out)?).await,
            Command::Send {
                address,
                characteristic,
                steps,
                out,
            } => {
                windows_impl::send(
                    &address,
                    &characteristic,
                    &steps,
                    windows_impl::Record::new(out)?,
                )
                .await
            }
        }
    });
    runtime.shutdown_background();
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::collections::{BTreeMap, BTreeSet};
    use std::fs::File;
    use std::io::Write as _;
    use std::time::{Duration, Instant};

    use btleplug::api::{
        BDAddr, Central as _, CentralEvent, CharPropFlags, Characteristic, Manager as _,
        Peripheral as _, PeripheralProperties, ScanFilter, WriteType,
    };
    use btleplug::platform::{Adapter, Manager, Peripheral, PeripheralId};
    use futures::StreamExt as _;
    use serde_json::{Value, json};
    use tokio::time::{MissedTickBehavior, timeout};
    use uuid::Uuid;

    use super::{Step, ascii, describe_mibeacon, hex};

    /// The 16-bit service of Xiaomi's "Mi" Bluetooth protocol (MiBeacon in advertisements).
    const XIAOMI_SERVICE: Uuid = Uuid::from_u128(0x0000fe95_0000_1000_8000_00805f9b34fb);
    /// Distinct advertisements printed per device; MiBeacons change with every frame.
    const SNAPSHOTS_PER_DEVICE: usize = 6;
    /// Time allowed for one Bluetooth operation on a connected device.
    const STEP: Duration = Duration::from_secs(8);
    /// Time allowed to hear the device before connecting to it.
    const FIND: Duration = Duration::from_secs(25);

    /// Prints to the terminal and, when asked, records one JSON object per line.
    pub(super) struct Record {
        start: Instant,
        file: Option<File>,
    }

    impl Record {
        pub(super) fn new(path: Option<String>) -> Result<Self, String> {
            let file = match path {
                Some(path) => {
                    Some(File::create(&path).map_err(|e| format!("cannot create {path}: {e}"))?)
                }
                None => None,
            };
            Ok(Self {
                start: Instant::now(),
                file,
            })
        }

        fn seconds(&self) -> f64 {
            (self.start.elapsed().as_secs_f64() * 1000.0).round() / 1000.0
        }

        fn line(&mut self, text: &str, mut entry: Value) {
            let t = self.seconds();
            println!("{t:8.2} s  {text}");
            if let Some(file) = &mut self.file {
                if let Some(object) = entry.as_object_mut() {
                    object.insert("t".to_owned(), json!(t));
                }
                let _ = writeln!(file, "{entry}");
            }
        }
    }

    /// `fe95` for a 16-bit Bluetooth UUID, the whole UUID otherwise.
    fn short(uuid: Uuid) -> String {
        let value = uuid.as_u128();
        // The Bluetooth base UUID, `0000xxxx-0000-1000-8000-00805f9b34fb`, without its 16-bit part.
        let base = 0x0000_1000_8000_0080_5f9b_34fb_u128;
        if value & !(0xffff_u128 << 96) == base {
            format!("{:04x}", value >> 96)
        } else {
            uuid.to_string()
        }
    }

    async fn adapter() -> Result<Adapter, String> {
        match ardoise_ble::adapter_info() {
            Ok(info) => println!(
                "adapter: {} ({})",
                info.status,
                info.name.as_deref().unwrap_or("unnamed")
            ),
            Err(error) => println!("adapter: {error}"),
        }
        let manager = Manager::new().await.map_err(|e| e.to_string())?;
        manager
            .adapters()
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .next()
            .ok_or_else(|| "no Bluetooth adapter".to_owned())
    }

    /// What a device advertises, without what changes with every frame (signal strength).
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
    struct Snapshot {
        name: Option<String>,
        services: BTreeSet<Uuid>,
        service_data: BTreeMap<Uuid, Vec<u8>>,
        manufacturer_data: BTreeMap<u16, Vec<u8>>,
    }

    impl Snapshot {
        fn of(properties: &PeripheralProperties) -> Self {
            Self {
                name: [&properties.local_name, &properties.advertisement_name]
                    .into_iter()
                    .flatten()
                    .map(|name| name.trim_end_matches('\0'))
                    .find(|name| !name.is_empty())
                    .map(str::to_owned),
                services: properties.services.iter().copied().collect(),
                service_data: properties
                    .service_data
                    .iter()
                    .map(|(uuid, data)| (*uuid, data.clone()))
                    .collect(),
                manufacturer_data: properties
                    .manufacturer_data
                    .iter()
                    .map(|(id, data)| (*id, data.clone()))
                    .collect(),
            }
        }

        fn is_xiaomi(&self) -> bool {
            self.service_data.contains_key(&XIAOMI_SERVICE)
                || self.services.contains(&XIAOMI_SERVICE)
        }

        fn text(&self) -> String {
            let mut text = format!("{:<22}", self.name.as_deref().unwrap_or("(no name)"));
            if !self.services.is_empty() {
                let list: Vec<String> = self.services.iter().map(|uuid| short(*uuid)).collect();
                text.push_str(&format!("  services [{}]", list.join(", ")));
            }
            for (uuid, data) in &self.service_data {
                text.push_str(&format!("  data {} [{}]", short(*uuid), hex(data)));
                if *uuid == XIAOMI_SERVICE {
                    text.push_str(&format!("  = {}", describe_mibeacon(data)));
                }
            }
            for (id, data) in &self.manufacturer_data {
                text.push_str(&format!("  company {id:#06x} [{}]", hex(data)));
            }
            text
        }

        fn json(&self) -> Value {
            json!({
                "name": self.name,
                "services": self.services.iter().map(|u| short(*u)).collect::<Vec<_>>(),
                "serviceData": self.service_data.iter()
                    .map(|(u, d)| (short(*u), json!(hex(d)))).collect::<serde_json::Map<_, _>>(),
                "manufacturerData": self.manufacturer_data.iter()
                    .map(|(id, d)| (format!("{id:#06x}"), json!(hex(d)))).collect::<serde_json::Map<_, _>>(),
            })
        }
    }

    pub(super) async fn scan(seconds: u64, mut record: Record) -> Result<(), String> {
        let adapter = adapter().await?;
        let mut events = adapter.events().await.map_err(|e| e.to_string())?;
        let _ = adapter.stop_scan().await;
        adapter
            .clear_peripherals()
            .await
            .map_err(|e| e.to_string())?;
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|e| e.to_string())?;
        println!("listening for {seconds} s (nothing is connected to)...");

        let mut seen: BTreeMap<String, BTreeSet<Snapshot>> = BTreeMap::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
        let mut sweep = tokio::time::interval(Duration::from_secs(1));
        sweep.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            let peripherals = tokio::select! {
                () = tokio::time::sleep_until(deadline) => break,
                event = events.next() => match event {
                    Some(
                        CentralEvent::DeviceDiscovered(id)
                        | CentralEvent::DeviceUpdated(id)
                        | CentralEvent::ManufacturerDataAdvertisement { id, .. }
                        | CentralEvent::ServiceDataAdvertisement { id, .. }
                        | CentralEvent::ServicesAdvertisement { id, .. },
                    ) => adapter.peripheral(&id).await.into_iter().collect(),
                    Some(_) => Vec::new(),
                    None => break,
                },
                _ = sweep.tick() => adapter.peripherals().await.map_err(|e| e.to_string())?,
            };
            for peripheral in peripherals {
                let Ok(Some(properties)) = peripheral.properties().await else {
                    continue;
                };
                let address = properties.address.to_string();
                let snapshot = Snapshot::of(&properties);
                let known = seen.entry(address.clone()).or_default();
                if known.contains(&snapshot) {
                    continue;
                }
                if known.len() < SNAPSHOTS_PER_DEVICE {
                    let rssi = properties
                        .rssi
                        .map_or_else(|| "?".to_owned(), |r| r.to_string());
                    let mut entry = snapshot.json();
                    if let Some(object) = entry.as_object_mut() {
                        object.insert("type".to_owned(), json!("advertisement"));
                        object.insert("address".to_owned(), json!(address));
                        object.insert("rssi".to_owned(), json!(properties.rssi));
                    }
                    record.line(
                        &format!("{address}  {rssi:>4} dBm  {}", snapshot.text()),
                        entry,
                    );
                }
                known.insert(snapshot);
            }
        }
        let _ = adapter.stop_scan().await;

        let xiaomi: Vec<&String> = seen
            .iter()
            .filter(|(_, snapshots)| snapshots.iter().any(Snapshot::is_xiaomi))
            .map(|(address, _)| address)
            .collect();
        println!(
            "{} device(s) heard, {} with the Xiaomi service (fe95){}",
            seen.len(),
            xiaomi.len(),
            if xiaomi.is_empty() {
                String::new()
            } else {
                format!(
                    ": {}",
                    xiaomi
                        .iter()
                        .map(|a| a.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            }
        );
        Ok(())
    }

    /// Listens until the device at `address` is heard, so that Windows knows it.
    async fn find(adapter: &Adapter, address: BDAddr) -> Result<Peripheral, String> {
        let id = PeripheralId::from(address);
        let _ = adapter.stop_scan().await;
        adapter
            .start_scan(ScanFilter::default())
            .await
            .map_err(|e| e.to_string())?;
        let started = Instant::now();
        let found = loop {
            if let Ok(peripheral) = adapter.peripheral(&id).await {
                break Some(peripheral);
            }
            if started.elapsed() >= FIND {
                break None;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        };
        let _ = adapter.stop_scan().await;
        found.ok_or_else(|| {
            format!(
                "{address} was not heard in {} s: is it switched on, near, and not connected to a phone?",
                FIND.as_secs()
            )
        })
    }

    /// A characteristic UUID from the command line: whole, or its 16-bit form.
    fn parse_uuid(text: &str) -> Result<Uuid, String> {
        if text.len() == 4 {
            return u16::from_str_radix(text, 16)
                .map(|short| {
                    Uuid::from_u128(u128::from(short) << 96 | 0x0000_1000_8000_0080_5f9b_34fb)
                })
                .map_err(|_| format!("not a characteristic UUID: {text}"));
        }
        Uuid::parse_str(text).map_err(|_| format!("not a characteristic UUID: {text}"))
    }

    pub(super) async fn send(
        address: &str,
        characteristic: &str,
        steps: &[Step],
        mut record: Record,
    ) -> Result<(), String> {
        let parsed: BDAddr = address
            .trim()
            .parse()
            .map_err(|_| format!("invalid Bluetooth address {address:?}"))?;
        let target = parse_uuid(characteristic)?;
        let adapter = adapter().await?;
        println!("looking for {parsed}...");
        let peripheral = find(&adapter, parsed).await?;
        timeout(STEP * 2, peripheral.connect())
            .await
            .map_err(|_| "the connection timed out".to_owned())?
            .map_err(|e| format!("cannot connect: {e}"))?;
        record.line("connected", json!({"type": "connected"}));
        let outcome = converse(&peripheral, target, steps, &mut record).await;
        let _ = timeout(STEP, peripheral.disconnect()).await;
        record.line("disconnected by the survey", json!({"type": "closed"}));
        outcome
    }

    async fn converse(
        peripheral: &Peripheral,
        target: Uuid,
        steps: &[Step],
        record: &mut Record,
    ) -> Result<(), String> {
        timeout(STEP * 2, peripheral.discover_services())
            .await
            .map_err(|_| "listing the services timed out".to_owned())?
            .map_err(|e| format!("cannot list the services: {e}"))?;
        let characteristics = peripheral.characteristics();
        let command = characteristics
            .iter()
            .find(|c| c.uuid == target)
            .ok_or_else(|| format!("the device has no characteristic {}", short(target)))?;
        let write_type = if command.properties.contains(CharPropFlags::WRITE) {
            WriteType::WithResponse
        } else if command
            .properties
            .contains(CharPropFlags::WRITE_WITHOUT_RESPONSE)
        {
            WriteType::WithoutResponse
        } else {
            return Err(format!("{} cannot be written", short(target)));
        };

        let mut notifications = peripheral
            .notifications()
            .await
            .map_err(|e| e.to_string())?;
        for characteristic in characteristics.iter().filter(|c| {
            c.service_uuid == command.service_uuid
                && c.properties
                    .intersects(CharPropFlags::NOTIFY | CharPropFlags::INDICATE)
        }) {
            let uuid = short(characteristic.uuid);
            timeout(STEP, peripheral.subscribe(characteristic))
                .await
                .map_err(|_| format!("enabling the notifications of {uuid} timed out"))?
                .map_err(|e| format!("notifications refused on {uuid}: {e}"))?;
            record.line(
                &format!("notifications enabled on {uuid}"),
                json!({"type": "subscribed", "uuid": uuid}),
            );
        }

        let mut count = 0usize;
        let mut open = true;
        for step in steps {
            let pause = match step {
                Step::Write(frame) => {
                    record.line(
                        &format!("-> {}  [{}]", short(target), hex(frame)),
                        json!({"type": "write", "uuid": short(target), "hex": hex(frame)}),
                    );
                    timeout(STEP, peripheral.write(command, frame, write_type))
                        .await
                        .map_err(|_| "the write timed out".to_owned())?
                        .map_err(|e| format!("the write failed: {e}"))?;
                    Duration::from_millis(600)
                }
                Step::Wait(seconds) => Duration::from_secs(*seconds),
            };
            let deadline = tokio::time::Instant::now() + pause;
            while open {
                tokio::select! {
                    () = tokio::time::sleep_until(deadline) => break,
                    notification = notifications.next() => match notification {
                        Some(notification) => {
                            count += 1;
                            let uuid = short(notification.uuid);
                            record.line(
                                &format!("<- {uuid}  [{}]", hex(&notification.value)),
                                json!({"type": "notification", "uuid": uuid, "hex": hex(&notification.value)}),
                            );
                        }
                        None => open = false,
                    },
                }
            }
            if !open || !peripheral.is_connected().await.unwrap_or(false) {
                record.line(
                    "the device closed the connection",
                    json!({"type": "disconnected"}),
                );
                break;
            }
        }
        println!("{count} notification(s) received");
        Ok(())
    }

    fn properties_text(properties: CharPropFlags) -> String {
        let names = [
            (CharPropFlags::BROADCAST, "broadcast"),
            (CharPropFlags::READ, "read"),
            (
                CharPropFlags::WRITE_WITHOUT_RESPONSE,
                "write-without-response",
            ),
            (CharPropFlags::WRITE, "write"),
            (CharPropFlags::NOTIFY, "notify"),
            (CharPropFlags::INDICATE, "indicate"),
            (CharPropFlags::AUTHENTICATED_SIGNED_WRITES, "signed-write"),
            (CharPropFlags::EXTENDED_PROPERTIES, "extended"),
        ];
        let list: Vec<&str> = names
            .iter()
            .filter(|(flag, _)| properties.contains(*flag))
            .map(|(_, name)| *name)
            .collect();
        list.join(" ")
    }

    pub(super) async fn gatt(
        address: &str,
        seconds: u64,
        mut record: Record,
    ) -> Result<(), String> {
        let parsed: BDAddr = address
            .trim()
            .parse()
            .map_err(|_| format!("invalid Bluetooth address {address:?}"))?;
        let adapter = adapter().await?;
        println!("looking for {parsed}...");
        let peripheral = find(&adapter, parsed).await?;
        if let Ok(Some(properties)) = peripheral.properties().await {
            let snapshot = Snapshot::of(&properties);
            let mut entry = snapshot.json();
            if let Some(object) = entry.as_object_mut() {
                object.insert("type".to_owned(), json!("advertisement"));
                object.insert("address".to_owned(), json!(parsed.to_string()));
            }
            record.line(&format!("heard: {}", snapshot.text()), entry);
        }

        timeout(STEP * 2, peripheral.connect())
            .await
            .map_err(|_| "the connection timed out".to_owned())?
            .map_err(|e| format!("cannot connect: {e}"))?;
        record.line("connected", json!({"type": "connected"}));
        let outcome = explore(&peripheral, seconds, &mut record).await;
        let _ = timeout(STEP, peripheral.disconnect()).await;
        record.line("disconnected by the survey", json!({"type": "closed"}));
        outcome
    }

    async fn explore(
        peripheral: &Peripheral,
        seconds: u64,
        record: &mut Record,
    ) -> Result<(), String> {
        timeout(STEP * 2, peripheral.discover_services())
            .await
            .map_err(|_| "listing the services timed out".to_owned())?
            .map_err(|e| format!("cannot list the services: {e}"))?;

        let mut notifying: Vec<Characteristic> = Vec::new();
        for service in peripheral.services() {
            record.line(
                &format!(
                    "service {}{}",
                    short(service.uuid),
                    if service.primary { "" } else { " (secondary)" }
                ),
                json!({"type": "service", "uuid": short(service.uuid), "primary": service.primary}),
            );
            for characteristic in &service.characteristics {
                let properties = properties_text(characteristic.properties);
                let mut text = format!(
                    "    characteristic {}  [{properties}]",
                    short(characteristic.uuid)
                );
                let mut entry = json!({
                    "type": "characteristic",
                    "service": short(service.uuid),
                    "uuid": short(characteristic.uuid),
                    "properties": properties,
                    "descriptors": characteristic.descriptors.iter()
                        .map(|d| short(d.uuid)).collect::<Vec<_>>(),
                });
                if characteristic.properties.contains(CharPropFlags::READ) {
                    let (shown, value) = match timeout(STEP, peripheral.read(characteristic)).await
                    {
                        Ok(Ok(value)) => {
                            let mut shown = format!("  = [{}]", hex(&value));
                            if let Some(printable) = ascii(&value) {
                                shown.push_str(&format!(" \"{printable}\""));
                            }
                            (shown, json!({"value": hex(&value)}))
                        }
                        Ok(Err(error)) => (
                            format!("  read failed: {error}"),
                            json!({"readError": error.to_string()}),
                        ),
                        Err(_) => (
                            "  read timed out".to_owned(),
                            json!({"readError": "timeout"}),
                        ),
                    };
                    text.push_str(&shown);
                    if let (Some(object), Some(more)) = (entry.as_object_mut(), value.as_object()) {
                        object.extend(more.clone());
                    }
                }
                record.line(&text, entry);
                if characteristic
                    .properties
                    .intersects(CharPropFlags::NOTIFY | CharPropFlags::INDICATE)
                {
                    notifying.push(characteristic.clone());
                }
            }
        }

        // The stream first, so that what the device sends as soon as it is enabled is not lost.
        let mut notifications = peripheral
            .notifications()
            .await
            .map_err(|e| e.to_string())?;
        for characteristic in &notifying {
            let uuid = short(characteristic.uuid);
            match timeout(STEP, peripheral.subscribe(characteristic)).await {
                Ok(Ok(())) => record.line(
                    &format!("notifications enabled on {uuid}"),
                    json!({"type": "subscribed", "uuid": uuid}),
                ),
                Ok(Err(error)) => record.line(
                    &format!("notifications refused on {uuid}: {error}"),
                    json!({"type": "subscribeError", "uuid": uuid, "error": error.to_string()}),
                ),
                Err(_) => record.line(
                    &format!("notifications timed out on {uuid}"),
                    json!({"type": "subscribeError", "uuid": uuid, "error": "timeout"}),
                ),
            }
        }

        println!("listening for {seconds} s: write on the tablet, press its buttons...");
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
        let mut alive = tokio::time::interval(Duration::from_millis(500));
        alive.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut count = 0usize;
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(deadline) => break,
                notification = notifications.next() => match notification {
                    Some(notification) => {
                        count += 1;
                        let uuid = short(notification.uuid);
                        record.line(
                            &format!("{uuid}  [{}]", hex(&notification.value)),
                            json!({"type": "notification", "uuid": uuid, "hex": hex(&notification.value)}),
                        );
                    }
                    None => break,
                },
                _ = alive.tick() => {
                    if !peripheral.is_connected().await.unwrap_or(false) {
                        record.line(
                            "the device closed the connection",
                            json!({"type": "disconnected"}),
                        );
                        break;
                    }
                }
            }
        }
        println!("{count} notification(s) received");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn the_command_line() {
        assert_eq!(
            Command::parse(&words("scan")),
            Ok(Command::Scan {
                seconds: 20,
                out: None
            })
        );
        assert_eq!(
            Command::parse(&words("scan 45 --out a.jsonl")),
            Ok(Command::Scan {
                seconds: 45,
                out: Some("a.jsonl".to_owned())
            })
        );
        assert_eq!(
            Command::parse(&words("gatt --out b.jsonl 00:00:5E:00:53:01 60")),
            Ok(Command::Gatt {
                address: "00:00:5E:00:53:01".to_owned(),
                seconds: 60,
                out: Some("b.jsonl".to_owned())
            })
        );
        assert_eq!(
            Command::parse(&words("send 00:00:5E:00:53:01 2a00 0a_ff01 wait:3 00")),
            Ok(Command::Send {
                address: "00:00:5E:00:53:01".to_owned(),
                characteristic: "2a00".to_owned(),
                steps: vec![
                    Step::Write(vec![0x0a, 0xff, 0x01]),
                    Step::Wait(3),
                    Step::Write(vec![0x00])
                ],
                out: None
            })
        );
        assert!(Command::parse(&words("send 00:00:5E:00:53:01 2a00")).is_err());
        assert!(Command::parse(&words("send 00:00:5E:00:53:01 2a00 0a0")).is_err());
        assert!(Command::parse(&words("send 00:00:5E:00:53:01 2a00 zz")).is_err());
        assert!(Command::parse(&words("send 00:00:5E:00:53:01 2a00 wait:soon")).is_err());
        assert!(Command::parse(&words("gatt")).is_err());
        assert!(Command::parse(&words("scan soon")).is_err());
        assert!(Command::parse(&words("scan 1 2")).is_err());
        assert!(Command::parse(&words("scan --out")).is_err());
        assert!(Command::parse(&[]).is_err());
    }

    #[test]
    fn bytes_as_text() {
        assert_eq!(hex(&[0x0a, 0xff, 0x00]), "0a ff 00");
        assert_eq!(hex(&[]), "");
        assert_eq!(ascii(b"Mi Board\0\0"), Some("Mi Board".to_owned()));
        assert_eq!(ascii(&[0x01, 0x41]), None);
        assert_eq!(ascii(&[0, 0]), None);
    }

    #[test]
    fn a_mibeacon_header() {
        // Version 5, standard authentication, registered, with address and capability.
        // Frame control 0x5930 = 0101 1001 0011 0000.
        let data = [
            0x30, 0x59, 0x5b, 0x04, 0x07, 0x01, 0x53, 0x00, 0x5e, 0x00, 0x00, 0x08,
        ];
        assert_eq!(
            describe_mibeacon(&data),
            "MiBeacon v5 product 0x045b counter 7, registered, auth mode 2 (standard), \
             address 00:00:5E:00:53:01, capability 0x08"
        );
        // Not registered, asking to be bound, encrypted, no address.
        let data = [0x08, 0x22, 0x34, 0x12, 0xff];
        assert_eq!(
            describe_mibeacon(&data),
            "MiBeacon v2 product 0x1234 counter 255, not registered, auth mode 0 (old), encrypted, solicited"
        );
        assert_eq!(describe_mibeacon(&[1, 2]), "MiBeacon too short (2 bytes)");
        assert_eq!(
            describe_mibeacon(&[0x10, 0x00, 0, 0, 0, 1, 2]),
            "MiBeacon v0 product 0x0000 counter 0, not registered, auth mode 0 (old), address cut short"
        );
    }
}
