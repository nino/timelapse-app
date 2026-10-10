import { invoke } from "@tauri-apps/api/core";

/** Label of the Activity window. Mirrors `ACTIVITY_WINDOW` in Rust. */
export const ACTIVITY_WINDOW = "activity";

/** What a background loop is doing. Mirrors `State` in `activity.rs`. */
export type WorkState = "starting" | "working" | "idle" | "resting" | "onBattery" | "lowPower" | "unavailable" | "retrying";

/** Times are RFC 3339 with the local offset. */
export type FrameRef = { day: string; number: number; at: string };
export type Failure = { at: string; message: string };

/** A batch being encoded. Mirrors `Encoding` in `activity.rs`. */
export type Encoding = {
  video: string;
  day: string;
  hour: number;
  frames: number;
  startedAt: string;
  /** Frames ffmpeg has encoded so far. */
  framesDone: number;
  /** When the encode was paused (unplugged), while it is. */
  pausedSince: string | null;
  /** Seconds spent paused before `pausedSince`. */
  pausedSecs: number;
};

/** A boost in progress. Mirrors `BoostStatus` in `boost.rs`. */
export type Boost = {
  until: string;
  /** Whether conversion and OCR also run on battery. */
  allowBattery: boolean;
};

/** Mirrors `Snapshot` in `activity.rs`. */
export type Activity = {
  onAcPower: boolean | null;
  boost: Boost | null;
  /** When low-power mode ends, while it is on. */
  lowPowerUntil: string | null;
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
    current: Encoding | null;
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
    /** Frames passed over unread since launch. */
    failed: number;
    nextCheckAt: string | null;
    lastError: Failure | null;
    /** The current pass reads days that only exist as video; `current.number` is then a position in the day's videos. */
    readingVideo: boolean;
    /** Video-only days still to read, as of the last pass over them; null until OCR first gets to them. */
    videoDaysLeft: number | null;
  };
};

export function getActivity(): Promise<Activity> {
  return invoke<Activity>("get_activity");
}

/** Which "Last error" line. Mirrors `ErrorSource` in `activity.rs`. */
export type ErrorSource = "capture" | "ocr";

/** Clear the error from `source` reported at `at`; a newer one stays. */
export function dismissError(source: ErrorSource, at: string): Promise<void> {
  return invoke<void>("dismiss_error", { source, at });
}

/** Run conversion and OCR at full speed for `minutes`, replacing any boost in progress. */
export function startBoost(minutes: number, allowBattery: boolean): Promise<Boost | null> {
  return invoke<Boost | null>("start_boost", { minutes, allowBattery });
}

export function stopBoost(): Promise<void> {
  return invoke<void>("stop_boost");
}

/** Keep conversion and OCR off for `minutes`, even on AC power, replacing any boost in progress. */
export function startLowPower(minutes: number): Promise<string | null> {
  return invoke<string | null>("start_low_power", { minutes });
}

export function stopLowPower(): Promise<void> {
  return invoke<void>("stop_low_power");
}

/** Let the boost in progress run on battery, or not. */
export function setBoostAllowBattery(allowBattery: boolean): Promise<Boost | null> {
  return invoke<Boost | null>("set_boost_allow_battery", { allowBattery });
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

/** Seconds an encode has spent encoding, as of `now`: time paused doesn't count. */
export function encodeSeconds(encoding: Encoding, now: number): number {
  const pausedNow = encoding.pausedSince ? (now - Date.parse(encoding.pausedSince)) / 1000 : 0;
  return (now - Date.parse(encoding.startedAt)) / 1000 - encoding.pausedSecs - pausedNow;
}

/**
 * How far along an encode is, as a whole percentage, and an estimate of the
 * seconds left at the rate so far (null until there is a rate to go by, and
 * while the encode is paused).
 */
export function encodeProgress(
  encoding: Encoding,
  now: number,
): { percent: number; secondsLeft: number | null } {
  const { frames, framesDone } = encoding;
  const percent = frames > 0 ? Math.floor((framesDone / frames) * 100) : 0;
  const elapsed = encodeSeconds(encoding, now);
  const secondsLeft =
    framesDone > 0 && elapsed >= 10 && !encoding.pausedSince
      ? ((frames - framesDone) * elapsed) / framesDone
      : null;
  return { percent, secondsLeft };
}

/** "3 frames", "1 frame". */
export function count(n: number, noun: string): string {
  return `${n.toLocaleString("en-US")} ${noun}${n === 1 ? "" : "s"}`;
}
