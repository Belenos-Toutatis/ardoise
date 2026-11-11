// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M14: the control window's preview with the computer's hand. With Pen the pen and the mouse write on the
// chosen layer (the window draws its own stroke, then hands it over to the session's), a finger keeps navigating, a
// finger is ignored while the pen is near; the pen's eraser end erases the chosen layer's strokes in every tool; the
// selection takes the chosen layer only; a hidden chosen layer blocks the pen.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { instantiateInk, type Ink } from '../../lib/ink';
import type { LiveBatch, LiveComputerPenInput, LiveSnapshot, LiveStrokesEdit, LiveStyle, LiveViewState } from '../../lib/live';
import { LiveComputer } from '../../lib/live-computer';
import { FakeContext } from '../../lib/live-fake-canvas';
import type { Combine } from '../../lib/live-select.svelte';
import { LiveState } from '../../lib/live-state.svelte';
import { fitTransform, pageToScreen } from '../../lib/live-view';
import type { Layer, Note, Stroke } from '../../lib/note';
import LiveCanvas from './LiveCanvas.svelte';

const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const RED = { r: 216, g: 0, b: 0, a: 255 };
const STYLE: LiveStyle = { tool: 'feltTip', color: RED, width: 120 };
const A4 = { width: 29700, height: 21600 };
const LAYERS: Layer[] = [
  { name: 'Calque 1', visible: true },
  { name: 'Correction', visible: true },
];

const cleanups: (() => void)[] = [];
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
  for (const cleanup of cleanups.splice(0)) cleanup();
  vi.restoreAllMocks();
});

/** A horizontal line from `x` at `y`, 1000 units long. */
function line(x: number, y: number, layer: number): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer,
    points: Array.from({ length: 20 }, (_, i) => ({ x: x + 50 * i, y, pressure: 0.6, t: 5 * i })),
  };
}

function snapshot(extra: { strokes?: Stroke[]; layers?: Layer[]; state?: LiveSnapshot['state'] } = {}): LiveSnapshot {
  const page: Note = {
    id: 'p1',
    title: '',
    created: 0,
    modified: 0,
    page: A4,
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: extra.layers ?? LAYERS,
    // Two strokes of the tablet's layer, one of the computer's, all crossing x = 3500.
    strokes: extra.strokes ?? [line(3000, 4000, 0), line(3000, 8000, 1), line(3000, 12000, 0)],
    timing: 'recorded',
    origin: { kind: 'live' },
  };
  return {
    state: extra.state ?? 'live',
    page,
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    layer: 0,
    computerLayer: 1,
    style: { tool: 'ballpoint', color: BLACK, width: 60 },
    view: { zoom: 1, x: A4.width / 2, y: A4.height / 2, board: false, laser: false },
    hover: null,
    presentation: null,
    saved: [],
    revision: 5,
  };
}

interface Init {
  pointerType?: string;
  buttons?: number;
  button?: number;
  pointerId?: number;
  t?: number;
  pressure?: number;
}

const TRANSFORM = fitTransform(A4, { width: 800, height: 560 }, 12);
const at = (x: number, y: number) => pageToScreen(TRANSFORM, { x, y });

