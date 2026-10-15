import React from "react";

import { getDayApps, type DayApps } from "../apps";
import { useAnswer } from "./useOcrSearch";

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
  const { value, error } = useAnswer(
    date !== null && frameCount > 0 ? date : null,
    active ? changes : "",
    (): Promise<DayApps> => getDayApps(date ?? ""),
  );
  React.useEffect(() => {
    if (error) console.warn("Could not find the day's apps:", error);
  }, [error]);
  return value;
}

/** `value`, changing at most once every `intervalMs`, to its latest. */
function useThrottled<T>(value: T, intervalMs: number): T {
  const [shown, setShown] = React.useState(value);
  const shownAt = React.useRef(0);
  React.useEffect(() => {
    if (Object.is(value, shown)) return;
    const wait = Math.max(0, shownAt.current + intervalMs - Date.now());
    const timer = setTimeout((): void => {
      shownAt.current = Date.now();
      setShown(value);
    }, wait);
    return (): void => clearTimeout(timer);
  }, [value, shown, intervalMs]);
  return shown;
}
