// M16 journey, the live control window's side panel (docs/design/side-panel.md, §10 "End-to-end journeys"): the tabs
// (six since M17: Calques, Page, Fond, Son, Projection, Aide), each with its content, collapse and reopening, the view
// remembered when live mode is closed and opened again (a Page remembered before the window had one included), the badges (the hidden layer, the projection, the sharing), the
// switches of the blackboard and the laser, the selection above the open tab, and Space on a focused tab (it turns the
// page, it does not toggle the tab).
//
// On the production executable with the simulated tablet (`--simulated-tablet`), in French and in English; the share is on
// the loopback (`[share] port` and `ARDOISE_SHARE_LOOPBACK`, as `journeys-m14-live.mjs`, whose helpers it follows). The
// presentation window is opened through `open_presentation`, as `journeys-m14-live.mjs` does.
//
// Written without a run (the other packages were not merged when this was written): the selectors are the doc's
// (§2 "DOM contract", §5, §8). The lines that rely on a hook the doc leaves to a package carry `M16-D: check`.
//
// Limits: the red recording dot needs a microphone (the journeys have none): the step is skipped with a log line when
// recording cannot start (O12). « Found again on reopening » (O13) is live mode ended and started again in the same run,
// and the value stored in the browser's storage read back.
import { closePanel, openPanel, PANEL } from './journeys-m11-share.mjs';
import { canvasData, LIVE_CANVAS, registerTablet, STATUS, stopLive, text } from './journeys-m12-resume.mjs';
import { checkRecordingDot, collapsePanel, openTab, SIDE, sideState, tabButton, tabPanel } from './side-panel.mjs';

const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
/** The presentation window covers the control window: without these flags the control window would stop painting. */
const BROWSER_ARGS = '--disable-features=CalculateNativeWinOcclusion --disable-backgrounding-occluded-windows --disable-renderer-backgrounding';

/** The rail's tabs in the live window, in order: Page and Fond came with M17 (docs/design/paper-projection-narration.md §1.6). */
const TABS = ['layers', 'page', 'background', 'sound', 'projection', 'help'];
const STORAGE_KEY = 'ardoise.sidePanel.live';
const ZONE = 'section.layers-zone[data-where="live"]';
const ZONE_BLOCK = '[data-section="zone"]';
/** What each tab shows, one control or block that must be on screen once the tab is open (§8). */
const CONTENT = {
  layers: ZONE,
  page: '[data-section="live-paper"] button[data-paper="white"]',
  background: '[data-section="background-image"] button[data-action="background-choose"]',
  sound: '[data-section="narration"]',
  projection: 'button[data-action="project"]',
  help: '[data-section="shortcuts"] dl.keys',
};
/** The other controls of the Projection tab (§8): the zone's buttons, the two switches, the share block. */
const PROJECTION_PARTS = [
  `${ZONE_BLOCK} button[data-action="whole-page"]`,
  `${ZONE_BLOCK} button[data-action="page-width"]`,
  `${ZONE_BLOCK} [data-zoom]`,
  `${ZONE_BLOCK} input[data-field="board"]`,
  `${ZONE_BLOCK} input[data-field="laser"]`,
  '[data-section="share"] button[data-action="share"]',
];
const SELECT_BUTTON = 'button[data-action="select"]';
const BAR = '[data-slot="select-bar"]';

const previewWidth = `document.querySelector('.live-view .live-preview').getBoundingClientRect().width`;

/** Page expression: the element is displayed (it has a box, so neither its own `hidden` nor a hidden parent's applies). */
const onScreen = (selector) => `(() => {
  const el = document.querySelector(${j(selector)});
  if (!el) return false;
  const box = el.getBoundingClientRect();
  return box.width > 0 && box.height > 0 && getComputedStyle(el).visibility !== 'hidden';
})()`;
const attr = (tab, name) => `(document.querySelector(${j(tabButton(tab))})?.getAttribute(${j(name)}) ?? null)`;
/** The hidden text of a tab, which is what assistive technology reads (name, count, dot). */
const tabText = (tab) => `(document.querySelector(${j(tabButton(tab))})?.textContent ?? '').replace(/\\s+/g, ' ').trim()`;
const stored = (app) =>
  app.evaluate(`(() => { try { return JSON.parse(localStorage.getItem(${j(STORAGE_KEY)}) ?? 'null'); } catch { return null; } })()`);
