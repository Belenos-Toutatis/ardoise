//! PDF pages rendered by Windows (`Windows.Data.Pdf`, built into Windows 10 and 11) for the
//! background images of notes (SPEC §4.4, P2): the size of each page of a PDF, and one page as
//! a PNG of a chosen size. Nothing is installed or bundled: the system renders the PDF.
//!
//! WinRT operations are completed with `.join()`, which blocks: call these functions from a
//! blocking task, never from the interface's thread. The document is read from memory, so the
//! user's file is opened once, read-only. Elsewhere (`cfg(not(windows))`, the cloud sessions'
//! Linux) every call answers [`PdfError::Unsupported`]; the logic around it is tested there
//! with images.

/// Largest PDF read, in bytes (a scanned book of a few hundred pages stays below).
pub const MAX_PDF_BYTES: usize = 512 << 20;
/// Most pages listed (a PDF claiming more is refused as damaged).
pub const MAX_PDF_PAGES: u32 = 10_000;

/// Size of a PDF page as displayed (its rotation applied), in points (1/72 inch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PdfPageSize {
    pub width: f32,
    pub height: f32,
}

/// Why a PDF cannot be read or rendered.
#[derive(Debug, thiserror::Error)]
pub enum PdfError {
    /// Not on Windows.
    #[cfg_attr(windows, allow(dead_code))]
    #[error("PDF files can only be read on Windows")]
    Unsupported,
    /// The PDF needs a password to be opened.
    #[error("the PDF is protected by a password")]
    Encrypted,
    /// Larger than [`MAX_PDF_BYTES`], or with more than [`MAX_PDF_PAGES`] pages.
    #[error("the PDF is too large")]
    TooLarge,
    /// The PDF has no such page (pages count from 1).
    #[error("the PDF has no page {0}")]
    NoSuchPage(u32),
    /// Windows could not read or render it.
    #[error("the PDF cannot be read: {0}")]
    Damaged(String),
}

/// The size of each page of the PDF `bytes`, in order.
pub fn page_sizes(bytes: &[u8]) -> Result<Vec<PdfPageSize>, PdfError> {
    if bytes.len() > MAX_PDF_BYTES {
        return Err(PdfError::TooLarge);
    }
    imp::page_sizes(bytes)
}

/// Page `page` (from 1) of the PDF `bytes`, rendered on white to a PNG of `width × height`
/// pixels.
pub fn render_page(bytes: &[u8], page: u32, width: u32, height: u32) -> Result<Vec<u8>, PdfError> {
    if bytes.len() > MAX_PDF_BYTES {
        return Err(PdfError::TooLarge);
    }
    if page == 0 {
        return Err(PdfError::NoSuchPage(page));
    }
    imp::render_page(bytes, page, width.max(1), height.max(1))
}

#[cfg(windows)]
mod imp {
    use windows::Data::Pdf::{PdfDocument, PdfPage, PdfPageRenderOptions};
    use windows::Storage::Streams::{DataReader, DataWriter, InMemoryRandomAccessStream};
    use windows::core::HRESULT;

    use super::{MAX_PDF_PAGES, PdfError, PdfPageSize};

    /// `HRESULT_FROM_WIN32(ERROR_WRONG_PASSWORD)`: what `PdfDocument` answers for a PDF that
    /// needs a password.
    const WRONG_PASSWORD: HRESULT = HRESULT(0x8007_052B_u32 as i32);
    /// Device-independent pixels per point: `PdfPage::Size` is in DIPs (1/96 inch).
    const POINTS_PER_DIP: f32 = 72.0 / 96.0;

    fn damaged(error: windows::core::Error) -> PdfError {
        PdfError::Damaged(error.message())
    }

