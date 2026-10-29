<script lang="ts" module>
  export interface MenuItem {
    id: string;
    label: string;
    danger?: boolean;
    onselect: () => void;
  }
</script>

<script lang="ts">
  /** A small "more actions" menu button: opens a list of actions, closes on Escape or outside click. */
  import { tick } from 'svelte';

  let { label, items }: { label: string; items: MenuItem[] } = $props();

  let open = $state(false);
  let root = $state<HTMLElement | null>(null);
  let trigger = $state<HTMLButtonElement | null>(null);

  async function toggle() {
    open = !open;
    if (open) {
      await tick();
      root?.querySelector<HTMLElement>('[role="menuitem"]')?.focus();
    }
  }

  function close(returnFocus: boolean) {
    open = false;
    if (returnFocus) trigger?.focus();
  }

  function choose(item: MenuItem) {
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
    aria-expanded={open}
    onclick={toggle}
  >
    <span aria-hidden="true">⋯</span>
  </button>
  {#if open}
    <div class="menu-list" role="menu" aria-label={label}>
      {#each items as item (item.id)}
        <button type="button" role="menuitem" class:danger={item.danger} onclick={() => choose(item)}>
          {item.label}
        </button>
      {/each}
    </div>
  {/if}
</div>
