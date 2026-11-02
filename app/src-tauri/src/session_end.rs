//! Windows ending the session (shutdown, restart for an update, logoff, an installer's Restart
//! Manager) while a note has unsaved changes (M10; `docs/design/interface.md`, "M10: drafts and
//! session end").
//!
//! Tao, Tauri's windowing layer, raises no `CloseRequested` for `WM_QUERYENDSESSION`: it lets
//! Windows go on, and on `WM_ENDSESSION` it ends the process at once (`std::process::exit(0)` in
//! tao 0.37's `thread_event_target_callback`). The editor's leave dialog never shows and its
//! unsaved edits would be gone. Here:
//!
//! - while the interface reports unsaved work ([`set_unsaved_work`]), the main window carries a
//!   shutdown block reason (`ShutdownBlockReasonCreate`): Windows names Ardoise and the reason
//!   on its "apps preventing shutdown" screen;
//! - the main window is subclassed ([`install`]): on `WM_QUERYENDSESSION` it asks the interface
//!   to write its drafts at once (event [`SESSION_ENDING`]) and, while work is unsaved, answers
//!   FALSE ([`blocks_session_end`]) so that Windows waits and shows the reason. The user can go
//!   back and save, or end anyway: the draft is on disk by then. A critical end
//!   (`ENDSESSION_CRITICAL`) is never held back.

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};

/// The event asking the main window to write its drafts now.
pub const SESSION_ENDING: &str = "session-ending";
/// Label of the main window.
const MAIN: &str = "main";
/// Longest reason Windows shows (`MAX_STR_BLOCKREASON`), in UTF-16 units.
const MAX_REASON: usize = 255;

/// The `lParam` bit of `WM_QUERYENDSESSION` for an end that cannot be held back (WinUser.h).
#[cfg_attr(not(windows), allow(dead_code))]
pub const ENDSESSION_CRITICAL: u32 = 0x4000_0000;

/// Unsaved work is reported by the interface.
static UNSAVED: AtomicBool = AtomicBool::new(false);

/// Whether `WM_QUERYENDSESSION` with these `lParam` flags is answered FALSE: only while work is
/// unsaved, and never for a critical end (Windows ends the process whatever the answer).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn blocks_session_end(unsaved: bool, flags: u32) -> bool {
    unsaved && flags & ENDSESSION_CRITICAL == 0
}

/// The reason as Windows takes it: trimmed, at most [`MAX_REASON`] UTF-16 units; `None` when
/// empty.
pub fn block_reason(reason: Option<&str>) -> Option<Vec<u16>> {
    let reason = reason?.trim();
    if reason.is_empty() {
        return None;
    }
    let mut wide: Vec<u16> = Vec::new();
    for c in reason.chars() {
        let mut buffer = [0u16; 2];
        let encoded = c.encode_utf16(&mut buffer);
        if wide.len() + encoded.len() > MAX_REASON {
            break;
        }
        wide.extend_from_slice(encoded);
    }
    wide.push(0);
    Some(wide)
}

/// The interface says whether a note has unsaved changes (`reason`: the sentence Windows shows,
/// in the interface language; `null` when nothing is unsaved).
#[tauri::command]
pub fn set_unsaved_work(app: AppHandle, reason: Option<String>) {
    let wide = block_reason(reason.as_deref());
    UNSAVED.store(wide.is_some(), Ordering::SeqCst);
    if let Some(window) = app.get_webview_window(MAIN) {
        let target = window.clone();
        // The reason belongs to the window's thread (`ShutdownBlockReasonCreate` refuses others).
        let _ = window.run_on_main_thread(move || imp::set_reason(&target, wide.as_deref()));
    }
}

/// Watches the session's end on the main window (call from the thread that created it).
pub fn install(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        imp::install(app, &window);
    }
}

/// Asks the main window to write its drafts now (from another thread: the window procedure
/// that calls this must return first).
fn ask_for_drafts(app: AppHandle) {
    std::thread::spawn(move || {
        use tauri::Emitter;
        let _ = app.emit_to(MAIN, SESSION_ENDING, ());
    });
}

