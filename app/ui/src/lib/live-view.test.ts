import { describe, expect, it } from 'vitest';
import type { LiveViewState, MonitorInfo } from './live';
import {
  DEFAULT_ASPECT,
  aspectOf,
  clampView,
  defaultMonitor,
  fitTransform,
  fitWidthView,
  intersect,
  moveViewTo,
  pageToScreen,
  panView,
  pinchPresentationView,
  pinchPreviewView,
  presentationTransform,
  projectedArea,
  roundView,
  sameView,
  screenToPage,
  wholePage,
  zoomView,
} from './live-view';

const A4 = { width: 29700, height: 21600 };
const view = (extra: Partial<LiveViewState> = {}): LiveViewState => ({
  zoom: 1,
  x: A4.width / 2,
  y: A4.height / 2,
  board: false,
  laser: false,
  ...extra,
});

describe('projected area', () => {
  it('shows the whole page at zoom 1, with bands on a wider screen', () => {
    const area = projectedArea(A4, view(), 16 / 9);
    // A4 landscape (1.375) on 16:9: the height fits, bands left and right.
    expect(area.height).toBeCloseTo(A4.height, 6);
    expect(area.width).toBeCloseTo(A4.height * (16 / 9), 6);
    expect(area.x + area.width / 2).toBeCloseTo(A4.width / 2, 6);
    expect(area.y).toBeCloseTo(0, 6);
    // On a 4:3 screen (narrower than the page) the width fits.
    const square = projectedArea(A4, view(), 4 / 3);
    expect(square.width).toBeCloseTo(A4.width, 6);
    expect(square.height).toBeCloseTo(A4.width * 0.75, 6);
  });

  it('keeps the page centred at zoom 1 whatever the centre says', () => {
    const area = projectedArea(A4, view({ x: 0, y: 99999 }), 16 / 9);
    expect(area.x + area.width / 2).toBeCloseTo(A4.width / 2, 6);
    expect(area.y + area.height / 2).toBeCloseTo(A4.height / 2, 6);
  });

  it('shows 1/zoom of the page around the centre, kept on the page', () => {
    const zoomed = projectedArea(A4, view({ zoom: 4, x: 5000, y: 5000 }), 16 / 9);
    expect(zoomed.height).toBeCloseTo(A4.height / 4, 6);
    expect(zoomed.x + zoomed.width / 2).toBeCloseTo(5000, 6);
    // Too close to a corner: the area stops at the page's edge.
    const corner = projectedArea(A4, view({ zoom: 4, x: 0, y: A4.height }), 16 / 9);
    expect(corner.x).toBeCloseTo(0, 6);
    expect(corner.y + corner.height).toBeCloseTo(A4.height, 6);
    // Zoom is limited to 1..8.
    expect(projectedArea(A4, view({ zoom: 50 }), 16 / 9).height).toBeCloseTo(A4.height / 8, 6);
    expect(projectedArea(A4, view({ zoom: 0.1 }), 16 / 9).height).toBeCloseTo(A4.height, 6);
    expect(projectedArea(A4, view({ zoom: Number.NaN }), Number.NaN).height).toBeCloseTo(A4.height, 6);
  });

  it('maps the area onto the presentation window', () => {
    const viewport = { width: 1920, height: 1080 };
    const v = view({ zoom: 2, x: 10000, y: 8000 });
    const t = presentationTransform(A4, viewport, v);
    const area = projectedArea(A4, v, aspectOf(viewport));
    const topLeft = pageToScreen(t, { x: area.x, y: area.y });
    const bottomRight = pageToScreen(t, { x: area.x + area.width, y: area.y + area.height });
    expect(topLeft.x).toBeCloseTo(0, 6);
    expect(topLeft.y).toBeCloseTo(0, 6);
    expect(bottomRight.x).toBeCloseTo(1920, 6);
    expect(bottomRight.y).toBeCloseTo(1080, 6);
    // The centre of the view is the centre of the screen.
    const centre = pageToScreen(t, { x: 10000, y: 8000 });
    expect(centre.x).toBeCloseTo(960, 6);
    expect(centre.y).toBeCloseTo(540, 6);
    expect(screenToPage(t, centre).x).toBeCloseTo(10000, 6);
  });

  it('fits the whole page in the preview with a margin', () => {
    const t = fitTransform(A4, { width: 800, height: 600 }, 12);
    expect(t.scale).toBeCloseTo(776 / A4.width, 9);
    const corner = pageToScreen(t, { x: A4.width, y: A4.height });
    expect(corner.x).toBeCloseTo(788, 6);
    expect(t.y).toBeCloseTo((600 - A4.height * t.scale) / 2, 6);
  });
});

