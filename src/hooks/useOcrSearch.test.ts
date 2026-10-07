import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { invoke } from '@tauri-apps/api/core';
import { watch } from '@tauri-apps/plugin-fs';
import { act, renderHook, waitFor } from '@testing-library/react';

import { mocked } from '../test/mocked';
import { TEST_ROOT } from '../test/setup';
import { useDayMatches, useMatchCounts } from './useOcrSearch';

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
    const { result } = renderHook(() => useDayMatches('2026-10-04', '  '));
    expect(result.current.matches).toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it('searches the day and searches again when OCR writes to the library', async () => {
    const first = [{ index: 3, endIndex: 5, lines: [] }];
    mocked(invoke).mockResolvedValueOnce(first);
    const { result } = renderHook(() => useDayMatches('2026-10-04', 'cargo'));
    expect(result.current.matches).toBeNull();
    await waitFor(() => expect(result.current.matches).toEqual(first));
    expect(invoke).toHaveBeenCalledWith('search_ocr_day', { date: '2026-10-04', query: 'cargo' });

    const second = [...first, { index: 9, endIndex: 10, lines: [] }];
    mocked(invoke).mockResolvedValueOnce(second);
    await waitFor(() => expect(watchers.has(TEST_ROOT)).toBe(true));
    act(() => watchers.get(TEST_ROOT)?.forEach((callback) => callback()));
    await waitFor(() => expect(result.current.matches).toEqual(second));
  });

  it('shows nothing from the previous query while a new one loads', async () => {
    mocked(invoke).mockResolvedValueOnce([{ index: 3, endIndex: 5, lines: [] }]);
    const { result, rerender } = renderHook(({ query }) => useDayMatches('2026-10-04', query), {
      initialProps: { query: 'cargo' },
    });
    await waitFor(() => expect(result.current.matches).toHaveLength(1));

    mocked(invoke).mockReturnValueOnce(new Promise(() => {}));
    rerender({ query: 'cargo test' });
    expect(result.current.matches).toBeNull();
  });

  it('reports a failed search', async () => {
    mocked(invoke).mockRejectedValueOnce('database is locked');
    const { result } = renderHook(() => useDayMatches('2026-10-04', 'cargo'));
    await waitFor(() => expect(result.current.matchesError?.message).toBe('database is locked'));
    expect(result.current.matches).toBeNull();
  });
});

describe('useMatchCounts', () => {
  it('counts matches per day', async () => {
    const counts = [{ day: '2026-10-04', count: 2 }];
    mocked(invoke).mockResolvedValueOnce(counts);
    const { result } = renderHook(() => useMatchCounts('cargo'));
    await waitFor(() => expect(result.current).toEqual(counts));
    expect(invoke).toHaveBeenCalledWith('count_ocr_matches', { query: 'cargo' });
  });
});
