import { describe, expect, it } from 'vitest';
import {
  BOARD_POSITIONS,
  DEFAULT_BOARD_POSITION,
  boardPositionOf,
  drawingTurns,
  isBoardPosition,
  isLandscapePosition,
} from './board-position';

describe('the Xiaomi blackboard\'s positions', () => {
  it('has the four positions of the settings file, upright first', () => {
    expect(BOARD_POSITIONS).toEqual(['buttonsBottom', 'buttonsTop', 'buttonsRight', 'buttonsLeft']);
    expect(DEFAULT_BOARD_POSITION).toBe('buttonsBottom');
  });

  it('recognises its own names only', () => {
    for (const position of BOARD_POSITIONS) expect(isBoardPosition(position)).toBe(true);
    // The Slate's names, a typo, nothing.
    for (const odd of ['landscape', 'portraitLeft', 'buttonsbottom', '', null, undefined, 3]) expect(isBoardPosition(odd)).toBe(false);
  });

  it('turns the picture by the quarter turns that bring the bottom edge where the buttons are', () => {
    expect(BOARD_POSITIONS.map(drawingTurns)).toEqual([0, 2, 3, 1]);
    // Buttons at the left or the right: the board is held wide.
    expect(BOARD_POSITIONS.map(isLandscapePosition)).toEqual([false, false, true, true]);
  });

  it('reads the default when the options lack the field or hold a wrong one', () => {
    expect(boardPositionOf({ boardPosition: 'buttonsLeft' })).toBe('buttonsLeft');
    expect(boardPositionOf({})).toBe('buttonsBottom');
    expect(boardPositionOf({ boardPosition: 'sideways' })).toBe('buttonsBottom');
    expect(boardPositionOf(null)).toBe('buttonsBottom');
  });
});
