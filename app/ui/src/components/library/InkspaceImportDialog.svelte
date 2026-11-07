<script lang="ts">
  /**
   * « Importer depuis Inkspace » (M10): finds Wacom Inkspace's data on this PC (or lets the user
   * choose a folder, a copy for instance), counts its notes, then imports them into a binder
   * with a progress bar and a Stop button. The report lists what was imported, skipped and
   * what failed; running the import again only adds what is missing.
   */
  import { onMount } from 'svelte';
  import { api, errorMessage } from '../../lib/api';
  import { pickInkspaceFolder } from '../../lib/dialogs';
  import { currentLocale, t } from '../../lib/i18n.svelte';
  import { DEFAULT_BINDER, type InkspaceProgress, type InkspaceReport, type InkspaceSource } from '../../lib/inkspace';
  import { formatDate } from '../../lib/library-logic';
  import { beginWork } from '../../lib/work';
  import Modal from '../Modal.svelte';

  let {
    binders,
    onclose,
    onshow,
    ondone = () => {},
  }: {
    /** Existing binders of the library (`/`-separated), proposed for the destination. */
    binders: string[];
    onclose: () => void;
    /** « Voir les notes importées »: the binder the notes went into (empty for the root). */
    onshow: (binder: string) => void;
    /** An import has finished or was stopped: the library has changed. */
    ondone?: () => void;
  } = $props();

  /** Looking for Inkspace on this PC. */
  let detecting = $state(true);
  /** Reading the folder (it can take a second). */
  let reading = $state(false);
  /** The folder being read or read: the one found, or chosen. */
  let folder = $state<string | null>(null);
  let source = $state.raw<InkspaceSource | null>(null);
  /** Why the folder cannot be used. */
  let readError = $state<string | null>(null);
  /** Why the last import did not start or failed. */
  let importError = $state<string | null>(null);
  let binder = $state(DEFAULT_BINDER);
  let running = $state(false);
  $effect(() => {
    if (running) return beginWork();
  });
  let stopping = $state(false);
  let progress = $state.raw<InkspaceProgress | null>(null);
  let report = $state.raw<InkspaceReport | null>(null);

  const busy = $derived(detecting || reading);
  /** Notes an import will add: the readable ones not yet in the library. */
  const toImport = $derived(source ? Math.max(0, source.notes - source.alreadyImported - source.unreadable) : 0);

  async function inspectFolder(path: string) {
    folder = path;
    source = null;
    readError = null;
    importError = null;
    reading = true;
    try {
      const found = await api.inkspaceInspect(path);
      source = found;
      folder = found.path;
    } catch (e) {
      readError = errorMessage(e);
    } finally {
      reading = false;
    }
  }

  async function pick() {
    const picked = await pickInkspaceFolder(t('inkspace.pickTitle'));
    if (picked) await inspectFolder(picked);
  }

  async function start() {
    if (!source || running || toImport === 0) return;
    running = true;
    stopping = false;
    progress = null;
    importError = null;
    try {
      report = await api.inkspaceImport(source.path, binder.trim(), (next) => (progress = next));
      ondone();
    } catch (e) {
      importError = errorMessage(e);
    } finally {
      running = false;
    }
  }

  async function stop() {
    stopping = true;
    try {
      await api.inkspaceCancel();
    } catch {
      // The import ends on its own; nothing to tell the user.
    }
  }

  /** The reason shown for a note that was not imported, translated like any command error. */
  function reason(failure: { code: string; message: string }): string {
    return errorMessage({ code: failure.code, message: failure.message });
  }

  onMount(() => {
    void (async () => {
      try {
        const found = await api.inkspaceDetect();
        if (found) await inspectFolder(found);
      } catch {
        // Not finding Inkspace is the same as not having it: the user may pick a folder.
      } finally {
        detecting = false;
      }
    })();
  });
</script>

