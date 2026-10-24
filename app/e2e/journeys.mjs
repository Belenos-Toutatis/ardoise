// The journeys of `cargo xtask e2e` (see run.mjs). Labels come from the interface's own
// translation files, so the same journey runs in French and in English.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const STROKES = '.editor g.ink path[data-stroke]';
const fills = (app) => app.evaluate(`[...document.querySelectorAll(${JSON.stringify(STROKES)})].map((p) => p.getAttribute('fill'))`);
const cardTitles = (app) =>
  app.evaluate(`[...document.querySelectorAll('.note-card')].map((c) => c.querySelector('.card-open')?.getAttribute('aria-label') ?? c.textContent)`);

/** Types into the library search field (replacing what is there). */
async function search(app, query) {
  await app.click('input.search');
  await app.key('a', { ctrl: true });
  await app.key('Backspace');
  if (query) await app.type(query);
}

export const journeys = [
  {
    name: 'ouvrir une note, la modifier, annuler, sauvegarder, la retrouver par recherche',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const newTitle = app.language === 'fr' ? 'Énergie – parcours de bout en bout' : 'Énergie – end-to-end journey';
      let initialFills;
      let strokeCount;
      let drawnFill;

      await check('la bibliothèque de démonstration s’affiche (7 notes)', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 7`, 'the 7 demo notes');
      });

      await check('la recherche « energie » (sans accent) trouve « Énergie cinétique »', async () => {
        await search(app, 'energie');
        await app.waitFor(`document.querySelectorAll('.note-card').length === 1`, 'one search result');
        const titles = await cardTitles(app);
        if (!titles[0].includes('Énergie cinétique')) throw new Error(`unexpected result ${titles}`);
      });

      await check('ouvrir la note : l’éditeur dessine ses traits', async () => {
        await app.click('.note-card .card-open');
        await app.waitFor(`document.querySelectorAll(${JSON.stringify(STROKES)}).length > 0`, 'the strokes of the note');
        initialFills = await fills(app);
        strokeCount = initialFills.length;
      });

      await check(`modifier : couleur « ${l('editor.colors.red')} » appliquée à toute la note`, async () => {
        await app.click(`[aria-label="${l('editor.colors.red')}"]`);
        await app.click('.toolbar .apply button', l('editor.applyAll.color'));
        await app.waitFor(
          `[...document.querySelectorAll(${JSON.stringify(STROKES)})].every((p) => /216, 0, 0|#d80000/i.test(p.getAttribute('fill')))`,
          'every stroke in red',
        );
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'yes'`, 'the unsaved mark');
      });

      await check('annuler (Ctrl+Z) : les couleurs d’origine reviennent', async () => {
        await app.key('z', { ctrl: true });
        await app.waitFor(
          `JSON.stringify([...document.querySelectorAll(${JSON.stringify(STROKES)})].map((p) => p.getAttribute('fill'))) === ${JSON.stringify(JSON.stringify(initialFills))}`,
          'the original colours',
        );
      });

      await check('modifier à nouveau : tracer un trait à la souris', async () => {
        const box = await app.evaluate(`(() => { const r = document.querySelector('.editor svg').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`);
        const points = Array.from({ length: 25 }, (_, i) => ({
          x: box.x + box.w * (0.3 + 0.4 * (i / 24)),
          y: box.y + box.h * (0.5 + 0.08 * Math.sin(i / 3)),
        }));
        await app.drag(points);
        await app.waitFor(`document.querySelectorAll(${JSON.stringify(STROKES)}).length === ${strokeCount + 1}`, 'the new stroke');
        drawnFill = (await fills(app)).at(-1);
      });

      await check(`renommer la note « ${newTitle} »`, async () => {
        await app.click('.title-field input');
        await app.key('a', { ctrl: true });
        await app.type(newTitle);
        await app.key('Enter');
        await app.waitFor(`document.querySelector('.editor').dataset.path.includes(${JSON.stringify(newTitle)})`, 'the renamed file');
      });

      await check('sauvegarder (Ctrl+S) : le fichier est écrit', async () => {
        await app.key('s', { ctrl: true });
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'no'`, 'the saved state');
      });

      await check('revenir à la bibliothèque', async () => {
        await app.click('.editor-top button');
        await app.waitFor(`document.querySelector('input.search') !== null`, 'the library');
      });

      const word = app.language === 'fr' ? 'bout' : 'journey';
      await check(`la recherche « ${word.toUpperCase()} » retrouve la note renommée`, async () => {
        await search(app, word.toUpperCase());
        await app.waitFor(`document.querySelectorAll('.note-card').length === 1`, 'the renamed note in the results');
        const titles = await cardTitles(app);
        if (!titles[0].includes(newTitle)) throw new Error(`unexpected result ${titles}`);
      });

      await check('la rouvrir : le trait ajouté est enregistré, les couleurs d’origine aussi', async () => {
        await app.click('.note-card .card-open');
        await app.waitFor(`document.querySelectorAll(${JSON.stringify(STROKES)}).length === ${strokeCount + 1}`, 'the saved stroke after reopening');
        const saved = await fills(app);
        const expected = [...initialFills, drawnFill];
        if (JSON.stringify(saved) !== JSON.stringify(expected)) {
          throw new Error(`saved colours ${JSON.stringify(saved)}, expected ${JSON.stringify(expected)}`);
        }
      });
    },
  },
  {
    // The simulated tablet (`--simulated-tablet`) holds and presses its button by itself, holds three
    // pages, and draws in live mode as soon as it is asked to; its identity is a throwaway folder.
    name: 'tablette simulée : enregistrement, synchronisation, mode direct',
    args: ['--simulated-tablet'],
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const j = JSON.stringify;
      const STATUS = '.tablet-status';
      const WIZARD = '.register-wizard';
      const cards = (n) => `document.querySelectorAll('.note-card').length ${n}`;
      const step = (name) => `document.querySelector(${j(`${WIZARD} [data-step="${name}"]`)}) !== null`;
      const LIVE_CANVAS = 'canvas.live-canvas';
      const canvasData = (field) => `document.querySelector(${j(LIVE_CANVAS)})?.dataset.${field}`;

      await check('la bibliothèque de démonstration s’affiche (7 notes)', async () => {
        await app.waitFor(cards('=== 7'), 'the 7 demo notes');
      });

      await check('la tablette simulée n’est pas encore enregistrée : le panneau propose l’assistant', async () => {
        await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="notRegistered"]`)}) !== null`, 'the sidebar block');
        await app.click(STATUS);
        await app.waitFor(`document.querySelector('.tablet-panel') !== null`, 'the tablet panel');
        await app.waitFor(`document.querySelector('.tablet-panel button[data-action="register"]') !== null`, 'the register button');
        const simulated = await app.evaluate(`document.querySelector('.tablet-panel').textContent.includes(${j(l('tablet.simulated'))})`);
        if (!simulated) throw new Error('the panel does not say the tablet is simulated');
      });

      await check('l’assistant commence par l’avertissement', async () => {
        await app.click('.tablet-panel button[data-action="register"]');
        await app.waitFor(step('warning'), 'the warning step');
        const warned = await app.evaluate(`document.querySelector(${j(WIZARD)}).textContent.includes(${j(l('tablet.register.warning'))})`);
        if (!warned) throw new Error('the warning sentence is missing');
      });

      await check('continuer : la tablette simulée tient puis appuie le bouton, l’enregistrement aboutit', async () => {
        await app.click(`${WIZARD} button[data-action="continue"]`);
        await app.waitFor(step('done'), 'the end of the registration', 60_000);
        await app.click(`${WIZARD} button[data-action="finish"]`);
        await app.waitFor(`document.querySelector(${j(WIZARD)}) === null`, 'the wizard to close');
      });

      await check('la tablette est connectée', async () => {
        await app.waitFor(
          `['connected', 'syncing'].includes(document.querySelector(${j(STATUS)}).dataset.connection)`,
          'the connection',
          30_000,
        );
      });

      await check('la synchronisation importe 3 pages : 10 notes dans la grille', async () => {
        // Automatic on connecting; "Sync now" takes over if it did not start by itself.
        try {
          await app.waitFor(cards('=== 10'), 'the 3 synced pages in the grid', 20_000);
        } catch {
          await app.click('.tablet-panel button[data-action="sync"]');
          await app.waitFor(cards('=== 10'), 'the 3 synced pages in the grid after "Sync now"', 40_000);
        }
      });

      await check('la tablette n’a plus de page : le résultat le dit', async () => {
        await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="connected"]`)}) !== null`, 'the end of the sync', 30_000);
        const saved = l('tablet.sync.savedMany').replace('{count}', '3');
        const deleted = l('tablet.sync.deletedMany').replace('{count}', '3');
        await app.waitFor(
          `(() => { const t = document.querySelector('.tablet-panel [data-slot="last-sync"]')?.textContent ?? ''; return t.includes(${j(saved)}) && t.includes(${j(deleted)}); })()`,
          'the result of the sync',
        );
        const stored = await app.evaluate(`document.querySelector('.tablet-panel [data-slot="stored-pages"]')?.textContent`);
        if (stored !== undefined && stored !== '0') throw new Error(`the tablet still holds ${stored} pages`);
      });

      await check('mode direct : la vue s’ouvre et reçoit le tracé de la tablette', async () => {
        await app.click('.tablet-panel button[data-action="live"]');
        await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
        await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'strokes on the live canvas', 10_000);
      });

      await check('la latence médiane est inférieure à 30 ms', async () => {
        await app.waitFor(
          `window.__ardoiseLatency !== undefined && window.__ardoiseLatency.samples().length >= 20`,
          'latency samples',
          15_000,
        );
        const median = await app.evaluate('window.__ardoiseLatency.median()');
        console.log(`    latence médiane : ${Number(median).toFixed(1)} ms`);
        if (!(median < 30)) throw new Error(`median latency ${median} ms, expected less than 30 ms`);
      });

      await check('→ passe à la page suivante : 2/2', async () => {
        await app.key('ArrowRight');
        await app.waitFor(`${canvasData('page')} === '2/2'`, 'page 2/2');
      });

      await check('arrêter le mode direct enregistre la session : au moins 11 notes dans la grille', async () => {
        await app.click('button[data-action="stop-live"]');
        await app.waitFor(`document.querySelector('input.search') !== null`, 'the library');
        await app.waitFor(cards('>= 11'), 'the live note in the grid', 15_000);
      });
    },
  },
];
