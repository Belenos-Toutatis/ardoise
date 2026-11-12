// @vitest-environment jsdom
/// <reference types="node" />
// The editor's projection window (M17, docs/design/paper-projection-narration.md §2.5): it subscribes to the projection,
// draws the editor's page and its stroke in progress as live's presentation draws its own, takes nothing from the pointer
// (O6), closes on Esc, Ctrl+W, Ctrl+F4 and Ctrl+Q, and asks for the editor's paper with the staged pixels' key.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale, t } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import { resetMockLive } from '../../lib/mock-live';
import { mockProjectionInvoke, mockProjectionState, resetMockProjection } from '../../lib/mock-projection';
import type { BackgroundImage, Note, Stroke } from '../../lib/note';
import type { ProjectionUpdate } from '../../lib/projection';
import EditorPresentation from './EditorPresentation.svelte';

vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

const BLACK = { r: 0, g: 0, b: 0, a: 255 };
const VIEW = { zoom: 1, x: 14850, y: 10800 };
const IMAGE: BackgroundImage = { name: 'fiche.pdf', pdfPage: 1, rect: { x: 1000, y: 0, width: 15000, height: 21600 }, quarterTurns: 0, opacity: 1 };

let cleanup: (() => void) | null = null;

let made = 0;
function stroke(layer = 0): Stroke {
  const t0 = (made += 100);
  return { tool: 'ballpoint', color: BLACK, width: 60, layer, points: [0, 1, 2].map((i) => ({ x: 2000 + t0 + i * 40, y: 3000, pressure: 0.5, t: t0 + i * 10 })) };
}

function note(strokes: Stroke[], over: Partial<Note> = {}): Note {
  return {
    id: 'note-e',
    title: '',
    created: 0,
    modified: 0,
    page: { width: 29700, height: 21600 },
    background: { paper: 'white', template: 'blank', texture: false },
    tags: [],
    layers: [
      { name: 'Calque 1', visible: true },
      { name: 'Calque 2', visible: true },
    ],
    strokes,
    timing: 'recorded',
    origin: { kind: 'live' },
    ...over,
  };
}

const update = (u: ProjectionUpdate) => mockProjectionInvoke('projection_update', { update: u });
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  setLocale('fr');
  resetMockLive();
  resetMockProjection();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  resetMockProjection();
  vi.restoreAllMocks();
});

async function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(EditorPresentation, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await flush();
  await tick();
  const main = () => target.querySelector<HTMLElement>('main.projection')!;
  const canvas = () => target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
  const drawn = () => Number(canvas().dataset.drawn);
  return { target, main, canvas, drawn };
}

const press = (key: string, init: KeyboardEventInit = {}) =>
  window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));

