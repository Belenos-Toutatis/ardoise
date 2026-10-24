// @vitest-environment jsdom
/// <reference types="node" />
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { FakeContext } from '../../lib/live-fake-canvas';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockLiveConfig, mockLiveEmit, mockLiveInvoke, mockLiveTick, resetMockLive } from '../../lib/mock-live';
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
  vi.useRealTimers();
});

function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Presentation, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const main = () => target.querySelector<HTMLElement>('main.presentation')!;
  const canvas = () => target.querySelector<HTMLCanvasElement>('canvas.live-canvas')!;
  return { target, main, canvas };
}

const press = (key: string, init: KeyboardEventInit = {}) =>
  window.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, cancelable: true, ...init }));

/** Starts live mode as the control window would. */
async function startLive() {
  mockLiveInvoke('live_start', {});
  await new Promise((resolve) => setTimeout(resolve, 5));
}

describe('presentation window', () => {
  it('shows only the projected page: no element of interface', async () => {
    const view = render();
    await startLive();
    await vi.waitFor(() => expect(view.main().dataset.state).toBe('live'));
    expect(view.target.querySelectorAll('button, input, select, textarea, a, [tabindex]')).toHaveLength(0);
    expect(view.target.querySelectorAll('canvas')).toHaveLength(1);
    expect(view.canvas().dataset.page).toBe('1/1');
    expect(view.main().getAttribute('aria-label')).toBeTruthy();
    mockLiveTick(120);
    await tick();
    expect(Number(view.canvas().dataset.strokes)).toBeGreaterThan(0);
  });

  it('follows the view, the blackboard and the laser set from the control window', async () => {
    const view = render();
    await startLive();
    await vi.waitFor(() => expect(view.main().dataset.state).toBe('live'));
    // The visible canvas' context first, then the offscreen cache's.
    await vi.waitFor(() => expect(contexts.length).toBeGreaterThanOrEqual(2));
    const [screen, cache] = contexts;
    const scaleOf = () => cache.ops('setTransform').at(-1)!.args[0] as number;
    await vi.waitFor(() => expect(cache.count('setTransform')).toBeGreaterThan(0));
    const before = scaleOf();
    const base = (await api.liveSubscribe(() => {})).view;
    await api.liveSetView({ ...base, zoom: 2, x: 12000, y: 8000 });
    await vi.waitFor(() => expect(scaleOf() / before).toBeCloseTo(2, 6));
    // Blackboard: slate paper.
    await api.liveSetView({ ...base, zoom: 2, x: 12000, y: 8000, board: true });
    await vi.waitFor(() => expect(cache.ops('fillRect').some((c) => c.fillStyle === 'rgb(42, 50, 48)')).toBe(true));
    // Laser pointer: a red dot where the pen hovers.
    await api.liveSetView({ ...base, laser: true });
    screen.reset();
    mockLiveEmit([{ type: 'hover', x: 15000, y: 10000 }]);
    await vi.waitFor(() => expect(screen.ops('fill').some((c) => c.fillStyle === '#ff1f1f')).toBe(true));
  });

  it('turns pages from the keyboard (or a presenter remote)', async () => {
    const view = render();
    await startLive();
    await vi.waitFor(() => expect(view.main().dataset.state).toBe('live'));
    press('PageDown');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('2/2'));
    press('ArrowLeft');
    await vi.waitFor(() => expect(view.canvas().dataset.page).toBe('1/2'));
  });

  it('closes with Esc', async () => {
    const view = render();
    await startLive();
    await api.openPresentation(null);
    await vi.waitFor(() => expect(view.main().dataset.state).toBe('live'));
    const close = vi.spyOn(api, 'closePresentation');
    press('Escape');
    expect(close).toHaveBeenCalledTimes(1);
    await vi.waitFor(async () => expect((await api.liveSubscribe(() => {})).presentation).toBeNull());
  });

  it('hides the mouse cursor after 2 s without moving', async () => {
    vi.useFakeTimers();
    const view = render();
    expect(view.main().classList.contains('cursor-hidden')).toBe(false);
    await vi.advanceTimersByTimeAsync(1900);
    expect(view.main().classList.contains('cursor-hidden')).toBe(false);
    await vi.advanceTimersByTimeAsync(200);
    expect(view.main().classList.contains('cursor-hidden')).toBe(true);
    window.dispatchEvent(new MouseEvent('mousemove'));
    await tick();
    expect(view.main().classList.contains('cursor-hidden')).toBe(false);
    await vi.advanceTimersByTimeAsync(2000);
    expect(view.main().classList.contains('cursor-hidden')).toBe(true);
  });
});