describe('view changes', () => {
  it('zooms around the centre, or keeps a point in place', () => {
    const z = zoomView(view({ zoom: 2, x: 10000, y: 8000 }), 2, A4, 16 / 9);
    expect(z.zoom).toBe(4);
    expect(z.x).toBeCloseTo(10000, 6);
    // Around the pointer: the point under it stays at the same place in the area.
    const at = { x: 12000, y: 9000 };
    const before = view({ zoom: 2, x: 10000, y: 8000 });
    const after = zoomView(before, 2, A4, 16 / 9, at);
    const rel = (v: LiveViewState) => {
      const a = projectedArea(A4, v, 16 / 9);
      return { u: (at.x - a.x) / a.width, v: (at.y - a.y) / a.height };
    };
    expect(rel(after).u).toBeCloseTo(rel(before).u, 6);
    expect(rel(after).v).toBeCloseTo(rel(before).v, 6);
    // From zoom 1 the area really sits on the page's centre, not on a stale centre.
    expect(zoomView(view({ x: 0, y: 0 }), 2, A4, 16 / 9).x).toBeCloseTo(A4.width / 2, 6);
    expect(zoomView(view({ zoom: 8 }), 2, A4, 16 / 9).zoom).toBe(8);
    expect(zoomView(view(), 0.5, A4, 16 / 9).zoom).toBe(1);
  });

  it('pans by a share of the area and stops at the edges', () => {
    const start = view({ zoom: 4, x: 10000, y: 8000 });
    const area = projectedArea(A4, start, 16 / 9);
    const moved = panView(start, 0.5, 0, A4, 16 / 9);
    expect(moved.x).toBeCloseTo(10000 + area.width / 2, 6);
    expect(moved.y).toBeCloseTo(8000, 6);
    let far = start;
    for (let i = 0; i < 20; i++) far = panView(far, 1, 1, A4, 16 / 9);
    const edge = projectedArea(A4, far, 16 / 9);
    expect(edge.x + edge.width).toBeCloseTo(A4.width, 6);
    expect(edge.y + edge.height).toBeCloseTo(A4.height, 6);
    expect(moveViewTo(start, { x: -5, y: -5 }, A4, 16 / 9).x).toBeCloseTo(area.width / 2, 6);
    expect(clampView(A4, view({ zoom: 9 }), 16 / 9).zoom).toBe(8);
    expect(wholePage(start, A4)).toEqual(view());
  });

  it('rounds what goes to Rust and recognises its echo', () => {
    const v = view({ zoom: 1.953125001, x: 1234.56, y: 789.4 });
    const r = roundView(v);
    expect(r).toEqual({ ...v, zoom: 1.953, x: 1235, y: 789 });
    // A float32 round trip through Rust and JSON gives the same numbers back.
    const echo = { ...r, zoom: Math.fround(r.zoom), x: Math.fround(r.x), y: Math.fround(r.y) };
    expect(sameView(r, echo)).toBe(true);
    expect(sameView(r, { ...r, board: true })).toBe(false);
    expect(sameView(r, { ...r, x: r.x + 1 })).toBe(false);
  });
});

