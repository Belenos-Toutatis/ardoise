// @vitest-environment jsdom
/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { instantiateInk, type Ink } from '../../lib/ink';
import { LatencyRecorder } from '../../lib/latency';
import { FakeContext } from '../../lib/live-fake-canvas';
import { sceneTransform } from '../../lib/live-render';
import type { LiveSnapshot } from '../../lib/live';
import { LiveState, subscribeLive } from '../../lib/live-state.svelte';
import type { BackgroundImage, Stroke } from '../../lib/note';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveSubscribe, mockLiveTick, resetMockLive } from '../../lib/mock-live';
import LiveCanvas from './LiveCanvas.svelte';

let cleanup: (() => void) | null = null;
let ink: Ink;
/** Animation frames wait here until the test lets a frame begin. */
let frames: FrameRequestCallback[] = [];

beforeEach(async () => {
  ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  resetMockLive();
  resetMockLibrary();
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 0;
  frames = [];
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => frames.push(callback));
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  resetMockLive();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

/** A frame begins: the callbacks asked for before it run (those they ask for wait for the next one). */
function beginFrame() {
  const due = frames;
  frames = [];
  for (const callback of due) callback(performance.now());
}

/** A canvas on a live session fed by the stand-in for Rust, live mode started. */
async function render() {
  const live = new LiveState();
  const latency = new LatencyRecorder();
  await subscribeLive(live, (onBatch) => mockLiveSubscribe(onBatch));
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(LiveCanvas, { target, props: { live, ink, mode: 'presentation', label: 'page', latency } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  mockLiveInvoke('live_start', {});
  await new Promise((resolve) => setTimeout(resolve, 5));
  await tick();
  beginFrame();
  latency.clear();
  return { live, latency };
}

describe('live canvas: latency', () => {
  it('counts a point as displayed at the start of the frame after its drawing', async () => {
    const { latency } = await render();

    mockLiveTick(60);
    await tick();
    // Frame k draws the points: their interface part is known, their end-to-end latency is not.
    beginFrame();
    expect(latency.uiSamples().length).toBeGreaterThan(0);
    expect(latency.samples()).toEqual([]);

    // The frame after: the points count as displayed, later than the end of their drawing.
    await new Promise((resolve) => setTimeout(resolve, 20));
    beginFrame();
    expect(latency.samples().length).toBe(latency.uiSamples().length);
    expect(latency.median()!).toBeGreaterThanOrEqual(20);
    expect(latency.median()!).toBeGreaterThan(latency.uiMedian()!);
  });

  it('records no end-to-end sample for a frame awaited while the window was hidden', async () => {
    const { latency } = await render();

    mockLiveTick(60);
    await tick();
    beginFrame();
    expect(latency.uiSamples().length).toBeGreaterThan(0);
    // The window is hidden and shown again before the next frame begins: that frame says nothing.
    document.dispatchEvent(new Event('visibilitychange'));
    beginFrame();
    expect(latency.samples()).toEqual([]);
  });
});

describe('live canvas: the paper', () => {
  const PAGE = { width: 20000, height: 10000 };
  const IMAGE: BackgroundImage = {
    name: 'fiche.pdf',
    pdfPage: 2,
    rect: { x: 4000, y: 1000, width: 8000, height: 4000 },
    quarterTurns: 1,
    opacity: 0.8,
  };
  const PICTURE = 'data:image/png;base64,QUJD';
  const PLAIN = 'data:image/png;base64,REVG';
  const BLACK = { r: 0, g: 0, b: 0, a: 255 };

  /** Ink on a known spot of the page: `(x, y)`, a short line to the right. */
  function inkAt(x: number, y: number): Stroke {
    return {
      tool: 'ballpoint',
      color: BLACK,
      width: 60,
      layer: 0,
      points: [0, 1, 2, 3].map((i) => ({ x: x + 100 * i, y, pressure: 0.6, t: 5 * i })),
    };
  }

  function snapshot(extra: Partial<LiveSnapshot['page']> = {}, zoom = 1): LiveSnapshot {
    return {
      state: 'live',
      page: {
        id: 'sheet',
        title: 'Fiche',
        created: 0,
        modified: 0,
        page: PAGE,
        background: { paper: 'cream', template: 'seyes', texture: false },
        tags: [],
        layers: [{ name: 'Calque 1', visible: true }],
        strokes: [inkAt(8000, 3000)],
        timing: 'recorded',
        origin: { kind: 'live' },
        ...extra,
      },
      pageIndex: 0,
      pageCount: 1,
      orientation: 'landscape',
      layer: 0,
      style: { tool: 'ballpoint', color: BLACK, width: 60 },
      view: { zoom, x: 8000, y: 3000, board: false, laser: false },
      hover: null,
      presentation: null,
      saved: [],
    };
  }

  /** Images that load as soon as they are given a source; `decode` waits for the test when asked. */
  class FakeImage {
    static decoding: (() => void)[] = [];
    static holdDecoding = false;
    onload: (() => void) | null = null;
    decoded = 0;
    private url = '';
    get src() {
      return this.url;
    }
    set src(url: string) {
      this.url = url;
      queueMicrotask(() => this.onload?.());
    }
    decode(): Promise<void> {
      this.decoded += 1;
      if (!FakeImage.holdDecoding) return Promise.resolve();
      return new Promise((resolve) => FakeImage.decoding.push(resolve));
    }
  }

  /** The canvas drawing `live`, the contexts it uses, and what Rust was asked. */
  function renderPaper(live: LiveState, mode: 'preview' | 'presentation') {
    FakeImage.decoding = [];
    FakeImage.holdDecoding = false;
    vi.stubGlobal('Image', FakeImage);
    const contexts: FakeContext[] = [];
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
      const ctx = new FakeContext();
      contexts.push(ctx);
      return ctx as unknown as RenderingContext;
    });
    const sheetPaper = vi.spyOn(api, 'livePageBackground').mockResolvedValue(PICTURE);
    const plainPaper = vi.spyOn(api, 'renderBackground').mockResolvedValue(PLAIN);
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(LiveCanvas, { target, props: { live, ink, mode, label: 'page' } });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    return { contexts, sheetPaper, plainPaper };
  }

  /** Lets the paper's promises and image load run, then begins a frame. */
  async function settle() {
    for (let i = 0; i < 5; i++) await new Promise((resolve) => setTimeout(resolve, 0));
    await tick();
    beginFrame();
  }

  /** Where the paper's picture was drawn: its arguments, the transform then, and the transforms of the ink over it. */
  function paperDrawn(contexts: FakeContext[]) {
    const out: { image: FakeImage; rect: unknown[]; transform: unknown[]; ink: unknown[][] }[] = [];
    for (const ctx of contexts) {
      let transform: unknown[] = [];
      for (const [i, call] of ctx.calls.entries()) {
        if (call.op === 'setTransform') transform = call.args;
        if (call.op === 'drawImage' && call.args[0] instanceof FakeImage) {
          let after = transform;
          const ink: unknown[][] = [];
          for (const later of ctx.calls.slice(i + 1)) {
            if (later.op === 'setTransform') after = later.args;
            if (later.op === 'fill') ink.push(after);
          }
          out.push({ image: call.args[0] as FakeImage, rect: call.args.slice(1), transform, ink });
        }
      }
    }
    return out;
  }

  it('asks an ordinary page\'s paper as ever, and never the session', async () => {
    const live = new LiveState();
    live.initial(snapshot());
    const { sheetPaper, plainPaper } = renderPaper(live, 'presentation');
    await settle();
    expect(plainPaper).toHaveBeenCalledTimes(1);
    const args = plainPaper.mock.calls[0];
    expect(args).toHaveLength(3);
    expect(args[0]).toEqual(PAGE);
    expect(args[1]).toEqual({ paper: 'cream', template: 'seyes', texture: false });
    expect(sheetPaper).not.toHaveBeenCalled();
  });

  it('asks the session for the paper of a resumed sheet, at the width of the zoom and with the blackboard', async () => {
    const live = new LiveState();
    live.initial(snapshot({ backgroundImage: IMAGE }));
    const { sheetPaper, plainPaper } = renderPaper(live, 'presentation');
    await settle();
    expect(plainPaper).not.toHaveBeenCalled();
    expect(sheetPaper).toHaveBeenCalledTimes(1);
    const [pageIndex, page, background, width] = sheetPaper.mock.calls[0];
    expect(pageIndex).toBe(0);
    expect(page).toEqual(PAGE);
    expect(background).toEqual({ paper: 'cream', template: 'seyes', texture: false });

    // Zoomed in: asked again, larger, while the picture it has stays.
    live.receive({ events: [{ type: 'view', view: { zoom: 8, x: 8000, y: 3000, board: false, laser: false } }], sentAt: 0 });
    await settle();
    expect(sheetPaper).toHaveBeenCalledTimes(2);
    expect(sheetPaper.mock.calls[1][3]).toBeGreaterThan(width);

    // The blackboard: the same sheet on slate paper.
    live.receive({ events: [{ type: 'view', view: { zoom: 8, x: 8000, y: 3000, board: true, laser: false } }], sentAt: 0 });
    await settle();
    expect(sheetPaper).toHaveBeenCalledTimes(3);
    expect(sheetPaper.mock.calls[2][2]).toEqual({ paper: 'slate', template: 'seyes', texture: false });
  });

  it('goes back to plain paper on the next page', async () => {
    const live = new LiveState();
    live.initial(snapshot({ backgroundImage: IMAGE }));
    const { contexts, sheetPaper, plainPaper } = renderPaper(live, 'presentation');
    await settle();
    expect(paperDrawn(contexts).at(-1)?.image.src).toBe(PICTURE);
    const next: LiveSnapshot = {
      ...snapshot({ id: 'next', background: { paper: 'white', template: 'blank', texture: false }, strokes: [] }),
      pageIndex: 1,
      pageCount: 2,
    };
    live.receive({ events: [{ type: 'snapshot', snapshot: next }], sentAt: 0 });
    await settle();
    expect(sheetPaper).toHaveBeenCalledTimes(1);
    expect(plainPaper).toHaveBeenCalledTimes(1);
    expect(paperDrawn(contexts).at(-1)?.image.src).toBe(PLAIN);
    expect(live.pageImage).toBeNull();
  });

  it('decodes a sheet\'s picture before it is drawn, and draws the strokes meanwhile', async () => {
    const live = new LiveState();
    live.initial(snapshot({ backgroundImage: IMAGE }));
    const { contexts } = renderPaper(live, 'presentation');
    FakeImage.holdDecoding = true;
    await settle();
    expect(FakeImage.decoding).toHaveLength(1);
    expect(paperDrawn(contexts)).toEqual([]);
    expect(contexts.some((c) => c.count('fill') > 0)).toBe(true);
    FakeImage.decoding[0]();
    await settle();
    expect(paperDrawn(contexts).at(-1)?.image.src).toBe(PICTURE);
  });

  it('leaves the decoding of an ordinary page\'s paper as it was', async () => {
    const live = new LiveState();
    live.initial(snapshot());
    const { contexts } = renderPaper(live, 'presentation');
    await settle();
    const drawn = paperDrawn(contexts).at(-1);
    expect(drawn?.image.src).toBe(PLAIN);
    expect(drawn?.image.decoded).toBe(0);
  });

  describe.each([
    ['presentation', 1],
    ['presentation', 4],
    ['preview', 1],
    ['preview', 4],
  ] as const)('%s at zoom %i', (mode, zoom) => {
    it('puts the picture over the whole page in the transform of the strokes', async () => {
      Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 2 });
      try {
        const live = new LiveState();
        live.initial(snapshot({ backgroundImage: IMAGE }, zoom));
        const { contexts } = renderPaper(live, mode);
        await settle();
        const drawn = paperDrawn(contexts).at(-1)!;
        expect(drawn.image.src).toBe(PICTURE);
        // The picture stands for the whole page: Rust put the image in it at its `rect` and turns.
        expect(drawn.rect).toEqual([0, 0, PAGE.width, PAGE.height]);
        // The scene's page transform, at the screen's pixels...
        const viewport = mode === 'preview' ? { width: 800, height: 560 } : { width: 1280, height: 720 };
        const t = sceneTransform(live, { mode, viewport });
        expect(drawn.transform).toEqual([t.scale * 2, 0, 0, t.scale * 2, t.x * 2, t.y * 2]);
        // ...and the same one for the ink: a stroke on a spot of the page falls on that spot of the
        // picture.
        expect(drawn.ink.length).toBeGreaterThan(0);
        expect(drawn.ink[0]).toEqual(drawn.transform);
      } finally {
        Object.defineProperty(window, 'devicePixelRatio', { configurable: true, value: 1 });
      }
    });
  });
});
