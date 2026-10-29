// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M8: the pen's eraser end and barrel button, pressure, palm rejection, finger gestures.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from 'vitest';
import { DEFAULT_TOOLS, docFromNote, select, type Doc, type ToolSettings } from '../../lib/editor/state';
import { hline, note } from '../../lib/editor/test-fixtures';
import { PenPresence } from '../../lib/gestures';
import { instantiateInk, type Ink } from '../../lib/ink';
import { touchPrefs } from '../../lib/touch-prefs.svelte';
import Canvas from './Canvas.svelte';
import CanvasHarness from './CanvasHarness.svelte';

let ink: Ink;
beforeAll(async () => {
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
});

let cleanup: (() => void) | null = null;

beforeEach(() => touchPrefs.set(false));
afterEach(() => {
  cleanup?.();
  cleanup = null;
  touchPrefs.set(false);
});

interface Init {
  pointerType?: string;
  buttons?: number;
  button?: number;
  pointerId?: number;
  pressure?: number;
  t?: number;
  ctrlKey?: boolean;
}

/** A stroke along y = 3000 from x = 1000 to 5000; the page is A4 landscape. */
async function render(extra: { tools?: Partial<ToolSettings>; readonly?: boolean; doc?: Doc } = {}) {
  const doc = extra.doc ?? docFromNote(note([hline(1000, 5000, 3000)]));
  const onchange = vi.fn();
  const onselect = vi.fn();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Canvas, {
    target,
    props: {
      doc,
      tools: { ...DEFAULT_TOOLS, ...extra.tools },
      ink,
      readonly: extra.readonly ?? false,
      onchange,
      onselect,
    },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const view = () => {
    const [scale, x, y] = svg.getAttribute('data-view')!.split(' ').map(Number);
    return { scale, x, y };
  };
  /** Page units (device units) to client pixels. */
  const client = (px: number, py: number) => {
    const v = view();
    return { x: px * v.scale + v.x, y: py * v.scale + v.y };
  };
  const fire = (type: string, at: { x: number; y: number }, init: Init = {}) => {
    const event = new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      clientX: at.x,
      clientY: at.y,
      button: init.button ?? 0,
      buttons: init.buttons ?? 1,
      ctrlKey: init.ctrlKey ?? false,
    });
    const props: Record<string, unknown> = {
      pointerType: init.pointerType ?? 'mouse',
      pointerId: init.pointerId ?? 1,
      pressure: init.pressure ?? 0.5,
      timeStamp: init.t ?? 0,
    };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    svg.dispatchEvent(event);
  };
  /** A gesture on page points: down, moves, up. */
  const stroke = async (points: [number, number][], init: Init = {}, up: Init = { buttons: 0 }) => {
    const [first, ...rest] = points;
    let t = init.t ?? 0;
    fire('pointerdown', client(...first), { ...init, t });
    for (const p of rest) fire('pointermove', client(...p), { ...init, t: (t += 10) });
    fire('pointerup', client(...points[points.length - 1]), { ...init, ...up, t: (t += 10) });
    await tick();
  };
  return { svg, view, client, fire, stroke, onchange, onselect, target };
}

const along = (x0: number, x1: number, y: number): [number, number][] =>
  Array.from({ length: 9 }, (_, i) => [x0 + ((x1 - x0) * i) / 8, y]);

describe('pen pressure', () => {
  it('records the pressure of the pen, so that the line follows it', async () => {
    const c = await render();
    const points = along(8000, 12000, 8000);
    let t = 0;
    c.fire('pointerdown', c.client(...points[0]), { pointerType: 'pen', pressure: 0.1, t });
    points.slice(1).forEach((p, i) => c.fire('pointermove', c.client(...p), { pointerType: 'pen', pressure: 0.1 + 0.1 * i, t: (t += 10) }));
    c.fire('pointerup', c.client(...points[8]), { pointerType: 'pen', buttons: 0, t: (t += 10) });
    await tick();
    expect(c.onchange).toHaveBeenCalledTimes(1);
    const added = c.onchange.mock.calls[0][0].note.strokes.at(-1);
    const pressures = added.points.map((p: { pressure: number }) => p.pressure);
    expect(pressures[0]).toBeCloseTo(0.1);
    expect(pressures.at(-1)).toBeGreaterThan(0.6);
    expect(new Set(pressures).size).toBeGreaterThan(3);
  });

  it('keeps a constant pressure for the mouse', async () => {
    const c = await render();
    await c.stroke(along(8000, 12000, 8000));
    const added = c.onchange.mock.calls[0][0].note.strokes.at(-1);
    expect(new Set(added.points.map((p: { pressure: number }) => p.pressure))).toEqual(new Set([0.5]));
  });
});

