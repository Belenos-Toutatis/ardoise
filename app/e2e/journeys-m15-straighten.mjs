// M15 journey, the setting of "Redresser les formes" (SPEC §4.4, docs/questions.md Q12): the split button, the panel
// with its count and its dashed preview on the page, « Redresser » (one undoable step), Escape and a click outside, the
// values kept, and a panel that stays within a low and a narrow window and scrolls.
//
// Written without a run (the window cannot be driven from the session that wrote it): the selectors are the panel's
// (`[data-panel="straighten"]`, `data-action`, `data-slot`, `data-field`, `#straighten-panel-<field>`), the canvas's
// (`g.straighten-preview`, `path.straighten-faded`) and the ones of the other editor journeys for the rest. It can only
// pass once the WebAssembly follows the tuning (package A of M15): the count must react to the minimum size.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const STROKES = '.editor g.ink path[data-stroke]';
const count = `document.querySelectorAll(${JSON.stringify(STROKES)}).length`;
const paths = `JSON.stringify([...document.querySelectorAll(${JSON.stringify(STROKES)})].map((p) => p.getAttribute('d')))`;
/** The page's sheet on screen, in window pixels, and how many millimetres it is wide. */
const sheetBox = `(() => {
  const svg = document.querySelector('.editor svg.canvas');
  const r = svg.querySelector('rect.sheet').getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height, mm: Number(svg.dataset.pageWidth) / 100 };
})()`;

