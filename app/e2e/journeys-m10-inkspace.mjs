// Inkspace import from the interface (after the M10 review): automatic detection, import into the binder
// « Inkspace », a second import that adds nothing, a folder chosen by hand, stopping an import, and the offer of
// the first-run wizard. Same pattern as journeys-m8-page.mjs (labels from the translation files).
// Not wired into run.mjs: the integrator imports `inkspaceJourneys` and runs them after merging.
//
// Inkspace's data: run.mjs points LOCALAPPDATA to the run's folder, so the journeys put a folder where the real
// detection looks (`<run>\LocalAppData\Packages\D91E29CF.InkspaceApp_e2e\LocalState`). By default it holds the four
// made-up notes of `ardoise.exe --create-demo-inkspace`. With ARDOISE_E2E_INKSPACE set to a LocalState folder (the
// user's copy, testdata\private\inkspace\LocalState: 22 notes), its note store (`WacomCloudDb` only, never `db\`) is
// copied there instead; the copy lives in the run's temporary folder, removed after a passing run.
import { createHash } from 'node:crypto';
import { cpSync, existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

/** Notes of `ardoise.exe --create-demo-inkspace`. */
const DEMO_NOTES = 4;

/** Where the app's detection looks in this run. */
const detectedFolder = (app) => join(app.dir, 'LocalAppData', 'Packages', 'D91E29CF.InkspaceApp_e2e', 'LocalState');

/** Puts Inkspace data in `localState`; returns how many notes it holds (null: count unknown, a private copy). */
function makeInkspace(app, localState) {
  const copy = process.env.ARDOISE_E2E_INKSPACE;
  if (copy) {
    mkdirSync(localState, { recursive: true });
    cpSync(join(copy, 'WacomCloudDb'), join(localState, 'WacomCloudDb'), { recursive: true });
    return null;
  }
  mkdirSync(dirname(localState), { recursive: true });
  execFileSync(join(app.dir, 'ardoise.exe'), ['--create-demo-inkspace', localState], { stdio: 'pipe' });
  return DEMO_NOTES;
}

/** SHA-256 of every file under `dir`, to prove Inkspace's folder was not modified. */
function fingerprint(dir) {
  const hash = createHash('sha256');
  const walk = (folder) => {
    for (const entry of readdirSync(folder, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      const path = join(folder, entry.name);
      hash.update(path);
      if (entry.isDirectory()) walk(path);
      else hash.update(readFileSync(path));
    }
  };
  walk(dir);
  return hash.digest('hex');
}

/** Every file of a binder; each must be a complete note (a ZIP named `.ardoise`, no temporary file). */
function binderNotes(app, binder) {
  const folder = join(app.library, binder);
  if (!existsSync(folder)) return [];
  const files = readdirSync(folder);
  for (const name of files) {
    if (!name.endsWith('.ardoise')) throw new Error(`unexpected file in the binder: ${name}`);
    const head = readFileSync(join(folder, name)).subarray(0, 2).toString('latin1');
    if (head !== 'PK' || statSync(join(folder, name)).size < 100) throw new Error(`incomplete note: ${name}`);
  }
  return files;
}

const dialog = `document.querySelector('.modal')`;
const slot = (name) => `document.querySelector('[data-slot="${name}"]')`;
const submit = `document.querySelector('.modal button[type="submit"]')`;

/** Opens the dialog from the sidebar and waits until the folder is read. */
async function openDialog(app) {
  await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the library');
  await app.click('[data-action="import-inkspace"]');
  await app.waitFor(dialog, 'the import dialog');
  await app.waitFor(`${slot('inkspace-count')} || ${slot('inkspace-not-found')}`, 'the folder read', 20_000);
}

/** The numbers of the count line: found, then already imported (when any), then unreadable (when any). */
const counts = `[...(${slot('inkspace-count')}?.textContent.matchAll(/\\d+/g) ?? [])].map((m) => Number(m[0]))`;

export const inkspaceJourneys = [
  {
    name: 'importer les notes d’Inkspace trouvées sur ce PC, une seule fois, puis depuis un dossier choisi',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const localState = detectedFolder(app);
      let expected = makeInkspace(app, localState);
      const before = fingerprint(localState);

      await check(`« ${l('inkspace.entry')} » trouve Inkspace et compte ses notes`, async () => {
        await openDialog(app);
        const shown = await app.evaluate(`${slot('inkspace-path')}?.textContent`);
        if (shown !== localState) throw new Error(`detected folder: ${shown}`);
        const [count] = await app.evaluate(counts);
        if (expected === null) expected = count;
        if (count !== expected) throw new Error(`${count} notes found, ${expected} expected`);
        const binder = await app.evaluate(`document.querySelector('.modal input[list]').value`);
        if (binder !== 'Inkspace') throw new Error(`default binder: ${binder}`);
      });

      await check('importer : progression, puis le bilan', async () => {
        await app.click('.modal button[type="submit"]');
        await app.waitFor(slot('inkspace-report'), 'the report', 120_000);
        const report = await app.evaluate(`${slot('inkspace-report')}.textContent`);
        const title = l('inkspace.doneTitle');
        if (!report.includes(title)) throw new Error(`report: ${report}`);
      });

      await check('les notes sont dans le classeur « Inkspace », complètes', async () => {
        const files = binderNotes(app, 'Inkspace');
        if (files.length !== expected) throw new Error(`${files.length} files, ${expected} expected`);
      });

      await check(`« ${l('inkspace.show')} » affiche le classeur`, async () => {
        await app.click('.modal .button.primary', l('inkspace.show'));
        await app.waitFor(`!${dialog}`, 'the dialog to close');
        await app.waitFor(
          `document.querySelector('[data-binder="Inkspace"]')?.getAttribute('aria-current') === 'true' && document.querySelectorAll('.note-card').length === ${expected}`,
          'the notes of the binder',
        );
      });

      await check('un deuxième import n’ajoute rien', async () => {
        await openDialog(app);
        // Every note found is already imported (the 22 notes of the development PC are all readable).
        const [found, already] = await app.evaluate(counts);
        if (found !== expected || already !== expected) throw new Error(`counts: ${found}, ${already}`);
        if (!(await app.evaluate(`${submit}?.disabled`))) throw new Error('the import button is enabled');
        await app.click('.modal button', l('library.cancel'));
        await app.waitFor(`!${dialog}`, 'the dialog to close');
      });

      await check('un dossier choisi à la main (copie) : les notes déjà importées sont reconnues', async () => {
        // The package folder above LocalState is accepted too.
        const copy = join(app.dir, 'Copie Inkspace');
        makeInkspace(app, join(copy, 'LocalState'));
        await app.evaluate(`window.__ardoiseE2eFolderPath = ${JSON.stringify(copy)}`);
        await openDialog(app);
        await app.click('.modal button', l('inkspace.pickOther'));
        await app.waitFor(
          `${slot('inkspace-path')}?.textContent === ${JSON.stringify(join(copy, 'LocalState'))} && ${slot('inkspace-count')}`,
          'the chosen folder read',
          20_000,
        );
        const [found, already] = await app.evaluate(counts);
        if (found !== expected || already !== expected || !(await app.evaluate(`${submit}?.disabled`))) {
          throw new Error(`notes of the copy are not recognised as imported: ${found}, ${already}`);
        }
        await app.click('.modal button', l('library.cancel'));
      });

      await check('un dossier qui n’est pas celui d’Inkspace est refusé avec une phrase traduite', async () => {
        await app.evaluate(`window.__ardoiseE2eFolderPath = ${JSON.stringify(app.library)}`);
        await openDialog(app);
        await app.click('.modal button', l('inkspace.pickOther'));
        await app.waitFor(`document.querySelector('.modal [role="alert"]')`, 'the error');
        const alert = await app.evaluate(`document.querySelector('.modal [role="alert"]').textContent`);
        const sentence = l('errorCodes.inkspaceNotFound').split(' (')[0];
        if (!alert.includes(sentence)) throw new Error(`error: ${alert}`);
        await app.click('.modal button', l('library.cancel'));
      });

      await check('le dossier d’Inkspace n’a pas changé', async () => {
        if (fingerprint(localState) !== before) throw new Error('Inkspace’s folder was modified');
      });
    },
  },
  {
    name: 'arrêter un import en cours ne laisse aucune note à moitié écrite',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const expected = makeInkspace(app, detectedFolder(app));

      await check('lancer l’import dans le classeur « Arrêt » puis l’arrêter aussitôt', async () => {
        await openDialog(app);
        await app.evaluate(
          `(() => { const i = document.querySelector('.modal input[list]'); i.value = 'Arrêt'; i.dispatchEvent(new Event('input', { bubbles: true })); })()`,
        );
        await app.click('.modal button[type="submit"]');
        // The stop button exists only while the import runs; a small folder may finish first.
        await app.click('.modal button', l('inkspace.stop')).catch(() => {});
        await app.waitFor(slot('inkspace-report'), 'the report', 120_000);
      });

      await check('chaque note écrite est complète', async () => {
        binderNotes(app, 'Arrêt');
        await app.click('.modal button', l('common.close'));
      });

      await check('relancer l’import ajoute le reste, sans doublon', async () => {
        await openDialog(app);
        if (!(await app.evaluate(`${submit}?.disabled`))) {
          await app.evaluate(
            `(() => { const i = document.querySelector('.modal input[list]'); i.value = 'Arrêt'; i.dispatchEvent(new Event('input', { bubbles: true })); })()`,
          );
          await app.click('.modal button[type="submit"]');
          await app.waitFor(slot('inkspace-report'), 'the report', 120_000);
        }
        const files = binderNotes(app, 'Arrêt');
        if (expected !== null && files.length !== expected) throw new Error(`${files.length} notes, ${expected} expected`);
      });
    },
  },
  {
    name: 'l’assistant du premier lancement propose l’import quand Inkspace est trouvé',
    async run(app, check) {
      const expected = makeInkspace(app, detectedFolder(app));

      await check('sans bibliothèque, l’assistant propose d’importer les notes trouvées', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the library');
        // This PC forgets its library: the next start of the interface is the first-run wizard.
        writeFileSync(join(app.dir, 'ardoise.toml'), `language = "${app.language}"\n`);
        await app.send('Page.reload');
        await app.waitFor(slot('wizard-inkspace'), 'the Inkspace offer', 20_000);
        const offer = await app.evaluate(`${slot('wizard-inkspace')}.textContent`);
        if (expected !== null && !offer.includes(String(expected))) throw new Error(`offer: ${offer}`);
        const checked = await app.evaluate(`${slot('wizard-inkspace')}.querySelector('input[type="checkbox"]').checked`);
        if (!checked) throw new Error('the offer is not checked by default');
      });
    },
  },
];
