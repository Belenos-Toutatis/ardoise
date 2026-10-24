//! The OpenH264 library on this PC: download from Cisco, integrity check, installation and
//! removal (Cisco's binary licence: docs/research/media-stack.md §1).
//!
//! - **Download**: one HTTPS GET of [`OPENH264_URL`], from Cisco's own server only (no mirror,
//!   no redirect followed, no proxy taken from the environment), through Windows' TLS
//!   (SChannel) and certificate store. The compressed file (about 450 kB) stays in memory;
//!   `progress` is called as it arrives and `cancel` is checked between reads.
//! - **Check**: bzip2 decompressed by a pure-Rust decoder (about 980 kB), then the SHA-256 of the
//!   library must be [`OPENH264_SHA256`], the value pinned in the app (the `openh264` crate
//!   checks it again when loading).
//! - **Installation**: written next to its final name, synced, then renamed over it
//!   (`ardoise_core::fsutil::write_atomic`): a crash or a failure leaves either the previous
//!   state or the whole file, never a half-written library, and no temporary file.
//! - [`status`] hashes the file every time: a damaged or replaced file is reported, never loaded.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

use crate::{CodecStatus, MediaError};

/// Where the library is downloaded from (the only network access of the app, SPEC §6.2 row
/// "Réseau").
pub const OPENH264_URL: &str = "https://ciscobinary.openh264.org/openh264-2.6.0-win64.dll.bz2";

/// SHA-256 of the decompressed `openh264-2.6.0-win64.dll`, as published by Cisco and in the
/// `openh264` crate's list of known libraries.
pub const OPENH264_SHA256: &str =
    "2076cb5675ec6c1a4c70e7a2a322552f547b6eeed649d6dfcd9e02a543b24691";

/// Name of the installed library.
pub(crate) const LIBRARY_NAME: &str = "openh264-2.6.0-win64.dll";

/// Largest download accepted (the file is 452 053 bytes): a wrong answer from a captive portal
/// or a proxy cannot fill the memory.
#[cfg_attr(not(windows), allow(dead_code))]
const MAX_COMPRESSED_BYTES: u64 = 8 << 20;
/// Largest decompressed library accepted (it is 978 520 bytes).
const MAX_LIBRARY_BYTES: u64 = 16 << 20;

/// The library's path in `dir`.
pub(crate) fn library_path(dir: &Path) -> PathBuf {
    dir.join(LIBRARY_NAME)
}

