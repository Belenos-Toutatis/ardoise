/**
 * Latency of live mode (SPEC §4.2: a point is on screen less than 30 ms after its receipt).
 * Contract: docs/design/tablet-live-classroom.md, "Latency".
 *
 * - End to end: wall-clock time when the frame that painted a point was drawn (taken in the
 *   `requestAnimationFrame` callback, after the canvas calls) minus `LivePoint.recv`, the
 *   wall-clock time when Rust received the point. Both clocks are the same PC's.
 * - Interface part: the same end minus the arrival of the batch in this window.
 *
 * The recorder keeps the last 2 000 samples of each; the control window shows the median and
 * exposes `window.__ardoiseLatency` for the end-to-end journey.
 */

/** Samples kept (the most recent ones). */
export const LATENCY_CAPACITY = 2000;

/** Wall-clock milliseconds since the Unix epoch, fractional (comparable with `LivePoint.recv`). */
export function wallClock(): number {
  return performance.timeOrigin + performance.now();
}

/**
 * The `q` quantile (0..1) of `values`, linearly interpolated between the closest ranks; null
 * when there is nothing to measure.
 */
export function percentile(values: readonly number[], q: number): number | null {
  if (values.length === 0) return null;
  const sorted = [...values].sort((a, b) => a - b);
  const rank = Math.min(1, Math.max(0, q)) * (sorted.length - 1);
  const low = Math.floor(rank);
  const high = Math.ceil(rank);
  return sorted[low] + (sorted[high] - sorted[low]) * (rank - low);
}

export function median(values: readonly number[]): number | null {
  return percentile(values, 0.5);
}

/** A fixed-size ring of the most recent samples. */
class Ring {
  private readonly values: Float64Array;
  private next = 0;
  private count = 0;

  constructor(capacity: number) {
    this.values = new Float64Array(capacity);
  }

  push(value: number): void {
    this.values[this.next] = value;
    this.next = (this.next + 1) % this.values.length;
    this.count = Math.min(this.count + 1, this.values.length);
  }

  /** Oldest first. */
  toArray(): number[] {
    const out: number[] = new Array(this.count);
    const start = (this.next - this.count + this.values.length) % this.values.length;
    for (let i = 0; i < this.count; i++) out[i] = this.values[(start + i) % this.values.length];
    return out;
  }

  get size(): number {
    return this.count;
  }

  clear(): void {
    this.next = 0;
    this.count = 0;
  }
}

/** A point drawn on screen: when Rust received it and when its batch arrived here. */
export interface PendingSample {
  /** Wall-clock ms when Rust received the point (`LivePoint.recv`). */
  recv: number;
  /** Wall-clock ms when its batch arrived in this window. */
  arrival: number;
}

export class LatencyRecorder {
  private readonly total: Ring;
  private readonly ui: Ring;

  constructor(capacity = LATENCY_CAPACITY) {
    this.total = new Ring(capacity);
    this.ui = new Ring(capacity);
  }

  /** Records points painted by the frame drawn at `end` (wall clock). */
  painted(points: readonly PendingSample[], end: number): void {
    for (const p of points) this.record(end - p.recv, end - p.arrival);
  }

  record(endToEnd: number, ui: number): void {
    // A point without a receipt stamp (or a broken clock) says nothing about the latency.
    if (Number.isFinite(endToEnd)) this.total.push(endToEnd);
    if (Number.isFinite(ui)) this.ui.push(ui);
  }

  /** End-to-end samples (ms), oldest first. */
  samples(): number[] {
    return this.total.toArray();
  }

  /** Interface-part samples (ms), oldest first. */
  uiSamples(): number[] {
    return this.ui.toArray();
  }

  get size(): number {
    return this.total.size;
  }

  median(): number | null {
    return median(this.samples());
  }

  p95(): number | null {
    return percentile(this.samples(), 0.95);
  }

  uiMedian(): number | null {
    return median(this.uiSamples());
  }

  uiP95(): number | null {
    return percentile(this.uiSamples(), 0.95);
  }

  clear(): void {
    this.total.clear();
    this.ui.clear();
  }
}

/** What the end-to-end journey reads (`window.__ardoiseLatency`). */
export interface LatencyProbe {
  samples(): number[];
  median(): number | null;
  p95(): number | null;
  uiSamples(): number[];
  uiMedian(): number | null;
}

declare global {
  interface Window {
    __ardoiseLatency?: LatencyProbe;
  }
}

/** Exposes `recorder` as `window.__ardoiseLatency`. */
export function exposeLatency(recorder: LatencyRecorder, target: Window | undefined = globalThis.window): void {
  if (!target) return;
  target.__ardoiseLatency = {
    samples: () => recorder.samples(),
    median: () => recorder.median(),
    p95: () => recorder.p95(),
    uiSamples: () => recorder.uiSamples(),
    uiMedian: () => recorder.uiMedian(),
  };
}
