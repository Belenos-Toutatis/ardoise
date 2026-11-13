// @vitest-environment jsdom
/// <reference types="node" />
// The shape tools on the page (docs/design/shape-tools.md): a drag with the line, rectangle or ellipse tool shows the
// shape and adds one ordinary stroke when the pointer is let go (the active layer, the tool, colour and width in use,
// times going up), a click adds nothing, Escape (`cancelGesture`) drops it, Shift and Alt invert two of the toggles for
// the stroke, the grid snaps the points when asked. The geometry alone is `lib/editor/shape-tools.test.ts`.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { shapePrefs } from '../../lib/editor/shape-prefs.svelte';
import type { SnapGrid } from '../../lib/editor/shape-tools';
import { DEFAULT_TOOLS, docFromNote, setActiveLayer, setLayerVisible, type Doc, type ToolSettings } from '../../lib/editor/state';
import { hline, note } from '../../lib/editor/test-fixtures';
import type { StrokeClock } from '../../lib/editor/writing-clock';
import { instantiateInk, type Ink } from '../../lib/ink';
import { templateSnapGrid } from '../../lib/mock-notes';
import { touchPrefs } from '../../lib/touch-prefs.svelte';
import Canvas from './Canvas.svelte';

let ink: Ink;
beforeAll(async () => {
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
});

let cleanup: (() => void) | null = null;

beforeEach(() => {
  shapePrefs.reset();
  touchPrefs.set(false);
});
afterEach(() => {
  cleanup?.();
  cleanup = null;
  shapePrefs.reset();
  touchPrefs.set(false);
});

interface Init {
  pointerType?: string;
  shiftKey?: boolean;
  altKey?: boolean;
  buttons?: number;
}

interface Props {
  doc?: Doc;
  tools?: Partial<ToolSettings>;
  snapGrid?: SnapGrid | null;
  strokeClock?: StrokeClock | null;
  onblocked?: () => void;
}

/** A canvas on a note with one stroke whose samples end at t = 200 (21 samples 10 ms apart). */
async function render(props: Props = {}) {
  const doc = props.doc ?? docFromNote(note([hline(1000, 3000, 3000)]));
  const onchange = vi.fn();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Canvas, {
    target,
    props: {
      doc,
      tools: { ...DEFAULT_TOOLS, mode: 'rectangle', ...props.tools },
      ink,
      onchange,
      onselect: vi.fn(),
      onblocked: props.onblocked,
      snapGrid: props.snapGrid ?? null,
      strokeClock: props.strokeClock ?? null,
    },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const client = (x: number, y: number) => {
    const [scale, vx, vy] = svg.getAttribute('data-view')!.split(' ').map(Number);
    return { x: x * scale + vx, y: y * scale + vy };
  };
  let clock = 1000;
  const fire = (type: string, x: number, y: number, init: Init = {}) => {
    const at = client(x, y);
    const event = new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      clientX: at.x,
      clientY: at.y,
      button: 0,
      buttons: init.buttons ?? (type === 'pointerup' ? 0 : 1),
      shiftKey: init.shiftKey,
      altKey: init.altKey,
    });
    for (const [key, value] of Object.entries({ pointerType: init.pointerType ?? 'mouse', pointerId: 1, pressure: 0.5, timeStamp: clock })) {
      Object.defineProperty(event, key, { value, configurable: true });
    }
    svg.dispatchEvent(event);
  };
  /** A drag from one page point to another, 400 ms long. */
  const drag = async (from: [number, number], to: [number, number], init: Init = {}) => {
    fire('pointerdown', from[0], from[1], init);
    for (let i = 1; i <= 4; i++) {
      clock += 100;
      fire('pointermove', from[0] + ((to[0] - from[0]) * i) / 4, from[1] + ((to[1] - from[1]) * i) / 4, init);
    }
    fire('pointerup', to[0], to[1], init);
    await tick();
  };
  const added = () => {
    const doc = onchange.mock.calls.at(-1)![0] as Doc;
    return doc.note.strokes.at(-1)!;
  };
  const live = () => svg.querySelectorAll('path.live').length;
  /** The box the stroke reaches, in whole page units. */
  const extent = (stroke: { points: { x: number; y: number }[] }) => {
    const xs = stroke.points.map((p) => p.x);
    const ys = stroke.points.map((p) => p.y);
    return { x0: Math.round(Math.min(...xs)), x1: Math.round(Math.max(...xs)), y0: Math.round(Math.min(...ys)), y1: Math.round(Math.max(...ys)) };
  };
  const cancel = () => (component as unknown as { cancelGesture(): void }).cancelGesture();
  const dragging = () => (component as unknown as { dragging(): boolean }).dragging();
  return { svg, fire, drag, onchange, added, live, extent, cancel, dragging, setClock: (t: number) => (clock = t) };
}

