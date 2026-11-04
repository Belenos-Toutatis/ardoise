// @vitest-environment jsdom
import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../lib/i18n.svelte';
import { FakeContext } from '../lib/live-fake-canvas';
import type { LiveEvent, LiveSnapshot } from '../lib/live';
import type { Layer, Note, Stroke } from '../lib/note';
import type { ShareMessage, SharedPage } from '../lib/share';
import Viewer from './Viewer.svelte';

vi.mock('../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const A4 = { width: 29700, height: 21600 };
const LAYERS: Layer[] = [{ name: 'Calque 1', visible: true }];
const CODE = 'K7MQ2X';
const encoder = new TextEncoder();

function stroke(x: number): Stroke {
  return {
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer: 0,
    points: Array.from({ length: 4 }, (_, i) => ({ x: x + 100 * i, y: 5000 + 50 * i, pressure: 0.5, t: 5 * i })),
  };
}

const sharedPage = (strokes: Stroke[]): SharedPage => ({
  id: 'n1',
  page: A4,
  background: { paper: 'white', template: 'blank', texture: false },
  layers: LAYERS,
  strokes,
});

function liveSnapshot(strokes: Stroke[]): LiveSnapshot {
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
    pageCount: 2,
    orientation: 'landscape',
    layer: 0,
    style: { tool: 'ballpoint', color: BLACK, width: 60 },
    view: { zoom: 1, x: 14850, y: 10800, board: false, laser: false },
    hover: null,
    presentation: null,
    saved: [],
  };
}

/** The Ardoise PC as the page sees it: `/join`, `/events`, and the streams it keeps open. */
class FakePc {
  code = CODE;
  joined = false;
  remaining = 5;
  /** Status of `/events` when the cookie is there; 200 streams. */
  eventsStatus = 200;
  /** `/events` never answers once the code is accepted (a filtering proxy holding the stream). */
  holdEvents = false;
  /** A `/join` answer replacing the normal ones. */
  joinOverride: (() => Response) | null = null;
  joins: string[] = [];
  eventRequests = 0;
  private controllers: ReadableStreamDefaultController<Uint8Array>[] = [];

  fetch = vi.fn(async (input: RequestInfo | URL, init?: RequestInit): Promise<Response> => {
    const url = String(input);
    if (url === '/join') {
      const { code } = JSON.parse(String(init?.body)) as { code: string };
      this.joins.push(code);
      if (this.joinOverride) return this.joinOverride();
      if (code === this.code) {
        this.joined = true;
        return new Response(null, { status: 204 });
      }
      this.remaining -= 1;
      return Response.json({ error: 'wrongCode', remaining: this.remaining }, { status: 403 });
    }
    if (url === '/events') {
      this.eventRequests += 1;
      if (!this.joined) return new Response(null, { status: 401 });
      if (this.holdEvents) return new Promise<Response>(() => undefined);
      if (this.eventsStatus !== 200) return new Response(null, { status: this.eventsStatus });
      return new Response(
        new ReadableStream<Uint8Array>({
          start: (controller) => {
            this.controllers.push(controller);
            controller.enqueue(encoder.encode('retry: 2000\n\n'));
          },
        }),
        { status: 200 },
      );
    }
    return new Response(null, { status: 404 });
  });

  /** Sends `message` to every stream open. */
  send(message: ShareMessage): void {
    const bytes = encoder.encode(`data: ${JSON.stringify(message)}\n\n`);
    for (const controller of this.controllers) controller.enqueue(bytes);
  }

  /** The connection drops. */
  cut(): void {
    for (const controller of this.controllers.splice(0)) {
      try {
        controller.close();
      } catch {
        // The page already closed it (it cancels the stream after `end`).
      }
    }
  }
}

let cleanup: (() => void) | null = null;
let pc: FakePc;

beforeEach(() => {
  pc = new FakePc();
  vi.stubGlobal('fetch', pc.fetch);
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  Object.defineProperty(document, 'fullscreenEnabled', { configurable: true, value: undefined });
});

function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Viewer, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  return {
    target,
    main: () => target.querySelector<HTMLElement>('main.viewer')!,
    canvas: () => target.querySelector<HTMLCanvasElement>('canvas.viewer-canvas'),
    view: (name: string) => target.querySelector<HTMLElement>(`[data-view="${name}"]`),
    input: () => target.querySelector<HTMLInputElement>('input.code')!,
    submit: () => target.querySelector<HTMLButtonElement>('form button[type="submit"]')!,
    error: () => target.querySelector<HTMLElement>('.error')!,
    badge: () => target.querySelector<HTMLElement>('[data-badge="paused"]'),
    banner: () => target.querySelector<HTMLElement>('[data-banner="reconnecting"]'),
    fullscreen: () => target.querySelector<HTMLButtonElement>('button[data-action="fullscreen"]'),
  };
}

