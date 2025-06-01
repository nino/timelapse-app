import { BaseDirectory } from "@tauri-apps/api/path";
import { readDir } from "@tauri-apps/plugin-fs";
import { useCallback, useEffect, useState } from "react";

function ensureError(val: unknown): Error {
  if (val instanceof Error) {
    return val;
  }
  return new Error(String(val));
}

export function useFolders(): {
  folders: Array<string>;
  foldersError: Error | null;
} {
  const [folders, setFolders] = useState<Array<string>>([]);
  const [foldersError, setFoldersError] = useState<Error | null>(null);

  const loadFolders = useCallback(async (): Promise<void> => {
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

  useEffect(() => {
    loadFolders();
  }, [loadFolders]);

  return { folders, foldersError };
}

export function useFiles(folder: string | null): {
  files: Array<string>;
  filesError: Error | null;
} {
  const [files, setFiles] = useState<Array<string>>([]);
  const [filesError, setFilesError] = useState<Error | null>(null);

  const loadFiles = useCallback(async (): Promise<void> => {
    try {
      setFilesError(null);
      if (!folder) {
        setFiles([]);
        return;
      }
      const entries = await readDir(`Timelapse/${folder}`, {
        baseDir: BaseDirectory.Home,
      });
      const fileList = Iterator.from(entries)
        .filter((entry) => entry.isFile)
        .map((entry) => entry.name)
        .toArray()
        .sort();
      setFiles(fileList);
    } catch (error) {
      setFilesError(ensureError(error));
    }
  }, [folder]);

  useEffect(() => {
    loadFiles();
  }, [loadFiles]);

  return { files, filesError };
}
