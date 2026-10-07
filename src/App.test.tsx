import { afterAll, beforeEach, describe, expect, it, mock, spyOn } from 'bun:test';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';

import { App } from './App';
import * as frames from './frames';
import { frameUrl, type Day } from './frames';
import * as library from './hooks/useLibrary';
import * as ocrSearch from './hooks/useOcrSearch';
import type { DayCount, DayMatch, LineBox } from './search';

// Spied on rather than replaced with `mock.module`: Bun runs every test file in
// one process, and a module mock would also replace the real hooks and
// helpers that useLibrary.test.ts and frames.test.ts are testing.
const useDays = spyOn(library, 'useDays');
const useDay = spyOn(library, 'useDay');
const getFrameTime = spyOn(frames, 'getFrameTime');
const useDayMatches = spyOn(ocrSearch, 'useDayMatches');
const useMatchCounts = spyOn(ocrSearch, 'useMatchCounts');
const useMatchLines = spyOn(ocrSearch, 'useMatchLines');
const useOcrVersion = spyOn(ocrSearch, 'useOcrVersion');
const getPendingFrames = spyOn(frames, 'getPendingFrames');

afterAll(() => {
  useDays.mockRestore();
  useDay.mockRestore();
  getFrameTime.mockRestore();
  useDayMatches.mockRestore();
  useMatchCounts.mockRestore();
  useMatchLines.mockRestore();
  useOcrVersion.mockRestore();
  getPendingFrames.mockRestore();
});

/**
 * Pretend OCR found `query` at `matches[date]` on each day. Other queries
 * find nothing.
 */
function mockSearch(
  query: string,
  matches: Record<string, Array<DayMatch>>,
  counts: Array<DayCount> = [],
  lines: Record<number, Array<LineBox>> = {},
): void {
  useOcrVersion.mockReturnValue('1');
  useMatchLines.mockImplementation((_date: string | null, frame: number | null, q: string) =>
    q === query && frame !== null ? (lines[frame] ?? []) : null,
  );
  useDayMatches.mockImplementation((date: string | null, q: string) => ({
    matches: q === '' ? [] : q === query && date ? (matches[date] ?? []) : [],
    matchesError: null,
  }));
  useMatchCounts.mockImplementation((q: string) => (q === query ? counts : []));
}

/** A match whose OCR'd frame number is its index plus one. */
function match(index: number, endIndex = index + 1): DayMatch {
  return { index, endIndex, frame: index + 1 };
}

function dayPicker(): HTMLElement {
  return screen.getByRole('button', { name: 'Day' });
}

/** Open the day picker and choose `date`. */
async function pickDay(date: string): Promise<void> {
  await openDayPicker();
  fireEvent.click(screen.getByRole('option', { name: new RegExp(date) }));
}

async function openDayPicker(): Promise<void> {
  fireEvent.click(dayPicker());
  await screen.findByRole('listbox');
}

function findBar(): HTMLInputElement {
  return screen.getByLabelText('Find text on screen') as HTMLInputElement;
}

/** Type `query` and wait out the find bar's debounce. */
async function search(query: string): Promise<void> {
  fireEvent.change(findBar(), { target: { value: query } });
  await waitFor(() =>
    expect(useDayMatches).toHaveBeenLastCalledWith(expect.anything(), query, expect.anything()),
  );
}

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

/** Where the scrubber is, as "frame / frames", both counted from 1. */
function position(): string {
  // `hidden`: an open day picker hides the rest of the page from queries.
  const slider = screen.getByRole('slider', { hidden: true }) as HTMLInputElement;
  return `${Number(slider.value) + 1} / ${Number(slider.max) + 1}`;
}

/** The frame currently requested by the `<img>`, as [day, index]. */
function shownFrame(): string {
  return image().getAttribute('src') ?? '';
}

/** Let the in-flight frame finish loading, as the browser would. */
function finishLoading(): void {
  fireEvent.load(image());
}

/**
 * Let the frame-time lookup for the frame on screen resolve inside `act`.
 * Every frame change starts one, and a test that ends before it resolves
 * gets React's "not wrapped in act(...)" warning when it lands afterwards.
 */
