// M9 journey "bridge to the open app" (`app_state`, `open_note`, `presentation`, `last_live_note`).
//
// On the production executable with the simulated tablet (`--simulated-tablet`, which streams a pen
// in live mode). With the app open, the journey starts `ardoise.exe --mcp` from the very copy the
// runner started (same folder, so same settings and same pipe), as an assistant would, and drives
// the app through it; the WebView shows that the requests were carried out like clicks: the editor
// shows the note, the live window projects (its "Stop projecting" button, and Rust's own record of
// the presentation window) and pages (`canvas.live-canvas[data-page]`), and the live session's notes
// are in the library. The "app not open" answers are checked by app/e2e/mcp.mjs.
import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { McpSession } from './mcp.mjs';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const NOTE = "Cours d'optique";

/** `ardoise.exe --mcp` of the copy the runner started, with the same LOCALAPPDATA. */
function startMcp(app) {
  const child = spawn(join(app.dir, 'ardoise.exe'), ['--mcp'], {
    env: { ...process.env, LOCALAPPDATA: join(app.dir, 'LocalAppData') },
    stdio: ['pipe', 'pipe', 'pipe'],
    windowsHide: true,
  });
  return new McpSession(child);
}

const resultText = (result) => result.content?.filter((c) => c.type === 'text').map((c) => c.text).join('\n');

/** The structured result of a call that must succeed. */
async function ok(session, tool, args = {}) {
  const result = await session.call(tool, args);
  if (result.isError) throw new Error(`${tool} ${JSON.stringify(args)}: ${resultText(result)}`);
  return result.structuredContent;
}

/** The `{ code, message }` of a call that must fail. */
async function refused(session, tool, args = {}) {
  const result = await session.call(tool, args);
  if (!result.isError) throw new Error(`${tool} ${JSON.stringify(args)} should fail: ${resultText(result)}`);
  return result.structuredContent;
}

