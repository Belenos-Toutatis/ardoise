import { describe, expect, it } from 'vitest';
import { revisionSourceTitle } from './note';

describe('revisionSourceTitle', () => {
  const untitled = 'Note sans titre';

  it('is the title the source has now', () => {
    expect(revisionSourceTitle({ source: { path: 'a', title: 'Optique' }, ofTitle: 'Cours' }, untitled)).toBe('Optique');
  });

  it('is the title of the time while the source cannot be found', () => {
    expect(revisionSourceTitle({ source: null, ofTitle: 'Cours' }, untitled)).toBe('Cours');
  });

  it('is « untitled » for an empty or blank title, whichever it comes from', () => {
    expect(revisionSourceTitle({ source: { path: 'a', title: '' }, ofTitle: 'Cours' }, untitled)).toBe(untitled);
    expect(revisionSourceTitle({ source: { path: 'a', title: ' \t' }, ofTitle: 'Cours' }, untitled)).toBe(untitled);
    expect(revisionSourceTitle({ source: null, ofTitle: '' }, untitled)).toBe(untitled);
  });

  it('trims the title it shows', () => {
    expect(revisionSourceTitle({ source: null, ofTitle: ' Cours ' }, untitled)).toBe('Cours');
  });
});
