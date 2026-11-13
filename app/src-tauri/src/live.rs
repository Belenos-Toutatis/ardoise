//! Live mode and the classroom (SPEC §4.2): the live session (pages that become notes, layers,
//! style, blackboard, laser pointer, view of the projected area) shared by every window, fed by
//! the tablet service and sent to the windows in batches through Tauri channels.
//!
//! Contract: docs/design/tablet-live-classroom.md (work package B), "Live session".
//!
//! - [`model`]: the shapes the interface receives and sends (`app/ui/src/lib/live.ts`);
//! - [`session`]: the session itself, pure (strokes, hover, pages, layers, view);
//! - [`edits`]: touching up a page while the tablet writes (what is checked, undo and redo);
//! - [`pen`]: the computer's pen in the control window (M14: what it sends, its times);
//! - [`layers`]: the layers zone of the live window (M14: what it asks, a move or a merge);
//! - [`paper`]: the paper and the image of the current page (M17: the Page and Fond tabs);
//! - [`hub`]: the session shared between threads, the batches sent to the windows, the pages
//!   written to the library;
//! - [`resume`]: a session that starts from an existing note (the sheet's file, the pen on a
//!   sheet that was not made for it, its time);
//! - [`activity`]: whether live mode runs or the presentation window is open, without a lock
//!   (background work waits meanwhile).

pub mod activity;
pub mod edits;
pub mod hub;
pub mod layers;
pub mod model;
pub mod paper;
pub mod pen;
pub mod resume;
pub mod session;

#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
mod board_tests;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ardoise_core::library::Library;
use ardoise_core::note::{Background, BackgroundImage, PageSize};
use ardoise_core::settings::{BoardPosition, LiveButton, Orientation};
use ardoise_device::pages::board_page;
use ardoise_device::service::{TabletService, TabletStatus};
use ardoise_device::transport::TabletKind;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, Webview};

use crate::AppContext;
use crate::commands::{CommandError, CommandResult, store};
use crate::diagnostic::report_language;
use crate::library::{library_error, library_of, path_text};
use crate::tablet::TabletRuntime;

pub use activity::Activity;
pub use edits::StrokesEdit;
pub use hub::{
    ButtonSetting, HubConfig, LiveError, LiveHub, LiveOverview, PagePosition, Start, StartOptions,
};
pub use layers::LayerOp;
pub use model::{
    ForkedSheet, LiveBatch, LiveCommand, LiveMark, LiveSnapshot, LiveStyle, LiveViewState,
    SnapshotReply,
};
pub use paper::LivePageImageOp;
pub use pen::ComputerPenInput;

use self::paper::PageImage;
use crate::narration::{self, NarrationRuntime};
use crate::notes::background_png;
use crate::thumbnails::png_data_url;

/// The live session of the running app.
pub struct LiveRuntime {
    /// Shared with the share on the network (`share.rs`), whose viewers subscribe to it.
    hub: Arc<LiveHub>,
}

impl Default for LiveRuntime {
    fn default() -> Self {
        Self {
            hub: Arc::new(LiveHub::new(HubConfig::default())),
        }
    }
}

impl LiveRuntime {
    pub fn hub(&self) -> &LiveHub {
        &self.hub
    }

    /// The hub, for a holder that outlives a command (the share's live feed).
    pub fn shared_hub(&self) -> Arc<LiveHub> {
        self.hub.clone()
    }
}

impl LiveError {
    /// Error for the interface.
    pub(crate) fn command_error(self) -> CommandError {
        match self {
            LiveError::NotRunning | LiveError::Stopped => CommandError::new("liveNotRunning", self),
            LiveError::Library(error) => library_error(error),
            LiveError::Narration(error) => narration::narration_error(error),
            LiveError::NarrationKeptApart { why, kept } => {
                CommandError::new(why.code(), path_text(&kept))
            }
            LiveError::NarrationLost { detail, .. } => {
                CommandError::new(NARRATION_NOT_KEPT, detail)
            }
            LiveError::Edit(error) => CommandError::new(error.code(), error),
            LiveError::Paper(error) => CommandError::new(error.code(), error),
        }
    }
}