async function render(options: { tool?: 'none' | 'select' | 'pen'; snapshot?: LiveSnapshot } = {}) {
  const live = new LiveState();
  live.initial(options.snapshot ?? snapshot());
  const pen: LiveComputerPenInput[] = [];
  const edits: { pageIndex: number; revision: number; edit: LiveStrokesEdit }[] = [];
  const onerror = vi.fn();
  const computer = new LiveComputer({
    live,
    commands: {
      liveComputerPen: async (input) => {
        pen.push(structuredClone(input));
        return true;
      },
      liveEditStrokes: async (pageIndex, revision, edit) => {
        edits.push({ pageIndex, revision, edit: structuredClone(edit) });
        return revision + 1;
      },
    },
    style: () => STYLE,
    onerror,
  });
  const target = document.createElement('div');
  document.body.append(target);
  const onview = vi.fn<(view: LiveViewState) => void>();
  const onselect = vi.fn<(picked: number[], combine: Combine) => void>();
  const onblocked = vi.fn();
  const component = mount(LiveCanvas, {
    target,
    props: { live, ink, mode: 'preview', label: 'page', onview, onselect, onblocked, tool: options.tool ?? 'pen', computer },
  });
  cleanups.push(() => {
    unmount(component);
    target.remove();
  });
  await tick();
  const canvas = target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
  let clock = 100;
  const fire = (type: string, p: { x: number; y: number }, init: Init = {}) => {
    const event = new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      clientX: p.x,
      clientY: p.y,
      button: init.button ?? 0,
      buttons: type === 'pointerup' ? 0 : (init.buttons ?? 1),
    });
    const props: Record<string, unknown> = {
      pointerType: init.pointerType ?? 'mouse',
      pointerId: init.pointerId ?? 1,
      pressure: init.pressure ?? 0.5,
      timeStamp: init.t ?? (clock += 8),
    };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    canvas.dispatchEvent(event);
    return event;
  };
  /** A gesture through page points. */
  const drag = (points: [number, number][], init: Init = {}) => {
    fire('pointerdown', at(...points[0]), init);
    for (const p of points.slice(1)) fire('pointermove', at(...p), init);
    fire('pointerup', at(...points[points.length - 1]), init);
  };
  const settle = () => computer.queue.idle();
  return { live, canvas, fire, drag, settle, computer, pen, edits, onview, onselect, onblocked, onerror };
}

const ERASER: Init = { pointerType: 'pen', buttons: 32, pointerId: 9 };
/** The fills of the visible canvas (the strokes in progress are drawn there, the finished ones on the cache). */
const framedFills = (color: string) => contexts[0]?.ops('fill').filter((call) => call.fillStyle === color).length ?? 0;

