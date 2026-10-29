// Milestone M10, library features: turning several notes, « Annuler » after a turn and after
// « Mettre à la corbeille », tag colours, renaming and deleting a binder, plurals of the trash.
// Same pattern as journeys.mjs (labels from the translation files, so the journeys run in French
// and in English). Not wired into run.mjs: the integrator imports `libraryJourneys` and runs
// them after merging, on Windows.
//
// Not covered here: Settings › change library goes through the system's folder picker, which the
// DevTools protocol cannot drive (see the manual check in the branch report).
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const locales = join(dirname(fileURLToPath(import.meta.url)), '..', 'ui', 'src', 'locales');
const catalogs = {
  fr: JSON.parse(readFileSync(join(locales, 'fr.json'), 'utf8')),
  en: JSON.parse(readFileSync(join(locales, 'en.json'), 'utf8')),
};
const text = (language, key) => key.split('.').reduce((node, part) => node?.[part], catalogs[language]);
const fill = (template, params) => template.replace(/\{(\w+)\}/g, (match, name) => (name in params ? String(params[name]) : match));
/** A plural message (`{ one, other }`) for `count`, as `tn` in i18n.svelte.ts writes it. */
const plural = (language, key, count, params = {}) => {
  const forms = text(language, key);
  const category = new Intl.PluralRules(language).select(count) === 'one' ? 'one' : 'other';
  return fill(forms[category] ?? forms.other, { count, ...params });
};

const cards = `document.querySelectorAll('.note-card').length`;
const page = `(() => { const s = document.querySelector('.editor svg.canvas'); return s ? s.dataset.pageWidth + 'x' + s.dataset.pageHeight : null; })()`;
const offerText = `document.querySelector('.activity-offer')?.textContent ?? ''`;

/** Marks the card titled `title` so that a selector can reach it: `[data-e2e="<mark>"]`. */
async function markCard(app, title, mark) {
  const found = await app.evaluate(`(() => {
    const card = [...document.querySelectorAll('.note-card')].find((c) => c.querySelector('.card-title')?.textContent.trim() === ${JSON.stringify(title)});
    if (!card) return false;
    card.dataset.e2e = ${JSON.stringify(mark)};
    return true;
  })()`);
  if (!found) throw new Error(`no card "${title}"`);
  return `[data-e2e="${mark}"]`;
}

/** Ticks the selection box of the card titled `title`. */
async function select(app, title) {
  const card = await markCard(app, title, `select-${title.length}-${Date.now()}`);
  await app.click(`${card} .card-check input`);
}

/** Opens a note from the library, reads its page size in the editor, and goes back. */
async function pageOf(app, l, title) {
  const card = await markCard(app, title, `open-${Date.now()}`);
  await app.click(`${card} .card-open`);
  const size = await app.waitFor(page, `the page of "${title}"`);
  await app.click('.editor button', l('editor.back'));
  await app.waitFor(`document.querySelectorAll('.note-card').length > 0`, 'the library again');
  return size;
}

