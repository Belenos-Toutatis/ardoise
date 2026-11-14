// Journeys of the user's feedback of 2026-11-14: exporting a binder whose notes are all in its sub-binders, what the summary
// holds (« Dans le sommaire »: the sub-binders, the notes, the bookmarks: for the summary dialog, the PDF's outline and the
// summary page at the start of the PDF), and the thumbnails of the pages of a PDF imported as notes.
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

/**
 * Creates a note through the app's own command, as the interface does, with bookmarks (one per label, one under the other)
 * saved the way the editor saves them.
 */
const createNote = (app, binder, title, labels = []) =>
  app.evaluate(`(async () => {
    const invoke = window.__TAURI_INTERNALS__.invoke;
    const made = await invoke('create_note', { binder: ${j(binder)}, title: ${j(title)} });
    const labels = ${j(labels)};
    if (labels.length > 0) {
      const opened = await invoke('open_note', { path: made.path });
      const note = opened.note;
      note.objects = labels.map((label, i) => ({ kind: 'bookmark', id: 'bm' + i, layer: 0, t: 0, x: 1000, y: 1000 + i * 2500, scale: 1, quarterTurns: 0, width: 3000, height: 925, label }));
      await invoke('save_note', { path: made.path, note, stamp: opened.stamp });
    }
    return true;
  })()`);

const pageCount = (bytes) => (bytes.toString('latin1').match(/\/Type\s*\/Page(?![s\w])/g) ?? []).length;
const linkCount = (bytes) => (bytes.toString('latin1').match(/\/Subtype\s*\/Link/g) ?? []).length;
const titleCount = (bytes, prefix) => bytes.toString('latin1').split(`/Title (${prefix}`).length - 1;

/** The primary button of the dialog is disabled. */
const runDisabled = (app) => app.evaluate(`document.querySelector('${DIALOG} .actions .button.primary').disabled`);

