import React from "react";

import { frameUrl, getPendingFrames, onFramesDecoded, type Day, type PendingFrames } from "../frames";
import { useLatestLoad } from "./useDirectoryChanges";
import { useMountEffect } from "./useMountEffect";

/**
 * The stretches of `day` that would have to be decoded from video before
 * they can be shown, with the frame count they were measured against. Days
 * served only from screenshots have nothing pending, so they never ask.
 *
 * Re-asked when the day's frame count changes and when `loaded` (the last
 * frame the viewer finished loading) was in a pending stretch, since that
 * load decoded its chunk. Loading a frame that was already decoded changes
 * nothing, so it doesn't ask. Also re-asked when the frame source reports
 * that it read ahead into the day.
 */
export function usePendingFrames(day: Day | null, loaded: string | null): PendingFrames | null {
  const [state, setState] = React.useState<{ date: string; pending: PendingFrames } | null>(null);
  const [decodes, setDecodes] = React.useState(0);
  const date = day?.date ?? null;
  const frameCount = day?.frameCount ?? 0;
  const hasVideo = day?.source === "video" || day?.source === "mixed";
  const current = hasVideo && state?.date === date ? state.pending : null;

  // A finished load of a pending frame means its chunk is decoded now.
  const [seenLoaded, setSeenLoaded] = React.useState(loaded);
  if (loaded !== seenLoaded) {
    setSeenLoaded(loaded);
    const index = date && loaded ? frameIndex(loaded, date) : null;
    if (index !== null && current?.ranges.some(({ start, end }) => start <= index && index < end)) {
      setDecodes((n) => n + 1);
    }
  }

  // Read-ahead decodes chunks no frame load asked for. It only reads ahead
  // into days with video, so listening all along costs nothing, and counting
  // per day means another day's decodes don't re-ask about this one.
  const [readAhead, setReadAhead] = React.useState<ReadonlyMap<string, number>>(() => new Map());
  const readAheadDecodes = date === null ? 0 : (readAhead.get(date) ?? 0);
  useMountEffect(() => {
    let stop: (() => void) | null = null;
    let unmounted = false;
    onFramesDecoded((decoded) => {
      setReadAhead((counts) => new Map(counts).set(decoded, (counts.get(decoded) ?? 0) + 1));
    }).then(
      (unlisten) => {
        if (unmounted) unlisten();
        else stop = unlisten;
      },
      (error: unknown) => {
        console.error("Error listening for decoded frames:", error);
      },
    );
    return (): void => {
      unmounted = true;
      stop?.();
    };
  });

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
    // `frameCount` and the decode counts aren't read, but each change means
    // the answer may have changed, so it is asked again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [date, hasVideo, frameCount, decodes, readAheadDecodes]);
  useLatestLoad(load);

  return current;
}

/** The index in `url` if it is a frame of `date`, else null. */
function frameIndex(url: string, date: string): number | null {
  const prefix = frameUrl(date, 0).slice(0, -1);
  if (!url.startsWith(prefix)) return null;
  const index = Number(url.slice(prefix.length));
  return Number.isInteger(index) ? index : null;
}
