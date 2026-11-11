// M14 journey, layers at export: the SVG and PDF choice « Séparés » or « Fusionnés », remembered per format, the sentence
// of the images, and a hidden layer that no file holds. Listed in docs/design/exports-and-operations.md, "Layers at export
// (M14)", "Tests (package D)".
//
// The note is a sheet written into the library (`dropSheet`, as the resume journeys do) with two visible layers and a
// hidden one, each holding ink, rather than a note drawn in the editor: the editor's layers zone is the other package's
// (A), and this journey is about the export dialog. The dialog is opened from the editor, which exports the note as it
// is in memory. The PDF and the SVG are read back as bytes: pdf-writer writes the optional content groups as plain
// objects (no object streams), so `/Type /OCG` and the group names can be searched for in the file.
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { PAGE, card, dropSheet, line } from './journeys-m12-resume.mjs';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const j = JSON.stringify;

const TITLE = 'Fiche calques E2E';
const FILE = '2026-11-11 Fiche calques E2E.ardoise';
/** The layers of the sheet, bottom first. The hidden one has an ASCII name so that a search of the bytes means something. */
const LAYERS = [
  { name: 'Calque 1', visible: true },
  { name: 'Correction', visible: true },
  { name: 'Hidden answers', visible: false },
];

/** Radio of the layers block. */
const layerRadio = (value) => `[data-slot="export-layers"] input[type="radio"][name="export-layers"][value="${value}"]`;

/** Whether the radio is checked; null when the block (or the radio) is not there. */
const checked = (app, value) => app.evaluate(`document.querySelector(${j(layerRadio(value))})?.checked ?? null`);

/** Opens the export dialog from the editor and waits until it has read the note (the hidden-layers sentence is the sign). */
async function openDialog(app) {
  await app.click('.toolbar button', text(app.language, 'editor.export.button'));
  await app.waitFor(`document.querySelector('[role="dialog"] .format-card') !== null`, 'the export dialog');
  await app.waitFor(`document.querySelector('[data-slot="export-layers-hidden"]') !== null`, 'the dialog to read the layers of the note');
}

/** Picks a format card by its name. */
async function chooseFormat(app, format) {
  await app.click('[role="dialog"] .format-card .choice-name', text(app.language, `export.formats.${format}`));
  await app.waitFor(
    `document.querySelector('[role="dialog"] .format-card.selected .choice-name')?.textContent === ${j(text(app.language, `export.formats.${format}`))}`,
    `the ${format} card selected`,
  );
}

