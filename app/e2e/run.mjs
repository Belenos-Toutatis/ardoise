// End-to-end journeys on the real executable (`cargo xtask e2e`).
//
// For each interface language: a portable copy of ardoise.exe in a temporary folder, a demo
// library created by `ardoise.exe --create-demo-library`, settings pointing this PC to it, then
// the real window is driven through WebView2's DevTools protocol (remote debugging port):
// clicks and keys are real input events, files are really written. Prints one line per step
// and exits with 0 only if every journey passed.
//
// Usage: node app/e2e/run.mjs <path to ardoise.exe>
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
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
import { q20Journeys } from './journeys-q20-library.mjs';
import { textJourneys } from './journeys-text.mjs';
import { mcpJourney } from './mcp.mjs';

// Milestone M7 journeys live in files of their own. `ARDOISE_E2E_ONLY=text` runs only the journeys whose
// name contains one of its `|`-separated parts (while working on them; `cargo xtask e2e` runs everything).
const only = process.env.ARDOISE_E2E_ONLY?.split('|').filter(Boolean);
const journeys = [...mainJourneys, ...editJourneys, ...handwritingJourneys, ...backgroundJourneys, ...dragJourneys, ...pageJourneys, ...inputJourneys, ...orientationJourneys, ...exportJourneys, ...bridgeJourneys, ...aboutJourneys, ...uiJourneys, ...libraryJourneys, ...inkspaceJourneys, ...hardFixJourneys, ...shareJourneys, ...resumeJourneys, ...touchUpJourneys, ...editorNavJourneys, ...closeKeyJourneys, ...revisionJourneys, ...editorLayersJourneys, ...liveComputerJourneys, ...exportLayersJourneys, ...straightenSettingJourneys, ...sidePanelEditorJourneys, ...sidePanelLiveJourneys, ...livePaperJourneys, ...editorProjectionJourneys, ...writingSoundJourneys, ...blackboardJourneys, ...q20Journeys, ...textJourneys].filter(
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

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** A TCP port nobody listens on (asked of the system, then released): the share of a journey uses it, so two runs never collide. */
function freePort() {
  return new Promise((resolve, reject) => {
    const server = createServer();
    server.once('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const { port } = server.address();
      server.close(() => resolve(port));
    });
  });
}

/**
 * Starts a portable copy of the app on a fresh demo library, in `language`, with `args` on its command line.
 * `settings`: extra text of ardoise.toml (tables such as `[share]`, put before the machine's table);
 * `env`: extra environment variables of the process;
 * `browserArgs`: extra command-line arguments of WebView2 (a journey that keeps a window over the control window asks
 * for the ones that stop Chromium from pausing a window it thinks hidden).
 */
