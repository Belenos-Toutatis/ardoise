# Tabbed side panel (milestone M16)

Decisions: `docs/questions.md` Q13 (2026-11-11), SPEC §5 « Panneau de côté à onglets », ROADMAP M16 — and M17, which
comes next and must plug in without rework (§11). Look and behaviour: the mockup the user validated,
`target/maquettes/maquette-barre-laterale.html` (not in git; its script is the reference for the rail, its CSS for the
look, its « nouveau » blocks are M17). Written on 2026-11-11 from reading the code at commit 3c5fc59, nothing built or
run: every "today" cites where it was read; "(supposé)" marks what was not checked.

**Words.** The **rail**: the column of icon tabs on the right edge of the editor and of the live control window. A
**tab**: Calques (`layers`), Page (`page`), Fond (`background`), Son (`sound`), Projection (`projection`), Aide
(`help`), always in that order. The **pane**: the panel left of the rail that shows the open tab. **Collapsed**: the
pane hidden, the rail alone, the page taking the width. The **above block**: what a window shows above the open tab,
whatever the tab (the selection; in the editor also the split).

## Rules

- One owner per file (§12); the contracts of package 0 are fixed: a package that needs to change a file it does not
  own, or a contract, says so in its report.
- Every `data-section`, `data-slot`, `data-action` and `data-field` that exists today stays on the same control
  wherever the control moves: the tests, the journeys and `perf.mjs` rely on them. One goes: live's toolbar group
  `[data-slot="share"]` (nothing reads it), whose button moves. New hooks are listed here.
- Where the mockup and SPEC/Q13 differ (the selection in live mode), SPEC and Q13 win; the differences, and every
  choice this document makes that the user may want to make, are in §13.
