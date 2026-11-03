import { describe, expect, it } from 'vitest';
import type { LiveBatch, LiveEvent, LiveSnapshot, LiveViewState } from './live';
import { DEFAULT_VIEW } from './live-state.svelte';
import { presentationTransform, projectedArea, sameView, screenToPage } from './live-view';
import type { Layer, Note } from './note';
import type { SharedPage } from './share';
import { ViewerState, noteSnapshot } from './viewer-state.svelte';
import {
  draggedView,
  fitOf,
  ownFrom,
  ownPage,
  ownWidth,
  pageKey,
  resolveView,
  zoomedView,
  type OwnView,
  type ViewInput,
} from './viewer-view';

const A4 = { width: 29700, height: 21600 };
const PORTRAIT = { width: 21600, height: 29700 };
const LAYERS: Layer[] = [{ name: 'Calque 1', visible: true }];
const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const WIDE = { width: 1600, height: 900 };
const SQUARE = { width: 800, height: 800 };

const pcView = (extra: Partial<LiveViewState> = {}): LiveViewState => ({
  zoom: 1,
  x: A4.width / 2,
  y: A4.height / 2,
  board: false,
  laser: false,
  ...extra,
});

function input(extra: Partial<ViewInput> = {}): ViewInput {
  return { pc: pcView(), page: A4, viewport: WIDE, key: pageKey('live', 0, 'p1', A4), ...extra };
}

/** The area shown stays on the page. */
function expectOnPage(view: LiveViewState, aspect: number) {
  const area = projectedArea(A4, view, aspect);
  expect(area.x).toBeGreaterThanOrEqual(-1e-6);
  expect(area.y).toBeGreaterThanOrEqual(-1e-6);
  expect(area.x + area.width).toBeLessThanOrEqual(A4.width + 1e-6);
  expect(area.y + area.height).toBeLessThanOrEqual(A4.height + 1e-6);
}

describe('viewer view: fit modes', () => {
  it('follows the PC while there is no own view', () => {
    const pc = pcView({ zoom: 3, x: 5000, y: 4000, board: true });
    expect(resolveView(input({ pc }), null)).toBe(pc);
    expect(fitOf(null, 'k')).toBeNull();
  });

  it("shows the whole page at zoom 1, on the PC's blackboard and laser", () => {
    const pc = pcView({ zoom: 4, x: 3000, y: 3000, board: true, laser: true });
    const view = resolveView(input({ pc }), ownPage(input({ pc })));
    expect(view).toMatchObject({ zoom: 1, x: A4.width / 2, y: A4.height / 2, board: true, laser: true });
  });

  it('fits the width: the page across the window, its top shown', () => {
    // A4 landscape on 16:9: the whole page fits by its height, the width fit zooms in.
    const view = resolveView(input(), ownWidth(input()));
    expect(view.zoom).toBeGreaterThan(1);
    const transform = presentationTransform(A4, WIDE, view);
    expect(transform.scale * A4.width).toBeCloseTo(WIDE.width, 3);
    expect(transform.y).toBeCloseTo(0, 6);
    expect(transform.x).toBeCloseTo(0, 6);
  });

  it('is the whole page when the window is as narrow as the page (the width fit never shows less)', () => {
    const narrow = { width: 1000, height: 1000 };
    const view = resolveView(input({ viewport: narrow }), ownWidth(input({ viewport: narrow })));
    expect(view.zoom).toBeCloseTo(1, 6);
  });

  it('keeps a fit mode when the window changes size (resize, full screen)', () => {
    const own = ownWidth(input());
    const wide = resolveView(input(), own);
    const square = resolveView(input({ viewport: SQUARE }), own);
    expect(presentationTransform(A4, SQUARE, square).scale * A4.width).toBeCloseTo(SQUARE.width, 3);
    expect(square.zoom).not.toBeCloseTo(wide.zoom, 3);
    expect(resolveView(input({ viewport: SQUARE }), ownPage(input())).zoom).toBe(1);
  });

  it('keeps a fit mode on another page and drops a free zoom to the whole page', () => {
    const other = input({ page: PORTRAIT, key: pageKey('live', 1, 'p2', PORTRAIT) });
    expect(fitOf(ownWidth(input()), other.key)).toBe('width');
    const free: OwnView = { fit: 'free', key: input().key, view: pcView({ zoom: 3, x: 5000, y: 5000 }) };
    expect(fitOf(free, input().key)).toBe('free');
    expect(fitOf(free, other.key)).toBe('page');
    expect(resolveView(other, free).zoom).toBe(1);
  });

  it('keys a note by its id and a live page by its index and size', () => {
    expect(pageKey('note', 0, 'n1', A4)).not.toBe(pageKey('note', 0, 'n2', A4));
    expect(pageKey('live', 0, 'a', A4)).toBe(pageKey('live', 0, 'b', A4));
    expect(pageKey('live', 0, 'a', A4)).not.toBe(pageKey('live', 1, 'a', A4));
    expect(pageKey('live', 0, 'a', A4)).not.toBe(pageKey('live', 0, 'a', PORTRAIT));
    expect(pageKey('live', 0, 'a', A4)).not.toBe(pageKey('note', 0, 'a', A4));
  });
});

