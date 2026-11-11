/**
 * The editor's projection (milestone M17; Rust side `app/src-tauri/src/projection.rs`, docs/design/paper-projection-narration.md
 * §2): what the editor tells the projection window's Rust side, and what the editor's Projection tab learns of the
 * window. The feed that computes the updates is `lib/projection-feed.ts`.
 */
import type { Tool } from './ink';
import type { MonitorInfo } from './live';
import type { Note, Rgba, Stroke } from './note';

/** The projected area: the zoom (1 = the whole page) and its centre, page units. */
export interface ProjectionView {
  zoom: number;
  x: number;
  y: number;
}

/** A point of the editor's stroke in progress. `wall`: Unix ms of the pointer sample (the window's latency figures count from it). */
export interface ProjectionPoint {
  x: number;
  y: number;
  /** Pressure, 0..1. */
  p: number;
  /** Note time, ms. */
  t: number;
  wall: number;
}

/** Most points one update may carry, and most strokes one `strokes` tail (the feed splits and sends the page instead). */
export const MAX_PROJECTION_POINTS = 4096;
export const MAX_PROJECTION_STROKES = 1 << 20;

/**
 * One update of the projection, applied in order. Refused with `projectionStale` (`strokes` whose `keep` exceeds what
 * the projection holds, or without a page) or `projectionInvalid` (numbers that make no sense): the feed then sends
 * `page` again.
 */
export type ProjectionUpdate =
  /** The whole page (a note opened, its page, paper, image, layers or image key changed). */
  | { kind: 'page'; note: Note; imageKey: string | null; view: ProjectionView }
  /** The strokes from `keep` on are replaced by `tail`; `ended`: the stroke in progress `id` is now among them. */
  | { kind: 'strokes'; keep: number; tail: Stroke[]; ended: number | null }
  | { kind: 'view'; view: ProjectionView }
  | { kind: 'strokeBegin'; id: number; tool: Tool; color: Rgba; width: number; layer: number; points: ProjectionPoint[] }
  | { kind: 'strokePoints'; id: number; points: ProjectionPoint[] }
  | { kind: 'strokeCancel'; id: number };

/** Where the projection is open (`projection_watch`): the screen, or null when it is closed. */
export interface ProjectionStatus {
  open: MonitorInfo | null;
}
