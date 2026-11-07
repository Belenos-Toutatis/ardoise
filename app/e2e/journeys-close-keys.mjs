// Journeys « fermer et quitter » on the production executable, in French and in English: the keys go through the real
// window, and Ctrl + Q must really end the process, which only the real app can show (the permissions of Tauri).
//
// 1. Library: with nothing open, Ctrl + W and Esc do nothing. The settings panel closes with Esc, Ctrl + W and Ctrl + F4.
// 2. Editor: Ctrl + W and Ctrl + F4 go back to the library. With a change, they ask, and answer « Rester » to their own
//    question; Ctrl + Q asks the same question. Ctrl + Q from the library ends the app.
// 3. Live mode, on a resumed note with the simulated tablet: Ctrl + W asks about ending live mode, Ctrl + Q about
//    quitting; « Quitter Ardoise » ends the app, and what the tablet wrote is in the note's file.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { LIVE_CANVAS, canvasData, libraryBack, registerTablet, text } from './journeys-m12-resume.mjs';

const j = JSON.stringify;
const QUESTION = '[role="alertdialog"]';
const SETTINGS = '.topbar button[aria-expanded]';
const CARD = '.note-card:nth-child(2)';

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** The window went away and the process ended. */
async function appEnded(app, what) {
  const deadline = Date.now() + 15_000;
  while (!app.exited()) {
    if (Date.now() > deadline) throw new Error(`${what}: the app is still running`);
    await sleep(100);
  }
}

const settingsOpen = (app) => app.evaluate(`document.querySelector(${j(SETTINGS)}).getAttribute('aria-expanded') === 'true'`);

/** Opens the settings panel with its button, and waits for what `press` does to close it. */
async function closesSettings(app, press, what) {
  await app.click(SETTINGS);
  await app.waitFor(`document.querySelector(${j(SETTINGS)}).getAttribute('aria-expanded') === 'true'`, 'the settings panel');
  await press();
  await app.waitFor(`document.querySelector(${j(SETTINGS)}).getAttribute('aria-expanded') === 'false'`, `${what} to close the settings panel`);
}

/** Opens the card in the editor, and waits until its strokes are drawn. */
async function openNote(app) {
  await app.click(`${CARD} .card-open`);
  await app.waitFor(`document.querySelectorAll('.editor g.ink path[data-stroke]').length > 0`, 'the strokes of the note');
}

/** Draws one stroke across the page with the mouse. */
async function draw(app) {
  const box = await app.evaluate(`(() => { const r = document.querySelector('.editor svg').getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; })()`);
  await app.drag(Array.from({ length: 25 }, (_, i) => ({ x: box.x + box.w * (0.3 + 0.4 * (i / 24)), y: box.y + box.h * 0.5 })));
}

