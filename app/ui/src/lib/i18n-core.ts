/** Pure translation helpers, kept free of Svelte state so they can be unit-tested. */

export type Locale = 'fr' | 'en';
export type LanguagePreference = 'auto' | Locale;
export const LOCALES: readonly Locale[] = ['fr', 'en'];

export interface Messages {
  [key: string]: string | Messages;
}

/** Every leaf key of a catalog, as dotted paths (`wizard.title`). */
export function flattenKeys(messages: Messages, prefix = ''): string[] {
  return Object.entries(messages).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    return typeof value === 'string' ? [path] : flattenKeys(value, path);
  });
}

export function lookup(messages: Messages, key: string): string | undefined {
  let node: string | Messages | undefined = messages;
  for (const part of key.split('.')) {
    if (node === undefined || typeof node === 'string') return undefined;
    node = node[part];
  }
  return typeof node === 'string' ? node : undefined;
}

/**
 * Plural messages: a catalog entry `{ "one": "…", "other": "…" }` whose wording follows a count,
 * chosen by the language's plural rules (in French, 0 and 1 take `one`; in English only 1 does).
 * Categories other than these two (French `many` for millions) fall back to `other`.
 */
export type PluralCategory = 'one' | 'other';

export function pluralCategory(locale: Locale, count: number): PluralCategory {
  return new Intl.PluralRules(locale).select(count) === 'one' ? 'one' : 'other';
}

/** The form of the plural message `key` for `count`, or undefined when the catalog has none. */
export function lookupPlural(messages: Messages, key: string, locale: Locale, count: number): string | undefined {
  return lookup(messages, `${key}.${pluralCategory(locale, count)}`) ?? lookup(messages, `${key}.other`);
}

/** Whether a catalog node is a plural message (it has an `other` form). */
export function isPluralMessage(node: string | Messages): node is Messages {
  return typeof node !== 'string' && 'other' in node;
}

/** Dotted paths of every plural message of a catalog. */
export function pluralKeys(messages: Messages, prefix = ''): string[] {
  return Object.entries(messages).flatMap(([key, value]) => {
    const path = prefix ? `${prefix}.${key}` : key;
    if (typeof value === 'string') return [];
    return isPluralMessage(value) ? [path] : pluralKeys(value, path);
  });
}

/** Replaces `{name}` placeholders; unknown placeholders are left visible. */
export function interpolate(template: string, params?: Record<string, string | number>): string {
  if (!params) return template;
  return template.replace(/\{(\w+)\}/g, (match, name: string) =>
    name in params ? String(params[name]) : match,
  );
}

export function placeholders(template: string): string[] {
  return [...template.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
}

/** French when Windows is in French, English otherwise, unless the user chose a language. */
export function resolveLocale(preference: LanguagePreference, systemLocale: string | null | undefined): Locale {
  if (preference !== 'auto') return preference;
  return systemLocale?.toLowerCase().startsWith('fr') ? 'fr' : 'en';
}
