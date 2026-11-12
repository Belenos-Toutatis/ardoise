/**
 * The editor's side of its projection (milestone M17; docs/design/paper-projection-narration.md §2.6): the screen
 * list and the choice, whether the projection is open (told by Rust, also when it is closed from its own window), the
 * projected area, and the feed of the page to the projection window (`lib/projection-feed.ts`).
 *
 * The projected area (§11 O5) is centred on the centre of what the editor's canvas shows (`visible`), kept on the page
 * as live's is (`clampView`), at the projection's own zoom (1 = the whole page, « Page entière »; « Largeur » the zoom
 * that fits the page's width, at least 1; − and + by `ZOOM_STEP` between 1 and 8), in the projector's shape. Moving or
 * zooming the editor's view moves the area, never changes its zoom. The view goes to Rust at most once per frame.
 *
 * Package E wires it into `Editor.svelte`: `watch()` at mount, `show()` from an effect while `presentation` is set
 * (the replay's note during a replay), `stroke()` from `Canvas`'s `oninprogress`, `visible()` from its `onvisible`,
 * `frame` to its `projected` (the dashed rectangle), `dispose()` in the destroy.
 */
import { api, errorMessage } from '../api';
import type { LiveViewState, MonitorInfo } from '../live';
import { ZOOM_STEP, aspectOf, clampView, clampZoom, defaultMonitor, fitWidthView, projectedArea } from '../live-view';
import type { Note, PageSize } from '../note';
import type { ProjectionStatus, ProjectionView } from '../projection';
import { ProjectionFeed, type InProgress } from '../projection-feed';
import type { Rect } from './geometry';

/** The commands the projection uses (the tests pass their own). */
export type ProjectionCommands = Pick<
  typeof api,
  'listMonitors' | 'projectionOpen' | 'projectionClose' | 'projectionWatch' | 'projectionUpdate'
>;

/** How the view waits for the next frame (the tests pass their own). */
export type FrameScheduler = (run: () => void) => void;

const nextFrame: FrameScheduler = (run) => {
  if (typeof requestAnimationFrame === 'function') requestAnimationFrame(() => run());
  else setTimeout(run, 16);
};