/// A new sound could not join its page's narration, nor be kept anywhere.
pub const NARRATION_NOT_KEPT: &str = "narrationNotKept";

/// A narration recorded in live mode goes to its pages before they are written for the last
/// time (live mode stops, the app closes, a new session starts). Leaving live mode must not fail
/// because of the microphone: a failure does not stop what follows, and comes back for whoever has
/// someone to tell. Also logged where the copies of the pages go, a windowed app having no console.
fn finish_live_narration(narration: &NarrationRuntime, hub: &LiveHub) -> Option<CommandError> {
    let error = narration::finish_live(narration, hub)?.err()?;
    log_narration_failure(&error, &backup_dir());
    Some(error)
}

fn log_narration_failure(error: &CommandError, backup: &Path) {
    let line = format!("the narration could not go with its pages: {error:?}");
    eprintln!("live mode: {line}");
    log_backup(backup, &line);
}

/// The drawing area of the tablet, from what the service knows (last connection, then the
/// registration).
fn tablet_size(status: &TabletStatus) -> Option<PageSize> {
    let (width, height) = match (&status.device, &status.registered) {
        (Some(device), _) => (device.width, device.height),
        (None, Some(registered)) => (registered.width, registered.height),
        (None, None) => return None,
    };
    (width > 0 && height > 0).then_some(PageSize { width, height })
}

/// Reads `[tablet] button` when the button is pressed (it may change during live mode); the
/// default (new page) when the settings cannot be read.
fn button_setting(context: &AppContext) -> ButtonSetting {
    let store = store(context).ok().cloned();
    Arc::new(move || {
        store
            .as_ref()
            .and_then(|store| store.load().ok())
            .map_or(LiveButton::default(), |settings| settings.tablet.button)
    })
}

/// How the tablet is held, from `[tablet] orientation` (landscape when the settings cannot be
/// read).
fn orientation_setting(context: &AppContext) -> Orientation {
    store(context)
        .ok()
        .and_then(|store| store.load().ok())
        .map_or(Orientation::default(), |settings| {
            settings.tablet.orientation
        })
}

/// The main window went away (the app ends): live mode stops and every page is written, so that
/// closing the app without "Stop" loses nothing; a page the library cannot take is kept in
/// [`backup_dir`].
pub fn main_window_closed(app: &AppHandle) {
    if let (Some(narration), Some(runtime)) = (
        app.try_state::<NarrationRuntime>(),
        app.try_state::<LiveRuntime>(),
    ) {
        finish_live_narration(&narration, &runtime.hub);
    }
    if let Some(tablet) = app.try_state::<TabletRuntime>() {
        let _ = tablet.with_service(TabletService::stop_live);
    }
    if let Some(runtime) = app.try_state::<LiveRuntime>() {
        close_session(&runtime.hub, &backup_dir());
        if let Some(context) = app.try_state::<AppContext>() {
            crate::app_bridge::remember_live_session(&context, &runtime.hub);
        }
    }
}

/// Windows may end the session (`session_end.rs`): what is on the page goes to disk now, in case
/// the end comes without more warning. The session goes on: the user may yet refuse.
pub fn session_may_end(app: &AppHandle) {
    if let Some(runtime) = app.try_state::<LiveRuntime>()
        && let Err(error) = runtime.hub.write_now()
    {
        eprintln!("live mode: cannot write the pages before the end of the session: {error}");
    }
}

