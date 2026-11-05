// Journey « page précédente / page suivante » of the editor (one note is one page).
//
// On the production executable and the demo library, in French and in English: the real window is driven with real
// clicks and real key events. The buttons sit in the editor's top bar and walk through the notes in the order the
// library showed them.
//
// 1. Open the second card: the position reads « 2 / 8 », both buttons work. Next and Previous (buttons, then Alt + →
//    and Alt + ←) change the note in the same editor, nothing is asked while nothing is unsaved. Alt + → typed in the
//    title field does nothing.
// 2. A drawn stroke, then Next: the dialog offers to save and open the other note; the answer saves the first note
//    (it is found again, with its stroke) and opens the next one. A second change, then Previous: « Rester » keeps
//    everything, « Ouvrir sans enregistrer » lets the change go.
// 3. Next once more, then back to the library: the focus is on the card of the note the editor ended on, not on the
//    one it started with.
import { EDITOR_STROKES, libraryBack, text } from './journeys-m12-resume.mjs';

const j = JSON.stringify;
const PREVIOUS = 'button[data-action="page-previous"]';
const NEXT = 'button[data-action="page-next"]';
const POSITION = '[data-slot="page-position"]';
const DIALOG = '[role="alertdialog"]';

const editorPath = (app) => app.evaluate(`document.querySelector('.editor')?.dataset.path ?? null`);
const strokeCount = (app) => app.evaluate(`document.querySelectorAll(${j(EDITOR_STROKES)}).length`);

/** The editor shows the note at `path`, drawn (its strokes are there). */
async function shows(app, path, what) {
  await app.waitFor(`document.querySelector('.editor')?.dataset.path === ${j(path)}`, `${what}: the editor on this note`);
  await app.waitFor(`document.querySelectorAll(${j(EDITOR_STROKES)}).length > 0`, `${what}: its strokes`);
}

/** Draws one stroke across the page with the mouse. */
async function draw(app) {
  const box = await app.evaluate(`(() => { const r = document.querySelector('.editor svg').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`);
  const points = Array.from({ length: 25 }, (_, i) => ({
    x: box.x + box.w * (0.3 + 0.4 * (i / 24)),
    y: box.y + box.h * (0.5 + 0.08 * Math.sin(i / 3)),
  }));
  await app.drag(points);
}

