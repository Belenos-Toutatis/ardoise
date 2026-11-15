// @vitest-environment jsdom
/// <reference types="node" />
// The selection tool's two ways (`SelectWay`, docs/questions.md Q22): a rectangle (a drag from one corner to the opposite one)
// and a lasso (a free outline). The rectangle takes what a lasso along its edge would: the same rule for strokes (at least
// half of their samples inside) and for page objects (their box centre), so the same things for the same area. Shift adds,
// Ctrl removes, a click takes what is under it, a click on the empty page clears, inside the selection's frame a drag moves
// it, and the pen's barrel button draws a lasso whatever the way.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeAll, describe, expect, it, vi } from 'vitest';
import type { Vec } from '../../lib/editor/geometry';
import { DEFAULT_TOOLS, docFromNote, objectSelection, select, type Doc, type SelectWay, type ToolSettings } from '../../lib/editor/state';
import { hline, line, note } from '../../lib/editor/test-fixtures';
import { instantiateInk, type Ink } from '../../lib/ink';
import type { BookmarkObject, Note, PageObject, TextObject } from '../../lib/note';
import { selectPrefs } from '../../lib/select-prefs.svelte';
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
  selectPrefs.reset();
});

const BLACK = { r: 0, g: 0, b: 0, a: 255 };

function frame(init: Partial<PageObject>): PageObject {
  return { kind: 'object', id: 'o', layer: 0, t: 0, x: 0, y: 0, scale: 1, quarterTurns: 0, width: 1000, height: 600, ...init };
}

const text = (id: string, x: number, y: number): TextObject => ({
  ...frame({ id, x, y }),
  kind: 'text',
  text: 'Abc',
  font: 'andika',
  bold: false,
  size: 600,
  color: BLACK,
});

const bookmark = (id: string, x: number, y: number): BookmarkObject => ({ ...frame({ id, x, y, width: 800, height: 400 }), kind: 'bookmark', label: id });

/**
 * Strokes 0 to 2 are horizontal lines at y = 2000, 4000 and 6000; stroke 3 is a bent one at the bottom. A text frame (0) and a bookmark (1)
 * stand in the middle of the page. Everything is on layer 0, the chosen one.
 */
function page(): Doc {
  const n: Note = {
    ...note([
      hline(1000, 3000, 2000), // 0: whole inside the areas below
      hline(1000, 9000, 4000), // 1: a fifth of its samples in the first area
      hline(2000, 6000, 6000), // 2: half of them (the edge case of the rule)
      line([[1500, 8000], [2500, 9000], [3500, 8000], [4500, 9000]]), // 3
    ]),
    objects: [text('frame', 1000, 10_000), bookmark('mark', 5000, 10_000)],
  };
  return docFromNote(n);
}

interface Held {
  shiftKey?: boolean;
  ctrlKey?: boolean;
}

async function render(doc: Doc, way: SelectWay | undefined, extra: Partial<ToolSettings> = {}, readonly = false) {
  const onchange = vi.fn<(doc: Doc) => void>();
  const onselect = vi.fn<(doc: Doc) => void>();
  const target = document.createElement('div');
  document.body.append(target);
  // The way is the computer's preference, shared with live mode (nothing chosen: the first-time rectangle).
  if (way) selectPrefs.set(way);
  else selectPrefs.reset();
  const tools: ToolSettings = { ...DEFAULT_TOOLS, mode: 'select', ...extra };
  const component = mount(Canvas, { target, props: { doc, tools, ink, readonly, onchange, onselect } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await tick();
  const svg = target.querySelector<SVGSVGElement>('svg.canvas')!;
  const client = (p: Vec | [number, number]) => {
    const [x, y] = Array.isArray(p) ? p : [p.x, p.y];
    const [scale, vx, vy] = svg.getAttribute('data-view')!.split(' ').map(Number);
    return { x: x * scale + vx, y: y * scale + vy };
  };
  const fire = (type: string, at: { x: number; y: number }, init: Held & { pointerType?: string; buttons?: number } = {}) => {
    const event = new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      clientX: at.x,
      clientY: at.y,
      button: 0,
      buttons: init.buttons ?? (type === 'pointerup' ? 0 : 1),
      shiftKey: init.shiftKey,
      ctrlKey: init.ctrlKey,
    });
    for (const [key, value] of Object.entries({ pointerType: init.pointerType ?? 'mouse', pointerId: 1, pressure: 0.5, timeStamp: 0 })) {
      Object.defineProperty(event, key, { value, configurable: true });
    }
    svg.dispatchEvent(event);
  };
  /** A drag along `points` (page units), with the keys held; `until` is called with the last move done and the button still down. */
  const drag = async (points: [number, number][], held: Held & { pointerType?: string; buttons?: number } = {}, until?: () => void) => {
    const [first, ...rest] = points;
    fire('pointerdown', client(first), held);
    for (const p of rest) fire('pointermove', client(p), held);
    // The page draws the gesture at the next microtask.
    await tick();
    until?.();
    fire('pointerup', client(points[points.length - 1]), { ...held, buttons: 0 });
    await tick();
  };
  const last = () => onselect.mock.calls[onselect.mock.calls.length - 1][0];
  return { svg, drag, onchange, onselect, last, client, fire };
}

