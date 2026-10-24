// @vitest-environment jsdom
/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { instantiateInk, type Ink } from '../../lib/ink';
import { LatencyRecorder } from '../../lib/latency';
import { FakeContext } from '../../lib/live-fake-canvas';
import { LiveState, subscribeLive } from '../../lib/live-state.svelte';
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
