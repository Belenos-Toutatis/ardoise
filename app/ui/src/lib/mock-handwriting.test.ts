// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from 'vitest';
import { api } from './api';
import { mockLibrary, resetMockLibrary } from './mock-data';
import { mockClipboard } from './mock-export';
import {
  ENGLISH_RECOGNIZER,
  mockHandwriting,
  mockOnHandwritingStatus,
  resetMockHandwriting,
  searchText,
  textDocument,
} from './mock-handwriting';
import type { IndexStatus } from './handwriting';

beforeEach(() => {
  resetMockLibrary();
  resetMockHandwriting();
  mockClipboard.available = true;
});

function pathOf(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no demo note "${title}"`);
  return path;
}

describe('the browser stand-in of the handwriting', () => {
  it('finds words where recognised words begin, with an excerpt', () => {
    const found = searchText('la lumière se propage\nen ligne droite dans un milieu homogène', ['lumiere', 'homo']);
    expect(found?.words).toEqual(['lumiere', 'homo']);
    expect(found?.excerpt).toEqual({ before: 'la ', word: 'lumière', after: ' se propage' });
    expect(searchText('un deux trois quatre cinq six sept', ['six'])?.excerpt).toEqual({
      before: '… deux trois quatre cinq ',
      word: 'six',
      after: ' sept',
    });
    expect(searchText('allumer', ['lum'])).toBeNull();
    expect(searchText('', ['x'])).toBeNull();
  });

  it('searches the demo library and reports its language', async () => {
    const hits = await api.handwritingSearch('Lumière');
    expect(hits.map((h) => h.path)).toEqual([pathOf('Cours de physique')]);
    expect(await api.handwritingSearch('  ')).toEqual([]);
    const info = await api.handwritingInfo();
    expect(info.language).toBe('fr');
    expect(info.status.state).toBe('idle');
    // English chosen, not installed: nothing recognised, and said so.
    const english = await api.setHandwritingLanguage('en');
    expect(english.language).toBeNull();
    expect(english.status).toMatchObject({ state: 'unavailable', reason: 'noRecognizer' });
    expect(await api.handwritingSearch('lumiere')).toEqual([]);
    await expect(api.handwritingText([{ path: pathOf('Cours de physique') }])).rejects.toMatchObject({
      code: 'handwritingNoRecognizer',
    });
    mockHandwriting.recognizers = [ENGLISH_RECOGNIZER];
    expect((await api.handwritingInfo()).language).toBe('en');
  });

  it('stops and resumes, telling the listeners', async () => {
    const seen: IndexStatus[] = [];
    const stop = mockOnHandwritingStatus((s) => seen.push(s));
    expect((await api.handwritingStop()).state).toBe('stopped');
    await Promise.resolve();
    expect(seen.at(-1)?.state).toBe('stopped');
    expect(await api.handwritingSearch('lumiere')).toEqual([]);
    await api.handwritingResume();
    await Promise.resolve();
    expect(seen.at(-1)?.state).toBe('idle');
    stop();
  });

  it('exports and copies the text, one note after the other with its title', async () => {
    const sources = [{ path: pathOf('Cours de physique') }, { path: pathOf('Réunion parents') }];
    const texts = await api.handwritingText(sources);
    expect(texts.map((t) => t.title)).toEqual(['Cours de physique', 'Réunion parents']);
    expect(await api.exportHandwritingText(sources, 'C:\\x\\texte.txt')).toBe('C:\\x\\texte.txt');
    const document = mockHandwriting.exported?.document ?? '';
    expect(document.startsWith('Cours de physique\r\n=================\r\n\r\nla lumière se propage\r\n')).toBe(true);
    expect(document).toContain('\r\n\r\nRéunion parents\r\n===============\r\n\r\nrendez-vous jeudi');
    await api.copyHandwritingText(sources.slice(0, 1));
    expect(mockHandwriting.copied).toBe(textDocument(texts.slice(0, 1)));
    mockClipboard.available = false;
    await expect(api.copyHandwritingText(sources)).rejects.toMatchObject({ code: 'clipboardUnavailable' });
    // A note in memory (the editor) is read as it is.
    const note = { ...mockLibrary().notes.get(sources[1].path)!.note, title: '' };
    const [unsaved] = await api.handwritingText([{ path: sources[1].path, note }]);
    expect(unsaved.title).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });
});
