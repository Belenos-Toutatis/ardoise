/**
 * The tabbed side panel of the editor and of the live control window (milestone M16; contract:
 * docs/design/side-panel.md, §2 "Model" and §3 "Persistence"): the tabs, the state of one window's panel (the tab
 * chosen, whether the pane is collapsed) and its persistence in the browser's storage of the WebView2 profile, like
 * the other per-PC choices of the interface (`ardoise.live.computerStyle`, `ardoise.export`): not in `ardoise.toml`,
 * which the PCs share through Nextcloud while the choice is this PC's.
 */

export type SideWhere = 'editor' | 'live';
export type SideTabId = 'layers' | 'page' | 'background' | 'sound' | 'projection' | 'help';
/** Every tab, in the rail's order; a window shows those it gives content for. */
export const SIDE_TABS: readonly SideTabId[] = ['layers', 'page', 'background', 'sound', 'projection', 'help'];
export type SideDot = 'warn' | 'rec' | 'live';
export interface SideBadge {
  count?: number;
  dot?: SideDot;
}
export type SideBadges = Partial<Record<SideTabId, SideBadge>>;
export interface SideView {
  tab: SideTabId;
  collapsed: boolean;
}
export const SIDE_DEFAULTS: Record<SideWhere, SideView> = {
  editor: { tab: 'layers', collapsed: false },
  live: { tab: 'layers', collapsed: false },
};

export function sideStorageKey(where: SideWhere): string {
  return `ardoise.sidePanel.${where}`;
}

function safeStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}

/**
 * The view remembered for a window, field by field: a `tab` that is not one of `SIDE_TABS` gives the window's default
 * tab, a `collapsed` that is not a boolean gives `false`; garbage or a blocked storage gives the defaults.
 */
export function loadSideView(where: SideWhere, storage: Pick<Storage, 'getItem'> | null = safeStorage()): SideView {
  const view = { ...SIDE_DEFAULTS[where] };
  let stored: unknown;
  try {
    stored = JSON.parse(storage?.getItem(sideStorageKey(where)) ?? 'null');
  } catch {
    return view;
  }
  if (!stored || typeof stored !== 'object') return view;
  const { tab, collapsed } = stored as { tab?: unknown; collapsed?: unknown };
  if (typeof tab === 'string' && (SIDE_TABS as readonly string[]).includes(tab)) view.tab = tab as SideTabId;
  if (typeof collapsed === 'boolean') view.collapsed = collapsed;
  return view;
}

/** Remembers `view` for a window; a blocked storage is ignored (the state holds for the session). */
export function saveSideView(where: SideWhere, view: SideView, storage: Pick<Storage, 'setItem'> | null = safeStorage()): void {
  try {
    storage?.setItem(sideStorageKey(where), JSON.stringify({ tab: view.tab, collapsed: view.collapsed }));
  } catch {
    // Blocked or full: the panel keeps working for this session.
  }
}

/** One window's panel: created by the window, given to `SidePanel`, driven by both. */
export class SidePanelState {
  /** The tab chosen; may be one this window lacks (then `shown` gives the default). */
  tab = $state<SideTabId>('layers');
  collapsed = $state(false);
  private readonly where: SideWhere;
  private readonly storage: Pick<Storage, 'getItem' | 'setItem'> | null;

  /** `storage`: where the view is remembered (default: the browser's; `null`: nowhere). */
  constructor(where: SideWhere, storage: Pick<Storage, 'getItem' | 'setItem'> | null = safeStorage()) {
    this.where = where;
    this.storage = storage;
    const view = loadSideView(where, storage);
    this.tab = view.tab;
    this.collapsed = view.collapsed;
  }

  /**
   * The tab shown among `available`: the chosen one, else the window's default, else the first. The tab chosen is
   * left as it is when the window lacks it: a choice made once a later milestone gives the tab is not lost.
   */
  shown(available: readonly SideTabId[]): SideTabId {
    if (available.includes(this.tab)) return this.tab;
    const fallback = SIDE_DEFAULTS[this.where].tab;
    if (available.includes(fallback)) return fallback;
    return available[0] ?? fallback;
  }

  /**
   * A click (or Enter) on a tab: on the tab shown while open, collapses; otherwise opens that tab. A tab that is not
   * in `available` is ignored.
   */
  toggle(tab: SideTabId, available: readonly SideTabId[]): void {
    if (!available.includes(tab)) return;
    if (!this.collapsed && this.shown(available) === tab) {
      this.collapsed = true;
    } else {
      this.tab = tab;
      this.collapsed = false;
    }
    this.save();
  }

  /**
   * Opens the pane, on `tab` when given (a selection: none; the replay: 'sound'). `persist: false` shows the view
   * without remembering it: the replay forces Son open and gives the view back when it ends, and a window closed
   * meanwhile must find the user's own view, not the replay's.
   */
  open(tab?: SideTabId, options: { persist?: boolean } = {}): void {
    if (tab !== undefined) this.tab = tab;
    this.collapsed = false;
    if (options.persist !== false) this.save();
  }

  collapse(): void {
    this.collapsed = true;
    this.save();
  }

  snapshot(): SideView {
    return { tab: this.tab, collapsed: this.collapsed };
  }

  restore(view: SideView): void {
    this.tab = view.tab;
    this.collapsed = view.collapsed;
    this.save();
  }

  private save(): void {
    saveSideView(this.where, this.snapshot(), this.storage);
  }
}
