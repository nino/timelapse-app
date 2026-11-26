import { afterEach, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { BaseDirectory } from '@tauri-apps/api/path';
import { watch } from '@tauri-apps/plugin-fs';
import { act, renderHook } from '@testing-library/react';

import { mocked } from '../test/mocked';
import { useDirectoryChanges } from './useDirectoryChanges';

// watch is replaced with a mock in src/test/setup.ts.

describe('useDirectoryChanges', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    mocked(watch).mockReset();
  });

  const spies: Array<{ mockRestore: () => void }> = [];
  afterEach(() => {
    spies.splice(0).forEach((spy) => spy.mockRestore());
  });

  it('calls back when the watcher reports a change', async () => {
    let fire: () => void = () => {};
    mocked(watch).mockImplementation(async (_path, callback) => {
      fire = (): void => callback({ type: 'any', paths: [], attrs: null });
      return (): void => {};
    });
    const onChange = mock();

    renderHook(() => useDirectoryChanges('Timelapse/2026-10-04', onChange));
    await act(async () => {});
    fire();

    expect(onChange).toHaveBeenCalledTimes(1);
    expect(watch).toHaveBeenCalledWith('Timelapse/2026-10-04', expect.any(Function), {
      baseDir: BaseDirectory.Home,
      delayMs: 500,
    });
  });

  it('shares one watcher among the hooks watching the same path', async () => {
    let fire: () => void = () => {};
    const unwatch = mock();
    mocked(watch).mockImplementation(async (_path, callback) => {
      fire = (): void => callback({ type: 'any', paths: [], attrs: null });
      return unwatch;
    });
    const first = mock();
    const second = mock();

    const a = renderHook(() => useDirectoryChanges('Timelapse', first));
    const b = renderHook(() => useDirectoryChanges('Timelapse', second));
    await act(async () => {});
    expect(watch).toHaveBeenCalledTimes(1);

    fire();
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(1);

    // The watcher stays while anyone is listening, and stops after the last.
    a.unmount();
    fire();
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).toHaveBeenCalledTimes(2);
    expect(unwatch).not.toHaveBeenCalled();
    b.unmount();
    expect(unwatch).toHaveBeenCalledTimes(1);

    // A later hook starts a watcher of its own.
    renderHook(() => useDirectoryChanges('Timelapse', mock()));
    await act(async () => {});
    expect(watch).toHaveBeenCalledTimes(2);
  });

  it('does not watch when there is no path', () => {
    renderHook(() => useDirectoryChanges(null, mock()));
    expect(watch).not.toHaveBeenCalled();
  });

  it('stops watching on unmount, even if the watcher arrives late', async () => {
    const unwatch = mock();
    let resolve: (stop: () => void) => void = () => {};
    mocked(watch).mockReturnValue(
      new Promise((r) => {
        resolve = r;
      }),
    );

    const { unmount } = renderHook(() => useDirectoryChanges('Timelapse', mock()));
    unmount();
    await act(async () => resolve(unwatch));

    expect(unwatch).toHaveBeenCalled();
  });

  it('falls back to polling when the watcher cannot be created', async () => {
    // Bun has no fake timers here, so catch the interval instead of waiting 3s.
    let poll: () => void = () => {};
    const setInterval = spyOn(globalThis, 'setInterval').mockImplementation(((
      callback: () => void,
    ): number => {
      poll = callback;
      return 1;
    }) as unknown as typeof globalThis.setInterval);
    spies.push(setInterval, spyOn(console, 'warn').mockImplementation(() => {}));
    mocked(watch).mockRejectedValue(new Error('fs.watch not allowed'));
    const onChange = mock();

    renderHook(() => useDirectoryChanges('Timelapse', onChange));
    await act(async () => {});
    expect(setInterval).toHaveBeenCalledWith(expect.any(Function), 3000);
    expect(onChange).not.toHaveBeenCalled();

    act(() => poll());
    expect(onChange).toHaveBeenCalledTimes(1);
  });
});
