// M10 journeys of the review fixes "drafts, session end, one window per copy" (not wired in run.mjs:
// the integrator adds `...hardFixJourneys` to the list of journeys).
//
// - One window per copy: a second launch of the copy the runner started exits with 0 at once and
//   leaves the first window running; `--mcp --read-only` of that copy still answers while it is
//   open; a copy in another folder runs beside it.
// - Drafts: an unsaved change is kept as a draft in the copy's LocalAppData within seconds; the app
//   is then killed (every process of the copy, as a crash or an empty battery would), started again
//   on another debugging port, and the library offers the draft, which the editor restores.
//
// The end of the Windows session (WM_QUERYENDSESSION) cannot be sent from the DevTools protocol:
// it was checked by hand on the development PC (docs/design/interface.md, "M10: drafts and session
// end").
import { execFileSync, spawn } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const STROKES = '.editor svg g.ink path:not(.live)';

/** The environment the runner gives its copy (its own LOCALAPPDATA), without a debugging port. */
function envOf(app) {
  const env = { ...process.env, LOCALAPPDATA: join(app.dir, 'LocalAppData') };
  delete env.WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS;
  return env;
}

/** Ends every process of the copy in `dir` (its WebView2 browser may outlive a killed app). */
async function killCopy(dir) {
  const quoted = dir.replace(/'/g, "''");
  for (let i = 0; i < 20; i++) {
    const left = execFileSync(
      'powershell',
      [
        '-NoProfile',
        '-Command',
        `$p = Get-CimInstance Win32_Process | Where-Object { $_.ProcessId -ne $PID -and $_.CommandLine -like '*${quoted}*' }; $p | ForEach-Object { Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }; @($p).Count`,
      ],
      { encoding: 'utf8' },
    ).trim();
    if (left === '0') return;
    await sleep(500);
  }
}

/** The draft files of the copy. */
function drafts(app) {
  const root = join(app.dir, 'LocalAppData', 'Ardoise', 'drafts');
  if (!existsSync(root)) return [];
  return readdirSync(root).flatMap((copy) =>
    readdirSync(join(root, copy))
      .filter((name) => name.endsWith('.json'))
      .map((name) => join(root, copy, name)),
  );
}

/** A minimal DevTools client of a window started here. */
async function devtools(port) {
  let target;
  for (let i = 0; i < 240 && !target; i++) {
    await sleep(250);
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      target = list.find((t) => t.type === 'page' && !t.url.includes('view=presentation'));
    } catch {
      // Not listening yet.
    }
  }
  if (!target) throw new Error('the restarted window did not open its DevTools endpoint');
  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve);
    ws.addEventListener('error', reject);
  });
  let id = 0;
  const pending = new Map();
  ws.addEventListener('message', (message) => {
    const data = JSON.parse(message.data);
    pending.get(data.id)?.(data);
    pending.delete(data.id);
  });
  const evaluate = async (expression) => {
    const reply = await new Promise((resolve) => {
      const n = ++id;
      pending.set(n, resolve);
      ws.send(JSON.stringify({ id: n, method: 'Runtime.evaluate', params: { expression, returnByValue: true, awaitPromise: true } }));
    });
    if (reply.result?.exceptionDetails) throw new Error(`page error: ${reply.result.exceptionDetails.exception?.description}`);
    return reply.result?.result?.value;
  };
  const waitFor = async (expression, what, timeout = 30_000) => {
    const deadline = Date.now() + timeout;
    for (;;) {
      const value = await evaluate(expression).catch(() => undefined);
      if (value) return value;
      if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
      await sleep(100);
    }
  };
  return { evaluate, waitFor, close: () => ws.close() };
}