describe('the Pen tool', () => {
  it('writes with the mouse: begin at once, then the samples, end; nothing moves the area', async () => {
    const view = await render();
    view.drag([
      [5000, 15000],
      [5400, 15100],
      [5800, 15300],
    ]);
    await view.settle();
    expect(view.onview).not.toHaveBeenCalled();
    // Everything came before `begin` left: it took the three samples, `end` none.
    expect(view.pen.map((call) => call.kind)).toEqual(['begin', 'end']);
    const begin = view.pen[0] as Extract<LiveComputerPenInput, { kind: 'begin' }>;
    expect(begin).toMatchObject({ stroke: 1, pageIndex: 0, style: STYLE });
    expect(begin.samples[0].x).toBeCloseTo(5000, -1);
    expect(begin.samples[0].y).toBeCloseTo(15000, -1);
    // A pressed mouse reports 0.5, kept as the pressure.
    expect(begin.samples[0].p).toBe(0.5);
    const all = view.pen.flatMap((call) => ('samples' in call ? call.samples : []));
    expect(all).toHaveLength(3);
    expect(all.every((s, i) => i === 0 || s.ms > all[i - 1].ms)).toBe(true);
  });

  it('writes with the pen, its pressure kept, and drops samples closer than half a pixel', async () => {
    const view = await render();
    const init = { pointerType: 'pen', pointerId: 4, pressure: 0.8 };
    view.fire('pointerdown', at(5000, 15000), init);
    view.fire('pointermove', at(5001, 15000), init);
    view.fire('pointermove', at(5600, 15000), init);
    view.fire('pointerup', at(5600, 15000), init);
    await view.settle();
    const all = view.pen.flatMap((call) => ('samples' in call ? call.samples : []));
    expect(all).toHaveLength(2);
    expect(all.every((s) => s.p === 0.8)).toBe(true);
  });

  it('draws its own stroke at once, and hands it over when the session says it is done', async () => {
    const view = await render();
    view.fire('pointerdown', at(5000, 15000));
    view.fire('pointermove', at(6000, 15200));
    await vi.waitFor(() => expect(framedFills('rgb(216, 0, 0)')).toBeGreaterThan(0));
    expect(view.computer.locals()).toHaveLength(1);
    view.fire('pointerup', at(6000, 15200));
    await view.settle();
    // Rust's copy comes, then its end: the stroke is in the list, the window's own goes.
    const batch = (...events: LiveBatch['events']): LiveBatch => ({ events, sentAt: 0 });
    const points = [5000, 6000].map((x, i) => ({ x, y: 15000, p: 0.5, t: 8 * i, recv: i }));
    view.live.receive(batch({ type: 'strokeStart', pageIndex: 0, stroke: { ...STYLE, layer: 1, points }, pen: 'computer', id: 1 }));
    expect(view.computer.locals()).toHaveLength(1);
    await tick();
    expect(view.canvas.dataset.strokes).toBe('4');
    view.live.receive(batch({ type: 'strokeEnd', pageIndex: 0, pen: 'computer', id: 1 }));
    expect(view.computer.locals()).toBeUndefined();
    expect(view.live.strokes).toHaveLength(4);
    await tick();
    expect(view.canvas.dataset.strokes).toBe('4');
  });

  it('a pen down on a hidden chosen layer writes nothing and says so', async () => {
    const view = await render({ snapshot: snapshot({ layers: [LAYERS[0], { ...LAYERS[1], visible: false }] }) });
    view.drag([
      [5000, 15000],
      [6000, 15000],
    ]);
    await view.settle();
    expect(view.onblocked).toHaveBeenCalledOnce();
    expect(view.pen).toEqual([]);
  });

  it('writes nothing once the session is over', async () => {
    const view = await render({ snapshot: snapshot({ state: 'idle' }) });
    view.drag([
      [5000, 15000],
      [6000, 15000],
    ]);
    await view.settle();
    expect(view.pen).toEqual([]);
  });

  it('pointercancel, a lost capture and a second finger drop the stroke', async () => {
    const view = await render();
    view.fire('pointerdown', at(5000, 15000));
    await view.settle();
    view.fire('pointermove', at(6000, 15000));
    view.fire('pointercancel', at(6000, 15000));
    await view.settle();
    expect(view.pen.map((call) => call.kind)).toEqual(['begin', 'cancel']);
    view.fire('pointerdown', at(5000, 15000), { pointerId: 2 });
    await view.settle();
    view.fire('lostpointercapture', at(5000, 15000), { pointerId: 2 });
    await view.settle();
    expect(view.pen.map((call) => call.kind)).toEqual(['begin', 'cancel', 'begin', 'cancel']);
    // The mouse writes, two fingers land: the pinch takes over.
    view.fire('pointerdown', at(5000, 15000), { pointerId: 3 });
    await view.settle();
    view.fire('pointerdown', at(10000, 10000), { pointerType: 'touch', pointerId: 20 });
    view.fire('pointerdown', at(12000, 10000), { pointerType: 'touch', pointerId: 21 });
    await view.settle();
    expect(view.pen.at(-1)).toEqual({ kind: 'cancel', stroke: 3 });
  });

  it('a finger moves the projected area and pinches it, never writes; the wheel zooms', async () => {
    const view = await render();
    view.fire('pointerdown', at(5000, 5000), { pointerType: 'touch', pointerId: 11, t: 1000 });
    view.fire('pointermove', at(7000, 7000), { pointerType: 'touch', pointerId: 11, t: 1030 });
    view.fire('pointerup', at(7000, 7000), { pointerType: 'touch', pointerId: 11, t: 1060 });
    expect(view.onview).toHaveBeenCalled();
    view.onview.mockClear();
    view.fire('pointerdown', at(10000, 10000), { pointerType: 'touch', pointerId: 12, t: 2000 });
    view.fire('pointerdown', at(12000, 10000), { pointerType: 'touch', pointerId: 13, t: 2005 });
    view.fire('pointermove', at(8000, 10000), { pointerType: 'touch', pointerId: 12, t: 2030 });
    view.fire('pointermove', at(14000, 10000), { pointerType: 'touch', pointerId: 13, t: 2035 });
    expect(view.onview).toHaveBeenCalled();
    expect(view.onview.mock.calls.at(-1)![0].zoom).toBeGreaterThan(1);
    view.onview.mockClear();
    const wheel = new WheelEvent('wheel', { deltaY: -120, clientX: 400, clientY: 280, cancelable: true, bubbles: true });
    view.canvas.dispatchEvent(wheel);
    expect(view.onview).toHaveBeenCalledOnce();
    await view.settle();
    expect(view.pen).toEqual([]);
  });

  it('ignores a finger while the pen touches or hovered a moment ago (palm rejection)', async () => {
    const view = await render();
    view.fire('pointerdown', at(5000, 15000), { pointerType: 'pen', pointerId: 4, t: 5000 });
    view.fire('pointerdown', at(9000, 9000), { pointerType: 'touch', pointerId: 30, t: 5010 });
    view.fire('pointermove', at(12000, 12000), { pointerType: 'touch', pointerId: 30, t: 5040 });
    view.fire('pointerup', at(5000, 15000), { pointerType: 'pen', pointerId: 4, t: 5100 });
    view.fire('pointerup', at(12000, 12000), { pointerType: 'touch', pointerId: 30, t: 5110 });
    expect(view.onview).not.toHaveBeenCalled();
    await view.settle();
    expect(view.pen.map((call) => call.kind)).toEqual(['begin', 'end']);
  });
});