describe('the editor’s projection window', () => {
  it('subscribes and shows only the page, black until the editor sends it', async () => {
    const view = await render();
    expect(mockProjectionState().subscribed).toBe(true);
    expect(view.main().dataset.state).toBe('idle');
    expect(view.main().getAttribute('aria-label')).toBe(t('presentation.editorTitle'));
    expect(view.target.querySelector('[role="img"]')?.getAttribute('aria-label')).toBe(t('presentation.editorCanvas'));
    expect(view.target.querySelectorAll('button, input, select, textarea, a, [tabindex]')).toHaveLength(0);
    expect(view.target.querySelectorAll('canvas')).toHaveLength(1);
    expect(window.__ardoiseLatency?.computer).toBeDefined();
  });

  it('draws the strokes, the stroke in progress, and only the visible layers', async () => {
    const view = await render();
    const strokes = [stroke(0), stroke(0), stroke(1)];
    update({ kind: 'page', note: note(strokes), imageKey: null, view: VIEW });
    await tick();
    expect(view.main().dataset.state).toBe('live');
    expect(view.drawn()).toBe(3);
    update({ kind: 'strokeBegin', id: 1, tool: 'feltTip', color: BLACK, width: 80, layer: 0, points: [{ x: 9000, y: 9000, p: 0.5, t: 900, wall: Date.now() }] });
    await tick();
    expect(view.drawn()).toBe(4);
    update({ kind: 'strokePoints', id: 1, points: [{ x: 9100, y: 9000, p: 0.5, t: 908, wall: Date.now() }] });
    const finished = stroke(0);
    update({ kind: 'strokes', keep: 3, tail: [finished], ended: 1 });
    await tick();
    expect(view.drawn()).toBe(4);
    expect(Number(view.canvas().dataset.strokes)).toBe(4);
    // The second layer hidden: the page again, one stroke fewer drawn.
    update({ kind: 'page', note: note([...strokes, finished], { layers: [{ name: 'Calque 1', visible: true }, { name: 'Calque 2', visible: false }] }), imageKey: null, view: VIEW });
    await tick();
    expect(view.drawn()).toBe(3);
  });

  it('follows the view of the projected area', async () => {
    const view = await render();
    update({ kind: 'page', note: note([]), imageKey: null, view: VIEW });
    update({ kind: 'view', view: { zoom: 1.563, x: 12000, y: 9000 } });
    await tick();
    expect(Number(view.canvas().dataset.zoom)).toBe(1.563);
  });

  it('closes on Esc, Ctrl+W, Ctrl+F4 and Ctrl+Q, and on nothing else', async () => {
    const close = vi.spyOn(api, 'projectionClose').mockResolvedValue(undefined);
    await render();
    for (const key of ['b', 'l', 'PageDown', '+', 'w', 'Escape']) {
      const init = key === 'Escape' ? { shiftKey: true } : {};
      press(key, init);
    }
    expect(close).not.toHaveBeenCalled();
    press('Escape');
    press('w', { ctrlKey: true });
    press('F4', { ctrlKey: true });
    press('q', { ctrlKey: true });
    expect(close).toHaveBeenCalledTimes(4);
  });

  it('takes nothing from the pointer: no marks, no pinch (the pen writes in the editor itself)', async () => {
    const view = await render();
    update({ kind: 'page', note: note([stroke()]), imageKey: null, view: VIEW });
    await tick();
    const canvas = view.canvas();
    expect(canvas.classList.contains('writable')).toBe(false);
    for (const type of ['pointerdown', 'pointermove', 'pointermove', 'pointerup']) {
      const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: 300, clientY: 200, buttons: type === 'pointerup' ? 0 : 1 });
      for (const [key, value] of Object.entries({ pointerType: 'pen', pointerId: 1, pressure: 0.5 })) Object.defineProperty(event, key, { value });
      canvas.dispatchEvent(event);
    }
    await tick();
    expect(view.drawn()).toBe(1);
    expect(view.target.querySelector('[data-action="clear-marks"]')).toBeNull();
  });

  it('asks for the editor’s paper with the key of its staged pixels, and the paper alone without them', async () => {
    const render$ = vi.spyOn(api, 'renderBackground');
    await render();
    update({ kind: 'page', note: note([], { backgroundImage: IMAGE }), imageKey: 'staged-3', view: VIEW });
    await vi.waitFor(() => expect(render$.mock.calls.some((c) => c[3]?.key === 'staged-3')).toBe(true));
    const call = render$.mock.calls.find((c) => c[3]?.key === 'staged-3')!;
    expect(call[3]).toEqual({ image: IMAGE, key: 'staged-3' });
    expect(call[1]).toEqual({ paper: 'white', template: 'blank', texture: false });
    // Another note whose image is not staged yet: the paper colour, no image asked for.
    render$.mockClear();
    update({ kind: 'page', note: note([], { id: 'note-f', backgroundImage: IMAGE, background: { paper: 'cream', template: 'lined', texture: false } }), imageKey: null, view: VIEW });
    await vi.waitFor(() => expect(render$).toHaveBeenCalled());
    expect(render$.mock.calls.at(-1)?.[3]).toBeUndefined();
    expect(render$.mock.calls.at(-1)?.[1]).toEqual({ paper: 'cream', template: 'lined', texture: false });
  });
});
