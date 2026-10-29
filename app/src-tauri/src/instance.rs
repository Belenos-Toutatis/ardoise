//! One windowed Ardoise per copy (M10; `docs/design/interface.md`, "M10: one window per copy").
//!
//! A double-click on an executable that is slow to start used to give two apps: two tablet
//! services competing for the Slate, two handwriting indexers, and the second one's bridge pipe
//! refused. The windowed app now first claims a named mutex for its copy (the hash of its
//! settings location, as in the bridge pipe's name, and the user's SID). A second launch of the
//! same copy finds its main window (a window class named after the copy), brings it to the front
//! and exits; when the first one is closing (its mutex is released meanwhile), the second one
//! goes on and becomes the app.
//!
//! Two copies in two folders (two settings files) run side by side. The command-line modes
//! (`--mcp`, `--self-test`, `--create-demo-library`, `--import-inkspace`,
//! `--diagnostic --simulated`) never claim the mutex: they keep working while the app is open.

use std::time::Duration;

use crate::{AppContext, Cli};

/// How long a second launch waits for the first one's window (it may still be starting).
pub const WAIT_FOR_WINDOW: Duration = Duration::from_secs(20);
/// How often it looks for the window meanwhile.
const POLL: Duration = Duration::from_millis(100);

/// Whether this launch is the windowed app, which claims its copy (the command-line modes do not).
pub fn guarded(cli: &Cli) -> bool {
    cli.mcp.is_none()
        && !cli.self_test
        && !(cli.diagnostic && cli.simulated)
        && cli.import_inkspace.is_none()
        && cli.create_demo_library.is_none()
}

/// The names of one copy's instance objects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceNames {
    /// The mutex, in the session's namespace (`Local\`).
    pub mutex: String,
    /// The class of the main window.
    pub class: String,
}

/// The names for the copy `copy` (its settings hash) run by `user` (a SID; anything but
/// letters, digits, `-`, `_` and `.` replaced, as in the pipe's name).
pub fn names(user: &str, copy: &str) -> InstanceNames {
    let user: String = user
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    InstanceNames {
        mutex: format!(r"Local\ardoise-instance-{user}-{copy}"),
        class: format!("Ardoise-{copy}"),
    }
}

/// The names of the running copy; `None` (no guard) when its settings location or the user
/// cannot be told.
pub fn names_for(context: &AppContext) -> Option<InstanceNames> {
    use ardoise_mcp::bridge::pipe::{current_user_sid, settings_hash, settings_key};
    let store = context.store.as_ref().ok()?;
    let user = current_user_sid().ok()?;
    Some(names(&user, &settings_hash(&settings_key(store.path()))))
}

/// What a launch found.
pub enum Launch {
    /// This is the app of its copy; keep the guard as long as the process runs.
    First(Option<imp::Guard>),
    /// The copy's app was already running: its window was brought to the front (or it is still
    /// starting). Exit.
    AlreadyRunning,
}

/// Claims the copy named `names` for this process.
pub fn claim(names: &InstanceNames) -> Launch {
    imp::claim(names)
}

/// What a second launch does on each look (pure, for the tests): `Show` once the first window
/// exists, `TakeOver` when the first app went away, `Wait` until the deadline, then `GiveUp`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    Show,
    TakeOver,
    Wait,
    GiveUp,
}

pub fn next_step(window_found: bool, released: bool, waited: Duration) -> Step {
    if window_found {
        Step::Show
    } else if released {
        Step::TakeOver
    } else if waited >= WAIT_FOR_WINDOW {
        Step::GiveUp
    } else {
        Step::Wait
    }
}

#[cfg(windows)]
pub mod imp {
    use std::time::Instant;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HWND, WAIT_ABANDONED,
        WAIT_OBJECT_0,
    };
    use windows_sys::Win32::System::Threading::{CreateMutexW, ReleaseMutex, WaitForSingleObject};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow,
    };

    use super::{InstanceNames, Launch, POLL, Step, next_step};

    /// The copy's mutex, owned by this process until it ends.
    pub struct Guard(HANDLE);

    impl Drop for Guard {
        fn drop(&mut self) {
            // SAFETY: a handle this process opened; releasing fails harmlessly on a thread that
            // does not own the mutex (the process ending abandons it anyway).
            unsafe {
                ReleaseMutex(self.0);
                CloseHandle(self.0);
            }
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    pub fn claim(names: &InstanceNames) -> Launch {
        let mutex = wide(&names.mutex);
        // SAFETY: a NUL-terminated name; default security (this user).
        let handle = unsafe { CreateMutexW(std::ptr::null(), 1, mutex.as_ptr()) };
        if handle.is_null() {
            // Not ours to take (another user's object of the same name): no guard.
            return Launch::First(None);
        }
        // SAFETY: read right after the call that set it.
        if unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
            return Launch::First(Some(Guard(handle)));
        }
        let guard = Guard(handle);
        let class = wide(&names.class);
        let start = Instant::now();
        loop {
            // SAFETY: a NUL-terminated class name; no title filter.
            let window: HWND = unsafe { FindWindowW(class.as_ptr(), std::ptr::null()) };
            let released = window.is_null() && {
                // SAFETY: our handle to the mutex; waits at most one poll.
                let wait = unsafe { WaitForSingleObject(guard.0, POLL.as_millis() as u32) };
                wait == WAIT_OBJECT_0 || wait == WAIT_ABANDONED
            };
            match next_step(!window.is_null(), released, start.elapsed()) {
                Step::Show => {
                    show(window);
                    return Launch::AlreadyRunning;
                }
                Step::TakeOver => return Launch::First(Some(guard)),
                Step::GiveUp => return Launch::AlreadyRunning,
                Step::Wait => {}
            }
        }
    }

    /// Restores the window when minimised and brings it to the front (this process was just
    /// started by the user, so Windows lets it give the focus away).
    fn show(window: HWND) {
        // SAFETY: a window handle found a moment ago; these calls fail harmlessly if it is gone.
        unsafe {
            if IsIconic(window) != 0 {
                ShowWindow(window, SW_RESTORE);
            } else {
                ShowWindow(window, SW_SHOW);
            }
            SetForegroundWindow(window);
        }
    }
}

