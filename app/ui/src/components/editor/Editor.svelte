<script lang="ts" module>
  /**
   * Done once the editor last torn down has finished its writes and released its background
   * images: an editor opening a note waits for it, so that this late release never empties the
   * staging area it has just filled.
   */
  let lastRelease: Promise<void> = Promise.resolve();
</script>

<script lang="ts">
  /**
   * The note editor (milestone M3). Contract used by the library screen: it opens the note at
   * `path`, calls `onsaved` with the note's path after each save or rename (the path changes
   * when the title changes, or becomes the copy's after "save as a copy"), and `onclose` when
   * the user goes back to the library. Neither is called once the editor is closed or destroyed.
   *
   * State: an undo/redo history of immutable `Doc` snapshots (note + selection + active layer,
   * `lib/editor/state.ts`), tool settings beside it. The note is dirty while the present note
   * is not the object last saved. Every write to the note's file (save, rename, copy) goes
   * through one queue, so that each uses the path and stamp left by the previous one. Going back
   * to the library waits for the writes still queued (a "save as a copy" needs the background
   * image's pixels, which Rust keeps only while the editor is there); the editor releases those
   * pixels when it goes away, once its writes are done.
   *
   * Replay mode (milestone M6, SPEC §4.5): the page is read-only and shows the note as drawn at the
   * playhead (`ReplayPlayer`, `lib/replay.ts`), with a bar of controls; the narration of the note
   * plays along, and a new one is recorded while the replay runs (`NarrationRecorder`). While it
   * records, the note cannot be renamed, copied or reloaded (the recording is saved into the note
   * at its current path when it stops); a recording that cannot be saved is kept for a retry.
   *
   * Milestone M7: "Redresser les formes" straightens the shapes of the selection
   * (`lib/editor/shapes.ts`, the WebAssembly geometry); "Revenir à l'original" puts back the ink
   * the note had when it came from the tablet or before its first edit (`note_original`,
   * `lib/editor/original.ts`). Both are one undoable step, saved like any edit.
   */
  import { untrack } from 'svelte';
  import { api, errorMessage, onLibraryChanged, onSessionEnding, type AppState } from '../../lib/api';
  import { NoteLoads, bridgeScreens } from '../../lib/app-bridge.svelte';
  import { loadCustomColors, rememberCustomColor } from '../../lib/editor/colors';
  import { binderOf, diskChange, type DiskChange } from '../../lib/editor/disk';
  import { DraftKeeper, findOffer, type Offer } from '../../lib/editor/drafts';
  import { decodeStrokes, encodeStrokes } from '../../lib/editor/clipboard';
  import type { PixelsRef } from '../../lib/background';
  import { pixelsOf, removeBackgroundImage, setBackgroundImage, setImageFit, setImageOpacity } from '../../lib/editor/image';
  import { strokesOutsidePage } from '../../lib/editor/ops';
  import type { Orientation } from '../../lib/editor/page-size';
  import { revertToOriginal } from '../../lib/editor/original';
  import { straightenSelection } from '../../lib/editor/shapes';
  import { canRedo, canUndo, commit, createHistory, redo, replacePresent, undo, type History } from '../../lib/editor/history';
  import {
    DEFAULT_TOOLS,
    addLayer,
    applyStyle,
    autoCrop,
    canDraw,
    clearSelection,
    deleteSelection,
    docFromNote,
    duplicateSelection,
    mergeDown,
    moveLayer,
    pasteStrokes,
    renameLayer,
    resizePage,
    rotate,
    rotateBackgroundImage,
    selectAll,
    selectedStrokes,
    setActiveLayer,
    setBackground,
    setLayerVisible,
    setOrientation,
    type Doc,
    type EraserMode,
    type Mode,
    type ToolSettings,
  } from '../../lib/editor/state';
  import {
    EMPTY_PREVIEW,
    canSplit,
    previewSplit,
    timeRange,
    type SplitMode,
  } from '../../lib/editor/split';
  import { defaultWindowClose, type WindowClose } from '../../lib/editor/window-close';
  import { largestFittingDpi } from '../../lib/export-options';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { loadInk, type Ink, type Tool } from '../../lib/ink';
  import type { BackgroundImage, ExportSource, FileStamp, Note, Rgba, SplitSpec, Stroke } from '../../lib/note';
  import type { NoteOriginal } from '../../lib/original';
  import type { NarrationInfo } from '../../lib/video';
  import ExportDialog from '../ExportDialog.svelte';
  import Modal from '../Modal.svelte';
  import PrintDialog from '../PrintDialog.svelte';
  import VideoExportDialog from '../VideoExportDialog.svelte';
  import { clock } from '../narration/format';
  import RecordPanel from '../narration/RecordPanel.svelte';
  import { NarrationRecorder } from '../narration/recorder.svelte';
  import BackgroundImageSection from './BackgroundImageSection.svelte';
  import Canvas from './Canvas.svelte';
  import LayersPanel from './LayersPanel.svelte';
  import OriginalPanel from './OriginalPanel.svelte';
  import PagePanel from './PagePanel.svelte';
  import RecoveryDialog from './RecoveryDialog.svelte';
  import { narrationType } from './narration-playback';
  import ReplayBar from './ReplayBar.svelte';
  import { ReplayPlayer } from './replay-player.svelte';
  import SplitPanel from './SplitPanel.svelte';
  import Toolbar from './Toolbar.svelte';

  let {
    path,
    app,
    onclose,
    onsaved,
    onchange,
    windowClose = defaultWindowClose(),
  }: {
    path: string;
    app: AppState;
    onclose: () => void;
    onsaved?: (path: string) => void;
    /** The app state changed (the microphone chosen here): the screen holding it takes the new one. */
    onchange?: (next: AppState) => void;
    /** How a window close is held back while there are unsaved changes (tests replace it). */
    windowClose?: WindowClose;
  } = $props();

  /** Nominal width remembered per ink tool (device units). */
  const DEFAULT_WIDTHS: Record<Tool, number> = {
    ballpoint: 60,
    fountainPen: 80,
    feltTip: 120,
    pencil: 50,
    brush: 150,
    highlighter: 500,
    chalk: 150,
  };

  /** How long a short notice stays, milliseconds. */
  const NOTICE_MS = 4000;

  /** Resolutions tried for the image copied to the clipboard: the first whose image fits. */
  const COPY_DPIS = [150, 96, 72];

  /** The copied image's resolution: 150 dpi, lower for a page that would exceed the size limit. */
  function copyDpi(source: ExportSource): number {
    const page = source.note?.page;
    return (page && largestFittingDpi([page], COPY_DPIS)) ?? COPY_DPIS[COPY_DPIS.length - 1];
  }

  /** The split panel's settings. */
  interface SplitState {
    mode: SplitMode;
    /** Instant of the cut, ms since `created`. */
    at: number;
    /** The traced zone, page units. */
    zone: [number, number][] | null;
    /** Title of the new note. */
    title: string;
    /** The user typed the title: it no longer follows the mode. */
    titleEdited: boolean;
    busy: boolean;
    error: string | null;
  }

  let currentPath = $state('');
  let loadError = $state<string | null>(null);
  /** The loads of this editor, for the assistant's `open_note` (answered once the note is shown). */
  const loads = new NoteLoads();
  let ink = $state.raw<Ink | null>(null);
  let history = $state.raw<History<Doc> | null>(null);
  let savedNote = $state.raw<Note | null>(null);
  let stamp = $state.raw<FileStamp | null>(null);
  let title = $state('');
  let titleDraft = $state('');
  let tools = $state.raw<ToolSettings>(DEFAULT_TOOLS);
  let widths = { ...DEFAULT_WIDTHS };
  let customColors = $state.raw<Rgba[]>(loadCustomColors());
  let saving = $state(false);
  let saveError = $state<string | null>(null);
  let conflict = $state<string | null>(null);
  let titleError = $state<string | null>(null);
  let disk = $state<DiskChange | null>(null);
  let notice = $state<string | null>(null);
  let noticeError = $state(false);
  let split = $state<SplitState | null>(null);
  /** What the export dialog exports; null while it is closed. */
  let exporting = $state.raw<ExportSource[] | null>(null);
  let printing = $state.raw<ExportSource[] | null>(null);
  let copying = $state(false);
  let leaving = $state(false);
  /** Another note was asked for while this one has changes: the path to open once answered. */
  let switching = $state<string | null>(null);
  /** Renames that failed so far: leaving waits for the answer of the queued ones. */
  let renameFailures = 0;
  /** The leave dialog was opened by a window close (not by the Back button). */
  let closing = $state(false);
  let spaceDown = $state(false);
  let canvas = $state<ReturnType<typeof Canvas>>();
  /** What the video export dialog exports; null while it is closed. */
  let video = $state.raw<ExportSource | null>(null);
  /** The video export dialog is exporting: the window must not close under it. */
  let videoRunning = $state(false);
  /** What "Revenir à l'original" would bring back; null while it is looked for. */
  let original = $state.raw<NoteOriginal | null>(null);
  let originalError = $state<string | null>(null);
  /** Bumped by each look for the original: only the latest answer counts. */
  let originalRequest = 0;

  // Replay mode.
  const player = new ReplayPlayer();
  const recorder = new NarrationRecorder();
  let replaying = $state(false);
  /** The narration stored in the note, while replaying. */
  let narration = $state.raw<NarrationInfo | null>(null);
  /** The narration's sound (a Blob URL) while it plays along. */
  let narrationUrl = $state<string | null>(null);
  let narrationLoading = $state(false);
  let narrationError = $state<string | null>(null);
  let recordOpen = $state(false);
  let confirmDelete = $state(false);
  /** The microphone chosen here, until the app state says so. */
  let chosenMicrophone = $state<string | null | undefined>(undefined);
  /** `Date.now()` when the recording's replay was at instant 0 and playing; places the sound in the replay. */
  let replayStartedAt: number | null = null;

  const doc = $derived(history?.present ?? null);
  const dirty = $derived(!!doc && doc.note !== savedNote);
  const hasSelection = $derived((doc?.selection.length ?? 0) > 0);
  const range = $derived(doc ? timeRange(doc.note) : null);
  const splitSpec = $derived.by((): SplitSpec | null => {
    if (!split) return null;
    if (split.mode === 'time') return { kind: 'time', at: split.at };
    return split.zone ? { kind: 'zone', polygon: split.zone.map(([x, y]): [number, number] => [x, y]) } : null;
  });
  const preview = $derived(doc && splitSpec ? previewSplit(doc.note, splitSpec) : EMPTY_PREVIEW);
  /** The page as drawn at the playhead: nothing selected, nothing to edit. */
  const replayDoc = $derived.by((): Doc | null => {
    const shown = player.shown;
    return replaying && doc && shown ? { ...doc, note: shown, selection: [] } : null;
  });
  /** The pan tool: a drag moves the page and never draws. */
  const replayTools = $derived<ToolSettings>({ ...tools, mode: 'pan' });
  const microphone = $derived(chosenMicrophone === undefined ? app.microphone : chosenMicrophone);
  const numberFormat = $derived(new Intl.NumberFormat(currentLocale(), { maximumFractionDigits: 2 }));
  /** The replay options a recording would use, as a sentence. */
  const recordInfo = $derived(
    t('narration.record.settings', {
      speed: t('replay.speedValue', { speed: numberFormat.format(player.options.speed) }),
      pauses: t(player.trimmed ? 'narration.record.pausesTrimmed' : 'narration.record.pausesKept'),
    }),
  );

  // Drafts (M10, `lib/editor/drafts.ts`): the unsaved note is kept outside the library a few
  // seconds after each change, and at once when Windows ends the session; a draft left by an
  // earlier run is offered when its note is opened.
  /** The id of the note in its file (the key of its draft; the edited note may carry another). */
  let noteId = '';
  let offer = $state.raw<Offer | null>(null);
  let recovering = $state(false);
  let recoveryError = $state<string | null>(null);
  let draftErrorShown = false;
  const drafts = new DraftKeeper(
    {
      write: (snapshot) => api.draftWrite(snapshot),
      discard: (id) => api.draftDiscard(id),
      unsaved: (unsaved) => api.setUnsavedWork(unsaved ? t('drafts.shutdownReason') : null),
    },
    {
      onerror: (error) => {
        if (!alive || draftErrorShown) return;
        draftErrorShown = true;
        showNotice(t('drafts.writeFailed', { message: errorMessage(error) }), true);
      },
    },
  );
  $effect(() => {
    const present = history?.present;
    const snapshot =
      present && dirty && stamp
        ? {
            path: currentPath,
            noteId,
            note: { ...present.note, title },
            baseStamp: stamp,
            imageKey: present.backgroundPixels?.key ?? null,
          }
        : null;
    untrack(() => drafts.update(snapshot));
  });

  /** False once the editor was closed or destroyed: late results call back no one. */
  let alive = true;
  /** Tail of the write queue. */
  let writes: Promise<void> = Promise.resolve();
  /** Writes queued or running. */
  let pendingWrites = 0;
  /** Bumped by each `open`: writes queued for the note opened before are dropped. */
  let generation = 0;
  /** The title once the queued renames are done. */
  let targetTitle = '';
  let noticeTimer: ReturnType<typeof setTimeout> | undefined;

  // (Re)open when the library screen gives another note; a path we produced ourselves (after
  // a rename or a copy) is already open.
  $effect(() => {
    const next = path;
    untrack(() => {
      if (next === currentPath) return;
      // Unsaved edits are never dropped silently, nor writes still queued (opening drops them).
      if (doc && (dirty || titleEdited())) {
        switching = next;
        leaving = true;
      } else if (pendingWrites > 0) {
        void settled().then(() => {
          if (alive && next !== currentPath) void open(next);
        });
      } else {
        void open(next);
      }
    });
  });

  // The assistant's bridge sees the note open and whether the editor may be left now
  // (`lib/app-bridge.svelte.ts`); the reasons are for the assistant, in English.
  $effect(() =>
    untrack(() =>
      bridgeScreens.register('editor', {
        path: () => currentPath || path,
        dirty: () => dirty,
        blocker: () =>
          recorder.busy
            ? 'a narration is being recorded in the editor'
            : videoRunning
              ? 'a video is being exported from the editor'
              : exporting || printing || video
                ? 'a dialog (export, print or video) is open in the editor: the teacher must close it first'
                : split
                  ? 'the split panel is open in the editor: the teacher must finish or cancel it first'
                  : leaving
                    ? 'the editor is asking whether to save its changes: the teacher must answer first'
                    : dirty || titleEdited()
                      ? 'the note open in the editor has unsaved changes: the teacher must save or close it first'
                      : pendingWrites > 0
                        ? 'the note open in the editor is being saved'
                        : null,
        loaded: () => loads.loaded(),
      }),
    ),
  );

  // Mounted: hold back window closes with unsaved changes, and watch the note on disk. The
  // teardown runs once, when the editor goes away.
  $effect(() => {
    const closer = untrack(() => windowClose);
    let stopped = false;
    const stops: (() => void)[] = [];
    const keep = (unlisten: () => void) => {
      if (stopped) unlisten();
      else stops.push(unlisten);
    };
    closer.subscribe(holdClose).then(keep, () => {
      // No close guard (e.g. not allowed): closing then works as before.
    });
    onLibraryChanged(() => void checkDisk()).then(keep, () => {
      // Not watched: saving still detects conflicts.
    });
    onSessionEnding(() => void drafts.flush()).then(keep, () => {
      // Not told: the drafts written after each change are there.
    });
    return () => {
      stopped = true;
      alive = false;
      clearTimeout(noticeTimer);
      for (const stop of stops) stop();
      player.dispose();
      recorder.dispose();
      // A recording is never thrown away silently: it goes into its note (or stays pending).
      salvageRecording();
      // Unsaved edits leave with the editor only as a draft (written now, with its image's pixels).
      const kept = drafts.dispose();
      dropNarrationSound();
      // The background images imported or opened here are no longer needed once the writes
      // still queued (a "save as a copy" started just before leaving) and the draft have used
      // their pixels; the next editor waits for this before it stages its own.
      lastRelease = Promise.all([settled(), kept])
        .then(() => api.releaseBackgroundImages())
        .catch(() => {});
    };
  });

  async function open(target: string) {
    // Another note is opened: the recording of this one is saved before the editor moves on, and
    // its unsaved edits (never expected here) are kept as a draft.
    salvageRecording();
    const detached = drafts.detach();
    offer = null;
    recoveryError = null;
    drafts.hold(false);
    generation += 1;
    currentPath = target;
    // Replaced by the load of another note if one starts before this one ends.
    const load = loads.begin();
    loadError = null;
    history = null;
    disk = null;
    split = null;
    exporting = null;
    printing = null;
    closeVideo();
    resetReplay();
    original = null;
    originalError = null;
    try {
      // The previous note's background images go; this note's image is kept in memory by Rust
      // (its key goes into the history with the note, `lib/editor/image.ts`).
      await settled();
      await detached;
      await lastRelease;
      await api.releaseBackgroundImages().catch(() => {});
      const [opened, module] = await Promise.all([api.openNote(target), ink ?? loadInk()]);
      if (currentPath !== target) return;
      let imageFailure: string | null = null;
      const staged = opened.note.backgroundImage
        ? await api.stageNoteBackground(target).catch((error: unknown) => {
            imageFailure = errorMessage(error);
            return null;
          })
        : null;
      if (currentPath !== target) return;
      const fresh = docFromNote(opened.note, t('editor.layers.defaultName', { n: 1 }));
      const start = staged ? { ...fresh, backgroundPixels: pixelsOf(staged) } : fresh;
      ink = module;
      noteId = opened.note.id;
      history = createHistory(start);
      savedNote = start.note;
      stamp = opened.stamp;
      title = opened.note.title;
      titleDraft = opened.note.title;
      targetTitle = opened.note.title;
      conflict = null;
      saveError = null;
      void refreshOriginal(true);
      void lookForDraft(target, opened);
      load.ok();
      // The note opens without its image: say so instead of showing a bare page.
      if (imageFailure !== null) showNotice(t('editor.backgroundLoadFailed', { message: imageFailure }), true);
    } catch (error) {
      if (currentPath === target) loadError = errorMessage(error);
      load.fail(error);
    }
  }

  /**
   * Looks for the note's original. Unless `force`, only while it has none: the first save that
   * changes the ink of a note keeps one, and it never changes afterwards.
   */
  async function refreshOriginal(force = false) {
    if (!force && original !== null && original.source !== 'none') return;
    const request = ++originalRequest;
    const gen = generation;
    try {
      const found = await api.noteOriginal(currentPath);
      if (request !== originalRequest || gen !== generation) return;
      original = found;
      originalError = null;
    } catch (error) {
      if (request !== originalRequest || gen !== generation) return;
      original = { source: 'none' };
      originalError = t('editor.original.failed', { message: errorMessage(error) });
    }
  }

  // --- Writes -----------------------------------------------------------------------------

  /** Runs `task` after every write queued before it; `skipped` if another note was opened since. */
  function enqueue<T>(task: () => Promise<T>, skipped: T): Promise<T> {
    const queuedFor = generation;
    pendingWrites += 1;
    const run = writes.then(() => (queuedFor === generation ? task() : skipped));
    const done = () => {
      pendingWrites -= 1;
    };
    writes = run.then(done, done);
    return run;
  }

  /** Resolves once no write is queued or running. */
  async function settled(): Promise<void> {
    while (pendingWrites > 0) await writes;
  }

  function notifySaved(saved: string) {
    if (alive) onsaved?.(saved);
  }

  function writeFailed(error: unknown) {
    const code = typeof error === 'object' && error !== null && 'code' in error ? (error as { code: unknown }).code : null;
    // The file changed or went away: the edits are kept, and can be saved as a copy.
    if (code === 'noteConflict' || code === 'noteNotFound') conflict = errorMessage(error);
    else saveError = errorMessage(error);
  }

  /** A title typed in the field and not renamed yet. */
  function titleEdited(): boolean {
    return !!doc && titleDraft.trim() !== targetTitle;
  }

  function save(): Promise<boolean> {
    if (!doc) return Promise.resolve(false);
    // A title typed but not committed yet (Ctrl+S in the field) is renamed first, unless a
    // recording is under way: the note keeps its name until it is stopped.
    if (!recorder.busy && titleDraft.trim() !== targetTitle) void retitle();
    return enqueue(writeNote, false);
  }

  async function writeNote(): Promise<boolean> {
    const present = history?.present;
    if (!present || !stamp) return false;
    if (present.note === savedNote) return true;
    const gen = generation;
    const target = currentPath;
    saving = true;
    saveError = null;
    conflict = null;
    try {
      const next = await api.saveNote(target, { ...present.note, title }, stamp, present.backgroundPixels?.key);
      if (gen !== generation) return false;
      stamp = next;
      savedNote = present.note;
      notifySaved(target);
      void refreshOriginal();
      return true;
    } catch (error) {
      if (gen === generation) writeFailed(error);
      return false;
    } finally {
      saving = false;
    }
  }

  function retitle(): Promise<void> {
    const next = titleDraft.trim();
    if (!doc || next === targetTitle) {
      titleDraft = targetTitle;
      return Promise.resolve();
    }
    targetTitle = next;
    return enqueue(async () => {
      if (!stamp) return;
      const gen = generation;
      titleError = null;
      try {
        const result = await api.retitleNote(currentPath, next, stamp);
        if (gen !== generation) return;
        currentPath = result.path;
        stamp = result.stamp;
        title = next;
        if (titleDraft.trim() === next) titleDraft = next;
        notifySaved(result.path);
      } catch (error) {
        if (gen !== generation) return;
        titleError = errorMessage(error);
        renameFailures += 1;
        targetTitle = title;
        titleDraft = title;
      }
    }, undefined);
  }

  /** Saves the edits as a new note in the same binder, which the editor then edits. */
  function saveCopy(): Promise<boolean> {
    return enqueue(async () => {
      const present = history?.present;
      if (!present) return false;
      const gen = generation;
      const copyTitle = t('editor.copyTitle', { title: title || t('editor.untitled') });
      saving = true;
      saveError = null;
      try {
        const created = await api.createNote(binderOf(currentPath, app.library), copyTitle);
        // `save_note` keeps the new file's own id; the content is the edited note's.
        const note: Note = { ...present.note, title: copyTitle, created: created.created, origin: { kind: 'ardoise' } };
        // The copy gets the background image too (the same staged pixels).
        let next: FileStamp;
        try {
          next = await api.saveNote(created.path, note, created.stamp, present.backgroundPixels?.key);
        } catch (error) {
          // No blank "Copie de …" note is left behind (it goes to the trash, best effort).
          await api.trashNotes([created.path]).catch(() => {});
          throw error;
        }
        if (gen !== generation) return false;
        currentPath = created.path;
        noteId = created.id;
        stamp = next;
        savedNote = present.note;
        title = copyTitle;
        targetTitle = copyTitle;
        titleDraft = copyTitle;
        conflict = null;
        disk = null;
        notifySaved(created.path);
        // The copy has no original until its own first edit.
        void refreshOriginal(true);
        // The copy has no narration.
        if (replaying) void loadNarration();
        return true;
      } catch (error) {
        if (gen === generation) saveError = errorMessage(error);
        return false;
      } finally {
        saving = false;
      }
    }, false);
  }

  // --- The note on disk -------------------------------------------------------------------

  let checking = false;
  let recheck = false;

  /** After a library change: did the open note's file change or go away under the editor? */
  async function checkDisk() {
    if (checking) {
      recheck = true;
      return;
    }
    checking = true;
    try {
      do {
        recheck = false;
        // Our own writes change the file too: look once they are done.
        await settled();
        if (!alive || !history || !stamp) return;
        const gen = generation;
        const target = currentPath;
        const known = stamp;
        let listing;
        try {
          listing = await api.libraryList();
        } catch {
          return;
        }
        if (!alive) return;
        if (gen !== generation || target !== currentPath || known !== stamp || pendingWrites > 0) {
          recheck = true;
          continue;
        }
        disk = diskChange(listing.notes, target, known);
      } while (recheck);
    } finally {
      checking = false;
    }
  }

  function reload() {
    void open(currentPath);
  }

  // --- Drafts (M10) -----------------------------------------------------------------------

  /** Offers the draft of the note just opened, if one was left (`RecoveryDialog`). */
  async function lookForDraft(target: string, opened: { note: Note; stamp: FileStamp }) {
    const gen = generation;
    const found = await findOffer(api.draftFind, api.draftDiscard, opened.note.id, opened);
    if (!alive || gen !== generation || currentPath !== target) return;
    offer = found;
    // Until the user chooses, nothing written here may replace that draft.
    drafts.hold(found !== null);
  }

  function closeOffer() {
    offer = null;
    recoveryError = null;
    drafts.hold(false);
  }

  /** The draft's note over the opened one, as one undoable step (Ctrl+Z gives the saved version back). */
  async function restoreDraft() {
    const current = offer;
    if (!current || !history || recovering) return;
    recovering = true;
    recoveryError = null;
    const gen = generation;
    try {
      const restored = current.draft.note;
      let pixels = history.present.backgroundPixels;
      const kept = current.draft.hasImage ? await api.draftStageImage(noteId) : null;
      if (gen !== generation || !history) return;
      if (kept) pixels = pixelsOf(kept);
      else if (!sameImage(restored.backgroundImage, history.present.note.backgroundImage)) pixels = undefined;
      const fresh = docFromNote(restored, t('editor.layers.defaultName', { n: 1 }));
      change(restored.backgroundImage && pixels ? { ...fresh, backgroundPixels: pixels } : fresh);
      // That draft now follows the note: replaced by the next one, removed once it is saved.
      drafts.adopt(noteId);
      closeOffer();
    } catch (error) {
      if (gen === generation) recoveryError = errorMessage(error);
    } finally {
      recovering = false;
    }
  }

  function sameImage(a: BackgroundImage | undefined, b: BackgroundImage | undefined): boolean {
    return !!a && !!b && a.name === b.name && a.pdfPage === b.pdfPage;
  }

  /** The draft as a new note beside this one (left untouched), which the editor then opens. */
  async function restoreDraftAsCopy() {
    const current = offer;
    if (!current || recovering) return;
    recovering = true;
    recoveryError = null;
    const gen = generation;
    try {
      const name = current.draft.title.trim() || title.trim() || t('editor.untitled');
      const created = await api.draftRestoreCopy(noteId, t('drafts.copyTitle', { title: name }));
      if (gen !== generation || !alive) return;
      closeOffer();
      const opening = open(created.path);
      notifySaved(created.path);
      await opening;
      showNotice(t('drafts.restoredCopy', { title: created.title }));
    } catch (error) {
      if (gen === generation) recoveryError = errorMessage(error);
    } finally {
      recovering = false;
    }
  }

  async function discardDraft() {
    if (!offer || recovering) return;
    closeOffer();
    await drafts.discard(noteId);
  }

  // --- Leaving ----------------------------------------------------------------------------

  /** Set while leaving waits for the queued writes. */
  let closeAfterWrites = false;

  /**
   * Back to the library, once the queued writes are done (a "save as a copy" still under way
   * needs the background image's pixels, which the editor releases when it goes away).
   */
  function close() {
    if (pendingWrites > 0) {
      if (closeAfterWrites) return;
      closeAfterWrites = true;
      const failures = renameFailures;
      void settled().then(() => {
        closeAfterWrites = false;
        // A rename that failed meanwhile (the title field's blur): its error is shown, not lost.
        if (alive && renameFailures === failures) close();
      });
      return;
    }
    alive = false;
    onclose();
  }

  function back() {
    if (recorder.busy) return showNotice(t('editor.replay.recordingBusy'), true);
    if (videoRunning) return showNotice(t('videoExport.closeBusy'), true);
    if (dirty || titleEdited()) leaving = true;
    else close();
  }

  /** The window is asked to close: true holds it back (unsaved changes, or a write under way). */
  function holdClose(): boolean {
    if (alive && recorder.busy) {
      // A narration being recorded would be lost: the window stays until it is stopped.
      showNotice(t('editor.replay.recordingBusy'), true);
      return true;
    }
    if (alive && videoRunning) {
      // A video being written would be cut short: the window stays until it is done or cancelled.
      showNotice(t('videoExport.closeBusy'), true);
      return true;
    }
    if (!alive || (!dirty && !titleEdited() && pendingWrites === 0)) return false;
    if (windowClose.destroy) {
      if (dirty || titleEdited()) {
        closing = true;
        leaving = true;
      } else {
        void closeWindow();
      }
    }
    return true;
  }

  async function closeWindow() {
    closing = false;
    leaving = false;
    await settled();
    alive = false;
    await windowClose.destroy?.();
  }

  async function saveAndLeave() {
    const fromWindow = closing;
    const target = switching;
    const failures = renameFailures;
    const ok = await save();
    await settled();
    leaving = false;
    closing = false;
    switching = null;
    // Nothing leaves while the save or the rename failed: their error stays in view.
    if (!ok || renameFailures !== failures) return;
    if (target !== null) void open(target);
    else if (fromWindow) await closeWindow();
    else close();
  }

  function discardAndLeave() {
    void drafts.discard(noteId);
    const target = switching;
    switching = null;
    if (target !== null) {
      leaving = false;
      void open(target);
    } else if (closing) void closeWindow();
    else {
      leaving = false;
      close();
    }
  }

  function stay() {
    const wanted = switching;
    leaving = false;
    closing = false;
    switching = null;
    // The library screen asked for another note: it goes back to following this one.
    if (wanted !== null) notifySaved(currentPath);
  }

  // --- Tools and style -------------------------------------------------------------------

  function showNotice(text: string, isError = false) {
    notice = text;
    noticeError = isError;
    clearTimeout(noticeTimer);
    noticeTimer = setTimeout(() => (notice = null), NOTICE_MS);
  }

  function hiddenLayerNotice() {
    showNotice(t('editor.layers.hiddenActive'));
  }

  function setMode(mode: Mode) {
    tools = { ...tools, mode };
  }

  function setTool(tool: Tool) {
    widths[tools.tool] = tools.width;
    tools = { ...tools, tool, width: widths[tool] };
    if (doc && hasSelection) change(applyStyle(doc, { tool }));
  }

  function setColor(color: Rgba) {
    tools = { ...tools, color };
    if (doc && hasSelection) change(applyStyle(doc, { color }));
  }

  function addCustomColor(color: Rgba) {
    customColors = rememberCustomColor(color, customColors);
    setColor({ ...color, a: 255 });
  }

  function setWidth(width: number, done: boolean) {
    tools = { ...tools, width };
    if (done && doc && hasSelection) change(applyStyle(doc, { width }));
  }

  function setEraser(next: { mode?: EraserMode; radius?: number }) {
    tools = { ...tools, eraser: next.mode ?? tools.eraser, eraserRadius: next.radius ?? tools.eraserRadius };
  }

  function applyAll(what: 'color' | 'tool' | 'width') {
    if (!doc) return;
    const style = what === 'color' ? { color: tools.color } : what === 'tool' ? { tool: tools.tool } : { width: tools.width };
    change(applyStyle(doc, style, true));
  }

  /** An undoable change. */
  function change(next: Doc) {
    if (history) history = commit(history, next);
  }

  /** A change of selection or active layer: not an undo step. */
  function reselect(next: Doc) {
    if (history) history = replacePresent(history, next);
  }

  function doUndo() {
    if (history && !split?.busy) history = undo(history);
  }

  function doRedo() {
    if (history && !split?.busy) history = redo(history);
  }

  /** How long a clipboard key waits for the browser's event before using the clipboard API, milliseconds. */
  const CLIPBOARD_EVENT_WAIT_MS = 60;

  // --- Clipboard (M8): strokes travel as marked JSON text on the system clipboard, so that a
  // paste works in another note, window or session (`lib/editor/clipboard.ts`). The keys go
  // through the browser's copy / cut / paste events (no permission to ask); the buttons of the
  // selection panel use the asynchronous clipboard API.

  /** Whether the editing keys are free to act now (no dialog, split panel or replay). */
  function canEditNow(target: EventTarget | null): boolean {
    return !!doc && !leaving && !offer && !exporting && !video && !split && !replaying && !isTyping(target);
  }

  /** Pastes strokes from the clipboard as one undoable step (the copies are then selected). */
  function pasteDecoded(strokes: Stroke[]) {
    if (!doc) return;
    if (!canDraw(doc)) return hiddenLayerNotice();
    change(pasteStrokes(doc, strokes));
  }

  /**
   * Whether a copy, cut or paste event came since the last clipboard key. Chromium raises them
   * for the key by itself; if one does not (nothing for it to copy on the page), the key falls
   * back on the asynchronous clipboard API a moment later.
   */
  let clipboardEventSeen = false;

  function expectClipboardEvent(kind: 'copy' | 'cut' | 'paste') {
    clipboardEventSeen = false;
    setTimeout(() => {
      if (clipboardEventSeen || !doc) return;
      if (kind === 'paste') void pasteFromClipboard(true);
      else if (hasSelection) void copyToClipboard(kind === 'cut');
    }, CLIPBOARD_EVENT_WAIT_MS);
  }

  function clipboardEvent(e: ClipboardEvent, cut: boolean) {
    clipboardEventSeen = true;
    if (!canEditNow(e.target) || !doc || !hasSelection) return;
    e.preventDefault();
    if (canvas?.dragging()) return;
    e.clipboardData?.setData('text/plain', encodeStrokes(selectedStrokes(doc)));
    if (cut) change(deleteSelection(doc));
  }

  function onpaste(e: ClipboardEvent) {
    clipboardEventSeen = true;
    if (!canEditNow(e.target)) return;
    const strokes = decodeStrokes(e.clipboardData?.getData('text/plain') ?? '');
    // Foreign content (text, an image…) is left alone.
    if (!strokes) return;
    e.preventDefault();
    if (!canvas?.dragging()) pasteDecoded(strokes);
  }

  /** The selection panel's Copy and Cut buttons. */
  async function copyToClipboard(cut: boolean) {
    const source = doc;
    if (!source || !hasSelection) return;
    try {
      await navigator.clipboard.writeText(encodeStrokes(selectedStrokes(source)));
    } catch {
      return showNotice(t('editor.clipboard.failed'), true);
    }
    // Cut deletes what was copied, unless the document changed while the clipboard was written.
    if (cut && doc === source) change(deleteSelection(source));
  }

  /** The selection panel's Paste button (or the Ctrl+V key, `quiet`: without a word when the clipboard is not ours). */
  async function pasteFromClipboard(quiet = false) {
    let text = '';
    try {
      text = await navigator.clipboard.readText();
    } catch {
      return showNotice(t('editor.clipboard.failed'), true);
    }
    const strokes = decodeStrokes(text);
    if (strokes) pasteDecoded(strokes);
    else if (!quiet) showNotice(t('editor.clipboard.nothing'));
  }

  // --- Page format (M8) ------------------------------------------------------------------

  /** Tells the user when ink fell out of the page by a change of format (it is kept, not drawn). */
  function inkOutsideNotice(before: Doc, after: Doc) {
    const outside = strokesOutsidePage(after.note) - strokesOutsidePage(before.note);
    if (outside > 0) showNotice(outside === 1 ? t('editor.page.inkOutsideOne') : t('editor.page.inkOutsideMany', { count: outside }));
  }

  function changeFormat(next: Doc) {
    if (!doc || next === doc) return;
    const before = doc;
    change(next);
    inkOutsideNotice(before, next);
  }

  function duplicate() {
    if (!doc) return;
    if (!canDraw(doc)) return hiddenLayerNotice();
    change(duplicateSelection(doc));
  }

  /** Puts a background image, switching the note's orientation first when the user asked to (one undoable step). */
  function setImage(image: BackgroundImage, pixels: PixelsRef, orientation?: Orientation) {
    if (!doc) return;
    const base = orientation ? setOrientation(doc, orientation) : doc;
    const next = setBackgroundImage(base, image, pixels);
    changeFormat(next);
  }

  /** "Redresser les formes": one undoable step, or a notice saying why nothing changed. */
  function straighten() {
    if (!doc || !ink || split || replaying) return;
    if (!hasSelection) return showNotice(t('editor.straighten.disabledHint'));
    const { doc: next, shapes } = straightenSelection(doc, ink);
    if (shapes === 0) return showNotice(t('editor.straighten.none'));
    change(next);
    showNotice(shapes === 1 ? t('editor.straighten.doneOne') : t('editor.straighten.doneMany', { count: shapes }));
  }

  /** "Revenir à l'original", confirmed: the original's ink, as one undoable step. */
  function revertOriginal() {
    if (!doc || !original || original.source === 'none' || split || replaying) return;
    const next = revertToOriginal(doc, original);
    if (next === doc) return;
    change(next);
    showNotice(t('editor.original.done'));
  }

  // --- Export, copy, split ----------------------------------------------------------------

  /** The note as the user sees it, with the title shown in the title field. */
  function currentSource(): ExportSource | null {
    return doc ? { path: currentPath, note: { ...doc.note, title }, imageKey: doc.backgroundPixels?.key } : null;
  }

  function openExport() {
    const source = currentSource();
    if (source) exporting = [source];
  }

  function openPrint() {
    const source = currentSource();
    if (source) printing = [source];
  }

  function closeVideo() {
    video = null;
    // A dialog that goes away takes its export with it (or ends it): the window is free again.
    videoRunning = false;
  }

  function openVideo() {
    const source = currentSource();
    if (source) video = source;
  }

  async function copyImage() {
    const source = currentSource();
    if (!source || copying) return;
    copying = true;
    try {
      await api.copyNoteImage(source, copyDpi(source));
      showNotice(t('editor.copyImage.done'));
    } catch (error) {
      showNotice(t('editor.copyImage.failed', { message: errorMessage(error) }), true);
    } finally {
      copying = false;
    }
  }

  function defaultSplitTitle(mode: SplitMode): string {
    const base = title.trim() || t('editor.untitled');
    return t(mode === 'time' ? 'editor.split.titleTime' : 'editor.split.titleZone', { title: base });
  }

  /** The split button: opens the panel, or closes it. */
  function toggleSplit() {
    if (!doc) return;
    if (split) return cancelSplit();
    if (hasSelection) reselect(clearSelection(doc));
    const span = timeRange(doc.note);
    split = {
      mode: 'time',
      at: span ? Math.round((span.min + span.max) / 2) : 0,
      zone: null,
      title: defaultSplitTitle('time'),
      titleEdited: false,
      busy: false,
      error: null,
    };
  }

  function cancelSplit() {
    if (split && !split.busy) split = null;
  }

  function setSplitMode(mode: SplitMode) {
    if (!split || split.busy) return;
    split.mode = mode;
    split.error = null;
    if (!split.titleEdited) split.title = defaultSplitTitle(mode);
  }

  /**
   * Splits the note: the new note is created in the same binder and the kept part is saved over
   * this one (unsaved changes included). The editor then shows the kept part, saved, with a fresh
   * undo history.
   */
  function confirmSplit() {
    const state = split;
    const spec = splitSpec;
    if (!state || !spec || state.busy || !canSplit(preview)) return;
    state.busy = true;
    state.error = null;
    void enqueue(async () => {
      const present = history?.present;
      if (!present || !stamp) return;
      const gen = generation;
      const target = currentPath;
      saving = true;
      try {
        const result = await api.splitNote(
          target,
          { ...present.note, title },
          stamp,
          spec,
          state.title.trim(),
          present.backgroundPixels?.key,
        );
        if (gen !== generation) return;
        const fresh = docFromNote(result.note, t('editor.layers.defaultName', { n: 1 }));
        // The kept part has the same background image, with the same pixels.
        const pixels = result.note.backgroundImage ? present.backgroundPixels : undefined;
        const start = pixels ? { ...fresh, backgroundPixels: pixels } : fresh;
        history = createHistory(start);
        savedNote = start.note;
        stamp = result.stamp;
        conflict = null;
        saveError = null;
        disk = null;
        split = null;
        showNotice(t('editor.split.done', { title: result.created.title }));
        notifySaved(target);
        void refreshOriginal();
      } catch (error) {
        if (gen === generation) state.error = errorMessage(error);
      } finally {
        saving = false;
        state.busy = false;
      }
    }, undefined).then(() => {
      // Skipped (another note was opened meanwhile): nothing is left to wait for.
      state.busy = false;
    });
  }

  // --- Replay and narration ---------------------------------------------------------------

  function enterReplay() {
    if (!doc || split || replaying || canvas?.dragging()) return;
    player.load(doc.note);
    narration = null;
    narrationError = null;
    replaying = true;
    void loadNarration();
  }

  function leaveReplay() {
    // Leaving would drop a narration being recorded: stop or cancel it first.
    if (!replaying || recorder.busy) return;
    resetReplay();
  }

  function toggleReplay() {
    if (replaying) leaveReplay();
    else enterReplay();
  }

  /** Back to editing (also when another note is opened): the replay, its sound and a recording are dropped. */
  function resetReplay() {
    player.pause();
    replaying = false;
    narration = null;
    narrationError = null;
    narrationLoading = false;
    recordOpen = false;
    confirmDelete = false;
    dropNarrationSound();
  }

  /**
   * The editor lets go of a recording nobody stopped (it is closed, or opens another note): it is
   * saved into the note it was made for. If that fails Rust keeps it, pending, with a backup.
   */
  function salvageRecording() {
    if (recorder.phase !== 'recording') return;
    player.pause();
    void recorder.stop(true, stamp, currentPath, replayStartedAt);
  }

  /** Reads the narration stored in the note (what the bar and the side panel show). */
  async function loadNarration() {
    const gen = generation;
    try {
      const info = await api.narrationInfo(currentPath);
      if (gen !== generation || !replaying) return;
      narration = info;
      narrationError = null;
      if (!info) dropNarrationSound();
    } catch (error) {
      if (gen !== generation || !replaying) return;
      narration = null;
      narrationError = t('replay.narrationFailed', { message: errorMessage(error) });
      dropNarrationSound();
    }
  }

  /** The "Lire avec la narration" switch: fetches the sound and takes the options it was recorded with. */
  async function playAlong(on: boolean) {
    const info = narration;
    if (!on || !info) return dropNarrationSound();
    const gen = generation;
    narrationLoading = true;
    narrationError = null;
    try {
      const sound = await api.narrationAudio(currentPath);
      if (gen !== generation || !replaying || narration !== info) return;
      const url = URL.createObjectURL(new Blob([sound], { type: narrationType(sound) }));
      dropNarrationSound();
      narrationUrl = url;
      player.setOptions({ speed: info.speed, maxPauseMs: info.maxPauseMs });
      // A sound that begins before instant 0 (live recordings do) gets a lead-in on the timeline.
      player.setNarrationSpan(Math.max(0, -info.startMs), info.startMs + info.durationMs);
    } catch (error) {
      if (gen === generation) narrationError = t('replay.narrationFailed', { message: errorMessage(error) });
    } finally {
      if (gen === generation) narrationLoading = false;
    }
  }

  /** The sound is no longer played along: the replay is as long as the drawing again. */
  function dropNarrationSound() {
    if (narrationUrl !== null) {
      try {
        URL.revokeObjectURL(narrationUrl);
      } catch {
        // Nothing to release.
      }
      narrationUrl = null;
    }
    player.setNarrationSpan(0, 0);
  }

  async function chooseMicrophone(device: string | null) {
    chosenMicrophone = device;
    try {
      const next = await api.setMicrophone(device);
      chosenMicrophone = next.microphone;
      onchange?.(next);
    } catch (error) {
      chosenMicrophone = undefined;
      showNotice(errorMessage(error), true);
    }
  }

  /** « Démarrer »: the replay plays from the start, with the options it has now, while the microphone records. */
  async function startRecording() {
    if (!replaying || recorder.busy) return;
    dropNarrationSound();
    player.pause();
    const { speed, maxPauseMs } = player.options;
    const started = await recorder.start(microphone, { kind: 'replay', path: currentPath, speed, maxPauseMs, startMs: 0 });
    if (!started) return;
    player.seek(0);
    player.play();
    // The replay is at instant 0 and playing from now on (a position other than 0 is allowed for).
    replayStartedAt = Date.now() - player.position;
  }

  /** Stops the recording; saved, it goes into the note's file and the note's stamp changes. */
  async function stopRecording(save: boolean) {
    if (recorder.phase !== 'recording') return;
    // The replay ends with the recording. Closing the panel now, while it is still busy, keeps its
    // microphone picker from listening again in between.
    player.pause();
    if (save) recordOpen = false;
    const result = await enqueue(
      async () => {
        // The note's current path (it may have been renamed) and stamp, and the replay's time origin.
        const outcome = await recorder.stop(
          save,
          save ? stamp : null,
          save ? currentPath : null,
          save ? replayStartedAt : null,
        );
        if (outcome.saved?.stamp) stamp = outcome.saved.stamp;
        return outcome;
      },
      { ok: false, saved: null },
    );
    if (!result.ok || !result.saved) return;
    showNotice(t('narration.record.saved', { duration: clock(result.saved.durationMs) }));
    await loadNarration();
  }

  /** « Réessayer »: the kept recording goes into the note as it is now (its current path and stamp). */
  async function retryRecording() {
    const saved = await enqueue(async () => {
      const result = await recorder.retry(currentPath, stamp);
      if (result?.stamp) stamp = result.stamp;
      return result;
    }, null);
    if (!saved) return;
    recordOpen = false;
    showNotice(t('narration.record.saved', { duration: clock(saved.durationMs) }));
    await loadNarration();
  }

  async function deleteNarration() {
    confirmDelete = false;
    await enqueue(async () => {
      if (!stamp) return;
      try {
        stamp = await api.deleteNarration(currentPath, stamp);
        dropNarrationSound();
        showNotice(t('narration.stored.deleted'));
      } catch (error) {
        narrationError = t('narration.stored.deleteFailed', { message: errorMessage(error) });
      }
    }, undefined);
    await loadNarration();
  }

  // --- Keyboard ---------------------------------------------------------------------------

  function isTyping(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    if (target instanceof HTMLInputElement) {
      return !['checkbox', 'radio', 'range', 'color', 'button'].includes(target.type);
    }
    return target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || target.isContentEditable;
  }

  /** Space presses buttons, toggles checkboxes and radios…: it pans only elsewhere. */
  function spaceActivates(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    if (
      target instanceof HTMLButtonElement ||
      target instanceof HTMLInputElement ||
      target instanceof HTMLSelectElement ||
      target instanceof HTMLTextAreaElement ||
      target instanceof HTMLAnchorElement ||
      target.isContentEditable
    ) {
      return true;
    }
    const role = target.getAttribute('role') ?? '';
    return ['button', 'checkbox', 'radio', 'switch', 'link', 'tab', 'option', 'menuitem', 'menuitemcheckbox', 'menuitemradio'].includes(role);
  }

  /** In replay mode Space also works on the timeline (a range input); buttons and fields keep it. */
  function spacePlays(target: EventTarget | null): boolean {
    if (target instanceof HTMLInputElement && target.type === 'range') return true;
    return !spaceActivates(target);
  }

  /** Ctrl + 2 (the page as wide as the editor), whatever the layout. */
  function isFitWidthKey(e: KeyboardEvent): boolean {
    return e.key === '2' || e.code === 'Digit2' || e.code === 'Numpad2';
  }

  /** Ctrl + 0 (the whole page), whatever the layout: 0 is typed with Shift on a French keyboard. */
  function isFitKey(e: KeyboardEvent): boolean {
    return e.key === '0' || e.code === 'Digit0' || e.code === 'Numpad0';
  }

  /** The keys of replay mode: R and Escape leave it, Space plays or pauses; nothing edits the note. */
  function replayKey(e: KeyboardEvent, key: string, mod: boolean) {
    if (mod) {
      if (isFitKey(e)) {
        e.preventDefault();
        canvas?.fit();
      } else if (isFitWidthKey(e)) {
        e.preventDefault();
        canvas?.fitWidth();
      }
      return;
    }
    if (e.altKey) return;
    switch (key) {
      case 'r':
      case 'Escape':
        e.preventDefault();
        leaveReplay();
        break;
      case ' ':
        if (spacePlays(e.target)) {
          e.preventDefault();
          // The replay of a recording runs on its own.
          if (!recorder.busy) player.toggle();
        }
        break;
    }
  }

  function onkeydown(e: KeyboardEvent) {
    if (!doc) return;
    const mod = e.ctrlKey || e.metaKey;
    const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (mod && key === 's') {
      e.preventDefault();
      if (dirty) void save();
      return;
    }
    if (leaving) {
      if (key === 'Escape') stay();
      return;
    }
    // A dialog or the split panel is open: the editing keys wait; Escape closes the panel.
    if (exporting || printing || video) return;
    if (split) {
      if (key === 'Escape') {
        e.preventDefault();
        cancelSplit();
      }
      return;
    }
    if (isTyping(e.target)) return;
    // A gesture under way commits on top of the doc it started from: changing the doc meanwhile
    // (undo, redo, delete, paste, select all…) would be overwritten, so those keys wait.
    if (canvas?.dragging() && (mod || ['Delete', 'Backspace', 'Escape'].includes(key))) {
      e.preventDefault();
      return;
    }
    if (replaying) {
      replayKey(e, key, mod);
      return;
    }
    // Copy, cut and paste: the browser's clipboard events do the work (see `onpaste`).
    if (mod && !e.shiftKey && !e.altKey && (key === 'c' || key === 'x' || key === 'v')) {
      expectClipboardEvent(key === 'c' ? 'copy' : key === 'x' ? 'cut' : 'paste');
      return;
    }
    if (mod) {
      const action: Record<string, (() => void) | undefined> = {
        z: e.shiftKey ? doRedo : doUndo,
        y: doRedo,
        a: () => reselect(selectAll(doc!)),
        '0': () => canvas?.fit(),
        '2': () => canvas?.fitWidth(),
      };
      // Ctrl + 0 and Ctrl + 2 are typed with Shift on some layouts (French): the key code says it too.
      const run = action[isFitKey(e) ? '0' : isFitWidthKey(e) ? '2' : key];
      if (run) {
        e.preventDefault();
        run();
      }
      return;
    }
    if (e.altKey) return;
    switch (key) {
      case 'Delete':
      case 'Backspace':
        if (hasSelection) {
          e.preventDefault();
          change(deleteSelection(doc));
        }
        break;
      case 'Escape':
        reselect(clearSelection(doc));
        break;
      case ' ':
        // Space on a focused control (button, checkbox, radio…) works it; elsewhere it pans
        // while held.
        if (!spaceActivates(e.target)) {
          e.preventDefault();
          spaceDown = true;
        }
        break;
      case 'v':
        setMode('select');
        break;
      case 'p':
        setMode('pen');
        break;
      case 'e':
        setMode('eraser');
        break;
      case 'h':
        setMode('pan');
        break;
      case 'r':
        enterReplay();
        break;
      case 'f':
        straighten();
        break;
    }
  }

  function onkeyup(e: KeyboardEvent) {
    if (e.key === ' ') spaceDown = false;
  }
