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
 * late" rather than "stale until restart". Every hook watching the same path
 * shares one watcher (see `subscribe`).
 */
export function useDirectoryChanges(
  path: string | null,
  onChange: () => void,
): void {
  const onChangeEvent = React.useEffectEvent(onChange);

  React.useEffect(() => {
    if (!path) {
      return;
    }
    return subscribe(path, () => onChangeEvent());
  }, [path]);
}

/** One watcher (or, failing that, one poll timer) and who is listening to it. */
type Shared = { listeners: Set<() => void>; stop: () => void };

/**
 * The watchers in use, by path. The library root alone is watched by the day
 * list, the open day and the OCR version; with one watcher each, every capture
 * would cross from Rust three times.
 */
const shared = new Map<string, Shared>();

/**
 * Call `listener` on every change to `path`, starting its watcher if no one
 * else is watching it. Returns the unsubscribe function, which stops the
 * watcher once its last listener has gone.
 */
function subscribe(path: string, listener: () => void): () => void {
  let entry = shared.get(path);
  if (!entry) {
    entry = start(path);
    shared.set(path, entry);
  }
  const current = entry;
  current.listeners.add(listener);
  return (): void => {
    current.listeners.delete(listener);
    if (current.listeners.size === 0 && shared.get(path) === current) {
      shared.delete(path);
      current.stop();
    }
  };
}

function start(path: string): Shared {
  const listeners = new Set<() => void>();
  // A copy, so a listener that unsubscribes on the way doesn't skip another.
  const notify = (): void => [...listeners].forEach((listener) => listener());

  let disposed = false;
  let unwatch: UnwatchFn | null = null;
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  watch(path, notify, {
    baseDir: BaseDirectory.Home,
    delayMs: WATCH_DEBOUNCE_MS,
  }).then(
    (stop) => {
      // Everyone may have stopped listening while the watcher was being set up.
      if (disposed) {
        stop();
      } else {
        unwatch = stop;
      }
    },
    (error: unknown) => {
      if (disposed) return;
      console.warn(`Could not watch ${path}, polling instead:`, error);
      pollTimer = setInterval(notify, POLL_INTERVAL_MS);
    },
  );

  return {
    listeners,
    stop: (): void => {
      disposed = true;
      unwatch?.();
      if (pollTimer) clearInterval(pollTimer);
    },
  };
}
