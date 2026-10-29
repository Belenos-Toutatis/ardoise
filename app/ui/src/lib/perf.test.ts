import { afterEach, describe, expect, it, vi } from 'vitest';
import { LIBRARY_LISTED_MARK, markWhenDrawn, noteDrawn } from './perf';

describe('performance marks', () => {
  afterEach(() => vi.unstubAllGlobals());

  it('sets a mark once, two frames after it is asked for', () => {
    const frames: Array<() => void> = [];
    vi.stubGlobal('requestAnimationFrame', (callback: () => void) => frames.push(callback));
    const mark = vi.spyOn(performance, 'mark').mockImplementation(() => ({}) as PerformanceMark);
    markWhenDrawn(LIBRARY_LISTED_MARK);
    markWhenDrawn(LIBRARY_LISTED_MARK);
    expect(frames).toHaveLength(1);
    frames.shift()?.();
    expect(mark).not.toHaveBeenCalled();
    frames.shift()?.();
    expect(mark).toHaveBeenCalledTimes(1);
    expect(mark).toHaveBeenCalledWith(LIBRARY_LISTED_MARK);
    mark.mockRestore();
  });

  it('records when a thumbnail was drawn', () => {
    const image = { dataset: {} as Record<string, string> };
    noteDrawn({ currentTarget: image } as unknown as Event);
    expect(Number(image.dataset.drawnAt)).toBeGreaterThan(0);
    noteDrawn({ currentTarget: null } as unknown as Event);
  });
});
