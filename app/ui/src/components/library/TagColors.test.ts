// @vitest-environment jsdom
// Milestone M10: tag colours, kept by the library (`_ardoise/tags.json`), shown on cards, in the
// sidebar and in the tags panel, and chosen where tags are managed.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import en from '../../locales/en.json';
import fr from '../../locales/fr.json';
import { api } from '../../lib/api';
import { lookup } from '../../lib/i18n-core';
import { resetMockLibrary } from '../../lib/mock-data';
import { TAG_COLORS, colorOf, isTagColor, tagKey } from '../../lib/tag-colors';
import { renderHome, type Ui } from './library-test-helpers';

let ui: Ui | null = null;

beforeEach(() => {
  resetMockLibrary();
});

afterEach(() => {
  ui?.destroy();
  ui = null;
  vi.restoreAllMocks();
});

const sidebarTag = (tag: string) => ui!.q(`.sidebar .nav-item[data-tag="${tag}"]`)!;
const cardChip = (title: string, tag: string) =>
  [...ui!.card(title).querySelectorAll<HTMLElement>('.chips .chip')].find((c) => c.textContent?.trim() === tag)!;

async function pick(within: Element, colorName: string) {
  await ui!.click(within.querySelector('.tag-color-trigger'));
  const palette = within.querySelector('[role="menu"]')!;
  await ui!.click(ui!.byLabel(colorName, palette));
}