export const editorNavJourneys = [
  {
    name: 'pages de l’éditeur : page précédente et page suivante, boutons et Alt + flèches, enregistrer ou non, retour à la bibliothèque',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let paths;
      let strokes;

      await check('la bibliothèque de démonstration s’affiche : on retient l’ordre des cartes', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        paths = await app.evaluate(`[...document.querySelectorAll('.note-card')].map((c) => c.dataset.path)`);
        if (new Set(paths).size !== 8) throw new Error(`unexpected cards ${paths}`);
      });

      await check('ouvrir la deuxième carte : « 2 / 8 », les deux boutons sont actifs, avec leur nom', async () => {
        await app.click('.note-card:nth-child(2) .card-open');
        await shows(app, paths[1], 'second card');
        await app.waitFor(`document.querySelector(${j(POSITION)})?.textContent.trim() === '2 / 8'`, 'the position 2 / 8');
        const state = await app.evaluate(`({
          previous: document.querySelector(${j(PREVIOUS)}).disabled,
          next: document.querySelector(${j(NEXT)}).disabled,
          previousLabel: document.querySelector(${j(PREVIOUS)}).getAttribute('aria-label'),
          nextLabel: document.querySelector(${j(NEXT)}).getAttribute('aria-label'),
          inTopBar: document.querySelector(${j(PREVIOUS)}).closest('.editor-top') !== null,
        })`);
        if (state.previous || state.next || !state.inTopBar) throw new Error(`unexpected buttons ${JSON.stringify(state)}`);
        if (state.previousLabel !== l('editor.pages.previous') || state.nextLabel !== l('editor.pages.next')) {
          throw new Error(`unexpected labels ${JSON.stringify(state)}`);
        }
      });

      await check('« Page suivante » : la troisième note s’ouvre dans le même éditeur, sans question', async () => {
        await app.click(NEXT);
        await shows(app, paths[2], 'third card');
        await app.waitFor(`document.querySelector(${j(POSITION)})?.textContent.trim() === '3 / 8'`, 'the position 3 / 8');
        if (await app.evaluate(`document.querySelector(${j(DIALOG)}) !== null`)) throw new Error('a dialog was opened');
      });

      await check('« Page précédente » : la deuxième note revient', async () => {
        await app.click(PREVIOUS);
        await shows(app, paths[1], 'back on the second card');
        await app.waitFor(`document.querySelector(${j(POSITION)})?.textContent.trim() === '2 / 8'`, 'the position 2 / 8');
        strokes = await strokeCount(app);
      });

      await check('Alt + → puis Alt + ← font de même', async () => {
        await app.key('ArrowRight', { alt: true });
        await shows(app, paths[2], 'Alt + right');
        await app.key('ArrowLeft', { alt: true });
        await shows(app, paths[1], 'Alt + left');
      });

      await check('Alt + → tapé dans le champ du titre ne change pas de page', async () => {
        await app.click('.title-field input');
        await app.key('ArrowRight', { alt: true });
        await new Promise((resolve) => setTimeout(resolve, 400));
        if ((await editorPath(app)) !== paths[1]) throw new Error('the page changed while typing in the title');
        // Leave the field as it was (Escape gives the title back and leaves it).
        await app.key('Escape');
      });

      await check('un trait tracé, puis « Page suivante » : la question s’ouvre, « enregistrer et ouvrir l’autre note » enregistre puis ouvre', async () => {
        await draw(app);
        await app.waitFor(`document.querySelectorAll(${j(EDITOR_STROKES)}).length === ${strokes + 1}`, 'the new stroke');
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'yes'`, 'the unsaved mark');
        await app.click(NEXT);
        await app.waitFor(`document.querySelector(${j(DIALOG)}) !== null`, 'the unsaved changes dialog');
        const wording = await app.evaluate(`[...document.querySelectorAll(${j(`${DIALOG} button`)})].map((b) => b.textContent.trim())`);
        const expected = [l('editor.leave.saveSwitch'), l('editor.leave.discardSwitch'), l('editor.leave.cancel')];
        if (JSON.stringify(wording) !== JSON.stringify(expected)) throw new Error(`unexpected answers ${JSON.stringify(wording)}`);
        if ((await editorPath(app)) !== paths[1]) throw new Error('the note changed before the answer');
        await app.click(`${DIALOG} button`, l('editor.leave.saveSwitch'));
        await shows(app, paths[2], 'third card after saving');
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'no'`, 'the next note, unmodified');
      });

      await check('la deuxième note a gardé son trait', async () => {
        await app.click(PREVIOUS);
        await shows(app, paths[1], 'second card again');
        await app.waitFor(`document.querySelectorAll(${j(EDITOR_STROKES)}).length === ${strokes + 1}`, 'the saved stroke');
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'no'`, 'nothing unsaved');
      });

      await check('une autre modification, « Page précédente », « Rester » : tout reste en place', async () => {
        await draw(app);
        await app.waitFor(`document.querySelectorAll(${j(EDITOR_STROKES)}).length === ${strokes + 2}`, 'the second stroke');
        await app.click(PREVIOUS);
        await app.waitFor(`document.querySelector(${j(DIALOG)}) !== null`, 'the dialog');
        await app.click(`${DIALOG} button`, l('editor.leave.cancel'));
        await app.waitFor(`document.querySelector(${j(DIALOG)}) === null`, 'the dialog closed');
        if ((await editorPath(app)) !== paths[1]) throw new Error('the note changed after « stay »');
        if ((await strokeCount(app)) !== strokes + 2) throw new Error('the change was lost after « stay »');
        if (!(await app.evaluate(`document.querySelector('.editor').dataset.dirty === 'yes'`))) throw new Error('the unsaved mark went');
      });

      await check('« Ouvrir sans enregistrer » : la note précédente s’ouvre, la modification est abandonnée', async () => {
        await app.click(PREVIOUS);
        await app.waitFor(`document.querySelector(${j(DIALOG)}) !== null`, 'the dialog');
        await app.click(`${DIALOG} button`, l('editor.leave.discardSwitch'));
        await shows(app, paths[0], 'first card');
        await app.waitFor(`document.querySelector(${j(POSITION)})?.textContent.trim() === '1 / 8'`, 'the position 1 / 8');
        await app.waitFor(`document.querySelector(${j(PREVIOUS)}).disabled === true`, '« previous » disabled on the first note');
        await app.click(NEXT);
        await shows(app, paths[1], 'second card, without the abandoned change');
        await app.waitFor(`document.querySelectorAll(${j(EDITOR_STROKES)}).length === ${strokes + 1}`, 'the saved stroke only');
      });

      await check('retour à la bibliothèque : le focus est sur la carte de la note ouverte en dernier, pas sur celle du départ', async () => {
        await app.click(NEXT);
        await shows(app, paths[2], 'third card, last one opened');
        await app.click('.editor-top button');
        await libraryBack(app);
        await app.waitFor(
          `document.activeElement?.closest('.note-card')?.dataset.path === ${j(paths[2])}`,
          'the focus on the card of the third note',
        );
      });
    },
  },
];
