//! Windows Media Foundation back end: MP4 writer, file probe and encoder list.
//!
//! Every Media Foundation object is a COM object. Each public entry point opens an
//! [`MfSession`] (COM initialised for the thread + `MFStartup`) and keeps it alive until all
//! its COM objects are released. The types holding COM pointers are neither `Send` nor `Sync`:
//! a writer stays on the thread that created it (export runs on one worker thread).

mod encoders;
mod probe;
mod writer;

pub use encoders::available_encoders;
pub use probe::probe_mp4;
pub use writer::Mp4Writer;

use std::marker::PhantomData;

use windows::Win32::Foundation::RPC_E_CHANGED_MODE;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFMediaBuffer, MF_VERSION, MFCreateAttributes, MFSTARTUP_LITE, MFShutdown,
    MFStartup,
};
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::GUID;

use crate::MediaError;

/// COM initialised on the current thread for as long as this value lives.
struct ComGuard {
    /// `false` when the thread was already in a single-threaded apartment, which we must not
    /// tear down.
    uninitialize: bool,
    /// COM initialisation is per thread: the guard must be dropped where it was created.
    _thread_bound: PhantomData<*const ()>,
}

impl ComGuard {
    fn new() -> Result<Self, MediaError> {
        // SAFETY: plain FFI call; the reserved pointer must be null.
        let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
        if hr.is_ok() {
            // S_OK, or S_FALSE when already initialised: both must be balanced.
            Ok(Self {
                uninitialize: true,
                _thread_bound: PhantomData,
            })
        } else if hr == RPC_E_CHANGED_MODE {
            // The thread is a single-threaded apartment (for example a UI thread). Media
            // Foundation objects are free-threaded, so COM is usable as it is.
            Ok(Self {
                uninitialize: false,
                _thread_bound: PhantomData,
            })
        } else {
            Err(MediaError::MediaFoundation {
                step: "CoInitializeEx",
                code: hr.0 as u32,
                message: hr.message(),
            })
        }
    }
}

impl Drop for ComGuard {
    fn drop(&mut self) {
        if self.uninitialize {
            // SAFETY: balances the successful CoInitializeEx of `new`, on the same thread
            // (the guard is not `Send`).
            unsafe { CoUninitialize() };
        }
    }
}

/// Media Foundation started (`MFStartup`) for as long as this value lives. Calls are
/// reference-counted by Windows, so several sessions may overlap.
struct MfGuard {
    _thread_bound: PhantomData<*const ()>,
}

impl MfGuard {
    fn new() -> Result<Self, MediaError> {
        // MFSTARTUP_LITE: no network sources; we only write and read local files.
        // SAFETY: plain FFI call with constants; balanced by MFShutdown in `drop`.
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_LITE) }.map_err(|e| {
            MediaError::MediaFoundationUnavailable {
                code: e.code().0 as u32,
                message: e.message(),
            }
        })?;
        Ok(Self {
            _thread_bound: PhantomData,
        })
    }
}

impl Drop for MfGuard {
    fn drop(&mut self) {
        // SAFETY: balances the successful MFStartup of `new`. Nothing useful can be done if
        // shutdown fails.
        let _ = unsafe { MFShutdown() };
    }
}

/// COM + Media Foundation for one writer or probe. Declare it before (so it drops after) every
/// COM object it protects.
pub(crate) struct MfSession {
    // Fields drop in declaration order: Media Foundation shuts down before COM goes away.
    _mf: MfGuard,
    _com: ComGuard,
}

impl MfSession {
    pub(crate) fn start() -> Result<Self, MediaError> {
        // The app delay-loads mfplat.dll and mfreadwrite.dll: calling into a missing one would
        // stop the process (Windows N editions without the Media Feature Pack), so check first.
        if !crate::media_foundation_available() {
            return Err(MediaError::MediaFoundationUnavailable {
                code: 0,
                message: "mfplat.dll or mfreadwrite.dll is missing (Windows N edition without the Media Feature Pack?)".into(),
            });
        }
        let com = ComGuard::new()?;
        let mf = MfGuard::new()?;
        Ok(Self { _mf: mf, _com: com })
    }
}

/// Converts Media Foundation errors, naming the call that failed.
pub(crate) trait StepExt<T> {
    fn step(self, step: &'static str) -> Result<T, MediaError>;
}

impl<T> StepExt<T> for windows::core::Result<T> {
    fn step(self, step: &'static str) -> Result<T, MediaError> {
        self.map_err(|e| mf_error(step, &e))
    }
}

pub(crate) fn mf_error(step: &'static str, error: &windows::core::Error) -> MediaError {
    MediaError::MediaFoundation {
        step,
        code: error.code().0 as u32,
        message: error.message(),
    }
}

/// Two `u32` packed in one `u64` attribute (frame size, frame rate, aspect ratio).
pub(crate) fn pack(high: u32, low: u32) -> u64 {
    (u64::from(high) << 32) | u64::from(low)
}

pub(crate) fn unpack(value: u64) -> (u32, u32) {
    ((value >> 32) as u32, value as u32)
}

/// A new, empty attribute store.
pub(crate) fn create_attributes(capacity: u32) -> Result<IMFAttributes, MediaError> {
    let mut attributes = None;
    // SAFETY: the out-pointer is a valid local; on success it receives an owned reference.
    unsafe { MFCreateAttributes(&mut attributes, capacity) }.step("MFCreateAttributes")?;
    attributes.ok_or_else(|| MediaError::MediaFoundation {
        step: "MFCreateAttributes",
        code: 0,
        message: "no attribute store returned".into(),
    })
}

/// Chained setters for an attribute store (media types are attribute stores too).
pub(crate) struct Attrs<'a>(pub(crate) &'a IMFAttributes);

