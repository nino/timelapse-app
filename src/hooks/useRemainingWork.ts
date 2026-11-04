import React from "react";

import { getRemainingWork, type RemainingWork } from "../activity";
import { usePolling } from "./usePolling";

/** How often the Activity window asks what is left. Each answer lists the library. */
const REMAINING_WORK_POLL_MS = 10_000;

/**
 * What OCR and video conversion have left to do, asked for every
 * `REMAINING_WORK_POLL_MS` while the page is visible. `null` until the first
 * answer, and the last answer is kept if a later ask fails.
 */
export function useRemainingWork(): RemainingWork | null {
  const [work, setWork] = React.useState<RemainingWork | null>(null);
  usePolling(getRemainingWork, REMAINING_WORK_POLL_MS, setWork, (error) =>
    console.warn("Could not ask what is left to do:", error),
  );
  return work;
}
