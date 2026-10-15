import { invoke } from "@tauri-apps/api/core";

import type { FrameRange } from "./frames";

/** Where one app was in front on a day. Mirrors `AppStretches` in Rust. */
export type AppStretches = {
  name: string;
  /** How many frames it was in front for. */
  frames: number;
  /** In order, end exclusive. */
  ranges: Array<FrameRange>;
};

/** Mirrors `DayApps` in Rust. */
export type DayApps = {
  /** The day's frame count when the stretches were found. */
  frameCount: number;
  /** Most-used first. Empty for a day with no app data. */
  apps: Array<AppStretches>;
};

/** Which app was in front across `date`. */
export function getDayApps(date: string): Promise<DayApps> {
  return invoke<DayApps>("get_day_apps", { date });
}

/**
 * The first frame at or after `index` that lies in one of `ranges` (in order,
 * end exclusive), or null if none does.
 */
export function nextInRanges(ranges: Array<FrameRange>, index: number): number | null {
  // The first range that ends after `index`.
  let low = 0;
  let high = ranges.length;
  while (low < high) {
    const mid = (low + high) >> 1;
    if (ranges[mid].end <= index) low = mid + 1;
    else high = mid;
  }
  const range = ranges[low];
  if (!range) return null;
  return Math.max(index, range.start);
}

/**
 * `ranges` with gaps shorter than `minGap` frames closed up, so a scrubber
 * marking them draws a handful of bars rather than one per app switch.
 */
export function mergeCloseRanges(ranges: Array<FrameRange>, minGap: number): Array<FrameRange> {
  const merged: Array<FrameRange> = [];
  for (const range of ranges) {
    const last = merged[merged.length - 1];
    if (last && range.start - last.end < minGap) {
      last.end = Math.max(last.end, range.end);
    } else {
      merged.push({ start: range.start, end: range.end });
    }
  }
  return merged;
}

/** How long `frames` one-a-second captures lasted: "2h 05m", "12m", "<1m". */
export function formatFrames(frames: number): string {
  const minutes = Math.floor(frames / 60);
  if (minutes < 1) return "<1m";
  if (minutes < 60) return `${minutes}m`;
  return `${Math.floor(minutes / 60)}h ${String(minutes % 60).padStart(2, "0")}m`;
}
