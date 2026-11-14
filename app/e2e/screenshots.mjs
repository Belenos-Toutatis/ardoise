// The pictures of the README (`docs/images/`), taken from the production executable.
//
// Not part of `cargo xtask e2e`: nothing runs this file but the person who wants the pictures again, after a change of the
// interface:
//
//     node app/e2e/screenshots.mjs <path to ardoise.exe> <output folder> [--only name,name]
//
// A portable copy of the executable is started on the demo library in French (the same harness as the journeys, so the real
// library and the real `ardoise.toml` of this PC are never touched), with the simulated tablets, and the window is driven
// through WebView2's DevTools protocol: real clicks, real keys and real pen events. The viewport is emulated at 1360 x 850, the
// pictures are WebP (a lossy copy of the screen, `Page.captureScreenshot`), and the dialogs are cut out of the window.
//
// What the pictures show is synthetic and made here: the demo library (`--create-demo-library`), notes written with the pen
// events of `shots-handwriting.mjs`, a PDF written below. No native file dialog is opened (they show the folders of the PC):
// the interface lets a script answer them (`window.__ardoiseE2eOpenPath`, `window.__ardoiseE2eSavePath`). The microphone is
// never started. The windows of the app open on this PC for the few minutes the run takes; the presentation window of the
// `projection` picture covers a whole screen for a few seconds.
//
// `--only` runs the named pictures (and what they need before): editor, context-menu, video, library, equation, summary,
// export, pdf, tablets, live, projection. A picture that fails is reported and the others go on.
import { mkdirSync, readFileSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { freePort, sleep, startApp } from './harness.mjs';
import { penStroke } from './journeys-m8-input.mjs';
import { attach, STATUS } from './journeys-m12-resume.mjs';
import { openTab } from './side-panel.mjs';
import { lineTo, widthOf, writeText } from './shots-handwriting.mjs';
import { removeStaleTemps, removeTemp, removeTemps } from './temp-dirs.mjs';

const args = process.argv.slice(2);
const onlyAt = args.indexOf('--only');
const only = onlyAt >= 0 ? args[onlyAt + 1]?.split(',').filter(Boolean) : null;
const positional = args.filter((arg, i) => !arg.startsWith('--') && (onlyAt < 0 || i !== onlyAt + 1));
if (positional.length < 2) {
  console.error('usage: node app/e2e/screenshots.mjs <ardoise.exe> <output folder> [--only name,name]');
  process.exit(2);
}
const exe = resolve(positional[0]);
const outDir = resolve(positional[1]);
mkdirSync(outDir, { recursive: true });

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const langAt = args.indexOf('--lang');
const lang = langAt >= 0 ? args[langAt + 1] : 'fr';
if (!['fr', 'en'].includes(lang)) {
  console.error('--lang: fr or en');
  process.exit(2);
}
const catalog = JSON.parse(readFileSync(join(locales, `${lang}.json`), 'utf8'));
/** A translated text of the catalogue of the language of the pictures (the label of a button, a title). */
const l = (key) => key.split('.').reduce((node, part) => node?.[part], catalog);
/** What the pictures write on the notes, in the language of the interface. */
const T = {
  fr: {
    physics: 'Énergie cinétique : cours',
    title: 'Énergie cinétique',
    definition: ['L’énergie cinétique d’un objet de masse m,', 'qui se déplace à la vitesse v, s’écrit :'],
    bookmark: 'Chapitre 3',
    chemistry: 'Combustion du dihydrogène',
    chemTitle: 'Combustion',
    chemSub: 'du dihydrogène',
    chemText: ['Le dihydrogène brûle dans le dioxygène :', 'il se forme de l’eau.'],
    balance: 'Équation bilan',
    review: 'à revoir',
  },
  en: {
    physics: 'Kinetic energy: lesson',
    title: 'kinetic energy',
    definition: ['The kinetic energy of an object of mass m,', 'moving at speed v, is written:'],
    bookmark: 'Chapter 3',
    chemistry: 'Hydrogen combustion',
    chemTitle: 'Combustion',
    chemSub: 'of hydrogen',
    chemText: ['Hydrogen burns in oxygen:', 'water is formed.'],
    balance: 'Balanced equation',
    review: 'to revise',
  },
}[lang];
const fill = (template, params) => template.replace(/\{(\w+)\}/g, (match, name) => (name in params ? String(params[name]) : match));
const j = JSON.stringify;

const VIEWPORT = { width: 1360, height: 850 };
const DIALOG = '[role="dialog"]';

// --- Capturing ---------------------------------------------------------------------------------------------------

const written = [];

/** A WebP of the window (or of `clip`, a rectangle in page pixels) saved as `<name>.webp`. */
async function shoot(app, name, { clip, quality = 84 } = {}) {
  const params = { format: 'webp', quality };
  if (clip) params.clip = { x: Math.max(0, Math.floor(clip.x)), y: Math.max(0, Math.floor(clip.y)), width: Math.ceil(clip.width), height: Math.ceil(clip.height), scale: 1 };
  const reply = await app.send('Page.captureScreenshot', params);
  if (!reply.result?.data) throw new Error(`no picture for ${name}: ${j(reply)}`);
  const path = join(outDir, `${name}.webp`);
  writeFileSync(path, Buffer.from(reply.result.data, 'base64'));
  const bytes = statSync(path).size;
  written.push({ name, bytes });
  console.log(`  ${name}.webp  ${(bytes / 1024).toFixed(0)} Ko`);
}

/** The rectangle of an element, grown by `margin` pixels and kept inside the viewport. */
async function rectOf(app, selector, margin = 10) {
  const r = await app.evaluate(`(() => {
    const el = document.querySelector(${j(selector)});
    if (!el) return null;
    const b = el.getBoundingClientRect();
    return { x: b.left, y: b.top, width: b.width, height: b.height, innerWidth, innerHeight };
  })()`);
  if (!r) throw new Error(`no element ${selector}`);
  const x = Math.max(0, r.x - margin);
  const y = Math.max(0, r.y - margin);
  return { x, y, width: Math.min(r.innerWidth - x, r.width + 2 * margin), height: Math.min(r.innerHeight - y, r.height + 2 * margin) };
}

/** Runs `action` with a window tall enough for any dialog to show whole (it is cut out afterwards), then puts the usual one back. */
async function tall(app, action) {
  await app.send('Emulation.setDeviceMetricsOverride', { width: VIEWPORT.width, height: 1700, deviceScaleFactor: 1, mobile: false });
  await sleep(600);
  try {
    return await action();
  } finally {
    await app.send('Emulation.setDeviceMetricsOverride', { ...VIEWPORT, deviceScaleFactor: 1, mobile: false });
    await sleep(300);
  }
}

const shootElement = async (app, name, selector, margin) => shoot(app, name, { clip: await rectOf(app, selector, margin) });

// --- Driving the window -------------------------------------------------------------------------------------------

const invoke = (app, command, params = {}) => app.evaluate(`window.__TAURI_INTERNALS__.invoke(${j(command)}, ${j(params)})`);

/** The page point (x, y), in the page's own units, as a window point (the editor's canvas tells its view). */
const pagePoint = (app, x, y) =>
  app.evaluate(`(() => {
    const svg = document.querySelector('.editor svg.canvas');
    const [scale, vx, vy] = svg.dataset.view.split(' ').map(Number);
    const r = svg.getBoundingClientRect();
    return { x: r.left + ${x} * scale + vx, y: r.top + ${y} * scale + vy };
  })()`);

async function clickPage(app, x, y, options = {}) {
  const at = await pagePoint(app, x, y);
  await app.mouse('mouseMoved', at.x, at.y);
  await app.mouse('mousePressed', at.x, at.y, options);
  await app.mouse('mouseReleased', at.x, at.y, options);
  return at;
}

/** A pen stroke whose pressure rises and falls like a hand's. */
const bell = (i, n) => 0.22 + 0.5 * Math.sin((Math.PI * (i + 0.5)) / n) ** 0.7;

/** Writes strokes given in page units with the pen. */
async function inkPage(app, strokes, force = bell) {
  for (const stroke of strokes) {
    const points = [];
    for (const p of stroke) points.push(await pagePoint(app, p.x, p.y));
    await penStroke(app, points, { force });
  }
}

/** A drag of the mouse from a page point to another (a shape). */
async function dragPage(app, from, to, steps = 14) {
  const a = await pagePoint(app, from.x, from.y);
  const b = await pagePoint(app, to.x, to.y);
  await app.drag(Array.from({ length: steps + 1 }, (_, i) => ({ x: a.x + ((b.x - a.x) * i) / steps, y: a.y + ((b.y - a.y) * i) / steps })));
}

const tool = (app, key) => app.click('.toolbar .tool', l(`editor.modes.${key}`));

const swatch = (app, colour) => app.click(`.toolbar button.swatch[aria-label=${j(l(`editor.colors.${colour}`))}]`);

/** A choice in a `<select>` by the text of its option. */
const choose = (app, selector, label) =>
  app.evaluate(`(() => {
    const el = document.querySelector(${j(selector)});
    const option = [...el.options].find((o) => o.textContent.trim() === ${j(label)});
    if (!option) throw new Error('no option ' + ${j(label)});
    el.value = option.value;
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return true;
  })()`);

/** The card of a note (its file name) is in the grid; clicks it open and waits for the editor. */
async function openCard(app, fileName) {
  const selector = `.note-card[data-path*=${j(fileName)}]`;
  await app.waitFor(`document.querySelector(${j(selector)}) !== null`, `the card of ${fileName}`, 30_000);
  await app.click(`${selector} .card-open`);
  await app.waitFor(`document.querySelector('.editor svg.canvas') !== null`, 'the editor');
  await sleep(600);
}

/** Back to the library from the editor (the note is saved first). */
async function backToLibrary(app) {
  await app.click('.editor-top button');
  await app.waitFor(`document.querySelector('.shell:not(.shell-hidden) input.search') !== null`, 'the library', 30_000);
  await sleep(400);
}

async function save(app) {
  await app.key('s', { ctrl: true });
  await app.waitFor(`document.querySelector('.editor')?.dataset.dirty === 'no'`, 'the note saved');
}

/** A note of the library, made through the app's own command; with bookmarks (one under the other) when asked. */
async function createNote(app, binder, title, bookmarks = []) {
  const made = await invoke(app, 'create_note', { binder, title });
  if (bookmarks.length > 0) await putBookmarks(app, made.path, bookmarks);
  return made;
}

/** Writes bookmarks into a note the way the editor saves them (the label is all a summary needs). */
async function putBookmarks(app, path, labels) {
  await app.evaluate(`(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const opened = await invoke('open_note', { path: ${j(path)} });
    const note = opened.note;
    note.objects = [...(note.objects ?? []), ...${j(labels)}.map((label, i) => ({ kind: 'bookmark', id: 'bm' + i, layer: 0, t: 0, x: 1000, y: 1000 + i * 2500, scale: 1, quarterTurns: 0, width: 3000, height: 925, label }))];
    await invoke('save_note', { path: ${j(path)}, note, stamp: opened.stamp });
    return true;
  })()`);
}

/** The listing of the library, by title. */
const notesByTitle = async (app) => {
  const listing = await invoke(app, 'library_list');
  return new Map(listing.notes.map((n) => [n.title, n]));
};

const cards = (condition) => `document.querySelectorAll('.note-card').length ${condition}`;

// --- The pictures ---------------------------------------------------------------------------------------------------

/** What the run knows, from one picture to the next. */
const state = {};

/** The notes of the pictures and the two simulated tablets. */
async function prepare(app) {
  await app.waitFor(cards('=== 8'), 'the 8 demo notes');

  // The two tablets, simulated: the Bamboo Slate registered, the Xiaomi blackboard added.
  await app.click(STATUS);
  await app.waitFor(`document.querySelector('.tablet-panel button[data-action="register"]') !== null`, 'the register button');
  await app.click('.tablet-panel button[data-action="register"]');
  await app.waitFor(`document.querySelector('.register-wizard [data-step="warning"]') !== null`, 'the warning step');
  if (wants('register')) {
    await sleep(400);
    await app.mouse('mouseMoved', 5, 5);
    await shootElement(app, 'enregistrer-tablette', DIALOG, 12);
  }
  await app.click('.register-wizard button[data-action="continue"]');
  await app.waitFor(`document.querySelector('.register-wizard [data-step="done"]') !== null`, 'the end of the registration', 60_000);
  await app.click('.register-wizard button[data-action="finish"]');
  await app.waitFor(`document.querySelector('.tablet-status[data-connection="connected"]') !== null`, 'the end of the sync', 60_000);
  await app.click(STATUS).catch(() => {});
  await sleep(300);
  if (!(await app.evaluate(`document.querySelector('.tablet-panel') !== null`))) await app.click(STATUS);
  await app.waitFor(`document.querySelector('.tablet-panel') !== null`, 'the tablet panel');
  await app.click('.tablet-panel [role="tab"][data-tab="tablets"]');
  await app.waitFor(`document.querySelector('.tablet-panel button[data-action="add-blackboard"]') !== null`, 'the add button');
  await app.click('.tablet-panel button[data-action="add-blackboard"]');
  await app.waitFor(`document.querySelector('.add-blackboard [data-step="intro"]') !== null`, 'the first screen');
  await app.click('.add-blackboard button[data-action="search"]');
  await app.waitFor(`document.querySelector('.add-blackboard [data-step="found"]') !== null`, 'the tablet heard', 30_000);
  if (wants('add-xiaomi')) {
    await sleep(400);
    await app.mouse('mouseMoved', 5, 5);
    await shootElement(app, 'ajouter-xiaomi', DIALOG, 12);
  }
  await app.click('.add-blackboard button[data-action="add-blackboard"]');
  await app.waitFor(`document.querySelector('.add-blackboard [data-step="added"]') !== null`, 'the end of the addition', 30_000);
  await app.click('.add-blackboard button[data-action="finish"]');
  await app.waitFor(`document.querySelector('.add-blackboard') === null`, 'the dialog to close');
  // The three pages of the Slate come in as notes, untitled: they are not what the pictures are about.
  await app.waitFor(cards('=== 11'), 'the pages of the tablet', 90_000);
  await sleep(1500);
  const listing = await invoke(app, 'library_list');
  const fromTablet = listing.notes.filter((n) => !n.title && n.created > Date.now() - 900_000);
  if (fromTablet.length !== 3) throw new Error(`${fromTablet.length} untitled new notes, expected 3`);
  const trashed = await invoke(app, 'trash_notes', { paths: fromTablet.map((n) => n.path) });
  for (const { id } of trashed.trashed) await invoke(app, 'trash_purge', { id });

  // The demo notes of the class get bookmarks, for the summary of the binder.
  const byTitle = await notesByTitle(app);
  await putBookmarks(app, byTitle.get('Cours d’optique')?.path ?? byTitle.get("Cours d'optique").path, ['Lentilles minces', 'Image réelle']);
  await putBookmarks(app, byTitle.get('Réaction chimique : combustion').path, [T.balance]);
  await putBookmarks(app, byTitle.get('Électricité – loi d’Ohm')?.path ?? byTitle.get("Électricité – loi d'Ohm").path, ['Loi d’Ohm', 'Montage en série']);

  // The two notes written with the pen.
  state.physics = await createNote(app, 'Classe/3e B', T.physics);
  state.chemistry = await createNote(app, 'Classe/3e B', T.chemistry);
  await app.waitFor(`document.querySelector(${j(`.note-card[data-path*=${j(state.physics.fileName)}]`)}) !== null`, 'the new notes');
}

/** « editor »: a page of a physics lesson: handwriting, a typed text, an equation, shapes, a bookmark, squared paper. */
async function editorPicture(app) {
  await openCard(app, state.physics.fileName);
  await openTab(app, 'page');
  await choose(app, 'select[data-field="template"]', l('editor.templates.grid5mm'));
  await sleep(300);

  // The title, in blue, underlined in red.
  await choose(app, '.toolbar select', l('editor.ink.tools.fountainPen'));
  await swatch(app, 'blue');
  await inkPage(app, writeText(T.title, { x: 2000, y: 3300, size: 105, seed: 3 }));
  await swatch(app, 'red');
  await inkPage(app, [lineTo(1900, 3800, 2000 + widthOf(T.title) * 105 + 200, 3700, 30)]);

  // A typed definition.
  await swatch(app, 'black');
  await tool(app, 'text');
  await clickPage(app, 2000, 5000);
  await app.waitFor(`document.activeElement?.matches('[data-slot="text-field"]') === true`, 'the text field');
  const lines = T.definition;
  for (const [i, line] of lines.entries()) {
    await app.type(i > 0 ? `
${line}` : line);
  }
  await app.key('Escape');
  await sleep(300);

  // The equation.
  await tool(app, 'equation');
  await swatch(app, 'black');
  await app.click(`[aria-label="${l('editor.equation.label')}"] button`, l('editor.text.sizes.large'));
  await clickPage(app, 2400, 7900);
  await app.waitFor(`document.activeElement?.matches('[data-slot="equation-source"]') === true`, 'the equation field');
  await app.type('E_c = \\dfrac{1}{2}\\, m\\, v^2');
  await app.waitFor(`document.querySelector('[data-slot="equation-panel"]')?.dataset.state === 'ready'`, 'the preview', 30_000);
  await app.key('Enter');
  await sleep(400);

  // The sketch: a car (a rectangle, two wheels) and the arrow of its speed.
  await tool(app, 'pen');
  await choose(app, '.toolbar select', l('editor.ink.tools.ballpoint'));
  await swatch(app, 'blue');
  await tool(app, 'rectangle');
  await dragPage(app, { x: 17200, y: 6400 }, { x: 24400, y: 8400 });
  await dragPage(app, { x: 18400, y: 5200 }, { x: 22600, y: 6400 });
  await tool(app, 'ellipse');
  await dragPage(app, { x: 18000, y: 8000 }, { x: 19600, y: 9600 });
  await dragPage(app, { x: 22000, y: 8000 }, { x: 23600, y: 9600 });
  await tool(app, 'line');
  await swatch(app, 'black');
  await dragPage(app, { x: 16600, y: 9900 }, { x: 25200, y: 9900 });
  await tool(app, 'pen');
  await swatch(app, 'red');
  await inkPage(app, [lineTo(18000, 4200, 23400, 4200, 24), [{ x: 22700, y: 3800 }, { x: 23400, y: 4200 }, { x: 22700, y: 4600 }]]);
  await inkPage(app, writeText('v', { x: 20300, y: 3600, size: 70, seed: 5 }));

  // The numbers, in black.
  await swatch(app, 'black');
  await inkPage(app, writeText('m = 2 kg', { x: 2000, y: 12600, size: 70, seed: 11 }));
  await inkPage(app, writeText('v = 10 m/s', { x: 2000, y: 14400, size: 70, seed: 12 }));
  await tool(app, 'equation');
  await clickPage(app, 9500, 13000);
  await app.waitFor(`document.activeElement?.matches('[data-slot="equation-source"]') === true`, 'the second equation field');
  await app.type('E_c = \\dfrac{1}{2} \\times 2 \\times 10^2 = 100\\ \\text{J}');
  await app.waitFor(`document.querySelector('[data-slot="equation-panel"]')?.dataset.state === 'ready'`, 'the second preview', 30_000);
  await app.key('Enter');
  await sleep(400);

  // A bookmark.
  await tool(app, 'bookmark');
  await clickPage(app, 22000, 1700);
  await app.waitFor(`document.activeElement?.matches('[data-slot="bookmark-field"], [data-slot="text-field"]') === true`, 'the bookmark field');
  await app.type(T.bookmark);
  await app.key('Escape');
  await sleep(300);

  // Nothing selected in the picture: a click on the empty page with the Selection tool.
  await tool(app, 'select');
  await clickPage(app, 14000, 17500);
  await tool(app, 'pen');
  await save(app);
  await openTab(app, 'layers');
  await app.mouse('mouseMoved', 5, 5);
  await sleep(600);
  await shoot(app, 'editeur');
}

/** « tablets »: the library with the tablet panel open on its list of tablets (the Slate and the Xiaomi blackboard). */
async function tabletsPicture(app) {
  await ensureTabletPanel(app, 'tablets');
  await sleep(500);
  const panel = await rectOf(app, '.tablet-panel', 0);
  await shoot(app, 'tablettes', { clip: { ...panel, height: Math.min(panel.height, 500) } });
}

/** « tablet-use »: the library with the tablet panel on its first tab (the connected Slate, live mode, sync). */
async function tabletUsePicture(app) {
  await ensureTabletPanel(app, 'use');
  await sleep(600);
  await app.mouse('mouseMoved', 700, 780);
  await shoot(app, 'tablette-utiliser');
}

/** The tablet panel is open on the tab `id` (« use », « settings », « tablets »). */
async function ensureTabletPanel(app, id) {
  if (!(await app.evaluate(`document.querySelector('.tablet-panel') !== null`))) await app.click(STATUS);
  await app.waitFor(`document.querySelector('.tablet-panel') !== null`, 'the tablet panel');
  await app.click(`.tablet-panel [role="tab"][data-tab="${id}"]`);
  await sleep(300);
}

async function closeTabletPanel(app) {
  if (await app.evaluate(`document.querySelector('.tablet-panel') !== null`)) {
    await app.click('.tablet-panel button', l('common.close'));
    await app.waitFor(`document.querySelector('.tablet-panel') === null`, 'the tablet panel to close');
  }
}

/** A real right click at a window point. */
async function rightClickAt(app, at) {
  await app.mouse('mouseMoved', at.x, at.y);
  await app.mouse('mousePressed', at.x, at.y, { button: 'right', buttons: 2 });
  await app.mouse('mouseReleased', at.x, at.y, { button: 'right', buttons: 0 });
}

/** « context-menu »: the menu of the page (right click on a shape with the Selection tool), in the editor still open. */
async function contextMenuPicture(app) {
  await tool(app, 'select');
  const at = await pagePoint(app, 20800, 6400);
  await app.mouse('mouseMoved', at.x, at.y);
  await app.mouse('mousePressed', at.x, at.y);
  await app.mouse('mouseReleased', at.x, at.y);
  await app.waitFor(`document.querySelector('[data-section="selection"]') !== null`, 'the shape selected');
  await rightClickAt(app, at);
  await app.waitFor(`document.querySelector('.menu-list[role="menu"]') !== null`, 'the context menu');
  await sleep(300);
  await shoot(app, 'menu-contextuel', { clip: { x: at.x - 300, y: at.y - 190, width: 600, height: 440 } });
  await app.key('Escape');
  await app.waitFor(`document.querySelector('.menu-list[role="menu"]') === null`, 'the menu to close');
  // Nothing selected for the next pictures.
  await clickPage(app, 14000, 17500);
  await tool(app, 'pen');
}

/** « replay »: the replay half way, then the video export dialog, in the editor still open. */
async function replayPicture(app) {
  await app.click('button[data-action="replay-toggle"]');
  await app.waitFor(`document.querySelector('input[data-action="replay-seek"]') !== null`, 'the replay timeline');
  await app.evaluate(`(() => {
    const el = document.querySelector('input[data-action="replay-seek"]');
    el.value = String(Math.round(Number(el.max) * 0.55));
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    return true;
  })()`);
  await sleep(1200);
  await app.mouse('mouseMoved', 5, 5);
  await shoot(app, 'relecture');
  await app.click('button[data-action="video-export"]');
  await app.waitFor(`document.querySelector(${j(`${DIALOG} h2`)})?.textContent === ${j(l('videoExport.title'))}`, 'the video dialog');
  const estimate = l('videoExport.estimate.time').split('{time}')[0];
  await app.waitFor(`document.querySelector(${j(`${DIALOG} .video-estimate`)})?.textContent.includes(${j(estimate)})`, 'the estimate', 20_000);
  await tall(app, async () => shootElement(app, 'export-video', DIALOG, 12));
  await app.key('Escape');
  await app.waitFor(`document.querySelector(${j(DIALOG)}) === null`, 'the dialog to close');
  await app.click('button[data-action="replay-toggle"]');
  await sleep(300);
  await backToLibrary(app);
}

/** « equation »: the second note, written with the pen, and the equation panel open on a chemical equation with its preview. */
async function equationPicture(app) {
  await openCard(app, state.chemistry.fileName);
  await openTab(app, 'page');
  await choose(app, 'select[data-field="template"]', l('editor.templates.seyes'));
  await sleep(300);
  await choose(app, '.toolbar select', l('editor.ink.tools.fountainPen'));
  await swatch(app, 'black');
  await inkPage(app, writeText(T.chemTitle, { x: 2000, y: 3300, size: 105, seed: 21 }));
  await inkPage(app, writeText(T.chemSub, { x: 2000, y: 5300, size: 80, seed: 22 }));
  await swatch(app, 'red');
  await inkPage(app, [lineTo(1900, 5900, 15500, 5800, 36)]);
  await swatch(app, 'black');
  await tool(app, 'text');
  await clickPage(app, 2000, 7300);
  await app.waitFor(`document.activeElement?.matches('[data-slot="text-field"]') === true`, 'the text field');
  await app.type(T.chemText[0]);
  await app.type(`\n${T.chemText[1]}`);
  await app.key('Escape');
  await tool(app, 'equation');
  await app.click(`[aria-label="${l('editor.equation.label')}"] button`, l('editor.text.sizes.large'));
  await clickPage(app, 2400, 10200);
  await app.waitFor(`document.activeElement?.matches('[data-slot="equation-source"]') === true`, 'the equation field');
  await app.type('\\ce{2H2 + O2 -> 2H2O}');
  await app.waitFor(`document.querySelector('[data-slot="equation-panel"]')?.dataset.state === 'ready'`, 'the preview', 30_000);
  await app.key('Enter');
  await sleep(400);
  await clickPage(app, 14500, 9000);
  await app.waitFor(`document.activeElement?.matches('[data-slot="equation-source"]') === true`, 'the second equation field');
  await app.type('\\ce{Cu^2+ + 2e- <=> Cu}');
  await app.waitFor(`document.querySelector('[data-slot="equation-panel"]')?.dataset.state === 'ready'`, 'the second preview', 30_000);
  await openTab(app, 'layers');
  await app.waitFor(`document.activeElement?.matches('[data-slot="equation-source"]') === true`, 'the field to keep the focus').catch(() => {});
  await sleep(500);
  await shoot(app, 'equation');
  await app.key('Enter');
  await sleep(300);
  await tool(app, 'select');
  await clickPage(app, 14000, 19500);
  await tool(app, 'pen');
  await save(app);
  await backToLibrary(app);
}

/** « library »: the library with its binders, tags and cards; the two notes get tags, one a star. */
async function libraryPicture(app) {
  const tagged = [
    [state.physics.id, ['physique', '3e B', 'énergie'], true],
    [state.chemistry.id, ['chimie', '3e B'], false],
  ];
  for (const [id, tags, favorite] of tagged) {
    const note = (await invoke(app, 'library_list')).notes.find((n) => n.id === id);
    const stamp = await invoke(app, 'set_note_tags', { path: note.path, tags, stamp: note.stamp });
    if (favorite) await invoke(app, 'set_note_favorite', { path: note.path, favorite, stamp });
  }
  await sleep(3000);
  await app.evaluate(`(() => { document.activeElement?.blur(); return true; })()`);
  await app.mouse('mouseMoved', 700, 780);
  await sleep(300);
  await shoot(app, 'bibliotheque');
}

const BINDER = 'Classe';
const binderRow = `.sidebar [data-binder=${j(BINDER)}]`;

/** « summary »: the summary of the binder (sub-binders, notes, bookmarks). */
async function summaryPicture(app) {
  await putBookmarks(app, state.chemistry.path, [T.balance]);
  await sleep(1500);
  await app.click(binderRow);
  await app.waitFor(`document.querySelector('button[data-action="binder-summary"]') !== null`, 'the summary button');
  await app.click('button[data-action="binder-summary"]');
  await app.waitFor(`document.querySelector('[data-dialog="binder-summary"]') !== null`, 'the summary');
  await app.click('input[data-option="summary-notes-title"]');
  await sleep(600);
  await tall(app, async () => shootElement(app, 'sommaire-classeur', DIALOG, 12));
  await app.key('Escape');
  await app.waitFor(`document.querySelector('[data-dialog="binder-summary"]') === null`, 'the summary to close');
}

/** « export »: the export of the binder to a PDF, with the summary page; the PDF is kept for the next picture. */
async function exportPicture(app) {
  state.binderPdf = join(app.dir, 'classe.pdf');
  const at = await app.centre(binderRow);
  await rightClickAt(app, at);
  await app.waitFor(`document.querySelector('.menu-list[role="menu"]') !== null`, 'the binder menu');
  await app.click('.menu-list [role="menuitem"]', l('library.binder.exportPdf'));
  await app.waitFor(`document.querySelector(${j(DIALOG)})?.textContent.includes(${j(fill(l('export.subjectBinder'), { name: BINDER }))})`, 'the export dialog');
  await app.click('input[data-option="summary-page"]');
  await app.mouse('mouseMoved', 5, 5);
  await sleep(500);
  await tall(app, async () => shootElement(app, 'export-classeur', DIALOG, 12));
  await app.evaluate(`window.__ardoiseE2eSavePath = ${j(state.binderPdf)}; true`);
  await app.click(`${DIALOG} button`, l('export.run'));
  await app.waitFor(`document.querySelector('${DIALOG} .export-status')?.textContent.includes(${j(fill(l('export.done'), { count: 1 }))})`, 'the export done', 60_000);
  await app.key('Escape');
  await app.waitFor(`document.querySelector(${j(DIALOG)}) === null`, 'the dialog to close');
}

/** A PDF of three A4 pages written by hand (standard fonts, no embedding): a worksheet of exercises. */
function worksheetPdf() {
  const esc = (line) => line.replace(/[\\()]/g, (c) => `\\${c}`).replace(/[^\x20-\x7e]/g, (c) => `\\${c.charCodeAt(0).toString(8).padStart(3, '0')}`);
  const text = (x, y, size, bold, line) => `BT /${bold ? 'F2' : 'F1'} ${size} Tf ${x} ${y} Td (${esc(line)}) Tj ET\n`;
  const pages = [
    [
      text(60, 770, 12, false, 'Classe de 3e B'),
      text(60, 730, 22, true, "Fiche d'exercices : les circuits"),
      text(60, 680, 14, true, 'Exercice 1'),
      text(60, 655, 12, false, 'Un circuit en série comporte une pile, une lampe et un interrupteur.'),
      text(60, 635, 12, false, 'Dessiner le schéma du circuit et indiquer le sens du courant.'),
      '1.2 w 140 380 m 140 560 l 400 560 l 400 380 l 140 380 l S\n1.2 w 250 548 40 24 re S\n1.2 w 150 440 m 150 480 l S\n',
      text(60, 330, 14, true, 'Exercice 2'),
      text(60, 305, 12, false, "Calculer l'intensité du courant qui traverse une résistance de 100 ohms"),
      text(60, 285, 12, false, 'soumise à une tension de 4,5 V.'),
    ],
    [
      text(60, 730, 22, true, "Fiche d'exercices : la loi d'Ohm"),
      text(60, 680, 14, true, 'Exercice 3'),
      text(60, 655, 12, false, 'Compléter le tableau avec les mesures du professeur.'),
      '1 w 60 560 m 460 560 l S 60 520 m 460 520 l S 60 480 m 460 480 l S 60 440 m 460 440 l S 60 560 m 60 440 l S 160 560 m 160 440 l S 260 560 m 260 440 l S 360 560 m 360 440 l S 460 560 m 460 440 l S\n',
      text(80, 534, 12, true, 'U (V)'),
      text(180, 534, 12, true, 'I (A)'),
      text(280, 534, 12, true, 'R (ohm)'),
      text(80, 494, 12, false, '4,5'),
      text(180, 494, 12, false, '0,045'),
      text(80, 454, 12, false, '9,0'),
      text(180, 454, 12, false, '0,090'),
    ],
    [
      text(60, 730, 22, true, "Fiche d'exercices : l'énergie"),
      text(60, 680, 14, true, 'Exercice 5'),
      text(60, 655, 12, false, 'Une voiture de 1 000 kg roule à 20 m/s.'),
      text(60, 635, 12, false, "Calculer son énergie cinétique, puis celle qu'elle aurait à 40 m/s."),
      '1.2 w 100 450 m 100 520 l S 100 520 m 300 520 l S\n',
    ],
  ];
  const objects = [
    '<< /Type /Catalog /Pages 2 0 R >>',
    `<< /Type /Pages /Kids [${pages.map((_, i) => `${5 + i * 2} 0 R`).join(' ')}] /Count ${pages.length} >>`,
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>',
    '<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>',
  ];
  for (const [i, content] of pages.entries()) {
    const body = content.join('');
    objects.push(`<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> /Contents ${6 + i * 2} 0 R >>`);
    objects.push(`<< /Length ${body.length} >>\nstream\n${body}endstream`);
  }
  let pdf = '%PDF-1.4\n';
  const offsets = objects.map((body, i) => {
    const offset = pdf.length;
    pdf += `${i + 1} 0 obj\n${body}\nendobj\n`;
    return offset;
  });
  const xref = pdf.length;
  pdf += `xref\n0 ${objects.length + 1}\n0000000000 65535 f \n`;
  pdf += offsets.map((o) => `${String(o).padStart(10, '0')} 00000 n \n`).join('');
  pdf += `trailer\n<< /Size ${objects.length + 1} /Root 1 0 R >>\nstartxref\n${xref}\n%%EOF\n`;
  // The strings above hold accented letters as octal escapes (WinAnsi = Latin-1 here), the rest is ASCII.
  return Buffer.from(pdf, 'latin1');
}

/** « pdf »: « Depuis un PDF ou une image » with the thumbnails of a worksheet, then the summary page of the binder's PDF as a note. */
async function pdfPicture(app) {
  const sheet = join(app.dir, 'fiche-exercices.pdf');
  writeFileSync(sheet, worksheetPdf());
  const boxes = `${DIALOG} .pdf-strip [role="checkbox"]`;
  await app.evaluate(`window.__ardoiseE2eOpenPath = ${j(sheet)}; true`);
  await app.click('button[data-action="new-from-file"]');
  await app.waitFor(`document.querySelectorAll(${j(boxes)}).length === 3`, 'three thumbnails');
  await app.waitFor(`document.querySelectorAll(${j(`${DIALOG} .pdf-strip img`)}).length === 3`, 'the three pictures', 30_000);
  await app.click(`${boxes}[data-page="3"]`);
  await app.mouse('mouseMoved', 5, 5);
  await sleep(500);
  await shootElement(app, 'depuis-pdf', DIALOG, 12);
  await app.click(`${DIALOG} button`, l('library.cancel'));
  await app.waitFor(`document.querySelector(${j(DIALOG)}) === null`, 'the dialog to close');

  // The binder's PDF: its first page is the summary; imported as a note, it shows what the summary page looks like.
  await app.evaluate(`window.__ardoiseE2eOpenPath = ${j(state.binderPdf)}; true`);
  await app.click('button[data-action="new-from-file"]');
  await app.waitFor(`document.querySelectorAll(${j(boxes)}).length >= 2`, 'the thumbnails of the binder PDF');
  await app.click(`${DIALOG} button[data-action="pages-none"]`);
  await app.click(`${boxes}[data-page="1"]`);
  await app.click(`${DIALOG} .actions button.primary`);
  await app.waitFor(`document.querySelector(${j(DIALOG)}) === null`, 'the dialog to close', 30_000);
  await app.waitFor(`[...document.querySelectorAll('.note-card .card-title')].some((e) => /classe - [^ ]+ 1/.test(e.textContent))`, 'the summary note', 30_000);
  const file = await app.evaluate(`[...document.querySelectorAll('.note-card')].find((c) => /classe - [^ ]+ 1/.test(c.querySelector('.card-title').textContent)).dataset.path`);
  await openCard(app, file.split('\\').pop());
  await sleep(2500);
  const stage = await rectOf(app, '.editor svg.canvas', 0);
  await shoot(app, 'page-sommaire', { clip: { ...stage, height: Math.min(stage.height, 470) } });
  await backToLibrary(app);
}

/** A window point of the live canvas from a page fraction (the page fills the canvas: landscape, 29 700 x 21 600). */
async function livePoint(app, fx, fy) {
  return app.evaluate(`(() => {
    const r = document.querySelector('canvas.live-canvas').getBoundingClientRect();
    return { x: r.left + r.width * ${fx}, y: r.top + r.height * ${fy} };
  })()`);
}

/** « live »: live mode with the simulated Slate writing, and a correction written by the teacher with the computer's pen. */
async function livePicture(app) {
  await ensureTabletPanel(app, 'use');
  await app.waitFor(`document.querySelector('.tablet-panel button[data-action="live"]:not([disabled])') !== null`, 'the live button');
  await app.click('.tablet-panel button[data-action="live"]');
  await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
  await app.waitFor(`Number(document.querySelector('canvas.live-canvas')?.dataset.strokes) > 0`, 'strokes on the live canvas', 15_000);
  // A blackboard: slate paper and white chalk for the tablet, the page cleared of what it wrote before.
  await openTab(app, 'page');
  await app.click('[data-section="live-paper"] button[data-paper="slate"]');
  await app.click(`button[aria-label=${j(l('live.colors.white'))}]`);
  await choose(app, '.live-view select', l('editor.ink.tools.chalk'));
  await app.click('button', l('live.clear'));
  // The simulated tablet writes by itself; the teacher adds a layer of his own and writes on it.
  await sleep(14_000);
  await openTab(app, 'layers');
  const zone = 'section.layers-zone[data-where="live"]';
  await app.click(`${zone} button[data-action="add-layer"]`);
  await app.waitFor(`document.querySelectorAll(${j(`${zone} li.layer`)}).length === 2`, 'a second layer');
  await app.click(`${zone} li.layer[data-layer="1"] [data-action="rename-layer"]`);
  await app.waitFor(`document.activeElement?.matches(${j(`${zone} li.layer[data-layer="1"] input.layer-rename`)}) === true`, 'the name field');
  await app.evaluate(`(() => { document.activeElement.select(); return true; })()`);
  await app.type('Correction');
  await app.key('Enter');
  await app.waitFor(`document.querySelector(${j(`${zone} input.layer-rename`)}) === null`, 'the name field to close');
  await app.evaluate(`(() => { document.activeElement?.blur(); return true; })()`);
  await sleep(300);
  await app.key('p');
  await app.waitFor(`document.querySelector('[data-slot="touch-up"] button[data-action="pen"]')?.getAttribute('aria-pressed') === 'true'`, 'the pen tool');
  const full = await livePoint(app, 1, 1);
  const zero = await livePoint(app, 0, 0);
  const toWindow = (p) => ({ x: zero.x + (p.x / 29700) * (full.x - zero.x), y: zero.y + (p.y / 21600) * (full.y - zero.y) });
  for (const stroke of writeText(T.review, { x: 5200, y: 14200, size: 120, seed: 31 })) await penStroke(app, stroke.map(toWindow), { force: bell });
  await penStroke(app, lineTo(5000, 15000, 15000, 14900, 30).map(toWindow), { force: bell });
  await sleep(1500);
  await app.mouse('mouseMoved', 5, 5);
  await shoot(app, 'direct');
}

/** « projection »: what the projector shows, with the blackboard on (the window of the presentation, read from its canvas). */
async function projectionPicture(app) {
  await invoke(app, 'open_presentation', { monitor: null });
  const presentation = await attach(app.port, (target) => target.url.includes('view=presentation'), 'the presentation window');
  try {
    await presentation.waitFor(`document.querySelector('canvas.live-canvas') !== null && Number(document.querySelector('canvas.live-canvas').dataset.strokes) > 0`, 'the page on the projector', 30_000);
    await sleep(2500);
    // The canvas, scaled down to 1280 pixels wide, as a WebP.
    const data = await presentation.evaluate(`(() => {
      const source = document.querySelector('canvas.live-canvas');
      const width = Math.min(1280, source.width);
      const copy = document.createElement('canvas');
      copy.width = width;
      copy.height = Math.round((source.height * width) / source.width);
      copy.getContext('2d').drawImage(source, 0, 0, copy.width, copy.height);
      return copy.toDataURL('image/webp', 0.82).split(',')[1];
    })()`);
    const path = join(outDir, 'projection.webp');
    writeFileSync(path, Buffer.from(data, 'base64'));
    written.push({ name: 'projection', bytes: statSync(path).size });
    console.log(`  projection.webp  ${(statSync(path).size / 1024).toFixed(0)} Ko`);
  } finally {
    presentation.close();
    await invoke(app, 'close_presentation').catch(() => {});
  }
  await app.click('button[data-action="stop-live"]');
  await app.waitFor(`document.querySelector('.shell:not(.shell-hidden) input.search') !== null`, 'the library', 30_000);
}

/** « editor-panels »: the toolbars, the Page tab, the export and print dialogs, in the editor still open. */
async function editorPanelsPicture(app) {
  await tool(app, 'rectangle');
  await sleep(300);
  await app.mouse('mouseMoved', 5, 5);
  await shoot(app, 'outils-formes', { clip: await rectOf(app, '.toolbar', 0) });
  await tool(app, 'text');
  await sleep(300);
  await shoot(app, 'outils-texte', { clip: await rectOf(app, '.toolbar', 0) });
  await tool(app, 'pen');
  await openTab(app, 'page');
  await sleep(400);
  await shootElement(app, 'panneau-page', '[data-side-panel]', 0);
  await openTab(app, 'layers');

  await app.click('.toolbar button', l('editor.export.button'));
  await app.waitFor(`document.querySelector(${j(`${DIALOG} .format-card`)}) !== null`, 'the export dialog');
  await app.mouse('mouseMoved', 5, 5);
  await tall(app, async () => shootElement(app, 'exporter-note', DIALOG, 12));
  await app.key('Escape');
  await app.waitFor(`document.querySelector(${j(DIALOG)}) === null`, 'the dialog to close');

  await app.click('.toolbar button', l('editor.print.button'));
  await app.waitFor(`document.querySelector('[data-dialog="print"]') !== null`, 'the print dialog');
  await app.mouse('mouseMoved', 5, 5);
  await sleep(500);
  await tall(app, async () => shootElement(app, 'imprimer', DIALOG, 12));
  await app.click(`${DIALOG} button`, l('library.cancel'));
  await app.waitFor(`document.querySelector(${j(DIALOG)}) === null`, 'the dialog to close');
}

/** « library-views »: several notes selected, a search, the menu of a card. */
async function libraryViewsPicture(app) {
  await app.evaluate(`(() => { document.activeElement?.blur(); return true; })()`);
  await app.key('a', { ctrl: true });
  await app.waitFor(`document.querySelector('.note-card[aria-selected="true"], .note-card.selected, .note-card [aria-checked="true"]') !== null`, 'the cards selected').catch(() => {});
  await sleep(600);
  await app.mouse('mouseMoved', 700, 800);
  await shoot(app, 'selection');
  await app.key('Escape');
  await sleep(300);

  await app.click('input.search');
  await app.type(lang === 'fr' ? 'énergie' : 'energy');
  await sleep(1500);
  await app.mouse('mouseMoved', 700, 800);
  await shoot(app, 'recherche');
  await app.evaluate(`(() => { const el = document.querySelector('input.search'); el.value = ''; el.dispatchEvent(new Event('input', { bubbles: true })); el.blur(); return true; })()`);
  await sleep(800);

  await app.click('.note-card .card-menu button[aria-haspopup="menu"]');
  await app.waitFor(`document.querySelector('.menu-list[role="menu"]') !== null`, 'the card menu');
  await sleep(300);
  const menu = await rectOf(app, '.menu-list[role="menu"]', 0);
  await shoot(app, 'menu-carte', { clip: { x: Math.max(0, menu.x - 260), y: Math.max(0, menu.y - 230), width: menu.width + 300, height: menu.height + 260 } });
  await app.key('Escape');
}

/** « diagnostic »: the guided test of the tablet (its first screen, the test is not started). */
async function diagnosticPicture(app) {
  await app.click('button', l('diagnostic.open'));
  await app.waitFor(`document.querySelector('aside.panel.wide') !== null`, 'the test');
  await sleep(800);
  await app.mouse('mouseMoved', 5, 5);
  await shootElement(app, 'tester-tablette', 'aside.panel.wide', 0);
  await app.click('aside.panel.wide button', l('common.close'));
}

/**
 * The pictures, in the order they are taken. `needs`: the pictures that must be taken before (the editor stays open from
 * `editor` to `replay`, and the notes of the library are written by `editor` and `equation`); `--only` adds them.
 */
/** « first-launch »: what a PC that has no library yet shows (a window of its own: the other pictures have a library). */
async function firstLaunchPicture() {
  console.log('first-launch');
  const fresh = await startApp(exe, lang, port + 50, [], { library: false });
  try {
    await fresh.send('Emulation.setDeviceMetricsOverride', { ...VIEWPORT, deviceScaleFactor: 1, mobile: false });
    await sleep(2500);
    // The screen names this PC and says how many Inkspace notes it found (the real ones of the PC that takes the
    // pictures): the name is replaced and the Inkspace box left out, so that nothing personal is on the picture.
    await fresh.evaluate(`(() => {
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      for (let node = walker.nextNode(); node; node = walker.nextNode()) node.textContent = node.textContent.replace(/\\(([A-Z0-9-]+)\\)/, '(MON-PC)');
      document.querySelectorAll('input[type="checkbox"]').forEach((box) => {
        let el = box;
        while (el.parentElement && !el.parentElement.querySelector('button')) el = el.parentElement;
        el.remove();
      });
      return true;
    })()`);
    await sleep(300);
    await shoot(fresh, 'premier-lancement', { clip: { x: 380, y: 120, width: 600, height: 480 } });
  } finally {
    await fresh.close();
    removeTemp(fresh.dir);
  }
}

const pictures = [
  { name: 'editor', run: editorPicture },
  { name: 'context-menu', run: contextMenuPicture, needs: ['editor'], stay: true },
  { name: 'editor-panels', run: editorPanelsPicture, needs: ['editor'], stay: true },
  { name: 'replay', run: replayPicture, needs: ['editor'], stay: true },
  { name: 'equation', run: equationPicture },
  { name: 'library', run: libraryPicture, needs: ['editor', 'equation'] },
  { name: 'library-views', run: libraryViewsPicture, needs: ['library'] },
  { name: 'summary', run: summaryPicture, needs: ['equation'] },
  { name: 'export', run: exportPicture },
  { name: 'pdf', run: pdfPicture, needs: ['export'] },
  { name: 'tablets', run: tabletsPicture, needs: ['library'] },
  { name: 'tablet-use', run: tabletUsePicture, needs: ['library'] },
  { name: 'diagnostic', run: diagnosticPicture, needs: ['library'] },
  { name: 'live', run: livePicture, stay: true },
  { name: 'projection', run: projectionPicture, needs: ['live'], stay: true },
];
const wants = (name) => !only || only.includes(name);
const needed = new Set();
const addWithNeeds = (name) => {
  if (needed.has(name)) return;
  needed.add(name);
  for (const dependency of pictures.find((p) => p.name === name)?.needs ?? []) addWithNeeds(dependency);
};
for (const picture of pictures) if (!only || only.includes(picture.name)) addWithNeeds(picture.name);


// --- The run --------------------------------------------------------------------------------------------------------

removeStaleTemps();
let failures = 0;
const port = Number(process.env.ARDOISE_E2E_PORT) || 9400 + 100 * Math.floor(Math.random() * 40);
const app = await startApp(exe, lang, port, ['--simulated-tablet', '--simulated-blackboard'], {
  settings: `[share]\nport = ${await freePort()}\n`,
});
try {
  await app.send('Emulation.setDeviceMetricsOverride', { ...VIEWPORT, deviceScaleFactor: 1, mobile: false });
  if (wants('first-launch')) await firstLaunchPicture();
  console.log('préparation');
  try {
    await prepare(app);
  } catch (error) {
    const reply = await app.send('Page.captureScreenshot', { format: 'png' });
    if (reply.result?.data) writeFileSync(join(outDir, 'echec-preparation.png'), Buffer.from(reply.result.data, 'base64'));
    throw error;
  }
  for (const picture of pictures) {
    if (!needed.has(picture.name)) continue;
    console.log(picture.name);
    try {
      if (picture.name !== 'tablets') await closeTabletPanel(app);
      if (!picture.stay && picture.name !== 'editor' && (await app.evaluate(`document.querySelector('.editor') !== null`))) await backToLibrary(app);
      await picture.run(app);
    } catch (error) {
      failures++;
      console.log(`  ÉCHEC : ${error.message}`);
      const reply = await app.send('Page.captureScreenshot', { format: 'png' });
      if (reply.result?.data) writeFileSync(join(outDir, `echec-${picture.name}.png`), Buffer.from(reply.result.data, 'base64'));
    }
  }
} finally {
  await app.close();
  removeTemp(app.dir);
  await removeTemps();
}
const total = written.reduce((sum, w) => sum + w.bytes, 0);
console.log(`\n${written.length} images, ${(total / 1024).toFixed(0)} Ko au total${failures ? `, ${failures} échec(s)` : ''}.`);
process.exit(failures === 0 ? 0 : 1);