const same = (a, b) => j(a) === j(b);
const layerRows = `document.querySelectorAll(${j(`${ZONE} li.layer`)}).length`;
const pressed = (selector) => `document.querySelector(${j(selector)})?.getAttribute('aria-pressed')`;

/** Enter on the focused element: a real key event with its character, as a button needs to be clicked by it. */
async function pressEnter(app) {
  await app.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' });
  await app.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
}

/** The side panel is in the state `{ tab, collapsed }` (waits for it). */
const inState = (app, tab, collapsed, what) =>
  app.waitFor(
    `(() => { const el = document.querySelector(${j(SIDE)}); return el?.dataset.tab === ${j(tab)} && el.dataset.collapsed === ${j(String(collapsed))}; })()`,
    what,
  );

/** Opens live mode from the tablet panel (the registration, or the end of the last live mode, left the library) and waits for the first strokes. */
async function openLive(app) {
  await app.click(STATUS);
  await app.waitFor(`document.querySelector('.tablet-panel button[data-action="live"]:not([disabled])') !== null`, 'the live button');
  await app.click('.tablet-panel button[data-action="live"]');
  await app.waitFor(`document.querySelector('.live-view[data-state="live"]') !== null`, 'live mode', 30_000);
  await app.waitFor(`Number(${canvasData('strokes')}) > 0`, 'strokes on the live canvas', 15_000);
  await app.waitFor(`document.querySelector(${j(SIDE)}) !== null`, 'the side panel');
}

/** A lasso, with the mouse, around the page on the preview (the page is fitted whole: its margins are inside). */
async function lassoPage(app) {
  const box = await app.evaluate(`(() => { const r = document.querySelector(${j(LIVE_CANVAS)}).getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`);
  const corner = (fx, fy) => ({ x: box.x + box.w * fx, y: box.y + box.h * fy });
  await app.drag([corner(0.03, 0.03), corner(0.97, 0.03), corner(0.97, 0.97), corner(0.03, 0.97), corner(0.03, 0.05)]);
}

