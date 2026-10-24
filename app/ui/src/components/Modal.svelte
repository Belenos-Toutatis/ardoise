<script lang="ts" module>
  let counter = 0;
</script>

<script lang="ts">
  /**
   * A modal dialog: title, content, Escape to close, Tab kept inside, focus moved in on opening
   * and given back on closing. `dismissible` false (an operation is running) ignores Escape.
   */
  import { onMount, type Snippet } from 'svelte';

  let {
    title,
    onclose,
    dismissible = true,
    children,
  }: {
    title: string;
    onclose: () => void;
    dismissible?: boolean;
    children: Snippet;
  } = $props();

  const titleId = `modal-title-${++counter}`;
  let dialog = $state<HTMLElement | null>(null);

  const FOCUSABLE = 'button, input, select, textarea, [href], [tabindex]:not([tabindex="-1"])';

  function focusable(): HTMLElement[] {
    return [...(dialog?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])].filter(
      (el) => !el.hasAttribute('disabled') && el.getAttribute('aria-hidden') !== 'true',
    );
  }

  onMount(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    (dialog?.querySelector<HTMLElement>('[data-autofocus]') ?? focusable()[0] ?? dialog)?.focus();
    return () => previous?.focus();
  });

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      if (dismissible) onclose();
      return;
    }
    if (event.key !== 'Tab') return;
    const items = focusable();
    if (items.length === 0) {
      event.preventDefault();
      return;
    }
    const first = items[0];
    const last = items[items.length - 1];
    const active = document.activeElement;
    if (event.shiftKey && (active === first || !dialog?.contains(active))) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && (active === last || !dialog?.contains(active))) {
      event.preventDefault();
      first.focus();
    }
  }
</script>

<div class="modal-backdrop">
  <!-- The dialog handles the keys that bubble up from its own controls. -->
  <div
    bind:this={dialog}
    class="card modal"
    role="dialog"
    aria-modal="true"
    aria-labelledby={titleId}
    tabindex="-1"
    {onkeydown}
  >
    <h2 id={titleId}>{title}</h2>
    {@render children()}
  </div>
</div>
