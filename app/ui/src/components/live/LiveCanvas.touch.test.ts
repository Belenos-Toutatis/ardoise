// @vitest-environment jsdom
/// <reference types="node" />
// Milestone M8: the control window's preview with fingers and a pen (pinch, double tap, palm rejection).
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { LiveViewState } from '../../lib/live';
import { instantiateInk, type Ink } from '../../lib/ink';
import { FakeContext } from '../../lib/live-fake-canvas';
import { LiveState, subscribeLive } from '../../lib/live-state.svelte';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveInvoke, mockLiveSubscribe, resetMockLive } from '../../lib/mock-live';
import LiveCanvas from './LiveCanvas.svelte';

let cleanup: (() => void) | null = null;
let ink: Ink;

beforeEach(async () => {
  ink = await instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  resetMockLive();
  resetMockLibrary();
  mockLiveConfig.pen = false;
  mockLiveConfig.waitMs = 0;
  vi.spyOn(HTMLCanvasElement.prototype, 'getContext').mockImplementation(() => new FakeContext() as unknown as RenderingContext);
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  resetMockLive();
  vi.restoreAllMocks();
});

interface Init {
  pointerType?: string;
  buttons?: number;
  pointerId?: number;
  t?: number;
}

async function render() {
  const live = new LiveState();
  await subscribeLive(live, (onBatch) => mockLiveSubscribe(onBatch));
  const target = document.createElement('div');
  document.body.append(target);
  const onview = vi.fn<(view: LiveViewState) => void>();
  const component = mount(LiveCanvas, { target, props: { live, ink, mode: 'preview', label: 'page', onview } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  mockLiveInvoke('live_start', {});
  await new Promise((resolve) => setTimeout(resolve, 5));
  await tick();
  const canvas = target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
  const fire = (type: string, x: number, y: number, init: Init = {}) => {
    const event = new MouseEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, button: 0, buttons: init.buttons ?? 1 });
    const props: Record<string, unknown> = {
      pointerType: init.pointerType ?? 'mouse',
      pointerId: init.pointerId ?? 1,
      pressure: 0.5,
      timeStamp: init.t ?? 0,
    };
    for (const [key, value] of Object.entries(props)) Object.defineProperty(event, key, { value, configurable: true });
    canvas.dispatchEvent(event);
  };
  return { live, canvas, fire, onview };
}

const finger = (id: number, t: number): Init => ({ pointerType: 'touch', pointerId: id, t });

describe('live preview: fingers and pen', () => {
  it('pinches to zoom the projected area', async () => {
    const view = await render();
    view.fire('pointerdown', 380, 280, finger(1, 0));
    view.onview.mockClear();
    view.fire('pointerdown', 440, 280, finger(2, 10));
    view.fire('pointermove', 540, 280, finger(2, 20));
    expect(view.onview).toHaveBeenCalled();
    const next = view.onview.mock.calls.at(-1)![0];
    expect(next.zoom).toBeGreaterThan(1.5);
  });

  it('moves the projected area with one finger, as with the mouse', async () => {
    const view = await render();
    view.live.changeView({ ...view.live.view, zoom: 3 }, async () => {});
    view.onview.mockClear();
    view.fire('pointerdown', 400, 280, finger(1, 0));
    view.fire('pointermove', 430, 300, finger(1, 20));
    expect(view.onview).toHaveBeenCalled();
  });

  it('shows the whole page with a double tap', async () => {
    const view = await render();
    view.live.changeView({ ...view.live.view, zoom: 4 }, async () => {});
    view.onview.mockClear();
    for (const [id, t] of [
      [1, 0],
      [2, 150],
    ]) {
      view.fire('pointerdown', 400, 280, finger(id, t));
      view.fire('pointerup', 400, 280, { ...finger(id, t + 50), buttons: 0 });
    }
    const last = view.onview.mock.calls.at(-1)![0];
    expect(last.zoom).toBe(1);
  });

  it('ignores fingers while the pen hovers', async () => {
    const view = await render();
    view.fire('pointermove', 100, 100, { pointerType: 'pen', buttons: 0, pointerId: 9, t: 0 });
    view.onview.mockClear();
    view.fire('pointerdown', 400, 280, finger(1, 100));
    view.fire('pointermove', 450, 300, finger(1, 120));
    view.fire('pointerup', 450, 300, { ...finger(1, 140), buttons: 0 });
    expect(view.onview).not.toHaveBeenCalled();
  });

  it('keeps the mouse and the pen tip dragging the area', async () => {
    const view = await render();
    view.onview.mockClear();
    view.fire('pointerdown', 400, 280, { pointerType: 'pen', pointerId: 9, t: 0 });
    view.fire('pointermove', 420, 290, { pointerType: 'pen', pointerId: 9, t: 10 });
    view.fire('pointerup', 420, 290, { pointerType: 'pen', pointerId: 9, buttons: 0, t: 20 });
    expect(view.onview).toHaveBeenCalled();
  });
});
