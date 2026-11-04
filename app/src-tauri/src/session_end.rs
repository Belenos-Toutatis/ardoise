//! Windows ending the session (shutdown, restart for an update, logoff, an installer's Restart
//! Manager) while a note has unsaved changes (M10; `docs/design/interface.md`, "M10: drafts and
//! session end") or live mode runs (M12; `docs/design/tablet-live-classroom.md`, "Session end").
//!
//! Tao, Tauri's windowing layer, raises no `CloseRequested` for `WM_QUERYENDSESSION`: it lets
//! Windows go on, and on `WM_ENDSESSION` it ends the process at once (`std::process::exit(0)` in
//! tao 0.37's `thread_event_target_callback`, the procedure of a hidden window of its own). The
//! editor's leave dialog never shows and its unsaved edits would be gone, as would what the live
//! session has not written yet: the ink of the last seconds and the narration under way. Here:
//!
//! - while the interface reports unsaved work ([`set_unsaved_work`]) or live mode runs, the main
//!   window carries a shutdown block reason (`ShutdownBlockReasonCreate`): Windows names Ardoise and
//!   the reason on its "apps preventing shutdown" screen;
//! - the main window is subclassed ([`install`]): on `WM_QUERYENDSESSION` it asks the interface
//!   to write its drafts at once (event [`SESSION_ENDING`]) and live mode to write its pages and,
//!   while work is unsaved, answers FALSE ([`blocks_session_end`]) so that Windows waits and shows
//!   the reason. The user can go back and save, or end anyway: the draft is on disk by then. A
//!   critical end (`ENDSESSION_CRITICAL`) is never held back;
//! - tao's hidden window is subclassed too, since Windows may tell it first: the first
//!   `WM_ENDSESSION` to arrive, on either window, closes live mode ([`crate::live::session_ending`])
//!   before tao ends the process, for at most [`SESSION_END_BUDGET`].

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::AppContext;
use crate::diagnostic::report_language;
use crate::live::{Activity, LiveRuntime};

/// The event asking the main window to write its drafts now.
pub const SESSION_ENDING: &str = "session-ending";
/// Label of the main window.
const MAIN: &str = "main";
/// Longest reason Windows shows (`MAX_STR_BLOCKREASON`), in UTF-16 units.
const MAX_REASON: usize = 255;

/// How long `WM_ENDSESSION` waits for live mode to close before tao ends the process: Windows
/// gives a program about 5 s to answer before it ends it, or shows its screen (assumed: the default
/// of `WaitToKillAppTimeout`, not measured here, and a machine may set it otherwise). A pass over
/// the pages is written long before that; what may not fit is encoding a long narration, whose
/// sound is on disk by then (`narration::finish_live_ending`).
pub const SESSION_END_BUDGET: Duration = Duration::from_secs(4);

/// Runs `work` on a thread of its own and waits for it, at most `budget` (the window procedure
/// cannot go on holding Windows up). Returns whether it ended in time; otherwise it goes on until
/// the process ends. Nothing it writes can be left half done: every write is atomic.
#[cfg_attr(not(windows), allow(dead_code))]
pub fn run_within(budget: Duration, work: impl FnOnce() + Send + 'static) -> bool {
    let (done, ended) = std::sync::mpsc::channel();
    let thread = std::thread::Builder::new()
        .name("session-end".to_owned())
        .spawn(move || {
            work();
            let _ = done.send(());
        });
    match thread {
        Ok(_) => ended.recv_timeout(budget).is_ok(),
        Err(error) => {
            eprintln!("Ardoise: cannot close live mode at the end of the session: {error}");
            false
        }
    }
}

/// The `lParam` bit of `WM_QUERYENDSESSION` for an end that cannot be held back (WinUser.h).
#[cfg_attr(not(windows), allow(dead_code))]
pub const ENDSESSION_CRITICAL: u32 = 0x4000_0000;

/// Unsaved work is reported by the interface.
static UNSAVED: AtomicBool = AtomicBool::new(false);
/// Whether live mode runs, read in the window procedure: no lock, and always up to date.
static LIVE: OnceLock<Activity> = OnceLock::new();
/// What the main window's reason is made of.
static REASONS: Mutex<Reasons> = Mutex::new(Reasons {
    editor: None,
    live: None,
});

/// Why the session should not end, each as [`block_reason`] gives it.
#[derive(Debug, Default, PartialEq)]
struct Reasons {
    editor: Option<Vec<u16>>,
    live: Option<Vec<u16>>,
}