describe('drawing a shape', () => {
  it('a line goes exactly from the first point to the last, as one ordinary stroke', async () => {
    const c = await render({ tools: { mode: 'line', width: 90, tool: 'fountainPen' } });
    await c.drag([5000, 8000], [12_500, 9500]);
    expect(c.onchange).toHaveBeenCalledTimes(1);
    const stroke = c.added();
    expect(stroke).toMatchObject({ tool: 'fountainPen', width: 90, layer: 0 });
    expect(stroke.color).toEqual(DEFAULT_TOOLS.color);
    const [first, last] = [stroke.points[0], stroke.points.at(-1)!];
    expect([Math.round(first.x), Math.round(first.y)]).toEqual([5000, 8000]);
    expect([Math.round(last.x), Math.round(last.y)]).toEqual([12_500, 9500]);
    // Straight, and dense enough for the replay to draw it progressively.
    expect(stroke.points.length).toBeGreaterThan(100);
    for (const p of stroke.points) expect(Math.abs((p.x - 5000) * 1500 - (p.y - 8000) * 7500)).toBeLessThan(1e-6 * 1e9);
    // The note holds the old stroke and the new one.
    expect((c.onchange.mock.calls[0][0] as Doc).note.strokes).toHaveLength(2);
  });

  it('a rectangle is one closed stroke through its four corners', async () => {
    const c = await render({ tools: { mode: 'rectangle' } });
    await c.drag([5000, 8000], [12_000, 11_000]);
    const stroke = c.added();
    expect([stroke.points[0].x, stroke.points[0].y].map(Math.round)).toEqual([5000, 8000]);
    // Closed: it ends where it began (the time is the end's).
    expect(stroke.points.at(-1)).toMatchObject({ x: stroke.points[0].x, y: stroke.points[0].y });
    for (const [x, y] of [[12_000, 8000], [12_000, 11_000], [5000, 11_000]]) {
      expect(stroke.points.some((p) => Math.abs(p.x - x) < 1e-3 && Math.abs(p.y - y) < 1e-3)).toBe(true);
    }
    expect(c.extent(stroke)).toEqual({ x0: 5000, x1: 12_000, y0: 8000, y1: 11_000 });
  });

  it('an ellipse is one closed stroke, finely sampled', async () => {
    const c = await render({ tools: { mode: 'ellipse' } });
    await c.drag([5000, 8000], [13_000, 12_000]);
    const stroke = c.added();
    expect(stroke.points.length).toBeGreaterThanOrEqual(73);
    expect(stroke.points.at(-1)).toMatchObject({ x: stroke.points[0].x, y: stroke.points[0].y });
    const { x0, x1, y0, y1 } = c.extent(stroke);
    expect([x0, x1, y0, y1]).toEqual([5000, 13_000, 8000, 12_000]);
  });

  it('goes on the active layer, drawn in the ink, colour and width in use, with a constant pressure', async () => {
    const two = docFromNote(note([hline(1000, 3000, 3000)], 2));
    const c = await render({ doc: setActiveLayer(two, 1), tools: { mode: 'rectangle', tool: 'brush', width: 150, color: { r: 200, g: 10, b: 10, a: 255 } } });
    await c.drag([5000, 8000], [9000, 10_000]);
    const stroke = c.added();
    expect(stroke).toMatchObject({ tool: 'brush', width: 150, layer: 1, color: { r: 200, g: 10, b: 10, a: 255 } });
    expect(new Set(stroke.points.map((p) => p.pressure))).toEqual(new Set([1]));
  });

  it('takes the times after the note’s ink, going up, over the time the drag took', async () => {
    const c = await render({ tools: { mode: 'rectangle' } });
    await c.drag([5000, 8000], [9000, 10_000]);
    const times = c.added().points.map((p) => p.t);
    // The note's last sample is at 200 ms: the shape starts 500 ms after it and lasts the 400 ms of the drag.
    expect(times[0]).toBe(700);
    expect(times.at(-1)).toBe(1100);
    for (let i = 1; i < times.length; i++) expect(times[i]).toBeGreaterThanOrEqual(times[i - 1]);
    expect(new Set(times).size).toBeGreaterThan(20);
  });

  it('a quick drag lasts a moment anyway, and a long one is drawn in two seconds at most', async () => {
    const quick = await render({ tools: { mode: 'line' } });
    quick.fire('pointerdown', 5000, 8000);
    quick.fire('pointerup', 9000, 8000);
    await tick();
    const t = quick.added().points.map((p) => p.t);
    expect(t.at(-1)! - t[0]).toBe(150);
    cleanup?.();
    const slow = await render({ tools: { mode: 'line' } });
    slow.fire('pointerdown', 5000, 8000);
    slow.setClock(61_000);
    slow.fire('pointerup', 9000, 8000);
    await tick();
    const u = slow.added().points.map((p) => p.t);
    expect(u.at(-1)! - u[0]).toBe(2000);
  });

  it('during a writing recording it takes the sound’s time, the real duration of the drag', async () => {
    const clock: StrokeClock = { noteTime: (timeStamp) => 50_000 + (timeStamp - 1000) };
    const c = await render({ tools: { mode: 'line' }, strokeClock: clock });
    await c.drag([5000, 8000], [9000, 8000]);
    const times = c.added().points.map((p) => p.t);
    expect(times[0]).toBe(50_000);
    expect(times.at(-1)).toBe(50_400);
  });

  it('a click draws nothing, and neither does a drag too small to be one', async () => {
    const c = await render({ tools: { mode: 'ellipse' } });
    c.fire('pointerdown', 5000, 8000);
    c.fire('pointerup', 5000, 8000);
    await tick();
    expect(c.onchange).not.toHaveBeenCalled();
    await c.drag([5000, 8000], [5040, 8040]);
    expect(c.onchange).not.toHaveBeenCalled();
  });

  it('shows the shape while it is dragged and nothing once it is added', async () => {
    const c = await render({ tools: { mode: 'rectangle' } });
    c.fire('pointerdown', 5000, 8000);
    c.fire('pointermove', 12_000, 11_000);
    await vi.waitFor(() => expect(c.live()).toBe(1));
    expect(c.svg.querySelector('path.live')!.getAttribute('d')).toMatch(/^M/);
    c.fire('pointerup', 12_000, 11_000);
    await tick();
    expect(c.live()).toBe(0);
  });

  it('works with the pen, and with a finger when it is allowed to draw', async () => {
    const pen = await render({ tools: { mode: 'line' } });
    await pen.drag([5000, 8000], [9000, 8000], { pointerType: 'pen' });
    expect(pen.onchange).toHaveBeenCalledTimes(1);
    cleanup?.();
    // A finger pans: no shape.
    const finger = await render({ tools: { mode: 'line' } });
    await finger.drag([5000, 8000], [9000, 8000], { pointerType: 'touch' });
    expect(finger.onchange).not.toHaveBeenCalled();
    cleanup?.();
    touchPrefs.set(true);
    const drawing = await render({ tools: { mode: 'line' } });
    await drawing.drag([5000, 8000], [9000, 8000], { pointerType: 'touch' });
    expect(drawing.onchange).toHaveBeenCalledTimes(1);
  });

  it('does not draw on a hidden layer, and says so', async () => {
    const hidden = setLayerVisible(docFromNote(note([hline(1000, 3000, 3000)], 2)), 0, false);
    const onblocked = vi.fn();
    const c = await render({ doc: setActiveLayer(hidden, 0), tools: { mode: 'rectangle' }, onblocked });
    await c.drag([5000, 8000], [9000, 10_000]);
    expect(onblocked).toHaveBeenCalled();
    expect(c.onchange).not.toHaveBeenCalled();
  });
});

