// Cases and expected numbers of `crates/ardoise-media/src/replay/tests.rs` (the Rust timeline).
import { describe, expect, it } from 'vitest';
import { bigNote, note as makeNote } from './editor/test-fixtures';
import type { Tool } from './ink';
import type { Note, Stroke } from './note';
import { ReplayTimeline, forDuration, type ReplayOptions } from './replay';
import { MAX_SPEED, MIN_SPEED } from './video';

const BLUE = { r: 30, g: 50, b: 140, a: 255 };

/** 20 samples from `from` to `to`, evenly spread from `t0` to `t1` ms (integer times, as in Rust). */
function line(tool: Tool, layer: number, from: [number, number], to: [number, number], t0: number, t1: number): Stroke {
  const n = 20;
  const points = Array.from({ length: n }, (_, i) => {
    const u = i / (n - 1);
    return {
      x: from[0] + (to[0] - from[0]) * u,
      y: from[1] + (to[1] - from[1]) * u,
      pressure: 0.4 + 0.5 * u,
      t: t0 + Math.floor(((t1 - t0) * i) / (n - 1)),
    };
  });
  return { tool, color: BLUE, width: 60, layer, points };
}

/** A stroke made of samples at these times, all at one place. */
function timed(times: number[]): Stroke {
  return {
    tool: 'ballpoint',
    color: BLUE,
    width: 60,
    layer: 0,
    points: times.map((t, i) => ({ x: 1000 + 20 * i, y: 1000, pressure: 0.5, t })),
  };
}

function note(strokes: Stroke[], visible: boolean[] = [true]): Note {
  return { ...makeNote(strokes), layers: visible.map((v, i) => ({ name: `${i + 1}`, visible: v })) };
}

const options = (speed: number, maxPauseMs: number | null): ReplayOptions => ({ speed, maxPauseMs });

/** Strokes over [0, 1000], [3000, 4000] and [4200, 5000] ms. */
function threeStrokes(): Note {
  return note([
    line('ballpoint', 0, [500, 500], [5500, 600], 0, 1000),
    line('ballpoint', 0, [500, 1500], [5500, 1600], 3000, 4000),
    line('ballpoint', 0, [500, 2500], [5500, 2600], 4200, 5000),
  ]);
}

const timeline = (n: Note, speed = 1, maxPauseMs: number | null = null) =>
  new ReplayTimeline(n, options(speed, maxPauseMs));

describe('replay timeline: speed', () => {
  it('divides the duration by the speed', () => {
    const n = threeStrokes();
    const normal = timeline(n);
    expect(normal.duration).toBe(5000);
    expect(normal.noteEnd).toBe(5000);
    // At speed 1 with every pause kept, replay time is note time.
    for (const t of [0, 1, 999, 2000, 3500, 5000]) {
      expect(normal.toReplay(t)).toBe(t);
      expect(normal.toNote(t)).toBeCloseTo(t, 6);
    }
    const fast = timeline(n, 2);
    expect(fast.duration).toBe(2500);
    expect(fast.toReplay(3000)).toBe(1500);
    expect(fast.toNote(1500)).toBeCloseTo(3000, 6);
    expect(timeline(n, MIN_SPEED).duration).toBe(20_000);
    expect(timeline(n, MAX_SPEED).duration).toBeCloseTo(312.5, 9);
    // Past the end, or before the start: clamped.
    expect(fast.toReplay(1e12)).toBe(fast.duration);
    expect(fast.toReplay(-5)).toBe(0);
    expect(fast.toReplay(NaN)).toBe(0);
    expect(fast.toNote(99_999)).toBe(5000);
  });

  it('refuses speeds outside 0.25 to 16', () => {
    const n = threeStrokes();
    for (const speed of [MIN_SPEED, 1, 3.7, MAX_SPEED]) {
      expect(() => timeline(n, speed), `${speed}`).not.toThrow();
    }
    for (const speed of [0, 0.2499, 16.01, -1, NaN, Infinity]) {
      expect(() => timeline(n, speed), `${speed}`).toThrow(RangeError);
    }
  });

  it('picks the speed that fits a target duration, within the limits', () => {
    const n = threeStrokes();
    const speedFor = (maxPause: number | null, target: number) => forDuration(n, maxPause, target).speed;
    expect(speedFor(null, 2500)).toBe(2);
    expect(speedFor(500, 7000)).toBe(0.5);
    expect(speedFor(null, 10)).toBe(MAX_SPEED);
    expect(speedFor(null, 0)).toBe(MAX_SPEED);
    expect(speedFor(null, 3_600_000)).toBe(MIN_SPEED);
    const picked = forDuration(n, 500, 1750);
    expect(picked).toEqual(options(2, 500));
    expect(new ReplayTimeline(n, picked).duration).toBe(1750);
    expect(forDuration(note([]), null, 10).speed).toBe(1);
  });
});

