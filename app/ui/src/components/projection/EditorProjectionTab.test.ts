// @vitest-environment jsdom
// The editor's Projection tab (M17, docs/design/paper-projection-narration.md §2.6): the projector (with the editor's
// sentence), the projected area (no blackboard nor laser, « Largeur » without live's key), then the share; the same hooks
// as live's, over the browser stand-in of the projection.
import { flushSync, mount, tick, unmount } from 'svelte';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { EditorProjection } from '../../lib/editor/projection.svelte';
import { setLocale, t } from '../../lib/i18n.svelte';
import { resetMockLive } from '../../lib/mock-live';
import { mockProjectionClosedByWindow, mockProjectionState, resetMockProjection } from '../../lib/mock-projection';
import type { Note } from '../../lib/note';
import EditorProjectionTab from './EditorProjectionTab.svelte';

let cleanup: (() => void) | null = null;

const note: Note = {
  id: 'tab-note',
  title: '',
  created: 0,
  modified: 0,
  page: { width: 29700, height: 21600 },
  background: { paper: 'white', template: 'blank', texture: false },
  tags: [],
  layers: [{ name: 'Calque 1', visible: true }],
  strokes: [],
  timing: 'recorded',
  origin: { kind: 'live' },
};

const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  setLocale('fr');
  resetMockLive();
  resetMockProjection();
});
afterEach(() => {
  cleanup?.();
  cleanup = null;
  resetMockProjection();
});

async function render() {
  const projection = new EditorProjection(undefined, { frame: (run) => run() });
  projection.watch();
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(EditorProjectionTab, { target, props: { projection } });
  cleanup = () => {
    projection.dispose();
    unmount(component);
    target.remove();
  };
  await flush();
  await tick();
  const q = <E extends Element = HTMLElement>(selector: string) => target.querySelector<E>(selector);
  const click = async (selector: string) => {
    q<HTMLButtonElement>(selector)!.click();
    await flush();
    await tick();
  };
  return { target, q, click, projection };
}

describe('the editor’s Projection tab', () => {
  it('holds the projector, the projected area and the share, in this order', async () => {
    const view = await render();
    const sections = [...view.target.querySelectorAll('section.pane-block')].map((s) => s.getAttribute('data-section'));
    expect(sections).toEqual(['projector', 'zone', 'share']);
    // What projecting does here, under the projector's title.
    const projector = view.q('[data-section="projector"]')!;
    expect(projector.querySelector('h3 + p.pane-help')?.textContent).toBe(t('editor.projection.help'));
    const zone = view.q('[data-section="zone"]')!;
    expect(zone.querySelector('p.pane-help')?.textContent).toBe(t('editor.projection.zoneHint'));
    expect(zone.querySelector<HTMLButtonElement>('button[data-action="page-width"]')!.title).toBe(t('editor.projection.widthTitle'));
    expect(zone.querySelector<HTMLButtonElement>('button[data-action="page-width"]')!.title).not.toContain('(W)');
    // No blackboard, no laser pointer (Q13).
    expect(view.q('[data-field="board"]')).toBeNull();
    expect(view.q('[data-field="laser"]')).toBeNull();
    expect(view.q('[data-section="share"] button[data-action="share"]')).not.toBeNull();
  });

  it('lists the screens when it first shows, and projects on the one chosen', async () => {
    const view = await render();
    expect(view.projection.loaded).toBe(true);
    const options = [...view.target.querySelectorAll('[data-section="projector"] select option')];
    expect(options).toHaveLength(2);
    expect(view.q('[data-presentation]')!.getAttribute('data-presentation')).toBe('');
    view.projection.show(note, null);
    await view.click('button[data-action="project"]');
    const open = mockProjectionState().open;
    expect(open?.primary).toBe(false);
    expect(view.q('[data-presentation]')!.getAttribute('data-presentation')).toBe(open!.key);
    expect(mockProjectionState().note?.id).toBe('tab-note');
    // Stopped from the tab.
    await view.click('button[data-action="stop-projecting"]');
    expect(mockProjectionState().open).toBeNull();
    expect(view.q('[data-presentation]')!.getAttribute('data-presentation')).toBe('');
    expect(view.q('button[data-action="project"]')).not.toBeNull();
  });

  it('follows a projection closed from its own window (Esc there)', async () => {
    const view = await render();
    await view.click('button[data-action="project"]');
    expect(view.q('button[data-action="stop-projecting"]')).not.toBeNull();
    mockProjectionClosedByWindow();
    flushSync();
    await tick();
    expect(view.projection.presentation).toBeNull();
    expect(view.q('button[data-action="project"]')).not.toBeNull();
  });

  it('zooms the projected area with − and +, back to the whole page and to the width', async () => {
    const view = await render();
    view.projection.show(note, null);
    await view.click('button[data-action="project"]');
    const zoom = () => view.q('[data-section="zone"] [data-zoom]')!.getAttribute('data-zoom');
    expect(zoom()).toBe('1');
    await view.click(`button[aria-label="${t('live.zoom.in')}"]`);
    await view.click(`button[aria-label="${t('live.zoom.in')}"]`);
    expect(zoom()).toBe('1.563');
    expect(mockProjectionState().view?.zoom).toBe(1.563);
    await view.click('button[data-action="whole-page"]');
    expect(zoom()).toBe('1');
    await view.click('button[data-action="page-width"]');
    expect(Number(zoom())).toBeGreaterThanOrEqual(1);
  });

  it('says why it cannot project without a screen', async () => {
    const { mockLiveConfig } = await import('../../lib/mock-live');
    mockLiveConfig.monitors = 0;
    const view = await render();
    expect(view.q('[data-section="projector"]')!.textContent).toContain(t('live.projection.none'));
    expect(view.q<HTMLButtonElement>('button[data-action="project"]')!.disabled).toBe(true);
  });

  it('speaks English', async () => {
    setLocale('en');
    const view = await render();
    expect(view.q('[data-section="projector"] h3 + p.pane-help')?.textContent).toBe(t('editor.projection.help'));
    expect(t('editor.projection.help')).toMatch(/projector/);
    expect(view.q<HTMLButtonElement>('button[data-action="page-width"]')!.title).toBe(t('editor.projection.widthTitle'));
  });
});
