// @vitest-environment jsdom
/// <reference types="node" />
// SPEC §4.2 « Retoucher pendant le direct »: moving and resizing the selection in the control window's preview, with
// the pen, a finger or the mouse. A press on a corner handle resizes around the opposite corner, a press inside the
// frame (or within the pointer's margin around it) moves, anywhere else the mouse and the pen draw a lasso and a
// finger navigates; Shift, Ctrl or Meta always give a lasso. The edit is asked for at the release; the strokes are
// drawn lifted meanwhile, and stay where the drag left them until the session's event is applied or the call fails.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { mount, tick, unmount, type ComponentProps } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { instantiateInk, type Ink } from '../../lib/ink';
import type { LiveBatch, LiveSnapshot, LiveViewState } from '../../lib/live';
import { FakeContext } from '../../lib/live-fake-canvas';
import type { Combine } from '../../lib/live-select.svelte';
import { LiveState } from '../../lib/live-state.svelte';
import { fitTransform, pageToScreen } from '../../lib/live-view';
import type { Layer, Note, Stroke } from '../../lib/note';
import { HANDLE_SIZE_PX, MAX_SCALE, MIN_SCALE, type PageTransform } from '../../lib/selection-grab';
import LiveCanvas from './LiveCanvas.svelte';

const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const A4 = { width: 29700, height: 21600 };
const LAYERS: Layer[] = [
  { name: 'Calque 1', visible: true },
  { name: 'Calque 2', visible: false },
];
const TRANSFORM = fitTransform(A4, { width: 800, height: 560 }, 12);
const UNIT = TRANSFORM.scale;
/** Where a page point is on the preview (the jsdom canvas has no offset). */
const at = (x: number, y: number) => pageToScreen(TRANSFORM, { x, y });

let cleanup: (() => void) | null = null;
let ink: Ink;
let contexts = new Map<HTMLCanvasElement, FakeContext>();

beforeEach(async () => {
  ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  contexts = new Map();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(function (this: HTMLCanvasElement) {
    let ctx = contexts.get(this);
    if (!ctx) {
      ctx = new FakeContext();
      contexts.set(this, ctx);
    }
    return ctx as unknown as RenderingContext;
  });
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
  vi.useRealTimers();
});

/** A rectangle drawn as a polyline, `width` × `height` page units from (`x`, `y`): its ink frame is 30 units wider on each side. */
function block(x: number, y: number, width = 6000, height = 4000, layer = 0): Stroke {
  const corners = [
    [x, y],
    [x + width, y],
    [x + width, y + height],
    [x, y + height],
    [x, y],
  ];
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer,
    points: corners.map(([px, py], i) => ({ x: px, y: py, pressure: 0.6, t: 5 * i })),
  };
}

function snapshot(strokes: Stroke[], pageId = 'p1'): LiveSnapshot {
  const page: Note = {
    id: pageId,
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
    revision: 5,
  };
}

interface Init {
  button?: number;
  pointerType?: string;
  buttons?: number;
  pointerId?: number;
  t?: number;
  shiftKey?: boolean;
  ctrlKey?: boolean;
  metaKey?: boolean;
}

/** The first block's frame (page units, as the selection draws it) and its corners on the screen. */
const FRAME = { x: 3000 - 30, y: 3000 - 30, width: 6060, height: 4060 };
const SCREEN = (() => {
  const a = at(FRAME.x, FRAME.y);
  const b = at(FRAME.x + FRAME.width, FRAME.y + FRAME.height);
  return { l: a.x, t: a.y, r: b.x, b: b.y, cx: (a.x + b.x) / 2, cy: (a.y + b.y) / 2 };
})();

interface Options {
  selection?: readonly number[];
  strokes?: Stroke[];
  ontransform?: ((transform: PageTransform) => Promise<boolean> | boolean) | null;
  tool?: 'none' | 'select' | 'pen';
}

