import '@testing-library/jest-dom';
import { expect, afterEach, vi } from 'vitest';
import { cleanup } from '@testing-library/react';
import * as matchers from '@testing-library/jest-dom/matchers';

import { setTimelapseRootForTests } from '../timelapseRoot';

// Extend Vitest's expect with jest-dom matchers
expect.extend(matchers);

// Production resolves this from Rust at startup; tests pin it to a value that
// is deliberately NOT the production name, so any path built from a hardcoded
// "Timelapse" literal shows up as a failure.
export const TEST_ROOT = 'Timelapse_test_root';
setTimelapseRootForTests(TEST_ROOT);

// Mock URL.createObjectURL and URL.revokeObjectURL
globalThis.URL.createObjectURL = vi.fn(() => 'blob:mock-url');
globalThis.URL.revokeObjectURL = vi.fn();

// Cleanup after each test case
afterEach(() => {
  cleanup();
});