async function settleFrameTime(): Promise<void> {
  await act(async () => {});
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
    mockSearch('', {});
    getPendingFrames.mockResolvedValue({ frameCount: 0, ranges: [] });
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
    it('has no Images/Videos tabs and no refresh button', async () => {
      mockLibrary({ '2024-12-20': 10, '2026-10-04': 3 });
      render(<App />);
      expect(screen.queryByRole('button', { name: /Images|Videos|Refresh/ })).not.toBeInTheDocument();
      expect(screen.queryByText(/Refresh/)).not.toBeInTheDocument();
      await settleFrameTime();
    });

    it('opens the newest day on its last frame', async () => {
      mockLibrary({ '2024-12-20': 10, '2026-10-04': 3 });
      render(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 2)));
      expect(dayPicker()).toHaveTextContent('2026-10-04');
      expect(position()).toBe('3 / 3');
    });

    it('lists days newest first and marks today', async () => {
      mockLibrary({ '2024-12-20': 10, [today()]: 3 });
      render(<App />);
      await openDayPicker();
      const options = screen.getAllByRole('option').map((o) => o.textContent?.trim());
      expect(options).toEqual([`${today()} (Today)`, '2024-12-20']);
      await settleFrameTime();
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

      await pickDay('2024-12-20');
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2024-12-20', 9)));
    });

    it('leaves the arrow keys to the day picker, open or closed', async () => {
      mockLibrary({ '2024-12-20': 10, '2026-10-04': 3 });
      render(<App />);
      await waitFor(() => expect(position()).toBe('3 / 3'));
      fireEvent.keyDown(dayPicker(), { key: 'ArrowLeft' });
      await openDayPicker();
      fireEvent.keyDown(screen.getByRole('listbox'), { key: 'ArrowLeft' });
      expect(position()).toBe('3 / 3');
      await settleFrameTime();
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
      expect(position()).toBe('6 / 100');
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
      await waitFor(() => expect(position()).toBe('500 / 500'));

      fireEvent.keyDown(window, { key: 'ArrowLeft' });
      expect(position()).toBe('499 / 500');
      fireEvent.keyDown(window, { key: 'ArrowLeft', shiftKey: true });
      expect(position()).toBe('489 / 500');
      fireEvent.keyDown(window, { key: 'ArrowLeft', altKey: true });
      expect(position()).toBe('389 / 500');
      fireEvent.keyDown(window, { key: 'ArrowRight', altKey: true });
      fireEvent.keyDown(window, { key: 'ArrowRight', altKey: true });
      expect(position()).toBe('500 / 500');
      await settleFrameTime();
    });

    it('keeps Shift and Option steps when the slider has focus', async () => {
      mockLibrary({ '2026-10-04': 500 });
      render(<App />);
      await waitFor(() => expect(position()).toBe('500 / 500'));

      const slider = screen.getByRole('slider');
      const event = fireEvent.keyDown(slider, { key: 'ArrowLeft', shiftKey: true });
      expect(event).toBe(false); // default prevented: no extra native step
      expect(position()).toBe('490 / 500');
      await settleFrameTime();
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
      expect(position()).toBe('4 / 4');
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
      await waitFor(() => expect(position()).toBe('3 / 11'));
      expect(shownFrame()).toBe(frameUrl('2026-10-04', 2));
    });

    it('moves to the new day at midnight when following the live edge', async () => {
      mockLibrary({ '2026-10-04': 3 });
      const { rerender } = render(<App />);
      await waitFor(() => expect(dayPicker()).toHaveTextContent('2026-10-04'));

      mockLibrary({ '2026-10-04': 3, '2026-10-05': 1 });
      rerender(<App />);
      await waitFor(() => expect(dayPicker()).toHaveTextContent('2026-10-05'));
    });

    it('stays on an older day when a new day appears', async () => {
      mockLibrary({ '2026-10-03': 3, '2026-10-04': 3 });
      const { rerender } = render(<App />);
      await waitFor(() => expect(dayPicker()).toHaveTextContent('2026-10-04'));
      await pickDay('2026-10-03');

      mockLibrary({ '2026-10-03': 3, '2026-10-04': 3, '2026-10-05': 1 });
      await act(async () => rerender(<App />));
      await openDayPicker();
      expect(screen.getByRole('option', { name: /2026-10-05/ })).toBeInTheDocument();
      expect(dayPicker()).toHaveTextContent('2026-10-03');
    });
  });

  describe('Find in timeline', () => {
    it('counts matches and steps through them with Enter and Shift+Enter', async () => {
      mockLibrary({ '2026-10-04': 100 });
      mockSearch('cargo', { '2026-10-04': [match(10, 12), match(12, 15), match(40), match(70, 75)] });
      render(<App />);
      await waitFor(() => expect(position()).toBe('100 / 100'));

      await search('cargo');
      // Back-to-back matches count as one stretch.
      expect(screen.getByText('3 matches')).toBeInTheDocument();

      // From the live edge, Enter wraps round to the first match.
      fireEvent.keyDown(findBar(), { key: 'Enter' });
      expect(position()).toBe('11 / 100');
      expect(screen.getByText('1 of 3')).toBeInTheDocument();
      fireEvent.keyDown(findBar(), { key: 'Enter' });
      expect(position()).toBe('41 / 100');
      fireEvent.keyDown(findBar(), { key: 'Enter', shiftKey: true });
      expect(position()).toBe('11 / 100');
      fireEvent.keyDown(findBar(), { key: 'Enter', shiftKey: true });
      expect(position()).toBe('71 / 100');
      expect(screen.getByText('3 of 3')).toBeInTheDocument();

      fireEvent.click(screen.getByRole('button', { name: 'Next match' }));
      expect(position()).toBe('11 / 100');
      await settleFrameTime();
    });

    it('marks the matches on the scrubber', async () => {
      // 101 frames, so frame N sits at N% along the track.
      mockLibrary({ '2026-10-04': 101 });
      mockSearch('cargo', { '2026-10-04': [match(10, 20), match(50), match(100)] });
      render(<App />);
      await search('cargo');

      const marks = Array.from(screen.getByTestId('match-marks').children) as Array<HTMLElement>;
      expect(marks.map((m) => [m.style.left, m.style.width])).toEqual([
        ['10%', '9%'],
        ['50%', '0%'],
        ['100%', '0%'],
      ]);
      await settleFrameTime();
    });

    it('says when nothing matches, and clears with Escape', async () => {
      mockLibrary({ '2026-10-04': 100 });
      render(<App />);
      await search('nothing');
      expect(screen.getByText('No matches')).toBeInTheDocument();
      expect(screen.getByRole('button', { name: 'Next match' })).toBeDisabled();

      fireEvent.keyDown(findBar(), { key: 'Escape' });
      expect(findBar()).toHaveValue('');
      await settleFrameTime();
    });

    it('leaves the arrow keys to the find bar while typing', async () => {
      mockLibrary({ '2026-10-04': 100 });
      render(<App />);
      await waitFor(() => expect(position()).toBe('100 / 100'));
      fireEvent.keyDown(findBar(), { key: 'ArrowLeft' });
      expect(position()).toBe('100 / 100');
      await settleFrameTime();
    });

    it('focuses the find bar on Cmd+F', async () => {
      mockLibrary({ '2026-10-04': 100 });
      render(<App />);
      fireEvent.keyDown(window, { key: 'f', metaKey: true });
      expect(document.activeElement).toBe(findBar());
      await settleFrameTime();
    });

    it('unfocuses the find bar when the header, which drags the window, is clicked', async () => {
      mockLibrary({ '2026-10-04': 100 });
      render(<App />);
      findBar().focus();
      fireEvent.mouseDown(screen.getByRole('banner'));
      expect(document.activeElement).not.toBe(findBar());
      await settleFrameTime();
    });

    it('outlines the matching lines on the frame the match stands for', async () => {
      mockLibrary({ '2026-10-04': 100 });
      const line = { x: 0.5, y: 0.25, width: 0.25, height: 0.125 };
      mockSearch('cargo', { '2026-10-04': [match(10, 20)] }, [], { 11: [line] });
      render(<App />);
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 99)));
      finishLoading();
      await search('cargo');
      expect(screen.queryByTestId('match-highlights')).not.toBeInTheDocument();

      fireEvent.change(screen.getByRole('slider'), { target: { value: '15' } });
      await waitFor(() => expect(shownFrame()).toBe(frameUrl('2026-10-04', 15)));
      // Not until the frame has loaded: the old one is still on screen.
      expect(screen.queryByTestId('match-highlights')).not.toBeInTheDocument();
      finishLoading();
      expect(useMatchLines).toHaveBeenLastCalledWith('2026-10-04', 11, 'cargo');
      const rect = screen.getByTestId('match-highlights').querySelector('rect');
      // 1800×1124 until a frame reports its own size; 4px of padding around.
      expect(rect?.getAttribute('x')).toBe(String(900 - 4));
      expect(rect?.getAttribute('y')).toBe(String(281 - 4));
      await settleFrameTime();
    });

    it('lists other days with matches and opens one on its first match', async () => {
      mockLibrary({ '2026-10-01': 50, '2026-10-02': 50, '2026-10-04': 100 });
      mockSearch(
        'cargo',
        { '2026-10-02': [match(7), match(30)] },
        [
          { day: '2026-10-04', count: 0 },
          { day: '2026-10-02', count: 2 },
          { day: '2026-10-01', count: 1 },
        ],
      );
      render(<App />);
      await waitFor(() => expect(dayPicker()).toHaveTextContent('2026-10-04'));
      await search('cargo');

      const otherDays = screen.getByRole('navigation', { name: 'Other days with matches' });
      expect(otherDays).toHaveTextContent('Also on2026-10-0222026-10-011');
      fireEvent.click(screen.getByRole('button', { name: /2026-10-02/ }));
      await waitFor(() => expect(position()).toBe('8 / 50'));
      expect(dayPicker()).toHaveTextContent('2026-10-02');
      await settleFrameTime();
    });

    it('forgets the jump to a first match when another day is picked first', async () => {
      mockLibrary({ '2026-10-02': 50, '2026-10-03': 50, '2026-10-04': 100 });
      // 10-02's matches never arrive while it is selected.
      useOcrVersion.mockReturnValue('1');
      useMatchLines.mockReturnValue(null);
      useMatchCounts.mockReturnValue([{ day: '2026-10-02', count: 1 }]);
      useDayMatches.mockImplementation(() => ({ matches: null, matchesError: null }));
      render(<App />);
      await search('cargo');
      fireEvent.click(screen.getByRole('button', { name: /2026-10-02/ }));
      await pickDay('2026-10-03');
      await waitFor(() => expect(position()).toBe('50 / 50'));

      // Now the matches are there, and 10-02 is opened from the picker.
      useDayMatches.mockImplementation((date: string | null) => ({
        matches: date === '2026-10-02' ? [match(7)] : [],
        matchesError: null,
      }));
      await pickDay('2026-10-02');
      await waitFor(() => expect(dayPicker()).toHaveTextContent('2026-10-02'));
      await settleFrameTime();
      expect(position()).toBe('50 / 50');
    });

    it('folds away all but the first few other days', async () => {
      mockLibrary({ '2026-10-04': 100 });
      const counts = ['07', '06', '05', '03', '02', '01'].map((d) => ({ day: `2026-09-${d}`, count: 1 }));
      mockSearch('cargo', {}, counts);
      render(<App />);
      await search('cargo');
      expect(screen.queryByRole('button', { name: /2026-09-02/ })).not.toBeInTheDocument();
      fireEvent.click(screen.getByRole('button', { name: '2 more days' }));
      expect(screen.getByRole('button', { name: /2026-09-02/ })).toBeInTheDocument();
      await settleFrameTime();
    });
  });

  describe('Decoding progress', () => {
    it('pales the stretches of the scrubber that are not decoded yet', async () => {
      getPendingFrames.mockResolvedValue({ frameCount: 300, ranges: [{ start: 150, end: 300 }] });
      mockLibrary({ '2026-10-01': 300 }, 'video');
      render(<App />);
      const stretches = (): NodeListOf<HTMLElement> =>
        document.querySelectorAll<HTMLElement>('[data-pending-stretch]');
      await waitFor(() => expect(stretches()).toHaveLength(1));
      expect(stretches()[0].style.left).toBe('50%');
      expect(stretches()[0].style.width).toBe('50%');
      expect(getPendingFrames).toHaveBeenCalledWith('2026-10-01');

      // Showing a frame decodes its chunk, so the scrubber asks again.
      getPendingFrames.mockResolvedValue({ frameCount: 300, ranges: [] });
      finishLoading();
      await waitFor(() => expect(stretches()).toHaveLength(0));
      await settleFrameTime();
    });
  });
});