describe('replay timeline: trimmed pauses', () => {
  it('cuts long pauses after the maximum pause', () => {
    const n = threeStrokes();
    // 1000 + 500 (of the 2000 ms pause) + 1000 + 200 (short pause, kept) + 800.
    const trimmed = timeline(n, 1, 500);
    expect(trimmed.duration).toBe(3500);
    expect(trimmed.toReplay(1200)).toBe(1200);
    // The rest of the pause is cut: it all maps to the instant of the cut...
    expect(trimmed.toReplay(1500)).toBe(1500);
    expect(trimmed.toReplay(2000)).toBe(1500);
    expect(trimmed.toReplay(3000)).toBe(1500);
    // ...where the next stroke starts.
    expect(trimmed.toNote(1500)).toBeCloseTo(3000, 6);
    expect(trimmed.toNote(1499)).toBeCloseTo(1499, 6);
    expect(trimmed.toReplay(4100)).toBe(2600);
    expect(trimmed.toReplay(4300)).toBe(2800);
    // The speed applies after the trimming.
    const fast = timeline(n, 2, 500);
    expect(fast.duration).toBe(1750);
    expect(fast.toReplay(4300)).toBe(1400);
    // No pause at all; pauses all shorter than the limit.
    const none = timeline(n, 1, 0);
    expect(none.duration).toBe(2800);
    expect(none.toNote(1000)).toBeCloseTo(3000, 6);
    expect(timeline(n, 1, 2000).duration).toBe(5000);
  });

  it('counts the time before the first stroke as a pause', () => {
    const late = note([line('ballpoint', 0, [0, 0], [900, 0], 10_000, 11_000)]);
    const kept = timeline(late);
    expect(kept.duration).toBe(11_000);
    expect(kept.noteAt(9_999).strokes).toHaveLength(0);
    const cut = timeline(late, 1, 500);
    expect(cut.duration).toBe(1_500);
    expect(cut.noteAt(499).strokes).toHaveLength(0);
    expect(cut.noteAt(500).strokes[0].points).toHaveLength(1);
  });

  it('does not count the pen standing still inside a stroke as a pause', () => {
    const n = note([timed([0, 100, 5100, 5200])]);
    const t = timeline(n, 1, 500);
    expect(t.duration).toBe(5200);
    expect(t.noteAt(5099).strokes[0].points).toHaveLength(2);
  });

  it('pauses only when no stroke is being drawn, whatever the order of the strokes', () => {
    // Unsorted in the note, on two layers, B written "during" A (merged or edited notes).
    const a = line('ballpoint', 0, [500, 500], [5500, 500], 0, 3000);
    const b = line('highlighter', 1, [500, 900], [5500, 900], 1000, 2000);
    const c = line('feltTip', 0, [500, 1500], [5500, 1500], 5000, 6000);
    const n = note([c, b, a], [true, true]);
    const t = timeline(n, 1, 1000);
    // Busy over [0, 3000] and [5000, 6000]; the 2000 ms pause is cut to 1000.
    expect(t.duration).toBe(5000);
    const at = (r: number) => t.noteAt(r).strokes.map((s) => s.points.length);
    expect(at(0)).toEqual([1]);
    expect(at(1500)).toEqual([10, 10]);
    expect(at(3999)).toEqual([20, 20]);
    expect(at(4000)).toEqual([1, 20, 20]);
    // The strokes keep their note order and layers.
    const full = t.noteAt(t.duration);
    expect(full.strokes).toEqual(n.strokes);
    expect(full.layers).toEqual(n.layers);
  });
});

