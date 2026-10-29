// @vitest-environment jsdom
import { mount, tick, unmount, type Component } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api, type AppState } from '../../lib/api';
import { setLocale } from '../../lib/i18n.svelte';
import { inkspaceOffer } from '../../lib/inkspace-offer.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockInkspace, resetMockInkspace } from '../../lib/mock-inkspace';
import Home from '../Home.svelte';
import InkspaceImportDialog from './InkspaceImportDialog.svelte';

type E2eWindow = { __ardoiseE2eFolderPath?: string };

const FOUND = mockInkspace.detected!;
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
  resetMockInkspace();
  mockInkspace.stepMs = 0;
  inkspaceOffer.pending = false;
  setLocale('fr');
});

afterEach(() => {
  cleanup?.();
  cleanup = null;
  delete (window as E2eWindow).__ardoiseE2eFolderPath;
  vi.restoreAllMocks();
});

function place<P extends Record<string, any>>(component: Component<P>, props: P) {
  const target = document.createElement('div');
  document.body.append(target);
  const mounted = mount(component, { target, props });
  cleanup = () => {
    unmount(mounted);
    target.remove();
  };
  return target;
}

/** Mounts the dialog; helpers look in the whole document (the modal sits outside `target`). */
function dialog(props: Partial<{ binders: string[]; onclose: () => void; onshow: (b: string) => void; ondone: () => void }> = {}) {
  const handlers = { onclose: vi.fn(), onshow: vi.fn(), ondone: vi.fn(), ...props };
  place(InkspaceImportDialog, { binders: ['Classe', 'Inkspace'], ...handlers });
  return handlers;
}

const slot = (name: string) => document.querySelector(`[data-slot="${name}"]`);
const text = () => document.body.textContent ?? '';
const status = () => document.querySelector('[role="status"]')?.textContent ?? '';
const alertText = () => document.querySelector('[role="alert"]')?.textContent ?? '';
const buttons = () => [...document.querySelectorAll('button')].map((b) => b.textContent?.trim());
function button(label: string): HTMLButtonElement {
  const found = [...document.querySelectorAll('button')].find((b) => b.textContent?.trim() === label);
  if (!found) throw new Error(`no button "${label}" among ${buttons().join(' | ')}`);
  return found;
}
const binderField = () => document.querySelector<HTMLInputElement>('input[list="inkspace-binders"]')!;

async function type(input: HTMLInputElement, value: string) {
  input.value = value;
  input.dispatchEvent(new Event('input', { bubbles: true }));
  await tick();
}

/** Waits for the detected Inkspace to be counted. */
async function found() {
  await vi.waitFor(() => expect(slot('inkspace-count')?.textContent).toContain('5 notes trouvées'));
}

/** A promise settled by hand, to hold an import at a chosen moment. */
function gate() {
  let open!: () => void;
  const until = new Promise<void>((resolve) => (open = resolve));
  return { until, open };
}

