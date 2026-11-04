/** Typed wrappers around the Rust commands of `app/src-tauri/src/commands.rs`. */
import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AboutInfo, AboutText } from './about';
import type { BackgroundSource, NotesProgress, StagedBackground } from './background';
import type { Decision, DiagnosticEvent, DiagnosticOptions } from './diagnostic';
import type { LanguagePreference } from './i18n-core';
import { t } from './i18n.svelte';
import type { InkspaceProgress, InkspaceReport, InkspaceSource } from './inkspace';
import type { LiveBatch, LiveCommand, LiveMark, LiveSnapshot, LiveStrokesEdit, LiveStyle, LiveViewState, MonitorInfo } from './live';
import { mockDiagnosticStart, mockInvoke, mockOnLibraryChanged } from './mock';
import { mockLiveSubscribe } from './mock-live';
import { mockShareSubscribe } from './mock-share';
import { mockTabletSubscribe } from './mock-tablet';
import type { Orientation } from './orientation';
import type { SharePortSetting, ShareContent, ShareStatus } from './share';
import type { BridgeAnswer, BridgeEvent } from './app-bridge.svelte';
import type { TabletEvent, TabletOptions, TabletStatus } from './tablet';
import type {
  DownloadProgress,
  Microphone,
  NarrationInfo,
  NarrationSaved,
  NarrationStatus,
  NarrationTarget,
  OpenH264State,
  VideoEstimate,
  VideoExportOptions,
  VideoExportResult,
  VideoProgress,
} from './video';
import { mockOpenH264Download, mockVideoExport } from './mock-video';
import type { NoteOriginal } from './original';
import type { Draft, DraftSnapshot, DraftSummary } from './editor/drafts';
import type {
  HandwritingHit,
  HandwritingInfo,
  HandwritingSetting,
  IndexStatus,
  NoteText,
  StructuredExport,
  StructuredFormat,
  TextLabels,
} from './handwriting';
import { mockOnHandwritingStatus } from './mock-handwriting';
import type {
  AutoExportSettings,
  Background,
  BackgroundImage,
  ExportFormat,
  ExportOptions,
  ExportSource,
  FileStamp,
  LibraryListing,
  Note,
  NoteSummary,
  PrintOptions,
  PrintResult,
  OpenedNote,
  MergeLayout,
  PageSize,
  SplitResult,
  SplitSpec,
  TabletInfo,
  TrashEntry,
} from './note';

export type ThemePreference = 'system' | 'light' | 'dark';

export interface AppState {
  version: string;
  machine: string;
  settingsPath: string;
  settingsError: string | null;
  language: LanguagePreference;
  theme: ThemePreference;
  library: string | null;
  systemLocale: string | null;
  selfTest: boolean;
  /** The executable was started with `--diagnostic`: open the tablet test right away. */
  openDiagnostic: boolean;
  /** Automatic export after a sync (shared by every PC). */
  autoExport: AutoExportSettings;
  /** Microphone chosen on this PC for the narration (null: the system's default). */
  microphone: string | null;
}

export interface LibraryInspection {
  /** The folder already holds an Ardoise library (`_ardoise/` present). */
  existing: boolean;
  /** Note files found in it, sub-folders included. */
  notes: number;
  /** False when the count stopped early (a very large folder): there may be more notes. */
  complete: boolean;
}

/** A note an operation on several notes could not handle, with the reason (`organize.rs`). */
export interface FailedNote {
  path: string;
  error: { code: string; message: string };
}

/** What moving notes to the trash did (`TrashedNotes` in `organize.rs`). */
export interface TrashedNotes {
  /** Each note moved, with the id of its trash entry (to restore it). */
  trashed: { path: string; id: string }[];
  failed: FailedNote[];
}

/** What deleting a binder did (`BinderDeleted` in `organize.rs`). */
export interface BinderDeleted {
  /** Its notes, now in the trash, each restorable to its own binder. */
  trashed: TrashEntry[];
  failed: FailedNote[];
  /** Folders kept because they still hold other files (relative, `/`-separated, deepest first). */
  kept: string[];
}

/** A stroke ready to draw: SVG path data to fill with the non-zero rule (`notes.rs`). */
export interface StrokeOutline {
  /** `#rrggbbaa`. */
  color: string;
  tool: string;
  layer: number;
  /** Page coordinates in device units (10 µm). */
  path: string;
}

/** The visible strokes of a note, drawn with the same geometry as the exports. */
export interface NoteOutlines {
  width: number;
  height: number;
  strokes: StrokeOutline[];
}

