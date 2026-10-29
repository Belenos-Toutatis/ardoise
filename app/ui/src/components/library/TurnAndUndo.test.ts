// @vitest-environment jsdom
// Milestone M10: turning several notes from the selection bar, and « Annuler » after a turn or
// after « Mettre à la corbeille ».
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { api } from '../../lib/api';
import { rotate, docFromNote } from '../../lib/editor/state';
import { rotateQuarter } from '../../lib/editor/ops';
import { undoNotice } from '../../lib/library-activity.svelte';
import { freshStamp, mockLibrary, resetMockLibrary, type StoredNote } from '../../lib/mock-data';
import type { Note } from '../../lib/note';
import { renderHome, type Ui } from './library-test-helpers';

let ui: Ui | null = null;

beforeEach(() => {
  resetMockLibrary();
  undoNotice.durationMs = 8000;
});

afterEach(() => {
  ui?.destroy();
  ui = null;
  vi.restoreAllMocks();
  vi.useRealTimers();
});

function stored(title: string): StoredNote {
  const found = [...mockLibrary().notes.values()].find((s) => s.note.title === title);
  if (!found) throw new Error(`no note ${title}`);
  return found;
}

function pathOf(title: string): string {
  const found = [...mockLibrary().notes].find(([, s]) => s.note.title === title);
  if (!found) throw new Error(`no note ${title}`);
  return found[0];
}

/** What a turn may change: the page, the ink and the background image. */
const drawing = (note: Note) => JSON.parse(JSON.stringify({ page: note.page, strokes: note.strokes, image: note.backgroundImage ?? null }));

const copyOf = (note: Note): Note => JSON.parse(JSON.stringify(note));

/**
 * Turned back, a note is the same up to the rounding of `height - (height - y)` (a millionth of a
 * device unit here; Rust stores f32 samples, a fraction of a micrometre on paper).
 */
function expectSameDrawing(actual: Note, expected: Note) {
  expect(actual.page).toEqual(expected.page);
  expect(actual.backgroundImage ?? null).toEqual(expected.backgroundImage ?? null);
  expect(actual.strokes.length).toBe(expected.strokes.length);
  actual.strokes.forEach((stroke, i) => {
    const points = expected.strokes[i].points;
    expect(stroke.points.length).toBe(points.length);
    stroke.points.forEach((p, j) => {
      expect(p.x).toBeCloseTo(points[j].x, 6);
      expect(p.y).toBeCloseTo(points[j].y, 6);
      expect([p.t, p.pressure]).toEqual([points[j].t, points[j].pressure]);
    });
  });
}

async function turn(direction: 'left' | 'right') {
  await ui!.click(ui!.q('.selection-bar [data-action="turn"]'));
  await ui!.click(ui!.q(`.selection-bar [data-action="${direction === 'left' ? 'turn-ccw' : 'turn-cw'}"]`));
  await vi.waitFor(() => expect(ui!.q('.activity-progress')).toBeNull());
}

