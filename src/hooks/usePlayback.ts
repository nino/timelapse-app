import React from "react";

import { nextInRanges } from "../apps";
import type { FrameRange } from "../frames";
import { useEventListener } from "./useEventListener";
import { useTimeout } from "./useTimeout";

/**
 * Play the selected day forward at `frameMs` a frame, one frame
 * after the other, until its last frame. Space toggles it too. Switching
 * days or hiding the window pauses; scrubbing while playing carries on from
 * wherever the scrubber was moved to. With `ranges`, only the frames in them
 * play: playback jumps over the gaps and stops after the last range.
 */
export function usePlayback({
  frameMs,
  selectedDay,
  frameCount,
  currentIndex,
  ranges,
  frameShown,
  goTo,
}: {
  frameMs: number;
  selectedDay: string | null;
  frameCount: number;
  currentIndex: number;
  /** The only frames to play, in order, end exclusive; null for all of them. */
  ranges: Array<FrameRange> | null;
  /** Whether the frame at `currentIndex` has finished loading. */
  frameShown: boolean;
  goTo: (index: number) => void;
}): { playing: boolean; togglePlaying: () => void } {
  // The day being played, so that opening another day stops playback.
  const [playingDay, setPlayingDay] = React.useState<string | null>(null);
  // The frame playback shows after `index`, or null at the end.
  const nextFrame = React.useCallback(
    (index: number): number | null => {
      const next = ranges === null ? index + 1 : nextInRanges(ranges, index + 1);
      return next !== null && next < frameCount ? next : null;
    },
    [ranges, frameCount],
  );
  const next = nextFrame(currentIndex);
  const atEnd = next === null;
  const playing = playingDay !== null && playingDay === selectedDay && !atEnd;
  // Each step waits for `frameMs` to pass since the last one (`due`) and for
  // its frame to be on screen, whichever comes last, so slow loads use up the
  // wait instead of adding to it.
  const [steps, setSteps] = React.useState(0);
  const [due, setDue] = React.useState(false);
  const step = (to: number): void => {
    setDue(false);
    setSteps((n) => n + 1);
    goTo(to);
    if (nextFrame(to) === null) setPlayingDay(null);
  };
  useTimeout(
    () => {
      if (frameShown && next !== null) step(next);
      else setDue(true);
    },
    playing && !due ? frameMs : null,
    steps,
  );
  // Scrubbed past the end (or the filter's last stretch): stop for good, so
  // nothing starts playing by itself if the end moves later on. Set during
  // render, as React allows for state derived from earlier renders.
  if (playingDay !== null && atEnd) setPlayingDay(null);
  // The wait is over and the frame has just arrived.
  if (playing && due && frameShown && next !== null) step(next);

  const togglePlaying = (): void => {
    if (playing) {
      setPlayingDay(null);
      return;
    }
    if (selectedDay === null || frameCount === 0) return;
    // Playing from the end starts the day (or the app's first stretch) over.
    if (atEnd) {
      const first = ranges === null ? 0 : ranges[0]?.start;
      if (first === undefined || first >= frameCount) return;
      goTo(first);
    }
    // The first frame shows as soon as it has loaded.
    setDue(true);
    setPlayingDay(selectedDay);
  };

  // Nobody is watching a hidden or minimised window: stop rather than keep
  // loading frames.
  useEventListener(
    "visibilitychange",
    () => {
      if (document.hidden) setPlayingDay(null);
    },
    "document",
  );

  // Space plays and pauses, except where it types or presses a button.
  useEventListener("keydown", (e) => {
    if (e.key !== " " || e.metaKey || e.ctrlKey || e.altKey) return;
    if (e.target instanceof Element) {
      if (e.target.closest('button, [role="listbox"], [aria-haspopup="listbox"], textarea')) return;
      if (e.target instanceof HTMLInputElement && e.target.type !== "range") return;
    }
    e.preventDefault();
    togglePlaying();
  });

  return { playing, togglePlaying };
}
