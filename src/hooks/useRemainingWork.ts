import React from "react";

import { getRemainingWork, type RemainingWork } from "../activity";
import { useMountEffect } from "./useMountEffect";

/** How often the Activity window asks what is left. Each answer lists the library. */
export const REMAINING_WORK_POLL_MS = 10_000;

/**
 * What OCR and video conversion have left to do, asked for every
 * `REMAINING_WORK_POLL_MS` while the page is visible. `null` until the first
 * answer, and the last answer is kept if a later ask fails.
 */
export function useRemainingWork(): RemainingWork | null {
  const [work, setWork] = React.useState<RemainingWork | null>(null);

  useMountEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let asking = false;

    const ask = (): void => {
      timer = undefined;
      asking = true;
      getRemainingWork()
        .then(
          (answer) => {
            if (!cancelled) setWork(answer);
          },
          (error: unknown) => console.warn("Could not ask what is left to do:", error),
        )
        .finally(() => {
          asking = false;
          schedule();
        });
    };
    const schedule = (): void => {
      if (cancelled || asking || timer !== undefined || document.visibilityState !== "visible") return;
      timer = setTimeout(ask, REMAINING_WORK_POLL_MS);
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
  });

  return work;
}
