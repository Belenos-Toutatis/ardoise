import { describe, expect, it } from 'vitest';
import type { Note } from '../note';
import { applySplit, canSplit, formatTimecode, previewAtTime, previewSplit, previewZone, timeRange } from './split';
import { hline, line, note } from './test-fixtures';

/** Two strokes written one after the other: A over t 0..90 ms, B over t 1000..1090 ms. */
function twoStrokes(): Note {
  const a = hline(1000, 1900, 2000); // 10 samples, t = 0, 10, … 90
  const b = hline(1000, 1900, 8000);
  b.points = b.points.map((p) => ({ ...p, t: 1000 + p.t }));
  return { ...note([a, b]), created: 5_000 };
}

describe('time range and timecode', () => {
  it('spans the samples of every stroke', () => {
    expect(timeRange(twoStrokes())).toEqual({ min: 0, max: 1090 });
    expect(timeRange(note([]))).toBeNull();
  });

  it('formats minutes, seconds and tenths', () => {
    expect(formatTimecode(0)).toBe('0:00.0');
    expect(formatTimecode(83_400)).toBe('1:23.4');
    expect(formatTimecode(3_723_050)).toBe('1:02:03.0');
    expect(formatTimecode(-5)).toBe('0:00.0');
  });
});

describe('split preview', () => {
  it('moves the strokes written from the instant on, and cuts the one that spans it', () => {
    const preview = previewAtTime(twoStrokes(), 500);
    expect([...preview.moved]).toEqual([1]);
    expect(preview.cut.size).toBe(0);
    expect(preview.keptCount).toBe(1);
    expect(preview.movedCount).toBe(1);

    const cut = previewAtTime(twoStrokes(), 45);
    expect([...cut.moved]).toEqual([1]);
    const part = cut.cut.get(0)!;
    expect(part.kept.points.map((p) => p.t)).toEqual([0, 10, 20, 30, 40]);
    expect(part.moved.points.map((p) => p.t)).toEqual([50, 60, 70, 80, 90]);
    // Stroke A has a part on each side; stroke B left entirely.
    expect(cut.keptCount).toBe(1);
    expect(cut.movedCount).toBe(2);
    expect(canSplit(cut)).toBe(true);
  });

  it('refuses a cut that leaves one side empty', () => {
    expect(canSplit(previewAtTime(twoStrokes(), 0))).toBe(false); // everything moves
    expect(canSplit(previewAtTime(twoStrokes(), 5000))).toBe(false); // nothing moves
    expect(canSplit(previewSplit(twoStrokes(), null))).toBe(false);
  });

  it('moves the strokes mostly inside the zone, only on visible layers', () => {
    const base = twoStrokes();
    const around = (y0: number, y1: number): [number, number][] => [
      [500, y0],
      [3000, y0],
      [3000, y1],
      [500, y1],
    ];
    const first = previewSplit(base, { kind: 'zone', polygon: around(1500, 2500) });
    expect([...first.moved]).toEqual([0]);
    expect(canSplit(first)).toBe(true);

    const hidden: Note = { ...base, layers: [{ name: 'L1', visible: false }] };
    expect(previewZone(hidden, around(1500, 2500).map(([x, y]) => ({ x, y }))).movedCount).toBe(0);

    // A zone around everything leaves nothing here.
    expect(canSplit(previewSplit(base, { kind: 'zone', polygon: around(0, 20000) }))).toBe(false);
  });
});

describe('applying a split', () => {
  it('cuts in time: times of the new note restart at 0 and its date follows', () => {
    const parts = applySplit(twoStrokes(), { kind: 'time', at: 45 })!;
    expect(parts.kept.strokes).toHaveLength(1);
    expect(parts.kept.strokes[0].points.map((p) => p.t)).toEqual([0, 10, 20, 30, 40]);
    expect(parts.extracted.strokes).toHaveLength(2);
    expect(parts.extracted.strokes[0].points.map((p) => p.t)).toEqual([0, 10, 20, 30, 40]);
    expect(parts.extracted.strokes[1].points[0].t).toBe(950);
    expect(parts.extracted.created).toBe(5_000 + 50);
    expect(parts.extracted.origin).toEqual({ kind: 'ardoise' });
    expect(parts.extracted.layers).toEqual(parts.kept.layers);
    expect(parts.extracted.page).toEqual(parts.kept.page);
  });

  it('cuts by zone: strokes move whole, positions and times unchanged', () => {
    const base = twoStrokes();
    const parts = applySplit(base, {
      kind: 'zone',
      polygon: [
        [500, 7000],
        [3000, 7000],
        [3000, 9000],
        [500, 9000],
      ],
    })!;
    expect(parts.extracted.strokes).toEqual([base.strokes[1]]);
    expect(parts.kept.strokes).toEqual([base.strokes[0]]);
    expect(parts.extracted.created).toBe(base.created);
  });

  it('returns null when one side would be empty', () => {
    expect(applySplit(twoStrokes(), { kind: 'time', at: 0 })).toBeNull();
    expect(applySplit(note([line([[0, 0]])]), { kind: 'time', at: 5 })).toBeNull();
  });
});
