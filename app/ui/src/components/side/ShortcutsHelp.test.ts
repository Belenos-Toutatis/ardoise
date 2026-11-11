// @vitest-environment jsdom
import { mount, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { EDITOR_SHORTCUTS } from '../../lib/editor/shortcuts';
import { setLocale, t } from '../../lib/i18n.svelte';
import { LIVE_SHORTCUTS } from '../../lib/live-shortcuts';
import type { Shortcut } from '../../lib/shortcuts';
import type { SideWhere } from '../../lib/side-panel.svelte';
import ShortcutsHelp from './ShortcutsHelp.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => setLocale('fr'));

afterEach(() => {
  cleanup?.();
  cleanup = null;
});

function render(where: SideWhere, shortcuts: readonly Shortcut<object>[]) {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(ShortcutsHelp, { target, props: { where, shortcuts } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const dt = (id: string) => target.querySelector<HTMLElement>(`dt[data-shortcut="${id}"]`)!;
  /** What the entry's `dt` shows, the chords as `Ctrl+Z` (one `kbd` per key). */
  const chords = (id: string) => dt(id).textContent!.replace(/\s+/g, ' ').trim();
  return { target, dt, chords };
}

describe('ShortcutsHelp: the editor', () => {
  it('shows the main entries in one list, the replay keys under their own heading, then the note on typing', () => {
    const { target, dt } = render('editor', EDITOR_SHORTCUTS);
    const section = target.querySelector('section.pane-block[data-section="shortcuts"]')!;
    expect(section.querySelector('h3.pane-title')!.textContent).toBe("Raccourcis de l'éditeur");
    expect(section.getAttribute('aria-labelledby')).toBe(section.querySelector('h3')!.id);
    const lists = [...section.querySelectorAll('dl.keys')];
    expect(lists).toHaveLength(2);
    expect(section.querySelectorAll('h4')).toHaveLength(1);
    expect(section.querySelector('h4')!.textContent).toBe('Pendant une relecture');
    const ids = (list: Element) => [...list.querySelectorAll('dt')].map((el) => el.dataset.shortcut);
    expect(ids(lists[0])).toEqual(EDITOR_SHORTCUTS.filter((entry) => !entry.group).map((entry) => entry.id));
    expect(ids(lists[1])).toEqual(['leaveReplay', 'playPause']);
    expect(dt('undo').nextElementSibling!.textContent).toBe('Annuler');
    expect(section.querySelector('p.pane-help')!.textContent).toBe(t('side.help.typingEditor'));
  });

  it('shows one chord per display, as kbd separated by « + » and « , », and leaves the variants out', () => {
    const { chords, dt } = render('editor', EDITOR_SHORTCUTS);
    expect(chords('undo')).toBe('Ctrl + Z');
    expect(chords('redo')).toBe('Ctrl + Y, Ctrl + Maj + Z');
    expect(chords('clipboard')).toBe('Ctrl + C, Ctrl + X, Ctrl + V');
    expect(chords('select')).toBe('V');
    expect(chords('panHold')).toBe('Espace');
    expect(chords('delete')).toBe('Suppr, Retour arrière');
    expect(chords('pages')).toBe('Alt + ←, Alt + →');
    expect(chords('fit')).toBe('Ctrl + 0');
    expect(chords('deselect')).toBe('Échap');
    expect([...dt('redo').querySelectorAll('kbd')].map((kbd) => kbd.textContent)).toEqual(['Ctrl', 'Y', 'Ctrl', 'Maj', 'Z']);
  });

  it('speaks English', () => {
    setLocale('en');
    const { chords, target } = render('editor', EDITOR_SHORTCUTS);
    expect(chords('redo')).toBe('Ctrl + Y, Ctrl + Shift + Z');
    expect(chords('deselect')).toBe('Esc');
    expect(target.querySelector('h3')!.textContent).toBe('Editor shortcuts');
    expect(target.querySelector('h4')!.textContent).toBe('During a replay');
  });
});

describe('ShortcutsHelp: live mode', () => {
  it('merges duplicate displays, shows keysLabel, and puts the projected window keys in their group', () => {
    const { target, chords, dt } = render('live', LIVE_SHORTCUTS);
    expect(target.querySelector('h3')!.textContent).toBe('Raccourcis du mode direct');
    expect(chords('leave')).toBe('Échap');
    expect(chords('next')).toBe('→, Pg suiv, Espace');
    expect(chords('zoom')).toBe('+, −');
    expect(chords('whole')).toBe('0');
    expect(chords('undo')).toBe('Ctrl + Z, Ctrl + Y');
    expect(chords('pan')).toBe('Maj + flèches');
    expect(dt('pan').querySelector('kbd')).toBeNull();
    expect(target.querySelector('h4')!.textContent).toBe('Dans la fenêtre projetée');
    const second = target.querySelectorAll('dl.keys')[1];
    expect([...second.querySelectorAll('dt')].map((el) => el.dataset.shortcut)).toEqual(['marks']);
    expect(target.querySelector('p.pane-help')!.textContent).toBe(t('side.help.typingLive'));
    expect(t('side.help.typingLive')).toContain('Espace tourne la page');
  });

  it('gives each entry the text of its table', () => {
    const { dt } = render('live', LIVE_SHORTCUTS);
    for (const entry of LIVE_SHORTCUTS) expect(dt(entry.id).nextElementSibling!.textContent, entry.id).toBe(t(entry.label));
    expect(dt('delete').nextElementSibling!.textContent).toBe('Supprimer la sélection');
  });
});
