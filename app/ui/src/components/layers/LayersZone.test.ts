// @vitest-environment jsdom
/// <reference types="node" />
import { readFileSync } from 'node:fs';
import { mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setLocale } from '../../lib/i18n.svelte';
import type { Layer } from '../../lib/note';
import LayersZoneHarness from './LayersZoneHarness.svelte';

let cleanup: (() => void) | null = null;

beforeEach(() => setLocale('fr'));
afterEach(() => {
  cleanup?.();
  cleanup = null;
});

const stack = (...visible: boolean[]): Layer[] => visible.map((v, i) => ({ name: `Calque ${i + 1}`, visible: v }));

interface Init {
  where?: 'editor' | 'live';
  layers?: Layer[];
  chosen?: number;
  tablet?: number;
  disabled?: boolean;
}

function render(init: Init = {}) {
  cleanup?.();
  const calls: unknown[][] = [];
  const oncall = vi.fn((name: string, ...args: unknown[]) => void calls.push([name, ...args]));
  const target = document.createElement('div');
  document.body.append(target);
  const where = init.where ?? 'editor';
  const initial = {
    where,
    layers: init.layers ?? stack(true, true, true),
    chosen: init.chosen ?? 2,
    // The editor has no tablet: it passes none.
    tablet: where === 'live' ? (init.tablet ?? 0) : undefined,
    disabled: init.disabled,
  };
  const harness = mount(LayersZoneHarness, { target, props: { initial, oncall } });
  cleanup = () => {
    unmount(harness);
    target.remove();
  };
  const zone = target.querySelector<HTMLElement>('section.layers-zone')!;
  const row = (index: number) => zone.querySelector<HTMLLIElement>(`li.layer[data-layer="${index}"]`)!;
  const part = <T extends HTMLElement = HTMLButtonElement>(index: number, selector: string) =>
    row(index).querySelector<T>(selector);
  const rowsOrder = () => [...zone.querySelectorAll<HTMLElement>('li.layer')].map((li) => Number(li.dataset.layer));
  const field = () => zone.querySelector<HTMLInputElement>('input.layer-rename');
  const notices = () => [...zone.querySelectorAll<HTMLElement>('p[data-slot="layer-warning"]')];
  const key = (el: Element, k: string) => {
    const event = new KeyboardEvent('keydown', { key: k, bubbles: true, cancelable: true });
    el.dispatchEvent(event);
    return event;
  };
  const type = (el: HTMLInputElement, text: string) => {
    el.value = text;
    el.dispatchEvent(new Event('input', { bubbles: true }));
  };
  /** Starts renaming layer `index` through its pencil. */
  const startRename = async (index: number) => {
    part(index, '[data-action="rename-layer"]')!.click();
    await tick();
    return field()!;
  };
  return {
    target,
    zone,
    row,
    part,
    rowsOrder,
    field,
    notices,
    key,
    type,
    startRename,
    calls,
    oncall,
    set: harness.set as (patch: Init) => void,
  };
}