describe('the back of the pen', () => {
  it('erases whatever the tool, with the eraser settings', async () => {
    const c = await render({ tools: { mode: 'pen', eraser: 'whole', eraserRadius: 300 } });
    await c.stroke(along(900, 5200, 3000), { pointerType: 'pen', buttons: 32, pressure: 0.3 });
    expect(c.onchange).toHaveBeenCalledTimes(1);
    expect(c.onchange.mock.calls[0][0].note.strokes).toHaveLength(0);
  });

  it('draws nothing and does not erase far from the strokes', async () => {
    const c = await render();
    await c.stroke(along(900, 5200, 9000), { pointerType: 'pen', buttons: 32 });
    expect(c.onchange).not.toHaveBeenCalled();
  });

  it('shows the eraser circle while it hovers, and not for the tip', async () => {
    const c = await render();
    c.fire('pointermove', c.client(3000, 3000), { pointerType: 'pen', buttons: 32 });
    await tick();
    expect(c.svg.querySelector('circle.eraser')).toBeTruthy();
    expect(c.svg.classList.contains('eraser-end')).toBe(true);
    c.fire('pointermove', c.client(3000, 3000), { pointerType: 'pen', buttons: 0 });
    await tick();
    expect(c.svg.querySelector('circle.eraser')).toBeNull();
    expect(c.svg.classList.contains('eraser-end')).toBe(false);
  });

  it('does nothing in a replay, where the page is read-only', async () => {
    const c = await render({ readonly: true, tools: { mode: 'pan' } });
    await c.stroke(along(900, 5200, 3000), { pointerType: 'pen', buttons: 32 });
    expect(c.onchange).not.toHaveBeenCalled();
    c.fire('pointermove', c.client(3000, 3000), { pointerType: 'pen', buttons: 32 });
    await tick();
    expect(c.svg.querySelector('circle.eraser')).toBeNull();
  });
});

describe('the pen barrel button', () => {
  it('draws a lasso that selects, whatever the tool', async () => {
    const c = await render({ tools: { mode: 'pen' } });
    // A loop around the stroke.
    const loop: [number, number][] = [
      [500, 2000],
      [5500, 2000],
      [5500, 4000],
      [500, 4000],
      [500, 2100],
    ];
    await c.stroke(loop, { pointerType: 'pen', buttons: 1 | 2 });
    expect(c.onchange).not.toHaveBeenCalled();
    expect(c.onselect).toHaveBeenCalledTimes(1);
    expect(c.onselect.mock.calls[0][0].selection).toEqual([0]);
  });

  it('also works when the button press arrives as a move, with no pointerdown', async () => {
    const c = await render({ tools: { mode: 'pen' } });
    const loop: [number, number][] = [
      [500, 2000],
      [5500, 2000],
      [5500, 4000],
      [500, 4000],
      [500, 2100],
    ];
    const pen = (t: number) => ({ pointerType: 'pen', buttons: 3, pointerId: 4, t });
    for (const [i, p] of loop.entries()) c.fire('pointermove', c.client(...p), pen(i * 10));
    await tick();
    // The button is released while the pen still hovers: the lasso ends there.
    c.fire('pointermove', c.client(500, 2100), { pointerType: 'pen', buttons: 0, pointerId: 4, t: 100 });
    await tick();
    expect(c.onchange).not.toHaveBeenCalled();
    expect(c.onselect).toHaveBeenCalledTimes(1);
    expect(c.onselect.mock.calls[0][0].selection).toEqual([0]);
    // The next move does not start another one.
    c.fire('pointermove', c.client(900, 2100), { pointerType: 'pen', buttons: 0, pointerId: 4, t: 110 });
    expect(c.onselect).toHaveBeenCalledTimes(1);
  });

  it('is not the mouse right button', async () => {
    const c = await render({ tools: { mode: 'pen' } });
    await c.stroke(along(8000, 12000, 8000), { pointerType: 'mouse', buttons: 2, button: 2 });
    expect(c.onselect).not.toHaveBeenCalled();
    expect(c.onchange).not.toHaveBeenCalled();
  });
});

