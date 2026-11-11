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

/**
 * The red dot of the Sound tab while recording (§4, O12): opens the tab, starts a recording from the narration panel,
 * checks the dot and the tab's hidden text, then cancels (nothing is kept). The journeys have no microphone, so a PC
 * without one cannot record: a log line says so and the function returns false (vitest checks the dot with
 * `mock-narration`). `l`: the journey's catalog lookup.
 *
 * The journeys run on the user's own PC, which has a microphone: a test must not switch it on behind their back. So
 * nothing is recorded unless `ARDOISE_E2E_MICROPHONE=1` asks for it; otherwise a log line says the check was left to
 * vitest.
 */
export async function checkRecordingDot(app, l) {
  if (process.env.ARDOISE_E2E_MICROPHONE !== '1') {
    console.log('    skipped: pas d’enregistrement réel sans ARDOISE_E2E_MICROPHONE=1 (le micro de ce PC reste éteint) ; pastille rouge vérifiée par vitest (O12)');
    return false;
  }
  await openTab(app, 'sound');
  const panel = tabPanel('sound');
  const visible = (selector) => `(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) return false; const r = el.getBoundingClientRect(); return r.width > 0 && r.height > 0; })()`;
  const start = `${panel} [data-action="narration-start"]`;
  if (!(await app.evaluate(visible(start)))) {
    await app.click(`${panel} [data-action="narration-open"]`);
    await app.waitFor(visible(start), 'the start button of the narration panel');
  }
  await app.click(start);
  const outcome = await app
    .waitFor(
      `document.querySelector('.record[data-phase="recording"]') ? 'recording' : document.querySelector('[data-slot="record-error"]') ? 'error' : false`,
      'a recording or its error',
      8000,
    )
    .catch(() => 'none');
  const dot = `(document.querySelector(${JSON.stringify(tabButton('sound'))})?.getAttribute('data-dot') ?? null)`;
  if (outcome !== 'recording') {
    console.log(`    skipped: l’enregistrement ne démarre pas sur ce PC (${outcome}), pastille rouge non vérifiée ici (O12)`);
    if ((await app.evaluate(dot)) === 'rec') throw new Error('the Sound tab shows a recording that did not start');
    await app.evaluate(`document.querySelector('[data-action="narration-close"]')?.click()`);
    return false;
  }
  await app.waitFor(`${dot} === 'rec'`, 'the red dot on the Sound tab');
  const said = await app.evaluate(`(document.querySelector(${JSON.stringify(tabButton('sound'))})?.textContent ?? '').replace(/\s+/g, ' ')`);
  if (!said.includes(l('side.dot.rec'))) throw new Error('the Sound tab does not say that it is recording');
  await app.click(`${panel} [data-action="narration-cancel"]`);
  await app.waitFor(`${dot} === null`, 'the red dot to go');
  return true;
}