async function startApp(language, port, args = [], { settings = '', env = {}, browserArgs = '' } = {}) {
  const dir = mkdtempSync(join(tmpdir(), `ardoise-e2e-${language}-`));
  const appExe = join(dir, 'ardoise.exe');
  copyFileSync(exe, appExe);
  const library = join(dir, 'Bibliothèque');
  mkdirSync(library);
  execFileSync(appExe, ['--create-demo-library', library], { stdio: 'pipe' });
  const machine = process.env.COMPUTERNAME ?? 'PC';
  const extra = settings ? `${settings.trimEnd()}\n\n` : '';
  writeFileSync(
    join(dir, 'ardoise.toml'),
    `language = "${language}"\n\n${extra}[machines."${machine}"]\nlibrary = '${library}'\n`,
  );
  // LOCALAPPDATA in the run's folder: a fresh WebView2 profile and thumbnail cache, nothing
  // shared with the Ardoise of this PC or with another run.
  const local = join(dir, 'LocalAppData');
  mkdirSync(local);
  const child = spawn(appExe, args, {
    env: {
      ...process.env,
      LOCALAPPDATA: local,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}${browserArgs ? ` ${browserArgs}` : ''}`,
      ...env,
    },
    stdio: 'ignore',
  });

  // Up to 40 s: a fresh WebView2 profile on a busy PC can take more than 20 s to listen.
  let target;
  for (let i = 0; i < 160 && !target; i++) {
    await sleep(250);
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      // The main window: not the projector's presentation or projection window, which may open later.
      target = list.find((t) => t.type === 'page' && !t.url.includes('view=presentation') && !t.url.includes('view=projection'));
    } catch {
      // Not listening yet.
    }
  }
  if (!target) {
    child.kill();
    throw new Error('the window did not open its DevTools endpoint');
  }
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve);
    ws.addEventListener('error', reject);
  });
  let id = 0;
  const pending = new Map();
  // The protocol's events (no id), kept for the journeys that wait for one (`app.waitForEvent`: a drag the page starts).
  const events = [];
  ws.addEventListener('message', (message) => {
    const data = JSON.parse(message.data);
    if (data.method) events.push(data);
    pending.get(data.id)?.(data);
    pending.delete(data.id);
  });
  // The window can go while a call is on its way (Ctrl + Q): whoever waits gets an empty reply, or node would end the run
  // without a word, as nothing would keep it alive for a reply that will never come.
  ws.addEventListener('close', () => {
    for (const resolve of pending.values()) resolve({ closed: true });
    pending.clear();
  });
  const send = (method, params = {}) =>
    new Promise((resolve) => {
      if (ws.readyState !== WebSocket.OPEN) return resolve({ closed: true });
      const n = ++id;
      pending.set(n, resolve);
      ws.send(JSON.stringify({ id: n, method, params }));
    });

  const app = {
    dir,
    library,
    language,
    /** The remote debugging port of this window: a journey attaches to the other windows (presentation) through it. */
    port,
    send,
    /** The next event `method` of the protocol (after `since`, an index of `events`), or null after `timeout` ms. */
    async waitForEvent(method, { timeout = 5000, since = 0 } = {}) {
      const deadline = Date.now() + timeout;
      for (;;) {
        const found = events.slice(since).find((event) => event.method === method);
        if (found) return found;
        if (Date.now() > deadline) return null;
        await sleep(50);
      }
    },
    /** How many protocol events have arrived so far (for `waitForEvent`'s `since`). */
    eventCount: () => events.length,
    /** Value of a JavaScript expression in the page (awaited). */
    async evaluate(expression) {
      const reply = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (reply.result?.exceptionDetails) {
        throw new Error(`page error: ${reply.result.exceptionDetails.exception?.description ?? 'unknown'}`);
      }
      return reply.result?.result?.value;
    },
    /** Waits until `expression` is truthy; returns its value. */
    async waitFor(expression, what, timeout = 10_000) {
      const deadline = Date.now() + timeout;
      for (;;) {
        const value = await app.evaluate(expression).catch(() => undefined);
        if (value) return value;
        if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
        await sleep(100);
      }
    },
    /**
     * Centre of the first element matching `selector` (optionally containing `text`), scrolled into
     * view unless `scroll` is false; `hit`: the element is the one on top at that point.
     */
    async centre(selector, text, { scroll = true } = {}) {
      const box = await app.evaluate(`(() => {
        const all = [...document.querySelectorAll(${JSON.stringify(selector)})];
        const el = ${text === undefined ? 'all[0]' : `all.find((e) => e.textContent.includes(${JSON.stringify(text)}))`};
        if (!el) return null;
        if (${scroll}) el.scrollIntoView({ block: 'center', inline: 'center' });
        const r = el.getBoundingClientRect();
        const x = r.left + r.width / 2;
        const y = r.top + r.height / 2;
        const top = document.elementFromPoint(x, y);
        const hit = !!top && (top === el || el.contains(top) || (top.tagName === 'LABEL' && top.contains(el)));
        return { x, y, hit };
      })()`);
      if (!box) throw new Error(`no element ${selector}${text === undefined ? '' : ` containing "${text}"`}`);
      return box;
    },
    /**
     * A real mouse click at the centre of an element, once it is the element on top there and has
     * stopped moving: a section that fills in above it (settings loaded, a status line, a thumbnail)
     * would otherwise move it between the measure and the press, and the click would land elsewhere.
     * After about a second it clicks anyway (an element covered on purpose).
     */
    async click(selector, text, { clickCount = 1 } = {}) {
      let box = await app.centre(selector, text);
      for (let attempt = 0; attempt < 20; attempt++) {
        await sleep(40);
        // Measured where it is, without scrolling it back (scrolling would hide a move); scrolled
        // again only when it is no longer the element on top there (moved away or out of view).
        let again = await app.centre(selector, text, { scroll: false });
        if (!again.hit) again = await app.centre(selector, text);
        const still = Math.abs(again.x - box.x) < 1 && Math.abs(again.y - box.y) < 1;
        box = again;
        if (still && again.hit) break;
      }
      const { x, y } = box;
      await app.mouse('mouseMoved', x, y);
      await app.mouse('mousePressed', x, y, { clickCount });
      await app.mouse('mouseReleased', x, y, { clickCount });
    },
    async mouse(type, x, y, { button = 'left', clickCount = 1, buttons } = {}) {
      await send('Input.dispatchMouseEvent', {
        type,
        x,
        y,
        button: type === 'mouseMoved' && buttons === undefined ? 'none' : button,
        buttons: buttons ?? (type === 'mousePressed' || (type === 'mouseMoved' && button !== 'none') ? 1 : 0),
        clickCount,
      });
    },
    /** Drags the mouse along `points` (page pixels) with the left button down. */
    async drag(points) {
      const [first, ...rest] = points;
      await app.mouse('mouseMoved', first.x, first.y);
      await app.mouse('mousePressed', first.x, first.y);
      for (const p of rest) {
        await app.mouse('mouseMoved', p.x, p.y, { buttons: 1, button: 'left' });
        await sleep(8);
      }
      const last = rest.at(-1) ?? first;
      await app.mouse('mouseReleased', last.x, last.y);
    },
    /** Types text into the focused element. */
    async type(text) {
      await send('Input.insertText', { text });
    },
    /** A key press, e.g. key('z', { ctrl: true }), key('Enter') or key('ArrowRight'). */
    async key(key, { ctrl = false, shift = false, alt = false } = {}) {
      const modifiers = (alt ? 1 : 0) | (ctrl ? 2 : 0) | (shift ? 8 : 0);
      const code = key.length === 1 ? `Key${key.toUpperCase()}` : key;
      const windowsVirtualKeyCode =
        key.length === 1
          ? key.toUpperCase().charCodeAt(0)
          : {
              Enter: 13,
              Escape: 27,
              Delete: 46,
              Backspace: 8,
              Tab: 9,
              PageUp: 33,
              PageDown: 34,
              ArrowLeft: 37,
              ArrowUp: 38,
              ArrowRight: 39,
              ArrowDown: 40,
              F4: 115,
            }[key];
      const base = { key, code, modifiers, windowsVirtualKeyCode };
      await send('Input.dispatchKeyEvent', { type: 'keyDown', ...base, text: key.length === 1 && !ctrl ? key : undefined });
      await send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
    },
    /** Saves a screenshot in the run's folder; returns its path. */
    async screenshot(name) {
      const reply = await send('Page.captureScreenshot', { format: 'png' });
      const path = join(dir, `${name}.png`);
      writeFileSync(path, Buffer.from(reply.result.data, 'base64'));
      return path;
    },
    /** The process has ended on its own (Ctrl + Q): nothing in the page answers any more. */
    exited: () => child.exitCode !== null,
    async close() {
      ws.close();
      if (child.exitCode === null) {
        const exited = new Promise((resolve) => child.once("exit", resolve));
        child.kill();
        await Promise.race([exited, sleep(5000)]);
      }
      await sleep(300);
    },
  };
  // `complete` is not enough: the interface shows "Loading…" until the settings are read, and a journey that clicks at once would find nothing.
  try {
    await app.waitFor('document.readyState === "complete" && document.querySelector("#app .shell") !== null', 'the interface');
  } catch (error) {
    await app.close();
    throw error;
  }
  return app;
}

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
      app = await startApp(language, port++, journey.args, { settings: given(journey.settings), env: given(journey.env), browserArgs: journey.browserArgs });
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
      // Only a passed journey leaves nothing behind; a failed one keeps its folder and capture.
      if (passed) {
        try {
          rmSync(app.dir, { recursive: true, force: true, maxRetries: 10, retryDelay: 200 });
        } catch {
          // A locked temporary file is not a test failure.
        }
      }
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
process.exit(failures === 0 ? 0 : 1);
