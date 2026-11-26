import React from "react";

import { mergeCloseRanges } from "./apps";
import type { FrameRange } from "./frames";
import { rangeAt, type Stop } from "./search";

/** Where the slider puts its thumb's centre for frame `index`, as a percentage of its range. */
function playheadPercent(index: number, frameCount: number): number {
  return (Math.min(index, frameCount - 1) / Math.max(frameCount - 1, 1)) * 100;
}

/** Where the searched text was on screen, drawn just above the scrubber. */
export function MatchMarks({
  stops,
  frameCount,
  currentIndex,
}: {
  stops: Array<Stop>;
  frameCount: number;
  currentIndex: number;
}): React.ReactNode {
  if (stops.length === 0 || frameCount === 0) return null;
  const current = rangeAt(stops, currentIndex);
  const at = (index: number): number => playheadPercent(index, frameCount);
  return (
    <div
      data-testid="match-marks"
      // Inset by the track's padding (4px) plus half the 6px playhead, the
      // range the playhead's centre moves over.
      className="absolute left-[7px] right-[7px] -top-3 h-3 pointer-events-none"
      aria-hidden="true"
    >
      {stops.map((stop, i) => (
        <span
          key={stop.index}
          className={`absolute bottom-0 h-3 min-w-[3px] -ml-px rounded-sm ${
            i === current ? "bg-yellow-600" : "bg-yellow-400"
          }`}
          style={{
            left: `${at(stop.index)}%`,
            width: `${at(stop.endIndex - 1) - at(stop.index)}%`,
          }}
        />
      ))}
    </div>
  );
}

/**
 * Where the filtered app was in front, drawn as a strip just under the
 * scrubber. Memoised, and gaps under a pixel or so closed up, so a day of
 * app switching neither re-renders nor draws thousands of bars per frame of
 * a scrub.
 */
export const AppMarks = React.memo(function AppMarks({
  ranges,
  frameCount,
}: {
  ranges: Array<FrameRange>;
  frameCount: number;
}): React.ReactNode {
  const merged = React.useMemo(
    (): Array<FrameRange> => mergeCloseRanges(ranges, Math.ceil(frameCount / 1000)),
    [ranges, frameCount],
  );
  if (merged.length === 0 || frameCount === 0) return null;
  const at = (index: number): number => playheadPercent(index, frameCount);
  return (
    <div
      data-testid="app-marks"
      className="absolute left-[7px] right-[7px] -bottom-2 h-1 pointer-events-none"
      aria-hidden="true"
    >
      {merged.map((range) => (
        <span
          key={range.start}
          className="absolute inset-y-0 min-w-[2px] rounded-full bg-primary"
          style={{
            left: `${at(range.start)}%`,
            width: `${at(range.end - 1) - at(range.start)}%`,
          }}
        />
      ))}
    </div>
  );
});
