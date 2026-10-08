import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { invoke } from '@tauri-apps/api/core';
import { fireEvent, render, screen, waitFor } from '@testing-library/react';

import type { Activity } from './activity';
import { ActivityView } from './ActivityView';
import { mocked } from './test/mocked';

// invoke is replaced with a mock in src/test/setup.ts.

const NOW = Date.parse('2026-10-07T20:00:00+02:00');
const secondsAgo = (s: number): string => new Date(NOW - s * 1000).toISOString();
const secondsAhead = (s: number): string => new Date(NOW + s * 1000).toISOString();

function activity(overrides: Partial<Activity> = {}): Activity {
  return {
    onAcPower: true,
    boost: null,
    capture: {
      running: true,
      lastFrame: { day: '2026-10-07', number: 1234, at: secondsAgo(2) },
      framesSaved: 4000,
      lastBlackAt: null,
      lastError: null,
    },
    conversion: {
      state: 'resting',
      current: null,
      nextCheckAt: secondsAhead(462),
      ready: 2,
      waitingForOcr: 1,
      last: { video: '2026-10-07--18-00-00--hourly.mov', finishedAt: secondsAgo(138), tookSecs: 250, error: null },
      videosMade: 1,
      framesDeleted: 3600,
    },
    ocr: {
      state: 'working',
      current: { day: '2026-10-07', number: 900, at: secondsAgo(0) },
      remaining: 334,
      recognized: 120,
      skipped: 780,
      nextCheckAt: null,
      lastError: null,
      readingVideo: false,
      videoDaysLeft: null,
    },
    ...overrides,
  };
}

let realNow: () => number;

beforeEach(() => {
  mock.clearAllMocks();
  mocked(invoke).mockReset();
  realNow = Date.now;
  Date.now = (): number => NOW;
});

afterEach(() => {
  Date.now = realNow;
});

