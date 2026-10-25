import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import fr from '../locales/fr.json';
import {
  hasRecognizer,
  missingRecognizer,
  searchNotice,
  statusNotice,
  textFileName,
  type IndexStatus,
  type RecognizerInfo,
} from './handwriting';
import { lookup } from './i18n-core';

const french: RecognizerInfo = { name: "Reconnaissance d'écriture Microsoft - Français", language: 'fr' };
const english: RecognizerInfo = { name: 'Microsoft English (US) Handwriting Recognizer', language: 'en' };
const spanish: RecognizerInfo = { name: 'Español', language: null };

function status(partial: Partial<IndexStatus>): IndexStatus {
  return { state: 'idle', reason: null, language: 'fr', done: 3, total: 3, failed: 0, ...partial };
}

describe('recognizers', () => {
  it('tells which language pack is worth installing', () => {
    expect(hasRecognizer([spanish, french], 'fr')).toBe(true);
    expect(hasRecognizer([spanish, french], 'en')).toBe(false);
    const frenchPc = { supported: true, recognizers: [french, spanish] };
    // Automatic: the interface language.
    expect(missingRecognizer({ ...frenchPc, setting: 'auto' }, 'fr')).toBeNull();
    expect(missingRecognizer({ ...frenchPc, setting: 'auto' }, 'en')).toBe('en');
    // A language chosen.
    expect(missingRecognizer({ ...frenchPc, setting: 'en' }, 'fr')).toBe('en');
    expect(missingRecognizer({ supported: true, recognizers: [english], setting: 'auto' }, 'fr')).toBe('fr');
    expect(missingRecognizer({ supported: true, recognizers: [english, french], setting: 'en' }, 'fr')).toBeNull();
    // Not Windows: nothing to install.
    expect(missingRecognizer({ supported: false, recognizers: [], setting: 'auto' }, 'fr')).toBeNull();
  });
});

describe('notices', () => {
  it('says in the search when the handwriting is not all searched', () => {
    expect(searchNotice(null)).toBeNull();
    expect(searchNotice(status({}))).toBeNull();
    expect(searchNotice(status({ state: 'indexing', done: 1 }))).toEqual({
      key: 'handwriting.search.indexing',
      values: { done: 1, total: 3 },
    });
    expect(searchNotice(status({ state: 'paused' }))?.key).toBe('handwriting.search.paused');
    expect(searchNotice(status({ state: 'stopped' }))?.key).toBe('handwriting.search.stopped');
    expect(searchNotice(status({ state: 'unavailable', reason: 'noRecognizer' }))?.key).toBe(
      'handwriting.search.noRecognizer',
    );
    expect(searchNotice(status({ state: 'unavailable', reason: 'unsupported' }))?.key).toBe(
      'handwriting.search.unsupported',
    );
    // No library: the library screen is not shown anyway.
    expect(searchNotice(status({ state: 'unavailable', reason: 'noLibrary' }))).toBeNull();
  });

  it('states the indexing in the settings, with an existing sentence for every state', () => {
    const states: IndexStatus[] = [
      status({}),
      status({ failed: 1, done: 2 }),
      status({ state: 'indexing' }),
      status({ state: 'paused' }),
      status({ state: 'stopped' }),
      status({ state: 'unavailable', reason: 'unsupported' }),
      status({ state: 'unavailable', reason: 'noRecognizer' }),
      status({ state: 'unavailable', reason: 'noLibrary' }),
    ];
    for (const s of states) {
      const { key } = statusNotice(s);
      expect(lookup(fr, key), key).toBeTruthy();
      expect(lookup(en, key), key).toBeTruthy();
    }
    expect(statusNotice(status({ failed: 1, done: 2 })).key).toBe('handwriting.status.idleFailed');
    expect(textFileName('2026-10-24 Cours')).toBe('2026-10-24 Cours.txt');
  });
});
