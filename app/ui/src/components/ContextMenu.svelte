<script lang="ts" module>
  /** Closes the menu that is open, if any: only one context menu is on screen. */
  let closeOpen: (() => void) | null = null;
</script>

<script lang="ts">
  /**
   * A context menu at a point of the window (the right click on a card, the page, a tag, a layer…). Moved to the end of
   * the document so that no clipping or stacking context of the screen holds it, kept inside the window, and opened with
   * the focus on its first entry. The arrow keys, Home and End move between the entries, Enter or Space chooses, Escape
   * closes and gives the focus back (to `returnTo`, else to where it was); a click or a right click elsewhere, a scroll,
   * a resize or the window losing the focus close it without. A disabled entry stays focusable (its `hint` is its
   * title) and does nothing. The screens give it the entries and own what they do; nothing here knows a command.
   */
  import { onMount } from 'svelte';
  import { clampToWindow, type MenuItem, type Point } from '../lib/context-menu';

  let {
    label,
    items,
    at,
    onclose,
    returnTo = null,
  }: {
    /** What the menu is for, for a screen reader (« Menu de la note … »). */
    label: string;
    items: MenuItem[];
    at: Point;
    /** The menu closed itself (Escape, an entry chosen, a click elsewhere…): the owner forgets it. */
    onclose: () => void;
    /** Where Escape and a chosen entry give the focus back; default: the element that had it when the menu opened. */
    returnTo?: HTMLElement | null;
  } = $props();

  let list = $state<HTMLElement | null>(null);
  /** Where the list ended up, inside the window; before the measure, the point itself. */
  let spot = $state<Point | null>(null);
  let previous: HTMLElement | null = null;
  let closed = false;

  const entries = () => [...(list?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];

  /** Closes the menu: `refocus` gives the focus back (Escape, an entry chosen); a click elsewhere leaves it where it went. */
  function finish(refocus: boolean) {
    if (closed) return;
    closed = true;
    detach();
    if (closeOpen === finishOpen) closeOpen = null;
    if (refocus) {
      const back = returnTo?.isConnected ? returnTo : previous?.isConnected ? previous : null;
      back?.focus({ preventScroll: true });
    }
    onclose();
  }
  const finishOpen = () => finish(false);

  const outside = (event: Event) => {
    if (!(event.target instanceof Node) || !list?.contains(event.target)) finish(false);
  };
  const shut = () => finish(false);

  function attach() {
    window.addEventListener('pointerdown', outside, true);
    window.addEventListener('contextmenu', outside, true);
    window.addEventListener('scroll', shut, true);
    window.addEventListener('wheel', shut, { capture: true, passive: true });
    window.addEventListener('resize', shut);
    window.addEventListener('blur', shut);
  }

  function detach() {
    window.removeEventListener('pointerdown', outside, true);
    window.removeEventListener('contextmenu', outside, true);
    window.removeEventListener('scroll', shut, true);
    window.removeEventListener('wheel', shut, true);
    window.removeEventListener('resize', shut);
    window.removeEventListener('blur', shut);
  }

  onMount(() => {
    previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    // Out of the screen's own boxes: a card with `position: relative`, a panel that scrolls or clips.
    if (list) document.body.appendChild(list);
    if (list) spot = clampToWindow(at, list.getBoundingClientRect(), { width: window.innerWidth, height: window.innerHeight });
    const all = entries();
    (all.find((entry) => entry.getAttribute('aria-disabled') !== 'true') ?? all[0])?.focus({ preventScroll: true });
    closeOpen?.();
    closeOpen = finishOpen;
    attach();
    return () => {
      detach();
      if (closeOpen === finishOpen) closeOpen = null;
    };
  });

  function choose(item: MenuItem) {
    if (item.disabled) return;
    // The focus first, so that what the entry opens (a dialog, a field) takes it from there.
    finish(true);
    item.onselect();
  }

  function onkeydown(event: KeyboardEvent) {
    if (event.key === 'Escape' || event.key === 'Tab') {
      event.preventDefault();
      event.stopPropagation();
      finish(true);
      return;
    }
    const all = entries();
    if (all.length === 0) return;
    const current = all.indexOf(document.activeElement as HTMLElement);
    let next: number;
    if (event.key === 'ArrowDown') next = current + 1;
    else if (event.key === 'ArrowUp') next = current < 0 ? all.length - 1 : current - 1;
    else if (event.key === 'Home') next = 0;
    else if (event.key === 'End') next = all.length - 1;
    else return;
    event.preventDefault();
    all[(next + all.length) % all.length]?.focus({ preventScroll: true });
  }
</script>

<!-- The keydown handler handles the keys bubbling up from the entries, where the focus is. -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  bind:this={list}
  class="menu-list context-menu"
  role="menu"
  aria-label={label}
  tabindex="-1"
  style:left="{(spot ?? at).x}px"
  style:top="{(spot ?? at).y}px"
  {onkeydown}
>
  {#each items as item (item.id)}
    {#if item.separatorBefore}<div class="separator" role="separator"></div>{/if}
    <button
      type="button"
      role="menuitem"
      class:danger={item.danger}
      class:disabled={item.disabled}
      aria-disabled={item.disabled ? 'true' : undefined}
      title={item.disabled ? item.hint : undefined}
      data-menu-item={item.id}
      onclick={() => choose(item)}
    >
      {item.label}
    </button>
  {/each}
</div>

<style>
  /* Over everything of the screen (the editor's panels, the library's grid): it is the topmost thing open. */
  .context-menu {
    position: fixed;
    right: auto;
    bottom: auto;
    z-index: 1000;
    max-height: calc(100vh - 8px);
    overflow-y: auto;
  }

  .context-menu:focus {
    outline: none;
  }

  .separator {
    height: 1px;
    margin: 0.2rem 0.3rem;
    background: var(--border);
  }
</style>
