import { describe, expect, it } from 'vitest';
import { LATENCY_CAPACITY, LatencyRecorder, exposeLatency, median, percentile, wallClock } from './latency';

describe('latency statistics', () => {
  it('interpolates percentiles between the closest ranks', () => {
    expect(percentile([], 0.5)).toBeNull();
    expect(median([7])).toBe(7);
    expect(median([3, 1, 2])).toBe(2);
    expect(median([4, 1, 3, 2])).toBe(2.5);
    const hundred = Array.from({ length: 101 }, (_, i) => 100 - i);
    expect(percentile(hundred, 0.95)).toBeCloseTo(95, 9);
    expect(percentile(hundred, 0)).toBe(0);
    expect(percentile(hundred, 1)).toBe(100);
    expect(percentile(hundred, 2)).toBe(100);
  });

  it('keeps the last 2 000 samples, oldest first', () => {
    const recorder = new LatencyRecorder();
    for (let i = 0; i < LATENCY_CAPACITY + 500; i++) recorder.record(i, i / 10);
    const samples = recorder.samples();
    expect(samples).toHaveLength(LATENCY_CAPACITY);
    expect(samples[0]).toBe(500);
    expect(samples[samples.length - 1]).toBe(LATENCY_CAPACITY + 499);
    expect(recorder.size).toBe(LATENCY_CAPACITY);
    expect(recorder.median()).toBeCloseTo(500 + (LATENCY_CAPACITY - 1) / 2, 9);
    expect(recorder.uiSamples()[0]).toBeCloseTo(50, 9);
    recorder.clear();
    expect(recorder.samples()).toEqual([]);
    expect(recorder.median()).toBeNull();
  });

  it('measures the interface part at the end of the drawing and the end to end at the next frame', () => {
    const recorder = new LatencyRecorder();
    const points = [
      { recv: 1000, arrival: 1002 },
      { recv: 1001, arrival: 1002 },
      { recv: Number.NaN, arrival: 1003 },
    ];
    // The frame ends its drawing at 1010: only the interface part (arrival to end) is known.
    recorder.painted(points, 1010);
    expect(recorder.uiSamples()).toEqual([8, 8, 7]);
    expect(recorder.samples()).toEqual([]);
    expect(recorder.median()).toBeNull();
    // The next frame begins at 1027 (16.7 ms later): the points count as displayed then.
    recorder.presented(points, 1027);
    expect(recorder.samples()).toEqual([27, 26]);
    expect(recorder.size).toBe(2);
    expect(recorder.p95()).toBeCloseTo(26.95, 9);
    expect(recorder.uiMedian()).toBe(8);
    expect(recorder.uiP95()).toBeCloseTo(8, 9);
  });

  it('reads the wall clock like Rust stamps it (ms since the Unix epoch)', () => {
    const before = Date.now();
    const now = wallClock();
    expect(Math.abs(now - before)).toBeLessThan(1000);
  });

  it('exposes the recorder to the end-to-end journey', () => {
    const recorder = new LatencyRecorder();
    const target = {} as Window;
    exposeLatency(recorder, target);
    expect(target.__ardoiseLatency?.median()).toBeNull();
    recorder.record(12, 3);
    recorder.record(14, 5);
    expect(target.__ardoiseLatency?.samples()).toEqual([12, 14]);
    expect(target.__ardoiseLatency?.median()).toBe(13);
    expect(target.__ardoiseLatency?.uiMedian()).toBe(4);
    expect(target.__ardoiseLatency?.p95()).toBeCloseTo(13.9, 9);
    expect(target.__ardoiseLatency?.uiSamples()).toEqual([3, 5]);
  });
});