describe('the layers zone: structure', () => {
  it('lists the layers top first under its title, in a labelled list', () => {
    const { zone, rowsOrder } = render();
    expect(zone.dataset.where).toBe('editor');
    expect(zone.querySelector('h3')!.textContent).toBe('Calques');
    expect(zone.querySelector('ul.layers')!.getAttribute('aria-label')).toBe('Calques de la page');
    expect(zone.getAttribute('aria-labelledby')).toBe(zone.querySelector('h3')!.id);
    expect(rowsOrder()).toEqual([2, 1, 0]);
  });

  it('frames the chosen row, with the data attribute, the class and aria-pressed on its name button', () => {
    const { zone, row, part } = render({ chosen: 1 });
    expect([...zone.querySelectorAll('li[data-chosen="true"]')].map((li) => (li as HTMLElement).dataset.layer)).toEqual(['1']);
    expect(row(1).classList.contains('chosen')).toBe(true);
    expect(row(0).classList.contains('chosen')).toBe(false);
    expect(part(1, 'button.layer-name')!.getAttribute('aria-pressed')).toBe('true');
    expect(part(0, 'button.layer-name')!.getAttribute('aria-pressed')).toBe('false');
    expect(part(2, 'button.layer-name')!.getAttribute('aria-pressed')).toBe('false');
  });

  it('names the layer, and what the chosen one is for, in the label and the title', () => {
    const { part } = render({ chosen: 1 });
    const chosen = part(1, 'button.layer-name')!;
    expect(chosen.textContent!.trim()).toBe('Calque 2');
    expect(chosen.getAttribute('aria-label')).toBe('Le stylet, la gomme et la sélection agissent sur « Calque 2 »');
    expect(chosen.title).toBe(chosen.getAttribute('aria-label'));
    const other = part(0, 'button.layer-name')!;
    expect(other.getAttribute('aria-label')).toBe('Agir sur « Calque 1 »');
    expect(other.title).toBe('Agir sur « Calque 1 »');
  });

  it('says in live that the computer acts on the chosen layer', () => {
    const { part } = render({ where: 'live', chosen: 1 });
    expect(part(1, 'button.layer-name')!.getAttribute('aria-label')).toBe("L'ordinateur agit sur « Calque 2 »");
  });

  it('puts the row controls in order: eye, name, pencil, then the chosen row’s three buttons', () => {
    const { row } = render({ chosen: 1 });
    expect([...row(1).children].map((el) => el.getAttribute('data-action') ?? (el.classList.contains('order') ? 'order' : el.tagName))).toEqual([
      'layer-visible',
      'choose-layer',
      'rename-layer',
      'order',
    ]);
    expect([...row(1).querySelectorAll('.order button')].map((b) => b.getAttribute('data-action'))).toEqual([
      'layer-up',
      'layer-down',
      'layer-merge',
    ]);
  });

  it('marks a hidden layer and shows the eye barred', () => {
    const { row, part } = render({ layers: stack(true, false, true), chosen: 2 });
    expect(row(1).dataset.hidden).toBe('true');
    expect(row(0).dataset.hidden).toBeUndefined();
    expect(part(1, '[data-action="layer-visible"]')!.dataset.visible).toBe('false');
    expect(part(1, '[data-action="layer-visible"] svg')!.getAttribute('data-icon')).toBe('eyeOff');
    expect(part(0, '[data-action="layer-visible"] svg')!.getAttribute('data-icon')).toBe('eye');
  });

  it('gives the icons no accessible name of their own', () => {
    const { zone } = render({ where: 'live' });
    const icons = [...zone.querySelectorAll('svg')];
    expect(icons.length).toBeGreaterThan(6);
    expect(icons.every((svg) => svg.getAttribute('aria-hidden') === 'true')).toBe(true);
  });

  it('is in English too', () => {
    setLocale('en');
    const { zone, part, notices } = render({
      layers: [
        { name: 'Base', visible: false },
        { name: 'Top', visible: true },
      ],
      chosen: 0,
    });
    expect(zone.querySelector('h3')!.textContent).toBe('Layers');
    expect(part(0, 'button.layer-name')!.getAttribute('aria-label')).toBe('The pen, the eraser and the selection act on “Base”');
    expect(part(0, '[data-action="layer-visible"]')!.getAttribute('aria-label')).toBe('Show “Base”');
    expect(part(1, '[data-action="layer-visible"]')!.getAttribute('aria-label')).toBe('Hide “Top”');
    expect(notices()[0].textContent!.trim()).toBe('The chosen layer, “Base”, is hidden: show it to write on it.');
  });
});

describe('the layers zone: choosing a layer', () => {
  it('chooses on a click anywhere on the row outside its buttons', () => {
    const { row, calls } = render({ chosen: 2 });
    row(0).click();
    expect(calls).toEqual([['choose', 0]]);
  });

  it('chooses once on a click on the name button, which is also the keyboard’s control', () => {
    const { part, calls } = render({ chosen: 2 });
    const name = part(1, 'button.layer-name')!;
    expect(name.tagName).toBe('BUTTON');
    expect(name.getAttribute('type')).toBe('button');
    expect(name.hasAttribute('tabindex')).toBe(false);
    name.focus();
    expect(document.activeElement).toBe(name);
    // Enter and Space on a focused button make the browser click it (the e2e journey presses them).
    name.click();
    expect(calls).toEqual([['choose', 1]]);
  });

  it('does nothing on the chosen row', () => {
    const { row, part, calls } = render({ chosen: 1 });
    row(1).click();
    part(1, 'button.layer-name')!.click();
    expect(calls).toEqual([]);
  });

  it('does not choose when a button of the row is used', () => {
    const { part, calls } = render({ chosen: 2 });
    part(0, '[data-action="layer-visible"]')!.click();
    part(0, '[data-action="rename-layer"]')!.click();
    expect(calls).toEqual([['visible', 0, false]]);
  });

  it('follows the chosen layer it is given', async () => {
    const { row, part, set } = render({ chosen: 2 });
    set({ chosen: 0 });
    await tick();
    expect(row(0).dataset.chosen).toBe('true');
    expect(row(2).dataset.chosen).toBeUndefined();
    expect(part(0, 'button.layer-name')!.getAttribute('aria-pressed')).toBe('true');
    expect(row(0).querySelector('.order')).not.toBeNull();
    expect(row(2).querySelector('.order')).toBeNull();
  });
});

