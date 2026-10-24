//! The real download of OpenH264 2.6.0 from Cisco into the codec folder of the app
//! (`ARDOISE_CODECS_DIR`, else `%LOCALAPPDATA%\Ardoise\codecs`), so that the fallback tests can
//! run. Never in CI: it is ignored unless asked for.
//!
//! `cargo test -p ardoise-media --test openh264_download -- --ignored --nocapture`
#![cfg(windows)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::sync::atomic::AtomicBool;
use std::time::Instant;

use ardoise_media::{CodecStatus, OPENH264_SHA256, OPENH264_URL, OpenH264Codec};

#[test]
#[ignore = "downloads OpenH264 from Cisco's server (about 450 kB) into the codec folder"]
fn download_openh264_from_cisco() {
    let codec = OpenH264Codec::default_location().expect("LOCALAPPDATA or ARDOISE_CODECS_DIR");
    let started = Instant::now();
    let (mut received, mut total, mut calls) = (0u64, None, 0u32);
    codec
        .download(
            &mut |bytes, size| {
                assert!(bytes >= received, "progress goes forward");
                (received, total, calls) = (bytes, size, calls + 1);
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    let elapsed = started.elapsed();
    let CodecStatus::Installed { path, bytes } = codec.status() else {
        panic!("not installed after the download: {:?}", codec.status());
    };
    println!("downloaded {OPENH264_URL}");
    println!(
        "  {received} compressed bytes (server announced {total:?}), {calls} progress calls, \
         {:.2} s",
        elapsed.as_secs_f64()
    );
    println!("  installed {} ({bytes} bytes)", path.display());
    println!("  SHA-256 {OPENH264_SHA256} (checked)");
    assert_eq!(total, Some(received));
    assert!(calls >= 2);
    // The release's sizes (docs/research/media-stack.md §1).
    assert_eq!((received, bytes), (452_053, 978_520));
    let leftovers: Vec<_> = std::fs::read_dir(codec.dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty(), "temporary files left: {leftovers:?}");
}
