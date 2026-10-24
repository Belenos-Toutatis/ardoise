// @vitest-environment jsdom
import { describe, expect, it, vi } from 'vitest';
import type { FileStamp, NoteSummary } from '../note';
import { binderOf, diskChange, samePath } from './disk';
import { browserWindowClose } from './window-close';

const stamp = (secs: number, len = 10): FileStamp => ({ modified: { secs_since_epoch: secs, nanos_since_epoch: 0 }, len });

const entry = (path: string, s: FileStamp) => ({ path, stamp: s }) as NoteSummary;

describe('the open note on disk', () => {
  it('finds the binder of a note from its path', () => {
    const library = 'C:\\demo\\Ardoise';
    expect(binderOf('C:\\demo\\Ardoise\\Classe\\3e B\\2026-10-20 Cours.ardoise', library)).toBe('Classe/3e B');
    expect(binderOf('C:\\demo\\Ardoise\\2026-10-22 Liste.ardoise', library)).toBe('');
    expect(binderOf('c:/demo/ardoise/Perso/x.ardoise', 'C:\\demo\\Ardoise\\')).toBe('Perso');
    expect(binderOf('D:\\ailleurs\\x.ardoise', library)).toBe('');
    expect(binderOf('C:\\demo\\Ardoise\\x.ardoise', null)).toBe('');
  });

  it('tells a changed or missing file from an unchanged one', () => {
    const notes = [entry('C:\\lib\\a.ardoise', stamp(1)), entry('C:\\lib\\b.ardoise', stamp(2))];
    expect(diskChange(notes, 'C:\\lib\\a.ardoise', stamp(1))).toBeNull();
    expect(diskChange(notes, 'c:/lib/A.ardoise', stamp(1))).toBeNull();
    expect(diskChange(notes, 'C:\\lib\\a.ardoise', stamp(1, 11))).toBe('changed');
    expect(diskChange(notes, 'C:\\lib\\b.ardoise', stamp(3))).toBe('changed');
    expect(diskChange(notes, 'C:\\lib\\c.ardoise', stamp(1))).toBe('missing');
    expect(samePath('C:\\lib\\a.ardoise', 'C:\\lib\\b.ardoise')).toBe(false);
  });

  it('holds back leaving the page in a browser only when the guard says so', async () => {
    const guard = vi.fn(() => false);
    const closer = browserWindowClose(window);
    expect(closer.destroy).toBeNull();
    const stop = await closer.subscribe(guard);
    const leave = () => {
      const event = new Event('beforeunload', { cancelable: true });
      window.dispatchEvent(event);
      return event.defaultPrevented;
    };
    expect(leave()).toBe(false);
    guard.mockReturnValue(true);
    expect(leave()).toBe(true);
    stop();
    expect(leave()).toBe(false);
    expect(guard).toHaveBeenCalledTimes(2);
  });
});
