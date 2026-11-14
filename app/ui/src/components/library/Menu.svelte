<script lang="ts" module>
  export type { MenuItem } from '../../lib/context-menu';

  /** Closes the menu that is open, if any: a right click on another card or binder opens its own menu, and only one is open. */
  let closeOpenMenu: (() => void) | null = null;
</script>

<script lang="ts">
  /**
   * A small "more actions" menu button: opens a list of actions under the button, closes on Escape or outside click;
   * `show()` opens it from the owner's right click: at the pointer (`show(point)`, a context menu), or under the button
   * when the keyboard's Menu key asked (no point).
   */
  import { tick } from 'svelte';
  import type { MenuItem, Point } from '../../lib/context-menu';
  import ContextMenu from '../ContextMenu.svelte';

  let { label, items }: { label: string; items: MenuItem[] } = $props();

  let open = $state(false);
  /** Where the menu stands when a right click opened it at the pointer (then `open`, the one under the button, is off). */
  let pointAt = $state<Point | null>(null);
  let root = $state<HTMLElement | null>(null);
  let trigger = $state<HTMLButtonElement | null>(null);

  async function toggle() {
    pointAt = null;
    open = !open;
    if (open) {
      if (closeOpenMenu !== closeMe) closeOpenMenu?.();
      closeOpenMenu = closeMe;
      await tick();
      root?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    }
  }

  /**
   * Opens the menu (the right click on what the menu belongs to): at `at`, the pointer, when given; else under the button,
   * and nothing when it is open there already.
   */
  export function show(at?: Point): void {
    if (at) {
      open = false;
      if (closeOpenMenu !== closeMe) closeOpenMenu?.();
      closeOpenMenu = closeMe;
      pointAt = at;
    } else if (!open) {
      pointAt = null;
      void toggle();
    }
  }

  function closeMe() {
    open = false;
    pointAt = null;
  }

  function close(returnFocus: boolean) {
    open = false;
    if (closeOpenMenu === closeMe) closeOpenMenu = null;
    if (returnFocus) trigger?.focus();
  }

  function choose(item: MenuItem) {
    // Kept focusable (not `disabled`) so that the hint can be read and the arrows still pass over it.
    if (item.disabled) return;
    close(false);
    item.onselect();
  }

  function onkeydown(event: KeyboardEvent) {
    if (!open) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      close(true);
      return;
    }
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    event.preventDefault();
    const entries = [...(root?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const at = entries.indexOf(document.activeElement as HTMLElement);
    const next = event.key === 'ArrowDown' ? at + 1 : at - 1;
    entries[(next + entries.length) % entries.length]?.focus();
  }

  function outside(event: MouseEvent) {
    if (open && root && !root.contains(event.target as Node)) open = false;
  }

  // Listens to the page only while the menu is open (every card has a menu: a listener each
  // would be a thousand listeners).
  $effect(() => {
    if (!open) return;
    window.addEventListener('click', outside);
    return () => window.removeEventListener('click', outside);
  });
</script>

<!-- The keydown handler only handles keys bubbling up from the buttons inside. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div class="menu" bind:this={root} {onkeydown}>
  <button
    bind:this={trigger}
    type="button"
    class="icon-button"
    aria-label={label}
    aria-haspopup="menu"
    aria-expanded={open || pointAt !== null}
    onclick={toggle}
  >
    <span aria-hidden="true">⋯</span>
  </button>
  {#if pointAt}
    <ContextMenu
      {label}
      {items}
      at={pointAt}
      returnTo={trigger}
      onclose={() => {
        pointAt = null;
        if (closeOpenMenu === closeMe) closeOpenMenu = null;
      }}
    />
  {/if}
  {#if open}
    <div class="menu-list" role="menu" aria-label={label}>
      {#each items as item (item.id)}
        <button
          type="button"
          role="menuitem"
          class:danger={item.danger}
          class:disabled={item.disabled}
          aria-disabled={item.disabled ? 'true' : undefined}
          title={item.disabled ? item.hint : undefined}
          onclick={() => choose(item)}
        >
          {item.label}
        </button>
      {/each}
    </div>
  {/if}
</div>
