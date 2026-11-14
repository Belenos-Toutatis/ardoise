// End-to-end journeys on the real executable (`cargo xtask e2e`).
//
// For each interface language: a portable copy of ardoise.exe in a temporary folder, a demo
// library created by `ardoise.exe --create-demo-library`, settings pointing this PC to it, then
// the real window is driven through WebView2's DevTools protocol (remote debugging port):
// clicks and keys are real input events, files are really written. Prints one line per step
// and exits with 0 only if every journey passed.
//
// Usage: node app/e2e/run.mjs <path to ardoise.exe>
import { readdirSync, statSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { freePort, sleep as harnessSleep, startApp } from './harness.mjs';
import { keepTemp, removeStaleTemps, removeTemp, removeTemps } from './temp-dirs.mjs';
import { fileURLToPath } from 'node:url';
import { journeys as mainJourneys } from './journeys.mjs';
import { backgroundJourneys } from './journeys-m7-background.mjs';
import { dragJourneys } from './journeys-m7-drag.mjs';
import { editJourneys } from './journeys-m7-edit.mjs';
import { journeys as handwritingJourneys } from './journeys-m7-hwr.mjs';
import { journeys as exportJourneys } from './journeys-m8-exports.mjs';
import { inputJourneys } from './journeys-m8-input.mjs';
import { orientationJourneys } from './journeys-m8-orientation.mjs';
import { pageJourneys } from './journeys-m8-page.mjs';
import { bridgeJourneys } from './journeys-m9-bridge.mjs';
import { aboutJourneys } from './journeys-m10-about.mjs';
import { hardFixJourneys } from './journeys-m10-fix-hard.mjs';
import { inkspaceJourneys } from './journeys-m10-inkspace.mjs';
import { libraryJourneys } from './journeys-m10-library.mjs';
import { uiJourneys } from './journeys-m10-ui.mjs';
import { shareJourneys, sweepBrowsers } from './journeys-m11-share.mjs';
import { resumeJourneys } from './journeys-m12-resume.mjs';
import { touchUpJourneys } from './journeys-m12-touchup.mjs';
import { revisionJourneys } from './journeys-m13-revision.mjs';
import { editorLayersJourneys } from './journeys-m14-editor.mjs';
import { liveComputerJourneys } from './journeys-m14-live.mjs';
import { straightenSettingJourneys } from './journeys-m15-straighten.mjs';
import { exportLayersJourneys } from './journeys-m14-export.mjs';
import { editorNavJourneys } from './journeys-editor-nav.mjs';
import { closeKeyJourneys } from './journeys-close-keys.mjs';
import { sidePanelEditorJourneys } from './journeys-m16-editor.mjs';
import { sidePanelLiveJourneys } from './journeys-m16-live.mjs';
import { livePaperJourneys } from './journeys-m17-live.mjs';
import { editorProjectionJourneys } from './journeys-m17-projection.mjs';
import { writingSoundJourneys } from './journeys-m17-sound.mjs';
import { blackboardJourneys } from './journeys-m18-blackboard.mjs';
import { bookmarkJourneys } from './journeys-bookmarks.mjs';
import { binderFixJourneys } from './journeys-binder-fixes.mjs';
import { q20Journeys } from './journeys-q20-library.mjs';
import { textJourneys } from './journeys-text.mjs';
import { equationJourneys } from './journeys-equation.mjs';
import { keysMenusJourneys } from './journeys-keys-menus.mjs';
import { guideJourneys } from './journeys-guide.mjs';
import { mcpJourney } from './mcp.mjs';

// Milestone M7 journeys live in files of their own. `ARDOISE_E2E_ONLY=text` runs only the journeys whose
// name contains one of its `|`-separated parts (while working on them; `cargo xtask e2e` runs everything).
const only = process.env.ARDOISE_E2E_ONLY?.split('|').filter(Boolean);
const journeys = [...mainJourneys, ...editJourneys, ...handwritingJourneys, ...backgroundJourneys, ...dragJourneys, ...pageJourneys, ...inputJourneys, ...orientationJourneys, ...exportJourneys, ...bridgeJourneys, ...aboutJourneys, ...uiJourneys, ...libraryJourneys, ...inkspaceJourneys, ...hardFixJourneys, ...shareJourneys, ...resumeJourneys, ...touchUpJourneys, ...editorNavJourneys, ...closeKeyJourneys, ...revisionJourneys, ...editorLayersJourneys, ...liveComputerJourneys, ...exportLayersJourneys, ...straightenSettingJourneys, ...sidePanelEditorJourneys, ...sidePanelLiveJourneys, ...livePaperJourneys, ...editorProjectionJourneys, ...writingSoundJourneys, ...blackboardJourneys, ...q20Journeys, ...textJourneys, ...bookmarkJourneys, ...equationJourneys, ...binderFixJourneys, ...keysMenusJourneys, ...guideJourneys].filter(
  (journey) => !only || only.some((part) => journey.name.includes(part)),
);

const exe = process.argv[2];
if (!exe) {
  console.error('usage: node app/e2e/run.mjs <ardoise.exe>');
  process.exit(2);
}

// The executable must contain the interface under test: warn when a source file is newer.
const sources = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src');
const newest = (dir) =>
  readdirSync(dir, { withFileTypes: true }).reduce((latest, entry) => {
    const path = join(dir, entry.name);
    return Math.max(latest, entry.isDirectory() ? newest(path) : statSync(path).mtimeMs);
  }, 0);
if (newest(sources) > statSync(exe).mtimeMs) {
  console.log('attention : l’interface a changé depuis la compilation de cet exécutable (lancer d’abord cargo xtask check pour le recompiler).');
}

const sleep = harnessSleep;

removeStaleTemps();
let failures = 0;
// A random base port: two runs at the same time (another worktree, another session) must not drive each other's window.
let port = Number(process.env.ARDOISE_E2E_PORT) || 9400 + 100 * Math.floor(Math.random() * 40);
for (const language of ['fr', 'en']) {
  for (const journey of journeys) {
    // A journey may ask for settings and environment variables that depend on a free TCP port (`app.sharePort`).
    const context = { sharePort: await freePort() };
    const given = (value) => (typeof value === 'function' ? value(context) : value);
    const label = `[${language.toUpperCase()}] ${journey.name}`;
    console.log(`\n${label}`);
    let app;
    try {
      app = await startApp(exe, language, port++, journey.args, { settings: given(journey.settings), env: given(journey.env), browserArgs: journey.browserArgs });
    } catch (error) {
      // A window that does not start fails this journey, not the whole run.
      failures++;
      console.log(`  ✘ 0. ${error.message}`);
      console.log(`  → ${label} : ÉCHEC`);
      continue;
    }
    app.sharePort = context.sharePort;
    let step = 0;
    // The step that threw, when it was inside `check`; otherwise the failure came after the last passed step.
    let failedStep = 0;
    let passed = false;
    const check = async (description, action) => {
      step++;
      failedStep = step;
      await action();
      failedStep = 0;
      console.log(`  ✔ ${step}. ${description}`);
    };
    try {
      await journey.run(app, check);
      console.log(`  → ${label} : OK (${step} étapes)`);
      passed = true;
    } catch (error) {
      failures++;
      // A window that no longer answers must not keep the error from being printed.
      const shot = await Promise.race([app.screenshot('failure').catch(() => null), sleep(5000).then(() => null)]);
      console.log(`  ✘ ${failedStep || step + 1}. ${error.message}`);
      console.log(`  → ${label} : ÉCHEC${shot ? ` (capture : ${shot})` : ''}`);
    } finally {
      await app.close();
      // Only a passed journey leaves nothing behind (at once, or at the end of the run when WebView2 still holds a
      // file); a failed one keeps its folder and capture, which a later run removes after a day.
      if (passed) removeTemp(app.dir);
      else keepTemp(app.dir);
    }
  }
}
// The MCP server (M9) is spoken to over stdio, without a window: a journey of its own, in each language.
if (!only || only.some((part) => 'serveur MCP'.includes(part))) {
  for (const language of ['fr', 'en']) {
    const label = `[${language.toUpperCase()}] serveur MCP`;
    console.log(`\n${label}`);
    const failed = await mcpJourney({ exe, language });
    console.log(`  → ${label} : ${failed === 0 ? 'OK' : `ÉCHEC (${failed})`}`);
    if (failed) failures++;
  }
}
// Nothing of the headless Edges must outlive the run: what `close()` missed is killed here, and said.
const leftEdges = await sweepBrowsers();
if (leftEdges > 0) console.log(`\nAttention : ${leftEdges} processus Edge sans fenêtre restaient ouverts après leurs parcours ; arrêtés.`);
console.log(failures === 0 ? '\nTous les parcours ont réussi.' : `\n${failures} parcours en échec.`);
await removeTemps();
process.exit(failures === 0 ? 0 : 1);
