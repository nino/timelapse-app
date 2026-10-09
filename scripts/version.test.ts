import { describe, expect, it } from 'bun:test';

import { compareVersions, isLegacy, nextVersion, parseTags, TAG_FORMAT } from './version';

describe('compareVersions', () => {
  it('compares numerically, not as text', () => {
    expect(compareVersions('0.2.10', '0.2.9')).toBeGreaterThan(0);
    expect(compareVersions('v1.0.0', '0.9.99')).toBeGreaterThan(0);
    expect(compareVersions('0.2.3', 'v0.2.3')).toBe(0);
  });
});

describe('isLegacy', () => {
  it('numbers versions before 0.2.0 by commit count', () => {
    expect(isLegacy('0.1.3')).toBe(true);
    expect(isLegacy('0.2.0')).toBe(false);
    expect(isLegacy('1.0.0')).toBe(false);
  });
});

describe('nextVersion', () => {
  it('starts at the config’s version when nothing is released', () => {
    expect(nextVersion('0.2.0', [])).toBe('0.2.0');
  });

  it('is one patch above the highest release', () => {
    expect(nextVersion('0.2.0', ['0.2.0', '0.2.9', '0.2.10'])).toBe('0.2.11');
  });

  it('jumps to the config’s version after a minor bump', () => {
    expect(nextVersion('0.3.0', ['0.2.0', '0.2.7'])).toBe('0.3.0');
    expect(nextVersion('0.3.0', ['0.3.0'])).toBe('0.3.1');
  });
});

describe('parseTags', () => {
  it('maps lightweight and annotated v-tags to their commits, skipping others', () => {
    expect(TAG_FORMAT).toContain('%(*objectname)');
    const refs = ['v0.2.0  aaa', 'v0.2.1 bbb ccc', 'beta  ddd', 'v0.2.2  aaa', ''].join('\n');
    expect(Object.fromEntries(parseTags(refs))).toEqual({ aaa: '0.2.2', bbb: '0.2.1' });
  });
});
