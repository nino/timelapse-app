import { afterEach, expect, mock } from 'bun:test';
import * as matchers from '@testing-library/jest-dom/matchers';
import { cleanup } from '@testing-library/react';

// Extend Bun's expect with jest-dom matchers (toBeInTheDocument etc.)
expect.extend(matchers);

// Bun runs every test file in one process, and `mock.module` replaces a module
// for the whole process, not just the file that called it. So the Tauri modules
// are mocked once here, with every export any test needs, instead of per file
// with differing shapes. Tests set behaviour per case via the shared mocks.
mock.module('@tauri-apps/plugin-fs', () => ({
  readDir: mock(),
  readFile: mock(),
  watch: mock(),
}));

mock.module('@tauri-apps/api/core', () => ({
  invoke: mock(),
}));

mock.module('@tauri-apps/api/event', () => ({
  listen: mock(() => Promise.resolve((): void => {})),
}));

// Production resolves this from Rust at startup; tests pin it to a value that
// is deliberately NOT the production name, so any path built from a hardcoded
// "Timelapse" literal shows up as a failure.
// Imported only after the mocks above are installed: timelapseRoot.ts imports
// `invoke` statically, and a static import here would be hoisted above them.
export const TEST_ROOT = 'Timelapse_test_root';
const { setTimelapseRootForTests } = await import('../timelapseRoot');
setTimelapseRootForTests(TEST_ROOT);

// Mock URL.createObjectURL and URL.revokeObjectURL
globalThis.URL.createObjectURL = mock(() => 'blob:mock-url');
globalThis.URL.revokeObjectURL = mock();

// Cleanup after each test case
afterEach(() => {
  cleanup();
});
