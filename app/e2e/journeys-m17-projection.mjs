// M17 journey, the editor's projection (docs/design/paper-projection-narration.md §7): « éditeur projeté : un trait de
// l'éditeur au vidéoprojecteur ». Package D writes it; it needs package C (the Rust side, `projection.rs`: the window
// `index.html?view=projection` and the five commands) and package E (the editor's wiring: `EditorProjectionTab` in the
// Projection tab, the feed fed from `Editor.svelte` and `Canvas.svelte`'s `oninprogress` / `onvisible`, the dashed frame
// `[data-slot="projected-area"]`, the blue dot of the tab while projecting). Helpers in `m17-helpers.mjs`.
//
// On the production executable, in French and in English; the share is on the loopback (`[share] port` and
// `ARDOISE_SHARE_LOOPBACK`, as `journeys-m16-editor.mjs`); the browser flags of `BROWSER_ARGS` keep the main window painting
// while the projection window covers it (a PC with one screen projects on it).
//
// The steps: a two-page PDF becomes two notes, page 1 opens; Projection, « Projeter »: the projection window shows the
// editor's strokes (`data-drawn`) on the PDF (`data-image`); a pen stroke drawn slowly shows before it is lifted (the
// window's computer latency grows, `data-drawn` counts it), lifted it is one more, the median latency under 50 ms; Ctrl+Z,
// Ctrl+Y; the chosen layer hidden then shown; + twice (`data-zoom` 1.56, the dashed frame in the editor), the editor zoomed
// with the wheel off its centre (the projected area moves, its zoom stays), « Page entière » (zoom 1, no frame); the replay
// (R) projected; Alt + → (saved, the next note projected); the share with it (the blue dot, the stream's note); Esc in the
// projection window (gone, the tab says so); projected again, back to the library: the window goes with the editor.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { closePanel, EventStream, openPanel, PANEL, request, until } from './journeys-m11-share.mjs';
import { EDITOR_STROKES, libraryBack, text } from './journeys-m12-resume.mjs';
import { twoPagePdf } from './journeys-m7-background.mjs';
import { BROWSER_ARGS, canvasFacts, projectionWindow } from './m17-helpers.mjs';
import { openTab, tabButton, tabPanel } from './side-panel.mjs';

const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const PDF_NAME = 'fiche-e2e.pdf';
const DIALOG = '[role="dialog"]';
const ALERT = '[role="alertdialog"]';
const ZONE = '[data-section="zone"]';
const FRAME = '[data-slot="projected-area"]';
const strokeCount = `document.querySelectorAll(${j(EDITOR_STROKES)}).length`;
const canvasBox = `(() => { const r = document.querySelector('.editor svg.canvas').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`;
const editorPath = `document.querySelector('.editor')?.dataset.path ?? null`;
const dot = `(document.querySelector(${j(tabButton('projection'))})?.getAttribute('data-dot') ?? null)`;
const presentationKey = `(document.querySelector(${j(`${tabPanel('projection')} [data-presentation]`)})?.getAttribute('data-presentation') ?? null)`;
/** The editor's dashed frame of the projected area (page units, `getBBox` of whatever shape draws it), or null. */
const frameBox = `(() => { const el = document.querySelector(${j(FRAME)}); if (!el) return null; const b = el.getBBox(); return { x: b.x, y: b.y, width: b.width, height: b.height }; })()`;
/** A coarse picture of what a window's canvas shows (a grid of pixels): it changes when the projected area moves. */
const canvasSignature = `(() => {
  const c = document.querySelector('canvas.live-canvas');
  const ctx = c.getContext('2d');
  let s = '';
  for (let gy = 1; gy < 12; gy++) for (let gx = 1; gx < 12; gx++) {
    const d = ctx.getImageData(Math.floor((c.width * gx) / 12), Math.floor((c.height * gy) / 12), 1, 1).data;
    s += (d[0] >> 5) + '' + (d[1] >> 5) + (d[2] >> 5);
  }
  return s;
})()`;
const median = (values) => {
  const sorted = [...values].sort((a, b) => a - b);
  return sorted.length ? sorted[Math.floor(sorted.length / 2)] : null;
};

