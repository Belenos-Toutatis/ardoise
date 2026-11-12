//! The paper and the image of the current page during live mode (SPEC §4.2 « Papier et fond
//! pendant le direct », `docs/questions.md` Q13; docs/design/paper-projection-narration.md §1):
//! what the Page and Fond tabs of the live window ask, and the pixels a page keeps of its
//! background image.
//!
//! - **The pixels** ([`PageImage`]): the member a page writes with its note (`background.png` or
//!   `background.jpg`, as staged: at most 4096 px a side), shared between the page, its writes and
//!   whoever draws it; never decoded under the hub's lock.
//! - **Decoded pictures** ([`DecodedPictures`]): a 4096 × 4096 picture is 64 MB of pixels, so they
//!   are not kept per page: the last [`DECODED_KEPT`] decoded, by key, for the thumbnails, the
//!   windows' paper (`live_page_background`) and the share's (`share::live_paper`).
//! - **What the Fond tab asks** ([`LivePageImageOp`]), checked here ([`checked_image`],
//!   [`within_reach`]) and applied by the session ([`ImageChange`]).
//! - **A page turned with the tablet** ([`refitted`]): its image is placed again with the fit it
//!   has, as `currentFit` (`lib/background.ts`) recognises it.

use std::fmt;
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};

use ardoise_core::format::BackgroundFile;
use ardoise_core::note::{BackgroundImage, ImageFit, ImageRect, PageSize};
use ardoise_render::Picture;
use ardoise_render::picture::dimensions;
use serde::Deserialize;

use super::model::ImagePixels;
use crate::background::{Staged, key_of};

/// Decoded pictures kept for the pages of live mode (each up to 64 MB), as the editor's staging
/// area keeps its own (`background::DECODED_KEPT`): the page on screen and the one before it.
pub const DECODED_KEPT: usize = 2;

/// Longest file name an image of a page may have, in characters (the editor's file names are
/// shorter: a longer one is a mistake of the interface).
pub const MAX_IMAGE_NAME: usize = 255;

/// How far an image's rectangle may reach beyond its page, in page sizes on each side: the editor
/// lets an image reach outside the page (only what is on it is drawn), not to infinity.
pub const MAX_IMAGE_REACH: f64 = 16.0;

/// The pixels of a live page's background image: the member written with the page, and its key.
#[derive(Clone, PartialEq, Eq)]
pub struct PageImage {
    /// What `background.png` / `background.jpg` holds (as staged: at most 4096 px a side).
    pub member: BackgroundFile,
    pub width: u32,
    pub height: u32,
    /// `background::key_of(&member.bytes)`: tells two files of the same name apart (windows, share).
    pub key: String,
}

impl PageImage {
    /// The pixels the live window staged (`stage_background_image`) under `key`, the key the staging
    /// area gave them (`background::key_of` of their bytes): taken as they are, nothing copied.
    pub fn staged(key: String, staged: Staged) -> PageImage {
        PageImage {
            member: staged.file,
            width: staged.width,
            height: staged.height,
            key,
        }
    }

    /// Back to what the staging area holds (a `set` refused).
    pub fn into_staged(self) -> Staged {
        Staged {
            file: self.member,
            width: self.width,
            height: self.height,
        }
    }

    /// The pixels of a note file's member (a resumed sheet): their size read from the header only
    /// (nothing decoded), their key hashed from the bytes. `None` when the header cannot be read:
    /// the member stays in the file, nothing is drawn.
    pub fn of_member(member: &BackgroundFile) -> Option<PageImage> {
        let (width, height) = dimensions(&member.bytes).ok()?;
        Some(PageImage {
            key: key_of(&member.bytes),
            member: member.clone(),
            width,
            height,
        })
    }

    /// What the snapshot says of them (`LiveSnapshot::image_pixels`).
    pub fn pixels(&self) -> ImagePixels {
        ImagePixels {
            key: self.key.clone(),
            width: self.width,
            height: self.height,
        }
    }

    /// The decoded pixels, from the pictures kept for live mode or decoded now (slow: never under
    /// the hub's lock). `None` when they cannot be decoded.
    pub fn picture(&self) -> Option<Arc<Picture>> {
        decoded().picture(self)
    }
}

