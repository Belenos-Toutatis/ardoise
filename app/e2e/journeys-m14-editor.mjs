// M14 journeys, the editor's layers zone (the chosen layer: the pen, the eraser and the selection act on it; show, hide,
// rename, move, merge). Package A (layers zone and editor); the journey is listed in
// docs/design/tablet-live-classroom.md, "M14: correct during live on one's own layer", "End-to-end journeys".
//
// Written without a run (the window cannot be driven from the session that wrote it): the selectors are the zone's
// (`section.layers-zone`, `li.layer[data-layer]`, `data-action`), the ones of the other editor journeys for the rest.
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const STROKES = '.editor g.ink path[data-stroke]';
const count = `document.querySelectorAll(${JSON.stringify(STROKES)}).length`;
/** The page's sheet on screen, in window pixels. */
const sheetBox = `(() => { const r = document.querySelector('.editor svg.canvas rect.sheet').getBoundingClientRect(); return { x: r.left, y: r.top, w: r.width, h: r.height }; })()`;

const ZONE = '.editor-side section.layers-zone';
/** The layers of the zone, top first: their `data-layer`. */
const rows = `[...document.querySelectorAll(${JSON.stringify(`${ZONE} li.layer`)})].map((li) => Number(li.dataset.layer))`;
/** `data-layer` of the chosen row, or null. */
const chosen = `(() => { const li = document.querySelector(${JSON.stringify(`${ZONE} li.layer[data-chosen="true"]`)}); return li ? Number(li.dataset.layer) : null; })()`;
const nameOf = (index) =>
  `(document.querySelector(${JSON.stringify(`${ZONE} li.layer[data-layer="${index}"] .layer-name`)})?.textContent ?? '').trim()`;
const warning = `document.querySelector(${JSON.stringify(`${ZONE} [data-slot="layer-warning"]`)})?.dataset.kind ?? null`;
/** How many strokes the selection panel says are selected (0 when there is no panel). */
const selectedCount = (l) => {
  const escape = (part) => part.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const [before, after] = l('editor.selection.count').split('{count}');
  const source = `${escape(before)}(\\d+)${escape(after)}`;
  return `(() => { const m = (document.querySelector('.editor-side')?.textContent ?? '').match(new RegExp(${JSON.stringify(source)})); return m ? Number(m[1]) : 0; })()`;
};

/** A straight line of mouse positions. */
const line = (x0, y0, x1, y1, n = 24) =>
  Array.from({ length: n + 1 }, (_, i) => ({ x: x0 + ((x1 - x0) * i) / n, y: y0 + ((y1 - y0) * i) / n }));