describe('turning several notes', () => {
  it('turns page, ink and background image exactly as the editor does, then « Annuler » turns them back', async () => {
    // One note with a background image: it must turn with the ink.
    stored('Cours de physique').note.backgroundImage = {
      name: 'fiche.png',
      rect: { x: 1000, y: 2000, width: 12000, height: 8000 },
      quarterTurns: 1,
    };
    const before = { physique: copyOf(stored('Cours de physique').note), courses: copyOf(stored('Liste de courses').note) };
    ui = await renderHome();
    await ui.select('Cours de physique', 'Liste de courses');
    expect(ui.q('.selection-bar')?.textContent).toContain('Sélection : 2');

    await turn('right');
    for (const [title, note] of [
      ['Cours de physique', before.physique],
      ['Liste de courses', before.courses],
    ] as const) {
      const now = stored(title).note;
      // The library's turn and the editor's « Tourner la feuille avec l'écriture » agree.
      expect(drawing(now)).toEqual(drawing(rotateQuarter(note, 'cw')));
      expect(drawing(now)).toEqual(drawing(rotate(docFromNote(note), 'cw').note));
      expect(now.page).toEqual({ width: note.page.height, height: note.page.width });
      expect(now.title).toBe(note.title);
    }
    expect(stored('Cours de physique').note.backgroundImage?.quarterTurns).toBe(2);
    // The note that was not selected did not change.
    expect(stored('Réunion parents').note.page).toEqual({ width: 29700, height: 21600 });

    const offer = ui.q('.activity-offer');
    expect(offer?.textContent).toContain("2 notes tournées d'un quart de tour à droite.");
    // The selection stays, so that another turn is one click away.
    expect(ui.q('.selection-bar')?.textContent).toContain('Sélection : 2');

    await ui.click(ui.button('Annuler', offer!));
    await vi.waitFor(() => expect(ui!.q('.activity-offer')?.textContent).toContain("Les 2 notes ont retrouvé leur sens d'origine."));
    expectSameDrawing(stored('Cours de physique').note, before.physique);
    expectSameDrawing(stored('Liste de courses').note, before.courses);
    expect(ui.q('.activity-offer [data-action="undo"]')).toBeNull();
  });

  it('turns to the left, in English too', async () => {
    const before = copyOf(stored('Réunion parents').note);
    ui = await renderHome('en');
    await ui.select('Réunion parents');
    await ui.click(ui.q('.selection-bar [data-action="turn"]'));
    expect(ui.byLabel('Turn the selected notes').textContent).toContain('A quarter turn to the left');
    await ui.click(ui.q('.selection-bar [data-action="turn-ccw"]'));
    await vi.waitFor(() => expect(ui!.q('.activity-offer')?.textContent).toContain('1 note turned a quarter turn to the left.'));
    expect(drawing(stored('Réunion parents').note)).toEqual(drawing(rotateQuarter(before, 'ccw')));
    expect(ui.button('Undo', ui.q('.activity-offer')!)).toBeTruthy();
  });

  it('saves each note with the stamp check and lists the notes that failed, translated', async () => {
    const before = copyOf(stored('Liste de courses').note);
    const beforeMeeting = copyOf(stored('Réunion parents').note);
    const save = api.saveNote;
    vi.spyOn(api, 'saveNote').mockImplementation((path, note, stamp, key) => {
      // Another computer saved « Liste de courses » between the reading and the writing.
      if (path === pathOf('Liste de courses')) stored('Liste de courses').stamp = freshStamp(4242);
      return save(path, note, stamp, key);
    });
    ui = await renderHome();
    await ui.select('Cours de physique', 'Liste de courses', 'Réunion parents');
    await turn('right');

    const failures = ui.q('.activity-failures');
    expect(failures?.getAttribute('role')).toBe('alert');
    expect(failures?.textContent).toContain("Une note n'a pas pu être tournée :");
    expect(failures?.textContent).toContain('« Liste de courses » : la note a été modifiée entre-temps');
    expect(drawing(stored('Liste de courses').note)).toEqual(drawing(before));
    expect(ui.q('.activity-offer')?.textContent).toContain('2 notes tournées');

    // The undo turns back only the notes that were turned.
    await ui.click(ui.button('Annuler', ui.q('.activity-offer')!));
    await vi.waitFor(() => expect(ui!.q('.activity-offer')?.textContent).toContain('Les 2 notes ont retrouvé'));
    expect(stored('Cours de physique').note.page).toEqual({ width: 29700, height: 21600 });
    expect(drawing(stored('Liste de courses').note)).toEqual(drawing(before));
    expectSameDrawing(stored('Réunion parents').note, beforeMeeting);
  });

  it('does not turn back a note changed since the turn', async () => {
    ui = await renderHome();
    await ui.select('Cours de physique', 'Réunion parents');
    await turn('left');
    const turned = drawing(stored('Réunion parents').note);
    stored('Réunion parents').stamp = freshStamp(777);
    await ui.click(ui.button('Annuler', ui.q('.activity-offer')!));
    await vi.waitFor(() => expect(ui!.q('.activity-failures')?.textContent).toContain("Une note n'a pas pu retrouver son sens d'origine :"));
    expect(ui.q('.activity-failures')?.textContent).toContain('« Réunion parents »');
    expect(drawing(stored('Réunion parents').note)).toEqual(turned);
    expect(stored('Cours de physique').note.page).toEqual({ width: 29700, height: 21600 });
  });

  it('shows the progress and stops after the note being turned', async () => {
    let release: () => void = () => {};
    const save = api.saveNote;
    vi.spyOn(api, 'saveNote').mockImplementation(async (...args) => {
      await new Promise<void>((resolve) => (release = resolve));
      return save(...args);
    });
    ui = await renderHome();
    await ui.select('Réunion parents', 'Liste de courses', 'Cours de physique');
    await ui.click(ui.q('.selection-bar [data-action="turn"]'));
    await ui.click(ui.q('.selection-bar [data-action="turn-cw"]'));

    const progress = ui.q('.activity-progress');
    expect(progress?.textContent).toContain('Rotation des notes : 0 sur 3');
    expect(ui.q<HTMLProgressElement>('.activity-progress progress')?.max).toBe(3);
    // A second turn cannot start meanwhile.
    expect(ui.q<HTMLButtonElement>('.selection-bar [data-action="turn"]')?.disabled).toBe(true);
    release();
    await vi.waitFor(() => expect(ui!.q('.activity-progress')?.textContent).toContain('1 sur 3'));
    await ui.click(ui.button('Arrêter', ui.q('.activity-progress')!));
    expect(ui.q('.activity-progress')?.textContent).toContain('Arrêt après cette note…');
    release();
    await vi.waitFor(() => expect(ui!.q('.activity-progress')).toBeNull());

    const turned = ['Réunion parents', 'Liste de courses', 'Cours de physique'].map(
      (title) => stored(title).note.page.width === 21600,
    );
    expect(turned).toEqual([true, true, false]);
    expect(ui.q('.activity-offer')?.textContent).toContain(
      "2 notes tournées d'un quart de tour à droite. Arrêté avant la fin : la dernière note n'a pas changé.",
    );
  });

  it('removes the notice after a few seconds, but not while the pointer is on it', async () => {
    undoNotice.durationMs = 30;
    ui = await renderHome();
    await ui.select('Réunion parents');
    await turn('right');
    const offer = ui.q('.activity-offer')!;
    offer.dispatchEvent(new MouseEvent('mouseenter'));
    await new Promise((resolve) => setTimeout(resolve, 80));
    expect(ui.q('.activity-offer')).not.toBeNull();
    offer.dispatchEvent(new MouseEvent('mouseleave'));
    await vi.waitFor(() => expect(ui!.q('.activity-offer')).toBeNull());
    // Gone: the note stays turned.
    expect(stored('Réunion parents').note.page.width).toBe(21600);
  });
});