describe('viewer view: zoom, move, clamping', () => {
  it('zooms around a point of the window: the page point under it stays under it', () => {
    const start = resolveView(input(), ownPage(input()));
    const at = { x: 1000, y: 300 };
    const before = screenToPage(presentationTransform(A4, WIDE, start), at);
    const next = zoomedView(input(), start, 2.5, at);
    expect(next.zoom).toBeCloseTo(2.5, 6);
    const after = screenToPage(presentationTransform(A4, WIDE, next), at);
    expect(after.x).toBeCloseTo(before.x, 3);
    expect(after.y).toBeCloseTo(before.y, 3);
  });

  it('stops at zoom 8 and at the whole page', () => {
    const start = resolveView(input(), ownPage(input()));
    expect(zoomedView(input(), start, 100).zoom).toBe(8);
    expect(zoomedView(input(), start, 0.01).zoom).toBe(1);
  });

  it('keeps the area on the page when zooming near a corner', () => {
    const start = resolveView(input(), ownPage(input()));
    expectOnPage(zoomedView(input(), start, 6, { x: 0, y: 0 }), WIDE.width / WIDE.height);
  });

  it('moves the page with the drag, and cannot lose it off screen', () => {
    const zoomed = zoomedView(input(), resolveView(input(), ownPage(input())), 4);
    const moved = draggedView(input(), zoomed, -100, -50);
    const scale = presentationTransform(A4, WIDE, zoomed).scale;
    expect(moved.x - zoomed.x).toBeCloseTo(100 / scale, 3);
    expect(moved.y - zoomed.y).toBeCloseTo(50 / scale, 3);
    // Far past every edge: the area stops at the page's edge.
    expectOnPage(draggedView(input(), zoomed, 1e6, 1e6), WIDE.width / WIDE.height);
    expectOnPage(draggedView(input(), zoomed, -1e6, -1e6), WIDE.width / WIDE.height);
  });

  it('takes no own view for a move that changes nothing (a drag on a page that fits)', () => {
    const whole = resolveView(input(), ownPage(input()));
    expect(ownFrom(input(), whole, draggedView(input(), whole, 80, 40))).toBeNull();
    // The PC's centre may lie off the page at zoom 1 (before its first move): still nothing.
    const raw = pcView({ x: 0, y: 0 });
    expect(ownFrom(input({ pc: raw }), raw, draggedView(input({ pc: raw }), raw, 10, 10))).toBeNull();
    expect(ownFrom(input(), whole, zoomedView(input(), whole, 0.5))).toBeNull();
  });

  it('takes a free own view for a zoom, and the whole-page fit again when zoomed back out', () => {
    const whole = resolveView(input(), ownPage(input()));
    const zoomed = zoomedView(input(), whole, 3);
    const own = ownFrom(input(), whole, zoomed);
    expect(own).toMatchObject({ fit: 'free', key: input().key });
    expect(sameView(resolveView(input(), own), zoomed)).toBe(true);
    expect(ownFrom(input(), zoomed, zoomedView(input(), zoomed, 1 / 3))?.fit).toBe('page');
  });

  it('keeps a free view on the page when the window changes shape', () => {
    const own = ownFrom(input(), pcView(), zoomedView(input(), pcView(), 6, { x: 1500, y: 100 }))!;
    expectOnPage(resolveView(input({ viewport: SQUARE }), own), 1);
  });
});

// --- The state: following the PC, or showing its own view --------------------------------------

