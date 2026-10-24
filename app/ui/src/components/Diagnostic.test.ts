// @vitest-environment jsdom
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../lib/api';
import { STEP_IDS, type StepId } from '../lib/diagnostic';
import { setLocale } from '../lib/i18n.svelte';
import { mockDiagnostic, stopMockDiagnostic } from '../lib/mock';
import Diagnostic from './Diagnostic.svelte';
import Home from './Home.svelte';

const app: AppState = {
  version: '0.1.0',
  machine: 'PC-TEST',
  settingsPath: 'C:\\Ardoise\\ardoise.toml',
  settingsError: null,
  language: 'auto',
  theme: 'system',
  library: 'C:\\demo',
  systemLocale: 'fr-FR',
  selfTest: false,
  openDiagnostic: false,
};

let cleanup: (() => void) | null = null;

beforeEach(() => {
  mockDiagnostic.stepMs = 0;
});

afterEach(async () => {
  cleanup?.();
  cleanup = null;
  vi.restoreAllMocks();
  // Let a mocked run that was interrupted wind down before the next test starts one.
  await stopMockDiagnostic();
});

function render() {
  const target = document.createElement('div');
  document.body.append(target);
  const onclose = vi.fn();
  const component = mount(Diagnostic, { target, props: { onclose } });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  const button = (label: string) => {
    const found = [...target.querySelectorAll('button')].find((b) => b.textContent?.includes(label));
    if (!found) throw new Error(`no button "${label}"`);
    return found;
  };
  const stepStatus = (id: StepId) => target.querySelector(`[data-step="${id}"]`)?.getAttribute('data-status');
  /** Answers each prompt with the button chosen by `pick` (from the prompt's step), until the summary shows. */
  const drive = async (pick: (prompt: string) => string) => {
    for (let i = 0; i < 12; i++) {
      await vi.waitFor(() => expect(target.querySelector('.card-prompt button, .summary')).toBeTruthy());
      if (target.querySelector('.summary')) return;
      const card = target.querySelector('.card-prompt');
      const label = pick(card?.textContent ?? '');
      button(label).click();
      await tick();
    }
    throw new Error('the test never ended');
  };
  return { target, button, stepStatus, drive, onclose };
}

