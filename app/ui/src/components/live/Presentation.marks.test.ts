// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M8: writing over the presentation (pen, finger, mouse), erasing, and pinching the view.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { FakeContext } from '../../lib/live-fake-canvas';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, resetMockLive } from '../../lib/mock-live';
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
