/**
 * "Redresser les formes" as the user sets it (SPEC §4.4, `docs/questions.md` Q12, milestone M15):
 * the defaults (the fixed thresholds of before M15), the limits of each value, and the
 * `ShapeTuning` that `Ink.straighten` takes. The values live in `[shapes]` of `ardoise.toml`
 * (`AppState.shapes`, `api.setShapes`), which Rust also brings within these limits.
 */
import type { ShapeTuning } from './ink';
import type { ShapeSettings } from './note';

/** The thresholds of before M15: straightening gives exactly the same results with them. */
export const DEFAULT_SHAPE_SETTINGS: Readonly<ShapeSettings> = Object.freeze({
  tolerance: 3,
  minSizeMm: 10,
  snapAngle: true,
  snapAngleDeg: 4,
  snapRound: true,
  snapRoundPercent: 10,
});

/** Inclusive limits of each value (`ardoise_ink::ShapeTuning`'s constants). */
export const SHAPE_LIMITS = Object.freeze({
  tolerance: [1, 5],
  minSizeMm: [5, 25],
  snapAngleDeg: [1, 10],
  snapRoundPercent: [2, 20],
} as const);

/** How much longer than `minSizeMm` a closed shape (circle, ellipse, rectangle) must be, in mm. */
export const CLOSED_EXTRA_MM = 3;

/** Below this minimum size, handwriting may be straightened: the panel says so. */
export const LETTERS_AT_RISK_BELOW_MM = 10;

function within(value: unknown, [min, max]: readonly [number, number], fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? Math.min(max, Math.max(min, value)) : fallback;
}

/** Any stored or typed value made usable: missing or wrong values take their default, numbers come within their limits. */
export function clampShapeSettings(given: Partial<ShapeSettings> | null | undefined): ShapeSettings {
  const d = DEFAULT_SHAPE_SETTINGS;
  const s = given ?? {};
  return {
    tolerance: Math.round(within(s.tolerance, SHAPE_LIMITS.tolerance, d.tolerance)),
    minSizeMm: within(s.minSizeMm, SHAPE_LIMITS.minSizeMm, d.minSizeMm),
    snapAngle: typeof s.snapAngle === 'boolean' ? s.snapAngle : d.snapAngle,
    snapAngleDeg: within(s.snapAngleDeg, SHAPE_LIMITS.snapAngleDeg, d.snapAngleDeg),
    snapRound: typeof s.snapRound === 'boolean' ? s.snapRound : d.snapRound,
    snapRoundPercent: within(s.snapRoundPercent, SHAPE_LIMITS.snapRoundPercent, d.snapRoundPercent),
  };
}

/** What `Ink.straighten` takes: an unticked alignment box is `null`. */
export function toShapeTuning(settings: ShapeSettings): ShapeTuning {
  const s = clampShapeSettings(settings);
  return {
    tolerance: s.tolerance,
    minSizeMm: s.minSizeMm,
    snapAngleDeg: s.snapAngle ? s.snapAngleDeg : null,
    snapRoundPercent: s.snapRound ? s.snapRoundPercent : null,
  };
}

/** Whether `a` and `b` are the same setting. */
export function sameShapeSettings(a: ShapeSettings, b: ShapeSettings): boolean {
  return (
    a.tolerance === b.tolerance &&
    a.minSizeMm === b.minSizeMm &&
    a.snapAngle === b.snapAngle &&
    a.snapAngleDeg === b.snapAngleDeg &&
    a.snapRound === b.snapRound &&
    a.snapRoundPercent === b.snapRoundPercent
  );
}

/** How long the panel waits after the last change before it keeps the values, in ms. */
export const SHAPES_SAVE_DELAY_MS = 300;

/** The smallest closed shape (circle, ellipse, rectangle) for a minimum size, in mm. */
export function closedMinMm(minSizeMm: number): number {
  return minSizeMm + CLOSED_EXTRA_MM;
}

/** Whether a minimum size is small enough for handwriting to be straightened too. */
export function lettersAtRisk(minSizeMm: number): boolean {
  return minSizeMm < LETTERS_AT_RISK_BELOW_MM;
}

/**
 * Keeps the settings through `save` a moment after the last change (a slider moves through many
 * values), and at once on `flush` (the panel closes, the editor goes away). Only the last values
 * are written; a write that fails is reported to `onerror` and the values stay pending until the
 * next change or flush.
 */
export class ShapesSaver {
  #pending: ShapeSettings | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  #running: Promise<void> = Promise.resolve();

  constructor(
    private readonly save: (settings: ShapeSettings) => Promise<unknown>,
    private readonly onerror: (error: unknown) => void = () => {},
    private readonly delayMs = SHAPES_SAVE_DELAY_MS,
  ) {}

  /** Values to keep after the delay (each call restarts it). */
  update(settings: ShapeSettings) {
    this.#pending = settings;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => void this.flush(), this.delayMs);
  }

  /** Whether values wait to be written. */
  get pending(): boolean {
    return this.#pending !== null;
  }

  /** Writes the pending values now; resolves once they (and the writes before) are done. */
  flush(): Promise<void> {
    clearTimeout(this.#timer);
    const settings = this.#pending;
    if (settings === null) return this.#running;
    this.#pending = null;
    this.#running = this.#running.then(async () => {
      try {
        await this.save(settings);
      } catch (error) {
        this.onerror(error);
      }
    });
    return this.#running;
  }
}