const PANEL = '[data-panel="straighten"]';
const MORE = '[data-action="straighten-settings"]';
const field = (name) => `#straighten-panel-${name}`;
/** The shapes the panel says the selection gives: the first number of its line (0 when it says none, or nothing is selected). */
const found = `(() => { const m = (document.querySelector('[data-slot="straighten-count"]')?.textContent ?? '').match(/\\d+/); return m ? Number(m[0]) : 0; })()`;
const previewCount = `document.querySelectorAll('g.straighten-preview path').length`;
const fadedCount = `document.querySelectorAll('g.ink path.straighten-faded').length`;
const value = (name) => `document.querySelector(${JSON.stringify(field(name))})?.value`;
const warning = `document.querySelector('[data-slot="size-warning"]') !== null`;
/** How many strokes the selection panel says are selected (0 when there is no panel). */
const selectedCount = (l) => {
  const escape = (part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const [before, after] = l('editor.selection.count').split('{count}');
  const source = `${escape(before)}(\\d+)${escape(after)}`;
  return `(() => { const m = (document.querySelector('.editor-side')?.textContent ?? '').match(new RegExp(${JSON.stringify(source)})); return m ? Number(m[1]) : 0; })()`;
};

/** A straight line of mouse positions. */
const line = (x0, y0, x1, y1, n = 30) =>
  Array.from({ length: n + 1 }, (_, i) => ({ x: x0 + ((x1 - x0) * i) / n, y: y0 + ((y1 - y0) * i) / n }));

export const straightenSettingJourneys = [
  {
    name: 'redresser : le réglage a son panneau, son aperçu en pointillés et ses valeurs retenues',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const tool = (mode) => `.toolbar button[title="${l(`editor.modes.${mode}`)}"]`;
      let box;
      let pxPerMm;
      /** Window pixels of a point of the sheet, as fractions of its width and height. */
      const at = (fx, fy) => ({ x: box.x + box.w * fx, y: box.y + box.h * fy });
      const openPanel = async () => {
        await app.click(MORE);
        await app.waitFor(`document.querySelector(${JSON.stringify(PANEL)}) !== null`, 'the panel');
      };
      /** Moves a slider with the keyboard (a real key press per step): the focus goes there first. */
      const nudge = async (name, key, times) => {
        await app.evaluate(`document.querySelector(${JSON.stringify(field(name))}).focus()`);
        for (let i = 0; i < times; i++) await app.key(key);
      };
      let hand;

      await check('une nouvelle note, une page qu’on mesure en millimètres', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('button.primary', l('library.newNote'));
        await app.waitFor(`document.querySelector('.editor svg.canvas rect.sheet') !== null`, 'the editor');
        // The page is fitted once the editor has measured its stage.
        await sleep(400);
        box = await app.evaluate(sheetBox);
        pxPerMm = box.w / box.mm;
        if (!(pxPerMm > 1)) throw new Error(`the page is ${pxPerMm} px per mm: too small to draw a 7 mm line`);
        if ((await app.evaluate(count)) !== 0) throw new Error('the new note is not empty');
      });

      await check('à la souris : une droite de 30 mm, une droite de 7 mm et un cercle de 25 mm', async () => {
        await app.click(tool('pen'));
        const a = at(0.15, 0.3);
        await app.drag(line(a.x, a.y, a.x + 30 * pxPerMm, a.y));
        await app.waitFor(`${count} === 1`, 'the 30 mm line');
        const b = at(0.15, 0.5);
        await app.drag(line(b.x, b.y, b.x + 7 * pxPerMm, b.y, 24));
        await app.waitFor(`${count} === 2`, 'the 7 mm line');
        const c = at(0.65, 0.5);
        const r = 12.5 * pxPerMm;
        const circle = Array.from({ length: 70 }, (_, i) => {
          const angle = (i / 64) * 2 * Math.PI;
          const k = 1 + 0.03 * Math.sin(3 * angle);
          return { x: c.x + r * k * Math.cos(angle), y: c.y + r * k * Math.sin(angle) };
        });
        await app.drag(circle);
        await app.waitFor(`${count} === 3`, 'the circle');
        hand = await app.evaluate(paths);
      });

      await check('un lasso autour de tout : trois traits sélectionnés', async () => {
        await app.click(tool('select'));
        await app.drag([at(0.08, 0.08), at(0.92, 0.08), at(0.92, 0.92), at(0.08, 0.92), at(0.08, 0.1)]);
        await app.waitFor(`${selectedCount(l)} === 3`, 'the selection of the three strokes');
      });

      await check(`« ${l('editor.straighten.button')} » a un petit bouton de réglage, le panneau est fermé`, async () => {
        const state = await app.evaluate(`(() => {
          const more = document.querySelector(${JSON.stringify(MORE)});
          return more && { label: more.getAttribute('aria-label'), expanded: more.getAttribute('aria-expanded'), controls: more.getAttribute('aria-controls'), open: document.querySelector(${JSON.stringify(PANEL)}) !== null };
        })()`);
        if (!state) throw new Error('no setting button');
        if (state.label !== l('editor.straighten.settings.button')) throw new Error(`the button is named "${state.label}"`);
        if (state.expanded !== 'false' || state.open) throw new Error('the panel should be closed');
        if (!state.controls) throw new Error('no aria-controls');
      });

      await check('ouvrir le panneau : valeurs par défaut, deux formes reconnues (la droite de 7 mm non), aperçu en pointillés', async () => {
        await openPanel();
        if ((await app.evaluate(`document.querySelector(${JSON.stringify(MORE)}).getAttribute('aria-expanded')`)) !== 'true') {
          throw new Error('aria-expanded is not true');
        }
        if ((await app.evaluate(value('tolerance'))) !== '3' || (await app.evaluate(value('size'))) !== '10') throw new Error('the values are not the defaults');
        await app.waitFor(`${found} === 2`, 'two shapes recognised');
        await app.waitFor(`${previewCount} === 2`, 'two dashed shapes on the page');
        if ((await app.evaluate(fadedCount)) !== 2) throw new Error('the two originals are not faded');
        const dashed = await app.evaluate(`getComputedStyle(document.querySelector('g.straighten-preview path')).strokeDasharray`);
        if (!dashed || dashed === 'none') throw new Error('the preview is not dashed');
        // Nothing changed in the note.
        if ((await app.evaluate(paths)) !== hand) throw new Error('the strokes changed while previewing');
      });

      await check('la taille minimale à 5 mm : la droite de 7 mm compte, l’avertissement apparaît ; à 10 mm elle ne compte plus', async () => {
        await nudge('size', 'ArrowLeft', 5);
        await app.waitFor(`${value('size')} === '5'`, 'the minimum at 5 mm');
        await app.waitFor(`${found} === 3`, 'three shapes with 5 mm');
        await app.waitFor(`${previewCount} === 3`, 'three dashed shapes');
        if ((await app.evaluate(warning)) !== true) throw new Error('no warning about letters below 1 cm');
        await nudge('size', 'ArrowRight', 5);
        await app.waitFor(`${found} === 2`, 'two shapes with 10 mm');
        await app.waitFor(`${previewCount} === 2`, 'two dashed shapes again');
        if ((await app.evaluate(warning)) !== false) throw new Error('the warning stays at 10 mm');
      });

      await check('décocher « jusqu’à N° » grise son curseur', async () => {
        await app.click('input[data-field="snap-angle"]');
        await app.waitFor(`document.querySelector(${JSON.stringify(field('angle'))}).disabled === true`, 'the disabled angle slider');
        if ((await app.evaluate(`document.querySelector(${JSON.stringify(field('round'))}).disabled`)) !== false) throw new Error('the other slider is disabled too');
        await app.click('input[data-field="snap-angle"]');
        await app.waitFor(`document.querySelector(${JSON.stringify(field('angle'))}).disabled === false`, 'the enabled angle slider');
      });

      await check('Échap ferme le panneau sans redresser, le focus revient au petit bouton, les valeurs restent', async () => {
        await nudge('tolerance', 'ArrowRight', 1);
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${JSON.stringify(PANEL)}) === null`, 'the closed panel');
        await app.waitFor(`document.activeElement?.dataset?.action === 'straighten-settings'`, 'the focus on the setting button');
        if ((await app.evaluate(paths)) !== hand) throw new Error('Escape straightened');
        if ((await app.evaluate(selectedCount(l))) !== 3) throw new Error('Escape deselected');
        await openPanel();
        if ((await app.evaluate(value('tolerance'))) !== '4') throw new Error('the tolerance was not kept');
        await app.waitFor(`${previewCount} >= 2`, 'the preview back');
      });

      await check('un clic ailleurs ferme aussi le panneau', async () => {
        await app.click('.editor-side h3');
        await app.waitFor(`document.querySelector(${JSON.stringify(PANEL)}) === null`, 'the closed panel');
        if ((await app.evaluate(`document.querySelectorAll('g.straighten-preview').length`)) !== 0) throw new Error('the preview stays');
        if ((await app.evaluate(paths)) !== hand) throw new Error('a click outside straightened');
      });

      await check('« Valeurs par défaut » remet les valeurs d’aujourd’hui', async () => {
        await openPanel();
        await app.click('[data-action="straighten-defaults"]');
        await app.waitFor(`${value('tolerance')} === '3' && ${value('size')} === '10' && ${value('angle')} === '4' && ${value('round')} === '10'`, 'the default values');
        await app.waitFor(`${found} === 2`, 'two shapes with the defaults');
      });

      await check('dans une fenêtre basse : le panneau ne dépasse pas le bas, défile à la molette, ses boutons restent visibles', async () => {
        await app.send('Emulation.setDeviceMetricsOverride', { width: 1100, height: 440, deviceScaleFactor: 1, mobile: false });
        await sleep(500);
        const measure = `(() => {
          const p = document.querySelector(${JSON.stringify(PANEL)});
          const r = p.getBoundingClientRect();
          const b = document.querySelector('[data-action="straighten-apply"]').getBoundingClientRect();
          return { bottom: r.bottom, top: r.top, scrolls: p.scrollHeight > p.clientHeight + 1, scrollTop: p.scrollTop, buttonBottom: b.bottom, height: window.innerHeight, view: document.querySelector('.editor svg.canvas').dataset.view, overscroll: getComputedStyle(p).overscrollBehaviorY, overflow: getComputedStyle(p).overflowY };
        })()`;
        const before = await app.evaluate(measure);
        if (before.bottom > before.height) throw new Error(`the panel goes past the bottom: ${before.bottom} > ${before.height}`);
        if (!before.scrolls) throw new Error('the panel does not scroll in a window this low');
        if (before.overflow !== 'auto' || before.overscroll !== 'contain') throw new Error(`overflow ${before.overflow}, overscroll ${before.overscroll}`);
        if (before.buttonBottom > before.height || before.buttonBottom < before.top) throw new Error('the buttons are not in sight');
        // A wheel turn over the panel scrolls it, and does not zoom the page behind.
        const x = (await app.evaluate(`document.querySelector(${JSON.stringify(PANEL)}).getBoundingClientRect().left + 120`));
        await app.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x, y: before.top + 80, deltaX: 0, deltaY: 400 });
        await sleep(300);
        const after = await app.evaluate(measure);
        if (!(after.scrollTop > 0)) throw new Error('the wheel did not scroll the panel');
        if (after.view !== before.view) throw new Error('the page zoomed instead');
        if (after.buttonBottom > after.height || after.bottom > after.height) throw new Error('the buttons or the panel left the window after scrolling');
      });

      await check('dans une fenêtre étroite : le panneau reste entre les bords', async () => {
        await app.send('Emulation.setDeviceMetricsOverride', { width: 420, height: 440, deviceScaleFactor: 1, mobile: false });
        await sleep(500);
        const edges = await app.evaluate(`(() => { const r = document.querySelector(${JSON.stringify(PANEL)}).getBoundingClientRect(); return { left: r.left, right: r.right, width: window.innerWidth }; })()`);
        if (edges.left < 0 || edges.right > edges.width) throw new Error(`the panel is out of the window: ${edges.left}..${edges.right} of ${edges.width}`);
      });

      await check('en thème sombre : le panneau prend le fond sombre de l’application', async () => {
        await app.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: 'dark' }] });
        await sleep(300);
        const colours = await app.evaluate(
          `(() => ({ panel: getComputedStyle(document.querySelector(${JSON.stringify(PANEL)})).backgroundColor, text: getComputedStyle(document.querySelector(${JSON.stringify(PANEL)})).color }))()`,
        );
        if (colours.panel !== 'rgb(29, 35, 43)') throw new Error(`the panel is ${colours.panel} in dark theme`);
        await app.send('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: 'light' }] });
        await app.send('Emulation.clearDeviceMetricsOverride');
        await sleep(500);
      });

      await check('à 5 mm, « Redresser » applique ce que montrait l’aperçu : trois formes, une seule étape', async () => {
        await nudge('size', 'ArrowLeft', 5);
        await app.waitFor(`${found} === 3`, 'three shapes with 5 mm');
        await app.click('[data-action="straighten-apply"]');
        await app.waitFor(`document.querySelector('.editor-notice') !== null`, 'the notice');
        const notice = await app.evaluate(`document.querySelector('.editor-notice').textContent.trim()`);
        const expected = l('editor.straighten.doneMany').replace('{count}', '3');
        if (notice !== expected) throw new Error(`the notice says "${notice}", not "${expected}"`);
        await app.waitFor(`document.querySelector(${JSON.stringify(PANEL)}) === null`, 'the closed panel');
        if ((await app.evaluate(`document.querySelectorAll('g.straighten-preview').length`)) !== 0) throw new Error('the preview stays');
        if ((await app.evaluate(count)) !== 3) throw new Error('the number of strokes changed');
        if ((await app.evaluate(paths)) === hand) throw new Error('nothing was straightened');
      });

      await check('Ctrl+Z défait les trois formes d’un coup', async () => {
        await app.key('z', { ctrl: true });
        await app.waitFor(`${paths} === ${JSON.stringify(hand)}`, 'the hand-drawn strokes back');
      });

      await check('la touche F redresse avec les mêmes valeurs (5 mm : trois formes)', async () => {
        await app.key('f');
        await app.waitFor(`document.querySelector('.editor-notice')?.textContent.trim() === ${JSON.stringify(l('editor.straighten.doneMany').replace('{count}', '3'))}`, 'three shapes straightened by F');
        await app.key('z', { ctrl: true });
        await app.waitFor(`${paths} === ${JSON.stringify(hand)}`, 'the hand-drawn strokes back');
      });

      await check('les valeurs sont écrites dans les réglages d’Ardoise (section [shapes])', async () => {
        const toml = join(app.dir, 'ardoise.toml');
        const deadline = Date.now() + 5000;
        for (;;) {
          if (readFileSync(toml, 'utf8').includes('[shapes]')) break;
          if (Date.now() > deadline) throw new Error('no [shapes] section in ardoise.toml');
          await sleep(100);
        }
      });

      await check('sauvegarder, revenir à la bibliothèque, rouvrir une note : le réglage est retrouvé', async () => {
        await app.key('s', { ctrl: true });
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'no'`, 'the saved state');
        await app.click('.editor-top button', l('editor.back'));
        await app.waitFor(`document.querySelector('.editor') === null`, 'the library');
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the library cards');
        await app.click('.note-card .card-open');
        await app.waitFor(`document.querySelector('.editor svg.canvas rect.sheet') !== null`, 'the editor');
        await app.click(tool('select'));
        await openPanel();
        if ((await app.evaluate(value('size'))) !== '5') throw new Error(`the minimum is ${await app.evaluate(value('size'))} mm, not 5`);
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${JSON.stringify(PANEL)}) === null`, 'the closed panel');
      });
    },
  },
];
