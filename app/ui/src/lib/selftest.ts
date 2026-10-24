import type { UiSelfTestReport } from './api';
import { flattenKeys, type Locale, type Messages } from './i18n-core';

/** Checks run inside the webview for `ardoise.exe --self-test`. */
export function uiSelfTest(root: Element | null, catalogs: Record<Locale, Messages>, locale: Locale): UiSelfTestReport {
  const fr = flattenKeys(catalogs.fr).sort();
  const en = flattenKeys(catalogs.en).sort();
  return {
    rendered: (root?.childElementCount ?? 0) > 0,
    i18nConsistent: fr.length > 0 && fr.join('\n') === en.join('\n'),
    locale,
    keyCount: fr.length,
  };
}