/** Calls `tool` until `accept(result)` holds (the app may still be starting). */
async function until(session, tool, args, accept, what, timeout = 15_000) {
  const deadline = Date.now() + timeout;
  let last;
  for (;;) {
    last = await session.call(tool, args);
    if (accept(last)) return last.structuredContent;
    if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}: ${resultText(last)}`);
    await sleep(250);
  }
}

const samePath = (a, b) => String(a).replaceAll('/', '\\').toLowerCase() === String(b).replaceAll('/', '\\').toLowerCase();

export const bridgeJourneys = [
  {
    name: 'passerelle MCP : état, ouvrir une note, présentation du mode direct, note du mode direct',
    args: ['--simulated-tablet'],
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const j = JSON.stringify;
      const STATUS = '.tablet-status';
      const WIZARD = '.register-wizard';
      const canvasPage = `document.querySelector('canvas.live-canvas')?.dataset.page`;
      const canvasStrokes = `Number(document.querySelector('canvas.live-canvas')?.dataset.strokes)`;
      const session = startMcp(app);
      try {
        let note;
        await check('--mcp de la même copie : app_state voit l’application ouverte sur la bibliothèque', async () => {
          await session.initialize();
          const tools = (await session.tools()).map((t) => t.name);
          for (const name of ['app_state', 'open_note', 'presentation', 'last_live_note']) {
            if (!tools.includes(name)) throw new Error(`${name} missing from tools/list`);
          }
          const state = await until(
            session,
            'app_state',
            {},
            (r) => !r.isError && r.structuredContent.screen === 'library',
            'the library on screen',
          );
          if (state.open !== true) throw new Error(JSON.stringify(state));
          if (!samePath(state.library, app.library)) throw new Error(`library ${state.library}, expected ${app.library}`);
          if (state.note !== null || state.live_mode !== null) throw new Error(JSON.stringify(state));
        });

        await check('open_note : l’éditeur montre « Cours d’optique », app_state la nomme', async () => {
          const notes = (await ok(session, 'list_notes', { limit: 100 })).notes;
          note = notes.find((n) => n.title === NOTE);
          if (!note) throw new Error(`« ${NOTE} » missing`);
          const opened = await ok(session, 'open_note', { note: note.id });
          if (opened.opened.id !== note.id) throw new Error(JSON.stringify(opened));
          const wanted = note.path.toLowerCase();
          await app.waitFor(
            `(document.querySelector('section.editor')?.dataset.path ?? '').toLowerCase().replaceAll('\\\\', '/').endsWith(${j(wanted)})`,
            'the editor showing the note',
          );
          const state = await ok(session, 'app_state');
          if (state.screen !== 'editor' || state.note?.id !== note.id || state.unsaved_changes !== false) {
            throw new Error(JSON.stringify(state));
          }
        });

        await check('sans mode direct, la présentation est refusée (liveNotRunning) ; retour à la bibliothèque', async () => {
          const error = await refused(session, 'presentation', { action: 'start' });
          if (error.code !== 'liveNotRunning') throw new Error(JSON.stringify(error));
          const none = await ok(session, 'last_live_note');
          if (none.session !== null) throw new Error(JSON.stringify(none));
          await app.click('.editor button', l('editor.back'));
          await app.waitFor(`document.querySelector('input.search') !== null`, 'the library');
        });

        await check('le professeur lance le mode direct (tablette simulée)', async () => {
          await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="notRegistered"]`)}) !== null`, 'the sidebar block');
          await app.click(STATUS);
          await app.waitFor(`document.querySelector('.tablet-panel button[data-action="register"]') !== null`, 'the register button');
          await app.click('.tablet-panel button[data-action="register"]');
          await app.waitFor(`document.querySelector(${j(`${WIZARD} [data-step="warning"]`)}) !== null`, 'the warning step');
          await app.click(`${WIZARD} button[data-action="continue"]`);
          await app.waitFor(`document.querySelector(${j(`${WIZARD} [data-step="done"]`)}) !== null`, 'the end of the registration', 60_000);
          await app.click(`${WIZARD} button[data-action="finish"]`);
          await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="connected"]`)}) !== null`, 'the end of the sync', 30_000);
          await app.click('.tablet-panel button[data-action="live"]');
          await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
          await app.waitFor(`${canvasStrokes} > 0`, 'strokes on the live canvas', 10_000);
          const state = await ok(session, 'app_state');
          if (state.screen !== 'live' || state.live_mode?.running !== true || state.current_page !== 1) {
            throw new Error(JSON.stringify(state));
          }
        });

        await check('presentation start : la fenêtre de présentation s’ouvre sur l’écran choisi', async () => {
          const started = await ok(session, 'presentation', { action: 'start' });
          if (started.presentation_open !== true || !started.screen) throw new Error(JSON.stringify(started));
          await app.waitFor(`document.querySelector('button[data-action="stop-projecting"]') !== null`, 'the "stop projecting" button');
          // Rust records the presentation window once it is built and placed full screen.
          const state = await ok(session, 'app_state');
          if (state.presentation_open !== true || state.presentation_screen !== started.screen) throw new Error(JSON.stringify(state));
        });

        await check('presentation next, page, previous : la page projetée suit', async () => {
          const next = await ok(session, 'presentation', { action: 'next' });
          if (next.page !== 2 || next.pages !== 2) throw new Error(JSON.stringify(next));
          await app.waitFor(`${canvasPage} === '2/2'`, 'page 2/2');
          const first = await ok(session, 'presentation', { action: 'page', page: 1 });
          if (first.page !== 1) throw new Error(JSON.stringify(first));
          await app.waitFor(`${canvasPage} === '1/2'`, 'page 1/2');
          await ok(session, 'presentation', { action: 'page', page: 2 });
          const back = await ok(session, 'presentation', { action: 'previous' });
          if (back.page !== 1 || back.pages !== 2) throw new Error(JSON.stringify(back));
          await app.waitFor(`${canvasPage} === '1/2'`, 'page 1/2 again');
          const beyond = await refused(session, 'presentation', { action: 'page', page: 5 });
          if (beyond.code !== 'invalidArguments') throw new Error(JSON.stringify(beyond));
          const busy = await refused(session, 'open_note', { note: note.id });
          if (busy.code !== 'appBusy') throw new Error(JSON.stringify(busy));
        });

        await check('presentation stop : la fenêtre se ferme, le mode direct continue', async () => {
          const stopped = await ok(session, 'presentation', { action: 'stop' });
          if (stopped.presentation_open !== false) throw new Error(JSON.stringify(stopped));
          await app.waitFor(`document.querySelector('button[data-action="project"]') !== null`, 'the "project" button');
          const state = await ok(session, 'app_state');
          if (state.presentation_open !== false || state.live_mode?.running !== true) throw new Error(JSON.stringify(state));
        });

        await check('last_live_note : la note du mode direct, pendant puis après l’arrêt', async () => {
          const during = await until(
            session,
            'last_live_note',
            {},
            (r) => !r.isError && r.structuredContent.notes.length > 0,
            'a note written by live mode',
          );
          if (during.session.running !== true || during.session.this_run !== true) throw new Error(JSON.stringify(during));
          await app.click('button[data-action="stop-live"]');
          await app.waitFor(`document.querySelector('input.search') !== null`, 'the library');
          const after = await ok(session, 'last_live_note');
          if (after.session.running !== false || !after.note?.found) throw new Error(JSON.stringify(after));
          const listed = (await ok(session, 'list_notes', { limit: 100 })).notes;
          for (const page of after.notes) {
            if (!page.found || !listed.some((n) => n.id === page.id)) throw new Error(`page ${page.page} not in the library: ${JSON.stringify(page)}`);
          }
          if (after.note.id !== after.notes.at(-1).id) throw new Error(JSON.stringify(after));
        });
      } finally {
        await session.close(3000);
      }
    },
  },
];
