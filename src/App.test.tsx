import { afterAll, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';

import { App } from './App';
import * as frames from './frames';
import { frameUrl, type Day } from './frames';
import * as library from './hooks/useLibrary';

// Spied on rather than replaced with `mock.module`: Bun runs every test file in
// one process, and a module mock would also replace the real hooks and
// helpers that useLibrary.test.ts and frames.test.ts are testing.
const useDays = spyOn(library, 'useDays');
const useDay = spyOn(library, 'useDay');
const getFrameTime = spyOn(frames, 'getFrameTime');

afterAll(() => {
  useDays.mockRestore();
  useDay.mockRestore();
  getFrameTime.mockRestore();
});

/** Pretend the library holds `counts[date]` frames for each day. */
function mockLibrary(counts: Record<string, number>, source: Day['source'] = 'screenshots'): void {
  useDays.mockReturnValue({ days: Object.keys(counts).sort(), daysError: null });
  useDay.mockImplementation((date: string | null) => ({
    day: date === null ? null : { date, frameCount: counts[date] ?? 0, source },
    dayError: null,
  }));
}

function image(): HTMLImageElement {
  return screen.getByRole('img') as HTMLImageElement;
}

/** The frame currently requested by the `<img>`, as [day, index]. */
function shownFrame(): string {
  return image().getAttribute('src') ?? '';
}

/** Let the in-flight frame finish loading, as the browser would. */
function finishLoading(): void {
  fireEvent.load(image());
}

function today(): string {
  const now = new Date();
  const pad = (n: number): string => String(n).padStart(2, '0');
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

describe('App', () => {
  beforeEach(() => {
    mock.clearAllMocks();
    getFrameTime.mockResolvedValue(null);
  });

  describe('Library states', () => {
    it('shows a library error', () => {
      useDays.mockReturnValue({ days: [], daysError: new Error('no access') });
      useDay.mockReturnValue({ day: null, dayError: null });
      render(<App />);
      expect(screen.getByText(/Could not load the timelapse library: no access/)).toBeInTheDocument();
    });

    it('shows a day error', () => {
      useDays.mockReturnValue({ days: ['2026-10-04'], daysError: null });
      useDay.mockReturnValue({ day: null, dayError: new Error('ffprobe missing') });
      render(<App />);
      expect(screen.getByText(/ffprobe missing/)).toBeInTheDocument();
    });

    it('says when there is nothing yet', () => {
      mockLibrary({});
      render(<App />);
      expect(screen.getByText('No screenshots yet')).toBeInTheDocument();
      expect(screen.getByRole('slider')).toBeDisabled();
    });

    it('says when a day has no frames', async () => {
      mockLibrary({ '2024-12-24': 0 }, 'empty');
      render(<App />);
      await waitFor(() => {
        expect(screen.getByText('Nothing was captured on this day')).toBeInTheDocument();
      });
    });
  });

  describe('One view for every day', () => {
    it('has no Images/Videos tabs and no refresh button', () => {
      mockLibrary({ '2024-12-20': 10, '2026-10-04': 3 });
      render(<App />);
      expect(screen.queryByRole('button')).not.toBeInTheDocument();
      expect(screen.queryByText(/Refresh/)).not.toBeInTheDocument();
    });

    it('opens the newest day on its last frame', async () => {
      mockLibrary({ '2024-12-20': 10, '2026-10-04': 3 });
      render(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 2)));
      expect(screen.getByLabelText('Day')).toHaveValue('2026-10-04');
      expect(screen.getByText('Frame 3 / 3')).toBeInTheDocument();
    });

    it('lists days newest first and marks today', () => {
      mockLibrary({ '2024-12-20': 10, [today()]: 3 });
      render(<App />);
      const options = screen.getAllByRole('option').map((o) => o.textContent?.trim());
      expect(options).toEqual([`${today()} (Today)`, '2024-12-20']);
    });

    it('serves a video day exactly like a screenshot day', async () => {
      mockLibrary({ '2024-12-20': 7059 }, 'video');
      render(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2024-12-20', 7058)));
    });

    it('switches days from the picker', async () => {
      mockLibrary({ '2024-12-20': 10, '2026-10-04': 3 });
      render(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 2)));
      finishLoading();

      fireEvent.change(screen.getByLabelText('Day'), { target: { value: '2024-12-20' } });
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2024-12-20', 9)));
    });
  });

  describe('Scrubbing', () => {
    it('moves with the slider', async () => {
      mockLibrary({ '2026-10-04': 100 });
      render(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 99)));
      finishLoading();

      fireEvent.change(screen.getByRole('slider'), { target: { value: '5' } });
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 5)));
      expect(screen.getByText('Frame 6 / 100')).toBeInTheDocument();
    });

    it('waits for the current frame before requesting the next, then skips to the latest', async () => {
      mockLibrary({ '2026-10-04': 100 });
      render(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 99)));

      const slider = screen.getByRole('slider');
      fireEvent.change(slider, { target: { value: '10' } });
      fireEvent.change(slider, { target: { value: '20' } });
      fireEvent.change(slider, { target: { value: '30' } });
      // Still loading frame 99, so nothing new has been requested.
      expect(shownFrame()).toBe(frameUrl('2026-10-04', 99));

      finishLoading();
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 30)));
    });

    it('steps with the arrow keys: 1, Shift 10, Option 100', async () => {
      mockLibrary({ '2026-10-04': 500 });
      render(<App />);
      await waitFor(() => expect(screen.getByText('Frame 500 / 500')).toBeInTheDocument());

      fireEvent.keyDown(window, { key: 'ArrowLeft' });
      expect(screen.getByText('Frame 499 / 500')).toBeInTheDocument();
      fireEvent.keyDown(window, { key: 'ArrowLeft', shiftKey: true });
      expect(screen.getByText('Frame 489 / 500')).toBeInTheDocument();
      fireEvent.keyDown(window, { key: 'ArrowLeft', altKey: true });
      expect(screen.getByText('Frame 389 / 500')).toBeInTheDocument();
      fireEvent.keyDown(window, { key: 'ArrowRight', altKey: true });
      fireEvent.keyDown(window, { key: 'ArrowRight', altKey: true });
      expect(screen.getByText('Frame 500 / 500')).toBeInTheDocument();
    });

    it('keeps Shift and Option steps when the slider has focus', async () => {
      mockLibrary({ '2026-10-04': 500 });
      render(<App />);
      await waitFor(() => expect(screen.getByText('Frame 500 / 500')).toBeInTheDocument());

      const slider = screen.getByRole('slider');
      const event = fireEvent.keyDown(slider, { key: 'ArrowLeft', shiftKey: true });
      expect(event).toBe(false); // default prevented: no extra native step
      expect(screen.getByText('Frame 490 / 500')).toBeInTheDocument();
    });

    it('says when a frame cannot be loaded', async () => {
      mockLibrary({ '2026-10-04': 3 });
      render(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 2)));
      fireEvent.error(image());
      expect(screen.getByText('Could not load this frame')).toBeInTheDocument();
    });
  });

  describe('Frame times', () => {
    it('shows the capture time', async () => {
      getFrameTime.mockResolvedValue({
        localTime: '2026-10-04T14:05:09+01:00',
        exact: true,
      });
      mockLibrary({ '2026-10-04': 3 });
      render(<App />);
      const expected = new Date('2026-10-04T14:05:09+01:00');
      const hhmm = `${String(expected.getHours()).padStart(2, '0')}:${String(expected.getMinutes()).padStart(2, '0')}`;
      await waitFor(() => expect(screen.getByText(hhmm)).toBeInTheDocument());
      expect(getFrameTime).toHaveBeenLastCalledWith('2026-10-04', 2);
    });

    it('marks estimated times', async () => {
      getFrameTime.mockResolvedValue({ localTime: '2024-12-20T12:48:38', exact: false });
      mockLibrary({ '2024-12-20': 3 }, 'video');
      render(<App />);
      await waitFor(() => expect(screen.getByText('~12:48')).toBeInTheDocument());
    });
  });

  describe('Live updates', () => {
    it('follows new captures while on the newest frame', async () => {
      mockLibrary({ '2026-10-04': 3 });
      const { rerender } = render(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 2)));
      finishLoading();

      mockLibrary({ '2026-10-04': 4 });
      rerender(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 3)));
      expect(screen.getByText('Frame 4 / 4')).toBeInTheDocument();
    });

    it('stays on a scrubbed-back frame when new captures arrive', async () => {
      mockLibrary({ '2026-10-04': 10 });
      const { rerender } = render(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 9)));
      finishLoading();
      fireEvent.change(screen.getByRole('slider'), { target: { value: '2' } });
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 2)));
      finishLoading();

      mockLibrary({ '2026-10-04': 11 });
      rerender(<App />);
      await waitFor(() => expect(screen.getByText('Frame 3 / 11')).toBeInTheDocument());
      expect(shownFrame()).toBe(frameUrl('2026-10-04', 2));
    });

    it('moves to the new day at midnight when following the live edge', async () => {
      mockLibrary({ '2026-10-04': 3 });
      const { rerender } = render(<App />);
      await waitFor(() => expect(screen.getByLabelText('Day')).toHaveValue('2026-10-04'));

      mockLibrary({ '2026-10-04': 3, '2026-10-05': 1 });
      rerender(<App />);
      await waitFor(() => expect(screen.getByLabelText('Day')).toHaveValue('2026-10-05'));
    });

    it('stays on an older day when a new day appears', async () => {
      mockLibrary({ '2026-10-03': 3, '2026-10-04': 3 });
      const { rerender } = render(<App />);
      await waitFor(() => expect(screen.getByLabelText('Day')).toHaveValue('2026-10-04'));
      fireEvent.change(screen.getByLabelText('Day'), { target: { value: '2026-10-03' } });

      mockLibrary({ '2026-10-03': 3, '2026-10-04': 3, '2026-10-05': 1 });
      await act(async () => rerender(<App />));
      expect(screen.getByRole('option', { name: /2026-10-05/ })).toBeInTheDocument();
      expect(screen.getByLabelText('Day')).toHaveValue('2026-10-03');
    });
  });
});
