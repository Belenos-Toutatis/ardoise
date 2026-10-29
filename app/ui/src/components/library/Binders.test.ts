// @vitest-environment jsdom
// Milestone M10: renaming and deleting binders from the sidebar.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { deepestFolders } from '../../lib/library-activity.svelte';
import { mockLibrary, resetMockLibrary } from '../../lib/mock-data';
import { mockForeignFiles } from '../../lib/mock-organize';
import { renderHome, type Ui } from './library-test-helpers';

let ui: Ui | null = null;

beforeEach(() => {
  resetMockLibrary();
  mockForeignFiles.clear();
});

afterEach(() => {
  ui?.destroy();
  ui = null;
  vi.restoreAllMocks();
});

const binderItem = (path: string) => ui!.q(`.sidebar .nav-item[data-binder="${path}"]`);
const binderOf = (title: string) => [...mockLibrary().notes.values()].find((s) => s.note.title === title)?.binder;

/** Opens the binder's menu and chooses an action. */
async function act(path: string, label: string) {
  const row = binderItem(path)!.closest('.nav-row')!;
  await ui!.click(row.querySelector('.menu .icon-button'));
  await ui!.click(ui!.button(label, row));
}

describe('renaming a binder', () => {
  it('renames it in place, its notes with it', async () => {
    ui = await renderHome();
    await act('Perso', 'Renommer…');
    const input = ui.byLabel('Nouveau nom du classeur « Perso »') as HTMLInputElement;
    expect(input.value).toBe('Perso');
    await ui.type(input, 'Personnel');
    await ui.click(ui.button('Renommer'));
    await vi.waitFor(() => expect(binderItem('Personnel')).not.toBeNull());
    expect(binderItem('Perso')).toBeNull();
    expect(document.activeElement).toBe(binderItem('Personnel'));
    expect(binderOf('Réunion parents')).toBe('Personnel');
    expect(ui.q('.binder-editor')).toBeNull();
  });

  it('never takes the name of another binder, and says so by the field', async () => {
    ui = await renderHome();
    await act('Perso', 'Renommer…');
    await ui.type(ui.byLabel('Nouveau nom du classeur « Perso »') as HTMLInputElement, 'Classe');
    await ui.click(ui.button('Renommer'));
    await vi.waitFor(() => expect(ui!.q('.binder-error')?.textContent).toContain('un classeur ou un fichier porte déjà ce nom'));
    expect(binderOf('Réunion parents')).toBe('Perso');
    expect(binderOf('Cours de physique')).toBe('Classe/3e B');
    // Escape gives up.
    await ui.key(ui.byLabel('Nouveau nom du classeur « Perso »'), 'Escape');
    expect(ui.q('.binder-editor')).toBeNull();
  });

  it('keeps showing the binder being viewed under its new name', async () => {
    ui = await renderHome();
    await ui.click(binderItem('Classe'));
    await ui.click(ui.byLabel('Déplier Classe'));
    await ui.click(binderItem('Classe/3e B'));
    expect(ui.q('#library-heading')?.textContent).toBe('3e B');
    await act('Classe', 'Renommer…');
    await ui.type(ui.byLabel('Nouveau nom du classeur « Classe »') as HTMLInputElement, 'Physique');
    await ui.click(ui.button('Renommer'));
    await vi.waitFor(() => expect(binderItem('Physique/3e B')?.getAttribute('aria-current')).toBe('true'));
    expect(ui.q('#library-heading')?.textContent).toBe('3e B');
    expect(ui.titles()).toEqual(['Cours de physique']);
  });
});

