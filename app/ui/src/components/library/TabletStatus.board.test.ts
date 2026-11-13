// @vitest-environment jsdom
// The sidebar's tablet block with the Xiaomi blackboard (M18).
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { resetMockLibrary } from '../../lib/mock-data';
import { mockTablet, resetMockTablet } from '../../lib/mock-tablet';
import { tablet } from '../../lib/tablet-state.svelte';
import TabletStatus from './TabletStatus.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  resetMockLibrary();
  resetMockTablet();
  tablet.reset();
  mockTablet.inRange = false;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
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

async function render(locale: 'fr' | 'en' = 'fr') {
  setLocale(locale);
  tablet.start();
  await settle();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(TabletStatus, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  return () => target.querySelector<HTMLButtonElement>('.tablet-status')!;
}

describe('sidebar tablet block with the blackboard', () => {
  it('shows both names while waiting, then the one that is connected, with its battery', async () => {
    api.tabletSetOptions({ autoSync: false, deleteAfterImport: true, orientation: 'landscape', button: 'newPage' });
    const root = await render();
    await api.tabletFindBlackboards();
    await beat(1);
    expect(root().textContent).toContain("Recherche d'une tablette Xiaomi…");
    await api.tabletAddBlackboard(tablet.status!.boardSearch!.found[0].address);
    await settle();
    expect(root().textContent).toContain('Bamboo Slate · Tablette Xiaomi');
    await beat(1.5);
    expect(root().dataset.connection).toBe('connected');
    expect(root().querySelector('.tablet-name')!.textContent).toBe('Tablette Xiaomi');
    expect(root().querySelector('.battery')!.textContent).toContain('90 %');
  });

  it('is not « none » when only the blackboard is known', async () => {
    await api.forgetTablet();
    const root = await render();
    expect(root().textContent).toContain('Aucune tablette enregistrée');
    await api.tabletFindBlackboards();
    await beat(1);
    await api.tabletAddBlackboard(tablet.status!.boardSearch!.found[0].address);
    await beat(2);
    expect(root().textContent).not.toContain('Aucune tablette enregistrée');
    expect(root().textContent).toContain('Tablette Xiaomi');
    // It syncs by itself on connecting: its three pages.
    expect(root().textContent).toContain('Synchronisation…');
    expect(root().textContent).toContain('Page 1 sur 3');
    await beat(4);
    expect(root().textContent).toContain('Connectée');
  });

  it('speaks English', async () => {
    const root = await render('en');
    await api.tabletFindBlackboards();
    await beat(1);
    await api.tabletAddBlackboard(tablet.status!.boardSearch!.found[0].address);
    await settle();
    expect(root().textContent).toContain('Bamboo Slate · Xiaomi tablet');
  });
});