/// Windows is ending the session (`WM_ENDSESSION`, `session_end.rs`): what [`main_window_closed`]
/// does, in the order that loses least if the process ends halfway.
pub fn session_ending(app: &AppHandle) {
    let Some(runtime) = app.try_state::<LiveRuntime>() else {
        return;
    };
    let hub = &runtime.hub;
    let remember = || {
        if let Some(context) = app.try_state::<AppContext>() {
            crate::app_bridge::remember_live_session(&context, hub);
        }
    };
    let narration = app.try_state::<NarrationRuntime>();
    close_at_session_end(hub, narration.as_deref(), &backup_dir(), remember);
    if let Some(tablet) = app.try_state::<TabletRuntime>() {
        let _ = tablet.with_service(TabletService::stop_live);
    }
}

/// The ink first, which takes least and loses most: it ends up on disk within moments. Then the
/// recording under way: a plain copy of its sound, then its pages, which encoding can make far
/// longer than Windows waits ([`narration::finish_live_ending`]). The copy goes once the pages
/// holding the sound are written. `remember` tells the assistant's bridge, after each step.
pub(crate) fn close_at_session_end(
    hub: &LiveHub,
    narration: Option<&NarrationRuntime>,
    backup: &Path,
    remember: impl Fn(),
) {
    close_session(hub, backup);
    remember();
    let Some(ended) = narration.and_then(|narration| narration::finish_live_ending(narration, hub))
    else {
        return;
    };
    if let Some(copy) = &ended.copy {
        log_backup(
            backup,
            &format!("copy of the recording under way: {}", copy.display()),
        );
    }
    if let Err(error) = &ended.saved {
        log_narration_failure(error, backup);
    }
    let failed = close_session(hub, backup);
    remember();
    if ended.saved.is_ok()
        && failed.is_empty()
        && let Some(copy) = &ended.copy
    {
        let _ = std::fs::remove_file(copy);
    }
}

/// Where the pages that could not be written to the library go when the app ends:
/// `%LOCALAPPDATA%\Ardoise\live-backup` (local to this PC, outside the library); under the
/// temporary folder when `LOCALAPPDATA` is not set (Linux).
pub fn backup_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .filter(|dir| !dir.is_empty())
        .map_or_else(std::env::temp_dir, PathBuf::from)
        .join("Ardoise")
        .join("live-backup")
}

/// Stops the session and writes its pages to the library; every page that cannot be written
/// there gets a complete copy in `backup`, whose path is logged (standard error, and
/// `live-backup.log` next to the copies, since a windowed app has no console). Returns the
/// copies.
fn close_session(hub: &LiveHub, backup: &Path) -> Vec<PathBuf> {
    if let Err(error) = hub.stop() {
        eprintln!("live mode: cannot write the pages to the library when closing: {error}");
    }
    let mut copies = Vec::new();
    for (index, copy) in hub.back_up_unsaved(backup) {
        let line = match &copy {
            Ok(path) => format!(
                "page {} could not be written to the library; backup copy: {}",
                index + 1,
                path.display()
            ),
            Err(error) => format!(
                "page {} could not be written to the library, nor its backup copy: {error}",
                index + 1
            ),
        };
        eprintln!("live mode: {line}");
        log_backup(backup, &line);
        if let Ok(path) = copy {
            copies.push(path);
        }
    }
    copies
}

/// Appends `line` (with the time) to `live-backup.log` in `dir`. Best effort.
fn log_backup(dir: &Path, line: &str) {
    use std::io::Write as _;
    let written = std::fs::create_dir_all(dir)
        .and_then(|()| {
            std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("live-backup.log"))
        })
        .and_then(|mut log| {
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            writeln!(log, "{now} {line}")
        });
    if let Err(error) = written {
        eprintln!("live mode: cannot write live-backup.log: {error}");
    }
}

/// Sends the session to this window from now on (one subscription per window: a new one
/// replaces the previous). The first batch starts with the snapshot returned here.
#[tauri::command]
pub async fn live_subscribe(
    webview: Webview,
    runtime: State<'_, LiveRuntime>,
    on_event: Channel<LiveBatch>,
) -> CommandResult<SnapshotReply> {
    Ok(SnapshotReply(
        runtime.hub.subscribe(webview.label(), Arc::new(on_event)),
    ))
}

