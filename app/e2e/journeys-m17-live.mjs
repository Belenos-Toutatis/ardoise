// M17 journey, live paper and background (docs/design/paper-projection-narration.md §7): « direct : papier et fond de la
// page en cours ». Package B writes it, after package A (the Rust side of `live_page_paper` and `live_page_image`, the
// snapshot's `pagePaper` and `imagePixels`, the share's paper); helpers in `m17-helpers.mjs`.
//
// On the production executable with the simulated tablet (`--simulated-tablet`), in French and in English; the share is on
// the loopback (`[share] port` and `ARDOISE_SHARE_LOOPBACK`, as `journeys-m14-live.mjs`); the presentation window is opened
// through `open_presentation` and attached as `view=presentation`. The pictures of the paper are read in two places: the
// share's `GET /paper/<key>` (the whole page, as a viewer's browser gets it) and the pixels of the presentation window's
// canvas (what the projector shows).
//
// The steps: the projector and the share on; Page: Crème, Seyès, texture → the presentation's `data-paper="cream"`,
// `data-template="seyes"`, the stream's snapshot has a `paperKey` and a `livePaper` follows, the picture is a PNG with
// the Seyès lines' blue on a line; Fond: a page of a two-page PDF placed → `data-image`, `data-paper-ready`, its pixels dark
// where the PDF's square lands (contained, so the sides are the paper's cream), another `paperKey`; opacity 50 %; with the
// template select focused, Page Down turns the page (so does Page Up), the same with the opacity slider focused; the new
// page is cream Seyès with texture and no image, coming back finds the image; the blackboard: the swatches wait and
// `paper-board` says so, Lined chosen under the board shows at once, the board off gives the cream back; « Terminer »: the
// first note holds the paper, the image's description and a `background.png` member, the second the paper alone.
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { closePanel, EventStream, request, until } from './journeys-m11-share.mjs';
import { attach, canvasData, notesOf, registerTablet, STATUS, stopLive, text } from './journeys-m12-resume.mjs';
import { twoPagePdf, readPng } from './journeys-m7-background.mjs';
import { BROWSER_ARGS, canvasFacts } from './m17-helpers.mjs';
import { openTab, tabPanel } from './side-panel.mjs';

const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const PAPER = '[data-section="live-paper"]';
const FOND = '[data-section="background-image"]';
const swatch = (id) => `${PAPER} button[data-paper="${id}"]`;
const TEMPLATE = `${PAPER} select[data-field="template"]`;
const TEXTURE = `${PAPER} input[data-field="texture"]`;
const OPACITY = `${FOND} input[type="range"]`;
const PDF_NAME = 'fiche-e2e.pdf';
/** The PDF is A4 portrait (595 x 842 pt); its black square is 200 pt wide, 40 pt from the top left corner (`twoPagePdf`). */
const PDF_POINTS = { width: 595, height: 842 };
const SQUARE_CENTRE = { x: 140, y: 140 };
const CREAM = [250, 244, 226];
const WHITE = [255, 255, 255];

const near = (rgb, want, tolerance = 14) => rgb.every((value, i) => Math.abs(value - want[i]) <= tolerance);

/** Page expression: `window` of the control window sets the value of a control as a user would, firing its `change`. */
const choose = (selector, value) => `(() => {
  const el = document.querySelector(${j(selector)});
  if (!el) return false;
  el.value = ${j(String(value))};
  el.dispatchEvent(new Event('change', { bubbles: true }));
  return true;
})()`;
const focus = (selector) => `(() => { const el = document.querySelector(${j(selector)}); el?.focus(); return document.activeElement === el; })()`;
const isDisabled = (selector) => `document.querySelector(${j(selector)})?.disabled === true`;

/** Joins the share with the code, as a browser would; the stream and the cookie (`/paper/<key>` needs it). */
async function joinStream(app, code) {
  const reply = await request(app.sharePort, { method: 'POST', path: '/join', headers: { 'Content-Type': 'application/json' }, body: j({ code }) });
  if (reply.status !== 200) throw new Error(`POST /join answered ${reply.status}`);
  const cookie = [reply.headers['set-cookie'] ?? []].flat().join('\n').split(';')[0].trim();
  const stream = await EventStream.open(app.sharePort, cookie);
  await stream.next((msg) => msg.type === 'live' && msg.reset === true, 'the first message of the stream', { timeout: 10_000 });
  return { stream, cookie };
}

