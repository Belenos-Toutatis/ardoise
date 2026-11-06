// @vitest-environment jsdom
/// <reference types="node" />
/**
 * A revision, a retouched copy of a note made by an assistant (SPEC §4.9): the editor says so under
 * the top bar and offers « Ouvrir l'original », « Ce qui a changé » and « Comparer ». The source
 * opens in the same editor by the rules of the page buttons (`Editor.pages.test.ts`).
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { addMockRevision, freshStamp, mockLibrary, notifyLibraryChanged, resetMockLibrary } from '../../lib/mock-data';
import { resetMockNarration } from '../../lib/mock-narration';
import { resetMockShare } from '../../lib/mock-share';
import { share } from '../../lib/share-state.svelte';
import EditorPagesHarness from './EditorPagesHarness.svelte';

vi.mock('../VideoExportDialog.svelte', async () => ({
  default: (await import('./VideoExportDialogDouble.svelte')).default,
}));

vi.mock('../../lib/ink', async (importOriginal) => {
  const actual = await importOriginal<typeof import('../../lib/ink')>();
  const { readFileSync } = await import('node:fs');
  const { resolve } = await import('node:path');
  const ink = actual.instantiateInk(readFileSync(resolve(process.cwd(), 'ui/src/assets/ardoise_ink.wasm')));
  return { ...actual, loadInk: () => ink };
});

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

const PHYSICS = 'Cours de physique';
const PHYSICS_ID = 'b1f0c6c2-0000-4000-8000-000000000001';
const SHOPPING = 'Liste de courses';
const COPY = 'Cours de physique (propre)';
const KEEP = "Enregistrer et ouvrir l'autre note";
const LET_GO = 'Ouvrir sans enregistrer';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockNarration();
  resetMockShare();
  share.reset();
  setLocale('fr');
  try {
    localStorage.clear();
  } catch {
    // No storage: nothing to clear.
  }
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  share.reset();
  resetMockShare();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function demoPath(title: string): string {
  const path = [...mockLibrary().notes.keys()].find((p) => p.includes(title));
  if (!path) throw new Error(`no note "${title}"`);
  return path;
}

/** The editor on the note at `path`, held like the library screen holds it. */
async function render(path: string, order: string[] | null = null) {
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const onsaved = vi.fn();
  const onpage = vi.fn();
  const component = mount(EditorPagesHarness, {
    target,
    props: { initial: path, pages: order ?? (undefined as never), app, onclose, onsaved, onpage },
  });
  const harness = component as unknown as { held(): string };
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const named = (text: string) => [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
  const action = (name: string) => target.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`);
  const titleField = () => target.querySelector<HTMLInputElement>('.title-field input')!;
  const bar = () => target.querySelector('[data-slot="revision-bar"]');
  const line = () => target.querySelector('[data-slot="revision-line"]')?.textContent?.trim() ?? null;
  const dialog = () => document.querySelector<HTMLElement>('[role="dialog"]');
  const alert = () => document.querySelector('[role="alertdialog"]');
  const editorPath = () => target.querySelector('.editor')?.getAttribute('data-path');
  const dirty = () => target.querySelector('.editor')?.getAttribute('data-dirty');
  const key = async (name: string, init: KeyboardEventInit = {}) => {
    window.dispatchEvent(new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init }));
    await tick();
  };
  const modify = async () => {
    named('Couleur')!.click();
    await tick();
    expect(dirty()).toBe('yes');
  };
  await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
  await tick();
  return {
    path,
    target,
    onclose,
    onsaved,
    onpage,
    named,
    action,
    titleField,
    bar,
    line,
    dialog,
    alert,
    editorPath,
    dirty,
    key,
    modify,
    held: () => harness.held(),
    shown: async (title: string) => {
      await vi.waitFor(() => expect(titleField().value).toBe(title));
      await vi.waitFor(() => expect(target.querySelector('svg.canvas')).toBeTruthy());
      expect(editorPath()).toBe(demoPath(title));
    },
  };
}

describe('the line of a revision', () => {
  it('is not there for an ordinary note', async () => {
    const view = await render(demoPath(PHYSICS));
    expect(view.bar()).toBeNull();
    expect(view.action('revision-open')).toBeNull();
  });

  it.each([
    ['fr', 'Copie retouchée de « Cours de physique »', ["Ouvrir l'original", 'Ce qui a changé', 'Comparer']],
    ['en', 'Retouched copy of “Cours de physique”', ['Open the original', 'What changed', 'Compare']],
  ] as const)('is under the top bar with its three buttons in %s', async (locale, line, labels) => {
    setLocale(locale);
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    expect(view.line()).toBe(line);
    expect(view.bar()?.getAttribute('aria-label')).toBe(locale === 'fr' ? 'Copie retouchée' : 'Retouched copy');
    const buttons = [...view.bar()!.querySelectorAll('button')].map((b) => b.textContent?.trim());
    expect(buttons).toEqual(labels);
    expect(view.action('revision-open')?.disabled).toBe(false);
    expect(view.action('revision-compare')?.disabled).toBe(false);
    // Under the top bar, before the page, not among the tools.
    const section = view.target.querySelector('.editor')!;
    const order = [...section.children];
    expect(order.indexOf(view.bar()!)).toBe(order.indexOf(section.querySelector('.editor-top')!) + 1);
    expect(view.bar()?.querySelector('[data-slot="revision-gone"]')).toBeNull();
  });

  it("names the source by the title it has now, not the one it had when the copy was made", async () => {
    const copy = addMockRevision();
    const source = mockLibrary().notes.get(demoPath(PHYSICS))!;
    source.note = { ...source.note, title: 'Optique' };
    const view = await render(copy);
    await vi.waitFor(() => expect(view.line()).toBe('Copie retouchée de « Optique »'));
  });

  it('names an untitled source « Note sans titre », not an empty pair of quotes', async () => {
    const copy = addMockRevision();
    mockLibrary().notes.get(demoPath(PHYSICS))!.note.title = '  ';
    const view = await render(copy);
    await vi.waitFor(() => expect(view.line()).toBe('Copie retouchée de « Note sans titre »'));
    // The source is gone, and the title of the time was empty too: same answer.
    cleanup?.();
    const lost = addMockRevision({ title: 'Copie' });
    mockLibrary().notes.get(lost)!.revision!.ofTitle = '';
    mockLibrary().notes.delete(demoPath(PHYSICS));
    const again = await render(lost);
    await vi.waitFor(() => expect(again.line()).toBe('Copie retouchée de « Note sans titre »'));
  });

  it('shows a title written by an assistant as text, markup and all', async () => {
    const copy = addMockRevision();
    mockLibrary().notes.get(demoPath(PHYSICS))!.note.title = '<b>gras</b> & <img src=x onerror=alert(1)>';
    const view = await render(copy);
    await vi.waitFor(() => expect(view.line()).toContain('<b>gras</b>'));
    expect(view.bar()?.querySelector('b, img')).toBeNull();
  });

  it('goes away after « Enregistrer comme copie »: the new note is not a retouched copy', async () => {
    const path = addMockRevision();
    const view = await render(path);
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    await view.modify();
    // The file changed under the editor: saving is a conflict, and the copy is offered.
    mockLibrary().notes.get(path)!.stamp = freshStamp();
    await view.key('s', { ctrlKey: true });
    await vi.waitFor(() => expect(view.target.querySelector('[role="alert"]')).toBeTruthy());
    view.named('Enregistrer comme copie')!.click();
    await vi.waitFor(() => expect(view.onsaved).toHaveBeenCalled());
    const copyPath = view.onsaved.mock.calls[0][0] as string;
    expect(copyPath).not.toBe(path);
    await vi.waitFor(() => expect(view.editorPath()).toBe(copyPath));
    expect(view.bar()).toBeNull();
    expect(view.target.querySelector('[data-action="revision-open"]')).toBeNull();
    // Nothing of the account went to the file of the copy; the revision it came from kept its own.
    expect(mockLibrary().notes.get(copyPath)?.revision).toBeUndefined();
    expect(mockLibrary().notes.get(path)?.revision?.of).toBe(PHYSICS_ID);
  });

  it('goes away with the revision: a note opened after it has no line', async () => {
    const copy = addMockRevision();
    const view = await render(copy, [copy, demoPath(SHOPPING)]);
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    view.target.querySelector<HTMLButtonElement>('button[data-action="page-next"]')!.click();
    await view.shown(SHOPPING);
    expect(view.bar()).toBeNull();
  });
});

describe('« Ouvrir l\'original »', () => {
  it('opens the source in the same editor, and the screen holding it follows', async () => {
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    const section = view.target.querySelector('.editor');
    view.action('revision-open')!.click();
    await view.shown(PHYSICS);
    expect(view.onpage).toHaveBeenCalledExactlyOnceWith(demoPath(PHYSICS));
    expect(view.held()).toBe(demoPath(PHYSICS));
    expect(view.target.querySelector('.editor')).toBe(section);
    expect(view.onclose).not.toHaveBeenCalled();
    // The original is an ordinary note.
    expect(view.bar()).toBeNull();
  });

  it('finds the source again by its id when it was renamed or moved since the note was opened', async () => {
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    const library = mockLibrary();
    const oldPath = demoPath(PHYSICS);
    const entry = library.notes.get(oldPath)!;
    library.notes.delete(oldPath);
    entry.note = { ...entry.note, title: 'Optique' };
    const moved = 'C:\\demo\\Ardoise\\Archives\\2026-10-20 Optique.ardoise';
    library.notes.set(moved, { ...entry, binder: 'Archives' });
    view.action('revision-open')!.click();
    await vi.waitFor(() => expect(view.editorPath()).toBe(moved));
    expect(view.onpage).toHaveBeenCalledExactlyOnceWith(moved);
  });

  it('keeps the page position when the source is in the list, and drops it when it is not', async () => {
    const copy = addMockRevision();
    const position = (view: Awaited<ReturnType<typeof render>>) =>
      view.target.querySelector('[data-slot="page-position"]')?.textContent?.trim() ?? null;
    const inList = await render(copy, [copy, demoPath(PHYSICS), demoPath(SHOPPING)]);
    await vi.waitFor(() => expect(inList.bar()).toBeTruthy());
    expect(position(inList)).toBe('1 / 3');
    inList.action('revision-open')!.click();
    await inList.shown(PHYSICS);
    expect(position(inList)).toBe('2 / 3');
    cleanup?.();

    // The library's list (a search, a binder) does not hold the source: no position, no error.
    const outside = await render(copy, [copy, demoPath(SHOPPING)]);
    await vi.waitFor(() => expect(outside.bar()).toBeTruthy());
    expect(position(outside)).toBe('1 / 2');
    outside.action('revision-open')!.click();
    await outside.shown(PHYSICS);
    expect(position(outside)).toBeNull();
    expect(outside.target.querySelector('button[data-action="page-next"]')).toBeNull();
    expect(outside.target.querySelector('.editor-notice')).toBeNull();
    expect(outside.onclose).not.toHaveBeenCalled();
  });

  it('is greyed with the reason written beside it when the original is not in the library', async () => {
    const copy = addMockRevision();
    mockLibrary().notes.delete(demoPath(PHYSICS));
    const view = await render(copy);
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    const open = view.action('revision-open')!;
    expect(open.disabled).toBe(true);
    const reason = view.bar()!.querySelector('[data-slot="revision-gone"]')!;
    expect(reason.textContent?.trim()).toBe("L'original n'est pas dans la bibliothèque");
    expect(open.getAttribute('aria-describedby')).toBe(reason.id);
    // The line still says whose copy it is, with the title of that time.
    expect(view.line()).toBe('Copie retouchée de « Cours de physique »');
    open.click();
    await tick();
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.editorPath()).toBe(copy);
    // Comparing needs the original too; the account does not.
    expect(view.action('revision-compare')?.disabled).toBe(true);
    expect(view.action('revision-changes')?.disabled).toBe(false);
  });

  it('follows the library: greyed when the source is trashed meanwhile, available again when it is back', async () => {
    const copy = addMockRevision();
    const view = await render(copy);
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    const library = mockLibrary();
    const path = demoPath(PHYSICS);
    const entry = library.notes.get(path)!;
    library.notes.delete(path);
    notifyLibraryChanged();
    await vi.waitFor(() => expect(view.action('revision-open')?.disabled).toBe(true));
    expect(view.bar()?.querySelector('[data-slot="revision-gone"]')).toBeTruthy();
    library.notes.set(path, entry);
    notifyLibraryChanged();
    await vi.waitFor(() => expect(view.action('revision-open')?.disabled).toBe(false));
    expect(view.bar()?.querySelector('[data-slot="revision-gone"]')).toBeNull();
  });

  it('says so, and stays, when the source went between the last look and the click', async () => {
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    vi.spyOn(api, 'revisionSource').mockResolvedValue(null);
    view.action('revision-open')!.click();
    await vi.waitFor(() => expect(view.target.querySelector('.editor-notice')).toBeTruthy());
    expect(view.target.querySelector('.editor-notice')?.textContent).toContain("L'original n'est pas dans la bibliothèque");
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.action('revision-open')?.disabled).toBe(true);
  });

  it('asks first when there are unsaved changes, and « Enregistrer et ouvrir l\'autre note » saves, then opens', async () => {
    const copy = addMockRevision();
    const save = vi.spyOn(api, 'saveNote');
    const view = await render(copy);
    await view.modify();
    view.action('revision-open')!.click();
    await vi.waitFor(() => expect(view.alert()).toBeTruthy());
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.named(KEEP)).toBeTruthy();
    expect(view.named(LET_GO)).toBeTruthy();

    view.named(KEEP)!.click();
    await view.shown(PHYSICS);
    expect(save).toHaveBeenCalledOnce();
    expect(save.mock.calls[0][0]).toBe(copy);
    expect(view.held()).toBe(demoPath(PHYSICS));
    // The revision kept its account through the save.
    expect(mockLibrary().notes.get(copy)?.revision?.of).toBe(PHYSICS_ID);
    expect(view.alert()).toBeNull();
  });

  it('« Ouvrir sans enregistrer » lets the changes go; « Rester » keeps everything as it was', async () => {
    const copy = addMockRevision();
    const save = vi.spyOn(api, 'saveNote');
    const view = await render(copy);
    await view.modify();
    view.action('revision-open')!.click();
    await vi.waitFor(() => expect(view.alert()).toBeTruthy());
    view.named('Rester')!.click();
    await tick();
    expect(view.alert()).toBeNull();
    expect(view.editorPath()).toBe(copy);
    expect(view.dirty()).toBe('yes');
    expect(view.held()).toBe(copy);
    expect(view.bar()).toBeTruthy();

    view.action('revision-open')!.click();
    await vi.waitFor(() => expect(view.alert()).toBeTruthy());
    view.named(LET_GO)!.click();
    await view.shown(PHYSICS);
    expect(save).not.toHaveBeenCalled();
  });

  it('stays on the copy, with the error in view, when it cannot be saved', async () => {
    const copy = addMockRevision();
    vi.spyOn(api, 'saveNote').mockRejectedValue({ code: 'libraryAccess', message: 'disk full' });
    const view = await render(copy);
    await view.modify();
    view.action('revision-open')!.click();
    await vi.waitFor(() => expect(view.alert()).toBeTruthy());
    view.named(KEEP)!.click();
    await vi.waitFor(() => expect(view.target.querySelector('.editor-banner[role="alert"]')).toBeTruthy());
    expect(view.onpage).not.toHaveBeenCalled();
    expect(view.editorPath()).toBe(copy);
    expect(view.dirty()).toBe('yes');
    expect(view.bar()).toBeTruthy();
  });

  it('is blocked while a video is being exported, and says why', async () => {
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    view.target.querySelector<HTMLButtonElement>('button[data-action="video-export"]')!.click();
    await tick();
    view.named('run-video')!.click();
    await tick();
    view.action('revision-open')!.click();
    await vi.waitFor(() => expect(view.target.querySelector('.editor-notice')).toBeTruthy());
    expect(view.target.querySelector('.editor-notice')?.textContent).toContain("avant d'ouvrir l'original");
    expect(view.onpage).not.toHaveBeenCalled();
  });
});

describe('« Ce qui a changé »', () => {
  it('shows the account, each entry dated, and closes with Escape and gives the focus back', async () => {
    const copy = addMockRevision({ summary: ['Lignes redressées.', 'Un « g » refait avec le rond du « q ».'] });
    const view = await render(copy);
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    const button = view.action('revision-changes')!;
    button.focus();
    button.click();
    await tick();
    const dialog = view.dialog()!;
    expect(dialog.getAttribute('aria-labelledby')).toBeTruthy();
    expect(dialog.querySelector('h2')?.textContent).toBe('Ce qui a changé');
    expect(dialog.textContent).toContain('Copie faite le');
    expect(dialog.textContent).toContain('« Cours de physique »');
    const items = [...dialog.querySelectorAll('[data-slot="revision-account"] li')];
    expect(items.map((li) => li.querySelector('.revision-text')?.textContent)).toEqual([
      'Lignes redressées.',
      'Un « g » refait avec le rond du « q ».',
    ]);
    for (const li of items) {
      const time = li.querySelector('time')!;
      expect(time.textContent?.trim()).toMatch(/2026/);
      expect(time.getAttribute('datetime')).toMatch(/^2026-11-06T/);
    }
    // Focus is inside the dialog, and Escape closes it.
    expect(dialog.contains(document.activeElement)).toBe(true);
    await view.key('Escape');
    expect(view.dialog()).toBeNull();
    expect(document.activeElement).toBe(button);
  });

  it('closes with its button too, and says when there is no account', async () => {
    const copy = addMockRevision({ summary: [] });
    const view = await render(copy);
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    view.action('revision-changes')!.click();
    await tick();
    expect(view.dialog()?.textContent).toContain("Aucun compte rendu n'accompagne cette copie.");
    view.dialog()!.querySelector<HTMLButtonElement>('button[data-action="revision-close"]')!.click();
    await tick();
    expect(view.dialog()).toBeNull();
  });

  it('writes the account in English when the interface is in English', async () => {
    setLocale('en');
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    view.action('revision-changes')!.click();
    await tick();
    expect(view.dialog()?.querySelector('h2')?.textContent).toBe('What changed');
    expect(view.dialog()?.textContent).toContain('Copy made on');
    expect(view.dialog()?.textContent).toContain('Close');
  });

  it('is plain text: what an assistant wrote is never markup', async () => {
    const hostile = '<img src=x onerror="window.__pwned = 1"> <b>gras</b> <script>window.__pwned = 2<\/script>\nligne deux &amp; [lien](http://exemple.test)';
    const copy = addMockRevision({ summary: [hostile] });
    const view = await render(copy);
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    view.action('revision-changes')!.click();
    await tick();
    const dialog = view.dialog()!;
    const text = dialog.querySelector('.revision-text')!;
    expect(text.textContent).toBe(hostile);
    expect(text.children).toHaveLength(0);
    expect(dialog.querySelector('img, b, script, a')).toBeNull();
    expect((window as unknown as { __pwned?: number }).__pwned).toBeUndefined();
  });

  it('holds the editing keys back while it is open', async () => {
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    view.action('revision-changes')!.click();
    await tick();
    await view.key('ArrowRight', { altKey: true });
    await view.key('a', { ctrlKey: true });
    expect(view.target.querySelector('[aria-labelledby="editor-selection-heading"]')).toBeNull();
    expect(view.onpage).not.toHaveBeenCalled();
  });
});

describe('« Comparer »', () => {
  /** Resolves the picture of a path to a recognisable data URL, recording the boxes asked for. */
  function pictures() {
    const asked: { path: string; w: number; h: number }[] = [];
    vi.spyOn(api, 'notePicture').mockImplementation(async (path, w, h) => {
      asked.push({ path, w, h });
      return `data:image/png;base64,${path.includes('propre') ? 'Q09QWQ==' : 'T1JJRw=='}`;
    });
    return asked;
  }

  it('draws the original and the copy, each by Rust at about the size of the dialog', async () => {
    const asked = pictures();
    const copy = addMockRevision();
    const view = await render(copy);
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    const button = view.action('revision-compare')!;
    button.focus();
    button.click();
    await tick();
    const dialog = view.dialog()!;
    expect(dialog.querySelector('h2')?.textContent).toBe("Comparer avec l'original");
    await vi.waitFor(() => expect(dialog.querySelectorAll('img')).toHaveLength(2));
    const [original, mine] = [...dialog.querySelectorAll('img')];
    expect(original.alt).toBe("La page de l'original");
    expect(mine.alt).toBe('La page de cette copie');
    expect(original.getAttribute('src')).toContain('T1JJRw==');
    expect(mine.getAttribute('src')).toContain('Q09QWQ==');
    expect(dialog.querySelector('[data-slot="revision-compare-original"] figcaption')?.textContent).toBe('Original : « Cours de physique »');
    expect(dialog.querySelector('[data-slot="revision-compare-copy"] figcaption')?.textContent).toBe('Cette copie');
    // The original's file and the copy's, at a size that is a box (whole pixels, not a speck, not a page).
    expect(asked.map((a) => a.path).sort()).toEqual([copy, demoPath(PHYSICS)].sort());
    for (const { w, h } of asked) {
      expect(Number.isInteger(w) && Number.isInteger(h)).toBe(true);
      expect(w).toBeGreaterThan(100);
      expect(h).toBeGreaterThan(100);
      expect(w).toBeLessThanOrEqual(2400);
    }
    expect(dialog.querySelector('[data-slot="revision-compare-unsaved"]')).toBeNull();

    // Escape closes it and gives the focus back to the button.
    expect(dialog.contains(document.activeElement)).toBe(true);
    await view.key('Escape');
    expect(view.dialog()).toBeNull();
    expect(document.activeElement).toBe(button);
  });

  it('does not draw again for a resize that leaves the rounded box as it was, and does for a real one', async () => {
    const asked = pictures();
    const sizes = { w: window.innerWidth, h: window.innerHeight };
    const resize = async (w: number, h: number) => {
      Object.defineProperty(window, 'innerWidth', { value: w, configurable: true, writable: true });
      Object.defineProperty(window, 'innerHeight', { value: h, configurable: true, writable: true });
      window.dispatchEvent(new Event('resize'));
      await tick();
      // Longer than the delay before a new drawing starts.
      await new Promise((done) => setTimeout(done, 450));
    };
    try {
      await resize(1024, 768);
      const view = await render(addMockRevision());
      await vi.waitFor(() => expect(view.bar()).toBeTruthy());
      view.action('revision-compare')!.click();
      await tick();
      await vi.waitFor(() => expect(view.dialog()?.querySelectorAll('img')).toHaveLength(2));
      expect(asked).toHaveLength(2);

      // One pixel more each way: the boxes round to the same size, so nothing is asked again.
      await resize(1025, 769);
      expect(asked).toHaveLength(2);
      expect(view.dialog()?.querySelectorAll('img')).toHaveLength(2);

      // A clearly smaller window draws both pages again, once.
      await resize(700, 500);
      expect(asked).toHaveLength(4);
      expect(view.dialog()?.querySelectorAll('img')).toHaveLength(2);
    } finally {
      Object.defineProperty(window, 'innerWidth', { value: sizes.w, configurable: true, writable: true });
      Object.defineProperty(window, 'innerHeight', { value: sizes.h, configurable: true, writable: true });
    }
  });

  it('keeps the focus inside the dialog with Tab and Shift+Tab', async () => {
    pictures();
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    view.action('revision-compare')!.click();
    await tick();
    const dialog = view.dialog()!;
    const close = dialog.querySelector<HTMLButtonElement>('button[data-action="revision-close"]')!;
    close.focus();
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', bubbles: true, cancelable: true }));
    expect(dialog.contains(document.activeElement)).toBe(true);
    dialog.dispatchEvent(new KeyboardEvent('keydown', { key: 'Tab', shiftKey: true, bubbles: true, cancelable: true }));
    expect(dialog.contains(document.activeElement)).toBe(true);
  });

  it('puts the two pages one above the other in a narrow window, side by side in a wide one', async () => {
    pictures();
    const stage = async (width: number) => {
      Object.defineProperty(window, 'innerWidth', { value: width, configurable: true, writable: true });
      const view = await render(addMockRevision());
      await vi.waitFor(() => expect(view.bar()).toBeTruthy());
      view.action('revision-compare')!.click();
      await tick();
      const stacked = view.dialog()!.querySelector('[data-slot="revision-compare-stage"]')!.classList.contains('stacked');
      cleanup?.();
      cleanup = null;
      return stacked;
    };
    const original = window.innerWidth;
    try {
      expect(await stage(480)).toBe(true);
      expect(await stage(1400)).toBe(false);
    } finally {
      Object.defineProperty(window, 'innerWidth', { value: original, configurable: true, writable: true });
    }
  });

  it('says that the copy is shown as saved when it has unsaved changes', async () => {
    pictures();
    const view = await render(addMockRevision());
    await view.modify();
    view.action('revision-compare')!.click();
    await tick();
    expect(view.dialog()?.querySelector('[data-slot="revision-compare-unsaved"]')?.textContent).toContain('telle qu\'elle est enregistrée');
  });

  it('shows the error of a page that cannot be drawn, and the other one anyway', async () => {
    vi.spyOn(api, 'notePicture').mockImplementation(async (path) => {
      if (path.includes('propre')) throw { code: 'render', message: 'out of memory' };
      return 'data:image/png;base64,T1JJRw==';
    });
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    view.action('revision-compare')!.click();
    await tick();
    await vi.waitFor(() => expect(view.dialog()?.querySelector('[role="alert"]')).toBeTruthy());
    expect(view.dialog()?.querySelector('[role="alert"]')?.textContent).toContain('out of memory');
    expect(view.dialog()?.querySelectorAll('img')).toHaveLength(1);
  });

  it('closes by itself when the source goes meanwhile, and the editing keys work again', async () => {
    pictures();
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    view.action('revision-compare')!.click();
    await tick();
    expect(view.dialog()).toBeTruthy();
    mockLibrary().notes.delete(demoPath(PHYSICS));
    notifyLibraryChanged();
    await vi.waitFor(() => expect(view.dialog()).toBeNull());
    await view.key('a', { ctrlKey: true });
    expect(view.target.querySelector('[aria-labelledby="editor-selection-heading"]')).toBeTruthy();
  });

  it('works with the mock renderer of the browser preview too', async () => {
    const view = await render(addMockRevision());
    await vi.waitFor(() => expect(view.bar()).toBeTruthy());
    view.action('revision-compare')!.click();
    await tick();
    await vi.waitFor(() => expect(view.dialog()?.querySelectorAll('img')).toHaveLength(2));
    for (const img of view.dialog()!.querySelectorAll('img')) {
      expect(img.getAttribute('src')).toMatch(/^data:image\/svg\+xml/);
    }
  });
});