describe('tag colours', () => {
  it('are chosen in the sidebar and shown on the cards, the sidebar and the tags panel', async () => {
    ui = await renderHome();
    const row = sidebarTag('physique').closest('.nav-row')!;
    expect(ui.byLabel("Couleur de l'étiquette « physique » : Aucune couleur", row)).toBeTruthy();
    await pick(row, 'Bleu');

    await vi.waitFor(() => expect(sidebarTag('physique').dataset.tagColor).toBe('blue'));
    expect(sidebarTag('physique').querySelector('.tag-dot')).not.toBeNull();
    expect(cardChip('Cours de physique', 'physique').dataset.tagColor).toBe('blue');
    expect(cardChip('Cours de physique', '3e B').dataset.tagColor).toBeUndefined();
    expect(ui.byLabel("Couleur de l'étiquette « physique » : Bleu", row).getAttribute('aria-expanded')).toBe('false');
    // Kept by the library, under the tag in lower case.
    expect(await api.tagColors()).toEqual({ physique: 'blue' });

    // The tags panel of the selection shows it too, and changes it.
    await ui.select('Cours de physique');
    await ui.click(ui.button('Étiquettes…'));
    const panel = ui.byLabel('Étiquettes…', ui.q('.selection-bar')!);
    const ofSelection = [...panel.querySelectorAll<HTMLElement>('.chip.removable')].find((c) => c.textContent?.includes('physique'))!;
    expect(ofSelection.dataset.tagColor).toBe('blue');
    const known = [...panel.querySelectorAll('.tag-entry')].find((e) => e.textContent?.includes('3e B'))!;
    await pick(known, 'Vert');
    await vi.waitFor(() => expect(cardChip('Cours de physique', '3e B').dataset.tagColor).toBe('green'));
    expect(sidebarTag('3e B').dataset.tagColor).toBe('green');
    expect(panel.textContent).toContain("Le rond à côté d'une étiquette change sa couleur");
  });

  it('can be removed, and follow the tag whatever its case', async () => {
    await api.setTagColor('  PHYSIQUE ', 'red');
    ui = await renderHome('en');
    await vi.waitFor(() => expect(sidebarTag('physique').dataset.tagColor).toBe('red'));
    const row = sidebarTag('physique').closest('.nav-row')!;
    await pick(row, 'No colour');
    await vi.waitFor(() => expect(sidebarTag('physique').dataset.tagColor).toBeUndefined());
    expect(cardChip('Cours de physique', 'physique').dataset.tagColor).toBeUndefined();
    expect(await api.tagColors()).toEqual({});
  });

  it('ignore a colour this version does not know (written by a newer one)', async () => {
    await api.setTagColor('physique', 'ultraviolet');
    ui = await renderHome();
    await ui.settle();
    expect(sidebarTag('physique').dataset.tagColor).toBeUndefined();
  });

  it('are read again when the window comes back to the front (chosen on another PC)', async () => {
    ui = await renderHome();
    expect(sidebarTag('physique').dataset.tagColor).toBeUndefined();
    // Another PC wrote _ardoise/tags.json, which Nextcloud brought here.
    await api.setTagColor('physique', 'teal');
    window.dispatchEvent(new Event('focus'));
    await vi.waitFor(() => expect(sidebarTag('physique').dataset.tagColor).toBe('teal'));
  });

  it('is a menu the arrow keys go through, starting on the current colour', async () => {
    await api.setTagColor('réunion', 'green');
    ui = await renderHome();
    await vi.waitFor(() => expect(sidebarTag('réunion').dataset.tagColor).toBe('green'));
    const row = sidebarTag('réunion').closest('.nav-row')!;
    const trigger = row.querySelector('.tag-color-trigger')!;
    expect(trigger.getAttribute('aria-haspopup')).toBe('menu');
    await ui.click(trigger);
    const items = [...row.querySelectorAll<HTMLElement>('[role="menuitemradio"]')];
    expect(items).toHaveLength(9);
    // One Tab stop: the colour chosen now.
    expect(items.filter((item) => item.tabIndex === 0).map((item) => item.getAttribute('aria-label'))).toEqual(['Vert']);
    expect(document.activeElement?.getAttribute('aria-label')).toBe('Vert');
    await ui.key(document.activeElement, 'ArrowRight');
    expect(document.activeElement?.getAttribute('aria-label')).toBe('Turquoise');
    await ui.key(document.activeElement, 'End');
    expect(document.activeElement?.getAttribute('aria-label')).toBe('Aucune couleur');
    await ui.key(document.activeElement, 'ArrowRight');
    expect(document.activeElement?.getAttribute('aria-label')).toBe('Rouge');
    await ui.click(document.activeElement);
    await vi.waitFor(() => expect(sidebarTag('réunion').dataset.tagColor).toBe('red'));
    // Back on the button that opened it.
    expect(document.activeElement).toBe(trigger);
  });

  it('closes the palette with Escape', async () => {
    ui = await renderHome();
    const row = sidebarTag('réunion').closest('.nav-row')!;
    await ui.click(row.querySelector('.tag-color-trigger'));
    expect(row.querySelector('[role="menu"]')).not.toBeNull();
    await ui.key(row.querySelector('[role="menuitemradio"]'), 'Escape');
    expect(row.querySelector('[role="menu"]')).toBeNull();
  });

  it('show a translated error when the colour cannot be saved', async () => {
    vi.spyOn(api, 'setTagColor').mockRejectedValue({ code: 'tagColorsNewer', message: 'tags.json' });
    ui = await renderHome();
    await pick(sidebarTag('physique').closest('.nav-row')!, 'Rose');
    await vi.waitFor(() => expect(ui!.q('[role="alert"]')?.textContent).toContain('version plus récente'));
  });
});

describe('palette helpers', () => {
  it('name tags as tags.json does and find their colour', () => {
    expect(tagKey('  Physique   Chimie ')).toBe('physique chimie');
    expect(colorOf({ 'physique chimie': 'teal' }, 'Physique  Chimie')).toBe('teal');
    expect(colorOf({ physique: 'nope' }, 'physique')).toBeNull();
    expect(colorOf({}, 'physique')).toBeNull();
    expect(isTagColor('blue')).toBe(true);
    expect(isTagColor('Blue')).toBe(false);
  });

  it('has a name for every colour in both languages', () => {
    for (const color of TAG_COLORS) {
      expect(lookup(fr, `library.tagColor.names.${color}`), color).toBeTruthy();
      expect(lookup(en, `library.tagColor.names.${color}`), color).toBeTruthy();
    }
  });
});
