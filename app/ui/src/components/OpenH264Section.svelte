<script lang="ts">
  /**
   * Settings: the backup MP4 encoder (OpenH264, downloaded from Cisco). Windows normally writes
   * MP4 itself; only an « N » edition without the Media Feature Pack needs this. Cisco requires
   * its notice where the user controls the codec and its licence where the licensing is shown,
   * both reproduced as the app sends them.
   */
  import { onMount } from 'svelte';
  import { api, errorMessage } from '../lib/api';
  import { currentLocale, t } from '../lib/i18n.svelte';
  import { megabytes, type DownloadProgress, type OpenH264State } from '../lib/video';

  /** Null until the state is known. */
  let codec = $state.raw<OpenH264State | null>(null);
  let download = $state.raw<DownloadProgress | null>(null);
  let downloading = $state(false);
  /** Cancellation was asked and the download has not stopped yet. */
  let cancelling = $state(false);
  let cancelled = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);

  onMount(() => {
    let alive = true;
    api
      .openH264State()
      .then((next) => {
        if (alive) codec = next;
      })
      .catch((e) => {
        if (alive) error = errorMessage(e);
      });
    return () => {
      alive = false;
    };
  });

  function megabytesText(bytes: number): string {
    return new Intl.NumberFormat(currentLocale(), { minimumFractionDigits: 1, maximumFractionDigits: 1 }).format(
      megabytes(bytes),
    );
  }

  async function install() {
    error = null;
    cancelled = false;
    cancelling = false;
    downloading = true;
    download = null;
    try {
      codec = await api.openH264Download((progress) => {
        if (downloading) download = progress;
      });
    } catch (e) {
      if (typeof e === 'object' && e !== null && 'code' in e && e.code === 'cancelled') cancelled = true;
      else error = errorMessage(e);
    } finally {
      downloading = false;
      cancelling = false;
      download = null;
    }
  }

  async function cancelDownload() {
    if (!downloading || cancelling) return;
    cancelling = true;
    try {
      await api.openH264CancelDownload();
    } catch (e) {
      cancelling = false;
      error = errorMessage(e);
    }
  }

  /** Runs a change of the codec's state (allow, remove) and keeps the answer. */
  async function change(action: () => Promise<OpenH264State>) {
    error = null;
    cancelled = false;
    busy = true;
    try {
      codec = await action();
    } catch (e) {
      error = errorMessage(e);
    } finally {
      busy = false;
    }
  }
</script>

<section class="settings-section" aria-labelledby="settings-openh264">
  <h3 id="settings-openh264">{t('openh264.title')}</h3>
  <p class="muted small">{t('openh264.intro')}</p>

  {#if codec}
    <p class="small" data-slot="media-foundation">
      {codec.mediaFoundation ? t('openh264.mediaFoundationOk') : t('openh264.mediaFoundationMissing')}
    </p>
    <p class="openh264-notice" data-slot="notice">{codec.notice}</p>

    <dl class="facts">
      <dt>{t('openh264.status.label')}</dt>
      <dd data-slot="status">
        {#if codec.status === 'installed'}
          {t('openh264.status.installed', {
            size: codec.bytes === null ? '?' : megabytesText(codec.bytes),
            version: codec.version,
          })}
        {:else}
          {t(`openh264.status.${codec.status}`)}
        {/if}
      </dd>
    </dl>

    <details class="openh264-license" open={codec.status !== 'installed'}>
      <summary>{t('openh264.licenseTitle')}</summary>
      <!-- A scrollable text must be reachable and scrollable with the keyboard. -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <pre tabindex="0">{codec.license}</pre>
    </details>

    {#if downloading}
      <div class="openh264-progress" role="status">
        <p class="small">{cancelling ? t('openh264.cancelling') : t('openh264.downloading')}</p>
        {#if download}
          {#if download.total}
            <progress max={download.total} value={download.received} aria-label={t('openh264.downloadProgressLabel')}></progress>
            <p class="muted small">
              {t('openh264.downloadProgress', {
                received: megabytesText(download.received),
                total: megabytesText(download.total),
              })}
            </p>
          {:else}
            <p class="muted small">{t('openh264.downloadReceived', { received: megabytesText(download.received) })}</p>
          {/if}
        {/if}
      </div>
      <div class="actions">
        <button type="button" class="button" disabled={cancelling} onclick={cancelDownload}>
          {t('openh264.cancelDownload')}
        </button>
      </div>
    {:else}
      {#if codec.status !== 'installed'}
        <p class="muted small">{t('openh264.downloadHint')}</p>
      {/if}
      {#if codec.status === 'installed'}
        <label class="check">
          <input
            type="checkbox"
            checked={codec.allowed}
            disabled={busy}
            onchange={(event) => {
              const allowed = event.currentTarget.checked;
              void change(() => api.setOpenH264Allowed(allowed));
            }}
          />
          {t('openh264.allow')}
        </label>
        <p class="muted small">{t('openh264.allowHint')}</p>
      {/if}
      <div class="actions">
        {#if codec.status !== 'installed'}
          <button type="button" class="button primary" disabled={busy} onclick={install}>
            {t('openh264.download')}
          </button>
        {/if}
        {#if codec.status !== 'notInstalled'}
          <button type="button" class="button danger" disabled={busy} onclick={() => change(() => api.openH264Remove())}>
            {t('openh264.remove')}
          </button>
        {/if}
      </div>
    {/if}
    {#if cancelled}
      <p class="muted small" role="status">{t('openh264.cancelled')}</p>
    {/if}
  {/if}
  {#if error}
    <p class="error" role="alert">{t('errors.generic', { message: error })}</p>
  {/if}
</section>
