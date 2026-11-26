import React from "react";

/**
 * Run `load` now, again whenever `load` changes, and whenever the returned
 * function is called, but only let the most recent call publish its result.
 * Without this a slow answer for a day the user has already left can resolve
 * last and overwrite the current day.
 *
 * With `oneAtATime`, a call made while a load is still running waits for it,
 * and of the calls that waited only the latest runs. Use it for loads asked
 * for at every scrub step: otherwise each step starts one, and when they are
 * slower than the steps they pile up in the backend, holding up everything
 * behind them long after the scrubbing stops.
 */
export function useLatestLoad(
  load: (isCurrent: () => boolean) => Promise<void>,
  { oneAtATime = false }: { oneAtATime?: boolean } = {},
): () => void {
  const generation = React.useRef(0);
  const running = React.useRef(false);
  const waiting = React.useRef<(() => void) | null>(null);

  const run = React.useCallback((): void => {
    const mine = ++generation.current;
    const start = (): void => {
      running.current = true;
      void load(() => mine === generation.current).finally((): void => {
        running.current = false;
        const next = waiting.current;
        waiting.current = null;
        next?.();
      });
    };
    if (oneAtATime && running.current) {
      waiting.current = start;
    } else {
      start();
    }
  }, [load, oneAtATime]);

  React.useEffect(() => {
    const counter = generation;
    run();
    return (): void => {
      // Invalidate anything still in flight when `load` changes or on unmount.
      counter.current++;
      waiting.current = null;
    };
  }, [run]);

  return run;
}
