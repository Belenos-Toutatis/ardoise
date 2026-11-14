// Journeys « touches du navigateur et clic droit » (retour de l'utilisateur du 2026-11-14), on the production executable, in
// French and in English, with real input events through the real window.
//
// 1. Ctrl + P (and Ctrl + Maj + P) open Ardoise's print dialog, in the library (the selected notes; the notice when nothing is
//    selected) and in the editor, and the web view's own print never appears: the page still answers, no other
//    WebView2 page (print preview, view-source, downloads…) is opened, and the page is not reloaded by F5 or Ctrl + R.
// 2. The right click: on a card, a menu of the app opens at the pointer; over the page background, the thumbnails, the
//    logo… no menu at all (the `contextmenu` event is cancelled); a right click in a text field is left to the system
//    (checked on a synthetic event: a real one would open the system's blocking menu); in the editor, over a stroke it
//    selects it and opens the page menu at the pointer, over the bare page it opens the other one; a pen's button opens none.
//
// A key the web view handles itself (reload, print…) leaves a trace the page can see: the page reloaded (a marker on `window`
// is lost) or another page target appeared in the DevTools list (`edge://print/` for the print preview). Those keys only
// reach the web view's accelerators through the real keyboard path, not through the DevTools protocol's key events (checked
// on the previous executable: F5 sent by the protocol reloads nothing, F5 sent by `real-key.ps1` reloads, Ctrl + P opens
// `edge://print/`), so they are sent by `real-key.ps1` (SendKeys to the window of this very process, refused unless it is
// the foreground one). What the page sees of each event is recorded by a listener.
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const j = JSON.stringify;
const REAL_KEY = join(dirname(fileURLToPath(import.meta.url)), 'real-key.ps1');
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const CARD = '.note-card:nth-child(2)';
const PRINT = '[data-dialog="print"]';

/** Installs the marker (lost by a reload) and the recorders of what the page sees of the keys and of the right click. */
const RECORD = `(() => {
  window.__marker = 'alive';
  window.__keys = [];
  window.__menus = [];
  window.addEventListener('keydown', (e) => {
    const entry = { key: e.key, ctrl: e.ctrlKey, shift: e.shiftKey, prevented: null };
    window.__keys.push(entry);
    // After the dispatch: every handler has had its say.
    setTimeout(() => (entry.prevented = e.defaultPrevented), 0);
  });
  window.addEventListener('contextmenu', (e) => {
    const entry = { target: e.target.tagName, type: e.constructor.name, pointerType: e.pointerType ?? null, button: e.button, x: e.clientX, y: e.clientY, prevented: null };
    window.__menus.push(entry);
    setTimeout(() => (entry.prevented = e.defaultPrevented), 0);
  });
  return true;
})()`;

/** The pages the DevTools endpoint lists: the print preview, a view-source or downloads page would be one more. */
async function pages(app) {
  const list = await (await fetch(`http://127.0.0.1:${app.port}/json/list`)).json();
  return list.map((t) => `${t.type} ${t.url}`);
}

/**
 * A key of the real keyboard (SendKeys syntax: `^p`, `{F5}`) sent to this window only: the page must have the focus first
 * (a key sent before the web view has it would be lost).
 */
async function realKey(app, keys) {
  await app.send('Page.bringToFront');
  await app.waitFor('document.hasFocus()', 'the page to have the focus');
  // The window may not be the foreground one yet (code 2: nothing was sent): a few tries.
  let status = 0;
  for (let attempt = 0; attempt < 5; attempt++) {
    try {
      execFileSync('powershell', ['-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', REAL_KEY, '-ProcessId', String(app.pid), '-Keys', keys], { stdio: 'ignore' });
      status = 0;
      break;
    } catch (error) {
      status = error.status;
      await sleep(500);
    }
  }
  if (status !== 0) throw new Error(`the window could not take the focus for the real key ${keys} (code ${status})`);
  await sleep(500);
}

/**
 * A real key, then its effect: sent again (three times at most) when nothing came of it, because the web view sometimes
 * has not its keyboard focus back yet when the window has just closed a dialog. A second Ctrl + P under an open dialog
 * does nothing, so a late effect is not doubled.
 */
async function realKeyUntil(app, keys, expression, what) {
  for (let attempt = 0; ; attempt++) {
    await realKey(app, keys);
    try {
      await app.waitFor(expression, what, 4000);
      return;
    } catch (error) {
      if (attempt >= 2) throw error;
    }
  }
}

/** A key through the protocol with its Windows key code. */
async function rawKey(app, key, { code = key, vk, ctrl = false, shift = false } = {}) {
  const modifiers = (ctrl ? 2 : 0) | (shift ? 8 : 0);
  const base = { key, code, modifiers, windowsVirtualKeyCode: vk };
  await app.send('Input.dispatchKeyEvent', { type: 'rawKeyDown', ...base });
  await app.send('Input.dispatchKeyEvent', { type: 'keyUp', ...base });
}

