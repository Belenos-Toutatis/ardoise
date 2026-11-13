/**
 * The tablet service as the interface sees it (`ardoise_device::service`, `app/src-tauri/src/tablet.rs`).
 * Contract: docs/design/tablet-live-classroom.md; the second tablet (the Xiaomi blackboard, M18):
 * docs/design/blackboard.md §5.
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
  | 'live'
  /** Looking for a blackboard to add (`boardSearch`). */
  | 'adding';

/** The two tablets Ardoise knows: the Wacom Bamboo Slate and the Xiaomi Mijia LCD blackboard. */
export type TabletKind = 'slate' | 'blackboard';

export type RegisterStep = 'holdButton' | 'searching' | 'connecting' | 'pressButton' | 'done';

import type { Orientation } from './orientation';

export type { Orientation };

/**
 * How the Xiaomi blackboard is held: where its two buttons (erase, store) are. Held upright, they are at the bottom
 * (the default). The interface: docs/design/blackboard.md §10; what Rust does with it: §11.
 */
export type BoardPosition = 'buttonsBottom' | 'buttonsTop' | 'buttonsRight' | 'buttonsLeft';

/** What the tablet's button does in live mode. */
export type LiveButton = 'newPage' | 'newLayer';

/** `[tablet]` of `ardoise.toml`, shared by every PC. */
export interface TabletOptions {
  autoSync: boolean;
  /** Delete each page from the tablet once its note is written and checked (on by default). */
  deleteAfterImport: boolean;
  /** How the Slate is held (four positions; the live window switches it too). The blackboard's is `boardPosition`. */
  orientation: Orientation;
  /**
   * How the blackboard is held (four positions; `[tablet] board_position` in the settings file). Rust refuses options
   * without it; a status without it (an older Rust side) is read with `boardPositionOf`.
   */
  boardPosition: BoardPosition;
  button: LiveButton;
}

/** What the tablet said about itself at the last connection. */
export interface DeviceDetails {
  /** Which tablet these details are about. */
  kind: TabletKind;
  name: string;
  /** Empty for the blackboard (it has none to read). */
  firmware: string;
  batteryPercent: number;
  /** Always false for the blackboard. */
  charging: boolean;
  /** Drawing area in device units (10 µm). */
  width: number;
  height: number;
  storedPages: number;
  /** `Bamboo Slate A4`, `Bamboo Slate A5`, `Wacom (W × H)` or `Xiaomi Mijia LCD blackboard`. */
  model: string;
  /** Unix milliseconds. */
  readAt: number;
  /** Total and free memory: the blackboard only (null for the Slate). */
  memory: DeviceMemory | null;
}

/** Memory of the blackboard, in bytes. */
export interface DeviceMemory {
  total: number;
  free: number;
}

/** A tablet the library knows: the Slate of `device.json`, the blackboard of `blackboard.json`. */
export interface KnownTablet {
  kind: TabletKind;
  /** The Slate's name; `Mijia LCD blackboard` for the blackboard. */
  name: string;
  address: string;
  /** RFC 3339: registered (Slate) or added (blackboard). */
  added: string;
  /** Drawing area in device units (10 µm); 20500 × 27200 for the blackboard. */
  width: number;
  height: number;
  model: string;
}

/** A blackboard heard by the search for one to add. */
export interface FoundTablet {
  address: string;
  /** As advertised (`xiaomi.dev.ble` on the real tablet). */
  name: string | null;
  /** Signal strength in dBm, when known. */
  rssi: number | null;
}

/** The search for a blackboard to add, while `connection` is `adding`. */
export interface BoardSearch {
  found: FoundTablet[];
  /** Unix milliseconds when the search gives up. */
  until: number;
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
  /**
   * Why the sync stopped before the end (refused by the tablet, page not saved...): `code`
   * selects `errorCodes.*`. Stays until the next sync, across reconnections.
   */
  error: { code: string; message: string } | null;
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
  /**
   * The tablet of the connection: set only while `connection` is `connecting`, `connected`, `syncing`
   * or `live`. `registered` stays the Slate's (null when only the blackboard is known).
   */
  active: TabletKind | null;
  /** Every known tablet, the Slate first (empty when none). */
  known: KnownTablet[];
  /** While `connection` is `adding`. */
  boardSearch: BoardSearch | null;
}

export type TabletEvent = { type: 'status'; status: TabletStatus };
