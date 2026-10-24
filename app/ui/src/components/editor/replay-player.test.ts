import { describe, expect, it } from 'vitest';
import { BLACK, note } from '../../lib/editor/test-fixtures';
import type { Note, Stroke } from '../../lib/note';
import { TRIMMED_PAUSE_MS } from '../../lib/video';
import { ReplayPlayer, type FrameClock } from './replay-player.svelte';

/** A clock moved by hand: frames are requested, then run by `advance`. */
function manualClock() {
  let now = 1000;
  let next = 0;
  const pending = new Map<number, () => void>();
  const clock: FrameClock = {
    now: () => now,
    request: (callback) => {
      next += 1;
      pending.set(next, callback);
      return next;
    },
    cancel: (handle) => void pending.delete(handle),
  };
  return {
    clock,
    pending: () => pending.size,
    /** Runs frames every 16 ms for `ms` milliseconds. */
    advance(ms: number) {
      for (let spent = 0; spent < ms; spent += 16) {
        now += 16;
        const frames = [...pending.entries()];
        pending.clear();
        for (const [, callback] of frames) callback();
      }
    },
  };
}

/** Strokes over [0, 1000], [3000, 4000] and [4200, 5000] ms (21 samples each). */
function slowNote(): Note {
  const stroke = (y: number, t0: number, t1: number): Stroke => ({
    tool: 'ballpoint',
    color: BLACK,
    width: 60,
    layer: 0,
    points: Array.from({ length: 21 }, (_, i) => ({
      x: 2000 + i * 500,
      y,
      pressure: 0.5,
      t: Math.round(t0 + ((t1 - t0) * i) / 20),
    })),
  });
  return note([stroke(3000, 0, 1000), stroke(6000, 3000, 4000), stroke(9000, 4200, 5000)]);
}

function player() {
  const hand = manualClock();
  const p = new ReplayPlayer(hand.clock);
  p.load(slowNote());
  return { p, hand };
}

describe('replay player', () => {
  it('knows the length of the replay and draws nothing but the first samples at the start', () => {
    const { p } = player();
    expect(p.end).toBe(5000);
    expect(p.position).toBe(0);
    expect(p.playing).toBe(false);
    expect(p.shown?.strokes).toHaveLength(1);
    expect(p.shown?.strokes[0].points).toHaveLength(1);
  });

  it('plays with the clock, stops at the end, and plays again from the start', () => {
    const { p, hand } = player();
    p.play();
    expect(p.playing).toBe(true);
    hand.advance(2000);
    expect(p.position).toBeGreaterThan(1900);
    expect(p.position).toBeLessThanOrEqual(2016);
    expect(p.shown?.strokes).toHaveLength(1);
    hand.advance(1600);
    expect(p.shown?.strokes).toHaveLength(2);
    hand.advance(4000);
    expect(p.playing).toBe(false);
    expect(p.position).toBe(5000);
    expect(hand.pending()).toBe(0);
    expect(p.shown?.strokes).toHaveLength(3);
    // Looping is off: play again starts over.
    p.play();
    expect(p.position).toBe(0);
    hand.advance(160);
    expect(p.playing).toBe(true);
    expect(p.position).toBeGreaterThan(100);
    p.pause();
    const at = p.position;
    hand.advance(500);
    expect(p.position).toBe(at);
    expect(hand.pending()).toBe(0);
  });

  it('seeks within the replay and plays on from there', () => {
    const { p, hand } = player();
    p.seek(3500);
    expect(p.position).toBe(3500);
    expect(p.shown?.strokes).toHaveLength(2);
    p.seek(-10);
    expect(p.position).toBe(0);
    p.seek(1e9);
    expect(p.position).toBe(5000);
    p.seek(4000);
    p.play();
    hand.advance(480);
    expect(p.position).toBeGreaterThan(4400);
    expect(p.position).toBeLessThan(4600);
  });

  it('changes speed and trims pauses without moving what is on the page', () => {
    const { p } = player();
    p.seek(3500);
    p.setSpeed(2);
    expect(p.end).toBe(2500);
    expect(p.position).toBeCloseTo(1750, 6);
    expect(p.shown?.strokes).toHaveLength(2);
    p.setSpeed(1);
    expect(p.position).toBeCloseTo(3500, 6);
    p.setTrim(true);
    expect(p.options.maxPauseMs).toBe(TRIMMED_PAUSE_MS);
    expect(p.trimmed).toBe(true);
    expect(p.end).toBe(3500);
    // 1000 + 500 of the 2000 ms pause + 500 into the second stroke.
    expect(p.position).toBeCloseTo(2000, 6);
    expect(p.shown?.strokes).toHaveLength(2);
    p.setTrim(false);
    expect(p.end).toBe(5000);
    // Speeds stay within 0.25x and 16x.
    p.setSpeed(100);
    expect(p.options.speed).toBe(16);
    p.setSpeed(0);
    expect(p.options.speed).toBe(0.25);
  });

  it('stays at the end when the options change there', () => {
    const { p } = player();
    p.seek(5000);
    p.setSpeed(4);
    expect(p.position).toBe(1250);
    expect(p.end).toBe(1250);
  });

  it('goes on past the end of the drawing while a narration is longer', () => {
    const { p, hand } = player();
    p.extent = 7000;
    expect(p.end).toBe(7000);
    p.seek(4900);
    p.play();
    hand.advance(1000);
    // The last image stays while the sound goes on.
    expect(p.playing).toBe(true);
    expect(p.shown?.strokes).toHaveLength(3);
    hand.advance(2500);
    expect(p.playing).toBe(false);
    expect(p.position).toBe(7000);
    p.extent = 0;
    p.seek(p.position);
    expect(p.position).toBe(5000);
  });

  it('cannot play an empty note, and starts over for another one', () => {
    const hand = manualClock();
    const p = new ReplayPlayer(hand.clock);
    p.load(note([]));
    p.play();
    expect(p.playing).toBe(false);
    expect(p.end).toBe(0);
    p.load(slowNote());
    p.setSpeed(2);
    p.seek(1000);
    p.load(slowNote());
    expect(p.position).toBe(0);
    // The options are kept from one replay to the next.
    expect(p.options.speed).toBe(2);
  });
});
