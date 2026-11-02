// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M8: writing over the presentation (pen, finger, mouse), erasing, and pinching the view.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { FakeContext } from '../../lib/live-fake-canvas';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveMarks, resetMockLive } from '../../lib/mock-live';
import Presentation from './Presentation.svelte';

vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

let cleanup: (() => void) | null = null;
const contexts: FakeContext[] = [];

beforeEach(() => {
  resetMockLive();
  resetMockLibrary();
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => {
    const ctx = new FakeContext();
    contexts.push(ctx);
    return ctx as unknown as RenderingContext;
  });
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 0;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  contexts.length = 0;
  resetMockLive();
  vi.restoreAllMocks();
});

interface Init {
  pointerType?: string;
  buttons?: number;
  button?: number;
  pointerId?: number;
  pressure?: number;
  t?: number;
}

async function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Presentation, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  mockLiveInvoke('live_start', {});
  await new Promise((resolve) => setTimeout(resolve, 5));
  const main = target.querySelector<HTMLElement>('main.presentation')!;
  await vi.waitFor(() => expect(main.dataset.state).toBe('live'));
  const canvas = target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
  const fire = (type: string, x: number, y: number, init: Init = {}) => {
    const event = new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      clientX: x,
      clientY: y,
      button: init.button ?? 0,
      buttons: init.buttons ?? 1,
    });
    const props: Record<string, unknown> = {
      pointerType: init.pointerType ?? 'mouse',
      pointerId: init.pointerId ?? 1,
      pressure: init.pressure ?? 0.5,
      timeStamp: init.t ?? 0,
    };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    canvas.dispatchEvent(event);
  };
  /** A line of marks across the middle of the screen. */
  const write = async (init: Init = {}, y = 300) => {
    fire('pointerdown', 400, y, init);
    for (let x = 440; x <= 800; x += 40) fire('pointermove', x, y, init);
    fire('pointerup', 800, y, { ...init, buttons: 0 });
    await tick();
  };
  const clearButton = () => target.querySelector<HTMLButtonElement>('button[data-action="clear-marks"]');
  /** The mark colour (red on a light page) painted on the screen canvas by the next frame. */
  const redFills = () =>
    contexts
      .filter((c) => c.ops('fill').length > 0)
      .flatMap((c) => c.ops('fill'))
      .filter((c) => c.fillStyle === 'rgb(216, 0, 0)').length;
  return { target, main, canvas, fire, write, clearButton, redFills };
}

