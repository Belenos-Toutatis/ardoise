/**
 * The state of the viewer's own view (docs/design/lan-share.md, "The viewer"; maths in
 * `viewer-view.ts`). The viewer follows the view the PC chose until the person at this screen
 * acts on it (a fit button, the wheel, a drag, a pinch, a key): from then on the viewer shows
 * its own view and the PC's `view` events no longer move it, until `follow()`. Purely local: nothing
 * is sent anywhere.
 *
 * - A fit mode (`page`, `width`) survives a page change, a new note, a resize and full screen: it
 *   is worked out again for what is shown. A free zoom belongs to the page it was made on and
 *   gives way to the whole page when the page changes (another page of the PC, another note, a
 *   sheet turned).
 * - The sheet going away (waiting, ended, code asked) is `reset()`: back to following.
 * - In a note (the editor's page) there is no PC view: the default is the whole page, and
 *   `follow()` goes back to it.
 */
import type { Vec } from './editor/geometry';
import type { LiveViewState } from './live';
import type { LiveState } from './live-state.svelte';
import { PAN_STEP, ZOOM_STEP, presentationTransform, type Size, type Transform } from './live-view';
import {
  draggedView,
  fitOf,
  ownFrom,
  ownPage,
  ownWidth,
  pageKey,
  pinchedView,
  resolveView,
  steppedView,
  zoomedView,
  type OwnFit,
  type OwnView,
  type ViewInput,
} from './viewer-view';

/** What the view is about: the sheet and which content it holds. */
export interface ViewSource {
  live: LiveState;
  shown: 'live' | 'note' | null;
}

/** `follow`: the PC's view; the rest is the own view's fit mode. */
export type ViewerFit = 'follow' | OwnFit;

/** jsdom (tests) has no layout: a nominal size, the one the canvas starts from. */
const NOMINAL: Size = { width: 1280, height: 720 };

export class ViewerView {
  /** The own view, or null while following the PC. */
  own = $state.raw<OwnView | null>(null);
  /** The window the page is drawn in, set by the canvas. */
  size = $state.raw<Size>(NOMINAL);

  constructor(private readonly source: () => ViewSource) {}

  private get input(): ViewInput {
    const { live, shown } = this.source();
    return {
      pc: live.view,
      page: live.pageSize,
      viewport: this.size,
      key: pageKey(shown ?? 'note', live.pageIndex, live.noteId, live.pageSize),
    };
  }

  /** The view to draw. */
  get current(): LiveViewState {
    return resolveView(this.input, this.own);
  }

  /** How the page is shown: following the PC, or which fit; a note without an own view is the whole page. */
  get fit(): ViewerFit {
    const input = this.input;
    return fitOf(this.own, input.key) ?? (this.source().shown === 'note' ? 'page' : 'follow');
  }

  /** Page units to window pixels for the view drawn. */
  get transform(): Transform {
    const { page, viewport } = this.input;
    return presentationTransform(page, viewport, this.current);
  }

  /** « Follow the PC » makes sense: live mode, with an own view. */
  get canFollow(): boolean {
    return this.source().shown === 'live' && this.own !== null;
  }

  setSize(size: Size): void {
    if (size.width !== this.size.width || size.height !== this.size.height) this.size = size;
  }

  showPage(): void {
    this.own = ownPage(this.input);
  }

  showWidth(): void {
    this.own = ownWidth(this.input);
  }

  /** Back to the PC's view (the whole page for a note). */
  follow(): void {
    this.own = null;
  }

  reset(): void {
    this.own = null;
  }

  /** One step of the buttons and of + / −, around the centre of the window. */
  zoomStep(direction: 1 | -1): void {
    this.apply((input, current) => zoomedView(input, current, ZOOM_STEP ** direction));
  }

  /** The wheel (or a trackpad): zoom by `factor` around `at`, a point of the window. */
  zoomAt(factor: number, at: Vec): void {
    this.apply((input, current) => zoomedView(input, current, factor, at));
  }

  /** A drag or a finger moved by `dx`, `dy` window pixels: the page follows. */
  drag(dx: number, dy: number): void {
    this.apply((input, current) => draggedView(input, current, dx, dy));
  }

  /** Shift + arrows: one step of the area shown. */
  step(dx: -1 | 0 | 1, dy: -1 | 0 | 1): void {
    this.apply((input, current) => steppedView(input, current, dx * PAN_STEP, dy * PAN_STEP));
  }

  /** Two fingers: the point under `from` goes to `to`, scaled by `factor`. */
  pinch(from: Vec, to: Vec, factor: number): void {
    this.apply((input, current) => pinchedView(input, current, from, to, factor));
  }

  /** Takes the own view for what `change` makes of the current one, unless it changes nothing. */
  private apply(change: (input: ViewInput, current: LiveViewState) => LiveViewState): void {
    if (this.source().shown === null) return;
    const input = this.input;
    const current = resolveView(input, this.own);
    const own = ownFrom(input, current, change(input, current));
    if (own) this.own = own;
  }
}
