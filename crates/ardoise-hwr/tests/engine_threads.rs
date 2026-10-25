//! Where Windows' engine does its work (`docs/design/extras.md`, "Handwriting recognition",
//! threads): the CPU time of every thread of the process during a few recognitions, with its
//! priority and name. The worker of [`SystemRecognizer`] is named `ardoise-handwriting`; at
//! background priority its priority reads -4.
//!
//! `cargo test -p ardoise-hwr --test engine_threads -- --ignored --nocapture`

#![cfg(windows)]

use std::collections::HashMap;
use std::time::Instant;

use ardoise_hwr::synthetic::Hand;
use ardoise_hwr::{Language, Recognizer, SystemRecognizer, recognizer_for};
use windows::Win32::Foundation::{CloseHandle, FILETIME};
use windows::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetThreadDescription, GetThreadPriority, GetThreadTimes, OpenThread,
    THREAD_QUERY_LIMITED_INFORMATION,
};

/// A thread of this process: CPU time (100 ns units), priority, name.
struct Sample {
    cpu: u64,
    priority: i32,
    name: String,
}

fn ticks(time: FILETIME) -> u64 {
    (u64::from(time.dwHighDateTime) << 32) | u64::from(time.dwLowDateTime)
}

/// Every thread of this process, by id.
fn threads() -> HashMap<u32, Sample> {
    let mut out = HashMap::new();
    // SAFETY: plain Win32 calls on handles opened here and closed before returning; the
    // structures are initialised as the calls require (`dwSize`).
    unsafe {
        let process = GetCurrentProcessId();
        let Ok(snapshot) = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) else {
            return out;
        };
        let mut entry = THREADENTRY32 {
            dwSize: std::mem::size_of::<THREADENTRY32>() as u32,
            ..Default::default()
        };
        let mut more = Thread32First(snapshot, &mut entry).is_ok();
        while more {
            if entry.th32OwnerProcessID == process
                && let Ok(thread) =
                    OpenThread(THREAD_QUERY_LIMITED_INFORMATION, false, entry.th32ThreadID)
            {
                let (mut created, mut exited, mut kernel, mut user) = Default::default();
                let _ = GetThreadTimes(thread, &mut created, &mut exited, &mut kernel, &mut user);
                let name = GetThreadDescription(thread)
                    .ok()
                    .and_then(|name| name.to_string().ok())
                    .unwrap_or_default();
                out.insert(
                    entry.th32ThreadID,
                    Sample {
                        cpu: ticks(kernel) + ticks(user),
                        priority: GetThreadPriority(thread),
                        name,
                    },
                );
                let _ = CloseHandle(thread);
            }
            more = Thread32Next(snapshot, &mut entry).is_ok();
        }
        let _ = CloseHandle(snapshot);
    }
    out
}

fn measure(label: &str, recognizer: &SystemRecognizer) {
    let text = "le chat dort sur le canapé\nnous étudions la lumière et les ondes\n\
                la forêt est très calme ce matin\nles élèves préparent une expérience";
    let strokes = Hand::default().write(text);
    // The engine's objects are made at the first recognition.
    recognizer.recognize(&strokes, Language::Fr).unwrap();
    let before = threads();
    let started = Instant::now();
    for _ in 0..5 {
        recognizer.recognize(&strokes, Language::Fr).unwrap();
    }
    let wall = started.elapsed();
    let after = threads();
    let mut rows: Vec<(u64, u32, &Sample, bool)> = after
        .iter()
        .map(|(id, sample)| {
            let earlier = before.get(id);
            (
                sample.cpu - earlier.map_or(0, |s| s.cpu),
                *id,
                sample,
                earlier.is_none(),
            )
        })
        .filter(|(cpu, ..)| *cpu > 0)
        .collect();
    rows.sort_by(|a, b| b.0.cmp(&a.0));
    let total: u64 = rows.iter().map(|row| row.0).sum();
    println!(
        "{label}: 5 recognitions of 4 lines in {wall:?}, {:.0} ms of CPU",
        total as f64 / 1e4
    );
    for (cpu, id, sample, new) in rows {
        println!(
            "  thread {id:6}  {:7.1} ms  priority {:3}  {}{:?}",
            cpu as f64 / 1e4,
            sample.priority,
            if new { "(new) " } else { "" },
            sample.name
        );
    }
}

#[test]
#[ignore = "measurement: run with --ignored --nocapture"]
fn where_the_engine_works() {
    let normal = SystemRecognizer::new();
    let installed = normal.recognizers().unwrap_or_default();
    if recognizer_for(&installed, Language::Fr).is_none() {
        println!("skipped: no French handwriting recognizer on this PC");
        return;
    }
    measure("normal priority", &normal);
    measure("background priority", &SystemRecognizer::background());
}
