// Milestone M8, part "input": pen (pressure, barrel button, eraser end), palm rejection, finger
// gestures, "Draw with a finger", fit the width. Same pattern as journeys-m7-edit.mjs (labels from
// the translation files); not wired into run.mjs yet: the integrator adds `inputJourneys` to the list.
//
// The pen and the fingers are synthesised through the DevTools protocol:
// `Input.dispatchMouseEvent` with `pointerType: 'pen'`, `force`, `tiltX/tiltY` and `buttons`, and
// `Input.dispatchTouchEvent` with one or several touch points. Whether the protocol can send the
// eraser button (32) depends on the WebView2 version: that one check says so in its message
// instead of failing obscurely.
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
const view = `(() => { const [scale, x, y] = document.querySelector('.editor svg.canvas').getAttribute('data-view').split(' ').map(Number); return { scale, x, y }; })()`;
const canvasBox = `(() => { const r = document.querySelector('.editor svg.canvas').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`;

/** A pen event: `type` is mousePressed / mouseMoved / mouseReleased. */
const pen = (app, type, x, y, { buttons = 0, force = 0.5 } = {}) =>
  app.send('Input.dispatchMouseEvent', {
    type,
    x,
    y,
    pointerType: 'pen',
    button: type === 'mouseMoved' && buttons === 0 ? 'none' : 'left',
    buttons,
    force,
    tiltX: 10,
    tiltY: -5,
    clickCount: type === 'mouseMoved' ? 0 : 1,
  });

/** A pen stroke along `points` with the given buttons held and a pressure that grows along the line. */
async function penStroke(app, points, { buttons = 1, force = (i, n) => 0.1 + (0.8 * i) / n } = {}) {
  const [first, ...rest] = points;
  await pen(app, 'mouseMoved', first.x, first.y);
  await pen(app, 'mousePressed', first.x, first.y, { buttons, force: force(0, points.length) });
  for (const [i, p] of rest.entries()) {
    await pen(app, 'mouseMoved', p.x, p.y, { buttons, force: force(i + 1, points.length) });
    await sleep(8);
  }
  const last = rest.at(-1) ?? first;
  await pen(app, 'mouseReleased', last.x, last.y, { buttons: 0 });
}

const touch = (app, type, points) =>
  app.send('Input.dispatchTouchEvent', {
    type,
    touchPoints: points.map((p, i) => ({ x: p.x, y: p.y, id: p.id ?? i + 1, force: 1 })),
  });

/** One finger moving along `points`. */
async function fingerStroke(app, points) {
  const [first, ...rest] = points;
  await touch(app, 'touchStart', [first]);
  for (const p of rest) {
    await touch(app, 'touchMove', [p]);
    await sleep(16);
  }
  await touch(app, 'touchEnd', []);
}

const line = (x0, y0, x1, y1, n = 24) =>
  Array.from({ length: n + 1 }, (_, i) => ({ x: x0 + ((x1 - x0) * i) / n, y: y0 + ((y1 - y0) * i) / n }));

async function openFirstNote(app) {
  await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
  await app.click('.note-card .card-open');
  await app.waitFor(`${count} > 0`, 'the strokes of the note');
  // The page is fitted once the editor has measured its stage.
  await sleep(300);
}

