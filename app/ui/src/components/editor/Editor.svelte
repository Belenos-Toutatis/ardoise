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
   * through one queue, so that each uses the path and stamp left by the previous one.
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
  import { api, errorMessage, onLibraryChanged, type AppState } from '../../lib/api';
  import { loadCustomColors, rememberCustomColor } from '../../lib/editor/colors';
  import { binderOf, diskChange, type DiskChange } from '../../lib/editor/disk';
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
    paste,
    renameLayer,
    rotate,
    selectAll,
    selectedStrokes,
    setActiveLayer,
    setBackground,
    setLayerVisible,
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
  import type { ExportSource, FileStamp, Note, Rgba, SplitSpec, Stroke } from '../../lib/note';
  import type { NoteOriginal } from '../../lib/original';
  import type { NarrationInfo } from '../../lib/video';
  import ExportDialog from '../ExportDialog.svelte';
  import VideoExportDialog from '../VideoExportDialog.svelte';
  import { clock } from '../narration/format';
  import RecordPanel from '../narration/RecordPanel.svelte';
  import { NarrationRecorder } from '../narration/recorder.svelte';
  import Canvas from './Canvas.svelte';
  import LayersPanel from './LayersPanel.svelte';
  import OriginalPanel from './OriginalPanel.svelte';
  import PagePanel from './PagePanel.svelte';
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
  let ink = $state.raw<Ink | null>(null);
  let history = $state.raw<History<Doc> | null>(null);
  let savedNote = $state.raw<Note | null>(null);
  let stamp = $state.raw<FileStamp | null>(null);
  let title = $state('');
  let titleDraft = $state('');
  let tools = $state.raw<ToolSettings>(DEFAULT_TOOLS);
  let widths = { ...DEFAULT_WIDTHS };
  let customColors = $state.raw<Rgba[]>(loadCustomColors());
  let clipboard: Stroke[] = [];
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
  let copying = $state(false);
  let leaving = $state(false);
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
    return replaying && doc && shown ? { note: shown, selection: [], activeLayer: doc.activeLayer } : null;
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
      if (next !== currentPath) void open(next);
    });
  });

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
    return () => {
      stopped = true;
      alive = false;
      clearTimeout(noticeTimer);
      for (const stop of stops) stop();
      player.dispose();
      recorder.dispose();
      // A recording is never thrown away silently: it goes into its note (or stays pending).
      salvageRecording();
      dropNarrationSound();
    };
  });

  async function open(target: string) {
    // Another note is opened: the recording of this one is saved before the editor moves on.
    salvageRecording();
    generation += 1;
    currentPath = target;
    loadError = null;
    history = null;
    disk = null;
    split = null;
    exporting = null;
    closeVideo();
    resetReplay();
    original = null;
    originalError = null;
    try {
      const [opened, module] = await Promise.all([api.openNote(target), ink ?? loadInk()]);
      if (currentPath !== target) return;
      const start = docFromNote(opened.note, t('editor.layers.defaultName', { n: 1 }));
      ink = module;
      history = createHistory(start);
      savedNote = start.note;
      stamp = opened.stamp;
      title = opened.note.title;
      titleDraft = opened.note.title;
      targetTitle = opened.note.title;
      conflict = null;
      saveError = null;
      void refreshOriginal(true);
    } catch (error) {
      if (currentPath === target) loadError = errorMessage(error);
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
      const next = await api.saveNote(target, { ...present.note, title }, stamp);
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
        const next = await api.saveNote(created.path, note, created.stamp);
        if (gen !== generation) return false;
        currentPath = created.path;
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

  // --- Leaving ----------------------------------------------------------------------------

  function close() {
    alive = false;
    onclose();
  }

  function back() {
    if (recorder.busy) return showNotice(t('editor.replay.recordingBusy'), true);
    if (videoRunning) return showNotice(t('videoExport.closeBusy'), true);
    if (dirty) leaving = true;
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
    if (!alive || (!dirty && pendingWrites === 0)) return false;
    if (windowClose.destroy) {
      if (dirty) {
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
    const ok = await save();
    leaving = false;
    closing = false;
    if (!ok) return;
    if (fromWindow) await closeWindow();
    else close();
  }

  function discardAndLeave() {
    if (closing) void closeWindow();
    else {
      leaving = false;
      close();
    }
  }

  function stay() {
    leaving = false;
    closing = false;
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

  function copySelection() {
    if (doc && hasSelection) clipboard = selectedStrokes(doc);
  }

  function pasteClipboard() {
    if (!doc || clipboard.length === 0) return;
    if (!canDraw(doc)) return hiddenLayerNotice();
    change(paste(doc, clipboard));
    // The copies are now selected: pasting again shifts again.
    if (history) clipboard = selectedStrokes(history.present);
  }

  function duplicate() {
    if (!doc) return;
    if (!canDraw(doc)) return hiddenLayerNotice();
    change(duplicateSelection(doc));
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
    return doc ? { path: currentPath, note: { ...doc.note, title } } : null;
  }

  function openExport() {
    const source = currentSource();
    if (source) exporting = [source];
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
        const result = await api.splitNote(target, { ...present.note, title }, stamp, spec, state.title.trim());
        if (gen !== generation) return;
        const start = docFromNote(result.note, t('editor.layers.defaultName', { n: 1 }));
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
      const wav = await api.narrationAudio(currentPath);
      if (gen !== generation || !replaying || narration !== info) return;
      const url = URL.createObjectURL(new Blob([wav], { type: 'audio/wav' }));
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

  /** The keys of replay mode: R and Escape leave it, Space plays or pauses; nothing edits the note. */
  function replayKey(e: KeyboardEvent, key: string, mod: boolean) {
    if (mod) {
      if (key === '0') {
        e.preventDefault();
        canvas?.fit();
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
    if (exporting || video) return;
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
    if (mod) {
      const action: Record<string, (() => void) | undefined> = {
        z: e.shiftKey ? doRedo : doUndo,
        y: doRedo,
        c: copySelection,
        v: pasteClipboard,
        a: () => reselect(selectAll(doc!)),
        '0': () => canvas?.fit(),
      };
      const run = action[key];
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

<svelte:window {onkeydown} {onkeyup} onblur={() => (spaceDown = false)} />

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
            cropping={tools.mode === 'crop'}
            disabled={split !== null}
            onbackground={(b) => change(setBackground(doc!, b))}
            onrotate={(direction) => change(rotate(doc!, direction))}
            onautocrop={() => change(autoCrop(doc!))}
            onmanualcrop={() => setMode(tools.mode === 'crop' ? 'select' : 'crop')}
          />
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

  {#if video}
    <VideoExportDialog
      source={video}
      subject={title.trim() || t('editor.untitled')}
      onclose={closeVideo}
      onrunning={(running) => (videoRunning = running)}
    />
  {/if}

  {#if leaving}
    <div class="editor-dialog-backdrop">
      <div class="card editor-dialog" role="alertdialog" aria-modal="true" aria-labelledby="editor-leave-title" aria-describedby="editor-leave-text">
        <h2 id="editor-leave-title">{t('editor.leave.title')}</h2>
        <p id="editor-leave-text">{t('editor.leave.message')}</p>
        <div class="actions">
          <!-- svelte-ignore a11y_autofocus -->
          <button type="button" class="button primary" autofocus onclick={saveAndLeave} disabled={saving}>
            {closing ? t('editor.leave.saveClose') : t('editor.leave.save')}
          </button>
          <button type="button" class="button danger" onclick={discardAndLeave}>
            {closing ? t('editor.leave.discardClose') : t('editor.leave.discard')}
          </button>
          <button type="button" class="button" onclick={stay}>{t('editor.leave.cancel')}</button>
        </div>
      </div>
    </div>
  {/if}
</section>
