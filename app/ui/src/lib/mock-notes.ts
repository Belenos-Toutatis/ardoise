/**
 * Browser stand-in for the editor commands (`open_note`, `save_note`, `note_original`,
 * `render_background`), over the demo library of `mock-data.ts`.
 */
import type { Background, BackgroundImage, FileStamp, Note, OpenedNote, PageSize, Paper, Rgba, RevisionSource } from './note';
import type { NoteOriginal } from './original';
import {
  UNHANDLED,
  fileName,
  freshStamp,
  mockLibrary,
  notePath,
  notifyLibraryChanged,
  sameStamp,
  type StoredNote,
} from './mock-data';

/** Notes are plain JSON: a JSON round trip is a deep copy. */
function copy<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

function stored(path: string) {
  const entry = mockLibrary().notes.get(path);
  if (!entry) throw { code: 'noteNotFound', message: path };
  return entry;
}

const isU32 = (value: unknown) => Number.isInteger(value) && (value as number) >= 0 && (value as number) <= 0xffff_ffff;

/**
 * What the desktop app's deserialisation refuses before the command runs: stroke layers and
 * sample times are `u32` in Rust, and serde rejects fractions and negative numbers.
 */
function checkNote(note: Note): void {
  for (const stroke of note.strokes) {
    const bad = !isU32(stroke.layer) ? stroke.layer : stroke.points.find((p) => !isU32(p.t))?.t;
    if (bad !== undefined) {
      throw `invalid args \`note\` for command \`save_note\`: invalid value: \`${bad}\`, expected u32`;
    }
  }
}

/**
 * Like `original::keep_original`: the first save that changes the ink keeps the version before
 * it (never an empty page). A tablet note stands for its `original.bin`.
 */
function keepOriginal(entry: StoredNote, next: Note): void {
  const before = entry.note;
  if (entry.original || before.strokes.length === 0) return;
  const ink = (n: Note) => JSON.stringify([n.page, n.layers, n.strokes]);
  if (ink(before) === ink(next)) return;
  const tablet = before.origin.kind === 'tablet';
  entry.original = copy({
    source: tablet ? 'tablet' : 'firstVersion',
    page: before.page,
    layers: before.layers,
    strokes: before.strokes,
    // The first version's `note.json` tells its paper and describes its background image (a tablet
    // page has neither).
    ...(!tablet ? { paper: before.background.paper } : {}),
    ...(!tablet && before.backgroundImage ? { backgroundImage: before.backgroundImage } : {}),
  });
}

/** The note of the demo library with this id, as `revision::find_source` finds it. */
function sourceOf(id: string): RevisionSource | null {
  for (const [path, entry] of mockLibrary().notes) {
    if (entry.note.id === id) return { path, title: entry.note.title };
  }
  return null;
}

/**
 * A stand-in for `note_picture`: the page as an SVG (paper and strokes as plain lines), at the size
 * of the box. The desktop app draws a PNG with the export renderer.
 */
