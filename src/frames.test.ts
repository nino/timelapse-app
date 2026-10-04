import { afterEach, describe, expect, it } from 'bun:test';

import { frameUrl } from './frames';

const realNavigator = Object.getOwnPropertyDescriptor(globalThis, 'navigator');

function pretendUserAgent(userAgent: string): void {
  Object.defineProperty(globalThis, 'navigator', { value: { userAgent }, configurable: true });
}

describe('frameUrl', () => {
  afterEach(() => {
    if (realNavigator) Object.defineProperty(globalThis, 'navigator', realNavigator);
  });

  it('uses the custom scheme on macOS', () => {
    pretendUserAgent('Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)');
    expect(frameUrl('2026-10-04', 42)).toBe('frames://localhost/2026-10-04/42');
  });

  it('uses the http form on Windows', () => {
    pretendUserAgent('Mozilla/5.0 (Windows NT 10.0; Win64; x64)');
    expect(frameUrl('2026-10-04', 0)).toBe('http://frames.localhost/2026-10-04/0');
  });
});
