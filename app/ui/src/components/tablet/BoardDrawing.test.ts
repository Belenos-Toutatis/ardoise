// @vitest-environment jsdom
import { mount, unmount } from 'svelte';
import { afterEach, describe, expect, it } from 'vitest';
import { BOARD_POSITIONS } from '../../lib/board-position';
import BoardDrawing from './BoardDrawing.svelte';

let cleanup: (() => void) | null = null;

afterEach(() => {
  cleanup?.();
  cleanup = null;
});

function draw(props: { position?: (typeof BOARD_POSITIONS)[number]; size?: number }) {
  const target = document.createElement('div');
  document.body.append(target);
  const component = mount(BoardDrawing, { target, props });
  cleanup = () => {
    unmount(component);
    target.remove();
  };
  return target.querySelector('svg')!;
}

describe('the Xiaomi blackboard\'s drawing', () => {
  it('is upright by default, portrait, with the bin and the disk on the bottom band, and is decorative', () => {
    const svg = draw({});
    expect(svg.getAttribute('data-position')).toBe('buttonsBottom');
    expect(svg.getAttribute('viewBox')).toBe('0 0 46 62');
    expect(svg.getAttribute('aria-hidden')).toBe('true');
    expect(svg.getAttribute('height')).toBe('40');
    expect(svg.getAttribute('width')).toBe('30');
    expect(svg.querySelector('g[transform]')).toBeNull();
    expect(svg.querySelector('.screen')).not.toBeNull();
    // The two marks (bin, disk) are one group of two strokes.
    expect(svg.querySelectorAll('.marks path')).toHaveLength(2);
  });

  it('turns the same picture for each position: half a turn, and a quarter turn either way, wide', () => {
    const turned = (position: (typeof BOARD_POSITIONS)[number]) => {
      const svg = draw({ position, size: 35 });
      const result = {
        viewBox: svg.getAttribute('viewBox'),
        width: svg.getAttribute('width'),
        height: svg.getAttribute('height'),
        transform: svg.querySelector('g')!.getAttribute('transform'),
      };
      cleanup?.();
      cleanup = null;
      return result;
    };
    expect(turned('buttonsBottom')).toEqual({ viewBox: '0 0 46 62', width: '26', height: '35', transform: null });
    expect(turned('buttonsTop')).toEqual({ viewBox: '0 0 46 62', width: '26', height: '35', transform: 'rotate(180 23 31)' });
    // Buttons at the right: the bottom edge goes to the right edge; at the left, to the left edge.
    expect(turned('buttonsRight')).toEqual({ viewBox: '0 0 62 46', width: '35', height: '26', transform: 'translate(0 46) rotate(-90)' });
    expect(turned('buttonsLeft')).toEqual({ viewBox: '0 0 62 46', width: '35', height: '26', transform: 'translate(62 0) rotate(90)' });
  });
});
