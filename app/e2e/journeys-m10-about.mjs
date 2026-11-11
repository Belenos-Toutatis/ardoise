// Milestone M10, part "about": the « À propos » screen of Settings, on the production executable.
// Same pattern as journeys-m8-page.mjs (labels from the translation files). Not wired into
// run.mjs: the integrator imports `aboutJourneys` and runs them after merging.
//
// What only the real executable can show: the version of the manifest, the licence and the
// notices embedded in the exe (not the browser stand-in's), and the list of the real components.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const DIALOG = '[role="dialog"][aria-modal="true"]';
const rows = `document.querySelectorAll(${JSON.stringify(`${DIALOG} tr[data-component]`)}).length`;
const slot = (name) => `document.querySelector(${JSON.stringify(`${DIALOG} [data-slot="${name}"]`)})?.textContent?.trim()`;

/** Types into the component filter (replacing what is there). */
async function filter(app, query) {
  await app.click(`${DIALOG} input[type="search"]`);
  await app.key('a', { ctrl: true });
  await app.key('Backspace');
  if (query) await app.type(query);
}

export const aboutJourneys = [
  {
    name: 'Réglages > À propos : version, licence GPL, crédits Tuhi, mention Cisco, composants et textes complets',
    async run(app, check) {
      const l = (key) => text(app.language, key);

      await check('ouvrir les Réglages : la section « À propos » donne la version et la licence', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the library');
        await app.click('header button.ghost', l('settings.title'));
        await app.waitFor(`document.querySelector('[data-slot="about-version"]')`, 'the About section of Settings');
        const version = await app.evaluate(`document.querySelector('[data-slot="about-version"]').textContent`);
        if (!/\d+\.\d+\.\d+/.test(version)) throw new Error(`no version in "${version}"`);
      });

      await check('le bouton ouvre l’écran ; il se lit au clavier (focus dans la fenêtre)', async () => {
        // The settings panel may still grow above the button (the handwriting section fills in as the notes are
        // read, more slowly in a full run): a click that landed beside the moving button is made again.
        for (let attempt = 1; ; attempt++) {
          await app.click('[data-action="open-about"]');
          try {
            await app.waitFor(`document.querySelector(${JSON.stringify(DIALOG)})`, 'the About dialog', 4000);
            break;
          } catch (error) {
            if (attempt === 3) throw error;
          }
        }
        await app.waitFor(`document.querySelector(${JSON.stringify(DIALOG)}).contains(document.activeElement)`, 'the focus in the dialog');
        const title = await app.evaluate(`document.querySelector(${JSON.stringify(`${DIALOG} h2`)}).textContent`);
        if (title !== l('about.title')) throw new Error(`title "${title}"`);
      });

      await check('licence GPL-3.0-or-later en clair, crédit Tuhi, mention Cisco et état d’OpenH264', async () => {
        if (!(await app.evaluate(slot('license'))).includes('GPL-3.0-or-later')) throw new Error('GPL-3.0-or-later missing');
        if ((await app.evaluate(slot('license-meaning'))) !== l('about.license.meaning')) throw new Error('plain sentence');
        if (!(await app.evaluate(slot('tuhi'))).includes('Tuhi')) throw new Error('Tuhi credit');
        if ((await app.evaluate(slot('cisco-notice'))) !== 'OpenH264 Video Codec provided by Cisco Systems, Inc.') {
          throw new Error('Cisco notice');
        }
        if (!(await app.evaluate(slot('cisco-download')))) throw new Error('download sentence');
        await app.waitFor(slot('cisco-status'), 'the OpenH264 status');
      });

      await check('le texte complet de la licence (GNU GPL v3) s’affiche et se masque', async () => {
        await app.click('[data-action="toggle-license"]');
        await app.waitFor(`${slot('license-text')}?.includes('GNU GENERAL PUBLIC LICENSE')`, 'the GPL text');
        const version3 = await app.evaluate(slot('license-text'));
        if (!version3.includes('Version 3, 29 June 2007')) throw new Error('not the GPL version 3');
        await app.click('[data-action="toggle-license"]');
        await app.waitFor(`!document.querySelector('[data-slot="license-text"]')`, 'the text hidden');
      });

      let total;
      await check('la liste des composants (Rust et npm) est celle de THIRD-PARTY-NOTICES', async () => {
        await app.waitFor(`${rows} > 200`, 'the component list');
        total = await app.evaluate(rows);
        const count = await app.evaluate(slot('component-count'));
        if (!count.includes(String(total))) throw new Error(`"${count}" for ${total} rows`);
        for (const name of ['tauri', 'svelte', 'openh264']) {
          if (!(await app.evaluate(`!!document.querySelector('tr[data-component="${name}"]')`))) throw new Error(`${name} missing`);
        }
      });

      await check('le filtre réduit la liste (« mpl »), puis la rétablit', async () => {
        await filter(app, 'mpl');
        await app.waitFor(`${rows} > 0 && ${rows} < ${total}`, 'the filtered list');
        const licences = await app.evaluate(
          `[...document.querySelectorAll(${JSON.stringify(`${DIALOG} tr[data-component]`)})].every((r) => /mpl/i.test(r.textContent))`,
        );
        if (!licences) throw new Error('a row does not match the filter');
        await filter(app, 'zzzz-introuvable');
        await app.waitFor(`${rows} === 0 && ${slot('no-component')}`, 'the empty message');
        await filter(app, '');
        await app.waitFor(`${rows} === ${total}`, 'the whole list');
      });

      await check('les textes complets des licences des composants s’affichent', async () => {
        await app.click('[data-action="toggle-notices"]');
        await app.waitFor(`${slot('notices-text')}?.includes('THIRD-PARTY NOTICES')`, 'the notices');
        const notices = await app.evaluate(slot('notices-text'));
        for (const needle of ['Tuhi', 'OpenH264 Video Codec provided by Cisco Systems, Inc.']) {
          if (!notices.includes(needle)) throw new Error(`"${needle}" missing from the notices`);
        }
      });

      await check('Échap ferme l’écran et redonne le focus au bouton', async () => {
        await app.key('Escape');
        await app.waitFor(`!document.querySelector(${JSON.stringify(DIALOG)})`, 'the dialog closed');
        await app.waitFor(`document.activeElement?.dataset?.action === 'open-about'`, 'the focus back on the button');
      });
    },
  },
];