export const inputJourneys = [
  {
    name: 'stylet : pression, lasso du bouton, dos du stylet',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let initial;
      let box;

      await check('ouvrir une note de démonstration', async () => {
        await openFirstNote(app);
        initial = await app.evaluate(count);
        box = await app.evaluate(canvasBox);
      });

      await check('un trait au stylet, de plus en plus appuyé : un trait de plus', async () => {
        await app.click(`.toolbar button[title="${l('editor.modes.pen')}"]`);
        await penStroke(app, line(box.x + box.w * 0.55, box.y + box.h * 0.85, box.x + box.w * 0.85, box.y + box.h * 0.85));
        await app.waitFor(`${count} === ${initial + 1}`, 'the pen stroke');
      });

      await check('le bouton du stylet trace un lasso (outil Stylo gardé) : la sélection apparaît', async () => {
        const loop = [
          [0.05, 0.05],
          [0.95, 0.05],
          [0.95, 0.95],
          [0.05, 0.95],
          [0.05, 0.1],
        ].map(([fx, fy]) => ({ x: box.x + box.w * fx, y: box.y + box.h * fy }));
        await penStroke(app, loop, { buttons: 3, force: () => 0.5 });
        await app.waitFor(`document.querySelector('.editor svg.canvas rect.selection') !== null`, 'the selection');
        if ((await app.evaluate(count)) !== initial + 1) throw new Error('the lasso changed the strokes');
        await app.key('Escape');
      });

      // `Input.dispatchMouseEvent` has no eraser button: WebView2 drops the bit 32 and delivers a plain
      // tip contact (buttons 1). The eraser end is then covered by the unit tests only (Canvas.pen-touch.test.ts)
      // and by docs/hardware-tests.md (H3 A2); this check says so instead of failing.
      await check('le dos du stylet efface le trait qu’il traverse (bouton 32), sans changer d’outil ; sauté si le protocole DevTools ne sait pas envoyer ce bouton', async () => {
        await app.evaluate(
          `(() => { window.__eraserSeen = false; document.querySelector('.editor svg.canvas').addEventListener('pointerdown', (e) => { if (e.buttons & 32) window.__eraserSeen = true; }, true); })()`,
        );
        const before = await app.evaluate(count);
        const y = box.y + box.h * 0.85;
        await penStroke(app, line(box.x + box.w * 0.5, y, box.x + box.w * 0.9, y), { buttons: 32, force: () => 0.5 });
        if (!(await app.evaluate('window.__eraserSeen'))) {
          console.log('    (sauté : le protocole DevTools ne transmet pas le bouton gomme (32) ; à vérifier à la main, docs/hardware-tests.md H3 A2)');
          // The pen came down as a plain tip: whatever it drew is not what this check is about.
          return;
        }
        try {
          await app.waitFor(`${count} < ${before}`, 'the erased stroke', 4000);
        } catch {
          throw new Error('the eraser end reached the page (buttons 32) but the stroke was not erased');
        }
      });
    },
  },
  {
    name: 'tactile : défilement, pincement, double toucher, paume, dessin au doigt, largeur',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let initial;
      let box;
      let fitted;

      await check('ouvrir une note de démonstration', async () => {
        await openFirstNote(app);
        initial = await app.evaluate(count);
        box = await app.evaluate(canvasBox);
        fitted = await app.evaluate(view);
      });

      await check('un doigt fait défiler la page sans dessiner', async () => {
        await fingerStroke(app, line(box.x + box.w * 0.4, box.y + box.h * 0.5, box.x + box.w * 0.55, box.y + box.h * 0.6));
        const after = await app.evaluate(view);
        if (Math.abs(after.x - fitted.x) < 20) throw new Error(`the page did not move (${JSON.stringify(after)})`);
        if (after.scale !== fitted.scale) throw new Error('the zoom changed');
        if ((await app.evaluate(count)) !== initial) throw new Error('a finger drew');
      });

      await check('deux doigts qui s’écartent zooment', async () => {
        const cx = box.x + box.w / 2;
        const cy = box.y + box.h / 2;
        const before = await app.evaluate(view);
        await touch(app, 'touchStart', [{ x: cx - 40, y: cy, id: 1 }, { x: cx + 40, y: cy, id: 2 }]);
        for (let step = 1; step <= 10; step++) {
          await touch(app, 'touchMove', [
            { x: cx - 40 - step * 12, y: cy, id: 1 },
            { x: cx + 40 + step * 12, y: cy, id: 2 },
          ]);
          await sleep(16);
        }
        await touch(app, 'touchEnd', []);
        const after = await app.evaluate(view);
        if (after.scale < before.scale * 2) throw new Error(`no zoom (${before.scale} -> ${after.scale})`);
      });

      await check('un double toucher ajuste la page', async () => {
        const x = box.x + box.w * 0.5;
        const y = box.y + box.h * 0.5;
        for (let i = 0; i < 2; i++) {
          await touch(app, 'touchStart', [{ x, y }]);
          await sleep(40);
          await touch(app, 'touchEnd', []);
          await sleep(80);
        }
        await app.waitFor(`Math.abs(${view}.scale - ${fitted.scale}) < 1e-6`, 'the page fitted again');
      });

      await check('le stylet qui survole : les doigts sont ignorés (paume)', async () => {
        const before = await app.evaluate(view);
        await pen(app, 'mouseMoved', box.x + box.w * 0.2, box.y + box.h * 0.2);
        await fingerStroke(app, line(box.x + box.w * 0.4, box.y + box.h * 0.5, box.x + box.w * 0.6, box.y + box.h * 0.6));
        const after = await app.evaluate(view);
        if (after.x !== before.x || after.y !== before.y) throw new Error('a finger moved the page under the hovering pen');
        // The pen leaves (the mouse takes over), then the fingers count again.
        await app.mouse('mouseMoved', box.x + box.w * 0.3, box.y + box.h * 0.3);
        await sleep(1500);
      });

      await check(`« ${l('editor.touch.draw')} » : le doigt dessine, et un deuxième doigt annule le trait`, async () => {
        await app.click('[data-action="finger-draw"]');
        await app.waitFor(`document.querySelector('[data-action="finger-draw"]').getAttribute('aria-pressed') === 'true'`, 'the pressed toggle');
        await app.click(`.toolbar button[title="${l('editor.modes.pen')}"]`);
        await fingerStroke(app, line(box.x + box.w * 0.55, box.y + box.h * 0.8, box.x + box.w * 0.8, box.y + box.h * 0.8));
        await app.waitFor(`${count} === ${initial + 1}`, 'the stroke drawn with a finger');
        // A second finger comes down while the first one draws: no stroke is added.
        const x = box.x + box.w * 0.6;
        const y = box.y + box.h * 0.7;
        await touch(app, 'touchStart', [{ x, y, id: 1 }]);
        await touch(app, 'touchMove', [{ x: x + 60, y, id: 1 }]);
        await touch(app, 'touchStart', [{ x: x + 60, y, id: 1 }, { x: x + 200, y: y + 50, id: 2 }]);
        await touch(app, 'touchMove', [{ x: x + 60, y, id: 1 }, { x: x + 260, y: y + 50, id: 2 }]);
        await touch(app, 'touchEnd', []);
        await sleep(200);
        if ((await app.evaluate(count)) !== initial + 1) throw new Error('the interrupted stroke was kept');
        await app.click('[data-action="finger-draw"]');
      });

      await check(`« ${l('editor.zoom.fitWidth')} » à côté de « ${l('editor.zoom.fit')} »`, async () => {
        await app.click('.zoom button', l('editor.zoom.fit'));
        await app.click('[data-action="fit-width"]');
        const wide = await app.evaluate(view);
        if (Math.abs(wide.x - 24) > 1) throw new Error(`the page does not start at the margin (${JSON.stringify(wide)})`);
        // Ctrl + 2 does the same after another zoom.
        await app.click('.zoom button', l('editor.zoom.fit'));
        await app.key('2', { ctrl: true });
        const again = await app.evaluate(view);
        if (Math.abs(again.scale - wide.scale) > 1e-9) throw new Error('Ctrl + 2 does not fit the width');
      });
    },
  },
];
