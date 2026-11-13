// Journeys of the Text tool (SPEC §4.4 « Texte tapé au clavier », docs/questions.md Q20 item 6; design:
// docs/design/page-objects.md): the demo note « Idées pour Noël » holds three text frames (`app/src-tauri/src/demo.rs`); a
// frame is put on the page with a click and typed in with real key events, ended with Escape, saved, found by the library's
// search, exported in SVG and PDF with its text, moved with the strokes and the move undone.
//
// On the production executable and the demo library, in French and in English. The canvas is reached through
// `.editor svg.canvas` (the toolbar's icons are SVG too); a page point is turned into a window point with the canvas's
// `data-view` (`screen = page × scale + (x, y)`).
//
// Not driven here: the OS's on-screen keyboard of a touch screen (a finger's tap on the page is a component test,
// `Editor.text.test.ts`, and a manual check on a touch PC); the look of the glyphs in the exports (Rust tests:
// `crates/ardoise-render/tests/text_frames.rs`, with a reference image).
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

/** The demo note with text frames, and how many it has. */
const NOTE = 'Idées pour Noël';
const DEMO_FRAMES = 3;
const FRAMES = '.editor svg.canvas path[data-object]';
const FIELD = '[data-slot="text-field"]';
/** A word no other journey searches for, typed in the new frame. */
const WORD = 'Zébrure';

