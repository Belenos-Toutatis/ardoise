/**
 * How the Xiaomi blackboard is held (`[tablet] boardPosition` of `ardoise.toml`): four positions, named by where its two
 * buttons (erase the screen on the left, store the page on the right of the bottom band) end up. Ardoise turns what the
 * blackboard writes so that it appears upright. The names are those of the settings file; the contract for the Rust
 * side is in docs/design/blackboard.md §10. The Slate's own four positions are in `orientation.ts`.
 */
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
 * pictures only (`BoardDrawing`); which way the written page is turned is the Rust side's (§9).
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
