/**
 * The words of the « pages first » refusal of live mode (SPEC §4.2, docs/design/blackboard.md §9.5): the Xiaomi
 * blackboard still holds stored pages and may not sync them first, so live mode is not started until they are
 * imported. Kept apart from the dialog's state (`live-pages-first.svelte.ts`) so that `errorMessage` can use it.
 */
import { t } from './i18n.svelte';

/** The code `live_start` and `live_resume` are refused with, and of the `refused` live event. */
export const PAGES_FIRST_CODE = 'blackboardPagesFirst';

/** The error (a rejected command, or the `refused` event) is that refusal. */
export function isPagesFirst(error: unknown): boolean {
  return typeof error === 'object' && error !== null && (error as { code?: unknown }).code === PAGES_FIRST_CODE;
}

/**
 * The sentence for `count` stored pages (one form for 1, one for several: `errorCodes.blackboardPagesFirst` and
 * `…One`). `{count}` never reaches the screen: this is the only way the text is built.
 */
export function pagesFirstText(count: number): string {
  return count === 1 ? t('errorCodes.blackboardPagesFirstOne') : t('errorCodes.blackboardPagesFirst', { count });
}

/** The count the service put in its (English) message, « the Xiaomi tablet holds 3 stored pages: … »; null when absent. */
export function pagesInMessage(message: string): number | null {
  const found = /holds (\d+) stored pages?/.exec(message);
  return found ? Number(found[1]) : null;
}
