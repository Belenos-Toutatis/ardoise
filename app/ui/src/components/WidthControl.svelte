<script lang="ts">
  /**
   * A line width, set three ways (the user's feedback of 2026-11-13: the slider alone is too coarse to set a precise
   * width): a slider for the rough value, « − » and « + » for one step at a time, and a field where the value is typed
   * in millimetres (comma or point). Widths are in device units, hundredths of a millimetre. `oninput` follows the
   * slider while it is dragged; `onchange` is the value that is kept (slider let go, a button, a typed value).
   */
  import { t } from '../lib/i18n.svelte';

  let {
    value,
    min = 10,
    max = 1000,
    step = 5,
    disabled = false,
    label,
    oninput,
    onchange,
  }: {
    value: number;
    min?: number;
    max?: number;
    step?: number;
    disabled?: boolean;
    /** What the width is of, for the controls' accessible names (« Épaisseur »). */
    label: string;
    oninput?: (width: number) => void;
    onchange: (width: number) => void;
  } = $props();

  const mm = (units: number) => (units / 100).toFixed(2);
  const clamp = (units: number) => Math.min(max, Math.max(min, Math.round(units)));

  /** One step down or up, landing on the steps' grid (0.62 mm goes to 0.60 or 0.65, not to 0.57 or 0.67). */
  function nudge(direction: -1 | 1) {
    const next = direction < 0 ? Math.ceil(value / step - 1e-9) * step - step : Math.floor(value / step + 1e-9) * step + step;
    onchange(clamp(next));
  }

  /** The typed value, kept if it reads as a number of millimetres; otherwise the field shows the width again. */
  function typed(event: Event) {
    const field = event.currentTarget as HTMLInputElement;
    const number = Number(field.value.trim().replace(',', '.'));
    if (field.value.trim() !== '' && Number.isFinite(number)) {
      const next = clamp(number * 100);
      if (next !== value) onchange(next);
    }
    field.value = mm(value);
  }

  function keydown(event: KeyboardEvent) {
    if (event.key === 'Enter') (event.currentTarget as HTMLInputElement).blur();
  }
</script>

<span class="width-control" data-slot="width-control">
  <button
    type="button"
    class="step"
    data-step="down"
    aria-label={t('editor.width.thinner', { label })}
    title={t('editor.width.thinner', { label })}
    disabled={disabled || value <= min}
    onclick={() => nudge(-1)}>−</button
  >
  <input
    type="range"
    {min}
    {max}
    {step}
    {value}
    {disabled}
    aria-label={label}
    oninput={(e) => (oninput ?? onchange)(Number(e.currentTarget.value))}
    onchange={(e) => onchange(Number(e.currentTarget.value))}
  />
  <button
    type="button"
    class="step"
    data-step="up"
    aria-label={t('editor.width.thicker', { label })}
    title={t('editor.width.thicker', { label })}
    disabled={disabled || value >= max}
    onclick={() => nudge(1)}>+</button
  >
  <input
    type="text"
    class="typed"
    inputmode="decimal"
    autocomplete="off"
    spellcheck="false"
    value={mm(value)}
    {disabled}
    aria-label={t('editor.width.typed', { label })}
    onchange={typed}
    onkeydown={keydown}
    onfocus={(e) => e.currentTarget.select()}
  />
  <span class="unit" aria-hidden="true">mm</span>
</span>

<style>
  .width-control {
    display: inline-flex;
    align-items: center;
    gap: 0.3rem;
  }

  input[type='range'] {
    width: 8rem;
    accent-color: var(--accent);
  }

  .step {
    width: 1.7rem;
    height: 1.7rem;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    color: var(--text);
    font-size: 1rem;
    line-height: 1;
    cursor: pointer;
  }

  .step:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .typed {
    width: 3.4rem;
    padding: 0.2rem 0.35rem;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
    font-size: 0.875rem;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .unit {
    color: var(--muted);
    font-size: 0.875rem;
  }

  .step:focus-visible,
  .typed:focus-visible,
  input[type='range']:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
