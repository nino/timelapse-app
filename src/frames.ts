import { invoke } from "@tauri-apps/api/core";

/** What the frame source knows about one day. Mirrors `DaySummary` in Rust. */
export type Day = {
  date: string;
  frameCount: number;
  /** Where the frames come from. Informational only: fetching is the same either way. */
  source: "screenshots" | "video" | "mixed" | "empty";
};

/** Mirrors `FrameTime` in Rust. */
export type FrameTime = {
  /** ISO 8601; naive (local) when estimated. */
  localTime: string;
  /** False when estimated from a video's start time. */
  exact: boolean;
};

export function listDays(): Promise<Array<string>> {
  return invoke<Array<string>>("list_days");
}

export function getDay(date: string): Promise<Day> {
  return invoke<Day>("get_day", { date });
}

export function getFrameTime(
  date: string,
  index: number,
): Promise<FrameTime | null> {
  return invoke<FrameTime | null>("get_frame_time", { date, index });
}

/**
 * URL of frame `index` of `date`, served by the `frames` protocol registered in
 * `src-tauri/src/lib.rs`. Usable directly as an `<img src>`.
 *
 * Custom protocols are `frames://localhost/…` on macOS and Linux but
 * `http://frames.localhost/…` on Windows.
 */
export function frameUrl(date: string, index: number): string {
  const base = navigator.userAgent.includes("Windows")
    ? "http://frames.localhost"
    : "frames://localhost";
  return `${base}/${date}/${index}`;
}
