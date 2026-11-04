import React from "react";

import { getDayVideos, type Day, type DayVideos } from "../frames";
import { useLatestLoad } from "./useDirectoryChanges";

/**
 * The stretches of `day` served from video, for the beta video player, or
 * null while they load and for days without video. Asked again whenever the
 * day's frame count changes, since a newly converted hour turns screenshots
 * into video.
 */
export function useDayVideos(day: Day | null): DayVideos | null {
  const [state, setState] = React.useState<{ date: string; videos: DayVideos } | null>(null);
  const date = day?.date ?? null;
  const frameCount = day?.frameCount ?? 0;
  const hasVideo = day?.source === "video" || day?.source === "mixed";

  // Today's frame count changes every second, so an answer slower than that
  // would never count as the latest. Each one is published unless a newer
  // request has already published, like `usePendingFrames`; an answer for
  // another day is filtered out below.
  const asked = React.useRef(0);
  const published = React.useRef(0);
  const load = React.useCallback(
    async (): Promise<void> => {
      if (!date || !hasVideo) return;
      const mine = ++asked.current;
      try {
        const videos = await getDayVideos(date);
        if (mine < published.current) return;
        published.current = mine;
        setState({ date, videos });
      } catch (error) {
        console.error("Error fetching the day's videos:", error);
      }
    },
    // `frameCount` isn't read, but a change means the answer may have changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [date, hasVideo, frameCount],
  );
  useLatestLoad(load);

  return hasVideo && state?.date === date ? state.videos : null;
}
