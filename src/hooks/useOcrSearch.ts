import React from "react";

import { countMatches, searchDay, type DayCount, type DayMatch } from "../search";
import { timelapseRoot } from "../timelapseRoot";
import { useDirectoryChanges, useLatestLoad } from "./useDirectoryChanges";

type Answer<T> = { key: string; value: T | null; error: Error | null };

/**
 * Run `ask` for `key` and keep its answer current. OCR writes to
 * `screenshots.db` in the library root, so a change there re-asks. Returns
 * null while a new key loads, never the previous key's answer; reloads that
 * find nothing new keep the old value.
 */
function useLiveAnswer<T>(
  key: string | null,
  ask: () => Promise<T>,
): { value: T | null; error: Error | null } {
  const [answer, setAnswer] = React.useState<Answer<T>>({ key: "", value: null, error: null });

  const load = React.useCallback(
    async (isCurrent: () => boolean): Promise<void> => {
      if (key === null) return;
      try {
        const value = await ask();
        if (!isCurrent()) return;
        setAnswer((previous) =>
          previous.key === key && JSON.stringify(previous.value) === JSON.stringify(value)
            ? previous
            : { key, value, error: null },
        );
      } catch (error) {
        if (isCurrent()) {
          setAnswer({
            key,
            value: null,
            error: error instanceof Error ? error : new Error(String(error)),
          });
        }
      }
    },
    // `ask` is rebuilt every render; `key` says everything it depends on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key],
  );

  const reload = useLatestLoad(load);
  useDirectoryChanges(key === null ? null : timelapseRoot(), reload);

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
): { matches: Array<DayMatch> | null; matchesError: Error | null } {
  const blank = query.trim() === "";
  const { value, error } = useLiveAnswer(
    date === null || blank ? null : JSON.stringify([date, query]),
    () => searchDay(date ?? "", query),
  );
  return { matches: blank ? [] : value, matchesError: error };
}

/** Days where `query` was on screen, newest first. Empty for a blank query. */
export function useMatchCounts(query: string): Array<DayCount> {
  const blank = query.trim() === "";
  const { value } = useLiveAnswer(blank ? null : query, () => countMatches(query));
  return blank ? [] : (value ?? []);
}
