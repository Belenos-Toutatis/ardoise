// Helpers of the journeys that drive the tabbed side panel (milestone M16; docs/design/side-panel.md, §10), shared by
// the journeys that use a tab's content and by `journeys-m16-editor.mjs` and `journeys-m16-live.mjs`.
//
// Every tab panel stays mounted and carries `hidden` when it is not the one shown, so a control of another tab is in the
// DOM but cannot be clicked: `app.waitFor` on its presence passes and `app.click` then lands elsewhere. A journey opens
// the tab before it acts (`openTab`). Calques is the default tab of both windows, so the layers zone needs nothing.

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

export const SIDE = '[data-side-panel]';
export const tabButton = (tab) => `${SIDE} .side-rail [role="tab"][data-tab="${tab}"]`;
export const tabPanel = (tab) => `${SIDE} [data-tab-panel="${tab}"]`;

/** { where, tab, collapsed } of the side panel on display, or null. */
export async function sideState(app) {
  return app.evaluate(`(() => {
    const el = document.querySelector(${JSON.stringify(SIDE)});
    return el ? { where: el.dataset.sidePanel, tab: el.dataset.tab, collapsed: el.dataset.collapsed === 'true' } : null;
  })()`);
}

/** Opens `tab`, expanding the pane when collapsed; nothing when it is already shown and open. Waits for it. */
export async function openTab(app, tab) {
  await app.waitFor(`document.querySelector(${JSON.stringify(tabButton(tab))}) !== null`, `the ${tab} tab in the rail`);
  const state = await sideState(app);
  if (!(state.tab === tab && !state.collapsed)) {
    await app.click(tabButton(tab));
    await app.waitFor(
      `(() => { const el = document.querySelector(${JSON.stringify(SIDE)}); return el?.dataset.tab === ${JSON.stringify(tab)} && el.dataset.collapsed === 'false'; })()`,
      `the ${tab} tab open`,
    );
  }
  // The panel is displayed once the pane no longer has `hidden`; a beat for the layout before the caller clicks into it.
  await app.waitFor(`document.querySelector(${JSON.stringify(tabPanel(tab))})?.hidden === false`, `the ${tab} panel shown`);
  await sleep(60);
}

/** Collapses the pane with « › » (or does nothing when collapsed); waits for data-collapsed="true". */
export async function collapsePanel(app) {
  const state = await sideState(app);
  if (state?.collapsed) return;
  await app.click(`${SIDE} [data-action="side-collapse"]`);
  await app.waitFor(`document.querySelector(${JSON.stringify(SIDE)})?.dataset.collapsed === 'true'`, 'the pane collapsed');
}
