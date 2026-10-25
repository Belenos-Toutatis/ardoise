// @vitest-environment jsdom
// The library search also finds notes by their handwriting (milestone M7), over the browser
// stand-in (`mock-handwriting.ts`: the demo note "Cours de physique" reads « la lumière se
// propage en ligne droite dans un milieu homogène »).
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { handwritingSearch } from '../../lib/library-state.svelte';
import { resetMockLibrary } from '../../lib/mock-data';
import { emitMockHandwritingStatus, mockHandwriting, resetMockHandwriting } from '../../lib/mock-handwriting';
import Home from '../Home.svelte';

const app: AppState = {
  version: '0.1.0',
  machine: 'PC-TEST',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: 'C:\\demo\\Ardoise',
  systemLocale: 'fr-FR',
  selfTest: false,
  openDiagnostic: false,
  autoExport: { format: 'off', folder: 'Exports' },
  microphone: null,
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockHandwriting();
  handwritingSearch.delayMs = 0;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  handwritingSearch.delayMs = 120;
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render(locale: 'fr' | 'en' = 'fr') {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Home, { target, props: { app, onchange: vi.fn() } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const all = (selector: string) => [...target.querySelectorAll<HTMLElement>(selector)];
  const titles = () => all('.note-card .card-title').map((el) => el.textContent?.trim());
  await vi.waitFor(() => expect(all('.note-card').length).toBe(3));
  const search = target.querySelector<HTMLInputElement>('input[type="search"]')!;
  const type = async (value: string) => {
    search.value = value;
    search.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
  };
  return { target, all, titles, type };
}

describe('library: search in the handwriting', () => {
  it('finds a note by a word of its handwriting, accents and case aside, and shows why', async () => {
    const ui = await render('fr');
    await ui.type('LUMIERE');
    await vi.waitFor(() => expect(ui.titles()).toEqual(['Cours de physique']));
    const [excerpt] = ui.all('.handwriting-excerpt');
    expect(excerpt.querySelector('.excerpt-title')?.textContent).toBe('Cours de physique');
    expect(excerpt.querySelector('mark')?.textContent).toBe('lumière');
    expect(excerpt.querySelector('.excerpt-text')?.textContent).toBe('la lumière se propage');
    expect(ui.target.querySelector('.handwriting-found')?.textContent).toBe("Trouvé dans l'écriture manuscrite : 1");
    // The excerpt opens the note.
    excerpt.click();
    await vi.waitFor(() => expect(ui.target.querySelector('.editor')).not.toBeNull());
  });

  it('combines words of the title and of the handwriting, and keeps the title search as it was', async () => {
    const ui = await render('fr');
    await ui.type('physique droite');
    await vi.waitFor(() => expect(ui.titles()).toEqual(['Cours de physique']));
    await ui.type('physique farine');
    await vi.waitFor(() => expect(ui.titles()).toEqual([]));
    // Inside a recognised word is not a match.
    await ui.type('umiere');
    await settle();
    expect(ui.titles()).toEqual([]);
    expect(ui.all('.handwriting-excerpt')).toEqual([]);
    await ui.type('reunion');
    expect(ui.titles()).toEqual(['Réunion parents']);
    expect(ui.all('.handwriting-excerpt')).toEqual([]);
    await ui.type('');
    expect(ui.titles().length).toBe(3);
    expect(ui.target.querySelector('.handwriting-results')).toBeNull();
  });

  it('says when the indexing is still running, and finds more notes as it goes on', async () => {
    mockHandwriting.status = { state: 'indexing', reason: null, language: 'fr', done: 1, total: 3, failed: 0 };
    const ui = await render('fr');
    await ui.type('jeudi');
    await vi.waitFor(() =>
      expect(ui.target.querySelector('.handwriting-notice')?.textContent).toContain('notes lues : 1 sur 3'),
    );
    expect(ui.titles()).toEqual(['Réunion parents']);
    // The indexing ends: the notice goes.
    mockHandwriting.status = null;
    emitMockHandwritingStatus();
    await settle();
    expect(ui.target.querySelector('.handwriting-notice')).toBeNull();
    expect(ui.titles()).toEqual(['Réunion parents']);
  });

  it('says when no recognizer is installed, in English too', async () => {
    mockHandwriting.recognizers = [];
    const ui = await render('en');
    await ui.type('lumiere');
    await vi.waitFor(() =>
      expect(ui.target.querySelector('.handwriting-notice')?.textContent).toBe(
        'The handwriting is not searched: no Windows handwriting recognizer is installed for this language (see the settings).',
      ),
    );
    expect(ui.titles()).toEqual([]);
    expect(ui.target.textContent).toContain('No note matches “lumiere”.');
  });
});
