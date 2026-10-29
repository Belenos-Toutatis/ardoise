// @vitest-environment jsdom
/**
 * The library screen's list of drafts left by an earlier run (M10): open the note (the editor
 * then offers the draft), recover a draft whose note is gone into a new note, discard one.
 */
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { LibraryState } from '../../lib/library-state.svelte';
import { resetMockDrafts, startNewMockSession } from '../../lib/mock-drafts';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import DraftsBanner from './DraftsBanner.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockDrafts();
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
});

function entry(title: string) {
  const found = [...mockLibrary().notes].find(([path]) => path.includes(title));
  if (!found) throw new Error(`no demo note "${title}"`);
  return { path: found[0], note: found[1].note, stamp: found[1].stamp };
}

/** A draft of the demo note `title`, with one stroke less. */
async function draftOf(title: string) {
  const { path, note, stamp } = entry(title);
  await api.draftWrite({ path, noteId: note.id, note: { ...note, strokes: note.strokes.slice(1) }, baseStamp: stamp, imageKey: null });
  return { path, id: note.id };
}

async function render() {
  const lib = new LibraryState();
  const stop = lib.start();
  const onopen = vi.fn();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(DraftsBanner, { target, props: { lib, onopen } });
  cleanup = () => {
    unmount(component);
    stop();
    target.remove();
  };
  await vi.waitFor(() => expect(lib.listing).not.toBeNull());
  const banner = () => target.querySelector<HTMLElement>('[data-slot="drafts-banner"]');
  const action = (name: string, within: ParentNode = target) => within.querySelector<HTMLButtonElement>(`[data-action="${name}"]`);
  return { lib, onopen, target, banner, action };
}

describe('drafts on the library screen', () => {
  it('shows nothing without drafts left by an earlier run', async () => {
    await draftOf('Cours de physique');
    const view = await render();
    await tick();
    // Written by this run (the note is open in the editor): not a leftover.
    expect(view.banner()).toBeNull();
  });

  it('lists the leftovers and opens their note', async () => {
    const { path } = await draftOf('Cours de physique');
    startNewMockSession();
    const view = await render();
    await vi.waitFor(() => expect(view.banner()).toBeTruthy());
    expect(view.banner()!.textContent).toContain("Ardoise s'est fermé sans enregistrer des modifications. Notes concernées : 1");
    expect(view.banner()!.textContent).toContain('Cours de physique');
    expect(view.banner()!.textContent).toContain('dernière copie le');
    view.action('drafts-open')!.click();
    expect(view.onopen).toHaveBeenCalledWith(path);
  });

  it('recovers the draft of a note that is gone into a new note', async () => {
    const { path, id } = await draftOf('Cours de physique');
    mockLibrary().notes.delete(path);
    startNewMockSession();
    const view = await render();
    await vi.waitFor(() => expect(view.banner()?.textContent).toContain("la note n'existe plus"));
    expect(view.action('drafts-open')).toBeNull();
    view.action('drafts-copy')!.click();
    await vi.waitFor(() => expect(view.onopen).toHaveBeenCalled());
    const [copyPath] = view.onopen.mock.calls[0] as [string];
    const copy = mockLibrary().notes.get(copyPath)!.note;
    expect(copy.title).toBe('Cours de physique (récupérée)');
    expect(copy.id).not.toBe(id);
    await vi.waitFor(() => expect(view.banner()).toBeNull());
  });

  it('discards a draft after asking once more', async () => {
    await draftOf('Cours de physique');
    await draftOf('Liste de courses');
    startNewMockSession();
    const view = await render();
    await vi.waitFor(() => expect(view.banner()?.querySelectorAll('li')).toHaveLength(2));
    const item = view.banner()!.querySelector<HTMLElement>(`li[data-draft="${entry('Liste de courses').note.id}"]`)!;
    view.action('drafts-discard', item)!.click();
    await tick();
    expect(item.textContent).toContain('Abandonner définitivement les modifications de « Liste de courses » ?');
    view.action('drafts-discard-yes', item)!.click();
    await vi.waitFor(() => expect(view.banner()?.querySelectorAll('li')).toHaveLength(1));
    expect(await api.draftFind(entry('Liste de courses').note.id)).toBeNull();
    expect(await api.draftFind(entry('Cours de physique').note.id)).not.toBeNull();
  });

  it('speaks English', async () => {
    setLocale('en');
    await draftOf('Cours de physique');
    startNewMockSession();
    const view = await render();
    await vi.waitFor(() => expect(view.banner()?.textContent).toContain('Ardoise closed without saving some changes'));
    expect(view.action('drafts-open')!.textContent?.trim()).toBe('Open');
  });
});
