//! Background images or PDF pages under a note (SPEC §4.4, P2; `docs/design/extras.md`,
//! "Background image or PDF").
//!
//! - **Import**: an image file (PNG, JPEG: `ardoise_render::picture`) or a page of a PDF
//!   (rendered by Windows at about 300 dpi, [`crate::pdf_pages`]) becomes, once, the member a
//!   note file stores (`background.png` or `background.jpg`, at most 4096 px a side).
//! - **Staging** ([`Staging`]): the editor never holds pixels. An image it imports, and the
//!   image of the note it opens, wait in memory here under a key (a hash of the stored bytes)
//!   that the editor keeps in its undo history beside the note. It passes the key back with
//!   whatever needs the pixels: drawing the page, saving, splitting, exporting the unsaved note.
//!   Nothing reaches the library before the note is saved, and an undo that brings an older
//!   image back finds its pixels under its own key. The editor empties the staging area when it
//!   closes or opens another note.
//! - **Saving** ([`settle`]): `notes::save_in` writes the staged member the command chose
//!   ([`with_staged`]) and drops the member of a note that no longer has an image.
//! - **New notes** ([`notes_from_file_in`]): one note per chosen PDF page at the size of the
//!   page, or one note for an image, A4-sized in the image's proportions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use ardoise_core::format::{BackgroundFile, NoteFile};
use ardoise_core::library::Library;
use ardoise_core::note::{Background, BackgroundImage, ImageFit, ImageRect, Note, PageSize};
use ardoise_core::ops::MAX_PAGE_SIDE;
use ardoise_render::picture::{self, MAX_STORED_SIDE};
use ardoise_render::{NoteView, Picture, PictureError, StoredImage};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::AppContext;
use crate::commands::{CommandError, CommandResult};
use crate::library::{
    NoteSummary, binder_path, blank_note, first_layer_name, library_error, library_of, now_ms,
    summary_of,
};
use crate::pdf_pages::{self, PdfError, PdfPageSize};
use crate::thumbnails::{file_thumbnail, thumbnail_view};

/// Resolution a PDF page is rendered at (less when its long side would exceed
/// [`MAX_STORED_SIDE`]).
pub const PDF_DPI: f32 = 300.0;
/// Device units (10 µm) per point (1/72 inch).
const UNITS_PER_POINT: f32 = 2540.0 / 72.0;
/// A4 portrait, in device units: the size an imported image's page fits in.
const A4: PageSize = PageSize {
    width: 21_000,
    height: 29_700,
};
/// Smallest side of a page made from a file (1 cm).
const MIN_PAGE_SIDE: u32 = 1000;
/// Largest image file read, in bytes.
const MAX_IMAGE_BYTES: u64 = 256 << 20;
/// Decoded pictures kept for drawing the editor's page (each up to 64 MB).
const DECODED_KEPT: usize = 2;

// --- Errors -------------------------------------------------------------------------------

/// The error the interface translates (`errorCodes.*`).
pub fn picture_error(error: PictureError) -> CommandError {
    let code = match error {
        PictureError::Unsupported => "backgroundUnsupported",
        PictureError::TooLarge { .. } => "backgroundTooLarge",
        PictureError::Damaged(_) | PictureError::Encode(_) => "backgroundDamaged",
    };
    CommandError::new(code, error)
}

/// The error the interface translates (`errorCodes.*`).
pub fn pdf_error(error: PdfError) -> CommandError {
    let code = match error {
        PdfError::Unsupported => "pdfUnsupported",
        PdfError::Encrypted => "pdfEncrypted",
        PdfError::TooLarge => "backgroundTooLarge",
        PdfError::NoSuchPage(_) => "pdfNoSuchPage",
        PdfError::Damaged(_) => "backgroundDamaged",
    };
    CommandError::new(code, error)
}

fn missing_image(key: &str) -> CommandError {
    CommandError::new(
        "backgroundMissing",
        format!("the background image {key} is no longer in memory"),
    )
}

// --- Source files -------------------------------------------------------------------------

/// What a chosen file holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SourceKind {
    Image,
    Pdf,
}

/// A page of a PDF, in points (`{ width, height }` in `api.ts`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct PageInfo {
    pub width: f32,
    pub height: f32,
}

impl From<PdfPageSize> for PageInfo {
    fn from(size: PdfPageSize) -> Self {
        PageInfo {
            width: size.width,
            height: size.height,
        }
    }
}

/// A file chosen to become a background (`BackgroundSource` in `api.ts`): its name, its kind
/// and, for a PDF, the size of each page.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct BackgroundSource {
    pub name: String,
    pub kind: SourceKind,
    pub pages: Vec<PageInfo>,
}

/// The kind of a file from its first kilobyte: a PDF (`%PDF-` anywhere in it, as readers
/// allow), or a PNG or JPEG image.
fn kind_of(head: &[u8]) -> Result<SourceKind, CommandError> {
    let head = head.get(..head.len().min(1024)).unwrap_or_default();
    if head.windows(5).any(|w| w == b"%PDF-") {
        Ok(SourceKind::Pdf)
    } else if picture::PictureFormat::sniff(head).is_some() {
        Ok(SourceKind::Image)
    } else {
        Err(picture_error(PictureError::Unsupported))
    }
}

/// The file at `path`, read whole, and its kind: its first kilobyte says what it is, and a file
/// larger than its kind may be is refused before the rest is read.
fn read_source(path: &Path) -> CommandResult<(Vec<u8>, SourceKind)> {
    let read_error = |error: std::io::Error| {
        CommandError::new(
            "backgroundUnreadable",
            format!("{}: {error}", path.display()),
        )
    };
    let mut file = std::fs::File::open(path).map_err(read_error)?;
    let size = file.metadata().map_err(read_error)?.len();
    let mut head = Vec::with_capacity(1024);
    (&mut file)
        .take(1024)
        .read_to_end(&mut head)
        .map_err(read_error)?;
    let kind = kind_of(&head)?;
    let limit = match kind {
        SourceKind::Pdf => pdf_pages::MAX_PDF_BYTES as u64,
        SourceKind::Image => MAX_IMAGE_BYTES,
    };
    if size > limit {
        return Err(CommandError::new(
            "backgroundTooLarge",
            format!("{size} bytes, at most {limit}"),
        ));
    }
    let mut bytes = head;
    file.read_to_end(&mut bytes).map_err(read_error)?;
    Ok((bytes, kind))
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// What the file at `path` holds (for the page chooser).
pub fn inspect(path: &Path) -> CommandResult<BackgroundSource> {
    let (bytes, kind) = read_source(path)?;
    let pages = match kind {
        SourceKind::Pdf => pdf_pages::page_sizes(&bytes)
            .map_err(pdf_error)?
            .into_iter()
            .map(PageInfo::from)
            .collect(),
        SourceKind::Image => {
            // Refused now rather than after the user chose: too large or not an image.
            let (width, height) = picture::dimensions(&bytes).map_err(picture_error)?;
            if u64::from(width) * u64::from(height) > picture::MAX_SOURCE_PIXELS {
                return Err(picture_error(PictureError::TooLarge { width, height }));
            }
            Vec::new()
        }
    };
    Ok(BackgroundSource {
        name: file_name(path),
        kind,
        pages,
    })
}

/// Pixel size a PDF page of `size` points is rendered at: [`PDF_DPI`], reduced so that the
/// long side is at most [`MAX_STORED_SIDE`].
pub fn render_size(size: PdfPageSize) -> (u32, u32) {
    let (w, h) = (
        size.width.max(1.0) / 72.0 * PDF_DPI,
        size.height.max(1.0) / 72.0 * PDF_DPI,
    );
    let shrink = (MAX_STORED_SIDE as f32 / w.max(h)).min(1.0);
    let side = |v: f32| (v * shrink).round().clamp(1.0, MAX_STORED_SIDE as f32) as u32;
    (side(w), side(h))
}

/// The pixels of page `page` (from 1) of a PDF, or of an image file (`page` ignored), ready
/// to store.
fn import(bytes: &[u8], kind: SourceKind, page: Option<u32>) -> CommandResult<StoredImage> {
    match kind {
        SourceKind::Image => picture::import_image(bytes).map_err(picture_error),
        SourceKind::Pdf => {
            let sizes = pdf_pages::page_sizes(bytes).map_err(pdf_error)?;
            let number = page.unwrap_or(1);
            let size = number
                .checked_sub(1)
                .and_then(|i| sizes.get(i as usize))
                .ok_or_else(|| pdf_error(PdfError::NoSuchPage(number)))?;
            let (width, height) = render_size(*size);
            let png = pdf_pages::render_page(bytes, number, width, height).map_err(pdf_error)?;
            picture::import_page(&png).map_err(picture_error)
        }
    }
}

// --- Staging ------------------------------------------------------------------------------

/// An image waiting in memory for the editor: the member it would be stored as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Staged {
    pub file: BackgroundFile,
    pub width: u32,
    pub height: u32,
}

