// @vitest-environment jsdom
// The tablet panel with the Xiaomi blackboard: two tablets known, its details, the sync confirmation, one forget each,
// and a library that knows the blackboard only.
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
  // The Slate is out of range: the blackboard is the one that connects.
  mockTablet.inRange = false;
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

const OPTIONS: TabletOptions = { autoSync: false, deleteAfterImport: true, orientation: 'landscape', boardPosition: 'buttonsBottom', button: 'newPage' };

interface Options {
  locale?: 'fr' | 'en';
  options?: Partial<TabletOptions>;
  /** Forget the Slate: only the blackboard is known. */
  boardOnly?: boolean;
  /** Wait for the blackboard to connect. */
  connect?: boolean;
}

/** Mounts the panel over a library that knows the blackboard, added through the search. */
async function render({ locale = 'fr', options = {}, boardOnly = false, connect = true }: Options = {}) {
  setLocale(locale);
  if (boardOnly) await api.forgetTablet();
  await api.tabletSetOptions({ ...OPTIONS, ...options });
  tablet.start();
  await settle();
  await api.tabletFindBlackboards();
  await beat(1);
  await api.tabletAddBlackboard(tablet.status!.boardSearch!.found[0].address);
  await settle();
  const handlers = { onclose: vi.fn(), onlive: vi.fn(), ontest: vi.fn() };
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(TabletPanel, { target, props: handlers });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  if (connect) await beat(1.5);
  await settle();
  const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const action = (name: string) => q<HTMLButtonElement>(`button[data-action="${name}"]`);
  const click = async (el: Element | null) => {
    if (!el) throw new Error('no such element');
    (el as HTMLElement).click();
    await settle();
  };
  return { target, q, action, click, ...handlers };
}

const names = (target: HTMLElement) =>
  [...target.querySelectorAll('[data-slot="known-tablet"]')].map((el) => el.getAttribute('data-kind'));

