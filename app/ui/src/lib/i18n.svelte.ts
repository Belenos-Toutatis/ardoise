import en from '../locales/en.json';
import fr from '../locales/fr.json';
import { interpolate, lookup, lookupPlural, type Locale, type Messages } from './i18n-core';

export const catalogs: Record<Locale, Messages> = { fr, en };

let current = $state<Locale>('fr');

export function currentLocale(): Locale {
  return current;
}

export function setLocale(locale: Locale): void {
  current = locale;
  document.documentElement.lang = locale;
}

/** Translated text for `key`; falls back to English, then to the key itself so gaps stay visible. */
export function t(key: string, params?: Record<string, string | number>): string {
  const message = lookup(catalogs[current], key) ?? lookup(catalogs.en, key) ?? key;
  return interpolate(message, params);
}

/**
 * Translated text of the plural message `key` (`{ one, other }` in the catalogs) for `count`,
 * which every form can show as `{count}`.
 */
export function tn(key: string, count: number, params?: Record<string, string | number>): string {
  const message = lookupPlural(catalogs[current], key, current, count) ?? lookupPlural(catalogs.en, key, 'en', count) ?? key;
  return interpolate(message, { count, ...params });
}
