/**
 * Zoom and pan: the map between page coordinates (device units) and screen pixels relative to
 * the canvas' top-left corner. `screen = page × scale + (x, y)`.
 */
import type { PageSize } from '../note';
import type { Rect, Vec } from './geometry';

export interface View {
  /** Screen pixels per device unit. */
  scale: number;
  x: number;
  y: number;
}

export interface Viewport {
  width: number;
  height: number;
}

/** About 0.2 % of the page's real size at 96 dpi … 10× its real size. */
export const MIN_SCALE = 0.002;
export const MAX_SCALE = 0.4;

export function clampScale(scale: number): number {
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale));
}

export function pageToScreen(view: View, p: Vec): Vec {
  return { x: p.x * view.scale + view.x, y: p.y * view.scale + view.y };
}

export function screenToPage(view: View, p: Vec): Vec {
  return { x: (p.x - view.x) / view.scale, y: (p.y - view.y) / view.scale };
}

/** Zooms by `factor`, keeping the page point under `at` (screen) in place. */
export function zoomAt(view: View, at: Vec, factor: number): View {
  const scale = clampScale(view.scale * factor);
  const anchor = screenToPage(view, at);
  return { scale, x: at.x - anchor.x * scale, y: at.y - anchor.y * scale };
}

export function panBy(view: View, dx: number, dy: number): View {
  return { ...view, x: view.x + dx, y: view.y + dy };
}

/** The whole page centred in the viewport, with `margin` pixels around it. */
export function fitView(page: PageSize, viewport: Viewport, margin = 24): View {
  const width = Math.max(1, viewport.width - 2 * margin);
  const height = Math.max(1, viewport.height - 2 * margin);
  const scale = clampScale(Math.min(width / Math.max(1, page.width), height / Math.max(1, page.height)));
  return {
    scale,
    x: (viewport.width - page.width * scale) / 2,
    y: (viewport.height - page.height * scale) / 2,
  };
}

/** The part of the page shown in the viewport, in device units (the SVG `viewBox`). */
export function visibleRect(view: View, viewport: Viewport): Rect {
  return {
    x: -view.x / view.scale,
    y: -view.y / view.scale,
    width: viewport.width / view.scale,
    height: viewport.height / view.scale,
  };
}

/**
 * Pixel width at which to render the page background for this zoom: the next power of two of
 * the page's on-screen width (so it is re-rendered only when the zoom doubles or halves),
 * between 512 and 4096 pixels.
 */
export function backgroundWidth(page: PageSize, scale: number, pixelRatio = 1): number {
  const needed = Math.max(1, page.width * scale * pixelRatio);
  return Math.min(4096, Math.max(512, 2 ** Math.ceil(Math.log2(needed))));
}

/**
 * The page as wide as the viewport (with `margin` pixels on each side), centred; a page taller
 * than the viewport shows its top, a shorter one is centred vertically.
 */
export function fitWidthView(page: PageSize, viewport: Viewport, margin = 24): View {
  const width = Math.max(1, viewport.width - 2 * margin);
  const scale = clampScale(width / Math.max(1, page.width));
  const height = page.height * scale;
  return {
    scale,
    x: (viewport.width - page.width * scale) / 2,
    y: height + 2 * margin <= viewport.height ? (viewport.height - height) / 2 : margin,
  };
}
