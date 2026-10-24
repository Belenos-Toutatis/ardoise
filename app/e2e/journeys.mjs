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

      await check('la rouvrir : le trait ajouté a bien été enregistré sur le disque', async () => {
        await app.click('.note-card .card-open');
        await app.waitFor(`document.querySelectorAll(${JSON.stringify(STROKES)}).length === ${strokeCount + 1}`, 'the saved stroke after reopening');
      });
    },
  },
];
