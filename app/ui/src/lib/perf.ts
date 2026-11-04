/**
 * Marks the performance harness (`cargo xtask perf`, `app/e2e/perf.mjs`) reads over the DevTools protocol.
 * They change nothing in the behaviour: a User Timing mark and a data attribute.
 */

/** The library grid is on screen with its cards: the end of the start-up measure. */
export const LIBRARY_LISTED_MARK = 'ardoise:library-listed';

const marked = new Set<string>();

/**
 * Sets the mark `name` once, after the page has been drawn with what the caller just displayed
 * (two animation frames later: the first one runs before the next paint, the second after it).
 */
export function markWhenDrawn(name: string): void {
  if (marked.has(name)) return;
  marked.add(name);
  if (typeof requestAnimationFrame !== 'function' || typeof performance?.mark !== 'function') return;
  requestAnimationFrame(() => requestAnimationFrame(() => performance.mark(name)));
}

/** `onload` of a thumbnail: when it was drawn (`performance.now()`), in `data-drawn-at`. */
export function noteDrawn(event: Event): void {
  const element = event.currentTarget as HTMLElement | null;
  if (element?.dataset) element.dataset.drawnAt = String(performance.now());
}

/** Measures of the redraws of a live canvas' cache: a full one, and an edit's repaint of an area. */
export const LIVE_FULL_REDRAW = 'ardoise:live-full-redraw';
export const LIVE_PARTIAL_REDRAW = 'ardoise:live-partial-redraw';

/**
 * A paint of a live canvas that redrew its cache: counted in `data-full-redraws` / `data-partial-redraws`, and its
 * time (the calls, not what the graphics process does with them) as a User Timing measure. An ordinary frame costs nothing.
 */
export function noteLiveRedraw(canvas: HTMLElement, report: { full: boolean; repainted?: number }, startedAt: number): void {
  if (!report.full && report.repainted === undefined) return;
  const counter = report.full ? 'fullRedraws' : 'partialRedraws';
  canvas.dataset[counter] = String(Number(canvas.dataset[counter] ?? 0) + 1);
  if (typeof performance?.measure === 'function') {
    performance.measure(report.full ? LIVE_FULL_REDRAW : LIVE_PARTIAL_REDRAW, { start: startedAt, end: performance.now() });
  }
}

/**
 * For the journeys and the performance harness, which reach them over the DevTools protocol: `canvas.repaintNow()`
 * paints the state as it is, as a frame would, and `canvas.repaintFromScratch()` draws that state from nothing, so that
 * they can compare, in one task of the page, a full redraw with what the edits' repaints left on the canvas.
 */
export function exposeRepaints(canvas: HTMLCanvasElement, repaintNow: () => void, redrawNext: () => void): void {
  Object.assign(canvas, {
    repaintNow,
    repaintFromScratch() {
      redrawNext();
      repaintNow();
    },
  });
}
