import { BaseDirectory } from "@tauri-apps/api/path";
import { readDir } from "@tauri-apps/plugin-fs";
import React from "react";

function ensureError(val: unknown): Error {
  if (val instanceof Error) {
    return val;
  }
  return new Error(String(val));
}

export function useFolders(): {
  folders: Array<string>;
  foldersError: Error | null;
  refreshFolders: () => void;
} {
  const [folders, setFolders] = React.useState<Array<string>>([]);
  const [foldersError, setFoldersError] = React.useState<Error | null>(null);

  const loadFolders = React.useCallback(async (): Promise<void> => {
    try {
      setFoldersError(null);
      const entries = await readDir("Timelapse", {
        baseDir: BaseDirectory.Home,
      });
      const folderList = entries
        .filter((entry) => entry.isDirectory)
        .filter((entry) => !entry.name.startsWith(".")) // Exclude hidden folders like .cache
        .map((entry) => entry.name);
      setFolders(folderList);
    } catch (error) {
      setFoldersError(ensureError(error));
    }
  }, []);

  React.useEffect(() => {
    loadFolders();
  }, [loadFolders]);

  return { folders, foldersError, refreshFolders: loadFolders };
}

export function useFiles(folder: string | null): {
  files: Array<string>;
  filesError: Error | null;
} {
  const [files, setFiles] = React.useState<Array<string>>([]);
  const [filesError, setFilesError] = React.useState<Error | null>(null);

  const loadFiles = React.useCallback(async (): Promise<void> => {
    try {
      setFilesError(null);
      if (!folder) {
        setFiles([]);
        return;
      }

      // Retry logic for video cache folders that might be still being created
      const maxRetries = 5;
      const retryDelay = 500; // ms
      let lastError: Error | null = null;

      for (let attempt = 0; attempt < maxRetries; attempt++) {
        try {
          const entries = await readDir(`Timelapse/${folder}`, {
            baseDir: BaseDirectory.Home,
          });
          const fileList = (entries)
            .filter((entry) => entry.isFile)
            .map((entry) => entry.name)
            .sort();

          // If we got files, or if this is not a cache folder, accept the result
          if (fileList.length > 0 || !folder.startsWith(".cache/")) {
            setFiles(fileList);
            return;
          }

          // For cache folders, if no files found, retry after delay
          if (attempt < maxRetries - 1) {
            console.log(`No files found in ${folder}, retrying in ${retryDelay}ms (attempt ${attempt + 1}/${maxRetries})...`);
            await new Promise(resolve => setTimeout(resolve, retryDelay));
          }
        } catch (error) {
          lastError = ensureError(error);
          // If directory doesn't exist yet, wait and retry
          if (attempt < maxRetries - 1) {
            console.log(`Error reading ${folder}, retrying in ${retryDelay}ms (attempt ${attempt + 1}/${maxRetries})...`, error);
            await new Promise(resolve => setTimeout(resolve, retryDelay));
          }
        }
      }

      // If we got here, all retries failed
      if (lastError) {
        throw lastError;
      } else {
        // No error, but no files found after all retries
        setFiles([]);
      }
    } catch (error) {
      setFilesError(ensureError(error));
    }
  }, [folder]);

  React.useEffect(() => {
    loadFiles();
  }, [loadFiles]);

  return { files, filesError };
}

export function useVideos(): {
  videos: Array<string>;
  videosError: Error | null;
  refreshVideos: () => void;
} {
  const [videos, setVideos] = React.useState<Array<string>>([]);
  const [videosError, setVideosError] = React.useState<Error | null>(null);

  const loadVideos = React.useCallback(async (): Promise<void> => {
    try {
      setVideosError(null);
      const entries = await readDir("Timelapse", {
        baseDir: BaseDirectory.Home,
      });
      const videoList = entries
        .filter((entry) => entry.isFile && entry.name.endsWith(".mov"))
        .map((entry) => entry.name)
        .sort(); // Chronological order (oldest first)
      setVideos(videoList);
    } catch (error) {
      setVideosError(ensureError(error));
    }
  }, []);

  React.useEffect(() => {
    loadVideos();
  }, [loadVideos]);

  return { videos, videosError, refreshVideos: loadVideos };
}
