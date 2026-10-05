// Types for the jest-dom matchers that src/test/setup.ts adds to Bun's expect.
// jest-dom ships this declaration as types/bun.d.ts but does not export it.
import type { expect } from 'bun:test';
import type { TestingLibraryMatchers } from '@testing-library/jest-dom/matchers';

declare module 'bun:test' {
  interface Matchers<T>
    extends TestingLibraryMatchers<ReturnType<typeof expect.stringContaining>, T> {}
}
