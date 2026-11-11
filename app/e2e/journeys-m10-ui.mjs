// Milestone M10, part "ui" (review fixes): behaviour during a lesson. Esc in live mode, undoing
// « Clear the page », the editor's leave dialog, a guided tablet test that survives opening a note,
// and the library's scroll position after the editor. Same pattern as journeys-m8-page.mjs (labels
// from the translation files). Not wired into run.mjs: the integrator imports `uiJourneys` and
// runs them after merging (the live journeys need `--simulated-tablet`, see `liveArgs`).
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { openTab } from './side-panel.mjs';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const j = JSON.stringify;
const LIVE_CANVAS = 'canvas.live-canvas';
const canvasData = (field) => `document.querySelector(${j(LIVE_CANVAS)})?.dataset.${field}`;
const STROKES = '.editor g.ink path[data-stroke]';
const strokeCount = `document.querySelectorAll(${j(STROKES)}).length`;

/** The simulated tablet draws in live mode as soon as asked to. */
export const liveArgs = ['--simulated-tablet'];

/** Registers the simulated tablet (it starts unregistered), opens live mode from the tablet panel and waits for the first strokes. */
async function startLive(app, check) {
  const STATUS = '.tablet-status';
  const WIZARD = '.register-wizard';
  await check('enregistrer la tablette simulée', async () => {
    await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="notRegistered"]`)}) !== null`, 'the sidebar block');
    await app.click(STATUS);
    await app.waitFor(`document.querySelector('.tablet-panel button[data-action="register"]') !== null`, 'the register button');
    await app.click('.tablet-panel button[data-action="register"]');
    await app.waitFor(`document.querySelector(${j(`${WIZARD} [data-step="warning"]`)}) !== null`, 'the warning step');
    await app.click(`${WIZARD} button[data-action="continue"]`);
    await app.waitFor(`document.querySelector(${j(`${WIZARD} [data-step="done"]`)}) !== null`, 'the end of the registration', 60_000);
    await app.click(`${WIZARD} button[data-action="finish"]`);
    await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="connected"]`)}) !== null`, 'the end of the sync', 60_000);
  });
  await check('ouvrir le mode direct (tablette simulée)', async () => {
    await app.waitFor(`document.querySelector('.tablet-panel button[data-action="live"]:not([disabled])') !== null`, 'the live button');
    await app.click('.tablet-panel button[data-action="live"]');
    await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
    await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'strokes on the live canvas', 10_000);
  });
}