/** Starts the share from the Projection tab; returns its access code. */
async function startShare(app) {
  await openTab(app, 'projection');
  await app.click('button[data-action="share"]');
  await app.waitFor(`document.querySelector('[data-share-code]') !== null`, 'the access code', 15_000);
  return app.evaluate(`document.querySelector('[data-share-code]').textContent.replace(/[\\s\\u00a0\\u202f]+/g, '')`);
}

/** Opens live mode from the tablet panel and waits for the first strokes. */
async function openLive(app) {
  await app.click(STATUS);
  await app.waitFor(`document.querySelector('.tablet-panel button[data-action="live"]:not([disabled])') !== null`, 'the live button');
  await app.click('.tablet-panel button[data-action="live"]');
  await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
  await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'strokes on the live canvas', 15_000);
}

/** The `paperKey` of the last snapshot of the stream (null: plain paper), or undefined before any. */
function lastPaperKey(stream) {
  let found;
  for (const entry of stream.messages) {
    if (entry.msg.type !== 'live') continue;
    for (const event of entry.msg.batch?.events ?? []) if (event.type === 'snapshot') found = event.snapshot.paperKey ?? null;
  }
  return found;
}

/** The share's picture of a paper: `GET /paper/<key>`, a PNG of the whole page. */
async function fetchPaper(app, cookie, key) {
  const reply = await request(app.sharePort, { path: `/paper/${key}`, headers: { Cookie: cookie }, timeout: 20_000 });
  if (reply.status !== 200) throw new Error(`GET /paper/${key} answered ${reply.status}`);
  if (!/image\/png/.test(String(reply.headers['content-type']))) throw new Error(`the paper is ${reply.headers['content-type']}, not a PNG`);
  return readPng(reply.body);
}

/**
 * What the projector shows: the canvas of a window read back as pixels. The page is letterboxed on black, so the rectangle
 * of the page is found from the middle row and the middle column (non-black), and the PDF's rectangle (contained) in it.
 * Returns `{ page, at(x, y) }` with `at` giving the colour of a point of the PDF (points) mapped on the canvas.
 */
async function drawn(target) {
  const scan = await target.evaluate(`(() => {
    const canvas = document.querySelector('canvas.live-canvas');
    const ctx = canvas.getContext('2d');
    const { width, height } = canvas;
    const row = ctx.getImageData(0, Math.floor(height / 2), width, 1).data;
    const col = ctx.getImageData(Math.floor(width / 2), 0, 1, height).data;
    const lit = (data, i) => data[i * 4] + data[i * 4 + 1] + data[i * 4 + 2] > 60;
    const span = (data, n) => { let a = -1, b = -1; for (let i = 0; i < n; i++) if (lit(data, i)) { if (a < 0) a = i; b = i; } return [a, b]; };
    return { width, height, x: span(row, width), y: span(col, height) };
  })()`);
  const [x0, x1] = scan.x;
  const [y0, y1] = scan.y;
  if (x0 < 0 || y0 < 0) throw new Error('the projector shows no page (all black)');
  const page = { x: x0, y: y0, width: x1 - x0 + 1, height: y1 - y0 + 1 };
  // The PDF page contained in the page, centred (the mock of `fitRect`): the tablet's page is landscape.
  const scale = Math.min(page.width / PDF_POINTS.width, page.height / PDF_POINTS.height);
  const offset = { x: page.x + (page.width - PDF_POINTS.width * scale) / 2, y: page.y + (page.height - PDF_POINTS.height * scale) / 2 };
  const pixel = async (x, y) =>
    target.evaluate(`(() => { const d = document.querySelector('canvas.live-canvas').getContext('2d').getImageData(${Math.round(x)}, ${Math.round(y)}, 1, 1).data; return [d[0], d[1], d[2]]; })()`);
  return {
    page,
    scale,
    offset,
    /** A point of the PDF page, in its points. */
    pdf: (x, y) => pixel(offset.x + x * scale, offset.y + y * scale),
    /** A point of the page, as fractions of its width and height. */
    side: (fx, fy) => pixel(page.x + page.width * fx, page.y + page.height * fy),
    /** The margin of the page left of the PDF, a few places between the lines of the template: the cream of the paper. */
    margin: async () => {
      const x = page.x + (offset.x - page.x) / 2;
      const found = [];
      for (let k = 0; k < 9; k++) found.push(await pixel(x, page.y + page.height * (0.2 + 0.07 * k)));
      return found;
    },
  };
}

