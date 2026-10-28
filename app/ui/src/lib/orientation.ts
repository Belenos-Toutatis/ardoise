/**
 * How the tablet is held (`[tablet] orientation` of `ardoise.toml`): the four positions of Wacom
 * Inkspace. Names are those of the settings file; "left" and "right" in the portrait positions
 * say where the tablet's button ends up. The same arithmetic as `pages::orient` and
 * `pages::turn_point` in Rust (crates/ardoise-device), used by the browser stand-ins.
 */
import type { Stroke } from './note';

export type Orientation = 'landscape' | 'landscapeFlipped' | 'portraitLeft' | 'portraitRight';

/** In the order the interface lists them. */
export const ORIENTATIONS: readonly Orientation[] = ['landscape', 'landscapeFlipped', 'portraitLeft', 'portraitRight'];

export function isOrientation(value: unknown): value is Orientation {
  return ORIENTATIONS.includes(value as Orientation);
}

/** Clockwise quarter turns from the tablet's own frame to the page's. */
export function quarterTurns(orientation: Orientation): number {
  switch (orientation) {
    case 'landscape':
      return 0;
    case 'portraitLeft':
      return 1;
    case 'landscapeFlipped':
      return 2;
    case 'portraitRight':
      return 3;
  }
}

/** Quarter turns that bring a page held as `from` to `to`. */
export function turnsFrom(to: Orientation, from: Orientation): number {
  return (quarterTurns(to) + 4 - quarterTurns(from)) % 4;
}

export function isPortrait(orientation: Orientation): boolean {
  return quarterTurns(orientation) % 2 === 1;
}

/** The page of a tablet whose drawing area is `width` × `height`, held as `orientation`. */
export function pageSizeFor(orientation: Orientation, width: number, height: number): { width: number; height: number } {
  return isPortrait(orientation) ? { width: height, height: width } : { width, height };
}

/** A point of a `width` × `height` page after `turns` clockwise quarter turns. */
export function turnPoint(x: number, y: number, width: number, height: number, turns: number): [number, number] {
  switch (((turns % 4) + 4) % 4) {
    case 1:
      return [height - y, x];
    case 2:
      return [width - x, height - y];
    case 3:
      return [y, width - x];
    default:
      return [x, y];
  }
}

/** A point of the tablet (`width` × `height`) in page coordinates. */
export function orientPoint(x: number, y: number, width: number, height: number, orientation: Orientation): [number, number] {
  return turnPoint(x, y, width, height, quarterTurns(orientation));
}

/** The strokes of a `width` × `height` page turned `turns` clockwise quarter turns. */
export function turnStrokes(strokes: Stroke[], width: number, height: number, turns: number): Stroke[] {
  if (turns % 4 === 0) return strokes;
  return strokes.map((s) => ({
    ...s,
    points: s.points.map((p) => {
      const [x, y] = turnPoint(p.x, p.y, width, height, turns);
      return { ...p, x, y };
    }),
  }));
}
