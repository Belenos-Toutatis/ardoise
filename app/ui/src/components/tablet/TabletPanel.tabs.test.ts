// @vitest-environment jsdom
// The tablet panel's three tabs (Q17): the rail and its keyboard, what each tab holds, the badge, the settings of the
// tablet that is chosen in the list, and the Xiaomi's four positions.
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockTablet, mockTabletPatch, resetMockTablet } from '../../lib/mock-tablet';
import type { TabletOptions } from '../../lib/tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import TabletPanel from './TabletPanel.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  resetMockLibrary();
  resetMockTablet();
  tablet.reset();
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
  resetMockTablet();
  vi.useRealTimers();
});

async function settle() {
  for (let i = 0; i < 3; i++) {
    await vi.advanceTimersByTimeAsync(0);
    await tick();
  }
}

const beat = async (beats = 1) => {
  await vi.advanceTimersByTimeAsync(Math.round(beats * mockTablet.stepMs));
  await settle();
};

const OPTIONS: TabletOptions = {
  autoSync: false,
  deleteAfterImport: true,
  orientation: 'landscape',
  boardPosition: 'buttonsBottom',
  button: 'newPage',
};

interface Options {
  locale?: 'fr' | 'en';
  /** Know the Xiaomi too (added through the search). */
  board?: boolean;
  /** Forget the Slate: only the Xiaomi is known. */
  boardOnly?: boolean;
  /** Which tablet is in range (and connects). */
  inRange?: 'slate' | 'board' | 'none';
  options?: Partial<TabletOptions>;
}

async function render({ locale = 'fr', board = false, boardOnly = false, inRange = 'slate', options = {} }: Options = {}) {
  setLocale(locale);
  mockTablet.inRange = inRange === 'slate';
  if (boardOnly) await api.forgetTablet();
  await api.tabletSetOptions({ ...OPTIONS, ...options });
  tablet.start();
  await settle();
  if (board || boardOnly) {
    await api.tabletFindBlackboards();
    await beat(1);
    await api.tabletAddBlackboard(tablet.status!.boardSearch!.found[0].address);
    await settle();
  }
  const handlers = { onclose: vi.fn(), onlive: vi.fn(), ontest: vi.fn() };
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(TabletPanel, { target, props: handlers });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  if (inRange !== 'none') await beat(1.5);
  await settle();
  const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const tabButton = (id: string) => q<HTMLButtonElement>(`[role="tab"][data-tab="${id}"]`)!;
  const panelOf = (id: string) => q(`[role="tabpanel"][data-tab-panel="${id}"]`)!;
  const click = async (el: Element | null) => {
    if (!el) throw new Error('no such element');
    (el as HTMLElement).click();
    await settle();
  };
  const key = async (el: Element, name: string, init: KeyboardEventInit = {}) => {
    const event = new KeyboardEvent('keydown', { key: name, bubbles: true, cancelable: true, ...init });
    el.dispatchEvent(event);
    await settle();
    return event;
  };
  const option = (name: string, value?: string) =>
    q<HTMLInputElement>(`input[data-option="${name}"]${value ? `[value="${value}"]` : ''}`);
  return { target, q, tabButton, panelOf, click, key, option, ...handlers };
}

