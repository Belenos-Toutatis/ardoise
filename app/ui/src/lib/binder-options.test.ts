// @vitest-environment jsdom
// What a binder's summary holds (« Dans le sommaire »), remembered on the computer: the defaults, the choice of the first
// version (« Afficher les dates ») read as what it meant, and the first summary never being empty.
import { beforeEach, describe, expect, it } from 'vitest';
import { DEFAULT_SUMMARY, effectiveChoices, loadSummaryChoices, saveSummaryChoices } from './binder-options';

beforeEach(() => {
  localStorage.clear();
});

describe('the choices of the summary', () => {
  it('start as nothing chosen, the defaults being the sub-binders and the bookmarks, no line per note', () => {
    expect(loadSummaryChoices()).toBeNull();
    expect(DEFAULT_SUMMARY).toEqual({ binders: true, notes: 'none', bookmarks: true });
    expect(effectiveChoices(null, true)).toEqual(DEFAULT_SUMMARY);
  });

  it('list the notes by title when nothing is chosen and the binder has no bookmark, so that a first summary is never empty', () => {
    expect(effectiveChoices(null, false)).toEqual({ binders: true, notes: 'title', bookmarks: true });
    // A choice of the user is never overridden: nothing may be what they want.
    const chosen = { binders: true, notes: 'none' as const, bookmarks: true };
    expect(effectiveChoices(chosen, false)).toEqual(chosen);
  });

  it('are remembered whole, and a damaged value is no choice', () => {
    saveSummaryChoices({ binders: false, notes: 'titleOrDate', bookmarks: false });
    expect(loadSummaryChoices()).toEqual({ binders: false, notes: 'titleOrDate', bookmarks: false });
    for (const bad of ['', 'x', '{}', '{"binders":true}', '{"binders":true,"notes":"all","bookmarks":true}', 'null']) {
      localStorage.setItem('ardoise.binderSummary', bad);
      expect(loadSummaryChoices(), bad).toBeNull();
    }
  });

  it('read the first version\'s box: « dates » is the note lines by title or date, « no dates » is no choice', () => {
    localStorage.setItem('ardoise.binderDates', 'true');
    expect(loadSummaryChoices()).toEqual({ binders: true, notes: 'titleOrDate', bookmarks: true });
    localStorage.setItem('ardoise.binderDates', 'false');
    expect(loadSummaryChoices()).toBeNull();
    // A choice of the new version wins over the old box.
    localStorage.setItem('ardoise.binderDates', 'true');
    saveSummaryChoices({ binders: true, notes: 'none', bookmarks: true });
    expect(loadSummaryChoices()).toEqual({ binders: true, notes: 'none', bookmarks: true });
  });
});