describe('ActivityView', () => {
  it('shows what each background job is doing', async () => {
    mocked(invoke).mockResolvedValue(activity());
    render(<ActivityView />);

    await waitFor(() => expect(screen.getByTestId('Capture-headline')).toHaveTextContent('Capturing every second'));
    expect(invoke).toHaveBeenCalledWith('get_activity');
    expect(screen.getByText(/2026-10-07 #01234/)).toHaveTextContent('(2s ago)');
    expect(screen.getByTestId('Video conversion-headline')).toHaveTextContent('Next batch starts in 7m 42s');
    expect(screen.getByText('2 hours ready, 1 waiting for OCR')).toBeInTheDocument();
    expect(screen.getByText(/hourly\.mov in 4m 10s/)).toBeInTheDocument();
    expect(screen.getByTestId('OCR-headline')).toHaveTextContent('Reading 2026-10-07 #00900');
    expect(screen.getByText('334 frames in this pass')).toBeInTheDocument();
  });

  it('names the encode in progress and how long it has run', async () => {
    const base = activity();
    mocked(invoke).mockResolvedValue(
      activity({
        conversion: {
          ...base.conversion,
          state: 'working',
          current: {
            video: 'v.mov',
            day: '2026-10-07',
            hour: 9,
            frames: 3412,
            startedAt: secondsAgo(133),
            framesDone: 1706,
            pausedSince: null,
            pausedSecs: 0,
          },
          nextCheckAt: null,
        },
      }),
    );
    render(<ActivityView />);

    await waitFor(() =>
      expect(screen.getByTestId('Video conversion-headline')).toHaveTextContent('Encoding 2026-10-07 09:00 (3,412 frames)'),
    );
    expect(screen.getByText('2m 13s')).toBeInTheDocument();
    // Half done after 2m 13s: about as long again to go.
    expect(screen.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
    expect(screen.getByText(/^50%/)).toHaveTextContent('50%, about 2m 13s left');
  });

  it('says when an encode is paused on battery', async () => {
    const base = activity();
    mocked(invoke).mockResolvedValue(
      activity({
        conversion: {
          ...base.conversion,
          state: 'onBattery',
          current: {
            video: 'v.mov',
            day: '2026-10-07',
            hour: 9,
            frames: 3412,
            startedAt: secondsAgo(400),
            framesDone: 1706,
            pausedSince: secondsAgo(100),
            pausedSecs: 167,
          },
          nextCheckAt: null,
        },
      }),
    );
    render(<ActivityView />);

    await waitFor(() =>
      expect(screen.getByTestId('Video conversion-headline')).toHaveTextContent('Encoding 2026-10-07 09:00 paused on battery'),
    );
    expect(screen.getByText('2m 13s')).toBeInTheDocument();
    expect(screen.getByText(/^50%/)).toHaveTextContent(/^50%$/);
  });

  it('says when work waits for AC power', async () => {
    const base = activity();
    mocked(invoke).mockResolvedValue(
      activity({
        onAcPower: false,
        conversion: { ...base.conversion, state: 'onBattery', nextCheckAt: secondsAhead(45) },
        ocr: { ...base.ocr, state: 'onBattery', nextCheckAt: secondsAhead(200) },
      }),
    );
    render(<ActivityView />);

    await waitFor(() => expect(screen.getByTestId('Power-headline')).toHaveTextContent(/On battery/));
    expect(screen.getByTestId('Video conversion-headline')).toHaveTextContent('Paused on battery; next check in 45s');
    expect(screen.getByTestId('OCR-headline')).toHaveTextContent('Paused on battery; next check in 3m 20s');
  });

  it('shows OCR reading old videos and how many days are left', async () => {
    const base = activity();
    mocked(invoke).mockResolvedValue(
      activity({
        ocr: {
          ...base.ocr,
          readingVideo: true,
          current: { day: '2024-12-20', number: 1234, at: secondsAgo(0) },
          videoDaysLeft: 212,
        },
      }),
    );
    render(<ActivityView />);

    await waitFor(() =>
      expect(screen.getByTestId('OCR-headline')).toHaveTextContent('Reading the 2024-12-20 video, frame 1,234'),
    );
    expect(screen.getByText('212 days left to read')).toBeInTheDocument();
    expect(screen.queryByText(/in this pass/)).not.toBeInTheDocument();
  });

  it('flags a failing capture', async () => {
    const base = activity();
    mocked(invoke).mockResolvedValue(
      activity({
        capture: { ...base.capture, lastError: { at: secondsAgo(1), message: "Can't get active window" } },
      }),
    );
    render(<ActivityView />);

    await waitFor(() => expect(screen.getByTestId('Capture-headline')).toHaveTextContent(/Capture failed/));
    expect(screen.getByText("Can't get active window")).toBeInTheDocument();
  });

  describe('boost', () => {
    /** Answers `get_activity` with `current()` and records the other commands. */
    function respond(current: () => Activity, answer?: (command: string) => Promise<unknown>): void {
      const reply = answer ?? (async (command: string): Promise<unknown> => {
        if (command === 'get_activity') return current();
        if (command === 'stop_boost') return undefined;
        return current().boost;
      });
      mocked(invoke).mockImplementation(<T,>(command: string): Promise<T> => reply(command) as Promise<T>);
    }

    it('starts a boost of the chosen length', async () => {
      let boost: Activity['boost'] = null;
      respond(() => activity({ boost }));
      render(<ActivityView />);

      await waitFor(() => expect(screen.getByTestId('Boost-headline')).toHaveTextContent(/^Off/));
      fireEvent.click(screen.getByRole('button', { name: '1h' }));
      expect(screen.getByRole('button', { name: '1h' })).toHaveAttribute('aria-pressed', 'true');
      fireEvent.click(screen.getByRole('checkbox', { name: 'Also on battery' }));
      boost = { until: secondsAhead(3600), allowBattery: true };
      fireEvent.click(screen.getByRole('button', { name: 'Boost!' }));

      expect(invoke).toHaveBeenCalledWith('start_boost', { minutes: 60, allowBattery: true });
      // Asks again straight away rather than at the next poll.
      await waitFor(() =>
        expect(screen.getByTestId('Boost-headline')).toHaveTextContent('Running at full speed for another 1h 00m'),
      );
      expect(screen.getByRole('button', { name: 'Stop boost' })).toBeInTheDocument();
      expect(screen.queryByRole('button', { name: '1h' })).not.toBeInTheDocument();
    });

    it('defaults to 20 minutes, not on battery', async () => {
      respond(() => activity());
      render(<ActivityView />);

      fireEvent.click(await screen.findByRole('button', { name: 'Boost!' }));
      expect(invoke).toHaveBeenCalledWith('start_boost', { minutes: 20, allowBattery: false });
      await waitFor(() => expect(screen.getByRole('button', { name: 'Boost!' })).toBeEnabled());
    });

    it('stops a boost', async () => {
      let boost: Activity['boost'] = { until: secondsAhead(125), allowBattery: false };
      respond(() => activity({ boost }));
      render(<ActivityView />);

      await waitFor(() =>
        expect(screen.getByTestId('Boost-headline')).toHaveTextContent('Running at full speed for another 2m 05s'),
      );
      boost = null;
      fireEvent.click(screen.getByRole('button', { name: 'Stop boost' }));

      expect(invoke).toHaveBeenCalledWith('stop_boost');
      await screen.findByRole('button', { name: 'Boost!' });
    });

    it('changes whether the boost in progress runs on battery', async () => {
      let boost: Activity['boost'] = { until: secondsAhead(600), allowBattery: false };
      respond(() => activity({ boost, onAcPower: false }));
      render(<ActivityView />);

      await waitFor(() => expect(screen.getByTestId('Power-headline')).toHaveTextContent(/wait until the Mac is plugged in/));
      expect(screen.getByTestId('Boost-headline')).toHaveTextContent(
        'Waiting for AC power (10m 00s left); tick “Also on battery” to start now',
      );
      boost = { until: secondsAhead(600), allowBattery: true };
      fireEvent.click(screen.getByRole('checkbox', { name: 'Also on battery' }));

      expect(invoke).toHaveBeenCalledWith('set_boost_allow_battery', { allowBattery: true });
      await waitFor(() => expect(screen.getByTestId('Power-headline')).toHaveTextContent('On battery: boosting anyway'));
      expect(screen.getByRole('checkbox', { name: 'Also on battery' })).toBeChecked();
      expect(screen.getByTestId('Boost-headline')).toHaveTextContent('Running at full speed for another 10m 00s');
    });

    it('shows why a boost could not start', async () => {
      respond(activity, async (command: string): Promise<unknown> => {
        if (command === 'get_activity') return activity();
        throw 'nope';
      });
      render(<ActivityView />);

      fireEvent.click(await screen.findByRole('button', { name: 'Boost!' }));
      expect(await screen.findByRole('alert')).toHaveTextContent('nope');
    });
  });
});