describe('tablet test panel', () => {
  it('runs every step when the user continues each prompt, in French', async () => {
    setLocale('fr');
    const { target, button, stepStatus, drive } = render();

    expect(target.textContent).toContain('Test de la tablette');
    expect(target.textContent).toContain('Durée : environ 5 minutes');
    const checkboxes = target.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    expect(checkboxes[0].checked).toBe(true);
    expect(checkboxes[1].checked).toBe(false);
    expect(STEP_IDS.every((id) => stepStatus(id) === 'pending')).toBe(true);

    button('Démarrer le test').click();
    await vi.waitFor(() => expect(target.querySelector('.card-prompt')).toBeTruthy());
    expect(target.textContent).toContain('Sur la tablette, dessinez 3 pages de référence');
    expect(target.textContent).toContain('Ardoise 2026');
    expect(button('Démarrer le test').disabled).toBe(true);
    expect(target.querySelector<HTMLInputElement>('input[type="checkbox"]')?.disabled).toBe(true);
    expect(document.activeElement).toBe(button("C'est fait, continuer"));

    const prompts: string[] = [];
    await drive((prompt) => {
      prompts.push(prompt);
      return "C'est fait, continuer";
    });
    expect(prompts).toHaveLength(3);
    expect(prompts[1]).toContain('jusqu\'à ce que le voyant clignote en bleu');
    expect(prompts[1]).toContain("Attention : la tablette ne fonctionnera plus avec l'application du téléphone");
    expect(prompts[2]).toContain('pendant 30 secondes');

    expect(target.textContent).toContain('8 réussie(s), 0 échec(s), 0 ignorée(s)');
    expect(STEP_IDS.every((id) => stepStatus(id) === 'passed')).toBe(true);
    expect(target.textContent).toContain('Journal : C:\\demo\\.ardoise\\diagnostics');
    expect(target.textContent).not.toContain('Test arrêté avant la fin.');
    // Details are translated, and the values shown.
    expect(target.textContent).toContain('Micrologiciel');
    expect(target.textContent).toContain('B160805A-C170704A');
    expect(target.textContent).toContain('297 × 216 mm');
    button('Recommencer le test');
  });

  it('counts a skipped step in the summary', async () => {
    setLocale('fr');
    const { target, button, stepStatus, drive } = render();
    button('Démarrer le test').click();

    await drive((prompt) => (prompt.includes('sans vous arrêter') ? 'Passer cette étape' : "C'est fait, continuer"));

    expect(stepStatus('live')).toBe('skipped');
    expect(target.querySelector('[data-step="live"]')?.textContent).toContain('Ignorée');
    expect(target.textContent).toContain('7 réussie(s), 0 échec(s), 1 ignorée(s)');
  });

  it('shows the stopped message when the user stops the test', async () => {
    setLocale('fr');
    const { target, button, stepStatus, drive } = render();
    button('Démarrer le test').click();

    await drive((prompt) => (prompt.includes('Recherche') ? 'Arrêter le test' : "C'est fait, continuer"));

    expect(target.textContent).toContain('Test arrêté avant la fin.');
    expect(stepStatus('adapter')).toBe('passed');
    expect(stepStatus('find')).toBe('pending');
    expect(stepStatus('live')).toBe('pending');
    expect(target.textContent).toContain('2 réussie(s), 0 échec(s), 0 ignorée(s)');
  });

  it('can be stopped while a step is running, and run again', async () => {
    setLocale('fr');
    mockDiagnostic.stepMs = 300;
    const { target, button, drive } = render();
    button('Démarrer le test').click();
    await vi.waitFor(() => expect(target.querySelector('.card-prompt')).toBeTruthy());
    button("C'est fait, continuer").click();
    await vi.waitFor(() => expect(target.querySelector('.progress')).toBeTruthy());
    expect(target.textContent).toContain('En attente de la tablette…');
    button('Arrêter le test').click();
    await vi.waitFor(() => expect(target.textContent).toContain('Test arrêté avant la fin.'));

    mockDiagnostic.stepMs = 0;
    button('Recommencer le test').click();
    await tick();
    await drive(() => "C'est fait, continuer");
    expect(target.textContent).not.toContain('Test arrêté avant la fin.');
    expect(target.textContent).toContain('8 réussie(s)');
  });

  it('shows progress while downloading and in live mode', async () => {
    setLocale('fr');
    mockDiagnostic.stepMs = 20;
    const { target, button } = render();
    button('Démarrer le test').click();
    const seen = new Set<string>();
    const watcher = setInterval(() => {
      const text = target.querySelector('.progress')?.textContent;
      if (text) seen.add(text);
      const prompt = target.querySelector('.card-prompt button');
      if (prompt) (prompt as HTMLButtonElement).click();
    }, 2);
    await vi.waitFor(() => expect(target.querySelector('.summary')).toBeTruthy(), { timeout: 5000 });
    clearInterval(watcher);
    expect(seen).toContain('Page 2 sur 3');
    expect([...seen].some((text) => /^\d+ points reçus — \d+ s$/.test(text))).toBe(true);
  });

  it('renders instructions in English', async () => {
    setLocale('en');
    const { target, button } = render();

    expect(target.textContent).toContain('Tablet test');
    expect(target.textContent).toContain('Bluetooth adapter');
    expect(target.textContent).toContain('Delete pages from the tablet once saved');
    button('Start the test').click();
    await vi.waitFor(() => expect(target.querySelector('.card-prompt')).toBeTruthy());
    expect(target.textContent).toContain('draw 3 reference pages');
    button('Done, continue').click();
    await tick();
    await vi.waitFor(() => expect(target.textContent).toContain('Hold the'));
    expect(target.textContent).toContain('no longer work with the phone app');
    expect(target.textContent).toContain('Skip this step');
    expect(target.textContent).toContain('Stop the test');
  });

  it('passes the chosen options to the run', async () => {
    setLocale('fr');
    const start = vi.spyOn(api, 'diagnosticStart').mockResolvedValue();
    const { target, button } = render();
    const [deleteBox, simulatedBox] = target.querySelectorAll<HTMLInputElement>('input[type="checkbox"]');
    deleteBox.click();
    simulatedBox.click();
    button('Démarrer le test').click();
    await vi.waitFor(() => expect(start).toHaveBeenCalledOnce());
    expect(start.mock.calls[0][0]).toEqual({ simulated: true, deleteAfterSync: false });
  });

  it('turns a refused start into a translated banner and lets the user retry', async () => {
    setLocale('fr');
    vi.spyOn(api, 'diagnosticStart').mockRejectedValue({ code: 'noLibrary', message: 'x' });
    const { target, button } = render();
    button('Démarrer le test').click();
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')).toBeTruthy());
    expect(target.querySelector('[role="alert"]')?.textContent).toContain("aucune bibliothèque n'est choisie");
    expect(button('Démarrer le test').disabled).toBe(false);
  });

  it('shows an error event as a banner', async () => {
    setLocale('en');
    vi.spyOn(api, 'diagnosticStart').mockImplementation(async (_options, onEvent) => {
      onEvent({ type: 'error', code: 'diagnosticRunning', message: 'busy' });
    });
    const { target, button } = render();
    button('Start the test').click();
    await vi.waitFor(() => expect(target.querySelector('[role="alert"]')).toBeTruthy());
    expect(target.querySelector('[role="alert"]')?.textContent).toContain('a test is already running');
    expect(button('Run the test again')).toBeTruthy();
  });

  it('falls back to the raw key for an unknown detail', async () => {
    setLocale('en');
    vi.spyOn(api, 'diagnosticStart').mockImplementation(async (_options, onEvent) => {
      onEvent({ type: 'started', step: 'info' });
      onEvent({
        type: 'finished',
        outcome: {
          step: 'info',
          status: 'failed',
          details: [
            { key: 'battery', value: '12 %' },
            { key: 'mystery', value: '42' },
          ],
        },
      });
    });
    const { target, button } = render();
    button('Start the test').click();
    await vi.waitFor(() => expect(target.querySelector('[data-step="info"] dl')).toBeTruthy());
    const labels = [...target.querySelectorAll('[data-step="info"] dt')].map((n) => n.textContent);
    expect(labels).toEqual(['Battery', 'mystery']);
    expect(target.querySelector('[data-step="info"]')?.textContent).toContain('Failed');
  });
});

describe('home screen', () => {
  function renderHome(openDiagnostic: boolean) {
    const target = document.createElement('div');
    document.body.append(target);
    const component = mount(Home, { target, props: { app: { ...app, openDiagnostic }, onchange: vi.fn() } });
    cleanup = () => {
      unmount(component);
      target.remove();
    };
    return target;
  }

  it('opens the tablet test from the top bar', async () => {
    setLocale('fr');
    const target = renderHome(false);
    expect(target.querySelector('.panel')).toBeNull();
    [...target.querySelectorAll('button')].find((b) => b.textContent?.includes('Tester la tablette'))?.click();
    await tick();
    expect(target.querySelector('.panel')?.getAttribute('aria-label')).toBe('Test de la tablette');
  });

  it('opens the tablet test right away when started with --diagnostic', async () => {
    setLocale('en');
    const target = renderHome(true);
    await vi.waitFor(() => expect(target.querySelector('.panel')).toBeTruthy());
    expect(target.querySelector('.panel')?.getAttribute('aria-label')).toBe('Tablet test');
  });
});
