//! The paper of the live page, for the viewers of the share (docs/design/lan-share.md, "The paper
//! of a live page"): a resumed sheet may have a template, a texture, a background image or a page
//! of a PDF, which a browser cannot draw by itself. Ordinary live pages are plain paper: they get
//! no key, no picture and no thread.
//!
//! - **The snapshots** sent to the viewers name the paper the page wants ([`paper_key`]), so a page
//!   change shows or hides a picture at once, whatever the order in which things arrive.
//! - **Watching** ([`LivePapers::saw`]) runs on the hub's flusher thread: it only compares a few
//!   fields under a lock of its own, for the batches that hold a snapshot.
//! - **Rendering** runs on a thread of its own ([`LivePapers::run`]), started when there is a paper
//!   to make and ended when there is none: the hub's lock is held only to look up the sheet's file.
//!   The picture is what `page.rs` makes for the editor's note, kept for when the teacher comes back
//!   to the sheet; it reaches the share through [`ardoise_share::Share::show_live_paper`], which
//!   holds it for the viewers to fetch, apart from the event stream.

use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};

use ardoise_core::note::{Background, BackgroundImage, PageSize, Paper as PaperColour};
use ardoise_share::LivePaper;

use super::page::{Paper, is_plain_paper};
use super::render_paper_png;
use crate::background::key_of;
use crate::live::LiveHub;
use crate::live::model::LiveSnapshot;

/// Pictures kept, most recent first: the sheet, and the sheet on the blackboard.
const PICTURES_KEPT: usize = 2;

/// Where a finished picture goes (the share, once it runs).
pub type Deliver = Box<dyn Fn(Option<LivePaper>) + Send + Sync>;

/// The background drawn: the page's own, or slate paper while the blackboard is on
/// (`effectiveBackground` in `live-render.ts`).
fn effective_background(background: Background, board: bool) -> Background {
    if board {
        Background {
            paper: PaperColour::Slate,
            ..background
        }
    } else {
        background
    }
}

/// What the live page of `snapshot` wants drawn under its strokes, without the title, tags or
/// anything else of the note: an opaque name, `None` on plain paper. It changes with the note, the
/// page size, the paper, the template, the texture and the image (its file's name goes only into
/// the digest). A fork gives the note a new id, hence a new key: the picture goes once more.
pub fn paper_key(snapshot: &LiveSnapshot) -> Option<String> {
    let note = &snapshot.page;
    let background = effective_background(note.background, snapshot.view.board);
    if is_plain_paper(background, note.background_image.is_some()) {
        return None;
    }
    key_for(
        &note.id,
        note.page,
        background,
        note.background_image.as_ref(),
    )
}

fn key_for(
    id: &str,
    page: PageSize,
    background: Background,
    image: Option<&BackgroundImage>,
) -> Option<String> {
    let bytes = serde_json::to_vec(&(id, page, background, image)).ok()?;
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(format!("{:016x}", hasher.finish()))
}

/// The paper a live page wants, as far as the snapshot tells.
#[derive(Debug, Clone, PartialEq)]
struct Sheet {
    key: String,
    page_index: usize,
    page: PageSize,
    background: Background,
}

impl Sheet {
    /// `None` on plain paper, without allocating anything: this runs on the hub's flusher.
    fn of(snapshot: &LiveSnapshot) -> Option<Sheet> {
        Some(Sheet {
            key: paper_key(snapshot)?,
            page_index: snapshot.page_index,
            page: snapshot.page.page,
            background: effective_background(snapshot.page.background, snapshot.view.board),
        })
    }
}

#[derive(Default)]
struct Work {
    /// The paper of the page shown, `None` on plain paper.
    wanted: Option<Sheet>,
    /// `wanted` is to be looked at.
    dirty: bool,
    /// The thread is at work.
    working: bool,
}

