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
   */
  import { untrack } from 'svelte';
  import { api, errorMessage, onLibraryChanged, type AppState } from '../../lib/api';
  import { loadCustomColors, rememberCustomColor } from '../../lib/editor/colors';
  import { binderOf, diskChange, type DiskChange } from '../../lib/editor/disk';
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
  import { defaultWindowClose, type WindowClose } from '../../lib/editor/window-close';
  import { t } from '../../lib/i18n.svelte';
  import { loadInk, type Ink, type Tool } from '../../lib/ink';
  import type { FileStamp, Note, Rgba, Stroke } from '../../lib/note';
  import Canvas from './Canvas.svelte';
  import LayersPanel from './LayersPanel.svelte';
  import PagePanel from './PagePanel.svelte';
  import Toolbar from './Toolbar.svelte';

  let {
    path,
    app,
    onclose,
    onsaved,
    windowClose = defaultWindowClose(),
  }: {
    path: string;
    app: AppState;
    onclose: () => void;
    onsaved?: (path: string) => void;
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
  let leaving = $state(false);
  /** The leave dialog was opened by a window close (not by the Back button). */
  let closing = $state(false);
  let spaceDown = $state(false);
  let canvas = $state<ReturnType<typeof Canvas>>();

  const doc = $derived(history?.present ?? null);
  const dirty = $derived(!!doc && doc.note !== savedNote);
  const hasSelection = $derived((doc?.selection.length ?? 0) > 0);

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
    };
  });

  async function open(target: string) {
    generation += 1;
    currentPath = target;
    loadError = null;
    history = null;
    disk = null;
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
    } catch (error) {
      if (currentPath === target) loadError = errorMessage(error);
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
    // A title typed but not committed yet (Ctrl+S in the field) is renamed first.
    if (titleDraft.trim() !== targetTitle) void retitle();
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
    if (dirty) leaving = true;
    else close();
  }

  /** The window is asked to close: true holds it back (unsaved changes, or a write under way). */
  function holdClose(): boolean {
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

  function showNotice(text: string) {
    notice = text;
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
    if (history) history = undo(history);
  }

  function doRedo() {
    if (history) history = redo(history);
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
    if (isTyping(e.target)) return;
    // A gesture under way commits on top of the doc it started from: changing the doc meanwhile
    // (undo, redo, delete, paste, select all…) would be overwritten, so those keys wait.
    if (canvas?.dragging() && (mod || ['Delete', 'Backspace', 'Escape'].includes(key))) {
      e.preventDefault();
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
        disabled={!doc}
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
        disabled={!history || !canUndo(history)}
        title={t('editor.undo')}
        aria-label={t('editor.undo')}
        onclick={doUndo}><span aria-hidden="true">↶</span></button
      >
      <button
        type="button"
        class="button ghost"
        disabled={!history || !canRedo(history)}
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
      <button type="button" class="button small-button primary" disabled={saving} onclick={saveCopy}>{t('editor.saveCopy')}</button>
      <button type="button" class="button small-button" onclick={() => (conflict = null)}>{t('editor.dismiss')}</button>
    </div>
  {:else if disk}
    <div class="banner editor-banner" role="status" data-disk={disk}>
      <strong>{disk === 'missing' ? t('editor.disk.missingTitle') : t('editor.disk.changedTitle')}</strong>
      <p>{disk === 'missing' ? t('editor.disk.missing') : dirty ? t('editor.disk.changedDirty') : t('editor.disk.changedClean')}</p>
      {#if disk === 'changed' && !dirty}
        <button type="button" class="button small-button primary" onclick={reload}>{t('editor.disk.reload')}</button>
      {:else}
        <button type="button" class="button small-button primary" disabled={saving} onclick={saveCopy}>{t('editor.saveCopy')}</button>
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
    <p class="banner editor-banner editor-notice" role="status">{notice}</p>
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
    />
    <div class="editor-main">
      <Canvas
        bind:this={canvas}
        {doc}
        {tools}
        {ink}
        spacePan={spaceDown}
        onchange={change}
        onselect={reselect}
        oncropped={() => setMode('select')}
        onblocked={hiddenLayerNotice}
      />
      <aside class="editor-side" aria-label={t('editor.sidePanel')}>
        {#if hasSelection}
          <section class="side-section" aria-labelledby="editor-selection-heading">
            <h3 id="editor-selection-heading">{t('editor.selection.label')}</h3>
            <p class="small muted">{t('editor.selection.count', { count: doc.selection.length })}</p>
            <div class="buttons">
              <button type="button" class="button small-button" onclick={duplicate}>
                {t('editor.selection.duplicate')}
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
          onbackground={(b) => change(setBackground(doc!, b))}
          onrotate={(direction) => change(rotate(doc!, direction))}
          onautocrop={() => change(autoCrop(doc!))}
          onmanualcrop={() => setMode(tools.mode === 'crop' ? 'select' : 'crop')}
        />
        <LayersPanel
          layers={doc.note.layers}
          active={doc.activeLayer}
          onactive={(i) => reselect(setActiveLayer(doc!, i))}
          onvisible={(i, visible) => change(setLayerVisible(doc!, i, visible))}
          onrename={(i, name) => change(renameLayer(doc!, i, name))}
          onmove={(from, to) => change(moveLayer(doc!, from, to))}
          onmerge={(i) => change(mergeDown(doc!, i))}
          onadd={() => change(addLayer(doc!, t('editor.layers.defaultName', { n: doc!.note.layers.length + 1 })))}
        />
      </aside>
    </div>
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