async function render(options: Options = {}) {
  const live = new LiveState();
  live.initial(snapshot(options.strokes ?? [block(3000, 3000), block(12000, 3000), block(3000, 12000, 6000, 4000, 1)]));
  const target = document.createElement('div');
  document.body.append(target);
  const onview = vi.fn<(view: LiveViewState) => void>();
  const onselect = vi.fn<(picked: number[], combine: Combine) => void>();
  const ontransform = vi.fn<(transform: PageTransform) => Promise<boolean> | boolean>(options.ontransform ?? (async () => true));
  const props: ComponentProps<typeof LiveCanvas> = {
    live,
    ink,
    mode: 'preview',
    label: 'page',
    onview,
    onselect,
    tool: options.tool ?? 'select',
    selection: options.selection ?? [0],
  };
  if (options.ontransform !== null) props.ontransform = ontransform;
  const component = mount(LiveCanvas, { target, props });
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
      button: init.button ?? 0,
      buttons: type === 'pointerup' ? 0 : (init.buttons ?? 1),
      shiftKey: init.shiftKey,
      ctrlKey: init.ctrlKey,
      metaKey: init.metaKey,
    });
    const props: Record<string, unknown> = {
      pointerType: init.pointerType ?? 'mouse',
      pointerId: init.pointerId ?? 1,
      pressure: 0.5,
      timeStamp: init.t ?? 0,
    };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    canvas.dispatchEvent(event);
    return event;
  };
  /** A drag from `from` to `to`, in a few moves, ending with the release. */
  const drag = (from: { x: number; y: number }, to: { x: number; y: number }, init: Init = {}) => {
    fire('pointerdown', from, init);
    for (const k of [0.25, 0.5, 0.75, 1]) fire('pointermove', { x: from.x + (to.x - from.x) * k, y: from.y + (to.y - from.y) * k }, init);
    fire('pointerup', to, init);
  };
  const ctx = () => canvas.getContext('2d') as unknown as FakeContext;
  /** Paints the state as a frame would, and tells what that frame drew on the canvas. */
  const frame = () => {
    ctx().reset();
    (canvas as unknown as { repaintNow(): void }).repaintNow();
    const images = ctx().ops('drawImage');
    return {
      /** The strokes were copied from their surface, moved and scaled (`drawImage` with a size). */
      lifted: images.filter((c) => c.args[3] !== undefined).map((c) => c.args as number[]),
      handles: ctx().ops('fillRect').filter((c) => (c.args as number[])[2] === HANDLE_SIZE_PX).length,
    };
  };
  return { live, canvas, fire, drag, frame, onview, onselect, ontransform, target };
}

/** Lets the answer of a call reach the canvas (a refused one puts the strokes back). */
const settle = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

/** The page units a drag of `px` screen pixels is. */
const units = (px: number) => px / UNIT;

