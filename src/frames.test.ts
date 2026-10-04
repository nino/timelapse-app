import { describe, it, expect, vi, afterEach } from 'vitest';

import { frameUrl } from './frames';

describe('frameUrl', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('uses the custom scheme on macOS', () => {
    vi.stubGlobal('navigator', { userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)' });
    expect(frameUrl('2026-10-04', 42)).toBe('frames://localhost/2026-10-04/42');
  });

  it('uses the http form on Windows', () => {
    vi.stubGlobal('navigator', { userAgent: 'Mozilla/5.0 (Windows NT 10.0; Win64; x64)' });
    expect(frameUrl('2026-10-04', 0)).toBe('http://frames.localhost/2026-10-04/0');
  });
});