describe('several operations', () => {
  it('waits for the running turn before undoing a trashing, and keeps both notices', async () => {
    let release: () => void = () => {};
    const save = api.saveNote;
    vi.spyOn(api, 'saveNote').mockImplementation(async (...args) => {
      await new Promise<void>((resolve) => (release = resolve));
      return save(...args);
    });
    ui = await renderHome();
    await ui.select('Cours de physique');
    await ui.click(ui.q('.selection-bar [data-action="turn"]'));
    await ui.click(ui.q('.selection-bar [data-action="turn-cw"]'));
    expect(ui.q('.activity-progress')).not.toBeNull();

    // Meanwhile another note goes to the trash, and its « Annuler » is pressed.
    await ui.key(ui.card('Liste de courses').querySelector('.card-open'), 'Delete');
    await vi.waitFor(() => expect(ui!.all('.note-card').length).toBe(2));
    await ui.click(ui.button('Annuler', ui.q('.activity-offer')!));
    // The restoring waits for the turn: nothing is lost, nothing is skipped.
    expect(mockLibrary().trash.size).toBe(1);
    release();
    await vi.waitFor(() => expect(mockLibrary().trash.size).toBe(0));
    await vi.waitFor(() => expect(ui!.all('.note-card').length).toBe(3));
    expect(stored('Cours de physique').note.page.width).toBe(21600);
    // Both outcomes are on display; the turn can still be undone.
    const offers = ui.all('.activity-offer').map((o) => o.textContent ?? '');
    expect(offers.some((o) => o.includes("1 note tournée d'un quart de tour à droite."))).toBe(true);
    expect(offers.some((o) => o.includes('Note restaurée.'))).toBe(true);
  });

  it('undoes the latest notice with Ctrl+Z, but never in a text field', async () => {
    ui = await renderHome();
    await ui.select('Réunion parents');
    await turn('left');
    const undo = ui.q('.activity-offer [data-action="undo"]')!;
    expect(undo.getAttribute('aria-keyshortcuts')).toBe('Control+Z');
    // In the search field, Ctrl+Z belongs to the field.
    await ui.key(ui.q('input.search'), 'z', { ctrlKey: true });
    expect(stored('Réunion parents').note.page.width).toBe(21600);
    await ui.key(document.body, 'z', { ctrlKey: true });
    await vi.waitFor(() => expect(stored('Réunion parents').note.page.width).toBe(29700));
    expect(ui.q('.activity-offer')?.textContent).toContain("La note a retrouvé son sens d'origine.");
  });
});

