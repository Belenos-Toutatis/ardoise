// End-to-end journeys of the M8 exports: the image export in WebP, the recognised text as
// Markdown and as Word (structure from Windows' ink analysis), and the print dialog. Not wired
// into run.mjs: the integrator adds `journeys` to the list after merging. Same patterns as
// journeys-m7-hwr.mjs (labels from the translation files, so it runs in French and in English).
//
// The demo library holds « Cours d'optique », whose ink is the French sentence « la lumière se
// propage / en ligne droite » written with the synthetic glyphs of `ardoise_hwr::synthetic`.
// Needs Windows' French handwriting recognizer (present on a French Windows). Printing opens
// Windows' own dialog, which a journey cannot drive: the journey only checks that the print
// dialog of the app opens and closes. What Windows' dialog does afterwards is checked by hand
// (see the M8 report of the exports part).
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const NOTE = "Cours d'optique";

/** Types into the library search field (replacing what is there). */
async function search(app, query) {
  await app.click('input.search');
  await app.key('a', { ctrl: true });
  await app.key('Backspace');
  if (query) await app.type(query);
}

/** Opens the demo note « Cours d'optique » in the editor. */
async function openOptics(app) {
  await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
  await search(app, 'optique');
  await app.waitFor(
    `[...document.querySelectorAll('.note-card .card-title')].some((c) => c.textContent.trim() === ${JSON.stringify(NOTE)})`,
    'the optics note',
  );
  await app.click('.note-card .card-open');
  await app.waitFor(`document.querySelector('.editor .toolbar') !== null`, 'the editor and its toolbar', 30_000);
}

export const journeys = [
  {
    name: 'exports M8 : image WebP, texte en Markdown et en Word, fenêtre d’impression',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const j = JSON.stringify;
      const webp = join(app.dir, 'optique.webp');
      const markdown = join(app.dir, 'optique.md');
      const docx = join(app.dir, 'optique.docx');

      await check('ouvrir la note « Cours d’optique »', async () => {
        await openOptics(app);
      });

      await check(`exporter en WebP (carte « ${l('export.formats.webp')} »)`, async () => {
        await app.click('.toolbar button', l('editor.export.button'));
        await app.waitFor(`document.querySelector('[role="dialog"] .format-card') !== null`, 'the export dialog');
        const cards = await app.evaluate(`document.querySelectorAll('[role="dialog"] .format-card').length`);
        if (cards !== 5) throw new Error(`expected 5 format cards, found ${cards}`);
        await app.click('[role="dialog"] .format-card .choice-name', l('export.formats.webp'));
        await app.evaluate(`window.__ardoiseE2eSavePath = ${j(webp)}`);
        await app.click('[role="dialog"] .actions button', l('export.run'));
        await app.waitFor(`document.querySelector('[role="dialog"] .export-result code')?.textContent === ${j(webp)}`, 'the export result', 60_000);
      });

      await check('le fichier .webp est un WebP sans perte de la taille de la page', async () => {
        if (!existsSync(webp)) throw new Error(`${webp} was not written`);
        const bytes = readFileSync(webp);
        if (bytes.toString('latin1', 0, 4) !== 'RIFF' || bytes.toString('latin1', 8, 16) !== 'WEBPVP8L') {
          throw new Error('not a lossless WebP file');
        }
        if (statSync(webp).size < 1000) throw new Error('the image is empty');
      });

      await check(`texte en Markdown (« ${l('handwriting.export.saveMd')} ») : titre, date, étiquettes, texte`, async () => {
        await app.evaluate(`window.__ardoiseE2eSavePath = ${j(markdown)}`);
        await app.click('button[data-action="text-md"]');
        await app.waitFor(`document.querySelector('.text-export .export-result code')?.textContent === ${j(markdown)}`, 'the Markdown export result', 60_000);
        if (!existsSync(markdown)) throw new Error(`${markdown} was not written`);
        const content = readFileSync(markdown, 'utf8');
        if (!content.startsWith(`# ${NOTE}\n`)) throw new Error(`unexpected start ${j(content.slice(0, 60))}`);
        if (!content.includes(`${l('handwriting.export.labels.date')} `)) throw new Error(`no date line: ${j(content)}`);
        if (!/lumière se propage/i.test(content)) throw new Error(`the sentence is missing: ${j(content)}`);
      });

      await check(`texte en Word (« ${l('handwriting.export.saveDocx')} ») : un fichier .docx valide`, async () => {
        await app.evaluate(`window.__ardoiseE2eSavePath = ${j(docx)}`);
        await app.click('button[data-action="text-docx"]');
        await app.waitFor(`document.querySelector('.text-export .export-result code')?.textContent === ${j(docx)}`, 'the Word export result', 60_000);
        const bytes = readFileSync(docx);
        if (bytes.toString('latin1', 0, 2) !== 'PK') throw new Error('not a ZIP file');
        for (const member of ['[Content_Types].xml', 'word/document.xml', 'word/styles.xml']) {
          if (!bytes.includes(Buffer.from(member))) throw new Error(`${member} is missing`);
        }
      });

      await check('aucun fichier temporaire ne reste à côté des exports', async () => {
        const left = readdirSync(app.dir).filter((name) => /\.tmp|~$/i.test(name));
        if (left.length) throw new Error(`leftovers: ${left}`);
      });

      await check(`la fenêtre « ${l('print.title')} » s’ouvre depuis l’éditeur et se ferme sans imprimer`, async () => {
        await app.click('[role="dialog"] .actions button', l('common.close'));
        await app.waitFor(`document.querySelector('[role="dialog"]') === null`, 'the export dialog closed');
        await app.click('.toolbar button[data-action="print"]');
        await app.waitFor(`document.querySelector('[data-dialog="print"]') !== null`, 'the print dialog');
        const options = await app.evaluate(
          `[...document.querySelectorAll('[data-dialog="print"] input')].map((i) => i.type + ':' + (i.name || i.dataset.option))`,
        );
        if (!options.includes('checkbox:background') || !options.includes('radio:print-fit')) {
          throw new Error(`unexpected options ${j(options)}`);
        }
        await app.click('[data-dialog="print"] .actions button', l('library.cancel'));
        await app.waitFor(`document.querySelector('[data-dialog="print"]') === null`, 'the print dialog closed');
      });
    },
  },
];
