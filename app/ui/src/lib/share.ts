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
  /** The port listened on; the one this PC asks for first (see `SharePortSetting`) when inactive. */
  port: number;
  /**
   * The port the share was started asking for. It differs from `port` when another program held
   * that one and the share took the first free port after it (kept for this PC); equal when
   * inactive.
   */
  requestedPort: number;
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

/** The share's port as the settings screen shows it (`share_port`, `set_share_port`). */
export interface SharePortSetting {
  /** The port this PC asks for first: its own if it has one, else `shared`. */
  port: number;
  /** `[share] port` of `ardoise.toml`, which every PC without a port of its own uses. */
  sharedPort: number;
  /** This PC has a port of its own (chosen here, or kept after a conflict). */
  own: boolean;
}

/** Lowest and highest port the settings accept. */
export const SHARE_PORT_MIN = 1024;
export const SHARE_PORT_MAX = 65535;

/** Whether `text` is a whole number from `SHARE_PORT_MIN` to `SHARE_PORT_MAX` (no sign, no decimals). */
export function parseSharePort(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d{1,5}$/.test(trimmed)) return null;
  const port = Number(trimmed);
  return port >= SHARE_PORT_MIN && port <= SHARE_PORT_MAX ? port : null;
}

export const INACTIVE_SHARE: ShareStatus = {
  active: false,
  port: 8765,
  requestedPort: 8765,
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
  /**
   * The whole page of the note open in the editor; `background`: PNG data URL of the paper, null
   * for plain paper, absent when the paper is the one already shown (a stroke erased, an undo:
   * the picture, which may weigh megabytes, is sent only when it changes).
   */
  | { type: 'note'; page: SharedPage; background?: string | null }
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
