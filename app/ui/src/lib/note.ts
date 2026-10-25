/**
 * A note as Rust serialises it (`ardoise_core::note::Note`, `ardoise_ink::model`). Coordinates
 * are page coordinates in device units (10 µm), origin top-left; an A4 landscape page is
 * 29700 × 21600. Times are milliseconds since `created`.
 */
import type { InkPoint, Tool } from './ink';

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
  timing: 'recorded' | 'synthesized';
  origin: Origin;
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
  problems: { path: string; message: string }[];
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

/** A note opened for editing. */
export interface OpenedNote {
  note: Note;
  stamp: FileStamp;
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

export type ExportFormat = 'png' | 'jpg' | 'svg' | 'pdf';

export interface ExportOptions {
  /** Resolution of raster formats, 72..=600. */
  dpi: number;
  /** Draw the paper (colour, template, texture); without it PNG is transparent, JPG white. */
  background: boolean;
  /** JPG quality, 1..=100. */
  quality: number;
  /** PDF page: the note's own size, or A4 in the note's orientation (note fitted, centred). */
  pdfPage: 'actual' | 'a4';
}

/**
 * A note to export: the note at `path`, or the editor's current `note` (unsaved changes
 * included). `path` also names the output file.
 */
export interface ExportSource {
  path: string;
  note?: Note;
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