describe('tablet panel: the rail', () => {
  it('has three tabs, each with its own panel, the first one shown', async () => {
    const { q, tabButton, panelOf } = await render();
    const list = q('[role="tablist"]')!;
    expect(list.getAttribute('aria-orientation')).toBe('vertical');
    expect(list.getAttribute('aria-label')).toBe('Sections du panneau de la tablette');
    const tabs = [...list.querySelectorAll('[role="tab"]')];
    expect(tabs.map((el) => el.getAttribute('data-tab'))).toEqual(['use', 'settings', 'tablets']);
    expect(tabs.map((el) => el.getAttribute('title'))).toEqual(['Utiliser', 'Réglages', 'Tablettes']);
    expect(tabs.map((el) => el.getAttribute('aria-selected'))).toEqual(['true', 'false', 'false']);
    expect(tabs.map((el) => (el as HTMLElement).tabIndex)).toEqual([0, -1, -1]);
    for (const id of ['use', 'settings', 'tablets']) {
      const panel = panelOf(id);
      expect(tabButton(id).getAttribute('aria-controls')).toBe(panel.id);
      expect(panel.getAttribute('aria-labelledby')).toBe(tabButton(id).id);
      expect(panel.hidden).toBe(id !== 'use');
    }
    expect(q('[data-slot="tablet-title"]')!.textContent).toBe('Utiliser');
  });

  it('shows the tab that is clicked, with its name as the title, and keeps the others mounted', async () => {
    const { q, tabButton, panelOf, click } = await render();
    await click(tabButton('settings'));
    expect(panelOf('settings').hidden).toBe(false);
    expect(panelOf('use').hidden).toBe(true);
    expect(tabButton('settings').getAttribute('aria-selected')).toBe('true');
    expect(q('[data-slot="tablet-title"]')!.textContent).toBe('Réglages');
    await click(tabButton('tablets'));
    expect(panelOf('tablets').hidden).toBe(false);
    expect(q('[data-slot="tablet-title"]')!.textContent).toBe('Tablettes');
    // What the first tab holds is still there.
    expect(panelOf('use').querySelector('button[data-action="sync"]')).not.toBeNull();
    await click(tabButton('use'));
    expect(panelOf('use').hidden).toBe(false);
  });

  it('moves the focus along the rail with the arrow keys, Home and End, and chooses with Enter or a click', async () => {
    const { tabButton, panelOf, key, click } = await render();
    const use = tabButton('use');
    const settings = tabButton('settings');
    const tablets = tabButton('tablets');
    use.focus();
    expect((await key(use, 'ArrowDown')).defaultPrevented).toBe(true);
    expect(document.activeElement).toBe(settings);
    // Moving does not choose: the panel is still the first.
    expect(panelOf('use').hidden).toBe(false);
    await key(settings, 'ArrowDown');
    expect(document.activeElement).toBe(tablets);
    await key(tablets, 'ArrowDown');
    expect(document.activeElement).toBe(use);
    await key(use, 'ArrowUp');
    expect(document.activeElement).toBe(tablets);
    await key(tablets, 'Home');
    expect(document.activeElement).toBe(use);
    await key(use, 'End');
    expect(document.activeElement).toBe(tablets);
    // A key with a modifier is left alone.
    expect((await key(tablets, 'ArrowUp', { shiftKey: true })).defaultPrevented).toBe(false);
    expect(document.activeElement).toBe(tablets);
    // A button is chosen by Enter (the browser clicks it) as by a click.
    await click(tablets);
    expect(tabButton('tablets').getAttribute('aria-selected')).toBe('true');
    expect(tabButton('tablets').tabIndex).toBe(0);
    expect(tabButton('use').tabIndex).toBe(-1);
  });

  it('names the tabs in English', async () => {
    const { q } = await render({ locale: 'en' });
    expect([...q('[role="tablist"]')!.querySelectorAll('[role="tab"]')].map((el) => el.getAttribute('title'))).toEqual([
      'Use',
      'Settings',
      'Tablets',
    ]);
    expect(q('[role="tablist"]')!.getAttribute('aria-label')).toBe('Tablet panel sections');
  });

  it('has no rail while the state is not known yet, and still offers the way out', async () => {
    vi.spyOn(api, 'tabletSubscribe').mockImplementation(() => new Promise(() => {}));
    const { q, target } = await render({ inRange: 'none' });
    expect(q('[data-slot="loading"]')).not.toBeNull();
    expect(q('[role="tablist"]')).toBeNull();
    expect(target.textContent).toContain('Fermer');
  });
});

