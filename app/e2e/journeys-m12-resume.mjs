// Milestone M12 journeys "resume live mode on an existing sheet" (SPEC §4.2, docs/questions.md Q9,
// docs/design/tablet-live-classroom.md "M12").
//
// On the production executable with the simulated tablet (`--simulated-tablet`), in French and in English.
// The sheets are written by the journey itself, as a Nextcloud sync would drop them into the library: a real
// `.ardoise` archive (a ZIP, written here with `node:zlib`) holding a background image, three strokes, a first
// version (`original.ardoise`), a recognised text, a narration member, a member and a field of `note.json` that
// this version does not know. What is checked is what a teacher would see (the real window, real clicks) and
// what the file holds afterwards, member by member.
//
// 1. From the card's menu: the live view opens on the sheet, whose background image and strokes are really drawn
//    (pixels of the canvas) in the control window, in the presentation window and on the page of a real browser
//    (headless Edge) that joined the share. The simulated pen writes, the note keeps its path (no duplicate), its
//    members are all there, the first version is untouched; « Revenir à l'original » gives back that first version.
//    Resuming and stopping at once, without a stroke, leaves the file as it was.
// 2. From the editor's toolbar with an unsaved change: the dialog saves, then the sheet opens. The position
//    buttons of a resumed sheet are disabled; the next page is a new note, and they work again.
// 3. A landscape sheet resumed with the tablet held another way: the notice says how to hold it, nothing is turned.
//    A sheet changed on disk during the resume: the notice appears, the file written elsewhere is left as it is and
//    the writing goes on in a new note beside it.
//
// Limit: the narration member is opaque bytes here (the journeys have no microphone and no Opus encoder); that it is
// kept byte for byte is checked, its time base and merge are covered by the Rust tests.
import { mkdirSync, readFileSync, readdirSync, renameSync, statSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { crc32, deflateRawSync, deflateSync, inflateRawSync } from 'node:zlib';
import { assertNoBrowserProblem, findEdge, startBrowser } from './journeys-m11-share.mjs';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
export const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
export const EDITOR_STROKES = '.editor g.ink path[data-stroke]';
export const LIVE_CANVAS = 'canvas.live-canvas';
export const canvasData = (field) => `document.querySelector(${j(LIVE_CANVAS)})?.dataset.${field}`;
export const IMAGE_NAME = 'quadrants.png';

// --- A ZIP archive, a PNG, the strokes of a note ----------------------------------------------------------

/** The members of a ZIP archive, decompressed: `Map` name → bytes (stored and deflated members only, as Ardoise writes). */
export function readZip(buffer) {
  let end = buffer.length - 22;
  while (end >= 0 && buffer.readUInt32LE(end) !== 0x06054b50) end--;
  if (end < 0) throw new Error('not a ZIP archive');
  const count = buffer.readUInt16LE(end + 10);
  let at = buffer.readUInt32LE(end + 16);
  const members = new Map();
  for (let i = 0; i < count; i++) {
    const method = buffer.readUInt16LE(at + 10);
    const size = buffer.readUInt32LE(at + 20);
    const nameLength = buffer.readUInt16LE(at + 28);
    const extraLength = buffer.readUInt16LE(at + 30);
    const commentLength = buffer.readUInt16LE(at + 32);
    const local = buffer.readUInt32LE(at + 42);
    const name = buffer.subarray(at + 46, at + 46 + nameLength).toString('utf8');
    const dataAt = local + 30 + buffer.readUInt16LE(local + 26) + buffer.readUInt16LE(local + 28);
    const raw = buffer.subarray(dataAt, dataAt + size);
    members.set(name, method === 8 ? inflateRawSync(raw) : Buffer.from(raw));
    at += 46 + nameLength + extraLength + commentLength;
  }
  return members;
}

/** A ZIP archive of `members` (`Map` or array of `[name, bytes]`): each deflated, except the ones in `stored`. */
export function writeZip(members, stored = []) {
  const parts = [];
  const central = [];
  let offset = 0;
  for (const [name, bytes] of members) {
    const nameBytes = Buffer.from(name, 'utf8');
    const keep = stored.includes(name);
    const data = keep ? bytes : deflateRawSync(bytes);
    const head = Buffer.alloc(30);
    head.writeUInt32LE(0x04034b50, 0);
    head.writeUInt16LE(20, 4);
    head.writeUInt16LE(0x0800, 6);
    head.writeUInt16LE(keep ? 0 : 8, 8);
    head.writeUInt16LE(0, 10);
    head.writeUInt16LE(0x21, 12); // 1980-01-01
    head.writeUInt32LE(crc32(bytes), 14);
    head.writeUInt32LE(data.length, 18);
    head.writeUInt32LE(bytes.length, 22);
    head.writeUInt16LE(nameBytes.length, 26);
    parts.push(head, nameBytes, data);
    const entry = Buffer.alloc(46);
    entry.writeUInt32LE(0x02014b50, 0);
    entry.writeUInt16LE(20, 4);
    entry.writeUInt16LE(20, 6);
    entry.writeUInt16LE(0x0800, 8);
    entry.writeUInt16LE(keep ? 0 : 8, 10);
    entry.writeUInt16LE(0, 12);
    entry.writeUInt16LE(0x21, 14);
    entry.writeUInt32LE(crc32(bytes), 16);
    entry.writeUInt32LE(data.length, 20);
    entry.writeUInt32LE(bytes.length, 24);
    entry.writeUInt16LE(nameBytes.length, 28);
    entry.writeUInt32LE(offset, 42);
    central.push(entry, nameBytes);
    offset += head.length + nameBytes.length + data.length;
  }
  const directory = Buffer.concat(central);
  const tail = Buffer.alloc(22);
  tail.writeUInt32LE(0x06054b50, 0);
  tail.writeUInt16LE(central.length / 2, 8);
  tail.writeUInt16LE(central.length / 2, 10);
  tail.writeUInt32LE(directory.length, 12);
  tail.writeUInt32LE(offset, 16);
  return Buffer.concat([...parts, directory, tail]);
}

/** A 64 × 48 PNG in four flat colours: red top left, green top right, blue bottom left, yellow bottom right. */
export const QUADRANT_COLORS = [
  [200, 40, 40],
  [40, 160, 40],
  [40, 40, 200],
  [240, 200, 30],
];
export function quadrantsPng() {
  const width = 64;
  const height = 48;
  const raw = Buffer.alloc((width * 3 + 1) * height);
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const color = QUADRANT_COLORS[(y < height / 2 ? 0 : 2) + (x < width / 2 ? 0 : 1)];
      raw.set(color, y * (width * 3 + 1) + 1 + x * 3);
    }
  }
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type, 'latin1'), data]);
    const out = Buffer.alloc(body.length + 8);
    out.writeUInt32BE(data.length, 0);
    body.copy(out, 4);
    out.writeUInt32BE(crc32(body), body.length + 4);
    return out;
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0);
  header.writeUInt32BE(height, 4);
  header.set([8, 2, 0, 0, 0], 8);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