/// What a new session needs, from what the app knows now.
fn start_options(context: &AppContext, tablet: &TabletRuntime, library: Library) -> StartOptions {
    let size = tablet
        .with_service(|service| tablet_size(&service.status()))
        .ok()
        .flatten()
        .unwrap_or(ardoise_core::note::A4_LANDSCAPE);
    StartOptions {
        library,
        french: report_language(context) == "fr",
        size,
        orientation: orientation_setting(context),
        button: button_setting(context),
    }
}

/// Hands a new session to the tablet service (a session already running just gives its snapshot).
fn start_streaming(
    start: Start,
    tablet: &TabletRuntime,
    hub: &LiveHub,
) -> CommandResult<LiveSnapshot> {
    match start {
        Start::Running(snapshot) => Ok(snapshot),
        Start::New { snapshot, sink } => {
            let id = sink.session();
            // The hub's lock is not held here: the service may call the sink at once.
            match tablet.with_service(move |service| service.start_live(Box::new(sink))) {
                Ok(()) => Ok(snapshot),
                Err(error) => {
                    hub.abort_start(id);
                    Err(error)
                }
            }
        }
    }
}

/// Starts live mode (or returns the session running): a new session waits for the tablet,
/// which streams as soon as it is connected.
#[tauri::command]
pub async fn live_start(
    context: State<'_, AppContext>,
    tablet: State<'_, TabletRuntime>,
    runtime: State<'_, LiveRuntime>,
    narration: State<'_, NarrationRuntime>,
) -> CommandResult<LiveSnapshot> {
    let library = library_of(&context)?;
    // A narration still recording for a session that stopped by itself (the tablet's service
    // left live mode) belongs to that session's pages, not to the new one.
    if !runtime.hub.page_position().is_some_and(|p| p.running) {
        finish_live_narration(&narration, &runtime.hub);
    }
    pages_first_now(&tablet)?;
    let options = start_options(&context, &tablet, library);
    let start = runtime
        .hub
        .start(options)
        .map_err(LiveError::command_error)?;
    start_streaming(start, &tablet, &runtime.hub)
}

/// The blackboard connected holds stored pages and may not sync them first (the automatic sync
/// or "delete after import" is off): live mode is refused at once (`blackboardPagesFirst`,
/// SPEC §4.2), before any session starts. The interface imports them
/// (`tablet_sync({ confirmBlackboardDelete: true })`), then asks again
/// (docs/design/blackboard.md §9.5). Without a running service, nothing to refuse.
pub(crate) fn pages_first_now(tablet: &TabletRuntime) -> CommandResult<()> {
    match tablet.with_service(TabletService::live_pages_first) {
        Ok(Err(error)) => Err(CommandError::new(
            ardoise_device::service::error_code(&error),
            error,
        )),
        Ok(Ok(())) | Err(_) => Ok(()),
    }
}

/// A tablet connected now (`active`) that cannot write on a sheet of size `sheet`
/// ([`resume::sheet_refusal`]: the other tablet's sheet, `sheetOtherTablet`; a page of the
/// blackboard the other way round than `board_position`, the blackboard's position set now,
/// `sheetBoardPosition`): the resume is refused at once, nothing changed. One that connects later
/// ends the resumed run the same way (the `refused` event), when the position set then is not of
/// the sheet's way round (set so before the resume; once the session holds the sheet, a change to
/// the other way round is refused, `boardPositionFixed`, Q19).
fn refused_now(
    active: Option<TabletKind>,
    board_position: BoardPosition,
    sheet: PageSize,
) -> CommandResult<()> {
    let Some(kind) = active else {
        return Ok(());
    };
    // The page the tablet's points arrive on: the Slate's is not looked at.
    let held = board_page(board_position);
    match resume::sheet_refusal(kind, sheet, held) {
        Some(refusal) => Err(CommandError::new(
            refusal.code(),
            format!(
                "the {} cannot write on this sheet ({} × {})",
                kind.name(),
                sheet.width,
                sheet.height
            ),
        )),
        None => Ok(()),
    }
}

