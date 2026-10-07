import React from "react";

import {
  countMatches,
  getMatchLines,
  getOcrVersion,
  searchDay,
  type DayCount,
  type DayMatch,
  type LineBox,
} from "../search";
import { timelapseRoot } from "../timelapseRoot";
import { useDirectoryChanges, useLatestLoad } from "./useDirectoryChanges";
import { ensureError } from "./useLibrary";

/**
 * A token that changes whenever OCR records something a search could find,
 * or null until it is first known. OCR writes to `screenshots.db` in the
 * library root, so a change there re-asks; captures change the root every
 * second too, but leave the token alone, so searches don't re-run for them.
 */
export function useOcrVersion(): string | null {
  const [version, setVersion] = React.useState<string | null>(null);

  const load = React.useCallback(async (isCurrent: () => boolean): Promise<void> => {
    try {
      const next = await getOcrVersion();
      if (isCurrent()) setVersion(next);
    } catch (error) {
      console.warn("Could not check for new OCR text:", error);
      // Searching still works without the token; it just won't re-run.
      if (isCurrent()) setVersion((previous) => previous ?? "unknown");
    }
  }, []);

  const reload = useLatestLoad(load);
  useDirectoryChanges(timelapseRoot(), reload);
  return version;
}

type Answer<T> = { key: string; value: T | null; error: Error | null };

/**
 * Run `ask` for `key`, and again whenever `version` changes. Returns null
 * while a new key loads, never the previous key's answer. A repeat that finds
 * nothing new keeps the old value, and one that fails keeps the last good
 * answer rather than wiping it.
 */
function useAnswer<T>(
  key: string | null,
  version: string | null,
  ask: () => Promise<T>,
): { value: T | null; error: Error | null } {
  const [answer, setAnswer] = React.useState<Answer<T>>({ key: "", value: null, error: null });

  const load = React.useCallback(
    async (isCurrent: () => boolean): Promise<void> => {
      if (key === null || version === null) return;
      try {
        const value = await ask();
        if (!isCurrent()) return;
        setAnswer((previous) =>
          previous.key === key && JSON.stringify(previous.value) === JSON.stringify(value)
            ? previous
            : { key, value, error: null },
        );
      } catch (error) {
        if (!isCurrent()) return;
        setAnswer((previous) =>
          previous.key === key && previous.value !== null
            ? previous
            : { key, value: null, error: ensureError(error) },
        );
      }
    },
    // `ask` is rebuilt every render; `key` says everything it depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key, version],
  );
  useLatestLoad(load);

  const current = key !== null && answer.key === key;
  return {
    value: current ? answer.value : null,
    error: current ? answer.error : null,
  };
}

/**
 * Where `query` was on screen on `date`, in index order. Empty for a blank
 * query; null while a new date or query loads.
 */
export function useDayMatches(
  date: string | null,
  query: string,
  version: string | null,
): { matches: Array<DayMatch> | null; matchesError: Error | null } {
  const blank = query.trim() === "";
  const { value, error } = useAnswer(
    date === null || blank ? null : JSON.stringify([date, query]),
    version,
    () => searchDay(date ?? "", query),
  );
  return { matches: blank ? [] : value, matchesError: error };
}

/** Days where `query` was on screen, newest first. Empty for a blank query. */
export function useMatchCounts(query: string, version: string | null): Array<DayCount> {
  const blank = query.trim() === "";
  const { value } = useAnswer(blank ? null : query, version, () => countMatches(query));
  return blank ? [] : (value ?? []);
}

/**
 * The lines of OCR'd frame `frame` of `date` holding a word of `query`, or
 * null while they load (or when there is no frame).
 */
export function useMatchLines(
  date: string | null,
  frame: number | null,
  query: string,
): Array<LineBox> | null {
  const { value } = useAnswer(
    date === null || frame === null || query.trim() === ""
      ? null
      : JSON.stringify([date, frame, query]),
    // An OCR'd frame's lines never change.
    "",
    () => getMatchLines(date ?? "", frame ?? 0, query),
  );
  return value;
}
