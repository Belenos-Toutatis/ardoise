// @vitest-environment jsdom
// The settings' tablet section with the Xiaomi blackboard (M18).
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { resetMockLibrary } from '../lib/mock-data';
import { mockTablet, resetMockTablet } from '../lib/mock-tablet';
import { mockVideo, resetMockVideo } from '../lib/mock-video';
import { tablet } from '../lib/tablet-state.svelte';
import Settings from './Settings.svelte';

vi.mock('./narration/MicrophonePicker.svelte', () => ({
  default: (anchor: ChildNode) => {
    const marker = document.createElement('div');
    anchor.before(marker);
  },
}));

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
  shapes: { tolerance: 3, minSizeMm: 10, snapAngle: true, snapAngleDeg: 4, snapRound: true, snapRoundPercent: 10 },
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  vi.useFakeTimers();
  resetMockLibrary();
  resetMockTablet();
  resetMockVideo();
  tablet.reset();
  mockTablet.inRange = false;
  void mockVideo;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  vi.useRealTimers();
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    await vi.advanceTimersByTimeAsync(0);
    await tick();
  }
}

async function render(locale: 'fr' | 'en', { boardOnly = false, ontest }: { boardOnly?: boolean; ontest?: () => void } = {}) {
  setLocale(locale);
  if (boardOnly) await api.forgetTablet();
  tablet.start();
  await settle();
  await api.tabletFindBlackboards();
  await vi.advanceTimersByTimeAsync(mockTablet.stepMs);
  await api.tabletAddBlackboard(tablet.status!.boardSearch!.found[0].address);
  await settle();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(Settings, { target, props: { app, onchange: vi.fn(), onclose: vi.fn(), ontest } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  await settle();
  const section = target.querySelector('[aria-labelledby="settings-tablet"]')!;
  const button = (text: string) => {
    const found = [...section.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
    if (!found) throw new Error(`no button "${text}"`);
    return found;
  };
  return { section, button };
}

describe('settings: the two tablets', () => {
  it('lists the Slate and the Xiaomi tablet, each with its facts and its own forget', async () => {
    const { section } = await render('fr');
    const known = [...section.querySelectorAll('[data-slot="known-tablet"]')];
    expect(known.map((el) => el.getAttribute('data-kind'))).toEqual(['slate', 'blackboard']);
    expect(known[0].textContent).toContain('AA:BB:CC:DD:EE:FF');
    expect(known[0].textContent).toContain('Enregistrée le');
    expect(known[1].textContent).toContain('Tablette Xiaomi');
    expect(known[1].textContent).toContain('00:00:5E:00:53:01');
    expect(known[1].textContent).toContain('Ajoutée le');
    expect(known[1].textContent).toContain('205 × 272 mm');
    for (const el of known) expect(el.querySelectorAll('button[data-action="forget"]')).toHaveLength(1);
  });

  it('forgets the Xiaomi tablet only, after a confirmation', async () => {
    const { section, button } = await render('fr');
    (section.querySelector('[data-kind="blackboard"] button[data-action="forget"]') as HTMLElement).click();
    await settle();
    expect(section.querySelector('[role="alertdialog"]')?.textContent).toContain('Oublier la tablette Xiaomi ?');
    button('Annuler').click();
    await settle();
    expect(section.querySelector('[role="alertdialog"]')).toBeNull();
    expect(tablet.known).toHaveLength(2);

    (section.querySelector('[data-kind="blackboard"] button[data-action="forget"]') as HTMLElement).click();
    await settle();
    button("Oui, l'oublier").click();
    await settle();
    expect(tablet.known.map((k) => k.kind)).toEqual(['slate']);
    expect(await api.tabletInfo()).not.toBeNull();
  });

  it('shows the blackboard alone, without the Slate options, and without the guided test', async () => {
    const ontest = vi.fn();
    const { section } = await render('fr', { boardOnly: true, ontest });
    expect(section.textContent).not.toContain('Aucune tablette');
    expect(section.textContent).toContain('Tablette Xiaomi');
    expect(section.querySelector('[data-option="autoSync"]')).not.toBeNull();
    expect(section.querySelector('[data-option="orientation"]')).toBeNull();
    expect(section.querySelector('[data-option="button"]')).toBeNull();
    expect(section.textContent).toContain('Tablette Xiaomi : sans cette option');
    expect([...section.querySelectorAll('button')].some((b) => b.textContent?.includes('Tester la tablette'))).toBe(false);
    // The Slate can still be registered from here.
    expect(section.querySelector('[data-action="register"]')).not.toBeNull();
  });

  it('sets one tablet at a time, as the tablet panel does: the Slate to begin with, the four positions of the Xiaomi once it is chosen', async () => {
    const save = vi.spyOn(api, 'tabletSetOptions');
    const { section } = await render('fr');
    const select = section.querySelector<HTMLSelectElement>('[data-slot="settings-tablet"]')!;
    expect([...select.options].map((o) => o.textContent)).toEqual(['Bamboo Slate', 'Tablette Xiaomi']);
    expect(select.value).toBe('slate');
    expect(section.querySelectorAll('[data-option="orientation"]')).toHaveLength(4);
    expect(section.querySelector('[data-option="boardPosition"]')).toBeNull();
    // Its name and its clock stay in the tablet panel.
    expect(section.querySelector('#tablet-name')).toBeNull();
    expect(section.querySelector('[data-action="set-time"]')).toBeNull();

    select.value = 'blackboard';
    select.dispatchEvent(new Event('change', { bubbles: true }));
    await settle();
    expect(section.querySelector('[data-option="orientation"]')).toBeNull();
    expect(section.querySelector('[data-option="button"]')).toBeNull();
    const positions = [...section.querySelectorAll<HTMLInputElement>('[data-option="boardPosition"]')];
    expect(positions.map((r) => r.value)).toEqual(['buttonsBottom', 'buttonsTop', 'buttonsRight', 'buttonsLeft']);
    expect(positions[0].checked).toBe(true);
    expect(section.textContent).toContain('Ces deux réglages valent pour toutes vos tablettes.');
    expect(section.textContent).toContain('Tablette Xiaomi : sans cette option');

    positions[3].click();
    await settle();
    expect(save).toHaveBeenLastCalledWith(expect.objectContaining({ boardPosition: 'buttonsLeft', orientation: 'landscape' }));
    expect((await api.tabletStatus()).options.boardPosition).toBe('buttonsLeft');
  });

  it('can add the Xiaomi tablet from the settings, in English too', async () => {
    setLocale('en');
    tablet.start();
    await settle();
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(Settings, { target, props: { app, onchange: vi.fn(), onclose: vi.fn() } });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    await settle();
    const add = target.querySelector<HTMLElement>('[data-action="add-blackboard"]')!;
    expect(add.textContent?.trim()).toBe('Add the Xiaomi tablet');
    add.click();
    await settle();
    expect(target.querySelector('.add-blackboard [data-step="intro"]')).not.toBeNull();
  });
});
