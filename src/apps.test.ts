import { describe, expect, it } from 'bun:test';

import { formatFrames, mergeCloseRanges, nextInRanges } from './apps';

describe('nextInRanges', () => {
  const ranges = [
    { start: 5, end: 8 },
    { start: 20, end: 21 },
  ];

  it('stays on a frame inside a range', () => {
    expect(nextInRanges(ranges, 5)).toBe(5);
    expect(nextInRanges(ranges, 7)).toBe(7);
  });

  it('jumps a gap to the next range', () => {
    expect(nextInRanges(ranges, 0)).toBe(5);
    expect(nextInRanges(ranges, 8)).toBe(20);
  });

  it('finds nothing after the last range', () => {
    expect(nextInRanges(ranges, 21)).toBeNull();
    expect(nextInRanges([], 0)).toBeNull();
  });
});

describe('mergeCloseRanges', () => {
  it('closes gaps shorter than the minimum', () => {
    expect(
      mergeCloseRanges(
        [
          { start: 0, end: 2 },
          { start: 3, end: 4 },
          { start: 10, end: 12 },
        ],
        2,
      ),
    ).toEqual([
      { start: 0, end: 4 },
      { start: 10, end: 12 },
    ]);
  });
});

describe('formatFrames', () => {
  it('reads one frame a second as time', () => {
    expect(formatFrames(30)).toBe('<1m');
    expect(formatFrames(12 * 60 + 5)).toBe('12m');
    expect(formatFrames(2 * 3600 + 5 * 60)).toBe('2h 05m');
  });
});
