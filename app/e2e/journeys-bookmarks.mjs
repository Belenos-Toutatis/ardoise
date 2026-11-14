// Journeys of the bookmarks and of the summary of a binder (SPEC §4.4 « Signets », §4.3 « Sommaire du classeur », §4.6;
// docs/questions.md Q20 item 5; design: docs/design/page-objects.md §7): in the demo note « Cours de physique » (binder
// `Classe/3e B`) a bookmark is put on the page with a click and typed with real key events, ended with Enter, saved (format
// 1.6), found by the library's search, absent from the SVG, present in the PDF's outline, listed in the summary of the binder,
// and the note opens at it; then renamed with a double click, removed by emptying its label, and the steps undone.
//
// On the production executable and the demo library, in French and in English. The canvas is reached through
// `.editor svg.canvas`; a page point is turned into a window point with the canvas's `data-view`
// (`screen = page × scale + (x, y)`).
//
// Not driven here: the OS's on-screen keyboard of a touch screen (a finger's tap with the tool is a component test,
// `Editor.bookmarks.test.ts`, and a manual check on a touch PC); the position a PDF viewer scrolls to for an outline entry
// (Rust test `crates/ardoise-render/tests/bookmarks.rs` reads the destinations back; docs/hardware-tests.md for a viewer).
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { noteOnDisk } from './m17-helpers.mjs';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const fill = (template, params) => template.replace(/\{(\w+)\}/g, (match, name) => (name in params ? String(params[name]) : match));
const j = JSON.stringify;

const NOTE = 'Cours de physique';
const BINDER = 'Classe/3e B';
const MARKS = '.editor svg.canvas g.bookmark';
const FIELD = '[data-slot="bookmark-field"]';
/** A word no other journey searches for (ASCII, so that the PDF writes the title as it is). */
const LABEL = 'Zebrure du TP';

