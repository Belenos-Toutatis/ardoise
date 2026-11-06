// M13 journey "an assistant retouches a note on a copy" (`preview_revision`, `create_revision`,
// `edit_revision`) and what the editor shows of such a copy.
//
// On the production executable. With the app open, the journey starts `ardoise.exe --mcp` from the
// very copy the runner started (same folder, so same settings and same pipe), as an assistant would:
// it previews a list of orders (a real PNG, and not one file of the library written), makes the
// copy (the source's file is the same bytes afterwards, `edit_revision` refuses the source), then
// has the app open the copy (`open_note` of the bridge): the editor says whose copy it is, « Ce qui
// a changé » shows the account the assistant wrote, « Comparer » shows two pictures that loaded
// and « Ouvrir l'original » opens the source.
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { McpSession } from './mcp.mjs';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const NOTE = "Cours d'optique";
const PNG_SIGNATURE = '89504e470d0a1a0a';
const ACCOUNT = {
  fr: 'Premier trait en rouge, pour essai.\nRien d’autre n’a changé.',
  en: 'First stroke in red, as a test.\nNothing else changed.',
};

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

/** Every file of the library outside its `_ardoise` folder, with a hash of its bytes. */
function snapshot(library) {
  const files = new Map();
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const path = join(dir, entry.name);
      if (entry.isDirectory()) {
        if (entry.name !== '_ardoise') walk(path);
      } else {
        files.set(path, createHash('sha256').update(readFileSync(path)).digest('hex'));
      }
    }
  };
  walk(library);
  return files;
}

/** The difference between two snapshots, as text (empty when they are the same). */
function changes(before, after) {
  const out = [];
  for (const [path, hash] of before) if (after.get(path) !== hash) out.push(`${after.has(path) ? 'changed' : 'removed'}: ${path}`);
  for (const path of after.keys()) if (!before.has(path)) out.push(`new: ${path}`);
  return out.join('\n');
}

const samePath = (a, b) => String(a).replaceAll('/', '\\').toLowerCase() === String(b).replaceAll('/', '\\').toLowerCase();

/** The image of a tool result: a real PNG, with its size. */
function pngOf(result) {
  const image = result.content.find((c) => c.type === 'image');
  if (!image || image.mimeType !== 'image/png') throw new Error(`no PNG image: ${JSON.stringify(result).slice(0, 200)}`);
  const bytes = Buffer.from(image.data, 'base64');
  if (bytes.subarray(0, 8).toString('hex') !== PNG_SIGNATURE) throw new Error('not a PNG');
  return { bytes, width: bytes.readUInt32BE(16), height: bytes.readUInt32BE(20) };
}

