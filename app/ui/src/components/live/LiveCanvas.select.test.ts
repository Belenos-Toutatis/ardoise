// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M12: the control window's preview with the Select tool on: a lasso or a click with the mouse or the
// pen chooses strokes through the real geometry; a finger and the wheel keep navigating; the tablet keeps writing;
// the presentation window is left as it was.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { instantiateInk, type Ink } from '../../lib/ink';
import type { LiveBatch, LiveSnapshot, LiveViewState } from '../../lib/live';
import { FakeContext } from '../../lib/live-fake-canvas';
import { SELECTION_COLORS } from '../../lib/live-render';
import type { Combine } from '../../lib/live-select.svelte';
import { LiveState } from '../../lib/live-state.svelte';
import { fitTransform, pageToScreen } from '../../lib/live-view';
import type { Layer, Note, Stroke } from '../../lib/note';
import LiveCanvas from './LiveCanvas.svelte';

const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const A4 = { width: 29700, height: 21600 };
const LAYERS: Layer[] = [
  { name: 'Calque 1', visible: true },
  { name: 'Calque 2', visible: false },
];

let cleanup: (() => void) | null = null;
let ink: Ink;
let contexts: FakeContext[] = [];

beforeEach(async () => {
  ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  contexts = [];
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
    const ctx = new FakeContext();
    contexts.push(ctx);
    return ctx as unknown as RenderingContext;
  });
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
});

function line(x: number, y: number, layer = 0): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer,
    points: Array.from({ length: 20 }, (_, i) => ({ x: x + 50 * i, y, pressure: 0.6, t: 5 * i })),
  };
}

function snapshot(strokes: Stroke[]): LiveSnapshot {
  const page: Note = {
    id: 'p1',
    title: '',
    created: 0,
    modified: 0,
    page: A4,
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: LAYERS,
    strokes,
    timing: 'recorded',
    origin: { kind: 'live' },
  };
  return {
    state: 'live',
    page,
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    layer: 0,
    style: { tool: 'ballpoint', color: BLACK, width: 60 },
    view: { zoom: 1, x: A4.width / 2, y: A4.height / 2, board: false, laser: false },
    hover: null,
    presentation: null,
    saved: [],
  };
}

interface Init {
  pointerType?: string;
  buttons?: number;
  pointerId?: number;
  t?: number;
  shiftKey?: boolean;
  ctrlKey?: boolean;
}

const TRANSFORM = fitTransform(A4, { width: 800, height: 560 }, 12);
/** Where a page point is on the preview (the jsdom canvas has no offset). */
const at = (x: number, y: number) => pageToScreen(TRANSFORM, { x, y });

async function render(
  options: { mode?: 'preview' | 'presentation'; selecting?: boolean; selection?: readonly number[]; strokes?: Stroke[]; computerLayer?: number } = {},
) {
  const live = new LiveState();
  live.initial({ ...snapshot(options.strokes ?? [line(3000, 4000), line(3000, 8000), line(12000, 4000, 1)]), computerLayer: options.computerLayer ?? 0 });
  const target = document.createElement('div');
  document.body.append(target);
  const onview = vi.fn<(view: LiveViewState) => void>();
  const onselect = vi.fn<(picked: number[], combine: Combine) => void>();
  const onmarks = vi.fn<(count: number) => void>();
  const mode = options.mode ?? 'preview';
  const component = mount(LiveCanvas, {
    target,
    props: {
      live,
      ink,
      mode,
      label: 'page',
      onview,
      onselect,
      onmarks,
      tool: options.selecting === false ? 'none' : 'select',
      selection: options.selection ?? [],
    },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await tick();
  const canvas = target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
  const fire = (type: string, p: { x: number; y: number }, init: Init = {}) => {
    const event = new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      clientX: p.x,
      clientY: p.y,
      button: 0,
      buttons: type === 'pointerup' ? 0 : (init.buttons ?? 1),
      shiftKey: init.shiftKey,
      ctrlKey: init.ctrlKey,
    });
    const props: Record<string, unknown> = {
      pointerType: init.pointerType ?? 'mouse',
      pointerId: init.pointerId ?? 1,
      pressure: 0.5,
      timeStamp: init.t ?? 0,
    };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    canvas.dispatchEvent(event);
  };
  /** A lasso round the page rectangle, with the mouse by default. */
  const lasso = (x0: number, y0: number, x1: number, y1: number, init: Init = {}) => {
    const corners = [at(x0, y0), at(x1, y0), at(x1, y1), at(x0, y1)];
    fire('pointerdown', corners[0], init);
    for (const corner of corners.slice(1)) fire('pointermove', corner, init);
    fire('pointermove', at(x0, y0 + 10), init);
    fire('pointerup', at(x0, y0 + 10), init);
  };
  const click = (x: number, y: number, init: Init = {}) => {
    fire('pointerdown', at(x, y), init);
    fire('pointerup', at(x, y), init);
  };
  return { live, canvas, fire, lasso, click, onview, onselect, onmarks };
}

