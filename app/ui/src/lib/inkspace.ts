/** Types of the Inkspace import (M10; Rust side: `app/src-tauri/src/import.rs`). */

/** `InkspaceSource` in `app/src-tauri/src/import.rs`: what a folder of Inkspace data holds. */
export interface InkspaceSource {
  /** Inkspace's `LocalState` folder (the folder given, normalised: the user may pick the package folder, LocalState, WacomCloudDb or WacomCloudDb\_). */
  path: string;
  /** Notes found (one per Inkspace page), readable or not. */
  notes: number;
  /** Notes among `notes` already in the library or its trash: an import skips them. */
  alreadyImported: number;
  /** Notes among `notes` that cannot be read (the import reports them as failures). */
  unreadable: number;
}

/** `ImportProgress`: notes handled so far (imported, skipped or failed) out of `total`. Sent once before the first note (done 0) and after each one. */
export interface InkspaceProgress {
  done: number;
  total: number;
}

/** `ImportFailure`: a note that was not imported. */
export interface InkspaceFailure {
  /** Inkspace's note id (UUID). */
  inkspaceId: string;
  /** When the note was written (Unix ms), if it could be read. */
  created: number | null;
  /** Selects `errorCodes.<code>`; `message` is the technical detail. */
  code: string;
  message: string;
}

/** `LibraryImport`: the final report. */
export interface InkspaceReport {
  /** Binder the notes went into, `/`-separated, '' for the library root. */
  binder: string;
  /** Notes found. */
  total: number;
  imported: number;
  /** Already in the library or its trash. */
  skipped: number;
  failed: InkspaceFailure[];
  /** The user stopped it: notes after the last one handled were not looked at (imported notes stay, each complete). */
  cancelled: boolean;
}

/** Binder the import proposes (Inkspace's notes have no binders of their own). */
export const DEFAULT_BINDER = 'Inkspace';
