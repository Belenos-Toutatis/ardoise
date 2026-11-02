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
