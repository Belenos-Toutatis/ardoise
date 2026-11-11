/**
 * The editor's side of its projection (milestone M17; docs/design/paper-projection-narration.md §2.6): the screen
 * list and the choice, whether the projection is open (told by Rust, also when it is closed from its own window), the
 * projected area, and the feed of the page to the projection window.
 *
 * Package 0: the final API, every method a no-op; package D builds it, package E wires it into `Editor.svelte`.
 */
import { api } from '../api';
import type { MonitorInfo } from '../live';
import type { Note } from '../note';
import type { InProgress } from '../projection-feed';
import type { Rect } from './geometry';

/** The commands the projection uses (the tests pass their own). */
export type ProjectionCommands = Pick<
  typeof api,
  'listMonitors' | 'projectionOpen' | 'projectionClose' | 'projectionWatch' | 'projectionUpdate'
>;

export class EditorProjection {
  monitors = $state.raw<MonitorInfo[]>([]);
  /** The screen chosen (`MonitorInfo.key`). */
  monitorKey = $state('');
  /** The list of screens was asked for at least once. */
  loaded = $state(false);
  /** The screen projected on, or null. */
  presentation = $state.raw<MonitorInfo | null>(null);
  busy = $state(false);
  error = $state<string | null>(null);
  /** The projection's own zoom (1 = the whole page). */
  zoom = $state(1);

  /** The projected area on the page (page units) while projecting with a zoom above 1, for the editor's frame. */
  get frame(): Rect | null {
    return null;
  }

  constructor(commands: ProjectionCommands = api) {
    void commands;
  }

  /** Starts hearing where the projection is open (once, at mount). */
  watch(): void {}

  /** Lists the screens again. */
  async refresh(): Promise<void> {}

  /** Opens the projection on the chosen screen. */
  async open(): Promise<void> {}

  /** Closes the projection. */
  async close(): Promise<void> {}

  /** While open: what the editor shows (a replay included) and the staged pixels of its image. */
  show(note: Note, imageKey: string | null): void {
    void [note, imageKey];
  }

  /** While open: the stroke in progress. */
  stroke(event: InProgress): void {
    void event;
  }

  /** What the editor's canvas shows of the page (page units): the projected area is centred on it. */
  visible(rect: Rect): void {
    void rect;
  }

  /** One step in (1) or out (-1) of the projection's own zoom, between 1 and 8. */
  zoomStep(direction: 1 | -1): void {
    void direction;
  }

  /** The projection's zoom 1: the whole page. */
  wholePage(): void {}

  /** The projection's zoom that shows the page's full width (at least 1). */
  pageWidth(): void {}

  /** The editor goes: closes the projection. */
  dispose(): void {}
}