#[cfg(windows)]
mod imp {
    use std::sync::OnceLock;
    use std::sync::atomic::Ordering;

    use tauri::{AppHandle, WebviewWindow};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Shutdown::{
        ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy,
    };
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_ENDSESSION, WM_QUERYENDSESSION};

    use super::{UNSAVED, ask_for_drafts, blocks_session_end};

    /// Identifies our subclass among the window's (tao has its own).
    const SUBCLASS_ID: usize = 0x4152_444f;

    static APP: OnceLock<AppHandle> = OnceLock::new();

    fn hwnd_of(window: &WebviewWindow) -> Option<HWND> {
        window.hwnd().ok().map(|hwnd| hwnd.0 as HWND)
    }

    pub fn install(app: &AppHandle, window: &WebviewWindow) {
        let _ = APP.set(app.clone());
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        // SAFETY: `hwnd` is the live main window, on the thread that created it (Tauri's setup);
        // the procedure lives as long as the program.
        let ok = unsafe { SetWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID, 0) };
        if ok == 0 {
            eprintln!("Ardoise: the end of the Windows session cannot be watched");
        }
    }

    pub fn set_reason(window: &WebviewWindow, reason: Option<&[u16]>) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        // SAFETY: called on the window's thread; `reason` is NUL-terminated (`block_reason`).
        unsafe {
            match reason {
                Some(reason) => {
                    ShutdownBlockReasonCreate(hwnd, reason.as_ptr());
                }
                None => {
                    ShutdownBlockReasonDestroy(hwnd);
                }
            }
        }
    }

    unsafe extern "system" fn procedure(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if message == WM_QUERYENDSESSION || (message == WM_ENDSESSION && wparam != 0) {
            if let Some(app) = APP.get() {
                ask_for_drafts(app.clone());
            }
            let flags = lparam as u32;
            if message == WM_QUERYENDSESSION
                && blocks_session_end(UNSAVED.load(Ordering::SeqCst), flags)
            {
                return 0;
            }
        }
        // SAFETY: the arguments Windows gave, passed on to the next procedure.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}

#[cfg(not(windows))]
mod imp {
    use tauri::{AppHandle, WebviewWindow};

    pub fn install(_app: &AppHandle, _window: &WebviewWindow) {
        let _ = super::ask_for_drafts;
    }

    pub fn set_reason(_window: &WebviewWindow, _reason: Option<&[u16]>) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The other `lParam` bits (WinUser.h): an installer's Restart Manager, a logoff.
    const ENDSESSION_CLOSEAPP: u32 = 0x0000_0001;
    const ENDSESSION_LOGOFF: u32 = 0x8000_0000;

    #[test]
    fn holds_the_end_back_only_with_unsaved_work_and_never_when_critical() {
        assert!(!blocks_session_end(false, 0));
        assert!(blocks_session_end(true, 0));
        assert!(blocks_session_end(true, ENDSESSION_LOGOFF));
        assert!(blocks_session_end(true, ENDSESSION_CLOSEAPP));
        assert!(!blocks_session_end(true, ENDSESSION_CRITICAL));
        assert!(!blocks_session_end(
            true,
            ENDSESSION_CRITICAL | ENDSESSION_LOGOFF
        ));
    }

    #[test]
    fn the_reason_is_trimmed_bounded_and_terminated() {
        assert_eq!(block_reason(None), None);
        assert_eq!(block_reason(Some("   ")), None);
        let wide = block_reason(Some("  Note non enregistrée  ")).unwrap();
        assert_eq!(
            String::from_utf16(&wide[..wide.len() - 1]).unwrap(),
            "Note non enregistrée"
        );
        assert_eq!(wide.last(), Some(&0));
        // Long text is cut at 255 units, never inside a surrogate pair.
        let long = "é".repeat(300) + &"😀".repeat(10);
        assert_eq!(block_reason(Some(&long)).unwrap().len(), 256);
        let emoji = "a".to_owned() + &"😀".repeat(200);
        let cut = block_reason(Some(&emoji)).unwrap();
        assert!(cut.len() <= 256);
        assert!(String::from_utf16(&cut[..cut.len() - 1]).is_ok());
    }
}
