// @vitest-environment jsdom
/// <reference types="node" />
// M14: the lasso, the click, the barrel button's lasso and the eraser (tool and back of the pen) act on the chosen layer only.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import { DEFAULT_TOOLS, docFromNote, setActiveLayer, setLayerVisible, type Doc, type ToolSettings } from '../../lib/editor/state';
import { hline, note } from '../../lib/editor/test-fixtures';
import { instantiateInk, type Ink } from '../../lib/ink';
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
});

/**
 * Strokes 0 and 2 are on layer 0, stroke 1 on layer 1 on top of stroke 0 (the same line, y = 3000);
 * stroke 2 is alone at y = 6000. The chosen layer is the top one unless `doc` says otherwise.
 */
const crossing = (): Doc =>
  docFromNote(note([hline(1000, 5000, 3000, 100, 0), hline(1000, 5000, 3000, 100, 1), hline(1000, 5000, 6000, 100, 0)], 2));

const around: [number, number][] = [
  [500, 2000],
  [5500, 2000],
  [5500, 7000],
  [500, 7000],
  [500, 2100],
];

async function render(doc: Doc, tools: Partial<ToolSettings> = {}) {
  const onchange = vi.fn();
  const onselect = vi.fn();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Canvas, {
    target,
    props: { doc, tools: { ...DEFAULT_TOOLS, ...tools }, ink, onchange, onselect },
  });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const client = (px: number, py: number) => {
    const [scale, x, y] = svg.getAttribute('data-view')!.split(' ').map(Number);
    return { x: px * scale + x, y: py * scale + y };
  };
  const fire = (type: string, at: { x: number; y: number }, init: { pointerType?: string; buttons?: number; t?: number } = {}) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: 0, buttons: init.buttons ?? 1 });
    const props = { pointerType: init.pointerType ?? 'mouse', pointerId: 1, pressure: 0.5, timeStamp: init.t ?? 0 };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    svg.dispatchEvent(event);
  };
  const stroke = async (points: [number, number][], init: { pointerType?: string; buttons?: number } = {}) => {
    const [first, ...rest] = points;
    let t = 0;
    fire('pointerdown', client(...first), { ...init, t });
    for (const p of rest) fire('pointermove', client(...p), { ...init, t: (t += 10) });
    fire('pointerup', client(...points[points.length - 1]), { ...init, buttons: 0, t: (t += 10) });
    await tick();
  };
  return { stroke, onchange, onselect };
}

const across = (y: number): [number, number][] => Array.from({ length: 9 }, (_, i) => [900 + (4300 * i) / 8, y]);

describe('the lasso takes the chosen layer only', () => {
  it('the top layer', async () => {
    const c = await render(crossing(), { mode: 'select' });
    await c.stroke(around);
    expect(c.onselect).toHaveBeenCalledTimes(1);
    expect(c.onselect.mock.calls[0][0].selection).toEqual([1]);
  });

  it('the bottom layer, although another one is drawn over its stroke', async () => {
    const c = await render(setActiveLayer(crossing(), 0), { mode: 'select' });
    await c.stroke(around);
    expect(c.onselect.mock.calls[0][0].selection).toEqual([0, 2]);
  });

  it('nothing while the chosen layer is hidden', async () => {
    const c = await render(setLayerVisible(crossing(), 1, false), { mode: 'select' });
    await c.stroke(around);
    expect(c.onselect.mock.calls[0][0].selection).toEqual([]);
  });
});

describe('a click takes the chosen layer’s stroke under it', () => {
  it('the chosen layer’s, not the topmost of the page', async () => {
    const c = await render(setActiveLayer(crossing(), 0), { mode: 'select' });
    await c.stroke([[3000, 3000]]);
    expect(c.onselect.mock.calls[0][0].selection).toEqual([0]);
  });

  it('nothing where only another layer has ink', async () => {
    const doc = crossing();
    // Layer 1 has no stroke at y = 6000: its click there finds nothing, though layer 0 has one.
    const c = await render(doc, { mode: 'select' });
    await c.stroke([[3000, 6000]]);
    expect(c.onselect.mock.calls[0][0].selection).toEqual([]);
  });
});

describe('the barrel button’s lasso takes the chosen layer only', () => {
  it('whatever the tool', async () => {
    const c = await render(setActiveLayer(crossing(), 0), { mode: 'pen' });
    await c.stroke(around, { pointerType: 'pen', buttons: 1 | 2 });
    expect(c.onchange).not.toHaveBeenCalled();
    expect(c.onselect.mock.calls[0][0].selection).toEqual([0, 2]);
  });
});

describe('the eraser takes the chosen layer only', () => {
  it('the back of the pen, whole', async () => {
    const c = await render(crossing(), { mode: 'pen', eraser: 'whole', eraserRadius: 300 });
    await c.stroke(across(3000), { pointerType: 'pen', buttons: 32 });
    expect(c.onchange).toHaveBeenCalledTimes(1);
    const strokes = c.onchange.mock.calls[0][0].note.strokes;
    expect(strokes.map((s: { layer: number }) => s.layer)).toEqual([0, 0]);
  });

  it('the back of the pen, partial', async () => {
    const c = await render(setActiveLayer(crossing(), 0), { mode: 'pen', eraser: 'partial', eraserRadius: 300 });
    await c.stroke([[3000, 2000], [3000, 4000]], { pointerType: 'pen', buttons: 32 });
    const strokes = c.onchange.mock.calls[0][0].note.strokes as { layer: number }[];
    // The layer-1 stroke is whole; the layer-0 one is cut in two where the eraser went.
    expect(strokes.filter((s) => s.layer === 1)).toHaveLength(1);
    expect(strokes.filter((s) => s.layer === 0)).toHaveLength(3);
  });

  it('the eraser tool', async () => {
    const c = await render(setActiveLayer(crossing(), 0), { mode: 'eraser', eraser: 'whole', eraserRadius: 300 });
    await c.stroke(across(3000));
    const strokes = c.onchange.mock.calls[0][0].note.strokes as { layer: number }[];
    expect(strokes.map((s) => s.layer)).toEqual([1, 0]);
  });

  it('nothing while the chosen layer is hidden', async () => {
    const c = await render(setLayerVisible(crossing(), 1, false), { mode: 'eraser', eraser: 'whole', eraserRadius: 300 });
    await c.stroke(across(3000));
    expect(c.onchange).not.toHaveBeenCalled();
  });
});
