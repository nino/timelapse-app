import { describe, expect, it } from 'bun:test';
import { render, screen, within } from '@testing-library/react';
import { getVersion } from '@tauri-apps/api/app';

import { byDay, formatDay, type ChangelogEntry } from './about';
import { AboutView } from './AboutView';
import { mocked } from './test/mocked';

const changelog: ChangelogEntry[] = [
  { version: '0.1.182', date: '2026-10-08', title: 'Capture raw pixels on macOS', pr: 155 },
  { version: '0.1.179', date: '2026-10-08', title: 'Add a low-power mode', pr: 154 },
  { version: '0.1.156', date: '2026-10-07', title: 'Add a Settings window', pr: 140 },
];

describe('AboutView', () => {
  it('shows the version and each day’s changes, newest first', async () => {
    mocked(getVersion).mockResolvedValueOnce('0.1.182');
    render(<AboutView changelog={changelog} />);

    expect(await screen.findByText('Version 0.1.182')).toBeInTheDocument();
    const days = within(screen.getByRole('region', { name: 'Change log' })).getAllByRole('region');
    expect(days.map((day) => day.getAttribute('aria-label'))).toEqual(['8 October 2026', '7 October 2026']);
    expect(within(days[0]).getAllByRole('listitem').map((item) => item.textContent)).toEqual([
      'Capture raw pixels on macOS0.1.182',
      'Add a low-power mode0.1.179',
    ]);
  });

  it('says so when the build has no change log', () => {
    render(<AboutView changelog={[]} />);
    expect(screen.getByText('This build has no change log.')).toBeInTheDocument();
  });
});

describe('byDay', () => {
  it('groups neighbouring entries from the same day', () => {
    expect(byDay(changelog).map((day) => [day.date, day.entries.length])).toEqual([
      ['2026-10-08', 2],
      ['2026-10-07', 1],
    ]);
  });
});

describe('formatDay', () => {
  it('names the day without shifting it by the time zone', () => {
    expect(formatDay('2026-01-01')).toBe('1 January 2026');
  });
});