<Modal title={t('inkspace.title')} {onclose} dismissible={!running}>
  {#if report}
    {@const done = report}
    <div data-slot="inkspace-report">
      <h3>{done.cancelled ? t('inkspace.cancelledTitle') : t('inkspace.doneTitle')}</h3>
      <p>
        {#if done.imported === 0}
          {t('inkspace.nothingImported')}
        {:else if done.binder === ''}
          {t(done.imported === 1 ? 'inkspace.importedRootOne' : 'inkspace.importedRoot', { count: done.imported })}
        {:else}
          {t(done.imported === 1 ? 'inkspace.importedOne' : 'inkspace.imported', {
            count: done.imported,
            binder: done.binder,
          })}
        {/if}
      </p>
      {#if done.skipped > 0}
        <p>{t(done.skipped === 1 ? 'inkspace.skippedOne' : 'inkspace.skipped', { count: done.skipped })}</p>
      {/if}
      {#if done.failed.length > 0}
        <p>
          {t(done.failed.length === 1 ? 'inkspace.failedOne' : 'inkspace.failed', { count: done.failed.length })}
        </p>
        <ul class="inkspace-failures">
          {#each done.failed as failure, index (index)}
            <li>
              <span class="inkspace-when">
                {failure.created === null ? t('inkspace.unknownDate') : formatDate(failure.created, currentLocale())}
              </span>
              <code>{failure.inkspaceId.slice(0, 8)}</code>
              <span>{reason(failure)}</span>
            </li>
          {/each}
        </ul>
      {/if}
      {#if done.cancelled}
        <p class="muted small">{t('inkspace.cancelledHint')}</p>
      {/if}
    </div>
    <div class="actions">
      {#if done.imported > 0}
        <button type="button" class="button primary" data-autofocus onclick={() => onshow(done.binder)}>
          {t('inkspace.show')}
        </button>
      {/if}
      <button type="button" class="button" onclick={onclose}>{t('common.close')}</button>
    </div>
  {:else}
    <form
      aria-busy={busy || running}
      onsubmit={(event) => {
        event.preventDefault();
        void start();
      }}
    >
      <p class="muted">{t('inkspace.intro')}</p>

      {#if folder}
        <div class="chosen">
          <span class="label">{t('inkspace.location')}</span>
          <code data-slot="inkspace-path">{folder}</code>
        </div>
      {/if}

      {#if busy}
        <p class="muted small" role="status">{detecting && !reading ? t('inkspace.detecting') : t('inkspace.reading')}</p>
      {:else if !folder}
        <p data-slot="inkspace-not-found">{t('inkspace.notFound')}</p>
      {/if}

      {#if source}
        <div data-slot="inkspace-count">
          <p>
            <strong>{t(source.notes === 1 ? 'inkspace.foundOne' : 'inkspace.found', { count: source.notes })}</strong>
          </p>
          {#if source.alreadyImported > 0}
            <p class="muted small">
              {t(source.alreadyImported === 1 ? 'inkspace.alreadyOne' : 'inkspace.already', {
                count: source.alreadyImported,
              })}
            </p>
          {/if}
          {#if source.unreadable > 0}
            <p class="muted small">
              {t(source.unreadable === 1 ? 'inkspace.unreadableOne' : 'inkspace.unreadable', {
                count: source.unreadable,
              })}
            </p>
          {/if}
        </div>

        {#if toImport === 0}
          <p class="muted">{t(source.unreadable > 0 ? 'inkspace.allReadableImported' : 'inkspace.allImported')}</p>
        {:else}
          <label class="field">
            <span>{t('inkspace.binder')}</span>
            <input class="text-input" type="text" list="inkspace-binders" bind:value={binder} disabled={running} />
            <datalist id="inkspace-binders">
              {#each binders as name (name)}
                <option value={name}></option>
              {/each}
            </datalist>
            <span class="muted small field-hint">{t('inkspace.binderHint')}</span>
          </label>
        {/if}
      {/if}

      {#if running}
        <div class="inkspace-progress">
          <progress
            max={progress?.total || 1}
            value={progress?.done ?? 0}
            aria-label={t('inkspace.progressLabel')}
          ></progress>
          <p class="muted small" role="status">
            {stopping
              ? t('inkspace.stopping')
              : progress
                ? t('inkspace.progress', { done: progress.done, total: progress.total })
                : t('inkspace.reading')}
          </p>
        </div>
      {/if}

      {#if readError}
        <p class="banner error inline" role="alert">{t('inkspace.readError', { message: readError })}</p>
      {/if}
      {#if importError}
        <p class="banner error inline" role="alert">{t('inkspace.importError', { message: importError })}</p>
      {/if}

      <div class="actions">
        {#if running}
          <!-- Enabled once the import is under way (its first progress): a stop is never lost. -->
          <button type="button" class="button" data-autofocus disabled={stopping || !progress} onclick={() => void stop()}>
            {t('inkspace.stop')}
          </button>
        {:else}
          {#if source && toImport > 0}
            <button type="submit" class="button primary" data-autofocus disabled={busy}>
              {t(toImport === 1 ? 'inkspace.importOne' : 'inkspace.import', { count: toImport })}
            </button>
          {/if}
          {#if readError && folder}
            <button type="button" class="button" disabled={busy} onclick={() => void inspectFolder(folder!)}>
              {t('inkspace.retry')}
            </button>
          {/if}
          <button type="button" class="button" disabled={busy} onclick={() => void pick()}>
            {folder ? t('inkspace.pickOther') : t('inkspace.pick')}
          </button>
          <button type="button" class="button" onclick={onclose}>{t('library.cancel')}</button>
        {/if}
      </div>
    </form>
  {/if}
</Modal>
