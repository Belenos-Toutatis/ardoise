//! The Windows side of the drag out (see `drag.rs`): an OLE drag (`DoDragDrop`) over files.
//!
//! The data object is the shell's own (`SHCreateDataObject` over the files' item IDs), the one
//! the Explorer hands out when it drags files: it carries `CF_HDROP` (read by Word, mail
//! programs, browsers…), "Shell IDList Array" and the file names, and it stores the extra
//! formats of the drag image. Only copying is allowed: the files are temporary, a "move" must
//! never make a program think it owns them. The picture that follows the pointer is the first
//! note, reduced.
//!
//! Everything here runs on the main thread (OLE drag and drop belongs to the thread of the
//! window), called from `drag::start_note_drag`.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::Win32::Foundation::{
    COLORREF, DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, POINT, S_OK, SIZE,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateDIBSection, DIB_RGB_COLORS, DeleteObject, HBITMAP,
};
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, CoCreateInstance, IDataObject};
use windows::Win32::System::Ole::{
    DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE, DoDragDrop, IDropSource, IDropSource_Impl,
    OleInitialize,
};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::Shell::Common::ITEMIDLIST;
use windows::Win32::UI::Shell::{
    CLSID_DragDropHelper, IDragSourceHelper, ILClone, ILFindLastID, ILFree, ILRemoveLastID,
    SHCreateDataObject, SHDRAGIMAGE, SHParseDisplayName,
};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_SWAPBUTTON};
use windows::core::{BOOL, HRESULT, PCWSTR, implement};

use crate::drag::{DragOutcome, preview_of_file};

/// Runs the drag of `files` (all in one folder) until the pointer is released or Escape is
/// pressed. The left button must be down when this is called, else the drag is cancelled at
/// once (it must never "drop" on whatever lies under a pointer that was released already).
pub fn drag_files(files: &[PathBuf]) -> Result<DragOutcome, String> {
    // Already done by the window toolkit; harmless when it is (S_FALSE), required when not.
    // SAFETY: plain COM initialisation of the calling (main) thread.
    let _ = unsafe { OleInitialize(None) };
    // `DoDragDrop` only looks at the buttons when the pointer moves: a button released before the
    // call would wait for the next movement. Cancel at once.
    if !primary_button_down() {
        return Ok(DragOutcome::Cancelled);
    }
    let data = file_data_object(files)?;
    if let Some(first) = files.first() {
        set_drag_image(&data, first);
    }
    let source: IDropSource = PointerSource::default().into();
    let mut effect = DROPEFFECT_NONE;
    // SAFETY: both interfaces are alive for the call, `effect` is a valid output location.
    let result = unsafe { DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect) };
    interpret(result, effect)
}

/// Whether the main mouse button (the left one, the right one when the buttons are swapped) is down.
fn primary_button_down() -> bool {
    // SAFETY: plain queries of the system and keyboard state.
    unsafe {
        let key = if GetSystemMetrics(SM_SWAPBUTTON) != 0 {
            VK_RBUTTON
        } else {
            VK_LBUTTON
        };
        GetAsyncKeyState(i32::from(key.0)) < 0
    }
}

/// What a `DoDragDrop` result means: dropped on a target that took the files, or cancelled.
fn interpret(result: HRESULT, effect: DROPEFFECT) -> Result<DragOutcome, String> {
    if result == DRAGDROP_S_DROP && effect != DROPEFFECT_NONE {
        Ok(DragOutcome::Dropped)
    } else if result == DRAGDROP_S_DROP || result == DRAGDROP_S_CANCEL {
        Ok(DragOutcome::Cancelled)
    } else if result.is_err() {
        Err(format!("DoDragDrop failed: {result:?}"))
    } else {
        Ok(DragOutcome::Cancelled)
    }
}

/// Owns an item ID list allocated by the shell.
struct ItemIds(*mut ITEMIDLIST);

impl Drop for ItemIds {
    fn drop(&mut self) {
        // SAFETY: the pointer came from the shell's allocator and is freed once, here.
        unsafe { ILFree(Some(self.0)) };
    }
}