impl Attrs<'_> {
    pub(crate) fn guid(&self, key: &GUID, value: &GUID) -> Result<&Self, MediaError> {
        // SAFETY: valid COM object; both GUID references live for the duration of the call.
        unsafe { self.0.SetGUID(key, value) }.step("IMFAttributes::SetGUID")?;
        Ok(self)
    }

    pub(crate) fn u32(&self, key: &GUID, value: u32) -> Result<&Self, MediaError> {
        // SAFETY: valid COM object; the key reference lives for the duration of the call.
        unsafe { self.0.SetUINT32(key, value) }.step("IMFAttributes::SetUINT32")?;
        Ok(self)
    }

    pub(crate) fn u64(&self, key: &GUID, value: u64) -> Result<&Self, MediaError> {
        // SAFETY: valid COM object; the key reference lives for the duration of the call.
        unsafe { self.0.SetUINT64(key, value) }.step("IMFAttributes::SetUINT64")?;
        Ok(self)
    }
}

/// Reads a string attribute, if present.
pub(crate) fn get_string(attributes: &IMFAttributes, key: &GUID) -> Option<String> {
    // SAFETY: valid COM object; the key reference lives for the duration of the call.
    let len = unsafe { attributes.GetStringLength(key) }.ok()?;
    let mut buffer = vec![0u16; len as usize + 1]; // + terminating NUL
    let mut written = 0u32;
    // SAFETY: `buffer` holds `len + 1` UTF-16 units as the API requires; `written` is a valid
    // out-pointer.
    unsafe { attributes.GetString(key, &mut buffer, Some(&mut written)) }.ok()?;
    buffer.truncate(written as usize);
    Some(String::from_utf16_lossy(&buffer))
}

/// A locked `IMFMediaBuffer`; unlocked when dropped.
pub(crate) struct LockedBuffer<'a> {
    buffer: &'a IMFMediaBuffer,
    data: *mut u8,
    max_len: usize,
    current_len: usize,
}

impl<'a> LockedBuffer<'a> {
    pub(crate) fn lock(buffer: &'a IMFMediaBuffer) -> Result<Self, MediaError> {
        let mut data = std::ptr::null_mut();
        let mut max_len = 0u32;
        let mut current_len = 0u32;
        // SAFETY: the out-pointers are valid locals. On success the buffer memory stays valid
        // and ours until `Unlock` (in `drop`).
        unsafe { buffer.Lock(&mut data, Some(&mut max_len), Some(&mut current_len)) }
            .step("IMFMediaBuffer::Lock")?;
        if data.is_null() {
            // SAFETY: the lock above succeeded and must be balanced.
            let _ = unsafe { buffer.Unlock() };
            return Err(MediaError::MediaFoundation {
                step: "IMFMediaBuffer::Lock",
                code: 0,
                message: "null buffer pointer".into(),
            });
        }
        Ok(Self {
            buffer,
            data,
            max_len: max_len as usize,
            current_len: (current_len as usize).min(max_len as usize),
        })
    }

    /// The whole allocated memory (`max_len` bytes), for writing.
    pub(crate) fn as_mut_slice(&mut self) -> &mut [u8] {
        // SAFETY: `Lock` granted access to `max_len` bytes at `data` until `Unlock`, which only
        // happens in `drop`; the `&mut self` borrow prevents any other access meanwhile.
        unsafe { std::slice::from_raw_parts_mut(self.data, self.max_len) }
    }

    /// The valid data (`current_len` bytes), for reading.
    pub(crate) fn as_slice(&self) -> &[u8] {
        // SAFETY: as above; `current_len <= max_len`.
        unsafe { std::slice::from_raw_parts(self.data, self.current_len) }
    }
}

impl Drop for LockedBuffer<'_> {
    fn drop(&mut self) {
        // SAFETY: balances the successful `Lock` of `lock`; no slice outlives `self`.
        let _ = unsafe { self.buffer.Unlock() };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_pairs() {
        assert_eq!(pack(1280, 720), (1280u64 << 32) | 720);
        assert_eq!(unpack(pack(1280, 720)), (1280, 720));
    }

    #[test]
    fn sessions_nest() {
        let a = MfSession::start().unwrap();
        let b = MfSession::start().unwrap();
        drop(a);
        drop(b);
    }
}
