/**
 * The menu of a note card (right click, the « ⋯ » button, the Menu key, a long press): every action on notes lives here, the
 * library has no bar of buttons. It acts on the whole selection when the card is part of it, else on that card's note alone
 * (a right click does not change the selection). With several notes, what only makes sense for one (Renommer, Changer la
 * date…, Reprendre en mode direct) is left out and a heading gives the count; « Assembler… » needs at least two.
 */
import type { MenuItem } from '../../lib/context-menu';
import { tn, t } from '../../lib/i18n.svelte';
import type { LibraryState } from '../../lib/library-state.svelte';
import type { NoteSummary } from '../../lib/note';

export interface CardMenu {
  /** What a screen reader says for the menu. */
  label: string;
  /** A line above the entries (« 3 notes sélectionnées »), only when the menu acts on several notes. */
  heading?: string;
  items: MenuItem[];
}

export interface CardMenuOptions {
  /** The title the card shows. */
  title: string;
  /** « Renommer »: the title field of the card. */
  rename: () => void;
  /** « Reprendre en mode direct » (a single note only); absent when the screen does not offer it. */
  resume?: (note: NoteSummary) => void;
  /** Why the note cannot be resumed now (no tablet, tablet busy); null when it can. */
  resumeHint?: string | null;
}

/** The notes the menu of the card of `note` acts on, in the order the grid shows them. */
export function menuTargets(lib: LibraryState, note: NoteSummary): NoteSummary[] {
  return lib.isSelected(note.path) ? lib.selectedDisplayed : [note];
}

/** Builds the menu of the card of `note` from the library's state now (called when the menu opens). */
export function cardMenu(lib: LibraryState, note: NoteSummary, options: CardMenuOptions): CardMenu {
  const targets = menuTargets(lib, note);
  const many = targets.length > 1;
  const paths = targets.map((n) => n.path);
  const busy = lib.activity.busy;
  const allFavorite = targets.every((n) => n.favorite);

  const items: MenuItem[] = [];
  /** Adds a group of entries, a rule above the first one unless it opens the menu. */
  const group = (...entries: (MenuItem | null)[]) => {
    const present = entries.filter((entry): entry is MenuItem => entry !== null);
    present.forEach((entry, at) => items.push(at === 0 && items.length > 0 ? { ...entry, separatorBefore: true } : entry));
  };

  group(
    many ? null : { id: 'rename', label: t('library.card.rename'), onselect: options.rename },
    many || !options.resume
      ? null
      : {
          id: 'resume',
          label: t('live.resume.action'),
          disabled: !!options.resumeHint,
          hint: options.resumeHint ?? undefined,
          onselect: () => options.resume?.(note),
        },
  );
  group(
    { id: 'move', label: t('library.selection.move'), onselect: () => lib.openMove(paths) },
    { id: 'tags', label: t('library.selection.tags'), onselect: () => lib.openTags(paths) },
    {
      id: 'favorite',
      label: allFavorite ? t('library.card.unfavorite') : t('library.card.favorite'),
      onselect: () => void (many ? lib.setFavorite(targets, !allFavorite) : lib.toggleFavorite(note)),
    },
    many ? null : { id: 'date', label: t('library.card.changeDate'), onselect: () => lib.openDate(note) },
  );
  group(
    { id: 'export', label: t('library.selection.export'), onselect: () => lib.openDialog('export', targets) },
    { id: 'print', label: t('library.selection.print'), onselect: () => lib.openDialog('print', targets) },
    many ? { id: 'assemble', label: t('library.selection.assemble'), onselect: () => lib.openDialog('assemble', targets) } : null,
    { id: 'duplicate', label: t('library.card.duplicate'), disabled: busy, onselect: () => void lib.duplicateNotes(targets) },
    { id: 'turn-ccw', label: t('library.turn.menuLeft'), disabled: busy, onselect: () => void lib.turnNotes(targets, 'ccw') },
    { id: 'turn-cw', label: t('library.turn.menuRight'), disabled: busy, onselect: () => void lib.turnNotes(targets, 'cw') },
  );
  group(lib.selected.length > 0 ? { id: 'clear', label: t('library.selection.clear'), onselect: () => lib.clearSelection() } : null);
  group({ id: 'trash', label: t('library.card.trash'), danger: true, onselect: () => void lib.trashNotes(paths) });

  return {
    label: many ? t('library.card.menuMany', { count: targets.length }) : t('library.card.menu', { title: options.title }),
    heading: many ? tn('library.selection.count', targets.length) : undefined,
    items,
  };
}
