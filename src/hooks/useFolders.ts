import { BaseDirectory } from "@tauri-apps/api/path";
import { readDir, watch, type UnwatchFn } from "@tauri-apps/plugin-fs";
import React from "react";

import { timelapseRoot } from "../timelapseRoot";

// The capture loop writes a file a second, so this mostly sets how often the
// viewer re-reads today's folder while it is being filled.
const WATCH_DEBOUNCE_MS = 500;
// Only used when the watcher cannot be set up (e.g. a missing permission).
const POLL_INTERVAL_MS = 3000;

function ensureError(val: unknown): Error {
  if (val instanceof Error) {
    return val;
  }
  return new Error(String(val));
}

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
 * Run `load` now and again whenever `deps` change, but only let the most recent
 * call publish its result. Without this a slow `readDir` for a folder the user
 * has already left can resolve last and overwrite the current folder's list.
 */
function useLatestLoad(
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

/** Date folders (`YYYY-MM-DD`) in the library root, kept current. */
export function useFolders(): {
  folders: Array<string>;
  foldersError: Error | null;
} {
  const [folders, setFolders] = React.useState<Array<string>>([]);
  const [foldersError, setFoldersError] = React.useState<Error | null>(null);

  const loadFolders = React.useCallback(
    async (isCurrent: () => boolean): Promise<void> => {
      try {
        const entries = await readDir(timelapseRoot(), {
          baseDir: BaseDirectory.Home,
        });
        if (!isCurrent()) return;
        const folderList = entries
          .filter((entry) => entry.isDirectory)
          .filter((entry) => !entry.name.startsWith(".")) // Exclude hidden folders like .cache
          .map((entry) => entry.name);
        setFolders((previous) => (sameList(previous, folderList) ? previous : folderList));
        setFoldersError(null);
      } catch (error) {
        if (isCurrent()) setFoldersError(ensureError(error));
      }
    },
    [],
  );

  const reload = useLatestLoad(loadFolders);
  useDirectoryChanges(timelapseRoot(), reload);

  return { folders, foldersError };
}

/**
 * Files in a date folder or a `.cache/<video>` folder, kept current.
 *
 * `files` is always the listing of the `folder` passed in this render: while a
 * newly selected folder is still loading it is empty rather than the previous
 * folder's list, so callers never index one folder's names into another.
 */
export function useFiles(folder: string | null): {
  files: Array<string>;
  filesError: Error | null;
} {
  const [listing, setListing] = React.useState<{
    folder: string | null;
    files: Array<string>;
  }>({ folder: null, files: [] });
  const [filesError, setFilesError] = React.useState<{
    folder: string | null;
    error: Error;
  } | null>(null);

  const isCacheFolder = folder?.startsWith(".cache/") ?? false;

  const loadFiles = React.useCallback(
    async (isCurrent: () => boolean): Promise<void> => {
      if (!folder) {
        setListing({ folder: null, files: [] });
        setFilesError(null);
        return;
      }

      // ffmpeg writes a cache folder while we are already trying to list it, so
      // for those an empty or missing directory is expected and worth retrying.
      // A date folder gets exactly one attempt: there is nothing to wait for, and
      // retrying would only delay a genuine error by the length of the loop.
      const maxRetries = isCacheFolder ? 5 : 1;
      const retryDelay = 500; // ms
      let lastError: Error | null = null;

      for (let attempt = 0; attempt < maxRetries; attempt++) {
        try {
          const entries = await readDir(`${timelapseRoot()}/${folder}`, {
            baseDir: BaseDirectory.Home,
          });
          if (!isCurrent()) return;
          const fileList = entries
            .filter((entry) => entry.isFile)
            .map((entry) => entry.name)
            .sort();

          // If we got files, or if this is not a cache folder, accept the result
          if (fileList.length > 0 || !isCacheFolder) {
            setListing((previous) =>
              previous.folder === folder && sameList(previous.files, fileList)
                ? previous
                : { folder, files: fileList },
            );
            setFilesError(null);
            return;
          }
          lastError = null;
        } catch (error) {
          if (!isCurrent()) return;
          lastError = ensureError(error);
        }

        if (attempt < maxRetries - 1) {
          await new Promise((resolve) => setTimeout(resolve, retryDelay));
          if (!isCurrent()) return;
        }
      }

      if (lastError) {
        setFilesError({ folder, error: lastError });
      } else {
        // No error, but no files found after all retries
        setListing({ folder, files: [] });
        setFilesError(null);
      }
    },
    [folder, isCacheFolder],
  );

  const reload = useLatestLoad(loadFiles);
  // A published cache folder never changes (see `publish_staged_frames`), so
  // only date folders need watching.
  useDirectoryChanges(
    folder && !isCacheFolder ? `${timelapseRoot()}/${folder}` : null,
    reload,
  );

  return {
    files: listing.folder === folder ? listing.files : EMPTY,
    filesError: filesError?.folder === folder ? filesError.error : null,
  };
}

/** `.mov` files in the library root, oldest first, kept current. */
export function useVideos(): {
  videos: Array<string>;
  videosError: Error | null;
} {
  const [videos, setVideos] = React.useState<Array<string>>([]);
  const [videosError, setVideosError] = React.useState<Error | null>(null);

  const loadVideos = React.useCallback(
    async (isCurrent: () => boolean): Promise<void> => {
      try {
        const entries = await readDir(timelapseRoot(), {
          baseDir: BaseDirectory.Home,
        });
        if (!isCurrent()) return;
        const videoList = entries
          .filter((entry) => entry.isFile && entry.name.endsWith(".mov"))
          .map((entry) => entry.name)
          .sort(); // Chronological order (oldest first)
        setVideos((previous) => (sameList(previous, videoList) ? previous : videoList));
        setVideosError(null);
      } catch (error) {
        if (isCurrent()) setVideosError(ensureError(error));
      }
    },
    [],
  );

  const reload = useLatestLoad(loadVideos);
  useDirectoryChanges(timelapseRoot(), reload);

  return { videos, videosError };
}

const EMPTY: Array<string> = [];

// Re-reads return a fresh array even when nothing changed; keeping the old one
// stops every watcher tick from re-running the effects that depend on it.
function sameList(a: Array<string>, b: Array<string>): boolean {
  return a.length === b.length && a.every((name, i) => name === b[i]);
}
