// Milestone M7, part "edit": straighten shapes, then back to the original. Same pattern as
// journeys.mjs (labels from the translation files); run by run.mjs after the main journeys.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { openTab } from './side-panel.mjs';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const STROKES = '.editor g.ink path[data-stroke]';
const count = `document.querySelectorAll(${JSON.stringify(STROKES)}).length`;
const paths = `JSON.stringify([...document.querySelectorAll(${JSON.stringify(STROKES)})].map((p) => p.getAttribute('d')))`;

export const editJourneys = [
  {
    name: 'redresser un cercle tracé à la main, puis revenir à l’original',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let initial;
      let initialPaths;

      await check('ouvrir une note de démonstration', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('.note-card .card-open');
        await app.waitFor(`${count} > 0`, 'the strokes of the note');
        initial = await app.evaluate(count);
        initialPaths = await app.evaluate(paths);
      });

      await check('tracer un cercle à main levée', async () => {
        const box = await app.evaluate(
          `(() => { const r = document.querySelector('.editor svg').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`,
        );
        const r = Math.min(box.w, box.h) * 0.15;
        const points = Array.from({ length: 70 }, (_, i) => {
          const a = (i / 64) * 2 * Math.PI;
          const k = 1 + 0.03 * Math.sin(3 * a);
          return { x: box.x + box.w * 0.6 + r * k * Math.cos(a), y: box.y + box.h * 0.6 + r * k * Math.sin(a) };
        });
        await app.drag(points);
        await app.waitFor(`${count} === ${initial + 1}`, 'the drawn circle');
      });

      await check(`« ${l('editor.straighten.button')} » est grisé sans sélection, puis redresse le cercle`, async () => {
        await app.click(`.toolbar button[title="${l('editor.modes.select')}"]`);
        await app.waitFor(`document.querySelector('[data-action="straighten"]')?.disabled === true`, 'the disabled action');
        await app.key('a', { ctrl: true });
        await app.waitFor(`document.querySelector('[data-action="straighten"]')?.disabled === false`, 'the enabled action');
        const before = await app.evaluate(paths);
        await app.click('[data-action="straighten"]');
        await app.waitFor(`document.querySelector('.editor-notice') !== null`, 'the notice');
        const after = await app.evaluate(paths);
        if (after === before) throw new Error('nothing was straightened');
        if ((await app.evaluate(count)) !== initial + 1) throw new Error('the number of strokes changed');
      });

      await check('annuler puis rétablir : une seule étape', async () => {
        const straight = await app.evaluate(paths);
        await app.key('z', { ctrl: true });
        await app.waitFor(`${paths} !== ${JSON.stringify(straight)}`, 'the hand-drawn circle back');
        await app.key('y', { ctrl: true });
        await app.waitFor(`${paths} === ${JSON.stringify(straight)}`, 'the straight circle again');
      });

      await check('sauvegarder : l’original devient disponible', async () => {
        await app.key('s', { ctrl: true });
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'no'`, 'the saved state');
        await app.waitFor(`document.querySelector('[data-action="original"]')?.disabled === false`, 'the original');
      });

      await check(`« ${l('editor.original.button')} » après confirmation, puis sauvegarde`, async () => {
        // « Revenir à l'original » lives in the Page tab of the side panel (M16).
        await openTab(app, 'page');
        await app.click('[data-action="original"]');
        await app.waitFor(`document.querySelector('[data-action="original-confirm"]') !== null`, 'the confirmation');
        await app.click('[data-action="original-confirm"]');
        await app.waitFor(`${paths} === ${JSON.stringify(initialPaths)}`, 'the original ink');
        await app.key('s', { ctrl: true });
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'no'`, 'the saved state');
      });
    },
  },
];
