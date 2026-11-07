<script lang="ts" module>
  let counter = 0;
  /** The modals open, the last one on top: only that one answers Escape. */
  const stack: number[] = [];

  /** A modal is open: Escape, Ctrl + W and Ctrl + F4 are its own, not those of the screen under it. */
  export function modalOpen(): boolean {
    return stack.length > 0;
  }
</script>

<script lang="ts">
  /**
   * A modal dialog: title, content, Escape (or Ctrl + W, Ctrl + F4) to close, Tab kept inside, focus
   * moved in on opening and given back on closing. `dismissible` false (an operation is running)
   * ignores those keys. They work wherever the focus is (a button that became disabled, the native
   * file picker that just closed: the focus is then on the page), for the topmost modal only; the
   * focus comes back to the card when the control that had it is disabled.
   */
  import { onMount, type Snippet } from 'svelte';
  import { closeKey } from '../lib/close-keys';

  let {
    title,
    onclose,
    dismissible = true,
    role = 'dialog',
    children,
  }: {
    title: string;
    onclose: () => void;
    dismissible?: boolean;
    /** `alertdialog` for a question that must be answered (unsaved changes, leaving). */
    role?: 'dialog' | 'alertdialog';
    children: Snippet;
  } = $props();

  const id = ++counter;
  const titleId = `modal-title-${id}`;
  let dialog = $state<HTMLElement | null>(null);

  const FOCUSABLE = 'button, input, select, textarea, [href], [tabindex]:not([tabindex="-1"])';

  function focusable(): HTMLElement[] {
    return [...(dialog?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? [])].filter(
      (el) => !el.hasAttribute('disabled') && el.getAttribute('aria-hidden') !== 'true',
    );
  }

  /** The focus went nowhere (the control that had it was disabled): back to the dialog. */
  function recoverFocus() {
    const active = document.activeElement;
    const lost = !active || active === document.body || (active instanceof HTMLElement && dialog?.contains(active) && 'disabled' in active && active.disabled);
    if (lost && document.hasFocus()) dialog?.focus();
  }

  onMount(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    (dialog?.querySelector<HTMLElement>('[data-autofocus]') ?? focusable()[0] ?? dialog)?.focus();
    stack.push(id);
    const watcher = new MutationObserver(recoverFocus);
    if (dialog) watcher.observe(dialog, { attributes: true, subtree: true, attributeFilter: ['disabled'] });
    return () => {
      watcher.disconnect();
      const at = stack.lastIndexOf(id);
      if (at >= 0) stack.splice(at, 1);
      previous?.focus();
    };
  });

  /** Escape, Ctrl + W and Ctrl + F4, wherever the focus is. */
  function windowKey(event: KeyboardEvent) {
    if (event.key !== 'Escape' && closeKey(event) !== 'close') return;
    if (event.defaultPrevented || stack[stack.length - 1] !== id) return;
    event.preventDefault();
    event.stopPropagation();
    if (dismissible) onclose();
  }

  function onkeydown(event: KeyboardEvent) {
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

<svelte:window onkeydown={windowKey} />

<div class="modal-backdrop">
  <!-- The dialog handles the keys that bubble up from its own controls. -->
  <div
    bind:this={dialog}
    class="card modal"
    {role}
    aria-modal="true"
    aria-labelledby={titleId}
    tabindex="-1"
    {onkeydown}
  >
    <h2 id={titleId}>{title}</h2>
    {@render children()}
  </div>
</div>
