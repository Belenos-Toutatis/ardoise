// End-to-end journey of the MCP server (milestone M9), separate from the WebView journeys: no
// window is driven. A portable copy of ardoise.exe is started as `ardoise.exe --mcp` on a demo
// library, as an assistant would start it (a child process with pipes for its standard handles),
// and spoken to in JSON-RPC, one message per line.
//
// Not wired into run.mjs / xtask: the integrator calls `mcpJourney({ exe, log })` after the other
// journeys. By hand: `node app/e2e/mcp.mjs target/release/ardoise.exe`.
//
// It checks: initialize; tools/list (every read tool; with --read-only no write tool); a call of
// each read tool: the render is a real PNG, the text of « Cours d'optique » has its words (needs
// Windows' French handwriting recognizer), the search finds it; errors come back as tool results.
// Parts of the server added later (write tools, prompts, bridge) are checked by their own
// functions below `readTools`, which the agents who write them add to `parts`.
import { spawn, execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { mcpWriteTools } from './mcp-write.mjs';

export const READ_TOOLS = ['get_note', 'get_note_text', 'list_binders', 'list_notes', 'list_tags', 'render_note', 'search_notes'];
/** Tools that change the library or the app: absent from a `--read-only` server. */
export const WRITE_TOOLS = [
  'create_note', 'update_note', 'move_note', 'trash_note', 'merge_notes', 'split_note',
  'straighten_shapes', 'export_note', 'set_note_text', 'app_state', 'open_note', 'presentation', 'last_live_note',
];
/** The checks of the later parts of the server (write tools and prompts, bridge), each given the
 *  helpers of the journey and run after the read tools. */
const parts = [mcpWriteTools];

const NOTE = "Cours d'optique";
/** The words of the demo note's handwriting (`demo.rs`: HANDWRITTEN_SENTENCE). */
const NOTE_WORDS = ['lumiere', 'propage', 'ligne', 'droite'];

const fold = (text) => text.normalize('NFD').replace(/\p{M}/gu, '').toLowerCase();

/** A server process and its JSON-RPC conversation over stdio. */
export class McpSession {
  constructor(child) {
    this.child = child;
    this.nextId = 1;
    this.pending = new Map();
    this.buffer = '';
    this.stderr = '';
    this.stdoutLines = [];
    child.stdout.setEncoding('utf8');
    child.stderr.setEncoding('utf8');
    child.stdout.on('data', (chunk) => this.#onData(chunk));
    child.stderr.on('data', (chunk) => (this.stderr += chunk));
    this.exited = new Promise((resolve) => child.on('exit', (code, signal) => resolve({ code, signal })));
  }

  #onData(chunk) {
    this.buffer += chunk;
    let at;
    while ((at = this.buffer.indexOf('\n')) >= 0) {
      const line = this.buffer.slice(0, at).replace(/\r$/, '');
      this.buffer = this.buffer.slice(at + 1);
      if (!line) continue;
      this.stdoutLines.push(line);
      let message;
      try {
        message = JSON.parse(line);
      } catch {
        // Anything that is not a JSON message on stdout breaks the protocol: the journey fails on it.
        for (const { reject } of this.pending.values()) reject(new Error(`not a protocol message on stdout: ${line.slice(0, 200)}`));
        this.pending.clear();
        continue;
      }
      const waiting = message.id !== undefined && this.pending.get(message.id);
      if (waiting) {
        this.pending.delete(message.id);
        waiting.resolve(message);
      }
    }
  }

  /** A request; resolves with the whole JSON-RPC response. */
  request(method, params, timeoutMs = 60_000) {
    const id = this.nextId++;
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id, method, params })}\n`);
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`no answer to ${method} after ${timeoutMs} ms; stderr: ${this.stderr.slice(-300)}`));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (message) => (clearTimeout(timer), resolve(message)),
        reject: (error) => (clearTimeout(timer), reject(error)),
      });
    });
  }

  notify(method, params) {
    this.child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method, params })}\n`);
  }

  /** `initialize` and `notifications/initialized`; returns the server's answer. */
  async initialize() {
    const answer = await this.request('initialize', {
      protocolVersion: '2025-06-18',
      capabilities: {},
      clientInfo: { name: 'ardoise-e2e', version: '0' },
    });
    this.notify('notifications/initialized');
    return answer.result;
  }

  async tools() {
    return (await this.request('tools/list', {})).result.tools;
  }

  /** The result of `tools/call` (a protocol error is thrown). */
  async call(name, args = {}, timeoutMs = 120_000) {
    const answer = await this.request('tools/call', { name, arguments: args }, timeoutMs);
    if (answer.error) throw new Error(`${name}: protocol error ${JSON.stringify(answer.error)}`);
    return answer.result;
  }

  /** Closes stdin and waits for the server to exit by itself. */
  async close(timeoutMs = 10_000) {
    this.child.stdin.end();
    const timer = new Promise((resolve) => setTimeout(() => resolve('timeout'), timeoutMs));
    const outcome = await Promise.race([this.exited, timer]);
    if (outcome === 'timeout') this.child.kill();
    return outcome;
  }
}

