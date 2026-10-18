import { beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { invoke } from '@tauri-apps/api/core';
import { watch } from '@tauri-apps/plugin-fs';
import { act, renderHook, waitFor } from '@testing-library/react';

import type { Day } from '../frames';
import { mocked } from '../test/mocked';
import { TEST_ROOT } from '../test/setup';
import { useDay, useDayStatuses, useDays } from './useLibrary';

// invoke and watch are replaced with mocks in src/test/setup.ts.

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
  mock.clearAllMocks();
  mocked(invoke).mockReset();
  watchers.clear();
  mocked(watch).mockImplementation(async (path, callback) => {
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
    mocked(invoke).mockResolvedValueOnce(['2024-12-20']);
    const { result } = renderHook(() => useDays());
    await waitFor(() => expect(result.current.days).toEqual(['2024-12-20']));
    expect(invoke).toHaveBeenCalledWith('list_days');

    mocked(invoke).mockResolvedValueOnce(['2024-12-20', '2026-10-04']);
    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    fireChange(TEST_ROOT);

    await waitFor(() =>
      expect(result.current.days).toEqual(['2024-12-20', '2026-10-04']),
    );
  });

  it('keeps the same array when nothing changed', async () => {
    mocked(invoke).mockResolvedValue(['2024-12-20']);
    const { result } = renderHook(() => useDays());
    await waitFor(() => expect(result.current.days).toEqual(['2024-12-20']));
    const first = result.current.days;

    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    fireChange(TEST_ROOT);
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));

    expect(result.current.days).toBe(first);
  });

  it('reports errors and clears them on a successful reload', async () => {
    mocked(invoke).mockRejectedValueOnce('library missing');
    const { result } = renderHook(() => useDays());
    await waitFor(() => expect(result.current.daysError?.message).toBe('library missing'));

    mocked(invoke).mockResolvedValueOnce(['2024-12-20']);
    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    fireChange(TEST_ROOT);
    await waitFor(() => expect(result.current.daysError).toBeNull());
  });
});

describe('useDay', () => {
  it('loads the day and follows new captures in its folder', async () => {
    mocked(invoke).mockResolvedValueOnce(day('2026-10-04', 10));
    const { result } = renderHook(() => useDay('2026-10-04'));
    await waitFor(() => expect(result.current.day?.frameCount).toBe(10));
    expect(invoke).toHaveBeenCalledWith('get_day', { date: '2026-10-04' });

    mocked(invoke).mockResolvedValueOnce(day('2026-10-04', 11));
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
    mocked(invoke).mockResolvedValueOnce(day('2026-10-03', 5));
    let resolveNew: (value: Day) => void = () => {};
    mocked(invoke).mockReturnValueOnce(
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
    mocked(invoke).mockReturnValueOnce(
      new Promise((resolve) => {
        resolveOld = resolve;
      }),
    );
    mocked(invoke).mockResolvedValueOnce(day('2026-10-04', 7));

    const { result, rerender } = renderHook(({ date }: { date: string }) => useDay(date), {
      initialProps: { date: '2026-10-03' },
    });
    rerender({ date: '2026-10-04' });
    await waitFor(() => expect(result.current.day?.frameCount).toBe(7));

    await act(async () => resolveOld(day('2026-10-03', 5)));
    expect(result.current.day?.date).toBe('2026-10-04');
  });
});

describe('useDayStatuses', () => {
  it('asks each time the picker opens and keeps the last answer while closed', async () => {
    mocked(invoke).mockResolvedValueOnce([{ date: '2026-10-04', ocr: 'todo', video: 'todo' }]);
    const { result, rerender } = renderHook(({ open }) => useDayStatuses(open), {
      initialProps: { open: false },
    });
    expect(invoke).not.toHaveBeenCalled();

    rerender({ open: true });
    await waitFor(() => expect(result.current.get('2026-10-04')?.ocr).toBe('todo'));
    expect(invoke).toHaveBeenCalledWith('get_day_statuses');

    mocked(invoke).mockResolvedValueOnce([{ date: '2026-10-04', ocr: 'done', video: 'todo' }]);
    rerender({ open: false });
    expect(result.current.get('2026-10-04')?.ocr).toBe('todo');
    rerender({ open: true });
    await waitFor(() => expect(result.current.get('2026-10-04')?.ocr).toBe('done'));
  });

  it('asks again every few seconds while open, keeping the map when nothing changed', async () => {
    const intervals = spyOn(globalThis, 'setInterval');
    mocked(invoke).mockResolvedValue([{ date: '2026-10-04', ocr: 'todo', video: 'todo' }]);
    const { result } = renderHook(() => useDayStatuses(true));
    await waitFor(() => expect(result.current.get('2026-10-04')?.video).toBe('todo'));
    const first = result.current;
    const [[tick, ms]] = intervals.mock.calls;
    expect(ms).toBe(5000);

    await act(async () => (tick as () => void)());
    expect(result.current).toBe(first);

    mocked(invoke).mockResolvedValue([{ date: '2026-10-04', ocr: 'done', video: 'done' }]);
    await act(async () => (tick as () => void)());
    await waitFor(() => expect(result.current.get('2026-10-04')?.video).toBe('done'));
    intervals.mockRestore();
  });
});
