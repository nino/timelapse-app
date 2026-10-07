import React from "react";

import type { PendingFrames } from "./frames";

/**
 * Where each pending stretch sits on the scrubber, as percentages of its
 * width, measured against the frame count the stretches were found with.
 */
export function stretchPositions(
  pending: PendingFrames | null,
): Array<{ left: number; width: number }> {
  if (!pending || pending.frameCount === 0) return [];
  const at = (index: number): number =>
    (Math.min(index, pending.frameCount) / pending.frameCount) * 100;
  return pending.ranges.map(({ start, end }) => ({
    left: at(start),
    width: at(end) - at(start),
  }));
}

/**
 * The stretches of the day still to be decoded from video, drawn as soft
 * white bands under the scrubber. Each is its own rounded, glowing element
 * so it reads as a deliberate mark rather than a hard-edged gap in the track.
 * Goes inside a `relative` container, before the range input.
 */
export function PendingStretches({
  pending,
}: {
  pending: PendingFrames | null;
}): React.ReactNode {
  const stretches = stretchPositions(pending);
  if (stretches.length === 0) return null;
  return (
    <div aria-hidden className="pointer-events-none absolute top-0 left-1 right-1 h-2">
      {stretches.map(({ left, width }) => (
        <div
          key={left}
          data-pending-stretch
          className="absolute inset-y-0 min-w-2 rounded-full bg-white/90 shadow-[0_0_6px_2px_rgba(255,255,255,0.8)]"
          style={{ left: `${left}%`, width: `${width}%` }}
        />
      ))}
    </div>
  );
}
