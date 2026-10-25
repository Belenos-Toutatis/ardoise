//! PDF pages rendered by Windows (`Windows.Data.Pdf`, built into Windows 10 and 11) for the
//! background images of notes (SPEC §4.4, P2): the size of each page of a PDF, and one page as
//! a PNG of a chosen size. Nothing is installed or bundled: the system renders the PDF.
//!
//! WinRT operations are completed with `.join()`, which blocks: call these functions from a
//! blocking task, never from the interface's thread. The document is read from memory, so the
//! user's file is opened once, read-only, and a [`PdfFile`] loaded once renders as many pages as
//! wanted. Elsewhere (`cfg(not(windows))`, the cloud sessions' Linux) every call answers
//! [`PdfError::Unsupported`]; the logic around it is tested there with images.
//!
//! A PDF that needs a password to be opened (a user password) fails to load with
//! `ERROR_WRONG_PASSWORD`: [`PdfError::Encrypted`]. A PDF protected only by an owner password
//! (permissions) loads and renders like in any reader (checked on Windows 11 with a PDF that
//! `lopdf` encrypted: `IsPasswordProtected` is false then, and the page is drawn).

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

#[cfg(test)]
thread_local! {
    /// PDFs loaded by this thread (tests check that an import loads its PDF once).
    pub static LOADS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A PDF loaded by Windows, ready to give its pages.
pub struct PdfFile {
    document: imp::Document,
}

impl PdfFile {
    /// Loads the PDF `bytes` (refused when larger than [`MAX_PDF_BYTES`], with more than
    /// [`MAX_PDF_PAGES`] pages, or needing a password).
    pub fn load(bytes: &[u8]) -> Result<PdfFile, PdfError> {
        if bytes.len() > MAX_PDF_BYTES {
            return Err(PdfError::TooLarge);
        }
        #[cfg(test)]
        LOADS.with(|loads| loads.set(loads.get() + 1));
        Ok(PdfFile {
            document: imp::Document::load(bytes)?,
        })
    }

    /// The size of each page, in order.
    pub fn page_sizes(&self) -> Result<Vec<PdfPageSize>, PdfError> {
        self.document.page_sizes()
    }

    /// Page `page` (from 1), rendered on white to a PNG of `width × height` pixels.
    pub fn render_page(&self, page: u32, width: u32, height: u32) -> Result<Vec<u8>, PdfError> {
        if page == 0 {
            return Err(PdfError::NoSuchPage(page));
        }
        self.document.render_page(page, width.max(1), height.max(1))
    }
}

/// The size of each page of the PDF `bytes`, in order.
pub fn page_sizes(bytes: &[u8]) -> Result<Vec<PdfPageSize>, PdfError> {
    PdfFile::load(bytes)?.page_sizes()
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

    /// A loaded document.
    pub(super) struct Document(PdfDocument);

    impl Document {
        pub(super) fn load(bytes: &[u8]) -> Result<Document, PdfError> {
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
            // No password is given: a PDF that needs one to be opened fails here; one that
            // only restricts permissions (owner password) loads.
            let document = PdfDocument::LoadFromStreamAsync(&stream)
                .and_then(|operation| operation.join())
                .map_err(|error| {
                    if error.code() == WRONG_PASSWORD {
                        PdfError::Encrypted
                    } else {
                        damaged(error)
                    }
                })?;
            let count = document.PageCount().map_err(damaged)?;
            if count > MAX_PDF_PAGES {
                return Err(PdfError::TooLarge);
            }
            Ok(Document(document))
        }

        pub(super) fn page_sizes(&self) -> Result<Vec<PdfPageSize>, PdfError> {
            let count = self.0.PageCount().map_err(damaged)?;
            (0..count)
                .map(|index| {
                    let page = self.0.GetPage(index).map_err(damaged)?;
                    let size = page.Size().map_err(damaged)?;
                    let _ = page.Close();
                    Ok(PdfPageSize {
                        width: size.Width * POINTS_PER_DIP,
                        height: size.Height * POINTS_PER_DIP,
                    })
                })
                .collect()
        }

        pub(super) fn render_page(
            &self,
            number: u32,
            width: u32,
            height: u32,
        ) -> Result<Vec<u8>, PdfError> {
            let count = self.0.PageCount().map_err(damaged)?;
            if number > count {
                return Err(PdfError::NoSuchPage(number));
            }
            let page = self.0.GetPage(number - 1).map_err(damaged)?;
            let rendered = render_scaled(&page, width, height);
            let _ = page.Close();
            rendered
        }
    }

    /// Side, in DIPs, of the small render that measures the display scale.
    const PROBE_DIPS: u32 = 100;

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

    /// Never made: loading fails.
    #[allow(dead_code)]
    pub(super) struct Document;

    impl Document {
        pub(super) fn load(_bytes: &[u8]) -> Result<Document, PdfError> {
            Err(PdfError::Unsupported)
        }

        pub(super) fn page_sizes(&self) -> Result<Vec<PdfPageSize>, PdfError> {
            Err(PdfError::Unsupported)
        }

        pub(super) fn render_page(
            &self,
            _number: u32,
            _width: u32,
            _height: u32,
        ) -> Result<Vec<u8>, PdfError> {
            Err(PdfError::Unsupported)
        }
    }
}
