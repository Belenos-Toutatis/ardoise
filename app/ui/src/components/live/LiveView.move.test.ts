// @vitest-environment jsdom
/// <reference types="node" />
// SPEC §4.2 « Retoucher pendant le direct »: moving and resizing the selection in the control window, against the
// browser stand-in of the session (a resumed sheet of two strokes): the edit sent through the real command, the
// selection that stays through the change of revision, one step of undo per move, the texts, the gestures dropped.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { FakeContext } from '../../lib/live-fake-canvas';
import type { LiveSnapshot } from '../../lib/live';
import { fitTransform, pageToScreen } from '../../lib/live-view';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, resetMockLive } from '../../lib/mock-live';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockTablet } from '../../lib/mock-tablet';
import { selectionFrame } from '../../lib/selection-grab';
import { tablet } from '../../lib/tablet-state.svelte';
import LiveView from './LiveView.svelte';

vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

const app: AppState = {
  version: '0.1.0',
  machine: 'PC-TEST',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: 'C:\\demo\\Ardoise',
  systemLocale: 'fr-FR',
  selfTest: false,
  openDiagnostic: false,
  autoExport: { format: 'off', folder: 'Exports' },
  microphone: null,
  shapes: { tolerance: 3, minSizeMm: 10, snapAngle: true, snapAngleDeg: 4, snapRound: true, snapRoundPercent: 10 },
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLive();
  resetMockTablet();
  resetMockNarration();
  resetMockLibrary();
  tablet.reset();
  setLocale('fr');
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 30;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  resetMockLive();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const sheetPath = () => [...mockLibrary().notes].find(([, s]) => s.note.title === 'Cours de physique')![0];
const PAGE = { width: 29700, height: 21600 };
const TRANSFORM = fitTransform(PAGE, { width: 800, height: 560 }, 12);
const UNIT = TRANSFORM.scale;
const at = (x: number, y: number) => pageToScreen(TRANSFORM, { x, y });

interface Init {
  pointerType?: string;
  pointerId?: number;
  t?: number;
}

function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(LiveView, { target, props: { app, resume: sheetPath(), onclose: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const root = () => target.querySelector<HTMLElement>('.live-view')!;
  const canvas = () => target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
  const action = (name: string) => target.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`)!;
  const bar = () => target.querySelector<HTMLElement>('[data-slot="select-bar"]');
  const fire = (type: string, p: { x: number; y: number }, init: Init = {}) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: p.x, clientY: p.y, button: 0, buttons: type === 'pointerup' ? 0 : 1 });
    const props: Record<string, unknown> = { pointerType: init.pointerType ?? 'mouse', pointerId: init.pointerId ?? 1, pressure: 0.5, timeStamp: init.t ?? 0 };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    canvas().dispatchEvent(event);
  };
  const drag = (from: { x: number; y: number }, to: { x: number; y: number }, init: Init = {}) => {
    fire('pointerdown', from, init);
    for (const k of [0.5, 1]) fire('pointermove', { x: from.x + (to.x - from.x) * k, y: from.y + (to.y - from.y) * k }, init);
    fire('pointerup', to, init);
  };
  /** A lasso round the first stroke (the wave at y = 3000). */
  const first = () => {
    const corners = [at(1500, 1800), at(22500, 1800), at(22500, 4200), at(1500, 4200)];
    fire('pointerdown', corners[0]);
    for (const corner of corners.slice(1)) fire('pointermove', corner);
    fire('pointermove', at(1500, 1810));
    fire('pointerup', at(1500, 1810));
  };
  return { target, root, canvas, action, bar, fire, drag, first };
}

const press = (key: string, init: KeyboardEventInit = {}): KeyboardEvent => {
  const event = new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init });
  window.dispatchEvent(event);
  return event;
};

/** The page as the session holds it. */
const page = () => (mockLiveInvoke('live_start', {}) as LiveSnapshot).page;

/** The view with the Select tool on and the first stroke chosen, and where its frame is on the screen. */
async function selected() {
  const view = render();
  await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
  press('v');
  await tick();
  view.first();
  await vi.waitFor(() => expect(view.bar()).not.toBeNull());
  const box = selectionFrame(page().strokes, page().layers, [0])!;
  const a = at(box.x, box.y);
  const b = at(box.x + box.width, box.y + box.height);
  const frame = { l: a.x, t: a.y, r: b.x, b: b.y, cx: (a.x + b.x) / 2, cy: (a.y + b.y) / 2 };
  return { view, box, frame };
}

describe('moving the selection in the control window', () => {
  it('a drag inside the frame moves the chosen stroke only, the selection stays, and the edit is one step', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const { view, frame } = await selected();
    const before = structuredClone(page().strokes);
    view.drag({ x: frame.cx, y: frame.cy }, { x: frame.cx + 80, y: frame.cy + 40 });
    await vi.waitFor(() => expect(edit).toHaveBeenCalledOnce());
    const [pageIndex, , change] = edit.mock.calls[0];
    expect(pageIndex).toBe(0);
    expect(change).toMatchObject({ kind: 'transform', indices: [0], scale: 1 });
    if (change.kind !== 'transform') throw new Error('not a transform');
    expect(change.x).toBeCloseTo(80 / UNIT, 6);
    expect(change.y).toBeCloseTo(40 / UNIT, 6);

    // The session has the moved stroke at the same place of the list, the other one is untouched.
    await vi.waitFor(() => expect(page().strokes[0].points[0].x).toBeCloseTo(before[0].points[0].x + change.x, 6));
    expect(page().strokes[1]).toEqual(before[1]);
    expect(page().strokes).toHaveLength(2);
    // The selection is still there, and so is its bar.
    await tick();
    expect(view.bar()!.textContent).toContain('Traits sélectionnés : 1');
    await vi.waitFor(() => expect(view.action('undo-edit').disabled).toBe(false));
  });

  it('can be moved again at once, and the next edit is not refused as stale', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const { view, frame } = await selected();
    const start = page().strokes[0].points[0].x;
    view.drag({ x: frame.cx, y: frame.cy }, { x: frame.cx + 60, y: frame.cy });
    await vi.waitFor(() => expect(page().strokes[0].points[0].x).toBeGreaterThan(start + 1));
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('2'));
    // The frame is now 60 pixels to the right.
    const moved = frame.cx + 60;
    const y = page().strokes[0].points[0].y;
    view.drag({ x: moved, y: frame.cy }, { x: moved, y: frame.cy + 50 });
    await vi.waitFor(() => expect(edit).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(page().strokes[0].points[0].y).toBeCloseTo(y + 50 / UNIT, 3));
    expect(view.target.querySelector('[role="alert"]')).toBeNull();
    expect(view.bar()!.textContent).toContain('Traits sélectionnés : 1');
  });

  it('a drag from a corner resizes around the opposite one, and the widths follow', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const { view, box, frame } = await selected();
    const before = structuredClone(page().strokes[0]);
    // The bottom right corner onto the middle of the diagonal: half the size around the top left corner.
    view.drag({ x: frame.r, y: frame.b }, { x: frame.cx, y: frame.cy });
    await vi.waitFor(() => expect(edit).toHaveBeenCalledOnce());
    const change = edit.mock.calls[0][2];
    expect(change).toMatchObject({ kind: 'transform', indices: [0] });
    if (change.kind !== 'transform') throw new Error('not a transform');
    expect(change.scale).toBeCloseTo(0.5, 6);
    expect(change.x).toBeCloseTo(box.x * 0.5, 3);
    expect(change.y).toBeCloseTo(box.y * 0.5, 3);
    await vi.waitFor(() => expect(page().strokes[0].width).toBeCloseTo(before.width * 0.5, 6));
    // The top left corner of the frame did not move: the ink starts where it did, once the width's half is counted.
    const after = page().strokes[0];
    expect(after.points[0].x - after.width / 2).toBeCloseTo(box.x, 3);
    expect(view.bar()).not.toBeNull();
  });

  it('Ctrl+Z puts the stroke back, in one step, and the selection goes with the undone edit', async () => {
    const { view, frame } = await selected();
    const before = structuredClone(page().strokes);
    view.drag({ x: frame.cx, y: frame.cy }, { x: frame.cx + 80, y: frame.cy + 40 });
    await vi.waitFor(() => expect(page().strokes[0]).not.toEqual(before[0]));
    await vi.waitFor(() => expect(view.action('undo-edit').disabled).toBe(false));
    expect(press('z', { ctrlKey: true }).defaultPrevented).toBe(true);
    await vi.waitFor(() => expect(page().strokes).toEqual(before));
    await vi.waitFor(() => expect(view.action('undo-edit').disabled).toBe(true));
    // One step: nothing more to undo; and it can be redone.
    expect(view.action('redo-edit').disabled).toBe(false);
    press('y', { ctrlKey: true });
    await vi.waitFor(() => expect(page().strokes[0]).not.toEqual(before[0]));
  });

  it('a refused edit leaves the stroke where it was, tells why, and keeps the selection to try again', async () => {
    vi.spyOn(api, 'liveEditStrokes').mockRejectedValueOnce({ code: 'liveEditInvalid', message: 'invalid' });
    const { view, frame } = await selected();
    const before = structuredClone(page().strokes);
    view.drag({ x: frame.cx, y: frame.cy }, { x: frame.cx + 80, y: frame.cy + 40 });
    await vi.waitFor(() => expect(view.target.querySelector('[role="alert"]')?.textContent).toContain("Cette retouche n'est pas possible"));
    expect(page().strokes).toEqual(before);
    expect(view.bar()).not.toBeNull();
  });

  it('a stale edit drops the selection and says so', async () => {
    vi.spyOn(api, 'liveEditStrokes').mockRejectedValueOnce({ code: 'liveEditStale', message: 'stale' });
    const { view, frame } = await selected();
    view.drag({ x: frame.cx, y: frame.cy }, { x: frame.cx + 80, y: frame.cy + 40 });
    await vi.waitFor(() => expect(view.bar()).toBeNull());
    expect(view.target.querySelector('[role="alert"]')?.textContent).toContain('La page a changé entre-temps');
  });

  it('Esc drops the drag under way: nothing is sent, and the selection stays', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const { view, frame } = await selected();
    view.fire('pointerdown', { x: frame.cx, y: frame.cy });
    view.fire('pointermove', { x: frame.cx + 60, y: frame.cy + 30 });
    expect(press('Escape').defaultPrevented).toBe(true);
    view.fire('pointermove', { x: frame.cx + 90, y: frame.cy + 30 });
    view.fire('pointerup', { x: frame.cx + 90, y: frame.cy + 30 });
    await tick();
    expect(edit).not.toHaveBeenCalled();
    expect(view.bar()).not.toBeNull();
    // Esc again deselects, as before.
    press('Escape');
    await vi.waitFor(() => expect(view.bar()).toBeNull());
  });

  it('turning the tool off (V) drops the drag under way', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const { view, frame } = await selected();
    view.fire('pointerdown', { x: frame.cx, y: frame.cy });
    view.fire('pointermove', { x: frame.cx + 60, y: frame.cy + 30 });
    press('v');
    await tick();
    view.fire('pointermove', { x: frame.cx + 90, y: frame.cy + 30 });
    view.fire('pointerup', { x: frame.cx + 90, y: frame.cy + 30 });
    await tick();
    expect(edit).not.toHaveBeenCalled();
    expect(view.bar()).toBeNull();
  });

  it('Ctrl+Z waits for the drag to end, as for the computer\'s pen', async () => {
    const undo = vi.spyOn(api, 'liveUndoEdit');
    const { view, frame } = await selected();
    view.fire('pointerdown', { x: frame.cx, y: frame.cy });
    view.fire('pointermove', { x: frame.cx + 60, y: frame.cy + 30 });
    press('z', { ctrlKey: true });
    await tick();
    expect(undo).not.toHaveBeenCalled();
    view.fire('pointercancel', { x: frame.cx + 60, y: frame.cy + 30 });
  });

  it('the pen and a finger move it too', async () => {
    const edit = vi.spyOn(api, 'liveEditStrokes');
    const { view, frame } = await selected();
    view.drag({ x: frame.cx, y: frame.cy }, { x: frame.cx + 50, y: frame.cy }, { pointerType: 'pen', pointerId: 5 });
    await vi.waitFor(() => expect(edit).toHaveBeenCalledOnce());
    await vi.waitFor(() => expect(view.canvas().dataset.strokes).toBe('2'));
    await tick();
    // A finger, a few seconds after the pen was lifted, 40 pixels to the right now.
    view.drag({ x: frame.cx + 50, y: frame.cy }, { x: frame.cx + 50, y: frame.cy + 40 }, { pointerType: 'touch', pointerId: 6, t: 60_000 });
    await vi.waitFor(() => expect(edit).toHaveBeenCalledTimes(2));
    expect(view.target.querySelector('[role="alert"]')).toBeNull();
  });
});

describe('the words of the Select tool', () => {
  it.each([
    ['fr', 'Glissez dans le cadre', 'déplacer', 'redimensionner'],
    ['en', 'Drag inside the frame', 'move', 'resize'],
  ] as const)('say how to move and resize (%s)', async (locale, drag, move, resize) => {
    setLocale(locale);
    const view = render();
    await vi.waitFor(() => expect(view.root().dataset.state).toBe('live'));
    const toggle = view.action('select');
    expect(toggle.title).toContain(move);
    expect(toggle.title).toContain(resize);
    press('v');
    await tick();
    const hint = view.target.querySelector('[data-slot="select-hint"]')!.textContent!;
    expect(hint).toContain(drag);
    expect(hint).toContain(resize);
  });
});
