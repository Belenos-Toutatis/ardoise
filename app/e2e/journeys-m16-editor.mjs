// M16 journey, the editor's side panel (docs/design/side-panel.md, §10 "End-to-end journeys"): six tabs, each with its
// content, collapse and reopening, the keyboard on the rail, the view remembered when the note is closed and opened
// again, the selection above the open tab, the replay showing Son, the badges (the hidden layer, the sharing) and the
// share started from the Projection tab (the button has left the toolbar).
//
// On the production executable, in French and in English; the share is on the loopback (`[share] port` and
// `ARDOISE_SHARE_LOOPBACK`, as `journeys-m11-share.mjs`). The helpers of `side-panel.mjs` open a tab before the journey
// acts in it: a tab's content stays mounted but hidden, and a click on a hidden control would land elsewhere.
//
// Written without a run (the other packages were not merged when this was written): the selectors are the doc's
// (§2 "DOM contract", §5, §7). The lines that rely on a hook the doc leaves to a package carry `M16-D: check`.
//
// The red recording dot is checked on a real recording of the simulated microphone (`--simulated-microphone`, M17: the
// PC's own microphone stays off), in a replay; it is skipped with a log line when recording cannot start (O12; vitest
// checks the dot with `mock-narration`). « Found again on reopening » (O13) is the editor closed and opened again in the same run, and the
// value stored in the browser's storage read back; the runner gives each journey a fresh profile, so no restart is tried.
import { closePanel, openPanel, PANEL } from './journeys-m11-share.mjs';
import { closeEditor, text } from './journeys-m12-resume.mjs';
import { checkRecordingDot, collapsePanel, openTab, SIDE, sideState, tabButton, tabPanel } from './side-panel.mjs';

const j = JSON.stringify;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** The rail's tabs in the editor in M16, in order (the Page, Fond, Son and Aide tabs exist here; docs/design/side-panel.md §1). */
const TABS = ['layers', 'page', 'background', 'sound', 'projection', 'help'];
const STORAGE_KEY = 'ardoise.sidePanel.editor';
/** What each tab shows, one control or block that must be on screen once the tab is open (§7). */
const CONTENT = {
  layers: 'section.layers-zone[data-where="editor"]',
  page: '[data-section="page-format"]',
  background: '[data-section="background-image"]',
  // M17: the mode of the recording (« Pendant que j'écris » by default, its own recording below it).
  sound: '[data-section="narration"] [data-field="narration-mode"]',
  projection: '[data-section="share"] button[data-action="share"]',
  help: '[data-section="shortcuts"] dl.keys',
};
/** The other blocks of the Page tab (§7). */
const PAGE_BLOCKS = ['[data-section="paper"]', '[data-section="turn-crop"]', '[data-section="original"]'];

const STROKES = '.editor g.ink path[data-stroke]';
const strokeCount = `document.querySelectorAll(${j(STROKES)}).length`;
const stageWidth = `document.querySelector('.editor .stage').getBoundingClientRect().width`;
/** The layers of the zone (it stays mounted when its tab is not the open one). */
const layerRows = `document.querySelectorAll('${CONTENT.layers} li.layer').length`;

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
/** The view the editor stored (the browser's storage of the WebView2 profile), or null. */
const stored = (app) =>
  app.evaluate(`(() => { try { return JSON.parse(localStorage.getItem(${j(STORAGE_KEY)}) ?? 'null'); } catch { return null; } })()`);
const same = (a, b) => j(a) === j(b);

