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
    lowPowerUntil: null,
    capture: {
      running: true,
      lastFrame: { day: '2026-10-07', number: 1234, at: secondsAgo(2) },
      framesSaved: 4000,
      lastBlackAt: null,
      lastError: null,
      failuresInARow: 0,
      failuresBeforeBackoff: 3,
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
      failed: 0,
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

  it('treats a single failed capture as a retry, not a failure', async () => {
    const base = activity();
    mocked(invoke).mockResolvedValue(
      activity({
        capture: {
          ...base.capture,
          lastError: { at: secondsAgo(1), message: "Can't get active window" },
          failuresInARow: 1,
        },
      }),
    );
    render(<ActivityView />);

    await waitFor(() =>
      expect(screen.getByTestId('Capture-headline')).toHaveTextContent('Capture failed; trying again in a second'),
    );
    expect(screen.getByText("Can't get active window")).toBeInTheDocument();
  });

  it('flags a capture that keeps failing', async () => {
    const base = activity();
    mocked(invoke).mockResolvedValue(
      activity({
        capture: {
          ...base.capture,
          lastError: { at: secondsAgo(1), message: "Can't get active window" },
          failuresInARow: 3,
        },
      }),
    );
    render(<ActivityView />);

    await waitFor(() =>
      expect(screen.getByTestId('Capture-headline')).toHaveTextContent(
        'Capture failed 3 times in a row; trying again every minute',
      ),
    );
  });

  it('goes back to capturing once a capture works after a failure', async () => {
    const base = activity();
    mocked(invoke).mockResolvedValue(
      activity({ capture: { ...base.capture, lastError: { at: secondsAgo(5), message: "Can't get active window" } } }),
    );
    render(<ActivityView />);

    await waitFor(() => expect(screen.getByTestId('Capture-headline')).toHaveTextContent('Capturing every second'));
  });

  it('dismisses a last error', async () => {
    const base = activity();
    const failure = { at: secondsAgo(30), message: 'Skipped frame 12' };
    mocked(invoke).mockImplementation(<T,>(command: string): Promise<T> =>
      Promise.resolve(
        command === 'get_activity' ? activity({ ocr: { ...base.ocr, lastError: failure } }) : null,
      ) as Promise<T>,
    );
    render(<ActivityView />);

    await waitFor(() => expect(screen.getByText('Skipped frame 12')).toBeInTheDocument());
    fireEvent.click(screen.getByRole('button', { name: 'Dismiss' }));

    expect(screen.queryByText('Skipped frame 12')).not.toBeInTheDocument();
    expect(invoke).toHaveBeenCalledWith('dismiss_error', { source: 'ocr', at: failure.at });
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

      await waitFor(() => expect(screen.getByTestId('Speed-headline')).toHaveTextContent(/^Normal/));
      fireEvent.click(screen.getByRole('button', { name: '1h' }));
      expect(screen.getByRole('button', { name: '1h' })).toHaveAttribute('aria-pressed', 'true');
      fireEvent.click(screen.getByRole('checkbox', { name: 'Also on battery' }));
      boost = { until: secondsAhead(3600), allowBattery: true };
      fireEvent.click(screen.getByRole('button', { name: 'Boost!' }));

      expect(invoke).toHaveBeenCalledWith('start_boost', { minutes: 60, allowBattery: true });
      // Asks again straight away rather than at the next poll.
      await waitFor(() =>
        expect(screen.getByTestId('Speed-headline')).toHaveTextContent('Boosting: full speed for another 1h 00m'),
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
        expect(screen.getByTestId('Speed-headline')).toHaveTextContent('Boosting: full speed for another 2m 05s'),
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
      expect(screen.getByTestId('Speed-headline')).toHaveTextContent(
        'Boost waiting for AC power (10m 00s left); tick “Also on battery” to start now',
      );
      boost = { until: secondsAhead(600), allowBattery: true };
      fireEvent.click(screen.getByRole('checkbox', { name: 'Also on battery' }));

      expect(invoke).toHaveBeenCalledWith('set_boost_allow_battery', { allowBattery: true });
      await waitFor(() => expect(screen.getByTestId('Power-headline')).toHaveTextContent('On battery: boosting anyway'));
      expect(screen.getByRole('checkbox', { name: 'Also on battery' })).toBeChecked();
      expect(screen.getByTestId('Speed-headline')).toHaveTextContent('Boosting: full speed for another 10m 00s');
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

  describe('low-power mode', () => {
    function respond(current: () => Activity): void {
      mocked(invoke).mockImplementation(<T,>(command: string): Promise<T> =>
        Promise.resolve(command === 'get_activity' ? current() : null) as Promise<T>,
      );
    }

    it('turns conversion and OCR off for the chosen length', async () => {
      let lowPowerUntil: string | null = null;
      respond(() => activity({ lowPowerUntil }));
      render(<ActivityView />);

      fireEvent.click(await screen.findByRole('button', { name: '30m' }));
      lowPowerUntil = secondsAhead(1800);
      fireEvent.click(screen.getByRole('button', { name: 'Low power' }));

      expect(invoke).toHaveBeenCalledWith('start_low_power', { minutes: 30 });
      await waitFor(() =>
        expect(screen.getByTestId('Speed-headline')).toHaveTextContent(
          'Low-power mode: conversion and OCR are off for another 30m 00s',
        ),
      );
      expect(screen.queryByRole('button', { name: 'Boost!' })).not.toBeInTheDocument();
      expect(screen.queryByRole('checkbox', { name: 'Also on battery' })).not.toBeInTheDocument();
    });

    it('ends low-power mode', async () => {
      let lowPowerUntil: string | null = secondsAhead(600);
      respond(() => activity({ lowPowerUntil }));
      render(<ActivityView />);

      lowPowerUntil = null;
      fireEvent.click(await screen.findByRole('button', { name: 'End low-power mode' }));

      expect(invoke).toHaveBeenCalledWith('stop_low_power');
      await screen.findByRole('button', { name: 'Low power' });
    });

    it('says conversion and OCR are off', async () => {
      const base = activity();
      respond(() =>
        activity({
          lowPowerUntil: secondsAhead(600),
          conversion: { ...base.conversion, state: 'lowPower', nextCheckAt: secondsAhead(600) },
          ocr: { ...base.ocr, state: 'lowPower', nextCheckAt: secondsAhead(600) },
        }),
      );
      render(<ActivityView />);

      await waitFor(() =>
        expect(screen.getByTestId('Video conversion-headline')).toHaveTextContent(
          'Off for low-power mode; resumes in 10m 00s',
        ),
      );
      expect(screen.getByTestId('OCR-headline')).toHaveTextContent('Off for low-power mode; resumes in 10m 00s');
    });
  });
});
