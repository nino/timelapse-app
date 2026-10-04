import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, renderHook } from '@testing-library/react';
import { BaseDirectory } from '@tauri-apps/api/path';

import { useDirectoryChanges, useLatestLoad } from './useDirectoryChanges';

vi.mock('@tauri-apps/plugin-fs', () => ({
  watch: vi.fn(),
}));

const { watch } = await import('@tauri-apps/plugin-fs');

describe('useDirectoryChanges', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('calls back when the watcher reports a change', async () => {
    let fire: () => void = () => {};
    vi.mocked(watch).mockImplementation(async (_path, callback) => {
      fire = (): void => callback({ type: 'any', paths: [], attrs: null });
      return (): void => {};
    });
    const onChange = vi.fn();

    renderHook(() => useDirectoryChanges('Timelapse/2026-10-04', onChange));
    await act(async () => {});
    fire();

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(watch).toHaveBeenCalledWith('Timelapse/2026-10-04', expect.any(Function), {
      baseDir: BaseDirectory.Home,
      delayMs: 500,
    });
  });

  it('does not watch when there is no path', () => {
    renderHook(() => useDirectoryChanges(null, vi.fn()));
    expect(watch).not.toHaveBeenCalled();
  });

  it('stops watching on unmount, even if the watcher arrives late', async () => {
    const unwatch = vi.fn();
    let resolve: (stop: () => void) => void = () => {};
    vi.mocked(watch).mockReturnValue(
      new Promise((r) => {
        resolve = r;
      }),
    );

    const { unmount } = renderHook(() => useDirectoryChanges('Timelapse', vi.fn()));
    unmount();
    await act(async () => resolve(unwatch));

    expect(unwatch).toHaveBeenCalled();
  });

  it('falls back to polling when the watcher cannot be created', async () => {
    vi.useFakeTimers();
    vi.spyOn(console, 'warn').mockImplementation(() => {});
    vi.mocked(watch).mockRejectedValue(new Error('fs.watch not allowed'));
    const onChange = vi.fn();

    renderHook(() => useDirectoryChanges('Timelapse', onChange));
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(onChange).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(3000);
    });
    expect(onChange).toHaveBeenCalledTimes(1);
  });
});

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
});