describe('the layers zone: the eye', () => {
  it('hides a visible layer and shows a hidden one, with the name of what it does', () => {
    const { part, calls } = render({ layers: stack(true, false, true), chosen: 2 });
    const hide = part(0, '[data-action="layer-visible"]')!;
    expect(hide.dataset.visible).toBe('true');
    expect(hide.getAttribute('aria-label')).toBe('Masquer « Calque 1 »');
    expect(hide.title).toBe('Masquer « Calque 1 »');
    const show = part(1, '[data-action="layer-visible"]')!;
    expect(show.dataset.visible).toBe('false');
    expect(show.getAttribute('aria-label')).toBe('Afficher « Calque 2 »');
    hide.click();
    show.click();
    expect(calls).toEqual([
      ['visible', 0, false],
      ['visible', 1, true],
    ]);
  });
});

describe('the layers zone: renaming', () => {
  it('opens a field in place of the name, focused with its text selected', async () => {
    const { row, part, startRename } = render({ chosen: 2 });
    const field = await startRename(1);
    expect(field).toBeTruthy();
    expect(field.value).toBe('Calque 2');
    expect(field.getAttribute('aria-label')).toBe('Nom du calque 2');
    expect(document.activeElement).toBe(field);
    expect([field.selectionStart, field.selectionEnd]).toEqual([0, 'Calque 2'.length]);
    expect(part(1, 'button.layer-name')).toBeNull();
    expect(row(1).contains(field)).toBe(true);
  });

  it('keeps the new name on Enter, trimmed, and gives the focus back to the name button', async () => {
    const { part, field, startRename, type, key, calls } = render({ chosen: 2 });
    const input = await startRename(1);
    type(input, '  Correction  ');
    const event = key(input, 'Enter');
    expect(event.defaultPrevented).toBe(true);
    await tick();
    await tick();
    expect(calls).toEqual([['rename', 1, 'Correction']]);
    expect(field()).toBeNull();
    expect(document.activeElement).toBe(part(1, 'button.layer-name'));
  });

  it('gives the old name back on Escape, and the key goes no further', async () => {
    const { part, field, startRename, type, key, calls } = render({ chosen: 2 });
    const onWindow = vi.fn();
    window.addEventListener('keydown', onWindow);
    try {
      const input = await startRename(1);
      type(input, 'Autre');
      key(input, 'Escape');
      await tick();
      await tick();
      expect(onWindow).not.toHaveBeenCalled();
    } finally {
      window.removeEventListener('keydown', onWindow);
    }
    expect(calls).toEqual([]);
    expect(field()).toBeNull();
    expect(part(1, 'button.layer-name')!.textContent!.trim()).toBe('Calque 2');
    expect(document.activeElement).toBe(part(1, 'button.layer-name'));
  });

  it('keeps the new name when the field is left', async () => {
    const { field, startRename, type, calls } = render({ chosen: 2 });
    const input = await startRename(0);
    type(input, 'Fond');
    input.dispatchEvent(new FocusEvent('blur'));
    await tick();
    expect(calls).toEqual([['rename', 0, 'Fond']]);
    expect(field()).toBeNull();
  });

  it('calls nothing for an empty name, or one that did not change, and shows the old name', async () => {
    const { part, field, startRename, type, key, calls } = render({ chosen: 2 });
    let input = await startRename(1);
    type(input, '   ');
    key(input, 'Enter');
    await tick();
    expect(field()).toBeNull();
    expect(part(1, 'button.layer-name')!.textContent!.trim()).toBe('Calque 2');
    input = await startRename(1);
    type(input, ' Calque 2 ');
    key(input, 'Enter');
    await tick();
    input = await startRename(1);
    key(input, 'Enter');
    await tick();
    expect(calls).toEqual([]);
  });

  it('calls once when Enter is followed by the blur the removal of the field brings', async () => {
    const { startRename, type, key, calls } = render({ chosen: 2 });
    const input = await startRename(1);
    type(input, 'Une fois');
    key(input, 'Enter');
    input.dispatchEvent(new FocusEvent('blur'));
    await tick();
    expect(calls).toEqual([['rename', 1, 'Une fois']]);
  });

  it('has one field at a time', async () => {
    const { zone, startRename } = render({ chosen: 2 });
    await startRename(0);
    await startRename(1);
    expect(zone.querySelectorAll('input.layer-rename')).toHaveLength(1);
    expect(zone.querySelector('li[data-layer="1"] input.layer-rename')).not.toBeNull();
  });

  it('does not choose the row when its field is clicked', async () => {
    const { startRename, calls } = render({ chosen: 2 });
    const input = await startRename(0);
    input.click();
    expect(calls).toEqual([]);
  });

  it('closes the field when the zone is disabled', async () => {
    const { field, startRename, set } = render({ chosen: 2 });
    await startRename(1);
    set({ disabled: true });
    await tick();
    await tick();
    expect(field()).toBeNull();
  });

  it('closes the field of a layer that is gone', async () => {
    const { field, startRename, set } = render({ chosen: 2 });
    await startRename(2);
    set({ layers: stack(true, true), chosen: 1 });
    await tick();
    await tick();
    expect(field()).toBeNull();
  });
});

