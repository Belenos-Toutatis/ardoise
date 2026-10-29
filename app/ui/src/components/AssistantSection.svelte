<script lang="ts">
  /**
   * Settings: the assistant (MCP, SPEC §4.9). What to paste into Claude Code or Claude Desktop to
   * declare this copy of Ardoise, built with the path of the running executable, with or without
   * `--read-only`; Ardoise never edits an assistant's configuration itself (docs/design/mcp.md,
   * guide docs/guide/assistant.*.md).
   */
  import { onMount } from 'svelte';
  import { api, errorMessage } from '../lib/api';
  import { claudeCodeCommand, claudeDesktopConfig } from '../lib/assistant-setup';
  import { t } from '../lib/i18n.svelte';

  type Target = 'claudeCode' | 'claudeDesktop';

  let exe = $state<string | null>(null);
  let error = $state<string | null>(null);
  let readOnly = $state(false);
  let copied = $state<Target | null>(null);
  let copyFailed = $state(false);

  const command = $derived(exe === null ? '' : claudeCodeCommand(exe, readOnly));
  const config = $derived(exe === null ? '' : claudeDesktopConfig(exe, readOnly));

  onMount(() => {
    api
      .mcpLaunchInfo()
      .then((info) => (exe = info.exe))
      .catch((e: unknown) => (error = errorMessage(e)));
  });

  async function copy(target: Target) {
    copyFailed = false;
    try {
      await navigator.clipboard.writeText(target === 'claudeCode' ? command : config);
      copied = target;
    } catch {
      copied = null;
      copyFailed = true;
    }
  }
</script>

<section class="settings-section" aria-labelledby="settings-assistant" data-slot="assistant">
  <h3 id="settings-assistant">{t('assistant.title')}</h3>
  <p class="muted small">{t('assistant.intro')}</p>
  {#if error}
    <p class="error" role="alert">{t('errors.generic', { message: error })}</p>
  {:else if exe !== null}
    <label class="choice">
      <input
        type="checkbox"
        data-setting="assistant-read-only"
        bind:checked={readOnly}
        onchange={() => (copied = null)}
      />
      {t('assistant.readOnly')}
    </label>
    <p class="muted small">{t('assistant.readOnlyHint')}</p>

    <h4 class="assistant-target">{t('assistant.code.title')}</h4>
    <p class="muted small">{t('assistant.code.hint')}</p>
    <pre class="assistant-snippet" data-snippet="claude-code"><code>{command}</code></pre>
    <button type="button" class="button" data-action="copy-claude-code" onclick={() => copy('claudeCode')}>
      {copied === 'claudeCode' ? t('assistant.copied') : t('assistant.copy')}
    </button>

    <h4 class="assistant-target">{t('assistant.desktop.title')}</h4>
    <p class="muted small">{t('assistant.desktop.hint')}</p>
    <pre class="assistant-snippet" data-snippet="claude-desktop"><code>{config}</code></pre>
    <button type="button" class="button" data-action="copy-claude-desktop" onclick={() => copy('claudeDesktop')}>
      {copied === 'claudeDesktop' ? t('assistant.copied') : t('assistant.copy')}
    </button>
    {#if copyFailed}
      <p class="error small" role="alert">{t('assistant.copyFailed')}</p>
    {/if}
    <p class="muted small">{t('assistant.appTools')}</p>
  {/if}
  <p class="muted small" data-slot="assistant-privacy">{t('assistant.privacy')}</p>
  <p class="muted small">{t('assistant.guide')}</p>
</section>

<style>
  .assistant-target {
    margin: 0.9rem 0 0.2rem;
    font-size: 0.9rem;
  }

  .assistant-snippet {
    margin: 0.3rem 0 0.4rem;
    padding: 0.5rem 0.6rem;
    border-radius: 6px;
    background: var(--surface-2);
    font-size: 0.8rem;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
</style>