/// The paper the viewers need, watched from the batches and made apart from them.
pub struct LivePapers {
    hub: Arc<LiveHub>,
    /// Never held while rendering.
    work: Mutex<Work>,
    /// PNG files. Only the thread takes it. Shared with the share, which holds the one in use.
    pictures: Mutex<Vec<(Paper, Arc<[u8]>)>>,
    deliver: OnceLock<Deliver>,
    #[cfg(test)]
    renders: std::sync::atomic::AtomicUsize,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl LivePapers {
    pub fn new(hub: Arc<LiveHub>) -> Arc<LivePapers> {
        Arc::new(LivePapers {
            hub,
            work: Mutex::new(Work::default()),
            pictures: Mutex::new(Vec::new()),
            deliver: OnceLock::new(),
            #[cfg(test)]
            renders: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// Papers go to `deliver` from now on, starting with the one already asked for. Set once: the
    /// share exists only after the feed it is given.
    pub fn attach(self: &Arc<Self>, deliver: Deliver) {
        let _ = self.deliver.set(deliver);
        let mut work = lock(&self.work);
        if work.dirty {
            self.wake(&mut work);
        }
    }

    /// The hub's flusher sends a batch holding this snapshot.
    pub fn saw(self: &Arc<Self>, snapshot: &LiveSnapshot) {
        let sheet = Sheet::of(snapshot);
        let mut work = lock(&self.work);
        if work.wanted != sheet {
            work.wanted = sheet;
            self.wake(&mut work);
        }
    }

    /// A viewer's subscription starts with this snapshot. The picture is looked at again, even for
    /// the same key: the viewers come back from the editor, where the file may have got other pixels.
    pub fn subscribed(self: &Arc<Self>, snapshot: &LiveSnapshot) {
        let sheet = Sheet::of(snapshot);
        let mut work = lock(&self.work);
        if work.wanted.is_some() || sheet.is_some() {
            work.wanted = sheet;
            self.wake(&mut work);
        }
    }

    /// Asks for a pass, on a new thread unless one is at work (or nothing can receive it yet:
    /// [`LivePapers::attach`] starts it).
    fn wake(self: &Arc<Self>, work: &mut Work) {
        work.dirty = true;
        if work.working || self.deliver.get().is_none() {
            return;
        }
        let papers = Arc::clone(self);
        let thread = std::thread::Builder::new()
            .name("share-paper".to_owned())
            .spawn(move || papers.run());
        match thread {
            Ok(_) => work.working = true,
            Err(error) => eprintln!("share: cannot start the paper's thread: {error}"),
        }
    }

    /// Makes and hands over the paper wanted, until nothing more is asked. A paper overtaken
    /// while it was rendered is not sent: its picture stays among those kept.
    fn run(&self) {
        loop {
            let wanted = {
                let mut work = lock(&self.work);
                if !work.dirty {
                    work.working = false;
                    return;
                }
                work.dirty = false;
                work.wanted.clone()
            };
            let picture = wanted.as_ref().and_then(|sheet| self.picture(sheet));
            if lock(&self.work).dirty {
                continue;
            }
            if let Some(deliver) = self.deliver.get() {
                deliver(picture);
            }
        }
    }

    /// The picture for `sheet`, rendered unless it is among those kept; `None` when it turns out
    /// plain (the image's pixels are gone) or cannot be made: the viewer shows the paper colour.
    fn picture(&self, sheet: &Sheet) -> Option<LivePaper> {
        let source = self.hub.page_image(sheet.page_index);
        let paper = Paper {
            page: sheet.page,
            background: sheet.background,
            // The key of the pixels themselves: another file under the same name is another paper.
            image: source.as_ref().and_then(|(image, base)| {
                let member = base.file.background.as_ref()?;
                Some((image.clone(), key_of(&member.bytes)))
            }),
        };
        if paper.is_plain() {
            return None;
        }
        let mut pictures = lock(&self.pictures);
        let entry = match pictures.iter().position(|(known, _)| *known == paper) {
            Some(at) => pictures.remove(at),
            None => {
                #[cfg(test)]
                self.renders
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let pixels = source.as_ref().and_then(|(_, base)| base.picture());
                let png = render_paper_png(&paper, pixels)?;
                (paper, Arc::from(png))
            }
        };
        let png = Arc::clone(&entry.1);
        pictures.insert(0, entry);
        pictures.truncate(PICTURES_KEPT);
        Some(LivePaper {
            key: sheet.key.clone(),
            png,
        })
    }

    /// Pictures rendered so far.
    #[cfg(test)]
    pub fn renders(&self) -> usize {
        self.renders.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Nothing is being made, nor asked.
    #[cfg(test)]
    pub fn idle(&self) -> bool {
        let work = lock(&self.work);
        !work.working && !work.dirty
    }
}