export const sidePanelLiveJourneys = [
  {
    name: 'panneau de côté du direct : onglets, repli, mémoire, pastilles, sélection, partage',
    args: ['--simulated-tablet'],
    settings: (ctx) => `[share]\nport = ${ctx.sharePort}\n`,
    env: { ARDOISE_SHARE_LOOPBACK: '1' },
    browserArgs: BROWSER_ARGS,
    async run(app, check) {
      const l = (key) => text(app.language, key);

      await registerTablet(app, check);

      await check('ouvrir le mode direct (tablette simulée) : six onglets (Page et Fond compris), Calques ouvert', async () => {
        await openLive(app);
        const rail = await app.evaluate(`[...document.querySelectorAll(${j(`${SIDE} .side-rail [role="tab"]`)})].map((b) => b.dataset.tab)`);
        if (!same(rail, TABS)) throw new Error(`the rail has the tabs ${j(rail)}, expected ${j(TABS)}`);
        const state = await sideState(app);
        if (!same(state, { where: 'live', tab: 'layers', collapsed: false })) throw new Error(`the panel starts as ${j(state)}`);
        const aside = await app.evaluate(`document.querySelector('aside.live-side')?.getAttribute('aria-label') ?? null`);
        if (aside !== l('side.label')) throw new Error(`the aside is labelled ${j(aside)}, expected ${j(l('side.label'))}`);
        // The share left the toolbar (its group, with it).
        if ((await app.evaluate(`document.querySelector('.live-toolbar [data-action="share"]') !== null || document.querySelector('.live-toolbar [data-slot="share"]') !== null`)) === true) {
          throw new Error('the share is still in the live toolbar');
        }
        // The blackboard and the laser stay in the toolbar (O2).
        for (const action of ['board', 'laser']) {
          if ((await app.evaluate(`document.querySelector('.live-toolbar button[data-action="${action}"]') !== null`)) !== true) throw new Error(`the toolbar lost its ${action} button`);
        }
      });

      await check('chaque onglet montre son contenu, et lui seul ; Projection : le vidéoprojecteur, la zone, les deux interrupteurs, le partage', async () => {
        for (const tab of TABS) {
          await openTab(app, tab);
          const shown = await app.evaluate(`[...document.querySelectorAll(${j(`${SIDE} [data-tab-panel]`)})].filter((p) => !p.hidden).map((p) => p.dataset.tabPanel)`);
          if (!same(shown, [tab])) throw new Error(`with ${tab} open the panels shown are ${j(shown)}`);
          const content = `${tabPanel(tab)} ${CONTENT[tab]}`;
          if ((await app.evaluate(onScreen(content))) !== true) throw new Error(`${content} is not on screen in the ${tab} tab`);
          const title = await app.evaluate(`document.querySelector(${j(`${SIDE} [data-slot="side-title"]`)})?.textContent?.trim() ?? null`);
          if (title !== l(`side.tabs.${tab}`)) throw new Error(`the pane's title is ${j(title)} in the ${tab} tab`);
        }
        // M16-D: check against package C (the zone block, the switches and the share block of Projection, §8).
        await openTab(app, 'projection');
        for (const part of PROJECTION_PARTS) {
          if ((await app.evaluate(onScreen(`${tabPanel('projection')} ${part}`))) !== true) throw new Error(`${part} is not on screen in the Projection tab`);
        }
        // The help lists every entry of the live table; the projected window's keys are in their own list.
        await openTab(app, 'help');
        const ids = Object.keys(l('live.keys.list'));
        const expected = ids.map((id) => l(`live.keys.list.${id}.action`)).sort();
        const section = `${tabPanel('help')} [data-section="shortcuts"]`;
        const shown = await app.evaluate(`[...document.querySelectorAll(${j(`${section} dd`)})].map((dd) => dd.textContent.trim()).sort()`);
        if (!same(shown, expected)) throw new Error(`the help lists ${j(shown)}, expected ${j(expected)}`);
        const marks = await app.evaluate(`document.querySelectorAll(${j(`${section} h4 + dl.keys dd`)}).length`);
        if (marks !== 1) throw new Error(`the projected window's list has ${marks} entries, expected 1`);
        if ((await app.evaluate(`document.querySelectorAll(${j(`${section} dt kbd`)}).length`)) < ids.length - 1) throw new Error('the help shows no key for some entries');
        await openTab(app, 'layers');
      });

      await check('un clic sur l’onglet ouvert replie le panneau (l’aperçu gagne au moins 200 px), un autre onglet le rouvre', async () => {
        const narrow = await app.evaluate(previewWidth);
        await app.click(tabButton('layers'));
        await inState(app, 'layers', true, 'the pane collapsed');
        await app.waitFor(`${previewWidth} - ${narrow} >= 200`, 'the preview to take the width of the pane');
        if ((await app.evaluate(onScreen(`${SIDE} .side-pane`))) === true) throw new Error('the pane is still displayed once collapsed');
        if ((await app.evaluate(onScreen(`${SIDE} .side-rail`))) !== true) throw new Error('the rail is gone with the pane');
        await app.click(tabButton('sound'));
        await inState(app, 'sound', false, 'the Sound tab open');
        await app.waitFor(`Math.abs(${previewWidth} - ${narrow}) < 4`, 'the preview to give the width back');
        if ((await app.evaluate(onScreen(`${tabPanel('sound')} ${CONTENT.sound}`))) !== true) throw new Error('the Sound tab shows nothing');
      });

      await check('tableau noir et laser : les interrupteurs de Projection suivent les boutons de la barre d’outils, dans les deux sens', async () => {
        await openTab(app, 'projection');
        for (const name of ['board', 'laser']) {
          const toggle = `${tabPanel('projection')} input[data-field="${name}"]`;
          const button = `.live-toolbar button[data-action="${name}"]`;
          if ((await app.evaluate(`document.querySelector(${j(toggle)}).checked`)) !== false) throw new Error(`the ${name} switch starts on`);
          await app.click(toggle);
          await app.waitFor(`${pressed(button)} === 'true'`, `the ${name} button of the toolbar to follow the switch`);
          if ((await app.evaluate(`document.querySelector(${j(toggle)}).checked`)) !== true) throw new Error(`the ${name} switch is off while it is on`);
          // From the toolbar: the switch follows.
          await app.click(button);
          await app.waitFor(`document.querySelector(${j(toggle)}).checked === false`, `the ${name} switch to follow the toolbar button`);
          if ((await app.evaluate(pressed(button))) !== 'false') throw new Error(`the ${name} button is still on`);
        }
      });

      await check('un interrupteur cliqué garde le focus : Page suivante et Espace tournent la page, l’interrupteur ne bascule pas, B le rebascule', async () => {
        await openTab(app, 'projection');
        for (const name of ['board', 'laser']) {
          const toggle = `${tabPanel('projection')} input[data-field="${name}"]`;
          // The page number alone: turning past the last page adds one, and the total grows.
          const number = `${canvasData('page')}?.split('/')[0]`;
          const page = () => app.evaluate(number);
          await app.click(toggle);
          await app.waitFor(`document.querySelector(${j(toggle)}).checked === true`, `the ${name} switch on`);
          await app.waitFor(`document.activeElement === document.querySelector(${j(toggle)})`, `the focus on the ${name} switch`);
          const first = await page();
          // The real key path: a key event with the focus on the switch, which is not a field to type in.
          await app.key('PageDown');
          await app.waitFor(`${number} !== ${j(first)}`, `Page Down to turn the page with the ${name} switch focused`);
          const second = await page();
          await app.key('ArrowLeft');
          await app.waitFor(`${number} === ${j(first)}`, `the left arrow to turn the page back with the ${name} switch focused`);
          // Space turns the page too, and does not toggle the switch.
          await app.key(' ');
          await app.waitFor(`${number} === ${j(second)}`, `Space to turn the page with the ${name} switch focused`);
          await sleep(100);
          if ((await app.evaluate(`document.querySelector(${j(toggle)}).checked`)) !== true) throw new Error(`Space toggled the ${name} switch`);
          await app.key('ArrowLeft');
          await app.waitFor(`${number} === ${j(first)}`, 'the first page again');
          // The key of the switch's own state still works.
          await app.key(name === 'board' ? 'b' : 'l');
          await app.waitFor(`document.querySelector(${j(toggle)}).checked === false`, `the ${name} key to switch it off`);
        }
      });

      await check('Projection, replié, « Terminer », le direct rouvert : Projection et replié, le choix est dans le stockage', async () => {
        await openTab(app, 'projection');
        await app.click(tabButton('projection'));
        await inState(app, 'projection', true, 'the Projection tab collapsed');
        const written = await stored(app);
        if (!same(written, { tab: 'projection', collapsed: true })) throw new Error(`the storage holds ${j(written)}`);
        await stopLive(app);
        await openLive(app);
        const state = await sideState(app);
        if (!same(state, { where: 'live', tab: 'projection', collapsed: true })) throw new Error(`the panel reopened as ${j(state)}`);
        const again = await stored(app);
        if (!same(again, { tab: 'projection', collapsed: true })) throw new Error(`the storage now holds ${j(again)}`);
      });

      await check('un onglet retenu avant que la fenêtre ait Page : Page s’affiche d’emblée, le stockage le garde', async () => {
        await app.evaluate(`(() => { localStorage.setItem(${j(STORAGE_KEY)}, ${j('{"tab":"page"}')}); return true; })()`);
        await stopLive(app);
        await openLive(app);
        const state = await sideState(app);
        if (!same(state, { where: 'live', tab: 'page', collapsed: false })) throw new Error(`the panel shows ${j(state)} with a stored Page`);
        const kept = await stored(app);
        if (kept?.tab !== 'page') throw new Error(`the storage now holds ${j(kept)}, it should still say page`);
        if ((await app.evaluate(onScreen(`${tabPanel('page')} ${CONTENT.page}`))) !== true) throw new Error('the Page tab shows nothing');
        await openTab(app, 'layers');
      });

      await check('pastilles : le nombre de calques, un point orange quand le calque de la tablette est masqué (même replié), rien une fois affiché', async () => {
        await openTab(app, 'layers');
        const rows = await app.evaluate(layerRows);
        if (!(rows >= 1)) throw new Error('the layers zone has no row');
        if ((await app.evaluate(attr('layers', 'data-count'))) !== String(rows)) throw new Error(`the Calques tab counts ${await app.evaluate(attr('layers', 'data-count'))}, the zone has ${rows} layers`);
        if ((await app.evaluate(attr('layers', 'data-dot'))) !== null) throw new Error('the Calques tab has a dot with every layer visible');
        const eye = `${tabPanel('layers')} li.layer[data-tablet="true"] [data-action="layer-visible"]`;
        await app.click(eye);
        await app.waitFor(`${attr('layers', 'data-dot')} === 'warn'`, 'the orange dot');
        await app.waitFor(`document.querySelector(${j(`${tabPanel('layers')} [data-slot="layer-warning"]`)})?.dataset.kind === 'tabletHidden'`, 'the zone’s warning');
        if (!(await app.evaluate(tabText('layers'))).includes(l('side.dot.warn'))) throw new Error('the Calques tab does not say that a layer in use is hidden');
        await collapsePanel(app);
        if ((await app.evaluate(onScreen(`${tabButton('layers')} .side-tab-dot[data-kind="warn"]`))) !== true) throw new Error('the dot is not visible on the collapsed rail');
        await openTab(app, 'layers');
        await app.click(eye);
        await app.waitFor(`${attr('layers', 'data-dot')} === null`, 'the dot to go once the layer is shown');
      });

      await check('projeter : un point bleu sur Projection ; « Arrêter de projeter » : plus de point', async () => {
        await openTab(app, 'projection');
        if ((await app.evaluate(attr('projection', 'data-dot'))) !== null) throw new Error('the Projection tab has a dot before anything is shown');
        // As `journeys-m14-live.mjs`: the presentation window opens on a screen of this PC.
        await app.evaluate(`window.__TAURI_INTERNALS__.invoke('open_presentation', { monitor: null }).then(() => true)`);
        await app.waitFor(`${attr('projection', 'data-dot')} === 'live'`, 'the blue dot while the projection is shown', 20_000);
        if (!(await app.evaluate(tabText('projection'))).includes(l('side.dot.live'))) throw new Error('the Projection tab does not say that a projection runs');
        await app.click(`${tabPanel('projection')} button[data-action="stop-projecting"]`);
        await app.waitFor(`${attr('projection', 'data-dot')} === null`, 'the dot to go once the projection closed', 15_000);
      });

      await check('Projection : « Partager sur le réseau » démarre le partage, un point bleu ; arrêté, plus de point', async () => {
        await openTab(app, 'projection');
        const idle = l('share.block.idle');
        const status = `document.querySelector(${j(`${tabPanel('projection')} [data-slot="share-status"]`)})?.textContent?.trim() ?? null`;
        if ((await app.evaluate(status)) !== idle) throw new Error(`the share block says ${j(await app.evaluate(status))} before the share`);
        await app.click(`${tabPanel('projection')} button[data-action="share"]`);
        await app.waitFor(`document.querySelector(${j(PANEL)}) !== null`, 'the share panel', 15_000);
        await app.waitFor(`document.querySelector('[data-share-code]') !== null`, 'the access code', 15_000);
        await closePanel(app, l('common.close'));
        await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the share panel to close');
        await app.waitFor(`${attr('projection', 'data-dot')} === 'live'`, 'the blue dot while the share runs');
        const running = await app.evaluate(status);
        if (running === idle || !running) throw new Error(`the share block still says ${j(running)} while the share runs`);
        await openPanel(app);
        await app.click('button[data-action="share-stop"]');
        await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the share panel to close after the stop', 10_000);
        await app.waitFor(`${attr('projection', 'data-dot')} === null`, 'the dot to go once the share stopped');
        await app.waitFor(`(${status}) === ${j(idle)}`, 'the share block to be idle again');
      });

      await check('Son : le point rouge pendant un enregistrement (si ce PC a un micro)', async () => {
        await checkRecordingDot(app, l);
      });

      await check('Sélection (V), un lasso le panneau étant replié : il se rouvre, la barre de sélection est au-dessus de l’onglet ouvert', async () => {
        await openTab(app, 'layers');
        await app.click(SELECT_BUTTON);
        await app.waitFor(`${pressed(SELECT_BUTTON)} === 'true'`, 'the Select tool on');
        await collapsePanel(app);
        await lassoPage(app);
        await inState(app, 'layers', false, 'the pane reopened by the selection');
        // M16-D: check against package C (the selection bar and the tool hint above the open tab, §5).
        await app.waitFor(`Number(document.querySelector(${j(`${SIDE} [data-slot="side-above"] ${BAR} .select-count`)})?.dataset.count ?? 0) > 0`, 'the selection bar above the open tab');
        const place = await app.evaluate(`(() => {
          const above = document.querySelector(${j(`${SIDE} [data-slot="side-above"]`)}).getBoundingClientRect();
          const tab = document.querySelector(${j(tabPanel('layers'))}).getBoundingClientRect();
          return { above: above.top, aboveBottom: above.bottom, tab: tab.top };
        })()`);
        if (!(place.above < place.tab)) throw new Error(`the selection bar is not above the tab: ${j(place)}`);
        if (!(place.aboveBottom <= place.tab + 1)) throw new Error(`the selection bar overlaps the tab: ${j(place)}`);
        // Padded like the editor's blocks, not flush against the edge of the pane.
        const padding = await app.evaluate(`parseFloat(getComputedStyle(document.querySelector(${j(`${SIDE} [data-slot="side-above"] > .pane-block`)})).paddingLeft)`);
        if (!(padding >= 8)) throw new Error(`the block above the tab has a left padding of ${padding}px`);
        if ((await app.evaluate(onScreen(`${SIDE} [data-slot="side-above"] ${BAR} button[data-action="delete-selection"]`))) !== true) throw new Error('the bar has no Delete button on screen');
        if ((await app.evaluate(onScreen(`${SIDE} [data-slot="side-above"] [data-slot="select-hint"]`))) !== true) throw new Error('the Select tool’s hint is not above the tab');
        // The bar no longer floats over the preview.
        if ((await app.evaluate(`document.querySelector('.live-preview ${BAR}') !== null`)) === true) throw new Error('the selection bar is still over the preview');
        await app.click(`${SIDE} ${BAR} button[data-action="deselect"]`);
        await app.waitFor(`document.querySelector(${j(BAR)}) === null`, 'the selection bar to go');
        await app.click(SELECT_BUTTON);
        await app.waitFor(`${pressed(SELECT_BUTTON)} === 'false'`, 'the Select tool off');
      });

      await check('un onglet au focus puis Espace : la page suivante, l’onglet ne bascule pas ; Entrée le replie puis le rouvre', async () => {
        await openTab(app, 'projection');
        await app.waitFor(`document.activeElement?.getAttribute('role') === 'tab' && document.activeElement.dataset.tab === 'projection'`, 'the focus on the tab');
        await app.waitFor(`${canvasData('page')}?.startsWith('1/')`, 'the first page');
        await app.key(' ');
        await app.waitFor(`${canvasData('page')}?.startsWith('2/')`, 'the next page');
        const state = await sideState(app);
        if (!same(state, { where: 'live', tab: 'projection', collapsed: false })) throw new Error(`Space changed the panel: ${j(state)}`);
        await app.key('ArrowLeft');
        await app.waitFor(`${canvasData('page')}?.startsWith('1/')`, 'the first page again');
        // Enter is how a keyboard user opens and folds the tabs here.
        await pressEnter(app);
        await inState(app, 'projection', true, 'Enter to collapse the Projection tab');
        await pressEnter(app);
        await inState(app, 'projection', false, 'Enter to open the Projection tab again');
        await sleep(100);
      });
    },
  },
];
