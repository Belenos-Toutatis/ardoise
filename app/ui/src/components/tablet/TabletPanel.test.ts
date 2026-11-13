// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockTablet, mockTabletPatch, resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import TabletPanel from './TabletPanel.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  resetMockLibrary();
  resetMockTablet();
  tablet.reset();
  mockTablet.inRange = true;
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

interface Options {
  locale?: 'fr' | 'en';
  /** Keep the tablet from syncing by itself. */
  manual?: boolean;
  /** Wait for the connection. */
  connect?: boolean;
  forget?: boolean;
}

async function render({ locale = 'fr', manual = true, connect = true, forget = false }: Options = {}) {
  setLocale(locale);
  if (forget) await api.forgetTablet();
  if (manual) await api.tabletSetOptions({ autoSync: false, deleteAfterImport: true, orientation: 'landscape', button: 'newPage' });
  tablet.start();
  await settle();
  const handlers = { onclose: vi.fn(), onlive: vi.fn(), ontest: vi.fn() };
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(TabletPanel, { target, props: handlers });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  if (connect && !forget) await beat(1.5);
  await settle();
  const q = <T extends Element = HTMLElement>(selector: string) => target.querySelector<T>(selector);
  const action = (name: string) => q<HTMLButtonElement>(`button[data-action="${name}"]`);
  const click = async (el: Element | null) => {
    if (!el) throw new Error('no such element');
    (el as HTMLElement).click();
    await settle();
  };
  const type = async (input: HTMLInputElement, value: string) => {
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
  };
  return { target, q, action, click, type, ...handlers };
}

