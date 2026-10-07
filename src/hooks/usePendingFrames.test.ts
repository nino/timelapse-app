import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { invoke } from '@tauri-apps/api/core';
import { renderHook, waitFor } from '@testing-library/react';

import type { Day } from '../frames';
import { mocked } from '../test/mocked';
import { pendingTrackBackground, usePendingFrames } from './usePendingFrames';

// invoke is replaced with a mock in src/test/setup.ts.

function day(date: string, source: Day['source']): Day {
  return { date, frameCount: 300, source };
}

beforeEach(() => {
  mock.clearAllMocks();
  mocked(invoke).mockReset();
});

describe('usePendingFrames', () => {
  it('asks about video days and asks again after each load', async () => {
    mocked(invoke).mockResolvedValueOnce([{ start: 0, end: 300 }]);
    const video = day('2026-10-01', 'video');
    const { result, rerender } = renderHook(
      ({ loads }: { loads: number }) => usePendingFrames(video, loads),
      { initialProps: { loads: 0 } },
    );
    await waitFor(() => expect(result.current).toEqual([{ start: 0, end: 300 }]));
    expect(invoke).toHaveBeenCalledWith('get_pending_frames', { date: '2026-10-01' });

    mocked(invoke).mockResolvedValueOnce([{ start: 150, end: 300 }]);
    rerender({ loads: 1 });
    await waitFor(() => expect(result.current).toEqual([{ start: 150, end: 300 }]));
  });

  it('never asks about screenshot days', () => {
    const { result } = renderHook(() => usePendingFrames(day('2026-10-04', 'screenshots'), 0));
    expect(result.current).toEqual([]);
    expect(invoke).not.toHaveBeenCalled();
  });

  it("does not show the previous day's answer for a new day", async () => {
    mocked(invoke).mockResolvedValueOnce([{ start: 0, end: 300 }]);
    const { result, rerender } = renderHook(({ d }: { d: Day }) => usePendingFrames(d, 0), {
      initialProps: { d: day('2026-10-01', 'video') },
    });
    await waitFor(() => expect(result.current).toHaveLength(1));

    mocked(invoke).mockReturnValueOnce(new Promise(() => {}));
    rerender({ d: day('2026-10-02', 'mixed') });
    expect(result.current).toEqual([]);
  });
});

describe('pendingTrackBackground', () => {
  it('has nothing to draw when everything is ready', () => {
    expect(pendingTrackBackground([], 100)).toBeUndefined();
  });

  it('pales each pending stretch in proportion to the day', () => {
    const background = pendingTrackBackground(
      [
        { start: 0, end: 25 },
        { start: 50, end: 100 },
      ],
      100,
    );
    expect(background).toBe(
      'linear-gradient(to right, ' +
        'transparent 0.000%, rgba(255, 255, 255, 0.75) 0.000%, ' +
        'rgba(255, 255, 255, 0.75) 25.000%, transparent 25.000%, ' +
        'transparent 50.000%, rgba(255, 255, 255, 0.75) 50.000%, ' +
        'rgba(255, 255, 255, 0.75) 100.000%, transparent 100.000%)',
    );
  });
});
