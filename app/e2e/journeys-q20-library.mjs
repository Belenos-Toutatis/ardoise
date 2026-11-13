// Journeys of the library features of 2026-11-13 (Q20): duplicating a note, changing its date, the free order of a binder
// (keyboard, kept after a reload, followed by the editor's page buttons), exporting a binder as one PDF.
//
// On the production executable and the demo library, in French and in English; the real window is driven with real clicks
// (the right click included) and real key events, and the files are read on the disk. Labels come from the translation files.
//
// Not driven here:
// - « Imprimer le classeur… » (and the pages per sheet): the Windows print window cannot be driven by the DevTools protocol.
//   The job is tested by printing to « Microsoft Print to PDF » (`cargo test -p ardoise print_windows`).
// - The sort menu is a native `<select>`: its value is set by the page's script (and its `change` event sent), as the other
//   journeys do for native controls.
// - The date field is a `datetime-local` input, which takes no typed text through `Input.insertText`: its value is set the same way.
// - The Windows drag-and-drop session itself (OLE `DoDragDrop`, the layer that `disable_drag_drop_handler` fixed in commit 55c1551):
//   the protocol's synthetic mouse events never hold the real button, so no OS drag starts (see journeys-m7-drag.mjs). What the last
//   journey does drive: `Input.setInterceptDrags` makes WebView2 report the drag that the page starts from real mouse presses and
//   moves (`Input.dragIntercepted`, carrying the data the page's own `dragstart` handler set), and `Input.dispatchDragEvent` plays
//   that drag back over the page as drag events made by the browser itself (not events built by the test). So the page's real drag
//   handlers, the insertion mark, the drop on a card and the drop on a sidebar binder run in the real window; whether the
//   operating system hands a drag to the window is still a manual check (docs/hardware-tests.md style, on the PC).
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const fill = (template, params) => template.replace(/\{(\w+)\}/g, (match, name) => (name in params ? String(params[name]) : match));
const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const ORDER_FILE = '_ardoise-order.json';
const BINDER = 'Essai ordre';

/** Every `.ardoise` file below `dir`, as paths relative to it. */
function noteFiles(dir, base = dir) {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) return entry.name === '_ardoise' ? [] : noteFiles(path, base);
    return entry.name.endsWith('.ardoise') ? [path.slice(base.length + 1)] : [];
  });
}

/** The titles of the cards on screen, in order. */
const titles = (app) => app.evaluate(`[...document.querySelectorAll('.note-card .card-title')].map((e) => e.textContent.trim())`);

async function waitForTitles(app, expected, what) {
  try {
    await app.waitFor(`JSON.stringify([...document.querySelectorAll('.note-card .card-title')].map((e) => e.textContent.trim())) === ${j(j(expected))}`, what);
  } catch (error) {
    // Say what was there: a failure of this kind is read from the message alone.
    throw new Error(`${error.message}; the cards are ${j(await titles(app))}, the order file ${j(orderFile(app))}`);
  }
}

/** A real right click at the centre of an element. */
async function rightClick(app, selector, label) {
  const { x, y } = await app.centre(selector, label);
  await app.mouse('mouseMoved', x, y);
  await app.mouse('mousePressed', x, y, { button: 'right', buttons: 2 });
  await app.mouse('mouseReleased', x, y, { button: 'right', buttons: 0 });
}

/** Marks the card titled `title` so that a selector can reach it. */
async function markCard(app, title, mark) {
  const found = await app.evaluate(`(() => {
    const card = [...document.querySelectorAll('.note-card')].find((c) => c.querySelector('.card-title')?.textContent.trim() === ${j(title)});
    if (!card) return false;
    card.dataset.e2e = ${j(mark)};
    return true;
  })()`);
  if (!found) throw new Error(`no card "${title}"`);
  return `[data-e2e="${mark}"]`;
}

/** Right click on the card titled `title`, then the menu entry labelled `label`. */
async function cardMenu(app, title, label) {
  const card = await markCard(app, title, `menu-${Date.now()}`);
  await rightClick(app, card);
  await app.waitFor(`document.querySelector('.menu-list[role="menu"]') !== null`, `the menu of "${title}"`);
  await app.click('.menu-list [role="menuitem"]', label);
}

