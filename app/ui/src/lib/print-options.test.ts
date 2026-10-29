// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import { DEFAULT_PRINT, loadPrintChoice, savePrintChoice } from './print-options';

beforeEach(() => {
  try {
    localStorage.clear();
  } catch {
    // No storage: nothing to clear.
  }
});

describe('print choices', () => {
  it('starts from the defaults and remembers what was chosen', () => {
    expect(loadPrintChoice()).toEqual(DEFAULT_PRINT);
    savePrintChoice({ background: false, fit: 'fit' });
    expect(loadPrintChoice()).toEqual({ background: false, fit: 'fit' });
  });

  it('ignores a stored value that is not valid', () => {
    localStorage.setItem('ardoise.print', JSON.stringify({ background: 'oui', fit: 'zoom' }));
    expect(loadPrintChoice()).toEqual(DEFAULT_PRINT);
    localStorage.setItem('ardoise.print', 'not json');
    expect(loadPrintChoice()).toEqual(DEFAULT_PRINT);
    localStorage.setItem('ardoise.print', 'null');
    expect(loadPrintChoice()).toEqual(DEFAULT_PRINT);
  });
});
