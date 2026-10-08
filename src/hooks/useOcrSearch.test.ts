import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { invoke } from '@tauri-apps/api/core';
import { watch } from '@tauri-apps/plugin-fs';
import { act, renderHook, waitFor } from '@testing-library/react';

import { mocked } from '../test/mocked';
import { TEST_ROOT } from '../test/setup';
import { useDayMatches, useMatchCounts, useMatchLines, useOcrVersion } from './useOcrSearch';

// invoke and watch are replaced with mocks in src/test/setup.ts.

const watchers = new Map<string, Array<() => void>>();

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

describe('useDayMatches', () => {
  it('asks nothing for a blank query', () => {
    const { result } = renderHook(() => useDayMatches('2026-10-04', '  ', '1'));
    expect(result.current.matches).toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it('waits for the OCR version, then searches again whenever it changes', async () => {
    const first = [{ index: 3, endIndex: 5, frame: 4 }];
    mocked(invoke).mockResolvedValueOnce(first);
    const { result, rerender } = renderHook(
      ({ version }) => useDayMatches('2026-10-04', 'cargo', version),
      { initialProps: { version: null as string | null } },
    );
    expect(invoke).not.toHaveBeenCalled();
    rerender({ version: '1' });
    expect(result.current.matches).toBeNull();
    await waitFor(() => expect(result.current.matches).toEqual(first));
    expect(invoke).toHaveBeenCalledWith('search_ocr_day', { date: '2026-10-04', query: 'cargo' });

    const second = [...first, { index: 9, endIndex: 10, frame: 10 }];
    mocked(invoke).mockResolvedValueOnce(second);
    rerender({ version: '2' });
    await waitFor(() => expect(result.current.matches).toEqual(second));

    // A failed repeat keeps what was found.
    mocked(invoke).mockRejectedValueOnce('database is locked');
    rerender({ version: '3' });
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(3));
    expect(result.current.matches).toEqual(second);
    expect(result.current.matchesError).toBeNull();
  });

  it('shows nothing from the previous query while a new one loads', async () => {
    mocked(invoke).mockResolvedValueOnce([{ index: 3, endIndex: 5, frame: 4 }]);
    const { result, rerender } = renderHook(({ query }) => useDayMatches('2026-10-04', query, '1'), {
      initialProps: { query: 'cargo' },
    });
    await waitFor(() => expect(result.current.matches).toHaveLength(1));

    mocked(invoke).mockReturnValueOnce(new Promise(() => {}));
    rerender({ query: 'cargo test' });
    expect(result.current.matches).toBeNull();
  });

  it('reports a failed search', async () => {
    mocked(invoke).mockRejectedValueOnce('database is locked');
    const { result } = renderHook(() => useDayMatches('2026-10-04', 'cargo', '1'));
    await waitFor(() => expect(result.current.matchesError?.message).toBe('database is locked'));
    expect(result.current.matches).toBeNull();
  });
});

describe('useMatchCounts', () => {
  it('counts matches per day', async () => {
    const counts = [{ day: '2026-10-04', count: 2 }];
    mocked(invoke).mockResolvedValueOnce(counts);
    const { result } = renderHook(() => useMatchCounts('cargo', '1'));
    await waitFor(() => expect(result.current).toEqual(counts));
    expect(invoke).toHaveBeenCalledWith('count_ocr_matches', { query: 'cargo' });
  });

  it('keeps the previous days while a changed query loads', async () => {
    const counts = [{ day: '2026-10-04', count: 2 }];
    mocked(invoke).mockResolvedValueOnce(counts);
    const { result, rerender } = renderHook(({ query }) => useMatchCounts(query, '1'), {
      initialProps: { query: 'cargo' },
    });
    await waitFor(() => expect(result.current).toEqual(counts));

    let finish: (value: unknown) => void = () => {};
    mocked(invoke).mockReturnValueOnce(new Promise((resolve) => (finish = resolve)));
    rerender({ query: 'cargo t' });
    expect(result.current).toEqual(counts);

    // A search that finds nothing hides them.
    await act(async () => finish([]));
    expect(result.current).toEqual([]);

    // A blank query hides them at once.
    mocked(invoke).mockResolvedValueOnce(counts);
    rerender({ query: 'cargo' });
    await waitFor(() => expect(result.current).toEqual(counts));
    rerender({ query: '' });
    expect(result.current).toEqual([]);
  });
});

describe('useOcrVersion', () => {
  it('asks again when the library root changes', async () => {
    mocked(invoke).mockResolvedValueOnce('1:1:1');
    const { result } = renderHook(() => useOcrVersion());
    await waitFor(() => expect(result.current).toBe('1:1:1'));
    expect(invoke).toHaveBeenCalledWith('get_ocr_version');

    mocked(invoke).mockResolvedValueOnce('2:2:2');
    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    act(() => watchers.get(TEST_ROOT)?.forEach((callback) => callback()));
    await waitFor(() => expect(result.current).toBe('2:2:2'));
  });
});

describe('useMatchLines', () => {
  it('fetches the lines of the frame a match was read from', async () => {
    const lines = [{ x: 0, y: 0, width: 1, height: 0.1 }];
    mocked(invoke).mockResolvedValueOnce(lines);
    const { result } = renderHook(() => useMatchLines('2026-10-04', 12, 'cargo'));
    await waitFor(() => expect(result.current).toEqual(lines));
    expect(invoke).toHaveBeenCalledWith('get_match_lines', { date: '2026-10-04', frame: 12, query: 'cargo' });
  });

  it('asks nothing without a frame', () => {
    const { result } = renderHook(() => useMatchLines('2026-10-04', null, 'cargo'));
    expect(result.current).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });
});
