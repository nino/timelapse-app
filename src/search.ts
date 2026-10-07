import { invoke } from "@tauri-apps/api/core";

/** A line of text OCR read, normalized to the frame, origin top-left. */
export type LineBox = {
  x: number;
  y: number;
  width: number;
  height: number;
};

/** Mirrors `DayMatch` in Rust: one OCR'd frame of the day whose text matched. */
export type DayMatch = {
  index: number;
  /** One past the last frame this OCR'd frame's text stands for. */
  endIndex: number;
  /** The lines of the frame holding a search word. */
  lines: Array<LineBox>;
};

/** Mirrors `DayCount` in Rust. */
export type DayCount = {
  day: string;
  count: number;
};

/**
 * A stretch of the day where the text stayed on screen: back-to-back matches
 * merged, so stepping through matches doesn't stop on every frame where some
 * other part of the screen changed.
 */
export type Stop = {
  index: number;
  endIndex: number;
};

export function searchDay(date: string, query: string): Promise<Array<DayMatch>> {
  return invoke<Array<DayMatch>>("search_ocr_day", { date, query });
}

export function countMatches(query: string): Promise<Array<DayCount>> {
  return invoke<Array<DayCount>>("count_ocr_matches", { query });
}

/** `matches` (in index order) merged into the stretches they cover. */
export function toStops(matches: Array<DayMatch>): Array<Stop> {
  const stops: Array<Stop> = [];
  for (const match of matches) {
    const last = stops[stops.length - 1];
    if (last && match.index <= last.endIndex) {
      last.endIndex = Math.max(last.endIndex, match.endIndex);
    } else {
      stops.push({ index: match.index, endIndex: match.endIndex });
    }
  }
  return stops;
}

/** Position in `ranges` of the one covering frame `index`, or -1. */
export function rangeAt(ranges: Array<Stop>, index: number): number {
  return ranges.findIndex((range) => range.index <= index && index < range.endIndex);
}

/** The stop after frame `index`, wrapping round to the first. */
export function nextStop(stops: Array<Stop>, index: number): Stop | null {
  return stops.find((stop) => stop.index > index) ?? stops[0] ?? null;
}

/**
 * The stop before the one frame `index` is in (or before `index`, if it is in
 * none), wrapping round to the last.
 */
export function previousStop(stops: Array<Stop>, index: number): Stop | null {
  const current = rangeAt(stops, index);
  const from = current === -1 ? index : stops[current].index;
  const before = stops.filter((stop) => stop.index < from);
  return before[before.length - 1] ?? stops[stops.length - 1] ?? null;
}
