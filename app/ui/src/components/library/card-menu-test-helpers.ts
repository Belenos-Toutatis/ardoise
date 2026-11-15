/**
 * The card's menu in the component tests: every action on notes is chosen there (the library has no bar of buttons). A
 * right click opens it at the pointer, moved to the end of the document.
 */

/** A mouse's right click on `card` (Chromium gives the event the device's pointer type); returns the menu that opened. */
export async function openCardMenu(card: Element, settle: () => Promise<void>, at = { x: 120, y: 90 }): Promise<HTMLElement> {
  const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, button: 2, clientX: at.x, clientY: at.y });
  Object.defineProperty(event, 'pointerType', { value: 'mouse' });
  card.dispatchEvent(event);
  await settle();
  const menu = document.querySelector<HTMLElement>('[role="menu"]');
  if (!menu) throw new Error('no menu opened');
  return menu;
}

/** The labels of the entries of the open menu. */
export const cardMenuLabels = (): string[] =>
  [...document.querySelectorAll('[role="menuitem"]')].map((entry) => entry.textContent?.trim() ?? '');

/** The entry of the open menu named `label`. */
export function cardMenuEntry(label: string): HTMLElement {
  const found = [...document.querySelectorAll<HTMLElement>('[role="menuitem"]')].find((entry) => entry.textContent?.trim() === label);
  if (!found) throw new Error(`no entry "${label}" among ${cardMenuLabels().join(' | ')}`);
  return found;
}

/** Opens the menu of `card` and chooses `label`: how an action on the selection (or on one note) is made. */
export async function chooseFromCardMenu(card: Element, label: string, settle: () => Promise<void>): Promise<void> {
  await openCardMenu(card, settle);
  cardMenuEntry(label).dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
  await settle();
}