export interface UiSelfTestReport {
  rendered: boolean;
  i18nConsistent: boolean;
  locale: string;
  keyCount: number;
}

/** True inside the desktop app; false in a plain browser during development. */
export function inDesktopApp(): boolean {
  return isTauri();
}

/** Decided at each call: the desktop runtime's globals are not guaranteed at module load. */
function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return inDesktopApp() ? invoke<T>(command, args) : mockInvoke<T>(command, args);
}

export const api = {
  appState: () => call<AppState>('app_state'),
  setLanguage: (language: LanguagePreference) => call<AppState>('set_language', { language }),
  setTheme: (theme: ThemePreference) => call<AppState>('set_theme', { theme }),
  inspectLibrary: (path: string) => call<LibraryInspection>('inspect_library', { path }),
  setLibrary: (path: string) => call<AppState>('set_library', { path }),
  selfTestReport: (report: UiSelfTestReport) => call<void>('self_test_report', { report }),
  noteOutlines: (path: string) => call<NoteOutlines>('note_outlines', { path }),
  /**
   * Starts the guided tablet test. Resolves as soon as the run has started; its progress then
   * arrives through `onEvent` until a `done` (or `error`) event.
   */
  diagnosticStart: (options: DiagnosticOptions, onEvent: (event: DiagnosticEvent) => void) => {
    if (!inDesktopApp()) return mockDiagnosticStart(options, onEvent);
    const channel = new Channel<DiagnosticEvent>();
    channel.onmessage = onEvent;
    return invoke<void>('diagnostic_start', { options, onEvent: channel });
  },
  /** Answers the pending blocking prompt of the tablet test. */
  diagnosticAnswer: (decision: Decision) => call<void>('diagnostic_answer', { decision }),
  /** Stops the tablet test (a `done` event with `cancelled` follows). */
  diagnosticCancel: () => call<void>('diagnostic_cancel'),

  // --- Library (milestone M3; Rust side: app/src-tauri/src/library.rs) ---------------------

  /** Every note of this computer's library, its binders and tags. */
  libraryList: () => call<LibraryListing>('library_list'),
  /** PNG data URL of a note's thumbnail (cached on this PC), or null if none can be made. */
  noteThumbnail: (path: string, stamp: FileStamp) => call<string | null>('note_thumbnail', { path, stamp }),
  /** A new blank note (A4 landscape, white) in `binder`; returns its summary. */
  createNote: (binder: string, title: string) => call<NoteSummary>('create_note', { binder, title }),
  createBinder: (binder: string) => call<void>('create_binder', { binder }),
  /** Moves notes into `binder`; returns their new paths, in order. */
  moveNotes: (paths: string[], binder: string) => call<string[]>('move_notes', { paths, binder }),
  /** Moves each note it can to the trash; says which went (with their trash ids) and which failed. */
  trashNotes: (paths: string[]) => call<TrashedNotes>('trash_notes', { paths }),
  trashList: () => call<TrashEntry[]>('trash_list'),
  /** Restores a trashed note; returns its path. */
  trashRestore: (id: string) => call<string>('trash_restore', { id }),
  trashPurge: (id: string) => call<void>('trash_purge', { id }),
  /** Empties the trash; returns how many notes were deleted for good. */
  trashEmpty: () => call<number>('trash_empty'),
  setNoteTags: (path: string, tags: string[], stamp: FileStamp) =>
    call<FileStamp>('set_note_tags', { path, tags, stamp }),
  setNoteFavorite: (path: string, favorite: boolean, stamp: FileStamp) =>
    call<FileStamp>('set_note_favorite', { path, favorite, stamp }),
  /** Changes a note's title, which renames its file. */
  retitleNote: (path: string, title: string, stamp: FileStamp) =>
    call<{ path: string; stamp: FileStamp }>('retitle_note', { path, title, stamp }),

  // --- Library: organising (milestone M10; Rust side: app/src-tauri/src/organize.rs) -------

  /** Renames a binder (`a/b`) in place; returns its new path. Refused (`binderExists`) rather than replacing a folder. */
  renameBinder: (binder: string, name: string) => call<string>('rename_binder', { binder, name }),
  /** Puts every note of a binder (and its sub-binders) in the trash, then removes the folders left empty. */
  deleteBinder: (binder: string) => call<BinderDeleted>('delete_binder', { binder }),
  /** Colour id of every tag that has one, keyed by the tag in lower case (`tagKey`). */
  tagColors: () => call<Record<string, string>>('tag_colors'),
  /** Gives a tag a colour of the palette, or none (`null`); returns every colour afterwards. */
  setTagColor: (tag: string, color: string | null) => call<Record<string, string>>('set_tag_color', { tag, color }),

  // --- Editor -------------------------------------------------------------------------------

  openNote: (path: string) => call<OpenedNote>('open_note', { path }),
  /**
   * Saves an edited note (thumbnail regenerated, tablet original and other members kept);
   * rejects with code `noteConflict` if the file changed since `stamp`. `imageKey`: the staged
   * pixels of the note's background image (`backgroundMissing` if they are gone); without it the
   * file's own image is kept.
   */
  saveNote: (path: string, note: Note, stamp: FileStamp, imageKey?: string) =>
    call<FileStamp>('save_note', { path, note, stamp, imageKey }),
  /**
   * PNG data URL of a page background (paper, template, texture) `widthPx` pixels wide, with the
   * background image `image` over the paper (its pixels staged under `key`).
   */
  renderBackground: (
    page: PageSize,
    background: Background,
    widthPx: number,
    image?: { image: BackgroundImage; key: string },
  ) => call<string>('render_background', { page, background, widthPx, image }),
  /**
   * The ink the note can go back to ("Revenir à l'original"): `{ source: 'none' }` when there is
   * nothing to go back to; rejects with `originalDamaged` when the original cannot be read.
   */
  noteOriginal: (path: string) => call<NoteOriginal>('note_original', { path }),

  // --- Exports, split, merge (milestone M5; Rust side: export.rs, clipboard.rs, operations.rs) ---

  /**
   * Exports notes. PDF: `destination` is one file holding every note, one page each. PNG, JPG,
   * SVG: a file for one note, a folder for several (one file per note, never overwriting).
   * Returns the paths written.
   */
  exportNotes: (sources: ExportSource[], format: ExportFormat, options: ExportOptions, destination: string) =>
    call<string[]>('export_notes', { sources, format, options, destination }),
  /**
   * Cancels the export under way: it then rejects with `cancelled`. Does nothing when no export
   * is running.
   */
  cancelExport: () => call<void>('cancel_export'),
  /** Copies the note's image to the clipboard (Windows); rejects with `clipboardUnavailable` elsewhere. */
  copyNoteImage: (source: ExportSource, dpi: number) => call<void>('copy_note_image', { source, dpi }),
  /** Whether notes can be dragged out of the app to other programs (Windows). */
  noteDragSupported: () => call<boolean>('note_drag_supported'),
  /**
   * Renders the notes as PNG files named after them, in a temporary folder of the app, for a
   * drag out (drag-out.ts); returns the files, one per note, in order.
   */
  prepareNoteDrag: (paths: string[]) => call<string[]>('prepare_note_drag', { paths }),
  /**
   * Starts the system drag over files from `prepareNoteDrag` (the mouse button must be down) and
   * resolves when it ends.
   */
  startNoteDrag: (files: string[]) => call<'dropped' | 'cancelled'>('start_note_drag', { files }),
  /**
   * Splits the editor's current `note`: the new note (titled `title`, same binder) is created
   * first, then the kept part is saved over `path` (`noteConflict` if the file changed since
   * `stamp`; the new note is then moved to the trash).
   */
  splitNote: (path: string, note: Note, stamp: FileStamp, split: SplitSpec, title: string, imageKey?: string) =>
    call<SplitResult>('split_note', { path, note, stamp, split, title, imageKey }),
  /** Assembles notes (oldest first) into a new note in the first one's binder; sources unchanged. */
  mergeNotes: (paths: string[], layout: MergeLayout, title: string) =>
    call<NoteSummary>('merge_notes', { paths, layout, title }),
  /** Sets the automatic export after a sync (`invalidFolder` if the folder is not acceptable). */
  setAutoExport: (autoExport: AutoExportSettings) => call<AppState>('set_auto_export', { autoExport }),

  // --- Settings: tablet ---------------------------------------------------------------------

  /** The tablet registered from this library, if any. */
  tabletInfo: () => call<TabletInfo | null>('tablet_info'),
  /** Forgets the registered tablet (it must then be registered again). */
  forgetTablet: () => call<void>('forget_tablet'),

  // --- Tablet (milestone M4; Rust side: tablet.rs, docs/design/tablet-live-classroom.md) -----

  tabletStatus: () => call<TabletStatus>('tablet_status'),
  /** Current status; then `onEvent` at every change (connection, battery, sync progress…). */
  tabletSubscribe: (onEvent: (event: TabletEvent) => void) => {
    if (!inDesktopApp()) return mockTabletSubscribe(onEvent);
    const channel = new Channel<TabletEvent>();
    channel.onmessage = onEvent;
    return invoke<TabletStatus>('tablet_subscribe', { onEvent: channel });
  },
  /** Starts the registration wizard (its steps arrive in the status). */
  tabletRegister: () => call<void>('tablet_register'),
  /** Cancels the registration or the sync under way. */
  tabletCancel: () => call<void>('tablet_cancel'),
  /** Syncs now, or as soon as the tablet connects. */
  tabletSync: () => call<void>('tablet_sync'),
  /** Renames the tablet (1–15 characters; `invalidName` otherwise). */
  tabletRename: (name: string) => call<TabletStatus>('tablet_rename', { name }),
  /** Sets the tablet clock; returns the drift left in seconds. */
  tabletSetTime: () => call<number>('tablet_set_time'),
  tabletSetOptions: (options: TabletOptions) => call<TabletStatus>('tablet_set_options', { options }),
  /** The live window's quick switch: how the tablet is held, applied at once to the live page. */
  tabletSetOrientation: (orientation: Orientation) => call<void>('tablet_set_orientation', { orientation }),
  /** Presses the simulated tablet's button (`--simulated-tablet` only). */
  simPressButton: () => call<void>('sim_press_button'),

  // --- Live mode and classroom (milestone M4; Rust side: live.rs, presentation.rs) ------------

  /** Current session; then `onBatch` with everything that happens, from every window. */
  liveSubscribe: (onBatch: (batch: LiveBatch) => void) => {
    if (!inDesktopApp()) return mockLiveSubscribe(onBatch);
    const channel = new Channel<LiveBatch>();
    channel.onmessage = onBatch;
    return invoke<LiveSnapshot>('live_subscribe', { onEvent: channel });
  },
  liveStart: () => call<LiveSnapshot>('live_start'),
  /**
   * Resumes live mode on an existing sheet of the library (M12): like `liveStart`, with that note
   * as the first page. Refused with `noteNotFound`, `notInLibrary`, `noteDamaged` or
   * `noteNewerVersion` (a running session then goes on).
   */
  liveResume: (path: string) => call<LiveSnapshot>('live_resume', { path }),
  /** Leaves live mode; returns the paths of the notes saved. */
  liveStop: () => call<string[]>('live_stop'),
  liveSetStyle: (style: LiveStyle) => call<void>('live_set_style', { style }),
  liveCommand: (command: LiveCommand) => call<void>('live_command', { command }),
  /** « Undo the clearing »: gives the current page the ink of the last « Clear the page » back. */
  liveRestoreCleared: () => call<boolean>('live_restore_cleared'),
  /**
   * Deletes or recolours strokes chosen on page `pageIndex` at `revision` (`LiveState.revision`):
   * refused with `liveEditStale` once that page is left or its strokes moved, `liveEditInvalid`
   * when the list or the colour makes no sense. Returns the page's revision after the edit (a change
   * of colour keeps it, so that the selection goes on); every window gets a `strokesEdited` event.
   */
  liveEditStrokes: (pageIndex: number, revision: number, edit: LiveStrokesEdit) =>
    call<number>('live_edit_strokes', { pageIndex, revision, edit }),
  /**
   * Undoes the last edit of page `pageIndex`, the one on screen, or `liveEditStale` (delete, change
   * of colour, « Clear the page »; never what the tablet wrote). False when there was none.
   */
  liveUndoEdit: (pageIndex: number) => call<boolean>('live_undo_edit', { pageIndex }),
  /** Does again the last edit undone on page `pageIndex`, likewise. False when there was none. */
  liveRedoEdit: (pageIndex: number) => call<boolean>('live_redo_edit', { pageIndex }),
  liveSetView: (view: LiveViewState) => call<void>('live_set_view', { view }),
  /**
   * The marks written over the presentation on page `pageIndex`, the whole list: the viewers of
   * the share draw them (temporary, never saved; ignored when the page is another one).
   */
  liveSetMarks: (pageIndex: number, marks: LiveMark[]) => call<void>('live_set_marks', { pageIndex, marks }),
  listMonitors: () => call<MonitorInfo[]>('list_monitors'),
  /** Opens the presentation window full screen on `monitor` (null: the saved or default screen). */
  /** `remember: false` (the assistant's bridge): the screen is not saved as this PC's choice. */
  openPresentation: (monitor: string | null, remember = true) =>
    call<MonitorInfo>('open_presentation', { monitor, remember }),
  closePresentation: () => call<void>('close_presentation'),

  // --- LAN share (M11; Rust side: share.rs, docs/design/lan-share.md) -----------------------

  shareStatus: () => call<ShareStatus>('share_status'),
  /** Current status; then `onStatus` at every change (start, stop, a screen joining or leaving, pause). */
  shareSubscribe: (onStatus: (status: ShareStatus) => void) => {
    if (!inDesktopApp()) return mockShareSubscribe(onStatus);
    const channel = new Channel<ShareStatus>();
    channel.onmessage = onStatus;
    return invoke<ShareStatus>('share_subscribe', { onEvent: channel });
  },
  /**
   * Starts the share, or returns the running one. If the port is held by another program the
   * share takes the first free port after it (`status.port` against `status.requestedPort`) and
   * keeps it for this PC. Fails with `shareNoFreePort`, `sharePortInUse`, `shareUnavailable`,
   * `shareFailed`.
   */
  shareStart: () => call<ShareStatus>('share_start'),
  /** The port of the share on this PC (settings screen). */
  sharePort: () => call<SharePortSetting>('share_port'),
  /** Chooses the port of the share on this PC (1024 to 65535; else `sharePortInvalid`); applies to the next share. */
  setSharePort: (port: number) => call<SharePortSetting>('set_share_port', { port }),
  shareStop: () => call<ShareStatus>('share_stop'),
  /** Freezes the shared image (true) or resumes it (false). */
  shareSetPaused: (paused: boolean) => call<ShareStatus>('share_set_paused', { paused }),
  /** What the main window shows now, hence what the share shows (ignored while no share runs). */
  shareShow: (content: ShareContent) => call<void>('share_show', { content }),

  // --- Replay, video, narration (milestone M6; Rust side: video.rs, narration.rs, codec.rs) ---

  /** Size, length and expected export time of a video export (no file written). */
  estimateVideoExport: (source: ExportSource, options: VideoExportOptions) =>
    call<VideoEstimate>('estimate_video_export', { source, options }),
  /**
   * Exports the replay of a note to `destination` (written only once complete); progress through
   * `onProgress`. Rejects with `cancelled` after `cancelVideoExport`.
   */
  exportVideo: (
    source: ExportSource,
    options: VideoExportOptions,
    destination: string,
    onProgress: (progress: VideoProgress) => void,
  ) => {
    if (!inDesktopApp()) return mockVideoExport(source, options, destination, onProgress);
    const channel = new Channel<VideoProgress>();
    channel.onmessage = onProgress;
    return invoke<VideoExportResult>('export_video', { source, options, destination, onProgress: channel });
  },
  cancelVideoExport: () => call<void>('cancel_video_export'),

  listMicrophones: () => call<Microphone[]>('list_microphones'),
  /** Remembers the microphone on this PC (null: the system's default). */
  setMicrophone: (device: string | null) => call<AppState>('set_microphone', { device }),
  /** Starts a level-only capture (no audio kept); `microphoneMonitorStop` ends it. */
  microphoneMonitor: (device: string | null) => call<void>('microphone_monitor', { device }),
  /** Stops a level-only capture, and only that: a recording is never touched by it. */
  microphoneMonitorStop: () => call<void>('microphone_monitor_stop'),
  /** Starts recording the narration (`narrationBusy` if one is already recording). */
  narrationStart: (device: string | null, target: NarrationTarget) =>
    call<void>('narration_start', { device, target }),
  /** Level and state of the capture under way; poll about 10 times a second. */
  narrationStatus: () => call<NarrationStatus>('narration_status'),
  /**
   * Stops the recording. With `save`, a replay recording is written into the note at `path` (the
   * editor's current path: the note may have been renamed since the start) with `stamp` (the
   * editor's; `noteConflict` if the file changed); `replayStartedAtMs` is `Date.now()` at the
   * moment the replay was at instant 0 and playing, which places the sound in replay time.
   * Live recordings go to their pages (`path` and `replayStartedAtMs` are null). A replay
   * recording that cannot be saved stays pending (`narrationRetry`, `narrationDiscard`).
   * Returns null when nothing is saved.
   */
  narrationStop: (save: boolean, stamp: FileStamp | null, path: string | null, replayStartedAtMs: number | null) =>
    call<NarrationSaved | null>('narration_stop', { save, stamp, path, replayStartedAtMs }),
  /** Saves the pending recording into the note at `path` (`noteConflict` etc. keep it pending). */
  narrationRetry: (path: string, stamp: FileStamp | null) => call<NarrationSaved>('narration_retry', { path, stamp }),
  /** Throws the pending recording away (`noPendingNarration` if there is none). */
  narrationDiscard: () => call<void>('narration_discard'),
  /** The narration stored in a note, or null. */
  narrationInfo: (path: string) => call<NarrationInfo | null>('narration_info', { path }),
  /** The narration as stored, an Ogg Opus stream (a WAV file in the browser stand-in), to play during the replay. */
  narrationAudio: (path: string) => call<ArrayBuffer>('narration_audio', { path }),
  /** Removes a note's narration; returns the new stamp. */
  deleteNarration: (path: string, stamp: FileStamp) => call<FileStamp>('delete_narration', { path, stamp }),

  openH264State: () => call<OpenH264State>('openh264_state'),
  /** Downloads OpenH264 from Cisco (SHA-256 checked) and allows the fallback. */
  openH264Download: (onProgress: (progress: DownloadProgress) => void) => {
    if (!inDesktopApp()) return mockOpenH264Download(onProgress);
    const channel = new Channel<DownloadProgress>();
    channel.onmessage = onProgress;
    return invoke<OpenH264State>('openh264_download', { onProgress: channel });
  },
  openH264CancelDownload: () => call<void>('openh264_cancel_download'),
  /** Deletes the library and disallows the fallback. */
  openH264Remove: () => call<OpenH264State>('openh264_remove'),
  setOpenH264Allowed: (allowed: boolean) => call<OpenH264State>('set_openh264_allowed', { allowed }),

  // --- Background image or PDF (M7; Rust side: background.rs, docs/design/extras.md) ----------

  /** What a file chosen by the user holds: an image, or a PDF with its pages' sizes. */
  inspectBackgroundFile: (path: string) => call<BackgroundSource>('inspect_background_file', { path }),
  /** Imports an image, or page `pdfPage` of a PDF, for the editor; nothing is written yet. */
  stageBackgroundImage: (path: string, pdfPage?: number) =>
    call<StagedBackground>('stage_background_image', { path, pdfPage }),
  /** Stages the background image of the note at `path` (null when it has none). */
  stageNoteBackground: (path: string) => call<StagedBackground | null>('stage_note_background', { path }),
  /** Forgets every staged image (the editor closes or opens another note). */
  releaseBackgroundImages: () => call<void>('release_background_images'),
  /**
   * New notes in `binder`: one per page of a PDF (`pages` from 1, all when absent) or one for an
   * image; `onProgress` hears how many are made, before the first one and after each.
   */
  createNotesFromFile: (
    binder: string,
    path: string,
    pages?: number[],
    onProgress: (progress: NotesProgress) => void = () => {},
  ) => {
    if (!inDesktopApp()) return call<NoteSummary[]>('create_notes_from_file', { binder, path, pages, onProgress });
    const channel = new Channel<NotesProgress>();
    channel.onmessage = onProgress;
    return invoke<NoteSummary[]>('create_notes_from_file', { binder, path, pages, onProgress: channel });
  },
  /** Stops `createNotesFromFile` after the page being rendered: the notes made are removed again and the call fails with the code `cancelled`. */
  cancelCreateNotes: () => call<void>('cancel_create_notes'),
  /** The background image's name of each note (null without one). */
  backgroundImageNames: (paths: string[]) => call<(string | null)[]>('background_image_names', { paths }),

  // --- Inkspace import (M10; Rust side: import.rs) -------------------------------------------

  /** Inkspace's `LocalState` folder found on this PC, or null (always null outside Windows). */
  inkspaceDetect: () => call<string | null>('inkspace_detect'),
  /**
   * Reads a folder of Inkspace data (can take a second) and counts its notes. Rejects with
   * `inkspaceNotFound`, `inkspaceUnreadable` (Inkspace is running, say), `inkspaceDamaged` or
   * `inkspaceTooLarge`.
   */
  inkspaceInspect: (path: string) => call<InkspaceSource>('inkspace_inspect', { path }),
  /**
   * Imports the notes of `path` into `binder` (empty: the library root), skipping those already
   * there; `onProgress` hears how many are handled, before the first one and after each. A
   * stopped import resolves with `cancelled: true`; `inkspaceBusy` when another one is running.
   */
  inkspaceImport: (path: string, binder: string, onProgress: (progress: InkspaceProgress) => void = () => {}) => {
    if (!inDesktopApp()) return call<InkspaceReport>('inkspace_import', { path, binder, onProgress });
    const channel = new Channel<InkspaceProgress>();
    channel.onmessage = onProgress;
    return invoke<InkspaceReport>('inkspace_import', { path, binder, onProgress: channel });
  },
  /** Asks the running import to stop after the note under way. */
  inkspaceCancel: () => call<void>('inkspace_cancel'),

  // --- Handwriting recognition (milestone M7; Rust side: handwriting.rs) ----------------------

  /** The recognizers of this PC, the language setting and the language it gives, the indexing. */
  handwritingInfo: () => call<HandwritingInfo>('handwriting_info'),
  /** Sets the recognition language (shared by every PC) and indexes again. */
  setHandwritingLanguage: (language: HandwritingSetting) =>
    call<HandwritingInfo>('set_handwriting_language', { language }),
  handwritingStatus: () => call<IndexStatus>('handwriting_status'),
  /** The notes whose handwriting has words of `query`, each with an excerpt. */
  handwritingSearch: (query: string) => call<HandwritingHit[]>('handwriting_search', { query }),
  /** Stops the background indexing until `handwritingResume`. */
  handwritingStop: () => call<IndexStatus>('handwriting_stop'),
  handwritingResume: () => call<void>('handwriting_resume'),
  /** The recognised text of each source (recognised now when it is not indexed yet). */
  handwritingText: (sources: ExportSource[]) => call<NoteText[]>('handwriting_text', { sources }),
  /** Saves the recognised text of `sources`, one note after the other, as UTF-8; returns the path. */
  exportHandwritingText: (sources: ExportSource[], destination: string) =>
    call<string>('export_handwriting_text', { sources, destination }),
  /** Copies the recognised text to the clipboard (Windows; `clipboardUnavailable` elsewhere). */
  copyHandwritingText: (sources: ExportSource[]) => call<void>('copy_handwriting_text', { sources }),
  /**
   * Saves the recognised text with its structure (title, date and tags, then paragraphs, list
   * items and drawings) as Markdown (drawings as PNG files next to it) or as a Word document.
   * `labels` are the words the file contains, in the interface language; `includeDrawings`
   * false leaves the drawings out.
   */
  exportStructuredText: (
    sources: ExportSource[],
    format: StructuredFormat,
    destination: string,
    labels: TextLabels,
    includeDrawings: boolean,
  ) => call<StructuredExport>('export_structured_text', { sources, format, destination, labels, includeDrawings }),
  /**
   * How many drawings the Markdown and Word exports of `sources` hold (the layout analysis takes a
   * few seconds a note; the desktop app keeps it for the export that follows).
   */
  structuredTextDrawings: (sources: ExportSource[]) => call<number>('structured_text_drawings', { sources }),

  // --- Printing (milestone M8; Rust side: print.rs, print_windows.rs) ---

  /**
   * Opens Windows' print dialog and prints each note on a page of its own, in its orientation.
   * Rejects with `printUnavailable` outside Windows.
   */
  printNotes: (sources: ExportSource[], options: PrintOptions) =>
    call<PrintResult>('print_notes', { sources, options }),

  // --- Assistant bridge (milestone M9; Rust side: app_bridge.rs, docs/design/mcp.md) ---
  /** The main window's answer to request `id` of the assistant's bridge. */
  bridgeReply: (id: number, answer: BridgeAnswer) => call<void>('bridge_reply', { id, answer }),
  /** How to start this copy's MCP server (Settings > Assistant). */
  mcpLaunchInfo: () => call<McpLaunch>('mcp_launch_info'),

  // --- About (milestone M10; Rust side: about.rs) -----------------------------------------------
  /** Version, licence and the third-party components built into this copy. */
  aboutInfo: () => call<AboutInfo>('about_info'),
  /** The licence of Ardoise (`license`) or the licences of its components (`notices`), in full. */
  aboutText: (which: AboutText) => call<string>('about_text', { which }),

  // --- Drafts and the end of the Windows session (M10; drafts.rs, session_end.rs) ---
  /** Keeps the editor's unsaved note outside the library (replacing its previous draft). */
  draftWrite: (snapshot: DraftSnapshot) =>
    call<void>('draft_write', {
      path: snapshot.path,
      noteId: snapshot.noteId,
      note: snapshot.note,
      baseStamp: snapshot.baseStamp,
      imageKey: snapshot.imageKey,
    }),
  /** The draft kept for a note of the library, if any. */
  draftFind: (noteId: string) => call<Draft | null>('draft_find', { noteId }),
  /** The drafts left by an earlier run of Ardoise, closed without saving them. */
  draftList: () => call<DraftSummary[]>('draft_list'),
  draftDiscard: (noteId: string) => call<void>('draft_discard', { noteId }),
  /** Stages the background image kept with a draft; null when it kept none. */
  draftStageImage: (noteId: string) => call<StagedBackground | null>('draft_stage_image', { noteId }),
  /** Makes a new note titled `title` of a draft (the note it came from is not touched), then forgets the draft. */
  draftRestoreCopy: (noteId: string, title: string) => call<NoteSummary>('draft_restore_copy', { noteId, title }),
  /** What Windows shows while unsaved work holds the end of the session back; null when nothing is unsaved. */
  setUnsavedWork: (reason: string | null) => call<void>('set_unsaved_work', { reason }),
};

