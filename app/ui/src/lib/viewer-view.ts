/**
 * The viewer's own view of the page (docs/design/lan-share.md, "The viewer"): what the other
 * computer shows is chosen by the Ardoise PC, but each screen may adjust how it displays it
 * (whole page, full width, zoom, move) without effect anywhere else. Pure functions on the
 * geometry of `live-view.ts`; `viewer-view.svelte.ts` holds the state.
 *
 * The own view is a fit mode plus, for a free zoom, a zoom and a centre (the vocabulary of
 * `LiveViewState`: zoom 1 is the whole page, up to 8). `page` and `width` are not stored as
 * numbers but worked out from the current size of the window, so that a resize or full screen
 * keeps them. Blackboard and laser dot are not part of the view: they stay the PC's choice.
 */
import type { Vec } from './editor/geometry';
import type { LiveViewState } from './live';
import {
  aspectOf,
  clampView,
  fitWidthView,
  moveViewTo,
  panView,
  pinchPresentationView,
  presentationTransform,
  sameView,
  screenToPage,
  wholePage,
  zoomView,
  type Size,
} from './live-view';
import type { PageSize } from './note';

/** `page`: the whole page; `width`: its full width (top shown); `free`: a zoom and a place chosen here. */
export type OwnFit = 'page' | 'width' | 'free';

export interface OwnView {
  fit: OwnFit;
  /** The page it was chosen for (`pageKey`): a free zoom does not follow to another page. */
  key: string;
  /** For `free`: the view that was chosen. */
  view: LiveViewState;
}

/** What the view is worked out from. */
export interface ViewInput {
  /** The view the PC chose (its blackboard and laser always apply). */
  pc: LiveViewState;
  page: PageSize;
  /** The window, CSS pixels. */
  viewport: Size;
  /** Identifies the page and the content shown: `pageKey`. */
  key: string;
}

/** Below this zoom a free view is the whole page again (and follows the window's size). */
const WHOLE = 1.001;

/** Which page of which content: a free zoom is dropped when it changes. */
export function pageKey(shown: 'live' | 'note', index: number, noteId: string, page: PageSize): string {
  return `${shown}:${shown === 'live' ? index : noteId}:${page.width}x${page.height}`;
}

/** The fit mode in force, or null when the viewer follows the PC. */
export function fitOf(own: OwnView | null, key: string): OwnFit | null {
  if (!own) return null;
  return own.fit === 'free' && own.key !== key ? 'page' : own.fit;
}

/** The view to draw: the PC's when following, else the own view on the PC's blackboard and laser. */
export function resolveView(input: ViewInput, own: OwnView | null): LiveViewState {
  const fit = fitOf(own, input.key);
  if (!own || !fit) return input.pc;
  const aspect = aspectOf(input.viewport);
  switch (fit) {
    case 'page':
      return wholePage(input.pc, input.page);
    case 'width':
      return fitWidthView(input.pc, input.page, aspect);
    case 'free':
      return clampView(input.page, { ...input.pc, zoom: own.view.zoom, x: own.view.x, y: own.view.y }, aspect);
  }
}

export function ownPage(input: ViewInput): OwnView {
  return { fit: 'page', key: input.key, view: wholePage(input.pc, input.page) };
}

export function ownWidth(input: ViewInput): OwnView {
  return { fit: 'width', key: input.key, view: fitWidthView(input.pc, input.page, aspectOf(input.viewport)) };
}

/**
 * The own view for `next` (a view worked out from `current`), or null when it changes nothing:
 * a drag on a page that fits, or a zoom out at the limit, does not detach the viewer from the PC.
 */
export function ownFrom(input: ViewInput, current: LiveViewState, next: LiveViewState): OwnView | null {
  // The PC's centre may lie off the page at zoom 1: compare what is drawn.
  if (sameView(clampView(input.page, current, aspectOf(input.viewport)), next)) return null;
  const whole = next.zoom <= WHOLE;
  return { fit: whole ? 'page' : 'free', key: input.key, view: whole ? wholePage(next, input.page) : next };
}

/** `current` zoomed by `factor`; with `at` (a point of the window) that point of the page stays under it. */
export function zoomedView(input: ViewInput, current: LiveViewState, factor: number, at?: Vec): LiveViewState {
  const aspect = aspectOf(input.viewport);
  const anchor = at ? screenToPage(presentationTransform(input.page, input.viewport, current), at) : undefined;
  return zoomView(current, factor, input.page, aspect, anchor);
}

/** `current` moved so that the page follows a drag of `dx`, `dy` window pixels. */
export function draggedView(input: ViewInput, current: LiveViewState, dx: number, dy: number): LiveViewState {
  const aspect = aspectOf(input.viewport);
  const from = clampView(input.page, current, aspect);
  const { scale } = presentationTransform(input.page, input.viewport, from);
  return moveViewTo(from, { x: from.x - dx / scale, y: from.y - dy / scale }, input.page, aspect);
}

/** `current` moved by `dx`, `dy` times the area shown (Shift + arrows). */
export function steppedView(input: ViewInput, current: LiveViewState, dx: number, dy: number): LiveViewState {
  return panView(current, dx, dy, input.page, aspectOf(input.viewport));
}

/** `current` after a two-finger pinch: the page point under `from` goes to `to`, scaled by `factor`. */
export function pinchedView(input: ViewInput, current: LiveViewState, from: Vec, to: Vec, factor: number): LiveViewState {
  return pinchPresentationView(current, input.page, input.viewport, from, to, factor);
}
