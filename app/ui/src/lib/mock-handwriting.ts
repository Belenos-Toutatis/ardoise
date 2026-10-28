/**
 * Browser stand-in for the handwriting commands (`handwriting_info`, `handwriting_search`, the
 * text export…), over the demo library of `mock-data.ts`. What it "recognises" in a note is a
 * fixed text per note id (`mockTexts`); any other note reads as nothing. Like the desktop app, a
 * query word is found where a recognised word begins with it, accents and case ignored.
 */
import type {
  Excerpt,
  HandwritingHit,
  HandwritingInfo,
  HandwritingLanguage,
  HandwritingSetting,
  IndexStatus,
  NoteText,
  RecognizerInfo,
  StructuredExport,
  StructuredFormat,
  TextLabels,
} from './handwriting';
import { normalize, queryWords } from './library-logic';
import { UNHANDLED, mockLibrary } from './mock-data';
import { mockClipboard } from './mock-export';
import { failure } from './mock-library';
import type { ExportSource, Note } from './note';

export const FRENCH_RECOGNIZER: RecognizerInfo = {
  name: "Reconnaissance d'écriture Microsoft - Français",
  language: 'fr',
};
export const ENGLISH_RECOGNIZER: RecognizerInfo = {
  name: 'Microsoft English (US) Handwriting Recognizer',
  language: 'en',
};

/** What the stand-in "reads" in the notes, by note id (the three demo notes by default). */
export const mockTexts = new Map<string, string>();

function defaultTexts(): [string, string][] {
  return [
    ['b1f0c6c2-0000-4000-8000-000000000001', 'la lumière se propage\nen ligne droite dans un milieu homogène'],
    ['b1f0c6c2-0000-4000-8000-000000000002', 'pain lait beurre\npommes et farine'],
    ['b1f0c6c2-0000-4000-8000-000000000003', 'rendez-vous jeudi à dix-sept heures'],
  ];
}

/** The stand-in's state; tests change it. */
export const mockHandwriting = {
  supported: true,
  recognizers: [FRENCH_RECOGNIZER] as RecognizerInfo[],
  setting: 'auto' as HandwritingSetting,
  /** The interface language `auto` follows. */
  interfaceLanguage: 'fr' as HandwritingLanguage,
  stopped: false,
  /** A status to report instead of the computed one (a test showing the indexing under way). */
  status: null as IndexStatus | null,
  /** The last text copied to the clipboard. */
  copied: null as string | null,
  /** The last text export. */
  exported: null as { destination: string; document: string } | null,
  /** The last text export with structure. */
  exportedStructured: null as { format: StructuredFormat; destination: string; labels: TextLabels } | null,
};

export function resetMockHandwriting(): void {
  Object.assign(mockHandwriting, {
    supported: true,
    recognizers: [FRENCH_RECOGNIZER],
    setting: 'auto',
    interfaceLanguage: 'fr',
    stopped: false,
    status: null,
    copied: null,
    exported: null,
  });
  mockTexts.clear();
  for (const [id, text] of defaultTexts()) mockTexts.set(id, text);
}

resetMockHandwriting();

/** The language recognised, as the desktop app resolves it. */
function language(): HandwritingLanguage | null {
  if (!mockHandwriting.supported) return null;
  const has = (l: HandwritingLanguage) => mockHandwriting.recognizers.some((r) => r.language === l);
  const setting = mockHandwriting.setting;
  if (setting !== 'auto') return has(setting) ? setting : null;
  if (has(mockHandwriting.interfaceLanguage)) return mockHandwriting.interfaceLanguage;
  return mockHandwriting.recognizers.find((r) => r.language)?.language ?? null;
}

function status(): IndexStatus {
  if (mockHandwriting.status) return { ...mockHandwriting.status };
  const lang = language();
  const total = mockLibrary().notes.size;
  const base = { language: lang, done: total, total, failed: 0, reason: null };
  if (!mockHandwriting.supported) return { ...base, state: 'unavailable', reason: 'unsupported', done: 0, total: 0 };
  if (!lang) return { ...base, state: 'unavailable', reason: 'noRecognizer', done: 0, total: 0 };
  if (mockHandwriting.stopped) return { ...base, state: 'stopped' };
  return { ...base, state: 'idle' };
}

const listeners = new Set<(status: IndexStatus) => void>();

/** What `onHandwritingStatus` does in the browser. */
export function mockOnHandwritingStatus(handler: (status: IndexStatus) => void): () => void {
  listeners.add(handler);
  return () => listeners.delete(handler);
}

/** Sends the current status to the listeners, like the desktop app's event. */
export function emitMockHandwritingStatus(): void {
  const current = status();
  for (const listener of [...listeners]) listener(current);
}

