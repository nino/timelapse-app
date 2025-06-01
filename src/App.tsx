import { useVirtualizer } from "@tanstack/react-virtual";
import { invoke } from "@tauri-apps/api/core";
import { BaseDirectory, homeDir, join } from "@tauri-apps/api/path";
import { readDir } from "@tauri-apps/plugin-fs";
import React, { ReactNode, useState } from "react";

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

  const filesListRef = React.useRef(null);
  const rowVirtualiser = useVirtualizer({
    count: files.length,
    getScrollElement: () => filesListRef.current,
    estimateSize: () => 35,
    overscan: 100,
  });

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
        <div ref={filesListRef} className="overflow-auto h-[200px]">
          <div
            style={{
              height: `${rowVirtualiser.getTotalSize()}px`,
              width: "100%",
              position: "relative",
            }}
          >
            {rowVirtualiser.getVirtualItems().map((row) => (
              <div
                key={row.index}
                style={{
                  position: "absolute",
                  top: 0,
                  left: 0,
                  width: "100%",
                  height: `${row.size}px`,
                  transform: `translateY(${row.start}px)`,
                }}
              >
                {files[row.index]}
              </div>
            ))}
          </div>
        </div>
      </div>
      <div className="rounded bg-gray-300 p-2">3</div>
    </main>
  );
}
