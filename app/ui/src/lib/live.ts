/**
 * Live mode and the classroom (`app/src-tauri/src/live.rs`, `presentation.rs`).
 * Contract: docs/design/tablet-live-classroom.md.
 */
import type { Tool } from './ink';
import type { Note, Rgba, Stroke } from './note';

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

export interface LiveSnapshot {
  state: LiveSessionState;
  /** The current page (a note: strokes so far, background, layers). */
  page: Note;
  pageIndex: number;
  pageCount: number;
  /** Layer receiving new strokes. */
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
  | { type: 'saved'; paths: string[] };

export interface LiveBatch {
  events: LiveEvent[];
  /** Wall-clock time (ms since the Unix epoch, fractional) when Rust sent the batch. */
  sentAt: number;
}
