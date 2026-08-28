import { describe, it, expect, vi, afterEach } from 'vitest';

import {
  initTimelapseRoot,
  setTimelapseRootForTests,
  timelapseRoot,
} from './timelapseRoot';
import { TEST_ROOT } from './test/setup';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

const { invoke } = await import('@tauri-apps/api/core');

afterEach(() => {
  // Restore what src/test/setup.ts established for the rest of the suite.
  setTimelapseRootForTests(TEST_ROOT);
});

describe('timelapseRoot', () => {
  it('throws rather than guessing when the root is unresolved', () => {
    setTimelapseRootForTests(null);

    // A silent fallback to "Timelapse" would mean a dev build writing into the
    // real screenshot library, so an unresolved root must be loud.
    expect(() => timelapseRoot()).toThrow(/initTimelapseRoot/);
  });

  it('takes the root from Rust, not from the bundler environment', async () => {
    setTimelapseRootForTests(null);
    vi.mocked(invoke).mockResolvedValue('Timelapse_from_rust');

    await initTimelapseRoot();

    expect(invoke).toHaveBeenCalledWith('get_timelapse_root_name');
    expect(timelapseRoot()).toBe('Timelapse_from_rust');
  });
});