export const livePaperJourneys = [
  {
    name: 'direct : papier et fond de la page en cours',
    args: ['--simulated-tablet'],
    settings: (ctx) => `[share]\nport = ${ctx.sharePort}\n`,
    env: { ARDOISE_SHARE_LOOPBACK: '1' },
    browserArgs: BROWSER_ARGS,
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const pdf = join(app.dir, PDF_NAME);
      writeFileSync(pdf, twoPagePdf());
      let presentation = null;
      let stream = null;
      let cookie = '';
      let notesBefore = [];
      let firstKey = null;
      let cream = null;
      let withImage = null;
      const facts = () => canvasFacts(presentation);
      const goesTo = (n, what) => app.waitFor(`${canvasData('page')}?.startsWith('${n}/')`, what);
      const paperKeyChanged = async (from, what) => {
        await until(() => {
          const key = lastPaperKey(stream);
          return typeof key === 'string' && key !== from ? key : null;
        }, what, 20_000);
        return lastPaperKey(stream);
      };

      await registerTablet(app, check);

      await check('ouvrir le mode direct, projeter, partager ; le flux du partage et la fenêtre projetée sont lus', async () => {
        notesBefore = notesOf(app.library).map((n) => n.path);
        await openLive(app);
        const code = await startShare(app);
        ({ stream, cookie } = await joinStream(app, code));
        // The share panel is a dialog over the window: closed, the share goes on and the tabs can be clicked.
        await closePanel(app, l('common.close'));
        await app.evaluate(`window.__TAURI_INTERNALS__.invoke('open_presentation', { monitor: null }).then(() => true)`);
        presentation = await attach(app.port, (t) => t.url.includes('view=presentation'), 'the presentation window');
        await presentation.waitFor(`document.querySelector('canvas.live-canvas') !== null`, 'the presentation canvas');
        const first = await facts();
        if (first.paper !== 'white' || first.template !== 'blank' || first.image !== '') {
          throw new Error(`a new session does not start on plain white paper: ${j(first)}`);
        }
        // Plain paper: the share names no picture for it.
        if (lastPaperKey(stream)) throw new Error(`a plain white page has a paperKey: ${lastPaperKey(stream)}`);
        await openTab(app, 'page');
        for (const id of ['white', 'cream', 'kraft', 'slate']) {
          if ((await app.evaluate(`document.querySelector(${j(swatch(id))}) !== null`)) !== true) throw new Error(`the Page tab has no ${id} swatch`);
        }
        if ((await app.evaluate(`document.querySelector(${j(swatch('white'))}).getAttribute('aria-pressed')`)) !== 'true') throw new Error('white is not the chosen paper');
        const title = await app.evaluate(`document.querySelector(${j(`${PAPER} h3`)}).textContent.trim()`);
        if (title !== l('editor.page.paperTitle')) throw new Error(`the block is titled ${j(title)}`);
        const help = await app.evaluate(`document.querySelector(${j(PAPER)}).textContent`);
        for (const key of ['live.page.help', 'live.page.back']) {
          if (!help.includes(l(key))) throw new Error(`the Page tab does not say ${key}`);
        }
      });

      await check('Page : Crème, Seyès, texture ; la fenêtre projetée suit, le flux annonce un papier (paperKey puis livePaper)', async () => {
        const mark = stream.mark();
        await app.click(swatch('cream'));
        await app.evaluate(choose(TEMPLATE, 'seyes'));
        await app.click(TEXTURE);
        await presentation.waitFor(
          `(() => { const d = document.querySelector('canvas.live-canvas').dataset; return d.paper === 'cream' && d.template === 'seyes'; })()`,
          'the presentation to show the cream Seyès paper',
          15_000,
        );
        const shown = await facts();
        if (shown.paper !== 'cream' || shown.template !== 'seyes') throw new Error(`the projector shows ${j(shown)}`);
        // The control window shows the same.
        const control = await canvasFacts(app);
        if (control.paper !== 'cream' || control.template !== 'seyes') throw new Error(`the control window shows ${j(control)}`);
        firstKey = await until(() => {
          const key = lastPaperKey(stream);
          return typeof key === 'string' ? key : null;
        }, 'the stream’s snapshot to name a paper', 20_000);
        await stream.next((msg) => msg.type === 'livePaper' && msg.key === firstKey, 'the livePaper message of the cream Seyès paper', { from: mark, timeout: 30_000 });
        if (!(await app.evaluate(`document.querySelector(${j(TEXTURE)}).checked`))) throw new Error('the texture switch is off');
        await presentation.waitFor(`document.querySelector('canvas.live-canvas').dataset.paperReady === 'true'`, 'the paper picture drawn', 15_000);
      });

      await check('GET /paper/<clé> : un PNG de la page, du crème avec le bleu des lignes de Seyès sur une ligne', async () => {
        cream = await fetchPaper(app, cookie, firstKey);
        const x = Math.floor(cream.width * 0.55);
        let lines = 0;
        let creamy = 0;
        for (let y = 0; y < cream.height; y++) {
          const [r, g, b] = cream.rgb(x, y);
          if (b - r > 15) lines += 1;
          else if (near([r, g, b], CREAM, 24)) creamy += 1;
        }
        if (lines < 5) throw new Error(`only ${lines} pixels of the picture are the blue of a Seyès line in a column`);
        if (creamy < cream.height * 0.5) throw new Error(`the picture is not cream: ${creamy} of ${cream.height} pixels in a column`);
      });

      await check('Fond : une page du PDF de deux pages placée ; la fenêtre projetée la montre (carré noir, côtés crème), un autre paperKey', async () => {
        await app.evaluate(`window.__ardoiseE2eOpenPath = ${j(pdf)}`);
        await openTab(app, 'background');
        const hint = await app.evaluate(`document.querySelector(${j(`${FOND} .pane-help`)}).textContent.trim()`);
        if (hint !== l('live.background.help')) throw new Error(`the Fond tab says ${j(hint)}`);
        await app.click(`${FOND} button[data-action="background-choose"]`);
        await app.waitFor(`document.querySelector(${j(`${FOND} [data-slot="background-pdf"]`)}) !== null`, 'the pages of the PDF');
        // One thumbnail per page, the first chosen, its picture drawn by Windows (a real image, not the placeholder).
        await app.waitFor(`document.querySelectorAll(${j(`${FOND} [role="radio"]`)}).length === 2`, 'a thumbnail for each of the two pages');
        await app.waitFor(
          `(() => { const img = document.querySelector(${j(`${FOND} [role="radio"] img`)}); return !!img && img.complete && img.naturalWidth > 0; })()`,
          'the first thumbnail to be drawn',
          30_000,
        );
        const chosenPage = await app.evaluate(`document.querySelector(${j(`${FOND} [role="radio"][aria-checked="true"]`)})?.dataset.page`);
        if (chosenPage !== '1') throw new Error(`the page chosen to begin with is ${j(chosenPage)}, not 1`);
        await app.click(`${FOND} .pane-actions button.primary`);
        // A portrait PDF on the tablet's landscape page: no question about turning the page.
        await presentation.waitFor(`document.querySelector('canvas.live-canvas').dataset.image === ${j(PDF_NAME)}`, 'the PDF to be the page’s image', 30_000);
        if ((await app.evaluate(`document.querySelector(${j(`${FOND} [data-slot="background-orientation-offer"]`)}) !== null`)) === true) {
          throw new Error('the Fond tab asks about the orientation of the page');
        }
        await presentation.waitFor(`document.querySelector('canvas.live-canvas').dataset.paperReady === 'true'`, 'the picture of the paper with the image', 30_000);
        const shown = await facts();
        if (shown.image !== PDF_NAME || !shown.paperReady) throw new Error(`the projector shows ${j(shown)}`);
        const pixels = await drawn(presentation);
        const square = await pixels.pdf(SQUARE_CENTRE.x, SQUARE_CENTRE.y);
        if (!square.every((v) => v < 60)) throw new Error(`the PDF's square is not dark on the projector: ${j(square)}`);
        const sheet = await pixels.pdf(PDF_POINTS.width * 0.5, PDF_POINTS.height * 0.6);
        if (!near(sheet, WHITE, 10)) throw new Error(`the PDF's own white is ${j(sheet)} on the projector`);
        const margin = await pixels.margin();
        // Contained: the page's sides keep the paper (cream, between the lines).
        if (margin.filter((px) => near(px, CREAM, 14)).length < 3) throw new Error(`the sides of the page are not the paper's cream: ${j(margin)}`);
        withImage = await paperKeyChanged(firstKey, 'another paperKey for the same paper with the image');
        const image = await fetchPaper(app, cookie, withImage);
        const px = Math.round((image.width * (pixels.offset.x - pixels.page.x + SQUARE_CENTRE.x * pixels.scale)) / pixels.page.width);
        const py = Math.round((image.height * (SQUARE_CENTRE.y * pixels.scale)) / pixels.page.height);
        if (!image.rgb(px, py).every((v) => v < 60)) throw new Error(`the share's picture is not dark where the PDF's square lands: ${j(image.rgb(px, py))}`);
        if (cream.width !== image.width) throw new Error('the two pictures of the paper are not the same size');
      });

      await check('opacité à 50 % : le carré devient gris sur la fenêtre projetée, un nouveau paperKey', async () => {
        const before = withImage;
        await app.evaluate(choose(OPACITY, 50));
        withImage = await paperKeyChanged(before, 'another paperKey with the image at half opacity');
        // `paperReady` may still be the full-opacity picture's for a moment (the window has not had the snapshot yet): the
        // pixels themselves are waited for.
        // The simulated pen keeps writing across the page: a point may be under its ink (black). Most of a grid of points
        // in the square (200 pt wide) are between the strokes.
        let square = null;
        await until(
          async () => {
            const pixels = await drawn(presentation);
            const samples = [];
            for (const dx of [-60, -30, 0, 30, 60]) {
              for (const dy of [-60, -30, 0, 30, 60]) samples.push(await pixels.pdf(SQUARE_CENTRE.x + dx, SQUARE_CENTRE.y + dy));
            }
            const grey = samples.filter((v) => v.every((c) => c > 80 && c < 190));
            square = grey.length > 0 ? grey[0] : samples[12];
            return grey.length >= samples.length / 2;
          },
          'the square to turn grey on the projector',
          30_000,
        ).catch(() => {
          throw new Error(`the square is not grey at half opacity: ${j(square)}`);
        });
        const label = await app.evaluate(`document.querySelector(${j(`${FOND} .pane-value`)}).textContent.trim()`);
        if (!label.startsWith('50')) throw new Error(`the slider says ${j(label)}`);
      });

      await check('la télécommande : avec le menu du modèle au focus, Page Suivante tourne la page (le papier ne change pas), Page Précédente la ramène', async () => {
        await openTab(app, 'page');
        if ((await app.evaluate(focus(TEMPLATE))) !== true) throw new Error('the template select did not take the focus');
        await goesTo(1, 'the first page');
        await app.key('PageDown');
        await goesTo(2, 'Page Down to turn the page with the template select focused');
        const template = await app.evaluate(`document.querySelector(${j(TEMPLATE)}).value`);
        if (template !== 'seyes') throw new Error(`the key changed the template to ${template}`);
        // The new page keeps the paper (template and texture too), not the image.
        const second = await canvasFacts(app);
        if (second.paper !== 'cream' || second.template !== 'seyes' || second.image !== '') throw new Error(`the next page shows ${j(second)}`);
        await presentation.waitFor(`document.querySelector('canvas.live-canvas').dataset.image === ''`, 'the projector to lose the image on the next page');
        // The simulated pen writes on this page too: a page without ink is not kept, and the end of the journey reads its note.
        await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'the simulated pen to write on the second page', 20_000);
        if (!(await app.evaluate(`document.querySelector(${j(TEXTURE)}).checked`))) throw new Error('the next page lost the texture');
        if ((await app.evaluate(`document.querySelector(${j(swatch('cream'))}).getAttribute('aria-pressed')`)) !== 'true') throw new Error('the next page is not cream');
        await app.evaluate(focus(TEMPLATE));
        await app.key('PageUp');
        await goesTo(1, 'Page Up to turn the page back with the template select focused');
        await app.waitFor(`${canvasData('image')} === ${j(PDF_NAME)}`, 'the image back on the first page');
        await presentation.waitFor(`document.querySelector('canvas.live-canvas').dataset.image === ${j(PDF_NAME)}`, 'the projector to show the image again');
      });

      await check('la télécommande aussi avec le curseur d’opacité au focus : la page tourne, l’image garde son opacité', async () => {
        await openTab(app, 'background');
        if ((await app.evaluate(focus(OPACITY))) !== true) throw new Error('the opacity slider did not take the focus');
        await app.key('PageDown');
        await goesTo(2, 'Page Down to turn the page with the slider focused');
        await app.key('PageUp');
        await goesTo(1, 'Page Up to turn the page back');
        await app.waitFor(`document.querySelector(${j(OPACITY)}) !== null`, 'the slider again on the first page');
        const value = await app.evaluate(`document.querySelector(${j(OPACITY)}).value`);
        if (Number(value) !== 50) throw new Error(`the opacity is ${value} after the remote's keys`);
      });

      await check('tableau noir : les pastilles attendent (paper-board), Lignes choisi se voit tout de suite, éteint le crème revient', async () => {
        await openTab(app, 'projection');
        await app.click(`${tabPanel('projection')} input[data-field="board"]`);
        await presentation.waitFor(`document.querySelector('canvas.live-canvas').dataset.paper === 'slate'`, 'the projector to turn slate', 15_000);
        await openTab(app, 'page');
        for (const id of ['white', 'cream', 'kraft', 'slate']) {
          if ((await app.evaluate(isDisabled(swatch(id)))) !== true) throw new Error(`the ${id} swatch is enabled under the blackboard`);
        }
        const board = await app.evaluate(`document.querySelector(${j(`${PAPER} [data-slot="paper-board"]`)})?.textContent?.trim() ?? null`);
        if (board !== l('live.page.board')) throw new Error(`the blackboard sentence is ${j(board)}`);
        if ((await app.evaluate(isDisabled(TEMPLATE))) === true) throw new Error('the template select is disabled under the blackboard');
        await app.evaluate(choose(TEMPLATE, 'lined'));
        await presentation.waitFor(`document.querySelector('canvas.live-canvas').dataset.template === 'lined'`, 'the projector to show the lined template', 15_000);
        if ((await facts()).paper !== 'slate') throw new Error('the page left the slate while the blackboard is on');
        await openTab(app, 'projection');
        await app.click(`${tabPanel('projection')} input[data-field="board"]`);
        await presentation.waitFor(`document.querySelector('canvas.live-canvas').dataset.paper === 'cream'`, 'the cream paper to come back', 15_000);
        await openTab(app, 'page');
        if ((await app.evaluate(isDisabled(swatch('cream')))) === true) throw new Error('the swatches stay disabled once the board is off');
        if ((await app.evaluate(`document.querySelector(${j(`${PAPER} [data-slot="paper-board"]`)}) !== null`)) === true) throw new Error('the blackboard sentence stays');
        const shown = await facts();
        if (shown.template !== 'lined') throw new Error(`the template after the board is ${shown.template}`);
        await openTab(app, 'background');
        const name = await app.evaluate(`document.querySelector(${j(`${FOND} [data-slot="background-image-name"]`)})?.textContent?.trim() ?? null`);
        if (!name?.includes(PDF_NAME)) throw new Error(`the image is no longer on the first page: ${j(name)}`);
      });

      await check('Terminer : la première note a le papier, la description de l’image et un membre background.png, la seconde le papier seul', async () => {
        presentation.close();
        presentation = null;
        stream.close();
        await app.evaluate(`window.__TAURI_INTERNALS__.invoke('close_presentation').then(() => true)`);
        await stopLive(app);
        await sleep(300);
        const added = notesOf(app.library)
          .filter((n) => !notesBefore.includes(n.path))
          .sort((a, b) => a.note.created - b.note.created);
        if (added.length !== 2) throw new Error(`${added.length} new notes after the live session: ${added.map((n) => n.path).join(', ')}`);
        const [first, second] = added;
        const background = first.note.background;
        if (background?.paper !== 'cream' || background?.template !== 'lined' || background?.texture !== true) {
          throw new Error(`the first note's background is ${j(background)}`);
        }
        const image = first.note.backgroundImage;
        if (image?.name !== PDF_NAME || image?.pdfPage !== 1 || Math.abs((image?.opacity ?? 1) - 0.5) > 0.01) {
          throw new Error(`the first note's image is ${j(image)}`);
        }
        const members = [...first.members.keys()];
        if (!members.some((name) => /^background\.(png|jpg)$/.test(name))) throw new Error(`the first note has no background member: ${j(members)}`);
        const next = second.note.background;
        // The second page was made from the first when it was cream Seyès with texture, before the board and the lined template.
        if (next?.paper !== 'cream' || next?.template !== 'seyes' || next?.texture !== true) throw new Error(`the second note's background is ${j(next)}`);
        if (second.note.backgroundImage !== undefined) throw new Error(`the second note has an image: ${j(second.note.backgroundImage)}`);
        const others = [...second.members.keys()].filter((name) => name.startsWith('background.'));
        if (others.length > 0) throw new Error(`the second note has background members: ${j(others)}`);
      });
    },
  },
];
