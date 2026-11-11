// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M17 (package B): the canvas keys its paper on the key of the image's pixels, renders it with the window's own
// `paper` when it has one (the editor's projection window), says what it shows in test attributes, and with
// `interactive` false takes nothing from the pointer.
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { instantiateInk, type Ink } from '../../lib/ink';
import type { LiveSnapshot, LiveViewState } from '../../lib/live';
import { FakeContext } from '../../lib/live-fake-canvas';
import { LiveState } from '../../lib/live-state.svelte';
import type { Background, BackgroundImage, Layer, PageSize, Stroke } from '../../lib/note';
import LiveCanvas from './LiveCanvas.svelte';

let cleanup: (() => void) | null = null;
let ink: Ink;
let frames: FrameRequestCallback[] = [];

const PAGE = { width: 20000, height: 10000 };
const IMAGE: BackgroundImage = { name: 'fiche.pdf', pdfPage: 1, rect: { x: 0, y: 0, width: 20000, height: 10000 }, quarterTurns: 0, opacity: 1 };
const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const PICTURE = 'data:image/png;base64,QUJD';
const PLAIN = 'data:image/png;base64,REVG';

class FakeImage {
  onload: (() => void) | null = null;
  private url = '';
  get src() {
    return this.url;
  }
  set src(url: string) {
    this.url = url;
    queueMicrotask(() => this.onload?.());
  }
  decode(): Promise<void> {
    return Promise.resolve();
  }
}