describe('importing from Inkspace', () => {
  it('shows the folder found and its notes, imports them with progress, then reports', async () => {
    const original = api.inkspaceImport;
    const held = gate();
    vi.spyOn(api, 'inkspaceImport').mockImplementation(async (path, binder, onProgress) => {
      onProgress?.({ done: 0, total: 5 });
      onProgress?.({ done: 2, total: 5 });
      await held.until;
      return original(path, binder, onProgress);
    });
    const { ondone } = dialog();

    expect(status()).toContain("Recherche d'Inkspace");
    await found();
    expect(slot('inkspace-path')?.textContent).toBe(FOUND);
    expect(slot('inkspace-count')?.textContent).toContain('dont 1 illisible');
    expect(slot('inkspace-count')?.textContent).not.toContain('déjà');
    expect(button('Importer 4 notes').disabled).toBe(false);
    expect(binderField().value).toBe('Inkspace');
    expect(text()).toContain('Laissez vide pour ranger les notes à la racine');

    button('Importer 4 notes').click();
    await vi.waitFor(() => expect(status()).toContain('Import des notes : 2 sur 5…'));
    expect(document.querySelector('progress')?.getAttribute('value')).toBe('2');
    expect(document.querySelector('progress')?.getAttribute('max')).toBe('5');
    // Nothing but the Stop button meanwhile.
    expect(buttons()).toEqual(["Arrêter l'import"]);
    expect(ondone).not.toHaveBeenCalled();

    held.open();
    await vi.waitFor(() => expect(slot('inkspace-report')).toBeTruthy());
    expect(ondone).toHaveBeenCalledTimes(1);
    const report = slot('inkspace-report')!.textContent!;
    expect(report).toContain('Import terminé');
    expect(report).toContain('4 notes importées dans le classeur « Inkspace »');
    expect(report).not.toContain('déjà présente');
    expect(report).toContain("1 note n'a pas pu être importée :");
    const lines = [...slot('inkspace-report')!.querySelectorAll('li')];
    expect(lines).toHaveLength(1);
    expect(lines[0].textContent).toContain('d91e29cf');
    expect(lines[0].textContent).toContain("cette note d'Inkspace ne peut pas être lue (unknown stroke format)");
    expect(mockLibrary().binders.has('Inkspace')).toBe(true);
  });

  it('knows the notes are already there when the dialog opens again', async () => {
    dialog();
    await found();
    button('Importer 4 notes').click();
    await vi.waitFor(() => expect(slot('inkspace-report')).toBeTruthy());
    cleanup?.();

    // The four readable notes are in the library; the damaged one cannot be imported.
    dialog();
    await vi.waitFor(() => expect(slot('inkspace-count')?.textContent).toContain('5 notes trouvées'));
    expect(slot('inkspace-count')?.textContent).toContain('dont 4 déjà dans la bibliothèque (elles seront ignorées)');
    expect(text()).toContain("Les notes lisibles d'Inkspace sont toutes déjà dans la bibliothèque");
    expect(button('Importer 0 notes').disabled).toBe(true);
    cleanup?.();

    // Everything in the library: nothing to do.
    vi.spyOn(api, 'inkspaceInspect').mockResolvedValue({ path: FOUND, notes: 5, alreadyImported: 5, unreadable: 0 });
    dialog();
    await found();
    expect(text()).toContain("Toutes les notes d'Inkspace sont déjà dans la bibliothèque");
    expect(button('Importer 0 notes').disabled).toBe(true);
    expect(document.querySelector('input[list="inkspace-binders"]')).toBeNull();
  });

  it('imports into the binder typed, proposes the existing ones, and the root when it is empty', async () => {
    const spy = vi.spyOn(api, 'inkspaceImport');
    dialog({ binders: ['Classe', 'Classe/Sciences'] });
    await found();
    expect([...document.querySelectorAll('#inkspace-binders option')].map((o) => o.getAttribute('value'))).toEqual([
      'Classe',
      'Classe/Sciences',
    ]);
    await type(binderField(), ' Classe/Sciences ');
    button('Importer 4 notes').click();
    await vi.waitFor(() => expect(slot('inkspace-report')).toBeTruthy());
    expect(spy).toHaveBeenCalledWith(FOUND, 'Classe/Sciences', expect.any(Function));
    expect(slot('inkspace-report')!.textContent).toContain('4 notes importées dans le classeur « Classe/Sciences »');
    cleanup?.();

    resetMockInkspace();
    mockInkspace.stepMs = 0;
    dialog();
    await found();
    await type(binderField(), '');
    button('Importer 4 notes').click();
    await vi.waitFor(() => expect(slot('inkspace-report')).toBeTruthy());
    expect(spy).toHaveBeenLastCalledWith(FOUND, '', expect.any(Function));
    // The second import skips the notes the first one made: the report says so, at the root.
    expect(slot('inkspace-report')!.textContent).toContain('4 notes importées à la racine de la bibliothèque');
  });

  it('asks for a folder when Inkspace is not found, and counts the notes of the one chosen', async () => {
    mockInkspace.detected = null;
    dialog();
    await vi.waitFor(() => expect(slot('inkspace-not-found')?.textContent).toContain("n'a pas trouvé Wacom Inkspace"));
    expect(slot('inkspace-path')).toBeNull();
    expect(slot('inkspace-count')).toBeNull();
    expect(buttons()).not.toContain('Importer 4 notes');

    (window as E2eWindow).__ardoiseE2eFolderPath = 'D:\\Copie\\LocalState\\WacomCloudDb';
    button('Choisir un dossier…').click();
    await vi.waitFor(() => expect(slot('inkspace-count')?.textContent).toContain('5 notes trouvées'));
    // The folder is shown as the LocalState folder the Rust side normalised it to.
    expect(slot('inkspace-path')?.textContent).toBe('D:\\Copie\\LocalState');
    expect(slot('inkspace-not-found')).toBeNull();
    expect(button('Importer 4 notes').disabled).toBe(false);
    expect(buttons()).toContain('Choisir un autre dossier…');
  });

  it('changes nothing when the folder picker is cancelled', async () => {
    vi.spyOn(window, 'prompt').mockReturnValue(null);
    dialog();
    await found();
    button('Choisir un autre dossier…').click();
    await tick();
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(slot('inkspace-path')?.textContent).toBe(FOUND);
    expect(slot('inkspace-count')?.textContent).toContain('5 notes trouvées');
  });

  it('says why a folder is not Inkspace’s, and offers to try again or choose another', async () => {
    mockInkspace.detected = null;
    const inspect = vi.spyOn(api, 'inkspaceInspect');
    dialog();
    await vi.waitFor(() => expect(slot('inkspace-not-found')).toBeTruthy());
    (window as E2eWindow).__ardoiseE2eFolderPath = 'C:\\Users\\demo\\Documents';
    button('Choisir un dossier…').click();
    await vi.waitFor(() => expect(alertText()).toContain("ce dossier ne contient pas de notes d'Inkspace"));
    expect(alertText()).toContain('(C:\\Users\\demo\\Documents)');
    expect(slot('inkspace-path')?.textContent).toBe('C:\\Users\\demo\\Documents');
    expect(buttons()).not.toContain('Importer 4 notes');

    button('Réessayer').click();
    await vi.waitFor(() => expect(inspect).toHaveBeenCalledTimes(2));
    await vi.waitFor(() => expect(alertText()).toContain("ce dossier ne contient pas de notes d'Inkspace"));

    (window as E2eWindow).__ardoiseE2eFolderPath = FOUND;
    button('Choisir un autre dossier…').click();
    await found();
    expect(document.querySelector('[role="alert"]')).toBeNull();
  });

  it('shows the translated reason when the detected folder cannot be read', async () => {
    vi.spyOn(api, 'inkspaceInspect').mockRejectedValue({ code: 'inkspaceUnreadable', message: 'fichier verrouillé' });
    dialog();
    await vi.waitFor(() => expect(alertText()).toContain('fermez Inkspace puis réessayez'));
    expect(alertText()).toContain('(fichier verrouillé)');
    expect(slot('inkspace-path')?.textContent).toBe(FOUND);
    expect(buttons()).toContain('Réessayer');
  });

  it('stops on request and says the rest can be added later', async () => {
    const held = gate();
    mockInkspace.hold = { before: 2, until: held.until };
    const cancel = vi.spyOn(api, 'inkspaceCancel');
    const { onshow } = dialog();
    await found();
    button('Importer 4 notes').click();
    await vi.waitFor(() => expect(status()).toContain('Import des notes : 2 sur 4…'));

    button("Arrêter l'import").click();
    await vi.waitFor(() => expect(status()).toContain('Arrêt après la note en cours…'));
    expect(cancel).toHaveBeenCalledTimes(1);
    expect(button("Arrêter l'import").disabled).toBe(true);

    held.open();
    await vi.waitFor(() => expect(slot('inkspace-report')).toBeTruthy());
    const report = slot('inkspace-report')!.textContent!;
    expect(report).toContain('Import arrêté');
    expect(report).toContain('2 notes importées dans le classeur « Inkspace »');
    expect(report).toContain("Relancez l'import pour ajouter les autres");
    // The note that cannot be read is reported whatever happens to the others.
    expect(document.querySelectorAll('li')).toHaveLength(1);

    button('Voir les notes importées').click();
    expect(onshow).toHaveBeenCalledWith('Inkspace');
  });

  it('returns to the folder when the import is refused, and closes from the report', async () => {
    vi.spyOn(api, 'inkspaceImport').mockRejectedValueOnce({ code: 'inkspaceBusy', message: '' });
    const { onclose, onshow } = dialog();
    await found();
    button('Importer 4 notes').click();
    await vi.waitFor(() => expect(alertText()).toContain('un import depuis Inkspace est déjà en cours'));
    expect(button('Importer 4 notes').disabled).toBe(false);
    expect(slot('inkspace-report')).toBeNull();

    button('Importer 4 notes').click();
    await vi.waitFor(() => expect(slot('inkspace-report')).toBeTruthy());
    expect(document.querySelector('[role="alert"]')).toBeNull();
    button('Fermer').click();
    expect(onclose).toHaveBeenCalledTimes(1);
    expect(onshow).not.toHaveBeenCalled();
  });

  it('only offers to show the notes when some were imported', async () => {
    vi.spyOn(api, 'inkspaceImport').mockResolvedValue({
      binder: 'Inkspace',
      total: 3,
      imported: 0,
      skipped: 2,
      failed: [{ inkspaceId: '0123456789abcdef', created: Date.UTC(2026, 0, 12, 9, 5), code: 'libraryAccess', message: 'disque plein' }],
      cancelled: false,
    });
    dialog();
    await found();
    button('Importer 4 notes').click();
    await vi.waitFor(() => expect(slot('inkspace-report')).toBeTruthy());
    const report = slot('inkspace-report')!.textContent!;
    expect(report).toContain("Aucune note n'a été importée.");
    expect(report).toContain('2 déjà présentes, ignorées');
    expect(report).toContain('01234567');
    expect(report).not.toContain('89abcdef');
    expect(report).toContain('la bibliothèque ne peut pas être lue ou modifiée (disque plein)');
    // A known date is written in the language of the interface.
    expect(report).toMatch(/2026/);
    expect(buttons()).not.toContain('Voir les notes importées');
  });

  it('speaks English', async () => {
    setLocale('en');
    dialog();
    await vi.waitFor(() => expect(slot('inkspace-count')?.textContent).toContain('5 notes found'));
    button('Import 4 notes').click();
    await vi.waitFor(() => expect(slot('inkspace-report')).toBeTruthy());
    const report = slot('inkspace-report')!.textContent!;
    expect(report).toContain('4 notes imported into the binder “Inkspace”');
    expect(report).toContain('1 note could not be imported:');
    expect(report).toContain('this Inkspace note cannot be read (unknown stroke format)');
    expect(buttons()).toContain('Show the imported notes');
  });
});

