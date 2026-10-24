import { describe, expect, it } from 'vitest';
import en from '../locales/en.json';
import fr from '../locales/fr.json';
import { flattenKeys, interpolate, lookup, placeholders, resolveLocale, type Messages } from './i18n-core';

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
    for (const key of flattenKeys(fr)) {
      expect(placeholders(lookup(en, key) ?? ''), key).toEqual(placeholders(lookup(fr, key) ?? ''));
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

  it('resolves the interface language', () => {
    expect(resolveLocale('auto', 'fr-FR')).toBe('fr');
    expect(resolveLocale('auto', 'en-US')).toBe('en');
    expect(resolveLocale('auto', 'de-DE')).toBe('en');
    expect(resolveLocale('auto', null)).toBe('en');
    expect(resolveLocale('fr', 'en-US')).toBe('fr');
  });
});
