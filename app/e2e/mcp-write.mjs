// End-to-end checks of the MCP write tools and the prompts (milestone M9, part 2), run by
// `mcpJourney` (app/e2e/mcp.mjs) on a demo library of their own, with a real `ardoise.exe --mcp`
// on pipes: create, update, move, trash, merge, split, straighten, export (png, md, pdf, a
// replay is too long for a journey), the search following the library after a write,
// set_note_text then search, and the prompts in the language of the app (also those of a
// --read-only server). No window is driven.
//
// By hand: `node app/e2e/mcp.mjs target/debug/ardoise.exe fr` (then `en`).
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { deflateSync } from 'node:zlib';

export const WRITE_ONLY_TOOLS = [
  'create_note', 'update_note', 'move_note', 'trash_note', 'merge_notes', 'split_note',
  'straighten_shapes', 'export_note', 'set_note_text', 'preview_revision', 'create_revision', 'edit_revision',
];
/** The write tools that change or remove what is there (`destructiveHint: true`). */
const DESTRUCTIVE = ['update_note', 'move_note', 'trash_note', 'split_note', 'straighten_shapes', 'edit_revision'];
/** The tool of the write group that writes nothing (it prepares what `create_revision` writes). */
const READ_ONLY_HINT = ['preview_revision'];
const PROMPTS = {
  fr: ['mettre_au_propre', 'resume_seance', 'titrer_et_etiqueter'],
  en: ['clean_up', 'lesson_summary', 'title_and_tag'],
};
/** The prompt that writes: only a server that is not read-only has it. */
const WRITING_PROMPT = {
  fr: 'ameliorer_note',
  en: 'improve_note',
};

const PNG_SIGNATURE = '89504e470d0a1a0a';

function crc32(bytes) {
  let crc = ~0;
  for (const byte of bytes) {
    crc ^= byte;
    for (let k = 0; k < 8; k++) crc = (crc >>> 1) ^ (0xedb88320 & -(crc & 1));
  }
  return ~crc >>> 0;
}

