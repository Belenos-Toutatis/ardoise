/**
 * A note as Rust serialises it (`ardoise_core::note::Note`, `ardoise_ink::model`). Coordinates
 * are page coordinates in device units (10 µm), origin top-left; an A4 landscape page is
 * 29700 × 21600. Times are milliseconds since `created`.
 */
import type { InkPoint, Tool } from './ink';
import type { NarrationInfo } from './video';

export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

export interface Stroke {
  tool: Tool;
  color: Rgba;
  /** Nominal diameter at full pressure, device units. */
  width: number;
  /** Index into `Note.layers`. */
  layer: number;
  points: InkPoint[];
}

export type Paper = 'white' | 'cream' | 'kraft' | 'slate' | { custom: Rgba };
export type Template = 'blank' | 'lined' | 'seyes' | 'grid5mm' | 'millimetric' | 'dotted' | 'music';

export interface Background {
  paper: Paper;
  template: Template;
  texture: boolean;
}

/** Page coordinates in device units (10 µm); may reach outside the page. */
export interface ImageRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/**
 * An imported image or PDF page drawn over the paper, under the strokes (format 1.2). Its pixels
 * are the note file's `background.png`/`background.jpg` member, which the interface never holds:
 * Rust renders the page background with it (`ardoise_core::note::BackgroundImage`).
 */
export interface BackgroundImage {
  /** Name of the imported file, for display. */
  name: string;
  /** Page of the PDF it comes from, from 1; absent for an image. */
  pdfPage?: number;
  /** Where the image is stretched to, after its quarter turns. */
  rect: ImageRect;
  /** Clockwise quarter turns of the stored pixels (0–3); absent means 0. */
  quarterTurns?: number;
  /** 0 to 1; absent means 1. */
  opacity?: number;
}

export interface Layer {
  name: string;
  visible: boolean;
}

export interface PageSize {
  width: number;
  height: number;
}

export type Origin =
  | { kind: 'tablet' }
  | { kind: 'live' }
  | { kind: 'inkspace'; id: string }
  | { kind: 'ardoise' };

export interface Note {
  id: string;
  title: string;
  created: number;
  modified: number;
  page: PageSize;
  background: Background;
  /** Imported image or PDF page under the strokes (format 1.2); absent when there is none. */
  backgroundImage?: BackgroundImage;
  tags: string[];
  /** Absent means false (format 1.0 files). */
  favorite?: boolean;
  layers: Layer[];
  strokes: Stroke[];
  /** What the page holds besides its strokes (format 1.4): text frames. Absent means none. */
  objects?: PageObject[];
  timing: 'recorded' | 'synthesized';
  origin: Origin;
}

/**
 * Something on the page that is not a stroke (format 1.4, `docs/design/page-objects.md`,
 * `ardoise_core::objects::PageObject`). Every kind has this placement: the point (u, v) of the
 * object's content (its own units, y down) is on the page at (x, y) + turn(scale × (u, v)), turn
 * being `quarterTurns` clockwise quarter turns; the content fills [0, width] × [0, height]. The
 * other keys belong to the kind; a kind this version does not know keeps them all (spread copies
 * carry them), is moved with the page by its placement, and is drawn nowhere.
 */
export interface PageObject {
  kind: string;
  /** Unique within the note. */
  id: string;
  /** Index into `Note.layers`: drawn with its layer, above its strokes. */
  layer: number;
  /** Milliseconds since `created`: when the replay shows it, all at once. */
  t: number;
  x: number;
  y: number;
  /** Page units per unit of the content, > 0. */
  scale: number;
  /** 0 to 3. */
  quarterTurns: number;
  width: number;
  height: number;
  [key: string]: unknown;
}

/** The families of the text tool (`ardoise_text::FontFamily`). */
export type FontFamily = 'latinModern' | 'andika';

export const FONT_FAMILIES: readonly FontFamily[] = ['latinModern', 'andika'];

/**
 * A text frame (SPEC §4.4): one line of the page per line of `text` (nothing wraps), one style for
 * the whole frame. `size` is the font's size (its em) in the object's units, device units at scale 1.
 */
export interface TextObject extends PageObject {
  kind: 'text';
  text: string;
  font: FontFamily;
  bold: boolean;
  size: number;
  color: Rgba;
}

