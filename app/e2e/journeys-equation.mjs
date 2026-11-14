// Journeys of the Equation tool (SPEC §4.4 « Équation », docs/questions.md Q20 item 7; design: docs/design/equations.md): an
// equation is written in LaTeX in the panel opened by a click on the page, with real key events, its preview drawn by the
// real MathJax (loaded on first use, from the app's own files), put down with Enter, saved (format 1.5, the source and the
// drawing), exported in SVG (its source as the path's title) and PDF (vector curves), found by the library's search; an
// invalid code shows MathJax's error and cannot be put down, Escape drops it, a double click reopens the source and
// « Mettre à jour » redraws it in place, Ctrl + Z undoes that.
//
// On the production executable and the demo library, in French and in English. The canvas is reached through
// `.editor svg.canvas` (the toolbar's icons are SVG too); a page point is turned into a window point with the canvas's
// `data-view` (`screen = page × scale + (x, y)`).
//
// Not driven here: the on-screen keyboard of a touch screen and a finger's tap (component tests, `Editor.equation.test.ts`,
// and a manual check on a touch PC); the drawing in the raster and video exports (Rust tests:
// `crates/ardoise-render/tests/equations.rs`, `crates/ardoise-media/src/replay/tests.rs`).
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { inflateSync } from 'node:zlib';
import { noteOnDisk } from './m17-helpers.mjs';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const fill = (template, params) => template.replace(/\{(\w+)\}/g, (match, name) => (name in params ? String(params[name]) : match));
const j = JSON.stringify;

/** A demo note with room on its page. */
const NOTE = 'Idées pour Noël';
const EQUATIONS = '.editor svg.canvas path[data-kind="equation"]';
const PANEL = '[data-slot="equation-panel"]';
const FIELD = '[data-slot="equation-source"]';
const PREVIEW = '[data-slot="equation-preview"] path';
const STATUS = '[data-slot="equation-status"]';
/** What the teacher types: a reaction, and a fraction (no other journey searches for « H2O »). */
const REACTION = '\\ce{2H2 + O2 -> 2H2O}';
const SPEED = 'v = \\dfrac{d}{t}';
const SPEED_AGAIN = 'v = \\dfrac{\\Delta x}{\\Delta t}';
/** Where the equation goes on the page (device units): below the demo's frames. */
const AT = { x: 9000, y: 15000 };
/** The stored drawing's grammar (`docs/file-format.md` §2.3). */
const PATH_DATA = /^(?:[MLQCZ]|[-\d. ,])+$/;

async function openNote(app, title) {
  const found = await app.evaluate(`(() => {
    const card = [...document.querySelectorAll('.note-card')].find((c) => c.querySelector('.card-title')?.textContent.trim() === ${j(title)});
    if (!card) return false;
    card.dataset.e2e = 'equation-note';
    return true;
  })()`);
  if (!found) throw new Error(`no card "${title}"`);
  await app.click('[data-e2e="equation-note"] .card-open');
  await app.waitFor(`document.querySelector('.editor svg.canvas') !== null`, 'the editor');
}

/** The window point of the page point (x, y), from the canvas's view. */
async function pagePoint(app, x, y) {
  return app.evaluate(`(() => {
    const svg = document.querySelector('.editor svg.canvas');
    const [scale, vx, vy] = svg.dataset.view.split(' ').map(Number);
    const r = svg.getBoundingClientRect();
    return { x: r.left + ${x} * scale + vx, y: r.top + ${y} * scale + vy };
  })()`);
}

async function clickPage(app, x, y) {
  const at = await pagePoint(app, x, y);
  await app.mouse('mouseMoved', at.x, at.y);
  await app.mouse('mousePressed', at.x, at.y);
  await app.mouse('mouseReleased', at.x, at.y);
}

const onDisk = async (app) => noteOnDisk(await app.evaluate(`document.querySelector('.editor').dataset.path`));

async function save(app) {
  await app.key('s', { ctrl: true });
  await app.waitFor(`document.querySelector('.editor')?.dataset.dirty === 'no'`, 'the note saved');
}