describe('replay timeline: mapping', () => {
  it('round-trips, monotonic, for several speeds and trimmings', () => {
    const n = threeStrokes();
    const cases: [number, number | null][] = [
      [1, null],
      [1.5, 500],
      [0.25, 0],
      [16, 150],
    ];
    for (const [speed, maxPause] of cases) {
      const t = timeline(n, speed, maxPause);
      const what = `speed ${speed}, max pause ${maxPause}`;
      // Note time inside a cut maps to the instant of the cut, shown as the time after it.
      const cut = (tau: number): number | null => {
        if (maxPause === null) return null;
        if (tau >= 1000 + maxPause && tau < 3000) return 3000;
        if (tau >= 4000 + maxPause && tau < 4200) return 4200;
        return null;
      };
      let previous = 0;
      for (let tau = 0; tau <= 5000; tau += 7) {
        const r = t.toReplay(tau);
        expect(r, `${what}: not monotonic at ${tau}`).toBeGreaterThanOrEqual(previous);
        previous = r;
        expect(t.toNote(r), `${what} at ${tau}`).toBeCloseTo(cut(tau) ?? tau, 3);
      }
      let previousNote = 0;
      const step = Math.max(t.duration / 997, 1e-3);
      for (let r = 0; r <= t.duration; r += step) {
        const tau = t.toNote(r);
        expect(tau, `${what}: not monotonic at ${r}`).toBeGreaterThanOrEqual(previousNote);
        previousNote = tau;
        expect(t.toReplay(tau), `${what} at ${r}`).toBeCloseTo(r, 3);
      }
      expect(t.toReplay(t.toNote(t.duration))).toBeCloseTo(t.duration, 9);
    }
  });
});

describe('replay timeline: what is drawn', () => {
  it('handles empty and degenerate notes', () => {
    const emptyStroke: Stroke = { ...timed([0]), points: [] };
    const hidden: Stroke = { ...timed([0, 100]), layer: 1 };
    const empties = [note([]), note([emptyStroke]), note([hidden], [true, false])];
    for (const n of empties) {
      for (const maxPause of [null, 0, 500]) {
        const t = timeline(n, 2, maxPause);
        expect(t.duration).toBe(0);
        expect(t.toNote(123)).toBe(0);
        expect(t.toReplay(123)).toBe(0);
        expect(t.noteAt(0).strokes).toHaveLength(0);
        expect(t.drawingOrder).toEqual([]);
      }
    }

    // Single-point strokes, identical timestamps: they appear together.
    let n = note([timed([500]), timed([500]), timed([2000])]);
    let t = timeline(n);
    expect(t.duration).toBe(2000);
    expect(t.noteAt(499.999).strokes).toHaveLength(0);
    expect(t.noteAt(500).strokes).toHaveLength(2);
    expect(t.noteAt(2000).strokes).toHaveLength(3);
    // Every sample at the same instant: the whole stroke at once.
    n = note([timed([100, 100, 100])]);
    t = timeline(n);
    expect(t.noteAt(99).strokes).toHaveLength(0);
    expect(t.noteAt(100).strokes[0].points).toHaveLength(3);
    // All at time zero: a replay of no length showing everything.
    n = note([timed([0, 0]), timed([0])]);
    t = timeline(n, 1, 0);
    expect(t.duration).toBe(0);
    expect(t.noteAt(0).strokes).toEqual(n.strokes);
    // Times going backwards inside a stroke: a sample never comes before the one it follows.
    n = note([timed([0, 50, 30, 80, 60])]);
    t = timeline(n);
    expect(t.duration).toBe(80);
    const shown = (r: number) => t.noteAt(r).strokes[0].points.length;
    expect([29, 49, 50, 79, 80].map(shown)).toEqual([1, 1, 3, 3, 5]);
  });

  it('neither draws nor waits for hidden layers', () => {
    const shown = line('ballpoint', 0, [0, 0], [900, 0], 0, 1000);
    const hidden = line('ballpoint', 1, [0, 900], [900, 900], 2000, 9000);
    // A layer index without a layer entry is drawn, as by every export.
    const orphan = line('ballpoint', 5, [0, 1900], [900, 1900], 1500, 1800);
    const n = note([shown, hidden, orphan], [true, false]);
    const t = timeline(n);
    expect(t.duration).toBe(1800);
    expect(t.drawingOrder).toEqual([0, 2]);
    expect(t.noteAt(10_000).strokes).toEqual([shown, orphan]);
  });

  it('follows the same rules for synthesized timing', () => {
    const recorded = threeStrokes();
    const synthesized: Note = { ...recorded, timing: 'synthesized', origin: { kind: 'inkspace', id: 'a'.repeat(36) } };
    const o = options(1.5, 400);
    const a = new ReplayTimeline(recorded, o);
    const b = new ReplayTimeline(synthesized, o);
    expect(a.duration).toBe(b.duration);
    for (let r = 0; r < a.duration; r += 37) {
      expect(a.toNote(r)).toBe(b.toNote(r));
      expect(a.noteAt(r).strokes).toEqual(b.noteAt(r).strokes);
    }
  });

  it('shows a stroke point by point, in note order, keeping the original objects of finished strokes', () => {
    const n = threeStrokes();
    const t = timeline(n);
    // Stroke 0 in progress at 500 ms (samples at 0, 52, ... 1000: 10 shown).
    const early = t.noteAt(500);
    expect(early.strokes).toHaveLength(1);
    expect(early.strokes[0]).not.toBe(n.strokes[0]);
    expect(early.strokes[0].points).toEqual(n.strokes[0].points.slice(0, early.strokes[0].points.length));
    expect(early.strokes[0].points.length).toBeGreaterThan(1);
    expect(early.strokes[0].points.length).toBeLessThan(20);
    // Finished strokes are the note's own objects; the one in progress is cut.
    const mid = t.noteAt(3500);
    expect(mid.strokes[0]).toBe(n.strokes[0]);
    expect(mid.strokes[1]).not.toBe(n.strokes[1]);
    expect(mid.strokes).toHaveLength(2);
    // A still frame gives the same note, and the same partial stroke.
    expect(t.noteAt(3500)).toBe(mid);
    expect(t.noteAt(3500.0001).strokes[1]).toBe(mid.strokes[1]);
    const end = t.noteAt(t.duration);
    expect(end.strokes.every((s, i) => s === n.strokes[i])).toBe(true);
    // Everything but the strokes is the note's.
    expect(end.page).toBe(n.page);
    expect(end.layers).toBe(n.layers);
    expect(end.background).toBe(n.background);
    // The note itself is never changed.
    expect(n.strokes[1].points).toHaveLength(20);
  });
});

