import { BaseDirectory } from "@tauri-apps/api/path";
import { readFile } from "@tauri-apps/plugin-fs";
import React, { ReactNode, useState, useCallback, useMemo } from "react";

import "./App.css";
import { useFiles, useFolders } from "./hooks/useFolders";

export function App(): ReactNode {
  const { folders, foldersError } = useFolders();
  const [selectedFolder, setSelectedFolder] = useState<string | null>(null);
  const { files, filesError } = useFiles(selectedFolder);
  const [currentImageIndex, setCurrentImageIndex] = useState(0);
  const [currentImageSrc, setCurrentImageSrc] = useState<string | null>(null);

  // Get current date folder name (YYYY-MM-DD format)
  const currentDateFolder = useMemo(() => {
    const today = new Date();
    return today.toISOString().split("T")[0];
  }, []);

  // Auto-select today's folder if it exists
  React.useEffect(() => {
    if (folders.length > 0 && !selectedFolder) {
      const todayFolder = folders.find(
        (folder) => folder === currentDateFolder,
      );
      if (todayFolder) {
        setSelectedFolder(todayFolder);
      } else {
        // If today's folder doesn't exist, select the most recent one
        const sortedFolders = [...folders].sort().reverse();
        setSelectedFolder(sortedFolders[0]);
      }
    }
  }, [folders, selectedFolder, currentDateFolder]);

  // Reset image index when folder changes
  React.useEffect(() => {
    setCurrentImageIndex(0);
  }, [selectedFolder]);

  // Keyboard navigation
  React.useEffect(() => {
    const handleKeydown = (e: KeyboardEvent): void => {
      if (!selectedFolder || files.length === 0) return;

      let step = 1;
      if (e.shiftKey) step = 10;
      if (e.altKey) step = 100; // Option key on Mac is altKey

      let newIndex = currentImageIndex;

      if (e.key === "ArrowLeft") {
        e.preventDefault();
        newIndex = Math.max(0, currentImageIndex - step);
        setCurrentImageIndex(newIndex);
      } else if (e.key === "ArrowRight") {
        e.preventDefault();
        newIndex = Math.min(files.length - 1, currentImageIndex + step);
        setCurrentImageIndex(newIndex);
      }
    };

    window.addEventListener("keydown", handleKeydown);
    return (): void => window.removeEventListener("keydown", handleKeydown);
  }, [selectedFolder, files.length, currentImageIndex]);

  // Load current image when folder or index changes
  React.useEffect(() => {
    async function loadImage(): Promise<void> {
      if (!selectedFolder || files.length === 0 || !files[currentImageIndex]) {
        setCurrentImageSrc(null);
        return;
      }

      try {
        const imagePath = `Timelapse/${selectedFolder}/${files[currentImageIndex]}`;
        console.log("Loading image from path:", imagePath);

        const imageData = await readFile(imagePath, {
          baseDir: BaseDirectory.Home,
        });

        console.log("Image data loaded, size:", imageData.length);

        // Create a blob URL from the binary data
        const blob = new Blob([imageData], { type: "image/jpeg" });
        const blobUrl = URL.createObjectURL(blob);

        console.log("Created blob URL:", blobUrl);
        setCurrentImageSrc(blobUrl);
      } catch (error) {
        console.error("Error loading image:", error);
        setCurrentImageSrc(null);
      }
    }

    loadImage();
  }, [selectedFolder, files, currentImageIndex]);

  // Clean up blob URLs when component unmounts or image changes
  React.useEffect(() => {
    return (): void => {
      if (currentImageSrc && currentImageSrc.startsWith("blob:")) {
        URL.revokeObjectURL(currentImageSrc);
      }
    };
  }, [currentImageSrc]);

  const handleScrubberChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const newIndex = parseInt(e.target.value, 10);
      setCurrentImageIndex(newIndex);
    },
    [],
  );

  const refreshFiles = useCallback(() => {
    // Force refresh by changing the selected folder
    const currentFolder = selectedFolder;
    setSelectedFolder(null);
    setTimeout(() => setSelectedFolder(currentFolder), 10);
  }, [selectedFolder]);

  const formatTime = useCallback((index: number) => {
    // Assume screenshots are taken every few seconds, estimate time
    const totalMinutes = Math.floor((index * 30) / 60); // Assuming 30 seconds between screenshots
    const hours = Math.floor(totalMinutes / 60);
    const minutes = totalMinutes % 60;
    return `${hours.toString().padStart(2, "0")}:${minutes
      .toString()
      .padStart(2, "0")}`;
  }, []);

  if (foldersError) {
    return (
      <main className="flex items-center justify-center h-screen">
        <div className="text-red-500">
          <p>Error loading folders: {foldersError.message}</p>
        </div>
      </main>
    );
  }

  if (filesError) {
    return (
      <main className="flex items-center justify-center h-screen">
        <div className="text-red-500">
          <p>Error loading files: {filesError.message}</p>
        </div>
      </main>
    );
  }

  return (
    <main className="h-screen overflow-hidden grid grid-rows-[min-content_1fr_56px] bg-gray-100 text-black">
      {/* Header with folder selection */}
      <header className="bg-gray-100 p-3 border-b border-gray-200">
        <div className="flex items-center gap-4">
          <h1 className="text-lg font-semibold">Timelapse Viewer</h1>
          <select
            value={selectedFolder || ""}
            onChange={(e) => setSelectedFolder(e.target.value || null)}
            className="bg-gray-700 text-white px-3 py-1 rounded border border-gray-600 focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            <option value="">Select a date…</option>
            {[...folders]
              .sort()
              .reverse()
              .map((folder) => (
                <option key={folder} value={folder}>
                  {folder} {folder === currentDateFolder ? "(Today)" : ""}
                </option>
              ))}
          </select>
          {selectedFolder && (
            <span className="text-gray-300 text-sm">
              {files.length} screenshots
            </span>
          )}
          {/* Frame counter */}
          {files.length > 0 && (
            <span className="text-gray-300 text-sm">
              Frame {currentImageIndex + 1} / {files.length}
            </span>
          )}
          {/* Time estimate */}
          {files.length > 0 && (
            <span className="text-gray-300 text-sm">
              ~{formatTime(currentImageIndex)}
            </span>
          )}
        </div>
      </header>

      {/* Main image preview area */}
      <div className="relative overflow-hidden">
        {currentImageSrc ? (
          <img
            src={currentImageSrc}
            alt={`Screenshot ${currentImageIndex + 1}`}
            className="max-w-full object-contain absolute top-0 left-0 bottom-0 right-0"
            onError={() => {
              console.error("Failed to load image:", currentImageSrc);
              setCurrentImageSrc(null);
            }}
          />
        ) : (
          <div className="text-gray-500 text-center">
            <p className="text-xl mb-2">
              {files.length > 0 ? "Loading image…" : "No screenshots available"}
            </p>
            {files.length === 0 && (
              <p>Select a date folder with screenshots to begin</p>
            )}
            {files.length > 0 && (
              <p className="text-sm mt-2">
                Trying to load: {files[currentImageIndex]}
              </p>
            )}
          </div>
        )}
      </div>

      {/* Bottom controls */}
      <div className="bg-gray-100 p-4">
        <div className="flex items-center gap-4">
          {/* Scrubber */}
          <div className="flex-1">
            <input
              type="range"
              min={0}
              max={Math.max(0, files.length - 1)}
              value={currentImageIndex}
              onChange={handleScrubberChange}
              disabled={files.length === 0}
              className="w-full h-2 bg-gray-600 rounded-lg appearance-none cursor-pointer 
                         slider:bg-blue-500 slider:rounded-lg slider:cursor-pointer
                         disabled:opacity-50 disabled:cursor-not-allowed"
            />
          </div>

          {/* Current time display */}
          <div className="text-sm text-gray-300 min-w-[60px] text-center backdrop-blur-xl">
            {files.length > 0 ? formatTime(currentImageIndex) : "--:--"}
          </div>

          {/* Refresh button */}
          <button
            onClick={refreshFiles}
            disabled={!selectedFolder}
            className="bg-gradient-to-b from-fuchsia-50 to-amber-50 hover:bg-blue-700 disabled:bg-gray-600 px-3 py-1 rounded-xl text-sm font-medium text-amber-800 transition-colors border-2 border-yellow-500 shadow-md shadow-amber-400/20"
            title="Refresh screenshots"
          >
            ↻ Refresh
          </button>
        </div>
      </div>
    </main>
  );
}
