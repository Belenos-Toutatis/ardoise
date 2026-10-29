import { describe, expect, it } from 'vitest';
import { filterComponents, type AboutComponent } from './about';

const components: AboutComponent[] = [
  { name: 'serde', version: '1.0.229', license: 'MIT OR Apache-2.0', kind: 'rust' },
  { name: 'webm-sys', version: '2.2.1', license: 'MPL-2.0', kind: 'rust' },
  { name: 'svelte', version: '5.43.0', license: 'MIT', kind: 'npm' },
  { name: 'imagequant', version: '4.4.1', license: 'GPL-3.0-or-later', kind: 'rust' },
];

const names = (list: AboutComponent[]) => list.map((c) => c.name);

describe('filterComponents', () => {
  it('keeps everything, in order, for an empty query', () => {
    expect(names(filterComponents(components, ''))).toEqual(['serde', 'webm-sys', 'svelte', 'imagequant']);
    expect(names(filterComponents(components, '   '))).toEqual(['serde', 'webm-sys', 'svelte', 'imagequant']);
  });

  it('finds by name, version or licence, ignoring case', () => {
    expect(names(filterComponents(components, 'SERDE'))).toEqual(['serde']);
    expect(names(filterComponents(components, '2.2.1'))).toEqual(['webm-sys']);
    expect(names(filterComponents(components, 'mpl'))).toEqual(['webm-sys']);
    expect(names(filterComponents(components, 'mit'))).toEqual(['serde', 'svelte']);
  });

  it('wants every word', () => {
    expect(names(filterComponents(components, 'mit apache'))).toEqual(['serde']);
    expect(names(filterComponents(components, 'mit mpl'))).toEqual([]);
  });

  it('does not modify its input', () => {
    const copy = [...components];
    filterComponents(components, 'zzz');
    expect(components).toEqual(copy);
  });
});