describe('writing over the presentation', () => {
  it('writes with the mouse, the pen or a finger, and shows a button to clear', async () => {
    const view = await render();
    expect(view.clearButton()).toBeNull();
    await view.write({ pointerType: 'mouse' });
    await vi.waitFor(() => expect(view.clearButton()).toBeTruthy());
    await vi.waitFor(() => expect(view.redFills()).toBeGreaterThan(0));
    await view.write({ pointerType: 'pen', pointerId: 2, t: 10_000 }, 350);
    await view.write({ pointerType: 'touch', pointerId: 3, t: 20_000 }, 400);
    // Nothing is written into the note: the live state has no new stroke.
    expect(Number(view.canvas.dataset.strokes)).toBe(0);
  });

  it('erases with the back of the pen, and clears with the button and with Delete', async () => {
    const view = await render();
    await view.write({ pointerType: 'mouse' });
    await vi.waitFor(() => expect(view.clearButton()).toBeTruthy());
    // The eraser end passes over the mark.
    view.fire('pointerdown', 600, 300, { pointerType: 'pen', buttons: 32, pointerId: 2, t: 1000 });
    view.fire('pointerup', 600, 300, { pointerType: 'pen', buttons: 0, pointerId: 2, t: 1010 });
    await vi.waitFor(() => expect(view.clearButton()).toBeNull());

    await view.write({ pointerType: 'mouse' });
    await vi.waitFor(() => expect(view.clearButton()).toBeTruthy());
    view.clearButton()!.click();
    await vi.waitFor(() => expect(view.clearButton()).toBeNull());

    await view.write({ pointerType: 'mouse' });
    await vi.waitFor(() => expect(view.clearButton()).toBeTruthy());
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(view.clearButton()).toBeNull());
  });

  it('keeps the marks the eraser did not touch', async () => {
    const view = await render();
    await view.write({ pointerType: 'mouse' }, 200);
    await view.write({ pointerType: 'mouse', t: 5000 }, 500);
    view.fire('pointerdown', 600, 200, { pointerType: 'pen', buttons: 32, pointerId: 2, t: 9000 });
    view.fire('pointerup', 600, 200, { pointerType: 'pen', buttons: 0, pointerId: 2, t: 9010 });
    await tick();
    // One mark is left: the button is still there.
    expect(view.clearButton()).toBeTruthy();
    view.fire('pointerdown', 600, 500, { pointerType: 'pen', buttons: 32, pointerId: 2, t: 9100 });
    view.fire('pointerup', 600, 500, { pointerType: 'pen', buttons: 0, pointerId: 2, t: 9110 });
    await vi.waitFor(() => expect(view.clearButton()).toBeNull());
  });

  it('ignores a finger while the pen hovers (palm rejection)', async () => {
    const view = await render();
    view.fire('pointermove', 100, 100, { pointerType: 'pen', buttons: 0, pointerId: 2, t: 100 });
    await view.write({ pointerType: 'touch', pointerId: 3, t: 200 });
    expect(view.clearButton()).toBeNull();
    // Later, with the pen gone, the finger writes.
    await view.write({ pointerType: 'touch', pointerId: 4, t: 5000 });
    await vi.waitFor(() => expect(view.clearButton()).toBeTruthy());
  });

  it('goes away with the page', async () => {
    const view = await render();
    await view.write({ pointerType: 'mouse' });
    await vi.waitFor(() => expect(view.clearButton()).toBeTruthy());
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'PageDown', bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(view.canvas.dataset.page).toBe('2/2'));
    await vi.waitFor(() => expect(view.clearButton()).toBeNull());
  });

  it('goes away with a quick switch of the orientation, which turns the page without changing its index', async () => {
    const view = await render();
    await view.write({ pointerType: 'mouse' });
    await vi.waitFor(() => expect(view.clearButton()).toBeTruthy());
    // A quarter turn swaps the page's size; the index stays 1/1.
    await api.tabletSetOrientation('portraitLeft');
    await vi.waitFor(() => expect(view.clearButton()).toBeNull());
    expect(view.canvas.dataset.page).toBe('1/1');

    // A half turn keeps the size but flips the page: the marks go too.
    await view.write({ pointerType: 'mouse', t: 5000 });
    await vi.waitFor(() => expect(view.clearButton()).toBeTruthy());
    await api.tabletSetOrientation('portraitRight');
    await vi.waitFor(() => expect(view.clearButton()).toBeNull());
  });

  it('drops the mark being written when a second finger comes down, and pinches the view instead', async () => {
    const view = await render();
    const base = (await api.liveSubscribe(() => {})).view;
    view.fire('pointerdown', 500, 300, { pointerType: 'touch', pointerId: 1, t: 0 });
    view.fire('pointermove', 540, 300, { pointerType: 'touch', pointerId: 1, t: 10 });
    view.fire('pointerdown', 600, 300, { pointerType: 'touch', pointerId: 2, t: 20 });
    view.fire('pointermove', 700, 300, { pointerType: 'touch', pointerId: 2, t: 30 });
    view.fire('pointerup', 700, 300, { pointerType: 'touch', pointerId: 2, buttons: 0, t: 40 });
    view.fire('pointerup', 540, 300, { pointerType: 'touch', pointerId: 1, buttons: 0, t: 40 });
    await tick();
    expect(view.clearButton()).toBeNull();
    await vi.waitFor(async () => expect((await api.liveSubscribe(() => {})).view.zoom).toBeGreaterThan(base.zoom + 0.5));
  });
});

