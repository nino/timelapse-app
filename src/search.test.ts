import { describe, expect, it } from 'bun:test';

import { nextStop, previousStop, rangeAt, toStops, type DayMatch } from './search';

function match(index: number, endIndex: number): DayMatch {
  return { index, endIndex, frame: index + 1 };
}

describe('toStops', () => {
  it('merges matches that touch or overlap', () => {
    expect(toStops([match(1, 3), match(3, 5), match(4, 6), match(9, 10)])).toEqual([
      { index: 1, endIndex: 6 },
      { index: 9, endIndex: 10 },
    ]);
    expect(toStops([])).toEqual([]);
  });
});

describe('stepping', () => {
  const stops = [
    { index: 10, endIndex: 20 },
    { index: 40, endIndex: 41 },
  ];

  it('finds the stop a frame is in', () => {
    expect(rangeAt(stops, 10)).toBe(0);
    expect(rangeAt(stops, 19)).toBe(0);
    expect(rangeAt(stops, 20)).toBe(-1);
  });

  it('goes to the next stop, wrapping round', () => {
    expect(nextStop(stops, 0)).toEqual(stops[0]);
    expect(nextStop(stops, 15)).toEqual(stops[1]);
    expect(nextStop(stops, 40)).toEqual(stops[0]);
    expect(nextStop([], 3)).toBeNull();
  });

  it('goes to the stop before the current one, wrapping round', () => {
    // Inside the second stop, "previous" is the first, not its own start.
    expect(previousStop(stops, 40)).toEqual(stops[0]);
    expect(previousStop(stops, 30)).toEqual(stops[0]);
    expect(previousStop(stops, 15)).toEqual(stops[1]);
    expect(previousStop(stops, 99)).toEqual(stops[1]);
    expect(previousStop([], 3)).toBeNull();
  });
});