function livePage(id: string): Note {
  return {
    id,
    title: '',
    created: 0,
    modified: 0,
    page: A4,
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: LAYERS,
    strokes: [],
    timing: 'recorded',
    origin: { kind: 'live' },
  };
}

function snapshot(pageIndex = 0, view: Partial<LiveViewState> = {}): LiveSnapshot {
  return {
    state: 'live',
    page: livePage(`p${pageIndex + 1}`),
    pageIndex,
    pageCount: 3,
    orientation: 'landscape',
    layer: 0,
    style: { tool: 'ballpoint', color: BLACK, width: 60 },
    view: { ...DEFAULT_VIEW, x: A4.width / 2, y: A4.height / 2, ...view },
    hover: null,
    presentation: null,
    saved: [],
  };
}

const batch = (...events: LiveEvent[]): LiveBatch => ({ events, sentAt: 0 });

function liveViewer(view: Partial<LiveViewState> = {}) {
  const v = new ViewerState();
  v.view.setSize(WIDE);
  v.handle({ type: 'live', reset: true, batch: batch({ type: 'snapshot', snapshot: snapshot(0, view) }) });
  return v;
}

const pcMoves = (v: ViewerState, view: Partial<LiveViewState>) =>
  v.handle({
    type: 'live',
    reset: false,
    batch: batch({ type: 'view', view: { ...DEFAULT_VIEW, x: A4.width / 2, y: A4.height / 2, ...view } }),
  });

const nextPage = (v: ViewerState) =>
  v.handle({ type: 'live', reset: false, batch: batch({ type: 'snapshot', snapshot: snapshot(1) }) });

describe('viewer view: following the PC or showing its own', () => {
  it('follows the view events of the PC by default', () => {
    const v = liveViewer();
    expect(v.view.fit).toBe('follow');
    expect(v.view.canFollow).toBe(false);
    pcMoves(v, { zoom: 3, x: 6000, y: 5000 });
    expect(v.view.current).toMatchObject({ zoom: 3, x: 6000, y: 5000 });
  });

  it('keeps its own view when the PC moves, and comes back with follow()', () => {
    const v = liveViewer({ zoom: 2, x: 8000, y: 6000 });
    v.view.showWidth();
    const own = v.view.current;
    expect(v.view.fit).toBe('width');
    expect(v.view.canFollow).toBe(true);
    pcMoves(v, { zoom: 5, x: 3000, y: 3000 });
    expect(v.view.current).toEqual(own);
    v.view.follow();
    expect(v.view.fit).toBe('follow');
    expect(v.view.canFollow).toBe(false);
    expect(v.view.current).toMatchObject({ zoom: 5, x: 3000, y: 3000 });
  });

  it("still shows the PC's blackboard and laser while on its own view", () => {
    const v = liveViewer();
    v.view.zoomStep(1);
    pcMoves(v, { board: true, laser: true });
    expect(v.view.current).toMatchObject({ board: true, laser: true });
    expect(v.view.current.zoom).toBeCloseTo(1.25, 6);
  });

  it("starts from the view shown: a zoom step from the PC's zoom, not from 1", () => {
    const v = liveViewer({ zoom: 2, x: 8000, y: 6000 });
    v.view.zoomStep(1);
    expect(v.view.fit).toBe('free');
    expect(v.view.current.zoom).toBeCloseTo(2.5, 6);
  });

  it('stays with the PC for a move that changes nothing (a drag on the whole page)', () => {
    const v = liveViewer();
    v.view.drag(50, 50);
    v.view.zoomStep(-1);
    v.view.step(1, 0);
    expect(v.view.fit).toBe('follow');
    expect(v.view.canFollow).toBe(false);
  });

  it('moves with the arrows, a drag and a pinch once zoomed', () => {
    const v = liveViewer();
    v.view.zoomStep(1);
    v.view.zoomStep(1);
    const x = v.view.current.x;
    v.view.step(1, 0);
    expect(v.view.current.x).toBeGreaterThan(x);
    const y = v.view.current.y;
    v.view.drag(0, -40);
    expect(v.view.current.y).toBeGreaterThan(y);
    const zoom = v.view.current.zoom;
    v.view.pinch({ x: 400, y: 300 }, { x: 400, y: 300 }, 1.5);
    expect(v.view.current.zoom).toBeCloseTo(zoom * 1.5, 6);
    v.view.zoomAt(1 / 1.5, { x: 400, y: 300 });
    expect(v.view.current.zoom).toBeCloseTo(zoom, 6);
  });

  it('keeps a fit mode on the next page of the PC', () => {
    const v = liveViewer();
    v.view.showWidth();
    nextPage(v);
    expect(v.live.pageIndex).toBe(1);
    expect(v.view.fit).toBe('width');
  });

  it('drops a free zoom to the whole page on the next page of the PC, still apart from the PC', () => {
    const v = liveViewer();
    v.view.zoomStep(1);
    v.view.zoomStep(1);
    expect(v.view.fit).toBe('free');
    nextPage(v);
    expect(v.view.fit).toBe('page');
    expect(v.view.current.zoom).toBe(1);
    expect(v.view.canFollow).toBe(true);
  });

  it('keeps its view across a reconnection (a new reset of the same page) and a pause', () => {
    const v = liveViewer();
    v.view.zoomStep(1);
    v.view.zoomStep(1);
    const own = v.view.current;
    v.status('reconnecting');
    v.status('open');
    v.handle({ type: 'live', reset: true, batch: batch({ type: 'snapshot', snapshot: snapshot(0, { zoom: 6 }) }) });
    expect(v.view.current).toEqual(own);
    v.handle({ type: 'pause', paused: true });
    expect(v.view.current).toEqual(own);
  });

  it('goes back to following when the sheet goes away (waiting, ended, code)', () => {
    const leaves = [
      (v: ViewerState) => v.handle({ type: 'waiting' }),
      (v: ViewerState) => v.handle({ type: 'end' }),
      (v: ViewerState) => v.status('needsCode'),
    ];
    for (const leave of leaves) {
      const v = liveViewer();
      v.view.showPage();
      expect(v.view.canFollow).toBe(true);
      leave(v);
      expect(v.view.canFollow).toBe(false);
      expect(v.view.own).toBeNull();
    }
  });

  it('does nothing without a sheet', () => {
    const v = new ViewerState();
    v.view.zoomStep(1);
    v.view.drag(10, 10);
    v.view.step(1, 0);
    expect(v.view.own).toBeNull();
  });
});

