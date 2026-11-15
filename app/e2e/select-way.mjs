// The Select tool has two ways, in the editor and in live mode alike (docs/questions.md Q22): a click on its button from
// another tool gives the way used last (the rectangle the very first time), a click while it is the tool turns to the
// other way, and the V key does the same. The way is one preference of the computer, shared by the editor and live mode.
// The button says which way is on with `data-way` ("rectangle" or "lasso") and its title / accessible name
// ("Sélection : rectangle", "Sélection : lasso"); its visible label stays "Sélection".
//
// These helpers put the tool in the way a journey needs, whatever the tool and the way were before.

/** The Select tool's button: the editor's toolbar (`.tool[data-way]`) or live mode's tool group (`data-action="select"`). */
export const SELECT_BUTTON = '.toolbar .tool[data-way], button[data-action="select"][data-way]';

/** What the button says now: `{ pressed, way, title, name }` (null when the window has no Select button). */
export function selectState(app) {
  return app.evaluate(`(() => {
    const b = document.querySelector(${JSON.stringify(SELECT_BUTTON)});
    return b && { pressed: b.getAttribute('aria-pressed') === 'true', way: b.dataset.way, title: b.title, name: b.getAttribute('aria-label'), label: b.textContent.trim() };
  })()`);
}

/**
 * Chooses the Select tool in `way` ('rectangle' or 'lasso') with real clicks on its button: one click from another tool (it
 * comes back in the way used last), one more to turn it, and none when it is already on in that way.
 */
export async function chooseSelect(app, way = 'rectangle') {
  let state = await selectState(app);
  if (!state) throw new Error('no Select tool in this window');
  for (let click = 0; click < 3 && !(state.pressed && state.way === way); click++) {
    await app.click(SELECT_BUTTON);
    await app.waitFor(`document.querySelector(${JSON.stringify(SELECT_BUTTON)})?.getAttribute('aria-pressed') === 'true'`, 'the Select tool');
    state = await selectState(app);
  }
  if (!(state.pressed && state.way === way)) throw new Error(`the Select tool is ${JSON.stringify(state)}, not the ${way}`);
}

/** A rectangle drag between two window points: the corners and a few points on the way, as a mouse moves. */
export function rectangleDrag(from, to, steps = 8) {
  return Array.from({ length: steps + 1 }, (_, i) => ({ x: from.x + ((to.x - from.x) * i) / steps, y: from.y + ((to.y - from.y) * i) / steps }));
}
