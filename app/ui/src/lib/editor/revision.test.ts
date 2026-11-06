import { describe, expect, it } from 'vitest';
import type { NoteSummary, RevisionOfNote } from '../note';
import { sameSource, sourceIn, withSourceIn } from './revision';

const stamp = { modified: { secs_since_epoch: 1, nanos_since_epoch: 0 }, len: 1 };

function summary(id: string, title: string, path: string, conflict = false): NoteSummary {
  return {
    path,
    binder: '',
    fileName: path.split('\\').pop()!,
    id,
    title,
    created: 0,
    modified: 0,
    tags: [],
    favorite: false,
    stamp,
    conflict: conflict ? { originalName: 'x.ardoise' } : null,
  };
}

const revision: RevisionOfNote = {
  of: 'source',
  ofTitle: 'Brouillon',
  created: 1,
  summary: [],
  source: { path: 'C:\\a\\Brouillon.ardoise', title: 'Brouillon' },
};

describe('the source of a revision in a listing', () => {
  it('is the note with the id of the source, by the title it has now', () => {
    const notes = [summary('other', 'Autre', 'C:\\a\\Autre.ardoise'), summary('source', 'Optique', 'C:\\a\\Optique.ardoise')];
    expect(sourceIn(notes, 'source')).toEqual({ path: 'C:\\a\\Optique.ardoise', title: 'Optique' });
    expect(sourceIn(notes, 'missing')).toBeNull();
    expect(sourceIn([], 'source')).toBeNull();
  });

  it("prefers the file itself to a sync client's conflict copy, which has the same id", () => {
    const notes = [
      summary('source', 'Brouillon', 'C:\\a\\Brouillon (conflicted copy).ardoise', true),
      summary('source', 'Brouillon', 'C:\\a\\Brouillon.ardoise'),
    ];
    expect(sourceIn(notes, 'source')?.path).toBe('C:\\a\\Brouillon.ardoise');
    // Only the copy left: better than nothing.
    expect(sourceIn(notes.slice(0, 1), 'source')?.path).toBe('C:\\a\\Brouillon (conflicted copy).ardoise');
  });

  it('keeps the very same revision while nothing moved, so that nothing is redrawn', () => {
    const same = [summary('source', 'Brouillon', 'C:\\a\\Brouillon.ardoise')];
    expect(withSourceIn(revision, same)).toBe(revision);
    const renamed = withSourceIn(revision, [summary('source', 'Optique', 'C:\\a\\Optique.ardoise')]);
    expect(renamed).not.toBe(revision);
    expect(renamed.source).toEqual({ path: 'C:\\a\\Optique.ardoise', title: 'Optique' });
    expect(renamed.ofTitle).toBe('Brouillon');
    expect(withSourceIn(revision, []).source).toBeNull();
    expect(sameSource(null, null)).toBe(true);
    expect(sameSource(null, revision.source)).toBe(false);
  });
});
