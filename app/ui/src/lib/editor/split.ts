/**
 * Splitting a note in two: which strokes (or parts of strokes) go to the new note. The editor
 * previews it live; the browser stand-in applies it. Same rules as `ardoise_core::ops`
 * (docs/design/exports-and-operations.md):
 *
 * - at an instant: samples with `t < at` stay, samples with `t >= at` move; a stroke that spans
 *   the instant is cut in two;
 * - by zone: a stroke moves when at least half of its samples are inside the polygon, only
 *   strokes on visible layers (the rule of the lasso, `selectInLasso`).
 */
import type { Note, SplitSpec, Stroke } from '../note';
import type { Vec } from './geometry';
import { selectInLasso } from './hit';

export type SplitMode = 'time' | 'zone';

/** First and last sample time of the whole note, milliseconds; null while it has no sample. */
export function timeRange(note: Note): { min: number; max: number } | null {
  let min = Infinity;
  let max = -Infinity;
  for (const stroke of note.strokes) {
    for (const p of stroke.points) {
      if (p.t < min) min = p.t;
      if (p.t > max) max = p.t;
    }
  }
  return min <= max ? { min, max } : null;
}

/** `1:23.4` (minutes, seconds, tenths), or `1:02:03.4` from one hour on. */
export function formatTimecode(ms: number): string {
  const tenths = Math.floor(Math.max(0, ms) / 100);
  const total = Math.floor(tenths / 10);
  const s = total % 60;
  const m = Math.floor(total / 60) % 60;
  const h = Math.floor(total / 3600);
  const two = (n: number) => String(n).padStart(2, '0');
  return `${h > 0 ? `${h}:${two(m)}` : String(m)}:${two(s)}.${tenths % 10}`;
}

/** What a split would do, stroke by stroke. */
export interface SplitPreview {
  /** Indices of the strokes that move entirely to the new note. */
  moved: ReadonlySet<number>;
  /** Strokes cut in two at the instant: the part that stays and the part that moves. */
  cut: ReadonlyMap<number, { kept: Stroke; moved: Stroke }>;
  /** Strokes with samples left in this note (a cut stroke counts on both sides). */
  keptCount: number;
  /** Strokes with something going to the new note. */
  movedCount: number;
}

export const EMPTY_PREVIEW: SplitPreview = { moved: new Set(), cut: new Map(), keptCount: 0, movedCount: 0 };

/** Both sides get something; otherwise the split is refused (`nothingToSplit`). */
export function canSplit(preview: SplitPreview): boolean {
  return preview.movedCount > 0 && preview.keptCount > 0;
}

function build(note: Note, moved: Set<number>, cut: Map<number, { kept: Stroke; moved: Stroke }>): SplitPreview {
  // A stroke without samples stays where it is but does not count: the desktop app refuses the
  // split (`nothingToSplit`) when every stroke that has samples moves.
  let keptCount = 0;
  note.strokes.forEach((stroke, i) => {
    if (stroke.points.length > 0 && !moved.has(i)) keptCount++;
  });
  return { moved, cut, keptCount, movedCount: moved.size + cut.size };
}

/** The strokes that move when cutting at the instant `at` (ms since `created`). */
export function previewAtTime(note: Note, at: number): SplitPreview {
  const moved = new Set<number>();
  const cut = new Map<number, { kept: Stroke; moved: Stroke }>();
  note.strokes.forEach((stroke, i) => {
    // Count first: the slider calls this at every step and almost no stroke is cut.
    let after = 0;
    for (const p of stroke.points) if (p.t >= at) after++;
    if (after === 0) return;
    if (after === stroke.points.length) moved.add(i);
    else {
      cut.set(i, {
        kept: { ...stroke, points: stroke.points.filter((p) => p.t < at) },
        moved: { ...stroke, points: stroke.points.filter((p) => p.t >= at) },
      });
    }
  });
  return build(note, moved, cut);
}

/** The strokes that move when cutting out the zone `polygon` (page units). */
export function previewZone(note: Note, polygon: readonly Vec[]): SplitPreview {
  return build(note, new Set(selectInLasso(note, polygon)), new Map());
}

export function previewSplit(note: Note, spec: SplitSpec | null): SplitPreview {
  if (!spec) return EMPTY_PREVIEW;
  return spec.kind === 'time'
    ? previewAtTime(note, spec.at)
    : previewZone(
        note,
        spec.polygon.map(([x, y]) => ({ x, y })),
      );
}

/**
 * Applies a split: the note that stays and the content of the new one (`id` and `title` empty,
 * to be filled by the caller), or null when one side would be empty. The new note's `created`
 * is the instant of its first sample for a cut in time, whose sample times then start at 0.
 */
export function applySplit(note: Note, spec: SplitSpec): { kept: Note; extracted: Note } | null {
  const preview = previewSplit(note, spec);
  if (!canSplit(preview)) return null;
  const keptStrokes: Stroke[] = [];
  let movedStrokes: Stroke[] = [];
  note.strokes.forEach((stroke, i) => {
    const part = preview.cut.get(i);
    if (part) {
      keptStrokes.push(part.kept);
      movedStrokes.push(part.moved);
    } else if (preview.moved.has(i)) movedStrokes.push(stroke);
    else keptStrokes.push(stroke);
  });
  let created = note.created;
  if (spec.kind === 'time') {
    // A loop, not `Math.min(...samples)`: a very large note would overflow the call's arguments.
    let first = Infinity;
    for (const stroke of movedStrokes) for (const p of stroke.points) if (p.t < first) first = p.t;
    created = note.created + first;
    movedStrokes = movedStrokes.map((s) => ({ ...s, points: s.points.map((p) => ({ ...p, t: p.t - first })) }));
  }
  const now = Date.now();
  return {
    kept: { ...note, strokes: keptStrokes },
    extracted: {
      ...note,
      id: '',
      title: '',
      created,
      modified: now,
      tags: [...note.tags],
      favorite: false,
      layers: note.layers.map((layer) => ({ ...layer })),
      strokes: movedStrokes,
      origin: { kind: 'ardoise' },
    },
  };
}
