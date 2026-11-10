import React from "react";

import { getActivity, type Activity } from "../activity";
import { usePolling } from "./usePolling";

/** How often the Activity window asks. The answer is read from memory in Rust. */
const ACTIVITY_POLL_MS = 1000;

/**
 * The latest activity report and when it arrived, asked for every
 * `ACTIVITY_POLL_MS` while the page is visible. Each answer re-renders, so
 * countdowns computed against `now` tick along with it. `refresh` asks again
 * straight away, after a change the window made itself.
 */
export function useActivity(): {
  activity: Activity | null;
  now: number;
  error: string | null;
  refresh: () => void;
} {
  const [state, setState] = React.useState<{
    activity: Activity | null;
    now: number;
    error: string | null;
  }>(() => ({ activity: null, now: Date.now(), error: null }));

  const refresh = usePolling(
    getActivity,
    ACTIVITY_POLL_MS,
    (activity) => setState({ activity, now: Date.now(), error: null }),
    (error) => setState((s) => ({ ...s, now: Date.now(), error: String(error) })),
  );
  return { ...state, refresh };
}
