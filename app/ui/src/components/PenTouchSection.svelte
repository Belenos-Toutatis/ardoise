<script lang="ts">
  /**
   * Settings: pen and touch. "Draw with a finger" (a touch screen without a pen); the choice is
   * remembered on this computer (`lib/touch-prefs.svelte.ts`). The pen's own behaviour is only
   * described here (docs/design/interface.md, "M8: pen and touch").
   */
  import { t } from '../lib/i18n.svelte';
  import { touchPrefs } from '../lib/touch-prefs.svelte';
</script>

<section class="settings-section" aria-labelledby="settings-pen-touch">
  <h3 id="settings-pen-touch">{t('settings.penTouch.title')}</h3>
  <label class="choice">
    <input
      type="checkbox"
      role="switch"
      data-setting="draw-with-finger"
      checked={touchPrefs.drawWithFinger}
      onchange={(e) => touchPrefs.set(e.currentTarget.checked)}
    />
    {t('settings.penTouch.drawWithFinger')}
  </label>
  <p class="muted small">{t('settings.penTouch.drawWithFingerHint')}</p>
  <p class="muted small">{t('settings.penTouch.penHint')}</p>
</section>
