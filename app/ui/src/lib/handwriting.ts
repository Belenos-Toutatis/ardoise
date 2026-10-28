/**
 * Handwriting recognition (Rust side: `app/src-tauri/src/handwriting.rs`): the shapes the
 * commands send and the pure helpers of the screens that show them.
 */

/** A language the app recognises. */
export type HandwritingLanguage = 'fr' | 'en';

/** The setting (`[handwriting] language`, shared by every PC). */
export type HandwritingSetting = 'auto' | HandwritingLanguage;

export interface RecognizerInfo {
  /** As Windows names it, in its own language. */
  name: string;
  /** null for a language the app does not know. */
  language: HandwritingLanguage | null;
}

export type IndexState = 'idle' | 'indexing' | 'paused' | 'stopped' | 'unavailable';

/** Why there is no recognition here. */
export type UnavailableReason = 'unsupported' | 'noRecognizer' | 'noLibrary';

/** Progress of the background indexing (event `handwriting-status`). */
export interface IndexStatus {
  state: IndexState;
  reason: UnavailableReason | null;
  language: HandwritingLanguage | null;
  /** Notes whose text is known, of `total`. */
  done: number;
  total: number;
  /** Notes that could not be read or recognised. */
  failed: number;
}

export interface HandwritingInfo {
  /** False outside Windows. */
  supported: boolean;
  /** Installed on this PC. */
  recognizers: RecognizerInfo[];
  setting: HandwritingSetting;
  /** The language recognised on this PC (null: no recognizer for it). */
  language: HandwritingLanguage | null;
  status: IndexStatus;
}

/** A short excerpt of the recognised text around a found word. */
export interface Excerpt {
  before: string;
  word: string;
  after: string;
}

/** What the handwriting of one note has of a query. */
export interface HandwritingHit {
  path: string;
  /** The query words found (lower case, accents removed, as `queryWords` gives them). */
  words: string[];
  excerpt: Excerpt;
}

/** The recognised text of one note. */
export interface NoteText {
  path: string;
  /** Its title, or its date when it has none. */
  title: string;
  /** Line after line. */
  text: string;
}

export const HANDWRITING_SETTINGS: HandwritingSetting[] = ['auto', 'fr', 'en'];

/** Whether a recognizer of `language` is installed. */
export function hasRecognizer(recognizers: readonly RecognizerInfo[], language: HandwritingLanguage): boolean {
  return recognizers.some((r) => r.language === language);
}

/**
 * The language whose handwriting pack is worth installing, or null: the one chosen when it is
 * missing, else (in automatic mode) the interface language when it is missing.
 */
export function missingRecognizer(
  info: Pick<HandwritingInfo, 'supported' | 'recognizers' | 'setting'>,
  interfaceLanguage: HandwritingLanguage,
): HandwritingLanguage | null {
  if (!info.supported) return null;
  const wanted = info.setting === 'auto' ? interfaceLanguage : info.setting;
  return hasRecognizer(info.recognizers, wanted) ? null : wanted;
}

/** A sentence key and its values. */
export interface Notice {
  key: string;
  values?: Record<string, string | number>;
}

/** What the library search says about the handwriting, or null when there is nothing to say. */
export function searchNotice(status: IndexStatus | null): Notice | null {
  if (!status) return null;
  const values = { done: status.done, total: status.total };
  switch (status.state) {
    case 'indexing':
      return { key: 'handwriting.search.indexing', values };
    case 'paused':
      return { key: 'handwriting.search.paused', values };
    case 'stopped':
      return { key: 'handwriting.search.stopped', values };
    case 'unavailable':
      if (status.reason === 'unsupported') return { key: 'handwriting.search.unsupported' };
      if (status.reason === 'noRecognizer') return { key: 'handwriting.search.noRecognizer' };
      return null;
    default:
      return null;
  }
}

/** The indexing as the settings screen states it. */
export function statusNotice(status: IndexStatus): Notice {
  const values = { done: status.done, total: status.total, failed: status.failed };
  switch (status.state) {
    case 'indexing':
      return { key: 'handwriting.status.indexing', values };
    case 'paused':
      return { key: 'handwriting.status.paused', values };
    case 'stopped':
      return { key: 'handwriting.status.stopped', values };
    case 'unavailable':
      return { key: `handwriting.status.${status.reason ?? 'noRecognizer'}` };
    default:
      return { key: status.failed > 0 ? 'handwriting.status.idleFailed' : 'handwriting.status.idle', values };
  }
}

/** The text export with structure: Markdown, or a Word document. */
export type StructuredFormat = 'md' | 'docx';

/** The words the exported files themselves contain, in the interface language. */
export interface TextLabels {
  date: string;
  tags: string;
  drawing: string;
}

export interface StructuredExport {
  /** The files written: the document, then the drawings' images next to a Markdown file. */
  paths: string[];
  /** False when Windows' layout analysis was not available: the structure was guessed from the text. */
  analyzed: boolean;
}

/** A file name for the text export of `stem` (a note's file name without extension). */
export function textFileName(stem: string): string {
  return `${stem}.txt`;
}
