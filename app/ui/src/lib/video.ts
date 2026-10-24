/**
 * Replay, video export, narration and the OpenH264 fallback (milestone M6): the shapes the Rust
 * commands of `app/src-tauri/src/{video,narration,codec}.rs` take and return
 * (docs/design/media.md, "Interface package: contracts").
 */
import type { FileStamp } from './note';

export type VideoFormat = 'mp4' | 'gif' | 'webp' | 'webm';
export const VIDEO_FORMATS: readonly VideoFormat[] = [
  'mp4',
  'gif',
  'webp',
  'webm',
];

/** 1280 × 720, 1920 × 1080, 3840 × 2160 (page fitted, bands around), or the page's proportions. */
export type VideoResolution = '720p' | '1080p' | '4k' | 'page';
export const VIDEO_RESOLUTIONS: readonly VideoResolution[] = [
  '720p',
  '1080p',
  '4k',
  'page',
];

export const FRAME_RATES = [10, 15, 24, 30, 60] as const;

/** Slowest and fastest replay (SPEC §4.5), and the speeds offered. */
export const MIN_SPEED = 0.25;
export const MAX_SPEED = 16;
export const SPEEDS = [0.25, 0.5, 1, 2, 4, 8, 16] as const;
/** Longest pause kept by "supprimer les temps morts", milliseconds. */
export const TRIMMED_PAUSE_MS = 500;

/** How long the replay lasts: a speed, or a target duration (speed clamped to 0.25×–16×). */
export type VideoTiming =
  { kind: 'speed'; speed: number } | { kind: 'duration'; seconds: number };

export interface VideoExportOptions {
  format: VideoFormat;
  resolution: VideoResolution;
  frameRate: number;
  timing: VideoTiming;
  /** Cut pauses after `TRIMMED_PAUSE_MS`. */
  trimPauses: boolean;
  /** Draw the paper; false = transparent (GIF and WebP only). */
  background: boolean;
  /** The final frame stays this long, seconds (0–60). */
  holdEndSeconds: number;
  /** Include the note's narration (MP4 and WebM, when the note has one); it imposes its replay options. */
  narration: boolean;
}

export interface VideoEstimate {
  width: number;
  height: number;
  frames: number;
  /** Length of the video, milliseconds. */
  durationMs: number;
  /** Expected export time, milliseconds. */
  exportMs: number;
  /** Speed actually used (target duration, narration). */
  speed: number;
  /** Narration included in the video, or null. */
  narrationMs: number | null;
}

export interface VideoProgress {
  framesDone: number;
  framesTotal: number;
  elapsedMs: number;
}

export interface VideoExportResult {
  path: string;
  frames: number;
  durationMs: number;
  audio: boolean;
  /** Who encoded the video (diagnostics). */
  encoder: string;
}

// --- Microphone and narration ----------------------------------------------------------------

export interface Microphone {
  /** Stable id: store it to find the same microphone again. */
  id: string;
  name: string;
  isDefault: boolean;
}

/** Where a recording goes: the note replayed in the editor, or the pages of live mode. */
export type NarrationTarget =
  | {
      kind: 'replay';
      path: string;
      speed: number;
      maxPauseMs: number | null;
      startMs: number;
    }
  | { kind: 'live' };

/**
 * `silent`: only exact digital silence for a second or more: Windows blocks desktop apps from the
 * microphone (Settings > Privacy & security > Microphone), or a hardware mute.
 */
export type CaptureState =
  'waiting' | 'noData' | 'receiving' | 'silent' | 'failed';

export interface NarrationStatus {
  state: 'idle' | 'monitoring' | 'recording';
  capture: CaptureState | null;
  /** Peak and RMS of the last 50 ms, linear 0..1. */
  level: { peak: number; rms: number };
  elapsedMs: number;
  /** Name of the microphone in use. */
  device: string | null;
  /** Technical detail of a failure. */
  failure: string | null;
}

export interface NarrationSaved {
  /** Replay target: the note's new stamp. Live target: null. */
  stamp: FileStamp | null;
  durationMs: number;
  /** Live target: pages that got narration. */
  pages: number;
}

export interface NarrationInfo {
  durationMs: number;
  base: 'replay' | 'note';
  speed: number;
  maxPauseMs: number | null;
  startMs: number;
}

// --- OpenH264 fallback ----------------------------------------------------------------------

export interface OpenH264State {
  status: 'notInstalled' | 'installed' | 'damaged';
  bytes: number | null;
  /** The user allows the fallback on this PC. */
  allowed: boolean;
  /** Windows Media Foundation can write MP4 here: the fallback is not needed. */
  mediaFoundation: boolean;
  version: string;
  /** "OpenH264 Video Codec provided by Cisco Systems, Inc." (shown as is). */
  notice: string;
  /** Cisco's binary licence, full text (shown as is). */
  license: string;
}

export interface DownloadProgress {
  received: number;
  total: number | null;
}