/** Types `text` in the code field as a user does. */
async function type(view: ReturnType<typeof render>, text: string) {
  const input = view.input();
  input.value = text;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await tick();
}

async function submit(view: ReturnType<typeof render>) {
  view.submit().click();
  await tick();
}

/** Joins with the right code and waits for the stream to open. */
async function join(view: ReturnType<typeof render>) {
  await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
  await type(view, CODE);
  await submit(view);
  await vi.waitFor(() => expect(pc.eventRequests).toBeGreaterThanOrEqual(2));
}

describe('viewer page: the code', () => {
  it('shows the code form when the PC answers 401, with the field focused', async () => {
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    expect(view.main().dataset.screen).toBe('code');
    expect(document.activeElement).toBe(view.input());
    expect(view.submit().disabled).toBe(true);
    expect(view.target.querySelector('canvas')).toBeNull();
    expect(document.title).toBe('Ardoise : écran partagé');
  });

  it('shows the code in capitals, without spaces, and sends it on Enter', async () => {
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    await type(view, 'k7m q2x');
    expect(view.input().value).toBe('K7MQ2X');
    expect(view.submit().disabled).toBe(false);
    view.target.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(pc.joins).toEqual(['K7MQ2X']));
  });

  it('does not send an incomplete code', async () => {
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    await type(view, 'K7M');
    expect(view.submit().disabled).toBe(true);
    view.target.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }));
    await tick();
    expect(pc.joins).toEqual([]);
  });

  it('says a wrong code is wrong, with the attempts left, and keeps the form', async () => {
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    await type(view, 'AAAAAA');
    await submit(view);
    await vi.waitFor(() => expect(view.error().textContent).toContain('Il vous reste 4 essais'));
    expect(view.error().dataset.error).toBe('wrongCode');
    pc.remaining = 1;
    await submit(view);
    await vi.waitFor(() => expect(view.error().textContent).toContain('Il vous reste 0 essai.'));
    expect(view.view('code')).not.toBeNull();
  });

  it('says how long to wait after too many attempts, and blocks the button until then', async () => {
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    vi.useFakeTimers({ toFake: ['setInterval', 'clearInterval', 'Date'] });
    pc.joinOverride = () => Response.json({ error: 'tooManyAttempts', retryAfter: 30 }, { status: 429, headers: { 'Retry-After': '30' } });
    await type(view, 'AAAAAA');
    await submit(view);
    await vi.waitFor(() => expect(view.error().textContent).toContain('Réessayez dans 30 s'));
    expect(view.submit().disabled).toBe(true);
    await vi.advanceTimersByTimeAsync(10_000);
    flushSync();
    expect(view.error().textContent).toContain('Réessayez dans 20 s');
    await vi.advanceTimersByTimeAsync(20_500);
    flushSync();
    expect(view.error().textContent?.trim()).toBe('');
    expect(view.submit().disabled).toBe(false);
  });

  it('says when the PC cannot be reached, or refuses', async () => {
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    await type(view, 'AAAAAA');
    pc.joinOverride = () => {
      throw new TypeError('Failed to fetch');
    };
    await submit(view);
    await vi.waitFor(() => expect(view.error().dataset.error).toBe('network'));
    expect(view.error().textContent).toContain('Impossible de joindre');
    pc.joinOverride = () => new Response('', { status: 413 });
    await submit(view);
    await vi.waitFor(() => expect(view.error().dataset.error).toBe('error'));
    expect(view.error().textContent).toContain('413');
  });

  it('speaks English to an English browser', async () => {
    setLocale('en');
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    expect(view.submit().textContent?.trim()).toBe('Join');
    expect(document.title).toBe('Ardoise: shared screen');
    expect(document.documentElement.lang).toBe('en');
  });

  it('leaves the form as soon as the code is accepted, even if the stream is held back, and then says why it may be', async () => {
    pc.holdEvents = true;
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    await type(view, CODE);
    await submit(view);
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    // No second « Rejoindre » to press, which would only open one more stream.
    expect(view.view('code')).toBeNull();
    expect(view.view('connecting')).not.toBeNull();
    expect(view.target.querySelector('[data-hint="slow"]')).toBeNull();
    await vi.advanceTimersByTimeAsync(12_000);
    flushSync();
    expect(view.target.querySelector('[data-hint="slow"]')?.textContent).toContain('filtre web');
    expect(pc.joins).toEqual([CODE]);
  });

  it('asks again, and says why, if the code was right but the browser dropped the cookie', async () => {
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    // The PC accepts the code but never recognises the browser afterwards.
    const accept = pc.fetch.getMockImplementation()!;
    pc.fetch.mockImplementation(async (input, init) => {
      if (String(input) === '/join') return new Response(null, { status: 204 });
      if (String(input) === '/events') return new Response(null, { status: 401 });
      return accept(input, init);
    });
    await type(view, CODE);
    await submit(view);
    await vi.waitFor(() => expect(view.error().dataset.error).toBe('cookies'));
    expect(view.view('code')).not.toBeNull();
  });
});