describe('moving the selection', () => {
  it('a mouse drag inside the frame moves it: the edit is a translation in page units, and no lasso starts', async () => {
    const view = await render();
    view.drag({ x: SCREEN.cx, y: SCREEN.cy }, { x: SCREEN.cx + 60, y: SCREEN.cy - 30 });
    expect(view.ontransform).toHaveBeenCalledOnce();
    const sent = view.ontransform.mock.calls[0][0];
    expect(sent.scale).toBe(1);
    expect(sent.x).toBeCloseTo(units(60), 6);
    expect(sent.y).toBeCloseTo(units(-30), 6);
    expect(view.onselect).not.toHaveBeenCalled();
    expect(view.onview).not.toHaveBeenCalled();
  });

  it('the pen moves it too, and a finger: it does not navigate then', async () => {
    const pen = await render();
    pen.drag({ x: SCREEN.cx, y: SCREEN.cy }, { x: SCREEN.cx - 40, y: SCREEN.cy + 25 }, { pointerType: 'pen', pointerId: 7 });
    expect(pen.ontransform).toHaveBeenCalledOnce();
    expect(pen.ontransform.mock.calls[0][0].x).toBeCloseTo(units(-40), 6);
    expect(pen.ontransform.mock.calls[0][0].y).toBeCloseTo(units(25), 6);
    expect(pen.onselect).not.toHaveBeenCalled();
    cleanup?.();

    const finger = await render();
    finger.drag({ x: SCREEN.cx, y: SCREEN.cy }, { x: SCREEN.cx + 50, y: SCREEN.cy + 50 }, { pointerType: 'touch', pointerId: 3, t: 10 });
    expect(finger.ontransform).toHaveBeenCalledOnce();
    expect(finger.ontransform.mock.calls[0][0].x).toBeCloseTo(units(50), 6);
    expect(finger.onview).not.toHaveBeenCalled();
    expect(finger.onselect).not.toHaveBeenCalled();
  });

  it('takes hold within the pointer\'s margin around the frame: 4 pixels for the mouse, 12 for the pen, 16 for a finger', async () => {
    const press = async (init: Init, outside: number) => {
      const view = await render();
      view.drag({ x: SCREEN.l - outside, y: SCREEN.cy }, { x: SCREEN.l - outside + 40, y: SCREEN.cy }, init);
      const moved = view.ontransform.mock.calls.length === 1;
      const lasso = view.onselect.mock.calls.length === 1;
      const navigated = view.onview.mock.calls.length > 0;
      cleanup?.();
      return { moved, lasso, navigated };
    };
    expect(await press({}, 3)).toMatchObject({ moved: true, lasso: false });
    expect(await press({}, 7)).toMatchObject({ moved: false, lasso: true });
    expect(await press({ pointerType: 'pen', pointerId: 2 }, 11)).toMatchObject({ moved: true, lasso: false });
    expect(await press({ pointerType: 'pen', pointerId: 2 }, 15)).toMatchObject({ moved: false, lasso: true });
    expect(await press({ pointerType: 'touch', pointerId: 3, t: 5 }, 15)).toMatchObject({ moved: true, navigated: false });
    // Beyond it a finger navigates, as before: no lasso, no edit.
    expect(await press({ pointerType: 'touch', pointerId: 3, t: 5 }, 20)).toMatchObject({ moved: false, lasso: false, navigated: true });
  });

  it('outside the frame the mouse and the pen draw a lasso, a finger navigates', async () => {
    const view = await render();
    view.drag({ x: SCREEN.r + 120, y: SCREEN.t }, { x: SCREEN.r + 200, y: SCREEN.t + 80 });
    expect(view.onselect).toHaveBeenCalledOnce();
    view.onselect.mockClear();
    view.drag({ x: SCREEN.r + 120, y: SCREEN.t }, { x: SCREEN.r + 200, y: SCREEN.t + 80 }, { pointerType: 'pen', pointerId: 4 });
    expect(view.onselect).toHaveBeenCalledOnce();
    expect(view.ontransform).not.toHaveBeenCalled();
    view.drag({ x: SCREEN.r + 120, y: SCREEN.t }, { x: SCREEN.r + 200, y: SCREEN.t + 80 }, { pointerType: 'touch', pointerId: 5, t: 5000 });
    expect(view.onview).toHaveBeenCalled();
    expect(view.ontransform).not.toHaveBeenCalled();
  });

  it('Shift, Ctrl and Meta make it a lasso even on the frame', async () => {
    const view = await render();
    const inside = { x: SCREEN.cx, y: SCREEN.cy };
    const to = { x: SCREEN.cx + 80, y: SCREEN.cy + 60 };
    view.drag(inside, to, { shiftKey: true });
    expect(view.onselect).toHaveBeenLastCalledWith(expect.any(Array), 'add');
    view.drag(inside, to, { ctrlKey: true });
    expect(view.onselect).toHaveBeenLastCalledWith(expect.any(Array), 'remove');
    view.drag(inside, to, { metaKey: true, pointerType: 'pen', pointerId: 9 });
    expect(view.onselect).toHaveBeenLastCalledWith(expect.any(Array), 'remove');
    expect(view.onselect).toHaveBeenCalledTimes(3);
    expect(view.ontransform).not.toHaveBeenCalled();
    // On a handle too.
    view.drag({ x: SCREEN.r, y: SCREEN.b }, to, { shiftKey: true });
    expect(view.ontransform).not.toHaveBeenCalled();
  });

  it('a press that does not go past a click changes nothing, and no new selection is made', async () => {
    const view = await render();
    const inside = { x: SCREEN.cx, y: SCREEN.cy };
    view.fire('pointerdown', inside);
    view.fire('pointerup', inside);
    view.fire('pointerdown', inside);
    view.fire('pointermove', { x: inside.x + 2, y: inside.y + 2 });
    view.fire('pointerup', { x: inside.x + 2, y: inside.y + 2 });
    expect(view.ontransform).not.toHaveBeenCalled();
    expect(view.onselect).not.toHaveBeenCalled();
    expect(view.frame().lifted).toEqual([]);
  });

  it('a drag that comes back to where it started sends nothing', async () => {
    const view = await render();
    const inside = { x: SCREEN.cx, y: SCREEN.cy };
    view.fire('pointerdown', inside);
    view.fire('pointermove', { x: inside.x + 50, y: inside.y });
    view.fire('pointermove', inside);
    view.fire('pointerup', inside);
    expect(view.ontransform).not.toHaveBeenCalled();
  });

  it('without a selection, without the tool or without ontransform, a press inside is what it always was', async () => {
    const none = await render({ selection: [] });
    none.drag({ x: SCREEN.cx, y: SCREEN.cy }, { x: SCREEN.cx + 80, y: SCREEN.cy + 60 });
    expect(none.ontransform).not.toHaveBeenCalled();
    expect(none.onselect).toHaveBeenCalledOnce();
    cleanup?.();

    const off = await render({ tool: 'none' });
    off.drag({ x: SCREEN.cx, y: SCREEN.cy }, { x: SCREEN.cx + 80, y: SCREEN.cy + 60 });
    expect(off.ontransform).not.toHaveBeenCalled();
    expect(off.onview).toHaveBeenCalled();
    cleanup?.();

    const without = await render({ ontransform: null });
    without.drag({ x: SCREEN.cx, y: SCREEN.cy }, { x: SCREEN.cx + 80, y: SCREEN.cy + 60 });
    expect(without.onselect).toHaveBeenCalledOnce();
  });

  it('a selection of a hidden layer has no frame to take hold of', async () => {
    const view = await render({ selection: [2] });
    view.drag({ x: at(6000, 14000).x, y: at(6000, 14000).y }, { x: at(6000, 14000).x + 60, y: at(6000, 14000).y });
    expect(view.ontransform).not.toHaveBeenCalled();
    expect(view.onselect).toHaveBeenCalledOnce();
  });

  it('the right button, the pen\'s eraser end and a second finger do not move anything', async () => {
    const view = await render();
    const inside = { x: SCREEN.cx, y: SCREEN.cy };
    const right = view.fire('pointerdown', inside, { button: 2, buttons: 2 });
    expect(right.defaultPrevented).toBe(false);
    view.fire('pointerup', inside);
    // The eraser end: a pen with the eraser button (32): it erases on the computer's layer, whatever the tool.
    view.drag(inside, { x: inside.x + 60, y: inside.y }, { pointerType: 'pen', buttons: 32, pointerId: 6 });
    expect(view.ontransform).not.toHaveBeenCalled();
  });
});

