/**
 * How the Xiaomi blackboard is held (`[tablet] board_position` of `ardoise.toml`, `boardPosition` on the wire): four
 * positions, named by where its two buttons (erase the screen on the left, store the page on the right of the bottom
 * band) end up. Ardoise turns what the blackboard writes so that it appears upright. The words are those of the
 * settings file; the interface is described in docs/design/blackboard.md §10, what Rust does in §11. The Slate's own
 * four positions are in `orientation.ts`.
 */
import type { Orientation } from './orientation';
import type { BoardPosition } from './tablet';

export type { BoardPosition };

/** The position of a blackboard that was never set: upright, the buttons at the bottom. */
export const DEFAULT_BOARD_POSITION: BoardPosition = 'buttonsBottom';

/** In the order the interface lists them. */
export const BOARD_POSITIONS: readonly BoardPosition[] = ['buttonsBottom', 'buttonsTop', 'buttonsRight', 'buttonsLeft'];

export function isBoardPosition(value: unknown): value is BoardPosition {
  return BOARD_POSITIONS.includes(value as BoardPosition);
}

/**
 * Clockwise quarter turns that bring the picture of the upright blackboard (buttons at the bottom) to a position:
 * the bottom edge goes to the left edge after a quarter turn, to the top after two, to the right after three. For the
 * pictures and for the stand-ins of Rust (`mock-tablet.ts`, `mock-live.ts`), which turn the written page the same way
 * (docs/design/blackboard.md §11, `BoardPosition::quarter_turns`).
 */
export function drawingTurns(position: BoardPosition): number {
  switch (position) {
    case 'buttonsBottom':
      return 0;
    case 'buttonsLeft':
      return 1;
    case 'buttonsTop':
      return 2;
    case 'buttonsRight':
      return 3;
  }
}

/** True for the two positions in which the board is held wider than tall. */
export function isLandscapePosition(position: BoardPosition): boolean {
  return drawingTurns(position) % 2 === 1;
}

/** The position of some options: an older Rust side, which lacks the field, gives the default. */
export function boardPositionOf(options: { boardPosition?: unknown } | null | undefined): BoardPosition {
  const value = options?.boardPosition;
  return isBoardPosition(value) ? value : DEFAULT_BOARD_POSITION;
}

/** The page of the blackboard held upright (the tablet's own frame), in the note's units. */
export const BOARD_PAGE = { width: 20500, height: 27200 } as const;

/** The page of the blackboard held as `position`: its width and height swapped in a landscape position (`board_page`). */
export function boardPage(position: BoardPosition): { width: number; height: number } {
  const { width, height } = BOARD_PAGE;
  return isLandscapePosition(position) ? { width: height, height: width } : { width, height };
}

/**
 * The name the live session gives a run of the blackboard in `position`: the Slate's word for the same quarter turns
 * (`LiveSnapshot.orientation`, `Orientation::from_quarter_turns`). It only counts the turns: during a blackboard run it
 * is never the Slate's orientation, and is never shown or offered as such.
 */
export function boardRunOrientation(position: BoardPosition): Orientation {
  return (['landscape', 'portraitLeft', 'landscapeFlipped', 'portraitRight'] as const)[drawingTurns(position)];
}

/** Whether `page` is a page of the blackboard, upright or turned (`is_blackboard_page`). */
export function isBoardPage(page: { width: number; height: number }): boolean {
  return (
    (page.width === BOARD_PAGE.width && page.height === BOARD_PAGE.height) ||
    (page.width === BOARD_PAGE.height && page.height === BOARD_PAGE.width)
  );
}