describe('Escape', () => {
  it('drops the shape under way: nothing is added when the pointer is let go', async () => {
    const c = await render({ tools: { mode: 'rectangle' } });
    c.fire('pointerdown', 5000, 8000);
    c.fire('pointermove', 12_000, 11_000);
    await vi.waitFor(() => expect(c.live()).toBe(1));
    expect(c.dragging()).toBe(true);
    c.cancel();
    await tick();
    expect(c.dragging()).toBe(false);
    expect(c.live()).toBe(0);
    c.fire('pointermove', 13_000, 12_000);
    c.fire('pointerup', 13_000, 12_000);
    await tick();
    expect(c.onchange).not.toHaveBeenCalled();
    expect(c.live()).toBe(0);
  });

  it('a pointer that is cancelled (a second finger, the pen leaving) adds nothing either', async () => {
    const c = await render({ tools: { mode: 'rectangle' } });
    c.fire('pointerdown', 5000, 8000);
    c.fire('pointermove', 12_000, 11_000);
    c.fire('pointercancel', 12_000, 11_000);
    await tick();
    expect(c.onchange).not.toHaveBeenCalled();
  });
});

describe('the toggles', () => {
  it('« Forme régulière » makes a square, a circle, a line on an axis or a diagonal', async () => {
    shapePrefs.regular = true;
    const sq = await render({ tools: { mode: 'rectangle' } });
    await sq.drag([5000, 8000], [12_000, 9000]);
    expect(sq.extent(sq.added())).toEqual({ x0: 5000, x1: 12_000, y0: 8000, y1: 15_000 });
    cleanup?.();
    const circle = await render({ tools: { mode: 'ellipse' } });
    await circle.drag([5000, 8000], [9000, 9000]);
    const e = circle.extent(circle.added());
    expect([e.x1 - e.x0, e.y1 - e.y0]).toEqual([4000, 4000]);
    cleanup?.();
    const line = await render({ tools: { mode: 'line' } });
    await line.drag([5000, 8000], [12_000, 8600]);
    expect(new Set(line.added().points.map((p) => Math.round(p.y)))).toEqual(new Set([8000]));
    cleanup?.();
    const diagonal = await render({ tools: { mode: 'line' } });
    await diagonal.drag([5000, 8000], [9000, 11_500]);
    const last = diagonal.added().points.at(-1)!;
    expect([Math.round(last.x), Math.round(last.y)]).toEqual([9000, 12_000]);
  });

  it('Shift inverts it for the stroke, whichever way the toggle is', async () => {
    const off = await render({ tools: { mode: 'rectangle' } });
    await off.drag([5000, 8000], [12_000, 9000], { shiftKey: true });
    expect(off.extent(off.added())).toEqual({ x0: 5000, x1: 12_000, y0: 8000, y1: 15_000 });
    cleanup?.();
    shapePrefs.regular = true;
    const on = await render({ tools: { mode: 'rectangle' } });
    await on.drag([5000, 8000], [12_000, 9000], { shiftKey: true });
    expect(on.extent(on.added())).toEqual({ x0: 5000, x1: 12_000, y0: 8000, y1: 9000 });
  });

  it('« Depuis le centre » makes the first point the centre, and Alt inverts it', async () => {
    shapePrefs.centre = true;
    const c = await render({ tools: { mode: 'ellipse' } });
    await c.drag([9000, 9000], [11_000, 10_000]);
    expect(c.extent(c.added())).toMatchObject({ x0: 7000, x1: 11_000, y0: 8000, y1: 10_000 });
    cleanup?.();
    const inverted = await render({ tools: { mode: 'ellipse' } });
    await inverted.drag([9000, 9000], [11_000, 10_000], { altKey: true });
    expect(inverted.extent(inverted.added())).toMatchObject({ x0: 9000, x1: 11_000, y0: 9000, y1: 10_000 });
    cleanup?.();
    shapePrefs.reset();
    const alt = await render({ tools: { mode: 'ellipse' } });
    await alt.drag([9000, 9000], [11_000, 10_000], { altKey: true });
    expect(alt.extent(alt.added())).toMatchObject({ x0: 7000, x1: 11_000, y0: 8000, y1: 10_000 });
  });

  it('a line from the centre grows both ways', async () => {
    shapePrefs.centre = true;
    const c = await render({ tools: { mode: 'line' } });
    await c.drag([9000, 9000], [11_000, 9500]);
    const stroke = c.added();
    expect([stroke.points[0].x, stroke.points[0].y].map(Math.round)).toEqual([7000, 8500]);
    expect([stroke.points.at(-1)!.x, stroke.points.at(-1)!.y].map(Math.round)).toEqual([11_000, 9500]);
  });

  it('« Aimanter au quadrillage » snaps the corners to the grid of the page, and does nothing without one', async () => {
    shapePrefs.snap = true;
    const grid = templateSnapGrid({ width: 29_700, height: 21_600 }, 'grid5mm')!;
    const c = await render({ tools: { mode: 'rectangle' }, snapGrid: grid });
    await c.drag([5130, 8240], [12_380, 11_070]);
    const { x0, x1, y0, y1 } = c.extent(c.added());
    // Every 500 units from the first line: x from 100 (297 mm holds 59 squares and 2 mm over), y from 800 (216 mm: 43 and 1 mm over → 50).
    for (const v of [x0, x1]) expect((v - grid.x!.first) % 500).toBeCloseTo(0, 6);
    for (const v of [y0, y1]) expect((v - grid.y!.first) % 500).toBeCloseTo(0, 6);
    expect(x1 - x0).toBeGreaterThan(6000);
    cleanup?.();
    const free = await render({ tools: { mode: 'rectangle' }, snapGrid: null });
    await free.drag([5130, 8240], [12_380, 11_070]);
    expect(free.extent(free.added())).toEqual({ x0: 5130, x1: 12_380, y0: 8240, y1: 11_070 });
  });
});
