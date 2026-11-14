/**
 * The choices of the binder dialogs that are remembered on this computer (like the other choices of the export
 * dialogs): the dates, and the summary page of the PDF.
 *
 * Whether the dates show in a binder's summary and in the bookmarks of its PDF: the one choice of both dialogs, remembered
 * on this computer (like the other choices of the export dialogs). Off at first: a summary like a book's has no time
 * stamps in it; an untitled note then takes the name « Note sans titre » instead of its date.
 */
const KEY = 'ardoise.binderDates';

/** The remembered choice (off when nothing valid is stored, or storage is blocked). */
export function loadShowDates(): boolean {
  try {
    return localStorage.getItem(KEY) === 'true';
  } catch {
    return false;
  }
}

export function saveShowDates(show: boolean): void {
  try {
    localStorage.setItem(KEY, show ? 'true' : 'false');
  } catch {
    // Nothing to remember it with: the next dialog starts with the dates off.
  }
}

const SUMMARY_PAGE_KEY = 'ardoise.binderSummaryPage';

/** Whether a binder's PDF starts with a summary page: off at first (an option, SPEC §4.6), then as the user left it. */
export function loadSummaryPage(): boolean {
  try {
    return localStorage.getItem(SUMMARY_PAGE_KEY) === 'true';
  } catch {
    return false;
  }
}

export function saveSummaryPage(add: boolean): void {
  try {
    localStorage.setItem(SUMMARY_PAGE_KEY, add ? 'true' : 'false');
  } catch {
    // Nothing to remember it with: the next dialog starts without the summary page.
  }
}

/** What the export and print dialogs of a binder take: its notes with or without the sub-binders and the dates. */
export interface BinderSources<S> {
  sourcesFor: (includeSub: boolean, withDates: boolean) => S[];
}

/** Where a binder's notes are, for the dialogs: the notes of its own, and those below it. */
export interface BinderContent {
  own: number;
  total: number;
  /** The sub-binders start included: the binder has no note of its own, only below it. */
  includeSubAtStart: boolean;
}

export function binderContent<S>(binder: BinderSources<S>): BinderContent {
  const own = binder.sourcesFor(false, false).length;
  const total = binder.sourcesFor(true, false).length;
  return { own, total, includeSubAtStart: own === 0 && total > 0 };
}