describe('fingers', () => {
  const finger = (id: number, t = 0): Init => ({ pointerType: 'touch', pointerId: id, buttons: 1, pressure: 1, t });

  it('do not draw by default: one finger pans the page', async () => {
    const c = await render();
    const before = c.view();
    await c.stroke(
      [
        [8000, 8000],
        [9000, 8000],
        [10000, 8000],
        [12000, 8500],
      ],
      finger(7),
    );
    expect(c.onchange).not.toHaveBeenCalled();
    const after = c.view();
    expect(after.scale).toBe(before.scale);
    expect(after.x).toBeGreaterThan(before.x + 20);
  });

  it('pinch with two fingers to zoom, around the fingers', async () => {
    const c = await render();
    const before = c.view();
    const a = { x: 400, y: 300 };
    const b = { x: 500, y: 300 };
    c.fire('pointerdown', a, finger(1, 0));
    c.fire('pointerdown', b, finger(2, 5));
    c.fire('pointermove', { x: 700, y: 300 }, finger(2, 20));
    c.fire('pointermove', { x: 300, y: 300 }, finger(1, 30));
    await tick();
    const after = c.view();
    expect(after.scale).toBeGreaterThan(before.scale * 3);
    c.fire('pointerup', { x: 300, y: 300 }, { ...finger(1, 40), buttons: 0 });
    c.fire('pointerup', { x: 700, y: 300 }, { ...finger(2, 40), buttons: 0 });
    expect(c.onchange).not.toHaveBeenCalled();
  });

  it('fit the page with a double tap', async () => {
    const c = await render();
    const fitted = c.view();
    // Zoom first with a pinch, then double tap.
    c.fire('pointerdown', { x: 400, y: 300 }, finger(1, 0));
    c.fire('pointerdown', { x: 500, y: 300 }, finger(2, 5));
    c.fire('pointermove', { x: 800, y: 300 }, finger(2, 20));
    c.fire('pointerup', { x: 400, y: 300 }, { ...finger(1, 30), buttons: 0 });
    c.fire('pointerup', { x: 800, y: 300 }, { ...finger(2, 30), buttons: 0 });
    await tick();
    expect(c.view().scale).toBeGreaterThan(fitted.scale * 2);
    c.fire('pointerdown', { x: 450, y: 350 }, finger(3, 1000));
    c.fire('pointerup', { x: 450, y: 350 }, { ...finger(3, 1060), buttons: 0 });
    c.fire('pointerdown', { x: 455, y: 352 }, finger(4, 1200));
    c.fire('pointerup', { x: 455, y: 352 }, { ...finger(4, 1250), buttons: 0 });
    await tick();
    expect(c.view().scale).toBeCloseTo(fitted.scale, 6);
  });

  it('are ignored while the pen hovers or touches (palm rejection)', async () => {
    const c = await render();
    const before = c.view();
    c.fire('pointermove', c.client(9000, 9000), { pointerType: 'pen', buttons: 0, pointerId: 5, t: 100 });
    await c.stroke(
      [
        [8000, 8000],
        [10000, 8000],
        [12000, 8500],
      ],
      finger(7, 200),
    );
    expect(c.view()).toEqual(before);
    // Once the pen has been away for a while, fingers work again.
    await c.stroke(
      [
        [8000, 8000],
        [10000, 8000],
        [12000, 8500],
      ],
      finger(8, 200 + PenPresence.HOVER_MS + 10),
    );
    expect(c.view().x).not.toBe(before.x);
  });

  it('is not disturbed by a finger while the pen draws', async () => {
    const c = await render();
    c.fire('pointerdown', c.client(8000, 8000), { pointerType: 'pen', pointerId: 5, pressure: 0.5, t: 0 });
    c.fire('pointermove', c.client(9000, 8000), { pointerType: 'pen', pointerId: 5, pressure: 0.5, t: 10 });
    c.fire('pointerdown', { x: 100, y: 100 }, finger(9, 20));
    c.fire('pointermove', { x: 300, y: 300 }, finger(9, 30));
    c.fire('pointermove', c.client(10000, 8000), { pointerType: 'pen', pointerId: 5, pressure: 0.5, t: 40 });
    c.fire('pointerup', c.client(10000, 8000), { pointerType: 'pen', pointerId: 5, buttons: 0, t: 50 });
    await tick();
    expect(c.onchange).toHaveBeenCalledTimes(1);
    expect(c.onchange.mock.calls[0][0].note.strokes.at(-1).points.length).toBeGreaterThan(2);
  });
});