describe('the library and the first-run offer', () => {
  async function home() {
    const target = place(Home, { app, onchange: vi.fn() });
    await vi.waitFor(() => expect(target.querySelectorAll('.note-card').length).toBeGreaterThan(0));
    return target;
  }

  it('opens from the sidebar, and shows the binder that received the notes', async () => {
    const target = await home();
    const entry = target.querySelector<HTMLButtonElement>('[data-action="import-inkspace"]')!;
    expect(entry.textContent).toContain('Importer depuis Inkspace…');
    expect(slot('inkspace-path')).toBeNull();

    entry.click();
    await found();
    button('Importer 4 notes').click();
    await vi.waitFor(() => expect(slot('inkspace-report')).toBeTruthy());
    button('Voir les notes importées').click();
    await vi.waitFor(() => expect(slot('inkspace-path')).toBeNull());
    await vi.waitFor(() =>
      expect(target.querySelector('[data-binder="Inkspace"]')?.getAttribute('aria-current')).toBe('true'),
    );
    expect(target.querySelectorAll('.note-card')).toHaveLength(4);
  });

  it('opens by itself once when the wizard offered the import and the user agreed', async () => {
    inkspaceOffer.pending = true;
    await home();
    await found();
    expect(inkspaceOffer.pending).toBe(false);
    // Closing leaves the library as it was: the user decides when to import.
    button('Annuler').click();
    await vi.waitFor(() => expect(slot('inkspace-path')).toBeNull());
    expect(mockLibrary().binders.has('Inkspace')).toBe(false);
  });
});