function info(): HandwritingInfo {
  return {
    supported: mockHandwriting.supported,
    recognizers: mockHandwriting.supported ? [...mockHandwriting.recognizers] : [],
    setting: mockHandwriting.setting,
    language: language(),
    status: status(),
  };
}

const CONTEXT_WORDS = 4;

function excerpt(words: string[], index: number): Excerpt {
  const before = words.slice(Math.max(0, index - CONTEXT_WORDS), index);
  const after = words.slice(index + 1, index + 1 + CONTEXT_WORDS);
  const head = (index > CONTEXT_WORDS ? '… ' : '') + before.join(' ');
  const tail = after.join(' ') + (index + 1 + CONTEXT_WORDS < words.length ? ' …' : '');
  return { before: head ? `${head} ` : '', word: words[index], after: tail ? ` ${tail}` : '' };
}

/** The query words found in `text` and the excerpt at the first one in reading order. */
export function searchText(text: string, words: string[]): { words: string[]; excerpt: Excerpt } | null {
  const lines = text.split('\n').map((line) => line.split(' ').filter(Boolean));
  const found: string[] = [];
  let first: { line: number; index: number } | null = null;
  for (const query of words) {
    let hit: { line: number; index: number } | null = null;
    lines.some((line, l) => {
      const index = line.findIndex((word) => normalize(word).startsWith(query));
      if (index >= 0) hit = { line: l, index };
      return index >= 0;
    });
    if (!hit) continue;
    const at: { line: number; index: number } = hit;
    found.push(query);
    if (!first || at.line < first.line || (at.line === first.line && at.index < first.index)) first = at;
  }
  if (!first) return null;
  return { words: found, excerpt: excerpt(lines[first.line], first.index) };
}

function search(query: string): HandwritingHit[] {
  const words = [...new Set(queryWords(query))];
  if (words.length === 0 || status().state === 'unavailable' || mockHandwriting.stopped) return [];
  const hits: HandwritingHit[] = [];
  for (const [path, stored] of mockLibrary().notes) {
    const found = searchText(mockTexts.get(stored.note.id) ?? '', words);
    if (found) hits.push({ path, ...found });
  }
  return hits;
}

function sourceNote(source: ExportSource): Note {
  return source.note ?? mockLibrary().notes.get(source.path)?.note ?? failure('noteNotFound', source.path);
}

function texts(sources: ExportSource[]): NoteText[] {
  if (!mockHandwriting.supported) failure('handwritingUnsupported', 'not Windows');
  if (!language()) failure('handwritingNoRecognizer', 'no recognizer');
  return sources.map((source) => {
    const note = sourceNote(source);
    return {
      path: source.path,
      title: note.title.trim() || new Date(note.created).toISOString().slice(0, 10),
      text: mockTexts.get(note.id) ?? '',
    };
  });
}

/** The text export of the desktop app: each note's title underlined, then its text. */
export function textDocument(notes: NoteText[]): string {
  return notes
    .map((note) => {
      const lines = note.text ? note.text.split('\n').map((line) => `${line}\r\n`).join('') : '';
      return `${note.title}\r\n${'='.repeat(Math.max(1, [...note.title].length))}\r\n\r\n${lines}`;
    })
    .join('\r\n\r\n');
}

/** Answers the handwriting commands; `UNHANDLED` for the others. */
export function mockHandwritingInvoke(command: string, args: Record<string, unknown>): unknown {
  switch (command) {
    case 'handwriting_info':
      return info();
    case 'set_handwriting_language':
      mockHandwriting.setting = args.language as HandwritingSetting;
      queueMicrotask(emitMockHandwritingStatus);
      return info();
    case 'handwriting_status':
      return status();
    case 'handwriting_search':
      return search(args.query as string);
    case 'handwriting_stop':
      mockHandwriting.stopped = true;
      queueMicrotask(emitMockHandwritingStatus);
      return status();
    case 'handwriting_resume':
      mockHandwriting.stopped = false;
      queueMicrotask(emitMockHandwritingStatus);
      return undefined;
    case 'handwriting_text':
      return texts(args.sources as ExportSource[]);
    case 'export_handwriting_text': {
      const destination = args.destination as string;
      mockHandwriting.exported = { destination, document: textDocument(texts(args.sources as ExportSource[])) };
      return destination;
    }
    case 'export_structured_text': {
      const destination = args.destination as string;
      const format = args.format as StructuredFormat;
      texts(args.sources as ExportSource[]);
      mockHandwriting.exportedStructured = { format, destination, labels: args.labels as TextLabels };
      return { paths: [destination], analyzed: true } satisfies StructuredExport;
    }
    case 'copy_handwriting_text': {
      const document = textDocument(texts(args.sources as ExportSource[]));
      if (!mockClipboard.available) failure('clipboardUnavailable', 'no clipboard in this test');
      mockHandwriting.copied = document;
      return undefined;
    }
    default:
      return UNHANDLED;
  }
}