describe('"Draw with a finger"', () => {
  const finger = (id: number, t = 0): Init => ({ pointerType: 'touch', pointerId: id, buttons: 1, pressure: 1, t });

  it('lets a finger draw, with a constant pressure', async () => {
    touchPrefs.set(true);
    const c = await render();
    await c.stroke(along(8000, 12000, 8000), finger(3));
    expect(c.onchange).toHaveBeenCalledTimes(1);
    const added = c.onchange.mock.calls[0][0].note.strokes.at(-1);
    expect(added.points.length).toBeGreaterThan(3);
    expect(new Set(added.points.map((p: { pressure: number }) => p.pressure))).toEqual(new Set([0.6]));
  });

  it('drops the stroke when a second finger comes down, and pinches instead', async () => {
    touchPrefs.set(true);
    const c = await render();
    const before = c.view();
    c.fire('pointerdown', c.client(8000, 8000), finger(1, 0));
    c.fire('pointermove', c.client(9000, 8000), finger(1, 10));
    c.fire('pointerdown', { x: 600, y: 400 }, finger(2, 20));
    c.fire('pointermove', { x: 900, y: 400 }, finger(2, 30));
    c.fire('pointerup', { x: 900, y: 400 }, { ...finger(2, 40), buttons: 0 });
    c.fire('pointerup', c.client(9000, 8000), { ...finger(1, 40), buttons: 0 });
    await tick();
    expect(c.onchange).not.toHaveBeenCalled();
    expect(c.svg.querySelector('path.live')).toBeNull();
    expect(c.view().scale).not.toBe(before.scale);
  });

  it('still leaves the mouse and the pen as they were', async () => {
    touchPrefs.set(true);
    const c = await render();
    await c.stroke(along(8000, 12000, 8000));
    await c.stroke(along(8000, 12000, 9000), { pointerType: 'pen', pointerId: 2, t: 5000 });
    expect(c.onchange).toHaveBeenCalledTimes(2);
  });

  it('is a button of the zoom bar, remembered on this computer', async () => {
    const c = await render();
    const button = c.target.querySelector<HTMLButtonElement>('button[data-action="finger-draw"]')!;
    expect(button.getAttribute('aria-pressed')).toBe('false');
    button.click();
    await tick();
    expect(touchPrefs.drawWithFinger).toBe(true);
    expect(button.getAttribute('aria-pressed')).toBe('true');
    expect(localStorage.getItem('ardoise.touch.drawWithFinger')).toBe('1');
  });
});

describe('fit the width', () => {
  it('has a button beside "Ajuster" that fills the width with the page', async () => {
    const c = await render();
    const fitted = c.view();
    c.target.querySelector<HTMLButtonElement>('button[data-action="fit-width"]')!.click();
    await tick();
    const wide = c.view();
    // The A4 landscape page in a landscape stage: the width fit is the page fit, never smaller.
    expect(wide.scale).toBeGreaterThanOrEqual(fitted.scale - 1e-9);
    expect(wide.x).toBeCloseTo(24, 3);
  });
});

