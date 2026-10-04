import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, renderHook, waitFor } from '@testing-library/react';

import { TEST_ROOT } from '../test/setup';
import { useDay, useDays } from './useLibrary';
import type { Day } from '../frames';

vi.mock('@tauri-apps/api/core', () => ({
  invoke: vi.fn(),
}));

vi.mock('@tauri-apps/plugin-fs', () => ({
  watch: vi.fn(),
}));

const { invoke } = await import('@tauri-apps/api/core');
const { watch } = await import('@tauri-apps/plugin-fs');

// Every path the hooks watch, so a test can play the filesystem.
const watchers = new Map<string, Array<() => void>>();

function fireChange(path: string): void {
  const callbacks = watchers.get(path);
  if (!callbacks?.length) throw new Error(`Nothing is watching ${path}`);
  act(() => callbacks.forEach((callback) => callback()));
}

function day(date: string, frameCount: number): Day {
  return { date, frameCount, source: 'screenshots' };
}

beforeEach(() => {
  vi.clearAllMocks();
  watchers.clear();
  vi.mocked(watch).mockImplementation(async (path, callback) => {
    const key = String(path);
    watchers.set(key, [
      ...(watchers.get(key) ?? []),
      (): void => callback({ type: 'any', paths: [key], attrs: null }),
    ]);
    return (): void => {};
  });
});

describe('useDays', () => {
  it('lists days and picks up new ones when the library root changes', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(['2024-12-20']);
    const { result } = renderHook(() => useDays());
    await waitFor(() => expect(result.current.days).toEqual(['2024-12-20']));
    expect(invoke).toHaveBeenCalledWith('list_days');

    vi.mocked(invoke).mockResolvedValueOnce(['2024-12-20', '2026-10-04']);
    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    fireChange(TEST_ROOT);

    await waitFor(() =>
      expect(result.current.days).toEqual(['2024-12-20', '2026-10-04']),
    );
  });

  it('keeps the same array when nothing changed', async () => {
    vi.mocked(invoke).mockResolvedValue(['2024-12-20']);
    const { result } = renderHook(() => useDays());
    await waitFor(() => expect(result.current.days).toEqual(['2024-12-20']));
    const first = result.current.days;

    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    fireChange(TEST_ROOT);
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));

    expect(result.current.days).toBe(first);
  });

  it('reports errors and clears them on a successful reload', async () => {
    vi.mocked(invoke).mockRejectedValueOnce('library missing');
    const { result } = renderHook(() => useDays());
    await waitFor(() => expect(result.current.daysError?.message).toBe('library missing'));

    vi.mocked(invoke).mockResolvedValueOnce(['2024-12-20']);
    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    fireChange(TEST_ROOT);
    await waitFor(() => expect(result.current.daysError).toBeNull());
  });
});

describe('useDay', () => {
  it('loads the day and follows new captures in its folder', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(day('2026-10-04', 10));
    const { result } = renderHook(() => useDay('2026-10-04'));
    await waitFor(() => expect(result.current.day?.frameCount).toBe(10));
    expect(invoke).toHaveBeenCalledWith('get_day', { date: '2026-10-04' });

    vi.mocked(invoke).mockResolvedValueOnce(day('2026-10-04', 11));
    const folder = `${TEST_ROOT}/2026-10-04`;
    await waitFor(() => expect(watchers.has(folder)).toBe(true));
    fireChange(folder);

    await waitFor(() => expect(result.current.day?.frameCount).toBe(11));
  });

  it('returns nothing for no date', () => {
    const { result } = renderHook(() => useDay(null));
    expect(result.current).toEqual({ day: null, dayError: null });
    expect(invoke).not.toHaveBeenCalled();
  });

  it('never shows the previous day while a new one loads', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(day('2026-10-03', 5));
    let resolveNew: (value: Day) => void = () => {};
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveNew = resolve;
      }),
    );

    const { result, rerender } = renderHook(({ date }: { date: string }) => useDay(date), {
      initialProps: { date: '2026-10-03' },
    });
    await waitFor(() => expect(result.current.day?.frameCount).toBe(5));

    rerender({ date: '2026-10-04' });
    expect(result.current.day).toBeNull();

    await act(async () => resolveNew(day('2026-10-04', 7)));
    expect(result.current.day?.frameCount).toBe(7);
  });

  it('ignores a slow answer for a day that is no longer selected', async () => {
    let resolveOld: (value: Day) => void = () => {};
    vi.mocked(invoke).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      }),
    );
    vi.mocked(invoke).mockResolvedValueOnce(day('2026-10-04', 7));

    const { result, rerender } = renderHook(({ date }: { date: string }) => useDay(date), {
      initialProps: { date: '2026-10-03' },
    });
    rerender({ date: '2026-10-04' });
    await waitFor(() => expect(result.current.day?.frameCount).toBe(7));

    await act(async () => resolveOld(day('2026-10-03', 5)));
    expect(result.current.day?.date).toBe('2026-10-04');
  });
});