describe('the layers zone: moving and merging', () => {
  it('offers up, down and merge on the chosen row only', () => {
    const { zone, row } = render({ chosen: 1 });
    expect(zone.querySelectorAll('[data-action="layer-up"]')).toHaveLength(1);
    expect(zone.querySelectorAll('[data-action="layer-down"]')).toHaveLength(1);
    expect(zone.querySelectorAll('[data-action="layer-merge"]')).toHaveLength(1);
    expect(row(1).querySelector('[data-action="layer-up"]')).not.toBeNull();
    expect(row(0).querySelector('[data-action="layer-up"]')).toBeNull();
    expect(row(2).querySelector('[data-action="layer-merge"]')).toBeNull();
  });

  it('moves the chosen layer one place up or down, and merges it into the one below', () => {
    const { part, calls } = render({ chosen: 1 });
    part(1, '[data-action="layer-up"]')!.click();
    part(1, '[data-action="layer-down"]')!.click();
    part(1, '[data-action="layer-merge"]')!.click();
    expect(calls).toEqual([
      ['move', 1, 2],
      ['move', 1, 0],
      ['merge', 1],
    ]);
  });

  it('labels them with the layer name', () => {
    const { part } = render({ chosen: 1 });
    expect(part(1, '[data-action="layer-up"]')!.getAttribute('aria-label')).toBe('Monter « Calque 2 »');
    expect(part(1, '[data-action="layer-down"]')!.getAttribute('aria-label')).toBe('Descendre « Calque 2 »');
    expect(part(1, '[data-action="layer-merge"]')!.getAttribute('aria-label')).toBe(
      'Fusionner « Calque 2 » avec le calque du dessous',
    );
  });

  it('cannot move the top layer up', () => {
    const { part, calls } = render({ chosen: 2 });
    const up = part(2, '[data-action="layer-up"]')!;
    expect(up.disabled).toBe(true);
    expect(part(2, '[data-action="layer-down"]')!.disabled).toBe(false);
    expect(part(2, '[data-action="layer-merge"]')!.disabled).toBe(false);
    up.click();
    expect(calls).toEqual([]);
  });

  it('cannot move the bottom layer down, nor merge it', () => {
    const { part, calls } = render({ chosen: 0 });
    expect(part(0, '[data-action="layer-up"]')!.disabled).toBe(false);
    expect(part(0, '[data-action="layer-down"]')!.disabled).toBe(true);
    expect(part(0, '[data-action="layer-merge"]')!.disabled).toBe(true);
    part(0, '[data-action="layer-down"]')!.click();
    part(0, '[data-action="layer-merge"]')!.click();
    expect(calls).toEqual([]);
  });

  it('has a lone layer neither move nor merge', () => {
    const { part } = render({ layers: stack(true), chosen: 0 });
    for (const action of ['layer-up', 'layer-down', 'layer-merge']) {
      expect(part(0, `[data-action="${action}"]`)!.disabled).toBe(true);
    }
  });

  it('says in live, in the merge title only, that Ctrl+Z does not undo it', () => {
    const live = render({ where: 'live', chosen: 1 });
    expect(live.part(1, '[data-action="layer-merge"]')!.title).toBe(
      "Fusionner « Calque 2 » avec le calque du dessous Ctrl+Z ne l'annule pas.",
    );
    expect(live.part(1, '[data-action="layer-merge"]')!.getAttribute('aria-label')).toBe(
      'Fusionner « Calque 2 » avec le calque du dessous',
    );
    const editor = render({ where: 'editor', chosen: 1 });
    expect(editor.part(1, '[data-action="layer-merge"]')!.title).toBe('Fusionner « Calque 2 » avec le calque du dessous');
  });
});

