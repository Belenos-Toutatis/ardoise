// @vitest-environment jsdom
// Milestone M14 (review): the live window tells a refused act of the computer's hand in the words of that act, not in
// those of the selection's touch-ups (« refaites la sélection »).
import { afterEach, describe, expect, it } from 'vitest';
import { errorMessage } from './api';
import { setLocale, t } from './i18n.svelte';
import { computerErrorText } from './live-errors';

const stale = { code: 'liveEditStale', message: 'stale' };
const invalid = { code: 'liveEditInvalid', message: 'invalid' };

afterEach(() => setLocale('fr'));

describe('the text of a refused act of the computer', () => {
  it('a layer operation on a page that changed asks to do it again, not to select again', () => {
    setLocale('fr');
    expect(computerErrorText(stale, 'layers')).toBe("La page a changé entre-temps : refaites l'opération sur les calques.");
    expect(computerErrorText(stale, 'layers')).not.toContain('sélection');
    expect(computerErrorText(invalid, 'layers')).toBe(t('errorCodes.liveLayerInvalid'));
    setLocale('en');
    expect(computerErrorText(stale, 'layers')).toBe('The page changed in the meantime: do the layer operation again.');
  });

  it('a stroke of the pen or an erasing that cannot be done says so', () => {
    setLocale('fr');
    expect(computerErrorText(invalid, 'pen')).toBe("Ce trait n'a pas pu être écrit.");
    expect(computerErrorText(invalid, 'eraser')).toBe("Ce coup de gomme n'a pas pu être fait.");
    setLocale('en');
    expect(computerErrorText(invalid, 'pen')).toBe('This stroke could not be written.');
    expect(computerErrorText(stale, 'eraser')).toBe('This erasing could not be done.');
  });

  it('keeps the sentences that already fit', () => {
    setLocale('fr');
    for (const code of ['liveLayerHidden', 'liveLayerInvalid', 'liveNotRunning']) {
      const error = { code, message: '' };
      for (const act of ['pen', 'eraser', 'layers'] as const) expect(computerErrorText(error, act)).toBe(errorMessage(error));
    }
  });
});