/** A portable copy of `exe` with a demo library and this PC's library setting, in a temporary folder. */
export function prepareApp(exe, language = 'fr') {
  const dir = mkdtempSync(join(tmpdir(), `ardoise-mcp-${language}-`));
  const appExe = join(dir, 'ardoise.exe');
  copyFileSync(exe, appExe);
  const library = join(dir, 'Bibliothèque');
  mkdirSync(library);
  execFileSync(appExe, ['--create-demo-library', library], { stdio: 'pipe' });
  const machine = process.env.COMPUTERNAME ?? 'PC';
  writeFileSync(join(dir, 'ardoise.toml'), `language = "${language}"\n\n[machines."${machine}"]\nlibrary = '${library}'\n`);
  const local = join(dir, 'LocalAppData');
  mkdirSync(local);
  return { dir, appExe, library, local };
}

/** Starts `ardoise.exe --mcp` the way an assistant does: pipes for stdin, stdout and stderr. */
export function startServer(app, extraArgs = []) {
  const child = spawn(app.appExe, ['--mcp', ...extraArgs], {
    // A LOCALAPPDATA of the run: the handwriting cache is not shared with this PC's Ardoise.
    env: { ...process.env, LOCALAPPDATA: app.local },
    stdio: ['pipe', 'pipe', 'pipe'],
    windowsHide: true,
  });
  return new McpSession(child);
}

const textOf = (result) => result.content.filter((c) => c.type === 'text').map((c) => c.text).join('\n');