/** A text frame this version can read and draw (a text frame of a font it does not know is kept but not drawn). */
export function isTextObject(object: PageObject): object is TextObject {
  return (
    object.kind === 'text' &&
    typeof object.text === 'string' &&
    FONT_FAMILIES.includes(object.font as FontFamily) &&
    typeof object.bold === 'boolean' &&
    typeof object.size === 'number' &&
    object.size > 0 &&
    typeof object.color === 'object' &&
    object.color !== null
  );
}

/** Modification time and size of a note file, as last seen; passed back when saving. */
export interface FileStamp {
  modified: { secs_since_epoch: number; nanos_since_epoch: number };
  len: number;
}

/** One note of the library listing (read from `note.json` only). */
export interface NoteSummary {
  path: string;
  /** Folder relative to the library root, `/`-separated; empty for the root. */
  binder: string;
  fileName: string;
  id: string;
  title: string;
  created: number;
  modified: number;
  tags: string[];
  favorite: boolean;
  /** The text of its text frames on visible layers, in reading order, one per line; empty without any (format 1.4). */
  typedText?: string;
  stamp: FileStamp;
  /** Set when the file is a Nextcloud conflict copy of `originalName`. */
  conflict: { originalName: string } | null;
}

export interface TagCount {
  tag: string;
  count: number;
}

export interface LibraryListing {
  notes: NoteSummary[];
  /** Every binder, relative and `/`-separated, sorted. */
  binders: string[];
  tags: TagCount[];
  /** Files that could not be read (damaged, newer version…). */
  problems: { path: string; code: string; message: string }[];
  /**
   * The free order of the binders that have one (`_ardoise-order.json`): the ids of their notes, first to last,
   * keyed by binder (`/`-separated). A binder without an entry has no order: its notes go by date.
   */
  orders: Record<string, string[]>;
}

export interface TrashEntry {
  id: string;
  title: string | null;
  /** Where it came from, relative to the library root. */
  original: string;
  /** Unix milliseconds. */
  deleted: number;
  noteId: string | null;
}

/** One line of the account of a revision. */
export interface RevisionEntry {
  /** Unix milliseconds. */
  at: number;
  /** What changed. Written by an assistant: shown as plain text, never as HTML. */
  text: string;
}

/** Where the source of a revision is in the library now. */
export interface RevisionSource {
  path: string;
  /** Its title now (it may have been renamed since the copy). */
  title: string;
}

/**
 * What makes a note a revision, a retouched copy of another note made by an assistant (SPEC §4.9):
 * the content of its `revision.json`, and its source found by id in the library.
 */
export interface RevisionOfNote {
  /** Id of the source. */
  of: string;
  /** Its title when the copy was made. */
  ofTitle: string;
  /** When the copy was made (Unix milliseconds). */
  created: number;
  /** The account, oldest first. */
  summary: RevisionEntry[];
  /** The source; null when it is gone (trashed, deleted, not synchronised here yet). */
  source: RevisionSource | null;
}

/**
 * The title to show for the source of a revision: the one it has now, else the one it had when the
 * copy was made, else `untitled` (the title of an untitled note is an empty string, which `??`
 * would let through and the line would read « »).
 */
export function revisionSourceTitle(revision: Pick<RevisionOfNote, 'source' | 'ofTitle'>, untitled: string): string {
  return (revision.source?.title ?? revision.ofTitle).trim() || untitled;
}

/** A note opened for editing. */
export interface OpenedNote {
  note: Note;
  stamp: FileStamp;
  /** Set when the note is a revision. */
  revision?: RevisionOfNote | null;
  /** The narration the note holds (M17: read when it opens, not only in a replay); null without one. */
  narration?: NarrationInfo | null;
}

/** The registered tablet (`_ardoise/device.json`), shared by every PC. */
export interface TabletInfo {
  name: string;
  address: string;
  /** RFC 3339. */
  registered: string;
  width: number;
  height: number;
}

// --- Milestone M5: exports, split, merge (docs/design/exports-and-operations.md) ----------

export type ExportFormat = 'png' | 'jpg' | 'webp' | 'svg' | 'pdf';

