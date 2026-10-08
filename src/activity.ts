import { invoke } from "@tauri-apps/api/core";

/** Label of the Activity window. Mirrors `ACTIVITY_WINDOW` in Rust. */
export const ACTIVITY_WINDOW = "activity";

/** What a background loop is doing. Mirrors `State` in `activity.rs`. */
export type WorkState = "starting" | "working" | "idle" | "resting" | "onBattery" | "unavailable";

/** Times are RFC 3339 with the local offset. */
export type FrameRef = { day: string; number: number; at: string };
export type Failure = { at: string; message: string };

/** Mirrors `Snapshot` in `activity.rs`. */
export type Activity = {
  onAcPower: boolean | null;
  capture: {
    running: boolean;
    lastFrame: FrameRef | null;
    framesSaved: number;
    lastBlackAt: string | null;
    lastError: Failure | null;
    /** Failed captures since the last one that worked. */
    failuresInARow: number;
    /** From this many failures in a row on, capture retries once a minute. */
    failuresBeforeBackoff: number;
  };
  conversion: {
    state: WorkState;
    current: { video: string; day: string; hour: number; frames: number; startedAt: string } | null;
    nextCheckAt: string | null;
    ready: number;
    waitingForOcr: number;
    last: { video: string; finishedAt: string; tookSecs: number; error: string | null } | null;
    videosMade: number;
    framesDeleted: number;
  };
  ocr: {
    state: WorkState;
    current: FrameRef | null;
    remaining: number;
    recognized: number;
    skipped: number;
    nextCheckAt: string | null;
    lastError: Failure | null;
  };
};

export function getActivity(): Promise<Activity> {
  return invoke<Activity>("get_activity");
}

/** A length of time, to the second under an hour: "45s", "3m 05s", "2h 14m". */
export function formatDuration(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, "0")}s`;
  return `${Math.floor(s / 3600)}h ${String(Math.floor((s % 3600) / 60)).padStart(2, "0")}m`;
}

/** How long ago `time` was, as of `now` (ms since the epoch). */
export function ago(time: string, now: number): string {
  const seconds = (now - Date.parse(time)) / 1000;
  return seconds < 1 ? "just now" : `${formatDuration(seconds)} ago`;
}

/** How long until `time`, as of `now` (ms since the epoch). */
export function until(time: string, now: number): string {
  const seconds = (Date.parse(time) - now) / 1000;
  return seconds < 1 ? "any moment now" : `in ${formatDuration(seconds)}`;
}

/** "3 frames", "1 frame". */
export function count(n: number, noun: string): string {
  return `${n.toLocaleString("en-US")} ${noun}${n === 1 ? "" : "s"}`;
}
