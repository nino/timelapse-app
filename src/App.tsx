import { invoke } from "@tauri-apps/api/core";
import { BaseDirectory, homeDir, join } from "@tauri-apps/api/path";
import { readDir } from "@tauri-apps/plugin-fs";
import { ReactNode, useState } from "react";
import { FixedSizeList } from "react-window";

import "./App.css";
import { useFiles, useFolders } from "./hooks/useFolders";

function ensureError(val: unknown): Error {
  if (val instanceof Error) {
    return val;
  }
  return new Error(String(val));
}

export function App(): ReactNode {
  const { folders, foldersError } = useFolders();
  const [selectedFolder, setSelectedFolder] = useState<string | null>(null);
  const { files, filesError } = useFiles(selectedFolder);
  // async function loadImages(folder: string) {
  //   try {
  //     const entries = await readDir(folder.path, {
  //       baseDir: BaseDirectory.Home,
  //     });

  //     const imageList = entries
  //       .filter(
  //         (entry) =>
  //           entry.name.toLowerCase().endsWith(".jpg") ||
  //           entry.name.toLowerCase().endsWith(".jpeg") ||
  //           entry.name.toLowerCase().endsWith(".png"),
  //       )
  //       .map((entry) => {
  //         return {
  //           name: entry.name,
  //           path: "",
  //         };
  //       });

  //     console.log(imageList);

  //     setFiles(imageList);
  //   } catch (error) {
  //     console.error("Failed to load images:", error);
  //   }
  // }
  return (
    <main className="grid grid-cols-3 gap-2 m-2">
      <div className="rounded bg-gray-300 p-2">
        {folders.map((folder) => (
          <div key={folder}>
            <button onClick={() => setSelectedFolder(folder)}>{folder}</button>
          </div>
        ))}
      </div>
      <div className="rounded bg-gray-300 p-2">
        <FixedSizeList
          height={300}
          width={200}
          itemSize={20}
          itemCount={files.length}
          overscanCount={300}
        >
          {({ index, style }) => <div style={style}>{files[index]}</div>}
        </FixedSizeList>
      </div>
      <div className="rounded bg-gray-300 p-2">3</div>
    </main>
  );
}
