// Journey "guide": the « Guide d'utilisation » button, the Aide tab's entry, the settings' Assistant button and F1, on the
// production executable, in French and in English.
//
// What only the real executable can show: `open_guide` finds the page from the executable's own folder (the journey's copy of
// `ardoise.exe` sits in the run's temporary folder) and falls back to GitHub when the `guide` folder, or the page, is missing.
// No browser is ever started: `ARDOISE_OPEN_LOG` (`guide.rs`) makes the command write what it would open to a file, which
// the journey reads back. The `guide` folder is made, and unmade, next to the copy while the app runs: the command looks
// again at each call.
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { makeTemp } from './temp-dirs.mjs';
import { closeEditor, text } from './journeys-m12-resume.mjs';
import { openTab } from './side-panel.mjs';

const GITHUB = 'https://github.com/Belenos-Toutatis/ardoise/blob/main/docs/guide/';
const NOTICE = '[data-slot="guide-notice"]';

/** Where this run's copy of the app writes what it would open (made by `env`, read by `run`). */
let openLog = '';

/** The lines the app has "opened" so far. */
const opened = () => (existsSync(openLog) ? readFileSync(openLog, 'utf8').split(/\r?\n/).filter(Boolean) : []);

/** Waits for one more line in the log and returns it. */
async function nextOpened(app, before, what) {
  const deadline = Date.now() + 10_000;
  for (;;) {
    const lines = opened();
    if (lines.length > before) return lines[before];
    if (Date.now() > deadline) throw new Error(`nothing was opened for ${what}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
}

/** A real F1 key press in the window (the key table of `run.mjs` does not know F1). */
async function pressF1(app) {
  const base = { key: 'F1', code: 'F1', windowsVirtualKeyCode: 112, modifiers: 0 };
  await app.send('Input.dispatchKeyEvent', { type: 'keyDown', ...base });
  await app.send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
}

const noticeText = (app) => app.evaluate(`document.querySelector(${JSON.stringify(NOTICE)})?.textContent?.trim() ?? null`);

async function dismissNotice(app) {
  if (await noticeText(app)) {
    await app.click(`${NOTICE} [data-action="dismiss-guide-notice"]`);
    await app.waitFor(`!document.querySelector(${JSON.stringify(NOTICE)})`, 'the notice closed');
  }
}

export const guideJourneys = [
  {
    name: 'guide : bouton, onglet Aide, Réglages et F1 ouvrent le guide local, ou celui de GitHub si le dossier manque',
    env: () => ({ ARDOISE_OPEN_LOG: (openLog = join(makeTemp('ardoise-guide-log-'), 'opened.log')) }),
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const lang = app.language;
      const folder = join(app.dir, 'guide');
      const page = (stem) => join(folder, `${stem}.${lang}.html`);
      const makeGuide = () => {
        mkdirSync(folder, { recursive: true });
        for (const stem of ['guide', 'assistant']) writeFileSync(page(stem), '<!doctype html><title>guide</title>');
      };

      await check('sans dossier « guide » à côté de l’exe : le bouton ouvre le guide de GitHub, dans la langue, et le dit', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the library');
        const button = 'header.topbar button[data-action="open-guide"]';
        if ((await app.evaluate(`document.querySelector(${JSON.stringify(button)}).getAttribute('aria-label')`)) !== l('guide.label')) {
          throw new Error('accessible name of the button');
        }
        await app.click(button);
        const line = await nextOpened(app, 0, 'the button without a guide folder');
        if (line !== `${GITHUB}guide.${lang}.md`) throw new Error(`opened "${line}"`);
        await app.waitFor(`document.querySelector(${JSON.stringify(NOTICE)})`, 'the web notice');
        if (!(await noticeText(app)).includes(l('guide.web'))) throw new Error(`notice "${await noticeText(app)}"`);
        await dismissNotice(app);
      });

      await check('avec le dossier « guide » à côté de l’exe : le bouton ouvre la page locale, sans message', async () => {
        makeGuide();
        await app.click('header.topbar button[data-action="open-guide"]');
        const line = await nextOpened(app, 1, 'the button with the guide folder');
        if (line !== page('guide')) throw new Error(`opened "${line}", wanted "${page('guide')}"`);
        await new Promise((resolve) => setTimeout(resolve, 300));
        if (await noticeText(app)) throw new Error('a notice for a local page');
      });

      await check('F1 ouvre la page locale, aussi dans l’éditeur ; l’Aide de l’éditeur a son bouton', async () => {
        await pressF1(app);
        const first = await nextOpened(app, 2, 'F1 in the library');
        if (first !== page('guide')) throw new Error(`F1: "${first}"`);
        await app.click('.note-card .card-open');
        await app.waitFor(`document.querySelector('.editor svg.canvas')`, 'the editor');
        await pressF1(app);
        const second = await nextOpened(app, 3, 'F1 in the editor');
        if (second !== page('guide')) throw new Error(`F1 in the editor: "${second}"`);
        // Still in the editor: F1 did not close or change anything.
        await app.waitFor(`document.querySelector('.editor svg.canvas')`, 'the editor still open');
        await openTab(app, 'help');
        const label = await app.evaluate(
          `document.querySelector('[data-tab-panel="help"] [data-section="guide"] button[data-action="open-guide"]')?.textContent?.trim()`,
        );
        if (label !== l('guide.open')) throw new Error(`button "${label}"`);
        const f1 = await app.evaluate(`document.querySelector('[data-tab-panel="help"] dt[data-shortcut="guide"]')?.textContent?.replace(/\\s+/g, ' ').trim()`);
        if (f1 !== 'F1') throw new Error(`F1 in the shortcut list: "${f1}"`);
        await app.click('[data-tab-panel="help"] [data-section="guide"] button[data-action="open-guide"]');
        const third = await nextOpened(app, 4, 'the Aide tab button');
        if (third !== page('guide')) throw new Error(`Aide button: "${third}"`);
        await closeEditor(app);
      });

      await check('Réglages > Assistant : le bouton ouvre le guide « Assistant » de la langue', async () => {
        await app.click('header button.ghost', l('settings.title'));
        await app.waitFor(`document.querySelector('[data-slot="assistant"]')`, 'the Assistant section');
        await app.click('[data-action="open-assistant-guide"]');
        const line = await nextOpened(app, 5, 'the Assistant button');
        if (line !== page('assistant')) throw new Error(`Assistant: "${line}"`);
      });

      await check('une page qui est un dossier n’est pas une page : le guide de GitHub s’ouvre (guide d’abord, assistant ensuite)', async () => {
        rmSync(page('assistant'));
        mkdirSync(page('assistant'));
        await app.click('[data-action="open-assistant-guide"]');
        const line = await nextOpened(app, 6, 'a folder in place of the page');
        if (line !== `${GITHUB}assistant.${lang}.md`) throw new Error(`opened "${line}"`);
        await app.waitFor(`document.querySelector(${JSON.stringify(NOTICE)})`, 'the web notice');
        await dismissNotice(app);
        // The other page is still the local one.
        await pressF1(app);
        const local = await nextOpened(app, 7, 'F1 after the Assistant page went');
        if (local !== page('guide')) throw new Error(`F1: "${local}"`);
      });

      await check('rien n’a été ouvert d’autre : huit lignes dans le journal, aucun navigateur démarré', async () => {
        const lines = opened();
        if (lines.length !== 8) throw new Error(`${lines.length} lines: ${lines.join(' | ')}`);
      });
    },
  },
];
