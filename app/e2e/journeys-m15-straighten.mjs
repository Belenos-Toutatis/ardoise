// M15 journey, the setting of "Redresser les formes" (SPEC §4.4, docs/questions.md Q12): the split button, the panel
// with its count and its dashed preview on the page, « Redresser » (one undoable step), Escape and a click outside, the
// values kept, and a panel that stays within a low and a narrow window and scrolls.
//
// It also holds the journey of the shape tools (Q20: line, rectangle, ellipse, their three toggles, the magnet on squared
// paper) and of Escape by steps up to the library, which draw with the same mouse helpers.
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

/** The number under `key` in the `[shapes]` table of an `ardoise.toml` text (`5` and `5.0` are the same), or null. */
function shapesValue(toml, key) {
  const lines = toml.split(/\r?\n/);
  const start = lines.findIndex((row) => row.trim() === '[shapes]');
  if (start < 0) return null;
  for (const row of lines.slice(start + 1)) {
    if (row.trim().startsWith('[')) break;
    const found = row.match(new RegExp(String.raw`^\s*${key}\s*=\s*(-?\d+(?:\.\d+)?)\s*$`));
    if (found) return Number(found[1]);
  }
  return null;
}

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
        // The note's place among the pages, in the top bar: outside the shapes panel, which now reaches over the side panel's
        // title (the shape tools moved its button to the right).
        await app.click('.editor [data-slot="page-position"]');
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

      await check('les valeurs sont écrites dans les réglages d’Ardoise (section [shapes], taille minimale 5 mm)', async () => {
        const toml = join(app.dir, 'ardoise.toml');
        const deadline = Date.now() + 5000;
        for (;;) {
          // The header alone is no proof (it is written with any value): the value the sliders left is.
          if (shapesValue(readFileSync(toml, 'utf8'), 'min_size_mm') === 5) break;
          if (Date.now() > deadline) throw new Error(`no min_size_mm = 5 in [shapes] of ardoise.toml:\n${readFileSync(toml, 'utf8')}`);
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
  {
    name: 'outils de tracé : ligne, rectangle, ellipse, les trois boutons, l’aimant sur le quadrillé 5 mm ; Échap par étapes jusqu’à la bibliothèque',
    // Q20 (SPEC §4.4, « Outils de tracé » and « Échap dans l'éditeur »). Written without a run (the window cannot be driven
    // from the session that wrote it): the selectors are the toolbar's (`[data-slot="shape-options"]`, `data-action="shape-
    // regular|centre|snap"`), the Page tab's template select and the ones of the journeys above. A shape is one stroke: its
    // outline's box (`getBBox`, in page units) is the shape's box grown by half the line width (0.6 mm at full pressure).
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const tool = (mode) => `.toolbar button[title="${l(`editor.modes.${mode}`)}"]`;
      const toggle = (name) => `[data-slot="shape-options"] [data-action="shape-${name}"]`;
      const SIDE = '[data-side-panel]';
      /** Half the line width of the ballpoint at full pressure, device units (a shape is drawn at full pressure). */
      const HALF = 30;
      const pressed = (name) => `document.querySelector(${JSON.stringify(toggle(name))})?.getAttribute('aria-pressed')`;
      /** The boxes of the outlines of all the strokes, in page units. */
      const boxes = `JSON.stringify([...document.querySelectorAll(${JSON.stringify(STROKES)})].map((p) => { const b = p.getBBox(); return [b.x, b.y, b.width, b.height]; }))`;
      let box;
      let pxPerMm;
      let pitchMm;
      const at = (fx, fy) => ({ x: box.x + box.w * fx, y: box.y + box.h * fy });
      /** The first line of squared paper along a length of `mm` millimetres, device units: the grid is centred (`ardoise_render::snap_grid`). */
      const firstLine = (mm) => ((mm - Math.floor(mm / 5) * 5) / 2) * 100;
      /** Is `value` (device units) on a line of the grid that starts at `first`, every 500? */
      const onGrid = (value, first) => Math.abs(((value - first) / 500) - Math.round((value - first) / 500)) * 500 < 3;
      const lastBox = async () => JSON.parse(await app.evaluate(boxes)).at(-1);

      await check('une nouvelle note, un fond quadrillé 5 mm : l’aimant est grisé avec un fond uni, puis disponible', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('button.primary', l('library.newNote'));
        await app.waitFor(`document.querySelector('.editor svg.canvas rect.sheet') !== null`, 'the editor');
        await sleep(400);
        box = await app.evaluate(sheetBox);
        pxPerMm = box.w / box.mm;
        pitchMm = 5;
        await app.click(tool('line'));
        await app.waitFor(`document.querySelector('[data-slot="shape-options"]') !== null`, 'the three toggles');
        const names = await app.evaluate(`[...document.querySelectorAll('[data-slot="shape-options"] button')].map((b) => b.textContent.trim())`);
        const expected = ['regular', 'centre', 'snap'].map((n) => l(`editor.shapeTools.${n}`));
        if (JSON.stringify(names) !== JSON.stringify(expected)) throw new Error(`the toggles read ${JSON.stringify(names)}, not ${JSON.stringify(expected)}`);
        if ((await app.evaluate(`document.querySelector(${JSON.stringify(toggle('snap'))}).disabled`)) !== true) throw new Error('the magnet is enabled on a plain page');
        // The Page tab: the paper's template.
        await app.click(`${SIDE} .side-rail [role="tab"][data-tab="page"]`);
        await app.waitFor(`document.querySelector('${SIDE} [data-tab-panel="page"]')?.hidden === false`, 'the Page tab');
        await app.evaluate(`(() => { const el = document.querySelector('select[data-field="template"]'); el.value = 'grid5mm'; el.dispatchEvent(new Event('change', { bubbles: true })); })()`);
        await app.waitFor(`document.querySelector(${JSON.stringify(toggle('snap'))}).disabled === false`, 'the magnet enabled on squared paper');
        await sleep(300);
        box = await app.evaluate(sheetBox);
      });

      await check('la ligne, sans rien demander : exactement du premier point au dernier, un seul trait', async () => {
        const a = at(0.2, 0.2);
        await app.drag(line(a.x, a.y, a.x + 40 * pxPerMm, a.y + 7 * pxPerMm, 20));
        await app.waitFor(`${count} === 1`, 'the line');
        const [x, y, w, h] = await lastBox();
        // 40 mm across, 7 mm down (a straight line is as wide as it is long, plus its own width).
        if (Math.abs(w - (4000 + 2 * HALF)) > 150 || Math.abs(h - (700 + 2 * HALF)) > 150) throw new Error(`the line's box is ${w} x ${h}, not about 4060 x 760 (nothing straightens unless asked)`);
        if (!(x > 0 && y > 0)) throw new Error(`the line is at ${x}, ${y}`);
      });

      await check('« Forme régulière » + « Aimanter » : un carré dont les coins sont sur le quadrillé', async () => {
        await app.click(tool('rectangle'));
        await app.click(toggle('regular'));
        await app.click(toggle('snap'));
        if ((await app.evaluate(pressed('regular'))) !== 'true' || (await app.evaluate(pressed('snap'))) !== 'true') throw new Error('the toggles are not pressed');
        const a = at(0.3, 0.4);
        // Wider than tall: the square takes the longer side.
        await app.drag(line(a.x, a.y, a.x + 50 * pxPerMm, a.y + 20 * pxPerMm, 20));
        await app.waitFor(`${count} === 2`, 'the square');
        const [x, y, w, h] = await lastBox();
        if (Math.abs(w - h) > 3) throw new Error(`not a square: ${w} x ${h}`);
        const first = await app.evaluate(`(() => { const s = document.querySelector('.editor svg.canvas'); return { x: (${firstLine.toString()})(Number(s.dataset.pageWidth) / 100), y: (${firstLine.toString()})(Number(s.dataset.pageHeight) / 100) }; })()`);
        for (const [value, from, what] of [[x + HALF, first.x, 'left'], [x + w - HALF, first.x, 'right'], [y + HALF, first.y, 'top'], [y + h - HALF, first.y, 'bottom']]) {
          if (!onGrid(value, from)) throw new Error(`the ${what} side is at ${value}, off the ${pitchMm} mm grid that starts at ${from}`);
        }
      });

      await check('« Depuis le centre » : un cercle autour du premier point, qui est sur le quadrillé', async () => {
        await app.click(tool('ellipse'));
        // « Forme régulière » and the magnet stay on from one tool to the next.
        if ((await app.evaluate(pressed('regular'))) !== 'true' || (await app.evaluate(pressed('snap'))) !== 'true') throw new Error('the toggles were not kept');
        await app.click(toggle('centre'));
        const c = at(0.7, 0.6);
        await app.drag(line(c.x, c.y, c.x + 25 * pxPerMm, c.y + 5 * pxPerMm, 20));
        await app.waitFor(`${count} === 3`, 'the circle');
        const [x, y, w, h] = await lastBox();
        if (Math.abs(w - h) > 3) throw new Error(`not a circle: ${w} x ${h}`);
        const first = await app.evaluate(`(() => { const s = document.querySelector('.editor svg.canvas'); return { x: (${firstLine.toString()})(Number(s.dataset.pageWidth) / 100), y: (${firstLine.toString()})(Number(s.dataset.pageHeight) / 100) }; })()`);
        if (!onGrid(x + w / 2, first.x) || !onGrid(y + h / 2, first.y)) throw new Error(`the centre ${x + w / 2}, ${y + h / 2} is off the grid`);
      });

      await check('Échap pendant le tracé d’une forme : rien n’est ajouté', async () => {
        await app.click(tool('rectangle'));
        const a = at(0.15, 0.8);
        await app.mouse('mouseMoved', a.x, a.y);
        await app.mouse('mousePressed', a.x, a.y);
        await app.mouse('mouseMoved', a.x + 30 * pxPerMm, a.y + 10 * pxPerMm, { buttons: 1, button: 'left' });
        await app.waitFor(`document.querySelectorAll('.editor g.ink path.live').length === 1`, 'the shape being drawn');
        await app.key('Escape');
        await app.waitFor(`document.querySelectorAll('.editor g.ink path.live').length === 0`, 'the shape dropped');
        await app.mouse('mouseReleased', a.x + 30 * pxPerMm, a.y + 10 * pxPerMm);
        await sleep(200);
        if ((await app.evaluate(count)) !== 3) throw new Error('Escape did not drop the shape');
        if ((await app.evaluate(`document.querySelector('.editor') !== null`)) !== true) throw new Error('Escape left the editor');
      });

      await check('Échap par étapes : désélectionne, puis enregistre et revient à la bibliothèque', async () => {
        await app.click(tool('select'));
        // Ctrl+A rather than a lasso: the landscape sheet is taller than the window.
        await app.key('a', { ctrl: true });
        await app.waitFor(`${selectedCount(l)} === 3`, 'the three shapes selected');
        await app.key('Escape');
        await app.waitFor(`${selectedCount(l)} === 0`, 'the selection dropped');
        if ((await app.evaluate(`document.querySelector('.editor') !== null`)) !== true) throw new Error('the first Escape left the editor');
        if ((await app.evaluate(`document.querySelector('.editor').dataset.dirty`)) !== 'yes') throw new Error('the note is not unsaved before the last step');
        await app.key('Escape');
        await app.waitFor(`document.querySelector('.editor') === null`, 'the library');
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the library cards');
        // Saved on the way: opening the note again shows the three shapes.
        await app.click('.note-card .card-open');
        await app.waitFor(`document.querySelector('.editor svg.canvas rect.sheet') !== null`, 'the editor again');
        await app.waitFor(`${count} === 3`, 'the three shapes saved');
      });
    },
  },
  {
    name: 'redresser : les valeurs de ardoise.toml sont lues au démarrage',
    // What an earlier run of the app (or a hand edit) left in the settings file.
    settings: '[shapes]\nmin_size_mm = 7\ntolerance = 4\nsnap_angle = false\n',
    async run(app, check) {
      const l = (key) => text(app.language, key);

      await check('ouvrir une note, puis le panneau de réglage', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('.note-card .card-open');
        await app.waitFor(`document.querySelector('.editor svg.canvas rect.sheet') !== null`, 'the editor');
        await app.click(`.toolbar button[title="${l('editor.modes.select')}"]`);
        await app.click(MORE);
        await app.waitFor(`document.querySelector(${JSON.stringify(PANEL)}) !== null`, 'the panel');
      });

      await check('la taille minimale est 7 mm, la tolérance 4 : les valeurs du fichier, pas celles d’origine', async () => {
        if ((await app.evaluate(value('size'))) !== '7') throw new Error(`the minimum is ${await app.evaluate(value('size'))}, not 7`);
        const shown = await app.evaluate(`document.querySelector('[data-slot="size-value"]').textContent.trim()`);
        const expected = l('editor.straighten.settings.size.value').replace('{mm}', '7');
        if (shown !== expected) throw new Error(`the size reads "${shown}", not "${expected}"`);
        if ((await app.evaluate(value('tolerance'))) !== '4') throw new Error(`the tolerance is ${await app.evaluate(value('tolerance'))}, not 4`);
      });

      await check('la case de l’alignement horizontal/vertical est décochée et son curseur grisé, l’autre reste active', async () => {
        const state = await app.evaluate(`(() => ({
          angleChecked: document.querySelector('input[data-field="snap-angle"]').checked,
          angleDisabled: document.querySelector(${JSON.stringify(field('angle'))}).disabled,
          roundChecked: document.querySelector('input[data-field="snap-round"]').checked,
          roundDisabled: document.querySelector(${JSON.stringify(field('round'))}).disabled,
        }))()`);
        if (state.angleChecked || !state.angleDisabled) throw new Error(`the angle box is ${state.angleChecked ? 'ticked' : 'unticked'}, its slider ${state.angleDisabled ? 'disabled' : 'enabled'}`);
        if (!state.roundChecked || state.roundDisabled) throw new Error('the round box should stay ticked');
      });
    },
  },
];