describe('resizing the selection', () => {
  it('a drag from a corner scales around the opposite one: factor on the diagonal, the anchor stays', async () => {
    const view = await render();
    // From the bottom right corner to twice the distance from the top left corner, along the diagonal.
    const corner = { x: SCREEN.r, y: SCREEN.b };
    const to = { x: SCREEN.l + (SCREEN.r - SCREEN.l) * 2, y: SCREEN.t + (SCREEN.b - SCREEN.t) * 2 };
    view.drag(corner, to);
    expect(view.ontransform).toHaveBeenCalledOnce();
    const { scale, x, y } = view.ontransform.mock.calls[0][0];
    expect(scale).toBeCloseTo(2, 6);
    // The top left corner of the frame is the anchor: (x', y') = anchor * (1 - scale).
    expect(x).toBeCloseTo(FRAME.x * (1 - 2), 3);
    expect(y).toBeCloseTo(FRAME.y * (1 - 2), 3);
    expect(view.onselect).not.toHaveBeenCalled();
  });

  it('each corner has its own anchor', async () => {
    // Refused each time: the strokes are back, ready for the next drag.
    const view = await render({ ontransform: async () => false });
    const corners = [
      { grab: { x: SCREEN.l, y: SCREEN.t }, anchor: { x: FRAME.x + FRAME.width, y: FRAME.y + FRAME.height } },
      { grab: { x: SCREEN.r, y: SCREEN.t }, anchor: { x: FRAME.x, y: FRAME.y + FRAME.height } },
      { grab: { x: SCREEN.l, y: SCREEN.b }, anchor: { x: FRAME.x + FRAME.width, y: FRAME.y } },
    ];
    for (const { grab, anchor } of corners) {
      view.ontransform.mockClear();
      // Half way to the anchor along the diagonal.
      const a = at(anchor.x, anchor.y);
      view.drag(grab, { x: (grab.x + a.x) / 2, y: (grab.y + a.y) / 2 });
      await settle();
      const { scale, x, y } = view.ontransform.mock.calls[0][0];
      expect(scale).toBeCloseTo(0.5, 6);
      expect(x).toBeCloseTo(anchor.x * 0.5, 3);
      expect(y).toBeCloseTo(anchor.y * 0.5, 3);
    }
  });

  it('the factor is the pointer projected onto the diagonal, and goes from the point that was pressed', async () => {
    const view = await render();
    // The pen presses 10 pixels inside the corner (a handle for a pen), and goes 30 pixels to the right of there.
    const press = { x: SCREEN.r - 10, y: SCREEN.b - 10 };
    view.drag(press, { x: press.x + 30, y: press.y }, { pointerType: 'pen', pointerId: 2 });
    const { scale } = view.ontransform.mock.calls[0][0];
    const dx = press.x - SCREEN.l;
    const dy = press.y - SCREEN.t;
    expect(scale).toBeCloseTo(((dx + 30) * dx + dy * dy) / (dx * dx + dy * dy), 6);
  });

  it('is never smaller than a twentieth, nor larger than twenty times', async () => {
    const view = await render({ ontransform: async () => false });
    const corner = { x: SCREEN.r, y: SCREEN.b };
    view.drag(corner, { x: SCREEN.l - 300, y: SCREEN.t - 300 });
    await settle();
    expect(view.ontransform.mock.calls[0][0].scale).toBe(MIN_SCALE);
    view.ontransform.mockClear();
    view.drag(corner, { x: SCREEN.l + 10_000, y: SCREEN.t + 10_000 });
    expect(view.ontransform.mock.calls[0][0].scale).toBe(MAX_SCALE);
  });

  it('a handle is taken within 8 pixels with the mouse, 16 with the pen, 22 with a finger', async () => {
    const press = async (init: Init, inside: number) => {
      const view = await render();
      view.drag({ x: SCREEN.r - inside, y: SCREEN.b - inside }, { x: SCREEN.r - inside - 30, y: SCREEN.b - inside - 30 }, init);
      const sent = view.ontransform.mock.calls[0]?.[0];
      cleanup?.();
      return sent === undefined ? null : sent.scale !== 1;
    };
    // 7 pixels inside the corner on both axes: a mouse takes the handle, 12 it takes the frame.
    expect(await press({}, 7)).toBe(true);
    expect(await press({}, 12)).toBe(false);
    expect(await press({ pointerType: 'pen', pointerId: 2 }, 15)).toBe(true);
    expect(await press({ pointerType: 'pen', pointerId: 2 }, 20)).toBe(false);
    expect(await press({ pointerType: 'touch', pointerId: 3, t: 5 }, 21)).toBe(true);
    expect(await press({ pointerType: 'touch', pointerId: 3, t: 5 }, 30)).toBe(false);
  });
});

