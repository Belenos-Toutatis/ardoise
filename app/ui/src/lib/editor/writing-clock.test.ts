// The time model of a narration recorded while writing in the editor (milestone M17; docs/design/paper-projection-narration.md
// §3.2, §3.8): where it starts in note time, the clock of the strokes, the wall clock Rust places the sound with, and
// where a stroke goes after such a narration.
import { describe, expect, it } from 'vitest';
import type { NarrationInfo } from '../video';
import { STROKE_GAP_MS, WRITING_GAP_MS, WritingClock, lastSampleTime, nextStrokeBase, noteSoundEnd, writingOrigin } from './writing-clock';
import { line, note } from './test-fixtures';

/** A stroke whose samples are at `times`, on `layer`. */
const timed = (times: number[], layer = 0) => {
  const stroke = line(
    times.map((_, i) => [1000 + i * 100, 2000]),
    layer,
  );
  return { ...stroke, points: stroke.points.map((p, i) => ({ ...p, t: times[i] })) };
};

const noteTime = (startMs: number, durationMs: number): NarrationInfo => ({ durationMs, base: 'note', speed: 1, maxPauseMs: null, startMs });
const replayTime = (startMs: number, durationMs: number): NarrationInfo => ({
  durationMs,
  base: 'replay',
  speed: 2,
  maxPauseMs: 500,
  startMs,
});

describe('writingOrigin', () => {
  it('starts a second after the last sample of the note', () => {
    expect(WRITING_GAP_MS).toBe(1000);
    expect(writingOrigin(note([timed([0, 400, 900]), timed([2000, 2500])]))).toBe(2500 + 1000);
  });

  it('counts the strokes of hidden layers, and the latest sample wherever it is', () => {
    const n = note([timed([5000, 9000], 1), timed([100, 200])], 2);
    n.layers[1].visible = false;
    expect(lastSampleTime(n)).toBe(9000);
    expect(writingOrigin(n)).toBe(10_000);
  });

  it('is 0 for a note without ink', () => {
    expect(lastSampleTime(note([]))).toBe(-1);
    expect(writingOrigin(note([]))).toBe(0);
  });
});

describe('WritingClock', () => {
  it('times a pointer event from the start: the origin plus the time since', () => {
    const clock = new WritingClock(3500, 10_000, 1_700_000_000_000);
    expect(clock.noteTime(10_000)).toBe(3500);
    expect(clock.noteTime(11_250.5)).toBe(4750.5);
    expect(clock.origin).toBe(3500);
    expect(clock.startedAt).toBe(10_000);
  });

  it('gives the wall clock of note time 0, from the wall clock at the start', () => {
    const wall = 1_700_000_000_000;
    const clock = new WritingClock(3500, 10_000, wall);
    expect(clock.noteZeroWallMs).toBe(wall - 3500);
    // A stroke drawn 2 s after the start and the sound heard at the same wall instant share their note time: Rust places
    // the sound's first sample (wall `w0`) at `w0 - noteZeroWallMs`, and the sample at wall `w` at `w - noteZeroWallMs`.
    const w = wall + 2000;
    expect(w - clock.noteZeroWallMs).toBe(clock.noteTime(10_000 + 2000));
  });

  it('starts at note time 0 on an empty page (a sound begun a few ms before it is a lead-in)', () => {
    const clock = new WritingClock(writingOrigin(note([])), 500, 1_000_000);
    expect(clock.noteTime(500)).toBe(0);
    expect(clock.noteZeroWallMs).toBe(1_000_000);
    // A microphone that opened 200 ms before the start: the sound begins at -200 in note time.
    expect(1_000_000 - 200 - clock.noteZeroWallMs).toBe(-200);
  });

  it('reads the wall clock by itself when not given one', () => {
    const before = Date.now();
    const clock = new WritingClock(100, 0);
    expect(clock.noteZeroWallMs).toBeGreaterThanOrEqual(before - 100);
    expect(clock.noteZeroWallMs).toBeLessThanOrEqual(Date.now() - 100);
  });
});

describe('nextStrokeBase', () => {
  it('without a narration, is the editor’s rule: 500 ms after the last sample (499 on an empty page)', () => {
    expect(STROKE_GAP_MS).toBe(500);
    expect(nextStrokeBase(note([timed([0, 1200])]), null)).toBe(1700);
    expect(nextStrokeBase(note([]), null)).toBe(499);
    expect(nextStrokeBase(note([timed([0, 1200])]), undefined)).toBe(1700);
  });

  it('after a narration in note time that ends after the ink, goes after its end (O14)', () => {
    const n = note([timed([0, 1200]), timed([3000, 4000])]);
    // Recorded while writing: from 4800 for 6 s, the teacher talked on after the last stroke.
    expect(noteSoundEnd(noteTime(4800, 6000))).toBe(10_800);
    expect(nextStrokeBase(n, noteTime(4800, 6000))).toBe(11_300);
    // A sound that ended before the last stroke changes nothing.
    expect(nextStrokeBase(n, noteTime(0, 2000))).toBe(4500);
  });

  it('keeps the editor’s rule after a replay narration (its times are the replay’s)', () => {
    const n = note([timed([0, 1200])]);
    expect(noteSoundEnd(replayTime(0, 60_000))).toBeNull();
    expect(nextStrokeBase(n, replayTime(0, 60_000))).toBe(1700);
    expect(noteSoundEnd(null)).toBeNull();
  });

  it('places the first stroke of an empty page after a narration recorded alone (a commentary)', () => {
    expect(nextStrokeBase(note([]), noteTime(-150, 3000))).toBe(2850 + 500);
  });
});
