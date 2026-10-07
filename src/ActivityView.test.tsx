import { afterEach, beforeEach, describe, expect, it, mock } from 'bun:test';
import { invoke } from '@tauri-apps/api/core';
import { render, screen, waitFor } from '@testing-library/react';

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
          current: { video: 'v.mov', day: '2026-10-07', hour: 9, frames: 3412, startedAt: secondsAgo(133) },
          nextCheckAt: null,
        },
      }),
    );
    render(<ActivityView />);

    await waitFor(() =>
      expect(screen.getByTestId('Video conversion-headline')).toHaveTextContent('Encoding 2026-10-07 09:00 (3,412 frames)'),
    );
    expect(screen.getByText('2m 13s')).toBeInTheDocument();
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
});
