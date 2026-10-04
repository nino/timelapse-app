import { afterEach, describe, expect, it } from 'bun:test';
import { invoke } from '@tauri-apps/api/core';

import {
  initTimelapseRoot,
  setTimelapseRootForTests,
  timelapseRoot,
} from './timelapseRoot';
import { mocked } from './test/mocked';
import { TEST_ROOT } from './test/setup';

// invoke is replaced with a mock in src/test/setup.ts.

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
    mocked(invoke).mockResolvedValue('Timelapse_from_rust');

    await initTimelapseRoot();

    expect(invoke).toHaveBeenCalledWith('get_timelapse_root_name');
    expect(timelapseRoot()).toBe('Timelapse_from_rust');
  });
});