/// Resumes live mode on the note at `path` (SPEC §4.2, `docs/questions.md` Q9): its first page is
/// that note, whose file receives what is written. A session still running goes first, as
/// `live_stop` would end it (its narration, its pages, what the assistant remembers of it); a
/// note that cannot be read leaves it running.
#[tauri::command]
pub async fn live_resume(app: AppHandle, path: String) -> CommandResult<LiveSnapshot> {
    // Reading the note and writing the old session's pages take disk time: off the async
    // runtime's threads.
    crate::notes::blocking(move || {
        let context = app.state::<AppContext>();
        let tablet = app.state::<TabletRuntime>();
        let runtime = app.state::<LiveRuntime>();
        let narration = app.state::<NarrationRuntime>();
        let library = library_of(&context)?;
        let path = Path::new(&path);
        let sheet = library.read(path).map_err(library_error)?.file.note.page;
        let (active, board_position) = tablet
            .with_service(|service| {
                let status = service.status();
                (status.active, status.options.board_position)
            })
            .unwrap_or_default();
        refused_now(active, board_position, sheet)?;
        pages_first_now(&tablet)?;
        finish_live_narration(&narration, &runtime.hub);
        if runtime.hub.page_position().is_some_and(|p| p.running) {
            // Without a running service there is nothing to stop on the tablet's side.
            let _ = tablet.with_service(TabletService::stop_live);
            runtime.hub.stop().map_err(LiveError::command_error)?;
            crate::app_bridge::remember_live_session(&context, &runtime.hub);
        }
        let options = start_options(&context, &tablet, library);
        let start = runtime
            .hub
            .resume(options, path)
            .map_err(LiveError::command_error)?;
        start_streaming(start, &tablet, &runtime.hub)
    })
    .await
}

/// What `live_stop` answers (`LiveStopped` in `live.ts`): the interface closes the view as soon as it
/// has it, so what the last write and the end of the recording did is said here and not only in the
/// events.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStopped {
    /// Paths of the session's notes.
    pub saved: Vec<String>,
    /// The resumed sheets that went on in a new note, in this session (the last write included).
    pub forks: Vec<ForkedSheet>,
    /// The recording still running when live mode stopped could not join its pages: why, in the
    /// usual `{ code, message }`. `narrationTooLong` and `narrationUnreadable` say where its sound
    /// is kept.
    pub narration: Option<CommandError>,
}

/// Stops the session and says what became of it.
fn stopped(hub: &LiveHub, narration: Option<CommandError>) -> CommandResult<LiveStopped> {
    let saved = hub.stop().map_err(LiveError::command_error)?;
    Ok(LiveStopped {
        saved,
        forks: hub.forked_sheets(),
        narration,
    })
}

/// Leaves live mode and writes every page. A narration recording in live mode is stopped first and
/// goes with its pages.
#[tauri::command]
pub async fn live_stop(app: AppHandle) -> CommandResult<LiveStopped> {
    // Writing the session's pages (and closing the narration) takes disk time: off the async
    // runtime's threads.
    crate::notes::blocking(move || {
        let context = app.state::<AppContext>();
        let tablet = app.state::<TabletRuntime>();
        let runtime = app.state::<LiveRuntime>();
        let narration = app.state::<NarrationRuntime>();
        let recording = finish_live_narration(&narration, &runtime.hub);
        // Without a running service there is nothing to stop on the tablet's side.
        let _ = tablet.with_service(TabletService::stop_live);
        let answer = stopped(&runtime.hub, recording)?;
        // Images staged in the Fond tab and never set (an import left, a refused `set`) are let go:
        // the pages hold their own pixels, and the editor is never open with live mode.
        crate::background::staging().clear();
        // The assistant's `last_live_note` finds the session after a restart too.
        crate::app_bridge::remember_live_session(&context, &runtime.hub);
        Ok(answer)
    })
    .await
}

