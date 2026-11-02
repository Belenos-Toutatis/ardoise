<script lang="ts">
  /**
   * Settings: the port of the share on the local network (SPEC §4.10). One number, 1024 to 65535,
   * kept for this PC in its own section of `ardoise.toml` (a conflict is a property of one PC,
   * and the file is shared by several). The other computer types the address with this port;
   * the firewall's permission follows the program, so changing it does not bring the prompt
   * back. A share already running keeps its port: the new one serves the next share.
   */
  import { onMount } from 'svelte';
  import { api, errorMessage, genericError } from '../lib/api';
  import { t } from '../lib/i18n.svelte';
  import { parseSharePort, SHARE_PORT_MAX, SHARE_PORT_MIN, type SharePortSetting } from '../lib/share';

  /** Null until the setting is known. */
  let setting = $state.raw<SharePortSetting | null>(null);
  /** The text of the field. */
  let draft = $state('');
  /** The last save was refused because the text is not a port. */
  let invalid = $state(false);
  /** The port just saved. */
  let saved = $state<number | null>(null);
  let saving = $state(false);
  let error = $state<string | null>(null);

  const changed = $derived(setting !== null && draft.trim() !== String(setting.port));

  onMount(() => {
    let alive = true;
    api
      .sharePort()
      .then((next) => {
        if (!alive) return;
        setting = next;
        draft = String(next.port);
      })
      .catch((e) => {
        if (alive) error = errorMessage(e);
      });
    return () => {
      alive = false;
    };
  });

  async function save() {
    error = null;
    saved = null;
    const port = parseSharePort(draft);
    invalid = port === null;
    if (port === null) return;
    saving = true;
    try {
      setting = await api.setSharePort(port);
      draft = String(setting.port);
      saved = setting.port;
    } catch (e) {
      error = errorMessage(e);
    } finally {
      saving = false;
    }
  }

  function edited() {
    invalid = false;
    saved = null;
  }
</script>

<section class="settings-section" aria-labelledby="settings-share-port">
  <h3 id="settings-share-port">{t('settings.sharePort.title')}</h3>
  <p class="muted small">{t('settings.sharePort.hint')}</p>
  <form
    onsubmit={(event) => {
      event.preventDefault();
      void save();
    }}
    novalidate
  >
    <label class="field">
      <span>{t('settings.sharePort.label')}</span>
      <input
        class="text-input"
        type="number"
        inputmode="numeric"
        min={SHARE_PORT_MIN}
        max={SHARE_PORT_MAX}
        step="1"
        data-setting="share-port"
        disabled={setting === null}
        value={draft}
        oninput={(event) => {
          draft = event.currentTarget.value;
          edited();
        }}
        aria-invalid={invalid}
        aria-describedby={invalid ? 'settings-share-port-invalid' : 'settings-share-port-range'}
      />
      <span id="settings-share-port-range" class="muted small field-hint">{t('settings.sharePort.range')}</span>
    </label>
    {#if invalid}
      <p id="settings-share-port-invalid" class="error" role="alert" data-slot="invalid">{t('settings.sharePort.invalid')}</p>
    {/if}
    <div class="actions">
      <button type="submit" class="button primary" data-action="share-port-save" disabled={!changed || saving}>
        {t('settings.sharePort.save')}
      </button>
    </div>
  </form>
  {#if setting?.own && setting.port !== setting.sharedPort}
    <p class="muted small" data-slot="own">{t('settings.sharePort.own', { shared: setting.sharedPort })}</p>
  {/if}
  {#if saved !== null}
    <p class="muted small" role="status" data-slot="saved">{t('settings.sharePort.saved', { port: saved })}</p>
  {/if}
  {#if error}
    <p class="error" role="alert">{genericError(error)}</p>
  {/if}
</section>