const varint = (value) => {
  const out = [];
  let n = value;
  while (n >= 0x80) {
    out.push((n % 0x80) | 0x80);
    n = Math.floor(n / 0x80);
  }
  out.push(n);
  return out;
};
const svarint = (value) => varint(value >= 0 ? value * 2 : -value * 2 - 1);

/** A straight stroke of `count` points from (x0, y0) to (x1, y1), starting `start` ms after the note was created. */
export function line(x0, y0, x1, y1, start, count = 24) {
  return {
    points: Array.from({ length: count }, (_, i) => ({
      x: x0 + ((x1 - x0) * i) / (count - 1),
      y: y0 + ((y1 - y0) * i) / (count - 1),
      p: 0.6,
      t: start + i * 40,
    })),
  };
}

/**
 * `strokes.bin` for `strokes` (docs/file-format.md §3): black fountain pen, 0.6 mm, first layer, unless a stroke says
 * otherwise (`tool`: its code in `Tool::ALL`, `color`, `width`, `layer`).
 */
export function strokesBin(strokes) {
  const bytes = [0x41, 0x53, 0x54, 0x4b, 1, ...varint(strokes.length)];
  let last = { x: 0, y: 0, p: 0, t: 0 };
  for (const stroke of strokes) {
    const width = Buffer.alloc(4);
    width.writeFloatLE(stroke.width ?? 60, 0);
    const { r, g, b, a } = stroke.color ?? { r: 0, g: 0, b: 0, a: 255 };
    bytes.push(stroke.tool ?? 1, r, g, b, a, ...width, ...varint(stroke.layer ?? 0), ...varint(stroke.points.length));
    for (const point of stroke.points) {
      const next = { x: Math.round(point.x * 100), y: Math.round(point.y * 100), p: Math.round(point.p * 65535), t: point.t };
      bytes.push(...svarint(next.x - last.x), ...svarint(next.y - last.y), ...svarint(next.p - last.p), ...svarint(next.t - last.t));
      last = next;
    }
  }
  return Buffer.from(bytes);
}

/** The number of strokes of a `strokes.bin`, and the bytes after its header (the strokes themselves). */
export function strokesOf(bytes) {
  if (bytes.subarray(0, 4).toString('latin1') !== 'ASTK') throw new Error('not a strokes.bin');
  let at = 5;
  let count = 0;
  for (let shift = 1; ; shift *= 0x80) {
    const byte = bytes[at++];
    count += (byte & 0x7f) * shift;
    if (!(byte & 0x80)) break;
  }
  return { count, body: bytes.subarray(at) };
}

// --- Sheets written into the library ----------------------------------------------------------------------

export const PAGE = { landscape: { width: 29700, height: 21600 }, portrait: { width: 21600, height: 29700 } };
const CREATED = Date.UTC(2026, 9, 28, 9, 0, 0);

/** The ink of a sheet before anything is resumed: three strokes across the page (two diagonals and a base line). */
const inkOf = (page) => [
  line(page.width * 0.1, page.height * 0.12, page.width * 0.9, page.height * 0.45, 0),
  line(page.width * 0.1, page.height * 0.45, page.width * 0.9, page.height * 0.12, 1500),
  line(page.width * 0.1, page.height * 0.9, page.width * 0.9, page.height * 0.9, 3000),
];
/** The first version of a sheet that was edited before: one stroke. */
const firstInkOf = (page) => [line(page.width * 0.2, page.height * 0.5, page.width * 0.8, page.height * 0.5, 0)];
/** What another PC writes into the same file while the sheet is resumed: two other strokes. */
const elsewhereInkOf = (page) => [
  line(page.width * 0.3, page.height * 0.2, page.width * 0.3, page.height * 0.8, 0),
  line(page.width * 0.7, page.height * 0.2, page.width * 0.7, page.height * 0.8, 1000),
];

