import en from '../locales/en.json';
import fr from '../locales/fr.json';
import { interpolate, lookup, type Locale, type Messages } from './i18n-core';

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