function pdfStreams(bytes) {
  const latin = bytes.toString('latin1');
  const out = [];
  for (const match of latin.matchAll(/stream\r?\n/g)) {
    const start = match.index + match[0].length;
    const end = latin.indexOf('endstream', start);
    if (end < 0) continue;
    const raw = bytes.subarray(start, end);
    try {
      out.push(inflateSync(raw).toString('latin1'));
    } catch {
      out.push(raw.toString('latin1'));
    }
  }
  return out;
}

async function exportAs(app, format, path) {
  const l = (key) => text(app.language, key);
  await app.click('.toolbar button', l('editor.export.button'));
  await app.waitFor(`document.querySelector('[role="dialog"] .format-card') !== null`, 'the export dialog');
  await app.click('[role="dialog"] .format-card .choice-name', l(`export.formats.${format}`));
  await app.waitFor(
    `document.querySelector('[role="dialog"] .format-card.selected .choice-name')?.textContent === ${j(l(`export.formats.${format}`))}`,
    `the ${format} format chosen`,
  );
  await app.evaluate(`window.__ardoiseE2eSavePath = ${j(path)}`);
  await app.click('[role="dialog"] .actions button', l('export.run'));
  await app.waitFor(`document.querySelector('[role="dialog"] .export-result code')?.textContent === ${j(path)}`, `the ${format} export`, 60_000);
  await app.click('[role="dialog"] .actions button', l('common.close'));
  await app.waitFor(`document.querySelector('[role="dialog"]') === null`, 'the export dialog closed');
  if (!existsSync(path)) throw new Error(`${path} was not written`);
}

/** Opens the panel at the page point `at` with the Equation tool, and types `latex` in it with real key events. */
async function writeAt(app, at, latex) {
  const l = (key) => text(app.language, key);
  await app.click('.toolbar .tool', l('editor.modes.equation'));
  await clickPage(app, at.x, at.y);
  await app.waitFor(`document.activeElement?.matches(${j(FIELD)}) === true`, 'the equation field focused');
  await app.type(latex);
  await app.waitFor(`document.querySelector(${j(FIELD)})?.value === ${j(latex)}`, 'the code typed');
}

/** Waits for the preview of what is typed now (MathJax loads the first time). */
const previewed = (app) =>
  app.waitFor(
    `document.querySelector(${j(PANEL)})?.dataset.state === 'ready' && /^M/.test(document.querySelector(${j(PREVIEW)})?.getAttribute('d') ?? '')`,
    'the preview',
    30_000,
  );

