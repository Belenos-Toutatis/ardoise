import { describe, expect, it } from 'vitest';
import {
  BOARD_PAGE,
  BOARD_POSITIONS,
  DEFAULT_BOARD_POSITION,
  boardPage,
  boardPositionOf,
  boardRunOrientation,
  drawingTurns,
  isBoardPage,
  isBoardPosition,
  isLandscapePosition,
} from './board-position';
import { quarterTurns } from './orientation';

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

  it('has the page of each position: swapped on the sides', () => {
    expect(BOARD_PAGE).toEqual({ width: 20500, height: 27200 });
    expect(BOARD_POSITIONS.map(boardPage)).toEqual([
      { width: 20500, height: 27200 },
      { width: 20500, height: 27200 },
      { width: 27200, height: 20500 },
      { width: 27200, height: 20500 },
    ]);
  });

  it('names a run by the Slate word for the same quarter turns, only to count them', () => {
    expect(BOARD_POSITIONS.map(boardRunOrientation)).toEqual(['landscape', 'landscapeFlipped', 'portraitRight', 'portraitLeft']);
    for (const position of BOARD_POSITIONS) expect(quarterTurns(boardRunOrientation(position))).toBe(drawingTurns(position));
  });

  it('knows a page of the blackboard, upright or turned, and no other', () => {
    expect(isBoardPage({ width: 20500, height: 27200 })).toBe(true);
    expect(isBoardPage({ width: 27200, height: 20500 })).toBe(true);
    for (const other of [{ width: 29700, height: 21600 }, { width: 21600, height: 29700 }, { width: 20500, height: 20500 }]) {
      expect(isBoardPage(other)).toBe(false);
    }
  });

  it('reads the default when the options lack the field or hold a wrong one', () => {
    expect(boardPositionOf({ boardPosition: 'buttonsLeft' })).toBe('buttonsLeft');
    expect(boardPositionOf({})).toBe('buttonsBottom');
    expect(boardPositionOf({ boardPosition: 'sideways' })).toBe('buttonsBottom');
    expect(boardPositionOf(null)).toBe('buttonsBottom');
  });
});
