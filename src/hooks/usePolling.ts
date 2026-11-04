import React from "react";

/**
 * Calls `ask` on mount and then every `intervalMs` while the page is
 * visible, handing each answer to `onAnswer` and each failure to `onError`.
 * The next ask waits for the last one to settle, so slow answers never pile
 * up, and a page that becomes visible again asks straight away. The latest
 * handlers are always called, so inline functions are fine.
 *
 * Returns a function that asks again now, after a change the page made
 * itself. An ask already under way may have started before that change; the
 * next scheduled one picks it up.
 */
export function usePolling<T>(
  ask: () => Promise<T>,
  intervalMs: number,
  onAnswer: (answer: T) => void,
  onError: (error: unknown) => void,
): () => void {
  const onAsk = React.useEffectEvent(ask);
  const onAnswerEvent = React.useEffectEvent(onAnswer);
  const onErrorEvent = React.useEffectEvent(onError);
  const askNow = React.useRef<() => void>(() => {});

  React.useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let asking = false;

    const run = (): void => {
      timer = undefined;
      asking = true;
      onAsk()
        .then(
          (answer) => {
            if (!cancelled) onAnswerEvent(answer);
          },
          (error: unknown) => {
            if (!cancelled) onErrorEvent(error);
          },
        )
        .finally(() => {
          asking = false;
          schedule();
        });
    };
    const schedule = (): void => {
      if (cancelled || asking || timer !== undefined || document.visibilityState !== "visible") return;
      timer = setTimeout(run, intervalMs);
    };
    const onVisibilityChange = (): void => {
      if (document.visibilityState === "visible" && !asking && timer === undefined) run();
    };
    askNow.current = (): void => {
      if (cancelled || asking) return;
      if (timer !== undefined) clearTimeout(timer);
      run();
    };

    run();
    document.addEventListener("visibilitychange", onVisibilityChange);
    return (): void => {
      cancelled = true;
      if (timer !== undefined) clearTimeout(timer);
      document.removeEventListener("visibilitychange", onVisibilityChange);
    };
  }, [intervalMs]);

  return React.useCallback((): void => askNow.current(), []);
}