#[tauri::command]
pub async fn live_set_style(
    runtime: State<'_, LiveRuntime>,
    style: LiveStyle,
) -> CommandResult<()> {
    runtime
        .hub
        .set_style(style)
        .map_err(LiveError::command_error)
}

#[tauri::command]
pub async fn live_command(
    runtime: State<'_, LiveRuntime>,
    command: LiveCommand,
) -> CommandResult<()> {
    runtime
        .hub
        .command(command.kind)
        .map_err(LiveError::command_error)
}

/// The paper of page `page_index` of the session as a PNG data URL, like `render_background`, with
/// the page's background image: a resumed sheet's or one the Fond tab set (M17). Its pixels are the
/// page's (`paper::PageImage`), not in the editor's staging area, which the editor empties when it
/// closes. The page's strokes and the tablet's input never wait for it: the hub's lock is held only
/// to find the pixels, and the windows load the picture apart from the strokes (they ask again when
/// the snapshot's `imagePixels.key` changes).
#[tauri::command]
pub async fn live_page_background(
    runtime: State<'_, LiveRuntime>,
    page_index: usize,
    page: PageSize,
    background: Background,
    width_px: u32,
) -> CommandResult<String> {
    let source = runtime.hub.page_image(page_index);
    tauri::async_runtime::spawn_blocking(move || {
        page_background_url(source, page, background, width_px)
    })
    .await
    .map_err(|error| CommandError::new("render", error))?
}

/// The paper as a PNG data URL, with the image of `source` ([`LiveHub::page_image`]). Slow: decoding
/// the image, when it is not among the pictures kept decoded.
pub(crate) fn page_background_url(
    source: Option<(BackgroundImage, Arc<PageImage>)>,
    page: PageSize,
    background: Background,
    width_px: u32,
) -> CommandResult<String> {
    let image = source.and_then(|(image, pixels)| Some((image, pixels.picture()?)));
    let image = image
        .as_ref()
        .map(|(image, picture)| (image.clone(), picture.as_ref()));
    background_png(page, background, width_px, image).map(|png| png_data_url(&png))
}

/// « Undo the clearing » after « Clear the page »: true when the ink came back.
#[tauri::command]
pub async fn live_restore_cleared(runtime: State<'_, LiveRuntime>) -> CommandResult<bool> {
    runtime
        .hub
        .restore_cleared()
        .map_err(LiveError::command_error)
}

/// Deletes, recolours, moves or resizes strokes the window chose on page `page_index` at
/// `revision` (the snapshot's and the events'): refused as `liveEditStale` once that page is left,
/// its strokes moved or its ink turned, `liveEditInvalid` when the list, the colour or the move
/// makes no sense.
/// Returns the page's revision after the edit; every window hears of it through a `strokesEdited`
/// event.
#[tauri::command]
pub async fn live_edit_strokes(
    runtime: State<'_, LiveRuntime>,
    page_index: usize,
    revision: u64,
    edit: StrokesEdit,
) -> CommandResult<u64> {
    runtime
        .hub
        .edit_strokes(page_index, revision, edit)
        .map_err(LiveError::command_error)
}

/// Undoes the last edit of page `page_index` (delete, change of colour, move or resize, « Clear the
/// page », a stroke of the computer, a gesture of its eraser as a whole; never what the tablet
/// wrote): true when there was one. Refused as `liveEditStale` when another page is on screen.
#[tauri::command]
pub async fn live_undo_edit(
    runtime: State<'_, LiveRuntime>,
    page_index: usize,
) -> CommandResult<bool> {
    runtime
        .hub
        .undo_edit(page_index)
        .map_err(LiveError::command_error)
}

/// Does again the last edit undone on page `page_index`: true when there was one. Refused as
/// `liveEditStale` when another page is on screen.
#[tauri::command]
pub async fn live_redo_edit(
    runtime: State<'_, LiveRuntime>,
    page_index: usize,
) -> CommandResult<bool> {
    runtime
        .hub
        .redo_edit(page_index)
        .map_err(LiveError::command_error)
}

