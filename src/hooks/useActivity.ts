import React from "react";

import { getActivity, type Activity } from "../activity";

/** How often the Activity window asks. The answer is read from memory in Rust. */
export const ACTIVITY_POLL_MS = 1000;

/**
 * The latest activity report and when it arrived, asked for every
 * `ACTIVITY_POLL_MS` while the page is visible. Each answer re-renders, so
 * countdowns computed against `now` tick along with it.
 */
export function useActivity(): {
  activity: Activity | null;
  now: number;
  error: string | null;
} {
  const [state, setState] = React.useState<{
    activity: Activity | null;
    now: number;
    error: string | null;
  }>(() => ({ activity: null, now: Date.now(), error: null }));

  React.useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let asking = false;

    const ask = (): void => {
      timer = undefined;
      asking = true;
      getActivity().then(
        (activity) => {
          if (!cancelled) setState({ activity, now: Date.now(), error: null });
        },
        (error: unknown) => {
          if (!cancelled) setState((s) => ({ ...s, now: Date.now(), error: String(error) }));
        },
      ).finally(() => {
        asking = false;
        schedule();
      });
    };
    // The next ask waits for this one, so slow answers never pile up.
    const schedule = (): void => {
      if (cancelled || asking || timer !== undefined || document.visibilityState !== "visible") return;
      timer = setTimeout(ask, ACTIVITY_POLL_MS);
    };
    const onVisibilityChange = (): void => {
      if (document.visibilityState === "visible" && !asking && timer === undefined) ask();
    };

    ask();
    document.addEventListener("visibilitychange", onVisibilityChange);
    return (): void => {
      cancelled = true;
      if (timer !== undefined) clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, []);

  return state;
}