/**
 * Calls `handler` when Windows ends the session (shutdown, restart, logoff): the drafts are
 * written at once. Returns a function that stops listening. Nothing comes in a plain browser.
 */
export async function onSessionEnding(handler: () => void): Promise<() => void> {
  if (!inDesktopApp()) return () => {};
  return listen('session-ending', () => handler());
}

/** `McpLaunch` in `app_bridge.rs`. */
export interface McpLaunch {
  /** Absolute path of the running executable. */
  exe: string;
  /** Whether assistants can drive this window now. */
  bridge: BridgeStatus;
}

/** `BridgeStatus` in `app_bridge.rs`. */
export interface BridgeStatus {
  state: 'serving' | 'retrying' | 'anotherCopy' | 'unavailable' | 'off';
  /** The technical reason, for `retrying` and `unavailable`. */
  detail: string | null;
}

/**
 * Calls `handler` with each request of the assistant's bridge (`bridge-request`, main window only).
 * Returns a function that stops listening. Nothing comes in a plain browser.
 */
export async function onBridgeRequest(handler: (event: BridgeEvent) => void): Promise<() => void> {
  if (!inDesktopApp()) return () => {};
  return listen<BridgeEvent>('bridge-request', (event) => handler(event.payload));
}

/**
 * Calls `handler` at every change of the handwriting indexing (progress, pause, end). Returns a
 * function that stops listening.
 */
