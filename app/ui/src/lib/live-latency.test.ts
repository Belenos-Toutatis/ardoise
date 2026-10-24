/// <reference types="node" />
/**
 * Interface part of the live latency (docs/design/tablet-live-classroom.md, "Latency"): 300
 * realistic batches (the browser stand-in's pen: 3 points per batch, strokes, hovering, a page
 * already holding 150 strokes, the projected area moved now and then) go through the same code
 * as the windows (`LiveState.receive`, `buildScene`, `LiveRenderer.paint` with the real
 * WebAssembly outlines) onto a fake 2D context, one frame right after each batch. The time from
 * a batch's arrival to the end of its drawing is printed (count, median, p95).
 *
 * What this does not measure: the wait for the next display frame (up to 16.7 ms at 60 Hz), the
 * browser's own rasterisation, and the IPC. The end-to-end figure comes from the real window on
 * Windows (`window.__ardoiseLatency`, e2e journey).
 */
import { readFileSync } from 'node:fs';
import { afterEach, describe, expect, it } from 'vitest';
import { instantiateInk } from './ink';
import { LatencyRecorder, wallClock } from './latency';
import type { LiveSnapshot } from './live';
import { FakeContext, FakeSurface } from './live-fake-canvas';
import { LiveRenderer, buildScene, type SceneOptions } from './live-render';
import { LiveState, subscribeLive } from './live-state.svelte';
import { mockLiveConfig, mockLiveEmit, mockLiveInvoke, mockLiveSubscribe, mockLiveTick, resetMockLive } from './mock-live';
import { resetMockLibrary } from './mock-data';
import type { Stroke } from './note';

afterEach(() => {
  resetMockLive();
});

/** A handwritten-looking stroke of `n` samples. */
function loops(x0: number, y0: number, n: number): Stroke {
  return {
    tool: 'ballpoint',
    color: { r: 0, g: 0, b: 0, a: 255 },
    width: 60,
    layer: 0,
    points: Array.from({ length: n }, (_, i) => {
      const u = i / (n - 1);
      const a = 2 * Math.PI * 3 * u;
      return { x: x0 + 2500 * u - 300 * Math.sin(a), y: y0 - 270 * (1 - Math.cos(a)), pressure: 0.3 + 0.5 * Math.sin(Math.PI * u), t: 5 * i };
    }),
  };
}

describe('live latency, interface part', () => {
  it('draws each batch well within the 30 ms budget', async () => {
    const ink = await instantiateInk(readFileSync(new URL('../assets/ardoise_ink.wasm', import.meta.url)));
    resetMockLive();
    resetMockLibrary();
    mockLiveConfig.pen = false;
    mockLiveConfig.waitMs = 0;

    const live = new LiveState();
    const renderer = new LiveRenderer(() => new FakeSurface(), ink);
    const screen = new FakeContext();
    const recorder = new LatencyRecorder();
    // The projector of a classroom: 1080p at 125 %.
    const options: SceneOptions = {
      mode: 'presentation',
      viewport: { width: 1536, height: 864 },
      pixelRatio: 1.25,
      aspect: 16 / 9,
      backgroundImage: null,
    };
    let frames = 0;
    const frame = () => {
      renderer.paint(screen, buildScene(live, options));
      recorder.painted(live.takeUnpainted(), wallClock());
      screen.reset();
      frames += 1;
    };
    await subscribeLive(live, (onBatch) =>
      mockLiveSubscribe((batch) => {
        onBatch(batch);
        frame();
      }),
    );
    const started = mockLiveInvoke('live_start', {}) as LiveSnapshot;
    await new Promise((resolve) => setTimeout(resolve, 5));
    expect(live.state).toBe('live');

    // The page already holds 150 strokes (a well-filled board).
    const crowded: LiveSnapshot = {
      ...started,
      state: 'live',
      page: { ...started.page, strokes: Array.from({ length: 150 }, (_, i) => loops(1500 + (i % 9) * 3000, 2500 + Math.floor(i / 9) * 1100, 100)) },
    };
    mockLiveEmit([{ type: 'snapshot', snapshot: crowded }]);
    expect(live.strokes).toHaveLength(150);
    // Warm-up (JIT, first outlines), not measured.
    mockLiveTick(100);
    recorder.clear();

    const view = { ...live.view };
    for (let i = 0; i < 300; i++) {
      if (i % 75 === 74) {
        // The teacher zooms on a detail now and then: the cache is redrawn.
        mockLiveEmit([{ type: 'view', view: { ...view, zoom: 1 + (i % 2), x: 9000 + i * 10, y: 7000 } }]);
      }
      mockLiveTick(1);
    }

    const ui = recorder.uiSamples();
    const median = recorder.uiMedian() ?? Number.NaN;
    const p95 = recorder.uiP95() ?? Number.NaN;
    console.log(
      `live latency, interface part: ${ui.length} points in ${frames} frames, median ${median.toFixed(3)} ms, ` +
        `p95 ${p95.toFixed(3)} ms; with the simulated Rust part: median ${(recorder.median() ?? Number.NaN).toFixed(3)} ms`,
    );
    expect(ui.length).toBeGreaterThan(300);
    expect(median).toBeLessThan(30);
    // Every point was painted by the frame after its batch.
    expect(live.takeUnpainted()).toEqual([]);
    expect(live.strokes.length).toBeGreaterThan(150);
  });
});