describe('viewer page: the shared page', () => {
  it('shows the page after the code, then follows the note: strokes, appended strokes, pause, end', async () => {
    const view = render();
    await join(view);

    pc.send({ type: 'note', page: sharedPage([stroke(1000), stroke(3000)]), background: null });
    await vi.waitFor(() => expect(view.canvas()).not.toBeNull());
    expect(view.view('code')).toBeNull();
    expect(view.main().dataset.screen).toBe('note');
    expect(view.canvas()!.dataset.shown).toBe('note');
    expect(view.canvas()!.dataset.strokes).toBe('2');
    expect(view.canvas()!.dataset.page).toBe('1/1');
    expect(view.canvas()!.parentElement!.getAttribute('aria-label')).toBe("Page partagée par le PC d'Ardoise");

    pc.send({ type: 'noteStrokes', strokes: [stroke(6000)] });
    await vi.waitFor(() => expect(view.canvas()!.dataset.strokes).toBe('3'));

    pc.send({ type: 'pause', paused: true });
    await vi.waitFor(() => expect(view.badge()).not.toBeNull());
    expect(view.badge()!.textContent?.trim()).toBe('En pause');
    pc.send({ type: 'pause', paused: false });
    await vi.waitFor(() => expect(view.badge()).toBeNull());

    pc.send({ type: 'end' });
    await vi.waitFor(() => expect(view.view('ended')).not.toBeNull());
    expect(view.canvas()).toBeNull();
    expect(view.main().dataset.screen).toBe('ended');
  });

  it('shows live mode: the snapshot, then the batches', async () => {
    const view = render();
    await join(view);
    pc.send({
      type: 'live',
      reset: true,
      batch: { sentAt: 0, events: [{ type: 'snapshot', snapshot: liveSnapshot([stroke(1000)]) }] },
    });
    await vi.waitFor(() => expect(view.canvas()).not.toBeNull());
    expect(view.canvas()!.dataset.shown).toBe('live');
    expect(view.canvas()!.dataset.strokes).toBe('1');
    expect(view.canvas()!.dataset.page).toBe('1/2');

    pc.send({
      type: 'live',
      reset: false,
      batch: {
        sentAt: 0,
        events: [
          {
            type: 'strokeStart',
            pageIndex: 0,
            stroke: { ...stroke(2000), points: [{ x: 2000, y: 3000, p: 0.5, t: 0, recv: 1 }] },
          },
        ],
      },
    });
    await vi.waitFor(() => expect(view.canvas()!.dataset.strokes).toBe('2'));
  });

  it('draws the marks written over the presentation, on top of the page, and stops when they go', async () => {
    const contexts: FakeContext[] = [];
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
      const ctx = new FakeContext();
      contexts.push(ctx);
      return ctx as unknown as RenderingContext;
    });
    const red = { r: 216, g: 0, b: 0, a: 255 };
    /** Fills of the colour of the marks: the page itself is black ink on white. */
    const redFills = () => contexts.flatMap((c) => c.ops('fill')).filter((call) => call.fillStyle === 'rgb(216, 0, 0)').length;
    const send = (events: LiveEvent[], reset = false) =>
      pc.send({ type: 'live', reset, batch: { sentAt: 0, events } });
    const view = render();
    await join(view);
    send([{ type: 'snapshot', snapshot: liveSnapshot([stroke(1000)]) }], true);
    await vi.waitFor(() => expect(view.canvas()).not.toBeNull());
    await vi.waitFor(() => expect(contexts.some((c) => c.ops('fill').length > 0)).toBe(true));
    expect(redFills()).toBe(0);

    const points = Array.from({ length: 6 }, (_, i) => ({ x: 3000 + 400 * i, y: 8000 + 200 * i, p: 0.5 }));
    send([{ type: 'marks', marks: [{ tool: 'feltTip', color: red, width: 140, points }] }]);
    await vi.waitFor(() => expect(redFills()).toBeGreaterThan(0));
    // The marks are not strokes of the page.
    expect(view.canvas()!.dataset.strokes).toBe('1');

    // Gone: the next frames no longer draw them.
    send([{ type: 'marks', marks: [] }]);
    await new Promise((resolve) => setTimeout(resolve, 60));
    const before = redFills();
    send([{ type: 'view', view: { zoom: 2, x: 14850, y: 10800, board: false, laser: false } }]);
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(redFills()).toBe(before);
  });

  it('draws the page: the surround, the paper colour and the strokes', async () => {
    const contexts: FakeContext[] = [];
    vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
      const ctx = new FakeContext();
      contexts.push(ctx);
      return ctx as unknown as RenderingContext;
    });
    const view = render();
    await join(view);
    pc.send({ type: 'note', page: sharedPage([stroke(1000)]), background: null });
    await vi.waitFor(() => expect(view.canvas()).not.toBeNull());
    await vi.waitFor(() => expect(contexts.some((c) => c.calls.some((call) => call.op === 'fill'))).toBe(true));
    const ops = contexts.flatMap((c) => c.calls.map((call) => call.op));
    expect(ops).toContain('fillRect');
    expect(ops).toContain('drawImage');
  });

  describe('the paper of a live page', () => {
    const PICTURE = 'data:image/png;base64,QUJD';

    /** Images that load as soon as they are given a source. */
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
    }

    function recordContexts() {
      const contexts: FakeContext[] = [];
      vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
        const ctx = new FakeContext();
        contexts.push(ctx);
        return ctx as unknown as RenderingContext;
      });
      vi.stubGlobal('Image', FakeImage);
      return contexts;
    }

    /** The paper's images drawn since `from`, with the transform in force and the strokes drawn over them. */
    function papersDrawn(contexts: FakeContext[]) {
      const drawn: { args: unknown[]; transform: unknown[]; inkAfter: unknown[][] }[] = [];
      for (const ctx of contexts) {
        let transform: unknown[] = [];
        for (const [i, call] of ctx.calls.entries()) {
          if (call.op === 'setTransform') transform = call.args;
          if (call.op === 'drawImage' && call.args[0] instanceof FakeImage) {
            let after = transform;
            const inkAfter: unknown[][] = [];
            for (const later of ctx.calls.slice(i + 1)) {
              if (later.op === 'setTransform') after = later.args;
              if (later.op === 'fill') inkAfter.push(after);
            }
            drawn.push({ args: call.args, transform, inkAfter });
          }
        }
      }
      return drawn;
    }

    const live = (snapshot: LiveSnapshot, reset = true): ShareMessage => ({
      type: 'live',
      reset,
      batch: { events: [{ type: 'snapshot', snapshot }], sentAt: 0 },
    });

    it('draws it under the strokes, over the whole page in page units, then goes on a plain page', async () => {
      const contexts = recordContexts();
      const view = render();
      await join(view);
      const sheet = { ...liveSnapshot([stroke(1000)]), paperKey: 'k1' };
      pc.send(live(sheet));
      await vi.waitFor(() => expect(view.canvas()?.dataset.shown).toBe('live'));
      pc.send({ type: 'livePaper', key: 'k1', picture: PICTURE });

      await vi.waitFor(() => expect(papersDrawn(contexts).at(-1)?.inkAfter.length).toBeGreaterThan(0));
      const drawn = papersDrawn(contexts).at(-1)!;
      expect((drawn.args[0] as FakeImage).src).toBe(PICTURE);
      // The whole page, under the same transform as the strokes that follow: a stroke at a spot of
      // the page falls on the same spot of the picture, whatever the view.
      expect(drawn.args.slice(1)).toEqual([0, 0, A4.width, A4.height]);
      expect(drawn.inkAfter[0]).toEqual(drawn.transform);

      // The next page is plain: its snapshot alone takes the picture away.
      for (const ctx of contexts) ctx.reset();
      pc.send(live({ ...liveSnapshot([]), page: { ...liveSnapshot([]).page, id: 'p2' }, pageIndex: 1 }, false));
      await vi.waitFor(() => expect(view.canvas()?.dataset.page).toBe('2/2'));
      await new Promise((resolve) => setTimeout(resolve, 60));
      expect(papersDrawn(contexts)).toEqual([]);
    });

    it('shows no picture on an ordinary page', async () => {
      const contexts = recordContexts();
      const view = render();
      await join(view);
      pc.send(live(liveSnapshot([stroke(1000)])));
      await vi.waitFor(() => expect(contexts.some((c) => c.count('fill') > 0)).toBe(true));
      expect(papersDrawn(contexts)).toEqual([]);
    });
  });

  it('shows the waiting screen when the PC shares nothing', async () => {
    const view = render();
    await join(view);
    pc.send({ type: 'waiting' });
    await vi.waitFor(() => expect(view.view('waiting')).not.toBeNull());
    expect(view.view('waiting')!.textContent).toContain("ne partage rien pour l'instant");
    expect(view.canvas()).toBeNull();
  });

  it('says "reconnecting" over the last image when the connection is cut, then goes on', async () => {
    const view = render();
    await join(view);
    pc.send({ type: 'note', page: sharedPage([stroke(1000)]), background: null });
    await vi.waitFor(() => expect(view.canvas()).not.toBeNull());
    pc.cut();
    await vi.waitFor(() => expect(view.banner()).not.toBeNull());
    expect(view.canvas()).not.toBeNull();
    // The PC answers again after the first delay (0.5 s) with the page it shows now.
    await vi.waitFor(() => expect(view.banner()).toBeNull(), { timeout: 3000 });
    pc.send({ type: 'note', page: sharedPage([stroke(1000), stroke(2000)]), background: null });
    await vi.waitFor(() => expect(view.canvas()!.dataset.strokes).toBe('2'));
  });

  it('says there are too many screens on 503, and goes on when there is room', async () => {
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    pc.eventsStatus = 503;
    await type(view, CODE);
    await submit(view);
    await vi.waitFor(() => expect(view.view('too-many')).not.toBeNull());
    expect(view.view('too-many')!.textContent).toContain("Trop d'écrans connectés");
    expect(view.main().dataset.screen).toBe('tooMany');
  });

  it('asks the code again when the share restarts', async () => {
    const view = render();
    await join(view);
    pc.send({ type: 'note', page: sharedPage([stroke(1000)]), background: null });
    await vi.waitFor(() => expect(view.canvas()).not.toBeNull());
    // The share stops, and a new one starts: the old permission is forgotten.
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    pc.send({ type: 'end' });
    await vi.advanceTimersByTimeAsync(0);
    flushSync();
    expect(view.view('ended')).not.toBeNull();
    pc.joined = false;
    await vi.advanceTimersByTimeAsync(5000);
    flushSync();
    expect(view.view('code')).not.toBeNull();
  });
});

