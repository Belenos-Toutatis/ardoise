// End-to-end journey of the handwriting recognition (milestone M7), run by run.mjs after the
// main journeys. Same patterns as journeys.mjs (labels from the translation files, so it runs in French
// and in English).
//
// The demo library holds « Cours d'optique », whose ink is the French sentence « la lumière se
// propage / en ligne droite » written with the synthetic glyphs of `ardoise_hwr::synthetic`; none
// of its words is in a title, a tag or a binder. The run's LOCALAPPDATA is fresh, so the app
// recognises the demo notes in the background first (a few seconds): the journey waits for it.
// Needs Windows' French handwriting recognizer (present on a French Windows).
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const NOTE = "Cours d'optique";
// The status line of the handwriting section: the OpenH264 section above it has a `data-slot="status"` too.
const HANDWRITING_STATUS = 'section[aria-labelledby="settings-handwriting"] [data-slot="status"]';
const cardTitles = (app) =>
  app.evaluate(`[...document.querySelectorAll('.note-card .card-title')].map((c) => c.textContent.trim())`);

/** Types into the library search field (replacing what is there). */
async function search(app, query) {
  await app.click('input.search');
  await app.key('a', { ctrl: true });
  await app.key('Backspace');
  if (query) await app.type(query);
}

/** Waits until the grid shows exactly the demo note found by its handwriting. */
async function onlyTheOpticsNote(app, what, timeout = 60_000) {
  await app.waitFor(
    `(() => { const t = [...document.querySelectorAll('.note-card .card-title')].map((c) => c.textContent.trim()); return t.length === 1 && t[0] === ${JSON.stringify(NOTE)}; })()`,
    what,
    timeout,
  );
}

export const journeys = [
  {
    name: 'écriture manuscrite : retrouver une note par un mot écrit à la main, exporter son texte',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const j = JSON.stringify;
      const txt = join(app.dir, 'texte-reconnu.txt');

      await check('la bibliothèque de démonstration s’affiche (8 notes)', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
      });

      await check('la recherche « lumiere » (sans accent) trouve « Cours d’optique » par son écriture', async () => {
        await search(app, 'lumiere');
        await onlyTheOpticsNote(app, 'the note found by its handwriting (after the background indexing)');
      });

      await check('un extrait du texte reconnu montre pourquoi la note a été trouvée', async () => {
        await app.waitFor(
          `document.querySelector('.handwriting-excerpt .excerpt-title')?.textContent === ${j(NOTE)}`,
          'the excerpt of the note',
        );
        const word = await app.evaluate(`document.querySelector('.handwriting-excerpt mark')?.textContent ?? ''`);
        if (!/^lumi[eè]re/i.test(word)) throw new Error(`unexpected highlighted word ${j(word)}`);
      });

      await check('la recherche « LIGNE DROITE » trouve la même note', async () => {
        await search(app, 'LIGNE DROITE');
        await onlyTheOpticsNote(app, 'the note found by two handwritten words', 15_000);
      });

      await check('un mot du titre et un mot manuscrit ensemble : « optique propage »', async () => {
        await search(app, 'optique propage');
        await onlyTheOpticsNote(app, 'the note found by its title and its handwriting', 15_000);
      });

      await check('un mot absent de l’écriture ne trouve rien', async () => {
        await search(app, 'electron');
        await app.waitFor(`document.querySelectorAll('.note-card').length === 0`, 'no result');
        const titles = await cardTitles(app);
        if (titles.length !== 0) throw new Error(`unexpected results ${titles}`);
      });

      await check(`les réglages listent les modules de reconnaissance de ce PC (« ${l('handwriting.settings.title')} »)`, async () => {
        await search(app, '');
        await app.click('.topbar button', l('settings.title'));
        await app.waitFor(`document.querySelector('[data-slot="recognizers"] li') !== null`, 'the installed recognizers');
        const idle = l('handwriting.status.idle').replace('{total}', '8');
        try {
          await app.waitFor(`document.querySelector(${j(HANDWRITING_STATUS)})?.textContent === ${j(idle)}`, 'every demo note read', 60_000);
        } catch (error) {
          const shown = await app.evaluate(`document.querySelector(${j(HANDWRITING_STATUS)})?.textContent ?? null`);
          throw new Error(`${error.message}: the status says ${j(shown)}, expected ${j(idle)}`);
        }
        await app.click('.topbar button', l('settings.title'));
      });

      await check(`ouvrir la note et exporter son texte en .txt (« ${l('handwriting.export.save')} »)`, async () => {
        await search(app, 'lumiere');
        await onlyTheOpticsNote(app, 'the note to open', 15_000);
        await app.click('.note-card .card-open');
        await app.waitFor(`document.querySelector('.editor .toolbar') !== null`, 'the editor and its toolbar', 30_000);
        await app.click('.toolbar button', l('editor.export.button'));
        await app.waitFor(`document.querySelector('[role="dialog"] .text-export') !== null`, 'the text export of the dialog');
        await app.evaluate(`window.__ardoiseE2eSavePath = ${j(txt)}`);
        await app.click('button[data-action="text-save"]');
        await app.waitFor(
          `document.querySelector('.text-export .export-result code')?.textContent === ${j(txt)}`,
          'the text export result',
          30_000,
        );
      });

      await check('le fichier .txt (UTF-8) contient le titre et la phrase reconnue', async () => {
        if (!existsSync(txt)) throw new Error(`${txt} was not written`);
        const content = readFileSync(txt, 'utf8');
        if (!content.startsWith(`${NOTE}\r\n`)) throw new Error(`unexpected start ${j(content.slice(0, 40))}`);
        if (!/lumière se propage/.test(content)) throw new Error(`the sentence is missing: ${j(content)}`);
      });
    },
  },
];
