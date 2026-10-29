//! The named pipe of the bridge: the open app serves it ([`PipeServer`]), `ardoise.exe --mcp`
//! connects to it for each request ([`PipeClient`]). Lines of [`super::protocol`] go through it.
//!
//! Security (another process of the user drives the app through it):
//!
//! - the pipe's DACL grants access to the current user's SID only, and denies network logons;
//! - `PIPE_REJECT_REMOTE_CLIENTS`: no client from another computer;
//! - `FILE_FLAG_FIRST_PIPE_INSTANCE` on the first instance, and the next instance is always
//!   created before the previous one is handed over, so the name is never free: the app never
//!   serves (or adds instances to) a pipe that another process created first;
//! - the client checks that the process serving the pipe runs as the same user, and connects
//!   with `SECURITY_IDENTIFICATION` (the server cannot act as the client);
//! - lines are bounded ([`MAX_LINE`]), every wait has a timeout, a connection idle for
//!   [`IDLE`] is closed and at most [`MAX_CONNECTIONS`] are served at once.
//!
//! The name ([`pipe_name`]) carries the user's SID and a short stable hash of the settings file
//! of the copy of Ardoise, so that `--mcp` started from one portable copy talks to the app
//! started from that same copy (the e2e journeys run copies side by side with the user's own).
//!
//! Outside Windows there is no bridge: [`PipeServer::start`] and [`PipeClient::connect`] answer
//! [`PipeError::Unsupported`].

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use imp::{PipeClient, PipeServer, current_user_sid};

use super::protocol::MAX_LINE;

/// Connections served at once; one more gets an `appBusy` answer.
pub const MAX_CONNECTIONS: usize = 4;
/// A connection that sends no request for this long is closed.
pub const IDLE: Duration = Duration::from_secs(30);
/// Longest wait to send an answer.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// A failure of the pipe.
#[derive(Debug, thiserror::Error)]
pub enum PipeError {
    /// Nobody serves the pipe.
    #[error("Ardoise is not open")]
    NotRunning,
    /// The first instance could not be created: another process serves this name already.
    #[error("another process already serves {0}")]
    AlreadyServed(String),
    /// The process serving the pipe is not one of this user.
    #[error("the pipe is not served by an Ardoise of this user: {0}")]
    Foreign(String),
    #[error("no answer in time")]
    Timeout,
    #[error("a line longer than {MAX_LINE} bytes")]
    TooLong,
    #[error("the assistant bridge exists on Windows only")]
    Unsupported,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// One rule of the pipe's DACL, read back from Windows (the tests check them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessRule {
    /// `true` for an access-allowed rule, `false` for an access-denied one.
    pub allow: bool,
    /// The SID as text (`S-1-5-…`).
    pub sid: String,
    pub mask: u32,
}

