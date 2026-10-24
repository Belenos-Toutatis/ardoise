/** Typed wrappers around the Rust commands of `app/src-tauri/src/commands.rs`. */
import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { Decision, DiagnosticEvent, DiagnosticOptions } from './diagnostic';
import type { LanguagePreference } from './i18n-core';
import { t } from './i18n.svelte';
import { mockDiagnosticStart, mockInvoke, mockOnLibraryChanged } from './mock';
import type {
  Background,
  FileStamp,
  LibraryListing,
  Note,
  NoteSummary,
  OpenedNote,
  PageSize,
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
}

export interface LibraryInspection {
  /** The folder already holds an Ardoise library (`_ardoise/` present). */
  existing: boolean;
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
  /** Moves notes to the trash. */
  trashNotes: (paths: string[]) => call<void>('trash_notes', { paths }),
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

  // --- Editor -------------------------------------------------------------------------------

  openNote: (path: string) => call<OpenedNote>('open_note', { path }),
  /**
   * Saves an edited note (thumbnail regenerated, tablet original and other members kept);
   * rejects with code `noteConflict` if the file changed since `stamp`.
   */
  saveNote: (path: string, note: Note, stamp: FileStamp) => call<FileStamp>('save_note', { path, note, stamp }),
  /** PNG data URL of a page background (paper, template, texture) `widthPx` pixels wide. */
  renderBackground: (page: PageSize, background: Background, widthPx: number) =>
    call<string>('render_background', { page, background, widthPx }),

  // --- Settings: tablet ---------------------------------------------------------------------

  /** The tablet registered from this library, if any. */
  tabletInfo: () => call<TabletInfo | null>('tablet_info'),
  /** Forgets the registered tablet (it must then be registered again). */
  forgetTablet: () => call<void>('forget_tablet'),
};

/**
 * Calls `handler` whenever notes change on disk (another PC through Nextcloud, or this app).
 * Returns a function that stops listening.
 */
export async function onLibraryChanged(handler: () => void): Promise<() => void> {
  if (!inDesktopApp()) return mockOnLibraryChanged(handler);
  return listen('library-changed', () => handler());
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
