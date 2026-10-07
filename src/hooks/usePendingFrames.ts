import React from "react";

import { getPendingFrames, type Day, type FrameRange } from "../frames";

const NONE: Array<FrameRange> = [];

/**
 * The frame ranges of `day` that would have to be decoded from video before
 * they can be shown. Re-asked whenever the day's frame count changes and
 * whenever `loadedFrames` does, since showing a frame decodes (and may evict) a chunk.
 * Days served only from screenshots have nothing pending, so they never ask.
 */
export function usePendingFrames(day: Day | null, loadedFrames: number): Array<FrameRange> {
  const [pending, setPending] = React.useState<{ date: string; ranges: Array<FrameRange> } | null>(
    null,
  );
  const date = day?.date ?? null;
  const frameCount = day?.frameCount ?? 0;
  const hasVideo = day?.source === "video" || day?.source === "mixed";

  React.useEffect(() => {
    if (!date || !hasVideo) return;
    let cancelled = false;
    getPendingFrames(date).then(
      (ranges) => {
        if (!cancelled) setPending({ date, ranges });
      },
      (error: unknown) => {
        if (!cancelled) console.error("Error fetching pending frames:", error);
      },
    );
    return (): void => {
      cancelled = true;
    };
  }, [date, hasVideo, frameCount, loadedFrames]);

  return hasVideo && pending?.date === date ? pending.ranges : NONE;
}

/**
 * A background for the scrubber that pales the stretches in `pending`, or
 * undefined when nothing is pending.
 */
export function pendingTrackBackground(
  pending: Array<FrameRange>,
  frameCount: number,
): string | undefined {
  if (pending.length === 0 || frameCount === 0) return undefined;
  const at = (index: number): string => `${((index / frameCount) * 100).toFixed(3)}%`;
  const stops = pending.flatMap(({ start, end }) => [
    `transparent ${at(start)}`,
    `${PENDING_COLOR} ${at(start)}`,
    `${PENDING_COLOR} ${at(end)}`,
    `transparent ${at(end)}`,
  ]);
  return `linear-gradient(to right, ${stops.join(", ")})`;
}

const PENDING_COLOR = "rgba(255, 255, 255, 0.75)";