export const hardFixJourneys = [
  {
    name: 'une seule fenêtre par copie (second lancement, --mcp, autre copie)',
    async run(app, check) {
      const exe = join(app.dir, 'ardoise.exe');
      await check('la bibliothèque de démonstration s’affiche', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
      });

      await check('un second lancement de la même copie se termine aussitôt (code 0), la fenêtre reste', async () => {
        const second = spawn(exe, [], { env: envOf(app), stdio: 'ignore' });
        const code = await Promise.race([
          new Promise((resolve) => second.once('exit', resolve)),
          sleep(30_000).then(() => 'still running'),
        ]);
        if (code === 'still running') {
          second.kill();
          throw new Error('the second launch kept running');
        }
        if (code !== 0) throw new Error(`the second launch exited with ${code}`);
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the first window, still there');
      });

      await check('`--mcp --read-only` de la même copie répond pendant que l’application est ouverte', async () => {
        const mcp = spawn(exe, ['--mcp', '--read-only'], { env: envOf(app), stdio: ['pipe', 'pipe', 'ignore'], windowsHide: true });
        const line = await new Promise((resolve) => {
          let out = '';
          mcp.stdout.on('data', (data) => {
            out += data;
            if (out.includes('\n')) resolve(out.split('\n')[0]);
          });
          mcp.stdin.write(
            `${JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'initialize', params: { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 'e2e', version: '1' } } })}\n`,
          );
          setTimeout(() => resolve(''), 30_000);
        });
        mcp.stdin.end();
        setTimeout(() => mcp.kill(), 2000);
        if (!JSON.parse(line || '{}').result?.serverInfo) throw new Error(`no MCP answer: ${line}`);
      });

      await check('une copie dans un autre dossier démarre à côté', async () => {
        const dir = mkdtempSync(join(tmpdir(), 'ardoise-e2e-other-copy-'));
        copyFileSync(exe, join(dir, 'ardoise.exe'));
        writeFileSync(join(dir, 'ardoise.toml'), `language = "${app.language}"\n`);
        mkdirSync(join(dir, 'LocalAppData'));
        const env = { ...envOf(app), LOCALAPPDATA: join(dir, 'LocalAppData') };
        const other = spawn(join(dir, 'ardoise.exe'), [], { env, stdio: 'ignore' });
        let exited = null;
        other.once('exit', (code) => (exited = code));
        await sleep(12_000);
        const early = exited;
        await killCopy(dir);
        if (early !== null) throw new Error(`the other copy exited with ${early}`);
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the first window, still there');
      });
    },
  },
  {
    name: 'brouillon d’une note non enregistrée après un arrêt brutal',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      await check('ouvrir une note et la modifier sans l’enregistrer', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await app.click('.note-card .card-open');
        await app.waitFor(`document.querySelectorAll(${JSON.stringify(STROKES)}).length > 0`, 'the strokes of the note');
        await app.click(`[aria-label="${l('editor.colors.red')}"]`);
        await app.click('.toolbar .apply button', l('editor.applyAll.color'));
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'yes'`, 'the unsaved mark');
      });

      await check('un brouillon est écrit dans LocalAppData en quelques secondes, hors de la bibliothèque', async () => {
        const deadline = Date.now() + 15_000;
        while (drafts(app).length === 0) {
          if (Date.now() > deadline) throw new Error('no draft written');
          await sleep(200);
        }
        const draft = JSON.parse(readFileSync(drafts(app)[0], 'utf8'));
        if (!draft.note.strokes.every((s) => s.color.r === 216)) throw new Error('the draft does not hold the change');
      });

      let restarted = null;
      let page = null;
      try {
        await check('tuer l’application puis la relancer : la bibliothèque propose le brouillon', async () => {
          await killCopy(app.dir);
          const port = 9000 + Math.floor(Math.random() * 300);
          restarted = spawn(join(app.dir, 'ardoise.exe'), [], {
            env: { ...envOf(app), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}` },
            stdio: 'ignore',
          });
          page = await devtools(port);
          await page.waitFor(`document.querySelector('[data-slot="drafts-banner"]')`, 'the drafts banner', 60_000);
        });

        await check('ouvrir la note : l’éditeur propose de récupérer les modifications, puis les rétablit', async () => {
          await page.evaluate(`document.querySelector('[data-action="drafts-open"]').click()`);
          await page.waitFor(
            `document.querySelector('[data-slot="draft-recovery"]')?.dataset.recovery === 'restore'`,
            'the recovery dialog',
          );
          await page.evaluate(`document.querySelector('[data-action="draft-restore"]').click()`);
          await page.waitFor(`document.querySelector('.editor').dataset.dirty === 'yes'`, 'the restored changes');
          await page.waitFor(
            `[...document.querySelectorAll(${JSON.stringify(STROKES)})].every((p) => /216, 0, 0/.test(p.getAttribute('fill')))`,
            'every stroke in red again',
          );
        });
      } finally {
        page?.close();
        if (restarted) await killCopy(app.dir);
      }
    },
  },
];
