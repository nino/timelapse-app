import React from "react";

import { getPendingFrames, hasVideo as hasVideoFrames, type Day, type PendingFrames } from "../frames";
import { useLatestLoad } from "./useDirectoryChanges";
import { useFramesDecoded } from "./useFramesDecoded";

/**
 * The stretches of `day` that would have to be decoded from video before
 * they can be shown (`ranges` without full previews, `undecoded` with or
 * without), with the frame count they were measured against. Days served
 * only from screenshots have nothing pending, so they never ask.
 *
 * Re-asked when the day's frame count changes, and when the frame source
 * reports decoding part of the day or building previews for it. In the app,
 * read-ahead decodes the chunks, and reports each one, including the chunk of
 * a frame the viewer asked for (that frame alone is decoded first).
 */
export function usePendingFrames(day: Day | null): PendingFrames | null {
  const [state, setState] = React.useState<{ date: string; pending: PendingFrames } | null>(null);
  const date = day?.date ?? null;
  const frameCount = day?.frameCount ?? 0;
  const hasVideo = hasVideoFrames(day);
  const current = hasVideo && state?.date === date ? state.pending : null;

  const decodes = useFramesDecoded(date, "decoded") + useFramesDecoded(date, "previews");

  // Answers may land out of order. Each one is published unless a newer
  // request has already published, so a burst of requests still shows the
  // latest answer that arrived rather than none at all.
  const asked = React.useRef(0);
  const published = React.useRef(0);
  const load = React.useCallback(async (): Promise<void> => {
    if (!date || !hasVideo) return;
    const mine = ++asked.current;
    try {
      const pending = await getPendingFrames(date);
      if (mine < published.current) return;
      published.current = mine;
      setState({ date, pending });
    } catch (error) {
      console.error("Error fetching pending frames:", error);
    }
    // `frameCount` and the decode count aren't read, but each change means
    // the answer may have changed, so it is asked again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [date, hasVideo, frameCount, decodes]);
  useLatestLoad(load);

  return current;
}

/** Whether `index` is in one of `ranges`. */
export function inRanges(ranges: ReadonlyArray<{ start: number; end: number }>, index: number): boolean {
  return ranges.some(({ start, end }) => start <= index && index < end);
}
