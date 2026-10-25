//! Copy of a note's image to the Windows clipboard ("PNG" and `CF_DIB`), to paste it into
//! Word or OneNote (SPEC §4.6). The data comes from `ardoise_render::clipboard_image`.
//!
//! The sequence of clipboard calls (open with retries, empty, hand over each format, free
//! what was not handed over, close) is written once over [`ClipboardApi`], so that it is
//! tested on every platform with a fake; the Windows implementation only wraps the system
//! calls. Outside Windows the command answers `clipboardUnavailable`.

use std::time::Duration;

use ardoise_core::library::Library;
use ardoise_render::{ClipboardImage, RenderOptions, clipboard_image_at, render_note};
use tauri::State;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::export::{ExportSource, check_dpi, export_error, load_sources, render_error};

/// Error code when the clipboard cannot be used (another program holds it, or not Windows).
pub const CLIPBOARD_UNAVAILABLE: &str = "clipboardUnavailable";
/// Another program may hold the clipboard for a moment: tries before giving up.
const OPEN_ATTEMPTS: u32 = 10;
#[cfg_attr(not(windows), allow(dead_code))]
const OPEN_RETRY_DELAY: Duration = Duration::from_millis(30);
/// Name of the clipboard format of PNG files, as Office registers it.
const PNG_FORMAT: &str = "PNG";

/// The clipboard data of `source` rendered at `dpi` with its paper.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn clipboard_payload(
    library: Option<&Library>,
    source: ExportSource,
    dpi: u32,
) -> CommandResult<ClipboardImage> {
    check_dpi(dpi)?;
    let loaded = load_sources(library, vec![source])?;
    let Some(source) = loaded.first() else {
        return Err(export_error("nothing to copy"));
    };
    let image =
        render_note(source.view(), &RenderOptions::at_dpi(dpi, true)).map_err(render_error)?;
    clipboard_image_at(&image, Some(dpi)).map_err(render_error)
}

/// The clipboard calls the copy needs (used on Windows; a fake in the tests).
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) trait ClipboardApi {
    /// A block of global memory.
    type Memory: Copy;
    /// Opens the clipboard; `false` while another program has it open.
    fn open(&mut self) -> bool;
    /// Empties it and takes ownership of it.
    fn empty(&mut self) -> bool;
    /// The identifier of a registered format, `None` if it cannot be registered.
    fn register_format(&mut self, name: &str) -> Option<u32>;
    /// The identifier of `CF_DIB`.
    fn dib_format(&self) -> u32;
    /// Movable global memory holding a copy of `bytes`, `None` if it cannot be allocated.
    fn alloc_copy(&mut self, bytes: &[u8]) -> Option<Self::Memory>;
    /// Hands `memory` over as `format`. On success the system owns it; on failure it is still
    /// ours to free.
    fn set(&mut self, format: u32, memory: Self::Memory) -> bool;
    fn free(&mut self, memory: Self::Memory);
    fn close(&mut self);
}

/// Hands `bytes` over as `format`, freeing the memory if the system does not take it.
fn place<A: ClipboardApi>(api: &mut A, format: u32, bytes: &[u8]) -> bool {
    let Some(memory) = api.alloc_copy(bytes) else {
        return false;
    };
    if api.set(format, memory) {
        true
    } else {
        api.free(memory);
        false
    }
}

/// Puts both formats of `payload` on the clipboard: "PNG" when it can (best effort), then
/// `CF_DIB`, which every program reads and which must succeed. The clipboard is always closed
/// once opened.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn put_on_clipboard<A: ClipboardApi>(
    api: &mut A,
    payload: &ClipboardImage,
    retry_delay: Duration,
) -> Result<(), String> {
    open_with_retries(api, retry_delay)?;
    let result = if !api.empty() {
        Err("cannot empty the clipboard".into())
    } else {
        if let Some(png) = api.register_format(PNG_FORMAT) {
            place(api, png, &payload.png);
        }
        let dib = api.dib_format();
        if place(api, dib, &payload.dib) {
            Ok(())
        } else {
            Err("cannot put the image on the clipboard".into())
        }
    };
    api.close();
    result
}

/// Opens the clipboard, trying again for a while when another program holds it.
fn open_with_retries<A: ClipboardApi>(api: &mut A, retry_delay: Duration) -> Result<(), String> {
    let mut attempt = 1;
    while !api.open() {
        if attempt >= OPEN_ATTEMPTS {
            return Err("another program is using the clipboard".into());
        }
        attempt += 1;
        std::thread::sleep(retry_delay);
    }
    Ok(())
}

