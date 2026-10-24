// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../lib/api';
import { setLocale } from '../lib/i18n.svelte';
import { resetMockLibrary } from '../lib/mock-data';
import { resetMockTablet } from '../lib/mock-tablet';
import { mockVideo, resetMockVideo, setMockOpenH264 } from '../lib/mock-video';
import { tablet } from '../lib/tablet-state.svelte';
import Settings from './Settings.svelte';

// The microphone picker belongs to the narration work package: its props are the contract, and
// the settings are tested against a stand-in so that its own controls do not count here.
const picker = vi.hoisted(() => ({
  props: null as null | { device: string | null; monitor?: boolean; onchange: (device: string | null) => void },
}));
vi.mock('./narration/MicrophonePicker.svelte', () => ({
  default: (anchor: ChildNode, props: NonNullable<typeof picker.props>) => {
    picker.props = props;
    const marker = document.createElement('div');
    marker.className = 'microphone-picker-stand-in';
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
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockLibrary();
  resetMockTablet();
  resetMockVideo();
  tablet.reset();
  picker.props = null;
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function render(locale: 'fr' | 'en', ontest?: () => void, shown: AppState = app) {
  setLocale(locale);
  const target = document.createElement('div');
  document.body.append(target);
  const onchange = vi.fn();
  const component = mount(Settings, { target, props: { app: shown, onchange, onclose: vi.fn(), ontest } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const button = (text: string) => {
    const found = [...target.querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
    if (!found) throw new Error(`no button "${text}"`);
    return found;
  };
  await settle();
  return { target, button, onchange };
}

describe('settings: tablet section', () => {
  it('shows the registered tablet', async () => {
    const { target } = await render('fr');
    const section = target.querySelector('[aria-labelledby="settings-tablet"]')!;
    expect(section.querySelector('h3')?.textContent).toBe('Tablette');
    expect(section.textContent).toContain('Bamboo Slate');
    expect(section.textContent).toContain('AA:BB:CC:DD:EE:FF');
    expect(section.textContent).toContain('297 × 216 mm');
    expect(section.textContent).toMatch(/12 oct\. 2026/);
  });

  it('forgets the tablet only after a confirmation', async () => {
    const { target, button } = await render('fr');
    button('Oublier la tablette').click();
    await settle();
    expect(target.querySelector('[role="alertdialog"]')?.textContent).toContain('Oublier la tablette « Bamboo Slate » ?');
    expect(await api.tabletInfo()).not.toBeNull();

    button('Annuler').click();
    await settle();
    expect(target.querySelector('[role="alertdialog"]')).toBeNull();
    expect(await api.tabletInfo()).not.toBeNull();

    button('Oublier la tablette').click();
    await settle();
    button("Oui, l'oublier").click();
    await settle();
    expect(await api.tabletInfo()).toBeNull();
    expect(target.textContent).toContain("Aucune tablette n'est enregistrée.");
    expect(target.textContent).not.toContain('Oublier la tablette');
  });

  it('says so when no tablet is registered, in English', async () => {
    await api.forgetTablet();
    const { target } = await render('en');
    expect(target.textContent).toContain('No tablet is registered.');
    expect(target.textContent).not.toContain('Forget the tablet');
  });

  it('keeps its English labels', async () => {
    const { target, button } = await render('en');
    expect(target.textContent).toContain('Registered on');
    button('Forget the tablet').click();
    await settle();
    expect(target.textContent).toContain('Forget the tablet “Bamboo Slate”?');
  });

  it('opens the tablet test', async () => {
    const ontest = vi.fn();
    const { button } = await render('fr', ontest);
    button('Tester la tablette…').click();
    expect(ontest).toHaveBeenCalledOnce();
  });

  it('shows the tablet options, delete-after-import on by default, and saves a change', async () => {
    const { target } = await render('fr');
    const section = target.querySelector('[aria-labelledby="settings-tablet"]')!;
    const option = (name: string, value?: string) =>
      section.querySelector<HTMLInputElement>(`input[data-option="${name}"]${value ? `[value="${value}"]` : ''}`)!;
    expect(section.textContent).toContain('Effacer de la tablette après import');
    expect(section.textContent).toContain("qu'après l'écriture et la vérification de son fichier");
    expect(option('autoSync').checked).toBe(true);
    expect(option('deleteAfterImport').checked).toBe(true);
    expect(option('hand', 'right').checked).toBe(true);
    expect(option('button', 'newPage').checked).toBe(true);

    option('deleteAfterImport').click();
    option('hand', 'left').click();
    await settle();
    const saved = (await api.tabletStatus()).options;
    expect(saved).toEqual({ autoSync: true, deleteAfterImport: false, hand: 'left', button: 'newPage' });
    expect(option('deleteAfterImport').checked).toBe(false);
    expect(option('hand', 'left').checked).toBe(true);
  });

  it('shows no tablet options without a tablet', async () => {
    await api.forgetTablet();
    const { target } = await render('en');
    expect(target.querySelector('[data-slot="tablet-options"]')).toBeNull();
  });

  it('speaks English in the tablet options', async () => {
    const { target } = await render('en');
    expect(target.textContent).toContain('Delete from the tablet after import');
    expect(target.textContent).toContain('Left-handed');
  });

  it('still offers language, theme and library', async () => {
    const { target } = await render('fr');
    expect(target.querySelectorAll('select').length).toBe(2);
    expect(target.textContent).toContain('C:\\demo\\Ardoise');
  });
});

describe('settings: automatic export', () => {
  afterEach(async () => {
    await api.setAutoExport({ format: 'off', folder: 'Exports' });
  });

  const section = (target: HTMLElement) => target.querySelector<HTMLElement>('[aria-labelledby="settings-auto-export"]')!;
  const radio = (target: HTMLElement, label: string) => {
    const found = [...section(target).querySelectorAll<HTMLLabelElement>('label.choice')].find((l) => l.textContent?.trim() === label);
    if (!found) throw new Error(`no choice "${label}"`);
    return found.querySelector('input')!;
  };
  const folder = (target: HTMLElement) => section(target).querySelector<HTMLInputElement>('input[type="text"]')!;
  const type = async (input: HTMLInputElement, value: string) => {
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    await settle();
  };
  const save = (target: HTMLElement) =>
    [...section(target).querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Enregistrer')!;

  it('explains the feature and shows the current choice', async () => {
    const { target } = await render('fr');
    expect(section(target).querySelector('h3')?.textContent).toBe('Export automatique après synchronisation');
    expect(section(target).textContent).toContain('application Nextcloud');
    expect(radio(target, 'Désactivé').checked).toBe(true);
    expect(radio(target, 'PDF').checked).toBe(false);
    expect(folder(target).value).toBe('Exports');
    expect(save(target).disabled).toBe(true);
  });

  it('saves the format and folder, reports the new state, and confirms', async () => {
    const spy = vi.spyOn(api, 'setAutoExport');
    const { target, onchange } = await render('fr');
    radio(target, 'PDF').click();
    await settle();
    await type(folder(target), ' /Téléphone\\PDF/ ');
    expect(save(target).disabled).toBe(false);
    save(target).click();
    await settle();
    expect(spy).toHaveBeenCalledWith({ format: 'pdf', folder: ' /Téléphone\\PDF/ ' });
    expect(onchange).toHaveBeenCalledWith(expect.objectContaining({ autoExport: { format: 'pdf', folder: 'Téléphone/PDF' } }));
    // The field shows the folder as the app stored it.
    expect(folder(target).value).toBe('Téléphone/PDF');
    expect(section(target).textContent).toContain('Réglage enregistré.');
    expect((await api.appState()).autoExport).toEqual({ format: 'pdf', folder: 'Téléphone/PDF' });
  });

  it('refuses a folder outside the library with an actionable message', async () => {
    const { target, onchange } = await render('fr');
    radio(target, 'PNG (image)').click();
    await settle();
    await type(folder(target), '../dehors');
    save(target).click();
    await settle();
    const alert = section(target).querySelector('[role="alert"]')!;
    expect(alert.textContent).toContain("ce dossier n'est pas utilisable");
    expect(alert.textContent).toContain('par exemple « Exports »');
    expect(onchange).not.toHaveBeenCalled();
    expect(section(target).textContent).not.toContain('Réglage enregistré.');
    expect((await api.appState()).autoExport.format).toBe('off');

    // Fixing the folder works, and clears the message.
    await type(folder(target), 'Exports');
    save(target).click();
    await settle();
    expect(section(target).querySelector('[role="alert"]')).toBeNull();
    expect(onchange).toHaveBeenCalledOnce();
  });

  it('refuses the library\'s own folder', async () => {
    const { target } = await render('fr');
    await type(folder(target), '_ardoise/exports');
    save(target).click();
    await settle();
    expect(section(target).querySelector('[role="alert"]')?.textContent).toContain("ce dossier n'est pas utilisable");
  });

  it('speaks English', async () => {
    const { target } = await render('en');
    expect(section(target).querySelector('h3')?.textContent).toBe('Automatic export after sync');
    expect(radio(target, 'Off').checked).toBe(true);
    expect(section(target).textContent).toContain('Nextcloud app');
    await type(folder(target), '..');
    [...section(target).querySelectorAll('button')].find((b) => b.textContent?.trim() === 'Save')!.click();
    await settle();
    expect(section(target).querySelector('[role="alert"]')?.textContent).toContain("this folder can't be used");
  });
});

describe('settings: microphone', () => {
  afterEach(async () => {
    await api.setMicrophone(null);
  });

  const section = (target: HTMLElement) => target.querySelector<HTMLElement>('[aria-labelledby="settings-microphone"]')!;

  it('shows the picker with its level meter, on the microphone chosen on this PC', async () => {
    const { target } = await render('fr', undefined, { ...app, microphone: 'usb-1' });
    expect(section(target).querySelector('h3')?.textContent).toBe('Micro');
    expect(section(target).querySelector('.microphone-picker-stand-in')).toBeTruthy();
    expect(picker.props?.device).toBe('usb-1');
    expect(picker.props?.monitor).toBe(true);
  });

  it('remembers the chosen microphone and reports the new state', async () => {
    const spy = vi.spyOn(api, 'setMicrophone');
    const { onchange } = await render('fr');
    picker.props!.onchange('usb-2');
    await settle();
    expect(spy).toHaveBeenCalledWith('usb-2');
    expect(onchange).toHaveBeenCalledWith(expect.objectContaining({ microphone: 'usb-2' }));
    // Back to the system's default.
    picker.props!.onchange(null);
    await settle();
    expect(spy).toHaveBeenLastCalledWith(null);
    expect(onchange).toHaveBeenLastCalledWith(expect.objectContaining({ microphone: null }));
  });

  it('reports a failure to remember it', async () => {
    vi.spyOn(api, 'setMicrophone').mockRejectedValueOnce({ code: 'settings', message: 'disque plein' });
    const { target, onchange } = await render('fr');
    picker.props!.onchange('usb-2');
    await settle();
    expect(onchange).not.toHaveBeenCalled();
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('le fichier de réglages ne peut pas être lu ou modifié');
  });

  it('speaks English', async () => {
    const { target } = await render('en');
    expect(section(target).querySelector('h3')?.textContent).toBe('Microphone');
    expect(section(target).textContent).toContain('remembered on this computer');
  });
});

describe('settings: OpenH264 backup encoder', () => {
  const section = (target: HTMLElement) => target.querySelector<HTMLElement>('[aria-labelledby="settings-openh264"]')!;
  const buttons = (target: HTMLElement) => [...section(target).querySelectorAll('button')].map((b) => b.textContent?.trim());
  const sectionButton = (target: HTMLElement, text: string) => {
    const found = [...section(target).querySelectorAll('button')].find((b) => b.textContent?.trim() === text);
    if (!found) throw new Error(`no button "${text}"`);
    return found;
  };
  const allow = (target: HTMLElement) => section(target).querySelector<HTMLInputElement>('input[type="checkbox"]')!;
  const sleep = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

  it('explains the backup, and says it is not needed when Windows writes MP4 itself', async () => {
    const { target } = await render('fr');
    const text = section(target).textContent!;
    expect(section(target).querySelector('h3')?.textContent).toBe('Encodeur vidéo de secours (OpenH264)');
    expect(text).toContain('Media Foundation');
    expect(text).toContain('« N »');
    expect(text).toContain('Cisco');
    expect(section(target).querySelector('[data-slot="media-foundation"]')?.textContent?.trim()).toBe(
      "Windows sait écrire les MP4 sur ce PC : le secours n'est pas nécessaire.",
    );
    expect(section(target).querySelector('[data-slot="status"]')?.textContent?.trim()).toBe('Non installé');
  });

  it('warns when Windows cannot write MP4 here', async () => {
    mockVideo.mediaFoundation = false;
    const { target } = await render('fr');
    expect(section(target).querySelector('[data-slot="media-foundation"]')?.textContent).toContain(
      'Windows ne sait pas écrire les MP4 sur ce PC',
    );
  });

  it('shows the Cisco notice exactly, and the full licence, reachable before downloading', async () => {
    const state = await api.openH264State();
    const { target } = await render('fr');
    expect(section(target).querySelector('[data-slot="notice"]')?.textContent).toBe(
      'OpenH264 Video Codec provided by Cisco Systems, Inc.',
    );
    expect(state.notice).toBe('OpenH264 Video Codec provided by Cisco Systems, Inc.');
    const license = section(target).querySelector<HTMLDetailsElement>('details.openh264-license')!;
    expect(license.querySelector('summary')?.textContent).toBe("Licence d'OpenH264 (Cisco)");
    expect(license.querySelector('pre')?.textContent).toBe(state.license);
    // Open while the user is about to download.
    expect(license.open).toBe(true);
  });

  it('shows the same notice and licence in English', async () => {
    const { target } = await render('en');
    expect(section(target).querySelector('[data-slot="notice"]')?.textContent).toBe(
      'OpenH264 Video Codec provided by Cisco Systems, Inc.',
    );
    expect(section(target).querySelector('details summary')?.textContent).toBe('OpenH264 licence (Cisco)');
    expect(section(target).querySelector('details pre')?.textContent).toContain('Cisco');
  });

  it('downloads from Cisco with a progress bar, then shows it installed and allowed', async () => {
    mockVideo.delayMs = 800;
    mockVideo.steps = 4;
    const { target } = await render('fr');
    expect(buttons(target)).toEqual(['Télécharger depuis Cisco']);
    sectionButton(target, 'Télécharger depuis Cisco').click();
    await sleep(500);
    await settle();
    expect(section(target).querySelector('[role="status"]')?.textContent).toContain('Téléchargement…');
    const bar = section(target).querySelector<HTMLProgressElement>('progress')!;
    expect(bar.value).toBeGreaterThan(0);
    expect(bar.value).toBeLessThan(bar.max);
    expect(section(target).textContent).toMatch(/\d,\d sur 0,9 Mo/);
    expect(buttons(target)).toEqual(['Annuler le téléchargement']);

    await sleep(600);
    await settle();
    expect(section(target).querySelector('progress')).toBeNull();
    expect(section(target).querySelector('[data-slot="status"]')?.textContent?.trim()).toBe('Installé (0,9 Mo, version 2.6.0)');
    expect(allow(target).checked).toBe(true);
    expect(buttons(target)).toEqual(['Supprimer']);
    expect((await api.openH264State()).status).toBe('installed');
    // The licence stays reachable, folded away now.
    expect(section(target).querySelector<HTMLDetailsElement>('details.openh264-license')!.open).toBe(false);
    expect(section(target).querySelector('details pre')?.textContent).toContain('Cisco');
  });

  it('cancels the download: a neutral notice, nothing installed, and it can be started again', async () => {
    mockVideo.delayMs = 5000;
    const { target } = await render('fr');
    sectionButton(target, 'Télécharger depuis Cisco').click();
    await settle();
    sectionButton(target, 'Annuler le téléchargement').click();
    await settle();
    expect(section(target).querySelector('[role="alert"]')).toBeNull();
    expect(section(target).textContent).toContain('Téléchargement annulé.');
    expect(section(target).querySelector('progress')).toBeNull();
    expect((await api.openH264State()).status).toBe('notInstalled');
    expect(sectionButton(target, 'Télécharger depuis Cisco').disabled).toBe(false);
  });

  it('reports a failed download', async () => {
    mockVideo.downloadFails = true;
    const { target } = await render('fr');
    sectionButton(target, 'Télécharger depuis Cisco').click();
    await settle();
    expect(section(target).querySelector('[role="alert"]')?.textContent).toContain("le téléchargement d'OpenH264 a échoué");
    expect(sectionButton(target, 'Télécharger depuis Cisco').disabled).toBe(false);
  });

  it('explains in plain words that Windows still uses OpenH264 when it cannot be removed', async () => {
    setMockOpenH264({ status: 'installed', bytes: 860_000, allowed: true });
    mockVideo.removeFails = true;
    const { target } = await render('fr');
    sectionButton(target, 'Supprimer').click();
    await settle();
    expect(section(target).querySelector('[role="alert"]')?.textContent).toContain(
      'Windows utilise encore OpenH264 : fermez puis relancez Ardoise, puis supprimez-le.',
    );
    expect((await api.openH264State()).status).toBe('installed');
  });

  it('allows, disallows and re-allows the installed encoder', async () => {
    setMockOpenH264({ status: 'installed', bytes: 860_000, allowed: true });
    const spy = vi.spyOn(api, 'setOpenH264Allowed');
    const { target } = await render('fr');
    expect(allow(target).checked).toBe(true);
    expect(allow(target).parentElement?.textContent?.trim()).toBe('Autoriser Ardoise à utiliser OpenH264 pour les MP4');

    allow(target).click();
    await settle();
    expect(spy).toHaveBeenLastCalledWith(false);
    expect(allow(target).checked).toBe(false);
    expect((await api.openH264State()).allowed).toBe(false);

    allow(target).click();
    await settle();
    expect(spy).toHaveBeenLastCalledWith(true);
    expect(allow(target).checked).toBe(true);
    expect((await api.openH264State()).allowed).toBe(true);
  });

  it('removes the installed encoder, which also disallows it', async () => {
    setMockOpenH264({ status: 'installed', bytes: 860_000, allowed: true });
    const { target } = await render('fr');
    expect(section(target).querySelector('[data-slot="status"]')?.textContent?.trim()).toBe('Installé (0,9 Mo, version 2.6.0)');
    sectionButton(target, 'Supprimer').click();
    await settle();
    expect(section(target).querySelector('[data-slot="status"]')?.textContent?.trim()).toBe('Non installé');
    expect(section(target).querySelector('input[type="checkbox"]')).toBeNull();
    expect(buttons(target)).toEqual(['Télécharger depuis Cisco']);
    const state = await api.openH264State();
    expect([state.status, state.allowed]).toEqual(['notInstalled', false]);
  });

  it('offers to remove or download again a damaged file', async () => {
    setMockOpenH264({ status: 'damaged', bytes: 12 });
    const { target } = await render('fr');
    expect(section(target).querySelector('[data-slot="status"]')?.textContent).toContain('Fichier endommagé');
    expect(buttons(target).sort()).toEqual(['Supprimer', 'Télécharger depuis Cisco']);
    expect(section(target).querySelector('input[type="checkbox"]')).toBeNull();
  });

  it('speaks English', async () => {
    setMockOpenH264({ status: 'installed', bytes: 860_000, allowed: true });
    const { target } = await render('en');
    expect(section(target).querySelector('h3')?.textContent).toBe('Backup video encoder (OpenH264)');
    expect(section(target).querySelector('[data-slot="media-foundation"]')?.textContent?.trim()).toBe(
      'Windows can write MP4 files on this PC: the backup is not needed.',
    );
    expect(section(target).querySelector('[data-slot="status"]')?.textContent?.trim()).toBe('Installed (0.9 MB, version 2.6.0)');
    expect(buttons(target)).toEqual(['Remove']);
  });
});