export const editorLayersJourneys = [
  {
    name: 'éditeur : la sélection, Ctrl+A et la gomme ne prennent que le calque choisi',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const tool = (mode) => `.toolbar button[title="${l(`editor.modes.${mode}`)}"]`;
      const action = (name) => `${ZONE} [data-action="${name}"]`;
      const inRow = (index, name) => `${ZONE} li.layer[data-layer="${index}"] [data-action="${name}"]`;
      let box;
      /** Window pixels of a point of the sheet, as fractions of its width and height. */
      const at = (fx, fy) => ({ x: box.x + box.w * fx, y: box.y + box.h * fy });
      /** A lasso around the middle of the page, which holds every stroke the journey draws. */
      const lasso = () =>
        app.drag([at(0.08, 0.08), at(0.9, 0.08), at(0.9, 0.92), at(0.08, 0.92), at(0.08, 0.1)]);
      let layers0;

      await check('une nouvelle note : la zone des calques remplace l’ancien panneau, un calque choisi', async () => {
        await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the demo notes');
        await app.click('button.primary', l('library.newNote'));
        await app.waitFor(`document.querySelector('.editor svg.canvas rect.sheet') !== null`, 'the editor');
        // The page is fitted once the editor has measured its stage.
        await sleep(400);
        box = await app.evaluate(sheetBox);
        await app.waitFor(`document.querySelector(${JSON.stringify(ZONE)}) !== null`, 'the layers zone');
        if ((await app.evaluate(`document.querySelector(${JSON.stringify(`${ZONE}[data-where="editor"]`)}) !== null`)) !== true) {
          throw new Error('the zone is not the editor’s');
        }
        // The Son tab's own switch (« Pendant que j'écris » / « Pendant une relecture », M17) is not a layer.
        if ((await app.evaluate(`document.querySelector('.editor-side input[type="radio"]:not([name="editor-narration-mode"])')`)) !== null) {
          throw new Error('the old radio buttons are still there');
        }
        layers0 = (await app.evaluate(rows)).length;
        if (layers0 < 1) throw new Error('a note has no layer');
        if ((await app.evaluate(chosen)) !== layers0 - 1) throw new Error('the chosen layer is not the top one');
        if ((await app.evaluate(count)) !== 0) throw new Error('the new note is not empty');
      });

      await check('deux traits horizontaux sur le premier calque', async () => {
        await app.click(tool('pen'));
        await app.drag(line(at(0.2, 0.4).x, at(0.2, 0.4).y, at(0.75, 0.4).x, at(0.75, 0.4).y));
        await app.waitFor(`${count} === 1`, 'the first stroke');
        await app.drag(line(at(0.2, 0.6).x, at(0.2, 0.6).y, at(0.75, 0.6).x, at(0.75, 0.6).y));
        await app.waitFor(`${count} === 2`, 'the second stroke');
      });

      await check(`« ${l('layers.add')} » : un calque en haut, choisi, et deux traits verticaux qui croisent les premiers`, async () => {
        await app.click(action('add-layer'));
        await app.waitFor(`${rows}.length === ${layers0 + 1}`, 'the new layer');
        if ((await app.evaluate(chosen)) !== layers0) throw new Error('the new layer is not the chosen one');
        await app.drag(line(at(0.35, 0.25).x, at(0.35, 0.25).y, at(0.35, 0.75).x, at(0.35, 0.75).y));
        await app.waitFor(`${count} === 3`, 'the third stroke');
        await app.drag(line(at(0.55, 0.25).x, at(0.55, 0.25).y, at(0.55, 0.75).x, at(0.55, 0.75).y));
        await app.waitFor(`${count} === 4`, 'the fourth stroke');
      });

      await check('un lasso autour de tout : seuls les deux traits du calque choisi sont sélectionnés', async () => {
        await app.click(tool('select'));
        await lasso();
        await app.waitFor(`${selectedCount(l)} === 2`, 'the selection of the chosen layer');
      });

      await check('Échap désélectionne ; Ctrl+A prend les mêmes deux traits', async () => {
        await app.key('Escape');
        await app.waitFor(`${selectedCount(l)} === 0`, 'the empty selection');
        await app.key('a', { ctrl: true });
        await app.waitFor(`${selectedCount(l)} === 2`, 'the select-all of the chosen layer');
      });

      await check('la gomme (outil) en travers des deux calques : seuls les traits du calque choisi s’en vont', async () => {
        await app.click(tool('eraser'));
        // Along the first horizontal stroke: it touches it and crosses both vertical ones.
        await app.drag(line(at(0.15, 0.4).x, at(0.15, 0.4).y, at(0.8, 0.4).x, at(0.8, 0.4).y, 40));
        await app.waitFor(`${count} === 2`, 'the erased strokes of the chosen layer only');
        if ((await app.evaluate(rows)).length !== layers0 + 1) throw new Error('a layer went');
      });

      await check('annuler : les quatre traits reviennent', async () => {
        await app.key('z', { ctrl: true });
        await app.waitFor(`${count} === 4`, 'the strokes back');
      });

      await check('un clic sur la première ligne choisit son calque et vide la sélection', async () => {
        await app.click(`${ZONE} li.layer[data-layer="${layers0 - 1}"] .layer-name`);
        await app.waitFor(`${chosen} === ${layers0 - 1}`, 'the layer below chosen');
        await app.waitFor(`${selectedCount(l)} === 0`, 'the selection dropped with the choice');
        await app.waitFor(`document.querySelector('.editor svg.canvas rect.selection') === null`, 'no selection frame');
      });

      await check('un lasso autour de tout prend maintenant les traits du premier calque', async () => {
        await app.click(tool('select'));
        await lasso();
        await app.waitFor(`${selectedCount(l)} === 2`, 'the strokes of the first layer');
        await app.key('Escape');
      });

      await check('le clavier : le bouton du nom, Espace ou Entrée choisissent le calque', async () => {
        await app.evaluate(
          `document.querySelector(${JSON.stringify(`${ZONE} li.layer[data-layer="${layers0}"] .layer-name`)}).focus()`,
        );
        await app.key(' ');
        await app.waitFor(`${chosen} === ${layers0}`, 'the layer chosen with the keyboard');
        // Back on the first layer for what follows, with Enter this time (a real key event with its character).
        await app.evaluate(
          `document.querySelector(${JSON.stringify(`${ZONE} li.layer[data-layer="${layers0 - 1}"] .layer-name`)}).focus()`,
        );
        await app.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13, text: '\r' });
        await app.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Enter', code: 'Enter', windowsVirtualKeyCode: 13 });
        await app.waitFor(`${chosen} === ${layers0 - 1}`, 'the layer chosen with Enter');
      });

      await check('masquer le calque choisi : il le reste, l’avertissement s’affiche, le stylet le dit et n’ajoute rien', async () => {
        await app.click(inRow(layers0 - 1, 'layer-visible'));
        // Its two horizontal strokes are no longer drawn.
        await app.waitFor(`${count} === 2`, 'the hidden layer not drawn');
        if ((await app.evaluate(chosen)) !== layers0 - 1) throw new Error('the hidden layer is no longer chosen');
        await app.waitFor(`${warning} === 'chosenHidden'`, 'the warning of the zone');
        await app.click(tool('pen'));
        await app.drag(line(at(0.2, 0.85).x, at(0.2, 0.85).y, at(0.8, 0.85).x, at(0.8, 0.85).y));
        await app.waitFor(
          `(document.querySelector('.editor-notice')?.textContent ?? '').includes(${JSON.stringify(l('editor.layers.hiddenActive'))})`,
          'the notice of the pen',
        );
        await sleep(200);
        if ((await app.evaluate(count)) !== 2) throw new Error('the pen drew on a hidden layer');
      });

      await check('afficher le calque : l’avertissement part, les traits reviennent', async () => {
        await app.click(inRow(layers0 - 1, 'layer-visible'));
        await app.waitFor(`${count} === 4`, 'the strokes back');
        await app.waitFor(`${warning} === null`, 'no warning');
      });

      await check('renommer : Entrée valide, Échap annule', async () => {
        await app.click(inRow(layers0 - 1, 'rename-layer'));
        await app.waitFor(`document.activeElement?.classList.contains('layer-rename') === true`, 'the focused field');
        await app.type('Fond');
        await app.key('Enter');
        await app.waitFor(`${nameOf(layers0 - 1)} === 'Fond'`, 'the new name');
        if ((await app.evaluate(`document.activeElement?.dataset.action ?? null`)) !== 'choose-layer') throw new Error('the focus did not return to the name button');
        await app.click(inRow(layers0 - 1, 'rename-layer'));
        await app.waitFor(`document.activeElement?.classList.contains('layer-rename') === true`, 'the focused field');
        await app.type('Autre');
        await app.key('Escape');
        await app.waitFor(`document.querySelector(${JSON.stringify(`${ZONE} input.layer-rename`)}) === null`, 'the field closed');
        if ((await app.evaluate(nameOf(layers0 - 1))) !== 'Fond') throw new Error('Escape did not give the old name back');
        // Esc went to the field, not to the editor: the note is still open.
        if ((await app.evaluate(`document.querySelector('.editor') !== null`)) !== true) throw new Error('Escape left the editor');
      });

      await check('monter, descendre et fusionner : sur la ligne choisie seulement, avec leurs limites', async () => {
        for (const name of ['layer-up', 'layer-down', 'layer-merge']) {
          if ((await app.evaluate(`document.querySelectorAll(${JSON.stringify(action(name))}).length`)) !== 1) {
            throw new Error(`${name} is not on exactly one row`);
          }
          if ((await app.evaluate(`document.querySelector(${JSON.stringify(inRow(layers0 - 1, name))}) !== null`)) !== true) {
            throw new Error(`${name} is not on the chosen row`);
          }
        }
        // The chosen layer is just below the new one: it can go up, and "Fond" now sits above.
        await app.click(inRow(layers0 - 1, 'layer-up'));
        await app.waitFor(`${nameOf(layers0)} === 'Fond'`, 'the layer one place up');
        await app.waitFor(`${chosen} === ${layers0}`, 'the choice following its layer');
        // It is now the top one: up is disabled.
        await app.waitFor(`document.querySelector(${JSON.stringify(action('layer-up'))}).disabled === true`, 'up disabled on the top layer');
        await app.click(inRow(layers0, 'layer-down'));
        await app.waitFor(`${nameOf(layers0 - 1)} === 'Fond' && ${chosen} === ${layers0 - 1}`, 'the layer back down');
      });

      await check('fusionner le calque du haut avec celui du dessous : un calque de moins, aucun trait perdu', async () => {
        await app.click(`${ZONE} li.layer[data-layer="${layers0}"] .layer-name`);
        await app.waitFor(`${chosen} === ${layers0}`, 'the top layer chosen');
        await app.click(inRow(layers0, 'layer-merge'));
        await app.waitFor(`${rows}.length === ${layers0}`, 'the merged layers');
        if ((await app.evaluate(count)) !== 4) throw new Error('a stroke was lost in the merge');
        if ((await app.evaluate(chosen)) !== layers0 - 1) throw new Error('the choice did not go down with the merge');
        // The lower layer keeps its name.
        if ((await app.evaluate(nameOf(layers0 - 1))) !== 'Fond') throw new Error('the lower layer lost its name');
      });
    },
  },
];
