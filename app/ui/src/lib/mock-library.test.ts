// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { api, onLibraryChanged } from './api';
import { mockLibrary, resetMockLibrary } from './mock-data';

beforeEach(() => {
  resetMockLibrary();
});

describe('browser stand-in for the library commands', () => {
  it('lists notes with binders and tag counts', async () => {
    const listing = await api.libraryList();
    expect(listing.notes.length).toBe(3);
    expect(listing.binders).toEqual(['Classe', 'Classe/3e B', 'Perso']);
    expect(listing.tags).toEqual([
      { tag: '3e B', count: 1 },
      { tag: 'physique', count: 1 },
      { tag: 'réunion', count: 1 },
    ]);
    const physics = listing.notes.find((n) => n.title === 'Cours de physique')!;
    expect(physics.binder).toBe('Classe/3e B');
    expect(physics.fileName).toBe('2026-10-20 Cours de physique.ardoise');
    expect(physics.conflict).toBeNull();
    expect(await api.noteThumbnail(physics.path, physics.stamp)).toMatch(/^data:image\/svg\+xml/);
    expect(await api.noteThumbnail('C:\\nowhere.ardoise', physics.stamp)).toBeNull();
  });

  it('rejects a change made with a stale stamp, for tags, favourites and titles', async () => {
    const [note] = (await api.libraryList()).notes;
    const fresh = await api.setNoteFavorite(note.path, true, note.stamp);
    expect(fresh).not.toEqual(note.stamp);
    for (const attempt of [
      () => api.setNoteFavorite(note.path, false, note.stamp),
      () => api.setNoteTags(note.path, ['x'], note.stamp),
      () => api.retitleNote(note.path, 'Autre', note.stamp),
    ]) {
      await expect(attempt()).rejects.toMatchObject({ code: 'noteConflict' });
    }
    await expect(api.setNoteTags(note.path, ['x'], fresh)).resolves.toBeTruthy();
  });

  it('retitling renames the file and keeps names unique', async () => {
    const { notes } = await api.libraryList();
    const a = notes.find((n) => n.title === 'Liste de courses')!;
    const renamed = await api.retitleNote(a.path, 'Courses', a.stamp);
    expect(renamed.path.endsWith('2026-10-22 Courses.ardoise')).toBe(true);
    expect(mockLibrary().notes.has(a.path)).toBe(false);
    await expect(api.retitleNote(renamed.path, 'a/b', renamed.stamp)).rejects.toMatchObject({ code: 'invalidName' });
  });

  it('creates binders and notes, and moves notes', async () => {
    await api.createBinder('Archives/2026');
    expect((await api.libraryList()).binders).toContain('Archives');
    const created = await api.createNote('Archives/2026', 'Neuf');
    expect(created.binder).toBe('Archives/2026');
    const [moved] = await api.moveNotes([created.path], 'Perso');
    expect(moved.startsWith('C:\\demo\\Ardoise\\Perso\\')).toBe(true);
    await expect(api.moveNotes([created.path], 'Perso')).rejects.toMatchObject({ code: 'noteNotFound' });
    await expect(api.createBinder('..')).rejects.toMatchObject({ code: 'invalidName' });
  });

  it('trashes, restores, purges and empties', async () => {
    const { notes } = await api.libraryList();
    await api.trashNotes([notes[0].path, notes[1].path]);
    const trash = await api.trashList();
    expect(trash.length).toBe(2);
    expect((await api.libraryList()).notes.length).toBe(1);

    const restored = await api.trashRestore(trash[0].id);
    expect(mockLibrary().notes.has(restored)).toBe(true);
    await expect(api.trashRestore(trash[0].id)).rejects.toMatchObject({ code: 'notInTrash' });
    await api.trashPurge(trash[1].id);
    expect(await api.trashEmpty()).toBe(0);
  });

  it('notifies listeners after a change', async () => {
    const changed = vi.fn();
    const stop = await onLibraryChanged(changed);
    await api.createBinder('Neuf');
    expect(changed).toHaveBeenCalledOnce();
    stop();
    await api.createBinder('Encore');
    expect(changed).toHaveBeenCalledOnce();
  });

  it('knows a demo tablet that can be forgotten, and gets it back with a new library', async () => {
    expect((await api.tabletInfo())?.name).toBe('Bamboo Slate');
    await api.forgetTablet();
    expect(await api.tabletInfo()).toBeNull();
    resetMockLibrary();
    expect(await api.tabletInfo()).not.toBeNull();
  });
});
