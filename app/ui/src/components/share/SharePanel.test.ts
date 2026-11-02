// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import { mockShare, mockShareSetViewers, resetMockShare } from '../../lib/mock-share';
import { share } from '../../lib/share-state.svelte';
import SharePanel from './SharePanel.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => {
  resetMockShare();
  share.reset();
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  share.reset();
  resetMockShare();
  vi.restoreAllMocks();
});

async function settle() {
  for (let i = 0; i < 4; i++) {
    await Promise.resolve();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

/** The panel as the main window shows it: mounted while `share.panelOpen`. */
async function render(open = true) {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(SharePanel, { target });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  if (open) {
    share.openPanel();
    await settle();
  }
  const panel = () => document.querySelector<HTMLElement>('.share-panel');
  const action = (name: string) => document.querySelector<HTMLButtonElement>(`button[data-action="${name}"]`);
  const addresses = () => [...document.querySelectorAll<HTMLElement>('[data-share-address]')];
  return { target, panel, action, addresses };
}

describe('the share panel, while a share runs', () => {
  it('starts the share when it opens, and shows the address, the code and the QR code', async () => {
    const view = await render();
    expect(share.status.active).toBe(true);
    expect(view.panel()).not.toBeNull();
    expect(document.querySelector('h2')?.textContent).toBe('Partager sur le réseau');
    expect(document.body.textContent).toContain('Sur l\'autre ordinateur, ouvrez un navigateur et tapez');

    const [main] = view.addresses();
    expect(main.dataset.shareAddress).toBe('http://192.168.1.23:8765/');
    expect(main.textContent).toBe('http://192.168.1.23:8765/');
    expect(main.classList.contains('main')).toBe(true);

    const code = document.querySelector<HTMLElement>('[data-share-code]')!;
    expect(code.dataset.shareCode).toBe('K7M Q2X');
    expect(code.textContent).toBe('K7M Q2X');

    const qr = document.querySelector<SVGSVGElement>('svg.share-qr')!;
    expect(qr.getAttribute('viewBox')).toBe(`-4 -4 ${share.status.qr!.size + 8} ${share.status.qr!.size + 8}`);
    expect(qr.querySelector('rect')?.getAttribute('fill')).toBe('#ffffff');
    expect(qr.querySelector('path')?.getAttribute('d')).toBe(share.status.qr!.path);
    expect(qr.getAttribute('aria-label')).toBe('Code QR de l\'adresse du partage');
  });

  it('shows the other addresses below the first one', async () => {
    mockShare.addresses = ['http://192.168.1.23:8765/', 'http://10.0.0.5:8765/', 'http://172.16.4.2:8765/'];
    const view = await render();
    const shown = view.addresses();
    expect(shown.map((a) => a.dataset.shareAddress)).toEqual(mockShare.addresses);
    expect(shown[0].classList.contains('main')).toBe(true);
    expect(shown[1].classList.contains('main')).toBe(false);
  });

  it('counts the screens connected, with the plural of the language', async () => {
    const view = await render();
    const viewers = () => document.querySelector<HTMLElement>('[data-share-viewers]')!;
    expect(viewers().dataset.shareViewers).toBe('0');
    expect(viewers().textContent?.trim()).toBe('Aucun écran connecté');
    mockShareSetViewers(1);
    await settle();
    expect(viewers().dataset.shareViewers).toBe('1');
    expect(viewers().textContent?.trim()).toBe('1 écran connecté');
    mockShareSetViewers(3);
    await settle();
    expect(viewers().dataset.shareViewers).toBe('3');
    expect(viewers().textContent?.trim()).toBe('3 écrans connectés');
    setLocale('en');
    await settle();
    expect(viewers().textContent?.trim()).toBe('3 screens connected');
    mockShareSetViewers(1);
    await settle();
    expect(viewers().textContent?.trim()).toBe('1 screen connected');
    expect(view.panel()).not.toBeNull();
  });

  it('says what is shown', async () => {
    await render();
    const shown = () => document.querySelector<HTMLElement>('[data-share-shown]')!;
    expect(shown().dataset.shareShown).toBe('waiting');
    expect(shown().textContent).toContain('L\'écran d\'attente');
    share.showLive();
    await settle();
    expect(shown().dataset.shareShown).toBe('live');
    expect(shown().textContent).toContain('La feuille en direct');
  });

  it('pauses and resumes', async () => {
    const view = await render();
    const pause = view.action('share-pause')!;
    expect(pause.getAttribute('aria-pressed')).toBe('false');
    expect(pause.textContent?.trim()).toBe('Pause');
    expect(document.querySelector('[data-share-paused]')).toBeNull();

    pause.click();
    await settle();
    expect(share.status.paused).toBe(true);
    expect(view.action('share-pause')!.getAttribute('aria-pressed')).toBe('true');
    expect(view.action('share-pause')!.textContent?.trim()).toBe('Reprendre');
    expect(document.querySelector('[data-share-paused]')?.textContent).toBe('En pause : l\'image partagée est figée');

    view.action('share-pause')!.click();
    await settle();
    expect(share.status.paused).toBe(false);
    expect(view.action('share-pause')!.getAttribute('aria-pressed')).toBe('false');
    expect(document.querySelector('[data-share-paused]')).toBeNull();
  });

  it('stops the share and closes the panel', async () => {
    const view = await render();
    view.action('share-stop')!.click();
    await settle();
    expect(share.status.active).toBe(false);
    expect(share.panelOpen).toBe(false);
  });

  it('closes with Escape or the button, and leaves the share running', async () => {
    const view = await render();
    window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    await settle();
    expect(share.panelOpen).toBe(false);
    expect(share.status.active).toBe(true);

    share.openPanel();
    view.action('share-close')!.click();
    await settle();
    expect(share.panelOpen).toBe(false);
    expect(share.status.active).toBe(true);
  });

  it('copies the first address', async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true });
    const view = await render();
    view.action('share-copy')!.click();
    await settle();
    expect(writeText).toHaveBeenCalledWith('http://192.168.1.23:8765/');
    expect(document.body.textContent).toContain('Adresse copiée');
  });

  it('tells what to do about the firewall and when the share ends', async () => {
    await render();
    const text = document.querySelector('.share-panel')!.textContent!;
    expect(text).toContain('pare-feu');
    expect(text).toContain('réseau privé');
    expect(text).toContain('quand vous fermez Ardoise');
  });

  it('has an English text too', async () => {
    setLocale('en');
    await render();
    const text = document.querySelector('.share-panel')!.textContent!;
    expect(text).toContain('On the other computer, open a browser and type');
    expect(text).toContain('private networks');
    expect(document.querySelector('button[data-action="share-stop"]')?.textContent?.trim()).toBe('Stop sharing');
  });
});