describe('moving and resizing the selection with the pen and a finger', () => {
  const twoLines = () => select(docFromNote(note([hline(1000, 5000, 3000), hline(1000, 5000, 6000)])), [0, 1]);
  const finger = (id: number, t = 0): Init => ({ pointerType: 'touch', pointerId: id, buttons: 1, pressure: 1, t });
  const movedBy = (changed: Doc) => changed.note.strokes[0].points[0].x - 1000;

  it('moves it with the pen tip in the selection tool', async () => {
    const c = await render({ doc: twoLines(), tools: { mode: 'select' } });
    await c.stroke(
      [
        [3000, 4500],
        [3300, 4500],
        [3800, 4800],
      ],
      { pointerType: 'pen', pointerId: 3 },
    );
    expect(c.onchange).toHaveBeenCalledTimes(1);
    expect(movedBy(c.onchange.mock.calls[0][0])).toBeCloseTo(800, 0);
  });

  it('moves it with a finger even though fingers only navigate, and a finger outside still pans', async () => {
    const c = await render({ doc: twoLines(), tools: { mode: 'select' } });
    const before = c.view();
    await c.stroke(
      [
        [3000, 4500],
        [3300, 4500],
        [3800, 4800],
      ],
      finger(1),
    );
    expect(c.onchange).toHaveBeenCalledTimes(1);
    expect(movedBy(c.onchange.mock.calls[0][0])).toBeCloseTo(800, 0);
    // The page did not move under the finger.
    expect(c.view()).toEqual(before);
    // Off the selection the same finger pans the page, as before.
    await c.stroke(
      [
        [9000, 12000],
        [10000, 12000],
        [11000, 12000],
      ],
      finger(2, 100),
    );
    expect(c.onchange).toHaveBeenCalledTimes(1);
    expect(c.view().x).toBeGreaterThan(before.x + 20);
  });

  it('resizes it from a handle with the pen and with a finger', async () => {
    for (const init of [{ pointerType: 'pen', pointerId: 3 }, finger(1)]) {
      const c = await render({ doc: twoLines(), tools: { mode: 'select' } });
      // The handle at the bottom right of the frame; the anchor is the opposite corner.
      await c.stroke(
        [
          [5030, 6030],
          [7000, 8000],
        ],
        init,
      );
      expect(c.onchange).toHaveBeenCalledTimes(1);
      const strokes = c.onchange.mock.calls[0][0].note.strokes;
      expect(strokes[0].points.at(-1).x).toBeGreaterThan(5500);
      cleanup?.();
    }
  });

  it('moves a selection made with the barrel button while the tool is still the pen, and draws elsewhere', async () => {
    const onchange = vi.fn();
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(CanvasHarness, {
      target,
      props: {
        initial: docFromNote(note([hline(1000, 5000, 3000), hline(1000, 5000, 6000)])),
        tools: { ...DEFAULT_TOOLS, mode: 'pen' },
        ink,
        onchange,
      },
    });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await tick();
    const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
    const [scale, vx, vy] = svg.getAttribute('data-view')!.split(' ').map(Number);
    const at = (x: number, y: number) => ({ x: x * scale + vx, y: y * scale + vy });
    const fire = (type: string, p: { x: number; y: number }, init: Init) => {
      const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: p.x, clientY: p.y, button: 0, buttons: init.buttons ?? 1 });
      for (const [key, value] of Object.entries({ pointerType: init.pointerType ?? 'pen', pointerId: init.pointerId ?? 3, pressure: 0.5, timeStamp: init.t ?? 0 })) {
        Object.defineProperty(event, key, { value, configurable: true });
      }
      svg.dispatchEvent(event);
    };
    const drag = async (points: [number, number][], init: Init) => {
      fire('pointerdown', at(...points[0]), init);
      for (const p of points.slice(1)) fire('pointermove', at(...p), init);
      fire('pointerup', at(...points[points.length - 1]), { ...init, buttons: 0 });
      await tick();
    };
    // The barrel-button lasso around both lines.
    await drag(
      [
        [500, 2000],
        [5500, 2000],
        [5500, 7000],
        [500, 7000],
        [500, 2100],
      ],
      { buttons: 3 },
    );
    expect(onchange).not.toHaveBeenCalled();
    expect(target.querySelectorAll('rect.handle').length).toBe(4);
    // The tip inside the frame moves the ink instead of drawing.
    await drag(
      [
        [3000, 4500],
        [3300, 4500],
        [3800, 4800],
      ],
      { t: 100 },
    );
    expect(onchange).toHaveBeenCalledTimes(1);
    const moved = onchange.mock.calls[0][0] as Doc;
    expect(moved.note.strokes).toHaveLength(2);
    expect(moved.note.strokes[0].points[0].x - 1000).toBeCloseTo(800, 0);
    // Away from the frame the tip draws again (and the selection goes).
    await drag(
      [
        [12000, 12000],
        [13000, 12500],
        [14000, 12000],
      ],
      { t: 200 },
    );
    expect(onchange).toHaveBeenCalledTimes(2);
    expect((onchange.mock.calls[1][0] as Doc).note.strokes).toHaveLength(3);
    expect(target.querySelectorAll('rect.handle').length).toBe(0);
  });

  it('keeps the gesture when a child of the canvas loses an implicit pointer capture', async () => {
    const c = await render({ doc: twoLines(), tools: { mode: 'select' } });
    const finger1 = finger(1);
    c.fire('pointerdown', c.client(3000, 4500), finger1);
    c.fire('pointermove', c.client(3800, 4800), { ...finger1, t: 10 });
    // A finger pressed on ink captures the pointer implicitly on that stroke; Chrome releases it
    // when the canvas captures it explicitly: that is not the end of the gesture.
    const path = c.svg.querySelector('path')!;
    path.dispatchEvent(new Event('lostpointercapture', { bubbles: true }));
    c.fire('pointerup', c.client(3800, 4800), { ...finger1, buttons: 0, t: 20 });
    await tick();
    expect(c.onchange).toHaveBeenCalledTimes(1);
  });
});