/// Images the editor imported or opened, by key; the few last decoded for drawing.
#[derive(Debug, Default)]
pub struct Staging {
    images: Mutex<HashMap<String, Arc<Staged>>>,
    decoded: Mutex<Vec<(String, Arc<Picture>)>>,
}

/// The staging area of this process.
pub fn staging() -> &'static Staging {
    static STAGING: LazyLock<Staging> = LazyLock::new(Staging::default);
    &STAGING
}

/// The key of a member's bytes: a hash and the length (the same bytes always get the same key
/// within a process, so staging the same image twice keeps one copy).
pub fn key_of(bytes: &[u8]) -> String {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    format!("{:016x}-{}", hasher.finish(), bytes.len())
}

impl Staging {
    /// Keeps `staged` and returns its key.
    pub fn put(&self, staged: Staged) -> String {
        let key = key_of(&staged.file.bytes);
        self.images
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(key.clone())
            .or_insert_with(|| Arc::new(staged));
        key
    }

    pub fn get(&self, key: &str) -> Option<Arc<Staged>> {
        self.images
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .cloned()
    }

    /// The decoded pixels of the image staged under `key`; `None` when there is none or it
    /// cannot be decoded.
    pub fn picture(&self, key: &str) -> Option<Arc<Picture>> {
        {
            let mut decoded = self.decoded.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(index) = decoded.iter().position(|(k, _)| k == key) {
                let entry = decoded.remove(index);
                let picture = Arc::clone(&entry.1);
                decoded.insert(0, entry);
                return Some(picture);
            }
        }
        let staged = self.get(key)?;
        let picture = Arc::new(Picture::decode(staged.file.bytes.clone()).ok()?);
        let mut decoded = self.decoded.lock().unwrap_or_else(PoisonError::into_inner);
        decoded.insert(0, (key.to_owned(), Arc::clone(&picture)));
        decoded.truncate(DECODED_KEPT);
        Some(picture)
    }

