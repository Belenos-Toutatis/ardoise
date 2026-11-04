/**
 * Live mode and the classroom (`app/src-tauri/src/live.rs`, `presentation.rs`).
 * Contract: docs/design/tablet-live-classroom.md.
 */
import type { Tool } from './ink';
import type { Note, Rgba, Stroke } from './note';
import type { Orientation } from './orientation';

export interface LiveStyle {
  tool: Tool;
  color: Rgba;
  /** Nominal diameter at full pressure, device units. */
  width: number;
}

/** View of the projected area, shared by every window. */
export interface LiveViewState {
  /** 1 = the whole page, up to 8. */
  zoom: number;
  /** Centre of the projected area, page units. */
  x: number;
  y: number;
  /** Blackboard: slate paper, chalk. */
  board: boolean;
  /** The hovering pen shows a dot and writes nothing. */
  laser: boolean;
}

export type LiveCommandKind = 'nextPage' | 'previousPage' | 'newPage' | 'newLayer' | 'clearPage' | 'button';
export interface LiveCommand {
  kind: LiveCommandKind;
}

export interface MonitorInfo {
  /** `name@x,y`: stable when screens are unplugged. */
  key: string;
  name: string;
  /** Physical pixels. */
  x: number;
  y: number;
  width: number;
  height: number;
  scale: number;
  primary: boolean;
  /** The saved choice. */
  chosen: boolean;
}

export type LiveSessionState = 'idle' | 'waiting' | 'live' | 'reconnecting';

/** Why a resumed sheet became a new note: it changed elsewhere, or no longer exists. */
export type SheetForkReason = 'changed' | 'deleted';

export interface LiveSnapshot {
  state: LiveSessionState;
  /** The current page (a note: strokes so far, background, layers). */
  page: Note;
  pageIndex: number;
  pageCount: number;
  /**
   * How the tablet is held: the page is in this orientation (the live window's quick switch). On a
   * resumed sheet held the other way round than the setting, how it is to be held for that page.
   */
  orientation: Orientation;
  /**
   * Whether the current page turns with the tablet: false on a resumed sheet, which is never turned
   * (the quick switch is disabled there). Absent on the share's viewers, which do not need it.
   */
  canTurnPage?: boolean;
  /** Layer receiving new strokes: the current page's last visible one. */
  layer: number;
  style: LiveStyle;
  view: LiveViewState;
  hover: { x: number; y: number } | null;
  /** The screen the presentation window is on, or null when it is closed. */
  presentation: MonitorInfo | null;
  /** Paths of the notes saved so far in this session. */
  saved: string[];
}

/** A point of the stroke in progress. */
export interface LivePoint {
  x: number;
  y: number;
  /** Pressure, 0..1. */
  p: number;
  /** Milliseconds since the note's `created`. */
  t: number;
  /** Wall-clock time (ms since the Unix epoch, fractional) when Rust received it. */
  recv: number;
}

/** A point of a mark written over the presentation. */
export interface LiveMarkPoint {
  x: number;
  y: number;
  /** Pressure, 0..1. */
  p: number;
}

/**
 * A temporary mark written over the presentation, as Rust holds it for the viewers of the share
 * (`live_set_marks`, the `marks` event): the ink and the points, in page units.
 */
export interface LiveMark {
  tool: Tool;
  color: Rgba;
  /** Line diameter at full pressure, page units. */
  width: number;
  points: LiveMarkPoint[];
}

/** A stroke as `strokeStart` sends it: its points carry their receipt time. */
export type LiveStroke = Omit<Stroke, 'points'> & { points: LivePoint[] };

export type LiveEvent =
  | { type: 'snapshot'; snapshot: LiveSnapshot }
  | { type: 'strokeStart'; pageIndex: number; stroke: LiveStroke }
  | { type: 'points'; pageIndex: number; points: LivePoint[] }
  | { type: 'strokeEnd'; pageIndex: number }
  | { type: 'hover'; x: number; y: number }
  | { type: 'hoverEnd' }
  | { type: 'view'; view: LiveViewState }
  /**
   * The marks written over the presentation, the whole list for the current page (empty: none, or
   * the page changed). Only the share's viewers draw them.
   */
  | { type: 'marks'; marks: LiveMark[] }
  | { type: 'saved'; paths: string[] }
  /**
   * A page could not be written into the library (sent once per page while the error repeats).
   * The pages stay in memory; a later `saved` means that everything is written.
   */
  | { type: 'saveFailed'; pageIndex: number; code: string; message: string }
  /**
   * The resumed sheet on this page changed elsewhere or was deleted: nothing was overwritten, and
   * the page goes on in a new note (`path`) in the same binder; `original` is the note left as it
   * is. Sent once, and to a window that subscribes later; never to the share's viewers.
   */
  | { type: 'sheetForked'; pageIndex: number; reason: SheetForkReason; original: string; path: string };

export interface LiveBatch {
  events: LiveEvent[];
  /** Wall-clock time (ms since the Unix epoch, fractional) when Rust sent the batch. */
  sentAt: number;
}
