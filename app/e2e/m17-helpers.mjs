// Helpers of the three M17 journeys (docs/design/paper-projection-narration.md §7): live paper and background
// (`journeys-m17-live.mjs`), the editor's projection (`journeys-m17-projection.mjs`) and the sound while writing
// (`journeys-m17-sound.mjs`). Shared here so that the three packages that write them do not touch each other's files.
import { readFileSync } from 'node:fs';
import { BROWSER_ARGS } from './journeys-m14-live.mjs';
import { penStroke } from './journeys-m8-input.mjs';
import { attach, readZip } from './journeys-m12-resume.mjs';
import { decodeStrokes } from './journeys-m12-touchup.mjs';

export { BROWSER_ARGS, penStroke };

/**
 * A DevTools connection to the editor's projection window (`index.html?view=projection`), opened by the editor's
 * « Projeter »: `evaluate`, `waitFor`, `close` (`attach` of `journeys-m12-resume.mjs`). Live's own presentation window is
 * attached the same way with `view=presentation`.
 */
export const projectionWindow = (app) => attach(app.port, (target) => target.url.includes('view=projection'), 'the projection window');

/**
 * What the canvas of a window (`canvas.live-canvas`: live's presentation window, the editor's projection window) says of
 * itself, from its test attributes (docs §2.5), or `null` when the window has no canvas yet: `drawn` (strokes drawn,
 * visible layers only, the one in progress included), `paper` (`white`, `cream`, `kraft`, `slate`, `custom`: the
 * effective paper), `template`, `image` (the image's name, empty without one), `paperReady` (the picture of the current
 * paper is drawn), `zoom` (the view's zoom). `target`: an `app` or a window of `attach`.
 */
export async function canvasFacts(target) {
  const facts = await target.evaluate(`(() => {
    const el = document.querySelector('canvas.live-canvas');
    if (!el) return null;
    const d = el.dataset;
    return { drawn: d.drawn ?? null, paper: d.paper ?? null, template: d.template ?? null, image: d.image ?? null, paperReady: d.paperReady ?? null, zoom: d.zoom ?? null };
  })()`);
  if (!facts) return null;
  return {
    drawn: facts.drawn === null ? null : Number(facts.drawn),
    paper: facts.paper,
    template: facts.template,
    image: facts.image,
    paperReady: facts.paperReady === 'true',
    zoom: facts.zoom === null ? null : Number(facts.zoom),
  };
}

/**
 * A note file as written on disk: `{ path, members, names, note, strokes, audio }`: the members of the archive (`Map`
 * name → bytes, `readZip`), their names in order, `note.json` parsed, the strokes of `strokes.bin` (`decodeStrokes`:
 * `{ tool, color, width, layer, points: [{ x, y, p, t }] }`, `[]` without that member) and `audio.opus` (bytes, or `null`).
 */
export function noteOnDisk(path) {
  const members = readZip(readFileSync(path));
  const note = JSON.parse(members.get('note.json').toString('utf8'));
  const strokes = members.has('strokes.bin') ? decodeStrokes(members.get('strokes.bin')) : [];
  return { path, members, names: [...members.keys()], note, strokes, audio: members.get('audio.opus') ?? null };
}

/**
 * The `ARDOISE_*` comments of a narration's `audio.opus` (Ogg Opus, docs/file-format.md §4.1), by name without the
 * prefix: `{ BASE: 'note', START_MS: '1234', SPEED: '1', MAX_PAUSE_MS: 'none' }` (values as text; `{}` for a stream without
 * the tags). The comment header is found by its `OpusTags` magic and read as the Opus specification lays it out (a vendor
 * string, then length-prefixed `NAME=value` comments).
 */
export function opusTags(bytes) {
  const buffer = Buffer.from(bytes);
  const at = buffer.indexOf('OpusTags', 0, 'latin1');
  if (at < 0) return {};
  let cursor = at + 8;
  const text = () => {
    const length = buffer.readUInt32LE(cursor);
    const value = buffer.subarray(cursor + 4, cursor + 4 + length).toString('utf8');
    cursor += 4 + length;
    return value;
  };
  text(); // the vendor
  const count = buffer.readUInt32LE(cursor);
  cursor += 4;
  const tags = {};
  for (let n = 0; n < count && cursor + 4 <= buffer.length; n++) {
    const comment = text();
    const eq = comment.indexOf('=');
    if (eq > 0 && comment.startsWith('ARDOISE_')) tags[comment.slice('ARDOISE_'.length, eq)] = comment.slice(eq + 1);
  }
  return tags;
}