/**
 * A sheet as an `.ardoise` archive: `note.json` (with a field this version does not know), `strokes.bin`,
 * `original.ardoise` (the first version, which has the same background image placement), `background.png`,
 * `audio.opus` (opaque bytes), `text.json` and a member nobody knows. Returns `{ bytes, members, id }`.
 */
export function sheet({ title, orientation = 'landscape', index = 1, image = true, ink, layers = [{ name: 'Layer 1', visible: true }], mark = 'kept as it is' }) {
  const page = PAGE[orientation];
  const id = `00000000-0000-4000-8000-${String(index).padStart(12, '0')}`;
  const noteJson = (extra = {}) => ({
    format: { version: 1, minor: 2, writer: 'Ardoise 0.1.0' },
    background: { paper: 'white', template: 'blank', texture: false },
    ...(image
      ? { backgroundImage: { name: IMAGE_NAME, opacity: 1, quarterTurns: 0, rect: { x: 0, y: 0, width: page.width, height: page.height } } }
      : {}),
    created: CREATED,
    id,
    layers,
    modified: CREATED,
    origin: { kind: 'ardoise' },
    page,
    tags: [],
    timing: 'recorded',
    title,
    'x-journey': mark,
    ...extra,
  });
  const original = writeZip(
    [
      ['note.json', Buffer.from(j(noteJson(), null, 2))],
      ['strokes.bin', strokesBin(firstInkOf(page))],
    ],
    [],
  );
  const members = new Map([
    ['note.json', Buffer.from(j(noteJson(), null, 2))],
    ['strokes.bin', strokesBin(ink ?? inkOf(page))],
    ['original.ardoise', original],
    ...(image ? [['background.png', quadrantsPng()]] : []),
    ['audio.opus', Buffer.from(`OggS${'narration '.repeat(40)}`, 'latin1')],
    ['text.json', Buffer.from(j({ pages: [{ text: 'Théorème de Thalès' }] }))],
    ['x-journey.bin', Buffer.from('a member of a newer version of the format', 'utf8')],
  ]);
  return { bytes: writeZip(members, ['original.ardoise', 'background.png', 'audio.opus']), members, id };
}

/** Writes `bytes` to `path` the way a sync does: a temporary name in the same folder, then a rename. */
export function dropFile(path, bytes) {
  mkdirSync(dirname(path), { recursive: true });
  const temporary = `${path}.part`;
  writeFileSync(temporary, bytes);
  // A sync client waits and tries again when the file is open for a moment (the app reads its notes).
  for (let attempt = 0; ; attempt++) {
    try {
      renameSync(temporary, path);
      return;
    } catch (error) {
      if (attempt >= 50 || !['EPERM', 'EBUSY', 'EACCES'].includes(error.code)) throw error;
      Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, 100);
    }
  }
}

/** Every `.ardoise` file of the library (the trash and the app's folder aside): `{ path, members, note }`. */
export function notesOf(library) {
  const found = [];
  const walk = (dir) => {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (entry.name.startsWith('_ardoise')) continue;
      const path = join(dir, entry.name);
      if (entry.isDirectory()) walk(path);
      else if (entry.name.endsWith('.ardoise')) {
        const members = readZip(readFileSync(path));
        found.push({ path, members, note: JSON.parse(members.get('note.json').toString('utf8')) });
      }
    }
  };
  walk(library);
  return found;
}

/** JSON with its keys in alphabetical order, to compare values whatever order the app wrote them in. */
export const canon = (value) => j(value, (_, v) => (v && typeof v === 'object' && !Array.isArray(v) ? Object.fromEntries(Object.entries(v).sort(([a], [b]) => (a < b ? -1 : 1))) : v));

export const titled = (library, title) => notesOf(library).filter((n) => n.note.title === title);
export const strokeCountOf = (members) => strokesOf(members.get('strokes.bin')).count;

// --- Windows other than the app's own ---------------------------------------------------------------------

/** A DevTools connection to another window of the app (the presentation window): `evaluate`, `waitFor`, `close`. */
export async function attach(port, predicate, what, timeout = 20_000) {
  const deadline = Date.now() + timeout;
  let target;
  while (!target) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
      target = list.find((t) => t.type === 'page' && predicate(t));
    } catch {
      // Not listening yet.
    }
    if (!target) {
      if (Date.now() > deadline) throw new Error(`timed out waiting for ${what}`);
      await sleep(200);
    }
  }
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
  const send = (method, params = {}) =>
    new Promise((resolve) => {
      const n = ++id;
      pending.set(n, resolve);
      ws.send(JSON.stringify({ id: n, method, params }));
    });
  const window = {
    async evaluate(expression) {
      const reply = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
      if (reply.result?.exceptionDetails) throw new Error(`page error: ${reply.result.exceptionDetails.exception?.description ?? 'unknown'}`);
      return reply.result?.result?.value;
    },
    async waitFor(expression, why, limit = 15_000) {
      const end = Date.now() + limit;
      for (;;) {
        const value = await window.evaluate(expression).catch(() => undefined);
        if (value) return value;
        if (Date.now() > end) throw new Error(`${what}: timed out waiting for ${why}`);
        await sleep(100);
      }
    },
    close: () => ws.close(),
  };
  return window;
}

/**
 * Page expression: where the four colours of the background image are on `selector`'s canvas (2D): the number of
 * pixels near each colour (a sample) and their centre, as fractions of the canvas. Ink is black, paper white.
 */
