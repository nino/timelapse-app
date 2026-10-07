import { describe, expect, it } from 'bun:test';

import { ago, count, formatDuration, until } from './activity';

describe('formatDuration', () => {
  it('reads to the second under an hour and to the minute after', () => {
    expect(formatDuration(0)).toBe('0s');
    expect(formatDuration(45.4)).toBe('45s');
    expect(formatDuration(185)).toBe('3m 05s');
    expect(formatDuration(2 * 3600 + 14 * 60 + 59)).toBe('2h 14m');
    expect(formatDuration(-3)).toBe('0s');
  });
});

describe('ago and until', () => {
  const now = Date.parse('2026-10-07T20:00:00+02:00');

  it('counts from now', () => {
    expect(ago('2026-10-07T19:58:30+02:00', now)).toBe('1m 30s ago');
    expect(ago('2026-10-07T20:00:00+02:00', now)).toBe('just now');
    expect(until('2026-10-07T20:07:42+02:00', now)).toBe('in 7m 42s');
    expect(until('2026-10-07T19:59:00+02:00', now)).toBe('any moment now');
  });
});

describe('count', () => {
  it('pluralises and groups digits', () => {
    expect(count(1, 'frame')).toBe('1 frame');
    expect(count(3600, 'frame')).toBe('3,600 frames');
  });
});