describe('the gesture is dropped, and nothing is sent', () => {
  const inside = { x: SCREEN.cx, y: SCREEN.cy };
  const to = { x: SCREEN.cx + 60, y: SCREEN.cy + 40 };

  it('Esc (cancelGesture), pointercancel and a lost capture', async () => {
    const view = await render();
    // pointercancel
    view.fire('pointerdown', inside);
    view.fire('pointermove', to);
    view.fire('pointercancel', to);
    view.fire('pointerup', to);
    expect(view.ontransform).not.toHaveBeenCalled();
    // A lost capture
    view.fire('pointerdown', inside);
    view.fire('pointermove', to);
    view.fire('lostpointercapture', to);
    view.fire('pointerup', to);
    expect(view.ontransform).not.toHaveBeenCalled();
    // Nothing was left lifted by either.
    expect(view.frame().lifted).toEqual([]);
  });

  it('a second finger: the pinch takes over', async () => {
    const view = await render();
    view.fire('pointerdown', inside, { pointerType: 'touch', pointerId: 3, t: 0 });
    view.fire('pointermove', to, { pointerType: 'touch', pointerId: 3, t: 20 });
    view.fire('pointerdown', { x: inside.x + 100, y: inside.y }, { pointerType: 'touch', pointerId: 4, t: 30 });
    view.fire('pointermove', { x: to.x + 100, y: to.y }, { pointerType: 'touch', pointerId: 4, t: 40 });
    view.fire('pointerup', to, { pointerType: 'touch', pointerId: 3, t: 50 });
    view.fire('pointerup', to, { pointerType: 'touch', pointerId: 4, t: 60 });
    expect(view.ontransform).not.toHaveBeenCalled();
    expect(view.frame().lifted).toEqual([]);
  });

  it('a change of page', async () => {
    const view = await render();
    view.fire('pointerdown', inside);
    view.fire('pointermove', to);
    expect(view.frame().lifted).toHaveLength(1);
    view.live.initial({ ...snapshot([block(3000, 3000)], 'p2'), pageIndex: 1, pageCount: 2 });
    await tick();
    view.fire('pointermove', { x: to.x + 10, y: to.y });
    view.fire('pointerup', to);
    expect(view.ontransform).not.toHaveBeenCalled();
    expect(view.frame().lifted).toEqual([]);
  });

  it('an edit applied while the drag goes on (the strokes it was made against are gone)', async () => {
    const view = await render();
    view.fire('pointerdown', inside);
    view.fire('pointermove', to);
    view.live.receive({
      sentAt: 0,
      events: [{ type: 'strokesEdited', pageIndex: 0, revision: 6, removed: [1], inserted: [], recoloured: [] }],
    });
    expect(view.frame().lifted).toEqual([]);
    view.fire('pointerup', to);
    expect(view.ontransform).not.toHaveBeenCalled();
  });

  it('Esc through cancelGesture drops it: the strokes stay, and the release sends nothing', async () => {
    const live = new LiveState();
    live.initial(snapshot([block(3000, 3000)]));
    const target = document.createElement('div');
    document.body.append(target);
    const ontransform = vi.fn(async () => true);
    const component = mount(LiveCanvas, {
      target,
      props: { live, ink, mode: 'preview', label: 'page', onview: vi.fn(), onselect: vi.fn(), tool: 'select', selection: [0], ontransform },
    }) as { cancelGesture(): boolean; gripping(): boolean };
    cleanup = () => {
      unmount(component as never);
      target.remove();
    };
    await tick();
    const canvas = target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
    const fire = (type: string, p: { x: number; y: number }) => {
      const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: p.x, clientY: p.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
      for (const [key, value] of Object.entries({ pointerType: 'mouse', pointerId: 1, pressure: 0.5, timeStamp: 0 })) Object.defineProperty(event, key, { value });
      canvas.dispatchEvent(event);
    };
    expect(component.cancelGesture()).toBe(false);
    fire('pointerdown', inside);
    expect(component.gripping()).toBe(true);
    fire('pointermove', to);
    expect(component.cancelGesture()).toBe(true);
    expect(component.gripping()).toBe(false);
    fire('pointermove', { x: to.x + 20, y: to.y });
    fire('pointerup', to);
    expect(ontransform).not.toHaveBeenCalled();
    expect(component.cancelGesture()).toBe(false);
  });
});

