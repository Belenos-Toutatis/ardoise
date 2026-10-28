// M8 journey "position of the tablet" (not wired into run.mjs: the integrator adds
// `...orientationJourneys` after the other journeys and runs it).
//
// On the production executable with the simulated tablet (`--simulated-tablet`, which streams a pen
// in live mode): the tablet panel offers the four positions, the live window's quick switch changes
// the position at once, the choice is saved in the settings (the panel shows it) and the live page
// goes on drawing after the switch. The geometry itself (corners, page size, the live page turned
// with its ink) is checked by the Rust and vitest tests; what only the real tablet can tell (where
// its button is) is in docs/hardware-tests.md.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

export const orientationJourneys = [
  {
    name: 'position de la tablette : quatre positions, changement aussitôt en mode direct',
    args: ['--simulated-tablet'],
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const j = JSON.stringify;
      const STATUS = '.tablet-status';
      const WIZARD = '.register-wizard';
      const LIVE_CANVAS = 'canvas.live-canvas';
      const canvasData = (field) => `document.querySelector(${j(LIVE_CANVAS)})?.dataset.${field}`;
      const radios = `document.querySelectorAll('.tablet-panel input[data-option="orientation"]')`;
      const pressed = `document.querySelector('[data-slot="orientation"] button[aria-pressed="true"]')?.dataset.orientation`;

      await check('la tablette simulée est enregistrée', async () => {
        await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="notRegistered"]`)}) !== null`, 'the sidebar block');
        await app.click(STATUS);
        await app.waitFor(`document.querySelector('.tablet-panel button[data-action="register"]') !== null`, 'the register button');
        await app.click('.tablet-panel button[data-action="register"]');
        await app.waitFor(`document.querySelector(${j(`${WIZARD} [data-step="warning"]`)}) !== null`, 'the warning step');
        await app.click(`${WIZARD} button[data-action="continue"]`);
        await app.waitFor(`document.querySelector(${j(`${WIZARD} [data-step="done"]`)}) !== null`, 'the end of the registration', 60_000);
        await app.click(`${WIZARD} button[data-action="finish"]`);
        await app.waitFor(`['connected', 'syncing'].includes(document.querySelector(${j(STATUS)}).dataset.connection)`, 'the connection', 30_000);
      });

      await check('le panneau propose les quatre positions, paysage d’abord', async () => {
        await app.waitFor(`${radios}.length === 4`, 'the four positions');
        const values = await app.evaluate(`[...${radios}].map((r) => r.value)`);
        if (values.join() !== 'landscape,landscapeFlipped,portraitLeft,portraitRight') throw new Error(`positions: ${values}`);
        const checked = await app.evaluate(`[...${radios}].find((r) => r.checked)?.value`);
        if (checked !== 'landscape') throw new Error(`checked position: ${checked}`);
        const named = await app.evaluate(
          `document.querySelector('.tablet-panel').textContent.includes(${j(l('tablet.orientation.portraitLeft'))})`,
        );
        if (!named) throw new Error('the positions are not named');
      });

      await check('mode direct : la fenêtre de commande offre les quatre positions, paysage actif', async () => {
        await app.waitFor(
          `document.querySelector(${j(`${STATUS}[data-connection="connected"]`)}) !== null`,
          'the end of the sync',
          30_000,
        );
        await app.click('.tablet-panel button[data-action="live"]');
        await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
        await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'strokes on the live canvas', 10_000);
        const buttons = await app.evaluate(`document.querySelectorAll('[data-slot="orientation"] button').length`);
        if (buttons !== 4) throw new Error(`${buttons} position buttons`);
        if ((await app.evaluate(pressed)) !== 'landscape') throw new Error('landscape is not the active position');
      });

      await check('portrait, bouton à gauche : appliqué aussitôt, le tracé continue', async () => {
        await app.click('[data-slot="orientation"] button[data-orientation="portraitLeft"]');
        await app.waitFor(`${pressed} === 'portraitLeft'`, 'the position to switch');
        const before = Number(await app.evaluate(canvasData('strokes')));
        await app.waitFor(`Number(${canvasData('strokes')}) > ${before}`, 'strokes after the switch', 15_000);
      });

      await check('paysage à l’envers puis retour : le réglage est enregistré', async () => {
        await app.click('[data-slot="orientation"] button[data-orientation="landscapeFlipped"]');
        await app.waitFor(`${pressed} === 'landscapeFlipped'`, 'the flipped position');
        await app.click('[data-slot="orientation"] button[data-orientation="landscape"]');
        await app.waitFor(`${pressed} === 'landscape'`, 'landscape again');
      });

      await check('arrêter le mode direct, le panneau montre la position choisie en dernier', async () => {
        await app.click('[data-slot="orientation"] button[data-orientation="portraitRight"]');
        await app.waitFor(`${pressed} === 'portraitRight'`, 'portrait, button on the right');
        await app.click('button[data-action="stop-live"]');
        await app.waitFor(`document.querySelector('input.search') !== null`, 'the library');
        await app.click(STATUS);
        await app.waitFor(`${radios}.length === 4`, 'the panel');
        await app.waitFor(`[...${radios}].find((r) => r.checked)?.value === 'portraitRight'`, 'the saved position');
        // Back to the default for the journeys that follow.
        await app.click('.tablet-panel input[data-option="orientation"][value="landscape"]');
      });
    },
  },
];