#[cfg(windows)]
mod system {
    //! The Win32 clipboard calls, nothing more.

    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL, HWND};
    use windows_sys::Win32::System::DataExchange::{
        CloseClipboard, EmptyClipboard, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    };
    use windows_sys::Win32::System::Memory::{
        GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock,
    };
    use windows_sys::Win32::System::Ole::CF_DIB;

    /// The clipboard, owned by `owner` (the app's window, or null) once emptied.
    pub(super) struct Win32Clipboard {
        pub(super) owner: HWND,
    }

    impl super::ClipboardApi for Win32Clipboard {
        type Memory = HGLOBAL;

        fn open(&mut self) -> bool {
            // SAFETY: `owner` is a window of this process or null; both are accepted.
            unsafe { OpenClipboard(self.owner) != 0 }
        }

        fn empty(&mut self) -> bool {
            // SAFETY: called between a successful `OpenClipboard` and `CloseClipboard`.
            unsafe { EmptyClipboard() != 0 }
        }

        fn register_format(&mut self, name: &str) -> Option<u32> {
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            // SAFETY: `wide` is a NUL-terminated UTF-16 string that outlives the call.
            let id = unsafe { RegisterClipboardFormatW(wide.as_ptr()) };
            (id != 0).then_some(id)
        }

        fn dib_format(&self) -> u32 {
            u32::from(CF_DIB)
        }

        fn alloc_copy(&mut self, bytes: &[u8]) -> Option<HGLOBAL> {
            // SAFETY: `GlobalAlloc` returns null or a movable block of at least
            // `bytes.len()` bytes. It is locked to get its address, filled with exactly
            // `bytes.len()` bytes (the regions cannot overlap: the block is new), then
            // unlocked before anything else may use it. On failure it is freed here.
            unsafe {
                let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
                if memory.is_null() {
                    return None;
                }
                let target: *mut c_void = GlobalLock(memory);
                if target.is_null() {
                    GlobalFree(memory);
                    return None;
                }
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), target.cast::<u8>(), bytes.len());
                // Returns 0 once the lock count reaches zero, which is the expected outcome.
                GlobalUnlock(memory);
                Some(memory)
            }
        }

        fn set(&mut self, format: u32, memory: HGLOBAL) -> bool {
            // SAFETY: the clipboard is open and emptied by us; `memory` is an unlocked
            // movable block. On success the system owns it and we never touch it again.
            unsafe { !SetClipboardData(format, memory).is_null() }
        }

        fn free(&mut self, memory: HGLOBAL) {
            // SAFETY: `memory` came from `GlobalAlloc` and was not handed over.
            unsafe {
                GlobalFree(memory);
            }
        }

        fn close(&mut self) {
            // SAFETY: called once after a successful `OpenClipboard` on this thread.
            unsafe {
                CloseClipboard();
            }
        }
    }
}

/// Puts `payload` on the Windows clipboard, owned by the window `owner` (an `HWND` as an
/// address, 0 for none).
#[cfg(windows)]
fn copy_to_system(payload: &ClipboardImage, owner: usize) -> CommandResult<()> {
    let mut api = system::Win32Clipboard {
        owner: owner as windows_sys::Win32::Foundation::HWND,
    };
    put_on_clipboard(&mut api, payload, OPEN_RETRY_DELAY)
        .map_err(|message| CommandError::new(CLIPBOARD_UNAVAILABLE, message))
}

/// Copies the note's image at `dpi` to the clipboard. `clipboardUnavailable` outside Windows.
#[tauri::command]
pub async fn copy_note_image(
    window: tauri::WebviewWindow,
    context: State<'_, AppContext>,
    source: ExportSource,
    dpi: u32,
) -> CommandResult<()> {
    #[cfg(windows)]
    {
        let sources = [source];
        let library = crate::export::library_for(&context, &sources)?;
        let [source] = sources;
        // The window becomes the clipboard owner (null would do, but the documentation of
        // `OpenClipboard` warns that `SetClipboardData` may then fail).
        let owner = window.hwnd().map_or(0, |hwnd| hwnd.0 as usize);
        tauri::async_runtime::spawn_blocking(move || {
            let payload = clipboard_payload(library.as_ref(), source, dpi)?;
            copy_to_system(&payload, owner)
        })
        .await
        .map_err(export_error)?
    }
    #[cfg(not(windows))]
    {
        let _ = (window, context, source, dpi);
        Err(CommandError::new(
            CLIPBOARD_UNAVAILABLE,
            "the clipboard copy is only available on Windows",
        ))
    }
}