export const exportLayersJourneys = [
  {
    name: 'export : calques séparés ou fusionnés, choix retenu par format',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const pdf = join(app.dir, 'calques.pdf');
      const svg = join(app.dir, 'calques.svg');

      await check('une fiche de deux calques visibles et d’un calque masqué, chacun avec son trait, est posée dans la bibliothèque', async () => {
        const page = PAGE.landscape;
        await dropSheet(app, {
          binder: '',
          file: FILE,
          title: TITLE,
          index: 41,
          image: false,
          layers: LAYERS,
          ink: [
            { ...line(page.width * 0.1, page.height * 0.2, page.width * 0.9, page.height * 0.2, 0), layer: 0 },
            { ...line(page.width * 0.1, page.height * 0.4, page.width * 0.9, page.height * 0.4, 1500), layer: 1 },
            { ...line(page.width * 0.1, page.height * 0.6, page.width * 0.9, page.height * 0.6, 3000), layer: 2 },
          ],
        });
      });

      await check('ouvrir la fiche dans l’éditeur', async () => {
        await app.click(`${card(FILE)} .card-open`);
        await app.waitFor(`document.querySelector('.editor .toolbar') !== null`, 'the editor and its toolbar', 30_000);
      });

      await check(`PNG : la phrase « ${l('export.layers.flat')} » et « ${l('export.layers.hiddenNever')} », pas de choix`, async () => {
        await openDialog(app);
        const flat = await app.evaluate(`document.querySelector('[data-slot="export-layers-flat"]')?.textContent.trim()`);
        if (flat !== l('export.layers.flat')) throw new Error(`the image sentence is ${j(flat)}`);
        const hidden = await app.evaluate(`document.querySelector('[data-slot="export-layers-hidden"]')?.textContent.trim()`);
        if (hidden !== l('export.layers.hiddenNever')) throw new Error(`the hidden sentence is ${j(hidden)}`);
        if (await app.evaluate(`document.querySelector('[data-slot="export-layers"]') !== null`)) throw new Error('an image has a layers choice');
      });

      await check(`SVG : « ${l('export.layers.separate')} » est choisi d’office (le fichier d’avant)`, async () => {
        await chooseFormat(app, 'svg');
        await app.waitFor(`document.querySelector('[data-slot="export-layers"]') !== null`, 'the layers block');
        const legend = await app.evaluate(`document.querySelector('[data-slot="export-layers"] legend')?.textContent.trim()`);
        if (legend !== l('export.layers.legend')) throw new Error(`the legend is ${j(legend)}`);
        if ((await checked(app, 'separate')) !== true || (await checked(app, 'merged')) !== false) throw new Error('SVG does not default to separate layers');
        if (await app.evaluate(`document.querySelector('[data-slot="export-layers-flat"]') !== null`)) throw new Error('the image sentence stays for SVG');
      });

      await check(`PDF : « ${l('export.layers.merged')} » est choisi d’office (le fichier d’avant)`, async () => {
        await chooseFormat(app, 'pdf');
        if ((await checked(app, 'merged')) !== true || (await checked(app, 'separate')) !== false) throw new Error('PDF does not default to merged layers');
      });

      await check(`PDF « ${l('export.layers.separate')} » : un groupe de calques par calque visible, aucun pour le calque masqué`, async () => {
        await app.click(`[data-slot="export-layers"] label`, l('export.layers.separate'));
        await app.waitFor(`document.querySelector(${j(layerRadio('separate'))})?.checked === true`, 'the separate choice');
        await app.evaluate(`window.__ardoiseE2eSavePath = ${j(pdf)}`);
        await app.click('[role="dialog"] .actions button', l('export.run'));
        await app.waitFor(`document.querySelector('[role="dialog"] .export-result code')?.textContent === ${j(pdf)}`, 'the PDF export result', 60_000);
        if (!existsSync(pdf)) throw new Error(`${pdf} was not written`);
        const bytes = readFileSync(pdf);
        const latin = bytes.toString('latin1');
        if (!latin.startsWith('%PDF-')) throw new Error('not a PDF file');
        if (!latin.includes('/OCProperties')) throw new Error('no /OCProperties: the layers are merged');
        const groups = latin.match(/\/Type\s*\/OCG\b/g) ?? [];
        if (groups.length !== 2) throw new Error(`${groups.length} optional content groups, expected 2`);
        for (const name of ['Calque 1', 'Correction']) {
          if (!latin.includes(`(${name})`)) throw new Error(`the group « ${name} » is missing`);
        }
        if (!/\/PageMode\s*\/UseOC\b/.test(latin)) throw new Error('the layers panel is not asked for (/PageMode /UseOC)');
        if (latin.includes('Hidden')) throw new Error('the hidden layer is in the PDF');
      });

      await check('rouvrir la fenêtre : le PDF reste « Séparés », le SVG « Séparés » (son choix d’origine), le PNG a sa phrase', async () => {
        await app.click('[role="dialog"] .actions button', l('common.close'));
        await app.waitFor(`document.querySelector('[role="dialog"]') === null`, 'the export dialog closed');
        await openDialog(app);
        // The format is remembered too: PDF, with the choice made for it.
        const selected = await app.evaluate(`document.querySelector('[role="dialog"] .format-card.selected .choice-name')?.textContent`);
        if (selected !== l('export.formats.pdf')) throw new Error(`the remembered format is ${j(selected)}`);
        if ((await checked(app, 'separate')) !== true) throw new Error('the PDF choice was not remembered');
        await chooseFormat(app, 'svg');
        if ((await checked(app, 'separate')) !== true) throw new Error('the SVG does not show separate layers');
        await chooseFormat(app, 'png');
        await app.waitFor(`document.querySelector('[data-slot="export-layers-flat"]') !== null`, 'the image sentence');
      });

      await check(`SVG « ${l('export.layers.merged')} » : un seul calque Inkscape de dessin, sans le calque masqué`, async () => {
        await chooseFormat(app, 'svg');
        await app.click(`[data-slot="export-layers"] label`, l('export.layers.merged'));
        await app.waitFor(`document.querySelector(${j(layerRadio('merged'))})?.checked === true`, 'the merged choice');
        await app.evaluate(`window.__ardoiseE2eSavePath = ${j(svg)}`);
        await app.click('[role="dialog"] .actions button', l('export.run'));
        await app.waitFor(`document.querySelector('[role="dialog"] .export-result code')?.textContent === ${j(svg)}`, 'the SVG export result', 60_000);
        const content = readFileSync(svg, 'utf8');
        // The locked background layer and the one layer of ink.
        const layers = content.match(/inkscape:groupmode="layer"/g) ?? [];
        if (layers.length !== 2) throw new Error(`${layers.length} Inkscape layers, expected the background and one of ink`);
        if (!content.includes('inkscape:label="Calque 1 + Correction"')) throw new Error('the merged layer is not labelled with what it holds');
        if (content.includes('Hidden')) throw new Error('the hidden layer is in the SVG');
        if ((content.match(/<path /g) ?? []).length !== 2) throw new Error('the SVG does not hold the two visible strokes');
      });

      await check('rouvrir la fenêtre : le SVG retient « Fusionnés », le PDF « Séparés »', async () => {
        await app.click('[role="dialog"] .actions button', l('common.close'));
        await app.waitFor(`document.querySelector('[role="dialog"]') === null`, 'the export dialog closed');
        await openDialog(app);
        const selected = await app.evaluate(`document.querySelector('[role="dialog"] .format-card.selected .choice-name')?.textContent`);
        if (selected !== l('export.formats.svg')) throw new Error(`the remembered format is ${j(selected)}`);
        if ((await checked(app, 'merged')) !== true) throw new Error('the SVG choice was not remembered');
        await chooseFormat(app, 'pdf');
        if ((await checked(app, 'separate')) !== true) throw new Error('the PDF choice was lost');
        await app.click('[role="dialog"] .actions button', l('common.close'));
        await app.waitFor(`document.querySelector('[role="dialog"]') === null`, 'the export dialog closed');
      });
    },
  },
];
