/**
 * Sharing of the sheet on the local network (milestone M11; Rust side: `crates/ardoise-share`,
 * `app/src-tauri/src/share.rs`). Contract: docs/design/lan-share.md.
 *
 * Two audiences:
 * - the app's main window (commands `share_*`, `ShareStatus`, `ShareContent`);
 * - the viewer, a page opened in a browser on another PC (`viewer.html`), which receives the
 *   `ShareMessage`s of the event stream `/events`. It never imports `api.ts` nor Tauri.
 */
import type { LiveBatch } from './live';
import type { Background, Layer, Note, PageSize, Stroke } from './note';

// --- App side --------------------------------------------------------------------------------

/** What the main window shows, hence what the share shows (`share_show`). */
export type ShareContent =
  | { kind: 'waiting' }
  | { kind: 'live' }
  /** The note open in the editor, unsaved changes included; `imageKey`: its staged background image. */
  | { kind: 'note'; note: Note; imageKey: string | null };

export type Shown = 'waiting' | 'live' | 'note';

/** A QR code: `size` modules a side, `path` in a `viewBox="0 0 size size"` (no border). */
export interface QrCode {
  size: number;
  path: string;
}

export interface ShareStatus {
  active: boolean;
  /** The port listened on; the configured one (`[share] port`) when inactive. */
  port: number;
  /** The code the page asks for, shown as `K7M Q2X`; null when inactive. */
  code: string | null;
  /** `http://192.168.1.23:8765/`…, private LAN addresses first; [] when inactive. */
  addresses: string[];
  /** QR code of `addresses[0]`. */
  qr: QrCode | null;
  /** Screens connected. */
  viewers: number;
  paused: boolean;
  shown: Shown;
}

export const INACTIVE_SHARE: ShareStatus = {
  active: false,
  port: 8765,
  code: null,
  addresses: [],
  qr: null,
  viewers: 0,
  paused: false,
  shown: 'waiting',
};

// --- Viewer side: the event stream -------------------------------------------------------------

/** The page of a note as the viewer draws it (the `Note` fields that are drawn). */
export interface SharedPage {
  id: string;
  page: PageSize;
  background: Background;
  layers: Layer[];
  strokes: Stroke[];
}

/** One message of `/events` (the `data:` of one Server-Sent Event, JSON). */
export type ShareMessage =
  /** Nothing to share: the waiting screen. */
  | { type: 'waiting' }
  /**
   * A batch of live mode, as the presentation window receives it (library paths and monitor
   * stripped). `reset`: the first batch of a subscription, starting with a snapshot: start again
   * from it.
   */
  | { type: 'live'; reset: boolean; batch: LiveBatch }
  /** The whole page of the note open in the editor; `background`: PNG data URL of the paper, null for plain paper. */
  | { type: 'note'; page: SharedPage; background: string | null }
  /** Strokes added at the end of the page last sent. */
  | { type: 'noteStrokes'; strokes: Stroke[] }
  /** The image is frozen (`true`) or follows again (`false`). */
  | { type: 'pause'; paused: boolean }
  /** The share stopped. */
  | { type: 'end' };

/** `POST /join` refusals (JSON body). */
export type JoinRefusal =
  | { error: 'wrongCode'; remaining: number }
  | { error: 'tooManyAttempts'; retryAfter: number };