/** A pen event of the DevTools protocol (`journeys-m8-input.mjs`): `pointerType: 'pen'`, the tip held with `buttons: 1`. */
const pen = (app, type, x, y, { buttons = 0, force = 0.5 } = {}) =>
  app.send('Input.dispatchMouseEvent', {
    type,
    x,
    y,
    pointerType: 'pen',
    button: type === 'mouseMoved' && buttons === 0 ? 'none' : 'left',
    buttons,
    force,
    clickCount: type === 'mouseMoved' ? 0 : 1,
  });

/** Points of a gentle wave across the editor's canvas, at height `fy` (a fraction of the canvas). */
function wave(box, fy, n = 60) {
  return Array.from({ length: n + 1 }, (_, i) => ({
    x: box.x + box.w * (0.3 + (0.4 * i) / n),
    y: box.y + box.h * (fy + 0.02 * Math.sin(i / 4)),
  }));
}

/** Whether the window `view=projection` is listed by the DevTools endpoint. */
async function projectionListed(app) {
  const list = await (await fetch(`http://127.0.0.1:${app.port}/json/list`)).json();
  return list.some((t) => t.type === 'page' && t.url.includes('view=projection'));
}

/** Joins the share with the code, as a browser would; the stream. */
async function joinStream(app, code) {
  const reply = await request(app.sharePort, { method: 'POST', path: '/join', headers: { 'Content-Type': 'application/json' }, body: j({ code }) });
  if (reply.status !== 200) throw new Error(`POST /join answered ${reply.status}`);
  const cookie = [reply.headers['set-cookie'] ?? []].flat().join('\n').split(';')[0].trim();
  return EventStream.open(app.sharePort, cookie);
}