/** A real right click (or a pen's) at a point of the window. */
async function rightClick(app, x, y, pointerType = 'mouse') {
  await app.send('Input.dispatchMouseEvent', { type: 'mouseMoved', x, y, button: 'none', buttons: 0, pointerType });
  await app.send('Input.dispatchMouseEvent', { type: 'mousePressed', x, y, button: 'right', buttons: 2, clickCount: 1, pointerType });
  await app.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x, y, button: 'right', buttons: 0, clickCount: 1, pointerType });
}

const menuOpen = (app) => app.evaluate(`document.querySelector('[role="menu"]') !== null`);
// After a moment: `prevented` is read once the event's dispatch is over.
const lastMenuEvent = async (app) => {
  await sleep(120);
  return app.evaluate(`window.__menus.at(-1) ?? null`);
};

async function closeMenu(app) {
  if (await menuOpen(app)) {
    await app.key('Escape');
    await app.waitFor(`document.querySelector('[role="menu"]') === null`, 'the menu to close');
  }
}

export const keysMenusJourneys = [
  {
    name: 'Ctrl + P imprime avec Ardoise, jamais avec le navigateur ; F5 et Ctrl + R ne rechargent pas',
    async run(app, check) {
      const baseline = [];
      await check('bibliothèque : les 8 notes de démonstration, les enregistreurs en place', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await app.evaluate(RECORD);
        baseline.push(...(await pages(app)));
      });

      await check('Ctrl + P avec une note cochée : la boîte « Imprimer » d’Ardoise s’ouvre, la page répond, aucune autre page', async () => {
        await app.click(`${CARD} .card-check input`);
        await app.waitFor(`document.querySelector('.selection-bar') !== null`, 'the selection bar');
        await realKeyUntil(app, '^p', `document.querySelector(${j(PRINT)}) !== null`, 'Ardoise’s print dialog');
        await sleep(800);
        const seen = await app.evaluate(`window.__keys.find((k) => k.key === 'p' && k.ctrl)`);
        if (!seen?.prevented) throw new Error(`the key was not cancelled: ${j(seen)}`);
        if ((await app.evaluate('window.__marker')) !== 'alive') throw new Error('the page was reloaded');
        const now = await pages(app);
        if (j(now) !== j(baseline)) throw new Error(`another page appeared: ${j(now)}`);
        // The page still answers: Escape closes the dialog.
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${j(PRINT)}) === null`, 'the dialog to close');
      });

      await check('Ctrl + Maj + P fait la même chose', async () => {
        await realKeyUntil(app, '^+p', `document.querySelector(${j(PRINT)}) !== null`, 'Ardoise’s print dialog');
        await sleep(500);
        const now = await pages(app);
        if (j(now) !== j(baseline)) throw new Error(`another page appeared: ${j(now)}`);
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${j(PRINT)}) === null`, 'the dialog to close');
      });

      await check('sans sélection, Ctrl + P ne devine pas : un message dit quoi faire', async () => {
        await app.click(`${CARD} .card-check input`);
        await app.waitFor(`document.querySelector('.selection-bar') === null`, 'the selection to go');
        await realKeyUntil(app, '^p', `document.querySelector('.library-main .notice') !== null`, 'the notice');
        if (await app.evaluate(`document.querySelector(${j(PRINT)}) !== null`)) throw new Error('a dialog opened');
        const now = await pages(app);
        if (j(now) !== j(baseline)) throw new Error(`another page appeared: ${j(now)}`);
      });

      await check('F5, Ctrl + R, Ctrl + U, Ctrl + J, Ctrl + S, Ctrl + O, Ctrl + F : la page n’est pas rechargée et rien ne s’ouvre', async () => {
        for (const keys of ['{F5}', '^r', '^+r', '^u', '^j', '^s', '^o', '^f']) {
          await realKey(app, keys);
          await sleep(300);
          if ((await app.evaluate('window.__marker')) !== 'alive') throw new Error(`${keys}: the page was reloaded`);
          const now = await pages(app);
          if (j(now) !== j(baseline)) throw new Error(`${keys}: another page appeared: ${j(now)}`);
        }
        if ((await app.evaluate('window.__marker')) !== 'alive') throw new Error('the page was reloaded');
        const now = await pages(app);
        if (j(now) !== j(baseline)) throw new Error(`another page appeared: ${j(now)}`);
        // Ctrl + F is the library's search: the key still reached the page.
        const search = await app.evaluate(`document.activeElement === document.querySelector('.topbar input.search')`);
        if (!search) throw new Error('Ctrl + F no longer focuses the search field');
      });

      await check('éditeur : Ctrl + P ouvre « Imprimer » de la note ouverte', async () => {
        await app.key('Escape');
        await app.click(`${CARD} .card-open`);
        await app.waitFor(`document.querySelectorAll('.editor g.ink path[data-stroke]').length > 0`, 'the strokes of the note');
        await realKey(app, '^p');
        await app.waitFor(`document.querySelector('.editor ${PRINT}') !== null`, 'the editor’s print dialog');
        await sleep(500);
        const now = await pages(app);
        if (j(now) !== j(baseline)) throw new Error(`another page appeared: ${j(now)}`);
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${j(PRINT)}) === null`, 'the dialog to close');
        if ((await app.evaluate('window.__marker')) !== 'alive') throw new Error('the page was reloaded');
      });
    },
  },
  {
    name: 'clic droit : le menu d’Ardoise sous le pointeur, jamais celui du navigateur',
    async run(app, check) {
      const centre = async (selector) => {
        const box = await app.centre(selector);
        return { x: box.x, y: box.y };
      };

      await check('bibliothèque : une carte ouvre son menu sous le pointeur', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length === 8`, 'the 8 demo notes');
        await app.evaluate(RECORD);
        const { x, y } = await centre(`${CARD} .card-open`);
        await rightClick(app, x, y);
        await app.waitFor(`document.querySelector('[role="menu"]') !== null`, 'the card menu');
        const at = await app.evaluate(`(() => { const m = document.querySelector('[role="menu"]'); const r = m.getBoundingClientRect(); return { left: r.left, top: r.top, parent: m.parentElement.tagName, label: m.getAttribute('aria-label') }; })()`);
        if (Math.abs(at.left - x) > 2 || Math.abs(at.top - y) > 2) throw new Error(`the menu is at ${at.left},${at.top}, the pointer at ${x},${y}`);
        if (at.parent !== 'BODY') throw new Error(`the menu is in ${at.parent}`);
        const event = await lastMenuEvent(app);
        console.log(`    contextmenu : ${j(event)}`);
        if (!(await lastMenuEvent(app)).prevented) throw new Error('the web view’s menu was not cancelled');
      });

      await check('Échap ferme le menu et rend le focus ; une barre de menu au clavier s’ouvre sous le bouton « ⋯ »', async () => {
        await closeMenu(app);
        const focused = await app.evaluate(`document.activeElement?.closest('.card-menu') !== null`);
        if (!focused) throw new Error('the focus did not go back to the « ⋯ » button');
        await app.evaluate(`document.querySelector(${j(`${CARD} .card-open`)}).focus()`);
        await rawKey(app, 'ContextMenu', { code: 'ContextMenu', vk: 93 });
        await sleep(500);
        const event = await lastMenuEvent(app);
        console.log(`    contextmenu du clavier : ${j(event)}`);
        if (await menuOpen(app)) {
          const inside = await app.evaluate(`document.querySelector(${j(CARD)}).contains(document.querySelector('[role="menu"]'))`);
          if (!inside) throw new Error('the Menu key opened the menu away from the card’s « ⋯ » button');
          await closeMenu(app);
        } else {
          console.log('    (la touche Menu envoyée par le protocole n’a pas fait naître d’événement)');
        }
      });

      await check('ailleurs : aucun menu (titre de la bibliothèque, nom de l’application, logo, titre d’une section de la barre latérale)', async () => {
        for (const selector of ['.library-scroll .section-header h2', '.topbar .brand', '.topbar .logo, .topbar svg', '.sidebar .nav-section h3']) {
          const before = (await app.evaluate('window.__menus.length')) ?? 0;
          const { x, y } = await centre(selector);
          await rightClick(app, x, y);
          await sleep(250);
          const event = await lastMenuEvent(app);
          if ((await app.evaluate('window.__menus.length')) === before) throw new Error(`${selector}: no contextmenu event`);
          if (!event.prevented) throw new Error(`${selector}: the web view’s menu was not cancelled`);
          if (await menuOpen(app)) throw new Error(`${selector}: a menu opened`);
        }
      });

      await check('un champ de texte garde le menu du système (événement synthétique : le vrai bloquerait)', async () => {
        const prevented = await app.evaluate(`(() => {
          const field = document.querySelector('.topbar input.search');
          const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2 });
          field.dispatchEvent(event);
          return event.defaultPrevented;
        })()`);
        if (prevented) throw new Error('the text field lost the system’s menu');
      });

      await check('éditeur : sur un trait, le trait est choisi et le menu de la page s’ouvre sous le pointeur', async () => {
        await app.click(`${CARD} .card-open`);
        await app.waitFor(`document.querySelectorAll('.editor g.ink path[data-stroke]').length > 0`, 'the strokes of the note');
        await app.key('v');
        // The demo note's strokes are on its first layer; the selection acts on the chosen one (the second, on top, at first).
        await app.click('.editor li.layer[data-layer="0"] [data-action="choose-layer"]');
        await app.waitFor(`document.querySelector('.editor li.layer[data-layer="0"]').dataset.chosen === 'true'`, 'the first layer chosen');
        const spot = await app.evaluate(`(() => {
          const path = document.querySelector('.editor g.ink path[data-stroke]');
          const point = path.getPointAtLength(path.getTotalLength() * 0.25);
          const m = path.getScreenCTM();
          return { x: point.x * m.a + m.e, y: point.y * m.d + m.f };
        })()`);
        await rightClick(app, spot.x, spot.y);
        await app.waitFor(`document.querySelector('[role="menu"]') !== null`, 'the page menu');
        const info = await app.evaluate(`(() => { const m = document.querySelector('[role="menu"]'); const r = m.getBoundingClientRect(); return { left: r.left, top: r.top, items: [...m.querySelectorAll('[role="menuitem"]')].map((b) => b.dataset.menuItem), selected: document.querySelector('[data-section="selection"]') !== null }; })()`);
        if (!info.selected) throw new Error('the stroke under the pointer was not selected');
        if (!info.items.includes('copy') || !info.items.includes('delete')) throw new Error(`entries: ${j(info.items)}`);
        if (Math.abs(info.left - spot.x) > 2) throw new Error(`the menu is at ${info.left}, the pointer at ${spot.x}`);
        if (!(await lastMenuEvent(app)).prevented) throw new Error('the web view’s menu was not cancelled');
        await closeMenu(app);
      });

      await check('éditeur : sur la page vide, l’autre menu (Coller, Tout sélectionner, Annuler, Rétablir)', async () => {
        await app.key('Escape');
        await app.waitFor(`document.querySelector('[data-section="selection"]') === null`, 'the selection to go');
        const spot = await app.evaluate(`(() => {
          const r = document.querySelector('.editor svg.canvas').getBoundingClientRect();
          return { x: r.left + r.width * 0.9, y: r.top + r.height * 0.4 };
        })()`);
        await rightClick(app, spot.x, spot.y);
        await app.waitFor(`document.querySelector('[role="menu"]') !== null`, 'the page menu');
        const items = await app.evaluate(`[...document.querySelectorAll('[role="menuitem"]')].map((b) => b.dataset.menuItem)`);
        if (j(items) !== j(['paste', 'select-all', 'undo', 'redo'])) throw new Error(`entries: ${j(items)}`);
        await app.click('[role="menu"] [data-menu-item="select-all"]');
        await app.waitFor(`document.querySelector('[data-section="selection"]') !== null`, 'the selection');
        if (await menuOpen(app)) throw new Error('the menu stayed open');
      });

      await check('éditeur : le clic droit sur un calque le choisit et ouvre le menu du calque', async () => {
        const { x, y } = await centre('.editor li.layer[data-layer="1"] .layer-name');
        await rightClick(app, x, y);
        await app.waitFor(`document.querySelector('[role="menu"]') !== null`, 'the layer menu');
        const items = await app.evaluate(`[...document.querySelectorAll('[role="menuitem"]')].map((b) => b.dataset.menuItem)`);
        if (j(items) !== j(['rename', 'visible', 'up', 'down', 'merge'])) throw new Error(`entries: ${j(items)}`);
        if (!(await app.evaluate(`document.querySelector('.editor li.layer[data-layer="1"]').dataset.chosen === 'true'`))) throw new Error('the layer was not chosen');
        if (!(await lastMenuEvent(app)).prevented) throw new Error('the web view’s menu was not cancelled');
        await closeMenu(app);
      });

      await check('éditeur : le bouton d’un stylet ne fait pas de menu', async () => {
        const spot = await app.evaluate(`(() => {
          const r = document.querySelector('.editor svg.canvas').getBoundingClientRect();
          return { x: r.left + r.width * 0.9, y: r.top + r.height * 0.4 };
        })()`);
        const before = (await app.evaluate('window.__menus.length')) ?? 0;
        await rightClick(app, spot.x, spot.y, 'pen');
        await sleep(400);
        const event = await lastMenuEvent(app);
        console.log(`    contextmenu du stylet : ${j(event)} (${before} avant)`);
        if (await menuOpen(app)) throw new Error('a menu opened from a pen');
        if (event && (await app.evaluate('window.__menus.length')) > before && !event.prevented) throw new Error('the web view’s menu showed for the pen');
      });
    },
  },
];