export async function onHandwritingStatus(handler: (status: IndexStatus) => void): Promise<() => void> {
  if (!inDesktopApp()) return mockOnHandwritingStatus(handler);
  return listen<IndexStatus>('handwriting-status', (event) => handler(event.payload));
}

/**
 * Calls `handler` with the indexing status now and at every change, without missing a change
 * that happens while listening starts. Returns a function that stops.
 */
export function followHandwritingStatus(handler: (status: IndexStatus) => void): () => void {
  let stopped = false;
  let events = 0;
  let unlisten: (() => void) | null = null;
  void onHandwritingStatus((status) => {
    events += 1;
    if (!stopped) handler(status);
  }).then((stop) => {
    if (stopped) return stop();
    unlisten = stop;
    // Read once listening: a status that changed before is not lost, a newer event wins.
    const before = events;
    api
      .handwritingStatus()
      .then((status) => {
        if (!stopped && events === before) handler(status);
      })
      .catch(() => {});
  });
  return () => {
    stopped = true;
    unlisten?.();
  };
}

/**
 * Calls `handler` whenever notes change on disk (another PC through Nextcloud, or this app).
 * Returns a function that stops listening.
 */
export async function onLibraryChanged(handler: () => void): Promise<() => void> {
  if (!inDesktopApp()) return mockOnLibraryChanged(handler);
  return listen('library-changed', () => handler());
}

