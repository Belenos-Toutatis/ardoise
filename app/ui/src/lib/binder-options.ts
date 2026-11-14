/**
 * The choices of the binder dialogs that are remembered on this computer (like the other choices of the export
 * dialogs): what the summary holds (« Dans le sommaire »: the same three choices for the summary dialog, the outline of the
 * binder's PDF and the summary page at its start), and whether the PDF starts with that page.
 */
import type { SummaryChoices, SummaryNotes } from './library-logic';

const KEY = 'ardoise.binderSummary';
/** The choice of the first version, a single box: « Afficher les dates ». */
const OLD_DATES_KEY = 'ardoise.binderDates';

const NOTES: readonly SummaryNotes[] = ['title', 'titleOrDate', 'none'];

/**
 * What the summary holds when the user has not chosen yet: the names of the sub-binders and the bookmarks, no line per
 * note (a book's contents, not a list of pages). A binder that has no bookmark at all would then have an empty summary:
 * for it the notes are listed until the user chooses (`effectiveChoices`).
 */
export const DEFAULT_SUMMARY: SummaryChoices = { binders: true, notes: 'none', bookmarks: true };

/**
 * The remembered choices; null when the user never chose (the defaults then depend on the binder, `effectiveChoices`).
 * The single box of the first version is read as what it meant: « dates » is the note lines named by their title or
 * their date, with everything else; « no dates » is no choice.
 */
export function loadSummaryChoices(): SummaryChoices | null {
  try {
    const stored = localStorage.getItem(KEY);
    if (stored !== null) {
      const value = JSON.parse(stored) as Partial<SummaryChoices> | null;
      if (value && typeof value.binders === 'boolean' && typeof value.bookmarks === 'boolean' && NOTES.includes(value.notes as SummaryNotes)) {
        return { binders: value.binders, notes: value.notes as SummaryNotes, bookmarks: value.bookmarks };
      }
      return null;
    }
    if (localStorage.getItem(OLD_DATES_KEY) === 'true') return { binders: true, notes: 'titleOrDate', bookmarks: true };
  } catch {
    // Storage blocked or damaged: no choice.
  }
  return null;
}

export function saveSummaryChoices(choices: SummaryChoices): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(choices));
  } catch {
    // Nothing to remember it with: the next dialog starts from the defaults.
  }
}

/**
 * What the dialogs use: the user's choices, else the defaults, with the notes listed (by title) when the binder has no
 * bookmark, so that a first summary is never empty.
 */
export function effectiveChoices(stored: SummaryChoices | null, hasBookmarks: boolean): SummaryChoices {
  if (stored) return stored;
  return hasBookmarks ? { ...DEFAULT_SUMMARY } : { ...DEFAULT_SUMMARY, notes: 'title' };
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
