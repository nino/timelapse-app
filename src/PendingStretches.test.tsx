import { describe, expect, it } from 'bun:test';

import { stretchPositions } from './PendingStretches';

describe('stretchPositions', () => {
  it('has nothing to draw when nothing is pending', () => {
    expect(stretchPositions(null)).toEqual([]);
    expect(stretchPositions({ frameCount: 100, ranges: [] })).toEqual([]);
    expect(stretchPositions({ frameCount: 0, ranges: [{ start: 0, end: 1 }] })).toEqual([]);
  });

  it('places each stretch in proportion to the day it was measured against', () => {
    expect(
      stretchPositions({
        frameCount: 200,
        ranges: [
          { start: 0, end: 50 },
          { start: 100, end: 250 },
        ],
      }),
    ).toEqual([
      { left: 0, width: 25 },
      { left: 50, width: 50 },
    ]);
  });
});
