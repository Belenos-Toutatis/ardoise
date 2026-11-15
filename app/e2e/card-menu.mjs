// The menu of a note card in the journeys: every action on notes (one note or the whole selection) is chosen there, the
// library has no bar of buttons (user request 2026-11-15). A real right click at the centre of the card, then the entry.
const j = JSON.stringify;

/** A real right click at the centre of an element. */
export async function rightClick(app, selector, label) {
  const { x, y } = await app.centre(selector, label);
  await app.mouse('mouseMoved', x, y);
  await app.mouse('mousePressed', x, y, { button: 'right', buttons: 2 });
  await app.mouse('mouseReleased', x, y, { button: 'right', buttons: 0 });
}

/** Marks the card titled `title` so that a selector can reach it. */
export async function markCard(app, title, mark) {
  const found = await app.evaluate(`(() => {
    const card = [...document.querySelectorAll('.note-card')].find((c) => c.querySelector('.card-title')?.textContent.trim() === ${j(title)});
    if (!card) return false;
    card.dataset.e2e = ${j(mark)};
    return true;
  })()`);
  if (!found) throw new Error(`no card "${title}"`);
  return `[data-e2e="${mark}"]`;
}

/** Right click on the card titled `title`; waits for its menu. On a selected card the menu acts on the whole selection. */
export async function openCardMenu(app, title) {
  const card = await markCard(app, title, `menu-${Date.now()}`);
  await rightClick(app, card);
  await app.waitFor(`document.querySelector('.menu-list[role="menu"]') !== null`, `the menu of "${title}"`);
}

/** The labels of the entries of the open menu, and its heading (the count of selected notes) when it has one. */
export const menuContent = (app) =>
  app.evaluate(`(() => {
    const menu = document.querySelector('[role="menu"]');
    return { heading: menu.querySelector('.menu-heading')?.textContent ?? null, label: menu.getAttribute('aria-label'), items: [...menu.querySelectorAll('[role="menuitem"]')].map((e) => e.textContent.trim()) };
  })()`);

/** Right click on the card titled `title`, then the menu entry labelled `label`. */
export async function cardMenu(app, title, label) {
  await openCardMenu(app, title);
  await app.click('.menu-list [role="menuitem"]', label);
}