impl Reasons {
    /// The one Windows shows (the editor and live mode are never open together).
    fn shown(&self) -> Option<&[u16]> {
        self.editor.as_deref().or(self.live.as_deref())
    }
}

fn reasons() -> MutexGuard<'static, Reasons> {
    REASONS.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What Windows shows while live mode runs. Here rather than in the interface's translations: it
/// holds whatever window is open, and live mode may be running with no view at all.
fn live_reason(french: bool) -> &'static str {
    if french {
        "Le mode direct est en cours. Revenez dans Ardoise et cliquez sur Terminer pour enregistrer le cours."
    } else {
        "Live mode is running. Go back to Ardoise and click Finish to save the lesson."
    }
}

/// Whether anything would be lost if the session ended now.
#[cfg_attr(not(windows), allow(dead_code))]
fn work_unsaved() -> bool {
    UNSAVED.load(Ordering::SeqCst) || LIVE.get().is_some_and(Activity::live)
}

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
    reasons().editor = wide;
    show_reason(&app);
}

/// Gives the main window the reason of the moment.
fn show_reason(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let target = window.clone();
        // The reason belongs to the window's thread (`ShutdownBlockReasonCreate` refuses others),
        // which takes the latest one: two changes in a row cannot end in the wrong order.
        let _ = window.run_on_main_thread(move || imp::set_reason(&target, reasons().shown()));
    }
}

/// Live mode started or stopped: the reason follows (from a thread of its own, in the hub's lock:
/// the language is looked up, and the window's thread may be waiting for that lock).
fn live_changed(app: &AppHandle) {
    let app = app.clone();
    let thread = std::thread::Builder::new()
        .name("session-end-reason".to_owned())
        .spawn(move || {
            let running = LIVE.get().is_some_and(Activity::live);
            let french = app
                .try_state::<AppContext>()
                .is_some_and(|context| report_language(&context) == "fr");
            reasons().live = running
                .then(|| block_reason(Some(live_reason(french))))
                .flatten();
            show_reason(&app);
        });
    if let Err(error) = thread {
        eprintln!("Ardoise: cannot update the reason shown at the end of the session: {error}");
    }
}

/// Watches the session's end on the main window and tao's hidden one, and live mode's state (call
/// from the thread that created them).
pub fn install(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<LiveRuntime>() {
        let _ = LIVE.set(runtime.hub().activity());
        let watching = app.clone();
        runtime
            .hub()
            .watch_running(Box::new(move |_| live_changed(&watching)));
    }
    if let Some(window) = app.get_webview_window(MAIN) {
        imp::install(app, &window);
    }
}

/// Asks the main window to write its drafts now (from another thread: the window procedure
/// that calls this must return first).
#[cfg_attr(not(windows), allow(dead_code))]
fn ask_for_drafts(app: AppHandle) {
    std::thread::spawn(move || {
        use tauri::Emitter;
        let _ = app.emit_to(MAIN, SESSION_ENDING, ());
    });
}

/// Windows asks whether the session may end: the drafts and live mode's pages are written now, while
/// the window procedure answers.
#[cfg_attr(not(windows), allow(dead_code))]
fn session_may_end(app: AppHandle) {
    ask_for_drafts(app.clone());
    std::thread::spawn(move || crate::live::session_may_end(&app));
}

/// Windows ends the session: the drafts once more, and what live mode has not written, within
/// what Windows allows. Waits, on the window's thread: tao ends the process right after.
#[cfg_attr(not(windows), allow(dead_code))]
fn session_ends(app: AppHandle) {
    ask_for_drafts(app.clone());
    if !run_within(SESSION_END_BUDGET, move || {
        crate::live::session_ending(&app)
    }) {
        eprintln!("Ardoise: live mode was still closing when Windows ended the session");
    }
}

#[cfg(windows)]
mod imp {
    use std::sync::{Once, OnceLock};