describe('viewer view: a note', () => {
  const shared = (id: string, page = A4): SharedPage => ({
    id,
    page,
    background: { paper: 'white', template: 'blank', texture: false },
    layers: LAYERS,
    strokes: [],
  });
  const noteViewer = () => {
    const v = new ViewerState();
    v.view.setSize(WIDE);
    v.handle({ type: 'note', page: shared('n1'), background: null });
    return v;
  };

  it('shows the whole page by default, with nothing to follow', () => {
    const v = noteViewer();
    expect(v.view.fit).toBe('page');
    expect(v.view.canFollow).toBe(false);
    expect(v.view.current.zoom).toBe(1);
    expect(noteSnapshot(shared('n1')).view.zoom).toBe(1);
  });

  it('fits the width, keeps it through updates and for another note, and follow() returns to the whole page', () => {
    const v = noteViewer();
    v.view.showWidth();
    expect(v.view.fit).toBe('width');
    expect(v.view.canFollow).toBe(false);
    v.handle({ type: 'note', page: shared('n1'), background: null });
    expect(v.view.fit).toBe('width');
    v.handle({ type: 'note', page: shared('n2', PORTRAIT), background: null });
    expect(v.view.fit).toBe('width');
    v.view.follow();
    expect(v.view.fit).toBe('page');
    expect(v.view.current.zoom).toBe(1);
  });

  it('drops a free zoom when another note is shown, not when the same one is updated', () => {
    const v = noteViewer();
    v.view.zoomStep(1);
    v.view.zoomStep(1);
    const zoom = v.view.current.zoom;
    v.handle({ type: 'noteStrokes', strokes: [] });
    v.handle({ type: 'note', page: shared('n1'), background: null });
    expect(v.view.current.zoom).toBe(zoom);
    v.handle({ type: 'note', page: shared('n2'), background: null });
    expect(v.view.current.zoom).toBe(1);
  });

  it('shows the whole page when the note takes the place of a zoomed live sheet', () => {
    const v = liveViewer();
    v.view.zoomStep(1);
    v.view.zoomStep(1);
    v.handle({ type: 'note', page: shared('n1'), background: null });
    expect(v.view.current.zoom).toBe(1);
    expect(v.view.canFollow).toBe(false);
  });
});