describe('the preview while the drag goes on', () => {
  it('shows the handles on the frame of the selection, and the strokes lifted only once the drag goes past a click', async () => {
    const view = await render();
    expect(view.frame()).toMatchObject({ lifted: [], handles: 4 });
    const inside = { x: SCREEN.cx, y: SCREEN.cy };
    view.fire('pointerdown', inside);
    expect(view.frame().lifted).toEqual([]);
    view.fire('pointermove', { x: inside.x + 2, y: inside.y });
    expect(view.frame().lifted).toEqual([]);
    view.fire('pointermove', { x: inside.x + 50, y: inside.y + 20 });
    const lifted = view.frame().lifted;
    expect(lifted).toHaveLength(1);
    view.fire('pointermove', { x: inside.x + 80, y: inside.y + 20 });
    const further = view.frame().lifted;
    expect(further[0][1] - lifted[0][1]).toBeCloseTo(30, 6);
    expect(further[0][2]).toBeCloseTo(lifted[0][2], 6);
    // The release with no edit asked (Esc earlier): the strokes go back.
    view.fire('pointercancel', inside);
    expect(view.frame().lifted).toEqual([]);
  });

  it('the handles are not drawn without the tool, and without ontransform', async () => {
    const off = await render({ tool: 'none' });
    expect(off.frame().handles).toBe(0);
    cleanup?.();
    const without = await render({ ontransform: null });
    expect(without.frame().handles).toBe(0);
  });

  it('after the release the strokes stay where the drag left them until the session has applied the edit', async () => {
    let answer!: (went: boolean) => void;
    const view = await render({ ontransform: () => new Promise<boolean>((resolve) => (answer = resolve)) });
    const inside = { x: SCREEN.cx, y: SCREEN.cy };
    const to = { x: SCREEN.cx + 80, y: SCREEN.cy + 40 };
    view.drag(inside, to);
    const dragged = view.frame().lifted;
    expect(dragged).toHaveLength(1);
    // The call is answered, the event has not come: still lifted, at the same place.
    answer(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(view.frame().lifted).toEqual(dragged);
    // Nothing is taken from a press meanwhile.
    view.fire('pointerdown', inside);
    view.fire('pointermove', to);
    view.fire('pointerup', to);
    expect(view.ontransform).toHaveBeenCalledOnce();
    expect(view.onselect).not.toHaveBeenCalled();
    // The event is applied: in that very frame the strokes are no longer lifted.
    view.live.receive({
      sentAt: 0,
      events: [
        {
          type: 'strokesEdited',
          pageIndex: 0,
          revision: 6,
          removed: [0],
          inserted: [{ index: 0, stroke: block(3000 + units(80), 3000 + units(40)) }],
          recoloured: [],
        },
      ],
    });
    expect(view.frame().lifted).toEqual([]);
  });

  it('the event may come before the answer: the preview is over as soon as it is applied', async () => {
    let answer!: (went: boolean) => void;
    const view = await render({ ontransform: () => new Promise<boolean>((resolve) => (answer = resolve)) });
    const inside = { x: SCREEN.cx, y: SCREEN.cy };
    view.drag(inside, { x: inside.x + 80, y: inside.y });
    expect(view.frame().lifted).toHaveLength(1);
    view.live.receive({
      sentAt: 0,
      events: [{ type: 'strokesEdited', pageIndex: 0, revision: 6, removed: [0], inserted: [{ index: 0, stroke: block(3000 + units(80), 3000) }], recoloured: [] }],
    });
    expect(view.frame().lifted).toEqual([]);
    answer(true);
    await Promise.resolve();
    await Promise.resolve();
    expect(view.frame().lifted).toEqual([]);
  });

  it('a refused edit puts the strokes back at once', async () => {
    const view = await render({ ontransform: async () => false });
    const inside = { x: SCREEN.cx, y: SCREEN.cy };
    view.drag(inside, { x: inside.x + 80, y: inside.y });
    await vi.waitFor(() => expect(view.frame().lifted).toEqual([]));
    const rejected = await render({ ontransform: () => Promise.reject(new Error('refused')) });
    rejected.drag(inside, { x: inside.x + 80, y: inside.y });
    await vi.waitFor(() => expect(rejected.frame().lifted).toEqual([]));
  });

  it('an answer that never brings the event does not keep the strokes lifted for ever', async () => {
    vi.useFakeTimers();
    const view = await render({ ontransform: async () => true });
    const inside = { x: SCREEN.cx, y: SCREEN.cy };
    view.drag(inside, { x: inside.x + 80, y: inside.y });
    await vi.advanceTimersByTimeAsync(10);
    expect(view.frame().lifted).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(2500);
    expect(view.frame().lifted).toEqual([]);
  });

  it('does not leave the cursor of a mouse over the frame behind, and shows what a press would take', async () => {
    const view = await render();
    const cursor = () => view.canvas.style.cursor;
    view.fire('pointermove', { x: SCREEN.cx, y: SCREEN.cy }, { buttons: 0 });
    await tick();
    expect(cursor()).toBe('move');
    view.fire('pointermove', { x: SCREEN.r, y: SCREEN.b }, { buttons: 0 });
    await tick();
    expect(cursor()).toBe('nwse-resize');
    view.fire('pointermove', { x: SCREEN.r, y: SCREEN.t }, { buttons: 0 });
    await tick();
    expect(cursor()).toBe('nesw-resize');
    view.fire('pointermove', { x: SCREEN.r + 100, y: SCREEN.t }, { buttons: 0 });
    await tick();
    expect(cursor()).toBe('');
  });
});

describe('the tablet keeps writing', () => {
  it('a stroke it ends meanwhile does not move the indices: the edit is still for the chosen strokes', async () => {
    const view = await render();
    const batch = (...events: LiveBatch['events']): LiveBatch => ({ events, sentAt: 0 });
    const point = (x: number, y: number, i: number) => ({ x, y, p: 0.6, t: 5 * i, recv: i });
    const inside = { x: SCREEN.cx, y: SCREEN.cy };
    view.fire('pointerdown', inside);
    view.fire('pointermove', { x: inside.x + 40, y: inside.y });
    view.live.receive(
      batch(
        { type: 'strokeStart', pageIndex: 0, stroke: { tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [point(15000, 15000, 0), point(15500, 15000, 1)] } },
        { type: 'strokeEnd', pageIndex: 0 },
      ),
    );
    view.fire('pointermove', { x: inside.x + 60, y: inside.y });
    view.fire('pointerup', { x: inside.x + 60, y: inside.y });
    expect(view.ontransform).toHaveBeenCalledOnce();
    expect(view.ontransform.mock.calls[0][0].x).toBeCloseTo(units(60), 6);
  });
});