impl fmt::Debug for PageImage {
    /// Not the bytes: megabytes of them.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PageImage")
            .field("key", &self.key)
            .field("extension", &self.member.extension)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}

/// The last decoded pictures of the pages, most recent first, by [`PageImage::key`]; one that cannot
/// be decoded is kept as such, so that it is not decoded again at every request.
#[derive(Default)]
pub struct DecodedPictures {
    kept: Mutex<Vec<(String, Option<Arc<Picture>>)>>,
    /// Pictures decoded so far.
    #[cfg(test)]
    decodes: std::sync::atomic::AtomicUsize,
}

/// The decoded pictures of this process.
fn decoded() -> &'static DecodedPictures {
    static DECODED: LazyLock<DecodedPictures> = LazyLock::new(DecodedPictures::default);
    &DECODED
}

impl DecodedPictures {
    fn lock(&self) -> MutexGuard<'_, Vec<(String, Option<Arc<Picture>>)>> {
        self.kept.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The decoded pixels of `image`: kept, or decoded outside this lock (two threads may then
    /// decode the same picture once each; it is kept once).
    pub fn picture(&self, image: &PageImage) -> Option<Arc<Picture>> {
        {
            let mut kept = self.lock();
            if let Some(index) = kept.iter().position(|(key, _)| *key == image.key) {
                let entry = kept.remove(index);
                let picture = entry.1.clone();
                kept.insert(0, entry);
                return picture;
            }
        }
        #[cfg(test)]
        self.decodes
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let picture = Picture::decode(image.member.bytes.clone())
            .ok()
            .map(Arc::new);
        let mut kept = self.lock();
        kept.retain(|(key, _)| *key != image.key);
        kept.insert(0, (image.key.clone(), picture.clone()));
        kept.truncate(DECODED_KEPT);
        picture
    }

    /// Pictures decoded so far.
    #[cfg(test)]
    pub fn decodes(&self) -> usize {
        self.decodes.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Pictures kept.
    #[cfg(test)]
    pub fn kept(&self) -> usize {
        self.lock().len()
    }
}

/// What the Fond tab asks of the page on screen (`LivePageImageOp` in `live.ts`). Checked by the
/// hub, not here: a bad value gets the coded error rather than the rejection of the command's
/// arguments.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum LivePageImageOp {
    /// The image staged under `key` (`stage_background_image`), placed as `image` says.
    Set {
        key: String,
        image: BackgroundImage,
    },
    /// The same pixels placed otherwise (fit, quarter turn, opacity).
    Place {
        image: BackgroundImage,
    },
    Remove,
}

/// What the session does with the image of the page on screen, once the hub checked it (the pixels
/// of a `set` taken out of the staging area before the hub's lock).
#[derive(Debug)]
pub enum ImageChange {
    Set {
        image: BackgroundImage,
        pixels: Arc<PageImage>,
    },
    Place {
        image: BackgroundImage,
    },
    Remove,
}

/// Why a change of a page's paper or image is refused. Nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PaperError {
    /// The page on screen is another one: the choice was made before a page change.
    #[error("the page changed since this was chosen")]
    Stale,
    #[error("the image's name must have 1 to {MAX_IMAGE_NAME} characters")]
    BadName,
    #[error("the pages of a PDF are numbered from 1")]
    BadPdfPage,
    /// Not finite, not of positive size, or too far from the page ([`within_reach`]).
    #[error("the image's place is not valid")]
    BadRect,
    #[error("the image's opacity is not a number")]
    BadOpacity,
    /// A `place` of another image than the page's (another name or page of the PDF), or on a page
    /// without one: new pixels come with a `set`.
    #[error("the page has no such image to place")]
    NotThisImage,
    /// The pixels of a `set` are not in the staging area (any more).
    #[error("the background image {0} is no longer in memory")]
    Missing(String),
}