describe('tablet panel: information', () => {
  it('shows everything the tablet reported', async () => {
    const { target, q } = await render();
    expect(q('.tablet-panel')).not.toBeNull();
    const info = q('[aria-labelledby="tablet-info"]')!;
    expect(info.textContent).toContain('Informations');
    expect(info.querySelector('[data-slot="state"]')!.textContent).toContain('Connectée');
    expect(info.querySelector('[data-slot="name"]')!.textContent).toBe('Bamboo Slate');
    expect(info.querySelector('.battery')!.textContent).toContain('87 %');
    expect(info.textContent).toContain('B160805A-C170704A');
    expect(info.querySelector('[data-slot="stored-pages"]')!.textContent).toBe('3');
    expect(info.textContent).toContain('Bamboo Slate A4');
    expect(info.textContent).toContain('297 × 216 mm');
    expect(info.textContent).toMatch(/2026/);
    expect(target.textContent).not.toContain("n'a pas encore été lue");
  });

  it('shows that nothing was read yet while the tablet is out of range', async () => {
    mockTablet.inRange = false;
    const { q } = await render({ connect: false });
    const info = q('[aria-labelledby="tablet-info"]')!;
    expect(info.textContent).toContain("n'a pas encore été lue");
    expect(info.textContent).toContain('Recherche de la tablette…');
    expect(q('[data-slot="hint"]')!.textContent).toContain('Appuyez brièvement sur le bouton');
    expect(info.querySelector('.battery')).toBeNull();
    // Even then, its name and size are those of the registration.
    expect(info.textContent).toContain('Bamboo Slate');
    expect(info.textContent).toContain('297 × 216 mm');
  });

  it('shows the battery charging', async () => {
    const { q } = await render();
    mockTabletPatch({ device: { ...tablet.status!.device!, charging: true } });
    await settle();
    expect(q('.battery')!.getAttribute('aria-label')).toBe('Batterie 87 %, en charge');
  });

  it('says it is a simulated tablet only when it is', async () => {
    const { q } = await render();
    expect(q('[data-slot="simulated"]')!.textContent).toContain('Tablette simulée');
    mockTabletPatch({ simulated: false });
    await settle();
    expect(q('[data-slot="simulated"]')).toBeNull();
    expect(q('button[data-action="sim-press"]')).toBeNull();
  });

  it('closes and opens the tablet test', async () => {
    const { action, click, onclose, ontest, target } = await render();
    await click(action('test'));
    expect(ontest).toHaveBeenCalledOnce();
    const close = [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Fermer')!;
    await click(close);
    expect(onclose).toHaveBeenCalledOnce();
  });

  it('shows the service error in words, and the way forward', async () => {
    const { q } = await render();
    mockTabletPatch({ connection: 'unavailable', error: { code: 'bluetoothOff', message: '' } });
    await settle();
    expect(q('[data-slot="tablet-error"]')!.textContent).toContain('Le Bluetooth est désactivé : activez-le dans les paramètres de Windows');
    expect(q('[data-slot="state"]')!.textContent).toContain('Indisponible');
  });
});

describe('tablet panel: sync', () => {
  it('syncs on demand with a progress and a result', async () => {
    const { q, action, click } = await render();
    expect(q('[data-slot="last-sync"]')!.textContent).toContain("Aucune synchronisation depuis le lancement d'Ardoise");
    expect(action('sync')!.disabled).toBe(false);
    await click(action('sync'));

    expect(action('sync')).toBeNull();
    const progress = q('[data-slot="sync-progress"]')!;
    expect(progress.textContent).toContain('Page 1 sur 3');
    expect(progress.querySelector<HTMLProgressElement>('progress')!.max).toBe(3);
    expect(action('cancel-sync')).not.toBeNull();
    expect(action('live')!.disabled).toBe(true);
    await beat(0.6);
    expect(q('[data-slot="sync-progress"]')!.textContent).toContain('Page 2 sur 3');

    await beat(2);
    expect(action('sync')).not.toBeNull();
    expect(q('[data-slot="sync-progress"]')).toBeNull();
    const last = q('[data-slot="last-sync"]')!.textContent!;
    expect(last).toContain('Dernière synchronisation');
    expect(last).toContain('3 notes enregistrées.');
    expect(last).toContain('3 pages effacées de la tablette.');
    expect(q('[data-slot="stored-pages"]')!.textContent).toBe('0');
    expect(mockLibrary().notes.size).toBe(6);
    expect(action('live')!.disabled).toBe(false);
  });

  it('cancels the sync between two pages', async () => {
    const { q, action, click } = await render();
    await click(action('sync'));
    await beat(0.7);
    await click(action('cancel-sync'));
    await beat(2);
    const last = q('[data-slot="last-sync"]')!.textContent!;
    expect(last).toContain('2 notes enregistrées.');
    expect(last).toContain('2 pages effacées de la tablette.');
    expect(q('[data-slot="stored-pages"]')!.textContent).toBe('1');
  });

  it('words one page, and no page', async () => {
    const { q } = await render();
    mockTabletPatch({ lastSync: { at: Date.now(), saved: ['a'], deleted: 1, error: null } });
    await settle();
    expect(q('[data-slot="last-sync"]')!.textContent).toContain('1 note enregistrée.');
    expect(q('[data-slot="last-sync"]')!.textContent).toContain('1 page effacée de la tablette.');
    mockTabletPatch({ lastSync: { at: Date.now(), saved: [], deleted: 0, error: null } });
    await settle();
    expect(q('[data-slot="last-sync"]')!.textContent).toContain('Aucune nouvelle page.');
    expect(q('[data-slot="last-sync"]')!.textContent).not.toContain('effacée');
  });

  it('shows a failed sync as a failure, even once the error banner is gone', async () => {
    const { q } = await render();
    // The first hardware session: the tablet refused the sync, nothing was downloaded.
    const refused = { code: 'syncRefused', message: 'the tablet refused the command (InvalidState)' };
    mockTabletPatch({ lastSync: { at: Date.now(), saved: [], deleted: 0, error: refused }, error: null });
    await settle();
    const last = q('[data-slot="last-sync"]')!;
    expect(last.getAttribute('data-failed')).toBe('true');
    expect(last.getAttribute('role')).toBe('alert');
    expect(last.textContent).toContain('a échoué');
    expect(last.textContent).toContain('la tablette a refusé la synchronisation (the tablet refused the command (InvalidState))');
    expect(last.textContent).toContain('Les pages non reçues restent sur la tablette');
    expect(last.textContent).not.toContain('Aucune nouvelle page.');
    // Some pages saved before the failure are said too.
    mockTabletPatch({
      lastSync: { at: Date.now(), saved: ['a'], deleted: 1, error: { code: 'libraryAccess', message: 'disk full' } },
    });
    await settle();
    expect(q('[data-slot="last-sync"]')!.textContent).toContain('1 note enregistrée.');
    expect(q('[data-slot="last-sync"]')!.textContent).toContain('disk full');
    // The next sync that works replaces it.
    mockTabletPatch({ lastSync: { at: Date.now(), saved: ['a', 'b'], deleted: 2, error: null } });
    await settle();
    expect(q('[data-slot="last-sync"]')!.getAttribute('data-failed')).toBeNull();
    expect(q('[data-slot="last-sync"]')!.textContent).toContain('2 notes enregistrées.');
  });

  it('says that only the oldest page can be read when nothing is deleted', async () => {
    const { q, action, click } = await render();
    await api.tabletSetOptions({ autoSync: false, deleteAfterImport: false, orientation: 'landscape', button: 'newPage' });
    await settle();
    // Before any sync, nothing was read: no message yet.
    expect(q('[data-slot="pages-waiting"]')).toBeNull();

    await click(action('sync'));
    await beat(3);
    expect(q('[data-slot="last-sync"]')!.textContent).toContain('1 note enregistrée.');
    expect(q('[data-slot="stored-pages"]')!.textContent).toBe('3');
    const waiting = q('[data-slot="pages-waiting"]')!;
    expect(waiting.textContent).toContain('3 pages attendent sur la tablette');
    expect(waiting.textContent).toContain("seule la plus ancienne peut être lue tant qu'elle n'est pas effacée");
    expect(waiting.textContent).toContain('« Effacer de la tablette après import »');

    // The next sync finds nothing new, and the message stays.
    await click(action('sync'));
    await beat(3);
    expect(q('[data-slot="last-sync"]')!.textContent).toContain('Aucune nouvelle page.');
    expect(q('[data-slot="pages-waiting"]')).not.toBeNull();

    // Turning deletion on hides it: the next sync gets the pages.
    await api.tabletSetOptions({ autoSync: false, deleteAfterImport: true, orientation: 'landscape', button: 'newPage' });
    await settle();
    expect(q('[data-slot="pages-waiting"]')).toBeNull();
  });

  it('says nothing about waiting pages when at most one is left, or when pages are deleted', async () => {
    const { q, action, click } = await render();
    await click(action('sync'));
    await beat(4);
    expect(q('[data-slot="pages-waiting"]')).toBeNull();

    await api.tabletSetOptions({ autoSync: false, deleteAfterImport: false, orientation: 'landscape', button: 'newPage' });
    mockTabletPatch({
      lastSync: { at: Date.now(), saved: ['a'], deleted: 0, error: null },
      device: { ...tablet.status!.device!, storedPages: 1 },
    });
    await settle();
    expect(q('[data-slot="pages-waiting"]')).toBeNull();
    mockTabletPatch({ device: { ...tablet.status!.device!, storedPages: 2 } });
    await settle();
    expect(q('[data-slot="pages-waiting"]')!.textContent).toContain('2 pages attendent');
  });

  it('says it in English too', async () => {
    const { q, action, click } = await render({ locale: 'en' });
    await api.tabletSetOptions({ autoSync: false, deleteAfterImport: false, orientation: 'landscape', button: 'newPage' });
    await settle();
    await click(action('sync'));
    await beat(3);
    const text = q('[data-slot="pages-waiting"]')!.textContent!;
    expect(text).toContain('3 pages are waiting on the tablet');
    expect(text).toContain('only the oldest one can be read until it is deleted');
    expect(text).toContain('"Delete from the tablet after import"');
  });

  it('waits for the tablet when it is not connected yet', async () => {
    mockTablet.inRange = false;
    const { q, action, click } = await render({ connect: false });
    expect(action('sync')!.disabled).toBe(false);
    await click(action('sync'));
    expect(q('[data-slot="hint"]')!.textContent).toContain('La synchronisation démarrera dès que la tablette sera connectée');
  });

  it('cannot sync during live mode or without a connection to wait for', async () => {
    const { action } = await render();
    mockTabletPatch({ connection: 'live' });
    await settle();
    expect(action('sync')!.disabled).toBe(true);
    mockTabletPatch({ connection: 'unavailable' });
    await settle();
    expect(action('sync')!.disabled).toBe(true);
  });

  it('shows why a sync was refused', async () => {
    const { q, action, click } = await render();
    vi.spyOn(api, 'tabletSync').mockRejectedValue({ code: 'tabletBusy', message: '' });
    await click(action('sync'));
    expect(q('[data-slot="tablet-error"]')!.textContent).toContain('La tablette est occupée');
    const dismiss = q('[data-slot="tablet-error"] button')!;
    await click(dismiss);
    expect(q('[data-slot="tablet-error"]')).toBeNull();
  });

  it('starts live mode', async () => {
    const { action, click, onlive } = await render();
    await click(action('live'));
    expect(onlive).toHaveBeenCalledOnce();
  });
});

describe('tablet panel: options', () => {
  const option = (target: HTMLElement, name: string, value?: string) =>
    target.querySelector<HTMLInputElement>(`input[data-option="${name}"]${value ? `[value="${value}"]` : ''}`)!;

  it('has deleting after import on by default, and says when the page is deleted', async () => {
    const { target } = await render({ manual: false });
    expect(option(target, 'deleteAfterImport').checked).toBe(true);
    expect(option(target, 'autoSync').checked).toBe(true);
    expect(target.textContent).toContain('Effacer de la tablette après import');
    expect(target.textContent).toContain("qu'après l'écriture et la vérification de son fichier");
    expect(option(target, 'deleteAfterImport').getAttribute('role')).toBe('switch');
    const hint = target.querySelector(`#${option(target, 'deleteAfterImport').getAttribute('aria-describedby')}`);
    expect(hint?.textContent).toContain('si quelque chose échoue, elle reste sur la tablette');
    // The protocol only hands over the oldest page: the option is how the others become readable.
    expect(hint?.textContent).toContain('ne livre que sa page la plus ancienne');
  });

  it('saves each option as soon as it changes', async () => {
    const save = vi.spyOn(api, 'tabletSetOptions');
    const { target, click } = await render();
    await click(option(target, 'deleteAfterImport'));
    await click(option(target, 'autoSync'));
    await click(option(target, 'button', 'newLayer'));
    await click(option(target, 'orientation', 'landscapeFlipped'));
    expect(save).toHaveBeenLastCalledWith({ autoSync: true, deleteAfterImport: false, orientation: 'landscapeFlipped', button: 'newLayer' });
    expect((await api.tabletStatus()).options).toEqual({
      autoSync: true,
      deleteAfterImport: false,
      orientation: 'landscapeFlipped',
      button: 'newLayer',
    });
    expect(option(target, 'orientation', 'landscapeFlipped').checked).toBe(true);
    expect(option(target, 'button', 'newLayer').checked).toBe(true);
    expect(option(target, 'deleteAfterImport').checked).toBe(false);
  });

  it('puts a switch back when the change is refused', async () => {
    const { target, q, click } = await render();
    vi.spyOn(api, 'tabletSetOptions').mockRejectedValue({ code: 'tablet', message: 'disk full' });
    await click(option(target, 'deleteAfterImport'));
    expect(option(target, 'deleteAfterImport').checked).toBe(true);
    expect(q('[data-slot="tablet-error"]')!.textContent).toContain('disk full');
  });

  it('speaks English', async () => {
    const { target } = await render({ locale: 'en' });
    expect(target.textContent).toContain('Delete from the tablet after import');
    expect(target.textContent).toContain('only once its file has been written to the library and checked');
    expect(target.textContent).toContain('hands over only its oldest page');
    expect(target.textContent).toContain('Tablet button in live mode');
    expect(target.textContent).toContain('Landscape upside down, button at the top');
  });
});

describe('tablet panel: tablet settings', () => {
  const nameField = (target: HTMLElement) => target.querySelector<HTMLInputElement>('#tablet-name')!;

  it('renames the tablet', async () => {
    const { q, action, click, type, target } = await render();
    expect(nameField(target).value).toBe('Bamboo Slate');
    expect(action('rename')!.disabled).toBe(true);
    await type(nameField(target), '  Classe 3B ');
    expect(action('rename')!.disabled).toBe(false);
    await click(action('rename'));
    expect(q('[data-slot="name"]')!.textContent).toBe('Classe 3B');
    expect(nameField(target).value).toBe('Classe 3B');
    expect(target.textContent).toContain('Tablette renommée.');
    expect((await api.tabletInfo())?.name).toBe('Classe 3B');
  });

  it('refuses a name of the wrong length or with accents, without asking the tablet', async () => {
    const rename = vi.spyOn(api, 'tabletRename');
    const { q, action, click, type, target } = await render();
    for (const bad of ['', '   ', 'Un nom beaucoup trop long', 'Éléphant']) {
      await type(nameField(target), bad);
      await click(action('rename'));
      expect(q('[role="alert"].error')?.textContent, bad).toContain('Le nom doit faire de 1 à 15 caractères');
      expect(nameField(target).getAttribute('aria-invalid')).toBe('true');
    }
    expect(rename).not.toHaveBeenCalled();
    // Typing again lifts the message.
    await type(nameField(target), 'Salle 12');
    expect(q('[role="alert"].error')).toBeNull();
    await click(action('rename'));
    expect(rename).toHaveBeenCalledWith('Salle 12');
  });

  it('says so when the tablet cannot be reached to rename it', async () => {
    mockTablet.inRange = false;
    const { q, action, click, type, target } = await render({ connect: false });
    await type(nameField(target), 'Salle 12');
    await click(action('rename'));
    expect(q('[data-slot="tablet-error"]')!.textContent).toContain("La tablette n'est pas connectée");
    expect(q('[data-slot="name"]')!.textContent).toBe('Bamboo Slate');
  });

  it('sets the clock and shows the drift left', async () => {
    const { q, action, click } = await render();
    expect(q('[data-slot="time-result"]')).toBeNull();
    mockTablet.drift = 0.4;
    await click(action('set-time'));
    expect(q('[data-slot="time-result"]')!.textContent).toContain('Heure réglée (écart restant : 0,4 s).');
  });

  it('shows the drift in English with a decimal point', async () => {
    const { q, action, click } = await render({ locale: 'en' });
    mockTablet.drift = 1.25;
    await click(action('set-time'));
    expect(q('[data-slot="time-result"]')!.textContent).toContain('Clock set (remaining drift: 1.3 s).');
  });

  it('forgets the tablet only after a confirmation', async () => {
    const { q, action, click, target } = await render();
    await click(action('forget'));
    const dialog = q('[role="alertdialog"]')!;
    expect(dialog.textContent).toContain('Oublier la tablette « Bamboo Slate » ?');
    expect(await api.tabletInfo()).not.toBeNull();

    await click([...dialog.querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Annuler')!);
    expect(q('[role="alertdialog"]')).toBeNull();
    expect(await api.tabletInfo()).not.toBeNull();

    await click(action('forget'));
    await click(action('forget-yes'));
    expect(await api.tabletInfo()).toBeNull();
    // The panel now offers to register one.
    expect(action('register')).not.toBeNull();
    expect(target.textContent).toContain("Aucune tablette n'est enregistrée.");
    expect(action('sync')).toBeNull();
  });
});

describe('tablet panel: no tablet registered', () => {
  it('offers to register the Slate or to add the Xiaomi tablet, and nothing else of the tablet', async () => {
    const { q, action, target } = await render({ forget: true });
    expect(action('register')).not.toBeNull();
    expect(action('add-blackboard')).not.toBeNull();
    expect(target.textContent).toContain('Enregistrez votre Bamboo Slate');
    expect(target.textContent).toContain('ajoutez votre tablette Xiaomi');
    for (const name of ['sync', 'live', 'rename', 'set-time', 'forget']) expect(action(name), name).toBeNull();
    expect(q('[data-slot="tablet-options"]')).toBeNull();
    // The test of the tablet stays reachable.
    expect(action('test')).not.toBeNull();
  });

  it('opens the wizard on its warning', async () => {
    const { q, action, click } = await render({ forget: true });
    expect(q('.register-wizard')).toBeNull();
    await click(action('register'));
    expect(q('.register-wizard [data-step]')?.getAttribute('data-step')).toBe('warning');
  });

  it('shows the new tablet once the wizard is through', async () => {
    const { q, action, click } = await render({ forget: true });
    await click(action('register'));
    await click(q('[data-action="continue"]'));
    await beat(4);
    expect(q('.register-wizard [data-step]')?.getAttribute('data-step')).toBe('done');
    await beat(1);
    await click(q('[data-action="finish"]'));
    expect(q('.register-wizard')).toBeNull();
    expect(action('register')).toBeNull();
    expect(action('sync')).not.toBeNull();
    expect(q('[data-slot="name"]')!.textContent).toBe('Bamboo Slate');
  });

  it('says so in English', async () => {
    const { target } = await render({ forget: true, locale: 'en' });
    expect(target.textContent).toContain('No tablet is registered.');
    expect(target.textContent).toContain('Register the Bamboo Slate');
    expect(target.textContent).toContain('Add the Xiaomi tablet');
  });

  it('shows a message while the state is not known yet', async () => {
    vi.spyOn(api, 'tabletSubscribe').mockImplementation(() => new Promise(() => {}));
    const { q } = await render({ connect: false });
    expect(q('[data-slot="loading"]')!.textContent).toContain("Lecture de l'état de la tablette");
  });
});