/// Whether the library in `dir` is there and intact.
pub(crate) fn status(dir: &Path) -> CodecStatus {
    let path = library_path(dir);
    match std::fs::read(&path) {
        Ok(bytes) => {
            let hash = sha256_hex(&bytes);
            if hash == OPENH264_SHA256 {
                CodecStatus::Installed {
                    path,
                    bytes: bytes.len() as u64,
                }
            } else {
                CodecStatus::Damaged(format!(
                    "{} has the SHA-256 {hash} instead of {OPENH264_SHA256}",
                    path.display()
                ))
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => CodecStatus::NotInstalled,
        Err(e) => CodecStatus::Damaged(format!("cannot read {}: {e}", path.display())),
    }
}

/// Downloads, checks and installs the library into `dir`.
pub(crate) fn download(
    dir: &Path,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<(), MediaError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(MediaError::Cancelled);
    }
    let compressed = fetch(OPENH264_URL, progress, cancel)?;
    install(dir, LIBRARY_NAME, &compressed, OPENH264_SHA256, cancel)
}

/// Decompresses `compressed`, checks the SHA-256 of the result against `expected_sha256` and
/// writes it atomically as `dir/name`. On any error nothing is left: no temporary file, and
/// the folder is removed again if this call created it.
pub(crate) fn install(
    dir: &Path,
    name: &str,
    compressed: &[u8],
    expected_sha256: &str,
    cancel: &AtomicBool,
) -> Result<(), MediaError> {
    let library = decompress(compressed)?;
    let hash = sha256_hex(&library);
    if hash != expected_sha256 {
        return Err(MediaError::CodecIntegrity(format!(
            "SHA-256 {hash} instead of {expected_sha256}"
        )));
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(MediaError::Cancelled);
    }
    let created = !dir.exists();
    std::fs::create_dir_all(dir)?;
    let written = ardoise_core::fsutil::write_atomic(&dir.join(name), &library);
    if written.is_err() && created {
        // Only succeeds when the folder is still empty.
        let _ = std::fs::remove_dir(dir);
    }
    Ok(written?)
}

/// Deletes the library and any temporary file an interrupted installation left in `dir`.
/// Removing what is not there succeeds.
pub(crate) fn remove(dir: &Path) -> Result<(), MediaError> {
    remove_if_present(&library_path(dir))?;
    // `write_atomic` writes `.{name}.{pid}.{n}.part` next to the target.
    let temporary = format!(".{LIBRARY_NAME}.");
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    for entry in entries {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(&temporary) && name.ends_with(".part") {
            remove_if_present(&entry.path())?;
        }
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<(), MediaError> {
    match ardoise_core::fsutil::remove_file_with_retry(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}

/// The bzip2 stream decompressed, at most [`MAX_LIBRARY_BYTES`].
fn decompress(compressed: &[u8]) -> Result<Vec<u8>, MediaError> {
    let mut library = Vec::new();
    bzip2_rs::DecoderReader::new(compressed)
        .take(MAX_LIBRARY_BYTES + 1)
        .read_to_end(&mut library)
        .map_err(|e| MediaError::CodecIntegrity(format!("not a valid bzip2 file: {e}")))?;
    if library.len() as u64 > MAX_LIBRARY_BYTES {
        return Err(MediaError::CodecIntegrity(format!(
            "more than {MAX_LIBRARY_BYTES} bytes once decompressed"
        )));
    }
    if library.is_empty() {
        return Err(MediaError::CodecIntegrity("empty file".into()));
    }
    Ok(library)
}

/// Lower-case hexadecimal SHA-256.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// GETs `url` (HTTPS, no redirect, no proxy) into memory.
#[cfg(windows)]
fn fetch(
    url: &str,
    progress: &mut dyn FnMut(u64, Option<u64>),
    cancel: &AtomicBool,
) -> Result<Vec<u8>, MediaError> {
    use std::time::Duration;
    use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

    let failed = |e: &dyn std::fmt::Display| MediaError::CodecDownload(e.to_string());
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        // Cisco's server answers directly: a redirect would lead to another host.
        .max_redirects(0)
        .proxy(None)
        .user_agent(concat!("Ardoise/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(20)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(300)))
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build()
        .into();
    let response = agent.get(url).call().map_err(|e| failed(&e))?;
    if response.status() != 200 {
        return Err(MediaError::CodecDownload(format!(
            "the server answered {}",
            response.status()
        )));
    }
    let total = response.body().content_length();
    let mut reader = response
        .into_body()
        .into_with_config()
        .limit(MAX_COMPRESSED_BYTES)
        .reader();
    let capacity = total.unwrap_or(0).min(MAX_COMPRESSED_BYTES);
    let mut data = Vec::with_capacity(usize::try_from(capacity).unwrap_or(0));
    let mut buffer = vec![0u8; 64 * 1024];
    progress(0, total);
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(MediaError::Cancelled);
        }
        let n = reader.read(&mut buffer).map_err(|e| failed(&e))?;
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..n]);
        progress(data.len() as u64, total);
    }
    if let Some(total) = total
        && data.len() as u64 != total
    {
        return Err(MediaError::CodecDownload(format!(
            "received {} bytes of {total}",
            data.len()
        )));
    }
    Ok(data)
}

/// The library is the Windows build: nothing to download elsewhere.
#[cfg(not(windows))]
fn fetch(
    _url: &str,
    _progress: &mut dyn FnMut(u64, Option<u64>),
    _cancel: &AtomicBool,
) -> Result<Vec<u8>, MediaError> {
    Err(MediaError::CodecUnavailable(
        "the OpenH264 fallback is only available on Windows".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// "hello OpenH264\n" compressed by `bzip2 -9` (Python's `bz2.compress`).
    const HELLO_BZ2: [u8; 59] = [
        0x42, 0x5a, 0x68, 0x39, 0x31, 0x41, 0x59, 0x26, 0x53, 0x59, 0x52, 0x47, 0x2e, 0xfd, 0x00,
        0x00, 0x04, 0xdd, 0x80, 0x00, 0x10, 0x40, 0x00, 0x15, 0x00, 0x00, 0x40, 0x82, 0x45, 0xc0,
        0x00, 0x20, 0x00, 0x22, 0x08, 0xcd, 0x4c, 0x65, 0x08, 0x06, 0x80, 0x3a, 0x40, 0x07, 0xda,
        0xb0, 0xa8, 0x85, 0xf1, 0x77, 0x24, 0x53, 0x85, 0x09, 0x05, 0x24, 0x72, 0xef, 0xd0,
    ];
    const HELLO: &[u8] = b"hello OpenH264\n";

    #[test]
    fn decompresses_and_rejects_what_is_not_bzip2() {
        assert_eq!(decompress(&HELLO_BZ2).unwrap(), HELLO);
        assert!(matches!(
            decompress(b"MZ not compressed"),
            Err(MediaError::CodecIntegrity(_))
        ));
        let mut truncated = HELLO_BZ2.to_vec();
        truncated.truncate(30);
        assert!(matches!(
            decompress(&truncated),
            Err(MediaError::CodecIntegrity(_))
        ));
    }

    #[test]
    fn installs_only_the_expected_file_and_leaves_nothing_otherwise() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("codecs");
        let cancel = AtomicBool::new(false);
        let good = sha256_hex(HELLO);

        // Wrong hash: nothing written, not even the folder.
        let wrong = install(&dir, "lib.dll", &HELLO_BZ2, OPENH264_SHA256, &cancel);
        assert!(matches!(wrong, Err(MediaError::CodecIntegrity(_))));
        assert!(!dir.exists());

        // Cancelled before writing.
        cancel.store(true, Ordering::Relaxed);
        let cancelled = install(&dir, "lib.dll", &HELLO_BZ2, &good, &cancel);
        assert!(matches!(cancelled, Err(MediaError::Cancelled)));
        assert!(!dir.exists());
        cancel.store(false, Ordering::Relaxed);

        install(&dir, "lib.dll", &HELLO_BZ2, &good, &cancel).unwrap();
        assert_eq!(std::fs::read(dir.join("lib.dll")).unwrap(), HELLO);
        let names: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["lib.dll"], "no temporary file left");
    }

    #[test]
    fn status_hashes_the_file_and_remove_cleans_up() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path();
        assert_eq!(status(dir), CodecStatus::NotInstalled);

        // A file with the right name but another content is damaged, never installed.
        std::fs::write(library_path(dir), b"not the library").unwrap();
        assert!(matches!(status(dir), CodecStatus::Damaged(m) if m.contains("SHA-256")));

        // An interrupted installation's temporary file, and an unrelated file.
        let leftover = dir.join(format!(".{LIBRARY_NAME}.1234.0.part"));
        std::fs::write(&leftover, b"half").unwrap();
        std::fs::write(dir.join("other.txt"), b"keep").unwrap();
        remove(dir).unwrap();
        assert_eq!(status(dir), CodecStatus::NotInstalled);
        assert!(!leftover.exists());
        assert!(dir.join("other.txt").exists());
        // Removing again, or from a missing folder, succeeds.
        remove(dir).unwrap();
        remove(&dir.join("missing")).unwrap();
    }

    #[test]
    fn the_pinned_values_name_the_same_release() {
        assert!(OPENH264_URL.starts_with("https://ciscobinary.openh264.org/"));
        assert!(OPENH264_URL.ends_with(&format!("{LIBRARY_NAME}.bz2")));
        assert!(LIBRARY_NAME.contains(crate::OPENH264_VERSION));
        assert_eq!(OPENH264_SHA256.len(), 64);
    }

    #[test]
    fn a_cancelled_download_does_not_touch_the_network() {
        let root = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(true);
        let result = download(root.path(), &mut |_, _| {}, &cancel);
        assert!(matches!(result, Err(MediaError::Cancelled)));
        assert_eq!(status(root.path()), CodecStatus::NotInstalled);
    }
}