/// The shell data object for `files`: `CF_HDROP` and the other file formats. All the files must
/// lie in one folder.
pub fn file_data_object(files: &[PathBuf]) -> Result<IDataObject, String> {
    let Some(first) = files.first() else {
        return Err("no file to drag".into());
    };
    let folder = first.parent().ok_or("the file has no folder")?;
    if files.iter().any(|f| f.parent() != Some(folder)) {
        return Err("the files to drag are not in one folder".into());
    }
    let absolute: Vec<ItemIds> = files
        .iter()
        .map(|file| parse_path(file))
        .collect::<Result<_, _>>()?;
    // The folder's own ID list is a copy of the first file's, minus its last item.
    // SAFETY: `absolute[0].0` is a valid ID list; the clone is owned by `ItemIds`.
    let parent = ItemIds(unsafe { ILClone(absolute[0].0) });
    if parent.0.is_null() {
        return Err("cannot copy an item ID list".into());
    }
    // SAFETY: `parent` is a valid, owned, modifiable ID list.
    if !unsafe { ILRemoveLastID(Some(parent.0)) }.as_bool() {
        return Err("cannot find the folder of the file".into());
    }
    // The children are the last items of the absolute lists (pointers into them).
    let children: Vec<*const ITEMIDLIST> = absolute
        .iter()
        // SAFETY: each pointer is a valid ID list; the result points inside it.
        .map(|ids| unsafe { ILFindLastID(ids.0) }.cast_const())
        .collect();
    // SAFETY: the lists outlive the call; the shell copies what it needs.
    unsafe {
        SHCreateDataObject(
            Some(parent.0.cast_const()),
            Some(&children),
            None::<&IDataObject>,
        )
    }
    .map_err(|error| format!("cannot create the data object: {error}"))
}

fn parse_path(path: &Path) -> Result<ItemIds, String> {
    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut ids: *mut ITEMIDLIST = std::ptr::null_mut();
    // SAFETY: `wide` is NUL-terminated and outlives the call; `ids` is a valid output location.
    unsafe {
        SHParseDisplayName(
            PCWSTR(wide.as_ptr()),
            None::<&windows::Win32::System::Com::IBindCtx>,
            &mut ids,
            0,
            None,
        )
    }
    .map_err(|error| format!("cannot find {}: {error}", path.display()))?;
    Ok(ItemIds(ids))
}

/// Gives the drag the picture of the note (best effort: without it the drag has only the
/// pointer).
fn set_drag_image(data: &IDataObject, file: &Path) {
    let Some((width, height, bgra)) = preview_of_file(file) else {
        return;
    };
    let (Ok(cx), Ok(cy)) = (i32::try_from(width), i32::try_from(height)) else {
        return;
    };
    let Some(bitmap) = bitmap_from_bgra(cx, cy, &bgra) else {
        return;
    };
    // SAFETY: the helper is a free-threaded in-process shell object.
    let helper: Result<IDragSourceHelper, _> =
        unsafe { CoCreateInstance(&CLSID_DragDropHelper, None, CLSCTX_INPROC_SERVER) };
    let image = SHDRAGIMAGE {
        sizeDragImage: SIZE { cx, cy },
        // The pointer holds the picture at its middle.
        ptOffset: POINT {
            x: cx / 2,
            y: cy / 2,
        },
        hbmpDragImage: bitmap,
        // Magenta as a colour key; the picture is opaque anyway.
        crColorKey: COLORREF(0x00FF_00FF),
    };
    let taken = match helper {
        // SAFETY: `image` and `data` are valid for the call; on success the helper owns the bitmap.
        Ok(helper) => unsafe { helper.InitializeFromBitmap(&image, data) }.is_ok(),
        Err(_) => false,
    };
    if !taken {
        // SAFETY: the bitmap is ours and not selected anywhere.
        let _ = unsafe { DeleteObject(bitmap.into()) };
    }
}

/// A 32-bit top-down DIB holding premultiplied `bgra` rows, `None` if Windows refuses.
fn bitmap_from_bgra(width: i32, height: i32, bgra: &[u8]) -> Option<HBITMAP> {
    if width <= 0 || height <= 0 || bgra.len() != width as usize * height as usize * 4 {
        return None;
    }
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let mut bits: *mut c_void = std::ptr::null_mut();
    // SAFETY: `info` describes a 32-bit DIB and `bits` receives the pixel memory it owns.
    let bitmap =
        unsafe { CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) }.ok()?;
    if bits.is_null() {
        // SAFETY: the bitmap is ours.
        let _ = unsafe { DeleteObject(bitmap.into()) };
        return None;
    }
    // SAFETY: the section holds exactly `bgra.len()` bytes (checked above).
    unsafe { std::ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), bgra.len()) };
    Some(bitmap)
}