/** How many strokes the selection block says are selected (0 when there is none); read in the block above the open tab. */
const selectedCount = (l) => {
  const escape = (part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const [before, after] = l('editor.selection.count').split('{count}');
  const source = `${escape(before)}(\\d+)${escape(after)}`;
  return `(() => { const m = (document.querySelector('${SIDE} [data-slot="side-above"]')?.textContent ?? '').match(new RegExp(${j(source)})); return m ? Number(m[1]) : 0; })()`;
};

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

export const sidePanelEditorJourneys = [
  {
    name: 'panneau de côté de l’éditeur : onglets, repli, pastilles, sélection, partage',
    args: ['--simulated-microphone'],
    settings: (ctx) => `[share]\nport = ${ctx.sharePort}\n`,
    env: { ARDOISE_SHARE_LOOPBACK: '1' },
    async run(app, check) {
      const l = (key) => text(app.language, key);

      await check('ouvrir une note de démonstration : six onglets dans l’ordre, Calques ouvert', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('.note-card .card-open');
        await app.waitFor(`${strokeCount} > 0`, 'the strokes of the note');
        // The page is fitted once the editor has measured its stage.
        await sleep(400);
        await app.waitFor(`document.querySelector(${j(SIDE)}) !== null`, 'the side panel');
        const rail = await app.evaluate(`[...document.querySelectorAll(${j(`${SIDE} .side-rail [role="tab"]`)})].map((b) => b.dataset.tab)`);
        if (!same(rail, TABS)) throw new Error(`the rail has the tabs ${j(rail)}, expected ${j(TABS)}`);
        const state = await sideState(app);
        if (!same(state, { where: 'editor', tab: 'layers', collapsed: false })) throw new Error(`the panel starts as ${j(state)}`);
        const aside = await app.evaluate(`document.querySelector('aside.editor-side')?.getAttribute('aria-label') ?? null`);
        if (aside !== l('side.label')) throw new Error(`the aside is labelled ${j(aside)}, expected ${j(l('side.label'))}`);
        // Titles: the open tab says it folds, the others say their name; the pane's title is the open tab's.
        const open = await app.evaluate(attr('layers', 'title'));
        if (open !== l('side.openTitle').replace('{tab}', l('side.tabs.layers'))) throw new Error(`the open tab's title is ${j(open)}`);
        const other = await app.evaluate(attr('page', 'title'));
        if (other !== l('side.tabs.page')) throw new Error(`the Page tab's title is ${j(other)}`);
        const title = await app.evaluate(`document.querySelector(${j(`${SIDE} [data-slot="side-title"]`)})?.textContent?.trim() ?? null`);
        if (title !== l('side.tabs.layers')) throw new Error(`the pane's title is ${j(title)}`);
        if ((await app.evaluate(`document.querySelector('.toolbar [data-action="share"]') !== null`)) === true) {
          throw new Error('the share button is still in the toolbar');
        }
      });

      await check('chaque onglet montre son contenu, et lui seul', async () => {
        for (const tab of TABS) {
          await openTab(app, tab);
          const shown = await app.evaluate(`[...document.querySelectorAll(${j(`${SIDE} [data-tab-panel]`)})].filter((p) => !p.hidden).map((p) => p.dataset.tabPanel)`);
          if (!same(shown, [tab])) throw new Error(`with ${tab} open the panels shown are ${j(shown)}`);
          const content = `${tabPanel(tab)} ${CONTENT[tab]}`;
          if ((await app.evaluate(onScreen(content))) !== true) throw new Error(`${content} is not on screen in the ${tab} tab`);
          const title = await app.evaluate(`document.querySelector(${j(`${SIDE} [data-slot="side-title"]`)})?.textContent?.trim() ?? null`);
          if (title !== l(`side.tabs.${tab}`)) throw new Error(`the pane's title is ${j(title)} in the ${tab} tab`);
        }
        for (const block of PAGE_BLOCKS) {
          await openTab(app, 'page');
          if ((await app.evaluate(onScreen(`${tabPanel('page')} ${block}`))) !== true) throw new Error(`${block} is not on screen in the Page tab`);
        }
        // Son (M17): « Pendant que j'écris » is the mode shown by default, with its own recording.
        await openTab(app, 'sound');
        const mode = await app.evaluate(`document.querySelector(${j(`${tabPanel('sound')} ${CONTENT.sound} input:checked`)})?.value ?? null`);
        if (mode !== 'writing') throw new Error(`the Sound tab opens on the mode ${j(mode)}, expected writing`);
        if ((await app.evaluate(onScreen(`${tabPanel('sound')} [data-action="narration-open"]`))) !== true) throw new Error('the Sound tab has no recording while writing');
        // The help lists every entry of the editor's table that has no group, with its label (the replay's two are in their own list).
        await openTab(app, 'help');
        const grouped = ['leaveReplay', 'playPause'];
        const expected = Object.keys(l('editor.keys.list'))
          .filter((id) => !grouped.includes(id))
          .map((id) => l(`editor.keys.list.${id}`))
          .sort();
        const first = `${tabPanel('help')} [data-section="shortcuts"] > dl.keys:first-of-type`;
        const shown = await app.evaluate(`[...document.querySelectorAll(${j(`${first} dd`)})].map((dd) => dd.textContent.trim()).sort()`);
        if (!same(shown, expected)) throw new Error(`the help lists ${j(shown)}, expected ${j(expected)}`);
        const keys = await app.evaluate(`document.querySelectorAll(${j(`${first} dt`)}).length`);
        if (keys !== expected.length) throw new Error(`the help has ${keys} entries, expected ${expected.length}`);
        if ((await app.evaluate(`document.querySelectorAll(${j(`${first} dt kbd`)}).length`)) < expected.length) throw new Error('the help shows no key for some entries');
        await openTab(app, 'layers');
      });

      await check('un clic sur l’onglet ouvert replie le panneau (la page gagne au moins 200 px), un autre onglet le rouvre', async () => {
        const narrow = await app.evaluate(stageWidth);
        await app.click(tabButton('layers'));
        await inState(app, 'layers', true, 'the pane collapsed');
        await app.waitFor(`${stageWidth} - ${narrow} >= 200`, 'the page to take the width of the pane');
        // The CSS `hidden` is not overridden by a display rule (R2): the pane has no box, the rail stays.
        if ((await app.evaluate(onScreen(`${SIDE} .side-pane`))) === true) throw new Error('the pane is still displayed once collapsed');
        if ((await app.evaluate(onScreen(`${SIDE} .side-rail`))) !== true) throw new Error('the rail is gone with the pane');
        if ((await app.evaluate(attr('layers', 'aria-expanded'))) !== 'false') throw new Error('the shown tab does not say it is collapsed');
        // A badge stays on the rail, collapsed: the count of the layers.
        if (!((await app.evaluate(attr('layers', 'data-count'))) >= 1)) throw new Error('the Calques tab has no layer count');
        await app.click(tabButton('page'));
        await inState(app, 'page', false, 'the Page tab open');
        await app.waitFor(`Math.abs(${stageWidth} - ${narrow}) < 4`, 'the page to give the width back');
        if ((await app.evaluate(onScreen(`${tabPanel('page')} ${CONTENT.page}`))) !== true) throw new Error('the Page tab shows nothing');
      });

      await check('« › » replie aussi, le focus va sur l’onglet affiché ; les flèches déplacent le focus sans rien activer, Entrée ouvre', async () => {
        await app.click(`${SIDE} [data-action="side-collapse"]`);
        await inState(app, 'page', true, 'the pane collapsed with the fold button');
        // M16-D: check against package A (the focus goes to the shown tab after `tick()`).
        await app.waitFor(`document.activeElement?.getAttribute('role') === 'tab' && document.activeElement.dataset.tab === 'page'`, 'the focus on the shown tab');
        await app.key('ArrowDown');
        await app.waitFor(`document.activeElement?.dataset.tab === 'background'`, 'the focus one tab down');
        await app.key('ArrowDown');
        await app.waitFor(`document.activeElement?.dataset.tab === 'sound'`, 'the focus two tabs down');
        // A move activates nothing: the pane would reopen at each press.
        const state = await sideState(app);
        if (!same(state, { where: 'editor', tab: 'page', collapsed: true })) throw new Error(`the arrows changed the panel: ${j(state)}`);
        await pressEnter(app);
        await inState(app, 'sound', false, 'Enter to open the Sound tab');
        if ((await app.evaluate(onScreen(`${tabPanel('sound')} ${CONTENT.sound}`))) !== true) throw new Error('the Sound tab shows nothing');
        await app.key('ArrowUp');
        await app.waitFor(`document.activeElement?.dataset.tab === 'background'`, 'the focus one tab up');
      });

      await check('Page, replié, retour à la bibliothèque, la note rouverte : Page et replié, le choix est dans le stockage', async () => {
        await openTab(app, 'page');
        await app.click(tabButton('page'));
        await inState(app, 'page', true, 'the Page tab collapsed');
        const written = await stored(app);
        if (!same(written, { tab: 'page', collapsed: true })) throw new Error(`the storage holds ${j(written)}`);
        await closeEditor(app);
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the library');
        await app.click('.note-card .card-open');
        await app.waitFor(`${strokeCount} > 0`, 'the strokes of the note');
        await app.waitFor(`document.querySelector(${j(SIDE)}) !== null`, 'the side panel');
        const state = await sideState(app);
        if (!same(state, { where: 'editor', tab: 'page', collapsed: true })) throw new Error(`the panel reopened as ${j(state)}`);
        const again = await stored(app);
        if (!same(again, { tab: 'page', collapsed: true })) throw new Error(`the storage now holds ${j(again)}`);
        await sleep(400);
      });

      await check('Ctrl+A, le panneau étant replié : il se rouvre, la sélection est au-dessus de l’onglet ouvert', async () => {
        await app.key('a', { ctrl: true });
        await inState(app, 'page', false, 'the pane reopened by the selection');
        // M16-D: check against B1 (the block above the open tab and its hooks, §5).
        await app.waitFor(`${selectedCount(l)} > 0`, 'the selection count above the open tab');
        const place = await app.evaluate(`(() => {
          const above = document.querySelector(${j(`${SIDE} [data-slot="side-above"]`)}).getBoundingClientRect();
          const tab = document.querySelector(${j(tabPanel('page'))}).getBoundingClientRect();
          return { above: above.top, aboveBottom: above.bottom, tab: tab.top };
        })()`);
        if (!(place.above < place.tab)) throw new Error(`the selection is not above the tab: ${j(place)}`);
        if (!(place.aboveBottom <= place.tab + 1)) throw new Error(`the selection overlaps the tab: ${j(place)}`);
        if ((await app.evaluate(onScreen(`${SIDE} [data-section="selection"] [data-action="copy-selection"]`))) !== true) throw new Error('the selection block has no Copy button on screen');
        // The tab shown did not change: the selection comes above whatever is open.
        await openTab(app, 'layers');
        if ((await app.evaluate(`document.querySelector(${j(`${SIDE} [data-slot="side-above"]`)}) !== null`)) !== true) throw new Error('the selection left with the tab change');
        await app.click(`${SIDE} [data-action="clear-selection"]`);
        await app.waitFor(`document.querySelector(${j(`${SIDE} [data-slot="side-above"]`)}) === null`, 'the block above to go with the selection');
      });

      await check('Relecture (R) : l’onglet Son s’ouvre, même replié ; R encore rend l’onglet et l’état d’avant', async () => {
        await collapsePanel(app);
        const before = await stored(app);
        await app.key('r');
        await inState(app, 'sound', false, 'the replay showing the Sound tab');
        // The view the replay forces is not remembered: closing the editor now would keep the user's own.
        const during = await stored(app);
        if (!same(during, before)) throw new Error(`the replay changed the stored view from ${j(before)} to ${j(during)}`);
        await app.waitFor(`document.querySelector('button[data-action="replay-toggle"]')?.getAttribute('aria-pressed') === 'true'`, 'the replay');
        // M16-D: check against B1 (the narration block of a replay holds RecordPanel's open button, §7).
        await app.waitFor(onScreen(`${tabPanel('sound')} [data-section="narration"] [data-action="narration-open"]`), 'the narration panel in the Sound tab');
        // The recording dot, on the simulated microphone (O12).
        await checkRecordingDot(app, l, { simulated: true });
        await app.key('r');
        await inState(app, 'layers', true, 'the replay to give the tab and the collapsed state back');
        await app.waitFor(`document.querySelector('button[data-action="replay-toggle"]')?.getAttribute('aria-pressed') === 'false'`, 'the replay to end');
      });

      await check('pastilles : le nombre de calques, un point orange quand le calque choisi est masqué (même replié), rien une fois affiché', async () => {
        await openTab(app, 'layers');
        const rows = await app.evaluate(layerRows);
        if (!(rows >= 1)) throw new Error('the layers zone has no row');
        if ((await app.evaluate(attr('layers', 'data-count'))) !== String(rows)) throw new Error(`the Calques tab counts ${await app.evaluate(attr('layers', 'data-count'))}, the zone has ${rows} layers`);
        if ((await app.evaluate(attr('layers', 'data-dot'))) !== null) throw new Error('the Calques tab has a dot with every layer visible');
        const eye = `${tabPanel('layers')} li.layer[data-chosen="true"] [data-action="layer-visible"]`;
        await app.click(eye);
        await app.waitFor(`${attr('layers', 'data-dot')} === 'warn'`, 'the orange dot');
        // The dot and the zone's notice agree (both come from `layerWarnings`).
        await app.waitFor(`document.querySelector(${j(`${tabPanel('layers')} [data-slot="layer-warning"]`)})?.dataset.kind === 'chosenHidden'`, 'the zone’s warning');
        if (!(await app.evaluate(tabText('layers'))).includes(l('side.dot.warn'))) throw new Error('the Calques tab does not say that a layer in use is hidden');
        await collapsePanel(app);
        if ((await app.evaluate(onScreen(`${tabButton('layers')} .side-tab-dot[data-kind="warn"]`))) !== true) throw new Error('the dot is not visible on the collapsed rail');
        await openTab(app, 'layers');
        await app.click(eye);
        await app.waitFor(`${attr('layers', 'data-dot')} === null`, 'the dot to go once the layer is shown');
        if ((await app.evaluate(attr('layers', 'data-count'))) !== String(rows)) throw new Error('the layer count changed');
      });

      await check('Projection : « Partager sur le réseau » ouvre le panneau du partage, fermé, un point bleu ; arrêté, plus de point', async () => {
        await openTab(app, 'projection');
        const idle = l('share.block.idle');
        const status = `document.querySelector(${j(`${tabPanel('projection')} [data-slot="share-status"]`)})?.textContent?.trim() ?? null`;
        if ((await app.evaluate(status)) !== idle) throw new Error(`the share block says ${j(await app.evaluate(status))} before the share`);
        if ((await app.evaluate(attr('projection', 'data-dot'))) !== null) throw new Error('the Projection tab has a dot before any share');
        await app.click(`${tabPanel('projection')} button[data-action="share"]`);
        await app.waitFor(`document.querySelector(${j(PANEL)}) !== null`, 'the share panel', 15_000);
        await app.waitFor(`document.querySelector('[data-share-code]') !== null`, 'the access code', 15_000);
        await closePanel(app, l('common.close'));
        await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the share panel to close');
        await app.waitFor(`${attr('projection', 'data-dot')} === 'live'`, 'the blue dot on the Projection tab');
        if (!(await app.evaluate(tabText('projection'))).includes(l('side.dot.live'))) throw new Error('the Projection tab does not say that a share runs');
        const running = await app.evaluate(status);
        if (running === idle || !running) throw new Error(`the share block still says ${j(running)} while the share runs`);
        if ((await app.evaluate(`document.querySelector('.toolbar [data-action="share"]') !== null`)) === true) throw new Error('the share button is in the toolbar');
        // Stopped from the panel the button opens.
        await openPanel(app);
        await app.click('button[data-action="share-stop"]');
        await app.waitFor(`document.querySelector(${j(PANEL)}) === null`, 'the share panel to close after the stop', 10_000);
        await app.waitFor(`${attr('projection', 'data-dot')} === null`, 'the dot to go once the share stopped');
        await app.waitFor(`(${status}) === ${j(idle)}`, 'the share block to be idle again');
      });
    },
  },
];
