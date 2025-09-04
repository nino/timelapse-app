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
      const entries = await readDir(`Timelapse/${folder}`, {
        baseDir: BaseDirectory.Home,
      });
      const fileList = (entries)
        .filter((entry) => entry.isFile)
        .map((entry) => entry.name)
        .sort();
      setFiles(fileList);
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
        .sort()
        .reverse(); // Most recent first
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