export const binderFixJourneys = [
  {
    name: 'retour 2026-11-14 : un classeur sans note à lui exporte celles de ses sous-classeurs ; « Dans le sommaire » : signets seuls sous les sous-classeurs, page de sommaire',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const target = join(app.dir, 'oral.pdf');
      const PARENT = 'oral du dnb';
      const done = fill(l('export.done'), { count: 1 });
      const latin = () => readFileSync(target).toString('latin1');
      const bytes = () => readFileSync(target);
      const exportAgain = async (what) => {
        await app.click(`${DIALOG} button`, l('export.again'));
        await app.waitFor(`document.querySelector('${DIALOG} .export-status')?.textContent.includes(${j(done)})`, `the export done (${what})`, 30_000);
        await new Promise((resolve) => setTimeout(resolve, 300));
      };
      const option = (name) => `input[data-option="${name}"]`;
      const checked = (name) => app.evaluate(`document.querySelector(${j(option(name))}).checked`);
      const openExport = async () => {
        await rightClick(app, `.sidebar [data-binder=${j(PARENT)}]`);
        await app.waitFor(`document.querySelector('.menu-list[role="menu"]') !== null`, 'the binder menu');
        await app.click('.menu-list [role="menuitem"]', l('library.binder.exportPdf'));
        await app.waitFor(`document.querySelector(${j(DIALOG)})?.textContent.includes(${j(fill(l('export.subjectBinder'), { name: PARENT }))})`, 'the export dialog');
      };

      await check('un classeur sans note à lui : « 2026 » (une note titrée à deux signets, deux notes sans titre ni signet) et « 2027 » (une note à un signet), et un classeur vide', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the demo notes');
        await createNote(app, `${PARENT}/2026`, 'Candidat un', ['Presentation', 'Questions']);
        await createNote(app, `${PARENT}/2026`, '');
        await createNote(app, `${PARENT}/2026`, '');
        await createNote(app, `${PARENT}/2027`, 'Candidat deux', ['Conclusion']);
        await app.evaluate(`window.__TAURI_INTERNALS__.invoke('create_binder', { binder: 'Vide' }).then(() => true)`);
        await app.waitFor(`document.querySelector('.sidebar [data-binder=${j(PARENT)}]') !== null`, 'the binder');
        await app.waitFor(`document.querySelectorAll('.note-card').length === 12`, 'the new notes');
      });

      await check('« Exporter le classeur en PDF » : « Inclure les sous-classeurs » est cochée d’emblée, avec la phrase qui le dit', async () => {
        await openExport();
        if (!(await checked('include-sub-binders'))) throw new Error('the sub-binders are not included');
        const said = await app.evaluate(`document.querySelector('[data-slot="binder-only-sub"]')?.textContent.trim() ?? ''`);
        if (said !== l('export.binderOnlySubIncluded')) throw new Error(`the sentence is "${said}"`);
      });

      await check('décochée : plus rien à exporter, le bouton attend et la phrase dit quoi cocher (aucune erreur de disque)', async () => {
        await app.click(option('include-sub-binders'));
        if (!(await runDisabled(app))) throw new Error('the button is enabled with nothing to export');
        const said = await app.evaluate(`document.querySelector('[data-slot="binder-only-sub"]')?.textContent.trim() ?? ''`);
        if (said !== l('export.binderOnlySub')) throw new Error(`the sentence is "${said}"`);
        if (await app.evaluate(`document.querySelector('${DIALOG} [role="alert"]') !== null`)) throw new Error('an error is shown');
        await app.click(option('include-sub-binders'));
      });

      await check('« Dans le sommaire » au départ : les sous-classeurs et les signets, pas de ligne par note ; le PDF a quatre pages et ses signets sont « 2026 » > « Presentation », « Questions », « 2027 » > « Conclusion »', async () => {
        if (!(await checked('summary-binders')) || !(await checked('summary-notes-none')) || !(await checked('summary-bookmarks'))) throw new Error('the defaults are not the sub-binders and the bookmarks');
        await app.evaluate(`window.__ardoiseE2eSavePath = ${j(target)}; true`);
        await app.click(`${DIALOG} button`, l('export.run'));
        await app.waitFor(`document.querySelector('${DIALOG} .export-status')?.textContent.includes(${j(done)})`, 'the export done', 30_000);
        if (pageCount(bytes()) !== 4) throw new Error(`${pageCount(bytes())} pages`);
        for (const title of ['2026', '2027', 'Presentation', 'Questions', 'Conclusion']) {
          if (!latin().includes(`/Title (${title})`)) throw new Error(`no bookmark "${title}"`);
        }
        // No bookmark for a note, titled or not.
        if (latin().includes('/Title (Candidat') || titleCount(bytes(), l('editor.untitled')) !== 0) throw new Error('a note has a bookmark');
        if (latin().includes(`/Title (${l('export.summaryOutline')})`)) throw new Error('a summary was not asked for');
      });

      await check('« Notes : Titre » : les notes ont leur signet, « Note sans titre » puis « Note sans titre (2) » ; « Titre ou date » : plus de « Note sans titre »', async () => {
        await app.click(option('summary-notes-title'));
        await exportAgain('title');
        if (!latin().includes('/Title (Candidat un)') || !latin().includes('/Title (Candidat deux)')) throw new Error('no bookmark for the titled notes');
        if (titleCount(bytes(), l('editor.untitled')) !== 2) throw new Error(`${titleCount(bytes(), l('editor.untitled'))} untitled bookmarks`);
        await app.click(option('summary-notes-titleOrDate'));
        await exportAgain('title or date');
        if (titleCount(bytes(), l('editor.untitled')) !== 0) throw new Error('an untitled bookmark remains with the dates on');
        if (!latin().includes('/Title (Candidat un)')) throw new Error('the titled note lost its bookmark');
      });

      await check('sans les sous-classeurs : les signets seuls, au premier niveau ; sans signets ni notes : rien à mettre, la case du sommaire attend et le PDF n’a pas de signets', async () => {
        await app.click(option('summary-notes-none'));
        await app.click(option('summary-binders'));
        await exportAgain('bookmarks only');
        if (latin().includes('/Title (2026)') || latin().includes('/Title (2027)')) throw new Error('the sub-binders are still named');
        if (!latin().includes('/Title (Conclusion)')) throw new Error('the bookmarks are gone');
        await app.click(option('summary-binders'));
        await app.click(option('summary-bookmarks'));
        if (await app.evaluate(`document.querySelector('[data-slot="summary-empty"]') === null`)) throw new Error('no sentence for the empty summary');
        if (!(await app.evaluate(`document.querySelector(${j(option('summary-page'))}).disabled`))) throw new Error('the summary page box is enabled');
        await exportAgain('nothing');
        if (latin().includes('/Outlines')) throw new Error('an outline was written with nothing in it');
        if (pageCount(bytes()) !== 4) throw new Error(`${pageCount(bytes())} pages`);
        await app.click(option('summary-bookmarks'));
      });

      await check('« Ajouter une page de sommaire au début » : cinq pages, un signet « Sommaire » en tête, un lien par ligne (deux sous-classeurs et trois signets, ceux-ci à leur endroit)', async () => {
        await app.click(option('summary-page'));
        await exportAgain('summary page');
        if (pageCount(bytes()) !== 5) throw new Error(`${pageCount(bytes())} pages`);
        if (!latin().includes(`/Title (${l('export.summaryOutline')})`)) throw new Error('no outline entry for the summary');
        if (linkCount(bytes()) !== 5) throw new Error(`${linkCount(bytes())} links`);
        const toSpot = (latin().match(/\/S\s*\/GoTo\s*\/D\s*\[[^\]]*\/XYZ/g) ?? []).length;
        if (toSpot !== 3) throw new Error(`${toSpot} links to a spot`);
        await app.click(`${DIALOG} button`, l('common.close'));
      });

      await check('rouvert, le dialogue garde les choix (sommaire coché, notes : Non, signets et sous-classeurs cochés)', async () => {
        await openExport();
        if (!(await checked('summary-page'))) throw new Error('the summary box was forgotten');
        if (!(await checked('summary-notes-none')) || !(await checked('summary-bookmarks')) || !(await checked('summary-binders'))) throw new Error('the choices were forgotten');
        await app.click(`${DIALOG} button`, l('library.cancel'));
      });

      await check('« Imprimer le classeur » : la même case cochée d’emblée, pas de case pour la page de sommaire ni pour le contenu du sommaire', async () => {
        await rightClick(app, `.sidebar [data-binder=${j(PARENT)}]`);
        await app.click('.menu-list [role="menuitem"]', l('library.binder.print'));
        await app.waitFor(`document.querySelector('[data-dialog="print"]') !== null`, 'the print dialog');
        if (!(await checked('include-sub-binders'))) throw new Error('the sub-binders are not included');
        if (await app.evaluate(`document.querySelector(${j(option('summary-page'))}) !== null || document.querySelector('[data-slot="summary-choices"]') !== null`)) throw new Error('a summary box in the print dialog');
        await app.click(`${DIALOG} button`, l('library.cancel'));
      });

      await check('le sommaire du classeur lit les mêmes choix : « 2026 » > signets, « 2027 » > signet ; « Notes : Titre » ajoute les lignes de notes, un clic sur l’une ouvre la note', async () => {
        await app.click(`.sidebar [data-binder=${j(PARENT)}]`);
        await app.click('button[data-action="binder-summary"]');
        await app.waitFor(`document.querySelector('[data-dialog="binder-summary"]') !== null`, 'the summary');
        const lines = () =>
          app.evaluate(
            `[...document.querySelectorAll('[data-row]')].map((e) => '.'.repeat(Number(e.style.getPropertyValue('--level'))) + (e.dataset.row === 'bookmark' ? '⚑' : e.dataset.row === 'binder' ? '#' : '') + (e.querySelector('.summary-label') ?? e).textContent.trim())`,
          );
        const bookmarksOnly = ['#2026', '.⚑Presentation', '.⚑Questions', '#2027', '.⚑Conclusion'];
        if (j(await lines()) !== j(bookmarksOnly)) throw new Error(`the summary reads ${j(await lines())}`);
        await app.click(option('summary-notes-title'));
        const withNotes = ['#2026', '.Candidat un', '..⚑Presentation', '..⚑Questions', `.${l('editor.untitled')}`, `.${l('editor.untitled')} (2)`, '#2027', '.Candidat deux', '..⚑Conclusion'];
        if (j(await lines()) !== j(withNotes)) throw new Error(`with the notes the summary reads ${j(await lines())}`);
        await app.click('[data-dialog="binder-summary"] [data-row="note"]');
        await app.waitFor(`document.querySelector('.editor svg.canvas') !== null`, 'the editor');
        if (await app.evaluate(`document.querySelector('[data-section="selection"]') !== null`)) throw new Error('a bookmark is selected after opening a note line');
        await app.click('.editor button', l('editor.back'));
        await app.waitFor(`document.querySelector('.note-card') !== null`, 'the library again');
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
