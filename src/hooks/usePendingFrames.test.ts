import { beforeEach, describe, expect, it, mock } from 'bun:test';
import { invoke } from '@tauri-apps/api/core';
import { renderHook, waitFor } from '@testing-library/react';

import { frameUrl, type Day, type PendingFrames } from '../frames';
import { mocked } from '../test/mocked';
import { pendingTrackBackground, usePendingFrames } from './usePendingFrames';

// invoke is replaced with a mock in src/test/setup.ts.

function day(date: string, source: Day['source'], frameCount = 300): Day {
  return { date, frameCount, source };
}

function pending(...ranges: Array<[number, number]>): PendingFrames {
  return { frameCount: 300, ranges: ranges.map(([start, end]) => ({ start, end })) };
}

beforeEach(() => {
  mock.clearAllMocks();
  mocked(invoke).mockReset();
});

describe('usePendingFrames', () => {
  it('asks again after a pending frame loads, and only then', async () => {
    mocked(invoke).mockResolvedValueOnce(pending([0, 150]));
    const video = day('2026-10-01', 'video');
    const { result, rerender } = renderHook(
      ({ loaded }: { loaded: string | null }) => usePendingFrames(video, loaded),
      { initialProps: { loaded: null } as { loaded: string | null } },
    );
    await waitFor(() => expect(result.current).toEqual(pending([0, 150])));
    expect(invoke).toHaveBeenCalledWith('get_pending_frames', { date: '2026-10-01' });

    // Frame 200 was already decoded: nothing to re-check.
    rerender({ loaded: frameUrl('2026-10-01', 200) });
    expect(invoke).toHaveBeenCalledTimes(1);

    mocked(invoke).mockResolvedValueOnce(pending());
    rerender({ loaded: frameUrl('2026-10-01', 20) });
    await waitFor(() => expect(result.current).toEqual(pending()));
    expect(invoke).toHaveBeenCalledTimes(2);
  });

  it('asks again when the day grows', async () => {
    mocked(invoke).mockResolvedValue(pending([0, 150]));
    const { rerender } = renderHook(({ d }: { d: Day }) => usePendingFrames(d, null), {
      initialProps: { d: day('2026-10-04', 'mixed', 300) },
    });
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(1));
    rerender({ d: day('2026-10-04', 'mixed', 301) });
    await waitFor(() => expect(invoke).toHaveBeenCalledTimes(2));
  });

  it('keeps an answer that lands after a newer request was sent', async () => {
    let answerFirst: (value: PendingFrames) => void = () => {};
    mocked(invoke).mockReturnValueOnce(
      new Promise((resolve) => {
        answerFirst = resolve;
      }),
    );
    mocked(invoke).mockReturnValueOnce(new Promise(() => {}));
    const { result, rerender } = renderHook(({ d }: { d: Day }) => usePendingFrames(d, null), {
      initialProps: { d: day('2026-10-04', 'mixed', 300) },
    });
    rerender({ d: day('2026-10-04', 'mixed', 301) });
    answerFirst(pending([0, 150]));
    await waitFor(() => expect(result.current).toEqual(pending([0, 150])));
  });

  it('never asks about screenshot days', () => {
    const { result } = renderHook(() => usePendingFrames(day('2026-10-04', 'screenshots'), null));
    expect(result.current).toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("does not show the previous day's answer for a new day", async () => {
    mocked(invoke).mockResolvedValueOnce(pending([0, 300]));
    const { result, rerender } = renderHook(({ d }: { d: Day }) => usePendingFrames(d, null), {
      initialProps: { d: day('2026-10-01', 'video') },
    });
    await waitFor(() => expect(result.current).not.toBeNull());

    mocked(invoke).mockReturnValueOnce(new Promise(() => {}));
    rerender({ d: day('2026-10-02', 'mixed') });
    expect(result.current).toBeNull();
  });
});

describe('pendingTrackBackground', () => {
  it('has nothing to draw when everything is ready', () => {
    expect(pendingTrackBackground(null)).toBeUndefined();
    expect(pendingTrackBackground({ frameCount: 100, ranges: [] })).toBeUndefined();
  });

  it('pales each pending stretch in proportion to the day it was measured against', () => {
    const background = pendingTrackBackground({
      frameCount: 200,
      ranges: [
        { start: 0, end: 50 },
        { start: 100, end: 200 },
      ],
    });
    expect(background).toBe(
      'linear-gradient(to right, ' +
        'transparent 0.000%, rgb(255, 255, 255) 0.000%, ' +
        'rgb(255, 255, 255) 25.000%, transparent 25.000%, ' +
        'transparent 50.000%, rgb(255, 255, 255) 50.000%, ' +
        'rgb(255, 255, 255) 100.000%, transparent 100.000%)',
    );
  });
});