export const editorProjectionJourneys = [
  {
    name: 'éditeur projeté : un trait de l’éditeur au vidéoprojecteur',
    settings: (ctx) => `[share]\nport = ${ctx.sharePort}\n`,
    env: { ARDOISE_SHARE_LOOPBACK: '1' },
    browserArgs: BROWSER_ARGS,
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const pdf = join(app.dir, PDF_NAME);
      writeFileSync(pdf, twoPagePdf());
      let projection = null;
      let box = null;
      let strokes = 0;
      let stream = null;
      const facts = () => canvasFacts(projection);
      const drawnIs = (n, what) => projection.waitFor(`Number(document.querySelector('canvas.live-canvas')?.dataset.drawn) === ${n}`, what, 15_000);
      const zoomNear = (zoom, what) =>
        projection.waitFor(`Math.abs(Number(document.querySelector('canvas.live-canvas')?.dataset.zoom) - ${zoom}) < 0.01`, what, 15_000);

      await check('un PDF de deux pages devient deux notes ; la page 1 s’ouvre dans l’éditeur', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await app.evaluate(`window.__ardoiseE2eOpenPath = ${j(pdf)}`);
        await app.click('button[data-action="new-from-file"]');
        await app.waitFor(`document.querySelector(${j(`${DIALOG} [data-slot="new-from-file-name"]`)})?.textContent === ${j(PDF_NAME)}`, 'the chosen PDF');
        await app.click(`${DIALOG} .actions button.primary`);
        await app.waitFor(`document.querySelector(${j(DIALOG)}) === null`, 'the dialog to close', 30_000);
        await app.waitFor(`document.querySelectorAll('.note-card').length === 10`, 'the two new notes', 15_000);
        await app.click('.note-card .card-open', 'fiche-e2e - page 1');
        await app.waitFor(
          `document.querySelector('[data-section="background-image"] [data-slot="background-image-name"]')?.textContent.includes(${j(PDF_NAME)})`,
          'the background image of the note',
          20_000,
        );
        // The page is fitted once the editor has measured its stage.
        await sleep(300);
        box = await app.evaluate(canvasBox);
        strokes = await app.evaluate(strokeCount);
      });

      await check('Projection, « Projeter » : la fenêtre projetée montre les traits de l’éditeur sur le PDF', async () => {
        await openTab(app, 'projection');
        const help = await app.evaluate(`document.querySelector(${j(`${tabPanel('projection')} [data-section="projector"]`)}).textContent`);
        if (!help.includes(l('editor.projection.help'))) throw new Error('the projector block does not say what projecting does here');
        if ((await app.evaluate(`document.querySelector(${j(`${tabPanel('projection')} [data-field="board"]`)}) !== null`)) === true) {
          throw new Error('the editor offers the blackboard');
        }
        await app.waitFor(`document.querySelector(${j(`${tabPanel('projection')} button[data-action="project"]:not([disabled])`)}) !== null`, 'the Project button');
        await app.click(`${tabPanel('projection')} button[data-action="project"]`);
        projection = await projectionWindow(app);
        await projection.waitFor(`document.querySelector('canvas.live-canvas') !== null`, 'the projection canvas');
        await app.waitFor(`(${presentationKey}) !== ''`, 'the tab to say where it projects', 15_000);
        await app.waitFor(`(${dot}) === 'live'`, 'the blue dot of the Projection tab');
        await drawnIs(strokes, 'the editor’s strokes on the projector');
        await projection.waitFor(`document.querySelector('canvas.live-canvas').dataset.image === ${j(PDF_NAME)}`, 'the PDF under the projected page', 20_000);
        await projection.waitFor(`document.querySelector('canvas.live-canvas').dataset.paperReady === 'true'`, 'the paper drawn with the PDF', 30_000);
        const label = await projection.evaluate(`document.querySelector('main')?.getAttribute('aria-label')`);
        if (label !== l('presentation.editorTitle')) throw new Error(`the projection window is named ${j(label)}`);
        if ((await projection.evaluate(`document.querySelectorAll('button, input, select').length`)) !== 0) throw new Error('the projection window shows controls');
      });

      await check('un trait au stylet, tracé lentement : il se voit au vidéoprojecteur avant d’être levé, puis il compte ; latence médiane < 50 ms', async () => {
        await openTab(app, 'layers');
        await app.click(`.toolbar button[title="${l('editor.modes.pen')}"]`);
        const samples = `window.__ardoiseLatency?.computer?.samples().length ?? 0`;
        const before = await projection.evaluate(samples);
        const points = wave(box, 0.55);
        await pen(app, 'mouseMoved', points[0].x, points[0].y);
        await pen(app, 'mousePressed', points[0].x, points[0].y, { buttons: 1 });
        for (const p of points.slice(1, 40)) {
          await pen(app, 'mouseMoved', p.x, p.y, { buttons: 1, force: 0.6 });
          await sleep(16);
        }
        // Still down: the projector shows the stroke in progress and measured its points.
        await drawnIs(strokes + 1, 'the stroke in progress on the projector');
        await projection.waitFor(`(${samples}) > ${before}`, 'the latency of the editor’s points on the projector');
        if ((await app.evaluate(strokeCount)) !== strokes) throw new Error('the editor counts the stroke before it is lifted');
        for (const p of points.slice(40)) {
          await pen(app, 'mouseMoved', p.x, p.y, { buttons: 1, force: 0.6 });
          await sleep(16);
        }
        await pen(app, 'mouseReleased', points.at(-1).x, points.at(-1).y);
        await app.waitFor(`${strokeCount} === ${strokes + 1}`, 'the new stroke in the editor');
        strokes += 1;
        await drawnIs(strokes, 'the stroke lifted, one of the page’s on the projector');
        await sleep(300);
        if ((await facts()).drawn !== strokes) throw new Error('the projector does not keep the lifted stroke once');
        const latency = await projection.evaluate(`window.__ardoiseLatency.computer.samples().slice(${before})`);
        const middle = median(latency);
        console.log(`    latence du trait de l’éditeur au vidéoprojecteur : médiane ${middle?.toFixed(1)} ms sur ${latency.length} points`);
        if (middle === null || !(middle < 50)) throw new Error(`the median latency of the editor's stroke on the projector is ${middle} ms`);
        // A second stroke for the replay further on.
        const second = wave(box, 0.7, 30);
        await pen(app, 'mouseMoved', second[0].x, second[0].y);
        await pen(app, 'mousePressed', second[0].x, second[0].y, { buttons: 1 });
        for (const p of second.slice(1)) {
          await pen(app, 'mouseMoved', p.x, p.y, { buttons: 1, force: 0.6 });
          await sleep(8);
        }
        await pen(app, 'mouseReleased', second.at(-1).x, second.at(-1).y);
        await app.waitFor(`${strokeCount} === ${strokes + 1}`, 'the second stroke in the editor');
        strokes += 1;
        await drawnIs(strokes, 'the second stroke on the projector');
      });

      await check('Ctrl+Z : un trait de moins au vidéoprojecteur ; Ctrl+Y : il revient', async () => {
        await app.key('z', { ctrl: true });
        await app.waitFor(`${strokeCount} === ${strokes - 1}`, 'the undo in the editor');
        await drawnIs(strokes - 1, 'the undo on the projector');
        await app.key('y', { ctrl: true });
        await app.waitFor(`${strokeCount} === ${strokes}`, 'the redo in the editor');
        await drawnIs(strokes, 'the redo on the projector');
      });

      await check('masquer le calque choisi : moins de traits au vidéoprojecteur ; l’afficher : ils reviennent', async () => {
        await openTab(app, 'layers');
        const eye = `${tabPanel('layers')} li.layer[data-chosen="true"] [data-action="layer-visible"]`;
        await app.click(eye);
        await projection.waitFor(`Number(document.querySelector('canvas.live-canvas').dataset.drawn) < ${strokes}`, 'fewer strokes drawn with the layer hidden', 15_000);
        await app.click(eye);
        await drawnIs(strokes, 'the strokes back with the layer shown');
      });

      await check('+ deux fois : zoom 1,56 au vidéoprojecteur, le cadre en pointillés dans l’éditeur ; la molette dans l’éditeur déplace la zone, pas son zoom', async () => {
        await openTab(app, 'projection');
        const zoomIn = `${tabPanel('projection')} ${ZONE} button[aria-label="${l('live.zoom.in')}"]`;
        await app.click(zoomIn);
        await app.click(zoomIn);
        await zoomNear(1.5625, 'the projector at zoom 1.56');
        const shown = await app.evaluate(`document.querySelector(${j(`${tabPanel('projection')} ${ZONE} [data-zoom]`)})?.getAttribute('data-zoom')`);
        if (Math.abs(Number(shown) - 1.5625) > 0.01) throw new Error(`the tab shows the zoom ${shown}`);
        await app.waitFor(`document.querySelector(${j(FRAME)}) !== null`, 'the dashed frame in the editor');
        const title = await app.evaluate(`document.querySelector(${j(`${tabPanel('projection')} button[data-action="page-width"]`)}).title`);
        if (title !== l('editor.projection.widthTitle')) throw new Error(`« Largeur » is titled ${j(title)}`);
        await sleep(400);
        const frameBefore = await app.evaluate(frameBox);
        const pictureBefore = await projection.evaluate(canvasSignature);
        // The wheel zooms the editor at the cursor, off its centre: what it shows moves, and its centre with it (towards
        // the bottom when the area already touches the top of the page, where it could not go further up).
        const towards = frameBefore.y < 1 ? 0.8 : 0.2;
        await app.send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: box.x + box.w / 2, y: box.y + box.h * towards, deltaX: 0, deltaY: -300 });
        await app.waitFor(
          `(() => { const b = ${frameBox}; return b !== null && Math.abs(b.y - ${frameBefore.y}) > 50; })()`,
          'the dashed frame to follow the editor’s view',
          10_000,
        );
        await projection.waitFor(`(${canvasSignature}) !== ${j(pictureBefore)}`, 'the projected area to move', 10_000);
        const after = await facts();
        if (Math.abs(after.zoom - 1.5625) > 0.01) throw new Error(`the projection's zoom changed to ${after.zoom} with the editor's`);
        const frameAfter = await app.evaluate(frameBox);
        if (Math.abs(frameAfter.width - frameBefore.width) > 1) throw new Error('the projected area changed size with the editor’s zoom');
      });

      await check('« Page entière » : zoom 1 au vidéoprojecteur, plus de cadre', async () => {
        await app.click(`${tabPanel('projection')} button[data-action="whole-page"]`);
        await zoomNear(1, 'the projector at zoom 1');
        await app.waitFor(`document.querySelector(${j(FRAME)}) === null`, 'the frame to go');
      });

      await check('Relecture (R) : le vidéoprojecteur suit la relecture (moins de traits, puis tous)', async () => {
        await app.key('r');
        await app.waitFor(`document.querySelector('button[data-action="replay-toggle"]')?.getAttribute('aria-pressed') === 'true'`, 'the replay');
        await projection.waitFor(`Number(document.querySelector('canvas.live-canvas').dataset.drawn) < ${strokes}`, 'the replay to start on the projector', 10_000);
        // R opens the replay paused at its start: « Lecture » plays it to its end.
        await app.click('button[data-action="replay-play"]');
        await drawnIs(strokes, 'the replay to show every stroke on the projector');
        await app.key('r');
        await app.waitFor(`document.querySelector('button[data-action="replay-toggle"]')?.getAttribute('aria-pressed') === 'false'`, 'the replay to end');
        await drawnIs(strokes, 'every stroke after the replay');
      });

      await check('Alt + → : enregistrée, la note suivante s’ouvre et passe au vidéoprojecteur, toujours ouvert', async () => {
        const before = await app.evaluate(editorPath);
        await app.key('ArrowRight', { alt: true });
        // The library's order decides whether page 1 has a next note; else the previous one (Alt + ←) does the same.
        const asked = await app.waitFor(`document.querySelector(${j(ALERT)}) !== null`, 'the unsaved changes dialog', 3000).catch(() => false);
        if (!asked) {
          await app.key('ArrowLeft', { alt: true });
          await app.waitFor(`document.querySelector(${j(ALERT)}) !== null`, 'the unsaved changes dialog');
        }
        await app.click(`${ALERT} button`, l('editor.leave.saveSwitch'));
        await app.waitFor(`(${editorPath}) !== null && (${editorPath}) !== ${j(before)}`, 'the next note');
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'no'`, 'the next note, unmodified');
        await sleep(500);
        const next = await app.evaluate(strokeCount);
        await drawnIs(next, 'the next note’s strokes on the projector');
        if ((await app.evaluate(presentationKey)) === '') throw new Error('the projection closed with the next note');
      });

      await check('partager en même temps : le point bleu reste, le flux montre la note', async () => {
        await openTab(app, 'projection');
        await app.click(`${tabPanel('projection')} button[data-action="share"]`);
        await app.waitFor(`document.querySelector('[data-share-code]') !== null`, 'the access code', 15_000);
        const code = await app.evaluate(`document.querySelector('[data-share-code]').textContent.replace(/[\\s\\u00a0\\u202f]+/g, '')`);
        await closePanel(app, l('common.close'));
        await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the share panel to close');
        stream = await joinStream(app, code);
        const note = await stream.next((msg) => msg.type === 'note', 'the note message', { timeout: 10_000 });
        const editorStrokes = await app.evaluate(strokeCount);
        if (note.msg.page?.strokes?.length !== editorStrokes) throw new Error(`the stream's note has ${note.msg.page?.strokes?.length} strokes, the editor ${editorStrokes}`);
        if ((await app.evaluate(dot)) !== 'live') throw new Error('no blue dot with the projection and the share');
        if ((await app.evaluate(presentationKey)) === '') throw new Error('the projection closed with the share');
      });

      await check('Échap dans la fenêtre projetée : elle se ferme, l’onglet le dit ; le partage arrêté, plus de point bleu', async () => {
        // A moment later, so that the answer of this call comes back before the window goes.
        await projection.evaluate(
          `(setTimeout(() => window.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })), 50), true)`,
        );
        projection.close();
        projection = null;
        await until(async () => !(await projectionListed(app)), 'the projection window to close', 10_000);
        await app.waitFor(`(${presentationKey}) === ''`, 'the tab to say that nothing is projected');
        await app.waitFor(`document.querySelector(${j(`${tabPanel('projection')} button[data-action="project"]`)}) !== null`, 'the Project button back');
        // The share still runs: the dot stays.
        if ((await app.evaluate(dot)) !== 'live') throw new Error('the blue dot went with the projection while the share runs');
        stream.close();
        await openPanel(app);
        await app.click('button[data-action="share-stop"]');
        await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the share panel to close after the stop', 10_000);
        await app.waitFor(`(${dot}) === null`, 'no blue dot with nothing projected nor shared');
      });

      await check('projeter à nouveau, puis revenir à la bibliothèque : la fenêtre projetée se ferme', async () => {
        await app.click(`${tabPanel('projection')} button[data-action="project"]`);
        projection = await projectionWindow(app);
        await projection.waitFor(`document.querySelector('canvas.live-canvas') !== null`, 'the projection canvas again');
        const shown = await app.evaluate(strokeCount);
        await drawnIs(shown, 'the note on the projector again');
        projection.close();
        projection = null;
        await app.click('.editor-top button');
        await libraryBack(app);
        await until(async () => !(await projectionListed(app)), 'the projection window to close with the editor', 10_000);
      });
    },
  },
];