    /// Forgets every image (the editor closed, or opened another note).
    pub fn clear(&self) {
        self.images
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.decoded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.images
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// An image staged for the editor (`StagedBackground` in `api.ts`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedBackground {
    pub key: String,
    /// The imported file's name.
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pdf_page: Option<u32>,
    /// Size of the stored pixels.
    pub width: u32,
    pub height: u32,
}

/// Imports page `pdf_page` of the PDF, or the image, at `path` into `staging`.
pub fn stage_file(
    staging: &Staging,
    path: &Path,
    pdf_page: Option<u32>,
) -> CommandResult<StagedBackground> {
    let (bytes, kind) = read_source(path)?;
    let stored = import(&bytes, kind, pdf_page)?;
    let (width, height) = (stored.width, stored.height);
    let key = staging.put(Staged {
        file: BackgroundFile {
            extension: stored.extension().to_owned(),
            bytes: stored.bytes,
        },
        width,
        height,
    });
    Ok(StagedBackground {
        key,
        name: file_name(path),
        pdf_page: match kind {
            SourceKind::Pdf => Some(pdf_page.unwrap_or(1)),
            SourceKind::Image => None,
        },
        width,
        height,
    })
}

/// Stages the background image of the note at `path` (the editor opens it); `None` when the
/// note has no image, or its member is missing or cannot be read (nothing is drawn then).
pub fn stage_note_image(
    staging: &Staging,
    library: &Library,
    path: &Path,
) -> CommandResult<Option<StagedBackground>> {
    let loaded = library.read(path).map_err(library_error)?;
    let file = loaded.file;
    let (Some(image), Some(member)) = (&file.note.background_image, file.background) else {
        return Ok(None);
    };
    let Ok((width, height)) = picture::dimensions(&member.bytes) else {
        return Ok(None);
    };
    let key = staging.put(Staged {
        file: member,
        width,
        height,
    });
    Ok(Some(StagedBackground {
        key,
        name: image.name.clone(),
        pdf_page: image.pdf_page,
        width,
        height,
    }))
}

/// The staged image under `key`, or `backgroundMissing`.
pub fn staged(staging: &Staging, key: &str) -> CommandResult<Arc<Staged>> {
    staging.get(key).ok_or_else(|| missing_image(key))
}

// --- Saving -------------------------------------------------------------------------------

/// The background member a save writes, chosen by the command ([`chosen_for_save`]), with its
/// decoded pixels for the thumbnail (the staging area usually has them decoded already: the
/// editor's page was drawn with them).
#[derive(Debug, Clone)]
pub struct Chosen {
    pub staged: Arc<Staged>,
    pub picture: Option<Arc<Picture>>,
}

thread_local! {
    /// The member `settle` writes, chosen by the command that saves ([`with_staged`]).
    static CHOSEN: RefCell<Option<Chosen>> = const { RefCell::new(None) };
}

/// Runs `save` (a `notes::save_in`, possibly within a split) with `chosen` as the background
/// member to write when the saved note has an image. `None` keeps the file's own member.
///
/// `save_in` keeps its signature (another part of M7 shares that function), so the choice
/// reaches [`settle`] through this thread's state, set for the duration of the call.
pub fn with_staged<T>(chosen: Option<Chosen>, save: impl FnOnce() -> T) -> T {
    let previous = CHOSEN.with(|current| current.replace(chosen));
    let result = save();
    CHOSEN.with(|current| *current.borrow_mut() = previous);
    result
}

/// Puts the right background member in `file` (as read, still holding the note it was read
/// with), about to be written with `note`, and returns the note's thumbnail (with its image).
/// A note whose image was removed loses the member; a note with an image gets the member chosen
/// by [`with_staged`] (whose decoded pixels draw the thumbnail), else keeps the file's own. A
/// member the note never described (written by another version) is kept as it is.
pub fn settle(file: &mut NoteFile, note: &Note) -> Option<Vec<u8>> {
    let chosen = CHOSEN.with(|current| current.borrow().clone());
    let picture = if note.background_image.is_none() {
        if file.note.background_image.is_some() {
            file.background = None;
        }
        None
    } else if let Some(chosen) = chosen {
        file.background = Some(chosen.staged.file.clone());
        chosen.picture
    } else {
        picture_of(note, file.background.as_ref()).map(Arc::new)
    };
    thumbnail_view(NoteView::new(note, picture.as_deref()))
}

/// The decoded pixels of a note's background member; `None` when the note has no image, or
/// the member is missing or cannot be decoded (nothing is drawn then).
pub fn picture_of(note: &Note, member: Option<&BackgroundFile>) -> Option<Picture> {
    note.background_image.as_ref()?;
    Picture::decode(member?.bytes.clone()).ok()
}

/// The staged member a save of `note` writes, with its pixels: the one under `key` when the
/// note has an image and a key (refused with `backgroundMissing` when it is not staged: the
/// note would be written with the wrong pixels); `None` keeps the file's own member (see
/// [`check_keyless`]).
pub fn chosen_for_save(
    staging: &Staging,
    note: &Note,
    key: Option<&str>,
) -> CommandResult<Option<Chosen>> {
    match (note.background_image.as_ref(), key) {
        (Some(_), Some(key)) => {
            let staged = staged(staging, key)?;
            Ok(Some(Chosen {
                staged,
                picture: staging.picture(key),
            }))
        }
        _ => Ok(None),
    }
}

/// A save without staged pixels keeps the file's own member (the editor could not stage the
/// note's image: its member is missing or unreadable). That member shows the image the file
/// describes, so the saved note may only describe the same image (the same file name and PDF
/// page), placed anywhere (rectangle, quarter turns: page operations move it) and at any
/// opacity; another image, or an image the file does not have, is refused with
/// `backgroundMissing` (its pixels are not here).
pub fn check_keyless(on_disk: &Note, note: &Note) -> CommandResult<()> {
    match (&note.background_image, &on_disk.background_image) {
        (None, _) => Ok(()),
        (Some(new), Some(old)) if new.name == old.name && new.pdf_page == old.pdf_page => Ok(()),
        (Some(new), _) => Err(CommandError::new(
            "backgroundMissing",
            format!(
                "the pixels of the background image {} are not given",
                new.name
            ),
        )),
    }
}

/// The pixels of a background image to render: those staged under `key`, or else (no key)
/// those of `member`. `None` for a note without an image, or pixels that cannot be decoded.
pub fn source_picture(
    note: &Note,
    key: Option<&str>,
    member: Option<&BackgroundFile>,
) -> CommandResult<Option<Arc<Picture>>> {
    if note.background_image.is_none() {
        return Ok(None);
    }
    match key {
        Some(key) => {
            staged(staging(), key)?;
            Ok(staging().picture(key))
        }
        None => Ok(picture_of(note, member).map(Arc::new)),
    }
}

// --- New notes from a file ----------------------------------------------------------------

/// The page of a note made from a PDF page of `size` points: the same size (1 pt = 1/72 in),
/// within 1 cm and 100 m a side.
pub fn page_for_pdf(size: PdfPageSize) -> PageSize {
    let side =
        |pt: f32| ((pt * UNITS_PER_POINT).round() as u32).clamp(MIN_PAGE_SIDE, MAX_PAGE_SIDE);
    PageSize {
        width: side(size.width),
        height: side(size.height),
    }
}

/// The page of a note made from an image of `width × height` pixels: as large as fits in an A4
/// sheet of the image's orientation, in the image's proportions.
pub fn page_for_image(width: u32, height: u32) -> PageSize {
    let (w, h) = (width.max(1) as f32, height.max(1) as f32);
    let (sheet_w, sheet_h) = if w > h {
        (A4.height as f32, A4.width as f32)
    } else {
        (A4.width as f32, A4.height as f32)
    };
    let scale = (sheet_w / w).min(sheet_h / h);
    let side = |v: f32| ((v * scale).round() as u32).clamp(MIN_PAGE_SIDE, MAX_PAGE_SIDE);
    PageSize {
        width: side(w),
        height: side(h),
    }
}

/// Title of the note made from page `page` of `name` (the file name without its extension; the
/// page number only for a PDF of several pages).
pub fn note_title(name: &str, page: Option<u32>, page_count: usize) -> String {
    let stem = Path::new(name)
        .file_stem()
        .map(|s| s.to_string_lossy().trim().to_owned())
        .unwrap_or_default();
    match page {
        Some(page) if page_count > 1 => format!("{stem} - page {page}"),
        _ => stem,
    }
}

/// A new note on `page`, holding `stored` as its background image, stretched over the whole
/// page (the page has the image's proportions).
fn note_with_image(
    title: &str,
    layer_name: &str,
    created: i64,
    page: PageSize,
    name: &str,
    pdf_page: Option<u32>,
    stored: StoredImage,
) -> NoteFile {
    let mut note = blank_note(title, layer_name, created);
    note.page = page;
    note.background = Background::default();
    note.background_image = Some(BackgroundImage {
        name: name.to_owned(),
        pdf_page,
        rect: ImageRect::fit(page, (stored.width, stored.height), ImageFit::Contain),
        quarter_turns: 0,
        opacity: 1.0,
    });
    let mut file = NoteFile::new(note);
    file.background = Some(BackgroundFile {
        extension: stored.extension().to_owned(),
        bytes: stored.bytes,
    });
    file.thumbnail = file_thumbnail(&file);
    file
}

/// Makes new notes in `binder` from the file at `path`: one per page of `pages` (from 1; all
/// pages when `None`) for a PDF, one for an image. Stops at the first page that fails; the
/// notes made before it stay in the library (each is complete).
pub fn notes_from_file_in(
    library: &Library,
    binder: &str,
    path: &Path,
    pages: Option<&[u32]>,
    layer_name: &str,
) -> CommandResult<Vec<NoteSummary>> {
    let (bytes, kind) = read_source(path)?;
    let name = file_name(path);
    let binder = binder_path(binder);
    let now = now_ms();
    let mut made = Vec::new();
    let mut create = |file: &NoteFile| -> CommandResult<()> {
        let saved = library.create(&binder, file).map_err(library_error)?;
        made.push(summary_of(library, &saved.path, &file.note, saved.stamp));
        Ok(())
    };
    match kind {
        SourceKind::Image => {
            let stored = picture::import_image(&bytes).map_err(picture_error)?;
            let page = page_for_image(stored.width, stored.height);
            let title = note_title(&name, None, 1);
            create(&note_with_image(
                &title, layer_name, now, page, &name, None, stored,
            ))?;
        }
        SourceKind::Pdf => {
            let sizes = pdf_pages::page_sizes(&bytes).map_err(pdf_error)?;
            let all: Vec<u32> = (1..=sizes.len() as u32).collect();
            let chosen = pages.unwrap_or(&all);
            for (i, &number) in chosen.iter().enumerate() {
                let size = number
                    .checked_sub(1)
                    .and_then(|index| sizes.get(index as usize))
                    .ok_or_else(|| pdf_error(PdfError::NoSuchPage(number)))?;
                let (width, height) = render_size(*size);
                let png =
                    pdf_pages::render_page(&bytes, number, width, height).map_err(pdf_error)?;
                let stored = picture::import_page(&png).map_err(picture_error)?;
                let title = note_title(&name, Some(number), sizes.len());
                // One millisecond apart: sorted by date, the pages keep their order.
                let created = now.saturating_add(i as i64);
                create(&note_with_image(
                    &title,
                    layer_name,
                    created,
                    page_for_pdf(*size),
                    &name,
                    Some(number),
                    stored,
                ))?;
            }
        }
    }
    Ok(made)
}

/// The background image's name of each note at `paths` (`None` for a note without one).
pub fn image_names_in(library: &Library, paths: &[String]) -> CommandResult<Vec<Option<String>>> {
    paths
        .iter()
        .map(|path| {
            let loaded = library.read(Path::new(path)).map_err(library_error)?;
            Ok(loaded.file.note.background_image.map(|image| image.name))
        })
        .collect()
}

// --- Commands -----------------------------------------------------------------------------

/// The work of a command, on a blocking thread (decoding and rendering take time).
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|error| CommandError::new("backgroundDamaged", error))?
}

