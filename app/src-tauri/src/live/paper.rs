//! The paper and the image of the current page during live mode (SPEC §4.2 « Papier et fond
//! pendant le direct », `docs/questions.md` Q13; docs/design/paper-projection-narration.md §1):
//! what the Page and Fond tabs of the live window ask, and the pixels a page keeps of its
//! background image.
//!
//! Package 0 (the contract): the shapes only, no logic yet. Package A fills in the rest.

use ardoise_core::format::BackgroundFile;
use ardoise_core::note::BackgroundImage;
use serde::Deserialize;

/// The pixels of a live page's background image: the member written with the page, and its key.
// Read once package A builds the pages' images (nothing builds one before).
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageImage {
    /// What `background.png` / `background.jpg` holds (as staged: at most 4096 px a side).
    pub member: BackgroundFile,
    pub width: u32,
    pub height: u32,
    /// `background::key_of(&member.bytes)`: tells two files of the same name apart (windows, share).
    pub key: String,
}

/// What the Fond tab asks of the page on screen (`LivePageImageOp` in `live.ts`). Checked by the
/// hub, not here: a bad value gets the coded error rather than the rejection of the command's
/// arguments.
#[allow(dead_code)]
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