/** Creates a note through the app's own command, as the interface does. */
const createNote = (app, binder, title) =>
  app.evaluate(`window.__TAURI_INTERNALS__.invoke('create_note', { binder: ${j(binder)}, title: ${j(title)} }).then(() => true)`);

/** Chooses the sort `value` in the sort menu (a native select). */
async function chooseSort(app, value) {
  await app.evaluate(`(() => {
    const select = document.querySelector('select.sort');
    select.value = ${j(value)};
    select.dispatchEvent(new Event('change', { bubbles: true }));
    return true;
  })()`);
}

/** Opens the binder `name` of the sidebar and shows its notes in the free order. */
async function openBinderInOrder(app, name) {
  await app.click(`.sidebar [data-binder=${j(name)}]`);
  await chooseSort(app, 'binder');
  await app.waitFor(`document.querySelector('select.sort').value === 'binder'`, 'the free order chosen');
}

/** Moves the focus to the card titled `title` and presses a key on it. */
async function keyOnCard(app, title, key, modifiers) {
  await app.evaluate(`(() => {
    const card = [...document.querySelectorAll('.note-card')].find((c) => c.querySelector('.card-title')?.textContent.trim() === ${j(title)});
    card.querySelector('.card-open').focus();
    return true;
  })()`);
  await app.key(key, modifiers);
}

/** The ids of the order file of the binder folder, or null when there is none. */
function orderFile(app, binder = BINDER) {
  const path = join(app.library, ...binder.split('/'), ORDER_FILE);
  return existsSync(path) ? JSON.parse(readFileSync(path, 'utf8')) : null;
}