/** Opens the demo note titled `title` from its card. */
async function openNote(app, title) {
  const found = await app.evaluate(`(() => {
    const card = [...document.querySelectorAll('.note-card')].find((c) => c.querySelector('.card-title')?.textContent.trim() === ${j(title)});
    if (!card) return false;
    card.dataset.e2e = 'text-note';
    return true;
  })()`);
  if (!found) throw new Error(`no card "${title}"`);
  await app.click('[data-e2e="text-note"] .card-open');
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

/** A real mouse click at the page point (x, y). */
async function clickPage(app, x, y) {
  const at = await pagePoint(app, x, y);
  await app.mouse('mouseMoved', at.x, at.y);
  await app.mouse('mousePressed', at.x, at.y);
  await app.mouse('mouseReleased', at.x, at.y);
}

/** The note open in the editor, as written on the disk. */
const onDisk = async (app) => noteOnDisk(await app.evaluate(`document.querySelector('.editor').dataset.path`));

/** Saves with Ctrl + S and waits until the editor says it is saved. */
async function save(app) {
  await app.key('s', { ctrl: true });
  await app.waitFor(`document.querySelector('.editor')?.dataset.dirty === 'no'`, 'the note saved');
}

/** The streams of a PDF file, inflated when they are compressed. */
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

/** Opens the export dialog of the editor, chooses `format`, writes the file at `path` and waits for it. */
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

export const textJourneys = [
  {
    name: 'outil Texte : poser un cadre, taper, Échap, enregistrer, le retrouver par la recherche, l’exporter en SVG et en PDF',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const typed = `${WORD} « été » 12 €`;

      await check(`ouvrir « ${NOTE} » : ses ${DEMO_FRAMES} cadres de texte sont dessinés`, async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await openNote(app, NOTE);
        await app.waitFor(`document.querySelectorAll(${j(FRAMES)}).length === ${DEMO_FRAMES}`, 'the demo frames');
        const curves = await app.evaluate(`[...document.querySelectorAll(${j(FRAMES)})].every((p) => /[QC]/.test(p.getAttribute('d')))`);
        if (!curves) throw new Error('a frame is not drawn with the curves of its glyphs');
      });

      await check(`l’outil « ${l('editor.modes.text')} » : ses options (police, taille, gras) s’affichent`, async () => {
        await app.click('.toolbar .tool', l('editor.modes.text'));
        await app.waitFor(`document.querySelector('[data-slot="text-options"]') !== null`, 'the text options');
        const label = await app.evaluate(`document.querySelector('[data-slot="text-options"]').getAttribute('aria-label')`);
        if (label !== l('editor.text.label')) throw new Error(`the options are labelled ${j(label)}`);
      });

      await check('un clic sur la page pose un cadre : le champ de saisie a le focus', async () => {
        await clickPage(app, 9000, 9000);
        await app.waitFor(`document.activeElement?.matches(${j(FIELD)}) === true`, 'the field focused');
        const name = await app.evaluate(`document.activeElement.getAttribute('aria-label')`);
        if (name !== l('editor.text.field')) throw new Error(`the field is named ${j(name)}`);
      });

      await check('taper (accents, guillemets, euro, et la lettre p du raccourci du stylo) : le cadre est dessiné en direct, l’outil reste', async () => {
        await app.type(typed);
        await app.key('p');
        await app.waitFor(`document.querySelector(${j(FIELD)})?.value === ${j(`${typed}p`)}`, 'the typed text');
        await app.key('Backspace');
        await app.waitFor(`document.querySelector(${j(FIELD)})?.value === ${j(typed)}`, 'the p removed');
        await app.waitFor(`document.querySelectorAll(${j(FRAMES)}).length === ${DEMO_FRAMES + 1}`, 'the new frame drawn');
        const pressed = await app.evaluate(`document.querySelector('.toolbar .tool[aria-pressed="true"]')?.title`);
        if (pressed !== l('editor.modes.text')) throw new Error(`the tool became ${j(pressed)}`);
      });

      await check('Échap termine la saisie : le texte reste, le cadre est sélectionné', async () => {
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${j(FIELD)}) === null`, 'the field gone');
        const count = await app.evaluate(`document.querySelector('[data-section="selection"] .pane-help')?.textContent.trim()`);
        const expected = fill(l('editor.selection.countMixed'), { strokes: 0, texts: 1 });
        if (count !== expected) throw new Error(`the selection says ${j(count)}, expected ${j(expected)}`);
        if ((await app.evaluate(`document.querySelectorAll(${j(FRAMES)}).length`)) !== DEMO_FRAMES + 1) throw new Error('the frame went');
      });

      await check('Ctrl + S : le fichier (format 1.4) garde le cadre, sa police, sa taille, son texte', async () => {
        await save(app);
        const file = await onDisk(app);
        if (file.note.format.minor < 4) throw new Error(`format ${file.note.format.version}.${file.note.format.minor}`);
        const frames = file.note.objects ?? [];
        if (frames.length !== DEMO_FRAMES + 1) throw new Error(`${frames.length} objects in the file`);
        const mine = frames.find((o) => o.text === typed);
        if (!mine) throw new Error(`no frame "${typed}" in ${j(frames.map((o) => o.text))}`);
        if (mine.kind !== 'text' || mine.font !== 'latinModern' || mine.size !== 600 || mine.bold !== false || !(mine.width > 0)) {
          throw new Error(`unexpected frame ${j(mine)}`);
        }
      });

      await check(`SVG : le cadre est dans le fichier, en tracés avec son texte en titre`, async () => {
        const svg = join(app.dir, 'texte.svg');
        await exportAs(app, 'svg', svg);
        const content = readFileSync(svg, 'utf8');
        if (!content.includes(`<title>${WORD}`)) throw new Error('the frame\'s text is not in the SVG');
        if (!/<path d="M[^"]*[QC]/.test(content)) throw new Error('no glyph path with curves in the SVG');
      });

      await check('PDF : vectoriel, avec le texte invisible qui rend le cadre cherchable', async () => {
        const pdf = join(app.dir, 'texte.pdf');
        await exportAs(app, 'pdf', pdf);
        const bytes = readFileSync(pdf);
        if (!bytes.toString('latin1').startsWith('%PDF-')) throw new Error('not a PDF');
        const streams = pdfStreams(bytes).join('\n');
        if (!/\b3 Tr\b/.test(streams)) throw new Error('no invisible text (3 Tr) in the PDF');
        if (!/\s-?[\d.]+ c\s/.test(streams)) throw new Error('no curve in the PDF');
      });

      await check('retour à la bibliothèque : la recherche trouve la note par son texte tapé (sans accent)', async () => {
        await app.click('.editor-top button', l('editor.back'));
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the library');
        await app.click('input.search');
        await app.key('a', { ctrl: true });
        await app.key('Backspace');
        await app.type('zebrure');
        await app.waitFor(`document.querySelectorAll('.note-card').length === 1`, 'one search result');
        const title = await app.evaluate(`document.querySelector('.note-card .card-title')?.textContent.trim()`);
        if (title !== NOTE) throw new Error(`the search found ${j(title)}`);
      });
    },
  },
  {
    name: 'outil Texte : un cadre se sélectionne et se déplace avec les traits, Ctrl + Z annule, un double-clic le rouvre',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let before;

      await check(`ouvrir « ${NOTE} », l’outil « ${l('editor.modes.select')} », Ctrl + A : traits et textes sélectionnés`, async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await openNote(app, NOTE);
        await app.waitFor(`document.querySelectorAll(${j(FRAMES)}).length === ${DEMO_FRAMES}`, 'the demo frames');
        before = (await onDisk(app)).note.objects;
        await app.click('.toolbar .tool', l('editor.modes.select'));
        // The focus on the page, not on the tool's button (Ctrl + A selects in a field).
        await app.evaluate(`document.activeElement?.blur(), true`);
        await app.key('a', { ctrl: true });
        await app.waitFor(`document.querySelector('[data-section="selection"]') !== null`, 'the selection');
        const count = await app.evaluate(`document.querySelector('[data-section="selection"] .pane-help')?.textContent.trim()`);
        if (!count?.includes(String(DEMO_FRAMES))) throw new Error(`the selection says ${j(count)}`);
      });

      await check('glisser la sélection : les cadres suivent, enregistrés au nouvel endroit', async () => {
        const frame = before[0];
        const from = await pagePoint(app, frame.x + frame.width / 2, frame.y + frame.height / 2);
        const to = { x: from.x + 40, y: from.y + 30 };
        await app.drag(Array.from({ length: 8 }, (_, i) => ({ x: from.x + ((to.x - from.x) * i) / 7, y: from.y + ((to.y - from.y) * i) / 7 })));
        await save(app);
        const after = (await onDisk(app)).note.objects;
        const moved = after.find((o) => o.id === frame.id);
        if (!moved || !(moved.x > frame.x) || !(moved.y > frame.y)) throw new Error(`the frame did not move: ${j(moved)}`);
      });

      await check('Ctrl + Z : le déplacement est annulé', async () => {
        await app.key('z', { ctrl: true });
        await save(app);
        const after = (await onDisk(app)).note.objects;
        const back = after.find((o) => o.id === before[0].id);
        if (Math.abs(back.x - before[0].x) > 0.01 || Math.abs(back.y - before[0].y) > 0.01) throw new Error(`not back: ${j(back)}`);
      });

      await check('un double-clic sur un cadre le rouvre pour la saisie, avec l’outil Texte', async () => {
        await app.key('Escape');
        const frame = before[0];
        const at = await pagePoint(app, frame.x + 50, frame.y + frame.height / 2);
        await app.mouse('mouseMoved', at.x, at.y);
        await app.mouse('mousePressed', at.x, at.y, { clickCount: 1 });
        await app.mouse('mouseReleased', at.x, at.y, { clickCount: 1 });
        await app.mouse('mousePressed', at.x, at.y, { clickCount: 2 });
        await app.mouse('mouseReleased', at.x, at.y, { clickCount: 2 });
        await app.waitFor(`document.activeElement?.matches(${j(FIELD)}) === true`, 'the field of the frame');
        const value = await app.evaluate(`document.activeElement.value`);
        if (value !== frame.text) throw new Error(`the field holds ${j(value)}`);
        const pressed = await app.evaluate(`document.querySelector('.toolbar .tool[aria-pressed="true"]')?.title`);
        if (pressed !== l('editor.modes.text')) throw new Error(`the tool is ${j(pressed)}`);
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${j(FIELD)}) === null`, 'the field gone');
      });
    },
  },
];
