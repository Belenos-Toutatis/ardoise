// The editor's selection tool has two ways (docs/questions.md Q22): a click on its button from another tool gives the
// rectangle, a click while it is the tool turns to the lasso, then the rectangle again; the V key does the same. The button,
// its title and the help line say which way is on. Real mouse and key events on the production executable.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
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

export const selectWayJourneys = [
  {
    name: 'éditeur : l’outil Sélection fait un rectangle, puis un lasso, puis un rectangle, au clic comme à la touche V',
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
      const stateOf = async () => {
        const s = await app.evaluate(`(() => {
          const b = document.querySelector(${j(SELECT_BUTTON)});
          const help = document.querySelector('.editor .select-help');
          return { title: b.title, way: b.dataset.way, pressed: b.getAttribute('aria-pressed'), label: b.querySelector('.tool-label')?.textContent.trim(), help: help ? help.textContent : null, icon: b.querySelector('svg rect') ? 'rectangle' : 'lasso' };
        })()`);
        return s;
      };
      /** Escape drops a selection (and, with none, would leave the editor: never pressed then). */
      const deselect = async () => {
        if ((await app.evaluate(selectedCount(l))) === 0) return;
        await app.key('Escape');
        await app.waitFor(`${selectedCount(l)} === 0`, 'the selection dropped');
      };
      const expectState = (s, way, pressed, what) => {
        const title = l(way === 'lasso' ? 'editor.modes.selectLasso' : 'editor.modes.selectRectangle');
        if (s.way !== way || s.title !== title || s.label !== title || s.pressed !== String(pressed) || s.icon !== way) {
          throw new Error(`${what}: the button is ${j(s)}, expected the ${way} (${title}), pressed ${pressed}`);
        }
      };

      await check('une nouvelle note, deux traits à gauche et à droite : le stylo est l’outil, le bouton propose le rectangle', async () => {
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
        expectState(await stateOf(), 'rectangle', false, 'with the pen');
      });

      await check('un clic sur le bouton : le rectangle, l’outil actif, l’icône et la ligne d’aide le disent', async () => {
        await app.click(SELECT_BUTTON);
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the selection tool');
        const s = await stateOf();
        expectState(s, 'rectangle', true, 'after the first click');
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

      await check('un autre outil puis Sélection : le rectangle, même si le lasso était choisi ; la touche P puis V de même', async () => {
        await app.click('.toolbar .tool', l('editor.modes.eraser'));
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'false'`, 'the eraser chosen');
        expectState(await stateOf(), 'rectangle', false, 'with the eraser');
        await chooseSelect(app, 'rectangle');
        expectState(await stateOf(), 'rectangle', true, 'back from the eraser');
        await app.key('v');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).dataset.way === 'lasso'`, 'the lasso again');
        await app.key('p');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'false'`, 'the pen chosen');
        await app.key('v');
        await app.waitFor(`document.querySelector(${j(SELECT_BUTTON)}).getAttribute('aria-pressed') === 'true'`, 'the selection tool by the V key');
        expectState(await stateOf(), 'rectangle', true, 'back from the pen by V');
      });

      await check('le rectangle prend ce que le lasso prend pour la même zone : tout le contenu de la page, d’un seul glissement', async () => {
        await deselect();
        await dragSeen(rectangleDrag(at(0.05, 0.2), at(0.95, 0.85)));
        await app.waitFor(`${selectedCount(l)} === 2`, 'both strokes selected');
        if ((await selectState(app)).way !== 'rectangle') throw new Error('the way changed with a selection');
      });
    },
  },
];
