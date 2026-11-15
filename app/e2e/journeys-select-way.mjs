// The Select tool has two ways, in the editor and in live mode (docs/questions.md Q22): a click on its button from another
// tool gives the way used last (the rectangle the very first time), a click while it is the tool turns to the other way,
// and the V key does the same. The button, its title and the help line say which way is on; its visible label stays short
// ("Sélection") so that the toolbar's buttons keep their height. Real mouse and key events on the production executable.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { LIVE_CANVAS, STATUS, canvasData, registerTablet } from './journeys-m12-resume.mjs';
import { SELECT_BUTTON, chooseSelect, rectangleDrag, selectState } from './select-way.mjs';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const STROKES = '.editor g.ink path[data-stroke]';
const count = `document.querySelectorAll(${j(STROKES)}).length`;
/** The page's sheet on screen, in window pixels. */
const sheetBox = `(() => { const r = document.querySelector('.editor svg.canvas rect.sheet').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`;
/** How many strokes the selection panel says are selected (0 when there is no panel). */
const selectedCount = (l) => {
  const escape = (part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const [before, after] = l('editor.selection.count').split('{count}');
  const source = `${escape(before)}(\\d+)${escape(after)}`;
  return `(() => { const m = (document.querySelector('.editor-side')?.textContent ?? '').match(new RegExp(${j(source)})); return m ? Number(m[1]) : 0; })()`;
};
/** The selection's frame is on the page. */
const frameShown = `document.querySelector('.editor svg.canvas rect.selection') !== null`;
const BAR = '[data-slot="select-bar"]';
const barCount = `Number(document.querySelector(${j(`${BAR} .select-count`)})?.dataset.count ?? 0)`;

/** Opens live mode from the tablet panel (the registration left the library) and waits for the first strokes. */
async function openLive(app) {
  await app.click(STATUS);
  await app.waitFor(`document.querySelector('.tablet-panel button[data-action="live"]:not([disabled])') !== null`, 'the live button');
  await app.click('.tablet-panel button[data-action="live"]');
  await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
  await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'strokes on the live canvas', 15_000);
}