describe('viewer page: full screen', () => {
  function allowFullscreen() {
    Object.defineProperty(document, 'fullscreenEnabled', { configurable: true, value: true });
    const request = vi.fn(() => Promise.resolve());
    document.documentElement.requestFullscreen = request;
    return request;
  }

  it('has no button where the browser has no Fullscreen API, nor before there is a page', async () => {
    const view = render();
    await join(view);
    pc.send({ type: 'note', page: sharedPage([]), background: null });
    await vi.waitFor(() => expect(view.canvas()).not.toBeNull());
    expect(view.fullscreen()).toBeNull();
  });

  it('goes full screen with the button and with a double click, and says how to leave', async () => {
    const request = allowFullscreen();
    const view = render();
    await vi.waitFor(() => expect(view.view('code')).not.toBeNull());
    expect(view.fullscreen()).toBeNull();
    await join(view);
    pc.send({ type: 'note', page: sharedPage([]), background: null });
    await vi.waitFor(() => expect(view.fullscreen()).not.toBeNull());
    expect(view.fullscreen()!.textContent?.trim()).toBe('Plein écran');

    view.fullscreen()!.click();
    expect(request).toHaveBeenCalledTimes(1);
    view.canvas()!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    expect(request).toHaveBeenCalledTimes(2);
    // A double click on the button itself is not a second toggle.
    view.fullscreen()!.dispatchEvent(new MouseEvent('dblclick', { bubbles: true }));
    expect(request).toHaveBeenCalledTimes(2);

    Object.defineProperty(document, 'fullscreenElement', { configurable: true, value: document.documentElement });
    document.dispatchEvent(new Event('fullscreenchange'));
    await tick();
    expect(view.fullscreen()!.textContent?.trim()).toBe('Quitter le plein écran');
    Object.defineProperty(document, 'fullscreenElement', { configurable: true, value: null });
  });

  it('hides the button and the cursor after 2 s without moving, and shows them when the mouse moves', async () => {
    allowFullscreen();
    const view = render();
    await join(view);
    pc.send({ type: 'note', page: sharedPage([]), background: null });
    await vi.waitFor(() => expect(view.fullscreen()).not.toBeNull());
    expect(view.main().classList.contains('idle')).toBe(false);

    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    window.dispatchEvent(new MouseEvent('mousemove'));
    await vi.advanceTimersByTimeAsync(1900);
    flushSync();
    expect(view.fullscreen()).not.toBeNull();
    await vi.advanceTimersByTimeAsync(200);
    flushSync();
    expect(view.main().classList.contains('idle')).toBe(true);
    expect(view.fullscreen()).toBeNull();

    window.dispatchEvent(new MouseEvent('mousemove'));
    flushSync();
    expect(view.main().classList.contains('idle')).toBe(false);
    expect(view.fullscreen()).not.toBeNull();
  });
});

