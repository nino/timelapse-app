import React from "react";

import { frameUrl, getPendingFrames, onFramesDecoded, type Day, type PendingFrames } from "../frames";

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
  const currentRef = React.useRef(current);
  React.useEffect(() => {
    currentRef.current = current;
  }, [current]);
  React.useEffect(() => {
    const pending = currentRef.current;
    const index = date && loaded ? frameIndex(loaded, date) : null;
    if (index === null || !pending) return;
    if (pending.ranges.some(({ start, end }) => start <= index && index < end)) {
      setDecodes((n) => n + 1);
    }
  }, [date, loaded]);

  // Read-ahead decodes chunks no frame load asked for.
  React.useEffect(() => {
    if (!date || !hasVideo) return;
    let stop: (() => void) | null = null;
    let unmounted = false;
    onFramesDecoded((decoded) => {
      if (decoded === date) setDecodes((n) => n + 1);
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
  }, [date, hasVideo]);

  // Answers may land out of order. Each one is published unless a newer
  // request has already published, so a burst of requests still shows the
  // latest answer that arrived rather than none at all.
  const asked = React.useRef(0);
  const published = React.useRef(0);
  React.useEffect(() => {
    if (!date || !hasVideo) return;
    const mine = ++asked.current;
    getPendingFrames(date).then(
      (pending) => {
        if (mine < published.current) return;
        published.current = mine;
        setState({ date, pending });
      },
      (error: unknown) => {
        console.error("Error fetching pending frames:", error);
      },
    );
  }, [date, hasVideo, frameCount, decodes]);

  return current;
}

/** The index in `url` if it is a frame of `date`, else null. */
function frameIndex(url: string, date: string): number | null {
  const prefix = frameUrl(date, 0).slice(0, -1);
  if (!url.startsWith(prefix)) return null;
  const index = Number(url.slice(prefix.length));
  return Number.isInteger(index) ? index : null;
}
