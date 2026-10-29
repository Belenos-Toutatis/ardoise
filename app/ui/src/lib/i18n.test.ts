import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import fr from '../locales/fr.json';
import {
  flattenKeys,
  interpolate,
  lookup,
  lookupPlural,
  placeholders,
  pluralCategory,
  pluralKeys,
  resolveLocale,
  type Messages,
} from './i18n-core';

const catalogs: Record<string, Messages> = { fr, en };

describe('translation catalogs', () => {
  it('have exactly the same keys in French and English', () => {
    const frKeys = flattenKeys(fr).sort();
    const enKeys = flattenKeys(en).sort();
    expect(frKeys.filter((k) => !enKeys.includes(k)), 'missing in en.json').toEqual([]);
    expect(enKeys.filter((k) => !frKeys.includes(k)), 'missing in fr.json').toEqual([]);
  });

  it('have no empty message', () => {
    for (const [locale, messages] of Object.entries(catalogs)) {
      const empty = flattenKeys(messages).filter((k) => !lookup(messages, k)?.trim());
      expect(empty, `empty messages in ${locale}`).toEqual([]);
    }
  });

  it('use the same placeholders in both languages', () => {
    // The forms of a plural message may show `{count}` or not (« Une page », "One page").
    const plural = new Set(pluralKeys(fr));
    const inPlural = (key: string) => plural.has(key.split('.').slice(0, -1).join('.'));
    for (const key of flattenKeys(fr)) {
      const used = (messages: Messages) =>
        placeholders(lookup(messages, key) ?? '').filter((name) => !(inPlural(key) && name === 'count'));
      expect(used(en), key).toEqual(used(fr));
    }
  });

  it('give every plural message exactly a `one` and an `other` form', () => {
    expect(pluralKeys(en).sort()).toEqual(pluralKeys(fr).sort());
    for (const [locale, messages] of Object.entries(catalogs)) {
      for (const key of pluralKeys(messages)) {
        const forms = key.split('.').reduce<string | Messages>((node, part) => (node as Messages)[part], messages) as Messages;
        expect(Object.keys(forms).sort(), `${locale} ${key}`).toEqual(['one', 'other']);
      }
    }
  });
});

describe('helpers', () => {
  it('looks up nested keys', () => {
    expect(lookup(fr, 'wizard.title')).toBe('Bienvenue dans Ardoise');
    expect(lookup(fr, 'wizard')).toBeUndefined();
    expect(lookup(fr, 'nope.missing')).toBeUndefined();
  });

  it('interpolates known placeholders and keeps unknown ones', () => {
    expect(interpolate('Version {version}', { version: '1.2' })).toBe('Version 1.2');
    expect(interpolate('{a} and {b}', { a: 1 })).toBe('1 and {b}');
  });

  it('chooses the plural form by the rules of each language', () => {
    expect([0, 1, 2, 1_000_000].map((n) => pluralCategory('fr', n))).toEqual(['one', 'one', 'other', 'other']);
    expect([0, 1, 2].map((n) => pluralCategory('en', n))).toEqual(['other', 'one', 'other']);
    const messages: Messages = { pages: { one: '{count} page', other: '{count} pages' } };
    expect(interpolate(lookupPlural(messages, 'pages', 'fr', 0) ?? '', { count: 0 })).toBe('0 page');
    expect(interpolate(lookupPlural(messages, 'pages', 'en', 0) ?? '', { count: 0 })).toBe('0 pages');
    expect(lookupPlural(messages, 'missing', 'fr', 1)).toBeUndefined();
  });

  it('writes the plurals the issue reported in both languages', () => {
    const say = (locale: 'fr' | 'en', key: string, count: number) =>
      interpolate(lookupPlural(catalogs[locale], key, locale, count) ?? '', { count });
    expect(say('fr', 'library.trashView.confirmEmpty', 1)).toMatch(/^Supprimer définitivement la note de la corbeille/);
    expect(say('fr', 'library.trashView.confirmEmpty', 3)).toMatch(/les 3 notes/);
    expect(say('en', 'library.trashView.confirmEmpty', 1)).toMatch(/^Delete the note in the trash/);
    expect(say('fr', 'print.done', 1)).toBe("Envoyé à l'imprimante : 1 page.");
    expect(say('fr', 'print.done', 4)).toBe("Envoyé à l'imprimante : 4 pages.");
    expect(say('en', 'print.done', 1)).toBe('Sent to the printer: 1 page.');
    expect(say('en', 'print.done', 2)).toBe('Sent to the printer: 2 pages.');
  });

  it('resolves the interface language', () => {
    expect(resolveLocale('auto', 'fr-FR')).toBe('fr');
    expect(resolveLocale('auto', 'en-US')).toBe('en');
    expect(resolveLocale('auto', 'de-DE')).toBe('en');
    expect(resolveLocale('auto', null)).toBe('en');
    expect(resolveLocale('fr', 'en-US')).toBe('fr');
  });
});
