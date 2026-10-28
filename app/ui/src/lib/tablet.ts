/**
 * The tablet service as the interface sees it (`ardoise_device::service`, `app/src-tauri/src/tablet.rs`).
 * Contract: docs/design/tablet-live-classroom.md.
 */
import type { TabletInfo } from './note';

export type Connection =
  | 'unavailable'
  | 'notRegistered'
  | 'registering'
  | 'searching'
  | 'connecting'
  | 'connected'
  | 'syncing'
  | 'live';

export type RegisterStep = 'holdButton' | 'searching' | 'connecting' | 'pressButton' | 'done';

import type { Orientation } from './orientation';

export type { Orientation };

/** What the tablet's button does in live mode. */
export type LiveButton = 'newPage' | 'newLayer';

/** `[tablet]` of `ardoise.toml`, shared by every PC. */
export interface TabletOptions {
  autoSync: boolean;
  /** Delete each page from the tablet once its note is written and checked (on by default). */
  deleteAfterImport: boolean;
  /** How the tablet is held (four positions; the live window switches it too). */
  orientation: Orientation;
  button: LiveButton;
}

/** What the tablet said about itself at the last connection. */
export interface DeviceDetails {
  name: string;
  firmware: string;
  batteryPercent: number;
  charging: boolean;
  /** Drawing area in device units (10 µm). */
  width: number;
  height: number;
  storedPages: number;
  /** `Bamboo Slate A4`, `Bamboo Slate A5` or `Wacom (W × H)`. */
  model: string;
  /** Unix milliseconds. */
  readAt: number;
}

export interface SyncProgress {
  /** 1-based index of the page being downloaded. */
  current: number;
  total: number;
  saved: number;
}

export interface LastSync {
  at: number;
  /** Paths of the notes written. */
  saved: string[];
  deleted: number;
}

export interface TabletStatus {
  connection: Connection;
  registered: TabletInfo | null;
  device: DeviceDetails | null;
  registerStep: RegisterStep | null;
  sync: SyncProgress | null;
  lastSync: LastSync | null;
  /** What waits for the tablet to connect. */
  pending: 'sync' | 'live' | null;
  /** Last failure: `code` selects `errorCodes.*`. */
  error: { code: string; message: string } | null;
  /** Started with `--simulated-tablet`. */
  simulated: boolean;
  options: TabletOptions;
}

export type TabletEvent = { type: 'status'; status: TabletStatus };