describe("viewer page: this screen's own view", () => {
  const bar = (view: ReturnType<typeof render>, action: string) =>
    view.target.querySelector<HTMLButtonElement>(`.controls button[data-action="${action}"]`);
  const click = async (view: ReturnType<typeof render>, action: string) => {
    bar(view, action)!.click();
    await tick();
  };
  const data = (view: ReturnType<typeof render>) => {
    const d = view.canvas()!.dataset;
    return { fit: d.fit, zoom: Number(d.zoom), scale: Number(d.scale) };
  };
  const key = async (init: KeyboardEventInit & { key: string }, target: EventTarget = window) => {
    const event = new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init });
    target.dispatchEvent(event);
    await tick();
    return event;
  };
  const sendLive = (zoom = 1) => {
    const snapshot = liveSnapshot([stroke(1000)]);
    pc.send({
      type: 'live',
      reset: true,
      batch: { sentAt: 0, events: [{ type: 'snapshot', snapshot: { ...snapshot, view: { ...snapshot.view, zoom } } }] },
    });
  };
  const pcView = (zoom: number) =>
    pc.send({
      type: 'live',
      reset: false,
      batch: { sentAt: 0, events: [{ type: 'view', view: { zoom, x: 14850, y: 10800, board: false, laser: false } }] },
    });
  /** A live sheet drawn, the controls shown. */
  async function liveSheet() {
    const view = render();
    await join(view);
    sendLive(2);
    await vi.waitFor(() => expect(view.canvas()).not.toBeNull());
    return view;
  }
  type Touch = { pointerType?: string; pointerId?: number; t?: number };
  const pointer = (target: EventTarget, type: string, at: { x: number; y: number }, init: Touch = {}) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: at.x, clientY: at.y, button: 0, buttons: 1 });
    const props = { pointerType: init.pointerType ?? 'mouse', pointerId: init.pointerId ?? 1, pressure: 0.5, timeStamp: init.t ?? 0 };
    for (const [name, value] of Object.entries(props)) Object.defineProperty(event, name, { value, configurable: true });
    target.dispatchEvent(event);
  };

  it('has the controls in French: whole page, full width, zoom out and in; nothing to follow yet', async () => {
    const view = await liveSheet();
    expect(view.target.querySelector('.controls')?.getAttribute('role')).toBe('toolbar');
    expect(bar(view, 'fit-page')!.textContent?.trim()).toBe('Page entière');
    expect(bar(view, 'fit-width')!.textContent?.trim()).toBe('Pleine largeur');
    expect(bar(view, 'zoom-out')!.getAttribute('aria-label')).toBe('Réduire');
    expect(bar(view, 'zoom-in')!.getAttribute('aria-label')).toBe('Agrandir');
    expect(bar(view, 'follow')).toBeNull();
    expect(data(view)).toMatchObject({ fit: 'follow', zoom: 2 });
  });

  it('has the controls in English for an English browser', async () => {
    setLocale('en');
    const view = await liveSheet();
    expect(bar(view, 'fit-page')!.textContent?.trim()).toBe('Whole page');
    expect(bar(view, 'fit-width')!.textContent?.trim()).toBe('Full width');
    expect(bar(view, 'zoom-out')!.getAttribute('aria-label')).toBe('Zoom out');
    expect(bar(view, 'zoom-in')!.getAttribute('aria-label')).toBe('Zoom in');
    await click(view, 'fit-width');
    expect(bar(view, 'follow')!.textContent?.trim()).toBe('Follow the PC');
  });

  it('follows the view of the PC until a control is used, then keeps its own until « Suivre le PC »', async () => {
    const view = await liveSheet();
    expect(data(view)).toMatchObject({ fit: 'follow', zoom: 2 });
    pcView(3);
    await vi.waitFor(() => expect(data(view).zoom).toBe(3));

    await click(view, 'fit-width');
    // The nominal 1280 × 720 window: the page across its width.
    expect(data(view).fit).toBe('width');
    expect(data(view).scale * A4.width).toBeCloseTo(1280, 0);
    expect(bar(view, 'fit-width')!.getAttribute('aria-pressed')).toBe('true');
    expect(bar(view, 'follow')).not.toBeNull();

    const own = data(view);
    pcView(5);
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(data(view)).toEqual(own);

    await click(view, 'follow');
    await vi.waitFor(() => expect(data(view)).toMatchObject({ fit: 'follow', zoom: 5 }));
    expect(bar(view, 'follow')).toBeNull();
  });

  it('shows the whole page, and zooms with the buttons', async () => {
    const view = await liveSheet();
    await click(view, 'fit-page');
    expect(data(view)).toMatchObject({ fit: 'page', zoom: 1 });
    expect(bar(view, 'fit-page')!.getAttribute('aria-pressed')).toBe('true');
    const whole = data(view).scale;
    await click(view, 'zoom-in');
    expect(data(view).fit).toBe('free');
    expect(data(view).scale).toBeGreaterThan(whole);
    await click(view, 'zoom-out');
    expect(data(view)).toMatchObject({ fit: 'page', zoom: 1 });
    expect(data(view).scale).toBeCloseTo(whole, 6);
  });

  it('has no « Suivre le PC » for a note: the whole page by default, and the buttons still work', async () => {
    const view = render();
    await join(view);
    pc.send({ type: 'note', page: sharedPage([stroke(1000)]), background: null });
    await vi.waitFor(() => expect(view.canvas()).not.toBeNull());
    expect(data(view)).toMatchObject({ fit: 'page', zoom: 1 });
    await click(view, 'fit-width');
    expect(data(view).fit).toBe('width');
    expect(data(view).scale * A4.width).toBeCloseTo(1280, 0);
    expect(bar(view, 'follow')).toBeNull();
    await click(view, 'fit-page');
    expect(data(view)).toMatchObject({ fit: 'page', zoom: 1 });
  });

  it('answers the keys: 0, Home, W, + and -, Esc', async () => {
    const view = await liveSheet();
    await key({ key: 'w' });
    expect(data(view).fit).toBe('width');
    await key({ key: '0' });
    expect(data(view)).toMatchObject({ fit: 'page', zoom: 1 });
    await key({ key: '+' });
    expect(data(view).fit).toBe('free');
    const zoomed = data(view).zoom;
    expect(zoomed).toBeCloseTo(1.25, 6);
    await key({ key: '+' });
    expect(data(view).zoom).toBeGreaterThan(zoomed);
    await key({ key: '-' });
    expect(data(view).zoom).toBeCloseTo(zoomed, 6);
    await key({ key: 'W' });
    expect(data(view).fit).toBe('width');
    await key({ key: 'Home' });
    expect(data(view).fit).toBe('page');
    await key({ key: 'Escape' });
    expect(data(view).fit).toBe('follow');
    expect(bar(view, 'follow')).toBeNull();
  });

  it('leaves alone the keys of the browser, of a focused button, and of a field', async () => {
    const view = await liveSheet();
    const ctrl = await key({ key: '0', ctrlKey: true });
    expect(ctrl.defaultPrevented).toBe(false);
    const space = await key({ key: ' ' }, bar(view, 'fit-page')!);
    expect(space.defaultPrevented).toBe(false);
    expect(data(view).fit).toBe('follow');
    const field = document.createElement('input');
    document.body.append(field);
    const typed = await key({ key: 'w' }, field);
    field.remove();
    expect(typed.defaultPrevented).toBe(false);
    expect(data(view).fit).toBe('follow');
  });

  it('moves with Shift + arrows once zoomed', async () => {
    const view = await liveSheet();
    await key({ key: '0' });
    await key({ key: '+' });
    await key({ key: '+' });
    const before = view.canvas()!.dataset.scale;
    await key({ key: 'ArrowRight', shiftKey: true });
    expect(data(view).fit).toBe('free');
    expect(view.canvas()!.dataset.scale).toBe(before);
  });

  it('goes full screen with F', async () => {
    Object.defineProperty(document, 'fullscreenEnabled', { configurable: true, value: true });
    const request = vi.fn(() => Promise.resolve());
    document.documentElement.requestFullscreen = request;
    await liveSheet();
    await key({ key: 'f' });
    expect(request).toHaveBeenCalledTimes(1);
  });

  it('zooms with the wheel, without scrolling the page', async () => {
    const view = await liveSheet();
    await click(view, 'fit-page');
    const whole = data(view);
    const event = new WheelEvent('wheel', { bubbles: true, cancelable: true, deltaY: -300, clientX: 900, clientY: 200 });
    view.canvas()!.dispatchEvent(event);
    await tick();
    expect(event.defaultPrevented).toBe(true);
    const zoomed = data(view);
    expect(zoomed.fit).toBe('free');
    expect(zoomed.zoom).toBeGreaterThan(1.3);
    expect(zoomed.scale / whole.scale).toBeCloseTo(zoomed.zoom, 3);
  });

  it('moves the page with a mouse drag, but a drag on a page that fits stays with the PC', async () => {
    const view = await liveSheet();
    const canvas = view.canvas()!;
    // The PC shows zoom 2: dragging moves the area, which is an own view.
    pointer(canvas, 'pointerdown', { x: 600, y: 400 });
    await tick();
    expect(canvas.classList.contains('dragging')).toBe(true);
    pointer(canvas, 'pointermove', { x: 500, y: 350 });
    pointer(canvas, 'pointerup', { x: 500, y: 350 });
    await tick();
    expect(data(view).fit).toBe('free');
    expect(canvas.classList.contains('dragging')).toBe(false);
    await click(view, 'follow');
    expect(data(view).fit).toBe('follow');

    pcView(1);
    await vi.waitFor(() => expect(data(view).zoom).toBe(1));
    pointer(canvas, 'pointerdown', { x: 600, y: 400 });
    pointer(canvas, 'pointermove', { x: 500, y: 350 });
    pointer(canvas, 'pointerup', { x: 500, y: 350 });
    await tick();
    expect(data(view).fit).toBe('follow');
  });

  it('moves with one finger and zooms with two', async () => {
    const view = await liveSheet();
    const canvas = view.canvas()!;
    // One finger: past the slop, then the page follows it.
    pointer(canvas, 'pointerdown', { x: 600, y: 400 }, { pointerType: 'touch', pointerId: 1, t: 0 });
    pointer(canvas, 'pointermove', { x: 560, y: 380 }, { pointerType: 'touch', pointerId: 1, t: 10 });
    pointer(canvas, 'pointermove', { x: 500, y: 350 }, { pointerType: 'touch', pointerId: 1, t: 20 });
    pointer(canvas, 'pointerup', { x: 500, y: 350 }, { pointerType: 'touch', pointerId: 1, t: 30 });
    await tick();
    expect(data(view).fit).toBe('free');
    await click(view, 'fit-page');
    const whole = data(view);

    // Two fingers moving apart: zoom in.
    pointer(canvas, 'pointerdown', { x: 600, y: 360 }, { pointerType: 'touch', pointerId: 1, t: 100 });
    pointer(canvas, 'pointerdown', { x: 680, y: 360 }, { pointerType: 'touch', pointerId: 2, t: 105 });
    pointer(canvas, 'pointermove', { x: 560, y: 360 }, { pointerType: 'touch', pointerId: 1, t: 120 });
    pointer(canvas, 'pointermove', { x: 720, y: 360 }, { pointerType: 'touch', pointerId: 2, t: 125 });
    pointer(canvas, 'pointerup', { x: 560, y: 360 }, { pointerType: 'touch', pointerId: 1, t: 130 });
    pointer(canvas, 'pointerup', { x: 720, y: 360 }, { pointerType: 'touch', pointerId: 2, t: 135 });
    await tick();
    expect(data(view).fit).toBe('free');
    expect(data(view).zoom).toBeGreaterThan(whole.zoom);
  });

  it('hides the controls with the cursor, keeps the keys, and shows the controls when the mouse moves', async () => {
    const view = await liveSheet();
    expect(view.target.querySelector('.controls')).not.toBeNull();
    vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] });
    window.dispatchEvent(new MouseEvent('mousemove'));
    await vi.advanceTimersByTimeAsync(2100);
    flushSync();
    expect(view.target.querySelector('.controls')).toBeNull();
    await key({ key: 'w' });
    expect(data(view).fit).toBe('width');
    window.dispatchEvent(new MouseEvent('mousemove'));
    flushSync();
    expect(view.target.querySelector('.controls')).not.toBeNull();
  });
});
