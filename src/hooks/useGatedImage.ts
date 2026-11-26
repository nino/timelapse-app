import React from "react";

import { noteFrameShown, timestamp } from "../diagnostics";

/**
 * Show `wanted` in an `<img>` (`undefined` to keep what it shows), but never
 * start loading a frame while another is still loading. Dragging the scrubber across a video day would otherwise
 * queue a decode for every chunk it passes; this way each load that finishes
 * jumps straight to wherever the scrubber is now. While `hold` is set, no new
 * load starts at all (a preview is standing in).
 */
export function useGatedImage(wanted: string | null | undefined, hold: boolean): {
  src: string | null;
  frameFailed: boolean;
  /** The last frame that finished loading successfully. */
  loaded: string | null;
  /** The last frame that finished loading, successfully or not. */
  settled: string | null;
  onLoad: () => void;
  onError: () => void;
} {
  const [state, setState] = React.useState<{
    src: string | null;
    /** Whether `src` is still loading. */
    inFlight: boolean;
    frameFailed: boolean;
    loaded: string | null;
    settled: string | null;
    /** When `src` was asked for (`performance.now()`), for the scrub log. */
    requestedAt: number;
  }>({ src: null, inFlight: false, frameFailed: false, loaded: null, settled: null, requestedAt: 0 });

  // Nothing loading: show the wanted frame. Otherwise the load that finishes
  // brings this round again. `undefined` keeps the frame shown.
  if (wanted !== undefined && !state.inFlight && !hold && wanted !== state.src) {
    setState({ ...state, src: wanted, inFlight: wanted !== null, requestedAt: timestamp() });
  }

  const settle = React.useCallback((failed: boolean): void => {
    setState((s) => ({
      ...s,
      inFlight: false,
      frameFailed: failed,
      settled: s.src,
      loaded: failed ? s.loaded : s.src,
    }));
  }, []);

  return {
    src: state.src,
    frameFailed: state.frameFailed,
    loaded: state.loaded,
    settled: state.settled,
    onLoad: React.useCallback((): void => {
      noteFrameShown(performance.now() - state.requestedAt);
      settle(false);
    }, [settle, state.requestedAt]),
    onError: React.useCallback((): void => settle(true), [settle]),
  };
}
