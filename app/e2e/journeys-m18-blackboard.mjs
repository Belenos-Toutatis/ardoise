// Milestone M18: the Xiaomi blackboard, the second tablet, through the interface.
//
// On the production executable with the simulated blackboard (`--simulated-blackboard`: a blackboard in range with
// three reference pages, battery 90 %, that writes by itself in live mode and whose store button is pressed with
// `sim_blackboard_store_page`; its file is a throwaway folder, never the library), in French and in English
// (`run.mjs` runs every journey in both). The Slate is not simulated here: the library knows no tablet at first.
// Contract: docs/design/blackboard.md §5 and §6. Labels come from the translation files.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const j = JSON.stringify;
const STATUS = '.tablet-status';
const PANEL = '.tablet-panel';
const ADD = '.add-blackboard';
const LIVE_CANVAS = 'canvas.live-canvas';
const canvasData = (field) => `document.querySelector(${j(LIVE_CANVAS)})?.dataset.${field}`;
const cards = (condition) => `document.querySelectorAll('.note-card').length ${condition}`;
const connection = (state) => `document.querySelector(${j(`${STATUS}[data-connection="${state}"]`)}) !== null`;
const step = (name) => `document.querySelector(${j(`${ADD} [data-step="${name}"]`)}) !== null`;
const invoke = (app, command) => app.evaluate(`window.__TAURI_INTERNALS__.invoke(${j(command)}).then(() => true)`);
/** The paths of the notes of the library, and the size of a note's page (`27200x20500`): read through the commands, not the grid. */
const notePaths = (app) => app.evaluate(`window.__TAURI_INTERNALS__.invoke('library_list').then((listing) => listing.notes.map((n) => n.path))`);
const pageSizeOfNote = (app, path) =>
  app.evaluate(`window.__TAURI_INTERNALS__.invoke('open_note', { path: ${j(path)} }).then((opened) => opened.note.page.width + 'x' + opened.note.page.height)`);

/** A tab of the tablet panel (« use », « settings », « tablets »): the panel opens on « use ». */
const showTab = (app, id) => app.click(`${PANEL} [role="tab"][data-tab="${id}"]`);

/** The tablet panel is open (a click on the sidebar block opens it, a second one would close it). */
async function openPanel(app) {
  if (!(await app.evaluate(`document.querySelector(${j(PANEL)}) !== null`))) await app.click(STATUS);
  await app.waitFor(`document.querySelector(${j(PANEL)}) !== null`, 'the tablet panel');
}