beforeEach(async () => {
  ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  frames = [];
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => frames.push(callback));
  vi.stubGlobal('Image', FakeImage);
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

function line(x: number, y: number, layer = 0): Stroke {
  return { tool: 'ballpoint', color: BLACK, width: 60, layer, points: [0, 1, 2].map((i) => ({ x: x + 100 * i, y, pressure: 0.6, t: 5 * i })) };
}

function snapshot(
  extra: Partial<LiveSnapshot['page']> = {},
  fields: Partial<LiveSnapshot> = {},
  view: Partial<LiveViewState> = {},
): LiveSnapshot {
  return {
    state: 'live',
    page: {
      id: 'sheet',
      title: '',
      created: 0,
      modified: 0,
      page: PAGE,
      background: { paper: 'cream', template: 'seyes', texture: false },
      tags: [],
      layers: [{ name: 'Calque 1', visible: true }],
      strokes: [line(8000, 3000), line(8000, 4000)],
      timing: 'recorded',
      origin: { kind: 'live' },
      ...extra,
    },
    pageIndex: 0,
    pageCount: 1,
    orientation: 'landscape',
    layer: 0,
    style: { tool: 'ballpoint', color: BLACK, width: 60 },
    view: { zoom: 1, x: 10000, y: 5000, board: false, laser: false, ...view },
    hover: null,
    presentation: null,
    saved: [],
    ...fields,
  };
}

function render(live: LiveState, props: Record<string, unknown> = {}, mode: 'preview' | 'presentation' = 'presentation') {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(LiveCanvas, { target, props: { live, ink, mode, label: 'page', ...props } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  return target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
}

/** Another snapshot reaches the window. */
function show(live: LiveState, next: LiveSnapshot) {
  live.receive({ events: [{ type: 'snapshot', snapshot: next }], sentAt: 0 });
}

async function settle() {
  for (let i = 0; i < 5; i++) await new Promise((resolve) => setTimeout(resolve, 0));
  await tick();
  const due = frames;
  frames = [];
  for (const callback of due) callback(performance.now());
}

describe('live canvas: what it says of the page', () => {
  it('counts the strokes drawn: visible layers only, the one in progress included', async () => {
    const hidden: Layer[] = [
      { name: 'A', visible: true },
      { name: 'B', visible: false },
    ];
    const live = new LiveState();
    live.initial(snapshot({ layers: hidden, strokes: [line(1000, 1000, 0), line(1000, 2000, 1), line(1000, 3000, 0)] }));
    const canvas = render(live);
    await settle();
    expect(canvas.dataset.strokes).toBe('3');
    expect(canvas.dataset.drawn).toBe('2');
    // The tablet's stroke in progress on the shown layer counts; on the hidden one it does not.
    live.receive({ events: [{ type: 'strokeStart', pageIndex: 0, stroke: { tool: 'ballpoint', color: BLACK, width: 60, layer: 0, points: [{ x: 5, y: 5, p: 0.5, t: 0, recv: 0 }] } }], sentAt: 0 });
    await settle();
    expect(canvas.dataset.drawn).toBe('3');
    show(live, snapshot({ layers: [{ name: 'A', visible: false }, { name: 'B', visible: true }], strokes: [line(1000, 1000, 0), line(1000, 2000, 1)] }));
    await settle();
    expect(canvas.dataset.drawn).toBe('1');
  });

  it('names the effective paper, the template, the image, the zoom', async () => {
    const live = new LiveState();
    live.initial(snapshot({ backgroundImage: IMAGE }, { imagePixels: { key: 'k1', width: 100, height: 50 } }, { zoom: 2 }));
    const canvas = render(live);
    await settle();
    expect(canvas.dataset.paper).toBe('cream');
    expect(canvas.dataset.template).toBe('seyes');
    expect(canvas.dataset.image).toBe('fiche.pdf');
    expect(canvas.dataset.zoom).toBe('2');
    // The blackboard: the effective paper is slate, the template stays.
    live.receive({ events: [{ type: 'view', view: { zoom: 2, x: 10000, y: 5000, board: true, laser: false } }], sentAt: 0 });
    await settle();
    expect(canvas.dataset.paper).toBe('slate');
    expect(canvas.dataset.template).toBe('seyes');
    // A custom colour, a page without an image.
    show(live, snapshot({ background: { paper: { custom: { r: 1, g: 2, b: 3, a: 255 } }, template: 'blank', texture: false } }));
    await settle();
    expect(canvas.dataset.paper).toBe('custom');
    expect(canvas.dataset.template).toBe('blank');
    expect(canvas.dataset.image).toBe('');
  });

  it('says the picture of the paper is ready once it is drawn, and not again for the same paper at another zoom', async () => {
    vi.spyOn(api, 'renderBackground').mockResolvedValue(PLAIN);
    const live = new LiveState();
    live.initial(snapshot());
    const canvas = render(live);
    expect(canvas.dataset.paperReady).toBe('false');
    await settle();
    expect(canvas.dataset.paperReady).toBe('true');
    // Another template: the old picture must not pass for the new paper.
    let release = () => {};
    vi.spyOn(api, 'renderBackground').mockImplementation(() => new Promise((resolve) => (release = () => resolve(PLAIN))));
    show(live, snapshot({ background: { paper: 'cream', template: 'grid5mm', texture: false } }));
    await tick();
    expect(canvas.dataset.paperReady).toBe('false');
    release();
    await settle();
    expect(canvas.dataset.paperReady).toBe('true');
    // The same paper asked larger at a deeper zoom: the picture shown stays shown.
    live.receive({ events: [{ type: 'view', view: { zoom: 8, x: 10000, y: 5000, board: false, laser: false } }], sentAt: 0 });
    await tick();
    expect(canvas.dataset.paperReady).toBe('true');
  });
});

describe('live canvas: the paper is keyed on the image\'s pixels', () => {
  it('renders again when another file is placed alike, not when nothing changed', async () => {
    const sheet = vi.spyOn(api, 'livePageBackground').mockResolvedValue(PICTURE);
    vi.spyOn(api, 'renderBackground').mockResolvedValue(PLAIN);
    const live = new LiveState();
    live.initial(snapshot({ backgroundImage: IMAGE }, { imagePixels: { key: 'one', width: 100, height: 50 } }));
    render(live);
    await settle();
    expect(sheet).toHaveBeenCalledTimes(1);
    // The same snapshot again: nothing to draw again.
    show(live, snapshot({ backgroundImage: IMAGE }, { imagePixels: { key: 'one', width: 100, height: 50 } }));
    await settle();
    expect(sheet).toHaveBeenCalledTimes(1);
    // Another file of the same name, on the same page, placed the same: another key, rendered again.
    show(live, snapshot({ backgroundImage: IMAGE }, { imagePixels: { key: 'two', width: 100, height: 50 } }));
    await settle();
    expect(sheet).toHaveBeenCalledTimes(2);
  });

  it('asks the session for the paper of any page with an image, not only a resumed sheet\'s', async () => {
    const sheet = vi.spyOn(api, 'livePageBackground').mockResolvedValue(PICTURE);
    const plain = vi.spyOn(api, 'renderBackground').mockResolvedValue(PLAIN);
    const live = new LiveState();
    live.initial(snapshot({ backgroundImage: IMAGE }, { pageIndex: 1, pageCount: 2, imagePixels: { key: 'k', width: 100, height: 50 } }));
    render(live);
    await settle();
    expect(sheet.mock.calls[0][0]).toBe(1);
    expect(plain).not.toHaveBeenCalled();
  });
});

describe('live canvas: its own way to render the paper', () => {
  it('uses `paper` for every page, with the page, the effective background and the width', async () => {
    const session = vi.spyOn(api, 'livePageBackground').mockResolvedValue(PICTURE);
    const plain = vi.spyOn(api, 'renderBackground').mockResolvedValue(PLAIN);
    const paper = vi.fn<(page: PageSize, background: Background, widthPx: number) => Promise<string>>().mockResolvedValue('data:image/png;base64,Tk9X');
    const live = new LiveState();
    live.initial(snapshot({ backgroundImage: IMAGE }, { imagePixels: { key: 'k', width: 100, height: 50 } }));
    render(live, { paper });
    await settle();
    expect(session).not.toHaveBeenCalled();
    expect(plain).not.toHaveBeenCalled();
    expect(paper).toHaveBeenCalledTimes(1);
    const [page, background, width] = paper.mock.calls[0];
    expect(page).toEqual(PAGE);
    expect(background).toEqual({ paper: 'cream', template: 'seyes', texture: false });
    expect(width).toBeGreaterThan(0);
  });
});

describe('live canvas: interactive', () => {
  /** A mouse press at the middle of the canvas, as the tests of the selection fire them. */
  function press(canvas: HTMLCanvasElement, type: string) {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: 400, clientY: 280, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    for (const [key, value] of Object.entries({ pointerType: 'mouse', pointerId: 1, pressure: 0.5, timeStamp: 0 })) {
      Object.defineProperty(event, key, { value, configurable: true });
    }
    canvas.dispatchEvent(event);
    return event;
  }

  it('writes temporary marks in the presentation by default, and not when it is false', async () => {
    for (const [interactive, marked] of [
      [true, true],
      [false, false],
    ] as const) {
      const live = new LiveState();
      live.initial(snapshot());
      const onmarkschange = vi.fn();
      const canvas = render(live, { interactive, onmarkschange });
      press(canvas, 'pointerdown');
      press(canvas, 'pointermove');
      press(canvas, 'pointerup');
      expect(onmarkschange.mock.calls.length > 0, `interactive ${interactive}`).toBe(marked);
      expect(canvas.classList.contains('writable')).toBe(interactive);
      cleanup?.();
      cleanup = null;
    }
  });

  it('does not take the pointer down at all (the window\'s cursor still wakes: the event is not stopped)', async () => {
    const live = new LiveState();
    live.initial(snapshot());
    const canvas = render(live, { interactive: false });
    const woken = vi.fn();
    window.addEventListener('pointerdown', woken);
    const event = press(canvas, 'pointerdown');
    window.removeEventListener('pointerdown', woken);
    expect(event.defaultPrevented).toBe(false);
    expect(woken).toHaveBeenCalledTimes(1);
  });

  it('keeps the wheel for the preview that is interactive', async () => {
    for (const [interactive, zoomed] of [
      [true, true],
      [false, false],
    ] as const) {
      const live = new LiveState();
      live.initial(snapshot());
      const onview = vi.fn();
      const canvas = render(live, { interactive, onview }, 'preview');
      await settle();
      canvas.dispatchEvent(new WheelEvent('wheel', { deltaY: -120, bubbles: true, cancelable: true, clientX: 400, clientY: 280 }));
      expect(onview.mock.calls.length > 0, `interactive ${interactive}`).toBe(zoomed);
      cleanup?.();
      cleanup = null;
    }
  });
});
