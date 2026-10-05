import type { Mock } from 'bun:test';

// Bun has no `vi.mocked`; this is the same type-only cast. Use it on a function
// that a test has replaced with `mock()`, to reach `mockResolvedValue` and co.
export function mocked<T extends (...args: Array<never>) => unknown>(fn: T): Mock<T> {
  return fn as unknown as Mock<T>;
}