/** A zoom as live sends it (three decimals: the value comes back identical through Rust and JSON). */
function roundZoom(zoom: number): number {
  return Math.round(clampZoom(zoom) * 1000) / 1000;
}

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

  /** What the editor's canvas shows of the page (page units), and the page shown. */
  #visibleRect = $state.raw<Rect | null>(null);
  #page = $state.raw<PageSize | null>(null);
  #latest: { note: Note; imageKey: string | null } | null = null;
  readonly #commands: ProjectionCommands;
  readonly #feed: ProjectionFeed;
  readonly #frame: FrameScheduler;
  #viewQueued = false;
  #watching = false;
  /** Bumped by every status heard and every open or close: an older answer of `projection_watch` is then stale. */
  #statusEpoch = 0;
  #disposed = false;

  /** The projected area on the page (page units) while projecting with a zoom above 1, for the editor's frame. */
  readonly #projected = $derived.by((): Rect | null => {
    const page = this.#page;
    if (!this.presentation || !page || this.zoom <= 1 + 1e-3) return null;
    return projectedArea(page, this.#liveView(page), this.#aspect());
  });

  get frame(): Rect | null {
    return this.#projected;
  }

  constructor(commands: ProjectionCommands = api, options: { frame?: FrameScheduler } = {}) {
    this.#commands = commands;
    this.#frame = options.frame ?? nextFrame;
    this.#feed = new ProjectionFeed((update) => this.#commands.projectionUpdate(update));
  }

  /** The feed (tests and the journeys read its refusals). */
  get feed(): ProjectionFeed {
    return this.#feed;
  }

  /** Starts hearing where the projection is open (once, at mount). */
  watch(): void {
    if (this.#watching || this.#disposed) return;
    this.#watching = true;
    const epoch = this.#statusEpoch;
    this.#commands
      .projectionWatch((status) => {
        this.#statusEpoch += 1;
        this.#status(status);
      })
      .then((status) => {
        // A status heard meanwhile (or an open or close of ours) is newer than this answer.
        if (epoch === this.#statusEpoch) this.#status(status);
      })
      .catch(() => {
        // Without the watch the tab still opens and closes the projection itself.
      });
  }

  /** Lists the screens again. */
  async refresh(): Promise<void> {
    try {
      const monitors = await this.#commands.listMonitors();
      if (this.#disposed) return;
      this.monitors = monitors;
      if (!monitors.some((m) => m.key === this.monitorKey)) this.monitorKey = defaultMonitor(monitors)?.key ?? '';
    } catch (error) {
      if (!this.#disposed) this.error = errorMessage(error);
    } finally {
      this.loaded = true;
    }
  }

  /** Opens the projection on the chosen screen. */
  async open(): Promise<void> {
    if (this.busy || this.#disposed) return;
    this.busy = true;
    this.error = null;
    try {
      const shown = await this.#commands.projectionOpen(this.monitorKey || null);
      if (this.#disposed) {
        // The editor went while the window was opening: it goes too.
        await this.#commands.projectionClose().catch(() => {});
        return;
      }
      this.#statusEpoch += 1;
      this.monitorKey = shown.key;
      this.#opened(shown);
    } catch (error) {
      if (!this.#disposed) this.error = errorMessage(error);
    } finally {
      this.busy = false;
    }
  }

  /** Closes the projection. */
  async close(): Promise<void> {
    this.error = null;
    try {
      await this.#commands.projectionClose();
      this.#statusEpoch += 1;
      this.#closed();
    } catch (error) {
      if (!this.#disposed) this.error = errorMessage(error);
    }
  }

  /** While open: what the editor shows (a replay included) and the staged pixels of its image. */
  show(note: Note, imageKey: string | null): void {
    if (this.#disposed) return;
    this.#latest = { note, imageKey };
    const page = this.#page;
    const resized = !page || page.width !== note.page.width || page.height !== note.page.height;
    if (resized) this.#page = note.page;
    if (!this.presentation) return;
    // The view of the new page goes with it (the page update carries it).
    if (resized) this.#feed.view(this.#projectionView(note.page));
    this.#feed.show(note, imageKey);
  }

  /** While open: the stroke in progress. */
  stroke(event: InProgress): void {
    if (this.presentation && !this.#disposed) this.#feed.stroke(event);
  }

  /** What the editor's canvas shows of the page (page units): the projected area is centred on it. */
  visible(rect: Rect): void {
    if (![rect.x, rect.y, rect.width, rect.height].every(Number.isFinite)) return;
    const was = this.#visibleRect;
    if (was && was.x === rect.x && was.y === rect.y && was.width === rect.width && was.height === rect.height) return;
    this.#visibleRect = { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
    this.#queueView();
  }

  /** One step in (1) or out (-1) of the projection's own zoom, between 1 and 8. */
  zoomStep(direction: 1 | -1): void {
    this.#setZoom(this.zoom * ZOOM_STEP ** direction);
  }

  /** The projection's zoom 1: the whole page. */
  wholePage(): void {
    this.#setZoom(1);
  }

  /** The projection's zoom that shows the page's full width (at least 1). */
  pageWidth(): void {
    const page = this.#page;
    if (!page) return;
    this.#setZoom(Math.max(1, fitWidthView(this.#liveView(page), page, this.#aspect()).zoom));
  }

  /** The editor goes: closes the projection. */
  dispose(): void {
    if (this.#disposed) return;
    const open = this.presentation !== null;
    this.#disposed = true;
    this.#feed.dispose();
    this.presentation = null;
    // An open under way closes the window once it resolves (`open`).
    if (open) void this.#commands.projectionClose().catch(() => {});
  }

  #setZoom(zoom: number): void {
    const next = roundZoom(zoom);
    if (next === this.zoom) return;
    this.zoom = next;
    this.#queueView();
  }

  /** The shape of the projected area: the screen projected on, else the one chosen. */
  #aspect(): number {
    return aspectOf(this.presentation ?? this.monitors.find((m) => m.key === this.monitorKey) ?? null);
  }

  /** The projected area as live's view: centred on what the editor shows, kept on the page. */
  #liveView(page: PageSize): LiveViewState {
    const r = this.#visibleRect;
    const centre = r ? { x: r.x + r.width / 2, y: r.y + r.height / 2 } : { x: page.width / 2, y: page.height / 2 };
    return clampView(page, { zoom: this.zoom, x: centre.x, y: centre.y, board: false, laser: false }, this.#aspect());
  }

  /** What Rust gets: whole page units, the zoom with three decimals (as live's `roundView`). */
  #projectionView(page: PageSize): ProjectionView {
    const view = this.#liveView(page);
    return { zoom: roundZoom(view.zoom), x: Math.round(view.x), y: Math.round(view.y) };
  }

  /** Sends the view on the next frame (several changes in a frame send one). */
  #queueView(): void {
    if (this.#viewQueued || !this.presentation || this.#disposed) return;
    this.#viewQueued = true;
    this.#frame(() => {
      this.#viewQueued = false;
      const page = this.#page;
      if (page && this.presentation && !this.#disposed) this.#feed.view(this.#projectionView(page));
    });
  }

  #status(status: ProjectionStatus): void {
    if (this.#disposed) return;
    if (status.open) this.#opened(status.open);
    else this.#closed();
  }

  /** The projection is open on `monitor`: a window that was closed starts from the whole page. */
  #opened(monitor: MonitorInfo): void {
    const was = this.presentation;
    this.presentation = monitor;
    if (was) return;
    this.#feed.reset();
    const page = this.#page;
    if (page) this.#feed.view(this.#projectionView(page));
    if (this.#latest) this.#feed.show(this.#latest.note, this.#latest.imageKey);
  }

  #closed(): void {
    if (!this.presentation) return;
    this.presentation = null;
    // Rust dropped the scene and the stroke: the next opening sends everything again.
    this.#feed.reset();
  }
}
