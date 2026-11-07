import { describe, expect, it } from 'vitest';
import { closeKey } from './close-keys';

describe('closeKey', () => {
  it('Ctrl + W and Ctrl + F4 close, Ctrl + Q quits', () => {
    expect(closeKey({ key: 'w', ctrlKey: true })).toBe('close');
    expect(closeKey({ key: 'W', ctrlKey: true })).toBe('close');
    expect(closeKey({ key: 'F4', ctrlKey: true })).toBe('close');
    expect(closeKey({ key: 'q', ctrlKey: true })).toBe('quit');
    expect(closeKey({ key: 'Q', ctrlKey: true })).toBe('quit');
  });

  it('other modifiers, or none, are other keys', () => {
    // W alone is « page width » in live mode, and Alt + F4 is for Windows.
    expect(closeKey({ key: 'w' })).toBeNull();
    expect(closeKey({ key: 'q' })).toBeNull();
    expect(closeKey({ key: 'F4' })).toBeNull();
    expect(closeKey({ key: 'F4', altKey: true })).toBeNull();
    expect(closeKey({ key: 'w', ctrlKey: true, shiftKey: true })).toBeNull();
    expect(closeKey({ key: 'q', ctrlKey: true, altKey: true })).toBeNull();
    expect(closeKey({ key: 's', ctrlKey: true })).toBeNull();
  });

  it('a held key acts once', () => {
    expect(closeKey({ key: 'w', ctrlKey: true, repeat: true })).toBeNull();
    expect(closeKey({ key: 'q', ctrlKey: true, repeat: true })).toBeNull();
  });
});