describe('helpers', () => {
  it('gives the shape of a screen, 16:9 when unknown', () => {
    expect(aspectOf({ width: 1280, height: 800 })).toBe(1.6);
    expect(aspectOf(null)).toBe(DEFAULT_ASPECT);
    expect(aspectOf({ width: 0, height: 0 })).toBe(DEFAULT_ASPECT);
  });

  it('intersects rectangles', () => {
    expect(intersect({ x: 0, y: 0, width: 10, height: 10 }, { x: 5, y: -5, width: 10, height: 10 })).toEqual({
      x: 5,
      y: 0,
      width: 5,
      height: 5,
    });
    expect(intersect({ x: 0, y: 0, width: 1, height: 1 }, { x: 2, y: 2, width: 1, height: 1 })).toBeNull();
  });

  it('offers the saved screen, else a secondary one, else the primary one', () => {
    const screen = (key: string, primary: boolean, chosen = false): MonitorInfo => ({
      key,
      name: key,
      x: 0,
      y: 0,
      width: 1920,
      height: 1080,
      scale: 1,
      primary,
      chosen,
    });
    expect(defaultMonitor([])).toBeNull();
    expect(defaultMonitor([screen('a', true)])?.key).toBe('a');
    expect(defaultMonitor([screen('a', true), screen('b', false)])?.key).toBe('b');
    expect(defaultMonitor([screen('a', true, true), screen('b', false)])?.key).toBe('a');
  });
});

describe('page width and pinch (M8)', () => {
  const PORTRAIT = { width: 21600, height: 29700 };

  it('fits the width of a portrait page on a 16:9 screen, showing its top', () => {
    const next = fitWidthView(view({ x: 0, y: 0 }), PORTRAIT, 16 / 9);
    // The area is as wide as the page: zoom = aspect × height / width.
    expect(next.zoom).toBeCloseTo(((16 / 9) * PORTRAIT.height) / PORTRAIT.width, 5);
    const area = projectedArea(PORTRAIT, next, 16 / 9);
    expect(area.width).toBeCloseTo(PORTRAIT.width, 0);
    expect(area.x).toBeCloseTo(0, 0);
    expect(area.y).toBeCloseTo(0, 0);
  });

  it('is the whole page when the page is already as wide as the screen allows', () => {
    // A very wide page: the width is the limit at zoom 1.
    const wide = { width: 40000, height: 10000 };
    expect(fitWidthView(view(), wide, 16 / 9).zoom).toBe(1);
  });

  it('stops at the largest zoom', () => {
    const tall = { width: 2000, height: 80000 };
    expect(fitWidthView(view(), tall, 16 / 9).zoom).toBe(8);
  });

  it('pinches the presentation: the page point under the fingers follows them', () => {
    const viewport = { width: 1280, height: 720 };
    const start = view({ zoom: 2, x: 14000, y: 9000 });
    const from = { x: 400, y: 300 };
    const to = { x: 500, y: 350 };
    const next = pinchPresentationView(start, A4, viewport, from, to, 1.5);
    expect(next.zoom).toBeCloseTo(3);
    const before = presentationTransform(A4, viewport, start);
    const after = presentationTransform(A4, viewport, next);
    const page = screenToPage(before, from);
    const moved = pageToScreen(after, page);
    expect(moved.x).toBeCloseTo(to.x, 0);
    expect(moved.y).toBeCloseTo(to.y, 0);
  });

  it('pinches the presentation out to the whole page and no further', () => {
    const viewport = { width: 1280, height: 720 };
    const next = pinchPresentationView(view({ zoom: 1.2, x: 5000, y: 5000 }), A4, viewport, { x: 100, y: 100 }, { x: 100, y: 100 }, 0.1);
    expect(next.zoom).toBe(1);
  });

  it('pinches the preview: the area zooms around the fingers and follows them', () => {
    const start = view({ zoom: 2, x: 14000, y: 9000 });
    const at = { x: 14000, y: 9000 };
    const zoomed = pinchPreviewView(start, A4, 16 / 9, at, at, 2);
    expect(zoomed.zoom).toBeCloseTo(4);
    expect(zoomed.x).toBeCloseTo(14000, 0);
    const moved = pinchPreviewView(start, A4, 16 / 9, at, { x: 15000, y: 9500 }, 1);
    expect(moved.zoom).toBeCloseTo(2);
    expect(moved.x).toBeCloseTo(15000, 0);
    expect(moved.y).toBeCloseTo(9500, 0);
  });
});