impl PaperError {
    /// The error's code for the interface (`errorCodes.*`): a stale choice asks for it again, an
    /// image no longer staged for another import; any other is a mistake of the interface.
    pub fn code(&self) -> &'static str {
        match self {
            PaperError::Stale => "liveEditStale",
            PaperError::Missing(_) => "backgroundMissing",
            _ => "liveEditInvalid",
        }
    }
}

/// The description of an image as a page takes it: refused when its name is empty or too long, its
/// PDF page is 0, its rectangle is not finite or not of positive size, or its opacity is not a
/// number; the opacity clamped to 0..=1 and the quarter turns taken modulo 4. How far it reaches
/// depends on the page ([`within_reach`], checked under the hub's lock).
pub fn checked_image(image: BackgroundImage) -> Result<BackgroundImage, PaperError> {
    let length = image.name.chars().count();
    if length == 0 || length > MAX_IMAGE_NAME {
        return Err(PaperError::BadName);
    }
    if image.pdf_page == Some(0) {
        return Err(PaperError::BadPdfPage);
    }
    let rect = image.rect;
    let finite = [rect.x, rect.y, rect.width, rect.height]
        .iter()
        .all(|v| v.is_finite());
    if !finite || rect.width <= 0.0 || rect.height <= 0.0 {
        return Err(PaperError::BadRect);
    }
    if !image.opacity.is_finite() {
        return Err(PaperError::BadOpacity);
    }
    Ok(BackgroundImage {
        opacity: image.opacity.clamp(0.0, 1.0),
        quarter_turns: image.quarter_turns % 4,
        ..image
    })
}

/// Whether `rect` stays within [`MAX_IMAGE_REACH`] page sizes of a page of `page`, on every side.
pub fn within_reach(rect: &ImageRect, page: PageSize) -> bool {
    let (width, height) = (f64::from(page.width), f64::from(page.height));
    let (x, y) = (f64::from(rect.x), f64::from(rect.y));
    let (right, bottom) = (x + f64::from(rect.width), y + f64::from(rect.height));
    x >= -MAX_IMAGE_REACH * width
        && y >= -MAX_IMAGE_REACH * height
        && right <= (MAX_IMAGE_REACH + 1.0) * width
        && bottom <= (MAX_IMAGE_REACH + 1.0) * height
}

/// The size of pixels `pixels` once turned `quarter_turns` clockwise (`displayedSize` in
/// `lib/background.ts`).
fn displayed(pixels: (u32, u32), quarter_turns: u8) -> (u32, u32) {
    if quarter_turns % 2 == 1 {
        (pixels.1, pixels.0)
    } else {
        pixels
    }
}

/// The fit `image` of pixels `pixels` has on `page`, as `currentFit` (`lib/background.ts`)
/// recognises it, in the order `turnImageAlone` asks (contain, cover, stretch: an image of the
/// page's proportions has the same rectangle for all three, and contain keeps it whole on a page
/// turned); `None` for an image placed by hand.
pub fn current_fit(
    image: &BackgroundImage,
    page: PageSize,
    pixels: (u32, u32),
) -> Option<ImageFit> {
    let shown = displayed(pixels, image.quarter_turns);
    let near = |a: f32, b: f32| (a - b).abs() <= 1f32.max(b.abs() * 1e-4);
    let i = image.rect;
    [ImageFit::Contain, ImageFit::Cover, ImageFit::Stretch]
        .into_iter()
        .find(|&fit| {
            let r = ImageRect::fit(page, shown, fit);
            near(i.x, r.x) && near(i.y, r.y) && near(i.width, r.width) && near(i.height, r.height)
        })
}

/// Where `image` (of pixels `pixels`), placed on a page of size `before`, goes on the page of size
/// `after` it became (the tablet turned, before any ink): with the fit it has, upright (its quarter
/// turns are its own, not the page's); contain when it was placed by hand.
pub fn refitted(
    image: &BackgroundImage,
    before: PageSize,
    after: PageSize,
    pixels: (u32, u32),
) -> ImageRect {
    let fit = current_fit(image, before, pixels).unwrap_or(ImageFit::Contain);
    ImageRect::fit(after, displayed(pixels, image.quarter_turns), fit)
}

#[cfg(test)]
pub(crate) mod tests;