export const uiJourneys = [
  {
    name: 'mode direct : Échap ferme la projection ou demande avant de terminer',
    args: liveArgs,
    async run(app, check) {
      const l = (key) => text(app.language, key);
      await startLive(app, check);

      await check('Échap sans projection : une question, le direct continue', async () => {
        await app.key('Escape');
        await app.waitFor(`document.querySelector('[role="alertdialog"], [role="dialog"]') !== null`, 'the question');
        const title = await app.evaluate(`document.querySelector('[role="dialog"] h2')?.textContent`);
        if (title !== l('live.leave.title')) throw new Error(`unexpected title "${title}"`);
        await app.click('[data-action="leave-cancel"]');
        await app.waitFor(`document.querySelector('[role="dialog"]') === null`, 'the question closed');
        if (!(await app.evaluate(`document.querySelector('.live-view') !== null`))) throw new Error('live mode ended');
      });

      await check('Échap avec la projection : elle se ferme, le direct continue sans question', async () => {
        // The projector block is in the Projection tab of the side panel (M16).
        await openTab(app, 'projection');
        await app.click('button[data-action="project"]');
        await app.waitFor(`document.querySelector('button[data-action="stop-projecting"]') !== null`, 'the projection');
        await app.key('Escape');
        await app.waitFor(`document.querySelector('button[data-action="project"]') !== null`, 'the projection closed');
        if (await app.evaluate(`document.querySelector('[role="dialog"]') !== null`)) throw new Error('a question was asked');
      });

      await check('Échap puis « Terminer » : retour à la bibliothèque', async () => {
        await app.key('Escape');
        await app.waitFor(`document.querySelector('[data-action="leave-confirm"]') !== null`, 'the question');
        await app.click('[data-action="leave-confirm"]');
        await app.waitFor(`document.querySelector('.shell:not(.shell-hidden) input.search') !== null`, 'the library', 15_000);
      });
    },
  },
  {
    name: 'mode direct : annuler l’effacement de la page, et Espace ne l’efface pas',
    args: liveArgs,
    async run(app, check) {
      const l = (key) => text(app.language, key);
      await startLive(app, check);

      await check('« Effacer la page » vide la page et propose d’annuler', async () => {
        // The simulated pen goes on drawing right after the clearing, so the count never rests at 0: let a few
        // strokes build up, then an observer catches the drop (a poll could miss it).
        await app.waitFor(`Number(${canvasData('strokes')}) >= 3`, 'a few strokes', 30_000);
        await app.evaluate(`(() => {
          const canvas = document.querySelector(${j(LIVE_CANVAS)});
          let last = Number(canvas.dataset.strokes);
          window.__dropped = false;
          window.__before = last;
          new MutationObserver(() => {
            const now = Number(canvas.dataset.strokes);
            if (now < last) window.__dropped = true;
            last = now;
          }).observe(canvas, { attributes: true, attributeFilter: ['data-strokes'] });
        })()`);
        await app.click('button[data-action="clear-page"]');
        await app.waitFor('window.__dropped === true', 'the page cleared');
        await app.waitFor(`document.querySelector('button[data-action="undo-clear"]') !== null`, 'the undo button');
        const label = await app.evaluate(`document.querySelector('button[data-action="undo-clear"]').textContent.trim()`);
        if (label !== l('live.clearUndo')) throw new Error(`unexpected label "${label}"`);
      });

      await check('« Annuler l’effacement » rend le tracé', async () => {
        await app.click('button[data-action="undo-clear"]');
        await app.waitFor(`Number(${canvasData('strokes')}) >= window.__before`, 'the strokes back');
      });

      await check('Espace avec « Effacer la page » au focus : page suivante, rien d’effacé', async () => {
        await app.evaluate(`document.querySelector('button[data-action="clear-page"]').focus()`);
        await app.key(' ');
        await app.waitFor(`${canvasData('page')}?.startsWith('2/')`, 'the next page');
        await app.key('ArrowLeft');
        await app.waitFor(`${canvasData('page')}?.startsWith('1/')`, 'the first page');
        if (!(Number(await app.evaluate(canvasData('strokes'))) > 0)) throw new Error('the first page was cleared');
      });
    },
  },
  {
    name: 'éditeur : la boîte de dialogue de sortie garde le focus et le rend',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      await check('ouvrir une note et la modifier (tout sélectionner, supprimer)', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('.note-card .card-open');
        await app.waitFor(`${strokeCount} > 0`, 'the strokes of the note');
        await app.key('a', { ctrl: true });
        await app.key('Delete');
        await app.waitFor(`document.querySelector('.editor')?.dataset.dirty === 'yes'`, 'the note changed');
      });

      await check('« Bibliothèque » ouvre la question, Tab y reste', async () => {
        await app.click('.editor-top button', l('editor.back'));
        await app.waitFor(`document.querySelector('[role="alertdialog"]') !== null`, 'the question');
        for (let i = 0; i < 6; i++) {
          await app.key('Tab');
          if (!(await app.evaluate(`document.querySelector('[role="alertdialog"]').contains(document.activeElement)`))) {
            throw new Error('Tab left the dialog');
          }
        }
      });

      await check('« Rester » rend le focus au bouton « Bibliothèque »', async () => {
        await app.click('[role="alertdialog"] button', l('editor.leave.cancel'));
        await app.waitFor(`document.querySelector('[role="alertdialog"]') === null`, 'the question closed');
        const label = await app.evaluate(`document.activeElement?.textContent ?? ''`);
        if (!label.includes(l('editor.back'))) throw new Error(`the focus is on "${label}"`);
      });

      await check('Échap répond « Rester » même sans focus dans la boîte', async () => {
        await app.click('.editor-top button', l('editor.back'));
        await app.waitFor(`document.querySelector('[role="alertdialog"]') !== null`, 'the question');
        await app.evaluate(`document.activeElement?.blur()`);
        await app.key('Escape');
        await app.waitFor(`document.querySelector('[role="alertdialog"]') === null`, 'the question closed');
      });

      await check('« Revenir sans enregistrer » ramène à la bibliothèque', async () => {
        await app.click('.editor-top button', l('editor.back'));
        await app.click('[role="alertdialog"] button', l('editor.leave.discard'));
        await app.waitFor(`document.querySelector('input.search') !== null && document.querySelector('.shell:not(.shell-hidden)') !== null`, 'the library');
      });
    },
  },
  {
    name: 'bibliothèque : ouvrir une note ne détruit ni le test guidé, ni le défilement',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      let scroll = 0;

      await check('ouvrir le test de la tablette (panneau latéral)', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('button', l('diagnostic.open'));
        await app.waitFor(`document.querySelector('aside.panel') !== null`, 'the diagnostic panel');
      });

      await check('faire défiler la liste, ouvrir une note', async () => {
        scroll = await app.evaluate(`(() => { const s = document.querySelector('.library-scroll'); s.scrollTop = 40; return s.scrollTop; })()`);
        await app.evaluate(`window.__panel = document.querySelector('aside.panel')`);
        await app.click('.note-card .card-open');
        await app.waitFor(`document.querySelector('.editor') !== null`, 'the editor');
      });

      await check('revenir : le même panneau, le même défilement, le focus sur la carte', async () => {
        await app.click('.editor-top button', l('editor.back'));
        await app.waitFor(`document.querySelector('.shell:not(.shell-hidden) input.search') !== null`, 'the library');
        if (!(await app.evaluate(`document.querySelector('aside.panel') === window.__panel`))) {
          throw new Error('the panel was built again');
        }
        const now = await app.evaluate(`document.querySelector('.library-scroll').scrollTop`);
        if (scroll > 0 && now !== scroll) throw new Error(`the scroll went from ${scroll} to ${now}`);
        const onCard = await app.evaluate(`document.activeElement?.classList.contains('card-open') === true`);
        if (!onCard) throw new Error('the focus did not return to the card');
      });
    },
  },
];
