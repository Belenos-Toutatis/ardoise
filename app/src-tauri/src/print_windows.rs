//! The Windows side of printing ([`crate::print`]): the system print dialog (`PrintDlgEx`), then
//! a GDI job with one page per note, the orientation of the device context following each
//! note (`ResetDC`) and the rendered note stretched over the sheet.
//!
//! The job itself ([`print_job`]) takes any device context, so that the test prints to
//! "Microsoft Print to PDF" with an output file (no dialog, no paper) and checks the PDF.

use std::path::Path;

use windows::Win32::Foundation::{GlobalFree, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, DEVMODEW, DIB_RGB_COLORS, DM_ORIENTATION, DeleteDC,
    GetDeviceCaps, HALFTONE, HDC, HORZRES, LOGPIXELSX, LOGPIXELSY, ResetDCW, SRCCOPY,
    SetStretchBltMode, StretchDIBits, VERTRES,
};
use windows::Win32::Storage::Xps::{AbortDoc, DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::Controls::Dialogs::{
    PD_NOCURRENTPAGE, PD_NOSELECTION, PD_PAGENUMS, PD_RESULT_PRINT, PD_RETURNDC,
    PD_USEDEVMODECOPIESANDCOLLATE, PRINTDLGEXW, PRINTPAGERANGE, PrintDlgExW, START_PAGE_GENERAL,
};
use windows::core::PCWSTR;

use crate::commands::CommandResult;
use crate::export::LoadedSource;
use crate::print::{
    Orientation, PrintOptions, PrintResult, Sheet, place, print_error, render_page, selected_notes,
};

/// Ranges the dialog can hold ("1-3, 5, 7-9").
const MAX_RANGES: u32 = 16;
/// `DMORIENT_PORTRAIT` and `DMORIENT_LANDSCAPE` of `DEVMODE.dmOrientation`.
const PORTRAIT: i16 = 1;
const LANDSCAPE: i16 = 2;

/// UTF-16 with the final null Windows wants.
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// A copy of a `DEVMODE` (the structure and the driver's private data after it), 8-byte aligned.
pub(crate) struct DevMode {
    words: Vec<u64>,
}

impl DevMode {
    /// Copies the `DEVMODE` at `source`, which must be valid for its `dmSize + dmDriverExtra`.
    ///
    /// # Safety
    /// `source` points to a complete `DEVMODEW`.
    unsafe fn copy_of(source: *const DEVMODEW) -> DevMode {
        // SAFETY: by the contract; the sizes are read from the structure itself.
        let bytes = unsafe { usize::from((*source).dmSize) + usize::from((*source).dmDriverExtra) };
        let mut words = vec![
            0u64;
            bytes
                .div_ceil(8)
                .max(std::mem::size_of::<DEVMODEW>().div_ceil(8))
        ];
        // SAFETY: `words` holds at least `bytes` bytes and does not overlap the source.
        unsafe {
            std::ptr::copy_nonoverlapping(
                source.cast::<u8>(),
                words.as_mut_ptr().cast::<u8>(),
                bytes,
            )
        };
        DevMode { words }
    }

    fn as_ptr(&mut self) -> *mut DEVMODEW {
        self.words.as_mut_ptr().cast::<DEVMODEW>()
    }

    /// Sets the orientation the next pages are printed in.
    fn set_orientation(&mut self, orientation: Orientation) {
        // SAFETY: `words` is at least as large as a `DEVMODEW` and 8-byte aligned.
        unsafe {
            let devmode = &mut *self.as_ptr();
            devmode.dmFields |= DM_ORIENTATION;
            devmode.Anonymous1.Anonymous1.dmOrientation = match orientation {
                Orientation::Portrait => PORTRAIT,
                Orientation::Landscape => LANDSCAPE,
            };
        }
    }
}

/// The printable area of the sheet in the device context now, and its resolution.
fn sheet_of(hdc: HDC) -> Sheet {
    // SAFETY: `hdc` is a live printer device context.
    unsafe {
        Sheet {
            width: GetDeviceCaps(Some(hdc), HORZRES),
            height: GetDeviceCaps(Some(hdc), VERTRES),
            dpi_x: GetDeviceCaps(Some(hdc), LOGPIXELSX),
            dpi_y: GetDeviceCaps(Some(hdc), LOGPIXELSY),
        }
    }
}

/// Draws the pages of `notes` (the indexes `selected`) on `hdc`, one note per sheet in its
/// orientation. `output` names a file to print to (a "print to file" printer), else the printer
/// sends the job to its port. Returns the number of pages printed; the job is aborted on the
/// first failure.
pub fn print_job(
    hdc: HDC,
    mut devmode: Option<DevMode>,
    title: &str,
    sources: &[LoadedSource],
    selected: &[usize],
    options: &PrintOptions,
    output: Option<&Path>,
) -> CommandResult<u32> {
    let name = wide(title);
    let file = output.map(|path| wide(&path.to_string_lossy()));
    let info = DOCINFOW {
        cbSize: std::mem::size_of::<DOCINFOW>() as i32,
        lpszDocName: PCWSTR(name.as_ptr()),
        lpszOutput: file.as_ref().map_or(PCWSTR::null(), |f| PCWSTR(f.as_ptr())),
        lpszDatatype: PCWSTR::null(),
        fwType: 0,
    };
    // SAFETY: `hdc` is a live printer device context and `info` points to null-terminated
    // strings that outlive the call.
    if unsafe { StartDocW(hdc, &info) } <= 0 {
        return Err(print_error("the printer refused the document"));
    }
    let mut pages = 0;
    for &index in selected {
        let Some(source) = sources.get(index) else {
            continue;
        };
        if let Err(error) = print_page(hdc, devmode.as_mut(), source, options) {
            // SAFETY: the document was started on this device context.
            unsafe { AbortDoc(hdc) };
            return Err(error);
        }
        pages += 1;
    }
    // SAFETY: the document was started on this device context and every page is ended.
    if unsafe { EndDoc(hdc) } <= 0 {
        return Err(print_error("the printer could not finish the document"));
    }
    Ok(pages)
}

fn print_page(
    hdc: HDC,
    devmode: Option<&mut DevMode>,
    source: &LoadedSource,
    options: &PrintOptions,
) -> CommandResult<()> {
    if let Some(devmode) = devmode {
        devmode.set_orientation(Orientation::of(source.note.page));
        // SAFETY: between two pages, with nothing selected into the context.
        if unsafe { ResetDCW(hdc, devmode.as_ptr()) }.is_invalid() {
            return Err(print_error(
                "the printer cannot change the page orientation",
            ));
        }
    }
    let placement = place(source.note.page, sheet_of(hdc), options.fit);
    let bitmap = render_page(source, &placement, options)?;
    let header = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: bitmap.width as i32,
        // Negative: the rows are top-down.
        biHeight: -(bitmap.height as i32),
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let info = BITMAPINFO {
        bmiHeader: header,
        ..Default::default()
    };
    // SAFETY: `hdc` is live and has a started document; `bitmap.bgra` holds
    // `width × height` 32-bit pixels, as `info` says.
    unsafe {
        if StartPage(hdc) <= 0 {
            return Err(print_error("the printer refused a page"));
        }
        SetStretchBltMode(hdc, HALFTONE);
        let drawn = StretchDIBits(
            hdc,
            placement.x,
            placement.y,
            placement.width,
            placement.height,
            0,
            0,
            bitmap.width as i32,
            bitmap.height as i32,
            Some(bitmap.bgra.as_ptr().cast()),
            &info,
            DIB_RGB_COLORS,
            SRCCOPY,
        );
        if drawn == 0 {
            return Err(print_error("the page could not be drawn"));
        }
        if EndPage(hdc) <= 0 {
            return Err(print_error("the printer could not finish a page"));
        }
    }
    Ok(())
}

/// Opens the print dialog over `owner` (the app's window) and prints `sources` as the user
/// chose; `printed: false` when the dialog is closed without printing.
pub fn print_with_dialog(
    owner: usize,
    sources: &[LoadedSource],
    options: &PrintOptions,
) -> CommandResult<PrintResult> {
    if sources.is_empty() {
        return Err(print_error("nothing to print"));
    }
    // The dialog needs COM in a single-threaded apartment on the thread that shows it.
    // SAFETY: balanced by `CoUninitialize` below, on the same thread.
    let com = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.is_ok();
    let result = show_dialog(owner, sources, options);
    if com {
        // SAFETY: the call above succeeded on this thread.
        unsafe { CoUninitialize() };
    }
    result
}

fn show_dialog(
    owner: usize,
    sources: &[LoadedSource],
    options: &PrintOptions,
) -> CommandResult<PrintResult> {
    let mut ranges = [PRINTPAGERANGE::default(); MAX_RANGES as usize];
    // SAFETY: an all-zero PRINTDLGEXW is the documented way to start (no callback, no template).
    let mut dialog: PRINTDLGEXW = unsafe { std::mem::zeroed() };
    dialog.lStructSize = std::mem::size_of::<PRINTDLGEXW>() as u32;
    dialog.hwndOwner = HWND(owner as *mut _);
    dialog.Flags = PD_RETURNDC | PD_NOSELECTION | PD_NOCURRENTPAGE | PD_USEDEVMODECOPIESANDCOLLATE;
    dialog.nMaxPageRanges = MAX_RANGES;
    dialog.lpPageRanges = ranges.as_mut_ptr();
    dialog.nMinPage = 1;
    dialog.nMaxPage = sources.len() as u32;
    dialog.nCopies = 1;
    dialog.nStartPage = START_PAGE_GENERAL;
    // SAFETY: `dialog` is initialised as the function requires, and `ranges` outlives the call.
    unsafe { PrintDlgExW(&mut dialog) }.map_err(|e| print_error(format!("print dialog: {e}")))?;
    let result = finish(&mut dialog, &ranges, sources, options);
    // SAFETY: the dialog allocated these; each is released once.
    unsafe {
        if !dialog.hDC.is_invalid() {
            let _ = DeleteDC(dialog.hDC);
        }
        for memory in [dialog.hDevMode, dialog.hDevNames] {
            if !memory.is_invalid() {
                let _ = GlobalFree(Some(memory));
            }
        }
    }
    result
}

/// What to do with the dialog's answer.
fn finish(
    dialog: &mut PRINTDLGEXW,
    ranges: &[PRINTPAGERANGE],
    sources: &[LoadedSource],
    options: &PrintOptions,
) -> CommandResult<PrintResult> {
    if dialog.dwResultAction != PD_RESULT_PRINT || dialog.hDC.is_invalid() {
        return Ok(PrintResult {
            printed: false,
            pages: 0,
        });
    }
    let chosen: Vec<(u32, u32)> = if dialog.Flags.contains(PD_PAGENUMS) {
        ranges
            .iter()
            .take(dialog.nPageRanges as usize)
            .map(|range| (range.nFromPage, range.nToPage))
            .collect()
    } else {
        Vec::new()
    };
    let selected = selected_notes(sources.len(), &chosen);
    let devmode = devmode_of(dialog.hDevMode);
    let title = match sources {
        [only] => only.note.title.trim().to_owned(),
        _ => String::new(),
    };
    let title = if title.is_empty() {
        "Ardoise".to_owned()
    } else {
        title
    };
    let pages = print_job(
        dialog.hDC, devmode, &title, sources, &selected, options, None,
    )?;
    Ok(PrintResult {
        printed: true,
        pages,
    })
}

/// A copy of the `DEVMODE` the dialog returned, if any.
fn devmode_of(memory: HGLOBAL) -> Option<DevMode> {
    if memory.is_invalid() {
        return None;
    }
    // SAFETY: the handle comes from the dialog; it is locked only for the copy.
    unsafe {
        let locked = GlobalLock(memory);
        if locked.is_null() {
            return None;
        }
        let copy = DevMode::copy_of(locked.cast());
        let _ = GlobalUnlock(memory);
        Some(copy)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ardoise_core::note::{A4_LANDSCAPE, A4_PORTRAIT};
    use windows::Win32::Graphics::Gdi::CreateDCW;
    use windows::Win32::Graphics::Printing::{
        ClosePrinter, DocumentPropertiesW, OpenPrinterW, PRINTER_HANDLE,
    };

    use super::*;
    use crate::export::LoadedSource;
    use crate::library::tests::sample_note;
    use crate::print::PrintFit;

    const PRINTER: &str = "Microsoft Print to PDF";

    /// The printer's default `DEVMODE`, `None` when the printer is not installed.
    fn default_devmode(name: &str) -> Option<DevMode> {
        let printer = wide(name);
        let mut handle = PRINTER_HANDLE::default();
        // SAFETY: plain calls with valid pointers; the handle is closed at the end.
        unsafe {
            OpenPrinterW(PCWSTR(printer.as_ptr()), &mut handle, None).ok()?;
            let size = DocumentPropertiesW(None, handle, PCWSTR(printer.as_ptr()), None, None, 0);
            let devmode = if size > 0 {
                let mut words = vec![0u64; (size as usize).div_ceil(8)];
                let out = words.as_mut_ptr().cast::<DEVMODEW>();
                // DM_OUT_BUFFER = 2.
                let done =
                    DocumentPropertiesW(None, handle, PCWSTR(printer.as_ptr()), Some(out), None, 2);
                (done > 0).then_some(DevMode { words })
            } else {
                None
            };
            let _ = ClosePrinter(handle);
            devmode
        }
    }

    fn source(title: &str, page: ardoise_core::note::PageSize) -> LoadedSource {
        let mut note = sample_note(title);
        note.page = page;
        LoadedSource {
            note,
            stem: title.to_owned(),
            picture: None,
        }
    }

    /// The size in points of each page of a PDF file.
    fn page_sizes(path: &Path) -> Vec<(f32, f32)> {
        let document = lopdf::Document::load(path).unwrap();
        document
            .get_pages()
            .values()
            .map(|&id| {
                let media: Vec<f32> = document
                    .get_dictionary(id)
                    .unwrap()
                    .get(b"MediaBox")
                    .unwrap()
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| {
                        v.as_float()
                            .unwrap_or_else(|_| v.as_i64().unwrap_or_default() as f32)
                    })
                    .collect();
                (media[2] - media[0], media[3] - media[1])
            })
            .collect()
    }

    /// Prints three notes (portrait, landscape, portrait) to "Microsoft Print to PDF", straight
    /// into a file, and reads the PDF back: one page per note, in its orientation.
    #[test]
    fn notes_print_to_pdf_each_on_a_page_in_its_orientation() {
        let Some(devmode) = default_devmode(PRINTER) else {
            eprintln!("skipped: the printer \"{PRINTER}\" is not installed");
            return;
        };
        let dir = tempfile::tempdir().unwrap();
        let output: PathBuf = dir.path().join("printed.pdf");
        let sources = vec![
            source("Portrait", A4_PORTRAIT),
            source("Paysage", A4_LANDSCAPE),
            source("Encore portrait", A4_PORTRAIT),
        ];
        let name = wide(PRINTER);
        let mut devmode = devmode;
        // SAFETY: valid null-terminated strings and a complete DEVMODE.
        let hdc = unsafe {
            CreateDCW(
                PCWSTR::null(),
                PCWSTR(name.as_ptr()),
                PCWSTR::null(),
                Some(devmode.as_ptr()),
            )
        };
        assert!(!hdc.is_invalid(), "cannot open the printer");
        let options = PrintOptions {
            background: true,
            fit: PrintFit::Fit,
        };
        let pages = print_job(
            hdc,
            Some(devmode),
            "Essai",
            &sources,
            &[0, 1, 2],
            &options,
            Some(&output),
        );
        // SAFETY: the context was created above.
        unsafe {
            let _ = DeleteDC(hdc);
        }
        assert_eq!(pages.unwrap(), 3);
        let sizes = page_sizes(&output);
        println!("pages of the printed PDF (points): {sizes:?}");
        assert_eq!(sizes.len(), 3);
        // A4 is 595 x 842 points, whatever the orientation.
        assert!(sizes[0].0 < sizes[0].1, "{sizes:?}");
        assert!(sizes[1].0 > sizes[1].1, "{sizes:?}");
        assert!(sizes[2].0 < sizes[2].1, "{sizes:?}");
        for (width, height) in &sizes {
            let (small, large) = (width.min(*height), width.max(*height));
            assert!(
                (small - 595.0).abs() < 3.0 && (large - 842.0).abs() < 3.0,
                "{sizes:?}"
            );
        }
        // Only the pages asked for are printed.
        let partial = dir.path().join("partial.pdf");
        let name = wide(PRINTER);
        let mut devmode = default_devmode(PRINTER).unwrap();
        // SAFETY: as above.
        let hdc = unsafe {
            CreateDCW(
                PCWSTR::null(),
                PCWSTR(name.as_ptr()),
                PCWSTR::null(),
                Some(devmode.as_ptr()),
            )
        };
        let pages = print_job(
            hdc,
            Some(devmode),
            "Essai",
            &sources,
            &[1],
            &options,
            Some(&partial),
        );
        // SAFETY: as above.
        unsafe {
            let _ = DeleteDC(hdc);
        }
        assert_eq!(pages.unwrap(), 1);
        assert_eq!(page_sizes(&partial).len(), 1);
    }

    /// Opens Windows' real print dialog over a window of the test, closes it as the user would
    /// (WM_CLOSE on its window) and checks that nothing is printed. On Windows 11 the dialog
    /// lives in a window of another process, titled like "Impression à partir de l'application
    /// Win32 - Imprimer". Ignored by default: it shows a window on the desktop for a moment.
    /// `cargo test -p ardoise print_windows -- --ignored`.
    #[test]
    #[ignore = "opens Windows' print dialog on the desktop"]
    fn closing_the_print_dialog_prints_nothing() {
        use std::time::{Duration, Instant};

        use windows::Win32::Foundation::{LPARAM, TRUE, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, EnumWindows, GetWindowTextW, IsWindowVisible,
            PostMessageW, WINDOW_EX_STYLE, WM_CLOSE, WS_OVERLAPPEDWINDOW, WS_VISIBLE,
        };
        use windows::core::{BOOL, w};

        /// Closes the print dialog's window; `found` says whether there was one.
        unsafe extern "system" fn close_print_dialog(window: HWND, found: LPARAM) -> BOOL {
            let mut title = [0u16; 128];
            // SAFETY: plain queries of a window handed over by the enumeration; `found` points
            // to the caller's `bool`, alive during the enumeration.
            unsafe {
                let length = GetWindowTextW(window, &mut title) as usize;
                let title = String::from_utf16_lossy(&title[..length]);
                if IsWindowVisible(window).as_bool()
                    && title.contains("Win32")
                    && (title.contains("Imprim") || title.contains("Print"))
                {
                    let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
                    *(found.0 as *mut bool) = true;
                }
            }
            TRUE
        }

        let sources = vec![source("Portrait", A4_PORTRAIT)];
        let options = PrintOptions {
            background: true,
            fit: PrintFit::Fit,
        };
        let worker = std::thread::spawn(move || {
            // The dialog needs an owner window (the app passes its main window): a small
            // visible window of the predefined STATIC class, on this thread.
            // SAFETY: a valid class name, no parent, no menu.
            let owner = unsafe {
                CreateWindowExW(
                    WINDOW_EX_STYLE(0),
                    w!("STATIC"),
                    w!("owner"),
                    WS_OVERLAPPEDWINDOW | WS_VISIBLE,
                    100,
                    100,
                    300,
                    200,
                    None,
                    None,
                    None,
                    None,
                )
            }
            .unwrap();
            let result = print_with_dialog(owner.0 as usize, &sources, &options);
            // SAFETY: the window was created above, on this thread.
            let _ = unsafe { DestroyWindow(owner) };
            result
        });
        let deadline = Instant::now() + Duration::from_secs(40);
        let mut shown = false;
        while Instant::now() < deadline && !worker.is_finished() {
            let mut found = false;
            // SAFETY: the callback only touches `found`, which outlives the call.
            unsafe {
                let _ = EnumWindows(
                    Some(close_print_dialog),
                    LPARAM(&mut found as *mut bool as isize),
                );
            }
            shown |= found;
            std::thread::sleep(Duration::from_millis(200));
        }
        assert!(worker.is_finished(), "the print dialog did not close");
        let result = worker.join().unwrap();
        assert!(shown, "the print dialog was not seen (result {result:?})");
        assert_eq!(
            result.unwrap(),
            PrintResult {
                printed: false,
                pages: 0
            }
        );
    }
}