describe('the layers zone: adding a layer', () => {
  it('has a button under the list that calls onadd', () => {
    const { zone, calls } = render();
    const add = zone.querySelector<HTMLButtonElement>('button[data-action="add-layer"]')!;
    expect(add.getAttribute('aria-label')).toBe('Ajouter un calque');
    expect(add.textContent!.trim()).toBe('+ Ajouter un calque');
    expect(zone.querySelector('ul.layers')!.compareDocumentPosition(add) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    add.click();
    expect(calls).toEqual([['add']]);
  });
});

describe('the layers zone: the tablet (live)', () => {
  it('shows no tablet icon in the editor', () => {
    const { zone } = render({ where: 'editor' });
    expect(zone.querySelector('[data-action="tablet-layer"]')).toBeNull();
    expect(zone.querySelector('[data-tablet]')).toBeNull();
  });

  it('shows the tablet icon on every row in live, and marks the tablet’s row', () => {
    const { zone, row, part } = render({ where: 'live', chosen: 2, tablet: 1 });
    expect(zone.querySelectorAll('button[data-action="tablet-layer"]')).toHaveLength(3);
    expect([...zone.querySelectorAll('li[data-tablet="true"]')].map((li) => (li as HTMLElement).dataset.layer)).toEqual(['1']);
    const here = part(1, '[data-action="tablet-layer"]')!;
    expect(here.dataset.here).toBe('true');
    expect(here.classList.contains('here')).toBe(true);
    expect(here.classList.contains('elsewhere')).toBe(false);
    expect(here.getAttribute('aria-label')).toBe('La tablette écrit sur « Calque 2 »');
    const elsewhere = part(0, '[data-action="tablet-layer"]')!;
    expect(elsewhere.dataset.here).toBeUndefined();
    expect(elsewhere.classList.contains('elsewhere')).toBe(true);
    expect(elsewhere.getAttribute('aria-label')).toBe('Faire écrire la tablette sur « Calque 1 »');
    expect(elsewhere.title).toBe('Faire écrire la tablette sur « Calque 1 »');
    // The icon comes after the pencil and before the chosen row's buttons.
    expect([...row(2).children].map((el) => el.getAttribute('data-action') ?? (el.classList.contains('order') ? 'order' : el.tagName))).toEqual([
      'layer-visible',
      'choose-layer',
      'rename-layer',
      'tablet-layer',
      'order',
    ]);
  });

  it('sends the tablet to another layer on a click, and does nothing on its own layer', () => {
    const { part, row, calls } = render({ where: 'live', chosen: 2, tablet: 1 });
    part(0, '[data-action="tablet-layer"]')!.click();
    part(1, '[data-action="tablet-layer"]')!.click();
    expect(calls).toEqual([['tablet', 0]]);
    // The click did not choose the row either.
    row(0).click();
    expect(calls).toEqual([
      ['tablet', 0],
      ['choose', 0],
    ]);
  });

  it('is hidden on the other rows until hover or focus, and always pale where a finger has no hover', () => {
    const source = readFileSync(`${process.cwd()}/ui/src/components/layers/LayersZone.svelte`, 'utf8');
    expect(source).toMatch(/\.tablet-button\.elsewhere\s*\{[^}]*opacity:\s*0;/);
    expect(source).toMatch(
      /\.layer:hover \.tablet-button\.elsewhere[^{]*,\s*\.tablet-button\.elsewhere:focus-visible\s*\{\s*opacity:\s*0\.55/,
    );
    expect(source).toMatch(/@media \(hover: none\)\s*\{\s*\.tablet-button\.elsewhere[^{]*\{\s*opacity:\s*0\.55/);
  });

  it('follows the tablet’s layer it is given', async () => {
    const { zone, part, set } = render({ where: 'live', chosen: 2, tablet: 0 });
    set({ tablet: 2 });
    await tick();
    expect([...zone.querySelectorAll('li[data-tablet="true"]')].map((li) => (li as HTMLElement).dataset.layer)).toEqual(['2']);
    expect(part(2, '[data-action="tablet-layer"]')!.dataset.here).toBe('true');
    expect(part(0, '[data-action="tablet-layer"]')!.dataset.here).toBeUndefined();
  });

  it('shows the tablet’s icon as a warning on a hidden layer', () => {
    const { part } = render({ where: 'live', layers: stack(false, true), chosen: 1, tablet: 0 });
    expect(part(0, '[data-action="tablet-layer"]')!.classList.contains('warn')).toBe(true);
    expect(part(1, '[data-action="tablet-layer"]')!.classList.contains('warn')).toBe(false);
  });
});

describe('the layers zone: warnings and help', () => {
  it('is quiet while everything shows', () => {
    expect(render({ where: 'live', chosen: 1, tablet: 0 }).notices()).toEqual([]);
  });

  it('warns that the chosen layer is hidden, in the editor and in live', () => {
    for (const where of ['editor', 'live'] as const) {
      const { notices } = render({ where, layers: stack(true, false), chosen: 1, tablet: 0 });
      expect(notices()).toHaveLength(1);
      expect(notices()[0].dataset.kind).toBe('chosenHidden');
      expect(notices()[0].getAttribute('role')).toBe('status');
      expect(notices()[0].classList.contains('notice')).toBe(true);
      expect(notices()[0].textContent!.trim()).toBe('Le calque choisi, « Calque 2 », est masqué : affichez-le pour y écrire.');
    }
  });

  it('warns in live that the tablet writes on a hidden layer, before the other warning', () => {
    const { notices } = render({ where: 'live', layers: stack(false, false, true), chosen: 1, tablet: 0 });
    expect(notices().map((n) => n.dataset.kind)).toEqual(['tabletHidden', 'chosenHidden']);
    expect(notices()[0].textContent!.trim()).toBe(
      "La tablette écrit sur « Calque 1 », qui est masqué : ce que l'élève écrit n'est ni projeté ni partagé.",
    );
  });

  it('never warns about the tablet in the editor', () => {
    const { notices } = render({ where: 'editor', layers: stack(false, true), chosen: 1 });
    expect(notices()).toEqual([]);
  });

  it('shows and removes a warning as the layers change', async () => {
    const { notices, set } = render({ where: 'live', chosen: 1, tablet: 0 });
    set({ layers: stack(true, false) });
    await tick();
    expect(notices().map((n) => n.dataset.kind)).toEqual(['chosenHidden']);
    set({ layers: stack(true, true) });
    await tick();
    expect(notices()).toEqual([]);
  });

  it('shows the help of the place', () => {
    const editor = render({ where: 'editor' });
    expect(editor.zone.querySelector('p.help[data-slot="layers-help"]')!.textContent!.trim()).toBe(
      'Le stylet, la gomme et la sélection agissent sur le calque choisi (encadré).',
    );
    const live = render({ where: 'live' });
    const help = live.zone.querySelector('p.help[data-slot="layers-help"]')!.textContent!;
    expect(help).toContain("Le stylet de l'ordinateur");
    expect(help).toContain('icône de tablette');
  });
});

describe('the layers zone: disabled', () => {
  it('disables every button, and a click on a row chooses nothing', () => {
    const { zone, row, calls } = render({ where: 'live', chosen: 1, tablet: 0, disabled: true });
    const buttons = [...zone.querySelectorAll<HTMLButtonElement>('button')];
    expect(buttons.length).toBeGreaterThan(10);
    expect(buttons.every((b) => b.disabled)).toBe(true);
    row(0).click();
    zone.querySelector<HTMLButtonElement>('[data-action="add-layer"]')!.click();
    expect(calls).toEqual([]);
  });

  it('gives everything back when it is enabled again', async () => {
    const { zone, set } = render({ chosen: 1, disabled: true });
    set({ disabled: false });
    await tick();
    expect(zone.querySelector<HTMLButtonElement>('[data-action="add-layer"]')!.disabled).toBe(false);
    expect(zone.querySelector<HTMLButtonElement>('li[data-layer="1"] [data-action="layer-up"]')!.disabled).toBe(false);
  });
});