describe('undo after « Mettre à la corbeille »', () => {
  it('restores the notes trashed with the button, into their binders', async () => {
    ui = await renderHome();
    await ui.select('Cours de physique', 'Réunion parents');
    await ui.click(ui.button('Mettre à la corbeille', ui.q('.selection-bar')!));
    await vi.waitFor(() => expect(ui!.titles()).toEqual(['Liste de courses']));
    expect(mockLibrary().trash.size).toBe(2);
    const offer = ui.q('.activity-offer')!;
    expect(offer.textContent).toContain('2 notes mises à la corbeille.');

    await ui.click(ui.button('Annuler', offer));
    await vi.waitFor(() => expect(ui!.titles()).toEqual(['Réunion parents', 'Liste de courses', 'Cours de physique']));
    expect(mockLibrary().trash.size).toBe(0);
    expect(stored('Cours de physique').binder).toBe('Classe/3e B');
    expect(stored('Réunion parents').binder).toBe('Perso');
    expect(ui.q('.activity-offer')?.textContent).toContain('2 notes restaurées.');
  });

  it('trashes every note it can, lists the one that failed and offers to restore the others', async () => {
    ui = await renderHome();
    await ui.select('Cours de physique', 'Liste de courses', 'Réunion parents');
    // Deleted meanwhile on another computer (the listing has not caught up yet).
    mockLibrary().notes.delete(pathOf('Liste de courses'));
    await ui.click(ui.button('Mettre à la corbeille', ui.q('.selection-bar')!));
    await vi.waitFor(() => expect(ui!.q('.activity-failures')).not.toBeNull());
    expect(mockLibrary().trash.size).toBe(2);
    expect(ui.q('.activity-failures')?.textContent).toContain("Une note n'a pas pu être mise à la corbeille :");
    expect(ui.q('.activity-failures')?.textContent).toContain("« Liste de courses » : la note n'existe plus");
    expect(ui.q('.activity-offer')?.textContent).toContain('2 notes mises à la corbeille.');
    await ui.click(ui.button('Annuler', ui.q('.activity-offer')!));
    await vi.waitFor(() => expect(ui!.titles()).toEqual(['Réunion parents', 'Cours de physique']));
  });

  it('restores a note trashed with the Delete key', async () => {
    ui = await renderHome('en');
    await ui.key(ui.card('Liste de courses').querySelector('.card-open'), 'Delete');
    await vi.waitFor(() => expect(ui!.all('.note-card').length).toBe(2));
    expect(ui.q('.activity-offer')?.textContent).toContain('Note moved to the trash.');
    await ui.click(ui.button('Undo', ui.q('.activity-offer')!));
    await vi.waitFor(() => expect(ui!.all('.note-card').length).toBe(3));
    expect(ui.q('.activity-offer')?.textContent).toContain('Note restored.');
  });

  it('says which note could not be restored', async () => {
    ui = await renderHome();
    await ui.key(ui.card('Liste de courses').querySelector('.card-open'), 'Delete');
    await vi.waitFor(() => expect(ui!.all('.note-card').length).toBe(2));
    // Emptied meanwhile (on another computer, say).
    mockLibrary().trash.clear();
    await ui.click(ui.button('Annuler', ui.q('.activity-offer')!));
    await vi.waitFor(() => expect(ui!.q('.activity-failures')).not.toBeNull());
    expect(ui.q('.activity-failures')?.textContent).toContain("Une note n'a pas pu être restaurée :");
    expect(ui.q('.activity-failures')?.textContent).toContain("« Liste de courses » : cette note n'est plus dans la corbeille");
  });
});
