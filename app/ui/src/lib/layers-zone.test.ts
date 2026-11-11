import { describe, expect, it } from 'vitest';
import type { Layer } from './note';
import { followMerge, followMove, layerWarnings, rowsTopFirst } from './layers-zone';
import { moveIndex } from './editor/ops';

const layers = (...visible: boolean[]): Layer[] => visible.map((v, i) => ({ name: `Calque ${i + 1}`, visible: v }));

describe('rowsTopFirst', () => {
  it('lists the top layer first, each with its index in the note', () => {
    const rows = rowsTopFirst(layers(true, false, true));
    expect(rows.map((r) => r.index)).toEqual([2, 1, 0]);
    expect(rows.map((r) => r.layer.name)).toEqual(['Calque 3', 'Calque 2', 'Calque 1']);
  });

  it('gives nothing for no layer and does not reorder its input', () => {
    expect(rowsTopFirst([])).toEqual([]);
    const input = layers(true, true);
    rowsTopFirst(input);
    expect(input.map((l) => l.name)).toEqual(['Calque 1', 'Calque 2']);
  });
});

describe('layerWarnings', () => {
  it('is silent while the chosen layer and the tablet’s layer show', () => {
    expect(layerWarnings(layers(true, true), 1, 0)).toEqual([]);
    expect(layerWarnings(layers(true, true), 1)).toEqual([]);
  });

  it('warns when the chosen layer is hidden', () => {
    expect(layerWarnings(layers(true, false), 1)).toEqual([{ kind: 'chosenHidden', index: 1, name: 'Calque 2' }]);
  });

  it('warns about the tablet’s layer only when there is a tablet', () => {
    expect(layerWarnings(layers(false, true), 1, 0)).toEqual([{ kind: 'tabletHidden', index: 0, name: 'Calque 1' }]);
    expect(layerWarnings(layers(false, true), 1)).toEqual([]);
  });

  it('puts the tablet’s warning first when both apply, also on one and the same layer', () => {
    expect(layerWarnings(layers(false, false), 1, 0).map((w) => [w.kind, w.index])).toEqual([
      ['tabletHidden', 0],
      ['chosenHidden', 1],
    ]);
    expect(layerWarnings(layers(true, false), 1, 1).map((w) => w.kind)).toEqual(['tabletHidden', 'chosenHidden']);
  });

  it('ignores an index without a layer', () => {
    expect(layerWarnings(layers(true), 4, 7)).toEqual([]);
    expect(layerWarnings([], 0, 0)).toEqual([]);
  });
});

describe('followMove', () => {
  it('follows the layer that moves', () => {
    expect(followMove(1, 1, 2)).toBe(2);
    expect(followMove(2, 2, 0)).toBe(0);
  });

  it('shifts the layers it passes over', () => {
    // 0 1 2 3, layer 1 up to 3: layers 2 and 3 go down.
    expect([0, 1, 2, 3].map((i) => followMove(i, 1, 3))).toEqual([0, 3, 1, 2]);
    // layer 3 down to 1: layers 1 and 2 go up.
    expect([0, 1, 2, 3].map((i) => followMove(i, 3, 1))).toEqual([0, 2, 3, 1]);
  });

  it('leaves the others alone and agrees with the editor’s move for every case', () => {
    for (let count = 1; count <= 5; count++) {
      for (let from = 0; from < count; from++) {
        for (let to = 0; to < count; to++) {
          for (let index = 0; index < count; index++) {
            expect(followMove(index, from, to)).toBe(moveIndex(index, from, to));
          }
        }
      }
    }
  });
});

describe('followMerge', () => {
  it('takes the merged layer down onto the one below it and shifts the ones above', () => {
    expect([0, 1, 2, 3].map((i) => followMerge(i, 2))).toEqual([0, 1, 1, 2]);
  });

  it('never goes below the bottom layer', () => {
    expect(followMerge(0, 0)).toBe(0);
  });
});
