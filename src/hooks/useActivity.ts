import React from "react";

import { getActivity, type Activity } from "../activity";

/** How often the Activity window asks. The answer is read from memory in Rust. */
export const ACTIVITY_POLL_MS = 1000;

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
  const askNow = React.useRef<() => void>(() => {});

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
    // An ask already under way may have started before the change; the next
    // scheduled one picks it up a second later.
    askNow.current = (): void => {
      if (cancelled || asking) return;
      if (timer !== undefined) clearTimeout(timer);
      ask();
    };

    ask();
    document.addEventListener("visibilitychange", onVisibilityChange);
    return (): void => {
      cancelled = true;
      if (timer !== undefined) clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, []);

  const refresh = React.useCallback((): void => askNow.current(), []);
  return { ...state, refresh };
}
