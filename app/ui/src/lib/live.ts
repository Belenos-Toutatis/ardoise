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

/**
 * What `live_edit_strokes` does to strokes chosen in the control window: `indices` are places in
 * `LiveState.strokes` (the finished strokes of the page, in any order, without duplicates).
 * `group`: the number of an eraser gesture (any number the window picks, one per gesture); the
 * steps of one group are undone and redone together, in one command.
 */
export type LiveStrokesEdit =
  | { kind: 'delete'; indices: number[]; group?: number }
  | { kind: 'recolour'; indices: number[]; color: Rgba };

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

/** Which pen wrote a stroke: the tablet's, or the computer's (M14: its pen or the mouse of the control window). */
export type LivePen = 'tablet' | 'computer';

/**
 * One sample of a computer pen stroke, as the control window sends it (`live_computer_pen`): `x`, `y`
 * in page units of the page shown, `p` the pressure 0..1 (0.5 for a pressed mouse), `ms` the event's
 * `timeStamp`. Rust turns `ms`, against `now` of the same call, into a wall-clock time.
 */
export interface ComputerSample {
  x: number;
  y: number;
  p: number;
  ms: number;
}

/**
 * What `live_computer_pen` takes. `stroke` is the window's number for the gesture (a counter from 1): Rust
 * ignores a call for another number than the stroke in progress, and a `begin` while one is in progress ends
 * that one first (kept). `now` is `performance.now()` when the call is made, the clock of `ms`. The stroke
 * goes on the page's computer layer, with `style` (`LiveStyle`, sanitized by Rust). At most 4096 samples a
 * call. Samples without finite coordinates are dropped, `p` is clamped.
 */
export type LiveComputerPenInput =
  | { kind: 'begin'; stroke: number; pageIndex: number; style: LiveStyle; samples: ComputerSample[]; now: number }
  | { kind: 'points'; stroke: number; samples: ComputerSample[]; now: number }
  | { kind: 'end'; stroke: number; samples: ComputerSample[]; now: number }
  | { kind: 'cancel'; stroke: number };

/**
 * What `live_layers` does to the layers of page `pageIndex` (the one on screen). Layers are indices in
 * `Note.layers`, bottom first, sent as plain numbers; a bad one is refused as a whole with
 * `liveLayerInvalid`. `add`: a layer on top, which the computer then acts on. `merge`: `layer` into the one
 * below (the lower one stays, with its name); it moves places, so the page's undo and redo are dropped.
 * `computer`, `tablet`: the layer the computer's pen (and eraser and selection), or the tablet, writes on;
 * they change no note. No op ends a stroke in progress, and none is undone by Ctrl+Z.
 */
export type LiveLayerOp =
  | { kind: 'add' }
  | { kind: 'rename'; layer: number; name: string }
  | { kind: 'visible'; layer: number; visible: boolean }
  | { kind: 'move'; from: number; to: number }
  | { kind: 'merge'; layer: number }
  | { kind: 'computer'; layer: number }
  | { kind: 'tablet'; layer: number };

export type LiveSessionState = 'idle' | 'waiting' | 'live' | 'reconnecting';

/** Why a resumed sheet became a new note: it changed elsewhere, or no longer exists. */
export type SheetForkReason = 'changed' | 'deleted';