/// FNV-1a, 64 bits: a hash that never changes between builds (unlike `DefaultHasher`).
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, &byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// The settings location as compared between copies: `\\?\` dropped, `/` as `\`, lower case
/// (Windows paths ignore case).
pub fn settings_key_of(path: &str) -> String {
    let path = path
        .strip_prefix(r"\\?\UNC\")
        .map(|rest| format!(r"\\{rest}"))
        .unwrap_or_else(|| path.strip_prefix(r"\\?\").unwrap_or(path).to_owned());
    path.replace('/', "\\").to_lowercase()
}

/// `\\.\pipe\ardoise-<user>-<hash>`: `user` (the SID) with anything but letters, digits, `-`,
/// `_` and `.` replaced, and 8 hex digits of the hash of `settings_key`.
pub fn pipe_name(user: &str, settings_key: &str) -> String {
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
    let hash = fnv1a64(settings_key.as_bytes());
    // The two halves folded: 8 hex digits are plenty for the few copies of one user.
    let short = (hash ^ (hash >> 32)) as u32;
    format!(r"\\.\pipe\ardoise-{user}-{short:08x}")
}

/// The settings file as a stable key: its folder resolved by the file system (true case, long
/// names, links followed) when it exists, then [`settings_key_of`].
pub fn settings_key(settings: &Path) -> String {
    let resolved = match (settings.parent(), settings.file_name()) {
        (Some(dir), Some(file)) => std::fs::canonicalize(dir)
            .map(|dir| dir.join(file))
            .unwrap_or_else(|_| settings.to_path_buf()),
        _ => PathBuf::from(settings),
    };
    settings_key_of(&resolved.to_string_lossy())
}

/// The pipe of the copy of Ardoise whose settings file is `settings`, for the current user.
pub fn pipe_name_for(settings: &Path) -> Result<String, PipeError> {
    Ok(pipe_name(&current_user_sid()?, &settings_key(settings)))
}

#[cfg(windows)]
mod imp {
    use std::io;
    use std::ptr::{null, null_mut};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread::JoinHandle;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, ERROR_BROKEN_PIPE, ERROR_FILE_NOT_FOUND,
        ERROR_IO_PENDING, ERROR_NO_DATA, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
        ERROR_PIPE_NOT_CONNECTED, ERROR_SEM_TIMEOUT, GENERIC_READ, GENERIC_WRITE, GetLastError,
        HANDLE, INVALID_HANDLE_VALUE, LocalFree, WAIT_OBJECT_0, WAIT_TIMEOUT,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        GetSecurityInfo, SDDL_REVISION_1, SE_KERNEL_OBJECT,
    };
    use windows_sys::Win32::Security::{
        ACCESS_ALLOWED_ACE, ACE_HEADER, ACL, ACL_SIZE_INFORMATION, AclSizeInformation,
        DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetAclInformation, GetLengthSid,
        GetTokenInformation, PSECURITY_DESCRIPTOR, PSID, SECURITY_ATTRIBUTES, TOKEN_QUERY,
        TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAG_FIRST_PIPE_INSTANCE, FILE_FLAG_OVERLAPPED, OPEN_EXISTING,
        PIPE_ACCESS_DUPLEX, ReadFile, SECURITY_IDENTIFICATION, SECURITY_SQOS_PRESENT, WriteFile,
    };
    use windows_sys::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, GetNamedPipeServerProcessId,
        PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES,
        PIPE_WAIT, WaitNamedPipeW,
    };
    use windows_sys::Win32::System::Threading::{
        CreateEventW, GetCurrentProcess, INFINITE, OpenProcess, OpenProcessToken,
        PROCESS_QUERY_LIMITED_INFORMATION, SetEvent, WaitForMultipleObjects,
    };

    use super::{AccessRule, IDLE, MAX_CONNECTIONS, PipeError, WRITE_TIMEOUT};
    use crate::bridge::AppBridge;
    use crate::bridge::protocol::{LineBuffer, answer_line, too_long_line};
    use crate::error::code;

    /// Access-allowed and access-denied ACE types (`winnt.h`).
    const ACCESS_ALLOWED_ACE_TYPE: u8 = 0;
    const ACCESS_DENIED_ACE_TYPE: u8 = 1;
    /// Buffer sizes of each pipe instance, bytes.
    const PIPE_BUFFER: u32 = 64 * 1024;

    /// A kernel handle, closed when dropped.
    struct Owned(HANDLE);

    // SAFETY: a kernel handle is a process-wide value; it may be used and closed from any thread.
    unsafe impl Send for Owned {}
    // SAFETY: as above; the operations used on shared handles (events) are thread-safe.
    unsafe impl Sync for Owned {}

    impl Drop for Owned {
        fn drop(&mut self) {
            if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
                // SAFETY: the handle is owned by this value and closed once.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    /// Memory that Windows allocated with `LocalAlloc`, freed when dropped.
    struct Local(*mut core::ffi::c_void);

    // SAFETY: the block is not tied to a thread; it is only read after creation.
    unsafe impl Send for Local {}
    // SAFETY: as above.
    unsafe impl Sync for Local {}

    impl Drop for Local {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the block was allocated by Windows with LocalAlloc and is freed once.
                unsafe { LocalFree(self.0) };
            }
        }
    }

    /// A SID copied out of a token (u32 words: SIDs need that alignment).
    struct Sid(Vec<u32>);

    impl Sid {
        fn ptr(&self) -> PSID {
            self.0.as_ptr() as PSID
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn last_error() -> u32 {
        // SAFETY: reads the calling thread's last error.
        unsafe { GetLastError() }
    }

    fn os_error(code: u32) -> io::Error {
        io::Error::from_raw_os_error(code as i32)
    }

    fn manual_event() -> io::Result<Owned> {
        // SAFETY: plain call; a null name and default security make an unnamed event.
        let event = unsafe { CreateEventW(null(), 1, 0, null()) };
        if event.is_null() {
            return Err(io::Error::last_os_error());
        }
        Ok(Owned(event))
    }

    /// The user of the token of `process`.
    fn token_user(process: HANDLE) -> io::Result<Sid> {
        let mut token: HANDLE = null_mut();
        // SAFETY: `process` is a process handle with query access; `token` receives a handle.
        if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Owned(token);
        let mut length = 0u32;
        // SAFETY: a size query (null buffer of length 0).
        unsafe { GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut length) };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        // u64 words: TOKEN_USER holds a pointer.
        let mut buffer = vec![0u64; (length as usize).div_ceil(8)];
        // SAFETY: the buffer has `length` bytes, aligned for TOKEN_USER.
        let ok = unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                buffer.as_mut_ptr().cast(),
                length,
                &mut length,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: Windows filled a TOKEN_USER at the start of the buffer; its SID points into
        // the buffer, which lives until the copy below is made.
        let sid = unsafe { (*buffer.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        // SAFETY: `sid` is a valid SID.
        let size = unsafe { GetLengthSid(sid) } as usize;
        let mut copy = vec![0u32; size.div_ceil(4)];
        // SAFETY: `size` bytes are readable at `sid` and writable in `copy`.
        unsafe { std::ptr::copy_nonoverlapping(sid.cast::<u8>(), copy.as_mut_ptr().cast(), size) };
        Ok(Sid(copy))
    }

    fn current_user() -> io::Result<Sid> {
        // SAFETY: the pseudo-handle of the current process needs no closing.
        token_user(unsafe { GetCurrentProcess() })
    }

    /// `S-1-5-…` of `sid`.
    fn sid_text(sid: PSID) -> io::Result<String> {
        let mut text: *mut u16 = null_mut();
        // SAFETY: `sid` is a valid SID; Windows allocates the text with LocalAlloc.
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
            return Err(io::Error::last_os_error());
        }
        let _free = Local(text.cast());
        let mut length = 0;
        // SAFETY: the text ends with a NUL.
        while unsafe { *text.add(length) } != 0 {
            length += 1;
        }
        // SAFETY: `length` characters are readable at `text`.
        let units = unsafe { std::slice::from_raw_parts(text, length) };
        Ok(String::from_utf16_lossy(units))
    }

    /// The SID of the user running this process, as text.
    pub fn current_user_sid() -> Result<String, PipeError> {
        let user = current_user()?;
        Ok(sid_text(user.ptr())?)
    }

    /// The security descriptor of the pipe: denied to network logons, allowed to `sid` only;
    /// `P` (protected): nothing inherited.
    fn user_only(sid: &str) -> io::Result<Local> {
        let sddl = wide(&format!("D:P(D;;GA;;;NU)(A;;GA;;;{sid})"));
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: a NUL-ended SDDL string; Windows allocates the descriptor with LocalAlloc.
        let ok = unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Local(descriptor))
    }

    fn create_instance(name: &[u16], descriptor: &Local, first: bool) -> io::Result<Owned> {
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let open_mode = PIPE_ACCESS_DUPLEX
            | FILE_FLAG_OVERLAPPED
            | if first {
                FILE_FLAG_FIRST_PIPE_INSTANCE
            } else {
                0
            };
        // SAFETY: `name` is NUL-ended; the attributes and their descriptor outlive the call.
        let pipe = unsafe {
            CreateNamedPipeW(
                name.as_ptr(),
                open_mode,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                PIPE_BUFFER,
                PIPE_BUFFER,
                0,
                &attributes,
            )
        };
        if pipe == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(Owned(pipe))
    }

    /// How an overlapped operation ended.
    enum Wait {
        /// Done, with the number of bytes moved.
        Done(u32),
        TimedOut,
        /// The stop event was set first.
        Stopped,
    }

    /// Runs the overlapped operation that `start` begins on `handle` and waits for it, at most
    /// `timeout` (none: no limit) or until `stop` is set; a wait cut short cancels the operation
    /// and waits for the cancellation, so the buffers given to `start` are never used after.
    fn overlapped(
        handle: HANDLE,
        timeout: Option<Duration>,
        stop: Option<HANDLE>,
        start: impl FnOnce(*mut OVERLAPPED) -> i32,
    ) -> io::Result<Wait> {
        let event = manual_event()?;
        // SAFETY: an all-zero OVERLAPPED is the documented initial value.
        let mut operation: OVERLAPPED = unsafe { std::mem::zeroed() };
        operation.hEvent = event.0;
        let mut moved = 0u32;
        if start(&mut operation) == 0 {
            match last_error() {
                // A client connected between CreateNamedPipe and ConnectNamedPipe.
                ERROR_PIPE_CONNECTED => return Ok(Wait::Done(0)),
                ERROR_IO_PENDING => {}
                error => return Err(os_error(error)),
            }
            let handles = [event.0, stop.unwrap_or(null_mut())];
            let count = if stop.is_some() { 2 } else { 1 };
            let millis = timeout.map_or(INFINITE, |t| {
                u32::try_from(t.as_millis()).map_or(INFINITE - 1, |ms| ms.min(INFINITE - 1))
            });
            // SAFETY: `count` valid event handles.
            let waited = unsafe { WaitForMultipleObjects(count, handles.as_ptr(), 0, millis) };
            if waited != WAIT_OBJECT_0 {
                // SAFETY: cancels this very operation, then waits until Windows is done with it.
                unsafe {
                    CancelIoEx(handle, &operation);
                    GetOverlappedResult(handle, &operation, &mut moved, 1);
                }
                return match waited {
                    WAIT_TIMEOUT => Ok(Wait::TimedOut),
                    w if w == WAIT_OBJECT_0 + 1 => Ok(Wait::Stopped),
                    _ => Err(io::Error::last_os_error()),
                };
            }
        }
        // SAFETY: the operation is complete (its event is set).
        if unsafe { GetOverlappedResult(handle, &operation, &mut moved, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Wait::Done(moved))
    }

    fn remaining(deadline: Instant) -> Result<Duration, PipeError> {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            Err(PipeError::Timeout)
        } else {
            Ok(left)
        }
    }

    /// The next line from `handle`; `None` at the end of the stream (or when `stop` is set).
    fn read_line(
        handle: HANDLE,
        buffer: &mut LineBuffer,
        deadline: Instant,
        stop: Option<HANDLE>,
    ) -> Result<Option<Vec<u8>>, PipeError> {
        let mut chunk = vec![0u8; 8 * 1024];
        loop {
            match buffer.next_line() {
                Ok(Some(line)) => return Ok(Some(line)),
                Ok(None) => {}
                Err(_) => return Err(PipeError::TooLong),
            }
            let left = remaining(deadline)?;
            let read = overlapped(handle, Some(left), stop, |operation| {
                // SAFETY: `chunk` outlives the operation (`overlapped` waits for its end).
                unsafe {
                    ReadFile(
                        handle,
                        chunk.as_mut_ptr(),
                        chunk.len() as u32,
                        null_mut(),
                        operation,
                    )
                }
            });
            match read {
                Ok(Wait::Done(count)) => buffer.push(&chunk[..count as usize]),
                Ok(Wait::TimedOut) => return Err(PipeError::Timeout),
                Ok(Wait::Stopped) => return Ok(None),
                Err(error)
                    if matches!(
                        error.raw_os_error().map(|c| c as u32),
                        Some(ERROR_BROKEN_PIPE | ERROR_PIPE_NOT_CONNECTED | ERROR_NO_DATA)
                    ) =>
                {
                    return Ok(None);
                }
                Err(error) => return Err(error.into()),
            }
        }
    }

    fn write_all(handle: HANDLE, data: &[u8], deadline: Instant) -> Result<(), PipeError> {
        let mut written = 0;
        while written < data.len() {
            let left = remaining(deadline)?;
            let rest = &data[written..];
            let wrote = overlapped(handle, Some(left), None, |operation| {
                // SAFETY: `data` outlives the operation (`overlapped` waits for its end).
                unsafe {
                    WriteFile(
                        handle,
                        rest.as_ptr(),
                        rest.len() as u32,
                        null_mut(),
                        operation,
                    )
                }
            })?;
            match wrote {
                Wait::Done(count) => written += count as usize,
                Wait::TimedOut | Wait::Stopped => return Err(PipeError::Timeout),
            }
        }
        Ok(())
    }

    /// The pipe server of the open app: one thread accepts connections, each connection is
    /// served by a thread of its own. Stopped when dropped.
    pub struct PipeServer {
        name: String,
        stop: Arc<Owned>,
        thread: Option<JoinHandle<()>>,
    }

    impl PipeServer {
        /// Creates the pipe `name` (its first instance: [`PipeError::AlreadyServed`] when
        /// another process holds the name) and serves `bridge` on it until stopped.
        pub fn start(name: &str, bridge: Arc<dyn AppBridge>) -> Result<PipeServer, PipeError> {
            let descriptor = user_only(&current_user_sid()?)?;
            let wide_name = wide(name);
            let first =
                create_instance(&wide_name, &descriptor, true).map_err(|error| {
                    match error.raw_os_error().map(|c| c as u32) {
                        Some(ERROR_ACCESS_DENIED | ERROR_PIPE_BUSY) => {
                            PipeError::AlreadyServed(name.to_owned())
                        }
                        _ => PipeError::Io(error),
                    }
                })?;
            let stop = Arc::new(manual_event()?);
            let accept = Accept {
                name: wide_name,
                descriptor,
                stop: Arc::clone(&stop),
                bridge,
                active: Arc::new(AtomicUsize::new(0)),
            };
            let thread = std::thread::Builder::new()
                .name("ardoise-bridge".into())
                .spawn(move || accept.run(first))?;
            Ok(PipeServer {
                name: name.to_owned(),
                stop,
                thread: Some(thread),
            })
        }

        pub fn name(&self) -> &str {
            &self.name
        }

        /// Stops accepting connections and ends those under way at their next read.
        pub fn stop(mut self) {
            self.shutdown();
        }

        fn shutdown(&mut self) {
            // SAFETY: a valid event handle.
            unsafe { SetEvent(self.stop.0) };
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    impl Drop for PipeServer {
        fn drop(&mut self) {
            self.shutdown();
        }
    }

    /// The accepting thread's state.
    struct Accept {
        name: Vec<u16>,
        descriptor: Local,
        stop: Arc<Owned>,
        bridge: Arc<dyn AppBridge>,
        active: Arc<AtomicUsize>,
    }

    impl Accept {
        fn run(self, first: Owned) {
            let mut waiting = first;
            loop {
                let connected = overlapped(waiting.0, None, Some(self.stop.0), |operation| {
                    // SAFETY: `waiting` is a pipe instance opened for overlapped I/O.
                    unsafe { ConnectNamedPipe(waiting.0, operation) }
                });
                match connected {
                    Ok(Wait::Done(_)) => {}
                    Ok(Wait::Stopped) => return,
                    Ok(Wait::TimedOut) => continue,
                    Err(error) if error.raw_os_error() == Some(ERROR_NO_DATA as i32) => {
                        // A client came and went before being accepted: the instance is reused.
                        // SAFETY: a server end of a pipe.
                        unsafe { DisconnectNamedPipe(waiting.0) };
                        continue;
                    }
                    Err(error) => {
                        eprintln!("assistant bridge: cannot accept a connection: {error}");
                        return;
                    }
                }
                // The next instance exists before this one is handed over: the name is never
                // free for another process.
                let next = match create_instance(&self.name, &self.descriptor, false) {
                    Ok(next) => next,
                    Err(error) => {
                        eprintln!("assistant bridge: cannot create the next instance: {error}");
                        serve(&waiting, &*self.bridge, &self.stop);
                        return;
                    }
                };
                let connection = std::mem::replace(&mut waiting, next);
                if self.active.load(Ordering::SeqCst) >= MAX_CONNECTIONS {
                    let busy = crate::bridge::protocol::encode(&busy_response());
                    let _ = write_all(connection.0, &busy, Instant::now() + WRITE_TIMEOUT);
                    continue;
                }
                self.active.fetch_add(1, Ordering::SeqCst);
                let bridge = Arc::clone(&self.bridge);
                let stop = Arc::clone(&self.stop);
                let active = Arc::clone(&self.active);
                let finished = Arc::clone(&self.active);
                let spawned = std::thread::Builder::new()
                    .name("ardoise-bridge-connection".into())
                    .spawn(move || {
                        serve(&connection, &*bridge, &stop);
                        active.fetch_sub(1, Ordering::SeqCst);
                    });
                if let Err(error) = spawned {
                    finished.fetch_sub(1, Ordering::SeqCst);
                    eprintln!("assistant bridge: cannot start a connection thread: {error}");
                }
            }
        }
    }

    fn busy_response() -> crate::bridge::protocol::Response {
        crate::bridge::protocol::Response {
            v: crate::bridge::protocol::PROTOCOL_VERSION,
            id: 0,
            ok: None,
            error: Some(crate::bridge::protocol::WireError {
                code: code::APP_BUSY.to_owned(),
                message: "Ardoise is serving other requests: try again".to_owned(),
            }),
        }
    }

    /// Answers the requests of one connection until it closes, stays idle for [`IDLE`], sends
    /// a line that is too long, or the server stops.
    fn serve(connection: &Owned, bridge: &dyn AppBridge, stop: &Owned) {
        let mut buffer = LineBuffer::new();
        loop {
            let line = match read_line(
                connection.0,
                &mut buffer,
                Instant::now() + IDLE,
                Some(stop.0),
            ) {
                Ok(Some(line)) => line,
                Err(PipeError::TooLong) => {
                    let _ = write_all(
                        connection.0,
                        &too_long_line(),
                        Instant::now() + WRITE_TIMEOUT,
                    );
                    return;
                }
                Ok(None) | Err(_) => return,
            };
            let answer = answer_line(&line, bridge);
            if write_all(connection.0, &answer, Instant::now() + WRITE_TIMEOUT).is_err() {
                return;
            }
        }
    }

    /// A connection to the open app's pipe.
    pub struct PipeClient {
        pipe: Owned,
        buffer: LineBuffer,
    }

    impl PipeClient {
        /// Connects to `name`, waiting at most `timeout` while every instance is busy:
        /// [`PipeError::NotRunning`] when nobody serves it, [`PipeError::Foreign`] when the
        /// serving process is not one of this user.
        pub fn connect(name: &str, timeout: Duration) -> Result<PipeClient, PipeError> {
            let wide_name = wide(name);
            let deadline = Instant::now() + timeout;
            loop {
                // SAFETY: a NUL-ended name; no template, default security.
                let pipe = unsafe {
                    CreateFileW(
                        wide_name.as_ptr(),
                        GENERIC_READ | GENERIC_WRITE,
                        0,
                        null(),
                        OPEN_EXISTING,
                        FILE_FLAG_OVERLAPPED | SECURITY_SQOS_PRESENT | SECURITY_IDENTIFICATION,
                        null_mut(),
                    )
                };
                if pipe != INVALID_HANDLE_VALUE {
                    let pipe = Owned(pipe);
                    check_server(pipe.0)?;
                    return Ok(PipeClient {
                        pipe,
                        buffer: LineBuffer::new(),
                    });
                }
                match last_error() {
                    ERROR_FILE_NOT_FOUND => return Err(PipeError::NotRunning),
                    ERROR_ACCESS_DENIED => {
                        return Err(PipeError::Foreign("access to the pipe is denied".into()));
                    }
                    ERROR_PIPE_BUSY | ERROR_SEM_TIMEOUT => {
                        let left = remaining(deadline)?;
                        let millis = u32::try_from(left.as_millis()).unwrap_or(u32::MAX).max(1);
                        // SAFETY: a NUL-ended name. A failure (time out, pipe gone) is seen by
                        // the next CreateFileW.
                        unsafe { WaitNamedPipeW(wide_name.as_ptr(), millis) };
                    }
                    error => return Err(os_error(error).into()),
                }
            }
        }

        /// Sends `line` (ended with `\n`) and returns the answer line (without its `\n`), all
        /// within `timeout`.
        pub fn exchange(&mut self, line: &[u8], timeout: Duration) -> Result<Vec<u8>, PipeError> {
            let deadline = Instant::now() + timeout;
            write_all(self.pipe.0, line, deadline)?;
            match read_line(self.pipe.0, &mut self.buffer, deadline, None)? {
                Some(answer) => Ok(answer),
                None => Err(PipeError::Io(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Ardoise closed the connection without answering",
                ))),
            }
        }

        /// Sends raw bytes (the tests send malformed requests).
        pub fn send_raw(&mut self, bytes: &[u8], timeout: Duration) -> Result<(), PipeError> {
            write_all(self.pipe.0, bytes, Instant::now() + timeout)
        }

        /// Reads one line (the tests).
        pub fn read_line(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>, PipeError> {
            read_line(
                self.pipe.0,
                &mut self.buffer,
                Instant::now() + timeout,
                None,
            )
        }

        /// The rules of the pipe's DACL, as Windows reports them.
        pub fn access_rules(&self) -> Result<Vec<AccessRule>, PipeError> {
            Ok(access_rules(self.pipe.0)?)
        }
    }

    /// The process serving `pipe` must run as the current user: a pipe squatted by another
    /// user's process is refused.
    fn check_server(pipe: HANDLE) -> Result<(), PipeError> {
        let mut pid = 0u32;
        // SAFETY: a client end of a pipe.
        if unsafe { GetNamedPipeServerProcessId(pipe, &mut pid) } == 0 {
            return Err(PipeError::Foreign(format!(
                "cannot tell which process serves it: {}",
                io::Error::last_os_error()
            )));
        }
        // SAFETY: plain call; the handle is closed by `Owned`.
        let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if process.is_null() {
            return Err(PipeError::Foreign(format!(
                "cannot read process {pid}: {}",
                io::Error::last_os_error()
            )));
        }
        let process = Owned(process);
        let theirs = token_user(process.0).map_err(|error| {
            PipeError::Foreign(format!("cannot read the user of process {pid}: {error}"))
        })?;
        let ours = current_user()?;
        // SAFETY: two valid SIDs.
        if unsafe { EqualSid(ours.ptr(), theirs.ptr()) } == 0 {
            return Err(PipeError::Foreign(format!(
                "process {pid} runs as another user"
            )));
        }
        Ok(())
    }

    /// The DACL of the kernel object behind `handle`.
    fn access_rules(handle: HANDLE) -> io::Result<Vec<AccessRule>> {
        let mut dacl: *mut ACL = null_mut();
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        // SAFETY: `handle` has READ_CONTROL (opened for reading); Windows allocates the
        // descriptor, which `dacl` points into.
        let status = unsafe {
            GetSecurityInfo(
                handle,
                SE_KERNEL_OBJECT,
                DACL_SECURITY_INFORMATION,
                null_mut(),
                null_mut(),
                &mut dacl,
                null_mut(),
                &mut descriptor,
            )
        };
        if status != 0 {
            return Err(os_error(status));
        }
        let _free = Local(descriptor);
        if dacl.is_null() {
            // A null DACL lets everyone in: reported as such.
            return Ok(vec![AccessRule {
                allow: true,
                sid: "S-1-1-0".into(),
                mask: u32::MAX,
            }]);
        }
        // SAFETY: an all-zero ACL_SIZE_INFORMATION is valid.
        let mut size: ACL_SIZE_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `dacl` is valid; the buffer is an ACL_SIZE_INFORMATION.
        let ok = unsafe {
            GetAclInformation(
                dacl,
                (&mut size as *mut ACL_SIZE_INFORMATION).cast(),
                std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
                AclSizeInformation,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        let mut rules = Vec::new();
        for index in 0..size.AceCount {
            let mut ace: *mut core::ffi::c_void = null_mut();
            // SAFETY: `index` is below the ACE count.
            if unsafe { GetAce(dacl, index, &mut ace) } == 0 {
                return Err(io::Error::last_os_error());
            }
            // SAFETY: every ACE starts with an ACE_HEADER.
            let header = unsafe { &*ace.cast::<ACE_HEADER>() };
            let rule = match header.AceType {
                ACCESS_ALLOWED_ACE_TYPE | ACCESS_DENIED_ACE_TYPE => {
                    // SAFETY: allowed and denied ACEs share the ACCESS_ALLOWED_ACE layout; the
                    // SID starts at `SidStart`.
                    let body = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
                    AccessRule {
                        allow: header.AceType == ACCESS_ALLOWED_ACE_TYPE,
                        sid: sid_text((&body.SidStart as *const u32).cast_mut().cast())?,
                        mask: body.Mask,
                    }
                }
                other => AccessRule {
                    allow: false,
                    sid: format!("ACE type {other}"),
                    mask: 0,
                },
            };
            rules.push(rule);
        }
        Ok(rules)
    }
}

#[cfg(not(windows))]
mod imp {
    use std::sync::Arc;
    use std::time::Duration;

    use super::{AccessRule, PipeError};
    use crate::bridge::AppBridge;

    /// No bridge outside Windows.
    pub struct PipeServer;

    impl PipeServer {
        pub fn start(_name: &str, _bridge: Arc<dyn AppBridge>) -> Result<PipeServer, PipeError> {
            Err(PipeError::Unsupported)
        }

        pub fn name(&self) -> &str {
            ""
        }

        pub fn stop(self) {}
    }

    /// No bridge outside Windows.
    pub struct PipeClient;

    impl PipeClient {
        pub fn connect(_name: &str, _timeout: Duration) -> Result<PipeClient, PipeError> {
            Err(PipeError::Unsupported)
        }

        pub fn exchange(&mut self, _line: &[u8], _timeout: Duration) -> Result<Vec<u8>, PipeError> {
            Err(PipeError::Unsupported)
        }

        pub fn send_raw(&mut self, _bytes: &[u8], _timeout: Duration) -> Result<(), PipeError> {
            Err(PipeError::Unsupported)
        }

        pub fn read_line(&mut self, _timeout: Duration) -> Result<Option<Vec<u8>>, PipeError> {
            Err(PipeError::Unsupported)
        }

        pub fn access_rules(&self) -> Result<Vec<AccessRule>, PipeError> {
            Err(PipeError::Unsupported)
        }
    }

    pub fn current_user_sid() -> Result<String, PipeError> {
        Err(PipeError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_is_fnv1a() {
        // Reference values of FNV-1a 64.
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_f739_67e8);
    }

    #[test]
    fn the_name_carries_the_user_and_the_settings() {
        let key = settings_key_of(r"C:\Outils\Ardoise\ardoise.toml");
        let name = pipe_name("S-1-5-21-1-2-3-1001", &key);
        assert!(
            name.starts_with(r"\\.\pipe\ardoise-S-1-5-21-1-2-3-1001-"),
            "{name}"
        );
        assert_eq!(
            name.len(),
            r"\\.\pipe\ardoise-S-1-5-21-1-2-3-1001-".len() + 8
        );
        // Same copy whatever the spelling; another copy, another name.
        assert_eq!(
            pipe_name(
                "S-1-5-21-1-2-3-1001",
                &settings_key_of(r"\\?\c:/outils/ARDOISE/ardoise.toml")
            ),
            name
        );
        assert_ne!(
            pipe_name(
                "S-1-5-21-1-2-3-1001",
                &settings_key_of(r"C:\Temp\e2e\ardoise.toml")
            ),
            name
        );
        // Another user, another name; odd characters cannot leave the pipe namespace.
        assert_ne!(pipe_name("S-1-5-21-1-2-3-1002", &key), name);
        let odd = pipe_name(r"Jean Dupont\..\x", &key);
        assert!(
            odd.starts_with(r"\\.\pipe\ardoise-Jean_Dupont_.._x-"),
            "{odd}"
        );
        assert_eq!(
            settings_key_of(r"\\?\UNC\serveur\partage\ardoise.toml"),
            r"\\serveur\partage\ardoise.toml"
        );
    }

    #[test]
    fn the_key_resolves_the_folder_when_it_exists() {
        let dir = tempfile::tempdir().unwrap();
        let direct = settings_key(&dir.path().join("ardoise.toml"));
        let roundabout = settings_key(&dir.path().join(".").join("ardoise.toml"));
        assert_eq!(direct, roundabout);
        assert!(direct.ends_with("ardoise.toml"));
        // A folder that does not exist is taken as written.
        assert_eq!(
            settings_key(Path::new("/nowhere/at/all/ardoise.toml")),
            settings_key_of("/nowhere/at/all/ardoise.toml")
        );
    }
}