/** The rectangle from (x0, y0) to (x1, y1) as a drag, and the same area as a lasso going round it (and back to its start). */
const box = (x0: number, y0: number, x1: number, y1: number): [number, number][] => [
  [x0, y0],
  [x1, y1],
];
const outline = (x0: number, y0: number, x1: number, y1: number): [number, number][] => [
  [x0, y0],
  [x1, y0],
  [x1, y1],
  [x0, y1],
  [x0 + 1, y0 + 1],
];

describe('the rectangle', () => {
  it('takes the strokes with at least half of their samples inside, and the objects whose box centre is inside', async () => {
    const c = await render(page(), 'rectangle');
    // Strokes 0 and 3 (all their samples inside) and 2 (27 of 41), not stroke 1 (37 of 81, just under the half); the text
    // frame's centre (1500, 10300) is inside, the bookmark's (5400, 10200) is not.
    await c.drag(box(500, 1500, 4600, 10_800));
    const doc = c.last();
    expect(doc.selection).toEqual([0, 2, 3]);
    expect(objectSelection(doc)).toEqual([0]);
  });

  it('takes the same strokes and objects as a lasso round the same area, whatever the area', async () => {
    const areas: [number, number, number, number][] = [
      [500, 1500, 4600, 10_800],
      [500, 3500, 9500, 6500],
      [3000, 3500, 9500, 10_800],
      [500, 500, 20_000, 12_000],
      [4000, 9000, 6500, 10_800],
      [500, 7500, 3000, 9500],
    ];
    for (const area of areas) {
      const rectangle = await render(page(), 'rectangle');
      await rectangle.drag(box(...area));
      const rectDoc = rectangle.last();
      cleanup?.();
      const lasso = await render(page(), 'lasso');
      await lasso.drag(outline(...area));
      const lassoDoc = lasso.last();
      cleanup?.();
      expect([rectDoc.selection, objectSelection(rectDoc)], `area ${area}`).toEqual([lassoDoc.selection, objectSelection(lassoDoc)]);
    }
  });

  it('is the same drawn from the bottom right corner to the top left one', async () => {
    const c = await render(page(), 'rectangle');
    await c.drag(box(4600, 10_800, 500, 1500));
    expect(c.last().selection).toEqual([0, 2, 3]);
    expect(objectSelection(c.last())).toEqual([0]);
  });

  it('is the way before any choice', async () => {
    const c = await render(page(), undefined);
    await c.drag(box(500, 1500, 4600, 10_800));
    expect(c.last().selection).toEqual([0, 2, 3]);
  });

  it('is drawn dashed like the lasso while the drag lasts, and gone when it ends', async () => {
    const c = await render(page(), 'rectangle');
    let seen: { x: number; y: number; width: number; height: number; removing: boolean } | null = null;
    await c.drag(box(500, 1500, 4600, 10_800), {}, () => {
      const r = c.svg.querySelector('rect.marquee');
      const at = (name: string) => Math.round(Number(r?.getAttribute(name)));
      seen = r && { x: at('x'), y: at('y'), width: at('width'), height: at('height'), removing: r.classList.contains('removing') };
      expect(r?.classList.contains('lasso')).toBe(true);
      // No lasso outline at the same time.
      expect(c.svg.querySelector('polyline.lasso')).toBeNull();
    });
    expect(seen).toEqual({ x: 500, y: 1500, width: 4100, height: 9300, removing: false });
    expect(c.svg.querySelector('rect.marquee')).toBeNull();
  });

  it('is drawn in the danger colour with Ctrl, which removes', async () => {
    const c = await render(select(page(), [0], []), 'rectangle');
    await c.drag(box(500, 1500, 4600, 10_800), { ctrlKey: true }, () => {
      expect(c.svg.querySelector('rect.marquee')?.classList.contains('removing')).toBe(true);
    });
  });

  it('adds to the selection with Shift, even started on the selection\'s frame', async () => {
    const c = await render(select(page(), [0], []), 'rectangle');
    // The press is inside the frame of stroke 0: a rectangle that adds, not a move.
    await c.drag(box(1500, 1500, 9500, 6500), { shiftKey: true });
    expect(c.onchange).not.toHaveBeenCalled();
    expect(c.last().selection).toEqual([0, 1, 2]);
  });

  it('removes from the selection with Ctrl, even started on the selection\'s frame', async () => {
    const c = await render(select(page(), [0, 1, 2], [0]), 'rectangle');
    await c.drag(box(1500, 1500, 9500, 4500), { ctrlKey: true });
    expect(c.onchange).not.toHaveBeenCalled();
    expect(c.last().selection).toEqual([2]);
    expect(objectSelection(c.last())).toEqual([0]);
  });

  it('replaces the selection without a key', async () => {
    // The press is outside the frame (which holds stroke 0 and the text frame).
    const c = await render(select(page(), [0], []), 'rectangle');
    await c.drag(box(500, 5800, 9500, 6300));
    expect(c.last().selection).toEqual([2]);
  });

  it('takes what is under a click, a text frame before the strokes', async () => {
    const c = await render(page(), 'rectangle');
    await c.drag([[2000, 2000]]);
    expect(c.last().selection).toEqual([0]);
    cleanup?.();
    const f = await render(page(), 'rectangle');
    await f.drag([[1500, 10_300]]);
    expect(objectSelection(f.last())).toEqual([0]);
    expect(f.last().selection).toEqual([]);
    expect(f.svg.querySelector('rect.marquee')).toBeNull();
  });

  it('clears the selection with a click on the empty page, and keeps it with Shift', async () => {
    const c = await render(select(page(), [0], []), 'rectangle');
    await c.drag([[8000, 8000]]);
    expect(c.last().selection).toEqual([]);
    cleanup?.();
    const k = await render(select(page(), [0], []), 'rectangle');
    await k.drag([[8000, 8000]], { shiftKey: true });
    expect(k.last().selection).toEqual([0]);
  });

  it('does not select when it is drawn flat (a line is no area)', async () => {
    const c = await render(page(), 'rectangle');
    await c.drag(box(500, 2000, 4600, 2000));
    expect(c.last().selection).toEqual([]);
  });

  it('leaves a drag started on the selection\'s frame to move it, as with the lasso', async () => {
    for (const way of ['rectangle', 'lasso'] as const) {
      const c = await render(select(page(), [0, 1], []), way);
      // Inside the frame, on its middle.
      await c.drag([[2000, 3000], [2600, 3400]]);
      expect(c.onselect, way).not.toHaveBeenCalled();
      expect(c.onchange, way).toHaveBeenCalledTimes(1);
      const moved = c.onchange.mock.calls[0][0];
      expect(moved.note.strokes[0].points[0].x, way).toBeGreaterThan(1000);
      cleanup?.();
    }
  });

  it('is never drawn by the pen\'s barrel button: that one draws a lasso whatever the tool or the way', async () => {
    for (const mode of ['select', 'pen'] as const) {
      const c = await render(page(), 'rectangle', { mode });
      let during: { lasso: boolean; marquee: boolean } | null = null;
      await c.drag(outline(500, 1500, 4600, 10_800), { pointerType: 'pen', buttons: 1 | 2 }, () => {
        during = { lasso: c.svg.querySelector('polyline.lasso') !== null, marquee: c.svg.querySelector('rect.marquee') !== null };
      });
      expect(during, mode).toEqual({ lasso: true, marquee: false });
      expect(c.last().selection, mode).toEqual([0, 2, 3]);
      cleanup?.();
    }
  });

  it('is drawn by the pen\'s tip and by the mouse alike', async () => {
    const c = await render(page(), 'rectangle');
    await c.drag(box(500, 1500, 4600, 10_800), { pointerType: 'pen' });
    expect(c.last().selection).toEqual([0, 2, 3]);
  });

  it('is not drawn in a replay (the tool is the pan there)', async () => {
    const c = await render(page(), 'rectangle', { mode: 'pan' }, true);
    let during = true;
    await c.drag(box(500, 1500, 4600, 10_800), {}, () => (during = c.svg.querySelector('rect.marquee') !== null));
    expect(during).toBe(false);
    expect(c.onselect).not.toHaveBeenCalled();
  });
});

describe('the lasso', () => {
  it('keeps drawing its outline, as before', async () => {
    const c = await render(page(), 'lasso');
    let during: { lasso: boolean; marquee: boolean } | null = null;
    await c.drag(outline(500, 1500, 4600, 10_800), {}, () => {
      during = { lasso: c.svg.querySelector('polyline.lasso') !== null, marquee: c.svg.querySelector('rect.marquee') !== null };
    });
    expect(during).toEqual({ lasso: true, marquee: false });
    expect(c.last().selection).toEqual([0, 2, 3]);
    expect(objectSelection(c.last())).toEqual([0]);
    expect(c.svg.querySelector('polyline.lasso')).toBeNull();
  });
});