/// What the file at `path` (chosen by the user anywhere) holds.
#[tauri::command]
pub async fn inspect_background_file(path: String) -> CommandResult<BackgroundSource> {
    blocking(move || inspect(Path::new(&path))).await
}

/// Imports an image or a PDF page for the editor, without writing anything.
#[tauri::command]
pub async fn stage_background_image(
    path: String,
    pdf_page: Option<u32>,
) -> CommandResult<StagedBackground> {
    blocking(move || stage_file(staging(), Path::new(&path), pdf_page)).await
}

/// Stages the background image of the note the editor opens.
#[tauri::command]
pub async fn stage_note_background(
    context: State<'_, AppContext>,
    path: String,
) -> CommandResult<Option<StagedBackground>> {
    let library = library_of(&context)?;
    blocking(move || stage_note_image(staging(), &library, Path::new(&path))).await
}

/// Forgets the staged images (the editor closed, or opened another note).
#[tauri::command]
pub fn release_background_images() {
    staging().clear();
}

/// New notes from a PDF (one per chosen page) or an image, in `binder`.
#[tauri::command]
pub async fn create_notes_from_file(
    context: State<'_, AppContext>,
    binder: String,
    path: String,
    pages: Option<Vec<u32>>,
) -> CommandResult<Vec<NoteSummary>> {
    let library = library_of(&context)?;
    let layer = first_layer_name(&crate::diagnostic::report_language(&context));
    blocking(move || {
        notes_from_file_in(&library, &binder, Path::new(&path), pages.as_deref(), layer)
    })
    .await
}

/// The background image's name of each note (the assembly keeps only the first one's).
#[tauri::command]
pub async fn background_image_names(
    context: State<'_, AppContext>,
    paths: Vec<String>,
) -> CommandResult<Vec<Option<String>>> {
    let library = library_of(&context)?;
    blocking(move || image_names_in(&library, &paths)).await
}

/// `{ image, key }` of `render_background`: the editor's image and the key of its pixels.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PlacedImage {
    pub image: BackgroundImage,
    pub key: String,
}

#[cfg(test)]
pub(crate) mod tests {
    use std::fs;
    use std::path::PathBuf;

    use ardoise_core::library::FileStamp;
    use ardoise_core::note::{Paper, Template};
    use ardoise_ink::{Point, Rgba, Stroke, Tool};
    use ardoise_render::{Image, RenderOptions, encode_jpeg, encode_png, render_note};

    use super::*;
    use crate::library::list;
    use crate::library::tests::{library, write_sample};
    use crate::notes::{background_png, save_in};
    use crate::operations::{MergeLayout, SplitSpec, merge_in, split_in};

    /// A `width × height` image: red on the left half, blue on the right.
    pub(crate) fn halves(width: u32, height: u32) -> Image {
        let pixels = (0..height)
            .flat_map(|_| 0..width)
            .flat_map(|x| {
                if x < width / 2 {
                    [220, 30, 30, 255]
                } else {
                    [30, 40, 220, 255]
                }
            })
            .collect();
        Image {
            width,
            height,
            pixels,
        }
    }

