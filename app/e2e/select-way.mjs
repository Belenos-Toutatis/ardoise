// The editor's selection tool has two ways (docs/questions.md Q22): a click on its button from another tool gives the
// rectangle, a click while it is the tool turns to the lasso, then the rectangle again. The button says which way is on
// with `data-way` ("rectangle" or "lasso") and its title ("Sélection : rectangle", "Sélection : lasso").
//
// These helpers put the tool in the way a journey needs, whatever the tool and the way were before.

/** The selection tool's button of the editor's toolbar (the only one with `data-way`). */
export const SELECT_BUTTON = '.toolbar .tool[data-way]';

/** What the button says now: `{ pressed, way, title }`. */
export function selectState(app) {
  return app.evaluate(`(() => {
    const b = document.querySelector(${JSON.stringify(SELECT_BUTTON)});
    return b && { pressed: b.getAttribute('aria-pressed') === 'true', way: b.dataset.way, title: b.title };
  })()`);
}

/**
 * Chooses the selection tool in `way` ('rectangle' or 'lasso') with real clicks on its button: one click from another tool
 * (the rectangle), one more for the lasso, and none when it is already in that way.
 */
export async function chooseSelect(app, way = 'rectangle') {
  let state = await selectState(app);
  if (!state) throw new Error('no selection tool in the toolbar');
  for (let click = 0; click < 3 && !(state.pressed && state.way === way); click++) {
    await app.click(SELECT_BUTTON);
    state = await selectState(app);
    await app.waitFor(`document.querySelector(${JSON.stringify(SELECT_BUTTON)})?.getAttribute('aria-pressed') === 'true'`, 'the selection tool');
  }
  if (!(state.pressed && state.way === way)) throw new Error(`the selection tool is ${JSON.stringify(state)}, not the ${way}`);
}

/** A rectangle drag between two window points: the corners and a few points on the way, as a mouse moves. */
export function rectangleDrag(from, to, steps = 8) {
  return Array.from({ length: steps + 1 }, (_, i) => ({ x: from.x + ((to.x - from.x) * i) / steps, y: from.y + ((to.y - from.y) * i) / steps }));
}
