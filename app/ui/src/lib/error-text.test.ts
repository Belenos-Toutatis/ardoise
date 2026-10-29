// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest';
import { errorMessage, genericError } from './api';
import { setLocale } from './i18n.svelte';

afterEach(() => setLocale('fr'));

describe('the sentence of an error shown under « Something went wrong »', () => {
  it('reads as two sentences in French, whatever the case of the coded phrase', () => {
    setLocale('fr');
    // A coded phrase starts in lower case and has no full stop (it also fills other sentences).
    expect(genericError(errorMessage({ code: 'libraryAccess', message: 'accès refusé' }))).toBe(
      'Une erreur est survenue. La bibliothèque ne peut pas être lue ou modifiée (accès refusé).',
    );
    // One that is already a sentence keeps its capital and its full stop.
    expect(genericError(errorMessage({ code: 'bluetoothOff', message: '' }))).toBe(
      'Une erreur est survenue. Le Bluetooth est désactivé : activez-le dans les paramètres de Windows.',
    );
  });

  it('reads as two sentences in English', () => {
    setLocale('en');
    expect(genericError('the library cannot be read (denied)')).toBe(
      'Something went wrong. The library cannot be read (denied).',
    );
    expect(genericError('Already a sentence!')).toBe('Something went wrong. Already a sentence!');
  });

  it('copes with an empty message', () => {
    setLocale('fr');
    expect(genericError('  ')).toBe('Une erreur est survenue.');
  });
});