</script>

<svelte:window
  {onkeydown}
  {onkeyup}
  {onpaste}
  oncopy={(e) => clipboardEvent(e, false)}
  oncut={(e) => clipboardEvent(e, true)}
  onblur={() => (spaceDown = false)} />

<section class="editor" data-path={currentPath} data-library={app.library} data-dirty={dirty ? 'yes' : 'no'}>
  <header class="editor-top">
    <button type="button" class="button ghost" onclick={back}>
      <span aria-hidden="true">←</span>
      {t('editor.back')}
    </button>
    <label class="title-field">
      <span class="visually-hidden">{t('editor.title')}</span>
      <input
        type="text"
        bind:value={titleDraft}
        placeholder={t('editor.untitled')}
        disabled={!doc || recorder.busy}
        onblur={retitle}
        onkeydown={(e) => {
          if (e.key === 'Enter') e.currentTarget.blur();
          if (e.key === 'Escape') {
            titleDraft = title;
            e.currentTarget.blur();
          }
        }}
      />
    </label>
    <span class="status" class:unsaved={dirty} role="status">
      {#if saving}
        {t('editor.saving')}
      {:else if doc}
        {dirty ? t('editor.unsaved') : t('editor.saved')}
      {/if}
    </span>
    <div class="top-actions">
      <button
        type="button"
        class="button ghost"
        disabled={!history || !canUndo(history) || replaying}
        title={t('editor.undo')}
        aria-label={t('editor.undo')}
        onclick={doUndo}><span aria-hidden="true">↶</span></button
      >
      <button
        type="button"
        class="button ghost"
        disabled={!history || !canRedo(history) || replaying}
        title={t('editor.redo')}
        aria-label={t('editor.redo')}
        onclick={doRedo}><span aria-hidden="true">↷</span></button
      >
      <button type="button" class="button primary" disabled={!dirty || saving} onclick={save} title={t('editor.saveShortcut')}>
        {t('editor.save')}
      </button>
    </div>
  </header>

  {#if conflict}
    <div class="banner error editor-banner" role="alert">
      <strong>{t('editor.conflictTitle')}</strong>
      <p>{conflict}</p>
      <p>{t('editor.conflictHint')}</p>
      <button type="button" class="button small-button primary" disabled={saving || recorder.busy} onclick={saveCopy}>{t('editor.saveCopy')}</button>
      <button type="button" class="button small-button" onclick={() => (conflict = null)}>{t('editor.dismiss')}</button>
    </div>
  {:else if disk}
    <div class="banner editor-banner" role="status" data-disk={disk}>
      <strong>{disk === 'missing' ? t('editor.disk.missingTitle') : t('editor.disk.changedTitle')}</strong>
      <p>{disk === 'missing' ? t('editor.disk.missing') : dirty ? t('editor.disk.changedDirty') : t('editor.disk.changedClean')}</p>
      {#if disk === 'changed' && !dirty}
        <button type="button" class="button small-button primary" disabled={recorder.busy} onclick={reload}>{t('editor.disk.reload')}</button>
      {:else}
        <button type="button" class="button small-button primary" disabled={saving || recorder.busy} onclick={saveCopy}>{t('editor.saveCopy')}</button>
      {/if}
      <button type="button" class="button small-button" onclick={() => (disk = null)}>{t('editor.dismiss')}</button>
    </div>
  {/if}
  {#if saveError}
    <div class="banner error editor-banner" role="alert">
      <p>{t('editor.saveError', { message: saveError })}</p>
      <button type="button" class="button small-button" onclick={() => (saveError = null)}>{t('editor.dismiss')}</button>
    </div>
  {/if}
  {#if notice}
    <p class="banner editor-banner editor-notice" class:error={noticeError} role={noticeError ? 'alert' : 'status'}>{notice}</p>
  {/if}
  {#if titleError}
    <div class="banner error editor-banner" role="alert">
      <p>{t('editor.retitleError', { message: titleError })}</p>
      <button type="button" class="button small-button" onclick={() => (titleError = null)}>{t('editor.dismiss')}</button>
    </div>
  {/if}

  {#if loadError}
    <p class="banner error" role="alert">{t('editor.loadError', { message: loadError })}</p>
  {:else if !doc || !ink}
    <p class="loading">{t('editor.loading')}</p>
  {:else}
    <Toolbar
      {tools}
      {customColors}
      {hasSelection}
      onmode={setMode}
      ontool={setTool}
      oncolor={setColor}
      oncustomcolor={addCustomColor}
      onwidth={setWidth}
      oneraser={setEraser}
      onapplyall={applyAll}
      splitting={split !== null}
      locked={split !== null || replaying}
      {replaying}
      replayDisabled={split !== null || recorder.busy}
      {copying}
      onsplit={toggleSplit}
      oncopy={copyImage}
      onexport={openExport}
      onprint={openPrint}
      onreplay={toggleReplay}
      onvideo={openVideo}
      onstraighten={straighten}
    />
    {#if replaying}
      <ReplayBar
        {player}
        {narration}
        audioUrl={narrationUrl}
        loading={narrationLoading}
        locked={recorder.busy}
        error={narrationError}
        onnarration={playAlong}
      />
    {/if}
    <div class="editor-main">
      <Canvas
        bind:this={canvas}
        doc={replayDoc ?? doc}
        tools={replaying ? replayTools : tools}
        {ink}
        spacePan={spaceDown}
        readonly={replaying}
        onchange={change}
        onselect={reselect}
        oncropped={() => setMode('select')}
        onblocked={hiddenLayerNotice}
        split={split ? { preview, tracing: split.mode === 'zone' && !split.busy, zone: split.mode === 'zone' ? split.zone : null } : null}
        onzone={(polygon) => {
          if (split && split.mode === 'zone') {
            split.zone = polygon;
            split.error = null;
          }
        }}
      />
      <aside class="editor-side" aria-label={t('editor.sidePanel')}>
        {#if replaying}
          <section class="side-section" aria-labelledby="editor-narration-heading" data-section="narration">
            <h3 id="editor-narration-heading">{t('narration.title')}</h3>
            {#if narration}
              <p class="small" data-slot="narration-stored">
                {t('narration.stored.info', { duration: clock(narration.durationMs) })}
              </p>
            {:else}
              <p class="small muted">{t('narration.stored.none')}</p>
            {/if}
            <RecordPanel
              bind:open={recordOpen}
              {recorder}
              device={microphone}
              ondevice={chooseMicrophone}
              onstart={startRecording}
              onstop={() => void stopRecording(true)}
              oncancel={() => void stopRecording(false)}
              onretry={() => void retryRecording()}
              openLabel={t('narration.record.open')}
              openHint={t('narration.record.openHint')}
              replaces={narration !== null}
              info={recordInfo}
              saveNote={t('narration.record.saveNote')}
              warning={dirty ? t('narration.record.unsavedWarning') : null}
              duringInfo={player.playing ? null : t('narration.record.afterEnd')}
            />
            {#if narration && !recorder.busy}
              {#if confirmDelete}
                <div class="confirm-delete" role="alert">
                  <p class="small">{t('narration.stored.deleteConfirm')}</p>
                  <div class="buttons">
                    <button type="button" class="button small-button danger" data-action="narration-delete-yes" onclick={deleteNarration}>
                      {t('narration.stored.deleteYes')}
                    </button>
                    <button type="button" class="button small-button" onclick={() => (confirmDelete = false)}>
                      {t('narration.stored.deleteNo')}
                    </button>
                  </div>
                </div>
              {:else}
                <button type="button" class="button small-button danger" data-action="narration-delete" onclick={() => (confirmDelete = true)}>
                  {t('narration.stored.delete')}
                </button>
              {/if}
            {/if}
          </section>
        {:else}
          {#if split}
            <SplitPanel
              mode={split.mode}
              {range}
              at={split.at}
              hasZone={split.zone !== null}
              {preview}
              bind:title={split.title}
              dirty={dirty}
              busy={split.busy}
              error={split.error}
              onmode={setSplitMode}
              onat={(value) => {
                if (split) split.at = value;
              }}
              onedittitle={() => {
                if (split) split.titleEdited = true;
              }}
              onclearzone={() => {
                if (split) split.zone = null;
              }}
              onconfirm={confirmSplit}
              oncancel={cancelSplit}
            />
          {/if}
          {#if hasSelection}
            <section class="side-section" aria-labelledby="editor-selection-heading">
              <h3 id="editor-selection-heading">{t('editor.selection.label')}</h3>
              <p class="small muted">{t('editor.selection.count', { count: doc.selection.length })}</p>
              <div class="buttons">
                <button type="button" class="button small-button" data-action="copy-selection" onclick={() => void copyToClipboard(false)}>
                  {t('editor.selection.copy')}
                </button>
                <button type="button" class="button small-button" data-action="cut-selection" onclick={() => void copyToClipboard(true)}>
                  {t('editor.selection.cut')}
                </button>
                <button type="button" class="button small-button" data-action="paste-selection" onclick={() => void pasteFromClipboard()}>
                  {t('editor.selection.paste')}
                </button>
                <button type="button" class="button small-button" onclick={duplicate}>
                  {t('editor.selection.duplicate')}
                </button>
                <button
                  type="button"
                  class="button small-button"
                  data-action="straighten-selection"
                  title={t('editor.straighten.hint')}
                  onclick={straighten}
                >
                  {t('editor.straighten.button')}
                </button>
                <button type="button" class="button small-button danger" onclick={() => change(deleteSelection(doc!))}>
                  {t('editor.selection.delete')}
                </button>
                <button type="button" class="button small-button" onclick={() => reselect(clearSelection(doc!))}>
                  {t('editor.selection.clear')}
                </button>
              </div>
            </section>
          {/if}
          <PagePanel
            background={doc.note.background}
            page={doc.note.page}
            cropping={tools.mode === 'crop'}
            disabled={split !== null}
            onbackground={(b) => change(setBackground(doc!, b))}
            onorientation={(orientation, scaleInk) => changeFormat(setOrientation(doc!, orientation, scaleInk))}
            onresize={(size, scaleInk) => changeFormat(resizePage(doc!, size, scaleInk))}
            onrotate={(direction) => change(rotate(doc!, direction))}
            onautocrop={() => change(autoCrop(doc!))}
            onmanualcrop={() => setMode(tools.mode === 'crop' ? 'select' : 'crop')}
          >
            <BackgroundImageSection
              page={doc.note.page}
              image={doc.note.backgroundImage}
              pixels={doc.backgroundPixels}
              disabled={split !== null}
              onset={(image, pixels, orientation) => setImage(image, pixels, orientation)}
              onturn={(direction) => change(rotateBackgroundImage(doc!, direction))}
              onfit={(fit) => change(setImageFit(doc!, fit))}
              onopacity={(opacity) => change(setImageOpacity(doc!, opacity))}
              onremove={() => change(removeBackgroundImage(doc!))}
            />
          </PagePanel>
          {#key currentPath}
            <OriginalPanel {original} error={originalError} disabled={split !== null} onrevert={revertOriginal} />
          {/key}
          <LayersPanel
            layers={doc.note.layers}
            active={doc.activeLayer}
            disabled={split !== null}
            onactive={(i) => reselect(setActiveLayer(doc!, i))}
            onvisible={(i, visible) => change(setLayerVisible(doc!, i, visible))}
            onrename={(i, name) => change(renameLayer(doc!, i, name))}
            onmove={(from, to) => change(moveLayer(doc!, from, to))}
            onmerge={(i) => change(mergeDown(doc!, i))}
            onadd={() => change(addLayer(doc!, t('editor.layers.defaultName', { n: doc!.note.layers.length + 1 })))}
          />
        {/if}
      </aside>
    </div>
  {/if}

  {#if exporting}
    <ExportDialog
      sources={exporting}
      subject={t('export.subjectNote', { title: title.trim() || t('editor.untitled') })}
      onclose={() => (exporting = null)}
    />
  {/if}

  {#if printing}
    <PrintDialog
      sources={printing}
      subject={t('export.subjectNote', { title: title.trim() || t('editor.untitled') })}
      onclose={() => (printing = null)}
    />
  {/if}

  {#if video}
    <VideoExportDialog
      source={video}
      subject={title.trim() || t('editor.untitled')}
      onclose={closeVideo}
      onrunning={(running) => (videoRunning = running)}
    />
  {/if}

  {#if leaving}
    <Modal title={t('editor.leave.title')} onclose={stay} role="alertdialog">
      <p>{t('editor.leave.message')}</p>
      <div class="actions">
        <button type="button" class="button primary" data-autofocus onclick={saveAndLeave} disabled={saving}>
          {switching !== null ? t('editor.leave.saveSwitch') : closing ? t('editor.leave.saveClose') : t('editor.leave.save')}
        </button>
        <button type="button" class="button danger" onclick={discardAndLeave}>
          {switching !== null ? t('editor.leave.discardSwitch') : closing ? t('editor.leave.discardClose') : t('editor.leave.discard')}
        </button>
        <button type="button" class="button" onclick={stay}>{t('editor.leave.cancel')}</button>
      </div>
    </Modal>
  {/if}

  {#if offer}
    <RecoveryDialog
      {offer}
      busy={recovering}
      error={recoveryError}
      onrestore={restoreDraft}
      oncopy={restoreDraftAsCopy}
      ondiscard={discardDraft}
      onlater={back}
    />
  {/if}
</section>
