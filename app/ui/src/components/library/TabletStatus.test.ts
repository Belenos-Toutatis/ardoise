// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockTablet, mockTabletPatch, resetMockTablet } from '../../lib/mock-tablet';
import type { Connection } from '../../lib/tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import TabletStatus from './TabletStatus.svelte';

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
  await tick();
};

async function render(locale: 'fr' | 'en' = 'fr', props: { open?: boolean; onopen?: () => void } = {}) {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(TabletStatus, { target, props });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const root = () => target.querySelector<HTMLButtonElement>('.tablet-status')!;
  return { target, root };
}

describe('sidebar tablet block', () => {
  it('shows the name and that the tablet is being looked for', async () => {
    const { root } = await render();
    expect(root().dataset.slot).toBe('tablet-status');
    expect(root().dataset.connection).toBe('searching');
    expect(root().textContent).toContain('Bamboo Slate');
    expect(root().textContent).toContain('Recherche de la tablette…');
    // Nothing is known of the battery before the first connection.
    expect(root().querySelector('.battery')).toBeNull();
    expect(root().querySelector('progress')).toBeNull();
  });

  it('shows the connection and the battery once connected', async () => {
    await api.tabletSetOptions({ autoSync: false, deleteAfterImport: true, orientation: 'landscape', button: 'newPage' });
    const { root } = await render();
    await beat(1.5);
    expect(root().dataset.connection).toBe('connected');
    expect(root().textContent).toContain('Connectée');
    const battery = root().querySelector<HTMLElement>('.battery')!;
    expect(battery.textContent).toContain('87 %');
    expect(battery.getAttribute('aria-label')).toBe('Batterie 87 %');
    expect(battery.dataset.charging).toBe('false');
  });

  it('shows charging and a nearly empty battery', async () => {
    const { root } = await render();
    await beat(1.5);
    const device = tablet.status!.device!;
    mockTabletPatch({ device: { ...device, batteryPercent: 9, charging: true } });
    await settle();
    let battery = root().querySelector<HTMLElement>('.battery')!;
    expect(battery.getAttribute('aria-label')).toBe('Batterie 9 %, en charge');
    expect(battery.dataset.charging).toBe('true');
    expect(battery.querySelector('.bolt')).not.toBeNull();
    expect(battery.classList.contains('low')).toBe(false);

    mockTabletPatch({ device: { ...device, batteryPercent: 9, charging: false } });
    await settle();
    battery = root().querySelector<HTMLElement>('.battery')!;
    expect(battery.querySelector('.bolt')).toBeNull();
    expect(battery.classList.contains('low')).toBe(true);
  });

  it('shows a progress bar and the page being received while syncing', async () => {
    const { root } = await render();
    await beat(2);
    expect(root().dataset.connection).toBe('syncing');
    expect(root().textContent).toContain('Synchronisation…');
    const bar = root().querySelector<HTMLProgressElement>('progress')!;
    expect(bar.max).toBe(3);
    expect(bar.value).toBe(1);
    expect(root().textContent).toContain('Page 1 sur 3');
    await beat(0.6);
    expect(root().querySelector<HTMLProgressElement>('progress')!.value).toBe(2);
    expect(root().textContent).toContain('Page 2 sur 3');
    await beat(3);
    expect(root().querySelector('progress')).toBeNull();
    expect(root().dataset.connection).toBe('connected');
  });

  it('names every state, in both languages', async () => {
    const words: Record<Connection, [string, string]> = {
      unavailable: ['Indisponible', 'Unavailable'],
      notRegistered: ['Non enregistrée', 'Not registered'],
      registering: ['Enregistrement…', 'Registering…'],
      searching: ['Recherche de la tablette…', 'Looking for the tablet…'],
      connecting: ['Connexion…', 'Connecting…'],
      connected: ['Connectée', 'Connected'],
      syncing: ['Synchronisation…', 'Syncing…'],
      live: ['Mode direct', 'Live mode'],
      adding: ["Recherche d'une tablette Xiaomi…", 'Looking for a Xiaomi tablet…'],
    };
    for (const [index, locale] of (['fr', 'en'] as const).entries()) {
      const { root } = await render(locale);
      for (const [connection, texts] of Object.entries(words) as [Connection, [string, string]][]) {
        mockTabletPatch({ connection });
        await settle();
        expect(root().dataset.connection).toBe(connection);
        if (connection !== 'notRegistered') expect(root().textContent, `${locale} ${connection}`).toContain(texts[index]);
      }
      cleanup?.();
      cleanup = null;
      tablet.reset();
      resetMockTablet();
      mockTablet.inRange = true;
    }
  });

  it('does not break when no tablet is registered', async () => {
    await api.forgetTablet();
    const { root } = await render();
    expect(root().dataset.connection).toBe('notRegistered');
    expect(root().textContent).toContain('Aucune tablette enregistrée');
    expect(root().querySelector('.battery')).toBeNull();
    expect(root().querySelector('progress')).toBeNull();
    expect(root().querySelector('.dot')).toBeNull();
  });

  it('says so in English when no tablet is registered', async () => {
    await api.forgetTablet();
    const { root } = await render('en');
    expect(root().textContent).toContain('No tablet registered');
  });

  it('forgets the battery of a tablet that was forgotten', async () => {
    const { root } = await render();
    await beat(1.5);
    expect(root().querySelector('.battery')).not.toBeNull();
    await tablet.forget();
    await settle();
    expect(root().querySelector('.battery')).toBeNull();
    expect(root().textContent).toContain('Aucune tablette enregistrée');
  });

  it('is a button that opens and closes the panel', async () => {
    const onopen = vi.fn();
    const { root } = await render('fr', { onopen });
    expect(root().getAttribute('aria-expanded')).toBe('false');
    expect(root().title).toBe('Ouvrir ou fermer le panneau de la tablette');
    root().click();
    expect(onopen).toHaveBeenCalledOnce();
  });

  it('shows the panel as open', async () => {
    const { root } = await render('fr', { open: true });
    expect(root().getAttribute('aria-expanded')).toBe('true');
  });

  it('says the state is unavailable when the service cannot be reached', async () => {
    vi.spyOn(api, 'tabletSubscribe').mockRejectedValue({ code: 'tablet', message: 'nope' });
    const { root } = await render();
    expect(root().dataset.connection).toBe('unavailable');
    expect(root().textContent).toContain('Aucune tablette enregistrée');
  });
});