describe("the pen's eraser end", () => {
  for (const tool of ['none', 'select', 'pen'] as const) {
    it(`erases whole strokes of the chosen layer only, with the ${tool} tool`, async () => {
      const view = await render({ tool });
      // Down across x = 3500, from y = 3000 to y = 13000: it crosses the three strokes.
      view.drag(
        [
          [3500, 3000],
          [3500, 6000],
          [3500, 9000],
          [3500, 13000],
        ],
        ERASER,
      );
      await view.settle();
      expect(view.edits).toHaveLength(1);
      expect(view.edits[0]).toMatchObject({ pageIndex: 0, revision: 5, edit: { kind: 'delete', indices: [1] } });
      expect(typeof (view.edits[0].edit as { group?: number }).group).toBe('number');
      expect(view.pen).toEqual([]);
      expect(view.onview).not.toHaveBeenCalled();
      expect(view.onselect).not.toHaveBeenCalled();
    });
  }

  it('erases with the button that changed (5) as well, and rings itself on the preview', async () => {
    const view = await render({ tool: 'none' });
    view.fire('pointerdown', at(3500, 7900), { pointerType: 'pen', button: 5, buttons: 0, pointerId: 9 });
    await view.settle();
    expect(view.edits).toHaveLength(1);
    await vi.waitFor(() => expect(contexts[0].ops('arc').some((call) => call.args[2] === 18)).toBe(true));
    view.fire('pointerup', at(3500, 7900), { pointerType: 'pen', button: 5, pointerId: 9 });
    expect(view.computer.eraser.ring).toBeNull();
  });

  it('finds nothing on a hidden chosen layer', async () => {
    const view = await render({ snapshot: snapshot({ layers: [LAYERS[0], { ...LAYERS[1], visible: false }] }) });
    view.drag(
      [
        [3500, 3000],
        [3500, 13000],
      ],
      ERASER,
    );
    await view.settle();
    expect(view.edits).toEqual([]);
  });
});

describe('the selection', () => {
  it('takes the strokes of the chosen layer only', async () => {
    const view = await render({ tool: 'select' });
    const corners: [number, number][] = [
      [2000, 2000],
      [8000, 2000],
      [8000, 14000],
      [2000, 14000],
      [2000, 2010],
    ];
    view.drag(corners);
    expect(view.onselect).toHaveBeenLastCalledWith([1], 'replace');
    // A click on a stroke of the tablet's layer picks nothing.
    view.drag([[3500, 4005]]);
    expect(view.onselect).toHaveBeenLastCalledWith([], 'replace');
    view.drag([[3500, 8005]]);
    expect(view.onselect).toHaveBeenLastCalledWith([1], 'replace');
  });

  it('shows the tool on the canvas', async () => {
    const view = await render({ tool: 'select' });
    expect(view.canvas.dataset.tool).toBe('select');
    expect(view.canvas.classList.contains('selecting')).toBe(true);
    const pen = await render({ tool: 'pen' });
    expect(pen.canvas.dataset.tool).toBe('pen');
    expect(pen.canvas.classList.contains('pen')).toBe(true);
  });
});
