import { getDayApps, type DayApps } from "../apps";
import { useAnswer } from "./useOcrSearch";

// While a day is still being captured, its apps are re-asked once this many
// new frames have come in, rather than on every capture.
const REFRESH_EVERY_FRAMES = 30;

/**
 * Which app was in front across `date`, or null while a new date loads.
 * Re-asked when OCR records something (it reads the menu bar of frames taken
 * before window recording) and every `REFRESH_EVERY_FRAMES` captures.
 */
export function useDayApps(
  date: string | null,
  frameCount: number,
  ocrVersion: string | null,
): DayApps | null {
  const { value } = useAnswer(
    date,
    ocrVersion === null ? null : `${ocrVersion}:${Math.floor(frameCount / REFRESH_EVERY_FRAMES)}`,
    () => getDayApps(date ?? ""),
  );
  return value;
}
