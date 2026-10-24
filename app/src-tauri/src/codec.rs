//! The OpenH264 fallback for MP4 (SPEC §4.5, §6.2 "Vidéo MP4"): Cisco's library, downloaded
//! only when the user asks, used only while the user allows it on this PC
//! (`[machines.<this PC>] openh264`), and removable at any time. It lives in
//! `%LOCALAPPDATA%\Ardoise\codecs`, local to the PC (never in the library nor next to the
//! portable exe). Contract: `docs/design/media.md`, "Interface package: contracts".
//!
//! Cisco's conditions: the notice "OpenH264 Video Codec provided by Cisco Systems, Inc." and the
//! licence of the binary are shown where the user controls the codec ([`OpenH264State`]).

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use ardoise_media::{CISCO_NOTICE, CodecStatus, MediaError, OPENH264_VERSION, OpenH264Codec};
use serde::Serialize;
use tauri::State;
use tauri::ipc::Channel;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult, store, update};
use crate::export::CANCELLED;
use crate::video::{PROGRESS_INTERVAL, Throttle};

/// Error code of a download that failed (network, checksum, disk).
pub const OPENH264_DOWNLOAD_FAILED: &str = "openh264DownloadFailed";

/// Cisco's licence for the OpenH264 binary, reproduced as Cisco requires.
pub const LICENSE: &str = include_str!("openh264-license.txt");

/// Where the codec lives: `%LOCALAPPDATA%\Ardoise\codecs`; under the temporary folder when
/// `LOCALAPPDATA` is not set (Linux).
pub fn codec_dir() -> PathBuf {
    codec_dir_in(std::env::var_os("LOCALAPPDATA"))
}

fn codec_dir_in(local_app_data: Option<OsString>) -> PathBuf {
    local_app_data
        .filter(|dir| !dir.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("Ardoise")
        .join("codecs")
}

/// `OpenH264State.status` in `video.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum InstallState {
    NotInstalled,
    Installed,
    Damaged,
}

/// `OpenH264State` in `video.ts`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenH264State {
    pub status: InstallState,
    pub bytes: Option<u64>,
    pub allowed: bool,
    pub media_foundation: bool,
    pub version: &'static str,
    pub notice: &'static str,
    pub license: &'static str,
}

/// `DownloadProgress` in `video.ts`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct DownloadProgress {
    pub received: u64,
    pub total: Option<u64>,
}

/// The state shown by the settings screen.
pub fn state_of(status: &CodecStatus, allowed: bool, media_foundation: bool) -> OpenH264State {
    let (status, bytes) = match status {
        CodecStatus::NotInstalled => (InstallState::NotInstalled, None),
        CodecStatus::Installed { bytes, .. } => (InstallState::Installed, Some(*bytes)),
        CodecStatus::Damaged(_) => (InstallState::Damaged, None),
    };
    OpenH264State {
        status,
        bytes,
        allowed,
        media_foundation,
        version: OPENH264_VERSION,
        notice: CISCO_NOTICE,
        license: LICENSE,
    }
}

/// Whether Windows Media Foundation offers an H.264 encoder here (then the fallback is not
/// needed). Asked once, on a thread of its own (it initialises COM for the calling thread).
pub fn media_foundation_h264() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        std::thread::Builder::new()
            .name("media-foundation-probe".into())
            .spawn(|| ardoise_media::available_encoders().is_ok_and(|e| !e.h264.is_empty()))
            .ok()
            .and_then(|probe| probe.join().ok())
            .unwrap_or(false)
    })
}

/// Whether the user allows the fallback on this PC (no when the settings cannot be read).
pub fn openh264_allowed(context: &AppContext) -> bool {
    store(context)
        .ok()
        .and_then(|store| {
            let settings = store.load().ok()?;
            Some(settings.openh264_allowed(store.machine()))
        })
        .unwrap_or(false)
}

/// Allows or disallows the fallback on this PC.
pub fn set_allowed_in(context: &AppContext, allowed: bool) -> CommandResult<()> {
    let machine = store(context)?.machine().to_owned();
    update(context, |settings| {
        settings.set_openh264_allowed(&machine, allowed)
    })?;
    Ok(())
}

fn current(context: &AppContext, codec: &OpenH264Codec) -> OpenH264State {
    state_of(
        &codec.status(),
        openh264_allowed(context),
        media_foundation_h264(),
    )
}

/// Error for the interface from a download.
pub fn download_error(error: MediaError) -> CommandError {
    match error {
        MediaError::Cancelled => CommandError::new(CANCELLED, error),
        other => CommandError::new(OPENH264_DOWNLOAD_FAILED, other),
    }
}

/// The download under way: its cancel flag, and whether one runs.
#[derive(Debug, Default)]
pub struct CodecDownload {
    cancel: Arc<AtomicBool>,
    running: Arc<AtomicBool>,
}

/// Clears the "running" flag when the download ends, however it ends.
struct Running(Arc<AtomicBool>);