/** A small valid PNG (a grey rectangle with a dark frame). */
function smallPng(width = 60, height = 40) {
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
    const out = Buffer.alloc(8 + data.length + 4);
    out.writeUInt32BE(data.length, 0);
    body.copy(out, 4);
    out.writeUInt32BE(crc32(body), 8 + data.length);
    return out;
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 6, 0, 0, 0], 8); // 8 bits, RGBA
  const rows = [];
  for (let y = 0; y < height; y++) {
    const row = Buffer.alloc(1 + width * 4);
    for (let x = 0; x < width; x++) {
      const edge = x === 0 || y === 0 || x === width - 1 || y === height - 1;
      row.set(edge ? [30, 30, 30, 255] : [225, 235, 245, 255], 1 + x * 4);
    }
    rows.push(row);
  }
  return Buffer.concat([
    Buffer.from(PNG_SIGNATURE, 'hex'),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(Buffer.concat(rows))),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/** Every note of the library, by title. */
async function notesByTitle(session) {
  const { notes } = (await session.call('list_notes', { limit: 500 })).structuredContent;
  return new Map(notes.map((n) => [n.title, n]));
}

/**
 * The checks of the write tools and of the prompts, on a server started on `makeApp()`'s own
 * demo library (the other parts of the journey keep theirs untouched). `check(name, body)` runs
 * one check.
 */
export async function mcpWriteTools({ makeApp, startServer, check, textOf, fold, language }) {
  const app = makeApp();
  const out = mkdtempSync(join(tmpdir(), 'ardoise-mcp-out-'));
  const session = startServer(app);
  const expectError = async (name, args, code) => {
    const result = await session.call(name, args);
    if (!result.isError) throw new Error(`${name} aurait dû échouer : ${JSON.stringify(args).slice(0, 200)}`);
    const got = result.structuredContent?.code ?? 'invalidArguments';
    if (got !== code) throw new Error(`${name} : code ${got} au lieu de ${code} (${textOf(result).slice(0, 200)})`);
  };
  try {
    await session.initialize();

    await check('tools/list : les douze outils d’écriture, destructeurs ceux qui changent ou retirent', async () => {
      const tools = await session.tools();
      for (const name of WRITE_ONLY_TOOLS) {
        const tool = tools.find((t) => t.name === name);
        if (!tool) throw new Error(`${name} absent`);
        const readOnly = READ_ONLY_HINT.includes(name);
        if (tool.annotations?.readOnlyHint !== readOnly) throw new Error(`${name} : readOnlyHint doit être ${readOnly}`);
        const destructive = DESTRUCTIVE.includes(name);
        if (tool.annotations?.destructiveHint !== destructive) throw new Error(`${name} : destructiveHint doit être ${destructive}`);
        if (tool.inputSchema?.type !== 'object' || !tool.description) throw new Error(`${name} : schéma ou description absents`);
      }
    });

    // --- create_note, update_note, move_note, trash_note ------------------------------------
    let created;
    let straightenedCopy;
    await check('create_note : une note vierge dans un classeur créé, avec titre, étiquettes et papier', async () => {
      const result = await session.call('create_note', {
        title: 'Note de test',
        binder: 'E2E/Création',
        tags: ['essai', 'MCP'],
        paper: 'a5',
        template: 'seyes',
      });
      if (result.isError) throw new Error(textOf(result));
      created = result.structuredContent.note;
      if (created.title !== 'Note de test' || created.binder !== 'E2E/Création') throw new Error(JSON.stringify(created));
      if (created.page.width_mm !== 148 || created.page.height_mm !== 210) throw new Error(`page : ${JSON.stringify(created.page)}`);
      const read = (await session.call('get_note', { note: created.id })).structuredContent;
      if (read.background.template !== 'seyes') throw new Error(`gabarit : ${read.background.template}`);
      if (read.layers[0].name !== (language === 'fr' ? 'Calque 1' : 'Layer 1')) throw new Error(`calque : ${read.layers[0].name}`);
    });
    await check('create_note : sur une image donnée par un chemin absolu ; le chemin relatif est refusé', async () => {
      const image = join(out, 'schéma.png');
      writeFileSync(image, smallPng());
      const result = await session.call('create_note', { from_file: image, title: 'Sur image', binder: 'E2E/Création' });
      if (result.isError) throw new Error(textOf(result));
      const note = (await session.call('get_note', { note: result.structuredContent.note.id })).structuredContent;
      const background = note.background.image;
      if (!background || background.name !== 'schéma.png') throw new Error(`image de fond : ${JSON.stringify(background)}`);
      await expectError('create_note', { from_file: 'schéma.png' }, 'invalidArguments');
    });
    await check('update_note : titre (le fichier est renommé), étiquettes, favori', async () => {
      const result = await session.call('update_note', {
        note: created.id, title: 'Note renommée', add_tags: ['plus'], remove_tags: ['mcp'], favorite: true,
      });
      if (result.isError) throw new Error(textOf(result));
      const { note, changed } = result.structuredContent;
      if (note.title !== 'Note renommée' || !note.favorite) throw new Error(JSON.stringify(note));
      if (note.tags.join() !== 'essai,plus') throw new Error(`étiquettes : ${note.tags}`);
      if (!note.path.includes('Note renommée') || !changed.includes('title')) throw new Error(`renommage : ${note.path}`);
      created = note;
    });
    await check('move_note : vers un autre classeur ; un nom pris reçoit « (2) », rien n’est remplacé', async () => {
      const moved = await session.call('move_note', { note: created.id, binder: 'E2E/Archive' });
      if (moved.isError || moved.structuredContent.note.binder !== 'E2E/Archive') throw new Error(textOf(moved));
      const twin = (await session.call('create_note', { title: 'Note renommée', binder: 'E2E/Création' })).structuredContent.note;
      const twinMoved = await session.call('move_note', { note: twin.id, binder: 'E2E/Archive' });
      if (twinMoved.isError) throw new Error(textOf(twinMoved));
      const archive = readdirSync(join(app.library, 'E2E', 'Archive')).filter((f) => f.endsWith('.ardoise'));
      if (archive.length !== 2) throw new Error(`fichiers : ${archive.join(', ')}`);
      await expectError('move_note', { note: created.id, binder: '../dehors' }, 'invalidName');
      created = twinMoved.structuredContent.note;
    });
    await check('trash_note : la note va à la corbeille de la bibliothèque, rien n’est supprimé', async () => {
      const result = await session.call('trash_note', { note: created.id });
      if (result.isError) throw new Error(textOf(result));
      if (!result.structuredContent.trash_id) throw new Error('pas d’identifiant de corbeille');
      if ((await notesByTitle(session)).has('Note renommée') && (await notesByTitle(session)).get('Note renommée').id === created.id) {
        throw new Error('la note est encore dans la liste');
      }
      const trash = readdirSync(join(app.library, '_ardoise', 'trash')).filter((f) => f.endsWith('.ardoise'));
      if (trash.length !== 1) throw new Error(`corbeille : ${trash.join(', ')}`);
      await expectError('trash_note', { note: created.id }, 'noteNotFound');
    });

    // --- merge_notes, split_note, straighten_shapes -----------------------------------------
    const notes = await notesByTitle(session);
    const optique = notes.get("Cours d'optique");
    const ohm = [...notes.values()].find((n) => n.title.startsWith('Électricité'));
    const chimie = [...notes.values()].find((n) => /chimique/i.test(n.title) && n.stroke_count > 0);
    const searchIds = async (query) => {
      const result = await session.call('search_notes', { query });
      if (result.isError) throw new Error(textOf(result));
      return { ids: result.structuredContent.notes.map((n) => n.id), text: result.structuredContent.text_search };
    };
    await check('search_notes avant les écritures : « lumiere » trouve « Cours d’optique » par son écriture', async () => {
      const { ids, text } = await searchIds('lumiere');
      if (!ids.includes(optique.id)) throw new Error(`non trouvé (il faut la reconnaissance d’écriture de Windows) : ${JSON.stringify(text)}`);
    });
    let merged;
    await check('merge_notes : une note nouvelle, les sources intactes', async () => {
      const result = await session.call('merge_notes', { notes: [optique.id, ohm.id], layout: 'endToEnd', title: 'Fusion' });
      if (result.isError) throw new Error(textOf(result));
      const { note } = result.structuredContent;
      merged = note;
      if (note.title !== 'Fusion' || note.stroke_count !== optique.stroke_count + ohm.stroke_count) throw new Error(JSON.stringify(note));
      if (!(note.page.height_mm > optique.page.height_mm * 1.9)) throw new Error(`page : ${JSON.stringify(note.page)}`);
      const after = await notesByTitle(session);
      if (after.get("Cours d'optique").stroke_count !== optique.stroke_count) throw new Error('une source a changé');
      await expectError('merge_notes', { notes: [optique.id], layout: 'overlay' }, 'nothingToMerge');
    });
    await check('search_notes suit la bibliothèque : la note fusionnée, faite après la première recherche, est trouvée par son écriture', async () => {
      const { ids, text } = await searchIds('lumiere');
      if (!ids.includes(merged.id)) throw new Error(`la nouvelle note n’est pas cherchée : ${JSON.stringify(text)}`);
      if (text.unread_notes !== 0) throw new Error(`notes non lues : ${JSON.stringify(text)}`);
    });
    await check('split_note : à un instant, deux notes qui se partagent les traits', async () => {
      const read = (await session.call('get_note', { note: optique.id, strokes: 'summary' })).structuredContent;
      const starts = read.strokes.map((s) => s.start_ms).sort((a, b) => a - b);
      const at = starts[Math.floor(starts.length / 2)];
      const result = await session.call('split_note', { note: optique.id, at_ms: at });
      if (result.isError) throw new Error(textOf(result));
      const { kept, created: part } = result.structuredContent;
      if (kept.id !== optique.id || kept.stroke_count + part.stroke_count !== optique.stroke_count) throw new Error(JSON.stringify(result.structuredContent));
      if (!kept.stroke_count || !part.stroke_count) throw new Error('une moitié est vide');
      await expectError('split_note', { note: optique.id }, 'invalidArguments');
    });
    await check('straighten_shapes : les dessins que trouve l’analyse de l’encre, redressés sur une copie, l’original intact', async () => {
      const before = (await session.call('get_note', { note: chimie.id, strokes: 'summary' })).structuredContent;
      const sourceFile = join(app.library, ...chimie.path.split('/'));
      const sourceBytes = readFileSync(sourceFile);
      const trashBefore = readdirSync(join(app.library, '_ardoise', 'trash')).length;
      const auto = await session.call('straighten_shapes', { note: chimie.id });
      if (auto.isError) throw new Error(`${textOf(auto)} (il faut l’analyse de l’encre de Windows)`);
      let result = auto.structuredContent;
      if (result.chosen_by !== 'ink_analysis') throw new Error(JSON.stringify(result).slice(0, 300));
      if (!result.saved) {
        // No drawing of the demo note is a clear shape: every stroke, given.
        const all = before.strokes.map((_, index) => index);
        const given = await session.call('straighten_shapes', { note: chimie.id, strokes: all });
        if (given.isError) throw new Error(textOf(given));
        result = given.structuredContent;
      }
      const { straightened, saved, shapes, note: copy } = result;
      if (!(straightened >= 1) || !saved || !shapes.length) throw new Error(JSON.stringify(result).slice(0, 300));
      // The original is the same bytes, and no copy of it went to the trash.
      if (!result.made_a_copy || result.original_unchanged !== true) throw new Error(JSON.stringify(result).slice(0, 300));
      if (!readFileSync(sourceFile).equals(sourceBytes)) throw new Error('le fichier de l’original a changé');
      const after = (await session.call('get_note', { note: chimie.id })).structuredContent;
      if (after.note.modified !== before.note.modified || after.note.stroke_count !== before.note.stroke_count) throw new Error('l’original a changé');
      if (readdirSync(join(app.library, '_ardoise', 'trash')).length !== trashBefore) throw new Error('la corbeille a changé');
      // The copy is a note next to it, a revision of it.
      const suffix = language === 'fr' ? '(propre)' : '(clean)';
      if (copy.id === chimie.id || !copy.title.endsWith(suffix) || copy.binder !== chimie.binder) throw new Error(`copie : ${JSON.stringify(copy)}`);
      const made = (await session.call('get_note', { note: copy.id })).structuredContent;
      if (made.revision?.of !== chimie.id || made.revision.account.length !== 1) throw new Error(`copie : ${JSON.stringify(made.revision)}`);
      await expectError('straighten_shapes', { note: chimie.id, strokes: [100000] }, 'invalidArguments');
      straightenedCopy = copy;
    });
    await check('edit_revision et straighten_shapes sur la copie : modifiée sur place, la version d’avant dans la corbeille', async () => {
      const copy = straightenedCopy;
      const trashBefore = readdirSync(join(app.library, '_ardoise', 'trash'));
      const edited = await session.call('edit_revision', {
        note: copy.id,
        orders: [{ op: 'recolor', select: { ids: [0] }, color: '#cc0000' }],
        summary: language === 'fr' ? 'Premier trait en rouge.' : 'First stroke in red.',
      });
      if (edited.isError) throw new Error(textOf(edited));
      const { previous_version: kept, saved, account_entries: entries, note } = edited.structuredContent;
      const suffix = language === 'fr' ? '(avant retouche)' : '(before retouching)';
      if (!saved || entries !== 2 || note.id !== copy.id) throw new Error(JSON.stringify(edited.structuredContent).slice(0, 300));
      if (!kept?.title?.endsWith(suffix) || !kept.in_trash || kept.id === copy.id) throw new Error(`version d’avant : ${JSON.stringify(kept)}`);
      const trash = readdirSync(join(app.library, '_ardoise', 'trash'));
      if (!trash.includes(kept.in_trash) || trash.length !== trashBefore.length + 1) throw new Error(`pas dans la corbeille : ${trash.join(', ')}`);
      // The shapes of a copy are straightened in it, and its version before goes to the trash too.
      const again = await session.call('straighten_shapes', { note: copy.id, strokes: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9] });
      if (again.isError) throw new Error(textOf(again));
      if (again.structuredContent.saved) {
        const earlier = again.structuredContent.previous_version;
        const word = language === 'fr' ? '(avant redressement)' : '(before straightening)';
        if (again.structuredContent.made_a_copy !== false || !earlier?.title?.endsWith(word)) throw new Error(JSON.stringify(again.structuredContent).slice(0, 300));
        if (!readdirSync(join(app.library, '_ardoise', 'trash')).includes(earlier.in_trash)) throw new Error('pas dans la corbeille');
      }
    });

    // --- export_note -------------------------------------------------------------------------
    await check('export_note : png et pdf dans un dossier, « (2) » à la seconde fois, rien d’écrasé', async () => {
      const folder = join(out, 'exports');
      const args = { notes: [ohm.id], format: 'png', path: folder, dpi: 72 };
      const first = await session.call('export_note', args);
      if (first.isError) throw new Error(textOf(first));
      const [file] = first.structuredContent.files;
      const bytes = readFileSync(file);
      if (bytes.subarray(0, 8).toString('hex') !== PNG_SIGNATURE) throw new Error('ce n’est pas un PNG');
      const before = readFileSync(file).length;
      const second = await session.call('export_note', args);
      const [other] = second.structuredContent.files;
      if (!other.endsWith(' (2).png') || other === file) throw new Error(`second export : ${other}`);
      if (readFileSync(file).length !== before) throw new Error('le premier fichier a été modifié');
      const pdf = await session.call('export_note', { notes: [ohm.id, chimie.id], format: 'pdf', path: join(folder, 'classe.pdf') });
      if (pdf.isError || readFileSync(pdf.structuredContent.files[0]).subarray(0, 4).toString() !== '%PDF') throw new Error(textOf(pdf));
    });
    await check('export_note : les chemins relatifs, les noms de fichier faux et les options hors limites sont refusés', async () => {
      await expectError('export_note', { notes: [ohm.id], format: 'png', path: 'exports/a.png' }, 'invalidArguments');
      await expectError('export_note', { notes: [ohm.id], format: 'png', path: join(out, 'a.jpg') }, 'invalidArguments');
      await expectError('export_note', { notes: [ohm.id], format: 'png', path: out, dpi: 5000 }, 'invalidArguments');
      await expectError('export_note', { notes: [ohm.id, chimie.id], format: 'mp4', path: out }, 'invalidArguments');
      await expectError('export_note', { notes: [ohm.id], format: 'webm', path: out, duration_seconds: 3600 }, 'invalidArguments');
      await expectError('export_note', { notes: [ohm.id], format: 'gif', path: out, speed: 1000 }, 'invalidArguments');
      // A refused export leaves no folder behind.
      await expectError('export_note', { notes: [ohm.id], format: 'png', path: join(out, 'neuf', 'x.png'), dpi: 5000 }, 'invalidArguments');
      if (existsSync(join(out, 'neuf'))) throw new Error('un dossier a été créé pour un export refusé');
    });
    await check('export_note : le texte en Markdown, avec ses dessins (reconnaissance de Windows)', async () => {
      const result = await session.call('export_note', { notes: [optique.id], format: 'md', path: join(out, 'texte'), language });
      if (result.isError) throw new Error(`${textOf(result)} (il faut la reconnaissance d’écriture de Windows)`);
      const [md] = result.structuredContent.files;
      if (!md.endsWith('.md')) throw new Error(`fichiers : ${result.structuredContent.files}`);
      const text = fold(readFileSync(md, 'utf8'));
      if (!text.includes('lumiere')) throw new Error(`le texte n’a pas les mots de la note : ${text.slice(0, 200)}`);
      // Its name taken by another export's drawings (its `.md` gone): the next name, those
      // drawings untouched.
      const stem = md.slice(0, -'.md'.length);
      rmSync(md);
      const images = `${stem}_images`;
      const theirs = join(images, 'mine.png');
      if (!existsSync(images)) mkdirSync(images);
      writeFileSync(theirs, 'theirs');
      const again = await session.call('export_note', { notes: [optique.id], format: 'md', path: join(out, 'texte'), language });
      if (again.isError) throw new Error(textOf(again));
      const [other] = again.structuredContent.files;
      if (other !== `${stem} (2).md`) throw new Error(`second export : ${other}`);
      if (readFileSync(theirs, 'utf8') !== 'theirs') throw new Error('un fichier du dossier des dessins a changé');
    });

    // --- set_note_text -----------------------------------------------------------------------
    await check('set_note_text : le mot corrigé est trouvé par search_notes, get_note_text le signale', async () => {
      const probe = fold('réfraction');
      const before = await session.call('search_notes', { query: probe });
      if (before.isError || before.structuredContent.notes.some((n) => n.id === chimie.id)) throw new Error(textOf(before));
      const file = join(app.library, ...chimie.path.split('/'));
      const bytes = readFileSync(file);
      const stored = await session.call('set_note_text', { note: chimie.id, text: 'Cours de chimie\nLa réfraction dévie le rayon', language });
      if (stored.isError) throw new Error(textOf(stored));
      const found = await session.call('search_notes', { query: probe });
      if (found.isError || !found.structuredContent.notes.some((n) => n.id === chimie.id && n.matched_in.includes('text'))) {
        throw new Error(`non trouvé : ${JSON.stringify(found.structuredContent).slice(0, 400)}`);
      }
      const text = (await session.call('get_note_text', { note: chimie.id })).structuredContent;
      if (!text.is_correction || !fold(text.text).includes('refraction') || !text.recognised_text) throw new Error(JSON.stringify(text).slice(0, 300));
      if (!readFileSync(file).equals(bytes)) throw new Error('le fichier de la note a été modifié');
      const removed = await session.call('set_note_text', { note: chimie.id, text: '' });
      const gone = await session.call('search_notes', { query: probe });
      if (removed.isError || gone.structuredContent.notes.some((n) => n.id === chimie.id)) throw new Error('la correction n’a pas été retirée');
    });

    // --- prompts -----------------------------------------------------------------------------
    await check('prompts/list : les quatre prompts dans la langue de l’application', async () => {
      const answer = await session.request('prompts/list', {});
      if (answer.error) throw new Error(JSON.stringify(answer.error));
      const names = answer.result.prompts.map((p) => p.name).sort();
      const expected = [...PROMPTS[language], WRITING_PROMPT[language]].sort();
      if (names.join() !== expected.join()) throw new Error(`prompts : ${names}`);
      for (const prompt of answer.result.prompts) if (!prompt.description) throw new Error(`${prompt.name} sans description`);
    });
    await check('prompts/get : chaque message nomme ses outils, dans l’ordre', async () => {
      const [clean, lesson, title] = [PROMPTS[language][0], PROMPTS[language][1], PROMPTS[language][2]];
      const get = async (name, args) => {
        const answer = await session.request('prompts/get', { name, arguments: args });
        if (answer.error) throw new Error(`${name} : ${JSON.stringify(answer.error)}`);
        return answer.result.messages.map((m) => m.content.text).join('\n');
      };
      const inOrder = (text, names, label) => {
        let from = 0;
        for (const n of names) {
          const at = text.indexOf(n, from);
          if (at < 0) throw new Error(`${label} : « ${n} » absent ou dans le désordre`);
          from = at + n.length;
        }
      };
      const cleaning = await get(clean, { note: ohm.id, format: 'latex' });
      inOrder(cleaning, ['get_note', 'render_note', 'region', 'get_note_text', 'export_note'], clean);
      for (const p of ['LuaLaTeX', 'circuitikz', 'mhchem', 'siunitx', 'pgfplots', 'chemfig']) {
        if (!cleaning.includes(p)) throw new Error(`${clean} : ${p} absent`);
      }
      if (!(await get(clean, {})).includes('last_live_note')) throw new Error(`${clean} sans note : last_live_note absent`);
      const titles = await get(title, {});
      inOrder(titles, ['render_note', 'get_note_text', 'update_note'], title);
      const untitled = [...(await notesByTitle(session)).values()].find((n) => n.title === '');
      if (!untitled || !titles.includes(untitled.id)) throw new Error(`${title} : la note sans titre n’est pas listée`);
      const day = (await session.call('get_note', { note: ohm.id })).structuredContent.note.created.slice(0, 10);
      const summary = await get(lesson, { date: day });
      inOrder(summary, ['get_note_text', 'render_note'], lesson);
      if (!summary.includes(ohm.id)) throw new Error(`${lesson} : la note du jour n’est pas listée`);
      const bad = await session.request('prompts/get', { name: lesson, arguments: { date: 'demain' } });
      if (!bad.error) throw new Error(`${lesson} : une date fausse doit être refusée`);
      const improve = await get(WRITING_PROMPT[language], { note: ohm.id, goals: 'ranger l’écriture' });
      inOrder(improve, ['get_note', 'render_note', 'ink_lines', 'preview_revision', 'create_revision'], WRITING_PROMPT[language]);
      if (!improve.includes(ohm.id) || !improve.includes('ranger l’écriture')) throw new Error(`${WRITING_PROMPT[language]} : note ou demande absentes`);
    });
    await check('prompts d’un serveur --read-only : aucun outil absent n’y est nommé', async () => {
      const readOnly = startServer(app, ['--read-only']);
      try {
        await readOnly.initialize();
        const listed = (await readOnly.request('prompts/list', {})).result.prompts.map((p) => p.name).sort();
        if (listed.join() !== PROMPTS[language].slice().sort().join()) throw new Error(`prompts en lecture seule : ${listed}`);
        const present = (await readOnly.tools()).map((t) => t.name);
        const absent = (await session.tools()).map((t) => t.name).filter((n) => !present.includes(n));
        const [clean, lesson, title] = PROMPTS[language];
        const texts = [];
        for (const args of [{}, { note: ohm.id, format: 'markdown' }, { format: 'word' }]) {
          const answer = await readOnly.request('prompts/get', { name: clean, arguments: args });
          if (answer.error) throw new Error(`${clean} : ${JSON.stringify(answer.error)}`);
          texts.push(answer.result.messages.map((m) => m.content.text).join('\n'));
        }
        for (const name of [title, lesson]) {
          const answer = await readOnly.request('prompts/get', { name, arguments: {} });
          if (answer.error) throw new Error(`${name} : ${JSON.stringify(answer.error)}`);
          texts.push(answer.result.messages.map((m) => m.content.text).join('\n'));
        }
        for (const text of texts) {
          const named = absent.filter((tool) => text.includes(`\`${tool}\``));
          if (named.length) throw new Error(`outils absents nommés : ${named.join(', ')}`);
        }
      } finally {
        await readOnly.close(3000);
      }
    });
  } finally {
    session.child.kill();
    for (const dir of [app.dir, out]) {
      try {
        if (existsSync(dir)) rmSync(dir, { recursive: true, force: true });
      } catch {
        // Left to the system when Windows still holds a file in it.
      }
    }
  }
}