describe('replay timeline: bookmarks', () => {
  it('neither draws nor waits for a bookmark (the editor is the only one)', () => {
    const plain = threeStrokes();
    const mark = {
      kind: 'bookmark',
      id: 'b',
      layer: 0,
      t: 60_000,
      x: 1000,
      y: 1000,
      scale: 1,
      quarterTurns: 0,
      width: 3000,
      height: 925,
      label: 'Fin',
    };
    const marked: Note = { ...plain, objects: [mark] };
    expect(timeline(marked).duration).toBe(timeline(plain).duration);
    expect(timeline(marked).noteEnd).toBe(timeline(plain).noteEnd);
    // The page shown at the end is the note's strokes.
    expect(timeline(marked).noteAt(timeline(marked).duration).strokes).toEqual(plain.strokes);
  });
});

describe('replay timeline: big notes', () => {
  // Generous bounds: they catch accidental quadratic work, not small regressions.
  it('builds quickly and draws a frame in a millisecond or so (300 strokes, 20 000 samples)', () => {
    const n = bigNote();
    const start = performance.now();
    const t = timeline(n, 1, 500);
    expect(performance.now() - start).toBeLessThan(300);
    expect(t.duration).toBeGreaterThan(0);
    const frames = performance.now();
    let last = t.noteAt(0);
    for (let i = 0; i <= 600; i++) last = t.noteAt((t.duration * i) / 600);
    expect(performance.now() - frames).toBeLessThan(1500);
    expect(last.strokes).toHaveLength(300);
    // The strokes already finished are the note's own, so their outlines stay cached.
    const middle = t.noteAt(t.duration / 2);
    const finished = middle.strokes.filter((s) => n.strokes.includes(s));
    expect(finished.length).toBeGreaterThanOrEqual(middle.strokes.length - 1);
  });
});