export interface LiveSnapshot {
  state: LiveSessionState;
  /** The current page (a note: strokes so far, background, layers). */
  page: Note;
  /**
   * The share's viewers only: which picture (`livePaper`, `/paper/<key>`) the page wants drawn under its strokes;
   * absent on plain paper (ordinary live pages), where the paper colour is drawn.
   */
  paperKey?: string;
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
  /**
   * The layer the tablet writes on, an index in `page.layers` (M14: it may be a hidden one, which the
   * presentation and the share then do not show).
   */
  layer: number;
  /**
   * The layer the computer's pen, eraser and selection act on. Absent on the share's viewers, which
   * do not need it (read it as `layer`).
   */
  computerLayer?: number;
  style: LiveStyle;
  view: LiveViewState;
  hover: { x: number; y: number } | null;
  /** The screen the presentation window is on, or null when it is closed. */
  presentation: MonitorInfo | null;
  /** Paths of the notes saved so far in this session. */
  saved: string[];
  /**
   * What the indices of the page's strokes mean (`live_edit_strokes` gives it back, and is refused
   * once it changed). It changes when strokes leave or come back: a delete, a clearing that removed
   * something, the undo or redo of either; not when the tablet appends a stroke (a selection stays
   * valid while the tablet writes), nor for a change of colour or a page turn. Each session has its
   * own. Absent on the share's viewers, like the next two.
   */
  revision?: number;
  /** The page has an edit to undo. */
  canUndo?: boolean;
  /** The page has an edit to redo. */
  canRedo?: boolean;
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

/** A stroke that comes back, and where it stands once it is back. */
export interface PlacedStroke {
  index: number;
  stroke: Stroke;
}

/** Strokes that take one colour. */
export interface RecolouredStrokes {
  color: Rgba;
  /** Ascending. */
  indices: number[];
}

/**
 * Finished strokes of a page were deleted, came back or changed colour (an edit, its undo or redo).
 * Applied in this order: `removed`, `inserted`, `recoloured`; the stroke in progress is not
 * concerned. Strokes that are not named keep their place and their object, hence their outline.
 * Never merged with another event. On the share's viewers the indices are those of their own list,
 * which has no hidden strokes, and `revision`, `canUndo` and `canRedo` are absent.
 */
export interface LiveStrokesEdited {
  type: 'strokesEdited';
  pageIndex: number;
  /** The page's revision after this event. */
  revision?: number;
  /** Indices in the list before this event, ascending. */
  removed: number[];
  /** Each with its index in the list after this event, ascending. */
  inserted: PlacedStroke[];
  /** Indices in the list after this event. */
  recoloured: RecolouredStrokes[];
  canUndo?: boolean;
  canRedo?: boolean;
}

export type LiveEvent =
  | { type: 'snapshot'; snapshot: LiveSnapshot }
  /**
   * The strokes in progress, one per pen (`pen` is written only for the computer's, `id` is the window's
   * number of its gesture: `LiveComputerPenInput.stroke`). The two may interleave: events of the
   * two pens are never merged, and a stroke of one pen never ends the other's.
   */
  | { type: 'strokeStart'; pageIndex: number; stroke: LiveStroke; pen?: 'computer'; id?: number }
  | { type: 'points'; pageIndex: number; points: LivePoint[]; pen?: 'computer'; id?: number }
  | { type: 'strokeEnd'; pageIndex: number; pen?: 'computer'; id?: number }
  /**
   * The stroke in progress of that pen is dropped, not kept (the computer's `cancel`, or « Clear the
   * page », which drops both pens' strokes).
   */
  | { type: 'strokeCancel'; pageIndex: number; pen?: 'computer'; id?: number }
  /**
   * What the page can undo or redo changed without strokes leaving or coming back (a computer stroke
   * was recorded as a step). Never sent to the share's viewers.
   */
  | { type: 'history'; pageIndex: number; canUndo: boolean; canRedo: boolean }
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
   * is. Sent once, and to a window that subscribes later while the session runs; never to the
   * share's viewers.
   */
  | { type: 'sheetForked'; pageIndex: number; reason: SheetForkReason; original: string; path: string }
  | LiveStrokesEdited;

export interface LiveBatch {
  events: LiveEvent[];
  /** Wall-clock time (ms since the Unix epoch, fractional) when Rust sent the batch. */
  sentAt: number;
}

/** A resumed sheet whose page went on in another note. */
export interface SheetFork {
  pageIndex: number;
  reason: SheetForkReason;
  /** The note left as it is, and the new one. */
  original: string;
  path: string;
}

/** What `live_stop` answers: the view closes on it, so what the last write did is said here too. */
export interface LiveStopped {
  /** Paths of the session's notes. */
  saved: string[];
  /** The resumed sheets that went on in a new note in this session, the last write included. */
  forks: SheetFork[];
  /**
   * A recording still running at the stop could not join its pages: `errorMessage` says why, and
   * where its sound is kept (`narrationTooLong`, `narrationUnreadable`). `null` otherwise.
   */
  narration: { code: string; message: string } | null;
}