export const selectWayJourneys = [
  {
    name: 'éditeur : l’outil Sélection fait un rectangle ou un lasso, au clic comme à la touche V, et revient dans la dernière façon',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let box;
      /** Window pixels of a point of the sheet, as fractions of its width and height. */
      const at = (fx, fy) => ({ x: box.x + box.w * fx, y: box.y + box.h * fy });
      const stroke = (from, to) => Array.from({ length: 25 }, (_, i) => ({ x: from.x + ((to.x - from.x) * i) / 24, y: from.y + ((to.y - from.y) * i) / 24 }));
      /** What is on the page while a drag is under way: a dashed rectangle, a lasso outline, both, or none. */
      const gesture = `(() => {
        const marquee = document.querySelector('.editor svg.canvas rect.marquee');
        return {
          marquee: marquee !== null,
          dashed: marquee ? getComputedStyle(marquee).strokeDasharray !== 'none' : false,
          lasso: document.querySelector('.editor svg.canvas polyline.lasso') !== null,
        };
      })()`;
      /** A real drag along `points`, looking at the page before the button is let go. */
      const dragSeen = async (points) => {
        const [first, ...rest] = points;
        await app.mouse('mouseMoved', first.x, first.y);
        await app.mouse('mousePressed', first.x, first.y);
        for (const p of rest) {
          await app.mouse('mouseMoved', p.x, p.y, { buttons: 1, button: 'left' });
          await sleep(8);
        }
        await sleep(120);
        const seen = await app.evaluate(gesture);
        const last = rest.at(-1) ?? first;
        await app.mouse('mouseReleased', last.x, last.y);
        return seen;
      };
      const stateOf = () =>
        app.evaluate(`(() => {
          const b = document.querySelector(${j(SELECT_BUTTON)});
          const help = document.querySelector('.editor .select-help');
          return { title: b.title, way: b.dataset.way, pressed: b.getAttribute('aria-pressed'), name: b.getAttribute('aria-label'), label: b.querySelector('.tool-label')?.textContent.trim(), help: help ? help.textContent : null, icon: b.querySelector('svg rect') ? 'rectangle' : 'lasso' };
        })()`);
      /** Escape drops a selection (and, with none, would leave the editor: never pressed then). */
      const deselect = async () => {
        if ((await app.evaluate(selectedCount(l))) === 0) return;
        await app.key('Escape');
        await app.waitFor(`${selectedCount(l)} === 0`, 'the selection dropped');
      };
      const expectState = (s, way, pressed, what) => {
        const title = l(way === 'lasso' ? 'editor.modes.selectLasso' : 'editor.modes.selectRectangle');
        // The visible label stays « Sélection »; the way is in the icon, the title and the accessible name.
        if (s.way !== way || s.title !== title || s.name !== title || s.label !== l('editor.modes.select') || s.pressed !== String(pressed) || s.icon !== way) {
          throw new Error(`${what}: the button is ${j(s)}, expected the ${way} (${title}), pressed ${pressed}`);
        }
      };
      const heights = () => app.evaluate(`[...document.querySelectorAll('.toolbar .tool')].map((b) => Math.round(b.getBoundingClientRect().height))`);
      const way = () => app.evaluate(`document.querySelector(${j(SELECT_BUTTON)}).dataset.way`);
      const pressedNow = () => app.evaluate(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed')`);

      await check('une nouvelle note, deux traits à gauche et à droite : le bouton propose la dernière façon, sans la dire dans son libellé', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('button.primary', l('library.newNote'));
        await app.waitFor(`document.querySelector('.editor svg.canvas rect.sheet') !== null`, 'the editor');
        // The page is fitted once the editor has measured its stage.
        await sleep(400);
        box = await app.evaluate(sheetBox);
        await app.drag(stroke(at(0.15, 0.4), at(0.35, 0.4)));
        await app.waitFor(`${count} === 1`, 'the left stroke');
        await app.drag(stroke(at(0.6, 0.62), at(0.85, 0.62)));
        await app.waitFor(`${count} === 2`, 'the right stroke');
        const s = await stateOf();
        expectState(s, s.way, false, 'with the pen');
      });

      await check('le bouton garde sa hauteur, et celle de ses voisins, quelle que soit la façon', async () => {
        const seen = [];
        for (const w of ['rectangle', 'lasso']) {
          await chooseSelect(app, w);
          seen.push(await heights());
          await app.click('.toolbar .tool', l('editor.modes.pen'));
          seen.push(await heights());
        }
        if (new Set(seen.map((h) => h.join())).size !== 1) throw new Error(`the buttons change height: ${j(seen)}`);
        if (new Set(seen[0]).size !== 1) throw new Error(`the tool buttons are not all as tall: ${j(seen[0])}`);
      });

      await check('un clic sur le bouton : le rectangle, l’outil actif, l’icône et la ligne d’aide le disent', async () => {
        await chooseSelect(app, 'rectangle');
        await app.click('.toolbar .tool', l('editor.modes.pen'));
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'false'`, 'the pen chosen');
        expectState(await stateOf(), 'rectangle', false, 'with the pen, after the rectangle');
        await app.click(SELECT_BUTTON);
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the selection tool');
        const s = await stateOf();
        expectState(s, 'rectangle', true, 'after the click');
        if (s.help !== l('editor.selectHelpRectangle')) throw new Error(`the help line is ${j(s.help)}`);
      });

      await check('glisser d’un coin à l’autre : un cadre en pointillés, pas de lasso ; au relâchement, seul le trait de gauche est pris', async () => {
        const seen = await dragSeen(rectangleDrag(at(0.08, 0.3), at(0.45, 0.5)));
        if (!seen.marquee || !seen.dashed || seen.lasso) throw new Error(`while dragging: ${j(seen)}`);
        await app.waitFor(`${selectedCount(l)} === 1`, 'the left stroke selected');
        await app.waitFor(frameShown, 'the selection frame');
        const after = await app.evaluate(gesture);
        if (after.marquee || after.lasso) throw new Error(`still drawn after the release: ${j(after)}`);
      });

      await check('un second clic : le lasso, toujours l’outil actif ; un lasso autour du trait de droite le prend, et lui seul', async () => {
        await deselect();
        await app.click(SELECT_BUTTON);
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).dataset.way === 'lasso'`, 'the lasso');
        const s = await stateOf();
        expectState(s, 'lasso', true, 'after the second click');
        if (s.help !== l('editor.selectHelpLasso')) throw new Error(`the help line is ${j(s.help)}`);
        const outline = [at(0.5, 0.5), at(0.92, 0.5), at(0.92, 0.75), at(0.5, 0.75), at(0.5, 0.52)];
        const seen = await dragSeen(outline.flatMap((p, i) => (i === 0 ? [p] : rectangleDrag(outline[i - 1], p, 4).slice(1))));
        if (seen.marquee || !seen.lasso) throw new Error(`while dragging: ${j(seen)}`);
        await app.waitFor(`${selectedCount(l)} === 1`, 'the right stroke selected');
      });

      await check('la touche V : le rectangle, puis le lasso, un tour par appui', async () => {
        await deselect();
        await app.key('v');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).dataset.way === 'rectangle'`, 'the rectangle by the V key');
        expectState(await stateOf(), 'rectangle', true, 'after V');
        await app.key('v');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).dataset.way === 'lasso'`, 'the lasso by the V key');
        expectState(await stateOf(), 'lasso', true, 'after V again');
      });

      await check('un autre outil puis Sélection : la dernière façon, le lasso ici ; la touche P puis V de même ; le bouton la propose entre-temps', async () => {
        await app.click('.toolbar .tool', l('editor.modes.eraser'));
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'false'`, 'the eraser chosen');
        expectState(await stateOf(), 'lasso', false, 'with the eraser');
        await app.click(SELECT_BUTTON);
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the selection tool back');
        expectState(await stateOf(), 'lasso', true, 'back from the eraser');
        await app.key('p');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'false'`, 'the pen chosen');
        await app.key('v');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the selection tool by the V key');
        expectState(await stateOf(), 'lasso', true, 'back from the pen by V');
        // Turned to the rectangle, it is the rectangle that comes back.
        await app.key('v');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).dataset.way === 'rectangle'`, 'the rectangle');
        await app.key('p');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'false'`, 'the pen chosen again');
        if ((await way()) !== 'rectangle') throw new Error('the button does not offer the way used last');
        await app.key('v');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the selection tool again');
        expectState(await stateOf(), 'rectangle', true, 'back in the rectangle');
      });

      await check('le rectangle prend ce que le lasso prend pour la même zone : tout le contenu de la page, d’un seul glissement', async () => {
        await deselect();
        await dragSeen(rectangleDrag(at(0.05, 0.2), at(0.95, 0.85)));
        await app.waitFor(`${selectedCount(l)} === 2`, 'both strokes selected');
        if ((await selectState(app)).way !== 'rectangle') throw new Error('the way changed with a selection');
      });
    },
  },
  {
    name: 'direct : l’outil Sélection fait un rectangle ou un lasso comme dans l’éditeur, la tablette écrivant, et se quitte par « Stylo »',
    args: ['--simulated-tablet'],
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let box;
      const corner = (fx, fy) => ({ x: box.x + box.w * fx, y: box.y + box.h * fy });
      const strokes = `Number(document.querySelector(${j(LIVE_CANVAS)})?.dataset.strokes ?? -1)`;
      const stateOf = () =>
        app.evaluate(`(() => {
          const b = document.querySelector(${j(SELECT_BUTTON)});
          const help = document.querySelector('[data-slot="select-hint"]');
          return { way: b.dataset.way, pressed: b.getAttribute('aria-pressed'), name: b.getAttribute('aria-label'), label: b.textContent.trim(), help: help ? help.textContent : null, icon: b.querySelector('svg rect') ? 'rectangle' : 'lasso' };
        })()`);
      const nameOf = (way) => l(way === 'lasso' ? 'live.select.lasso' : 'live.select.rectangle');
      const expectState = (s, way, pressed, what) => {
        if (s.way !== way || s.name !== nameOf(way) || s.label !== l('live.select.toggle') || s.pressed !== String(pressed) || s.icon !== way) {
          throw new Error(`${what}: the button is ${j(s)}, expected the ${way}, pressed ${pressed}`);
        }
      };
      const pressedNow = () => app.evaluate(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed')`);
      const refresh = async () => {
        box = await app.evaluate(`(() => { const r = document.querySelector(${j(LIVE_CANVAS)}).getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`);
      };

      await registerTablet(app, check);

      await check('ouvrir le mode direct (tablette simulée) : le bouton « Sélection » propose une façon, l’outil est coupé', async () => {
        await openLive(app);
        await app.waitFor(`${strokes} >= 0`, 'the preview');
        await refresh();
        const s = await stateOf();
        expectState(s, s.way, false, 'before any choice');
      });

      await check('la tablette écrit : au moins six traits finis sur la page', async () => {
        await app.waitFor(`${strokes} >= 6`, 'six strokes written by the simulated pen', 60_000);
      });

      await check('un clic sur « Sélection » : l’outil est actif ; un rectangle sur le haut de la page prend des traits, la tablette continue d’écrire', async () => {
        await chooseSelect(app, 'rectangle');
        const s = await stateOf();
        expectState(s, 'rectangle', true, 'rectangle');
        if (s.help !== l('live.select.helpRectangle')) throw new Error(`the help line is ${j(s.help)}`);
        const before = await app.evaluate(strokes);
        await app.drag(rectangleDrag(corner(0.03, 0.03), corner(0.97, 0.3), 10));
        await app.waitFor(`${barCount} > 0`, 'the rectangle to choose strokes');
        const picked = await app.evaluate(barCount);
        await app.waitFor(`${strokes} > ${before}`, 'the tablet to go on writing under the selection', 30_000);
        console.log(`    rectangle : ${picked} traits pris`);
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${j(BAR)}) === null`, 'Esc to deselect');
        if ((await app.evaluate(`document.querySelector('.live-view') !== null`)) !== true) throw new Error('Esc left live mode');
        if ((await pressedNow()) !== 'true') throw new Error('Esc left the Select tool');
      });

      await check('la touche V : le lasso, l’outil reste actif ; un lasso sur le haut de la page prend des traits aussi', async () => {
        await app.key('v');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).dataset.way === 'lasso'`, 'the lasso');
        const s = await stateOf();
        expectState(s, 'lasso', true, 'after V');
        if (s.help !== l('live.select.helpLasso')) throw new Error(`the help line is ${j(s.help)}`);
        const outline = [corner(0.03, 0.03), corner(0.97, 0.03), corner(0.97, 0.3), corner(0.03, 0.3), corner(0.03, 0.05)];
        await app.drag(outline.flatMap((p, i) => (i === 0 ? [p] : rectangleDrag(outline[i - 1], p, 6).slice(1))));
        await app.waitFor(`${barCount} > 0`, 'the lasso to choose strokes');
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${j(BAR)}) === null`, 'Esc to deselect');
      });

      await check('« Stylo » quitte la Sélection ; en revenir redonne le lasso (la dernière façon), et un clic sur le bouton le tourne', async () => {
        await app.click('button[data-action="pen"]');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'false'`, 'the Pen tool to take over');
        expectState(await stateOf(), 'lasso', false, 'with the pen');
        await app.click(SELECT_BUTTON);
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the Select tool back');
        expectState(await stateOf(), 'lasso', true, 'back from the pen');
        await app.click(SELECT_BUTTON);
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).dataset.way === 'rectangle'`, 'the rectangle');
        if ((await pressedNow()) !== 'true') throw new Error('a click on the button left the tool');
        // Nothing but the pen leaves it.
        await app.click('button[data-action="pen"]');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'false'`, 'the Pen tool again');
        await app.click(SELECT_BUTTON);
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the Select tool back');
        expectState(await stateOf(), 'rectangle', true, 'back in the rectangle');
      });
    },
  },
];
