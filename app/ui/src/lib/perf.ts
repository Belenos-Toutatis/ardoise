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
