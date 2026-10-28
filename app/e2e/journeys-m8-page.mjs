// Milestone M8, part "page": portrait / landscape without turning the ink, paper size, and copying
// strokes between notes. Same pattern as journeys-m7-edit.mjs (labels from the translation files).
// Not wired into run.mjs: the integrator imports `pageJourneys` and runs them after merging.
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
const count = `document.querySelectorAll(${JSON.stringify(STROKES)}).length`;
const paths = `JSON.stringify([...document.querySelectorAll(${JSON.stringify(STROKES)})].map((p) => p.getAttribute('d')))`;
const page = `(() => { const s = document.querySelector('.editor svg.canvas'); return s.dataset.pageWidth + 'x' + s.dataset.pageHeight; })()`;

export const pageJourneys = [
  {
    name: 'portrait puis paysage sans tourner l’écriture, format A4, annuler',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let initialPaths;

      await check('ouvrir une note de démonstration (page paysage)', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('.note-card .card-open');
        await app.waitFor(`${count} > 0`, 'the strokes of the note');
        initialPaths = await app.evaluate(paths);
        if ((await app.evaluate(page)) !== '29700x21600') throw new Error('the demo page is not the tablet landscape page');
      });

      await check(`« ${l('editor.page.portrait')} » échange la largeur et la hauteur`, async () => {
        await app.click('[data-action="orientation-portrait"]');
        await app.waitFor(`${page} === '21600x29700'`, 'the portrait page');
      });

      await check('l’écriture n’a pas tourné (mêmes tracés)', async () => {
        const now = await app.evaluate(paths);
        if (now !== initialPaths) throw new Error('the strokes moved or turned');
      });

      await check('format A4 dans l’orientation courante', async () => {
        await app.evaluate(
          `(() => { const s = document.querySelector('[data-field="paper-size"]'); s.value = 'a4'; s.dispatchEvent(new Event('change', { bubbles: true })); })()`,
        );
        await app.waitFor(`${page} === '21000x29700'`, 'the A4 portrait page');
      });

      await check('annuler deux fois : retour à la page paysage d’origine', async () => {
        await app.key('z', { ctrl: true });
        await app.key('z', { ctrl: true });
        await app.waitFor(`${page} === '29700x21600'`, 'the original page');
      });
    },
  },
  {
    name: 'copier, coller et couper des traits (presse-papiers du système)',
    async run(app, check) {
      let initial;

      await check('ouvrir une note et tout sélectionner', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('.note-card .card-open');
        await app.waitFor(`${count} > 0`, 'the strokes of the note');
        initial = await app.evaluate(count);
        await app.key('a', { ctrl: true });
      });

      await check('Ctrl+C puis Ctrl+V : les copies sont ajoutées et sélectionnées', async () => {
        await app.key('c', { ctrl: true });
        await app.key('v', { ctrl: true });
        await app.waitFor(`${count} === ${initial * 2}`, 'the pasted strokes');
      });

      await check('Ctrl+Z annule le collage en une étape', async () => {
        await app.key('z', { ctrl: true });
        await app.waitFor(`${count} === ${initial}`, 'the strokes before the paste');
      });

      await check('Ctrl+X coupe la sélection', async () => {
        await app.key('a', { ctrl: true });
        await app.key('x', { ctrl: true });
        await app.waitFor(`${count} === 0`, 'the cut');
        await app.key('v', { ctrl: true });
        await app.waitFor(`${count} === ${initial}`, 'the pasted strokes');
      });
    },
  },
];