    use tauri::{AppHandle, WebviewWindow};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Shutdown::{
        ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy,
    };
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumThreadWindows, GetClassNameW, WM_ENDSESSION, WM_QUERYENDSESSION,
    };
    use windows_sys::core::BOOL;

    use super::{blocks_session_end, session_ends, session_may_end, work_unsaved};

    /// Identifies our subclass among the window's (tao has its own).
    const SUBCLASS_ID: usize = 0x4152_444f;
    /// The main window: it answers `WM_QUERYENDSESSION`.
    const MAIN_WINDOW: usize = 0;
    /// Tao's hidden window: only its `WM_ENDSESSION` matters.
    const TAO_WINDOW: usize = 1;
    /// The window class of tao's hidden window (`create_event_target_window`, tao 0.37.2).
    const TAO_CLASS: &str = "Tao Thread Event Target";

    /// What the window procedure asks for.
    struct Hooks {
        /// `WM_QUERYENDSESSION`, from another thread.
        may_end: Box<dyn Fn() + Send + Sync>,
        /// The first `WM_ENDSESSION`: returns once it is done with.
        ends: Box<dyn Fn() + Send + Sync>,
    }

    static HOOKS: OnceLock<Hooks> = OnceLock::new();
    static ENDED: Once = Once::new();

    fn hwnd_of(window: &WebviewWindow) -> Option<HWND> {
        window.hwnd().ok().map(|hwnd| hwnd.0 as HWND)
    }

    pub fn install(app: &AppHandle, window: &WebviewWindow) {
        let (asking, ending) = (app.clone(), app.clone());
        let _ = HOOKS.set(Hooks {
            may_end: Box::new(move || session_may_end(asking.clone())),
            ends: Box::new(move || session_ends(ending.clone())),
        });
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        // SAFETY: `hwnd` is the live main window, on the thread that created it (Tauri's setup);
        // the procedure lives as long as the program.
        let ok = unsafe { SetWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID, MAIN_WINDOW) };
        if ok == 0 {
            eprintln!("Ardoise: the end of the Windows session cannot be watched");
        }
        // SAFETY: the callback runs here, on this thread, for the windows of this thread.
        unsafe {
            EnumThreadWindows(GetCurrentThreadId(), Some(watch_tao_window), 0);
        }
    }

    /// Tao ends the process when its hidden window gets `WM_ENDSESSION`, and nothing says that the
    /// main window hears of it first (Windows goes through the top-level windows): this subclass runs
    /// before tao's procedure. Installed on the window's own thread, as `SetWindowSubclass` requires.
    unsafe extern "system" fn watch_tao_window(hwnd: HWND, _: LPARAM) -> BOOL {
        let mut class = [0u16; 64];
        // SAFETY: the buffer's length is given; `hwnd` is a window of this thread, being listed.
        let length = unsafe { GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) };
        let length = usize::try_from(length).unwrap_or(0).min(class.len());
        if String::from_utf16_lossy(&class[..length]) == TAO_CLASS {
            // SAFETY: as above; the procedure lives as long as the program.
            unsafe { SetWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID, TAO_WINDOW) };
        }
        1
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
        window: usize,
    ) -> LRESULT {
        match message {
            WM_QUERYENDSESSION if window == MAIN_WINDOW => {
                if let Some(hooks) = HOOKS.get() {
                    (hooks.may_end)();
                }
                if blocks_session_end(work_unsaved(), lparam as u32) {
                    return 0;
                }
            }
            WM_ENDSESSION if wparam != 0 => {
                if let Some(hooks) = HOOKS.get() {
                    ENDED.call_once(|| (hooks.ends)());
                }
            }
            _ => {}
        }
        // SAFETY: the arguments Windows gave, passed on to the next procedure.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }

    /// The procedure on real windows, with messages sent as Windows sends them: no shutdown, no
    /// tao and no app, but the answers and what is asked for, on the window's thread.
    #[cfg(test)]
    mod tests {
        use std::ptr::{null, null_mut};
        use std::sync::atomic::{AtomicUsize, Ordering};

        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, SendMessageW,
        };

        use super::*;
        use crate::live::Activity;
        use crate::session_end::{ENDSESSION_CRITICAL, LIVE, UNSAVED};

        static ASKED: AtomicUsize = AtomicUsize::new(0);
        static ENDS: AtomicUsize = AtomicUsize::new(0);

        fn window(data: usize) -> HWND {
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // SAFETY: a plain hidden window of a system class, on this thread.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    null(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    null_mut(),
                    null_mut(),
                    null_mut(),
                    null(),
                )
            };
            assert!(!hwnd.is_null());
            // SAFETY: as above.
            assert_ne!(
                unsafe { SetWindowSubclass(hwnd, Some(procedure), SUBCLASS_ID, data) },
                0
            );
            hwnd
        }

        fn send(hwnd: HWND, message: u32, wparam: WPARAM, lparam: u32) -> LRESULT {
            // SAFETY: a window of this thread.
            unsafe { SendMessageW(hwnd, message, wparam, lparam as LPARAM) }
        }

        #[test]
        fn the_procedure_answers_as_windows_expects_and_asks_for_the_right_work() {
            let live = Activity::default();
            assert!(LIVE.set(live.clone()).is_ok());
            assert!(
                HOOKS
                    .set(Hooks {
                        may_end: Box::new(|| {
                            ASKED.fetch_add(1, Ordering::SeqCst);
                        }),
                        ends: Box::new(|| {
                            ENDS.fetch_add(1, Ordering::SeqCst);
                        }),
                    })
                    .is_ok()
            );
            let (main, tao) = (window(MAIN_WINDOW), window(TAO_WINDOW));

            // Nothing unsaved: Windows may go on, the work is asked for all the same.
            assert_ne!(send(main, WM_QUERYENDSESSION, 0, 0), 0);
            assert_eq!(ASKED.load(Ordering::SeqCst), 1);

            // While live mode runs, or the editor has unsaved work: held back, except a critical end.
            live.set_live(true);
            assert_eq!(send(main, WM_QUERYENDSESSION, 0, 0), 0);
            assert_ne!(send(main, WM_QUERYENDSESSION, 0, ENDSESSION_CRITICAL), 0);
            live.set_live(false);
            UNSAVED.store(true, Ordering::SeqCst);
            assert_eq!(send(main, WM_QUERYENDSESSION, 0, 0), 0);
            UNSAVED.store(false, Ordering::SeqCst);
            assert_eq!(ASKED.load(Ordering::SeqCst), 4);

            // Tao's window answers nothing itself and asks for nothing at the query.
            live.set_live(true);
            assert_ne!(send(tao, WM_QUERYENDSESSION, 0, 0), 0);
            live.set_live(false);
            assert_eq!(ASKED.load(Ordering::SeqCst), 4);

            // The end was called off: nothing happens.
            send(main, WM_ENDSESSION, 0, 0);
            send(tao, WM_ENDSESSION, 0, 0);
            assert_eq!(ENDS.load(Ordering::SeqCst), 0);

            // The end, wherever it arrives first: closed once, before the message goes on.
            send(tao, WM_ENDSESSION, 1, 0);
            assert_eq!(ENDS.load(Ordering::SeqCst), 1);
            send(main, WM_ENDSESSION, 1, 0);
            assert_eq!(ENDS.load(Ordering::SeqCst), 1);

            // SAFETY: windows of this thread, made above.
            unsafe {
                DestroyWindow(main);
                DestroyWindow(tao);
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use tauri::{AppHandle, WebviewWindow};

    pub fn install(_app: &AppHandle, _window: &WebviewWindow) {}

    pub fn set_reason(_window: &WebviewWindow, _reason: Option<&[u16]>) {}
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    use super::*;

    #[test]
    fn work_gets_the_time_allowed_and_no_more() {
        let done = Arc::new(AtomicBool::new(false));
        let finished = done.clone();
        assert!(run_within(Duration::from_secs(30), move || {
            finished.store(true, Ordering::SeqCst);
        }));
        assert!(done.load(Ordering::SeqCst));

        // Too long: the wait ends, the work goes on.
        let (go_on, waiting) = std::sync::mpsc::channel::<()>();
        let done = Arc::new(AtomicBool::new(false));
        let finished = done.clone();
        let begun = std::time::Instant::now();
        assert!(!run_within(Duration::from_millis(30), move || {
            let _ = waiting.recv();
            finished.store(true, Ordering::SeqCst);
        }));
        assert!(begun.elapsed() < Duration::from_secs(10));
        assert!(!done.load(Ordering::SeqCst));
        drop(go_on);

        // A failure in the work says it did not end, at once.
        assert!(!run_within(Duration::from_secs(30), || panic!("in a test")));
    }

    #[test]
    fn the_editors_reason_goes_first_and_the_lessons_reason_is_shown_otherwise() {
        let editor = block_reason(Some("note")).unwrap();
        let mut reasons = Reasons::default();
        assert_eq!(reasons.shown(), None);
        reasons.live = block_reason(Some(live_reason(true)));
        assert_eq!(reasons.shown(), reasons.live.as_deref());
        reasons.editor = Some(editor.clone());
        assert_eq!(reasons.shown(), Some(editor.as_slice()));
        reasons.editor = None;
        reasons.live = None;
        assert_eq!(reasons.shown(), None);
    }

    #[test]
    fn the_lessons_reason_fits_what_windows_shows_in_both_languages() {
        for french in [true, false] {
            let reason = live_reason(french);
            let wide = block_reason(Some(reason)).unwrap();
            assert_eq!(wide.len(), reason.encode_utf16().count() + 1, "not cut");
        }
        assert_ne!(live_reason(true), live_reason(false));
    }

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
