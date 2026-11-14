// Journeys of the user's feedback of 2026-11-14: exporting a binder whose notes are all in its sub-binders, the dates of
// the summary and of the PDF's bookmarks, the summary page at the start of a binder's PDF, and the thumbnails of the
// pages of a PDF imported as notes.
//
// On the production executable and the demo library, in French and in English; the real window is driven with real clicks
// (the right click included) and the files are read on the disk. Labels come from the translation files.
//
// Not driven here: « Imprimer le classeur… » (the Windows print window cannot be driven by the DevTools protocol; its dialog is
// tested by vitest: `BinderDialogs.test.ts`), and the look of the summary page (read as a picture by hand, see the report).
import { readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { twoPagePdf } from './journeys-m7-background.mjs';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const fill = (template, params) => template.replace(/\{(\w+)\}/g, (match, name) => (name in params ? String(params[name]) : match));
const j = JSON.stringify;
const DIALOG = '[role="dialog"]';

/** A real right click at the centre of an element. */
async function rightClick(app, selector, label) {
  const { x, y } = await app.centre(selector, label);
  await app.mouse('mouseMoved', x, y);
  await app.mouse('mousePressed', x, y, { button: 'right', buttons: 2 });
  await app.mouse('mouseReleased', x, y, { button: 'right', buttons: 0 });
}

/** Creates a note through the app's own command, as the interface does. */
const createNote = (app, binder, title) =>
  app.evaluate(`window.__TAURI_INTERNALS__.invoke('create_note', { binder: ${j(binder)}, title: ${j(title)} }).then(() => true)`);

const pageCount = (bytes) => (bytes.toString('latin1').match(/\/Type\s*\/Page(?![s\w])/g) ?? []).length;
const linkCount = (bytes) => (bytes.toString('latin1').match(/\/Subtype\s*\/Link/g) ?? []).length;
const titleCount = (bytes, prefix) => bytes.toString('latin1').split(`/Title (${prefix}`).length - 1;

/** The primary button of the dialog is disabled. */
const runDisabled = (app) => app.evaluate(`document.querySelector('${DIALOG} .actions .button.primary').disabled`);

export const binderFixJourneys = [
  {
    name: 'retour 2026-11-14 : un classeur sans note à lui exporte celles de ses sous-classeurs, dates et page de sommaire',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const target = join(app.dir, 'oral.pdf');
      const PARENT = 'oral du dnb';
      const done = fill(l('export.done'), { count: 1 });

      await check('un classeur sans note à lui, avec un sous-classeur de deux notes (une titrée, une sans titre) et un classeur vide', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the demo notes');
        await createNote(app, `${PARENT}/2026`, 'Candidat un');
        await createNote(app, `${PARENT}/2026`, '');
        await createNote(app, `${PARENT}/2026`, '');
        await app.evaluate(`window.__TAURI_INTERNALS__.invoke('create_binder', { binder: 'Vide' }).then(() => true)`);
        await app.waitFor(`document.querySelector('.sidebar [data-binder=${j(PARENT)}]') !== null`, 'the binder');
      });

      await check('« Exporter le classeur en PDF » : « Inclure les sous-classeurs » est cochée d’emblée, avec la phrase qui le dit', async () => {
        await rightClick(app, `.sidebar [data-binder=${j(PARENT)}]`);
        await app.waitFor(`document.querySelector('.menu-list[role="menu"]') !== null`, 'the binder menu');
        await app.click('.menu-list [role="menuitem"]', l('library.binder.exportPdf'));
        await app.waitFor(`document.querySelector(${j(DIALOG)})?.textContent.includes(${j(fill(l('export.subjectBinder'), { name: PARENT }))})`, 'the export dialog');
        if (!(await app.evaluate(`document.querySelector('input[data-option="include-sub-binders"]').checked`))) throw new Error('the sub-binders are not included');
        const said = await app.evaluate(`document.querySelector('[data-slot="binder-only-sub"]')?.textContent.trim() ?? ''`);
        if (said !== l('export.binderOnlySubIncluded')) throw new Error(`the sentence is "${said}"`);
        if (await runDisabled(app)) throw new Error('the button is disabled');
      });

      await check('décochée : plus rien à exporter, le bouton attend et la phrase dit quoi cocher (aucune erreur de disque)', async () => {
        await app.click('input[data-option="include-sub-binders"]');
        if (!(await runDisabled(app))) throw new Error('the button is enabled with nothing to export');
        const said = await app.evaluate(`document.querySelector('[data-slot="binder-only-sub"]')?.textContent.trim() ?? ''`);
        if (said !== l('export.binderOnlySub')) throw new Error(`the sentence is "${said}"`);
        if (await app.evaluate(`document.querySelector('${DIALOG} [role="alert"]') !== null`)) throw new Error('an error is shown');
        await app.click('input[data-option="include-sub-binders"]');
      });

      await check('dates décochées (par défaut) : le PDF a trois pages, les notes sans titre sont « Note sans titre » puis « Note sans titre (2) »', async () => {
        if (await app.evaluate(`document.querySelector('input[data-option="show-dates"]').checked`)) throw new Error('the dates are on at first');
        await app.evaluate(`window.__ardoiseE2eSavePath = ${j(target)}; true`);
        await app.click(`${DIALOG} button`, l('export.run'));
        await app.waitFor(`document.querySelector('${DIALOG} .export-status')?.textContent.includes(${j(done)})`, 'the export done', 30_000);
        const bytes = readFileSync(target);
        if (pageCount(bytes) !== 3) throw new Error(`${pageCount(bytes)} pages`);
        if (!bytes.toString('latin1').includes('/Title (Candidat un)')) throw new Error('no bookmark for the titled note');
        if (titleCount(bytes, l('editor.untitled')) !== 2) throw new Error(`${titleCount(bytes, l('editor.untitled'))} untitled bookmarks`);
        if (bytes.toString('latin1').includes('/Title (Sommaire') || bytes.toString('latin1').includes('/Title (Summary')) throw new Error('a summary was not asked for');
      });

      await check('« Afficher les dates » : les notes sans titre portent leur date, plus « Note sans titre »', async () => {
        await app.click('input[data-option="show-dates"]');
        await app.click(`${DIALOG} button`, l('export.again'));
        await app.waitFor(`document.querySelector('${DIALOG} .export-status')?.textContent.includes(${j(done)})`, 'the second export done', 30_000);
        await new Promise((resolve) => setTimeout(resolve, 300));
        const bytes = readFileSync(target);
        if (titleCount(bytes, l('editor.untitled')) !== 0) throw new Error('an untitled bookmark remains with the dates on');
        if (pageCount(bytes) !== 3) throw new Error(`${pageCount(bytes)} pages`);
      });

      await check('« Ajouter une page de sommaire au début » : quatre pages, un signet « Sommaire » en tête, un lien par ligne', async () => {
        await app.click('input[data-option="show-dates"]');
        await app.click('input[data-option="summary-page"]');
        await app.click(`${DIALOG} button`, l('export.again'));
        await app.waitFor(`document.querySelector('${DIALOG} .export-status')?.textContent.includes(${j(done)})`, 'the third export done', 30_000);
        await new Promise((resolve) => setTimeout(resolve, 300));
        const bytes = readFileSync(target);
        if (pageCount(bytes) !== 4) throw new Error(`${pageCount(bytes)} pages`);
        const latin = bytes.toString('latin1');
        if (!latin.includes(`/Title (${l('export.summaryOutline')})`)) throw new Error('no outline entry for the summary');
        // The rows: the sub-binder « 2026 » and its three notes.
        if (linkCount(bytes) !== 4) throw new Error(`${linkCount(bytes)} links`);
        if (titleCount(bytes, l('editor.untitled')) !== 2) throw new Error('the dates came back');
        await app.click(`${DIALOG} button`, l('common.close'));
      });

      await check('rouvert, le dialogue garde les choix (sommaire coché, dates décochées)', async () => {
        await rightClick(app, `.sidebar [data-binder=${j(PARENT)}]`);
        await app.click('.menu-list [role="menuitem"]', l('library.binder.exportPdf'));
        await app.waitFor(`document.querySelector(${j(DIALOG)}) !== null`, 'the export dialog');
        if (!(await app.evaluate(`document.querySelector('input[data-option="summary-page"]').checked`))) throw new Error('the summary box was forgotten');
        if (await app.evaluate(`document.querySelector('input[data-option="show-dates"]').checked`)) throw new Error('the dates box is on');
        await app.click(`${DIALOG} button`, l('library.cancel'));
      });

      await check('« Imprimer le classeur » : la même case cochée d’emblée, pas de case pour la page de sommaire', async () => {
        await rightClick(app, `.sidebar [data-binder=${j(PARENT)}]`);
        await app.click('.menu-list [role="menuitem"]', l('library.binder.print'));
        await app.waitFor(`document.querySelector('[data-dialog="print"]') !== null`, 'the print dialog');
        if (!(await app.evaluate(`document.querySelector('input[data-option="include-sub-binders"]').checked`))) throw new Error('the sub-binders are not included');
        if (await app.evaluate(`document.querySelector('input[data-option="summary-page"]') !== null`)) throw new Error('a summary box in the print dialog');
        await app.click(`${DIALOG} button`, l('library.cancel'));
      });
    },
  },
  {
    name: 'retour 2026-11-14 : choisir les pages d’un PDF sur des miniatures pour en faire des notes',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const pdf = join(app.dir, 'miniatures-e2e.pdf');
      writeFileSync(pdf, twoPagePdf());
      const boxes = `${DIALOG} .pdf-strip [role="checkbox"]`;

      await check('« Depuis un PDF ou une image » : une miniature par page, dessinée par Windows, les deux pages choisies', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length >= 8`, 'the notes');
        await app.evaluate(`window.__ardoiseE2eOpenPath = ${j(pdf)}`);
        await app.click('button[data-action="new-from-file"]');
        await app.waitFor(`document.querySelectorAll(${j(boxes)}).length === 2`, 'two thumbnails');
        await app.waitFor(`document.querySelectorAll(${j(`${DIALOG} .pdf-strip img`)}).length === 2`, 'the two pictures', 20_000);
        const checked = await app.evaluate(`[...document.querySelectorAll(${j(boxes)})].map((b) => b.getAttribute('aria-checked'))`);
        if (checked.join() !== 'true,true') throw new Error(`checked: ${checked}`);
      });

      await check('un clic sur la miniature de la page 2 la retire : « certaines pages », le champ dit 1', async () => {
        await app.click(`${boxes}[data-page="2"]`);
        await app.waitFor(`document.querySelector('input[value="some"]').checked`, 'some pages');
        const field = await app.evaluate(`document.querySelector('input[aria-label=${j(l('background.library.pageList'))}]').value`);
        if (field !== '1') throw new Error(`the field says "${field}"`);
        const checked = await app.evaluate(`[...document.querySelectorAll(${j(boxes)})].map((b) => b.getAttribute('aria-checked'))`);
        if (checked.join() !== 'true,false') throw new Error(`checked: ${checked}`);
      });

      await check('au clavier : Espace sur la page 2 la remet, « Aucune » les vide toutes, « Toutes » les remet', async () => {
        await app.evaluate(`document.querySelector(${j(`${boxes}[data-page="2"]`)}).focus(); true`);
        await app.key(' ');
        await app.waitFor(`document.querySelector(${j(`${boxes}[data-page="2"]`)}).getAttribute('aria-checked') === 'true'`, 'page 2 chosen again');
        await app.click(`${DIALOG} button[data-action="pages-none"]`);
        await app.waitFor(`[...document.querySelectorAll(${j(boxes)})].every((b) => b.getAttribute('aria-checked') === 'false')`, 'none chosen');
        if (!(await runDisabled(app))) throw new Error('the button is enabled with no page');
        await app.click(`${DIALOG} button[data-action="pages-all"]`);
        await app.waitFor(`[...document.querySelectorAll(${j(boxes)})].every((b) => b.getAttribute('aria-checked') === 'true')`, 'all chosen');
      });

      await check('la page 2 seule : une note, « miniatures-e2e - page 2 »', async () => {
        await app.click(`${boxes}[data-page="1"]`);
        await app.click(`${DIALOG} .actions button.primary`);
        await app.waitFor(`document.querySelector(${j(DIALOG)}) === null`, 'the dialog to close', 30_000);
        await app.waitFor(`[...document.querySelectorAll('.note-card .card-title')].some((e) => e.textContent.trim() === 'miniatures-e2e - page 2')`, 'the note of page 2', 15_000);
        if (await app.evaluate(`[...document.querySelectorAll('.note-card .card-title')].some((e) => e.textContent.trim() === 'miniatures-e2e - page 1')`)) throw new Error('page 1 was made too');
      });
    },
  },
];
