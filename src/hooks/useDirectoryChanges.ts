import { BaseDirectory } from "@tauri-apps/api/path";
import { watch, type UnwatchFn } from "@tauri-apps/plugin-fs";
import React from "react";

// The capture loop writes a file a second, so this mostly sets how often the
// viewer re-asks about today while it is being filled.
const WATCH_DEBOUNCE_MS = 500;
// Only used when the watcher cannot be set up (e.g. a missing permission).
const POLL_INTERVAL_MS = 3000;

/**
 * Call `onChange` whenever the contents of `path` (relative to `$HOME`) change.
 *
 * Uses the fs plugin's debounced watcher, and falls back to polling if the
 * watcher can't be created, so a missing permission degrades to "slightly
 * late" rather than "stale until restart".
 */
export function useDirectoryChanges(
  path: string | null,
  onChange: () => void,
): void {
  const onChangeRef = React.useRef(onChange);
  React.useEffect(() => {
    onChangeRef.current = onChange;
  }, [onChange]);

  React.useEffect(() => {
    if (!path) {
      return;
    }

    let disposed = false;
    let unwatch: UnwatchFn | null = null;
    let pollTimer: ReturnType<typeof setInterval> | null = null;

    watch(path, () => onChangeRef.current(), {
      baseDir: BaseDirectory.Home,
      delayMs: WATCH_DEBOUNCE_MS,
    }).then(
      (stop) => {
        // The component may have unmounted while the watcher was being set up.
        if (disposed) {
          stop();
        } else {
          unwatch = stop;
        }
      },
      (error: unknown) => {
        if (disposed) return;
        console.warn(`Could not watch ${path}, polling instead:`, error);
        pollTimer = setInterval(() => onChangeRef.current(), POLL_INTERVAL_MS);
      },
    );

    return (): void => {
      disposed = true;
      unwatch?.();
      if (pollTimer) clearInterval(pollTimer);
    };
  }, [path]);
}

/**
 * Run `load` now, again whenever `load` changes, and whenever the returned
 * function is called, but only let the most recent call publish its result.
 * Without this a slow answer for a day the user has already left can resolve
 * last and overwrite the current day.
 */
export function useLatestLoad(
  load: (isCurrent: () => boolean) => Promise<void>,
): () => void {
  const generation = React.useRef(0);

  const run = React.useCallback((): void => {
    const mine = ++generation.current;
    void load(() => mine === generation.current);
  }, [load]);

  React.useEffect(() => {
    const counter = generation;
    run();
    return (): void => {
      // Invalidate anything still in flight when `load` changes or on unmount.
      counter.current++;
    };
  }, [run]);

  return run;
}
