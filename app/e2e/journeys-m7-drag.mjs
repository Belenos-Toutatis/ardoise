// M7 journey "drag a note out of the library" (run by run.mjs after the other journeys).
//
// The system drag itself (OLE `DoDragDrop`) needs the real mouse button to be down, which the
// DevTools protocol's synthetic mouse events do not give: the app then cancels the drag at once
// (by design: a drag must never "drop" under a pointer that was released). So this journey checks
// everything up to there on the real executable: the handle on the cards, the real rendering of
// the PNG files in the app's temporary folder, named after the notes, for one note and for a
// selection, and the interface coming back to rest. Dropping into an Explorer folder is checked
// by hand (docs/design/extras.md, "Verification on Windows").
import { existsSync, mkdirSync, readFileSync, readdirSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const dragRoot = () => join(tmpdir(), 'Ardoise', 'drag');
const dragFolders = () => (existsSync(dragRoot()) ? readdirSync(dragRoot()) : []);

/** Waits for a drag folder that was not in `before`; returns its PNG files (name -> bytes). */
async function newDragFiles(before, expected) {
  const deadline = Date.now() + 15_000;
  for (;;) {
    const folder = dragFolders().find((name) => !before.includes(name));
    if (folder) {
      const names = readdirSync(join(dragRoot(), folder)).filter((n) => n.endsWith('.png'));
      if (names.length >= expected) {
        return new Map(names.map((name) => [name, readFileSync(join(dragRoot(), folder, name))]));
      }
    }
    if (Date.now() > deadline) throw new Error(`no drag folder with ${expected} PNG file(s) appeared`);
    await sleep(100);
  }
}

/** Presses on the handle of the card `index`, moves away and lets go. */
async function dragHandle(app, index) {
  const point = await app.evaluate(`(() => {
    const handle = document.querySelectorAll('.note-card .drag-handle')[${index}];
    handle.closest('.note-card').scrollIntoView({ block: 'center' });
    const r = handle.getBoundingClientRect();
    return { x: r.left + r.width / 2, y: r.top + r.height / 2 };
  })()`);
  await app.drag([point, { x: point.x - 20, y: point.y + 30 }, { x: point.x - 40, y: point.y + 60 }]);
}

export const dragJourneys = [
  {
    name: 'glisser des notes vers l’extérieur : poignée, fichiers PNG nommés comme les notes',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      mkdirSync(dragRoot(), { recursive: true });
      let titles = [];

      await check('chaque carte a une poignée avec son infobulle', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card .drag-handle').length === 7`, 'a handle on the 7 cards');
        const tooltips = await app.evaluate(`[...document.querySelectorAll('.note-card .drag-handle')].map((h) => h.title)`);
        if (!tooltips.every((tip) => tip === l('library.drag.handle'))) throw new Error(`unexpected tooltips ${tooltips}`);
        titles = await app.evaluate(`[...document.querySelectorAll('.note-card .card-title')].map((e) => e.textContent.trim())`);
      });

      await check('glisser une note par sa poignée : son PNG est rendu, nommé comme elle', async () => {
        const before = dragFolders();
        await dragHandle(app, 0);
        const files = await newDragFiles(before, 1);
        const [name, bytes] = [...files][0];
        if (!name.endsWith(`${titles[0]}.png`)) throw new Error(`"${name}" is not named after "${titles[0]}"`);
        if (!bytes.subarray(0, 8).equals(PNG_SIGNATURE) || bytes.length < 2000) throw new Error('not a real PNG');
      });

      await check('l’interface revient au repos et n’affiche aucune erreur', async () => {
        await app.waitFor(
          `!document.querySelector('.drag-status') && !document.querySelector('.drag-handle.busy') && !document.querySelector('[role="alert"]')`,
          'the interface at rest after the drag',
        );
      });

      await check('la sélection de deux notes : la poignée les emporte toutes les deux', async () => {
        await app.click('.note-card:nth-child(1) .card-check input');
        await app.click('.note-card:nth-child(2) .card-check input');
        await app.waitFor(`document.querySelectorAll('.note-card.selected').length === 2`, 'two selected cards');
        const tooltip = await app.evaluate(`document.querySelectorAll('.note-card .drag-handle')[0].title`);
        if (!tooltip.includes('2')) throw new Error(`the handle does not say how many notes: ${tooltip}`);
        const help = await app.evaluate(`document.querySelector('.selection-bar')?.textContent ?? ''`);
        if (!help.includes(l('library.drag.help'))) throw new Error('the selection bar does not explain the drag');
        const before = dragFolders();
        await dragHandle(app, 0);
        const files = await newDragFiles(before, 2);
        const names = [...files.keys()];
        for (const title of [titles[0], titles[1]]) {
          if (!names.some((n) => n.endsWith(`${title}.png`))) throw new Error(`no file for "${title}" among ${names}`);
        }
        for (const bytes of files.values()) {
          if (!bytes.subarray(0, 8).equals(PNG_SIGNATURE)) throw new Error('not a PNG');
        }
      });

      await check('les dossiers de glisser restent dans le dossier temporaire d’Ardoise, hors de la bibliothèque', async () => {
        if (dragRoot().startsWith(app.library)) throw new Error('drag files are inside the library');
      });
    },
  },
];