#[cfg(not(windows))]
pub mod imp {
    use super::{InstanceNames, Launch};

    /// No guard outside Windows (the app is built there for the tests only).
    pub struct Guard;

    pub fn claim(_names: &InstanceNames) -> Launch {
        Launch::First(None)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::mcp::McpArgs;

    fn parse(args: &[&str]) -> Cli {
        Cli::parse(args.iter().map(|a| a.to_string()))
    }

    #[test]
    fn only_the_windowed_app_claims_its_copy() {
        assert!(guarded(&parse(&[])));
        assert!(guarded(&parse(&["--diagnostic"])));
        assert!(guarded(&parse(&["--simulated-tablet"])));
        assert!(!guarded(&parse(&["--mcp"])));
        assert!(!guarded(&parse(&["--mcp", "--read-only"])));
        assert!(!guarded(&parse(&["--self-test"])));
        assert!(!guarded(&parse(&["--diagnostic", "--simulated"])));
        assert!(!guarded(&parse(&["--import-inkspace"])));
        assert!(!guarded(&parse(&["--create-demo-library", "D:\\Démo"])));
        assert!(!guarded(&Cli {
            mcp: Some(McpArgs { read_only: true }),
            ..Cli::default()
        }));
        assert!(!guarded(&Cli {
            create_demo_library: Some(PathBuf::new()),
            ..Cli::default()
        }));
    }

    #[test]
    fn names_follow_the_copy_and_the_user() {
        let a = names("S-1-5-21-1-2-3-1001", "0badcafe");
        assert_eq!(
            a.mutex,
            r"Local\ardoise-instance-S-1-5-21-1-2-3-1001-0badcafe"
        );
        assert_eq!(a.class, "Ardoise-0badcafe");
        // Another copy (another settings location): other names, so both run.
        let b = names("S-1-5-21-1-2-3-1001", "12345678");
        assert_ne!(a.mutex, b.mutex);
        assert_ne!(a.class, b.class);
        // Another user: another mutex.
        assert_ne!(a.mutex, names("S-1-5-21-1-2-3-1002", "0badcafe").mutex);
        // Nothing but plain characters reaches the object's name.
        let odd = names(r"S-1\5/x y", "0badcafe");
        assert_eq!(odd.mutex, r"Local\ardoise-instance-S-1_5_x_y-0badcafe");
    }

    #[test]
    fn the_copy_hash_differs_between_two_folders() {
        use ardoise_mcp::bridge::pipe::{settings_hash, settings_key_of};
        let one = settings_hash(&settings_key_of(r"C:\Users\prof\Ardoise\ardoise.toml"));
        let same = settings_hash(&settings_key_of(r"c:/users/PROF/ardoise/ARDOISE.toml"));
        let other = settings_hash(&settings_key_of(r"E:\Clé\Ardoise\ardoise.toml"));
        assert_eq!(one, same);
        assert_ne!(one, other);
    }

    #[test]
    fn a_second_launch_shows_the_first_window_or_takes_over() {
        let soon = Duration::from_millis(300);
        assert_eq!(next_step(true, false, soon), Step::Show);
        assert_eq!(next_step(true, true, soon), Step::Show);
        assert_eq!(next_step(false, true, soon), Step::TakeOver);
        assert_eq!(next_step(false, false, soon), Step::Wait);
        assert_eq!(next_step(false, false, WAIT_FOR_WINDOW), Step::GiveUp);
        assert_eq!(next_step(false, true, WAIT_FOR_WINDOW), Step::TakeOver);
    }

    #[cfg(windows)]
    #[test]
    fn the_mutex_lets_one_process_in_per_copy() {
        // A name of its own, so that no running Ardoise is involved.
        let names = InstanceNames {
            mutex: format!(r"Local\ardoise-instance-test-{}", uuid::Uuid::new_v4()),
            class: format!("Ardoise-test-{}", uuid::Uuid::new_v4()),
        };
        // Mutexes are owned per thread, and a process that ends abandons its own: each claim
        // runs on a thread of its own, the first one ending as the first app would.
        let (release, released) = std::sync::mpsc::channel::<()>();
        let (claimed, first_claimed) = std::sync::mpsc::channel::<bool>();
        let first_names = names.clone();
        let first = std::thread::spawn(move || {
            let launch = claim(&first_names);
            claimed
                .send(matches!(launch, Launch::First(Some(_))))
                .unwrap();
            released.recv().unwrap();
        });
        assert!(first_claimed.recv().unwrap(), "the first claim must win");
        // No window of that class exists: the second claim waits until the first one ends.
        let second_names = names.clone();
        let second =
            std::thread::spawn(move || matches!(claim(&second_names), Launch::First(Some(_))));
        std::thread::sleep(Duration::from_millis(400));
        assert!(
            !second.is_finished(),
            "the second claim must wait for the first"
        );
        release.send(()).unwrap();
        first.join().unwrap();
        assert!(second.join().unwrap(), "the second claim takes over");
    }
}
