//! Scans for Bluetooth LE devices and prints their advertisements.
//!
//! Usage: `cargo run -p ardoise-ble --example scan [seconds]` (default 10 seconds).
//! A device is printed when first heard and again whenever its name or manufacturer data
//! changes (for example a tablet entering register mode). Nothing is ever connected to.

use std::collections::BTreeMap;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use ardoise_ble::BleConnector;
use ardoise_device::transport::{Advertisement, Connector};

fn main() -> ExitCode {
    let seconds = match std::env::args().nth(1) {
        None => 10,
        Some(arg) => match arg.parse::<u64>() {
            Ok(n) => n,
            Err(_) => {
                eprintln!("usage: scan [seconds]");
                return ExitCode::FAILURE;
            }
        },
    };

    match ardoise_ble::adapter_info() {
        Ok(info) => println!(
            "adapter: {} ({})",
            info.status,
            info.name.as_deref().unwrap_or("unnamed")
        ),
        Err(e) => println!("adapter: {e}"),
    }

    let mut connector = match BleConnector::new() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("scanning for {seconds} s...");
    let start = Instant::now();
    let mut seen: BTreeMap<String, Advertisement> = BTreeMap::new();
    let mut reports = 0usize;
    let result = connector.find(Duration::from_secs(seconds), &mut |ad| {
        reports += 1;
        let changed = seen.get(&ad.address).is_none_or(|old| {
            old.name != ad.name
                || old.company_id != ad.company_id
                || old.manufacturer_data != ad.manufacturer_data
        });
        if changed {
            println!("{:6.1} s  {}", start.elapsed().as_secs_f32(), describe(ad));
        }
        seen.insert(ad.address.clone(), ad.clone());
        false
    });

    if let Err(e) = result {
        eprintln!("error: {e}");
        return ExitCode::FAILURE;
    }
    let wacom = seen.values().filter(|ad| ad.is_wacom()).count();
    let register = seen.values().filter(|ad| ad.in_register_mode()).count();
    println!(
        "{} device(s) seen ({} Wacom, {} in register mode), {} advertisement report(s)",
        seen.len(),
        wacom,
        register,
        reports
    );
    ExitCode::SUCCESS
}

fn describe(ad: &Advertisement) -> String {
    let mut text = format!(
        "{}  {:>4} dBm  {:<24}",
        ad.address,
        ad.rssi.map_or_else(|| "?".to_owned(), |r| r.to_string()),
        ad.name.as_deref().unwrap_or("(no name)")
    );
    if let Some(id) = ad.company_id {
        let data: Vec<String> = ad
            .manufacturer_data
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        text.push_str(&format!("  company {id:#06x} [{}]", data.join(" ")));
    }
    if ad.is_wacom() {
        text.push_str("  WACOM");
        if ad.in_register_mode() {
            text.push_str(" (register mode)");
        }
    }
    text
}