describe('the share panel, while it starts or when it cannot', () => {
  it('shows a spinner while the share starts', async () => {
    mockShare.startDelayMs = 80;
    const view = await render();
    expect(view.panel()?.getAttribute('aria-busy')).toBe('true');
    expect(view.panel()?.querySelector('.spinner')).not.toBeNull();
    expect(document.body.textContent).toContain('Démarrage du partage');
    expect(view.action('share-stop')).toBeNull();
    await vi.waitFor(() => expect(share.status.active).toBe(true));
    await settle();
    expect(view.panel()?.getAttribute('aria-busy')).toBe('false');
    expect(view.panel()?.querySelector('.spinner')).toBeNull();
    expect(view.action('share-stop')).not.toBeNull();
  });

  const failures: [string, string, string[]][] = [
    ['sharePortInUse', '8765', ['déjà utilisé par un autre programme', 'réglages d\'Ardoise', 'Port du partage']],
    ['shareNoFreePort', '8765-8785', ['plage 8765-8785', 'réglages d\'Ardoise', 'Port du partage']],
    ['shareUnavailable', 'no assets', ['version d\'Ardoise ne peut pas partager']],
    ['shareFailed', 'bind: boom', ['n\'a pas pu démarrer', 'bind: boom']],
  ];
  for (const [code, message, expected] of failures) {
    it(`says ${code} in words, and offers to try again`, async () => {
      mockShare.failStart = { code, message };
      const view = await render();
      const error = document.querySelector<HTMLElement>('[data-share-error]')!;
      expect(error.getAttribute('role')).toBe('alert');
      for (const part of expected) expect(error.textContent, part).toContain(part);
      expect(error.textContent).not.toContain('errorCodes');
      // The explanation alone, as a sentence: no « Une erreur est survenue. » before it.
      expect(error.textContent).not.toContain('Une erreur est survenue');
      expect(error.textContent).toMatch(/^[A-ZÀ-Ý].*\.$/);
      expect(share.status.active).toBe(false);
      expect(view.action('share-stop')).toBeNull();

      mockShare.failStart = null;
      view.action('share-start')!.click();
      await settle();
      expect(document.querySelector('[data-share-error]')).toBeNull();
      expect(share.status.active).toBe(true);
    });
  }

  it('says the port problem in English too', async () => {
    setLocale('en');
    mockShare.failStart = { code: 'sharePortInUse', message: '8765' };
    await render();
    const text = document.querySelector('[data-share-error]')!.textContent!;
    expect(text).toContain('already used by another program');
    expect(text).toContain('Share port');
    expect(text).not.toContain('ardoise.toml');
    expect(text).not.toContain('Something went wrong');
    expect(text.startsWith('The share')).toBe(true);
  });

  it('says that the port asked for was held and which one the share uses', async () => {
    mockShare.fallbackPort = 8766;
    mockShare.addresses = ['http://192.168.1.23:8766/'];
    await render();
    expect(share.status.port).toBe(8766);
    expect(share.status.requestedPort).toBe(8765);
    const notice = document.querySelector<HTMLElement>('[data-share-port-moved]')!;
    expect(notice.getAttribute('role')).toBe('status');
    expect(notice.textContent!.trim()).toBe(
      'Le port 8765 est utilisé par un autre programme : le partage utilise le port 8766, retenu pour ce PC.',
    );
    expect(document.querySelector('[data-share-address]')!.textContent).toBe('http://192.168.1.23:8766/');
  });

  it('says it in English too', async () => {
    setLocale('en');
    mockShare.fallbackPort = 8766;
    await render();
    expect(document.querySelector('[data-share-port-moved]')!.textContent!.trim()).toBe(
      'Port 8765 is used by another program: the share uses port 8766, kept for this PC.',
    );
  });

  it('says nothing about the port when the port asked for is the one used', async () => {
    await render();
    expect(share.status.port).toBe(share.status.requestedPort);
    expect(document.querySelector('[data-share-port-moved]')).toBeNull();
  });

  it('keeps the port found for the next share, with no notice then', async () => {
    mockShare.fallbackPort = 8766;
    const view = await render();
    expect(document.querySelector('[data-share-port-moved]')).not.toBeNull();
    view.action('share-stop')!.click();
    await settle();
    expect(share.status.port).toBe(8766);
    expect(share.status.requestedPort).toBe(8766);
    share.openPanel();
    await settle();
    expect(share.status.active).toBe(true);
    expect(document.querySelector('[data-share-port-moved]')).toBeNull();
  });
});