const colorsOn = (selector) => `(() => {
  const c = document.querySelector(${j(selector)});
  if (!c || c.width === 0) return null;
  const d = c.getContext('2d').getImageData(0, 0, c.width, c.height).data;
  const colors = ${j(QUADRANT_COLORS)};
  const sums = colors.map(() => ({ n: 0, x: 0, y: 0 }));
  for (let y = 0; y < c.height; y += 2) {
    for (let x = 0; x < c.width; x += 2) {
      const i = (y * c.width + x) * 4;
      colors.forEach((k, n) => {
        if (Math.abs(d[i] - k[0]) + Math.abs(d[i + 1] - k[1]) + Math.abs(d[i + 2] - k[2]) < 45) {
          sums[n].n++;
          sums[n].x += x;
          sums[n].y += y;
        }
      });
    }
  }
  const total = (c.width / 2) * (c.height / 2);
  return sums.map((s) => ({ share: s.n / total, x: s.n ? s.x / s.n / c.width : -1, y: s.n ? s.y / s.n / c.height : -1 }));
})()`;

/** Throws unless the four colours are drawn, each in its own quarter of the canvas (red top left, green top right...). */
function assertQuadrants(found, where) {
  if (!found) throw new Error(`${where}: no canvas to read`);
  const names = ['red (top left)', 'green (top right)', 'blue (bottom left)', 'yellow (bottom right)'];
  found.forEach((color, n) => {
    if (!(color.share > 0.02)) throw new Error(`${where}: the ${names[n]} part of the background image is not drawn (${j(found)})`);
    const left = n % 2 === 0;
    const top = n < 2;
    if ((color.x < 0.5) !== left || (color.y < 0.5) !== top) throw new Error(`${where}: the ${names[n]} part is in the wrong place (${j(found)})`);
  });
}
const quadrantsDrawn = (selector, where) => async (target) => {
  assertQuadrants(await target.evaluate(colorsOn(selector)), where);
  return true;
};

// --- Steps shared by the journeys -------------------------------------------------------------------------

export const STATUS = '.tablet-status';
const WIZARD = '.register-wizard';

/** Registers the simulated tablet (it starts unregistered) and waits until it is connected and has synced. */
export async function registerTablet(app, check) {
  await check('enregistrer la tablette simulée', async () => {
    await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="notRegistered"]`)}) !== null`, 'the sidebar block');
    await app.click(STATUS);
    await app.waitFor(`document.querySelector('.tablet-panel button[data-action="register"]') !== null`, 'the register button');
    await app.click('.tablet-panel button[data-action="register"]');
    await app.waitFor(`document.querySelector(${j(`${WIZARD} [data-step="warning"]`)}) !== null`, 'the warning step');
    await app.click(`${WIZARD} button[data-action="continue"]`);
    await app.waitFor(`document.querySelector(${j(`${WIZARD} [data-step="done"]`)}) !== null`, 'the end of the registration', 60_000);
    await app.click(`${WIZARD} button[data-action="finish"]`);
    await app.waitFor(`document.querySelector(${j(`${STATUS}[data-connection="connected"]`)}) !== null`, 'the end of the sync', 60_000);
    // The tablet panel stays open beside the grid, and would cover the cards of its last column.
    await app.click('.tablet-panel button', text(app.language, 'common.close'));
    await app.waitFor(`document.querySelector('.tablet-panel') === null`, 'the tablet panel to close');
  });
}

/** Puts a sheet into the library of the run in `binder` ('' = the root) and waits until the grid shows its card. */
export async function dropSheet(app, { binder, file, ...options }) {
  const made = sheet(options);
  const path = join(app.library, binder, file);
  dropFile(path, made.bytes);
  await app.waitFor(
    `[...document.querySelectorAll('.note-card')].some((c) => c.dataset.path.includes(${j(file)}))`,
    `the card of ${file} (is the library watched?)`,
    30_000,
  );
  return { ...made, path };
}

export const card = (file) => `.note-card[data-path*=${j(file)}]`;

/** « Reprendre en mode direct » in the menu of a card. */
export async function resumeFromCard(app, file) {
  await app.click(`${card(file)} .card-menu button[aria-haspopup="menu"]`);
  await app.waitFor(`document.querySelector('[role="menu"] [role="menuitem"]') !== null`, 'the card menu');
  await app.click('[role="menu"] [role="menuitem"]', text(app.language, 'live.resume.action'));
  await app.waitFor(`document.querySelector('.live-view') !== null`, 'the live view');
}

/** Waits until the library grid is back (live mode closed). */
export const libraryBack = (app) => app.waitFor(`document.querySelector('.shell:not(.shell-hidden) input.search') !== null`, 'the library', 30_000);

/** Stops live mode with the button and waits for the library. */
export async function stopLive(app) {
  await app.click('button[data-action="stop-live"]');
  await libraryBack(app);
}

/** The facts of the editor that a resumed sheet must keep. */
const editorFacts = (app) =>
  app.evaluate(`({
    image: document.querySelector('[data-section="background-image"] [data-slot="background-image-name"]')?.textContent ?? null,
    strokes: document.querySelectorAll(${j(EDITOR_STROKES)}).length,
    original: document.querySelector('[data-section="original"] button[data-action="original"]')?.disabled === false,
  })`);

