import { describe, expect, it } from 'bun:test';
import { act, renderHook } from '@testing-library/react';

import { useLatestLoad } from './useLatestLoad';

describe('useLatestLoad', () => {
  it('only lets the most recent call publish', async () => {
    const resolvers: Array<() => void> = [];
    const published: Array<number> = [];
    let calls = 0;
    const load = async (isCurrent: () => boolean): Promise<void> => {
      const mine = ++calls;
      await new Promise<void>((resolve) => resolvers.push(resolve));
      if (isCurrent()) published.push(mine);
    };

    const { result } = renderHook(() => useLatestLoad(load));
    act(() => result.current());

    // The second call finishes first; the first must then be ignored.
    await act(async () => resolvers[1]());
    await act(async () => resolvers[0]());

    expect(published).toEqual([2]);
  });

  it('with oneAtATime, runs only the latest of the calls made while one is running', async () => {
    const resolvers: Array<() => void> = [];
    const started: Array<number> = [];
    const published: Array<number> = [];
    let calls = 0;
    const load = async (isCurrent: () => boolean): Promise<void> => {
      const mine = ++calls;
      started.push(mine);
      await new Promise<void>((resolve) => resolvers.push(resolve));
      if (isCurrent()) published.push(mine);
    };

    const { result } = renderHook(() => useLatestLoad(load, { oneAtATime: true }));
    act(() => result.current());
    act(() => result.current());
    act(() => result.current());

    // Only the first is running; the two calls that waited collapse into one.
    expect(started).toEqual([1]);
    await act(async () => resolvers[0]());
    expect(started).toEqual([1, 2]);
    await act(async () => resolvers[1]());

    expect(published).toEqual([2]);
    expect(started).toEqual([1, 2]);
  });
});
