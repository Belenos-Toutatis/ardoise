/**
 * Geometry of the classroom view (docs/design/tablet-live-classroom.md, "View"): the part of the
 * page the presentation window shows, from `LiveViewState` (zoom 1..8, centre in page units)
 * and the shape of the projector's screen. The control window draws the same area as a
 * rectangle on its preview, so both windows use these functions.
 *
 * Zoom 1 shows the whole page fitted in the screen; zoom z shows 1/z of that, centred on
 * (`x`, `y`). The centre is kept so that the area stays on the page when it is smaller than the
 * page (and on the page's centre when it is larger).
 */
import type { Rect, Vec } from './editor/geometry';
import type { LiveViewState, MonitorInfo } from './live';
import type { PageSize } from './note';

export const MIN_ZOOM = 1;
export const MAX_ZOOM = 8;
/** One step of the zoom buttons and of + / −. */
export const ZOOM_STEP = 1.25;
/** One step of Shift + arrow: this share of the projected area. */
export const PAN_STEP = 0.15;
/** Shape of the projected area when no screen is known (16:9 projectors are the most common). */
export const DEFAULT_ASPECT = 16 / 9;

/** `screen = page × scale + (x, y)`, in CSS pixels. */
export interface Transform {
  scale: number;
  x: number;
  y: number;
}

export interface Size {
  width: number;
  height: number;
}

export function clampZoom(zoom: number): number {
  if (!Number.isFinite(zoom)) return MIN_ZOOM;
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

/** Width / height of a screen; `DEFAULT_ASPECT` when unknown or degenerate. */
export function aspectOf(size: Size | null | undefined): number {
  if (!size || !(size.width > 0) || !(size.height > 0)) return DEFAULT_ASPECT;
  return size.width / size.height;
}

/** Scale that fits the whole page in `viewport` with `margin` pixels around it. */
export function fitScale(page: PageSize, viewport: Size, margin = 0): number {
  const width = Math.max(1, viewport.width - 2 * margin);
  const height = Math.max(1, viewport.height - 2 * margin);
  return Math.min(width / Math.max(1, page.width), height / Math.max(1, page.height));
}

/** The whole page centred in `viewport` (the control window's preview). */
export function fitTransform(page: PageSize, viewport: Size, margin = 0): Transform {
  const scale = fitScale(page, viewport, margin);
  return {
    scale,
    x: (viewport.width - page.width * scale) / 2,
    y: (viewport.height - page.height * scale) / 2,
  };
}

function clampCentre(centre: number, span: number, extent: number): number {
  if (!Number.isFinite(centre) || span >= extent) return extent / 2;
  return Math.min(extent - span / 2, Math.max(span / 2, centre));
}

/**
 * The page area (page units) a screen of shape `aspect` shows for `view`. At zoom 1 it is the
 * whole page plus the bands that fill the screen around it.
 */
export function projectedArea(page: PageSize, view: LiveViewState, aspect: number): Rect {
  const a = aspect > 0 && Number.isFinite(aspect) ? aspect : DEFAULT_ASPECT;
  // A screen `a` wide and 1 high: the fitted scale, then the zoom.
  const scale = fitScale(page, { width: a, height: 1 }) * clampZoom(view.zoom);
  const width = a / scale;
  const height = 1 / scale;
  const x = clampCentre(view.x, width, page.width);
  const y = clampCentre(view.y, height, page.height);
  return { x: x - width / 2, y: y - height / 2, width, height };
}

/** `view` with its zoom in 1..8 and its centre where the projected area stays on the page. */
export function clampView(page: PageSize, view: LiveViewState, aspect: number): LiveViewState {
  const zoom = clampZoom(view.zoom);
  const area = projectedArea(page, { ...view, zoom }, aspect);
  return { ...view, zoom, x: area.x + area.width / 2, y: area.y + area.height / 2 };
}

/** The presentation window's transform: the projected area fills `viewport`. */
export function presentationTransform(page: PageSize, viewport: Size, view: LiveViewState): Transform {
  const area = projectedArea(page, view, aspectOf(viewport));
  const scale = viewport.width / area.width;
  return { scale, x: -area.x * scale, y: -area.y * scale };
}

/**
 * Zooms by `factor`. With `at` (a page point, e.g. under the mouse) that point keeps its place
 * in the projected area; otherwise the centre stays.
 */
export function zoomView(view: LiveViewState, factor: number, page: PageSize, aspect: number, at?: Vec): LiveViewState {
  const zoom = clampZoom(view.zoom * factor);
  // From zoom 1 the area is centred on the page: start from where it really is.
  const from = clampView(page, view, aspect);
  const k = from.zoom / zoom;
  const centre = at ? { x: at.x + (from.x - at.x) * k, y: at.y + (from.y - at.y) * k } : { x: from.x, y: from.y };
  return clampView(page, { ...view, zoom, ...centre }, aspect);
}

/** Moves the area by `dx`, `dy` times its own size (Shift + arrows). */
export function panView(view: LiveViewState, dx: number, dy: number, page: PageSize, aspect: number): LiveViewState {
  const area = projectedArea(page, view, aspect);
  const from = clampView(page, view, aspect);
  return clampView(page, { ...view, x: from.x + dx * area.width, y: from.y + dy * area.height }, aspect);
}

/** Centres the area on `centre` (page units), as far as the page allows. */
export function moveViewTo(view: LiveViewState, centre: Vec, page: PageSize, aspect: number): LiveViewState {
  return clampView(page, { ...view, x: centre.x, y: centre.y }, aspect);
}

/** Zoom 1 on the page's centre. */
export function wholePage(view: LiveViewState, page: PageSize): LiveViewState {
  return { ...view, zoom: 1, x: page.width / 2, y: page.height / 2 };
}

/**
 * What is sent to Rust: whole page units and a zoom with three decimals, so that the value
 * comes back identical through Rust's numbers and JSON (the echo is then recognised).
 */
export function roundView(view: LiveViewState): LiveViewState {
  return { ...view, zoom: Math.round(clampZoom(view.zoom) * 1000) / 1000, x: Math.round(view.x), y: Math.round(view.y) };
}

export function sameView(a: LiveViewState, b: LiveViewState): boolean {
  return (
    a.board === b.board &&
    a.laser === b.laser &&
    Math.abs(a.zoom - b.zoom) < 1e-3 &&
    Math.abs(a.x - b.x) < 0.5 &&
    Math.abs(a.y - b.y) < 0.5
  );
}

/** Intersection of two rectangles, or null when they do not overlap. */
export function intersect(a: Rect, b: Rect): Rect | null {
  const x = Math.max(a.x, b.x);
  const y = Math.max(a.y, b.y);
  const right = Math.min(a.x + a.width, b.x + b.width);
  const bottom = Math.min(a.y + a.height, b.y + b.height);
  return right > x && bottom > y ? { x, y, width: right - x, height: bottom - y } : null;
}

export function pageToScreen(transform: Transform, p: Vec): Vec {
  return { x: p.x * transform.scale + transform.x, y: p.y * transform.scale + transform.y };
}

export function screenToPage(transform: Transform, p: Vec): Vec {
  return { x: (p.x - transform.x) / transform.scale, y: (p.y - transform.y) / transform.scale };
}

/**
 * The screen offered first for the presentation, like `presentation.rs` chooses: the saved
 * choice, else the first screen that is not the primary one, else the primary one.
 */
export function defaultMonitor(monitors: readonly MonitorInfo[]): MonitorInfo | null {
  return monitors.find((m) => m.chosen) ?? monitors.find((m) => !m.primary) ?? monitors[0] ?? null;
}