/** Opens the sheet in the editor; waits for its strokes, its image section and the answer about its original. */
export async function openInEditor(app, file, strokes) {
  await app.click(`${card(file)} .card-open`);
  await app.waitFor(`document.querySelectorAll(${j(EDITOR_STROKES)}).length >= ${strokes}`, 'the strokes of the note');
  await app.waitFor(
    `document.querySelector('[data-section="original"] [data-slot="original-help"]')?.textContent !== ${j(text(app.language, 'editor.original.loading'))}`,
    'the answer about the original',
  );
  await app.waitFor(`document.querySelector('[data-section="background-image"] [data-slot="background-image-name"]') !== null`, 'the image section');
}

/** Back from the editor to the library; discards an unsaved change. */
export async function closeEditor(app, { discard = false } = {}) {
  await app.click('.editor-top button');
  if (discard) {
    await app.waitFor(`document.querySelector('[role="alertdialog"]') !== null`, 'the leave dialog');
    await app.click('[role="alertdialog"] button.danger');
  }
  await libraryBack(app);
}

// --- Journeys ---------------------------------------------------------------------------------------------

export const resumeJourneys = [
  {
    name: 'reprise d’une feuille en direct : depuis la carte, image de fond, écriture, fichier conservé, présentation et partage',
    args: ['--simulated-tablet'],
    // The share listens on 127.0.0.1 only, on a port of ours (as the share's own journey does).
    settings: (ctx) => `[share]\nport = ${ctx.sharePort}\n`,
    env: { ARDOISE_SHARE_LOOPBACK: '1' },
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const title = 'Fiche reprise E2E';
      const file = '2026-10-28 Fiche reprise E2E.ardoise';
      let made;
      let browser = null;
      let presentation = null;

      try {
        await registerTablet(app, check);

        await check('une fiche (image de fond, trois traits, première version, texte, narration) est posée dans le classeur « Reprise »', async () => {
          made = await dropSheet(app, { binder: 'Reprise', file, title, index: 1 });
        });

        await check('l’éditeur montre l’image de fond, les trois traits et un original disponible', async () => {
          await openInEditor(app, file, 3);
          const facts = await editorFacts(app);
          if (facts.image !== IMAGE_NAME) throw new Error(`background image: ${j(facts.image)}`);
          if (facts.strokes !== 3) throw new Error(`${facts.strokes} strokes, expected 3`);
          if (!facts.original) throw new Error(`« ${l('editor.original.button')} » is not available`);
          await closeEditor(app);
        });

        await check(`menu de la carte, « ${l('live.resume.action')} » : le mode direct s’ouvre sur la fiche, avec ses trois traits`, async () => {
          await resumeFromCard(app, file);
          await app.waitFor(`Number(${canvasData('strokes')}) >= 3`, 'the three strokes of the sheet on the live canvas', 20_000);
          await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'the tablet to stream', 30_000);
        });

        await check('fenêtre de commande : l’image de fond est dessinée à sa place sur le canevas', async () => {
          await app.waitFor(`(() => { const f = ${colorsOn(LIVE_CANVAS)}; return f && f.every((c) => c.share > 0.02); })()`, 'the background image on the live canvas', 20_000);
          assertQuadrants(await app.evaluate(colorsOn(LIVE_CANVAS)), 'control window');
        });

        await check('fenêtre de présentation : le même fond et les mêmes traits', async () => {
          await app.evaluate(`window.__TAURI_INTERNALS__.invoke('open_presentation', { monitor: null }).then(() => true)`);
          presentation = await attach(app.port, (t) => t.url.includes('view=presentation'), 'the presentation window');
          await presentation.waitFor(`Number(${canvasData('strokes')}) >= 3`, 'the strokes in the presentation');
          await presentation.waitFor(`(() => { const f = ${colorsOn(LIVE_CANVAS)}; return f && f.every((c) => c.share > 0.02); })()`, 'the background image in the presentation', 20_000);
          assertQuadrants(await presentation.evaluate(colorsOn(LIVE_CANVAS)), 'presentation window');
          presentation.close();
          presentation = null;
          await app.evaluate(`window.__TAURI_INTERNALS__.invoke('close_presentation').then(() => true)`);
        });

        await check('partage : une page de vrai navigateur (Edge sans fenêtre) montre la fiche, son fond et ses traits', async () => {
          const edge = findEdge();
          if (!edge) {
            console.log('    skipped: Edge not found (C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe)');
            return;
          }
          await app.click('button[data-action="share"]');
          await app.waitFor(`document.querySelector('[data-share-code]') !== null`, 'the access code', 15_000);
          const code = await app.evaluate(`document.querySelector('[data-share-code]').textContent.replace(/[\\s\\u00a0\\u202f]+/g, '')`);
          browser = await startBrowser(edge);
          await browser.navigate(`http://127.0.0.1:${app.sharePort}/`);
          await browser.waitFor(`document.querySelector('main.viewer')?.dataset.screen === 'code'`, 'the code form', 30_000);
          await browser.click('#viewer-code');
          await browser.type(code);
          await browser.waitFor(`document.querySelector('form button[type="submit"]')?.disabled === false`, 'the join button');
          await browser.click('form button[type="submit"]');
          await browser.waitFor(`document.querySelector('main.viewer')?.dataset.screen === 'live'`, 'the live sheet', 20_000);
          await browser.waitFor(`Number(document.querySelector('canvas.viewer-canvas')?.dataset.strokes) >= 3`, 'the strokes on the page', 20_000);
          await browser.waitFor(`(() => { const f = ${colorsOn('canvas.viewer-canvas')}; return f && f.every((c) => c.share > 0.02); })()`, 'the paper on the page', 20_000);
          assertQuadrants(await browser.evaluate(colorsOn('canvas.viewer-canvas')), 'shared page');
          assertNoBrowserProblem(browser);
          await browser.close();
          browser = null;
          await app.click('button[data-action="share-stop"]');
          await app.waitFor(`document.querySelector('.share-panel') === null`, 'the share panel to close', 10_000);
        });

        await check('le stylet simulé écrit sur la feuille : des traits s’ajoutent aux trois premiers', async () => {
          await app.waitFor(`Number(${canvasData('strokes')}) >= 5`, 'two strokes of the pen', 40_000);
        });

        let after;
        await check('arrêter : la même note (même chemin, pas de doublon), tous ses membres, ses traits d’abord intacts', async () => {
          await stopLive(app);
          const same = titled(app.library, title);
          if (same.length !== 1) throw new Error(`${same.length} notes titled ${title}: ${same.map((n) => n.path).join(', ')}`);
          if (same[0].path !== made.path) throw new Error(`the note moved: ${same[0].path}`);
          if (same[0].note.id !== made.id) throw new Error(`another id: ${same[0].note.id}`);
          after = same[0];
          const cards = await app.evaluate(`[...document.querySelectorAll('.note-card')].filter((c) => c.dataset.path.includes(${j(file)})).length`);
          if (cards !== 1) throw new Error(`${cards} cards for the note`);
          for (const [name, bytes] of made.members) {
            if (name === 'note.json' || name === 'strokes.bin') continue;
            if (!after.members.has(name)) throw new Error(`the member ${name} is gone`);
            if (!after.members.get(name).equals(bytes)) throw new Error(`the member ${name} changed`);
          }
          const note = after.note;
          const first = JSON.parse(made.members.get('note.json').toString('utf8'));
          for (const key of ['id', 'title', 'created', 'page', 'backgroundImage', 'background', 'origin', 'layers', 'x-journey']) {
            if (canon(note[key]) !== canon(first[key])) throw new Error(`note.json: ${key} is ${j(note[key])}, was ${j(first[key])}`);
          }
          const before = strokesOf(made.members.get('strokes.bin'));
          const now = strokesOf(after.members.get('strokes.bin'));
          if (before.count !== 3 || !(now.count >= 5)) throw new Error(`${before.count} strokes then ${now.count}`);
          if (!now.body.subarray(0, before.body.length).equals(before.body)) throw new Error('the first three strokes were rewritten');
        });

        await check('rouvrir la note : image de fond et original encore là ; « Revenir à l’original » rend la première version (un trait)', async () => {
          await openInEditor(app, file, 5);
          const facts = await editorFacts(app);
          if (facts.image !== IMAGE_NAME) throw new Error(`background image: ${j(facts.image)}`);
          if (!facts.original) throw new Error(`« ${l('editor.original.button')} » is not available any more`);
          await app.click('[data-section="original"] button[data-action="original"]');
          await app.click('[data-action="original-confirm"]');
          await app.waitFor(`document.querySelectorAll(${j(EDITOR_STROKES)}).length === 1`, 'the first version (one stroke)');
          await closeEditor(app, { discard: true });
          const kept = readFileSync(made.path);
          if (!readZip(kept).get('original.ardoise').equals(made.members.get('original.ardoise'))) throw new Error('the first version changed');
        });

        await check('reprendre sans écrire (reprise puis arrêt aussitôt) : le fichier ne change pas d’un octet', async () => {
          // The simulated pen starts to write as soon as live mode streams, so the stop must come before its first point:
          // an attempt that lost that race wrote the note (as it should) and is not the one this step is about.
          for (let attempt = 1; attempt <= 15; attempt++) {
            const bytes = readFileSync(made.path);
            const stamp = statSync(made.path).mtimeMs;
            const stopped = await app.evaluate(`(async () => {
              await window.__TAURI_INTERNALS__.invoke('live_resume', { path: ${j(made.path)} });
              return await window.__TAURI_INTERNALS__.invoke('live_stop');
            })()`);
            if (stopped.saved.length !== 0) {
              console.log(`    (essai ${attempt} : le stylet simulé a écrit avant l’arrêt, nouvel essai)`);
              continue;
            }
            if (!readFileSync(made.path).equals(bytes)) throw new Error('the file changed');
            if (statSync(made.path).mtimeMs !== stamp) throw new Error('the file was rewritten (new stamp)');
            return;
          }
          throw new Error('the simulated pen wrote before the stop in each of 15 attempts');
        });
      } finally {
        presentation?.close();
        await browser?.close();
      }
    },
  },

  {
    name: 'reprise d’une feuille en direct : depuis l’éditeur avec une modification, positions désactivées, page suivante',
    args: ['--simulated-tablet'],
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const title = 'Fiche éditeur E2E';
      const file = '2026-10-28 Fiche éditeur E2E.ardoise';
      let made;
      let rootBefore;
      const ORIENTATION_BUTTONS = '[data-slot="orientation"] button';

      await registerTablet(app, check);

      await check('une fiche de trois traits est posée dans le classeur « Reprise »', async () => {
        made = await dropSheet(app, { binder: 'Reprise', file, title, index: 2 });
        rootBefore = notesOf(app.library).map((n) => n.path);
      });

      await check('éditeur : un trait tracé à la souris (modification non enregistrée)', async () => {
        await openInEditor(app, file, 3);
        const box = await app.evaluate(`(() => { const r = document.querySelector('.editor svg').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`);
        const points = Array.from({ length: 25 }, (_, i) => ({ x: box.x + box.w * (0.3 + 0.4 * (i / 24)), y: box.y + box.h * (0.6 + 0.05 * Math.sin(i / 3)) }));
        await app.drag(points);
        await app.waitFor(`document.querySelectorAll(${j(EDITOR_STROKES)}).length === 4`, 'the new stroke');
        await app.waitFor(`document.querySelector('.editor').dataset.dirty === 'yes'`, 'the unsaved mark');
      });

      await check(`« ${l('live.resume.action')} » de la barre d’outils : la boîte propose d’enregistrer, « ${l('editor.leave.saveResume')} » enregistre puis ouvre la feuille`, async () => {
        await app.click('button[data-action="resume-live"]');
        await app.waitFor(`document.querySelector('[role="alertdialog"]') !== null`, 'the leave dialog');
        const labels = await app.evaluate(`[...document.querySelectorAll('[role="alertdialog"] .actions button')].map((b) => b.textContent.trim())`);
        if (!labels.includes(l('editor.leave.saveResume')) || !labels.includes(l('editor.leave.discardResume'))) throw new Error(`buttons: ${j(labels)}`);
        await app.click('[role="alertdialog"] .actions button', l('editor.leave.saveResume'));
        await app.waitFor(`document.querySelector('.live-view') !== null`, 'the live view');
        // The sheet was saved first: its file already holds the stroke drawn in the editor.
        const saved = strokeCountOf(readZip(readFileSync(made.path)));
        if (saved < 4) throw new Error(`the file holds ${saved} strokes, expected 4 or more`);
        await app.waitFor(`Number(${canvasData('strokes')}) >= 4`, 'the four strokes on the live canvas', 20_000);
        await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'the tablet to stream', 30_000);
      });

      await check('les quatre boutons de position sont désactivés sur la feuille reprise', async () => {
        const states = await app.evaluate(`[...document.querySelectorAll(${j(ORIENTATION_BUTTONS)})].map((b) => b.disabled)`);
        if (states.length !== 4 || !states.every(Boolean)) throw new Error(`position buttons disabled: ${j(states)}`);
      });

      await check('page suivante : une nouvelle page, les boutons de position fonctionnent de nouveau', async () => {
        await app.click(`button[aria-label=${j(l('live.pages.next'))}]`);
        await app.waitFor(`${canvasData('page')} === '2/2'`, 'the second page');
        await app.waitFor(`[...document.querySelectorAll(${j(ORIENTATION_BUTTONS)})].every((b) => !b.disabled)`, 'the position buttons to work again');
        // The pen writes on the new page, which becomes a note of its own.
        await app.waitFor(`Number(${canvasData('strokes')}) >= 1`, 'a stroke on the new page', 30_000);
      });

      await check('arrêter : la fiche reprise reste une seule note, la page suivante est une note neuve à la racine', async () => {
        await stopLive(app);
        const notes = notesOf(app.library);
        const same = notes.filter((n) => n.path === made.path);
        if (same.length !== 1 || same[0].note.id !== made.id) throw new Error('the resumed sheet is not the same note any more');
        if (titled(app.library, title).length !== 1) throw new Error('the resumed sheet was duplicated');
        const added = notes.filter((n) => !rootBefore.includes(n.path));
        if (added.length !== 1) throw new Error(`${added.length} new notes: ${added.map((n) => n.path).join(', ')}`);
        if (dirname(added[0].path) !== app.library) throw new Error(`the new note is in ${dirname(added[0].path)}, not in the library root`);
        if (added[0].members.has('background.png')) throw new Error('the new note took the image of the sheet');
        if (added[0].note.id === made.id) throw new Error('the new page has the id of the sheet');
        if (!(strokeCountOf(same[0].members) >= 4)) throw new Error('the sheet lost its strokes');
      });
    },
  },

  {
    name: 'reprise d’une feuille en direct : paysage avec la tablette tenue autrement, feuille modifiée ailleurs pendant la reprise',
    args: ['--simulated-tablet'],
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const title = 'Fiche paysage E2E';
      const file = '2026-10-28 Fiche paysage E2E.ardoise';
      const changedTitle = 'Fiche modifiée E2E';
      const changedFile = '2026-10-28 Fiche modifiée E2E.ardoise';
      const ORIENTATION_BUTTONS = '[data-slot="orientation"] button';
      let landscape;

      await registerTablet(app, check);

      await check('la tablette est tenue en portrait, bouton à gauche (réglage de la tablette)', async () => {
        await app.click(STATUS);
        await app.waitFor(`document.querySelectorAll('.tablet-panel input[data-option="orientation"]').length === 4`, 'the positions');
        await app.click('.tablet-panel input[data-option="orientation"][value="portraitLeft"]');
        await app.waitFor(`document.querySelector('.tablet-panel input[data-option="orientation"][value="portraitLeft"]').checked`, 'the setting');
        await app.click('.tablet-panel button', l('common.close'));
        await app.waitFor(`document.querySelector('.tablet-panel') === null`, 'the tablet panel to close');
      });

      await check('une fiche en paysage est posée dans le classeur « Reprise »', async () => {
        landscape = await dropSheet(app, { binder: 'Reprise', file, title, index: 3 });
      });

      await check('reprendre la fiche : l’avis dit comment tenir la tablette, les positions sont désactivées', async () => {
        await resumeFromCard(app, file);
        await app.waitFor(`document.querySelector('[data-slot="resume-hold"]') !== null`, 'the notice about how to hold the tablet', 30_000);
        const shown = await app.evaluate(`document.querySelector('[data-slot="resume-hold"]').textContent.trim()`);
        const expected = l('live.resume.hold').replace('{position}', l('tablet.orientation.landscape'));
        if (shown !== expected) throw new Error(`the notice says ${j(shown)}, expected ${j(expected)}`);
        const states = await app.evaluate(`[...document.querySelectorAll(${j(ORIENTATION_BUTTONS)})].map((b) => b.disabled)`);
        if (states.length !== 4 || !states.every(Boolean)) throw new Error(`position buttons disabled: ${j(states)}`);
        const pressed = await app.evaluate(`document.querySelector('[data-slot="orientation"] button[aria-pressed="true"]')?.dataset.orientation`);
        if (pressed !== 'landscape') throw new Error(`the position shown is ${pressed}, the sheet is held in landscape`);
        await app.waitFor(`Number(${canvasData('strokes')}) >= 4`, 'the pen to write on the sheet', 40_000);
      });

      await check('arrêter : la fiche n’est pas tournée (même format, ses traits d’abord intacts)', async () => {
        await stopLive(app);
        const [after] = titled(app.library, title);
        const first = JSON.parse(landscape.members.get('note.json').toString('utf8'));
        if (canon(after.note.page) !== canon(first.page)) throw new Error(`the page is ${j(after.note.page)}, was ${j(first.page)}`);
        const before = strokesOf(landscape.members.get('strokes.bin'));
        const now = strokesOf(after.members.get('strokes.bin'));
        if (!(now.count > before.count)) throw new Error(`${now.count} strokes, the pen wrote nothing`);
        if (!now.body.subarray(0, before.body.length).equals(before.body)) throw new Error('the strokes of the sheet were turned or rewritten');
        // The new strokes stay on the page.
        if (now.body.length <= before.body.length) throw new Error('no new bytes');
      });

      let theirs;
      let changed;
      await check('une autre fiche est reprise ; pendant ce temps le fichier est remplacé par une copie venue d’ailleurs (autre estampille, autre encre)', async () => {
        changed = await dropSheet(app, { binder: 'Reprise', file: changedFile, title: changedTitle, index: 4 });
        await resumeFromCard(app, changedFile);
        await app.waitFor(`Number(${canvasData('strokes')}) >= 3`, 'the sheet on the live canvas', 20_000);
        theirs = sheet({ title: changedTitle, index: 4, ink: elsewhereInkOf(PAGE.landscape), mark: 'written elsewhere' });
        await sleep(1100); // a stamp the file system tells apart
        dropFile(changed.path, theirs.bytes);
      });

      await check('l’avis apparaît : la feuille a été modifiée ailleurs, l’écriture continue dans une nouvelle note', async () => {
        await app.waitFor(`document.querySelector('[data-slot="sheet-forked"][data-reason="changed"]') !== null`, 'the notice about the sheet changed elsewhere', 60_000);
        const shown = await app.evaluate(`document.querySelector('[data-slot="sheet-forked"]').textContent`);
        if (!shown.includes(l('live.resume.forked.changed'))) throw new Error(`the notice says ${j(shown)}`);
        if (!shown.includes(l('live.resume.forked.sheet').replace('{title}', changedTitle))) throw new Error(`the notice does not name the sheet: ${j(shown)}`);
      });

      await check('arrêter : le fichier écrit ailleurs est intact, une nouvelle note est à côté dans le même classeur', async () => {
        await stopLive(app);
        if (!readFileSync(changed.path).equals(theirs.bytes)) throw new Error('the file written elsewhere was overwritten');
        const beside = readdirSync(dirname(changed.path)).filter((name) => name.endsWith('.ardoise') && name.includes(changedTitle));
        if (beside.length !== 2) throw new Error(`${beside.length} notes of that title in the binder: ${beside.join(', ')}`);
        const other = notesOf(app.library).find((n) => n.note.title === changedTitle && n.path !== changed.path);
        if (!other) throw new Error('no new note');
        if (dirname(other.path) !== dirname(changed.path)) throw new Error(`the new note is in ${dirname(other.path)}`);
        if (other.note.id === changed.id) throw new Error('the new note has the id of the sheet');
        // It carries everything the sheet had (image, first version, text, narration, unknown member) and the writing.
        for (const name of ['background.png', 'original.ardoise', 'audio.opus', 'text.json', 'x-journey.bin']) {
          if (!other.members.get(name)?.equals(changed.members.get(name))) throw new Error(`the new note lacks ${name}`);
        }
        if (!(strokeCountOf(other.members) >= 4)) throw new Error(`${strokeCountOf(other.members)} strokes in the new note`);
        // The last write lands as live mode stops: the library shows it after the watcher's quiet period and a new listing.
        await app.waitFor(
          `[...document.querySelectorAll('.note-card')].filter((c) => c.dataset.path.includes(${j(changedTitle)})).length === 2`,
          'a card for each of the two notes',
          15_000,
        );
      });
    },
  },
];