/**
 * « Something went wrong. » followed by the error's text as a sentence of its own: the
 * `errorCodes.*` phrases start in lower case and have no full stop (they also fill other
 * sentences), so this gives the first letter a capital and the end a full stop.
 */
export function genericError(message: string): string {
  const text = sentence(message);
  if (text === '') return t('errors.generic', { message: '' }).trim();
  return t('errors.generic', { message: text });
}

/**
 * An `errorCodes.*` phrase as a sentence by itself: a capital first and a full stop at the end,
 * without the "An error occurred." of `genericError`. For the places where the phrase explains
 * the problem fully (the share panel).
 */
export function sentence(message: string): string {
  const text = message.trim();
  if (text === '') return '';
  const capital = text.charAt(0).toLocaleUpperCase() + text.slice(1);
  return /[.!?…]$/.test(capital) ? capital : `${capital}.`;
}

/**
 * Text of an error thrown by a command. Rust sends `{ code, message }`: the code picks a
 * translated sentence (`errorCodes.*`), the message is the technical detail.
 */
export function errorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'code' in error && 'message' in error) {
    const { code, message } = error as { code: string; message: string };
    const key = `errorCodes.${code}`;
    const text = t(key, { detail: message });
    return text === key ? message : text;
  }
  // Anything else (a rejected argument, a bug) is not meant for the user as is.
  return t('errorCodes.unexpected', { detail: error instanceof Error ? error.message : String(error) });
}