export const equationJourneys = [
  {
    name: 'outil Équation : écrire une réaction en LaTeX, l’aperçu, Entrée, enregistrer, l’exporter en SVG et en PDF, la retrouver',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let drawn = '';

      await check(`ouvrir « ${NOTE} », l’outil « ${l('editor.modes.equation')} » : sa taille s’affiche`, async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await openNote(app, NOTE);
        await app.click('.toolbar .tool', l('editor.modes.equation'));
        await app.waitFor(`document.querySelector('[data-slot="equation-options"]') !== null`, 'the equation options');
        const label = await app.evaluate(`document.querySelector('[data-slot="equation-options"]').getAttribute('aria-label')`);
        if (label !== l('editor.equation.label')) throw new Error(`the options are labelled ${j(label)}`);
      });

      await check('un clic sur la page ouvre le panneau, le champ du code a le focus et son nom', async () => {
        await clickPage(app, AT.x, AT.y);
        await app.waitFor(`document.activeElement?.matches(${j(FIELD)}) === true`, 'the field focused');
        const name = await app.evaluate(`document.querySelector('label[for="equation-source"]')?.textContent`);
        if (name !== l('editor.equation.field')) throw new Error(`the field is named ${j(name)}`);
      });

      await check('taper le code (dont la lettre p du raccourci du stylo) : l’aperçu se dessine en tracés, l’outil reste, la page ne change pas', async () => {
        await app.type(REACTION);
        await app.key('p');
        await app.waitFor(`document.querySelector(${j(FIELD)})?.value === ${j(`${REACTION}p`)}`, 'the typed code and p');
        await app.key('Backspace');
        await app.waitFor(`document.querySelector(${j(FIELD)})?.value === ${j(REACTION)}`, 'the p removed');
        await previewed(app);
        drawn = await app.evaluate(`document.querySelector(${j(PREVIEW)}).getAttribute('d')`);
        if (!PATH_DATA.test(drawn) || !/C/.test(drawn)) throw new Error(`the preview is not our path data: ${drawn.slice(0, 80)}`);
        const pressed = await app.evaluate(`document.querySelector('.toolbar .tool[aria-pressed="true"]')?.title`);
        if (pressed !== l('editor.modes.equation')) throw new Error(`the tool became ${j(pressed)}`);
        if ((await app.evaluate(`document.querySelectorAll(${j(EQUATIONS)}).length`)) !== 0) throw new Error('drawn on the page before it was put down');
        if ((await app.evaluate(`document.querySelector('.editor').dataset.dirty`)) !== 'no') throw new Error('the note changed');
      });

      await check('Entrée pose l’équation : sur la page, sélectionnée, le panneau fermé', async () => {
        await app.key('Enter');
        await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the panel closed');
        await app.waitFor(`document.querySelectorAll(${j(EQUATIONS)}).length === 1`, 'the equation on the page');
        const d = await app.evaluate(`document.querySelector(${j(EQUATIONS)}).getAttribute('d')`);
        if (d !== drawn) throw new Error('the page does not draw the preview');
        const count = await app.evaluate(`document.querySelector('[data-section="selection"] .pane-help')?.textContent.trim()`);
        const expected = fill(l('editor.selection.countMixed'), { strokes: 0, texts: 1 });
        if (count !== expected) throw new Error(`the selection says ${j(count)}, expected ${j(expected)}`);
      });

      await check('Ctrl + S : le fichier (format 1.5) garde le code, le dessin, la taille et la couleur', async () => {
        await save(app);
        const file = await onDisk(app);
        if (file.note.format.minor < 5) throw new Error(`format ${file.note.format.version}.${file.note.format.minor}`);
        const mine = (file.note.objects ?? []).find((o) => o.kind === 'equation');
        if (!mine) throw new Error('no equation in the file');
        if (mine.latex !== REACTION || mine.path !== drawn || Math.abs(mine.scale - 0.6) > 1e-6) throw new Error(`unexpected equation ${j({ ...mine, path: mine.path.length })}`);
        if (Math.abs(mine.x - AT.x) > 1 || !(mine.width > 0) || !(mine.height > 0)) throw new Error(`misplaced: ${j({ x: mine.x, y: mine.y, w: mine.width, h: mine.height })}`);
      });

      await check('SVG : l’équation est un tracé, son code LaTeX en titre', async () => {
        const svg = join(app.dir, 'equation.svg');
        await exportAs(app, 'svg', svg);
        const content = readFileSync(svg, 'utf8');
        if (!content.includes('<title>\\ce{2H2 + O2 -&gt; 2H2O}</title>')) throw new Error('the source is not the title of a path in the SVG');
        if (!/<path d="M[^"]*C[^"]*"[^>]*><title>\\ce/.test(content)) throw new Error('no curved path for the equation');
      });

      await check('PDF : vectoriel, l’équation en courbes', async () => {
        const pdf = join(app.dir, 'equation.pdf');
        await exportAs(app, 'pdf', pdf);
        const bytes = readFileSync(pdf);
        if (!bytes.toString('latin1').startsWith('%PDF-')) throw new Error('not a PDF');
        if (!/\s-?[\d.]+ c\s/.test(pdfStreams(bytes).join('\n'))) throw new Error('no curve in the PDF');
      });

      await check('retour à la bibliothèque : la recherche trouve la note par le code de l’équation', async () => {
        await app.click('.editor-top button', l('editor.back'));
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the library');
        await app.click('input.search');
        await app.key('a', { ctrl: true });
        await app.key('Backspace');
        await app.type('H2O');
        await app.waitFor(`document.querySelectorAll('.note-card').length === 1`, 'one search result');
        const title = await app.evaluate(`document.querySelector('.note-card .card-title')?.textContent.trim()`);
        if (title !== NOTE) throw new Error(`the search found ${j(title)}`);
        await app.key('Escape');
      });
    },
  },
  {
    name: 'outil Équation : une erreur de LaTeX empêche de poser, Échap abandonne, un double-clic rouvre, Mettre à jour, Ctrl + Z',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let first;

      await check('un code incorrect affiche l’erreur de MathJax et ne se pose pas, même avec Entrée', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await openNote(app, NOTE);
        await writeAt(app, AT, '\\frac{1');
        const expected = fill(l('editor.equation.problems.latex'), { message: 'Missing close brace' });
        await app.waitFor(`document.querySelector(${j(STATUS)})?.textContent.trim() === ${j(expected)}`, 'MathJax\'s error', 30_000);
        const disabled = await app.evaluate(`document.querySelector('[data-action="equation-place"]').disabled`);
        if (!disabled) throw new Error('an invalid equation can be put down');
        await app.key('Enter');
        if ((await app.evaluate(`document.querySelector(${j(PANEL)}) !== null`)) !== true) throw new Error('the panel closed');
        if ((await app.evaluate(`document.querySelectorAll(${j(EQUATIONS)}).length`)) !== 0) throw new Error('put down anyway');
      });

      await check('Échap ferme le panneau sans rien changer', async () => {
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the panel closed');
        if ((await app.evaluate(`document.querySelector('.editor').dataset.dirty`)) !== 'no') throw new Error('the note changed');
      });

      await check('poser une fraction avec le bouton', async () => {
        await writeAt(app, AT, SPEED);
        await previewed(app);
        await app.click('[data-action="equation-place"]');
        await app.waitFor(`document.querySelectorAll(${j(EQUATIONS)}).length === 1`, 'the equation on the page');
        await save(app);
        first = (await onDisk(app)).note.objects.find((o) => o.kind === 'equation');
        if (first?.latex !== SPEED) throw new Error(`unexpected ${j(first?.latex)}`);
      });

      await check('un double-clic avec la sélection rouvre son code ; Mettre à jour la redessine au même endroit', async () => {
        await app.click('.toolbar .tool', l('editor.modes.select'));
        const at = await pagePoint(app, first.x + (first.width * first.scale) / 2, first.y + (first.height * first.scale) / 2);
        await app.mouse('mouseMoved', at.x, at.y);
        await app.mouse('mousePressed', at.x, at.y, { clickCount: 1 });
        await app.mouse('mouseReleased', at.x, at.y, { clickCount: 1 });
        await app.mouse('mousePressed', at.x, at.y, { clickCount: 2 });
        await app.mouse('mouseReleased', at.x, at.y, { clickCount: 2 });
        await app.waitFor(`document.activeElement?.matches(${j(FIELD)}) === true`, 'the field of the equation');
        const value = await app.evaluate(`document.activeElement.value`);
        if (value !== SPEED) throw new Error(`the field holds ${j(value)}`);
        const button = await app.evaluate(`document.querySelector('[data-action="equation-place"]').textContent.trim()`);
        if (button !== l('editor.equation.update')) throw new Error(`the button says ${j(button)}`);
        await app.key('a', { ctrl: true });
        await app.type(SPEED_AGAIN);
        await app.waitFor(`document.querySelector(${j(FIELD)})?.value === ${j(SPEED_AGAIN)}`, 'the new code');
        await previewed(app);
        await app.waitFor(`document.querySelector('[data-action="equation-place"]').disabled === false`, 'the update ready', 30_000);
        await app.click('[data-action="equation-place"]');
        await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the panel closed');
        await save(app);
        const after = (await onDisk(app)).note.objects.find((o) => o.kind === 'equation');
        if (after.id !== first.id || after.latex !== SPEED_AGAIN) throw new Error(`not updated: ${j(after.latex)}`);
        if (Math.abs(after.x - first.x) > 0.01 || Math.abs(after.y - first.y) > 0.01) throw new Error('its top-left corner moved');
      });

      await check('Ctrl + Z : le premier code revient', async () => {
        await app.evaluate(`document.activeElement?.blur(), true`);
        await app.key('z', { ctrl: true });
        await save(app);
        const back = (await onDisk(app)).note.objects.find((o) => o.kind === 'equation');
        if (back.latex !== SPEED) throw new Error(`the source is ${j(back.latex)}`);
      });
    },
  },
];
