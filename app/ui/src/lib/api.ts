/** Typed wrappers around the Rust commands of `app/src-tauri/src/commands.rs`. */
import { Channel, invoke, isTauri } from '@tauri-apps/api/core';
import type { Decision, DiagnosticEvent, DiagnosticOptions } from './diagnostic';
import type { LanguagePreference } from './i18n-core';
import { t } from './i18n.svelte';
import { mockDiagnosticStart, mockInvoke } from './mock';

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
};

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
  return error instanceof Error ? error.message : String(error);
}