describe('tablet panel: two tablets', () => {
  it('lists both, says which one is connected, with one « Oublier » each', async () => {
    const { target, q } = await render();
    expect(names(target)).toEqual(['slate', 'blackboard']);
    const slate = q('[data-slot="known-tablet"][data-kind="slate"]')!;
    const board = q('[data-slot="known-tablet"][data-kind="blackboard"]')!;
    expect(slate.textContent).toContain('Bamboo Slate');
    expect(slate.textContent).toContain('Pas connectée');
    expect(board.textContent).toContain('Tablette Xiaomi');
    expect(board.textContent).toContain('Connectée');
    expect(board.textContent).toContain('Xiaomi Mijia LCD blackboard');
    for (const el of [slate, board]) expect(el.querySelectorAll('button[data-action="forget"]')).toHaveLength(1);
    // Both are there: nothing more to add.
    expect(q('[data-action="register"]')).toBeNull();
    expect(q('[data-action="add-blackboard"]')).toBeNull();
  });

  it('forgets the blackboard after a confirmation, and keeps the Slate', async () => {
    const forget = vi.spyOn(api, 'tabletForgetBlackboard');
    const forgetSlate = vi.spyOn(api, 'forgetTablet');
    const { q, target, click } = await render();
    await click(q('[data-kind="blackboard"] button[data-action="forget"]'));
    const dialog = q('[role="alertdialog"]')!;
    expect(dialog.textContent).toContain('Oublier la tablette Xiaomi ?');
    expect(dialog.textContent).toContain('reste liée à Mi Home');
    // The other tablet has no question.
    expect(q('[data-kind="slate"] [role="alertdialog"]')).toBeNull();
    expect(forget).not.toHaveBeenCalled();

    await click([...dialog.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Annuler')!);
    expect(q('[role="alertdialog"]')).toBeNull();
    expect(forget).not.toHaveBeenCalled();

    await click(q('[data-kind="blackboard"] button[data-action="forget"]'));
    await click(q('button[data-action="forget-yes"]'));
    expect(forget).toHaveBeenCalledOnce();
    expect(forgetSlate).not.toHaveBeenCalled();
    expect(names(target)).toEqual(['slate']);
    expect(await api.tabletInfo()).not.toBeNull();
    // It can be added again.
    expect(q('[data-action="add-blackboard"]')).not.toBeNull();
  });

  it('forgets the Slate alone, with the confirmation it always had', async () => {
    const { q, target, click } = await render();
    await click(q('[data-kind="slate"] button[data-action="forget"]'));
    expect(q('[role="alertdialog"]')!.textContent).toContain('Oublier la tablette « Bamboo Slate » ?');
    await click(q('button[data-action="forget-yes"]'));
    expect(await api.tabletInfo()).toBeNull();
    expect(names(target)).toEqual(['blackboard']);
    // The panel goes on with the blackboard: it is still connected.
    expect(q('[data-slot="state"]')!.textContent).toContain('Connectée');
    expect(q('[data-action="register"]')).not.toBeNull();
  });

  it('asks to switch on the Xiaomi or press the Slate button while waiting for one of the two', async () => {
    const { q } = await render({ connect: false });
    expect(q('[data-slot="hint"]')!.textContent).toContain('Allumez la tablette Xiaomi, ou appuyez brièvement sur le bouton de la Bamboo Slate');
  });
});

describe('tablet panel: the blackboard\'s details', () => {
  it('shows the blackboard on the first tab with its own drawing, battery and pages, and no name', async () => {
    const { q } = await render();
    const card = q('[aria-labelledby="tablet-info"]')!;
    expect(card.getAttribute('data-kind')).toBe('blackboard');
    expect(card.querySelector('[data-slot="card-name"]')!.textContent).toBe('Tablette Xiaomi');
    expect(card.querySelector('[data-slot="state"]')!.textContent).toContain('Connectée');
    expect(card.querySelector('.board-drawing')!.getAttribute('data-position')).toBe('buttonsBottom');
    expect(card.querySelector('.orientation-icon')).toBeNull();
    expect(card.querySelector('.battery')!.textContent).toContain('90 %');
    expect(card.querySelector('.battery')!.getAttribute('data-charging')).toBe('false');
    expect(card.querySelector('[data-slot="stored-pages"]')!.textContent).toBe('3 pages en mémoire');
    expect(card.querySelector('[data-slot="name"]')).toBeNull();
  });

  it('shows battery, free memory of the total, pages and model on the Tablets tab, and hides what it does not have', async () => {
    const { q, target } = await render();
    const board = q('[data-slot="known-tablet"][data-kind="blackboard"]')!;
    expect(board.querySelector('.battery')!.textContent).toContain('90 %');
    expect(board.querySelector('[data-slot="known-memory"]')!.textContent!.replace(/\s+/g, ' ').trim()).toBe('126,4 Mo sur 126,9 Mo');
    expect(board.querySelector('[data-slot="known-pages"]')!.textContent).toBe('3');
    expect(board.textContent).toContain('Xiaomi Mijia LCD blackboard');
    expect(board.textContent).toContain('205 × 272 mm');
    // No firmware, no "charging": the Slate's own.
    expect(board.textContent).not.toContain('Version du firmware');
    expect(board.querySelector('.battery')!.getAttribute('data-charging')).toBe('false');
    expect(board.querySelector('[data-slot="board-clock"]')!.textContent).toContain("mise à l'heure à chaque connexion");
    // Its settings have no name to change, no clock to set.
    expect(q('[data-slot="tablet-options"]')!.getAttribute('data-kind')).toBe('blackboard');
    expect(q('#tablet-name')).toBeNull();
    for (const name of ['rename', 'set-time']) expect(q(`button[data-action="${name}"]`), name).toBeNull();
    expect(target.textContent).not.toContain('Réglages de la tablette');
  });

  it('shows the memory in English too', async () => {
    const { q } = await render({ locale: 'en' });
    expect(q('[data-slot="known-memory"]')!.textContent!.replace(/\s+/g, ' ').trim()).toBe('126.4 MB of 126.9 MB');
    expect(q('[data-slot="known-tablet"][data-kind="blackboard"]')!.textContent).toContain('Free memory');
  });

  it('counts the pages down while syncing, and shows the memory freed', async () => {
    const { q, action, click } = await render();
    await click(action('sync'));
    await beat(0.7);
    expect(q('[data-slot="stored-pages"]')!.getAttribute('data-count')).toBe('2');
    await beat(3);
    expect(q('[data-slot="stored-pages"]')!.getAttribute('data-count')).toBe('0');
    expect(q('[data-slot="known-memory"]')!.textContent!.replace(/\s+/g, ' ').trim()).toBe('126,9 Mo sur 126,9 Mo');
    expect(q('[data-slot="last-sync"]')!.textContent).toContain('3 notes enregistrées.');
    expect(q('[data-slot="last-sync"]')!.textContent).toContain('3 pages effacées de la tablette.');
  });

  it('puts the Slate settings back when the Slate is the tablet that was read', async () => {
    const { q } = await render();
    mockTablet.inRange = true;
    // The Slate connects once the blackboard link is closed: make it the last one read.
    await api.tabletFindBlackboards();
    await api.tabletCancel();
    await beat(2);
    expect(tablet.status?.active).toBe('slate');
    // The settings are the Slate's, the tablet that is connected.
    expect(q('[data-slot="tablet-options"]')!.getAttribute('data-kind')).toBe('slate');
    expect(q('#tablet-name')).not.toBeNull();
    expect(q('button[data-action="set-time"]')).not.toBeNull();
    expect(q('[data-kind="slate"]')!.textContent).toContain('Connectée');
  });
});

describe('tablet panel: syncing the blackboard', () => {
  it('syncs without a question when the pages are deleted after import', async () => {
    const sync = vi.spyOn(api, 'tabletSync');
    const { q, action, click } = await render();
    await click(action('sync'));
    expect(q('[data-slot="sync-confirm"]')).toBeNull();
    expect(sync).toHaveBeenCalledWith(false);
    expect(tablet.status?.connection).toBe('syncing');
  });

  it('says the pages wait, and asks to confirm their deletion, with the option off', async () => {
    const sync = vi.spyOn(api, 'tabletSync');
    const { q, action, click } = await render({ options: { deleteAfterImport: false } });
    // Nothing was imported by itself, and the user is told what waits.
    expect(tablet.status?.connection).toBe('connected');
    expect(q('[data-slot="board-waiting"]')!.textContent).toContain('3 pages attendent sur la tablette Xiaomi');
    expect(q('[data-slot="board-delete-hint"]')!.textContent).toContain("effacées de la tablette quand même");

    await click(action('sync'));
    expect(sync).not.toHaveBeenCalled();
    const confirm = q('[data-slot="sync-confirm"]')!;
    expect(confirm.getAttribute('role')).toBe('alertdialog');
    expect(confirm.textContent).toContain('Les pages seront effacées de la tablette');
    expect(confirm.textContent).toContain('ne donne que sa page la plus ancienne');
    expect(action('sync')).toBeNull();

    // Cancelling changes nothing.
    await click(action('sync-cancel'));
    expect(q('[data-slot="sync-confirm"]')).toBeNull();
    expect(sync).not.toHaveBeenCalled();
    expect(tablet.status?.device?.storedPages).toBe(3);

    // Accepting syncs, with the flag.
    await click(action('sync'));
    await click(action('sync-confirm'));
    expect(sync).toHaveBeenCalledOnce();
    expect(sync).toHaveBeenCalledWith(true);
    await beat(3);
    expect(tablet.status?.lastSync).toMatchObject({ deleted: 3 });
    expect(tablet.status?.device?.storedPages).toBe(0);
    expect(q('[data-slot="board-waiting"]')).toBeNull();
  });

  it('words one page, and asks in English too', async () => {
    const { q, action, click } = await render({ locale: 'en', options: { deleteAfterImport: false } });
    expect(q('[data-slot="board-waiting"]')!.textContent).toContain('3 pages are waiting on the Xiaomi tablet');
    await click(action('sync'));
    expect(q('[data-slot="sync-confirm"]')!.textContent).toContain('The pages will be deleted from the tablet');
    expect(action('sync-confirm')!.textContent!.trim()).toBe('Sync and delete');
    const device = tablet.status!.device!;
    mockTabletPatch({ device: { ...device, memory: { ...device.memory! }, storedPages: 1 } });
    await settle();
    await click(action('sync-cancel'));
    expect(q('[data-slot="board-waiting"]')!.textContent).toContain('1 page is waiting');
  });

  it('asks when the service asks, if the blackboard connected after the click', async () => {
    mockTablet.inRange = false;
    const { q, action, click } = await render({ options: { deleteAfterImport: false }, connect: false });
    // The panel believes the Slate may be the one that connects: the click goes to the service...
    vi.spyOn(api, 'tabletSync').mockRejectedValueOnce({ code: 'blackboardConfirmDelete', message: '' });
    mockTabletPatch({ connection: 'connected' });
    await settle();
    await click(action('sync'));
    // ...which refuses and the question appears, without an error banner.
    expect(q('[data-slot="sync-confirm"]')).not.toBeNull();
    expect(q('[data-slot="tablet-error"]')).toBeNull();
  });

  it('opens the confirmation when the worker asks for it, instead of a red banner', async () => {
    const { q, action } = await render({ options: { deleteAfterImport: false } });
    mockTabletPatch({ error: { code: 'blackboardConfirmDelete', message: '' } });
    await settle();
    expect(q('[data-slot="sync-confirm"]')).not.toBeNull();
    expect(q('[data-slot="tablet-error"]')).toBeNull();
    // Accepting goes on with the flag, as from the button.
    const sync = vi.spyOn(api, 'tabletSync');
    action('sync-confirm')!.click();
    await settle();
    expect(sync).toHaveBeenCalledWith(true);
  });

  it('shows no red banner for that request even when the Xiaomi is not connected', async () => {
    const { q } = await render({ options: { deleteAfterImport: false }, connect: false });
    mockTabletPatch({ error: { code: 'blackboardConfirmDelete', message: '' } });
    await settle();
    expect(q('[data-slot="tablet-error"]')).toBeNull();
    expect(q('[data-slot="sync-confirm"]')).toBeNull();
  });

  it('closes the confirmation by itself when the Xiaomi is no longer the connected tablet', async () => {
    const { q, action, click } = await render({ options: { deleteAfterImport: false } });
    await click(action('sync'));
    expect(q('[data-slot="sync-confirm"]')).not.toBeNull();
    // The link is lost: waiting again, nothing is connected.
    mockTabletPatch({ connection: 'searching' });
    await settle();
    expect(q('[data-slot="sync-confirm"]')).toBeNull();
    expect(action('sync-confirm')).toBeNull();
    // It does not come back by itself when the Xiaomi reconnects.
    mockTabletPatch({ connection: 'connected', active: 'blackboard' });
    await settle();
    expect(q('[data-slot="sync-confirm"]')).toBeNull();
  });

  it('disables the sync when only the Xiaomi is known, not connected, with its pages imported on request', async () => {
    const sync = vi.spyOn(api, 'tabletSync');
    const { q, action } = await render({ boardOnly: true, options: { deleteAfterImport: false }, connect: false });
    expect(action('sync')!.disabled).toBe(true);
    expect(q('[data-slot="board-on-request"]')!.textContent).toContain("n'importe ses pages qu'à votre demande");
    expect(sync).not.toHaveBeenCalled();
    // Connected: the button works (through the confirmation) and the sentence goes.
    mockTablet.boardInRange = true;
    await api.tabletCancel();
    mockTabletPatch({ connection: 'connected', active: 'blackboard' });
    await settle();
    expect(action('sync')!.disabled).toBe(false);
    expect(q('[data-slot="board-on-request"]')).toBeNull();
  });

  it('keeps the sync enabled when deletion is on, or when the Slate is known too', async () => {
    const on = await render({ boardOnly: true, connect: false });
    expect(on.action('sync')!.disabled).toBe(false);
    expect(on.q('[data-slot="board-on-request"]')).toBeNull();
    cleanup?.();
    cleanup = null;
    tablet.reset();
    resetMockLibrary();
    resetMockTablet();
    mockTablet.inRange = false;
    const both = await render({ options: { deleteAfterImport: false }, connect: false });
    expect(both.action('sync')!.disabled).toBe(false);
    expect(both.q('[data-slot="board-on-request"]')).toBeNull();
  });

  it('does not warn, with the option off, while the Slate is the connected tablet or none is', async () => {
    const sync = vi.spyOn(api, 'tabletSync');
    const { q, action, click } = await render({ options: { deleteAfterImport: false }, connect: false });
    // Nothing connected yet: the sync waits, without a question.
    expect(tablet.status?.active).toBeNull();
    await click(action('sync'));
    expect(q('[data-slot="sync-confirm"]')).toBeNull();
    expect(sync).toHaveBeenCalledWith(false);
    expect(q('[data-slot="board-waiting"]')).toBeNull();
  });

  it('shows the refusal of the service in words', async () => {
    const { q, action, click } = await render();
    vi.spyOn(api, 'tabletSync').mockRejectedValue({ code: 'transferIncomplete', message: '' });
    await click(action('sync'));
    expect(q('[data-slot="tablet-error"]')!.textContent).toContain("n'a pas été reçue en entier");
  });
});

describe('tablet panel: options with the blackboard', () => {
  const option = (target: HTMLElement, name: string) => target.querySelector(`input[data-option="${name}"]`);

  it('shows the Xiaomi\'s settings while it is the connected tablet, the Slate\'s once it is chosen in the list', async () => {
    const { target, q } = await render();
    // The Xiaomi is connected: its position, not the Slate's button and position.
    expect(option(target, 'boardPosition')).not.toBeNull();
    expect(option(target, 'orientation')).toBeNull();
    expect(option(target, 'button')).toBeNull();
    expect(option(target, 'deleteAfterImport')).not.toBeNull();
    const select = q<HTMLSelectElement>('[data-slot="settings-tablet"]')!;
    select.value = 'slate';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    expect(option(target, 'orientation')).not.toBeNull();
    expect(option(target, 'button')).not.toBeNull();
    expect(option(target, 'boardPosition')).toBeNull();
    expect(option(target, 'deleteAfterImport')).not.toBeNull();
  });

  it('shows neither the button nor the Slate\'s position when only the blackboard is known, but its own position', async () => {
    const { target, q } = await render({ boardOnly: true });
    expect(option(target, 'orientation')).toBeNull();
    expect(option(target, 'button')).toBeNull();
    expect(option(target, 'boardPosition')).not.toBeNull();
    expect(option(target, 'autoSync')).not.toBeNull();
    expect(option(target, 'deleteAfterImport')).not.toBeNull();
    expect(q('[data-slot="board-delete-hint"]')).not.toBeNull();
  });
});

describe('tablet panel: a library that knows the blackboard only', () => {
  it('assumes no Slate anywhere', async () => {
    const { q, target, action } = await render({ boardOnly: true });
    expect(tablet.status?.registered).toBeNull();
    expect(names(target)).toEqual(['blackboard']);
    expect(q('[data-slot="state"]')!.textContent).toContain('Connectée');
    expect(q('.battery')!.textContent).toContain('90 %');
    expect(action('sync')).not.toBeNull();
    expect(action('live')!.disabled).toBe(false);
    // The Slate can still be registered, and the guided test (the Slate's) is not offered.
    expect(action('register')).not.toBeNull();
    expect(action('test')).toBeNull();
    expect(action('sim-press')).toBeNull();
    expect(action('sim-store')).not.toBeNull();
    expect(target.textContent).not.toContain('Aucune tablette');
  });

  it('waits for the blackboard with the right words', async () => {
    const { q } = await render({ boardOnly: true, connect: false });
    expect(q('[data-slot="hint"]')!.textContent).toContain('Allumez la tablette Xiaomi');
    expect(q('[data-slot="hint"]')!.textContent).toContain('Fermez Mi Home');
    expect(q('[data-slot="known-not-read"]')!.textContent).toContain("n'a pas encore été lue");
    expect(q('[data-slot="known-tablet"][data-kind="blackboard"]')!.textContent).toContain('205 × 272 mm');
    expect(q('[aria-labelledby="tablet-info"]')!.textContent).toContain('Tablette Xiaomi');
  });

  it('stores a page with the simulated button, and imports it', async () => {
    const { q, action, click } = await render({ boardOnly: true });
    await click(action('sim-store'));
    expect(q('[data-slot="stored-pages"]')!.getAttribute('data-count')).toBe('4');
  });

  it('offers to add the blackboard again once it is forgotten, and nothing else', async () => {
    const { q, target, click } = await render({ boardOnly: true });
    await click(q('button[data-action="forget"]'));
    await click(q('button[data-action="forget-yes"]'));
    expect(tablet.status?.connection).toBe('notRegistered');
    expect(target.textContent).toContain("Aucune tablette n'est enregistrée.");
    expect(q('[data-action="add-blackboard"]')).not.toBeNull();
    expect(q('[data-action="register"]')).not.toBeNull();
    expect(q('button[data-action="sync"]')).toBeNull();
  });
});

describe('tablet panel: adding the blackboard from the panel', () => {
  it('opens the blackboard flow, not the Slate wizard', async () => {
    setLocale('fr');
    await api.forgetTablet();
    tablet.start();
    await settle();
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(TabletPanel, { target, props: { onclose: vi.fn(), onlive: vi.fn(), ontest: vi.fn() } });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await settle();
    target.querySelector<HTMLElement>('[data-action="add-blackboard"]')!.click();
    await settle();
    expect(target.querySelector('.add-blackboard [data-step]')?.getAttribute('data-step')).toBe('intro');
    expect(target.querySelector('.register-wizard')).toBeNull();
    // The Slate keeps its own flow.
    target.querySelector<HTMLElement>('[data-action="cancel-add"]')!.click();
    await settle();
    target.querySelector<HTMLElement>('[data-action="register"]')!.click();
    await settle();
    expect(target.querySelector('.register-wizard [data-step]')?.getAttribute('data-step')).toBe('warning');
  });
});