- M16 moves what exists into the rail and refreshes its look; it adds no function. What M17 brings does not show,
  not even greyed (Q13: « Jusqu'à M17, ce qui n'existe pas encore n'apparaît pas »).

## 1. Today

### Editor

`Editor.svelte:2051` `<aside class="editor-side" aria-label={t('editor.sidePanel')}>` … `:2200`. Its CSS is global:
`app.css:1502–1508` (`.editor-side`: 270 px, `overflow: auto`, left border), `:1510–1541` (`.side-section` and its
`h3`, `p`, `.row`, `.row select`, `.check`), `:1566–1570` (220 px when the window is ≤ 900 px).

| Block | Where | Hooks | Shown / state | M16 |
|---|---|---|---|---|
| Narration | `Editor.svelte:2053–2098`, `section.side-section[data-section="narration"]` (h3 `#editor-narration-heading`, `narration.title`), `RecordPanel` (2062–2078) | `[data-slot="narration-stored"]` (2056); `narration-delete`, `narration-delete-yes` (2084, 2093); RecordPanel's `narration-open`, `-start`, `-stop`, `-cancel`, `-close`, `-retry`, `-discard(-yes/-no)`, `[data-slot="narration-pending\|replaces\|save-note\|unsaved-warning\|record-error"]` (`components/narration/RecordPanel.svelte:75–176`) | only `{#if replaying}` (2052), and then it is the **only** block (the rest is the `{:else}`); `narration` is loaded by `enterReplay` (1473); `recordOpen` (279), `confirmDelete` (280), `recorder` (271) | Son |
| Split | `SplitPanel.svelte:54` `section.side-section.split-panel` (`#editor-split-heading`), `Editor.svelte:2100–2124` | `.split-panel` (tests) | `{#if split}` (236), opened by the toolbar's ✂ (`toggleSplit`, 1391) | above block |
| Selection | `Editor.svelte:2125–2159` `section.side-section` (`#editor-selection-heading`, `editor.selection.label`; count `editor.selection.count`) | `copy-selection`, `cut-selection`, `paste-selection`, `straighten-selection`; Dupliquer, Supprimer, Désélectionner without hooks | `{#if hasSelection}` (288) | above block |
| Page | `PagePanel.svelte:85–228` `section.side-section` (`#editor-page-heading`, `editor.page.label`), `Editor.svelte:2160–2183` | paper `select` (87–97), template `select` (98–109), texture checkbox (110–118), `div.group[data-section="page-format"]` (119–189: `orientation-portrait\|landscape`, `data-field="paper-size\|width-mm\|height-mm\|scale-ink"`, `apply-size`), rotate group (190–217: `rotate-ccw`, `rotate-cw`), two crop buttons without hooks (218–226), `{@render children?.()}` (227) | always outside a replay; `disabled={split !== null}` | Page (without the image) |
| Background image | `BackgroundImageSection.svelte:116` `div.background-image[data-section="background-image"]` (h4 `background.editor.title`), rendered as PagePanel's `children` (`Editor.svelte:2172–2182`) | `background-choose`, `background-switch-orientation`, `background-keep-orientation`, `background-turn-ccw\|cw`, `[data-slot="background-orientation-offer\|background-pdf\|background-image-name"]` | its own state: `pending` (a PDF page to choose), `busy`, `error`, `offer`, `draft` | Fond |
| Original | `OriginalPanel.svelte:58` `section.side-section[data-section="original"]` (`#editor-original-heading`), `Editor.svelte:2184–2186` (`{#key currentPath}`) | `[data-slot="original-help"]`, `original`, `original-confirm`, `original-cancel` | its own `confirming` | Page |
| Layers | `LayersZone.svelte:114` `section.layers-zone[data-section="layers"][data-where="editor"]`, h3 `layers.title` (115), `Editor.svelte:2187–2198` | as `tablet-live-classroom.md`, "The layers zone" | always outside a replay; `disabled={split !== null}` | Calques |
| Share button | `Toolbar.svelte:336` `<ShareButton />` (import :8) in the toolbar's `.share` group (`editor.share.label`); `ShareButton.svelte:15–30` `button.share-button[data-action="share"]`, `.share-indicator[data-viewers]` while a share runs; opens the `SharePanel` modal of `Home.svelte:453` (`share.openPanel()`) | `data-action="share"` | `share.status` (`lib/share-state.svelte.ts`) | Projection; leaves the toolbar (Q13) |

### Live control window

`LiveView.svelte:1082` `<aside class="live-side">` … `:1215`; CSS scoped: `.live-side` 290 px (1553–1559),
`.side-block` (1561–1576), `.keys`, `kbd` (1595–1619).

| Block | Where | Hooks | State | M16 |
|---|---|---|---|---|
| Layers | 1083–1096, `LayersZone where="live"` | `section.layers-zone[data-where="live"]`… | `live.layers`, `live.computerLayer`, `live.layer` | Calques |
| Projection | 1098–1147 `section.side-block` (`#live-projection-title`, `live.projection.title`) | screen `select` (1105, no hook: `LiveView.test.ts:285` reads `.live-side select`), `project` (1123), `stop-projecting` (1116), refresh (aria-label `live.projection.refresh`), `p[data-presentation]` (1138) | `monitors`, `monitorKey`, `monitorsLoaded`, `projecting`, `projectionError` (167–171), `live.presentation` | Projection, « Vidéoprojecteur » |
| Narration | 1149–1166 `section.side-block[data-section="narration"]` | RecordPanel (as the editor's), `[data-slot="narration-saved"][data-pages]` | `recorder` (135), `narrationOpen` (176), `narrated` (193) | Son |
| Zoom | 1168–1204 `section.side-block` (`#live-zoom-title`, `live.zoom.label`) | − and + (aria-labels `live.zoom.out\|in`), `[data-zoom]` (1179), `whole-page` (1190), `page-width` (1193), hint `live.preview.hint` (1197), `[data-slot="select-hint"]` (1198–1200, Select on), `[data-slot="pen-hint"]` (1201–1203, Pen on) | `live.view.zoom` | Projection, « Zone projetée »; the two tool hints go to the above block |
| Keys | 1206–1214 `section.side-block` (`#live-keys-title`, `live.keys.title`), `dl.keys` from `KEYS` (112–130) and the texts `live.keys.list.<id>.{keys,action}` | `dl.keys` (`LiveView.computer.test.ts:219`) | static | Aide |
| Toolbar: share | 1037–1039 `div.group[data-slot="share"]` + `<ShareButton />` (import :68) | `data-action="share"` | | Projection, « Partage sur le réseau »; leaves the toolbar (Q13) |
| Toolbar: blackboard, laser | 935–955 `button.toggle[data-action="board"\|"laser"]`, `aria-pressed` | | `live.view.board\|laser` | stay; also switches in Projection (O2) |
| Selection bar | 1059–1070 `div.select-bar[role="toolbar"][data-slot="select-bar"]`, floating over the preview (scoped CSS 1470–1490) | `.select-count[data-count]`, `delete-selection`, `deselect`, `live.select.colors` | `{#if selection.any}` (`selection`, 133) | above block |

The rest of the live toolbar stays as it is: the tablet's style, the pages and `new-page`, `clear-page`, the
computer's group `[data-slot="touch-up"]`, `[data-slot="orientation"]`.

### What each window lacks until M17

- Live: **Page** (colour, template and texture of the current page's paper) and **Fond** (an image or a PDF page under
  the current page). No tab at all in M16.
- Editor: **Projection** holds the share only (the projector block is M17); **Son** records only during a replay
  (« Pendant que j'écris » is M17).

So in M16 the editor's rail has six tabs, the live window's four (Calques, Son, Projection, Aide).

## 2. The shared component

### Files

Package 0 commits skeletons that honour the props and the DOM contract below (no style, no keyboard); package A
completes them.

- `components/side/SidePanel.svelte`: rail and pane.
- `components/side/SideIcon.svelte`: the mockup's inline SVG icons, `aria-hidden`, `currentColor`, 24×24 viewBox:
  `layers`, `page`, `background` (the mockup's `image`), `sound` (`mic`), `projection` (`projector`), `help` (`keys`),
  `collapse` (the chevron of the « › » button), `close` (✕).
- `components/side/ShortcutsHelp.svelte`: the Aide tab's content, from a table (§6).
- `components/share/ShareBlock.svelte`: the « Partage sur le réseau » block of both Projection tabs, around the
  unchanged `ShareButton`.
- `lib/side-panel.svelte.ts`: the tabs, one window's state, its persistence (§3).
- `components/side/side-test-helpers.ts`: vitest helpers for the windows' tests (§10).

### Model: `lib/side-panel.svelte.ts` (package 0, then A)

```ts
export type SideWhere = 'editor' | 'live';
export type SideTabId = 'layers' | 'page' | 'background' | 'sound' | 'projection' | 'help';
/** Every tab, in the rail's order; a window shows those it gives content for. */
export const SIDE_TABS: readonly SideTabId[] = ['layers', 'page', 'background', 'sound', 'projection', 'help'];
export type SideDot = 'warn' | 'rec' | 'live';
export interface SideBadge { count?: number; dot?: SideDot }
export type SideBadges = Partial<Record<SideTabId, SideBadge>>;
export interface SideView { tab: SideTabId; collapsed: boolean }
export const SIDE_DEFAULTS: Record<SideWhere, SideView> = {
  editor: { tab: 'layers', collapsed: false },
  live: { tab: 'layers', collapsed: false },
};
export function sideStorageKey(where: SideWhere): string;               // `ardoise.sidePanel.${where}`
export function loadSideView(where: SideWhere, storage?: Pick<Storage, 'getItem'> | null): SideView;
export function saveSideView(where: SideWhere, view: SideView, storage?: Pick<Storage, 'setItem'> | null): void;

/** One window's panel: created by the window, given to `SidePanel`, driven by both. */
export class SidePanelState {
  tab: SideTabId;          // $state: the tab chosen; may be one this window lacks (then `shown` gives the default)
  collapsed: boolean;      // $state
  constructor(where: SideWhere, storage?: Pick<Storage, 'getItem' | 'setItem'> | null);
  /** The tab shown among `available`: the chosen one, else the window's default, else the first. */
  shown(available: readonly SideTabId[]): SideTabId;
  /** A click (or Enter) on a tab: on the tab shown while open, collapses; otherwise opens that tab. */
  toggle(tab: SideTabId, available: readonly SideTabId[]): void;
  /** Opens the pane, on `tab` when given (a selection: none; the replay: 'sound'). */
  open(tab?: SideTabId): void;
  collapse(): void;
  snapshot(): SideView;
  restore(view: SideView): void;
}
```

Every method that changes `tab` or `collapsed` saves (§3).

### `SidePanel.svelte`

```ts
let { where, panel, badges = {}, above, layers, page, background, sound, projection, help }: {
  where: SideWhere;
  panel: SidePanelState;
  badges?: SideBadges;
  /** Shown above the open tab, whatever the tab; given only while there is something to show (§5). */
  above?: Snippet;
  layers?: Snippet; page?: Snippet; background?: Snippet; sound?: Snippet; projection?: Snippet; help?: Snippet;
} = $props();
```

- **How a window gives a tab's content.** Snippets declared as direct children of `<SidePanel>` are passed as its props
  of the same name (Svelte 5). A tab exists in a window if and only if its snippet is given; `available` is
  `SIDE_TABS` filtered by the snippets given. No events: the component calls `panel.toggle(id, available)` on a tab and
  `panel.collapse()` on « › »; the window calls `panel.open(…)`.

  ```svelte
  <SidePanel where="live" panel={side} {badges} above={selection.any || selection.active || penOn ? aboveBlock : undefined}>
    {#snippet layers()}<LayersZone where="live" … />{/snippet}
    {#snippet sound()}…{/snippet}
    {#snippet projection()}<ProjectorBlock … /><ZoneBlock …>…</ZoneBlock><ShareBlock />{/snippet}
    {#snippet help()}<ShortcutsHelp where="live" shortcuts={LIVE_SHORTCUTS} />{/snippet}
  </SidePanel>
  {#snippet aboveBlock()}…{/snippet}
  ```
- **Tab contents stay mounted.** Every tab panel is rendered; the ones not shown carry `hidden`, and so does the pane
  when collapsed. Reasons: what a block holds for the moment survives a tab switch and a collapse (the PDF page being
  chosen in Fond, a layer being renamed, the original's confirmation, sizes being typed, an open recording panel); the
  vitest tests that reach moved content with `querySelector` and `.click()` keep working (jsdom ignores `hidden`); no
  block is heavy. The consequence for the real window: a hidden control is in the DOM but cannot be clicked, so a
  journey opens the tab before acting (§10, risk R1).

### DOM contract (frozen by package 0)

```html
<div class="side-panel" data-side-panel="editor|live" data-tab="{shown}" data-collapsed="true|false">
  <div class="side-rail" role="tablist" aria-orientation="vertical" aria-label="{side.tablist}">
    <button type="button" role="tab" class="side-tab" id="side-tab-{where}-{id}" data-tab="{id}"
            aria-selected="{id === shown}" aria-expanded="{id === shown && !collapsed}"
            aria-controls="side-tabpanel-{where}-{id}" tabindex="{id === shown ? 0 : -1}"
            title="{id === shown && !collapsed ? side.openTitle : side.tabs.<id>}"
            data-count="{n}" data-dot="warn|rec|live">      <!-- data-count, data-dot only when there is one -->
      <SideIcon name="{id}" />
      <span class="visually-hidden">{side.tabs.<id>}{, side.badge.<id>}{, side.dot.<dot>}</span>
      <span class="side-tab-count" aria-hidden="true">{n}</span>              <!-- with a count -->
      <span class="side-tab-dot" data-kind="{dot}" aria-hidden="true"></span>   <!-- with a dot -->
    </button>
    …
  </div>
  <div class="side-pane" hidden="{collapsed}">
    <div class="side-pane-head">
      <h2 class="side-pane-title" data-slot="side-title">{side.tabs.<shown>}</h2>
      <button type="button" class="side-fold" data-action="side-collapse" aria-label="{side.collapse}" title="{side.collapse}">
        <SideIcon name="collapse" /></button>
    </div>
    <div class="side-above" data-slot="side-above">{above}</div>             <!-- only when `above` is given -->
    <div class="side-pane-body">
      <div class="side-tabpanel" role="tabpanel" id="side-tabpanel-{where}-{id}" aria-labelledby="side-tab-{where}-{id}"
           data-tab-panel="{id}" hidden="{id !== shown}">{snippet}</div>
      …
    </div>
  </div>
</div>
```

The windows keep their `aside`: `aside.editor-side` and `aside.live-side`, both with `aria-label={t('side.label')}`,
around the component (journeys and tests select `.editor-side …` and `aside.live-side`). A may add elements and
classes, never rename or remove these.

### Keyboard and focus

- WAI-ARIA tabs with manual activation: `role="tablist"` (vertical), `role="tab"`, `role="tabpanel"`. The mockup's
  toggle buttons (`aria-pressed`) were set aside: a screen reader would not tie a button to its panel. The collapse is
  `aria-expanded` on the selected tab (ARIA 1.2 lists `aria-expanded` among the states of `tab` (supposé: package 0
  confirms with `npm run check`, whose a11y rules follow aria-query; if it warns, drop `aria-expanded` and add « replié »
  / « collapsed » to the tab's hidden text, `side.collapsed`)).
- Roving focus: only the shown tab has `tabindex="0"`. On the tablist, ArrowDown / ArrowUp move the focus to the next /
  previous tab (wrapping), Home / End to the first / last; they do not activate (activating on a move would reopen a
  collapsed pane at each press). The handler calls `preventDefault()` for these four keys only, never
  `stopPropagation()`.
- Enter activates the focused tab in both windows, Space in the editor (native button click). **In live mode Space is
  always « next page », even on a focused button** (`lib/live-keys.ts:23–26`; `LiveView.svelte:750–752` cancels the
  keyup so that the button is not clicked; M10 decision): a focused tab is not activated by Space there, by design, and
  the rail must not handle Space itself. The live help says so (`side.help.typingLive`).
- No clash with the windows' keys: ArrowUp, ArrowDown, Home and End are unused by `liveKeyAction` without Shift
  (`live-keys.ts:91–139`) and by the editor (`Editor.svelte:1720–1841`); ArrowLeft / ArrowRight (pages in live, Alt + ←
  / → in the editor) are left to the window. The live handler already returns on `e.defaultPrevented` (`:721`).
  `spaceActivates` (`Editor.svelte:1659–1674`) already lists `role="tab"`, so Space on a focused tab never pans.
- Focus: a click or Enter on a tab keeps the focus on it, whether it opens or collapses. « › » disappears with the pane:
  after `tick()` the focus goes to the shown tab. A selection that reopens the pane, and the replay that shows Son, move
  no focus. DOM order is rail then pane (`flex-direction: row-reverse` draws the pane left of the rail): Tab reaches the
  tabs, then the open tab's content.

### Collapsed state, layout, narrow windows

- Collapsed: the pane has `hidden`, the rail stays, the stage takes the width (`Canvas`'s root `.stage` is `flex: 1`,
  `Canvas.svelte:709`, in the flex row `.editor-main`, `app.css:1496–1500`; `.live-preview` is `flex: 1`, `LiveView.svelte:1462–1468`). The editor's view keeps its scale
  and origin on a resize (`Canvas.svelte:268–274` only measures; `fit()` runs on a change of page size, 255–265): the
  page does not jump, more of it shows (O14). The live preview measures and redraws itself (`LiveCanvas.svelte:332`).
- Widths: pane 290 px, 240 px when the window is ≤ 900 px wide (today: editor 270 / 220, live 290); rail 48 px, tabs
  38 × 38 (mockup). The main window is at least 800 × 560 (`app/src-tauri/src/lib.rs:404`): at 800 px the stage keeps
  512 px with the pane open, so no overlay mode is needed. The pane's body scrolls (`overflow: auto`, `min-height: 0`);
  the rail does not (six tabs need about 270 px).
- `hidden` and CSS: app.css has no `[hidden] { display: none }` rule and an author `display` beats the browser's
  `hidden`. Write `.side-pane:not([hidden]) { display: flex; … }`, and the same for any element that toggles `hidden`
  and has a display rule.

### Look: tokens and class names

- New token (package 0, `app.css`): `--accent-soft`, light `#e6eefc`, dark `#23314a` (the mockup's), in the three blocks
  (`:root` 1–22, the dark media query 24–42, `:root[data-theme='dark']` 44–60). Everything else exists: `--surface`,
  `--surface-2`, `--border`, `--muted`, `--accent`, `--accent-text`, `--warn`, `--warn-bg` (13–14), `--danger`. Dots:
  orange `--warn`, red `--danger` (the mockup's `--rec`, `#d92d20` / `#f97066`, is `--danger` in dark and close to it in
  light: no new token), blue `--accent`. Selected tab: `--accent-soft` background, `--accent` icon, a 3 px `--accent`
  bar on its left (mockup `.tab.on::before`). The above block: `--accent-soft` background (mockup `.selection`).
- **Collisions.** In M15 the global `.panel` rule (`app.css:306–315`, `position: absolute; inset: 0 0 0 auto`) squashed
  `StraightenPanel` to a 14 px strip in the real window while jsdom, which applies no stylesheet, saw nothing (commit
  49d2dd9). app.css also styles, globally, `.count` (570), `.actions` (480), `.chosen` (232), `.badge` (1160),
  `.selection-bar` (809), `.selection-panel` (834), `.icon-button` (580), `.check` (352), `.editor .buttons` (1357),
  `.side-section .row`, `.banner.notice`. So none of the mockup's names (`.panel`, `.panel-head`, `.tab`, `.count`,
  `.dot`, `.block`, `.row`, `.selection`, `.icon`, `.switch`, `.swatch`, `.actions`, `.notice`) is used as is.
  - Shell, scoped in `SidePanel.svelte`: `side-panel`, `side-rail`, `side-tab`, `side-tab-count`, `side-tab-dot`,
    `side-pane`, `side-pane-head`, `side-pane-title`, `side-fold`, `side-above`, `side-pane-body`, `side-tabpanel`.
  - Block look, for the windows' blocks inside the pane, defined once in `SidePanel.svelte` as
    `.side-pane :global(.pane-…)` (they apply only inside a pane and cannot leak): `pane-block` (padding 0.75rem 1rem,
    bottom border, column, gap 0.55rem; the last one without border), `pane-title` (the discreet block title, an `h3`:
    0.72rem, uppercase, letter-spacing 0.06em, `--muted`, 700), `pane-row` (label on the left, control on the right,
    0.875rem), `pane-help` (0.8rem, `--muted`), `pane-actions` (a wrapping row of buttons), `pane-switch` (a checkbox
    drawn as a switch), `pane-swatches` / `pane-swatch` (round colour pastilles, their name under), `pane-value` (a
    slider's value, tabular figures), `dl.keys` and `kbd` (the help).
  - None of these names is used today (checked with `grep` over `app.css` and `components/**/*.svelte`). A greps again
    before merging. `side-section` (global) and `side-block` (scoped in LiveView) go with the blocks that used them
    (B, C), the h2 rules of app.css (79–90, `white-space: nowrap`) are overridden by `.side-pane-title`.
- **Switches** are `<input type="checkbox" role="switch" aria-checked={checked} class="pane-switch">` inside a
  `label.pane-row`, not buttons: the tests that read `input[type="checkbox"]` (`data-field="scale-ink"`, the texture)
  keep working, and `appearance: none` draws the mockup's `.switch`. (`aria-checked` repeated for svelte-check's rule
  that a `switch` must have it: supposé, package 0 checks.)
- The mockup's refresh, item by item: discreet block titles → `pane-title`; paper colour → swatches (B); options →
  switches: texture and « scale the writing » (B), blackboard and laser in Projection (C); sliders with their value →
  the background image's opacity keeps its value in `pane-value` (it shows one already); the live zoom stays − / value /
  + (O9); icon buttons → the rail, « › », the « ✕ » of the selection block, the rotate buttons (⟲ ⟳ already).
- Dark theme: tokens only. The paper swatches show the paper's own colour (`paperRgb`, `lib/live-render.ts:134`, the
  renderer's colours) ringed with `--border`.

### Help, share block

- `ShortcutsHelp.svelte` (§6): props `{ where: SideWhere; shortcuts: readonly Shortcut<object>[] }`. Renders
  `section.pane-block[data-section="shortcuts"]`: `h3.pane-title` (`side.help.editor` / `side.help.live`), `dl.keys`
  of the entries without group, then one more `h4` + `dl.keys` per group present (`replay`: `side.help.replay`;
  `presentation`: `side.help.presentation`), then `p.pane-help` (`side.help.typingEditor` / `side.help.typingLive`).
  Each entry: `<dt data-shortcut={id}>` with its shown chords (one per display, duplicates merged, separated by « , »),
  each chord as `<kbd>Ctrl</kbd> + <kbd>Z</kbd>`, or the text of `keysLabel`; `<dd>{t(label)}</dd>`.
- `ShareBlock.svelte`: no props. `section.pane-block[data-section="share"]`: `h3.pane-title` (`share.block.title`),
  `p.pane-help[data-slot="share-status"]` (`share.block.idle`, or `tn('share.block.active', viewers)` while a share
  runs, plus `share.paused` when paused), then `<ShareButton />` unchanged (its look, checked by
  `journeys-m11-share.mjs:1036–1054`, does not change; its doc comment says where it lives now).

## 3. Persistence

- In the browser's storage of the WebView2 profile, like the other per-PC choices of the interface:
  `ardoise.touch.drawWithFinger` (`lib/touch-prefs.svelte.ts:9`), `ardoise.live.computerStyle`
  (`lib/live-computer-style.ts:13`), `ardoise.export` (`lib/export-options.ts:62`). Not in `ardoise.toml`, which the PCs
  share through Nextcloud, while Q13 says « sur chaque PC ».
- Keys `ardoise.sidePanel.editor` and `ardoise.sidePanel.live`; value JSON `{"tab":"page","collapsed":false}`.
- Defaults: `{ tab: 'layers', collapsed: false }` in both windows (Q13: « Le direct s'ouvre sur Calques »; the editor's
  is the mockup's starting state, O5). Whether live always opens on Calques or only by default: O1.
- Read field by field, as `loadComputerStyle`: a `tab` not in `SIDE_TABS` gives the default tab, a `collapsed` that is
  not a boolean gives `false`, garbage or a blocked storage gives the defaults. Written by every change of
  `SidePanelState` (a tab, « › », `open()` from a selection or the replay, `restore()`); a blocked storage is ignored
  (the state holds for the session).
- **A remembered tab the window lacks** (live in M16: `page`, `background`): the window shows its default tab
  (`shown()`), and the stored value stays as it is until the user picks a tab; a choice made once M17 has the tab is not
  lost by a window without it.
- One state per mounted window: the editor keeps its `SidePanelState` from one note to the next (Alt + ←/→ keeps the
  component), and reads the storage again when it is mounted again.
- The journeys start on a fresh profile each (`run.mjs:103–108`: `LOCALAPPDATA` in the journey's folder), so on the
  defaults. Component tests clear the storage in `beforeEach` (as `Editor.test.ts:52–58` does).

## 4. Badges

The `badges` prop, derived by each window:

| Tab | Badge | Editor | Live |
|---|---|---|---|
| Calques | count | `doc.note.layers.length` | `live.layers.length` |
| Calques | orange dot | `layerWarnings(doc.note.layers, doc.activeLayer).length > 0` (the chosen layer is hidden) | `layerWarnings(live.layers, live.computerLayer, live.layer).length > 0` (the tablet's or the chosen layer is hidden) |
| Son | red dot | `recorder.phase === 'recording'` (`Editor.svelte:271`) | `recorder.phase === 'recording'` (`LiveView.svelte:135`) |
| Projection | blue dot | `share.status.active` (M17: `\|\| projecting`) | `live.presentation !== null \|\| share.status.active` |
| Page, Fond, Aide | none | | |

- `layerWarnings` (`lib/layers-zone.ts:33`) is what the zone's warnings use (`LayersZone.svelte:52`): the dot and the
  zone's notice cannot disagree.
- Only `recording`, not `starting` or `saving` (`components/narration/recorder.svelte.ts:41–104`: short phases); a
  paused share is still a share.
- Visible collapsed too (their point): the count bottom right (`.side-tab-count`, `--muted` on `--surface-2`), one dot
  top right (`.side-tab-dot`, ringed with `--surface`). For assistive technology the tab's hidden text reads « Calques,
  2 calques, un calque utilisé est masqué ». For tests: `data-count`, `data-dot` on the tab.

## 5. The block above the open tab

The `above` snippet renders into `.side-above[data-slot="side-above"]`, between the pane's head and the open tab,
whatever the tab; it hides with the pane.

**Editor.**
- Contents, in this order: `SplitPanel` while `split` (the component unchanged but for its look: it leaves the list of
  blocks), then the selection block while `hasSelection`: `section.pane-block[data-section="selection"]`
  (`aria-labelledby="editor-selection-heading"`), the discreet title `editor.selection.label`, the count
  (`editor.selection.count`) with a « ✕ » `data-action="clear-selection"` (`aria-label` and `title`
  `editor.selection.clear`, « Désélectionner »: the tests' `button(label)` helper, `Editor.test.ts:116–121`, finds it by
  the old button's text) beside it, then the buttons `copy-selection`, `cut-selection`, `paste-selection`, `duplicate-selection` (new hook),
  `straighten-selection`, `delete-selection` (new hook, danger). The selection's style changes stay in the toolbar.
- Reopening: when `hasSelection` turns true (not at every change of the selection) and when a split starts
  (`toggleSplit`), `side.open()`: the tab shown, pane expanded. The mockup's recolour pastilles in the block are not
  added (O8).

**Live window.**
- Contents: while `selection.any`, the selection bar taken from over the preview (`LiveView.svelte:1059–1070`) with
  `[data-slot="select-bar"]`, `role="toolbar"`, `.select-count[data-count]`, `delete-selection`, `deselect` and
  `live.select.colors` kept (`journeys-m12-touchup.mjs:60`, `perf.mjs:456`, the LiveView select, review and computer
  tests rely on them); while Select is on, `p[data-slot="select-hint"]` (`live.select.help`); while Pen is on,
  `p[data-slot="pen-hint"]` (`live.pen.help`). The hints are in the zoom block today (1198–1203), which goes to
  Projection, out of sight while live shows Calques (O4).
- Reopening: when `selection.any` turns true, `side.open()`, as in the editor (SPEC: « La sélection s'affiche au-dessus
  de l'onglet ouvert »; the mockup showed it in the editor only: O3). The tool hints do not reopen the pane.
- The preview loses the bar's floating CSS (1470–1490).

## 6. The help tab and the keys

### Keys handled today

- **Editor**: `onkeydown` (`Editor.svelte:1720–1841`), `replayKey` (1692–1718), the helpers `isTyping` (1650),
  `spaceActivates` (1659), `spacePlays` (1676), `isFitWidthKey` (1682), `isFitKey` (1687), `onkeyup` (1843); Ctrl + W /
  F4 through `closeKey` (`lib/close-keys.ts:11–16`); Ctrl + Q in `App.svelte:43–47`, for every screen. The gates of
  `onkeydown`, in their order: the close keys computed (`closeKey` and no modal); no doc → close only; Ctrl + S; the
  leave question → Esc stays; a dialog or the share panel → nothing; the split → Esc or a close key cancels it; the
  shapes panel → Esc closes it; a close key → leave the replay, else back; a text field → nothing more; Alt + ← / →;
  a gesture under way → Ctrl keys, Delete, Backspace and Esc wait; in a replay `replayKey`; Ctrl + C / X / V; the other
  Ctrl keys; Alt → nothing; the plain keys.
- **Live**: `liveKeyAction` (`live-keys.ts:79–140`) and `liveEditKeyAction` (163–177), run by `LiveView.onkeydown`
  (719–747): the touch-up keys first while a session runs, then `liveKeyAction` through `runKeyAction`
  (`lib/live-actions.ts:73–104`); Ctrl + Q in `App.svelte`.
- The guide's chapter 17 (`docs/guide/guide.fr.md`, « Raccourcis clavier ») lists the same keys.

**Two findings.**
- The live list today (`KEYS`, `LiveView.svelte:112–130`, texts written by hand in `live.keys.list.<id>.keys`) shows
  « Suppr, Retour arrière : effacer les annotations » (`marks`) and, for `delete`, « sans sélection : effacer les
  annotations ». In the control window those keys do nothing without a selection: `liveKeyAction` gives `clearMarks`
  (`live-keys.ts:132–134`) and `runKeyAction` calls `actions.clearMarks?.()`, which the control window does not define
  (`LiveView.svelte:239–245`; only `Presentation.svelte:55` does). They are keys of the presentation window: M16 lists
  them in a group « Dans la fenêtre projetée » and corrects the texts (§9, O10).
- Chords act that no list shows: Shift + a letter (V, P, E, H, R, F in the editor, whose key is lower-cased at 1731
  with no Shift test; V and P in live, `live-keys.ts:165–173`), Ctrl + Shift + S, A, Y, 0, 2 in the editor (1732,
  1786–1800 test no Shift), `=` for `+`, `Spacebar`. They stay as they are (harmless; keyboard layouts) and the tables
  list them as not shown.

### One source, checked both ways

Decision: one declarative table per window, which the help renders and which tests confront with the key handling **in
both directions**; not a table-driven dispatcher.

- Why not a dispatcher: `liveKeyAction` carries tested subtleties (repeat for pan and zoom only, Shift tolerated for
  B, L, W, Page Up and Page Down, key codes for 0 and the keypad, Space on buttons, the close keys before the field
  test) that a generic matcher would have to re-encode in the classroom's most used path; the editor's handler weaves
  its keys with gates. Rewriting both is a risk for no gain once the tests below exist.
- Why it cannot be wrong: the sweep fails when a chord in the table does something else (or nothing) in the context it
  is listed for, **and** when a chord the window handles is in no entry. The probes fail when a shown shortcut has no
  effect in the mounted window. That is ROADMAP's « une seule liste … que les tests confrontent aux touches traitées »
  and « chaque raccourci affiché agit vraiment dans sa fenêtre ».
- What it costs: the editor's mapping must run without the component. B2 extracts it, pure, into `lib/editor/keys.ts`
  (as live's is in `live-keys.ts`); the gates stay in `Editor.svelte`, in their order.

### Types: `lib/shortcuts.ts` (package 0, then A)

```ts
/** A key press as the tables write it. `shift: 'any'` accepts it with and without Shift. */
export interface Chord { key: string; code?: string; ctrl?: boolean; shift?: boolean | 'any'; alt?: boolean }
export interface ShortcutKey<C> {
  chord: Chord;
  /** What the window's mapping gives for it (the `does` words of §6 tables). */
  does: string;
  /** The contexts where it does that (each field given must match); absent: every context. */
  in?: Partial<C>;
  /** Shown in the help (default true); false: a variant the mapping takes too (Shift + V, `=` for +…). */
  shown?: boolean;
}
export interface Shortcut<C> {
  id: string;
  /** i18n key of what it does. */
  label: string;
  keys: readonly ShortcutKey<C>[];
  /** i18n key shown instead of the chords (« Maj + flèches »). */
  keysLabel?: string;
  /** Help group; absent: the window's main list. */
  group?: 'replay' | 'presentation';
}
export interface EditorKeyContext { replaying: boolean }
export interface LiveKeyContext { running: boolean; selection: boolean }
/** Whether `e` is the chord (single letters compared without case; `code` compared when given; Ctrl = Ctrl or Meta). */
export function chordMatches(chord: Chord, e: KeyInput): boolean;
/** The concrete key presses of a chord (two with `shift: 'any'`), with the US key code. */
export function chordEvents(chord: Chord): KeyInput[];
/** The parts of a chord as the help shows them, translated: ['Ctrl', 'Z'], ['Maj', '→']… (A). */
export function chordParts(chord: Chord): string[];
```

`KeyInput` is `lib/live-keys.ts`'s. Key names: `shortcuts.keys.*` (§9); a single letter shows upper-case, `-` shows
as « − », `' '` as « Espace ».

### The sweep: `lib/shortcuts-sweep.ts` (package 0, then A)

```ts
export const SWEEP_KEYS: readonly string[];     // a–z, 0–9, ' ', '+', '-', '=', '−', the four arrows, PageUp, PageDown,
                                                // Home, End, Delete, Backspace, Escape, Enter, Tab, F1–F12
export const SWEEP_MODIFIERS: readonly { ctrl?: true; shift?: true; alt?: true }[];
                                                // {}, Shift, Ctrl, Ctrl + Shift, Alt, Alt + Shift (Ctrl + Alt, AltGr, not swept)
/** Every disagreement between `table` and `does` over `contexts`, as sentences; [] when they agree. */
export function sweepShortcuts<C extends object>(
  table: readonly Shortcut<C>[],
  contexts: readonly C[],
  does: (e: KeyInput, context: C) => string | null,
): string[];
```

- Forward: for each key of each entry, for each context matching its `in`, for each event of `chordEvents`,
  `does(e, context) === key.does`.
- Backward: for each key of `SWEEP_KEYS` × `SWEEP_MODIFIERS` × context (letters upper-case with Shift, US codes:
  `KeyV`, `Digit0`, `Space`, `Equal`, `Minus`; `repeat: false`; `target: null`), when `does` is not null, some table
  key whose chord matches, whose `in` matches and whose `does` is the same exists.

### Editor: `lib/editor/keys.ts` (package 0 type and stub, then B2) and its wiring (B1)

```ts
export type EditorKeyKind =
  | 'close' | 'save' | 'previousPage' | 'nextPage'
  | 'leaveReplay' | 'playPause'
  | 'copy' | 'cut' | 'paste' | 'undo' | 'redo' | 'selectAll' | 'fit' | 'fitWidth'
  | 'delete' | 'deselect' | 'panHold' | 'select' | 'pen' | 'eraser' | 'pan' | 'replay' | 'straighten';
export interface EditorKeyAction { kind: EditorKeyKind }
/** What a key press asks of the editor; no gate (dialogs, fields, gestures): the editor applies them. */
export function editorKeyAction(e: KeyInput, context: EditorKeyContext): EditorKeyAction | null;
```

Mapping, today's behaviour exactly (`key` = `e.key` lower-cased when it is one character, `mod` = Ctrl or Meta):
1. `closeKey(e) === 'close'` → `close`.
2. `mod && key === 's'` → `save`.
3. Alt, no Ctrl, no Shift, ← / → → `previousPage` / `nextPage`.
4. In a replay: with `mod`, `isFitKey(e)` → `fit`, `isFitWidthKey(e)` → `fitWidth`, else null; with Alt, null; `r` or
   `Escape` → `leaveReplay`; `' '` → `playPause`; else null.
5. `mod`, no Shift, no Alt: `c`, `x`, `v` → `copy`, `cut`, `paste`.
6. `mod`: `isFitKey` → `fit`, `isFitWidthKey` → `fitWidth` (checked first, as 1795 does), `z` → `redo` with Shift else
   `undo`, `y` → `redo`, `a` → `selectAll`; else null.
7. Alt → null.
8. `Delete`, `Backspace` → `delete`; `Escape` → `deselect`; `' '` → `panHold`; `v`, `p`, `e`, `h` → `select`, `pen`,
   `eraser`, `pan`; `r` → `replay`; `f` → `straighten`; else null.

`isFitKey`, `isFitWidthKey` move into the module (exported). The editor's `onkeydown` becomes, gates unchanged and in
the same order:

```ts
function onkeydown(e: KeyboardEvent) {
  const action = editorKeyAction(e, { replaying });
  const closes = action?.kind === 'close' && !modalOpen();
  if (!doc) { if (closes) { e.preventDefault(); back(); } return; }
  if (action?.kind === 'save') { e.preventDefault(); if (dirty) void save(); return; }
  if (leaving) { if (e.key === 'Escape') stay(); return; }
  if (exporting || printing || video || share.panelOpen || revisionDialog) return;
  if (split) { if (e.key === 'Escape' || closes) { e.preventDefault(); cancelSplit(); } return; }
  if (shapesPanel && e.key === 'Escape' && !(e.ctrlKey || e.metaKey)) { e.preventDefault(); closeShapes(true); return; }
  if (closes) { e.preventDefault(); if (canvas?.dragging()) return; if (replaying) leaveReplay(); else back(); return; }
  if (isTyping(e.target)) return;
  if (action?.kind === 'previousPage' || action?.kind === 'nextPage') {
    e.preventDefault(); if (!canvas?.dragging()) void goPage(action.kind === 'previousPage' ? -1 : 1); return;
  }
  if (canvas?.dragging() && (e.ctrlKey || e.metaKey || ['Delete', 'Backspace', 'Escape'].includes(e.key))) { e.preventDefault(); return; }
  if (action) runEditorKey(e, action);
}
```

`runEditorKey` does what the replaced code did: `leaveReplay` (preventDefault, `leaveReplay()`); `playPause` (when
`spacePlays(e.target)`: preventDefault, `player.toggle()` unless `recorder.busy`); `fit` / `fitWidth` (preventDefault,
`canvas?.fit()` / `fitWidth()`); `copy` / `cut` / `paste` (`expectClipboardEvent(kind)`, no preventDefault);
`undo`, `redo`, `selectAll` (preventDefault, `doUndo`, `doRedo`, `reselect(selectAll(doc))`); `delete` (with a
selection: preventDefault, `change(deleteSelection(doc))`); `deselect` (`reselect(clearSelection(doc))`); `panHold`
(unless `spaceActivates(e.target)`: preventDefault, `spaceDown = true`); `select`, `pen`, `eraser`, `pan`
(`setMode`); `replay` (`enterReplay()`); `straighten` (`straighten()`); `close`, `save`, `previousPage`, `nextPage`:
nothing (they were handled above, or a modal is open and the key is the modal's, as today); an exhaustive `switch`
(`satisfies never`).

### The tables (package 0, then B2 for the editor's, C for live's)

Rows in help order. « → x » is `does`; [ ] the context (`in`); « hidden » are keys with `shown: false`. Read from the
code (§6 "Keys handled today"); the sweeps of B2 and C are the check.

**`lib/editor/shortcuts.ts`**: `EDITOR_SHORTCUTS`, `EDITOR_KEY_CONTEXTS = [{ replaying: false }, { replaying: true }]`;
label keys `editor.keys.list.<id>`.

| id | Shown | Hidden | Context |
|---|---|---|---|
| `select` | V → select | Shift + V | [replaying: false] |
| `pen` | P → pen | Shift + P | [false] |
| `eraser` | E → eraser | Shift + E | [false] |
| `pan` | H → pan | Shift + H | [false] |
| `panHold` | Space → panHold | Shift + Space | [false] |
| `straighten` | F → straighten | Shift + F | [false] |
| `replay` | R → replay | Shift + R | [false] |
| `undo` | Ctrl + Z → undo | | [false] |
| `redo` | Ctrl + Y, Ctrl + Shift + Z → redo | Ctrl + Shift + Y | [false] |
| `selectAll` | Ctrl + A → selectAll | Ctrl + Shift + A | [false] |
| `clipboard` | Ctrl + C → copy, Ctrl + X → cut, Ctrl + V → paste | | [false] |
| `delete` | Delete, Backspace → delete | Shift + Delete, Shift + Backspace | [false] |
| `deselect` | Esc → deselect | Shift + Esc | [false] |
| `fit` | Ctrl + 0 → fit | Ctrl + Shift + 0; Ctrl (+ Shift) + `à` with code `Digit0`; Ctrl + `0` with code `Numpad0` | every context |
| `fitWidth` | Ctrl + 2 → fitWidth | Ctrl + Shift + 2; Ctrl (+ Shift) + `é` with code `Digit2`; code `Numpad2` | every context |
| `pages` | Alt + ← → previousPage, Alt + → → nextPage | | every context |
| `save` | Ctrl + S → save | Ctrl + Shift + S | every context |
| `close` | Ctrl + W, Ctrl + F4 → close | | every context |
| `quit` | Ctrl + Q → quit | | every context |
| `leaveReplay` (group `replay`) | R, Esc → leaveReplay | Shift + R, Shift + Esc | [replaying: true] |
| `playPause` (group `replay`) | Space → playPause | Shift + Space | [true] |

The editor's `does` for the sweep: `closeKey(e) === 'quit' ? 'quit' : editorKeyAction(e, context)?.kind ?? null`.

**`lib/live-shortcuts.ts`**: `LIVE_SHORTCUTS`, `LIVE_KEY_CONTEXTS = [{ running: false, selection: false }, { running:
true, selection: false }, { running: true, selection: true }]` (a selection exists only while a session runs:
`LiveView.svelte:262–269`); label keys `live.keys.list.<id>.action` (kept).

| id | Shown | Hidden | Context |
|---|---|---|---|
| `next` | →, Page Down, Space → nextPage | Shift + Page Down; `Spacebar` | every context |
| `previous` | ←, Page Up → previousPage | Shift + Page Up | every context |
| `clear` | Ctrl + Delete → clearPage | Ctrl + Backspace | every context |
| `board` | B → board | Shift + B | every context |
| `laser` | L → laser | Shift + L | every context |
| `select` | V → select | Shift + V | [running: true] |
| `pen` | P → pen | Shift + P | [running: true] |
| `delete` | Delete, Backspace → deleteSelection | | [running: true, selection: true] |
| `undo` | Ctrl + Z → undo, Ctrl + Y → redo | Ctrl + Shift + Z → redo | [running: true] |
| `zoom` | + (`shift: 'any'`) → zoomIn, − (key `-`, `shift: 'any'`) → zoomOut | `=` (any) → zoomIn, `−` U+2212 (any) → zoomOut | every context |
| `whole` | 0 (any) → wholePage | `à` with code `Digit0` (any); `0` with code `Numpad0` | every context |
| `width` | W → fitWidth | Shift + W | every context |
| `pan` | `keysLabel: 'shortcuts.keys.shiftArrows'`; Shift + ←, →, ↑, ↓ → pan | | every context |
| `leave` | Esc → deselect [running, selection]; Esc → leave [selection: false] (shown once) | | as given |
| `close` | Ctrl + W, Ctrl + F4 → leave | | every context |
| `quit` | Ctrl + Q → quit | | every context |
| `marks` (group `presentation`) | Delete, Backspace → clearMarks | | [selection: false] |

Live's `does`: the touch-up keys first as `LiveView.onkeydown` does (`context.running ? liveEditKeyAction(e,
context.selection)?.kind`), then `closeKey(e) === 'quit'` → `quit`, then `liveKeyAction(e)`: `command` → its command
(`nextPage`, `previousPage`, `clearPage`), `zoom` → `zoomIn` / `zoomOut`, the others their `kind` (`board`, `laser`,
`wholePage`, `fitWidth`, `clearMarks`, `pan`, `leave`).

### Tests of the help

- B2: `lib/editor/keys.test.ts` (each branch of the mapping and today's subtleties: Ctrl + `à` with `Digit0`, Ctrl +
  Shift + Z, Alt + Shift + ← ignored, the replay keys, Ctrl + Shift + C does nothing) and `lib/editor/shortcuts.test.ts`:
  `sweepShortcuts(EDITOR_SHORTCUTS, EDITOR_KEY_CONTEXTS, does)` is `[]`.
- C: `lib/live-shortcuts.test.ts`, the same for live.
- A: `lib/shortcuts.test.ts` (`chordMatches`, `chordEvents`, `chordParts` in FR and EN), `lib/shortcuts-sweep.test.ts`
  (a table with a wrong `does`, a missing chord and a missing context each give their sentence), `ShortcutsHelp.test.ts`
  (every entry rendered with its shown chords, duplicates merged, `keysLabel`, the groups, FR and EN).
- B1: `Editor.shortcuts.test.ts`; C: `LiveView.shortcuts.test.ts`. A `PROBES` record keyed by entry id; a first test
  fails when an entry of the window's table (live: not the `presentation` group, which `Presentation.marks.test.ts`
  covers) has no probe; each probe mounts the window, sets what the shortcut needs (a selection, a replay, a running
  session, a note with a next one in its binder…), dispatches the entry's first shown chord on `window`, and checks its
  effect (the toolbar's pressed tool, `api.saveNote` called, the page shown, `share.panelOpen`, `live.view.board`,
  `canvas`'s view…). Where an effect is not visible in jsdom (Space held), the probe reads what the window gives
  `Canvas` (`spacePan`) through the harness.

## 7. Editor in M16 (packages B1, B2)

- `Editor.svelte`: `const side = new SidePanelState('editor')`; `<aside class="editor-side" aria-label={t('side.label')}>`
  around `<SidePanel where="editor" panel={side} {badges} above={split || hasSelection ? aboveBlock : undefined}>`, with
  `locked = split !== null || replaying` given as `disabled` to every tab's content that edits the note (the replay is
  read-only: until M16 it hid those blocks):
  - `layers`: `LayersZone` as today (`disabled={locked}`).
  - `page`: `PagePanel` (no `children` any more), then `OriginalPanel` in its `{#key currentPath}`.
  - `background`: `BackgroundImageSection`.
  - `sound`: `section.pane-block[data-section="narration"]` with the title `narration.title`; during a replay today's
    content (2056–2097: stored line, RecordPanel, delete); outside one, `p.pane-help` `editor.sound.replayFirst` and a
    button `data-action="sound-replay"` (`editor.sound.startReplay`) that calls `enterReplay()`, disabled as the
    toolbar's replay button (`split !== null || recorder.busy`). Recommended in a new `components/editor/EditorSound.svelte`
    (M17 adds its « Pendant que j'écris » there).
  - `projection`: `<ShareBlock />`.
  - `help`: `<ShortcutsHelp where="editor" shortcuts={EDITOR_SHORTCUTS} />`.
  - `aboveBlock`: §5.
- The replay shows Son (O6): `enterReplay()` (1473) keeps `side.snapshot()` and calls `side.open('sound')`;
  `resetReplay()` (1494) calls `side.restore(kept)` when it kept one. A split starting and a selection appearing call
  `side.open()` (§5).
- Badges: §4.
- Keys: wire `editorKeyAction` (B2) as in §6, once B2 is merged.
- `Toolbar.svelte`: `<ShareButton />` (336) and its import (8) go; the `.share` group keeps replay, split, copy,
  export, print, video, resume.
- `PagePanel.svelte`: no `children`; three blocks instead of one section:
  `section.pane-block[data-section="paper"]` (`h3.pane-title#editor-page-heading` `editor.page.paperTitle`, then
  `PaperBlock`), `div.pane-block[data-section="page-format"]` (the hook kept; its h4 becomes `h3.pane-title`
  `editor.page.formatLabel`; `scale-ink` a switch, `data-field` kept), `section.pane-block[data-section="turn-crop"]`
  (`editor.page.turnCropTitle`; the rotate buttons with `editor.page.rotateHint` as `pane-help`, then the two crops,
  which get `data-action="auto-crop"` and `"manual-crop"`).
- `PaperBlock.svelte` (new; M17 puts it in live's Page tab): props `{ background: Background; disabled?: boolean;
  onbackground: (background: Background) => void }`. `div.pane-swatches` of `button.pane-swatch[data-paper="white|cream|kraft|slate"]`
  (`aria-pressed`, the colour from `paperRgb`, the name `editor.papers.<id>` under it); a custom paper shows a fifth
  swatch `data-paper="custom"` (`editor.papers.custom`), pressed and disabled, as the select's extra option does today;
  the template `select` (`data-field="template"`, new); the texture switch (`data-field="texture"`, new).
- `BackgroundImageSection.svelte`: its root becomes `section.pane-block[data-section="background-image"]` (hook kept),
  its h4 `h3.pane-title`; no top border or margin (it is no longer under the page block); M17 can then put it, or a
  variant, in live's Fond tab.
- `OriginalPanel.svelte`: `section.pane-block[data-section="original"]`, title `pane-title`.
- `SplitPanel.svelte`: `section.pane-block.split-panel` (the class kept).
- `app.css` (after package 0): `.editor-side` (1502–1508) becomes `display: flex; flex: none; min-height: 0;` (no
  width, overflow or border: the component has them); the `@media (max-width: 900px)` width (1566–1570) and the
  `.side-section` rules (1510–1541) go once no block uses them.
- Optional (O14): a re-fit of the page on collapse and expand. Not in M16 unless the user asks.

**Splitting B.** `Editor.svelte` (2,261 lines) and `Editor.test.ts` (2,513) are one agent's work, but the key
extraction is separable without touching the same files: **B2** owns `lib/editor/keys.ts`, `lib/editor/shortcuts.ts`
and their tests (pure code, no component); **B1** owns everything else of the editor and wires the mapping in
`Editor.svelte` after merging B2. If the lead wants a third agent: **B1b** may own the blocks' components
(`PagePanel.svelte`, new `PaperBlock.svelte`, `BackgroundImageSection.svelte`, `OriginalPanel.svelte`,
`SplitPanel.svelte`, new `PaperBlock.test.ts`) with their props unchanged but for `PagePanel`'s `children`; B1a keeps
`Editor.svelte`, `Toolbar.svelte`, `EditorSound.svelte`, `app.css` and every `Editor*.test.ts` (they mount the editor),
and adapts the tests that count the page's controls after merging B1b.

## 8. Live window in M16 (package C)

- `LiveView.svelte`: `const side = new SidePanelState('live')`; `<aside class="live-side" aria-label={t('side.label')}>`
  around `<SidePanel where="live" panel={side} {badges} above={…}>` with:
  - `layers`: `LayersZone` as today (1083–1096).
  - `sound`: the narration block (1149–1166) as `section.pane-block[data-section="narration"]`.
  - `projection`: `ProjectorBlock`, `ZoneBlock` (with the blackboard and laser switches and `live.preview.hint` in its
    children), `ShareBlock`.
  - `help`: `<ShortcutsHelp where="live" shortcuts={LIVE_SHORTCUTS} />`.
  - no `page`, no `background` (M17).
- Toolbar: the `[data-slot="share"]` group (1037–1039) and the `ShareButton` import (68) go; blackboard and laser stay
  (O2).
- The above block, the reopening: §5. Badges: §4.
- `components/projection/ProjectorBlock.svelte` (new; M17 puts it in the editor's Projection): props `{ monitors:
  MonitorInfo[]; monitorKey: string ($bindable); loaded: boolean; presentation: MonitorInfo | null; busy: boolean;
  error: string | null; onproject: () => void; onstop: () => void; onrefresh: () => void }`;
  `section.pane-block[data-section="projector"]`, title `live.projection.projector`; today's content and hooks
  (`project`, `stop-projecting`, the refresh, `p[data-presentation]`, the single-screen and no-screen texts, the
  error). The screen `select` is the first `select` of the live side panel (`LiveView.test.ts:285`: the layers zone
  has none and the narration's microphone picker shows only once its panel is open; keep it so, or give the test a
  hook). `monitorName` (713–717) goes with it, or into `lib/monitors.ts`.
- `components/projection/ZoneBlock.svelte` (new; M17 reuses it): props `{ zoom: number; min: number; max: number;
  onzoom: (direction: 1 | -1) => void; onwhole: () => void; onwidth: () => void; children?: Snippet; hint?: string }`;
  `section.pane-block[data-section="zone"]`, title `live.zoom.area`; − / `[data-zoom]` / + (aria-labels
  `live.zoom.out|in`), `whole-page`, `page-width` (title `live.zoom.fitWidthTitle`), then `children`, then the hint.
  Live's children: two switches, `data-field="board"` and `data-field="laser"` (`live.board`, `live.laser`), checked
  from `live.view.board|laser`, calling `toggleBoard(actions)` / `toggleLaser(actions)`; their `data-field` leaves the
  toolbar's `[data-action="board"|"laser"]` unique.
- `KEYS` (112–130) and the keys block go; the texts `live.keys.list.<id>.keys` and `live.keys.title` become unused (D
  removes them).
- CSS: `.live-side` becomes `display: flex; flex: none; min-height: 0;`; `.side-block`, `.field-row`, `.keys`, `kbd`
  and the floating `.select-bar` rules go or move.
- `lib/live-keys.ts`: its comment tables point to `lib/live-shortcuts.ts` (no code change).

## 9. Translations (package 0)

All in both catalogs, added by package 0 (FR in the user's tone: short, plain, « » and plain spaces, as the files are).

New:

| Key | Français | English |
|---|---|---|
| `side.label` | Panneau de côté | Side panel |
| `side.tablist` | Onglets du panneau | Panel tabs |
| `side.tabs.layers` | Calques | Layers |
| `side.tabs.page` | Page | Page |
| `side.tabs.background` | Fond | Background |
| `side.tabs.sound` | Son | Sound |
| `side.tabs.projection` | Projection | Projection |
| `side.tabs.help` | Aide | Help |
| `side.openTitle` | {tab} (cliquer pour replier) | {tab} (click to collapse) |
| `side.collapse` | Replier le panneau | Collapse the panel |
| `side.collapsed` | replié | collapsed |
| `side.badge.layers` | one: {count} calque · other: {count} calques | one: {count} layer · other: {count} layers |
| `side.dot.warn` | un calque utilisé est masqué | a layer in use is hidden |
| `side.dot.rec` | enregistrement en cours | recording |
| `side.dot.live` | projection ou partage en cours | projecting or sharing |
| `side.help.editor` | Raccourcis de l'éditeur | Editor shortcuts |
| `side.help.live` | Raccourcis du mode direct | Live mode shortcuts |
| `side.help.replay` | Pendant une relecture | During a replay |
| `side.help.presentation` | Dans la fenêtre projetée | In the projected window |
| `side.help.typingEditor` | Ils n'agissent pas pendant que vous tapez dans un champ de texte, sauf Ctrl + S, Ctrl + W, Ctrl + F4 et Ctrl + Q. | They do nothing while you type in a text field, except Ctrl + S, Ctrl + W, Ctrl + F4 and Ctrl + Q. |
| `side.help.typingLive` | Ils n'agissent pas pendant que vous tapez dans un champ de texte, sauf Ctrl + W, Ctrl + F4 et Ctrl + Q. Espace tourne la page même quand un bouton a le focus. | They do nothing while you type in a text field, except Ctrl + W, Ctrl + F4 and Ctrl + Q. Space turns the page even when a button has the focus. |
| `shortcuts.keys.arrowLeft` / `arrowRight` / `arrowUp` / `arrowDown` | ← / → / ↑ / ↓ | ← / → / ↑ / ↓ |
| `shortcuts.keys.pageUp` | Pg préc | Page Up |
| `shortcuts.keys.pageDown` | Pg suiv | Page Down |
| `shortcuts.keys.space` | Espace | Space |
| `shortcuts.keys.delete` | Suppr | Delete |
| `shortcuts.keys.backspace` | Retour arrière | Backspace |
| `shortcuts.keys.escape` | Échap | Esc |
| `shortcuts.keys.ctrl` | Ctrl | Ctrl |
| `shortcuts.keys.shift` | Maj | Shift |
| `shortcuts.keys.alt` | Alt | Alt |
| `shortcuts.keys.shiftArrows` | Maj + flèches | Shift + arrows |
| `editor.keys.list.select` | Outil Sélection | Select tool |
| `editor.keys.list.pen` | Outil Stylo | Pen tool |
| `editor.keys.list.eraser` | Outil Gomme | Eraser tool |
| `editor.keys.list.pan` | Outil Déplacer la page | Move-the-page tool |
| `editor.keys.list.panHold` | Déplacer la page tant que la touche est enfoncée (sauf si un bouton a le focus) | Move the page while the key is held (unless a button has the focus) |
| `editor.keys.list.straighten` | Redresser les formes de la sélection | Straighten the shapes of the selection |
| `editor.keys.list.replay` | Relecture | Replay |
| `editor.keys.list.undo` | Annuler | Undo |
| `editor.keys.list.redo` | Rétablir | Redo |
| `editor.keys.list.selectAll` | Tout sélectionner (les traits du calque choisi) | Select all (the strokes of the chosen layer) |
| `editor.keys.list.clipboard` | Copier, couper, coller des traits | Copy, cut, paste strokes |
| `editor.keys.list.delete` | Supprimer la sélection | Delete the selection |
| `editor.keys.list.deselect` | Désélectionner (ferme d'abord la découpe ou le réglage de « Redresser les formes » s'ils sont ouverts) | Deselect (first closes the split or the setting of “Straighten shapes” when open) |
| `editor.keys.list.fit` | Ajuster : la page entière | Fit: the whole page |
| `editor.keys.list.fitWidth` | Largeur : la page sur toute la largeur | Width: the page across the window |
| `editor.keys.list.pages` | Page précédente, page suivante (la note d'avant, celle d'après) | Previous page, next page (the note before, the note after) |
| `editor.keys.list.save` | Enregistrer | Save |
| `editor.keys.list.close` | Revenir à la bibliothèque (ferme d'abord la découpe ou la relecture) | Back to the library (first closes the split or the replay) |
| `editor.keys.list.quit` | Quitter Ardoise | Quit Ardoise |
| `editor.keys.list.leaveReplay` | Quitter la relecture | Leave the replay |
| `editor.keys.list.playPause` | Lecture / pause | Play / pause |
| `editor.page.paperTitle` | Papier | Paper |
| `editor.page.turnCropTitle` | Tourner et recadrer | Turn and crop |
| `editor.sound.replayFirst` | La narration s'écoute et s'enregistre pendant une relecture. | A narration is heard and recorded during a replay. |
| `editor.sound.startReplay` | Lancer la relecture | Start the replay |
| `live.projection.projector` | Vidéoprojecteur | Projector |
| `live.zoom.area` | Zone projetée | Projected area |
| `share.block.title` | Partage sur le réseau | Network sharing |
| `share.block.idle` | Un autre ordinateur du réseau affiche la page dans un navigateur. | Another computer on the network shows the page in a browser. |
| `share.block.active` | one: Partagé : {count} écran connecté. · other: Partagé : {count} écrans connectés. | one: Shared: {count} screen connected. · other: Shared: {count} screens connected. |

Changed:

| Key | Français | English |
|---|---|---|
| `live.keys.list.delete.action` | Supprimer la sélection | Delete the selection |
| `live.keys.list.leave.action` | Abandonner le geste du stylet en cours, fermer le panneau « Encre de l'ordinateur », enlever la sélection ; sinon fermer la projection si elle est affichée, sinon terminer le mode direct (après confirmation) | Drop the pen gesture under way, close the “Computer's ink” panel, deselect; otherwise close the projection when it is shown, otherwise end live mode (after confirmation) |
| `live.keys.list.marks.action` | Effacer les annotations tracées sur la présentation | Clear the annotations drawn over the presentation |

Unused once B1 and C are in, removed by D after a search of `app/ui/src`, `app/e2e`, `app/scripts` and
`app/src-tauri/src`: `editor.sidePanel`, `live.keys.title`, every `live.keys.list.<id>.keys`.

A key found missing after package 0 is added by the lead on `m16` (packages do not edit the catalogs), then merged into
the packages' branches.

## 10. Impact on the existing tests and journeys

### Vitest

Keeping the tabs mounted (§2) keeps most tests as they are: content found through `querySelector` and clicked with
`.click()` is still there. What must change (owner):

- B1: `Editor.test.ts:1395–1430` (the split locks the side panel: it counts 15 controls in
  `section[aria-labelledby="editor-page-heading"]`, background image included; the page's blocks, the swatches and the
  switches change the list, the image is in Fond); `Editor.share.test.ts:151` (the button in the toolbar → in the
  Projection tab, and none in `.toolbar`); `Editor.page.test.ts:158` (texts of the page blocks); any test asserting the
  side panel's order; new `Editor.side.test.ts` (each tab's content in its `[data-tab-panel]`, Son outside and during a
  replay, the replay showing Son and giving the tab back, the selection and the split above and reopening, badges, the
  storage written) and `Editor.shortcuts.test.ts` (§6).
- C: `LiveView.computer.test.ts:122` (`aside.live-side`'s first child is no longer the zone: read it in
  `[data-tab-panel="layers"]`); `LiveView.share.test.ts:102` (the button « in its toolbar » → in Projection, none in
  `.live-toolbar`); new `LiveView.side.test.ts` (the four tabs and no Page or Fond, Calques by default, a stored `page`
  shown as Calques and kept in the storage, the above block, badges with `mock-narration`'s recording and
  `mock-share`, the switches) and `LiveView.shortcuts.test.ts` (§6, and Space with a rail tab focused turns the page).
- A: `LayersZone.test.ts` if the zone's heading or padding changes (it stays an `h3` labelling the section, made
  `visually-hidden`: the pane's title already says « Calques »); new `SidePanel.test.ts` (with
  `SidePanelHarness.svelte`), `side-panel.test.ts`, `ShortcutsHelp.test.ts`, `ShareBlock.test.ts`.
- Unchanged, checked against the moves: `Editor.edit.test.ts` (`[data-section="original"]`), `BackgroundImage.test.ts`
  (the section, the rotate button by label), `Editor.pages.test.ts` and `Editor.test.ts`'s narration tests (in a
  replay, Son holds today's content), `LiveView.test.ts` (`.live-side select`, `data-zoom`, `whole-page`,
  `stop-projecting`, narration), `LiveView.select.test.ts` and `LiveView.review.test.ts` (`select-bar`,
  `delete-selection`, `select-hint`), `Home.share.test.ts:230` (`section.live-view button[data-action="share"]`).

Helpers (`components/side/side-test-helpers.ts`, package 0): `rememberSide(where, view)` (writes the storage before a
mount), `openSideTab(root, tab)` (clicks `[role="tab"][data-tab=…]` unless that tab is shown and open, then `tick()`),
`sideOf(root)` (`{ tab, collapsed }` from the root's data attributes), `tabPanelOf(el)` (the `data-tab-panel` of an
element).

### End-to-end journeys

`app/e2e/side-panel.mjs` (package 0):

```js
export const SIDE = '[data-side-panel]';
export const tabButton = (tab) => `${SIDE} .side-rail [role="tab"][data-tab="${tab}"]`;
export const tabPanel = (tab) => `${SIDE} [data-tab-panel="${tab}"]`;
/** { where, tab, collapsed } of the side panel on display, or null. */
export async function sideState(app) { … }
/** Opens `tab`, expanding the pane when collapsed; nothing when it is already shown and open. Waits for it. */
export async function openTab(app, tab) { … }
/** Collapses the pane with « › » (or does nothing when collapsed); waits for data-collapsed="true". */
export async function collapsePanel(app) { … }
```

The editor's default tab is Calques and live's too, so the journeys that use the layers zone need nothing. Every other
click into a tab's content needs `openTab` first (risk R1: `waitFor` on a hidden control passes, the click then lands
elsewhere):

| File | Journey, line | What moved | Change (D) |
|---|---|---|---|
| `journeys-m7-edit.mjs` | « redresser un cercle…, puis revenir à l'original », 72–78 (`[data-action="original"]`, `original-confirm`) | Original → Page | `openTab(app, 'page')` at the start of the step at 75 |
| `journeys-m7-background.mjs` | 133 reads `[data-slot="background-image-name"]` (text of hidden content: works); 139 clicks `button[title=editor.page.rotateCw]` | rotate → Page | `openTab(app, 'page')` before 139 |
| `journeys-m8-page.mjs` | 36 `orientation-portrait`; 46–48 sets `paper-size` by script | Page | `openTab(app, 'page')` before 36 |
| `journeys-m10-ui.mjs` | 68 clicks `project` (69, 71 wait for presence) | Projection | `openTab(app, 'projection')` before 68 |
| `journeys-m11-share.mjs` | `openPanel` 601–605 (used in the editor at 1129); 655, 1209, 1253 click the share in live; 1036–1054, the share button's look in the editor | share → Projection, both windows | `openPanel` calls `openTab(app, 'projection')` first; 655, 1209, 1253 use it; the look check opens the tab first and adds: no `.toolbar [data-action="share"]` in the editor |
| `journeys-m12-resume.mjs` | 560 share in live; 611–617 original in the editor (`editorFacts` 471–476 and `openInEditor` 479–487 read hidden content: work) | Projection, Page | `openTab` before 560 and 616 |
| `journeys-m12-touchup.mjs` | `startShare` 716–720 (live); 1129, 1371 `${BAR} delete-selection` (the bar moved to the above block: visible); 1576, 1585 `board` (toolbar kept, O2) | Projection | `openTab(app, 'projection')` in `startShare` |
| `journeys-m14-live.mjs` | `startShare` 418–422; 590 waits for `pen-hint` (above block: visible) | Projection | `openTab` in `startShare` |
| `journeys-m15-straighten.mjs` | 46 reads `.editor-side`'s text (the selection count, now in the above block inside it: works); 184 clicks `.editor-side h3` « elsewhere » | | 184 → `[data-slot="side-title"]` (a stable target outside the shapes panel) |
| `journeys-m14-editor.mjs` | 25 `.editor-side section.layers-zone`, 38 `.editor-side` text, 72 no radio in `.editor-side` | | none (Calques is the default; no block has radios) |
| `journeys-m9-bridge.mjs` | 164, 193 wait for `stop-projecting` / `project` (presence only) | | none |
| `journeys.mjs` | the replay journey (258): the replay now shows Son | | none |
| `perf.mjs` | 456 `[data-slot="select-bar"] .select-count` (kept), 627–652 the zone and `touch-up` | | none |

New journeys (D), driven as the others, in French and in English:

- **`journeys-m16-editor.mjs`** « panneau de côté de l'éditeur : onglets, repli, pastilles, sélection, partage » (the
  share on loopback, `[share] port` and `ARDOISE_SHARE_LOOPBACK` as `journeys-m11-share.mjs`): open a demo note; six
  tabs, Calques open; each tab shows its content (`[data-tab-panel=…]` not hidden and its block visible: the layers
  zone, `page-format`, `background-image`, `sound-replay`, `share`, `dl.keys` with one `dt` per main entry of the
  table); collapse with a click on the open tab (the canvas grows by at least 200 px), reopen with another icon (that
  tab open); collapse with « › » (the focus on the shown tab); ArrowDown twice then Enter opens the tab two below;
  choose Page, collapse, back to the library, open a note: Page, collapsed, and `ardoise.sidePanel.editor` is
  `{"tab":"page","collapsed":true}`; with the pane collapsed, Ctrl + A: the pane reopens, `[data-slot="side-above"]`
  shows the count above the tab panel (its top above the panel's); Calques' `data-count` is the number of layers, hide
  the chosen layer → `data-dot="warn"`, show it → none; Projection, « Partager sur le réseau » → the share panel; closed,
  Projection's `data-dot="live"`; stop → none; no share button in `.toolbar`; Relecture (R) → Son open, R again → the
  tab before; the recording dot: try `narration-open` then `narration-start` in a replay, skipped with a log line when
  the PC has no microphone (O12).
- **`journeys-m16-live.mjs`** « panneau de côté du direct » (`--simulated-tablet`, the share on loopback): open live
  (`registerTablet`, the tablet panel); four tabs (no Page, no Fond), Calques open; each tab shows its content
  (narration, `project`, `whole-page`, the board and laser switches, `share`, `dl.keys`); collapse and reopen; choose
  Projection, collapse, Terminer, open live again: Projection, collapsed; write `{"tab":"page"}` into
  `ardoise.sidePanel.live`, stop and start again: Calques shown, the storage still says `page`; hide the tablet's layer
  → `data-dot="warn"` on Calques, show it; project (`open_presentation` through `__TAURI_INTERNALS__`, as
  `journeys-m14-live.mjs:538`) → `data-dot="live"`, close it; start the share from Projection → the dot, stop → none;
  the recording dot as in the editor (skipped without a microphone); Select (V), a lasso on the preview with the pane
  collapsed → the pane reopens with the select bar above the tab; a click on a rail tab, then Space → the next page,
  the tab not toggled.

Registered in `run.mjs` by package 0 (`export const sidePanelEditorJourneys = []`, `sidePanelLiveJourneys = []`), filled
by D.

## 11. M17 fit

- **Live, Page and Fond**: `LiveView` adds `{#snippet page()}` and `{#snippet background()}`; the rail shows them in
  `SIDE_TABS`' order with no change to `SidePanel`, `SidePanelState` or the storage (a `page` remembered since then is
  shown at once). Page reuses `PaperBlock` (§7) with the live operation as `onbackground`; Fond reuses
  `BackgroundImageSection` or a variant (it is a standalone block from M16 on).
- **Editor, Projection**: the editor's `projection` snippet gets `ProjectorBlock` and `ZoneBlock` (§8, written with
  editor-agnostic props for this) above `ShareBlock`; the editor's Projection badge gets `|| projecting`.
- **Editor, Son « Pendant que j'écris »**: added in `EditorSound.svelte` (§7) above the recording; the red dot already
  follows `recorder.phase`, whatever starts the recording.
- **Help**: a key M17 adds is a row of the window's table; the sweeps and probes then require it.
- Nothing of M16's DOM contract, props or storage changes for M17.

## 12. Work packages

One owner per file, as in M4 and M14. Each package in its own worktree and branch, merged into `m16` by the lead.

| # | Package (profile) | Owns | Size | Starts |
|---|---|---|---|---|
| 0 | Contract (the lead, one commit) | see below | S–M: ~700 lines, mostly data, catalogs and skeletons | first |
| A | Rail, help, share block (developpeur) | `components/side/*` (`SidePanel.svelte`, `SideIcon.svelte`, `ShortcutsHelp.svelte`, `side-test-helpers.ts`, new `SidePanelHarness.svelte`, their tests); `lib/side-panel.svelte.ts`, `lib/shortcuts.ts`, `lib/shortcuts-sweep.ts` and their tests; `components/share/ShareBlock.svelte` (+ test), `ShareButton.svelte` (comment only); `components/layers/LayersZone.svelte` and `LayersZone.test.ts`; `components/narration/RecordPanel.svelte` only if its look needs it | M: ~1,200 lines | after 0 |
| B2 | Editor keys (developpeur) | `lib/editor/keys.ts`, `lib/editor/keys.test.ts`, `lib/editor/shortcuts.ts`, `lib/editor/shortcuts.test.ts` | S: ~450 lines | after 0; merged before B1 wires its keys |
| B1 | Editor (developpeur) | `components/editor/Editor.svelte`, `Toolbar.svelte`, `PagePanel.svelte`, new `PaperBlock.svelte`, `BackgroundImageSection.svelte`, `OriginalPanel.svelte`, `SplitPanel.svelte`, new `EditorSound.svelte`, `Canvas.svelte` (only for O14), every `components/editor/*.test.ts` and harness, `app.css` (after 0) | L: ~1,800 lines touched, half of it tests | after 0, with A, B2, C |
| C | Live window (developpeur) | `components/live/LiveView.svelte`, new `components/projection/ProjectorBlock.svelte`, `ZoneBlock.svelte` (+ tests), `lib/live-shortcuts.ts` and `lib/live-shortcuts.test.ts`, `lib/live-keys.ts` (comments), every `components/live/LiveView*.test.ts` (+ new `LiveView.side.test.ts`, `LiveView.shortcuts.test.ts`), `components/Home.share.test.ts` if it needs it | M–L: ~1,400 lines | after 0, with A, B |
| D | Journeys, guides, closing (developpeur) | `app/e2e/side-panel.mjs`, `journeys-m16-editor.mjs`, `journeys-m16-live.mjs`, `run.mjs`, the journeys of §10's table; `docs/guide/guide.fr.md`, `guide.en.md` (side panel, share, original, chapter 17 from the tables), `CHANGELOG.md`, `docs/notes-de-version.md`, `docs/design/interface.md` (one line pointing here), this file (the "as built" notes, taken from the other packages' reports: they do not edit it); the catalogs after A–C (removals of §9) | M: ~1,300 lines | journeys written after 0, run after A, B1, B2, C are merged; guides last |
| R | Independent review (relecteur); remarks fixed by their package's owner | – | – | last |

Order: 0; then A, B2, B1, C and D's journeys in parallel (B1 and C build against package 0's skeletons and contract);
merge B2, then A, then B1 and C (disjoint files); D runs the journeys and writes the guides; R. `ROADMAP.md` is ticked
only after `cargo xtask check` and `cargo xtask e2e` on the PC (CLAUDE.md).

### Package 0, the contract (committed by the lead before anything else)

1. `app/ui/src/lib/side-panel.svelte.ts`: §2 "Model", complete (types, `SIDE_TABS`, `SIDE_DEFAULTS`, storage key,
   load and save field by field, `SidePanelState`).
2. `app/ui/src/components/side/SidePanel.svelte`: a skeleton with the final props and the DOM contract of §2 (rail,
   tabs with their attributes, pane, `hidden`, above block, tab panels, « › »), the clicks wired to `panel.toggle` and
   `panel.collapse`; no style, no arrows. It settles the `aria-expanded` and `role="switch"` questions with
   `npm run check` (§2).
3. `app/ui/src/components/side/ShortcutsHelp.svelte` and `app/ui/src/components/share/ShareBlock.svelte`: skeletons
   with their final props and hooks (§2 "Help, share block").
4. `app/ui/src/components/side/side-test-helpers.ts`: §10.
5. `app/ui/src/lib/shortcuts.ts` (types, `chordMatches`, `chordEvents`; `chordParts` may be simple) and
   `app/ui/src/lib/shortcuts-sweep.ts` (complete: B2 and C need it at once).
6. `app/ui/src/lib/editor/keys.ts`: `EditorKeyKind`, `EditorKeyAction`, and `editorKeyAction` returning `null` (B2
   writes it; nothing calls it before B1 wires it).
7. `app/ui/src/lib/editor/shortcuts.ts` and `app/ui/src/lib/live-shortcuts.ts`: the two tables of §6 and their
   contexts.
8. `app/ui/src/locales/fr.json`, `en.json`: every new and changed key of §9.
9. `app/ui/src/app.css`: `--accent-soft` in its three blocks.
10. `app/e2e/side-panel.mjs` (complete, §10), `app/e2e/journeys-m16-editor.mjs` and `journeys-m16-live.mjs` as empty
    lists, registered in `app/e2e/run.mjs`.

Nothing uses the new files yet, so `cargo xtask check` stays as it was. Package 0 does not touch any window.

#### Package 0, as built

Everything above is built as written; what was settled or added:

- **`aria-expanded` on `role="tab"`**: `npm run check` (svelte-check, `--fail-on-warnings`) accepts it, so it stays and
  `side.collapsed` stays unused (a spare key; D may remove it with the others of §9).
- **`role="switch"`**: svelte-check accepts `<input type="checkbox" role="switch">` with or without `aria-checked`.
  Decision: **no `aria-checked`** (ARIA in HTML says not to repeat the state of a native checkbox: the browser exposes
  `checked`). A, B1 and C write `<input type="checkbox" role="switch" class="pane-switch" …>` only; §2's « aria-checked
  repeated » is dropped.
- **Arrow keys on the rail (package A)**: an `onkeydown` on the `role="tablist"` div makes svelte-check warn
  `a11y_interactive_supports_focus` (a `tablist` with a handler must have a `tabindex`). Put the handler on each tab
  button instead (the keys bubble from the focused tab and the tablist stays plain), or `svelte-ignore` it with a reason.
- **`SidePanelState.toggle(tab, available)`** does nothing for a tab that is not in `available`. The constructor's second
  argument is the storage (default: the browser's `localStorage`; `null`: none), so a test can inject one.
- **Extra exports** (A may use them): `SideIconName` (`SideIcon.svelte`'s `module` script), `usKeyCode(key)` and
  `keyPress(key, code, mods)` in `lib/shortcuts.ts` (the sweep and `chordEvents` build their key presses with them: a
  letter is lower-case, upper-case with Shift, `code` defaults to the US one).
- **`chordParts`** is final, not simple: Ctrl, Alt, Maj (only `shift: true`, not `'any'`), then the key (`-` shown as
  « − », a letter in capitals, the named keys through `shortcuts.keys.*`). `ShortcutsHelp` is complete too (content and
  hooks; no style): its ids are `side-help-title-{where}`, and `ShareBlock`'s `side-share-title`.
- **`SidePanel.svelte`** is the contract and nothing more: no style (the `hidden` rule `.side-pane:not([hidden])` is A's),
  no arrow keys, no focus move after « › ». Tests build the snippets with `createRawSnippet`, so `SidePanelHarness.svelte`
  is not needed unless A wants one.
- **The editor's table was checked** against a scratch implementation of §6's mapping (the nine numbered rules, with
  `closeKey` and `isFitKey`): `sweepShortcuts(EDITOR_SHORTCUTS, EDITOR_KEY_CONTEXTS, does)` is `[]`, so B2's sweep test
  should pass as soon as `editorKeyAction` follows §6; if it does not, the mapping differs from §6 (read the sentences).
  The live table passes its sweep now (`lib/live-shortcuts.test.ts`, which C extends with the probes' parts).
- **Tests that package 0 commits for files that A, B2 or C own** (they extend them, nobody recreates them):
  `lib/side-panel.test.ts`, `lib/shortcuts.test.ts`, `lib/shortcuts-sweep.test.ts`, `components/side/SidePanel.test.ts`,
  `components/side/ShortcutsHelp.test.ts`, `components/share/ShareBlock.test.ts`, `lib/live-shortcuts.test.ts`, and
  `lib/shortcuts-tables.test.ts` (labels and contexts of both tables, in both catalogs). `lib/editor/shortcuts.test.ts`
  and `lib/editor/keys.test.ts` stay B2's.
- **Catalogs**: `side`, `shortcuts` (new top-level namespaces, after `editor`), `editor.keys`, `editor.sound`,
  `editor.page.paperTitle|turnCropTitle`, `live.projection.projector`, `live.zoom.area`, `share.block`, and the three
  changed `live.keys.list.<id>.action`; `side.badge.layers` and `share.block.active` are plural (`one` / `other`).

## 13. Risks and open points

Open points (the user's call; each with a recommendation; none is decided by this document beyond its default):

**Decided by the lead (2026-11-11): every recommendation below is taken (O1–O15), within what the user accepted in
Q13; the user is told in the M16 report and can still change any of them.**

- **O1. Does live always open on Calques?** Q13 says the tab and the collapsed state are remembered « pour chaque
  fenêtre » and « Le direct s'ouvre sur Calques »; the end criteria want the tab and the collapsed state found again on
  reopening in each window; the mockup's script forces Calques and an open pane each time live opens. Recommendation:
  Calques is live's default, then live remembers like the editor (what §3 does).
- **O2. Blackboard and laser.** ROADMAP lists them in live's Projection; Q13 moves only the share out of a toolbar.
  Recommendation: keep the toolbar's buttons (often used during class, next to the pages) and show the same states as
  switches in « Zone projetée ». Alternative: only in the tab (the toolbar gets lighter; B and L stay).
- **O3. The selection in live mode.** SPEC puts it above the open tab (both windows); the mockup showed it in the editor
  only; today it floats over the preview. Recommendation: move it above the open tab and reopen the pane, as the editor.
- **O4. Live's tool hints** (`select-hint`, `pen-hint`) go above the open tab while the tool is on. Recommendation: yes
  (in Projection they would hide behind Calques, live's usual tab).
- **O5. The editor's default tab**: Calques, the mockup's start. Recommendation: keep it.
- **O6. A replay shows Son** and gives back the tab and collapsed state it found when it ends. Recommendation: yes (the
  narration used to be the only block of a replay; without it, recording needs an extra click).
- **O7. Son outside a replay** in M16: a sentence and « Lancer la relecture »; the narration's length is only known in
  a replay (`enterReplay` loads it), so it is not shown outside one (that would read it for every note opened).
  Recommendation: as designed; M17 reworks this tab anyway.
- **O8. Recolour pastilles in the editor's selection block** (mockup): not added; the toolbar's swatches recolour a
  selection already. Recommendation: not in M16.
- **O9. Live zoom as a slider** (mockup) or − / value / + (today, with the keys + and −). Recommendation: keep − / +.
- **O10. Presentation-only keys in live's help** (§6 findings): a group « Dans la fenêtre projetée » and corrected
  texts. Recommendation: yes (the help would otherwise show keys that do nothing in its window, which the probes forbid).
- **O11. Paper colour**: swatches instead of the select; a custom paper shown as a pressed, disabled fifth swatch.
  Recommendation: yes (ROADMAP's « pastilles pour la couleur du papier »).
- **O12. The recording dot in the journeys**: the journeys have no microphone (`journeys-m12-resume.mjs:22`); the step
  is skipped with a log line when recording cannot start (as the Edge steps), and vitest checks the dot with
  `mock-narration`. Alternative, not in M16: a simulated microphone in the exe.
- **O13. « Réouverture » in the journeys**: closing and reopening the editor or live mode in the same run, plus the
  stored JSON; the runner gives each journey a fresh profile, so an exe restart is not tried. Recommendation: accept
  (a `Page.reload` could be added; supposé harmless).
- **O14. Re-fitting the page when the pane collapses or opens**: today's resize keeps the view, so the page stays where
  it was and more of the stage shows. Recommendation: keep it in M16 (no `Canvas.svelte` change).
- **O15. No key to fold the panel** (none asked). Recommendation: none in M16.

Risks:

- **R1. Hidden but mounted content.** `waitFor(querySelector(…) !== null)` passes while the control cannot be clicked;
  `app.click` clicks anyway after about a second (`run.mjs:208–226`), elsewhere. D opens the tab before every action of
  §10's table and greps every `app.click(` of the journeys for content of a tab. Tests asserting visibility read
  `hidden` on the `[data-tab-panel]`.
- **R2. CSS `hidden` overridden by a `display` rule** (§2): the pane would not collapse in the real window while jsdom
  shows it collapsed. A writes `:not([hidden])`; D's journey measures the canvas growing.
- **R3. Global class collisions**: prefixes `side-` and `pane-`, the look scoped under `.side-pane`; jsdom would not
  show a collision (M15), the journeys' measures would.
- **R4. The editor's key refactor**: the mapping keeps today's order and tests; the existing keyboard tests of the
  editor (`Editor.test.ts`, `Editor.edit`, `Editor.pages`, `Editor.shapes`, `Home.close-keys`) guard it, with the sweep
  and the probes.
- **R5. B1 and C build on skeletons**: A may add to the DOM contract, not rename; B1 and C test their content and the
  contract's attributes, A tests the rail's behaviour.
- **R6. jsdom has no layout**: widths, the narrow window, the dark theme and the collapse are seen only in the real
  window (D's journeys measure what they can; the lead looks on Windows).
- **R7. Space in live** on a focused tab turns the page (by design); a keyboard user opens tabs with Enter there.
- **R8. Narration panel hidden while open**: a recording panel left open keeps its microphone picker listening when
  another tab shows (as it did when scrolled away). Accepted.

## Not done on purpose

A key to fold the panel; resizing the pane by dragging; reordering tabs; badges on Page, Fond and Aide; the recolour
pastilles in the editor's selection block; a zoom slider in live; live's Page and Fond, the editor's projector,
recording while writing (M17); a simulated microphone.

## To check on Windows

The rail with the pen, a finger and the mouse on the Surface (38 px tabs, the titles on hover, `:hover` under the pen);
Narrator reading the tabs, their badges and the collapsed state (supposé); the 800 × 560 window; the dark theme; the
focus ring on the tabs; the share button's look inside Projection (`journeys-m11-share.mjs`); Space with a tab focused
in live mode.