describe('marks reported for the viewers of the share', () => {
  /** The last list the window sent. */
  const lastSent = () => mockLiveMarks[mockLiveMarks.length - 1];

  it('sends the marks while one is written, not on every move, and the whole mark once the pen is up', async () => {
    const view = await render();
    expect(mockLiveMarks).toHaveLength(0);
    await view.write({ pointerType: 'mouse' });
    await vi.waitFor(() => expect(lastSent()?.marks[0]?.points.length).toBeGreaterThan(5));
    // The first point went at once; the 10 moves were merged.
    expect(mockLiveMarks.length).toBeGreaterThanOrEqual(2);
    expect(mockLiveMarks.length).toBeLessThanOrEqual(4);
    expect(mockLiveMarks[0].marks[0].points).toHaveLength(1);
    expect(lastSent()).toMatchObject({ pageIndex: 0, marks: [{ tool: 'feltTip', color: { r: 216, g: 0, b: 0, a: 255 } }] });
    // Only the ink and the points leave the window.
    expect(Object.keys(lastSent().marks[0]).sort()).toEqual(['color', 'points', 'tool', 'width']);
    expect(Object.keys(lastSent().marks[0].points[0]).sort()).toEqual(['p', 'x', 'y']);

    // A second mark: both are in the list.
    await view.write({ pointerType: 'pen', pointerId: 2, t: 10_000 }, 400);
    await vi.waitFor(() => expect(lastSent().marks).toHaveLength(2));
    // The window still draws its own marks; the viewers' copy is no business of its canvas.
    expect(view.clearButton()).toBeTruthy();
  });

  it('sends the list again, shorter, when the back of the pen erases a mark', async () => {
    const view = await render();
    await view.write({ pointerType: 'mouse' }, 200);
    await view.write({ pointerType: 'mouse', t: 5000 }, 500);
    await vi.waitFor(() => expect(lastSent()?.marks).toHaveLength(2));
    view.fire('pointerdown', 600, 200, { pointerType: 'pen', buttons: 32, pointerId: 2, t: 9000 });
    view.fire('pointerup', 600, 200, { pointerType: 'pen', buttons: 0, pointerId: 2, t: 9010 });
    await vi.waitFor(() => expect(lastSent().marks).toHaveLength(1));
  });

  it('sends an empty list when the marks are cleared, by the button or by Delete', async () => {
    const view = await render();
    await view.write({ pointerType: 'mouse' });
    await vi.waitFor(() => expect(lastSent()?.marks).toHaveLength(1));
    view.clearButton()!.click();
    await vi.waitFor(() => expect(lastSent().marks).toEqual([]));

    await view.write({ pointerType: 'mouse', t: 5000 });
    await vi.waitFor(() => expect(lastSent().marks).toHaveLength(1));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(lastSent().marks).toEqual([]));
    const sent = mockLiveMarks.length;
    // Clearing nothing says nothing.
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Delete', bubbles: true, cancelable: true }));
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(mockLiveMarks).toHaveLength(sent);
  });

  it('sends an empty list for the new page when the page changes, and drops a mark cut short by a second finger', async () => {
    const view = await render();
    await view.write({ pointerType: 'mouse' });
    await vi.waitFor(() => expect(lastSent()?.marks).toHaveLength(1));
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'PageDown', bubbles: true, cancelable: true }));
    await vi.waitFor(() => expect(lastSent()).toEqual({ pageIndex: 1, marks: [] }));

    const sent = mockLiveMarks.length;
    view.fire('pointerdown', 500, 300, { pointerType: 'touch', pointerId: 1, t: 0 });
    await vi.waitFor(() => expect(mockLiveMarks.length).toBeGreaterThan(sent));
    view.fire('pointerdown', 600, 300, { pointerType: 'touch', pointerId: 2, t: 20 });
    await vi.waitFor(() => expect(lastSent().marks).toEqual([]));
  });

  it('takes its marks off the viewers when the window closes', async () => {
    const view = await render();
    await view.write({ pointerType: 'mouse' });
    await vi.waitFor(() => expect(lastSent()?.marks).toHaveLength(1));
    cleanup?.();
    cleanup = null;
    expect(lastSent().marks).toEqual([]);
    expect(view.canvas.isConnected).toBe(false);
  });
});
