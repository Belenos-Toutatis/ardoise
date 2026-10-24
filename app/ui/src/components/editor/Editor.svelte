<script lang="ts">
  /**
   * The note editor (milestone M3). Contract used by the library screen: it opens the note at
   * `path`, calls `onsaved` with the note's path after each save or rename (the path changes
   * when the title changes), and `onclose` when the user goes back to the library.
   *
   * State: an undo/redo history of immutable `Doc` snapshots (note + selection + active layer,
   * `lib/editor/state.ts`), tool settings beside it. The note is dirty while the present note
   * is not the object last saved.
   */
  import { untrack } from 'svelte';
  import { api, errorMessage, type AppState } from '../../lib/api';
  import { loadCustomColors, rememberCustomColor } from '../../lib/editor/colors';
  import { canRedo, canUndo, commit, createHistory, redo, replacePresent, undo, type History } from '../../lib/editor/history';
  import {
    DEFAULT_TOOLS,
    addLayer,
    applyStyle,
    autoCrop,
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
  }: { path: string; app: AppState; onclose: () => void; onsaved?: (path: string) => void } = $props();

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
  let leaving = $state(false);
  let spaceDown = $state(false);
  let canvas = $state<ReturnType<typeof Canvas>>();

  const doc = $derived(history?.present ?? null);
  const dirty = $derived(!!doc && doc.note !== savedNote);
  const hasSelection = $derived((doc?.selection.length ?? 0) > 0);

  // (Re)open when the library screen gives another note; a path we produced ourselves (after
  // a rename) is already open.
  $effect(() => {
    const next = path;
    untrack(() => {
      if (next !== currentPath) void open(next);
    });
  });

  async function open(target: string) {
    currentPath = target;
    loadError = null;
    history = null;
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
      conflict = null;
      saveError = null;
    } catch (error) {
      if (currentPath === target) loadError = errorMessage(error);
    }
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

  async function save(): Promise<boolean> {
    if (!doc || !stamp || saving) return false;
    saving = true;
    saveError = null;
    conflict = null;
    const note = doc.note;
    try {
      stamp = await api.saveNote(currentPath, { ...note, title }, stamp);
      savedNote = note;
      onsaved?.(currentPath);
      return true;
    } catch (error) {
      const code = typeof error === 'object' && error !== null && 'code' in error ? (error as { code: unknown }).code : null;
      if (code === 'noteConflict') conflict = errorMessage(error);
      else saveError = errorMessage(error);
      return false;
    } finally {
      saving = false;
    }
  }

  async function retitle() {
    const next = titleDraft.trim();
    if (!stamp || next === title) {
      titleDraft = title;
      return;
    }
    titleError = null;
    try {
      const result = await api.retitleNote(currentPath, next, stamp);
      currentPath = result.path;
      stamp = result.stamp;
      title = next;
      titleDraft = next;
      onsaved?.(result.path);
    } catch (error) {
      titleError = errorMessage(error);
      titleDraft = title;
    }
  }

  function back() {
    if (dirty) leaving = true;
    else onclose();
  }

  async function saveAndLeave() {
    if (await save()) {
      leaving = false;
      onclose();
    } else {
      leaving = false;
    }
  }

  // --- Tools and style -------------------------------------------------------------------

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

  function copySelection() {
    if (doc && hasSelection) clipboard = selectedStrokes(doc);
  }

  function pasteClipboard() {
    if (doc && clipboard.length > 0) {
      change(paste(doc, clipboard));
      // The copies are now selected: pasting again shifts again.
      if (history) clipboard = selectedStrokes(history.present);
    }
  }

  // --- Keyboard ---------------------------------------------------------------------------

  function isTyping(target: EventTarget | null): boolean {
    if (!(target instanceof HTMLElement)) return false;
    if (target instanceof HTMLInputElement) {
      return !['checkbox', 'radio', 'range', 'color', 'button'].includes(target.type);
    }
    return target instanceof HTMLTextAreaElement || target instanceof HTMLSelectElement || target.isContentEditable;
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
      if (key === 'Escape') leaving = false;
      return;
    }
    if (isTyping(e.target)) return;
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
        // Space on a focused button presses it; elsewhere it pans while held.
        if (!(e.target instanceof HTMLButtonElement)) {
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
      <button type="button" class="button small-button" onclick={() => (conflict = null)}>{t('editor.dismiss')}</button>
    </div>
  {/if}
  {#if saveError}
    <div class="banner error editor-banner" role="alert">
      <p>{t('editor.saveError', { message: saveError })}</p>
      <button type="button" class="button small-button" onclick={() => (saveError = null)}>{t('editor.dismiss')}</button>
    </div>
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
      />
      <aside class="editor-side" aria-label={t('editor.sidePanel')}>
        {#if hasSelection}
          <section class="side-section" aria-labelledby="editor-selection-heading">
            <h3 id="editor-selection-heading">{t('editor.selection.label')}</h3>
            <p class="small muted">{t('editor.selection.count', { count: doc.selection.length })}</p>
            <div class="buttons">
              <button type="button" class="button small-button" onclick={() => change(duplicateSelection(doc!))}>
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
          <button type="button" class="button primary" autofocus onclick={saveAndLeave} disabled={saving}>{t('editor.leave.save')}</button>
          <button type="button" class="button danger" onclick={() => onclose()}>{t('editor.leave.discard')}</button>
          <button type="button" class="button" onclick={() => (leaving = false)}>{t('editor.leave.cancel')}</button>
        </div>
      </div>
    </div>
  {/if}
</section>