    /// Writes `bytes` as `name` in a fresh folder (the user's file, outside the library).
    fn user_file(name: &str, bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    fn rgb(image: &Image, x: u32, y: u32) -> [u8; 3] {
        let [r, g, b, _] = image.pixel(x, y).unwrap();
        [r, g, b]
    }

    fn close(a: [u8; 3], b: [u8; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= 6)
    }

    /// The note at `path` rendered 297 px wide, with its background image.
    fn rendered(library: &Library, path: &Path) -> Image {
        let file = library.read(path).unwrap().file;
        let picture = picture_of(&file.note, file.background.as_ref());
        render_note(
            NoteView::new(&file.note, picture.as_ref()),
            &RenderOptions::fit_width(file.note.page, 297),
        )
        .unwrap()
    }

    fn placed(name: &str, page: PageSize, staged: &StagedBackground) -> BackgroundImage {
        BackgroundImage {
            name: name.to_owned(),
            pdf_page: staged.pdf_page,
            rect: ImageRect::fit(page, (staged.width, staged.height), ImageFit::Stretch),
            quarter_turns: 0,
            opacity: 1.0,
        }
    }

    #[test]
    fn images_are_staged_once_under_their_key() {
        let staging = Staging::default();
        let (_dir, path) = user_file("fiche.png", &encode_png(&halves(40, 20)).unwrap());
        let staged = stage_file(&staging, &path, Some(3)).unwrap();
        assert_eq!(staged.name, "fiche.png");
        assert_eq!(
            (staged.width, staged.height, staged.pdf_page),
            (40, 20, None)
        );
        let again = stage_file(&staging, &path, None).unwrap();
        assert_eq!(again.key, staged.key);
        assert_eq!(staging.len(), 1);
        let member = staging.get(&staged.key).unwrap();
        assert_eq!(member.file.extension, "png");
        let picture = staging.picture(&staged.key).unwrap();
        assert_eq!((picture.width(), picture.height()), (40, 20));
        // A photo is stored as JPEG.
        let photo = Image {
            width: 64,
            height: 48,
            pixels: (0..64 * 48u32)
                .flat_map(|i| {
                    let n = i.wrapping_mul(2_654_435_761);
                    [n as u8, (n >> 8) as u8, (n >> 16) as u8, 255]
                })
                .collect(),
        };
        let (_dir2, jpeg) = user_file("photo.jpg", &encode_jpeg(&photo, 85).unwrap());
        let photo = stage_file(&staging, &jpeg, None).unwrap();
        assert_eq!(staging.get(&photo.key).unwrap().file.extension, "jpg");
        assert_eq!(staging.len(), 2);
        staging.clear();
        assert_eq!(staging.len(), 0);
        assert!(staging.picture(&staged.key).is_none());
        assert_eq!(staged_code(&staging, &staged.key), "backgroundMissing");
    }

    /// CRC-32 of PNG chunks (ISO 3309).
    fn crc32(bytes: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &b in bytes {
            crc ^= u32::from(b);
            for _ in 0..8 {
                crc = if crc & 1 == 1 {
                    (crc >> 1) ^ 0xedb8_8320
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    fn staged_code(staging: &Staging, key: &str) -> &'static str {
        staged(staging, key).unwrap_err().code()
    }

    #[test]
    fn unusable_files_are_refused_with_their_reason() {
        let staging = Staging::default();
        let (_dir, text) = user_file("notes.txt", b"pas une image");
        assert_eq!(
            stage_file(&staging, &text, None).unwrap_err().code(),
            "backgroundUnsupported"
        );
        assert_eq!(inspect(&text).unwrap_err().code(), "backgroundUnsupported");
        // A PNG header announcing 60 000 × 60 000 pixels (its checksum made again).
        let mut png = encode_png(&halves(4, 4)).unwrap();
        png[16..20].copy_from_slice(&60_000u32.to_be_bytes());
        png[20..24].copy_from_slice(&60_000u32.to_be_bytes());
        let crc = crc32(&png[12..29]);
        png[29..33].copy_from_slice(&crc.to_be_bytes());
        let (_dir2, huge) = user_file("huge.png", &png);
        assert_eq!(inspect(&huge).unwrap_err().code(), "backgroundTooLarge");
        let (_dir3, broken) = user_file("broken.jpg", &[0xff, 0xd8, 0xff, 0xe0, 0, 4, 1]);
        assert_eq!(
            stage_file(&staging, &broken, None).unwrap_err().code(),
            "backgroundDamaged"
        );
        let missing = text.with_file_name("absent.png");
        assert_eq!(
            stage_file(&staging, &missing, None).unwrap_err().code(),
            "backgroundUnreadable"
        );
        let (_dir4, fine) = user_file("fiche.png", &encode_png(&halves(4, 4)).unwrap());
        assert_eq!(
            inspect(&fine).unwrap(),
            BackgroundSource {
                name: "fiche.png".into(),
                kind: SourceKind::Image,
                pages: vec![],
            }
        );
        assert_eq!(staging.len(), 0);
    }

    #[test]
    fn pages_and_titles_follow_the_file() {
        // A4 portrait PDF page (595.28 × 841.89 pt): the note's page is A4 portrait.
        let a4 = page_for_pdf(PdfPageSize {
            width: 595.28,
            height: 841.89,
        });
        assert!(
            a4.width.abs_diff(21_000) <= 1 && a4.height.abs_diff(29_700) <= 1,
            "{a4:?}"
        );
        let tiny = page_for_pdf(PdfPageSize {
            width: 1.0,
            height: 1e12,
        });
        assert_eq!((tiny.width, tiny.height), (MIN_PAGE_SIDE, MAX_PAGE_SIDE));
        // Images fit an A4 sheet of their orientation, in their proportions.
        assert_eq!(
            page_for_image(4000, 3000),
            PageSize {
                width: 28_000,
                height: 21_000
            }
        );
        assert_eq!(
            page_for_image(1000, 2000),
            PageSize {
                width: 14_850,
                height: 29_700
            }
        );
        assert_eq!(page_for_image(0, 0).width, 21_000);
        // 300 dpi, at most 4096 px.
        assert_eq!(
            render_size(PdfPageSize {
                width: 595.28,
                height: 841.89
            }),
            (2480, 3508)
        );
        assert_eq!(
            render_size(PdfPageSize {
                width: 1190.55,
                height: 1683.78
            }),
            (2896, MAX_STORED_SIDE)
        );
        assert_eq!(note_title("fiche.pdf", Some(2), 3), "fiche - page 2");
        assert_eq!(note_title("fiche.pdf", Some(1), 1), "fiche");
        assert_eq!(
            note_title("Photo du tableau.JPG", None, 1),
            "Photo du tableau"
        );
        assert_eq!(key_of(b"abc"), key_of(b"abc"));
        assert_ne!(key_of(b"abc"), key_of(b"abd"));
    }

    #[test]
    fn saving_writes_the_staged_image_and_undo_brings_the_old_one_back() {
        let staging = Staging::default();
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Fiche");
        let note = library.read(&saved.path).unwrap().file.note;
        let page = note.page;
        assert_eq!(
            stage_note_image(&staging, &library, &saved.path).unwrap(),
            None
        );

        // Import a first image and save: the member is written, the thumbnail shows it.
        let (_d1, red_file) = user_file("rouge.png", &encode_png(&halves(40, 20)).unwrap());
        let red = stage_file(&staging, &red_file, None).unwrap();
        let mut with_red = note.clone();
        with_red.background_image = Some(placed("rouge.png", page, &red));
        let chosen = chosen_for_save(&staging, &with_red, Some(&red.key)).unwrap();
        let stamp = with_staged(chosen, || {
            save_in(&library, &saved.path, with_red.clone(), &saved.stamp)
        })
        .unwrap();
        let file = library.read(&saved.path).unwrap().file;
        assert_eq!(
            file.background,
            Some(staging.get(&red.key).unwrap().file.clone())
        );
        let thumbnail = ardoise_render::decode_png(file.thumbnail.as_ref().unwrap()).unwrap();
        assert!(close(rgb(&thumbnail, 5, 100), [220, 30, 30]));
        assert!(close(
            rgb(&rendered(&library, &saved.path), 280, 100),
            [30, 40, 220]
        ));

        // The editor reopens the note: its member is staged under the same key.
        let reopened = stage_note_image(&staging, &library, &saved.path)
            .unwrap()
            .unwrap();
        assert_eq!(reopened.key, red.key);
        assert_eq!(reopened.name, "rouge.png");

        // Replaced by another image, saved, then undone and saved again: the first image
        // comes back from its key, nothing is lost.
        let blue_image = Image {
            pixels: [10, 10, 200, 255].repeat(16),
            width: 4,
            height: 4,
        };
        let (_d2, blue_file) = user_file("bleu.png", &encode_png(&blue_image).unwrap());
        let blue = stage_file(&staging, &blue_file, None).unwrap();
        let mut with_blue = note.clone();
        with_blue.background_image = Some(placed("bleu.png", page, &blue));
        let chosen = chosen_for_save(&staging, &with_blue, Some(&blue.key)).unwrap();
        let stamp =
            with_staged(chosen, || save_in(&library, &saved.path, with_blue, &stamp)).unwrap();
        assert!(close(
            rgb(&rendered(&library, &saved.path), 5, 100),
            [10, 10, 200]
        ));
        let chosen = chosen_for_save(&staging, &with_red, Some(&red.key)).unwrap();
        let stamp = with_staged(chosen, || {
            save_in(&library, &saved.path, with_red.clone(), &stamp)
        })
        .unwrap();
        assert!(close(
            rgb(&rendered(&library, &saved.path), 5, 100),
            [220, 30, 30]
        ));

        // Removed: the member goes too (no orphan pixels left in the file).
        let stamp = save_in(&library, &saved.path, note.clone(), &stamp).unwrap();
        let file = library.read(&saved.path).unwrap().file;
        assert_eq!((file.note.background_image, file.background), (None, None));

        // A key that is not staged is refused before anything is written.
        staging.clear();
        assert_eq!(
            chosen_for_save(&staging, &with_red, Some(&red.key))
                .unwrap_err()
                .code(),
            "backgroundMissing"
        );
        assert!(
            chosen_for_save(&staging, &note, Some(&red.key))
                .unwrap()
                .is_none()
        );
        assert_eq!(FileStamp::of(&saved.path).unwrap(), stamp);
    }

    #[test]
    fn a_save_without_a_key_keeps_the_file_member() {
        let staging = Staging::default();
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Fiche");
        let (_d, image) = user_file("rouge.png", &encode_png(&halves(40, 20)).unwrap());
        let red = stage_file(&staging, &image, None).unwrap();
        let mut note = library.read(&saved.path).unwrap().file.note;
        note.background_image = Some(placed("rouge.png", note.page, &red));
        let chosen = chosen_for_save(&staging, &note, Some(&red.key)).unwrap();
        let stamp = with_staged(chosen, || {
            save_in(&library, &saved.path, note.clone(), &saved.stamp)
        })
        .unwrap();
        // Another change (the image moved by a quarter turn elsewhere): saved without a key.
        if let Some(image) = note.background_image.as_mut() {
            image.opacity = 0.5;
        }
        save_in(&library, &saved.path, note, &stamp).unwrap();
        let file = library.read(&saved.path).unwrap().file;
        assert_eq!(
            file.background,
            Some(staging.get(&red.key).unwrap().file.clone())
        );
        assert_eq!(file.note.background_image.map(|i| i.opacity), Some(0.5));
    }

    /// A note of the library with the red and blue `rouge.png` as its background image.
    fn with_red_image(library: &Library) -> (PathBuf, FileStamp, Note, Arc<Staged>) {
        let staging = Staging::default();
        let saved = write_sample(library, "", "Fiche");
        let (_d, image) = user_file("rouge.png", &encode_png(&halves(40, 20)).unwrap());
        let red = stage_file(&staging, &image, None).unwrap();
        let mut note = library.read(&saved.path).unwrap().file.note;
        note.background_image = Some(placed("rouge.png", note.page, &red));
        let chosen = chosen_for_save(&staging, &note, Some(&red.key)).unwrap();
        let stamp = with_staged(chosen, || {
            save_in(library, &saved.path, note.clone(), &saved.stamp)
        })
        .unwrap();
        (saved.path, stamp, note, staging.get(&red.key).unwrap())
    }

    #[test]
    fn a_save_without_a_key_may_not_describe_another_image() {
        use crate::notes::save_with_key;
        let (_dir, library) = library();
        let (path, stamp, note, red) = with_red_image(&library);

        // The same image, moved by a quarter turn and fainter: its member is the file's.
        let mut turned = note.clone();
        if let Some(image) = turned.background_image.as_mut() {
            image.quarter_turns = 1;
            image.rect.x += 100.0;
            image.opacity = 0.5;
        }
        let stamp = save_with_key(&library, &path, turned, &stamp, None).unwrap();
        assert_eq!(
            library.read(&path).unwrap().file.background,
            Some(red.file.clone())
        );

        // Another file, or another page of it: its pixels are not given, nothing is written.
        for other in [
            BackgroundImage {
                name: "bleu.png".into(),
                ..note.background_image.clone().unwrap()
            },
            BackgroundImage {
                pdf_page: Some(2),
                ..note.background_image.clone().unwrap()
            },
        ] {
            let mut changed = note.clone();
            changed.background_image = Some(other);
            let error = save_with_key(&library, &path, changed.clone(), &stamp, None).unwrap_err();
            assert_eq!(error.code(), "backgroundMissing");
            assert_eq!(FileStamp::of(&path).unwrap(), stamp);
            // A split without a key is refused the same way, before anything is created.
            let count = list(&library).notes.len();
            let error = split_in(
                &library,
                &path,
                changed,
                &stamp,
                &SplitSpec::Time { at: 120 },
                "Suite",
                None,
            )
            .unwrap_err();
            assert_eq!(error.code(), "backgroundMissing");
            assert_eq!(list(&library).notes.len(), count);
        }

        // "Save as a copy" without a key: the blank file has no image, so neither its pixels.
        let blank = library
            .create(
                Path::new(""),
                &NoteFile::new(blank_note("Fiche (copie)", "Calque 1", 1)),
            )
            .unwrap();
        let error =
            save_with_key(&library, &blank.path, note.clone(), &blank.stamp, None).unwrap_err();
        assert_eq!(error.code(), "backgroundMissing");

        // Without its image: saved, and the member goes.
        let mut removed = note;
        removed.background_image = None;
        save_with_key(&library, &path, removed, &stamp, None).unwrap();
        assert_eq!(library.read(&path).unwrap().file.background, None);
    }

    #[test]
    fn a_split_after_the_image_was_removed_gives_the_new_note_no_member() {
        let (_dir, library) = library();
        let (path, stamp, mut note, _red) = with_red_image(&library);
        // The editor removed the image (not saved), then splits.
        note.background_image = None;
        let result = split_in(
            &library,
            &path,
            note,
            &stamp,
            &SplitSpec::Time { at: 120 },
            "Suite",
            None,
        )
        .unwrap();
        let created = library.read(Path::new(&result.created.path)).unwrap().file;
        assert_eq!(
            (created.note.background_image, created.background),
            (None, None)
        );
        let kept = library.read(&path).unwrap().file;
        assert_eq!((kept.note.background_image, kept.background), (None, None));
    }

    #[test]
    fn the_thumbnail_of_a_save_uses_the_decoded_staged_pixels() {
        let staging = Staging::default();
        let (_dir, library) = library();
        let saved = write_sample(&library, "", "Fiche");
        let (_d, image) = user_file("rouge.png", &encode_png(&halves(40, 20)).unwrap());
        let red = stage_file(&staging, &image, None).unwrap();
        let mut note = library.read(&saved.path).unwrap().file.note;
        note.background_image = Some(placed("rouge.png", note.page, &red));
        // The pixels decoded once by the staging area (here, blue ones stand for them): the
        // save draws the thumbnail with them instead of decoding the member again.
        let blue = Image {
            pixels: [10, 10, 200, 255].repeat(16),
            width: 4,
            height: 4,
        };
        let mut chosen = chosen_for_save(&staging, &note, Some(&red.key))
            .unwrap()
            .unwrap();
        assert!(chosen.picture.is_some());
        chosen.picture = Some(Arc::new(
            Picture::decode(encode_png(&blue).unwrap()).unwrap(),
        ));
        with_staged(Some(chosen), || {
            save_in(&library, &saved.path, note, &saved.stamp)
        })
        .unwrap();
        let file = library.read(&saved.path).unwrap().file;
        assert_eq!(
            file.background,
            Some(staging.get(&red.key).unwrap().file.clone())
        );
        let thumbnail = ardoise_render::decode_png(file.thumbnail.as_ref().unwrap()).unwrap();
        assert!(close(rgb(&thumbnail, 5, 100), [10, 10, 200]));
    }

    #[test]
    fn the_editor_page_and_the_exports_draw_the_staged_image() {
        let staging = staging();
        let (_d, image) = user_file("demi.png", &encode_png(&halves(40, 20)).unwrap());
        let staged = stage_file(staging, &image, None).unwrap();
        let page = PageSize {
            width: 4000,
            height: 2000,
        };
        let image = placed("demi.png", page, &staged);
        let picture = staging.picture(&staged.key).unwrap();
        let png = background_png(
            page,
            Background {
                paper: Paper::Cream,
                template: Template::Seyes,
                texture: false,
            },
            400,
            Some((image.clone(), &picture)),
        )
        .unwrap();
        let drawn = ardoise_render::decode_png(&png).unwrap();
        assert!(close(rgb(&drawn, 20, 100), [220, 30, 30]));
        assert!(close(rgb(&drawn, 380, 100), [30, 40, 220]));

        // An unsaved editor note exported with the key; a key no longer staged is refused.
        let mut note = crate::library::tests::sample_note("Export");
        note.page = page;
        note.background_image = Some(image);
        let source = crate::export::ExportSource {
            path: String::new(),
            note: Some(note.clone()),
            image_key: Some(staged.key.clone()),
        };
        let loaded = crate::export::load_sources(None, vec![source.clone()]).unwrap();
        assert!(loaded[0].picture.is_some());
        let options = crate::export::ExportOptions {
            dpi: 72,
            background: true,
            quality: 90,
            pdf_page: crate::export::PdfPageSize::Actual,
        };
        let bytes = crate::export::image_bytes(
            loaded[0].view(),
            crate::export::ExportFormat::Png,
            &options,
        )
        .unwrap();
        let exported = ardoise_render::decode_png(&bytes).unwrap();
        assert!(close(rgb(&exported, 3, 20), [220, 30, 30]));
        let missing = crate::export::ExportSource {
            image_key: Some("0-0".into()),
            ..source
        };
        assert_eq!(
            crate::export::load_sources(None, vec![missing])
                .unwrap_err()
                .code(),
            "backgroundMissing"
        );
    }

    fn stroke_at(x: f32, y: f32) -> Stroke {
        Stroke {
            tool: Tool::Ballpoint,
            color: Rgba::BLACK,
            width: 60.0,
            layer: 0,
            points: (0..10)
                .map(|i| Point {
                    x: x + 100.0 * i as f32,
                    y,
                    pressure: 0.5,
                    t: 10 * i + (x as u32),
                })
                .collect(),
        }
    }

    #[test]
    fn a_split_gives_both_notes_the_image() {
        let staging = Staging::default();
        let (_dir, library) = library();
        let saved = write_sample(&library, "Classe", "Fiche");
        let opened = library.read(&saved.path).unwrap();
        let (_d, image) = user_file("demi.png", &encode_png(&halves(40, 20)).unwrap());
        let staged = stage_file(&staging, &image, None).unwrap();
        let mut note = opened.file.note.clone();
        note.background_image = Some(placed("demi.png", note.page, &staged));
        let chosen = chosen_for_save(&staging, &note, Some(&staged.key)).unwrap();
        let result = split_in(
            &library,
            &saved.path,
            note,
            &opened.stamp,
            &SplitSpec::Time { at: 120 },
            "Suite",
            chosen,
        )
        .unwrap();
        let member = Some(staging.get(&staged.key).unwrap().file.clone());
        let kept = library.read(&saved.path).unwrap().file;
        let created = library.read(Path::new(&result.created.path)).unwrap().file;
        assert_eq!(kept.background, member);
        assert_eq!(created.background, member);
        assert_eq!(created.note.background_image, kept.note.background_image);
        assert!(created.note.background_image.is_some());
        let thumbnail = ardoise_render::decode_png(created.thumbnail.as_ref().unwrap()).unwrap();
        assert!(close(rgb(&thumbnail, 5, 100), [220, 30, 30]));
    }

    #[test]
    fn an_assembly_keeps_the_first_image_on_the_first_page_only() {
        let (_dir, library) = library();
        let picture_file = BackgroundFile {
            extension: "png".into(),
            bytes: encode_png(&halves(40, 20)).unwrap(),
        };
        let page = PageSize {
            width: 4000,
            height: 2000,
        };
        let write = |title: &str, image: Option<ImageRect>, x: f32| {
            let mut note = blank_note(title, "Calque 1", 1_792_843_200_000);
            note.page = page;
            note.strokes.push(stroke_at(x, 1000.0));
            note.background_image = image.map(|rect| BackgroundImage {
                name: format!("{title}.png"),
                pdf_page: None,
                rect,
                quarter_turns: 0,
                opacity: 1.0,
            });
            let mut file = NoteFile::new(note);
            if file.note.background_image.is_some() {
                file.background = Some(picture_file.clone());
            }
            library.create(Path::new(""), &file).unwrap().path
        };
        // The first image covers its page and reaches 1000 units beyond its bottom edge.
        let cover = ImageRect {
            x: 0.0,
            y: 0.0,
            width: 4000.0,
            height: 3000.0,
        };
        let first = write("premier", Some(cover), 500.0);
        let second = write(
            "second",
            Some(ImageRect {
                x: 0.0,
                y: 0.0,
                width: 4000.0,
                height: 2000.0,
            }),
            800.0,
        );
        let paths = [first, second].map(|p| p.to_string_lossy().into_owned());
        assert_eq!(
            image_names_in(&library, &paths).unwrap(),
            [
                Some("premier.png".to_owned()),
                Some("second.png".to_owned())
            ]
        );
        // End to end: cut at the first page's bottom edge (2000 of 3000 units: 14 of 20 rows,
        // rounded out to whole pixels).
        let merged = merge_in(&library, &paths, MergeLayout::EndToEnd, "Les deux").unwrap();
        let file = library.read(Path::new(&merged.path)).unwrap().file;
        let image = file.note.background_image.clone().unwrap();
        assert_eq!(image.name, "premier.png");
        assert_eq!(image.rect.y, 0.0);
        assert!(
            image.rect.height <= 2150.0 && image.rect.height >= 2000.0,
            "{:?}",
            image.rect
        );
        let stored = picture::dimensions(&file.background.as_ref().unwrap().bytes).unwrap();
        assert_eq!(stored, (40, 14));
        // Overlay of pages of the same size: nothing to cover, the image stays whole.
        let merged = merge_in(&library, &paths, MergeLayout::Overlay, "Superposées").unwrap();
        let file = library.read(Path::new(&merged.path)).unwrap().file;
        assert_eq!(file.note.background_image.unwrap().rect, cover);
        assert_eq!(file.background, Some(picture_file.clone()));
        // A first note without an image: none in the assembly, the second one's is dropped.
        let reversed = [paths[1].clone(), paths[0].clone()];
        let plain = write("sans", None, 300.0).to_string_lossy().into_owned();
        let merged = merge_in(
            &library,
            &[plain, reversed[0].clone()],
            MergeLayout::Overlay,
            "Sans image",
        )
        .unwrap();
        let file = library.read(Path::new(&merged.path)).unwrap().file;
        assert_eq!((file.note.background_image, file.background), (None, None));
    }

    /// A two-page PDF made by the app's own exporter: an A4 landscape note with a thick black
    /// stroke across the left half, and an A4 portrait one.
    #[cfg(windows)]
    fn two_page_pdf() -> Vec<u8> {
        use ardoise_core::note::{A4_LANDSCAPE, A4_PORTRAIT};
        let mut first = blank_note("Page 1", "Calque 1", 0);
        first.page = A4_LANDSCAPE;
        first.strokes.push(Stroke {
            tool: Tool::FeltTip,
            color: Rgba::BLACK,
            width: 2000.0,
            layer: 0,
            points: (0..20)
                .map(|i| Point {
                    x: 2000.0,
                    y: 1000.0 + 1000.0 * i as f32,
                    pressure: 1.0,
                    t: i,
                })
                .collect(),
        });
        let mut second = blank_note("Page 2", "Calque 1", 0);
        second.page = A4_PORTRAIT;
        ardoise_render::notes_to_pdf(&[&first, &second], &ardoise_render::PdfOptions::default())
            .unwrap()
    }

    /// Windows renders the PDF pages: sizes, a page staged at 300 dpi, two notes made from them.
    #[cfg(windows)]
    #[test]
    fn a_pdf_of_two_pages_becomes_two_notes() {
        let (_d, pdf) = user_file("fiche.pdf", &two_page_pdf());
        let source = inspect(&pdf).unwrap();
        println!("pages of the PDF: {:?}", source.pages);
        assert_eq!(source.kind, SourceKind::Pdf);
        assert_eq!(source.pages.len(), 2);
        // 297 × 216 mm, then 216 × 297 mm, in points.
        assert!(
            (source.pages[0].width - 841.89).abs() < 1.0,
            "{:?}",
            source.pages
        );
        assert!(
            (source.pages[0].height - 612.28).abs() < 1.0,
            "{:?}",
            source.pages
        );
        assert!(
            (source.pages[1].width - 612.28).abs() < 1.0,
            "{:?}",
            source.pages
        );

        let staging = Staging::default();
        let staged = stage_file(&staging, &pdf, Some(2)).unwrap();
        assert_eq!(staged.pdf_page, Some(2));
        // 300 dpi whatever the display's scale.
        println!("page 2 rendered at {} x {} px", staged.width, staged.height);
        assert!(staged.width.abs_diff(2551) <= 2, "{}", staged.width);
        assert!(staged.height.abs_diff(3508) <= 2, "{}", staged.height);
        assert_eq!(staging.get(&staged.key).unwrap().file.extension, "png");
        assert_eq!(
            stage_file(&staging, &pdf, Some(3)).unwrap_err().code(),
            "pdfNoSuchPage"
        );

        let (_dir, library) = library();
        let made = notes_from_file_in(&library, "Fiches", &pdf, None, "Calque 1").unwrap();
        assert_eq!(made.len(), 2);
        assert_eq!(made[0].title, "fiche - page 1");
        assert_eq!(made[1].title, "fiche - page 2");
        assert_eq!(made[0].binder, "Fiches");
        let file = library.read(Path::new(&made[0].path)).unwrap().file;
        assert!(
            file.note.page.width.abs_diff(29_700) <= 2,
            "{:?}",
            file.note.page
        );
        assert!(
            file.note.page.height.abs_diff(21_600) <= 2,
            "{:?}",
            file.note.page
        );
        let image = file.note.background_image.clone().unwrap();
        assert_eq!(
            (image.name.as_str(), image.pdf_page),
            ("fiche.pdf", Some(1))
        );
        assert!(image.rect.width >= 29_690.0 && image.rect.height >= 21_590.0);
        assert!(file.thumbnail.is_some());
        // The PDF's black stroke at x = 20 mm is in the note's background.
        let drawn = rendered(&library, Path::new(&made[0].path));
        assert!(
            rgb(&drawn, 20, 100).iter().all(|&v| v < 60),
            "{:?}",
            rgb(&drawn, 20, 100)
        );
        assert_eq!(rgb(&drawn, 200, 100), [255, 255, 255]);
        // Only the second page, chosen.
        let made = notes_from_file_in(&library, "", &pdf, Some(&[2]), "Calque 1").unwrap();
        assert_eq!(made.len(), 1);
        let file = library.read(Path::new(&made[0].path)).unwrap().file;
        assert_eq!(file.note.background_image.unwrap().pdf_page, Some(2));
        assert!(file.note.page.height > file.note.page.width);
        assert_eq!(
            notes_from_file_in(&library, "", &pdf, Some(&[5]), "Calque 1")
                .unwrap_err()
                .code(),
            "pdfNoSuchPage"
        );
    }

    /// A PDF that needs a password is refused with its own code.
    #[cfg(windows)]
    #[test]
    fn a_pdf_with_a_password_is_refused() {
        use lopdf::{Document, EncryptionState, EncryptionVersion, Object, Permissions};
        let mut document = Document::load_mem(&two_page_pdf()).unwrap();
        let id = Object::String(b"0123456789abcdef".to_vec(), lopdf::StringFormat::Literal);
        document.trailer.set("ID", vec![id.clone(), id]);
        let version = EncryptionVersion::V2 {
            document: &document,
            owner_password: "propriétaire",
            user_password: "secret",
            key_length: 128,
            permissions: Permissions::all(),
        };
        let state = EncryptionState::try_from(version).unwrap();
        document.encrypt(&state).unwrap();
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let (_d, pdf) = user_file("protégé.pdf", &bytes);
        assert_eq!(inspect(&pdf).unwrap_err().code(), "pdfEncrypted");
        let staging = Staging::default();
        assert_eq!(
            stage_file(&staging, &pdf, Some(1)).unwrap_err().code(),
            "pdfEncrypted"
        );
        // A damaged PDF.
        let (_d2, broken) = user_file("cassé.pdf", b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog");
        assert_eq!(inspect(&broken).unwrap_err().code(), "backgroundDamaged");
    }

    /// Elsewhere than on Windows, PDF files are refused with their own code.
    #[cfg(not(windows))]
    #[test]
    fn pdf_files_need_windows() {
        let (_d, pdf) = user_file("fiche.pdf", b"%PDF-1.7\n%%EOF\n");
        assert_eq!(inspect(&pdf).unwrap_err().code(), "pdfUnsupported");
        let (_dir, library) = library();
        assert_eq!(
            notes_from_file_in(&library, "", &pdf, None, "Layer 1")
                .unwrap_err()
                .code(),
            "pdfUnsupported"
        );
    }

    #[test]
    fn an_image_becomes_one_note_of_its_proportions() {
        let (_dir, library) = library();
        let (_d, image) = user_file("Tableau.png", &encode_png(&halves(400, 300)).unwrap());
        let made = notes_from_file_in(&library, "", &image, None, "Layer 1").unwrap();
        assert_eq!(made.len(), 1);
        assert_eq!(made[0].title, "Tableau");
        let file = library.read(Path::new(&made[0].path)).unwrap().file;
        assert_eq!(file.note.page, page_for_image(400, 300));
        assert_eq!(file.note.layers[0].name, "Layer 1");
        let image = file.note.background_image.clone().unwrap();
        assert_eq!(
            (image.pdf_page, image.quarter_turns, image.opacity),
            (None, 0, 1.0)
        );
        let drawn = rendered(&library, Path::new(&made[0].path));
        assert!(close(rgb(&drawn, 10, 100), [220, 30, 30]));
        assert!(close(rgb(&drawn, 287, 100), [30, 40, 220]));
    }
}