describe('deleting a binder', () => {
  it('asks first, says how many notes go to the trash, and « Annuler » brings it all back', async () => {
    ui = await renderHome();
    await act('Classe', 'Supprimer…');
    const confirm = ui.q('[role="alertdialog"]')!;
    expect(confirm.getAttribute('aria-label')).toBe('Supprimer le classeur « Classe »');
    expect(confirm.textContent).toContain(
      'Supprimer le classeur « Classe » ? Sa note va à la corbeille, puis son dossier est supprimé. Son sous-classeur est inclus.',
    );
    expect(confirm.textContent).toContain('Une note restaurée depuis la corbeille retrouve son classeur.');
    // The harmless choice has the focus: Enter does not delete.
    expect(document.activeElement?.textContent?.trim()).toBe('Annuler');
    // Nothing happens before the confirmation.
    expect(mockLibrary().trash.size).toBe(0);
    await ui.click(ui.button('Annuler', confirm));
    expect(ui.q('[role="alertdialog"]')).toBeNull();
    expect(document.activeElement).toBe(binderItem('Classe'));

    await act('Classe', 'Supprimer…');
    await ui.click(ui.button('Supprimer le classeur'));
    await vi.waitFor(() => expect(binderItem('Classe')).toBeNull());
    expect(document.activeElement).toBe(ui.q('.sidebar [data-view="all"]'));
    expect(mockLibrary().trash.size).toBe(1);
    expect(ui.titles()).toEqual(['Réunion parents', 'Liste de courses']);
    const offer = ui.q('.activity-offer')!;
    expect(offer.textContent).toContain('Classeur « Classe » supprimé : 1 note mise à la corbeille.');

    await ui.click(ui.button('Annuler', offer));
    await vi.waitFor(() => expect(binderItem('Classe')).not.toBeNull());
    expect(binderOf('Cours de physique')).toBe('Classe/3e B');
    expect(mockLibrary().trash.size).toBe(0);
    expect(ui.q('.activity-offer')?.textContent).toContain('Classeur « Classe » rétabli avec sa note.');
  });

  it('counts every note, in English too', async () => {
    const courses = [...mockLibrary().notes].find(([, s]) => s.note.title === 'Liste de courses')![0];
    await api.moveNotes([courses], 'Perso');
    ui = await renderHome('en');
    await act('Perso', 'Delete…');
    expect(ui.q('[role="alertdialog"]')?.textContent).toContain(
      'Delete the binder "Perso"? Its 2 notes go to the trash, then its folder is removed.',
    );
    await ui.click(ui.button('Delete the binder'));
    await vi.waitFor(() => expect(ui!.q('.activity-offer')?.textContent).toContain('Binder "Perso" deleted: 2 notes moved to the trash.'));
  });

  it('removes an empty binder, and can bring it back', async () => {
    await api.createBinder('Vide');
    ui = await renderHome();
    await act('Vide', 'Supprimer…');
    expect(ui.q('[role="alertdialog"]')?.textContent).toContain('Il ne contient aucune note : son dossier sera supprimé.');
    await ui.click(ui.button('Supprimer le classeur'));
    await vi.waitFor(() => expect(binderItem('Vide')).toBeNull());
    expect(ui.q('.activity-offer')?.textContent).toContain('Classeur « Vide » supprimé.');
    await ui.click(ui.button('Annuler', ui.q('.activity-offer')!));
    await vi.waitFor(() => expect(binderItem('Vide')).not.toBeNull());
    expect(ui.q('.activity-offer')?.textContent).toContain('Classeur « Vide » rétabli.');
  });

  it('says which folders stay because they hold other files', async () => {
    mockForeignFiles.add('Perso');
    ui = await renderHome();
    await ui.click(binderItem('Perso'));
    await act('Perso', 'Supprimer…');
    await ui.click(ui.button('Supprimer le classeur'));
    await vi.waitFor(() => expect(ui!.q('.activity-offer')).not.toBeNull());
    expect(ui.q('.activity-offer')?.textContent).toContain(
      "Le dossier « Perso » n'a pas pu être supprimé : il contient encore des fichiers, ou il est en cours d'utilisation.",
    );
    // The view of the deleted binder gives way to all the notes.
    expect(ui.q('.sidebar [data-view="all"]')?.getAttribute('aria-current')).toBe('true');
    expect(binderOf('Réunion parents')).toBeUndefined();
  });

  it('reports a failure to delete it, translated', async () => {
    vi.spyOn(api, 'deleteBinder').mockRejectedValue({ code: 'binderNotFound', message: 'Perso' });
    ui = await renderHome();
    await act('Perso', 'Supprimer…');
    await ui.click(ui.button('Supprimer le classeur'));
    await vi.waitFor(() => expect(ui!.q('[role="alert"]')?.textContent).toContain("ce classeur n'existe plus"));
  });

  it('lists the notes that could not go to the trash', async () => {
    vi.spyOn(api, 'deleteBinder').mockResolvedValue({
      trashed: [],
      failed: [{ path: 'C:\\demo\\Ardoise\\Perso\\2026-10-23 Réunion parents.ardoise', error: { code: 'libraryAccess', message: 'denied' } }],
      kept: ['Perso'],
    });
    ui = await renderHome();
    await act('Perso', 'Supprimer…');
    await ui.click(ui.button('Supprimer le classeur'));
    await vi.waitFor(() => expect(ui!.q('.activity-failures')).not.toBeNull());
    expect(ui.q('.activity-failures')?.textContent).toContain("Une note n'a pas pu être mise à la corbeille (elle reste dans son classeur) :");
    expect(ui.q('.activity-failures')?.textContent).toContain('« 2026-10-23 Réunion parents.ardoise » : la bibliothèque ne peut pas être lue ou modifiée');
  });
});

describe('kept folders', () => {
  it('names only the deepest ones', () => {
    expect(deepestFolders(['Classe/Vieux', 'Classe/3e B/TP', 'Classe/3e B', 'Classe'])).toEqual(['Classe/Vieux', 'Classe/3e B/TP']);
    expect(deepestFolders(['Classe'])).toEqual(['Classe']);
    expect(deepestFolders([])).toEqual([]);
  });
});
