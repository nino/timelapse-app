import React from "react";

import { getDayApps, type DayApps } from "../apps";
import { useAnswer } from "./useOcrSearch";
import { useTimeout } from "./useTimeout";

// While an app is picked, its stretches are re-asked at most this often, as
// new captures come in or OCR reads older frames' menu bars.
const REFRESH_MS = 30_000;

/**
 * Which app was in front across `date`, or null while a new date loads (or
 * before the day's frames are known). Asked once per day, and refreshed only
 * while `active` (an app is picked), since working it out reads the whole day.
 */
export function useDayApps(
  date: string | null,
  frameCount: number,
  ocrVersion: string | null,
  active: boolean,
): DayApps | null {
  const changes = useThrottled(`${ocrVersion}:${frameCount}`, REFRESH_MS);
  const { value } = useAnswer(
    date !== null && frameCount > 0 ? date : null,
    active ? changes : "",
    (): Promise<DayApps> => getDayApps(date ?? ""),
    (error) => console.warn("Could not find the day's apps:", error),
  );
  return value;
}

/**
 * `value`, changing at most once every `intervalMs`, to its latest. The first
 * change after a quiet spell shows straight away.
 */
function useThrottled(value: string, intervalMs: number): string {
  const [shown, setShown] = React.useState(value);
  // Whether `shown` changed less than `intervalMs` ago.
  const [cooling, setCooling] = React.useState(false);
  if (value !== shown && !cooling) {
    setShown(value);
    setCooling(true);
  }
  useTimeout(() => setCooling(false), cooling ? intervalMs : null, shown);
  return shown;
}
