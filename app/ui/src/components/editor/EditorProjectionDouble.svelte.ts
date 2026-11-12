/**
 * Test helper: a stand-in for `EditorProjection` (`lib/editor/projection.svelte.ts`, package D) that records what the
 * editor tells it, so that the editor's wiring is tested apart from the projection itself (M17, docs/design/
 * paper-projection-narration.md §2.6). Its state is reactive like the real one's; `instances` holds every one made.
 */
import type { MonitorInfo } from '../../lib/live';
import type { Note } from '../../lib/note';
import type { InProgress } from '../../lib/projection-feed';
import type { Rect } from '../../lib/editor/geometry';

export class ProjectionDouble {
  static instances: ProjectionDouble[] = [];

  monitors = $state.raw<MonitorInfo[]>([]);
  monitorKey = $state('');
  loaded = $state(false);
  presentation = $state.raw<MonitorInfo | null>(null);
  busy = $state(false);
  error = $state<string | null>(null);
  zoom = $state(1);
  /** What `frame` answers (the test sets it). */
  shownFrame = $state.raw<Rect | null>(null);

  readonly calls: { method: string; args: unknown[] }[] = [];
  readonly shows: { note: Note; imageKey: string | null }[] = [];
  readonly strokes: InProgress[] = [];
  readonly visibles: Rect[] = [];

  constructor() {
    ProjectionDouble.instances.push(this);
  }

  get frame(): Rect | null {
    return this.shownFrame;
  }

  #record(method: string, ...args: unknown[]) {
    this.calls.push({ method, args });
  }

  watch(): void {
    this.#record('watch');
  }
  async refresh(): Promise<void> {
    this.#record('refresh');
  }
  async open(): Promise<void> {
    this.#record('open');
  }
  async close(): Promise<void> {
    this.#record('close');
  }
  show(note: Note, imageKey: string | null): void {
    this.#record('show');
    this.shows.push({ note, imageKey });
  }
  stroke(event: InProgress): void {
    this.#record('stroke');
    this.strokes.push(event.kind === 'end' ? event : { ...event, points: [...event.points] });
  }
  visible(rect: Rect): void {
    this.#record('visible');
    this.visibles.push(rect);
  }
  zoomStep(direction: 1 | -1): void {
    this.#record('zoomStep', direction);
  }
  wholePage(): void {
    this.#record('wholePage');
  }
  pageWidth(): void {
    this.#record('pageWidth');
  }
  dispose(): void {
    this.#record('dispose');
  }

  /** How many times `method` was called. */
  count(method: string): number {
    return this.calls.filter((call) => call.method === method).length;
  }
}
