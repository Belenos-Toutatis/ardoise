//! Console output for command-line modes of a GUI-subsystem executable.

/// Release builds have no console of their own. When started from a terminal without
/// redirection, borrow the parent's console so `println!` reaches it. Redirected output
/// (pipes, files) already has valid handles and is left alone.
#[cfg(windows)]
pub fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };

    // SAFETY: plain Win32 calls without pointers; failure (no parent console) is harmless.
    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE);
        if handle.is_null() {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

#[cfg(not(windows))]
pub fn attach_parent_console() {}