/** The checks of the read tools on a server started without --read-only. */
async function readTools(session, check) {
  let note;
  await check('list_notes : la bibliothèque de démonstration, avec « Cours d’optique »', async () => {
    const result = await session.call('list_notes', { limit: 100 });
    if (result.isError) throw new Error(textOf(result));
    const { total, notes } = result.structuredContent;
    note = notes.find((n) => n.title === NOTE);
    if (!note) throw new Error(`« ${NOTE} » absente : ${notes.map((n) => n.title).join(', ')}`);
    if (total < 8) throw new Error(`seulement ${total} notes`);
    if (!/^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}[+-Z]/.test(note.created)) throw new Error(`date non ISO : ${note.created}`);
    if (!(note.page.width_mm === 297 && note.page.height_mm === 216)) throw new Error(`page : ${JSON.stringify(note.page)}`);
  });
  await check('list_binders et list_tags', async () => {
    const binders = (await session.call('list_binders')).structuredContent;
    if (!Array.isArray(binders.binders) || binders.total_notes < 8) throw new Error(JSON.stringify(binders).slice(0, 300));
    const tags = (await session.call('list_tags')).structuredContent;
    if (!tags.tags.some((t) => t.tag === 'optique')) throw new Error(JSON.stringify(tags).slice(0, 300));
  });
  await check('get_note : métadonnées, calques et traits en millimètres', async () => {
    const byId = (await session.call('get_note', { note: note.id, strokes: 'summary' })).structuredContent;
    const byPath = (await session.call('get_note', { note: note.path, strokes: 'points', max_points: 300 })).structuredContent;
    if (byId.note.id !== note.id || byPath.note.id !== note.id) throw new Error('la note n’est pas retrouvée par son id et par son chemin');
    if (!byId.strokes.length || !byId.strokes[0].bounds_mm) throw new Error('pas de traits');
    const points = byPath.strokes.reduce((sum, s) => sum + (s.points_mm?.length ?? 0), 0);
    if (points === 0 || points > 300) throw new Error(`${points} points pour un budget de 300`);
  });
  await check('render_note : une vraie image PNG de la page et d’une zone', async () => {
    const page = await session.call('render_note', { note: note.id });
    const image = page.content.find((c) => c.type === 'image');
    if (!image || image.mimeType !== 'image/png') throw new Error(`pas d'image PNG : ${JSON.stringify(page).slice(0, 200)}`);
    const bytes = Buffer.from(image.data, 'base64');
    if (bytes.subarray(0, 8).toString('hex') !== '89504e470d0a1a0a') throw new Error('signature PNG absente');
    const width = bytes.readUInt32BE(16);
    const height = bytes.readUInt32BE(20);
    if (Math.max(width, height) > 2000 || width < 500) throw new Error(`taille ${width} x ${height}`);
    const info = JSON.parse(textOf(page));
    if (info.width_px !== width || info.height_px !== height) throw new Error(`taille annoncée ${info.width_px} x ${info.height_px}`);
    const zone = await session.call('render_note', { note: note.id, dpi: 300, region: { x: 20, y: 20, width: 100, height: 60 } });
    const crop = Buffer.from(zone.content.find((c) => c.type === 'image').data, 'base64');
    if (crop.readUInt32BE(16) < 1000) throw new Error('la zone n’est pas rendue à 300 dpi');
  });
  await check('get_note_text : le texte reconnu de « Cours d’optique » contient ses mots', async () => {
    const result = await session.call('get_note_text', { note: note.id });
    if (result.isError) throw new Error(`${textOf(result)} (il faut la reconnaissance d’écriture française de Windows)`);
    const { text, lines, structure } = result.structuredContent;
    const words = fold(text);
    const missing = NOTE_WORDS.filter((w) => !words.includes(w));
    if (missing.length) throw new Error(`mots manquants ${missing.join(', ')} dans « ${text} »`);
    if (!lines.length || !(lines[0].bounds_mm.width > 0)) throw new Error('lignes sans position');
    if (!structure.items.length) throw new Error('structure vide');
  });
  await check('search_notes : « lumiere » (sans accent) trouve « Cours d’optique » par son écriture', async () => {
    // The search sees the notes the recognition has read: get_note_text just read this one.
    const result = (await session.call('search_notes', { query: 'lumiere' })).structuredContent;
    if (!result.notes.some((n) => n.title === NOTE && n.matched_in.includes('text'))) {
      throw new Error(JSON.stringify(result).slice(0, 400));
    }
    const byTitle = (await session.call('search_notes', { query: 'optique' })).structuredContent;
    if (!byTitle.notes.some((n) => n.title === NOTE)) throw new Error('le titre n’est pas trouvé');
  });
  await check('les erreurs sont des résultats d’outil avec le code de l’interface', async () => {
    const missing = await session.call('get_note', { note: 'Absente/introuvable.ardoise' });
    if (!missing.isError || missing.structuredContent.code !== 'noteNotFound') throw new Error(JSON.stringify(missing));
    const bad = await session.call('list_notes', { sort: 'au hasard' });
    if (!bad.isError) throw new Error('un argument invalide doit être refusé');
  });
}

/**
 * The whole journey: a full server, then a `--read-only` one. `log(line)` prints a step; returns
 * the number of failed checks.
 */