export const blackboardJourneys = [
  {
    name: 'tablette Xiaomi simulée : ajout, pages, informations, mode direct, oubli',
    args: ['--simulated-blackboard'],
    async run(app, check) {
      const l = (key) => text(app.language, key);

      await check('la bibliothèque de démonstration s’affiche (8 notes), aucune tablette n’est connue', async () => {
        await app.waitFor(cards('=== 8'), 'the 8 demo notes');
        await app.waitFor(connection('notRegistered'), 'the sidebar block');
      });

      await check('le panneau propose l’enregistrement de la Bamboo Slate et l’ajout de la tablette Xiaomi', async () => {
        await app.click(STATUS);
        await app.waitFor(`document.querySelector(${j(`${PANEL} button[data-action="register"]`)}) !== null`, 'the register button');
        await app.waitFor(`document.querySelector(${j(`${PANEL} button[data-action="add-blackboard"]`)}) !== null`, 'the add button');
        const simulated = await app.evaluate(`document.querySelector(${j(PANEL)}).textContent.includes(${j(l('tablet.simulated'))})`);
        if (!simulated) throw new Error('the panel does not say the tablet is simulated');
      });

      await check('ajouter la tablette Xiaomi : pas de bouton à maintenir, il faut allumer la tablette et fermer Mi Home', async () => {
        await app.click(`${PANEL} button[data-action="add-blackboard"]`);
        await app.waitFor(step('intro'), 'the first screen');
        const body = await app.evaluate(`document.querySelector(${j(ADD)}).textContent`);
        for (const key of ['tablet.board.intro', 'tablet.board.introMiHome', 'tablet.board.introLinked']) {
          if (!body.includes(l(key))) throw new Error(`the sentence ${key} is missing`);
        }
        if (body.includes(l('tablet.register.step.holdButton.text'))) throw new Error('it asks to hold a button');
        if (await app.evaluate(`document.querySelector('.register-wizard') !== null`)) throw new Error('the Slate wizard is open');
      });

      await check('chercher : la tablette est entendue (une seule, sans signal), on l’ajoute', async () => {
        await app.click(`${ADD} button[data-action="search"]`);
        await app.waitFor(step('found'), 'the tablet heard', 30_000);
        const found = await app.evaluate(`document.querySelectorAll(${j(`${ADD} [data-slot="found"] li`)}).length`);
        if (found !== 1) throw new Error(`${found} tablets heard`);
        if (await app.evaluate(`document.querySelector(${j(`${ADD} [data-slot="signal"]`)}) !== null`)) {
          throw new Error('the signal is shown for a single tablet');
        }
        await app.click(`${ADD} button[data-action="add-blackboard"]`);
        await app.waitFor(step('added'), 'the end of the addition', 30_000);
        await app.click(`${ADD} button[data-action="finish"]`);
        await app.waitFor(`document.querySelector(${j(ADD)}) === null`, 'the dialog to close');
      });

      await check('la tablette Xiaomi est connue, sans Bamboo Slate : le panneau la liste et propose d’oublier celle-là seule', async () => {
        await showTab(app, 'tablets');
        await app.waitFor(
          `document.querySelectorAll(${j(`${PANEL} [data-slot="known-tablet"]`)}).length === 1`,
          'one known tablet',
        );
        const kind = await app.evaluate(`document.querySelector(${j(`${PANEL} [data-slot="known-tablet"]`)}).dataset.kind`);
        if (kind !== 'blackboard') throw new Error(`the known tablet is a ${kind}`);
        const forgets = await app.evaluate(`document.querySelectorAll(${j(`${PANEL} button[data-action="forget"]`)}).length`);
        if (forgets !== 1) throw new Error(`${forgets} forget buttons`);
        // The Slate can still be registered, the Xiaomi cannot be added twice.
        if (await app.evaluate(`document.querySelector(${j(`${PANEL} button[data-action="add-blackboard"]`)}) !== null`)) {
          throw new Error('the panel still offers to add the Xiaomi tablet');
        }
        if (await app.evaluate(`document.querySelector(${j(`${PANEL} button[data-action="register"]`)}) === null`)) {
          throw new Error('the panel no longer offers the Slate');
        }
      });

      await check('elle se connecte seule et ses trois pages deviennent des notes : 11 notes dans la grille', async () => {
        await app.waitFor(cards('=== 11'), 'the 3 pages in the grid', 60_000);
      });

      await check('le résultat le dit : 3 notes enregistrées, 3 pages effacées de la tablette, 0 page restante', async () => {
        await showTab(app, 'use');
        await app.waitFor(connection('connected'), 'the end of the sync', 30_000);
        const saved = l('tablet.sync.savedMany').replace('{count}', '3');
        const deleted = l('tablet.sync.deletedMany').replace('{count}', '3');
        await app.waitFor(
          `(() => { const t = document.querySelector(${j(`${PANEL} [data-slot="last-sync"]`)})?.textContent ?? ''; return t.includes(${j(saved)}) && t.includes(${j(deleted)}); })()`,
          'the result of the sync',
          30_000,
        );
        await app.waitFor(
          `document.querySelector(${j(`${PANEL} [data-slot="stored-pages"]`)})?.dataset.count === '0'`,
          'no page left on the tablet',
        );
      });

      await check('les informations : batterie 90 %, mémoire libre, 0 page, modèle ; ni nom, ni firmware, ni horloge', async () => {
        // The card of the first tab has the battery and the pages; the list of tablets has the rest.
        await showTab(app, 'tablets');
        const facts = await app.evaluate(`(() => {
          const card = document.querySelector(${j(`${PANEL} [aria-labelledby="tablet-info"]`)});
          const info = document.querySelector(${j(`${PANEL} [data-slot="known-tablet"][data-kind="blackboard"]`)});
          return {
            battery: card.querySelector('.battery')?.dataset.battery,
            charging: card.querySelector('.battery')?.dataset.charging,
            memory: info.querySelector('[data-slot="known-memory"]')?.textContent.trim() ?? null,
            text: info.textContent,
            name: card.querySelector('[data-slot="name"]') !== null,
            rename: document.querySelector(${j(`${PANEL} #tablet-name`)}) !== null,
            clock: document.querySelector(${j(`${PANEL} button[data-action="set-time"]`)}) !== null,
          };
        })()`);
        if (facts.battery !== '90') throw new Error(`battery ${facts.battery}`);
        if (facts.charging !== 'false') throw new Error('it says charging');
        if (!facts.memory || !/\d/.test(facts.memory)) throw new Error(`memory "${facts.memory}"`);
        if (!facts.text.includes('Xiaomi Mijia LCD blackboard')) throw new Error('the model is missing');
        if (facts.text.includes(l('tablet.info.firmware'))) throw new Error('the firmware is shown');
        if (facts.name || facts.rename || facts.clock) throw new Error('rename or clock settings are shown for the Xiaomi tablet');
        const position = await app.evaluate(`document.querySelector('[data-option="orientation"]') !== null`);
        if (position) throw new Error('the position of the tablet is offered without a Slate');
      });

      await check('réglages : les quatre positions de la tablette Xiaomi, en hauteur boutons en bas d’abord', async () => {
        await showTab(app, 'settings');
        const radios = `document.querySelectorAll(${j(`${PANEL} input[data-option="boardPosition"]`)})`;
        await app.waitFor(`${radios}.length === 4`, 'the four positions of the blackboard');
        const values = await app.evaluate(`[...${radios}].map((r) => r.value)`);
        if (values.join() !== 'buttonsBottom,buttonsTop,buttonsRight,buttonsLeft') throw new Error(`positions: ${values}`);
        const checked = await app.evaluate(`[...${radios}].find((r) => r.checked)?.value`);
        if (checked !== 'buttonsBottom') throw new Error(`checked position: ${checked}`);
        const body = await app.evaluate(`document.querySelector(${j(`${PANEL} [data-tab-panel="settings"]`)}).textContent`);
        for (const key of ['tablet.boardPosition.buttonsLeft', 'tablet.options.boardPositionHint']) {
          if (!body.includes(l(key))) throw new Error(`the text ${key} is missing`);
        }
        // Chosen and used in a step of its own, further down (« boutons à gauche »).
        await showTab(app, 'use');
      });

      await check('mode direct : la page est en hauteur, les quatre boutons de position de la Xiaomi (pas ceux de la Bamboo), le tracé de la tablette arrive', async () => {
        await showTab(app, 'use');
        await app.waitFor(`document.querySelector(${j(`${PANEL} button[data-action="live"]:not([disabled])`)}) !== null`, 'the live button');
        await app.click(`${PANEL} button[data-action="live"]`);
        await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
        await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'strokes on the live canvas', 20_000);
        await app.waitFor(`${canvasData('pageSize')} === '20500x27200'`, 'the portrait page of the blackboard');
        if (await app.evaluate(`document.querySelector('[data-slot="orientation"]') !== null || document.querySelector('.orientation-icon') !== null`)) {
          throw new Error('the Slate position switch is shown for the blackboard');
        }
        const board = await app.evaluate(`(() => {
          const group = document.querySelector('[data-slot="board-position"]');
          const buttons = [...(group?.querySelectorAll('button') ?? [])];
          return {
            label: group?.getAttribute('aria-label'),
            positions: buttons.map((b) => b.dataset.position).join(),
            drawings: buttons.filter((b) => b.querySelector('svg.board-drawing')?.dataset.position === b.dataset.position).length,
            pressed: buttons.filter((b) => b.getAttribute('aria-pressed') === 'true').map((b) => b.dataset.position).join(),
          };
        })()`);
        if (board.label !== l('live.xiaomi.label')) throw new Error(`unexpected label "${board.label}"`);
        if (board.positions !== 'buttonsBottom,buttonsTop,buttonsRight,buttonsLeft') throw new Error(`positions: ${board.positions}`);
        if (board.drawings !== 4) throw new Error(`${board.drawings} drawings of the blackboard`);
        if (board.pressed !== 'buttonsBottom') throw new Error(`pressed: ${board.pressed}`);
      });

      // Q19 (docs/design/blackboard.md §10, §11): the switch turns the page and its ink at once (a quarter turn swaps the
      // page's shape) and saves the setting; back upright, as the next steps expect. The simulated hand keeps writing.
      await check('mode direct : cliquer « boutons à gauche » tourne la page et ce qui y est écrit, et l’enregistre ; puis retour en hauteur', async () => {
        const switchTo = (position) => app.click(`[data-slot="board-position"] button[data-position="${position}"]`);
        const pressed = `document.querySelector('[data-slot="board-position"] button[aria-pressed="true"]')?.dataset.position`;
        const saved = `window.__TAURI_INTERNALS__.invoke('tablet_status').then((status) => status.options.boardPosition)`;
        const before = Number(await app.evaluate(canvasData('strokes')));
        if (!(before > 0)) throw new Error('no ink to turn');
        await switchTo('buttonsLeft');
        await app.waitFor(`${canvasData('pageSize')} === '27200x20500'`, 'the page turned to landscape', 10_000);
        await app.waitFor(`${pressed} === 'buttonsLeft'`, 'the position pressed');
        const turned = Number(await app.evaluate(canvasData('strokes')));
        if (turned < before) throw new Error(`${turned} strokes after the switch, ${before} before`);
        await app.waitFor(`${saved}.then((position) => position === 'buttonsLeft')`, 'the position saved in the settings');
        if (!(await app.evaluate(`document.querySelector('.live-view[data-state="live"]') !== null`))) {
          throw new Error('live mode is no longer live');
        }
        await switchTo('buttonsBottom');
        await app.waitFor(`${canvasData('pageSize')} === '20500x27200'`, 'the page upright again', 10_000);
        await app.waitFor(`${pressed} === 'buttonsBottom'`, 'the position pressed again');
        await app.waitFor(`${saved}.then((position) => position === 'buttonsBottom')`, 'the starting position saved again');
        const back = Number(await app.evaluate(canvasData('strokes')));
        if (back < turned) throw new Error(`${back} strokes after switching back, ${turned} before`);
      });

      // The store button is pressed inside this step on purpose: nothing the window shows changes when it is pressed (the
      // simulated hand writes for 600 s, and the tablet's page count is not read again during live mode), so a step of
      // its own could not fail. The real check is the notes: the stored page is a 13th note, imported after live mode.
      await check('enregistrer une page avec le bouton pendant le direct, terminer : la note du direct, puis la page enregistrée sont importées', async () => {
        const before = Number(await app.evaluate(canvasData('strokes')));
        await invoke(app, 'sim_blackboard_store_page');
        await app.waitFor(`Number(${canvasData('strokes')}) > ${before}`, 'strokes after the page was stored', 20_000);
        if (!(await app.evaluate(`document.querySelector('.live-view[data-state="live"]') !== null`))) {
          throw new Error('live mode is no longer live');
        }
        await app.click('button[data-action="stop-live"]');
        await app.waitFor(`document.querySelector('.shell:not(.shell-hidden) input.search') !== null`, 'the library', 30_000);
        await app.waitFor(cards('>= 12'), 'the live note in the grid', 30_000);
        // The page stored during live mode is read once nothing else runs (a safe duplicate of the live note).
        await app.waitFor(cards('>= 13'), 'the stored page imported afterwards', 60_000);
      });

      // H8 (docs/design/blackboard.md §9.5): with « delete after import » off the blackboard may not be synced first, so live
      // mode is refused while it holds a page and the window asks to import it. The refusal comes as the `refused` event
      // (the page count is read again when live mode is asked) or as the rejected `live_start`: the dialog is the same.
      await check('mode direct avec une page sur la tablette et l’effacement après import désactivé : la question d’import, annuler, puis importer et démarrer', async () => {
        const deleteOption = `${PANEL} input[data-option="deleteAfterImport"]`;
        const dialog = '[data-slot="pages-first"]';
        await openPanel(app);
        await app.waitFor(connection('connected'), 'the blackboard idle again', 60_000);
        await showTab(app, 'settings');
        await app.click(deleteOption);
        await app.waitFor(`document.querySelector(${j(deleteOption)})?.checked === false`, 'the option switched off');
        const before = Number(await app.evaluate(`document.querySelectorAll('.note-card').length`));
        await invoke(app, 'sim_blackboard_store_page');
        await showTab(app, 'use');
        await app.click(`${PANEL} button[data-action="live"]`);
        await app.waitFor(`document.querySelector(${j(dialog)}) !== null`, 'the question about the stored page', 30_000);
        const pages = Number(await app.evaluate(`document.querySelector(${j(dialog)}).dataset.pages`));
        if (!(pages >= 1)) throw new Error(`${pages} pages announced`);
        const sentence = pages === 1 ? l('errorCodes.blackboardPagesFirstOne') : l('errorCodes.blackboardPagesFirst').replace('{count}', String(pages));
        const shown = await app.evaluate(`document.querySelector(${j(`${dialog} [data-slot="pages-first-text"]`)}).textContent.trim()`);
        if (shown !== sentence) throw new Error(`unexpected sentence "${shown}"`);
        if (await app.evaluate(`document.body.textContent.includes('{count}')`)) throw new Error('a {count} reached the screen');
        for (const action of ['pages-first-import', 'pages-first-cancel']) {
          if (!(await app.evaluate(`document.querySelector(${j(`button[data-action="${action}"]`)}) !== null`))) throw new Error(`no ${action} button`);
        }
        if (await app.evaluate(`document.querySelector('[data-slot="start-failed"]') !== null`)) throw new Error('the start banner is shown');

        // Escape cancels: the library and the tablet panel come back, the focus on « Live mode », nothing was imported.
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${j(dialog)}) === null && document.querySelector('.shell:not(.shell-hidden)') !== null`, 'the library after cancelling', 30_000);
        await app.waitFor(`document.querySelector(${j(PANEL)}) !== null`, 'the tablet panel again');
        await app.waitFor(`document.activeElement?.dataset?.action === 'live'`, 'the focus on « Live mode »');
        if (await app.evaluate(cards(`> ${before}`))) throw new Error('a page was imported by cancelling');

        // Again, then import and delete: the page becomes a note and live mode starts.
        await app.waitFor(`document.querySelector(${j(`${PANEL} button[data-action="live"]:not([disabled])`)}) !== null`, 'the live button');
        await app.click(`${PANEL} button[data-action="live"]`);
        await app.waitFor(`document.querySelector(${j(dialog)}) !== null`, 'the question again', 30_000);
        await app.click('button[data-action="pages-first-import"]');
        await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode after the import', 90_000);
        await app.waitFor(`document.querySelector(${j(dialog)}) === null`, 'the dialog to close');
        await app.waitFor(cards(`>= ${before + 1}`), 'the imported page as a note', 30_000);
        await app.click('button[data-action="stop-live"]');
        await app.waitFor(`document.querySelector('.shell:not(.shell-hidden) input.search') !== null`, 'the library', 30_000);

        // The option back on, as it was.
        await openPanel(app);
        await showTab(app, 'settings');
        await app.click(deleteOption);
        await app.waitFor(`document.querySelector(${j(deleteOption)})?.checked === true`, 'the option switched on again');
        await showTab(app, 'use');
      });

      // Q17 (docs/design/blackboard.md §11): the position is kept, a page stored in it is turned upright (landscape on the
      // side), and so is the live page, which starts in it. Put back at the end, for the next steps.
      await check('position « boutons à gauche » : retenue, la page enregistrée devient une note en largeur, le direct aussi ; retour en position de départ', async () => {
        const radios = `${PANEL} input[data-option="boardPosition"]`;
        const chosen = `[...document.querySelectorAll(${j(radios)})].find((r) => r.checked)?.value`;
        const saved = `window.__TAURI_INTERNALS__.invoke('tablet_status').then((status) => status.options.boardPosition)`;
        const choose = async (value) => {
          await openPanel(app);
          await showTab(app, 'settings');
          await app.click(`${radios}[value="${value}"]`);
          await app.waitFor(`${chosen} === ${j(value)}`, `the position ${value} to be chosen`);
          await app.waitFor(`${saved}.then((position) => position === ${j(value)})`, `the position ${value} to be saved`);
        };
        await openPanel(app);
        await app.waitFor(connection('connected'), 'the blackboard idle again', 60_000);
        await choose('buttonsLeft');

        // Kept: the panel closed and opened again shows it, the Slate's position did not move.
        await app.click(STATUS);
        await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the panel to close');
        await openPanel(app);
        await showTab(app, 'settings');
        await app.waitFor(`${chosen} === 'buttonsLeft'`, 'the position kept after reopening the panel');
        if (await app.evaluate(`document.querySelector(${j(`${PANEL} input[data-option="orientation"]`)}) !== null`)) {
          throw new Error('the position of the Slate is offered without a Slate');
        }

        // The screen erased first: the same screen stored again would only update the note of its earlier store, made
        // upright. Live mode starts on the landscape page, with the blackboard's switch on « buttons on the left », never
        // the Slate's (the quarter turns are not the Slate's orientation), and the simulated pen writes the new screen.
        await showTab(app, 'use');
        await invoke(app, 'sim_blackboard_erase');
        await app.waitFor(`document.querySelector(${j(`${PANEL} button[data-action="live"]:not([disabled])`)}) !== null`, 'the live button');
        await app.click(`${PANEL} button[data-action="live"]`);
        await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
        await app.waitFor(`${canvasData('pageSize')} === '27200x20500'`, 'the landscape page of the blackboard');
        await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'strokes on the landscape live page', 20_000);
        if (await app.evaluate(`document.querySelector('[data-slot="orientation"]') !== null || document.querySelector('[data-slot="resume-hold"]') !== null`)) {
          throw new Error('the Slate position switch or its notice is shown for the blackboard');
        }
        const pressedLeft = await app.evaluate(`document.querySelector('[data-slot="board-position"] button[aria-pressed="true"]')?.dataset.position`);
        if (pressedLeft !== 'buttonsLeft') throw new Error(`the switch shows ${pressedLeft}`);
        await app.click('button[data-action="stop-live"]');
        await app.waitFor(`document.querySelector('.shell:not(.shell-hidden) input.search') !== null`, 'the library', 30_000);

        // That screen stored: a new page (the screen was erased before the run), imported by itself (automatic sync,
        // deletion on) as a landscape note.
        await openPanel(app);
        await app.waitFor(connection('connected'), 'the blackboard idle after live mode', 60_000);
        const before = await notePaths(app);
        await invoke(app, 'sim_blackboard_store_page');
        try {
          await app.waitFor(cards(`>= ${before.length + 1}`), 'the stored page as a note', 30_000);
        } catch {
          // Not imported by itself: asked for, which is the same import.
          await app.click(`${PANEL} button[data-action="sync"]`);
          await app.waitFor(cards(`>= ${before.length + 1}`), 'the stored page as a note after a sync', 60_000);
        }
        const added = (await notePaths(app)).filter((path) => !before.includes(path));
        if (added.length !== 1) throw new Error(`${added.length} new notes for one stored page`);
        const size = await pageSizeOfNote(app, added[0]);
        if (size !== '27200x20500') throw new Error(`the page of the new note is ${size}, not landscape`);

        // Back to the starting position, so that the next steps are as before.
        await openPanel(app);
        await app.waitFor(connection('connected'), 'the blackboard idle again', 60_000);
        await choose('buttonsBottom');
        await showTab(app, 'use');
      });

      await check('oublier la tablette Xiaomi : confirmation, puis plus aucune tablette connue', async () => {
        await openPanel(app);
        await showTab(app, 'tablets');
        await app.waitFor(`document.querySelector(${j(`${PANEL} button[data-action="forget"]`)}) !== null`, 'the forget button');
        await app.click(`${PANEL} button[data-kind="blackboard"][data-action="forget"]`);
        await app.waitFor(`document.querySelector(${j(`${PANEL} [role="alertdialog"]`)}) !== null`, 'the question');
        const question = await app.evaluate(`document.querySelector(${j(`${PANEL} [role="alertdialog"]`)}).textContent`);
        if (!question.includes(l('settings.boardForgetConfirm'))) throw new Error('unexpected question');
        await app.click(`${PANEL} button[data-action="forget-yes"]`);
        await app.waitFor(connection('notRegistered'), 'no tablet known');
        await app.waitFor(
          `document.querySelector(${j(`${PANEL} [data-slot="known-tablet"]`)}) === null && document.querySelector(${j(`${PANEL} button[data-action="add-blackboard"]`)}) !== null`,
          'the panel offering to add a tablet again',
        );
        // The notes stay.
        if (!(await app.evaluate(cards('>= 13')))) throw new Error('notes were lost');
      });
    },
  },
];