export interface ExportOptions {
  /** Resolution of raster formats, 72..=600. */
  dpi: number;
  /** Draw the paper (colour, template, texture); without it PNG is transparent, JPG white. */
  background: boolean;
  /** JPG quality, 1..=100. */
  quality: number;
  /** PDF page: the note's own size, or A4 in the note's orientation (note fitted, centred). */
  pdfPage: 'actual' | 'a4';
  /**
   * SVG and PDF: one layer per layer of the note (`separate`: Inkscape layers, PDF optional content) or
   * one drawing (`merged`). Absent: the format's default (SVG separate, PDF merged). Ignored by the images,
   * which are always merged. Hidden layers are never exported.
   */
  layers?: 'separate' | 'merged';
}

/**
 * A note to export: the note at `path`, or the editor's current `note` (unsaved changes
 * included). `path` also names the output file. `imageKey`: the staged pixels of the editor
 * note's background image (`lib/background.ts`); without it they come from the file.
 */
export interface ExportSource {
  path: string;
  note?: Note;
  imageKey?: string;
  /**
   * Where the note goes in the outline (the bookmarks) of a PDF that holds several notes: the binders it is in, below the
   * exported one (none for a note of the exported binder itself), and the title its bookmark shows. Only « Exporter le
   * classeur en PDF » gives it; an export without it has no outline.
   */
  outline?: { path: string[]; title: string };
}

/** How a printed note sits on the sheet: its real size (reduced if too large), or as large as fits. */
export type PrintFit = 'actual' | 'fit';

/** How many notes share a sheet of paper. */
export type PrintPerSheet = 1 | 2 | 4;

export interface PrintOptions {
  /** Print the paper (colour, template, texture, background image); otherwise ink on white. */
  background: boolean;
  fit: PrintFit;
  /** Notes per sheet: 1, or 2 or 4 side by side on one sheet. */
  perSheet: PrintPerSheet;
}

export interface PrintResult {
  /** False when the user closed Windows' print dialog without printing. */
  printed: boolean;
  /** Pages sent to the printer. */
  pages: number;
}

/** How to split a note: at an instant (ms since `created`), or by a traced zone (page units). */
export type SplitSpec = { kind: 'time'; at: number } | { kind: 'zone'; polygon: [number, number][] };

/** The kept part, as saved over the note, and the new note made of the rest. */
export interface SplitResult {
  note: Note;
  stamp: FileStamp;
  created: NoteSummary;
}

/** Overlay: every note on one page; end to end: one long page, notes one below the other. */
export type MergeLayout = 'overlay' | 'endToEnd';

/** Automatic export of each synced note into a sub-folder of the library (SPEC §3.2). */
export interface AutoExportSettings {
  format: 'off' | 'pdf' | 'png';
  /** Relative to the library root, `/`-separated. */
  folder: string;
}

/**
 * "Redresser les formes" as the user set it (SPEC §4.4, Q12): `[shapes]` of `ardoise.toml`, the
 * same for every note and every PC using this copy. A value an alignment box turns off is kept,
 * so that ticking the box again brings it back. `lib/shape-settings.ts`: defaults, limits, and the
 * `ShapeTuning` that `Ink.straighten` takes.
 */
export interface ShapeSettings {
  /** 1 (strict) to 5 (lenient); 3: the thresholds of before M15. */
  tolerance: number;
  /** Smallest line or arrow, in mm (5 to 25; 10); a closed shape needs 3 mm more. */
  minSizeMm: number;
  /** Make what is nearly horizontal or vertical exactly so. */
  snapAngle: boolean;
  /** Up to how many degrees (1 to 10; 4). */
  snapAngleDeg: number;
  /** Make a circle or a square of what is nearly round or square. */
  snapRound: boolean;
  /** Within how many percent of round or square (2 to 20; 10). */
  snapRoundPercent: number;
}

/** Colours of Inkspace's palette, offered first. */
export const INKSPACE_COLORS: Rgba[] = [
  { r: 0x00, g: 0x00, b: 0x00, a: 255 },
  { r: 0x51, g: 0xa7, b: 0xf9, a: 255 },
  { r: 0x84, g: 0xbd, b: 0x00, a: 255 },
  { r: 0xbb, g: 0x3e, b: 0x9a, a: 255 },
  { r: 0xd8, g: 0x00, b: 0x00, a: 255 },
  { r: 0xff, g: 0x6e, b: 0x03, a: 255 },
  { r: 0x6b, g: 0x6b, b: 0x6b, a: 255 },
];

export function cssColor(c: Rgba): string {
  return `rgba(${c.r}, ${c.g}, ${c.b}, ${(c.a / 255).toFixed(3)})`;
}