export async function mcpJourney({ exe, language = 'fr', log = console.log } = {}) {
  let failures = 0;
  const check = async (name, body) => {
    const started = Date.now();
    try {
      await body();
      log(`  ok    ${name} (${Date.now() - started} ms)`);
    } catch (error) {
      failures++;
      log(`  ECHEC ${name} : ${error.message}`);
    }
  };

  const app = prepareApp(exe, language);
  try {
    log(`MCP (${language}) : serveur complet`);
    const session = startServer(app);
    try {
      await check('initialize : le serveur se présente', async () => {
        const result = await session.initialize();
        if (result.serverInfo?.name !== 'ardoise') throw new Error(JSON.stringify(result).slice(0, 300));
        if (!result.capabilities?.tools) throw new Error('pas de capacité « tools »');
      });
      await check('tools/list : tous les outils de lecture, avec schéma et annotations', async () => {
        const tools = await session.tools();
        const names = tools.map((t) => t.name);
        const missing = READ_TOOLS.filter((n) => !names.includes(n));
        if (missing.length) throw new Error(`outils absents : ${missing.join(', ')}`);
        for (const tool of tools.filter((t) => READ_TOOLS.includes(t.name))) {
          if (tool.inputSchema?.type !== 'object') throw new Error(`${tool.name} : pas de schéma`);
          if (tool.annotations?.readOnlyHint !== true) throw new Error(`${tool.name} : readOnlyHint absent`);
          if (!tool.description) throw new Error(`${tool.name} : pas de description`);
        }
      });
      await check('prompts/list répond', async () => {
        const answer = await session.request('prompts/list', {});
        if (answer.error || !Array.isArray(answer.result.prompts)) throw new Error(JSON.stringify(answer));
      });
      await readTools(session, check);
      await check('rien d’autre que du protocole sur la sortie standard, et sortie propre à la fermeture', async () => {
        const stray = session.stdoutLines.filter((line) => {
          try {
            return JSON.parse(line).jsonrpc !== '2.0';
          } catch {
            return true;
          }
        });
        if (stray.length) throw new Error(`lignes étrangères : ${stray[0].slice(0, 200)}`);
        const outcome = await session.close();
        if (outcome === 'timeout' || outcome.code !== 0) throw new Error(`fin : ${JSON.stringify(outcome)} ; stderr : ${session.stderr.slice(-300)}`);
      });
    } finally {
      session.child.kill();
    }

    log(`MCP (${language}) : serveur --read-only`);
    const readOnly = startServer(app, ['--read-only']);
    try {
      await check('--read-only : aucun outil d’écriture ni de passerelle', async () => {
        await readOnly.initialize();
        const names = (await readOnly.tools()).map((t) => t.name);
        const extra = names.filter((n) => !READ_TOOLS.includes(n));
        if (extra.length) throw new Error(`outils en trop : ${extra.join(', ')}`);
        if (WRITE_TOOLS.some((n) => names.includes(n))) throw new Error('un outil d’écriture est présent');
        const refused = await readOnly.request('tools/call', { name: 'trash_note', arguments: { note: 'x' } });
        if (!refused.error && !refused.result?.isError) throw new Error('trash_note aurait dû être refusé');
      });
      await check('--read-only : les outils de lecture marchent', async () => {
        const result = await readOnly.call('list_notes', { limit: 3 });
        if (result.isError || result.structuredContent.notes.length !== 3) throw new Error(textOf(result));
      });
    } finally {
      await readOnly.close(3000);
    }
    for (const part of parts) {
      await part({ makeApp: () => prepareApp(exe, language), startServer, check, textOf, fold, language, log });
    }
  } finally {
    try {
      rmSync(app.dir, { recursive: true, force: true });
    } catch {
      // The temporary folder is left to the system when Windows still holds a file in it.
    }
  }
  return failures;
}

// `node app/e2e/mcp.mjs <ardoise.exe> [fr|en]`
if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const exe = process.argv[2];
  if (!exe) {
    console.error('usage: node app/e2e/mcp.mjs <ardoise.exe> [fr|en]');
    process.exit(2);
  }
  const failures = await mcpJourney({ exe, language: process.argv[3] ?? 'fr' });
  console.log(failures === 0 ? 'MCP OK' : `MCP : ${failures} échec(s)`);
  process.exit(failures === 0 ? 0 : 1);
}