impl Drop for Running {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Downloads the codec into `codec`'s folder; `progress` gets at most 10 reports a second plus
/// the last one.
pub fn download_in(
    codec: &OpenH264Codec,
    progress: &mut dyn FnMut(DownloadProgress),
    cancel: &AtomicBool,
) -> CommandResult<()> {
    let mut throttle = Throttle::new(PROGRESS_INTERVAL);
    let mut report = |received: u64, total: Option<u64>| {
        let done = total.is_some_and(|total| received >= total);
        if throttle.ready(Instant::now(), done) {
            progress(DownloadProgress { received, total });
        }
    };
    codec.download(&mut report, cancel).map_err(download_error)
}

#[tauri::command]
pub async fn openh264_state(context: State<'_, AppContext>) -> CommandResult<OpenH264State> {
    Ok(current(&context, &OpenH264Codec::new(codec_dir())))
}

/// Downloads OpenH264 from Cisco (SHA-256 checked) and allows the fallback.
#[tauri::command]
pub async fn openh264_download(
    context: State<'_, AppContext>,
    download: State<'_, CodecDownload>,
    on_progress: Channel<DownloadProgress>,
) -> CommandResult<OpenH264State> {
    if download.running.swap(true, Ordering::AcqRel) {
        return Err(CommandError::new(
            OPENH264_DOWNLOAD_FAILED,
            "a download is already running",
        ));
    }
    let running = Running(Arc::clone(&download.running));
    let cancel = Arc::clone(&download.cancel);
    cancel.store(false, Ordering::Relaxed);
    let codec = OpenH264Codec::new(codec_dir());
    let downloaded = {
        let codec = codec.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _running = running;
            // The window may be gone: the download still ends normally.
            download_in(
                &codec,
                &mut |progress| {
                    let _ = on_progress.send(progress);
                },
                &cancel,
            )
        })
        .await
        .map_err(|error| CommandError::new(OPENH264_DOWNLOAD_FAILED, error))?
    };
    downloaded?;
    set_allowed_in(&context, true)?;
    Ok(current(&context, &codec))
}

#[tauri::command]
pub fn openh264_cancel_download(download: State<'_, CodecDownload>) {
    download.cancel.store(true, Ordering::Relaxed);
}

/// Disallows the fallback, then deletes the library.
#[tauri::command]
pub async fn openh264_remove(context: State<'_, AppContext>) -> CommandResult<OpenH264State> {
    set_allowed_in(&context, false)?;
    let codec = OpenH264Codec::new(codec_dir());
    if codec.status() != CodecStatus::NotInstalled {
        codec
            .remove()
            .map_err(|error| CommandError::new("unexpected", error))?;
    }
    Ok(current(&context, &codec))
}

#[tauri::command]
pub async fn set_openh264_allowed(
    context: State<'_, AppContext>,
    allowed: bool,
) -> CommandResult<OpenH264State> {
    set_allowed_in(&context, allowed)?;
    Ok(current(&context, &OpenH264Codec::new(codec_dir())))
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use serde_json::json;

    use super::*;
    use crate::library::tests::context;

    #[test]
    fn the_codec_lives_in_the_local_app_data_folder() {
        let local = codec_dir_in(Some(OsString::from(r"C:\Users\prof\AppData\Local")));
        assert_eq!(
            local,
            Path::new(r"C:\Users\prof\AppData\Local")
                .join("Ardoise")
                .join("codecs")
        );
        let elsewhere = std::env::temp_dir().join("Ardoise").join("codecs");
        assert_eq!(codec_dir_in(None), elsewhere);
        assert_eq!(codec_dir_in(Some(OsString::new())), elsewhere);
        assert!(codec_dir().ends_with(Path::new("Ardoise").join("codecs")));
    }

    #[test]
    fn the_state_carries_the_notice_and_the_licence() {
        let state = state_of(&CodecStatus::NotInstalled, false, false);
        assert_eq!(state.status, InstallState::NotInstalled);
        assert_eq!(state.bytes, None);
        assert_eq!(state.version, "2.6.0");
        assert_eq!(
            state.notice,
            "OpenH264 Video Codec provided by Cisco Systems, Inc."
        );
        assert!(state.license.contains("Cisco"), "{}", state.license);
        assert!(state.license.len() > 1000);
        let value = serde_json::to_value(&state).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "allowed",
                "bytes",
                "license",
                "mediaFoundation",
                "notice",
                "status",
                "version"
            ]
        );
        assert_eq!(value["status"], "notInstalled");

        let installed = state_of(
            &CodecStatus::Installed {
                path: PathBuf::from("openh264.dll"),
                bytes: 1_234_567,
            },
            true,
            true,
        );
        assert_eq!(installed.status, InstallState::Installed);
        assert_eq!(installed.bytes, Some(1_234_567));
        assert!(installed.allowed && installed.media_foundation);
        let damaged = state_of(&CodecStatus::Damaged("hash".into()), true, false);
        assert_eq!(
            serde_json::to_value(damaged).unwrap()["status"],
            json!("damaged")
        );
        assert_eq!(
            serde_json::to_value(DownloadProgress {
                received: 10,
                total: None
            })
            .unwrap(),
            json!({ "received": 10, "total": null })
        );
    }

    #[test]
    fn the_fallback_is_allowed_per_machine() {
        let settings = tempfile::tempdir().unwrap();
        let context = context(settings.path());
        assert!(!openh264_allowed(&context));
        set_allowed_in(&context, true).unwrap();
        assert!(openh264_allowed(&context));
        set_allowed_in(&context, false).unwrap();
        assert!(!openh264_allowed(&context));
    }

    #[test]
    fn download_errors_get_the_interface_codes() {
        assert_eq!(download_error(MediaError::Cancelled).code(), CANCELLED);
        assert_eq!(
            download_error(MediaError::CodecUnavailable("offline".into())).code(),
            OPENH264_DOWNLOAD_FAILED
        );
        assert_eq!(
            download_error(MediaError::Io(std::io::Error::other("disk full"))).code(),
            OPENH264_DOWNLOAD_FAILED
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn media_foundation_is_only_on_windows() {
        assert!(!media_foundation_h264());
    }
}