export const revisionJourneys = [
  {
    name: 'assistant : copie retouchée, original intact, éditeur (Ouvrir l’original, Ce qui a changé, Comparer)',
    async run(app, check) {
      const l = (key, values = {}) =>
        Object.entries(values).reduce((out, [name, value]) => out.replaceAll(`{${name}}`, value), text(app.language, key));
      const j = JSON.stringify;
      const account = ACCOUNT[app.language];
      const copyTitle = `${NOTE} (${app.language === 'fr' ? 'propre' : 'clean'})`;
      const recolor = { op: 'recolor', select: { ids: [0] }, color: '#ff0000' };
      const session = startMcp(app);
      try {
        let source;
        let sourceBytes;
        let copy;
        await check('--mcp : les outils de la retouche sont là, le prompt aussi', async () => {
          await session.initialize();
          const tools = (await session.tools()).map((t) => t.name);
          for (const name of ['get_strokes', 'ink_lines', 'preview_revision', 'create_revision', 'edit_revision', 'straighten_shapes']) {
            if (!tools.includes(name)) throw new Error(`${name} missing from tools/list`);
          }
          const prompts = (await session.request('prompts/list', {})).result.prompts.map((p) => p.name);
          const wanted = app.language === 'fr' ? 'ameliorer_note' : 'improve_note';
          if (!prompts.includes(wanted)) throw new Error(`${wanted} missing: ${prompts.join(', ')}`);
          const notes = (await ok(session, 'list_notes', { limit: 100 })).notes;
          source = notes.find((n) => n.title === NOTE);
          if (!source) throw new Error(`« ${NOTE} » missing`);
          sourceBytes = readFileSync(join(app.library, ...source.path.split('/')));
        });

        await check('get_strokes et ink_lines lisent la note ; render_note écrit les numéros des traits', async () => {
          const strokes = await ok(session, 'get_strokes', { note: source.id, strokes: [0, 1] });
          if (strokes.strokes.length !== 2 || !strokes.strokes[0].samples.length) throw new Error(JSON.stringify(strokes).slice(0, 300));
          const [i, x, y, pressure, t] = strokes.strokes[0].samples[0];
          if (![i, x, y, pressure, t].every(Number.isFinite)) throw new Error(JSON.stringify(strokes.strokes[0].samples[0]));
          const lines = await ok(session, 'ink_lines', { note: source.id });
          if (!Array.isArray(lines.lines) || !Array.isArray(lines.apart)) throw new Error(JSON.stringify(lines).slice(0, 300));
          const plain = pngOf(await session.call('render_note', { note: source.id, dpi: 100 }));
          const labelled = pngOf(await session.call('render_note', { note: source.id, dpi: 100, labels: true }));
          if (plain.bytes.equals(labelled.bytes)) throw new Error('labels: true draws nothing');
        });

        await check('preview_revision : une vraie image PNG et le rapport, et pas un fichier de la bibliothèque n’est écrit', async () => {
          const before = snapshot(app.library);
          const result = await session.call('preview_revision', { note: source.id, orders: [recolor], labels: true, dpi: 100 });
          if (result.isError) throw new Error(resultText(result));
          const png = pngOf(result);
          if (png.width < 500) throw new Error(`picture ${png.width} x ${png.height}`);
          const info = JSON.parse(resultText(result));
          if (info.preview !== true || info.written !== false || info.labels !== true) throw new Error(JSON.stringify(info).slice(0, 300));
          if (info.report?.orders?.[0]?.op !== 'recolor' || !(info.report.orders[0].selected >= 1)) throw new Error(JSON.stringify(info.report).slice(0, 300));
          const bad = await refused(session, 'preview_revision', { note: source.id, orders: [{ op: 'recolor', select: { ids: [100000] }, color: '#ff0000' }] });
          if (bad.code !== 'invalidOrders') throw new Error(JSON.stringify(bad));
          const diff = changes(before, snapshot(app.library));
          if (diff) throw new Error(`the library changed:\n${diff}`);
        });

        await check('create_revision : une copie à côté, la note d’origine garde exactement les mêmes octets', async () => {
          const made = await ok(session, 'create_revision', { note: source.id, orders: [recolor], summary: account });
          copy = made.note;
          if (made.created !== true || copy.id === source.id || copy.title !== copyTitle) throw new Error(JSON.stringify(made).slice(0, 400));
          if (!readFileSync(join(app.library, ...source.path.split('/'))).equals(sourceBytes)) throw new Error('the source file changed');
          const read = await ok(session, 'get_note', { note: copy.id, strokes: 'summary' });
          if (read.revision?.of !== source.id || read.revision.account?.[0]?.text !== account) throw new Error(JSON.stringify(read.revision));
          if (read.strokes[0].color !== '#ff0000') throw new Error(`first stroke: ${read.strokes[0].color}`);
          if (read.note.favorite !== false) throw new Error('the copy is a favourite');
        });

        await check('edit_revision refuse la note d’origine (notARevision), qui reste intacte ; la copie, elle, se modifie', async () => {
          const error = await refused(session, 'edit_revision', { note: source.id, orders: [recolor], summary: 'essai' });
          if (error.code !== 'notARevision') throw new Error(JSON.stringify(error));
          if (!readFileSync(join(app.library, ...source.path.split('/'))).equals(sourceBytes)) throw new Error('the source file changed');
          const edited = await ok(session, 'edit_revision', {
            note: copy.id,
            orders: [{ op: 'recolor', select: { ids: [1] }, color: '#0000ff' }],
            summary: `${account}\n+ ${app.language === 'fr' ? 'deuxième trait en bleu' : 'second stroke in blue'}`,
          });
          if (edited.saved !== true || edited.account_entries !== 2) throw new Error(JSON.stringify(edited).slice(0, 400));
          if (!readFileSync(join(app.library, ...source.path.split('/'))).equals(sourceBytes)) throw new Error('the source file changed');
        });

        await check('l’application ouvre la copie : la ligne « Copie retouchée de … » est là', async () => {
          const opened = await ok(session, 'open_note', { note: copy.id });
          if (opened.opened.id !== copy.id) throw new Error(JSON.stringify(opened));
          await app.waitFor(
            `(document.querySelector('section.editor')?.dataset.path ?? '').toLowerCase().replaceAll('\\\\', '/').endsWith(${j(copy.path.toLowerCase())})`,
            'the editor showing the copy',
          );
          await app.waitFor(`document.querySelector('[data-slot="revision-line"]') !== null`, 'the line of the revision');
          const line = await app.evaluate(`document.querySelector('[data-slot="revision-line"]').textContent.trim()`);
          const wanted = l('editor.revision.line', { title: NOTE });
          if (line !== wanted) throw new Error(`line « ${line} », expected « ${wanted} »`);
        });

        await check('Ce qui a changé : le compte rendu de l’assistant, daté, les retours à la ligne gardés', async () => {
          await app.click('[data-action="revision-changes"]');
          await app.waitFor(`document.querySelector('[data-dialog="revision-account"]') !== null`, 'the account');
          const entries = await app.evaluate(`[...document.querySelectorAll('[data-slot="revision-account"] li')].map((li) => ({ text: li.querySelector('.revision-text').textContent, time: li.querySelector('time').textContent.trim() }))`);
          if (entries.length !== 2) throw new Error(JSON.stringify(entries));
          if (entries[0].text !== account) throw new Error(`entry « ${entries[0].text} »`);
          if (!entries[0].time) throw new Error('the entry has no date');
          const preserved = await app.evaluate(`getComputedStyle(document.querySelector('[data-slot="revision-account"] .revision-text')).whiteSpace`);
          if (!preserved.startsWith('pre')) throw new Error(`white-space ${preserved}`);
          await app.click('[data-action="revision-close"]');
          await app.waitFor(`document.querySelector('[data-dialog="revision-account"]') === null`, 'the account closed');
        });

        await check('Comparer : l’original et la copie côte à côte, deux images vraiment chargées et différentes', async () => {
          await app.click('[data-action="revision-compare"]');
          const loaded = (slot) =>
            `(() => { const i = document.querySelector('[data-slot="revision-compare-${slot}"] img'); return !!i && i.complete && i.naturalWidth > 0; })()`;
          await app.waitFor(`${loaded('original')} && ${loaded('copy')}`, 'the two pictures', 20_000);
          const [a, b] = await app.evaluate(`['original', 'copy'].map((slot) => document.querySelector('[data-slot="revision-compare-' + slot + '"] img').src)`);
          if (!a.startsWith('data:image/png') || !b.startsWith('data:image/png')) throw new Error('not PNG pictures');
          if (a === b) throw new Error('the two pictures are the same');
          const caption = await app.evaluate(`document.querySelector('[data-slot="revision-compare-original"] figcaption').textContent`);
          if (caption !== l('editor.revision.comparison.original', { title: NOTE })) throw new Error(`caption « ${caption} »`);
          await app.click('[data-action="revision-close"]');
          await app.waitFor(`document.querySelector('[data-dialog="revision-compare"]') === null`, 'the comparison closed');
        });

        await check('Ouvrir l’original : l’éditeur montre la note d’origine, sans la ligne de copie', async () => {
          await app.click('[data-action="revision-open"]');
          await app.waitFor(
            `(document.querySelector('section.editor')?.dataset.path ?? '').toLowerCase().replaceAll('\\\\', '/').endsWith(${j(source.path.toLowerCase())})`,
            'the editor showing the original',
          );
          await app.waitFor(`document.querySelector('[data-slot="revision-bar"]') === null`, 'no line on the original');
          const state = await ok(session, 'app_state');
          if (state.screen !== 'editor' || state.note?.id !== source.id || state.unsaved_changes !== false) throw new Error(JSON.stringify(state));
          if (!samePath(state.library, app.library)) throw new Error(`library ${state.library}`);
          if (!readFileSync(join(app.library, ...source.path.split('/'))).equals(sourceBytes)) throw new Error('the source file changed');
        });
      } finally {
        await session.close(3000);
      }
    },
  },
];
