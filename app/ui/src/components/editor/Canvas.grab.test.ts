// @vitest-environment jsdom
/// <reference types="node" />
// SPEC §4.2 (the live control window takes hold of the selection the same way): in the editor too, how near a press
// must be to the selection's frame or to one of its handles depends on the pointer: the pen and the finger reach farther
// than the mouse (`lib/selection-grab.ts`). The behaviour is otherwise the one of `Canvas.pen-touch.test.ts`.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { DEFAULT_TOOLS, docFromNote, select, type Doc } from '../../lib/editor/state';
import { hline, note } from '../../lib/editor/test-fixtures';
import { instantiateInk, type Ink } from '../../lib/ink';
import { touchPrefs } from '../../lib/touch-prefs.svelte';
import Canvas from './Canvas.svelte';

let ink: Ink;
beforeAll(async () => {
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
});

let cleanup: (() => void) | null = null;
afterEach(() => {
  cleanup?.();
  cleanup = null;
  touchPrefs.set(false);
});

interface Init {
  pointerType?: string;
  pointerId?: number;
  t?: number;
}

/** Two lines of 4000 units, 3000 apart, both selected, in the selection tool. */
async function render() {
  const doc: Doc = select(docFromNote(note([hline(1000, 5000, 3000), hline(1000, 5000, 6000)])), [0, 1]);
  const onchange = vi.fn<(doc: Doc) => void>();
  const onselect = vi.fn();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Canvas, { target, props: { doc, tools: { ...DEFAULT_TOOLS, mode: 'select' }, ink, readonly: false, onchange, onselect } });
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
  const client = (px: number, py: number) => {
    const v = view();
    return { x: px * v.scale + v.x, y: py * v.scale + v.y };
  };
  /** The selection's frame on the screen, in client pixels. */
  const frame = () => {
    const rect = svg.querySelector<SVGRectElement>('rect.selection')!;
    const [x, y, w, h] = ['x', 'y', 'width', 'height'].map((name) => Number(rect.getAttribute(name)));
    const a = client(x, y);
    const b = client(x + w, y + h);
    return { l: a.x, t: a.y, r: b.x, b: b.y, cy: (a.y + b.y) / 2 };
  };
  const fire = (type: string, p: { x: number; y: number }, init: Init = {}) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: p.x, clientY: p.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    const props: Record<string, unknown> = { pointerType: init.pointerType ?? 'mouse', pointerId: init.pointerId ?? 1, pressure: 0.5, timeStamp: init.t ?? 0 };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    svg.dispatchEvent(event);
  };
  const drag = async (from: { x: number; y: number }, by: { x: number; y: number }, init: Init = {}) => {
    fire('pointerdown', from, init);
    fire('pointermove', { x: from.x + by.x / 2, y: from.y + by.y / 2 }, { ...init, t: (init.t ?? 0) + 10 });
    fire('pointermove', { x: from.x + by.x, y: from.y + by.y }, { ...init, t: (init.t ?? 0) + 20 });
    fire('pointerup', { x: from.x + by.x, y: from.y + by.y }, { ...init, t: (init.t ?? 0) + 30 });
    await tick();
  };
  return { view, frame, drag, onchange, onselect };
}

const pen: Init = { pointerType: 'pen', pointerId: 3 };
const finger = (t = 0): Init => ({ pointerType: 'touch', pointerId: 4, t });

describe('how near a press must be to the frame of the selection', () => {
  it('a pen 10 pixels outside the frame moves the selection, a mouse there draws a lasso instead', async () => {
    const withPen = await render();
    const { l, cy } = withPen.frame();
    await withPen.drag({ x: l - 10, y: cy }, { x: 60, y: 0 }, pen);
    expect(withPen.onchange).toHaveBeenCalledTimes(1);
    const moved = withPen.onchange.mock.calls[0][0].note.strokes[0];
    expect(moved.points[0].x - 1000).toBeCloseTo(60 / withPen.view().scale, 0);
    expect(withPen.onselect).not.toHaveBeenCalled();
    cleanup?.();

    const withMouse = await render();
    await withMouse.drag({ x: l - 10, y: cy }, { x: 60, y: 0 });
    expect(withMouse.onchange).not.toHaveBeenCalled();
    expect(withMouse.onselect).toHaveBeenCalledTimes(1);
  });

  it('the reach is 4 pixels for the mouse, 12 for the pen, 16 for a finger', async () => {
    const takes = async (init: Init, outside: number) => {
      const c = await render();
      const { l, cy } = c.frame();
      await c.drag({ x: l - outside, y: cy }, { x: 60, y: 0 }, init);
      const moved = c.onchange.mock.calls.length === 1;
      cleanup?.();
      return moved;
    };
    expect(await takes({}, 3)).toBe(true);
    expect(await takes({}, 6)).toBe(false);
    expect(await takes(pen, 11)).toBe(true);
    expect(await takes(pen, 14)).toBe(false);
    expect(await takes(finger(), 15)).toBe(true);
    expect(await takes(finger(), 20)).toBe(false);
  });

  it('a finger that does not reach the frame still pans the page', async () => {
    const c = await render();
    const before = c.view();
    const { l, cy } = c.frame();
    await c.drag({ x: l - 30, y: cy }, { x: 60, y: 0 }, finger());
    expect(c.onchange).not.toHaveBeenCalled();
    expect(c.view().x).toBeGreaterThan(before.x + 20);
  });

  it('a handle is within 8 pixels for the mouse, 16 for the pen, 22 for a finger (inside the corner)', async () => {
    const resizes = async (init: Init, inside: number) => {
      const c = await render();
      const { r, b } = c.frame();
      await c.drag({ x: r - inside, y: b - inside }, { x: 40, y: 40 }, init);
      const strokes = c.onchange.mock.calls[0]?.[0].note.strokes;
      cleanup?.();
      // A resize changes the width, a move does not.
      return strokes ? strokes[0].width !== hline(0, 1, 0).width : null;
    };
    expect(await resizes({}, 7)).toBe(true);
    expect(await resizes({}, 12)).toBe(false);
    expect(await resizes(pen, 15)).toBe(true);
    expect(await resizes(pen, 20)).toBe(false);
    expect(await resizes(finger(), 21)).toBe(true);
    expect(await resizes(finger(), 30)).toBe(false);
  });
});