describe('live preview with the Select tool', () => {
  it('chooses what a mouse lasso encloses, with the real geometry', async () => {
    const view = await render();
    view.lasso(2500, 3000, 4500, 5000);
    expect(view.onselect).toHaveBeenCalledExactlyOnceWith([0], 'replace');
    view.onselect.mockClear();
    view.lasso(2500, 3000, 4500, 9000);
    expect(view.onselect).toHaveBeenCalledExactlyOnceWith([0, 1], 'replace');
    // The area was not dragged.
    expect(view.onview).not.toHaveBeenCalled();
  });

  it('chooses with the pen too', async () => {
    const view = await render();
    view.lasso(2500, 7000, 4500, 9000, { pointerType: 'pen', pointerId: 7 });
    expect(view.onselect).toHaveBeenCalledExactlyOnceWith([1], 'replace');
  });

  it('picks the stroke under a click, and a click on nothing chooses nothing (the tool then clears)', async () => {
    const view = await render();
    view.click(3500, 8010);
    expect(view.onselect).toHaveBeenLastCalledWith([1], 'replace');
    view.click(20000, 14000);
    expect(view.onselect).toHaveBeenLastCalledWith([], 'replace');
    view.lasso(15000, 15000, 18000, 18000);
    expect(view.onselect).toHaveBeenLastCalledWith([], 'replace');
  });

  it('Shift adds and Ctrl removes', async () => {
    const view = await render();
    view.click(3500, 8010, { shiftKey: true });
    expect(view.onselect).toHaveBeenLastCalledWith([1], 'add');
    view.lasso(2500, 3000, 4500, 5000, { ctrlKey: true });
    expect(view.onselect).toHaveBeenLastCalledWith([0], 'remove');
  });

  it('cannot choose a stroke of a hidden layer', async () => {
    const view = await render();
    view.lasso(11000, 3000, 14000, 5000);
    expect(view.onselect).toHaveBeenLastCalledWith([], 'replace');
    view.click(12500, 4005);
    expect(view.onselect).toHaveBeenLastCalledWith([], 'replace');
    view.lasso(0, 0, 29000, 21000);
    expect(view.onselect).toHaveBeenLastCalledWith([0, 1], 'replace');
  });

  it('a finger and the wheel keep navigating; nothing is chosen with them', async () => {
    const view = await render();
    view.fire('pointerdown', at(3500, 4000), { pointerType: 'touch', pointerId: 3, t: 0 });
    view.fire('pointermove', at(5000, 6000), { pointerType: 'touch', pointerId: 3, t: 30 });
    view.fire('pointerup', at(5000, 6000), { pointerType: 'touch', pointerId: 3, t: 60 });
    expect(view.onview).toHaveBeenCalled();
    expect(view.onselect).not.toHaveBeenCalled();

    view.onview.mockClear();
    const wheel = new WheelEvent('wheel', { deltaY: -120, clientX: 400, clientY: 280, cancelable: true, bubbles: true });
    view.canvas.dispatchEvent(wheel);
    expect(wheel.defaultPrevented).toBe(true);
    expect(view.onview).toHaveBeenCalledOnce();
    expect(view.onview.mock.calls[0][0].zoom).toBeGreaterThan(1);
  });

  it('without the tool the mouse still drags the projected area and chooses nothing', async () => {
    const view = await render({ selecting: false });
    view.lasso(2500, 3000, 4500, 5000);
    expect(view.onview).toHaveBeenCalled();
    expect(view.onselect).not.toHaveBeenCalled();
  });

  it('draws the lasso as it grows, and drops it when the gesture is cancelled', async () => {
    const view = await render();
    const lassoDrawn = () =>
      contexts.some((c) => c.ops('stroke').some((call) => call.strokeStyle === SELECTION_COLORS.light.line));
    view.fire('pointerdown', at(2500, 3000));
    view.fire('pointermove', at(4500, 3000));
    view.fire('pointermove', at(4500, 5000));
    await vi.waitFor(() => expect(lassoDrawn()).toBe(true));
    view.fire('pointerup', at(4500, 5000));
    expect(view.onselect).toHaveBeenCalledOnce();

    view.onselect.mockClear();
    view.fire('pointerdown', at(2500, 3000));
    view.fire('pointermove', at(4500, 5000));
    view.fire('pointercancel', at(4500, 5000));
    view.fire('pointerup', at(4500, 5000));
    expect(view.onselect).not.toHaveBeenCalled();
  });

  it('paints the selection it is given, over the page', async () => {
    await render({ selection: [1] });
    // The offscreen surface of the highlight holds the ring and the dashed box.
    await vi.waitFor(() =>
      expect(
        contexts.some((c) => c.ops('fill').some((call) => call.fillStyle === SELECTION_COLORS.light.halo) && c.count('strokeRect') === 1),
      ).toBe(true),
    );
  });

  it('keeps drawing what the tablet writes, and its indices, while strokes are chosen', async () => {
    const view = await render({ selection: [0] });
    const batch = (...events: LiveBatch['events']): LiveBatch => ({ events, sentAt: 0 });
    const point = (x: number, y: number, i: number) => ({ x, y, p: 0.6, t: 5 * i, recv: i });
    // A stroke in progress: it is not in the list yet, so a lasso over it chooses nothing.
    view.live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: { tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [point(15000, 12000, 0), point(15500, 12000, 1)] } }));
    view.live.receive(batch({ type: 'points', pageIndex: 0, points: [point(16000, 12000, 2), point(16500, 12000, 3)] }));
    expect(view.live.current?.points.length).toBe(4);
    await tick();
    expect(view.canvas.dataset.strokes).toBe('4');
    view.lasso(14500, 11000, 17500, 13000);
    expect(view.onselect).toHaveBeenLastCalledWith([], 'replace');
    // It ends: it is the next stroke, and the earlier indices are the same.
    view.live.receive(batch({ type: 'strokeEnd', pageIndex: 0 }));
    expect(view.live.strokes).toHaveLength(4);
    expect(view.live.strokes[0].points[0].y).toBe(4000);
    view.lasso(14500, 11000, 17500, 13000);
    expect(view.onselect).toHaveBeenLastCalledWith([3], 'replace');
    await vi.waitFor(() => expect(contexts.some((c) => c.count('fill') > 0)).toBe(true));
  });

  it('lassos at the right place when zoomed in on a corner (the preview always shows the whole page)', async () => {
    const view = await render();
    view.live.changeView({ zoom: 4, x: 5000, y: 5000, board: false, laser: false }, async () => {});
    view.lasso(2500, 7000, 4500, 9000);
    expect(view.onselect).toHaveBeenLastCalledWith([1], 'replace');
  });
});

describe('presentation window', () => {
  it('ignores the Select tool: its marks are written as before', async () => {
    const view = await render({ mode: 'presentation', selecting: true });
    view.fire('pointerdown', { x: 300, y: 300 });
    view.fire('pointermove', { x: 400, y: 340 });
    view.fire('pointermove', { x: 500, y: 300 });
    view.fire('pointerup', { x: 500, y: 300 });
    expect(view.onmarks).toHaveBeenLastCalledWith(1);
    expect(view.onselect).not.toHaveBeenCalled();
  });
});