/// A call of the computer's pen or mouse in the control window (M14; `ComputerPenInput`, always
/// sent one at a time through the window's queue): `begin` answers `true` or is refused
/// (`liveNotRunning`, `liveEditStale` for another page, `liveLayerHidden` when the computer's layer
/// is hidden, `liveEditInvalid` for samples that make no sense); `points` and `end` answer whether
/// the stroke goes on (`false`: the window stops sending for that gesture); `cancel` whether there
/// was a stroke to drop.
#[tauri::command]
pub async fn live_computer_pen(
    runtime: State<'_, LiveRuntime>,
    input: ComputerPenInput,
) -> CommandResult<bool> {
    runtime
        .hub
        .computer_pen(input)
        .map_err(LiveError::command_error)
}

/// A layer operation of the live window on page `page_index` (M14; `LayerOp`): refused as a whole
/// as `liveEditStale` once that page is left, `liveLayerInvalid` when it makes no sense there,
/// `liveNotRunning` once the session is stopped (except choosing where a pen acts). Every window
/// hears of it through a snapshot. Not undone by Ctrl+Z.
#[tauri::command]
pub async fn live_layers(
    runtime: State<'_, LiveRuntime>,
    page_index: usize,
    op: LayerOp,
) -> CommandResult<()> {
    runtime
        .hub
        .layers(page_index, op)
        .map_err(LiveError::command_error)
}

/// Changes the paper (colour, template, texture) of page `page_index` of the session, the one on
/// screen (M17; docs/design/paper-projection-narration.md §1.3): with the blackboard on, the template
/// and the texture at once and the colour when it is turned off. Refused as `liveEditStale` once
/// that page is left, `liveNotRunning` before the first start and once the session is stopped.
/// Every window hears of it through a snapshot (none when nothing changed); a new page takes this
/// paper. Not undone by Ctrl+Z.
#[tauri::command]
pub async fn live_page_paper(
    runtime: State<'_, LiveRuntime>,
    page_index: usize,
    background: Background,
) -> CommandResult<()> {
    runtime
        .hub
        .set_page_paper(page_index, background)
        .map_err(LiveError::command_error)
}

/// Sets, places again or removes the background image of page `page_index`, the one on screen (M17;
/// `LivePageImageOp`): a `set` takes the image staged under its key (`stage_background_image`) out
/// of the staging area, for the page to keep. Refused as `liveEditStale` once that page is left,
/// `liveNotRunning` before the first start and once the session is stopped, `liveEditInvalid` for a
/// description that makes no sense (or a `place` of another image than the page's),
/// `backgroundMissing` for a `set` whose key is not staged; a refused `set` leaves the image staged.
/// Every window hears of it through a snapshot (none when nothing changed). Not undone by Ctrl+Z.
#[tauri::command]
pub async fn live_page_image(
    runtime: State<'_, LiveRuntime>,
    page_index: usize,
    op: LivePageImageOp,
) -> CommandResult<()> {
    runtime
        .hub
        .change_page_image(page_index, op, crate::background::staging())
        .map_err(LiveError::command_error)
}

#[tauri::command]
pub async fn live_set_view(
    runtime: State<'_, LiveRuntime>,
    view: LiveViewState,
) -> CommandResult<()> {
    runtime.hub.set_view(view).map_err(LiveError::command_error)
}

/// The marks written over the presentation on page `page_index`, the whole list: the share's
/// viewers draw them. Temporary, kept in memory only; the list is bounded (`MAX_MARKS`,
/// `MAX_MARKS_POINTS`).
#[tauri::command]
pub async fn live_set_marks(
    runtime: State<'_, LiveRuntime>,
    page_index: usize,
    marks: Vec<LiveMark>,
) -> CommandResult<()> {
    runtime
        .hub
        .set_marks(page_index, marks)
        .map_err(LiveError::command_error)
}