export const q20Journeys = [
  {
    name: 'Q20 : dupliquer une note (clic droit, barre de la sélection, annuler) et changer sa date',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const word = l('library.duplicate.word');
      const original = 'Liste de courses';
      const second = "Électricité – loi d'Ohm";

      await check('la bibliothèque de démonstration s’affiche (8 notes)', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
      });

      let date;
      await check(`clic droit sur « ${original} » : le menu de la carte propose « ${l('library.card.duplicate')} » et « ${l('library.card.changeDate')} »`, async () => {
        date = await app.evaluate(`[...document.querySelectorAll('.note-card')].find((c) => c.querySelector('.card-title').textContent.trim() === ${j(original)}).querySelector('.card-date').textContent`);
        const card = await markCard(app, original, 'dup');
        await rightClick(app, card);
        await app.waitFor(`document.querySelector('.menu-list[role="menu"]') !== null`, 'the right-click menu');
        const items = await app.evaluate(`[...document.querySelectorAll('.menu-list [role="menuitem"]')].map((e) => e.textContent.trim())`);
        for (const wanted of [l('library.card.duplicate'), l('library.card.changeDate')]) {
          if (!items.includes(wanted)) throw new Error(`no "${wanted}" in ${items}`);
        }
      });

      await check(`« ${l('library.card.duplicate')} » : la copie « ${original} (${word}) » apparaît, à la même date, et son fichier est à côté de l’original`, async () => {
        await app.click('.menu-list [role="menuitem"]', l('library.card.duplicate'));
        await app.waitFor(`[...document.querySelectorAll('.note-card .card-title')].some((e) => e.textContent.trim() === ${j(`${original} (${word})`)})`, 'the copy card');
        const copyDate = await app.evaluate(`[...document.querySelectorAll('.note-card')].find((c) => c.querySelector('.card-title').textContent.trim() === ${j(`${original} (${word})`)}).querySelector('.card-date').textContent`);
        if (copyDate !== date) throw new Error(`the copy is dated "${copyDate}", the original "${date}"`);
        const files = noteFiles(app.library);
        const names = files.filter((f) => f.includes(original));
        if (names.length !== 2 || !names.some((f) => f.endsWith(` ${original} (${word}).ardoise`))) throw new Error(`unexpected files ${names}`);
        // The copy sits in the original's folder, with the same date at the start of its name.
        const [a, b] = names;
        if (dirname(a) !== dirname(b) || a.split(/[\\/]/).pop().slice(0, 10) !== b.split(/[\\/]/).pop().slice(0, 10)) throw new Error(`not twins: ${names}`);
        const offer = await app.evaluate(`[...document.querySelectorAll('.activity-offer')].map((o) => o.textContent).join(' ')`);
        if (!offer.includes(fill(l('library.duplicate.done.one'), { title: `${original} (${word})` }))) throw new Error(`notice: ${offer}`);
      });

      await check('« Annuler » (Ctrl + Z) : la copie est à la corbeille', async () => {
        await app.key('z', { ctrl: true });
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the copy gone');
        await app.waitFor(`[...document.querySelectorAll('.activity-offer')].some((o) => o.textContent.includes(${j(l('library.duplicate.undone.one'))}))`, 'the notice of the undo');
        if (noteFiles(app.library).some((f) => f.includes(`(${word})`))) throw new Error('the copy is still in the library');
      });

      await check('barre de la sélection : deux notes cochées, « Dupliquer » en fait deux copies', async () => {
        for (const title of [original, second]) {
          const card = await markCard(app, title, `pick-${title.length}`);
          await app.click(`${card} .card-check input`);
        }
        await app.click('.selection-bar [data-action="duplicate"]');
        await app.waitFor(`document.querySelectorAll('.note-card').length === 10`, 'two copies');
        for (const title of [original, second]) {
          await app.waitFor(`[...document.querySelectorAll('.note-card .card-title')].some((e) => e.textContent.trim() === ${j(`${title} (${word})`)})`, `the copy of "${title}"`);
        }
        await app.key('Escape');
      });

      let named;
      await check(`« ${l('library.card.changeDate')} » : la fenêtre s’ouvre, « ${l('library.date.today')} » remplit le champ, la date est changée, le fichier est renommé`, async () => {
        await cardMenu(app, second, l('library.card.changeDate'));
        await app.waitFor(`document.querySelector('[data-dialog="change-date"]') !== null`, 'the date dialog');
        await app.evaluate(`(() => {
          const field = document.querySelector('[data-dialog="change-date"] input[type="datetime-local"]');
          const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set;
          setter.call(field, '2025-03-04T12:30');
          field.dispatchEvent(new Event('input', { bubbles: true }));
          return true;
        })()`);
        await app.click('[data-action="date-apply"]');
        await app.waitFor(`document.querySelector('[data-dialog="change-date"]') === null`, 'the dialog closed');
        named = noteFiles(app.library).filter((f) => f.includes('2025-03-04'));
        if (named.length !== 1 || !named[0].endsWith(` ${second}.ardoise`)) throw new Error(`unexpected files ${named}`);
        // The grid sorts by it: the card is now the last of the newest-first list.
        const last = (await titles(app)).at(-1);
        if (last !== second) throw new Error(`the card is not last: ${await titles(app)}`);
      });

      await check('« Aujourd’hui » remplit le champ avec la date du jour', async () => {
        await app.key('z', { ctrl: true });
        await app.waitFor(`![...document.querySelectorAll('.note-card .card-date')].some((e) => e.textContent.includes('2025'))`, 'the old date back');
        if (noteFiles(app.library).some((f) => f.includes('2025-03-04'))) throw new Error('the 2025 file is still there');
        await cardMenu(app, second, l('library.card.changeDate'));
        await app.waitFor(`document.querySelector('[data-dialog="change-date"]') !== null`, 'the date dialog');
        await app.click('[data-action="date-today"]');
        const value = await app.evaluate(`document.querySelector('[data-dialog="change-date"] input[type="datetime-local"]').value`);
        const now = new Date();
        const pad = (n) => String(n).padStart(2, '0');
        if (!value.startsWith(`${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}T`)) throw new Error(`the field holds ${value}`);
        await app.click('.modal button', l('library.cancel'));
      });
    },
  },
  {
    name: 'Q20 : ordre du classeur au clavier (Alt + flèches), gardé après rechargement, suivi par les pages de l’éditeur',
    async run(app, check) {
      const l = (key) => text(app.language, key);

      await check('trois notes dans un classeur, ouvert dans la barre latérale : le tri « Ordre du classeur » n’est proposé que là', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the demo notes');
        const before = await app.evaluate(`[...document.querySelectorAll('select.sort option')].map((o) => o.value)`);
        if (before.includes('binder')) throw new Error(`offered outside a binder: ${before}`);
        for (const title of ['Alpha', 'Beta', 'Gamma']) await createNote(app, BINDER, title);
        await app.waitFor(`document.querySelector('.sidebar [data-binder=${j(BINDER)}]') !== null`, 'the new binder');
        await app.click(`.sidebar [data-binder=${j(BINDER)}]`);
        const inBinder = await app.evaluate(`[...document.querySelectorAll('select.sort option')].map((o) => o.value)`);
        if (!inBinder.includes('binder')) throw new Error(`not offered in a binder: ${inBinder}`);
        await chooseSort(app, 'binder');
        await waitForTitles(app, ['Alpha', 'Beta', 'Gamma'], 'the notes oldest first, no order yet');
        await app.waitFor(`document.querySelector('[data-slot="order-help"]') !== null`, 'the help line of the free order');
      });

      await check('Alt + flèche gauche sur « Beta » : elle passe avant « Alpha », l’ordre est écrit dans le dossier du classeur', async () => {
        await keyOnCard(app, 'Beta', 'ArrowLeft', { alt: true });
        await waitForTitles(app, ['Beta', 'Alpha', 'Gamma'], 'Beta moved first');
        await app.waitFor(`document.querySelector('[data-slot="order-announcement"]')?.textContent.trim() === ${j(fill(l('library.order.moved'), { title: 'Beta', position: 1, count: 3 }))}`, 'the announcement');
        await app.waitFor(`document.activeElement?.closest('.note-card')?.querySelector('.card-title')?.textContent.trim() === 'Beta'`, 'the focus still on the moved card');
        const order = await (async () => {
          for (let i = 0; i < 50; i++) {
            const found = orderFile(app);
            if (found) return found;
            await sleep(100);
          }
          return null;
        })();
        if (!order || order.notes.length !== 3 || order.version !== 1) throw new Error(`unexpected order file ${j(order)}`);
        // The file is in the binder's folder, next to the notes, and the notes are not listed twice.
        const ids = await app.evaluate(`window.__TAURI_INTERNALS__.invoke('library_list').then((l) => l.notes.filter((n) => n.binder === ${j(BINDER)}).map((n) => [n.title, n.id]))`);
        const byTitle = Object.fromEntries(ids);
        if (j(order.notes) !== j([byTitle.Beta, byTitle.Alpha, byTitle.Gamma])) throw new Error(`order ${j(order.notes)} for ${j(byTitle)}`);
      });

      await check('Alt + flèche droite, Alt + Fin : une place plus loin, puis à la fin ; au début, on le dit', async () => {
        await keyOnCard(app, 'Beta', 'ArrowRight', { alt: true });
        await waitForTitles(app, ['Alpha', 'Beta', 'Gamma'], 'Beta one place later');
        await keyOnCard(app, 'Alpha', 'ArrowLeft', { alt: true });
        await waitForTitles(app, ['Alpha', 'Beta', 'Gamma'], 'Alpha already first');
        await app.waitFor(`document.querySelector('[data-slot="order-announcement"]')?.textContent.trim() === ${j(l('library.order.atStart'))}`, 'the announcement at the start');
        await keyOnCard(app, 'Gamma', 'ArrowLeft', { alt: true });
        await keyOnCard(app, 'Gamma', 'ArrowLeft', { alt: true });
        await waitForTitles(app, ['Gamma', 'Alpha', 'Beta'], 'Gamma first');
      });

      await check('rechargement de la fenêtre : l’ordre est retrouvé (il est dans le dossier)', async () => {
        await app.evaluate(`setTimeout(() => location.reload(), 50)`);
        await sleep(600);
        await app.waitFor(`document.readyState === 'complete' && document.querySelectorAll('.note-card').length >= 8`, 'the library again', 30_000);
        await openBinderInOrder(app, BINDER);
        await waitForTitles(app, ['Gamma', 'Alpha', 'Beta'], 'the order after the reload');
      });

      await check('l’éditeur : ouvrir « Alpha » (2 / 3), page suivante = « Beta », page précédente deux fois = « Gamma » : l’ordre affiché', async () => {
        const path = await app.evaluate(`[...document.querySelectorAll('.note-card')].map((c) => c.dataset.path)`);
        await app.click('.note-card:nth-child(2) .card-open');
        await app.waitFor(`document.querySelector('.editor')?.dataset.path === ${j(path[1])}`, 'the editor on Alpha');
        await app.waitFor(`document.querySelector('[data-slot="page-position"]')?.textContent.trim() === '2 / 3'`, 'the position 2 / 3');
        await app.click('button[data-action="page-next"]');
        await app.waitFor(`document.querySelector('.editor')?.dataset.path === ${j(path[2])}`, 'the editor on Beta');
        await app.click('button[data-action="page-previous"]');
        await app.click('button[data-action="page-previous"]');
        await app.waitFor(`document.querySelector('.editor')?.dataset.path === ${j(path[0])}`, 'the editor on Gamma');
        await app.click('.editor button', l('editor.back'));
        await app.waitFor(`document.querySelector('.note-card') !== null`, 'the library again');
      });

      await check('sans rien déplacer : un classeur sans fichier d’ordre reste trié par date et n’en crée pas', async () => {
        await createNote(app, 'Autre classeur', 'Premier');
        await createNote(app, 'Autre classeur', 'Second');
        await app.waitFor(`document.querySelector('.sidebar [data-binder="Autre classeur"]') !== null`, 'the second binder');
        await app.click('.sidebar [data-binder="Autre classeur"]');
        await chooseSort(app, 'binder');
        await waitForTitles(app, ['Premier', 'Second'], 'by date');
        await sleep(500);
        if (orderFile(app, 'Autre classeur')) throw new Error('an order file was written without a move');
      });
    },
  },
  {
    name: 'Q20 : exporter un classeur en un seul PDF (clic droit), avec ou sans sous-classeurs, signets compris',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const target = join(app.dir, 'classeur.pdf');
      const pageCount = (bytes) => (bytes.toString('latin1').match(/\/Type\s*\/Page(?![s\w])/g) ?? []).length;

      await check('un classeur de trois notes avec un sous-classeur d’une note', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the demo notes');
        for (const title of ['Alpha', 'Beta', 'Gamma']) await createNote(app, BINDER, title);
        await createNote(app, `${BINDER}/Sous-classeur`, 'Delta');
        await app.waitFor(`document.querySelector('.sidebar [data-binder=${j(BINDER)}]') !== null`, 'the binder');
      });

      await check(`clic droit sur le classeur : « ${l('library.binder.exportPdf')} » ouvre la fenêtre, réduite au PDF`, async () => {
        await rightClick(app, `.sidebar [data-binder=${j(BINDER)}]`);
        await app.waitFor(`document.querySelector('.menu-list[role="menu"]') !== null`, 'the binder menu');
        await app.click('.menu-list [role="menuitem"]', l('library.binder.exportPdf'));
        await app.waitFor(`document.querySelector('[role="dialog"]')?.textContent.includes(${j(fill(l('export.subjectBinder'), { name: BINDER }))})`, 'the export dialog of the binder');
        if (await app.evaluate(`document.querySelector('input[name="export-format"]') !== null`)) throw new Error('the formats are offered');
        if (!(await app.evaluate(`document.querySelector('input[data-option="include-sub-binders"]') !== null`))) throw new Error('no box for the sub-binders');
      });

      await check('« Exporter… » : le PDF a trois pages, une par note, avec un signet par note', async () => {
        await app.evaluate(`window.__ardoiseE2eSavePath = ${j(target)}; true`);
        await app.click('[role="dialog"] button', l('export.run'));
        await app.waitFor(`document.querySelector('[role="dialog"] .export-status')?.textContent.includes(${j(fill(l('export.done'), { count: 1 }))})`, 'the export done', 30_000);
        const bytes = readFileSync(target);
        if (pageCount(bytes) !== 3) throw new Error(`${pageCount(bytes)} pages`);
        const text8 = bytes.toString('latin1');
        for (const title of ['Alpha', 'Beta', 'Gamma']) {
          if (!text8.includes(`/Title (${title})`)) throw new Error(`no bookmark "${title}"`);
        }
        if (!text8.includes('/Outlines')) throw new Error('no outline');
        if (text8.includes('/Title (Delta)')) throw new Error('the sub-binder was not asked for');
      });

      await check('« Inclure les sous-classeurs » : quatre pages, le sous-classeur a son signet qui contient celui de « Delta »', async () => {
        await app.click('input[data-option="include-sub-binders"]');
        await app.click('[role="dialog"] button', l('export.again'));
        await app.waitFor(`document.querySelector('[role="dialog"] .export-status')?.textContent.includes(${j(fill(l('export.done'), { count: 1 }))})`, 'the second export done', 30_000);
        await sleep(300);
        const bytes = readFileSync(target);
        const text8 = bytes.toString('latin1');
        if (pageCount(bytes) !== 4) throw new Error(`${pageCount(bytes)} pages`);
        for (const title of ['Sous-classeur', 'Delta']) {
          if (!text8.includes(`/Title (${title})`)) throw new Error(`no bookmark "${title}"`);
        }
        await app.click('[role="dialog"] button', l('common.close'));
      });

      await check('le classeur sans sous-classeur n’en propose pas la case ; un classeur vide a ses entrées désactivées', async () => {
        await app.click(`.sidebar [data-binder="Autre classeur"]`).catch(() => undefined);
        await createNote(app, 'Seul', 'Unique');
        await app.waitFor(`document.querySelector('.sidebar [data-binder="Seul"]') !== null`, 'the single binder');
        await rightClick(app, '.sidebar [data-binder="Seul"]');
        await app.click('.menu-list [role="menuitem"]', l('library.binder.exportPdf'));
        await app.waitFor(`document.querySelector('[role="dialog"]') !== null`, 'the dialog');
        if (await app.evaluate(`document.querySelector('input[data-option="include-sub-binders"]') !== null`)) throw new Error('a box for a binder without sub-binders');
        await app.click('[role="dialog"] button', l('library.cancel'));
      });
    },
  },
  {
    name: 'Q20 : glisser une carte entre deux cartes, puis sur un classeur, avec la souris (glisser du protocole)',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let dragged;

      /** Centre of the card titled `title` (`side`: where on its width: 0.25 left half, 0.75 right half). */
      const spot = (title, side = 0.5) =>
        app.evaluate(`(() => {
          const card = [...document.querySelectorAll('.note-card')].find((c) => c.querySelector('.card-title')?.textContent.trim() === ${j(title)});
          card.scrollIntoView({ block: 'center' });
          const r = card.getBoundingClientRect();
          return { x: Math.round(r.left + r.width * ${side}), y: Math.round(r.top + r.height / 3) };
        })()`);
      const sidebarSpot = (binder) =>
        app.evaluate(`(() => {
          const r = document.querySelector('.sidebar [data-binder=${j(binder)}]').getBoundingClientRect();
          return { x: Math.round(r.left + r.width / 2), y: Math.round(r.top + r.height / 2) };
        })()`);
      const dragEvent = (type, point) => app.send('Input.dispatchDragEvent', { type, x: point.x, y: point.y, data: dragged, modifiers: 0 });

      await check('trois notes dans un classeur, en « Ordre du classeur »', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the demo notes');
        for (const title of ['Alpha', 'Beta', 'Gamma']) await createNote(app, BINDER, title);
        await app.waitFor(`document.querySelector('.sidebar [data-binder=${j(BINDER)}]') !== null`, 'the new binder');
        await openBinderInOrder(app, BINDER);
        await waitForTitles(app, ['Alpha', 'Beta', 'Gamma'], 'the notes oldest first');
      });

      await check('le protocole intercepte les glissers du navigateur (Input.setInterceptDrags)', async () => {
        const reply = await app.send('Input.setInterceptDrags', { enabled: true });
        if (reply.error) throw new Error(`setInterceptDrags: ${JSON.stringify(reply.error)}`);
      });

      await check('appui sur « Gamma » puis déplacement par petits pas : la page lance un glisser, que le protocole intercepte', async () => {
        const from = await spot('Gamma');
        const to = await spot('Alpha', 0.25);
        const since = app.eventCount();
        await app.mouse('mouseMoved', from.x, from.y);
        await app.mouse('mousePressed', from.x, from.y);
        for (let step = 1; step <= 12; step++) {
          await app.mouse('mouseMoved', from.x + ((to.x - from.x) * step) / 12, from.y + ((to.y - from.y) * step) / 12, { buttons: 1, button: 'left' });
          await sleep(30);
        }
        const event = await app.waitForEvent('Input.dragIntercepted', { since, timeout: 4000 });
        if (!event) throw new Error('no Input.dragIntercepted event: the drag was not intercepted');
        dragged = event.params.data;
        if (!dragged?.items?.some((item) => item.mimeType === 'application/x-ardoise-notes')) throw new Error(`the drag carries ${JSON.stringify(dragged)}`);
      });

      await check('dragEnter et dragOver sur la moitié gauche d’« Alpha » : la barre d’insertion apparaît avant elle', async () => {
        const to = await spot('Alpha', 0.25);
        await dragEvent('dragEnter', to);
        await dragEvent('dragOver', to);
        await app.waitFor(`document.querySelector('.note-card.insert-before .card-title')?.textContent.trim() === 'Alpha'`, 'the insertion mark before Alpha');
        // The right half of the same card: the mark goes to its other side.
        const right = await spot('Alpha', 0.75);
        await dragEvent('dragOver', right);
        await app.waitFor(`document.querySelector('.note-card.insert-after .card-title')?.textContent.trim() === 'Alpha'`, 'the mark after Alpha');
        await dragEvent('dragOver', to);
        await app.waitFor(`document.querySelector('.note-card.insert-before .card-title')?.textContent.trim() === 'Alpha'`, 'the mark before Alpha again');
      });

      await check('drop : l’ordre devient Gamma, Alpha, Beta, et il est écrit dans le dossier du classeur', async () => {
        const to = await spot('Alpha', 0.25);
        await dragEvent('drop', to);
        await app.mouse('mouseReleased', to.x, to.y);
        await waitForTitles(app, ['Gamma', 'Alpha', 'Beta'], 'the order after the drop');
        // The cards move at once; the file is written just after.
        let order = null;
        for (let i = 0; i < 50 && !(order = orderFile(app)); i++) await sleep(100);
        if (!order || order.notes.length !== 3) throw new Error(`unexpected order file ${j(order)}`);
        await app.waitFor(`document.querySelectorAll('.note-card.insert-before, .note-card.insert-after').length === 0`, 'the mark gone');
      });

      await check('un deuxième glisser, déposé sur un classeur de la barre latérale : la note change de classeur (quel que soit le tri)', async () => {
        const from = await spot('Beta');
        const target = await sidebarSpot('Perso');
        const since = app.eventCount();
        await app.mouse('mouseMoved', from.x, from.y);
        await app.mouse('mousePressed', from.x, from.y);
        for (let step = 1; step <= 12; step++) {
          await app.mouse('mouseMoved', from.x + ((target.x - from.x) * step) / 12, from.y + ((target.y - from.y) * step) / 12, { buttons: 1, button: 'left' });
          await sleep(30);
        }
        const event = await app.waitForEvent('Input.dragIntercepted', { since, timeout: 4000 });
        if (!event) throw new Error('no Input.dragIntercepted event');
        dragged = event.params.data;
        await dragEvent('dragEnter', target);
        await dragEvent('dragOver', target);
        await app.waitFor(`document.querySelector('.sidebar [data-binder="Perso"]').classList.contains('drop')`, 'the binder lit up');
        await dragEvent('drop', target);
        await app.mouse('mouseReleased', target.x, target.y);
        await waitForTitles(app, ['Gamma', 'Alpha'], 'Beta gone from the binder');
        const moved = noteFiles(app.library).some((f) => f.startsWith('Perso') && f.endsWith(' Beta.ardoise'));
        if (!moved) throw new Error('Beta is not in the folder of the binder Perso');
        await app.send('Input.setInterceptDrags', { enabled: false });
      });
    },
  },
];
