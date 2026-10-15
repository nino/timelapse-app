import React from "react";

import { getDay, getDayStatuses, listDays, type Day, type DayStatus } from "../frames";
import { timelapseRoot } from "../timelapseRoot";
import { useDirectoryChanges, useLatestLoad } from "./useDirectoryChanges";

export function ensureError(val: unknown): Error {
  if (val instanceof Error) {
    return val;
  }
  return new Error(String(val));
}

/** Every day with frames, oldest first, kept current. */
export function useDays(): {
  days: Array<string>;
  daysError: Error | null;
} {
  const [days, setDays] = React.useState<Array<string>>([]);
  const [daysError, setDaysError] = React.useState<Error | null>(null);

  const load = React.useCallback(
    async (isCurrent: () => boolean): Promise<void> => {
      try {
        const list = await listDays();
        if (!isCurrent()) return;
        setDays((previous) => (sameList(previous, list) ? previous : list));
        setDaysError(null);
      } catch (error) {
        if (isCurrent()) setDaysError(ensureError(error));
      }
    },
    [],
  );

  const reload = useLatestLoad(load);
  // New day folders and new videos both land in the root.
  useDirectoryChanges(timelapseRoot(), reload);

  return { days, daysError };
}

/** How often the day picker's marks are asked for again while it is open. */
const DAY_STATUS_INTERVAL_MS = 5000;

/**
 * Each day's OCR and video status by date, while `open` (the day picker is
 * open): asked for on opening and every few seconds after. Not on library
 * changes, which come every second while capturing. Kept from the last
 * answer while closed, after a failure, and when nothing changed, so the
 * options don't re-render.
 */
export function useDayStatuses(open: boolean): Map<string, DayStatus> {
  const [statuses, setStatuses] = React.useState<Map<string, DayStatus>>(() => new Map());

  React.useEffect(() => {
    if (!open) return;
    let current = true;
    const load = async (): Promise<void> => {
      try {
        const list = await getDayStatuses();
        if (!current) return;
        setStatuses((previous) => (sameStatuses(previous, list) ? previous : byDate(list)));
      } catch (error) {
        console.warn("Could not read day statuses:", error);
      }
    };
    void load();
    const timer = setInterval(() => void load(), DAY_STATUS_INTERVAL_MS);
    return (): void => {
      current = false;
      clearInterval(timer);
    };
  }, [open]);

  return statuses;
}

function byDate(list: Array<DayStatus>): Map<string, DayStatus> {
  return new Map(list.map((status): [string, DayStatus] => [status.date, status]));
}

function sameStatuses(previous: Map<string, DayStatus>, list: Array<DayStatus>): boolean {
  return (
    previous.size === list.length &&
    list.every((status) => {
      const old = previous.get(status.date);
      return old?.ocrDone === status.ocrDone && old.videoDone === status.videoDone;
    })
  );
}

/**
 * The selected day's frame count and source, kept current while it is being
 * captured. `day` is null while a newly selected date loads, never the
 * previous date's answer.
 */
export function useDay(date: string | null): {
  day: Day | null;
  dayError: Error | null;
} {
  const [state, setState] = React.useState<{
    date: string | null;
    day: Day | null;
    error: Error | null;
  }>({ date: null, day: null, error: null });

  const load = React.useCallback(
    async (isCurrent: () => boolean): Promise<void> => {
      if (!date) return;
      try {
        const day = await getDay(date);
        if (!isCurrent()) return;
        setState((previous) =>
          previous.date === date &&
          previous.day?.frameCount === day.frameCount &&
          previous.day?.source === day.source
            ? previous
            : { date, day, error: null },
        );
      } catch (error) {
        if (isCurrent()) setState({ date, day: null, error: ensureError(error) });
      }
    },
    [date],
  );

  const reload = useLatestLoad(load);
  // Screenshots arrive in the day folder; videos may land in either place.
  useDirectoryChanges(date ? `${timelapseRoot()}/${date}` : null, reload);
  useDirectoryChanges(date ? timelapseRoot() : null, reload);

  const current = state.date === date && date !== null;
  return {
    day: current ? state.day : null,
    dayError: current ? state.error : null,
  };
}

function sameList(a: Array<string>, b: Array<string>): boolean {
  return a.length === b.length && a.every((name, i) => name === b[i]);
}