export const libraryJourneys = [
  {
    name: 'tourner deux notes depuis la sélection, annuler, puis vérifier dans l’éditeur',
    async run(app, check) {
      const l = (key) => text(app.language, key);

      await check('la bibliothèque de démonstration s’affiche (8 notes)', async () => {
        await app.waitFor(`${cards} === 8`, 'the 8 demo notes');
      });

      await check(`sélectionner « Liste de courses » et « Électricité – loi d'Ohm », « ${l('library.turn.button')} » à droite`, async () => {
        await select(app, 'Liste de courses');
        await select(app, "Électricité – loi d'Ohm");
        await app.waitFor(`document.querySelector('.selection-bar')?.textContent.includes(${JSON.stringify(fill(l('library.selection.count'), { count: 2 }))})`, 'two selected notes');
        await app.click('.selection-bar [data-action="turn"]');
        await app.click('.selection-bar [data-action="turn-cw"]');
        const done = plural(app.language, 'library.turn.doneRight', 2);
        await app.waitFor(`${offerText}.includes(${JSON.stringify(done)})`, 'the notice of the turn');
      });

      await check(`« ${l('library.activity.undo')} » remet les deux notes dans leur sens`, async () => {
        await app.click('.activity-offer [data-action="undo"]');
        const undone = plural(app.language, 'library.turn.undone', 2);
        await app.waitFor(`${offerText}.includes(${JSON.stringify(undone)})`, 'the notes turned back', 20_000);
        const size = await pageOf(app, l, 'Liste de courses');
        if (size !== '29700x21600') throw new Error(`the note is ${size} after the undo`);
      });

      await check('tourner une note à gauche : l’éditeur la montre en portrait (page, écriture et fond ensemble)', async () => {
        // The selection of the first turn is still there.
        if (await app.evaluate(`document.querySelector('.selection-bar') !== null`)) {
          await app.click('.selection-bar button', l('library.selection.clear'));
        }
        await select(app, 'Liste de courses');
        await app.click('.selection-bar [data-action="turn"]');
        await app.click('.selection-bar [data-action="turn-ccw"]');
        await app.waitFor(`${offerText}.includes(${JSON.stringify(plural(app.language, 'library.turn.doneLeft', 1))})`, 'the notice of the turn');
        const size = await pageOf(app, l, 'Liste de courses');
        if (size !== '21600x29700') throw new Error(`the note is ${size} after the turn`);
      });
    },
  },
  {
    name: 'mettre une note à la corbeille avec Suppr, annuler ; vider la corbeille (singulier)',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      await app.waitFor(`${cards} === 8`, 'the 8 demo notes');

      await check('Suppr sur une carte met la note à la corbeille et propose « Annuler »', async () => {
        const card = await markCard(app, 'Liste de courses', 'trash');
        await app.evaluate(`document.querySelector('${card} .card-open').focus()`);
        await app.key('Delete');
        await app.waitFor(`${cards} === 7`, 'one note less');
        await app.waitFor(`${offerText}.includes(${JSON.stringify(plural(app.language, 'library.trashUndo.done', 1))})`, 'the undo notice');
      });

      await check(`« ${l('library.activity.undo')} » la restaure à sa place`, async () => {
        await app.click('.activity-offer [data-action="undo"]');
        await app.waitFor(`${cards} === 8`, 'the note back');
        await app.waitFor(`document.querySelector('[data-view="trash"] .count')?.textContent.trim() === '0'`, 'an empty trash');
      });

      await check('vider la corbeille d’une seule note : la confirmation est au singulier', async () => {
        const card = await markCard(app, 'Liste de courses', 'trash-again');
        await app.evaluate(`document.querySelector('${card} .card-open').focus()`);
        await app.key('Delete');
        await app.waitFor(`${cards} === 7`, 'one note less');
        await app.click('[data-view="trash"]');
        await app.click('.library-section button', l('library.trashView.emptyAll'));
        const expected = plural(app.language, 'library.trashView.confirmEmpty', 1);
        await app.waitFor(`document.querySelector('[role="alertdialog"]')?.textContent.includes(${JSON.stringify(expected)})`, 'the singular confirmation');
        await app.click('[role="alertdialog"] button', l('library.cancel'));
      });
    },
  },
  {
    name: 'couleur d’une étiquette : la choisir dans la barre latérale, la voir sur les cartes et dans _ardoise/tags.json',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      await app.waitFor(`${cards} === 8`, 'the 8 demo notes');

      await check(`étiquette « physique » : couleur « ${l('library.tagColor.names.blue')} »`, async () => {
        await app.click('.sidebar .nav-row:has([data-tag="physique"]) .tag-color-trigger');
        await app.click(`.sidebar [role="menuitemradio"][aria-label="${l('library.tagColor.names.blue')}"]`);
        await app.waitFor(`document.querySelector('.sidebar [data-tag="physique"]')?.dataset.tagColor === 'blue'`, 'the coloured tag');
      });

      await check('les cartes portant l’étiquette la montrent en bleu', async () => {
        await app.waitFor(
          `[...document.querySelectorAll('.note-card .chip')].filter((c) => c.textContent.trim() === 'physique').every((c) => c.dataset.tagColor === 'blue')`,
          'blue chips',
        );
      });

      await check('la couleur est rangée dans la bibliothèque (_ardoise/tags.json), pas dans les notes', async () => {
        const file = join(app.library, '_ardoise', 'tags.json');
        const saved = JSON.parse(readFileSync(file, 'utf8'));
        if (saved.version !== 1 || saved.tags?.physique?.color !== 'blue') throw new Error(JSON.stringify(saved));
      });

      await check(`« ${l('library.tagColor.none')} » la retire`, async () => {
        await app.click('.sidebar .nav-row:has([data-tag="physique"]) .tag-color-trigger');
        await app.click(`.sidebar [role="menuitemradio"][aria-label="${l('library.tagColor.none')}"]`);
        await app.waitFor(`!document.querySelector('.sidebar [data-tag="physique"]')?.dataset.tagColor`, 'no colour');
      });
    },
  },
  {
    name: 'classeurs : renommer « Perso », puis le supprimer, puis annuler',
    async run(app, check) {
      const l = (key) => text(app.language, key);
      const newName = app.language === 'fr' ? 'Personnel' : 'Personal';
      const notesIn = (folder) => readdirSync(join(app.library, folder)).filter((name) => name.endsWith('.ardoise'));
      await app.waitFor(`${cards} === 8`, 'the 8 demo notes');

      await check(`« ${l('library.binder.rename')} » : « Perso » devient « ${newName} », le dossier aussi`, async () => {
        await app.click('.sidebar .nav-row:has([data-binder="Perso"]) .menu .icon-button');
        await app.click('.sidebar .menu-list button', l('library.binder.rename'));
        await app.waitFor(`document.activeElement?.closest('.binder-editor') !== null`, 'the name field');
        await app.key('a', { ctrl: true });
        await app.type(newName);
        await app.key('Enter');
        await app.waitFor(`document.querySelector('.sidebar [data-binder="${newName}"]') !== null`, 'the renamed binder');
        if (existsSync(join(app.library, 'Perso')) || notesIn(newName).length !== 2) throw new Error('the folder did not follow');
      });

      await check('le renommer comme « Classe » est refusé, rien ne bouge', async () => {
        await app.click(`.sidebar .nav-row:has([data-binder="${newName}"]) .menu .icon-button`);
        await app.click('.sidebar .menu-list button', l('library.binder.rename'));
        await app.waitFor(`document.activeElement?.closest('.binder-editor') !== null`, 'the name field');
        await app.key('a', { ctrl: true });
        await app.type('Classe');
        await app.key('Enter');
        await app.waitFor(`document.querySelector('.binder-error') !== null`, 'the refusal');
        await app.key('Escape');
        if (notesIn(newName).length !== 2 || notesIn('Classe/3e B').length !== 4) throw new Error('notes moved');
      });

      await check(`« ${l('library.binder.delete')} » : la confirmation annonce 2 notes, puis le dossier disparaît`, async () => {
        await app.click(`.sidebar .nav-row:has([data-binder="${newName}"]) .menu .icon-button`);
        await app.click('.sidebar .menu-list button', l('library.binder.delete'));
        const expected = plural(app.language, 'library.binder.confirm', 2, { name: newName });
        await app.waitFor(`document.querySelector('.binder-confirm')?.textContent.includes(${JSON.stringify(expected)})`, 'the confirmation');
        await app.click('.binder-confirm button', l('library.binder.deleteDo'));
        await app.waitFor(`${cards} === 6`, 'two notes less');
        await app.waitFor(`!document.querySelector('.sidebar [data-binder="${newName}"]')`, 'the binder gone');
        if (existsSync(join(app.library, newName))) throw new Error('the folder is still there');
      });

      await check(`« ${l('library.activity.undo')} » rétablit le classeur et ses deux notes`, async () => {
        await app.click('.activity-offer [data-action="undo"]');
        await app.waitFor(`${cards} === 8`, 'the notes back', 20_000);
        if (notesIn(newName).length !== 2) throw new Error('the notes are not back in their binder');
      });
    },
  },
];