describe('tablet panel: the badge and the sync button', () => {
  it('counts the pages that wait on the tablet on the first tab, whatever tab is open', async () => {
    const { tabButton, click } = await render();
    const use = tabButton('use');
    expect(use.getAttribute('data-count')).toBe('3');
    expect(use.querySelector('.tp-count')!.textContent).toBe('3');
    // Assistive technology hears it after the tab's name.
    expect(use.querySelector('.visually-hidden')!.textContent).toBe('Utiliser, 3 pages en attente sur la tablette');
    await click(tabButton('settings'));
    expect(tabButton('use').getAttribute('data-count')).toBe('3');
    expect(tabButton('settings').getAttribute('data-count')).toBeNull();
  });

  it('says one page, and nothing when there is none', async () => {
    const { tabButton } = await render();
    mockTabletPatch({ device: { ...tablet.status!.device!, storedPages: 1 } });
    await settle();
    expect(tabButton('use').getAttribute('data-count')).toBe('1');
    expect(tabButton('use').querySelector('.visually-hidden')!.textContent).toBe('Utiliser, 1 page en attente sur la tablette');
    mockTabletPatch({ device: { ...tablet.status!.device!, storedPages: 0 } });
    await settle();
    expect(tabButton('use').getAttribute('data-count')).toBeNull();
    expect(tabButton('use').querySelector('.tp-count')).toBeNull();
    expect(tabButton('use').querySelector('.visually-hidden')!.textContent).toBe('Utiliser');
  });

  it('has no badge while the tablet is not connected', async () => {
    const { tabButton } = await render({ inRange: 'none' });
    expect(tablet.status?.device).toBeNull();
    expect(tabButton('use').getAttribute('data-count')).toBeNull();
  });

  it('says on the sync button how many pages wait, and nothing when none does', async () => {
    const { q, tabButton, click } = await render();
    const button = q('button[data-action="sync"]')!;
    expect(button.textContent!.replace(/\s+/g, ' ').trim()).toBe('Synchroniser 3 pages');
    await click(button);
    await beat(4);
    expect(tabButton('use').getAttribute('data-count')).toBeNull();
    expect(q('button[data-action="sync"]')!.textContent!.trim()).toBe('Synchroniser');
    expect(q('[data-slot="sync-waiting"]')).toBeNull();
  });

  it('puts live mode above the sync button', async () => {
    const { q } = await render();
    const live = q('button[data-action="live"]')!;
    const sync = q('button[data-action="sync"]')!;
    expect(live.textContent!.trim()).toBe('Mode direct');
    expect(live.compareDocumentPosition(sync) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });
});

describe('tablet panel: the Settings tab', () => {
  it('shows the Slate\'s settings only, with one tablet known: no list, nothing about sharing', async () => {
    const { q, panelOf, option } = await render();
    const page = panelOf('settings');
    expect(page.querySelector('[data-slot="settings-tablet"]')).toBeNull();
    expect(page.querySelector('[data-slot="tablet-options"]')!.getAttribute('data-kind')).toBe('slate');
    expect(option('autoSync')).not.toBeNull();
    expect(option('deleteAfterImport')).not.toBeNull();
    expect(option('button')).not.toBeNull();
    expect(option('orientation')).not.toBeNull();
    expect(option('boardPosition')).toBeNull();
    expect(page.querySelector('[data-slot="shared-options"]')).toBeNull();
    // Its name and its clock.
    expect(page.querySelector('#tablet-name')).not.toBeNull();
    expect(page.querySelector('button[data-action="set-time"]')).not.toBeNull();
    expect(q('[data-slot="board-delete-hint"]')).toBeNull();
  });

  it('shows the Xiaomi\'s settings with the Xiaomi alone: no button, no Slate position, no name, no clock', async () => {
    const { panelOf, option, q } = await render({ boardOnly: true, inRange: 'board' });
    mockTablet.boardInRange = true;
    const page = panelOf('settings');
    expect(page.querySelector('[data-slot="settings-tablet"]')).toBeNull();
    expect(page.querySelector('[data-slot="tablet-options"]')!.getAttribute('data-kind')).toBe('blackboard');
    expect(option('autoSync')).not.toBeNull();
    expect(option('deleteAfterImport')).not.toBeNull();
    expect(q('[data-slot="board-delete-hint"]')!.textContent).toContain('Tablette Xiaomi : sans cette option');
    expect(option('button')).toBeNull();
    expect(option('orientation')).toBeNull();
    expect(page.querySelector('#tablet-name')).toBeNull();
    expect(page.querySelector('button[data-action="set-time"]')).toBeNull();
  });

  it('says that there is nothing to set without a tablet', async () => {
    await api.forgetTablet();
    const { panelOf, option } = await render({ inRange: 'none' });
    expect(panelOf('settings').querySelector('[data-slot="settings-none"]')!.textContent).toContain('Aucune tablette n\'est connue');
    expect(option('autoSync')).toBeNull();
  });

  it('offers a list of the known tablets, the connected one chosen, and shows the settings of the one chosen', async () => {
    mockTablet.boardInRange = true;
    const { panelOf, option, click, q } = await render({ board: true, inRange: 'none' });
    mockTablet.boardInRange = true;
    await beat(2);
    expect(tablet.status?.active).toBe('blackboard');
    const select = panelOf('settings').querySelector<HTMLSelectElement>('[data-slot="settings-tablet"]')!;
    expect([...select.options].map((o) => o.textContent)).toEqual(['Bamboo Slate', 'Tablette Xiaomi']);
    expect(select.value).toBe('blackboard');
    expect(option('boardPosition')).not.toBeNull();
    expect(option('orientation')).toBeNull();
    // Both tablets: the two common options are one setting, and it is said.
    expect(panelOf('settings').querySelector('[data-slot="shared-options"]')!.textContent).toContain('valent pour toutes vos tablettes');

    select.value = 'slate';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    expect(panelOf('settings').querySelector('[data-slot="tablet-options"]')!.getAttribute('data-kind')).toBe('slate');
    expect(option('orientation')).not.toBeNull();
    expect(option('button')).not.toBeNull();
    expect(option('boardPosition')).toBeNull();
    expect(panelOf('settings').querySelector('#tablet-name')).not.toBeNull();
    expect(q('[data-slot="board-delete-hint"]')).toBeNull();
    // The choice is the user's: it stays while the Xiaomi stays connected.
    await beat(1);
    expect(select.value).toBe('slate');
    void click;
  });

  it('chooses the first tablet, the Slate, when none is connected', async () => {
    const { panelOf } = await render({ board: true, inRange: 'none' });
    expect(tablet.status?.active).toBeNull();
    expect(panelOf('settings').querySelector<HTMLSelectElement>('[data-slot="settings-tablet"]')!.value).toBe('slate');
  });

  it('follows the tablet that connects, until the user chooses', async () => {
    const { panelOf } = await render({ board: true, inRange: 'none' });
    const select = panelOf('settings').querySelector<HTMLSelectElement>('[data-slot="settings-tablet"]')!;
    expect(select.value).toBe('slate');
    mockTabletPatch({ connection: 'connected', active: 'blackboard' });
    await settle();
    expect(select.value).toBe('blackboard');
    mockTabletPatch({ connection: 'connected', active: 'slate' });
    await settle();
    expect(select.value).toBe('slate');
  });

  it('keeps the settings in French and in English', async () => {
    const { panelOf } = await render({ locale: 'en', board: true, inRange: 'none' });
    const page = panelOf('settings');
    expect(page.textContent).toContain('Tablet to set up');
    expect(page.textContent).toContain('These two settings apply to all your tablets.');
  });
});

describe('tablet panel: the Xiaomi\'s four positions', () => {
  const POSITIONS = ['buttonsBottom', 'buttonsTop', 'buttonsRight', 'buttonsLeft'];

  it('offers four positions, each with its drawing, in French', async () => {
    const { panelOf, option } = await render({ boardOnly: true, inRange: 'none' });
    const radios = [...panelOf('settings').querySelectorAll<HTMLInputElement>('input[data-option="boardPosition"]')];
    expect(radios.map((r) => r.value)).toEqual(POSITIONS);
    const labels = radios.map((r) => r.closest('label')!.textContent!.trim());
    expect(labels).toEqual([
      'En hauteur, boutons en bas',
      'En hauteur, boutons en haut',
      'En largeur, boutons à droite',
      'En largeur, boutons à gauche',
    ]);
    for (const radio of radios) {
      const drawing = radio.closest('label')!.querySelector('.board-drawing')!;
      expect(drawing.getAttribute('data-position')).toBe(radio.value);
      expect(drawing.getAttribute('aria-hidden')).toBe('true');
    }
    const group = panelOf('settings').querySelector('[role="radiogroup"][aria-label="Position de la tablette"]')!;
    const hint = group.querySelector(`#${group.getAttribute('aria-describedby')}`)!;
    expect(hint.textContent).toBe("Ardoise redresse ce que vous écrivez pour qu'il apparaisse à l'endroit.");
    // Upright, buttons at the bottom, until the user says otherwise.
    expect(option('boardPosition', 'buttonsBottom')!.checked).toBe(true);
  });

  it('names them in English', async () => {
    const { panelOf } = await render({ locale: 'en', boardOnly: true, inRange: 'none' });
    const labels = [...panelOf('settings').querySelectorAll<HTMLInputElement>('input[data-option="boardPosition"]')].map((r) =>
      r.closest('label')!.textContent!.trim(),
    );
    expect(labels).toEqual([
      'Upright, buttons at the bottom',
      'Upright, buttons at the top',
      'Wide, buttons on the right',
      'Wide, buttons on the left',
    ]);
  });

  it('saves the position with the other options as soon as it is chosen, and leaves the Slate\'s own', async () => {
    const save = vi.spyOn(api, 'tabletSetOptions');
    const { option, click } = await render({ boardOnly: true, inRange: 'none', options: { orientation: 'portraitLeft' } });
    await click(option('boardPosition', 'buttonsRight'));
    expect(save).toHaveBeenLastCalledWith({
      autoSync: false,
      deleteAfterImport: true,
      orientation: 'portraitLeft',
      boardPosition: 'buttonsRight',
      button: 'newPage',
    });
    expect((await api.tabletStatus()).options.boardPosition).toBe('buttonsRight');
    expect((await api.tabletStatus()).options.orientation).toBe('portraitLeft');
    expect(option('boardPosition', 'buttonsRight')!.checked).toBe(true);
    await click(option('boardPosition', 'buttonsLeft'));
    expect((await api.tabletStatus()).options.boardPosition).toBe('buttonsLeft');
    expect(option('boardPosition', 'buttonsBottom')!.checked).toBe(false);
  });

  it('keeps each tablet\'s position apart: the Slate\'s radios do not change the Xiaomi\'s', async () => {
    const { panelOf, option, click } = await render({ board: true, inRange: 'none' });
    await click(option('orientation', 'landscapeFlipped'));
    const options = (await api.tabletStatus()).options;
    expect(options.orientation).toBe('landscapeFlipped');
    expect(options.boardPosition).toBe('buttonsBottom');
    void panelOf;
  });

  it('puts a position back when the change is refused', async () => {
    const { option, click, q } = await render({ boardOnly: true, inRange: 'none' });
    vi.spyOn(api, 'tabletSetOptions').mockRejectedValue({ code: 'tablet', message: 'disk full' });
    await click(option('boardPosition', 'buttonsTop'));
    expect(option('boardPosition', 'buttonsBottom')!.checked).toBe(true);
    expect(q('[data-slot="tablet-error"]')!.textContent).toContain('disk full');
  });

  it('reads the default when the options come without a position (a Rust side that has none yet)', async () => {
    const { option } = await render({ boardOnly: true, inRange: 'none' });
    const { boardPosition, ...older } = tablet.status!.options;
    void boardPosition;
    mockTabletPatch({ options: older as TabletOptions });
    await settle();
    expect(option('boardPosition', 'buttonsBottom')!.checked).toBe(true);
  });
});

describe('tablet panel: the Tablets tab', () => {
  it('lists the tablets with what they reported, one « Forget » each, and says how to add one', async () => {
    mockTablet.boardInRange = true;
    const { panelOf, q } = await render({ board: true, inRange: 'none' });
    mockTablet.boardInRange = true;
    await beat(2);
    const page = panelOf('tablets');
    const board = page.querySelector('[data-slot="known-tablet"][data-kind="blackboard"]')!;
    expect(board.getAttribute('data-connected')).toBe('true');
    expect(board.textContent).toContain('Connectée');
    expect(board.querySelector('[data-slot="known-memory"]')!.textContent!.replace(/\s+/g, ' ').trim()).toBe('126,4 Mo sur 126,9 Mo');
    expect(board.querySelector('[data-slot="known-firmware"]')).toBeNull();
    expect(board.textContent).toContain('205 × 272 mm');
    expect(board.querySelector('[data-slot="board-clock"]')!.textContent).toContain("mise à l'heure à chaque connexion");
    const slate = page.querySelector('[data-slot="known-tablet"][data-kind="slate"]')!;
    expect(slate.getAttribute('data-connected')).toBe('false');
    expect(slate.textContent).toContain('Pas connectée');
    expect(slate.querySelector('.battery')).toBeNull();
    for (const el of [board, slate]) expect(el.querySelectorAll('button[data-action="forget"]')).toHaveLength(1);
    // Both are known: nothing to add.
    expect(page.querySelector('[data-slot="add-tablet"]')).toBeNull();
    expect(q('[data-slot="known-not-read"]')).toBeNull();
  });

  it('says « Add a tablet » with the entry of the tablet that is missing', async () => {
    const { panelOf } = await render();
    const add = panelOf('tablets').querySelector('[data-slot="add-tablet"]')!;
    expect(add.textContent).toContain('Ajouter une tablette');
    expect(add.querySelector('[data-action="add-blackboard"]')).not.toBeNull();
    expect(add.querySelector('[data-action="register"]')).toBeNull();
  });

  it('offers both entries when no tablet is known, here as on the first tab', async () => {
    await api.forgetTablet();
    const { panelOf } = await render({ inRange: 'none' });
    for (const id of ['use', 'tablets']) {
      expect(panelOf(id).querySelector('[data-action="register"]'), id).not.toBeNull();
      expect(panelOf(id).querySelector('[data-action="add-blackboard"]'), id).not.toBeNull();
    }
  });

  it('keeps the guided test of the Slate on this tab', async () => {
    const { panelOf, ontest, click } = await render();
    await click(panelOf('tablets').querySelector('[data-action="test"]'));
    expect(ontest).toHaveBeenCalledOnce();
  });
});