// --- Text: the handwriting text export (handwriting.rs) -------------------------------------

/// `CF_UNICODETEXT`: UTF-16 text ending with a NUL, which every program reads.
const UNICODE_TEXT_FORMAT: u32 = 13;

/// `text` as `CF_UNICODETEXT` data: UTF-16 little-endian, NUL-terminated.
fn unicode_text(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// Puts `text` on the clipboard as `CF_UNICODETEXT`. The clipboard is always closed once opened.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn put_text_on_clipboard<A: ClipboardApi>(
    api: &mut A,
    text: &str,
    retry_delay: Duration,
) -> Result<(), String> {
    open_with_retries(api, retry_delay)?;
    let result = if !api.empty() {
        Err("cannot empty the clipboard".into())
    } else if place(api, UNICODE_TEXT_FORMAT, &unicode_text(text)) {
        Ok(())
    } else {
        Err("cannot put the text on the clipboard".into())
    };
    api.close();
    result
}

/// Copies `text` to the clipboard, owned by the window `owner` (an `HWND` as an address, 0 for
/// none). `clipboardUnavailable` outside Windows.
pub fn copy_text(text: &str, owner: usize) -> CommandResult<()> {
    #[cfg(windows)]
    {
        let mut api = system::Win32Clipboard {
            owner: owner as windows_sys::Win32::Foundation::HWND,
        };
        put_text_on_clipboard(&mut api, text, OPEN_RETRY_DELAY)
            .map_err(|message| CommandError::new(CLIPBOARD_UNAVAILABLE, message))
    }
    #[cfg(not(windows))]
    {
        let _ = (text, owner);
        Err(CommandError::new(
            CLIPBOARD_UNAVAILABLE,
            "the clipboard copy is only available on Windows",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::library::path_text;
    use crate::library::tests::{library, write_sample};

    /// Records the calls; fails where told to.
    #[derive(Default)]
    struct Fake {
        busy_opens: u32,
        fail_empty: bool,
        fail_alloc: bool,
        refuse: Vec<u32>,
        no_png_format: bool,
        calls: Vec<String>,
        /// Memory blocks currently allocated and not handed over or freed.
        live: Vec<u32>,
        next: u32,
        /// Bytes handed over, by format.
        placed: Vec<(u32, usize)>,
    }

    impl ClipboardApi for Fake {
        type Memory = u32;

        fn open(&mut self) -> bool {
            self.calls.push("open".into());
            if self.busy_opens > 0 {
                self.busy_opens -= 1;
                false
            } else {
                true
            }
        }
        fn empty(&mut self) -> bool {
            self.calls.push("empty".into());
            !self.fail_empty
        }
        fn register_format(&mut self, name: &str) -> Option<u32> {
            self.calls.push(format!("register {name}"));
            (!self.no_png_format).then_some(0xc0de)
        }
        fn dib_format(&self) -> u32 {
            8
        }
        fn alloc_copy(&mut self, bytes: &[u8]) -> Option<u32> {
            if self.fail_alloc {
                return None;
            }
            self.next += 1;
            self.live.push(self.next);
            self.calls.push(format!("alloc {}", bytes.len()));
            Some(self.next)
        }
        fn set(&mut self, format: u32, memory: u32) -> bool {
            self.calls.push(format!("set {format}"));
            if self.refuse.contains(&format) {
                return false;
            }
            // The system owns it now.
            self.live.retain(|&m| m != memory);
            self.placed.push((format, memory as usize));
            true
        }
        fn free(&mut self, memory: u32) {
            self.calls.push("free".into());
            assert!(self.live.contains(&memory), "freeing memory we do not own");
            self.live.retain(|&m| m != memory);
        }
        fn close(&mut self) {
            self.calls.push("close".into());
        }
    }

    fn payload() -> ClipboardImage {
        ClipboardImage {
            png: vec![1; 10],
            dib: vec![2; 20],
        }
    }

    fn put(fake: &mut Fake) -> Result<(), String> {
        put_on_clipboard(fake, &payload(), Duration::ZERO)
    }

    #[test]
    fn both_formats_are_handed_over_and_the_clipboard_closed() {
        let mut fake = Fake {
            busy_opens: 2,
            ..Fake::default()
        };
        put(&mut fake).unwrap();
        assert_eq!(
            fake.calls,
            [
                "open",
                "open",
                "open",
                "empty",
                "register PNG",
                "alloc 10",
                "set 49374",
                "alloc 20",
                "set 8",
                "close"
            ]
        );
        assert!(fake.live.is_empty(), "nothing leaked");
        assert_eq!(fake.placed.len(), 2);
    }

    #[test]
    fn memory_the_system_refused_is_freed() {
        // PNG refused: still a success with the DIB alone.
        let mut fake = Fake {
            refuse: vec![0xc0de],
            ..Fake::default()
        };
        put(&mut fake).unwrap();
        assert!(fake.live.is_empty());
        assert!(fake.calls.contains(&"free".to_owned()));
        assert_eq!(fake.placed, [(8, 2)]);

        // DIB refused: an error, memory freed, clipboard closed.
        let mut fake = Fake {
            refuse: vec![8],
            ..Fake::default()
        };
        assert!(put(&mut fake).is_err());
        assert!(fake.live.is_empty());
        assert_eq!(fake.calls.last().map(String::as_str), Some("close"));

        // No "PNG" format: the DIB alone.
        let mut fake = Fake {
            no_png_format: true,
            ..Fake::default()
        };
        put(&mut fake).unwrap();
        assert_eq!(fake.placed.len(), 1);
    }

    #[test]
    fn failures_close_the_clipboard_and_a_busy_one_is_reported() {
        let mut fake = Fake {
            fail_empty: true,
            ..Fake::default()
        };
        assert!(put(&mut fake).is_err());
        assert_eq!(fake.calls, ["open", "empty", "close"]);

        let mut fake = Fake {
            fail_alloc: true,
            ..Fake::default()
        };
        assert!(put(&mut fake).is_err());
        assert_eq!(fake.calls.last().map(String::as_str), Some("close"));

        // Held by another program for good: never closed, since never opened.
        let mut fake = Fake {
            busy_opens: 100,
            ..Fake::default()
        };
        assert!(put(&mut fake).is_err());
        assert_eq!(fake.calls.len(), OPEN_ATTEMPTS as usize);
        assert!(fake.calls.iter().all(|c| c == "open"));
    }

    #[test]
    fn text_goes_as_nul_terminated_utf16() {
        assert_eq!(unicode_text("é"), [0xe9, 0, 0, 0]);
        let mut fake = Fake {
            busy_opens: 1,
            ..Fake::default()
        };
        put_text_on_clipboard(&mut fake, "forêt\r\n", Duration::ZERO).unwrap();
        assert_eq!(
            fake.calls,
            ["open", "open", "empty", "alloc 16", "set 13", "close"]
        );
        assert!(fake.live.is_empty());
        let mut fake = Fake {
            refuse: vec![13],
            ..Fake::default()
        };
        assert!(put_text_on_clipboard(&mut fake, "x", Duration::ZERO).is_err());
        assert!(fake.live.is_empty(), "refused memory freed");
        assert_eq!(fake.calls.last().map(String::as_str), Some("close"));
    }

    #[test]
    fn the_payload_is_the_note_at_the_resolution_asked() {
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Essai");
        let source = ExportSource {
            path: path_text(&saved.path),
            note: None,
            image_key: None,
        };
        let payload = clipboard_payload(Some(&library), source.clone(), 96).unwrap();
        let image = ardoise_render::decode_png(&payload.png).unwrap();
        // A4 landscape at 96 dpi.
        assert_eq!((image.width, image.height), (1123, 816));
        let field = |at: usize| u32::from_le_bytes(payload.dib[at..at + 4].try_into().unwrap());
        assert_eq!((field(4), field(8)), (1123, 816));
        assert_eq!(
            payload.dib.len(),
            40 + (1123 * 3usize).div_ceil(4) * 4 * 816
        );
        // With its paper: opaque.
        assert!(image.pixels.as_chunks::<4>().0.iter().all(|p| p[3] == 255));

        let error = clipboard_payload(Some(&library), source.clone(), 1200).unwrap_err();
        assert_eq!(error.code(), "exportFailed");
        let error = clipboard_payload(None, source, 150).unwrap_err();
        assert_eq!(error.code(), "noLibrary");
    }
}
