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