    fn load(bytes: &[u8]) -> Result<PdfDocument, PdfError> {
        let stream = InMemoryRandomAccessStream::new().map_err(damaged)?;
        let writer = DataWriter::CreateDataWriter(&stream).map_err(damaged)?;
        writer.WriteBytes(bytes).map_err(damaged)?;
        writer
            .StoreAsync()
            .map_err(damaged)?
            .join()
            .map_err(damaged)?;
        writer
            .FlushAsync()
            .map_err(damaged)?
            .join()
            .map_err(damaged)?;
        writer.DetachStream().map_err(damaged)?;
        stream.Seek(0).map_err(damaged)?;
        let document = PdfDocument::LoadFromStreamAsync(&stream)
            .and_then(|operation| operation.join())
            .map_err(|error| {
                if error.code() == WRONG_PASSWORD {
                    PdfError::Encrypted
                } else {
                    damaged(error)
                }
            })?;
        if document.IsPasswordProtected().unwrap_or(false) {
            return Err(PdfError::Encrypted);
        }
        let count = document.PageCount().map_err(damaged)?;
        if count > MAX_PDF_PAGES {
            return Err(PdfError::TooLarge);
        }
        Ok(document)
    }

    pub(super) fn page_sizes(bytes: &[u8]) -> Result<Vec<PdfPageSize>, PdfError> {
        let document = load(bytes)?;
        let count = document.PageCount().map_err(damaged)?;
        (0..count)
            .map(|index| {
                let page = document.GetPage(index).map_err(damaged)?;
                let size = page.Size().map_err(damaged)?;
                let _ = page.Close();
                Ok(PdfPageSize {
                    width: size.Width * POINTS_PER_DIP,
                    height: size.Height * POINTS_PER_DIP,
                })
            })
            .collect()
    }

    /// Side, in DIPs, of the small render that measures the display scale.
    const PROBE_DIPS: u32 = 100;

    pub(super) fn render_page(
        bytes: &[u8],
        number: u32,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>, PdfError> {
        let document = load(bytes)?;
        let count = document.PageCount().map_err(damaged)?;
        if number > count {
            return Err(PdfError::NoSuchPage(number));
        }
        let page = document.GetPage(number - 1).map_err(damaged)?;
        let rendered = render_scaled(&page, width, height);
        let _ = page.Close();
        rendered
    }

    /// The page rendered `width × height` pixels. The renderer's sizes are in DIPs, which it
    /// multiplies by the display's scale (1.5 at 150 %): a small render first measures that
    /// scale, so that the page comes out at the size asked whatever the screen.
    fn render_scaled(page: &PdfPage, width: u32, height: u32) -> Result<Vec<u8>, PdfError> {
        let probe = render(page, PROBE_DIPS, PROBE_DIPS)?;
        let scale = match ardoise_render::picture::dimensions(&probe) {
            Ok((w, _)) if w > 0 => w as f32 / PROBE_DIPS as f32,
            _ => 1.0,
        };
        let dips = |px: u32| ((px as f32 / scale).round() as u32).max(1);
        render(page, dips(width), dips(height))
    }

    /// The page rendered at `width × height` DIPs, as a PNG on white.
    fn render(page: &PdfPage, width: u32, height: u32) -> Result<Vec<u8>, PdfError> {
        let options = PdfPageRenderOptions::new().map_err(damaged)?;
        options.SetDestinationWidth(width).map_err(damaged)?;
        options.SetDestinationHeight(height).map_err(damaged)?;
        // The default encoder is PNG and the default background white.
        let output = InMemoryRandomAccessStream::new().map_err(damaged)?;
        page.RenderWithOptionsToStreamAsync(&output, &options)
            .and_then(|action| action.join())
            .map_err(damaged)?;
        let size =
            u32::try_from(output.Size().map_err(damaged)?).map_err(|_| PdfError::TooLarge)?;
        let input = output.GetInputStreamAt(0).map_err(damaged)?;
        let reader = DataReader::CreateDataReader(&input).map_err(damaged)?;
        let loaded = reader
            .LoadAsync(size)
            .map_err(damaged)?
            .join()
            .map_err(damaged)?;
        let mut png = vec![0u8; loaded as usize];
        reader.ReadBytes(&mut png).map_err(damaged)?;
        Ok(png)
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{PdfError, PdfPageSize};

    pub(super) fn page_sizes(_bytes: &[u8]) -> Result<Vec<PdfPageSize>, PdfError> {
        Err(PdfError::Unsupported)
    }

    pub(super) fn render_page(
        _bytes: &[u8],
        _number: u32,
        _width: u32,
        _height: u32,
    ) -> Result<Vec<u8>, PdfError> {
        Err(PdfError::Unsupported)
    }
}