/** Opens the demo note titled `title` from its card. */
async function openNote(app, title) {
  const found = await app.evaluate(`(() => {
    const card = [...document.querySelectorAll('.note-card')].find((c) => c.querySelector('.card-title')?.textContent.trim() === ${j(title)});
    if (!card) return false;
    card.dataset.e2e = 'bookmark-note';
    return true;
  })()`);
  if (!found) throw new Error(`no card "${title}"`);
  await app.click('[data-e2e="bookmark-note"] .card-open');
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
async function clickPage(app, x, y, clickCount = 1) {
  const at = await pagePoint(app, x, y);
  await app.mouse('mouseMoved', at.x, at.y);
  await app.mouse('mousePressed', at.x, at.y, { clickCount });
  await app.mouse('mouseReleased', at.x, at.y, { clickCount });
}

/** The note open in the editor, as written on the disk. */
const onDisk = async (app) => noteOnDisk(await app.evaluate(`document.querySelector('.editor').dataset.path`));

/** Saves with Ctrl + S and waits until the editor says it is saved. */
async function save(app) {
  await app.key('s', { ctrl: true });
  await app.waitFor(`document.querySelector('.editor')?.dataset.dirty === 'no'`, 'the note saved');
}

/** The bookmarks on the page, as they read. */
const marks = (app) => app.evaluate(`[...document.querySelectorAll(${j(MARKS)})].map((g) => g.dataset.bookmark)`);

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

/** Picks the bookmark tool, taps the page at (x, y) and types `label` in the field that takes the focus (no step ended yet). */
async function putBookmark(app, x, y, label) {
  const l = (key) => text(app.language, key);
  await app.click('.toolbar .tool', l('editor.modes.bookmark'));
  await clickPage(app, x, y);
  await app.waitFor(`document.activeElement?.matches(${j(FIELD)}) === true`, 'the field focused');
  await app.type(label);
  await app.waitFor(`document.querySelector(${j(FIELD)})?.value === ${j(label)}`, 'the typed label');
}

export const bookmarkJourneys = [
  {
    name: 'signets : poser un signet, l’enregistrer, le retrouver par la recherche et dans le sommaire du classeur, l’ouvrir à son endroit',
    async run(app, check) {
      const l = (key) => text(app.language, key);

      await check(`l’outil « ${l('editor.modes.bookmark')} » existe dans la barre d’outils, avec son aide`, async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await openNote(app, NOTE);
        await app.click('.toolbar .tool', l('editor.modes.bookmark'));
        await app.waitFor(`document.querySelector('[data-slot="bookmark-help"]') !== null`, 'the help of the tool');
        const pressed = await app.evaluate(`document.querySelector('.toolbar .tool[aria-pressed="true"]')?.title`);
        if (pressed !== l('editor.modes.bookmark')) throw new Error(`the tool is ${j(pressed)}`);
      });

      await check('un clic sur la page pose un signet : le champ du libellé a le focus et un nom accessible', async () => {
        await clickPage(app, 20_000, 3_000);
        await app.waitFor(`document.activeElement?.matches(${j(FIELD)}) === true`, 'the field focused');
        const name = await app.evaluate(`document.activeElement.getAttribute('aria-label')`);
        if (name !== l('editor.bookmark.field')) throw new Error(`the field is named ${j(name)}`);
      });

      await check('taper le libellé (et la lettre p, raccourci du stylo) : le signet est dessiné en direct, l’outil reste', async () => {
        await app.type(LABEL);
        await app.key('p');
        await app.waitFor(`document.querySelector(${j(FIELD)})?.value === ${j(`${LABEL}p`)}`, 'the typed label');
        await app.key('Backspace');
        await app.waitFor(`document.querySelector(${j(FIELD)})?.value === ${j(LABEL)}`, 'the p removed');
        await app.waitFor(`document.querySelectorAll(${j(MARKS)}).length === 1`, 'the bookmark drawn');
        if (JSON.stringify(await marks(app)) !== j([LABEL])) throw new Error(`the bookmark says ${j(await marks(app))}`);
        const pressed = await app.evaluate(`document.querySelector('.toolbar .tool[aria-pressed="true"]')?.title`);
        if (pressed !== l('editor.modes.bookmark')) throw new Error(`the tool became ${j(pressed)}`);
      });

      await check('Entrée termine la saisie : le signet reste, sélectionné', async () => {
        await app.key('Enter');
        await app.waitFor(`document.querySelector(${j(FIELD)}) === null`, 'the field gone');
        const count = await app.evaluate(`document.querySelector('[data-section="selection"] .pane-help')?.textContent.trim()`);
        const expected = fill(l('editor.selection.countMixed'), { strokes: 0, texts: 1 });
        if (count !== expected) throw new Error(`the selection says ${j(count)}, expected ${j(expected)}`);
      });

      await check('Ctrl + S : le fichier (format 1.6) garde le signet, son libellé, sa boîte', async () => {
        await save(app);
        const file = await onDisk(app);
        if (file.note.format.minor < 6) throw new Error(`format ${file.note.format.version}.${file.note.format.minor}`);
        const mine = (file.note.objects ?? []).find((o) => o.kind === 'bookmark');
        if (!mine || mine.label !== LABEL || !(mine.width > 0) || !(mine.height > 0)) throw new Error(`unexpected objects ${j(file.note.objects)}`);
      });

      await check('SVG : le signet n’est pas dans l’image (ni son texte, ni un tracé de plus)', async () => {
        const svg = join(app.dir, 'signet.svg');
        await exportAs(app, 'svg', svg);
        const content = readFileSync(svg, 'utf8');
        if (content.includes('Zebrure')) throw new Error('the bookmark is in the SVG');
      });

      await check('PDF d’une note : ses signets font le plan du PDF', async () => {
        const pdf = join(app.dir, 'signet.pdf');
        await exportAs(app, 'pdf', pdf);
        const bytes = readFileSync(pdf).toString('latin1');
        if (!bytes.includes(`/Title (${LABEL})`)) throw new Error('the bookmark is not in the PDF outline');
        if (!bytes.includes('/Outlines')) throw new Error('no outline');
      });

      await check('retour à la bibliothèque : la recherche trouve la note par son signet (sans accent ni casse)', async () => {
        await app.click('.editor-top button', l('editor.back'));
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the library');
        await app.click('input.search');
        await app.key('a', { ctrl: true });
        await app.key('Backspace');
        await app.type('ZEBRURE');
        await app.waitFor(`document.querySelectorAll('.note-card').length === 1`, 'one search result');
        const title = await app.evaluate(`document.querySelector('.note-card .card-title')?.textContent.trim()`);
        if (title !== NOTE) throw new Error(`the search found ${j(title)}`);
        await app.click('input.search');
        await app.key('a', { ctrl: true });
        await app.key('Backspace');
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the whole library');
      });

      await check('le classeur : son bouton « Sommaire » ouvre la liste de ses signets, note par note', async () => {
        await app.click(`.sidebar [data-binder=${j(BINDER)}]`);
        await app.click('[data-action="binder-summary"]');
        await app.waitFor(`document.querySelector('[data-dialog="binder-summary"]') !== null`, 'the summary');
        const title = await app.evaluate(`document.querySelector('.modal h2')?.textContent`);
        if (title !== fill(l('library.summary.title'), { name: '3e B' })) throw new Error(`the dialog is titled ${j(title)}`);
        const lines = await app.evaluate(
          `[...document.querySelectorAll('.summary-note-title, .summary-item')].map((e) => e.classList.contains('summary-item') ? '- ' + e.querySelector('.summary-label').textContent.trim() : e.childNodes[0].textContent.trim())`,
        );
        if (JSON.stringify(lines) !== j([NOTE, `- ${LABEL}`])) throw new Error(`the summary reads ${j(lines)}`);
        const name = await app.evaluate(`document.querySelector('.summary-item').getAttribute('aria-label')`);
        if (name !== fill(l('library.summary.open'), { note: NOTE, label: LABEL })) throw new Error(`the item is named ${j(name)}`);
      });

      await check('un clic sur le signet du sommaire ouvre la note à cet endroit : le signet est sélectionné', async () => {
        await app.click('.summary-item');
        await app.waitFor(`document.querySelector('.editor svg.canvas') !== null`, 'the editor');
        await app.waitFor(`document.querySelectorAll(${j(MARKS)}).length === 1`, 'the bookmark shown');
        await app.waitFor(`document.querySelector('[data-section="selection"] .pane-help') !== null`, 'the bookmark selected');
        const count = await app.evaluate(`document.querySelector('[data-section="selection"] .pane-help')?.textContent.trim()`);
        const expected = fill(l('editor.selection.countMixed'), { strokes: 0, texts: 1 });
        if (count !== expected) throw new Error(`the selection says ${j(count)}`);
        // The page is shown with the bookmark at the middle of the window.
        const where = await app.evaluate(`(() => {
          const svg = document.querySelector('.editor svg.canvas');
          const r = svg.getBoundingClientRect();
          const b = document.querySelector(${j(MARKS)}).getBoundingClientRect();
          return { dx: (b.left + b.width / 2) - (r.left + r.width / 2), dy: (b.top + b.height / 2) - (r.top + r.height / 2), w: r.width, h: r.height };
        })()`);
        if (Math.abs(where.dx) > where.w / 4 || Math.abs(where.dy) > where.h / 4) throw new Error(`the bookmark is not at the middle: ${j(where)}`);
      });
    },
  },
  {
    name: 'signets : renommer par un double-clic, supprimer en vidant le libellé, Ctrl + Z, déplacer avec la sélection',
    async run(app, check) {
      const l = (key) => text(app.language, key);

      await check('poser un signet', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await openNote(app, NOTE);
        await putBookmark(app, 20_000, 3_000, 'Avant');
        await app.key('Enter');
        await app.waitFor(`document.querySelector(${j(FIELD)}) === null`, 'the field gone');
        if (JSON.stringify(await marks(app)) !== j(['Avant'])) throw new Error(`the bookmarks are ${j(await marks(app))}`);
      });

      await check('un double-clic avec l’outil de sélection rouvre le libellé : le renommer, Échap, un seul pas', async () => {
        await app.click('.toolbar .tool', l('editor.modes.select'));
        await app.evaluate(`document.activeElement?.blur(), true`);
        const mark = (await onDiskBookmarks(app))[0] ?? { x: 20_000, y: 3_000 - 462, width: 3000, height: 925 };
        await clickPage(app, mark.x + 200, mark.y + mark.height / 2, 2);
        await app.waitFor(`document.activeElement?.matches(${j(FIELD)}) === true`, 'the field of the bookmark');
        const value = await app.evaluate(`document.activeElement.value`);
        if (value !== 'Avant') throw new Error(`the field holds ${j(value)}`);
        await app.key('a', { ctrl: true });
        await app.type('Après');
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${j(FIELD)}) === null`, 'the field gone');
        if (JSON.stringify(await marks(app)) !== j(['Après'])) throw new Error(`the bookmarks are ${j(await marks(app))}`);
      });

      await check('Ctrl + Z rend l’ancien libellé, Ctrl + Y le nouveau', async () => {
        await app.evaluate(`document.activeElement?.blur(), true`);
        await app.key('z', { ctrl: true });
        await app.waitFor(`JSON.stringify([...document.querySelectorAll(${j(MARKS)})].map((g) => g.dataset.bookmark)) === ${j(j(['Avant']))}`, 'the old label');
        await app.key('y', { ctrl: true });
        await app.waitFor(`JSON.stringify([...document.querySelectorAll(${j(MARKS)})].map((g) => g.dataset.bookmark)) === ${j(j(['Après']))}`, 'the new label');
      });

      await check('glisser la sélection déplace le signet, enregistré au nouvel endroit', async () => {
        await save(app);
        const before = (await onDiskBookmarks(app))[0];
        const from = await pagePoint(app, before.x + 200, before.y + before.height / 2);
        const to = { x: from.x - 60, y: from.y + 50 };
        await app.drag(Array.from({ length: 8 }, (_, i) => ({ x: from.x + ((to.x - from.x) * i) / 7, y: from.y + ((to.y - from.y) * i) / 7 })));
        await save(app);
        const after = (await onDiskBookmarks(app))[0];
        if (!(after.x < before.x) || !(after.y > before.y)) throw new Error(`the bookmark did not move: ${j(before)} → ${j(after)}`);
      });

      await check('vider le libellé supprime le signet ; Ctrl + Z le rend', async () => {
        const mark = (await onDiskBookmarks(app))[0];
        await clickPage(app, mark.x + 200, mark.y + mark.height / 2, 2);
        await app.waitFor(`document.activeElement?.matches(${j(FIELD)}) === true`, 'the field of the bookmark');
        await app.key('a', { ctrl: true });
        await app.key('Backspace');
        await app.key('Enter');
        await app.waitFor(`document.querySelectorAll(${j(MARKS)}).length === 0`, 'the bookmark gone');
        await app.evaluate(`document.activeElement?.blur(), true`);
        await app.key('z', { ctrl: true });
        await app.waitFor(`document.querySelectorAll(${j(MARKS)}).length === 1`, 'the bookmark back');
      });
    },
  },
];

/** The bookmarks of the note open, as written on the disk. */
async function onDiskBookmarks(app) {
  return ((await onDisk(app)).note.objects ?? []).filter((o) => o.kind === 'bookmark');
}