/// Drop source for a drag started by the left button: it goes on while the button is down,
/// drops when it is released, cancels on Escape or when the button was up from the start.
#[implement(IDropSource)]
#[derive(Default)]
struct PointerSource {
    started: std::cell::Cell<bool>,
}

impl IDropSource_Impl for PointerSource_Impl {
    fn QueryContinueDrag(&self, escape_pressed: BOOL, key_state: MODIFIERKEYS_FLAGS) -> HRESULT {
        let first = !self.started.replace(true);
        let left_down = key_state.0 & MK_LBUTTON.0 != 0;
        if escape_pressed.as_bool() || (first && !left_down) {
            DRAGDROP_S_CANCEL
        } else if !left_down {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use windows::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, CoInitializeEx, DVASPECT_CONTENT, FORMATETC, TYMED_HGLOBAL,
    };
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    use windows::Win32::System::Ole::{CF_HDROP, ReleaseStgMedium};
    use windows::core::w;

    use super::*;

    fn com() {
        // SAFETY: each test thread initialises COM for itself.
        let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    }

    fn hdrop_format() -> FORMATETC {
        FORMATETC {
            cfFormat: CF_HDROP.0,
            ptd: std::ptr::null_mut(),
            dwAspect: DVASPECT_CONTENT.0,
            lindex: -1,
            tymed: TYMED_HGLOBAL.0 as u32,
        }
    }

    /// The file names a `CF_HDROP` block holds (wide characters, double NUL at the end).
    fn read_hdrop(data: &IDataObject) -> Vec<String> {
        let format = hdrop_format();
        // SAFETY: `format` is valid; the medium is released below.
        let mut medium = unsafe { data.GetData(&format) }.unwrap();
        // SAFETY: the medium is an HGLOBAL (TYMED_HGLOBAL) holding a DROPFILES block.
        let names = unsafe {
            let hglobal = medium.u.hGlobal;
            let base = GlobalLock(hglobal).cast::<u8>();
            assert!(!base.is_null());
            let offset = std::ptr::read_unaligned(base.cast::<u32>()) as usize;
            let wide = std::ptr::read_unaligned(base.add(16).cast::<u32>());
            assert_eq!(wide, 1, "file names must be wide");
            let mut names = Vec::new();
            let mut current = Vec::new();
            let mut at = base.add(offset).cast::<u16>();
            loop {
                let unit = std::ptr::read_unaligned(at);
                at = at.add(1);
                if unit == 0 {
                    if current.is_empty() {
                        break;
                    }
                    names.push(String::from_utf16(&current).unwrap());
                    current.clear();
                } else {
                    current.push(unit);
                }
            }
            let _ = GlobalUnlock(hglobal);
            names
        };
        // SAFETY: the medium came from GetData and is released once.
        unsafe { ReleaseStgMedium(&mut medium) };
        names
    }

    fn two_files() -> (tempfile::TempDir, Vec<PathBuf>) {
        let dir = tempfile::tempdir().unwrap();
        let files: Vec<PathBuf> = ["Cours de maths.png", "Réunion – parents.png"]
            .iter()
            .map(|name| {
                let path = dir.path().join(name);
                fs::write(&path, b"png").unwrap();
                path
            })
            .collect();
        (dir, files)
    }

    #[test]
    fn the_data_object_gives_both_files_as_cf_hdrop() {
        com();
        let (_dir, files) = two_files();
        let data = file_data_object(&files).unwrap();
        // SAFETY: valid FORMATETC for the calls.
        assert_eq!(unsafe { data.QueryGetData(&hdrop_format()) }, S_OK);
        let names = read_hdrop(&data);
        let expected: Vec<String> = files
            .iter()
            .map(|f| f.to_string_lossy().into_owned())
            .collect();
        assert_eq!(names.len(), 2);
        for (got, want) in names.iter().zip(&expected) {
            assert!(
                got.eq_ignore_ascii_case(want)
                    || fs::canonicalize(got).ok() == fs::canonicalize(want).ok(),
                "{got} != {want}"
            );
        }
    }

    #[test]
    fn the_data_object_also_offers_the_shell_id_list_and_refuses_text() {
        com();
        let (_dir, files) = two_files();
        let data = file_data_object(&files).unwrap();
        // SAFETY: a valid constant name.
        let id_list = unsafe { RegisterClipboardFormatW(w!("Shell IDList Array")) };
        let mut format = hdrop_format();
        format.cfFormat = u16::try_from(id_list).unwrap();
        // SAFETY: valid FORMATETC.
        assert_eq!(unsafe { data.QueryGetData(&format) }, S_OK);
        format.cfFormat = 1; // CF_TEXT
        // SAFETY: valid FORMATETC.
        assert_ne!(unsafe { data.QueryGetData(&format) }, S_OK);
    }

    #[test]
    fn a_single_file_and_bad_lists_are_handled() {
        com();
        let (_dir, files) = two_files();
        let data = file_data_object(&files[..1]).unwrap();
        assert_eq!(read_hdrop(&data).len(), 1);
        assert!(file_data_object(&[]).is_err());
        let other = tempfile::tempdir().unwrap();
        let elsewhere = other.path().join("a.png");
        fs::write(&elsewhere, b"x").unwrap();
        assert!(file_data_object(&[files[0].clone(), elsewhere]).is_err());
        assert!(file_data_object(&[files[0].with_file_name("absent.png")]).is_err());
    }

    #[test]
    fn the_drag_image_is_attached_without_breaking_the_object() {
        com();
        let (_dir, files) = two_files();
        // A real PNG as the first file.
        let png = ardoise_render::encode_png(&ardoise_render::Image {
            width: 40,
            height: 30,
            pixels: [250u8, 250, 240, 255].repeat(40 * 30),
        })
        .unwrap();
        fs::write(&files[0], png).unwrap();
        let data = file_data_object(&files).unwrap();
        set_drag_image(&data, &files[0]);
        assert_eq!(read_hdrop(&data).len(), 2);
    }

    #[test]
    fn the_source_cancels_a_drag_that_started_without_the_button() {
        let source = PointerSource::default();
        let released = MODIFIERKEYS_FLAGS(0);
        let down = MK_LBUTTON;
        let escape = true;
        let no = false;
        let source: IDropSource = source.into();
        // SAFETY: plain calls on a live object.
        unsafe {
            assert_eq!(source.QueryContinueDrag(no, released), DRAGDROP_S_CANCEL);
        }
        let source: IDropSource = PointerSource::default().into();
        // SAFETY: as above.
        unsafe {
            assert_eq!(source.QueryContinueDrag(no, down), S_OK);
            assert_eq!(source.QueryContinueDrag(no, down), S_OK);
            assert_eq!(source.QueryContinueDrag(no, released), DRAGDROP_S_DROP);
        }
        let source: IDropSource = PointerSource::default().into();
        // SAFETY: as above.
        unsafe {
            assert_eq!(source.QueryContinueDrag(escape, down), DRAGDROP_S_CANCEL);
            assert_eq!(
                source.GiveFeedback(DROPEFFECT_COPY),
                DRAGDROP_S_USEDEFAULTCURSORS
            );
        }
    }

    #[test]
    fn dodragdrop_results_are_interpreted() {
        assert_eq!(
            interpret(DRAGDROP_S_DROP, DROPEFFECT_COPY),
            Ok(DragOutcome::Dropped)
        );
        assert_eq!(
            interpret(DRAGDROP_S_DROP, DROPEFFECT_NONE),
            Ok(DragOutcome::Cancelled)
        );
        assert_eq!(
            interpret(DRAGDROP_S_CANCEL, DROPEFFECT_NONE),
            Ok(DragOutcome::Cancelled)
        );
        assert!(interpret(HRESULT(0x8000_4005_u32 as i32), DROPEFFECT_NONE).is_err());
    }

    #[test]
    fn a_drag_without_a_pressed_button_is_cancelled_at_once() {
        com();
        let (_dir, files) = two_files();
        // In a test run nobody holds the mouse button (if somebody does, the real drag would
        // start: not a thing to run in a test).
        if !primary_button_down() {
            assert_eq!(drag_files(&files).unwrap(), DragOutcome::Cancelled);
        }
    }
}