export const closeKeyJourneys = [
  {
    name: 'fermer et quitter : Échap, Ctrl + W, Ctrl + F4 et Ctrl + Q, dans la bibliothèque et l’éditeur',
    async run(app, check) {
      const l = (key) => text(app.language, key);

      await check('bibliothèque seule : Ctrl + W, Ctrl + F4 et Échap ne font rien', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await app.key('w', { ctrl: true });
        await app.key('F4', { ctrl: true });
        await app.key('Escape');
        await sleep(300);
        if (app.exited()) throw new Error('the app ended');
        if (await settingsOpen(app)) throw new Error('the settings panel opened');
        await libraryBack(app);
      });

      await check('le panneau des réglages se ferme avec Échap, Ctrl + W et Ctrl + F4', async () => {
        await closesSettings(app, () => app.key('Escape'), 'Esc');
        await closesSettings(app, () => app.key('w', { ctrl: true }), 'Ctrl + W');
        await closesSettings(app, () => app.key('F4', { ctrl: true }), 'Ctrl + F4');
      });

      await check('éditeur sans changement : Ctrl + W et Ctrl + F4 reviennent à la bibliothèque', async () => {
        await openNote(app);
        await app.key('w', { ctrl: true });
        await libraryBack(app);
        await openNote(app);
        await app.key('F4', { ctrl: true });
        await libraryBack(app);
      });

      await check('éditeur avec un changement : Ctrl + W demande, et répond « Rester » à sa propre question', async () => {
        await openNote(app);
        await draw(app);
        await app.key('w', { ctrl: true });
        await app.waitFor(`document.querySelector(${j(QUESTION)}) !== null`, 'the question');
        await app.key('w', { ctrl: true });
        await app.waitFor(`document.querySelector(${j(QUESTION)}) === null`, 'the question to go');
        const editor = await app.evaluate(`document.querySelector('.editor') !== null`);
        if (!editor) throw new Error('the editor closed');
      });

      await check('Ctrl + Q avec un changement pose la même question, et l’application reste ouverte', async () => {
        await app.key('q', { ctrl: true });
        await app.waitFor(`document.querySelector(${j(QUESTION)}) !== null`, 'the question');
        const labels = await app.evaluate(`[...document.querySelectorAll(${j(`${QUESTION} .actions button`)})].map((b) => b.textContent.trim())`);
        if (!labels.includes(l('editor.leave.cancel'))) throw new Error(`buttons: ${j(labels)}`);
        await app.click(`${QUESTION} .actions button`, l('editor.leave.cancel'));
        await app.waitFor(`document.querySelector(${j(QUESTION)}) === null`, 'the question to go');
        if (app.exited()) throw new Error('the app ended');
      });

      await check('revenir sans enregistrer, puis Ctrl + Q dans la bibliothèque : l’application se termine', async () => {
        await app.key('F4', { ctrl: true });
        await app.waitFor(`document.querySelector(${j(QUESTION)}) !== null`, 'the question');
        await app.click(`${QUESTION} button.danger`);
        await libraryBack(app);
        await app.key('q', { ctrl: true });
        await appEnded(app, 'Ctrl + Q');
      });
    },
  },
  {
    name: 'fermer et quitter : mode direct, Ctrl + W puis Ctrl + Q, ce qui est écrit reste dans la note',
    args: ['--simulated-tablet'],
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let file;
      let before;
      await registerTablet(app, check);

      await check('reprendre une note en mode direct : la tablette simulée y écrit', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length >= 8`, 'the demo notes');
        const path = await app.evaluate(`document.querySelector(${j(CARD)}).dataset.path`);
        file = join(app.library, path);
        before = readFileSync(file);
        await app.click(`${CARD} .card-menu button[aria-haspopup="menu"]`);
        await app.waitFor(`document.querySelector('[role="menu"] [role="menuitem"]') !== null`, 'the card menu');
        await app.click('[role="menu"] [role="menuitem"]', l('live.resume.action'));
        await app.waitFor(`document.querySelector(${j(LIVE_CANVAS)}) !== null`, 'the live view');
        await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'the tablet to stream', 30_000);
        const start = Number(await app.evaluate(canvasData('strokes')));
        await app.waitFor(`Number(${canvasData('strokes')}) > ${start}`, 'a stroke of the simulated pen', 30_000);
      });

      await check('Ctrl + W demande s’il faut terminer, et Ctrl + F4 répond « Continuer »', async () => {
        await app.key('w', { ctrl: true });
        await app.waitFor(`document.querySelector('[data-action="leave-cancel"]') !== null`, 'the question');
        await app.key('F4', { ctrl: true });
        await app.waitFor(`document.querySelector('[data-action="leave-cancel"]') === null`, 'the question to go');
      });

      await check('Ctrl + Q demande s’il faut quitter : « Continuer » garde le mode direct', async () => {
        await app.key('q', { ctrl: true });
        await app.waitFor(`document.querySelector('[data-action="quit-cancel"]') !== null`, 'the question');
        await app.click('[data-action="quit-cancel"]');
        await app.waitFor(`document.querySelector('[data-action="quit-cancel"]') === null`, 'the question to go');
        const live = await app.evaluate(`document.querySelector('.live-view[data-state="live"]') !== null`);
        if (!live) throw new Error('live mode stopped');
      });

      await check('« Quitter Ardoise » : l’application se termine, et ce qui est écrit est dans le fichier de la note', async () => {
        await app.key('q', { ctrl: true });
        await app.waitFor(`document.querySelector('[data-action="quit-confirm"]') !== null`, 'the question');
        await app.click('[data-action="quit-confirm"]');
        await appEnded(app, '« Quitter Ardoise »');
        if (readFileSync(file).equals(before)) throw new Error('the note was not written');
      });
    },
  },
];