function pictureDataUrl(note: Note, maxWidth: number, maxHeight: number): string {
  const scale = Math.min(maxWidth / note.page.width, maxHeight / note.page.height);
  const [w, h] = [Math.round(note.page.width * scale), Math.round(note.page.height * scale)];
  const background = decodeURIComponent(backgroundDataUrl(note.page, note.background, w).split(',')[1]);
  const hidden = (layer: number) => note.layers[layer]?.visible === false;
  const lines = note.strokes
    .filter((stroke) => !hidden(stroke.layer))
    .map((stroke) => {
      const points = stroke.points.map((p) => `${p.x / 100},${p.y / 100}`).join(' ');
      const { r, g, b, a } = stroke.color;
      return `<polyline points="${points}" fill="none" stroke="rgb(${r},${g},${b})" stroke-opacity="${a / 255}" stroke-width="${stroke.width / 100}" stroke-linecap="round"/>`;
    })
    .join('');
  const inner = background.replace(/^<svg[^>]*>/, '').replace(/<\/svg>$/, '');
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${w}" height="${h}" viewBox="0 0 ${note.page.width / 100} ${note.page.height / 100}">` +
    `${inner}${lines}</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

export function mockNotesInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'open_note': {
      const entry = stored(args.path as string);
      const revision = entry.revision ? { ...entry.revision, source: sourceOf(entry.revision.of) } : null;
      return copy({ note: entry.note, stamp: entry.stamp, revision }) satisfies OpenedNote;
    }
    case 'revision_source':
      return sourceOf(args.id as string);
    case 'note_picture':
      return pictureDataUrl(stored(args.path as string).note, args.maxWidth as number, args.maxHeight as number);
    case 'save_note': {
      const path = args.path as string;
      const entry = stored(path);
      checkNote(args.note as Note);
      if (!sameStamp(entry.stamp, args.stamp as FileStamp)) throw { code: 'noteConflict', message: path };
      const note = copy(args.note as Note);
      keepOriginal(entry, note);
      // Like `save_in`: the file keeps its own id.
      note.id = entry.note.id;
      note.modified = Date.now();
      entry.note = note;
      entry.stamp = freshStamp(JSON.stringify(note).length);
      notifyLibraryChanged();
      return copy(entry.stamp);
    }
    case 'note_original': {
      const entry = stored(args.path as string);
      return copy(entry.original ?? { source: 'none' }) satisfies NoteOriginal;
    }
    case 'render_background':
      return backgroundDataUrl(
        args.page as PageSize,
        args.background as Background,
        args.widthPx as number,
        (args.image as { image: BackgroundImage } | undefined)?.image,
      );
    case 'retitle_note': {
      // Fallback only: the library's stand-in (`mock-library.ts`) handles it first when it does.
      const path = args.path as string;
      const entry = stored(path);
      if (!sameStamp(entry.stamp, args.stamp as FileStamp)) throw { code: 'noteConflict', message: path };
      const library = mockLibrary();
      entry.note = { ...entry.note, title: (args.title as string).trim() };
      entry.stamp = freshStamp();
      const next = notePath(library.root, entry.binder, fileName(entry.note));
      library.notes.delete(path);
      library.notes.set(next, entry);
      notifyLibraryChanged();
      return { path: next, stamp: copy(entry.stamp) };
    }
    default:
      return UNHANDLED;
  }
}

// --- Backgrounds as SVG ----------------------------------------------------------------------
// Same papers and templates as `ardoise-render/src/background.rs`, in millimetres; good enough
// for the browser (the desktop app renders a PNG with the export renderer).

const PAPERS: Record<Exclude<Paper, { custom: Rgba }>, string> = {
  white: '#ffffff',
  cream: '#faf4e2',
  kraft: '#ceae80',
  slate: '#2a3230',
};

function paperColor(paper: Paper): { css: string; dark: boolean } {
  const c =
    typeof paper === 'string'
      ? PAPERS[paper]
      : `rgb(${paper.custom.r}, ${paper.custom.g}, ${paper.custom.b})`;
  const rgb =
    typeof paper === 'string'
      ? [1, 3, 5].map((i) => parseInt(PAPERS[paper].slice(i, i + 2), 16))
      : [paper.custom.r, paper.custom.g, paper.custom.b];
  const luminance = 0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2];
  return { css: c, dark: luminance < 110 };
}

function steps(start: number, end: number, step: number): number[] {
  const out: number[] = [];
  for (let v = start; v <= end + 1e-9; v += step) out.push(Math.round(v * 1000) / 1000);
  return out;
}

const centred = (size: number, step: number) => (size - Math.floor(size / step) * step) / 2;

function templateSvg(background: Background, w: number, h: number, dark: boolean): string {
  const ink = dark ? 'rgb(235,238,230)' : 'rgb(60,110,190)';
  const hl = (y: number, color: string, width: number, x0 = 0, x1 = w) =>
    `<line x1="${x0}" y1="${y}" x2="${x1}" y2="${y}" stroke="${color}" stroke-width="${width}"/>`;
  const vl = (x: number, color: string, width: number, y0 = 0, y1 = h) =>
    `<line x1="${x}" y1="${y0}" x2="${x}" y2="${y1}" stroke="${color}" stroke-width="${width}"/>`;
  const alpha = (color: string, a: number) => color.replace('rgb(', 'rgba(').replace(')', `,${a})`);
  switch (background.template) {
    case 'blank':
      return '';
    case 'lined':
      return steps(16, h - 4, 8).map((y) => hl(y, alpha(ink, 0.45), 0.2)).join('');
    case 'seyes': {
      const thin = alpha(dark ? ink : 'rgb(95,140,215)', dark ? 0.18 : 0.42);
      const strong = alpha(dark ? ink : 'rgb(80,80,185)', dark ? 0.34 : 0.62);
      const lines = steps(16, h, 2).map((y, i) => (i % 4 === 0 ? hl(y, strong, 0.2) : hl(y, thin, 0.12)));
      const verticals = steps(40, w, 8).map((x) => vl(x, strong, 0.2));
      return lines.join('') + verticals.join('') + vl(32, 'rgba(220,40,70,0.75)', 0.25);
    }
    case 'grid5mm':
      return (
        steps(centred(h, 5), h, 5).map((y) => hl(y, alpha(ink, 0.36), 0.15)).join('') +
        steps(centred(w, 5), w, 5).map((x) => vl(x, alpha(ink, 0.36), 0.15)).join('')
      );
    case 'millimetric': {
      const orange = dark ? 'rgb(240,200,150)' : 'rgb(225,115,40)';
      const style = (i: number): [string, number] =>
        i % 10 === 0 ? [alpha(orange, 0.75), 0.25] : i % 5 === 0 ? [alpha(orange, 0.5), 0.15] : [alpha(orange, 0.32), 0.08];
      const ox = centred(w, 10);
      const oy = centred(h, 10);
      const out: string[] = [];
      for (let i = -Math.floor(oy); i <= Math.floor(h - oy); i++) out.push(hl(oy + i, ...style(Math.abs(i))));
      for (let i = -Math.floor(ox); i <= Math.floor(w - ox); i++) out.push(vl(ox + i, ...style(Math.abs(i))));
      return out.join('');
    }
    case 'dotted': {
      const ox = centred(w, 5);
      const oy = centred(h, 5);
      const dot = dark ? 'rgba(235,238,230,0.45)' : 'rgba(70,80,100,0.6)';
      return (
        `<defs><pattern id="d" x="${ox - 2.5}" y="${oy - 2.5}" width="5" height="5" patternUnits="userSpaceOnUse">` +
        `<circle cx="2.5" cy="2.5" r="0.3" fill="${dot}"/></pattern></defs>` +
        `<rect width="${w}" height="${h}" fill="url(#d)"/>`
      );
    }
    case 'music': {
      const staff = dark ? 'rgba(235,238,230,0.45)' : 'rgba(40,40,50,0.65)';
      const pitch = 20;
      const count = Math.max(0, Math.floor((h - 24 + 12) / pitch));
      const top = (h - (count * pitch - 12)) / 2;
      const out: string[] = [];
      for (let s = 0; s < count; s++) {
        const y0 = top + s * pitch;
        for (let l = 0; l < 5; l++) out.push(hl(y0 + l * 2, staff, 0.15, 12, w - 12));
        out.push(vl(12, staff, 0.15, y0, y0 + 8), vl(w - 12, staff, 0.15, y0, y0 + 8));
      }
      return out.join('');
    }
  }
}

/** An SVG data URL of the page background, `widthPx` pixels wide. */
/** Where the desktop app draws a background image: a placeholder with the file's name. */
function imageSvg(image: BackgroundImage | undefined): string {
  if (!image) return '';
  const { x, y, width, height } = image.rect;
  const [rx, ry, rw, rh] = [x, y, width, height].map((v) => v / 100);
  const name = image.name.replace(/[<>&"]/g, '');
  return (
    `<g opacity="${image.opacity ?? 1}"><rect x="${rx}" y="${ry}" width="${rw}" height="${rh}" fill="#dbe6f5" stroke="#7a95c4"/>` +
    `<text x="${rx + rw / 2}" y="${ry + rh / 2}" font-size="${Math.max(4, rh / 20)}" text-anchor="middle" fill="#4a6496">${name}</text></g>`
  );
}

export function backgroundDataUrl(page: PageSize, background: Background, widthPx: number, image?: BackgroundImage): string {
  const w = page.width / 100;
  const h = page.height / 100;
  const { css, dark } = paperColor(background.paper);
  const width = Math.max(1, Math.round(widthPx || 1024));
  const height = Math.max(1, Math.round((width * page.height) / Math.max(1, page.width)));
  const texture = background.texture
    ? `<filter id="t"><feTurbulence type="fractalNoise" baseFrequency="0.9" numOctaves="2" seed="7"/>` +
      `<feColorMatrix values="0 0 0 0 ${dark ? 1 : 0} 0 0 0 0 ${dark ? 1 : 0} 0 0 0 0 ${dark ? 1 : 0} 0 0 0 ${dark ? 0.08 : 0.06} 0"/></filter>` +
      `<rect width="${w}" height="${h}" filter="url(#t)"/>`
    : '';
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none">` +
    `<rect width="${w}" height="${h}" fill="${css}"/>${texture}${templateSvg(background, w, h, dark)}${imageSvg(image)}</svg>`;
  return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}
