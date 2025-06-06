import { useVirtualizer } from "@tanstack/react-virtual";
import React, { ReactNode, useState } from "react";

import "./App.css";
import { useFiles, useFolders } from "./hooks/useFolders";

export function App(): ReactNode {
  const { folders } = useFolders();
  const [selectedFolder, setSelectedFolder] = useState<string | null>(null);
  const { files } = useFiles(selectedFolder);

  const filesListRef = React.useRef(null);
  const rowVirtualiser = useVirtualizer({
    count: files.length,
    getScrollElement: () => filesListRef.current,
    estimateSize: () => 35,
    overscan: 8,
  });

  return (
    <main className="grid grid-cols-3 gap-2 m-2 select-none">
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
