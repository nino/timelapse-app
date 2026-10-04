import { invoke } from "@tauri-apps/api/core";
import { BaseDirectory } from "@tauri-apps/api/path";
import { readFile } from "@tauri-apps/plugin-fs";
import React from "react";

import "./App.css";
import { useFiles, useFolders, useVideos } from "./hooks/useFolders";
import { timelapseRoot } from "./timelapseRoot";

type ViewMode = "images" | "videos";

export function App(): React.ReactNode {
  const { folders, foldersError } = useFolders();
  const { videos, videosError } = useVideos();
  const [viewMode, setViewMode] = React.useState<ViewMode>("images");
  const [selectedFolder, setSelectedFolder] = React.useState<string | null>(
    null,
  );
  const [selectedVideo, setSelectedVideo] = React.useState<string | null>(null);
  const { files, filesError } = useFiles(selectedFolder);
  const [currentImageIndex, setCurrentImageIndex] = React.useState(0);
  const [currentImageSrc, setCurrentImageSrc] = React.useState<string | null>(
    null,
  );
  const [videoCacheFolder, setVideoCacheFolder] = React.useState<string | null>(
    null,
  );
  const [isExtractingFrames, setIsExtractingFrames] = React.useState(false);
  const [extractionError, setExtractionError] = React.useState<string | null>(
    null,
  );
  const { files: videoFiles } = useFiles(
    videoCacheFolder ? `.cache/${videoCacheFolder}` : null,
  );
  const [currentTimestamp, setCurrentTimestamp] = React.useState<string | null>(
    null,
  );

  // Today's folder name. Recomputed whenever the folder list changes so the
  // "(Today)" label moves over at midnight instead of sticking to launch day.
  // Local date, to match `create_day_dir_if_needed` in Rust.
  const currentDateFolder = React.useMemo(
    () => localDateFolder(new Date()),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [folders],
  );

  // Auto-select today's folder if it exists (for images mode)
  React.useEffect(() => {
    if (viewMode === "images" && folders.length > 0 && !selectedFolder) {
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
  }, [folders, selectedFolder, currentDateFolder, viewMode]);

  // Auto-select most recent video (for videos mode)
  React.useEffect(() => {
    if (viewMode === "videos" && videos.length > 0 && !selectedVideo) {
      setSelectedVideo(videos[videos.length - 1]); // Videos are in chronological order, so last is most recent
    }
  }, [videos, selectedVideo, viewMode]);

  // Follow the newest frame. Opening a folder jumps to its last frame; after
  // that, new captures only move the index if it was already on the last one,
  // so scrubbing back through the day isn't interrupted every second.
  const followedListing = React.useRef<{ key: string; length: number }>({
    key: "",
    length: 0,
  });
  React.useEffect(() => {
    const key = `${viewMode}:${selectedFolder ?? ""}`;
    const previous = followedListing.current;
    followedListing.current = { key, length: files.length };
    // Recording the videos key too means coming back to images counts as
    // opening the folder afresh, rather than inheriting the video's index.
    if (viewMode !== "images") return;
    const last = Math.max(files.length - 1, 0);
    setCurrentImageIndex((index) =>
      previous.key !== key || index >= previous.length - 1
        ? last
        : Math.min(index, last),
    );
  }, [selectedFolder, files.length, viewMode]);

  // Roll over to the new day's folder at midnight, but only for someone who
  // was watching the live edge of the previous newest day.
  const newestFolder = React.useMemo(
    () => [...folders].sort().at(-1) ?? null,
    [folders],
  );
  const isAtLiveEdge =
    files.length === 0 || currentImageIndex >= files.length - 1;
  const liveEdgeRef = React.useRef({ selectedFolder, isAtLiveEdge });
  React.useEffect(() => {
    liveEdgeRef.current = { selectedFolder, isAtLiveEdge };
  }, [selectedFolder, isAtLiveEdge]);
  const previousNewestFolder = React.useRef(newestFolder);
  React.useEffect(() => {
    const previous = previousNewestFolder.current;
    previousNewestFolder.current = newestFolder;
    const viewer = liveEdgeRef.current;
    if (
      previous &&
      newestFolder &&
      newestFolder !== previous &&
      viewer.selectedFolder === previous &&
      viewer.isAtLiveEdge
    ) {
      setSelectedFolder(newestFolder);
    }
  }, [newestFolder]);

  // Reset image index when video frames are loaded
  React.useEffect(() => {
    if (viewMode === "videos" && videoFiles.length > 0) {
      setCurrentImageIndex(0); // Start from first frame for videos
    }
  }, [videoFiles.length, viewMode]);

  // Extract frames from selected video
  React.useEffect(() => {
    async function extractFrames(): Promise<void> {
      if (viewMode !== "videos" || !selectedVideo) {
        setVideoCacheFolder(null);
        setIsExtractingFrames(false);
        setExtractionError(null);
        return;
      }

      try {
        setIsExtractingFrames(true);
        setExtractionError(null);
        setVideoCacheFolder(null); // Clear old frames immediately
        console.log("Extracting frames from video:", selectedVideo);

        // Call Tauri command to extract frames (uses cache if available)
        const cacheFolder = await invoke<string>("extract_video_frames", {
          videoFilename: selectedVideo,
        });

        console.log("Frames extracted to cache folder:", cacheFolder);
        setVideoCacheFolder(cacheFolder);
        setIsExtractingFrames(false);
      } catch (error) {
        // Without this the UI sits on "Loading video…" forever, which makes a
        // missing ffmpeg look identical to a slow extraction.
        console.error("Error extracting frames:", error);
        setExtractionError(
          error instanceof Error ? error.message : String(error),
        );
        setVideoCacheFolder(null);
        setIsExtractingFrames(false);
      }
    }

    extractFrames();
  }, [selectedVideo, viewMode]);

  // Keyboard navigation
  React.useEffect(() => {
    const handleKeydown = (e: KeyboardEvent): void => {
      const activeFiles = viewMode === "images" ? files : videoFiles;

      if (viewMode === "images" && (!selectedFolder || files.length === 0))
        return;
      if (viewMode === "videos" && videoFiles.length === 0) return;

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
        newIndex = Math.min(activeFiles.length - 1, currentImageIndex + step);
        setCurrentImageIndex(newIndex);
      }
    };

    window.addEventListener("keydown", handleKeydown);
    return (): void => window.removeEventListener("keydown", handleKeydown);
  }, [
    selectedFolder,
    files.length,
    videoFiles.length,
    currentImageIndex,
    viewMode,
  ]);

  // The file on screen, relative to the library root. Effects below key on
  // this string rather than on the file arrays, which change every time a new
  // capture lands even when the visible frame hasn't.
  const currentFramePath =
    viewMode === "images"
      ? selectedFolder && files[currentImageIndex]
        ? `${selectedFolder}/${files[currentImageIndex]}`
        : null
      : videoCacheFolder && videoFiles[currentImageIndex]
        ? `.cache/${videoCacheFolder}/${videoFiles[currentImageIndex]}`
        : null;

  // Load current image when folder or index changes (works for both images and videos)
  React.useEffect(() => {
    if (!currentFramePath) {
      setCurrentImageSrc(null);
      return;
    }

    let cancelled = false;
    async function loadImage(path: string): Promise<void> {
      try {
        const imageData = await readFile(`${timelapseRoot()}/${path}`, {
          baseDir: BaseDirectory.Home,
        });
        // A later frame was requested while this one was loading.
        if (cancelled) return;
        const blob = new Blob([imageData], { type: "image/jpeg" });
        setCurrentImageSrc(URL.createObjectURL(blob));
      } catch (error) {
        if (cancelled) return;
        console.error(
          path.startsWith(".cache/") ? "Error loading frame:" : "Error loading image:",
          error,
        );
        setCurrentImageSrc(null);
      }
    }

    loadImage(currentFramePath);
    return (): void => {
      cancelled = true;
    };
  }, [currentFramePath]);

  // Clean up blob URLs when component unmounts or image changes
  React.useEffect(() => {
    return (): void => {
      if (currentImageSrc && currentImageSrc.startsWith("blob:")) {
        URL.revokeObjectURL(currentImageSrc);
      }
    };
  }, [currentImageSrc]);

  const handleScrubberChange = React.useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      const newIndex = parseInt(e.target.value, 10);
      setCurrentImageIndex(newIndex);
    },
    [],
  );

  // Fetch timestamp for current frame from database
  const currentScreenshot =
    viewMode === "images" ? (files[currentImageIndex] ?? null) : null;
  React.useEffect(() => {
    let cancelled = false;
    async function fetchTimestamp(): Promise<void> {
      if (!currentScreenshot) {
        setCurrentTimestamp(null);
        return;
      }

      try {
        // Extract frame number from filename (e.g., "00001.png" -> 1)
        const filename = currentScreenshot;
        const frameNumber = parseInt(filename.replace(".png", ""), 10);

        const metadata = await invoke<[string, string] | null>(
          "get_screenshot_metadata",
          {
            frameNumber,
          },
        );

        if (cancelled) return;
        if (metadata && metadata[1]) {
          setCurrentTimestamp(metadata[1]); // Use local_time
        } else {
          setCurrentTimestamp(null);
        }
      } catch (error) {
        if (cancelled) return;
        console.error("Error fetching timestamp:", error);
        setCurrentTimestamp(null);
      }
    }

    fetchTimestamp();
    return (): void => {
      cancelled = true;
    };
  }, [currentScreenshot]);

  const formatTime = React.useCallback(
    (index: number) => {
      // If we have a real timestamp from the database, use it
      if (currentTimestamp) {
        try {
          const date = new Date(currentTimestamp);
          const hours = date.getHours();
          const minutes = date.getMinutes();
          return `${hours.toString().padStart(2, "0")}:${minutes
            .toString()
            .padStart(2, "0")}`;
        } catch (e) {
          console.error("Error parsing timestamp:", e);
        }
      }

      // Fall back to estimate if no timestamp available
      const totalMinutes = Math.floor((index * 1) / 60); // Assuming 1 second between screenshots
      const hours = Math.floor(totalMinutes / 60);
      const minutes = totalMinutes % 60;
      return `${hours.toString().padStart(2, "0")}:${minutes
        .toString()
        .padStart(2, "0")}`;
    },
    [currentTimestamp],
  );

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

  if (videosError) {
    return (
      <main className="flex items-center justify-center h-screen">
        <div className="text-red-500">
          <p>Error loading videos: {videosError.message}</p>
        </div>
      </main>
    );
  }

  return (
    <main className="h-screen overflow-hidden grid grid-rows-[min-content_1fr_56px] bg-gray-100 text-black">
      {/* Header with mode toggle and content selection */}
      <header className="bg-gray-100 p-3 border-b border-gray-200">
        <div className="flex items-center gap-4">
          <h1 className="text-lg font-semibold">Timelapse Viewer</h1>

          {/* Mode toggle */}
          <div className="flex bg-gray-200 rounded-lg p-1">
            <button
              onClick={() => setViewMode("images")}
              className={`px-3 py-1 rounded text-sm font-medium transition-colors ${
                viewMode === "images"
                  ? "bg-white text-gray-900 shadow-sm"
                  : "text-gray-600 hover:text-gray-900"
              }`}
            >
              Images
            </button>
            <button
              onClick={() => setViewMode("videos")}
              className={`px-3 py-1 rounded text-sm font-medium transition-colors ${
                viewMode === "videos"
                  ? "bg-white text-gray-900 shadow-sm"
                  : "text-gray-600 hover:text-gray-900"
              }`}
            >
              Videos
            </button>
          </div>

          {/* Images mode selectors */}
          {viewMode === "images" && (
            <>
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
              {selectedFolder && files.length > 0 && (
                <>
                  <span className="text-gray-600 text-sm">
                    {files.length} screenshots
                  </span>
                  <span className="text-gray-600 text-sm tabular-nums">
                    Frame {currentImageIndex + 1} / {files.length}
                  </span>
                  <span className="text-gray-600 text-sm tabular-nums">
                    {currentTimestamp ? "" : "~"}
                    {formatTime(currentImageIndex)}
                  </span>
                </>
              )}
            </>
          )}

          {/* Videos mode selectors */}
          {viewMode === "videos" && (
            <>
              <select
                value={selectedVideo || ""}
                onChange={(e) => setSelectedVideo(e.target.value || null)}
                className="bg-gray-700 text-white px-3 py-1 rounded border border-gray-600 focus:outline-none focus:ring-2 focus:ring-blue-500"
              >
                <option value="">Select a video…</option>
                {[...videos]
                  .sort()
                  .reverse()
                  .map((video) => (
                    <option key={video} value={video}>
                      {video}
                    </option>
                  ))}
              </select>
              {videos.length > 0 && (
                <span className="text-gray-600 text-sm">
                  {videos.length} videos
                </span>
              )}
              {selectedVideo && videoFiles.length > 0 && (
                <>
                  <span className="text-gray-600 text-sm">
                    {videoFiles.length} frames
                  </span>
                  <span className="text-gray-600 text-sm tabular-nums">
                    Frame {currentImageIndex + 1} / {videoFiles.length}
                  </span>
                </>
              )}
            </>
          )}
        </div>
      </header>

      {/* Main content area */}
      <div className="relative overflow-hidden object-contain">
        {currentImageSrc ? (
          <img
            src={currentImageSrc}
            alt={
              viewMode === "images"
                ? `Screenshot ${currentImageIndex + 1}`
                : `Frame ${currentImageIndex + 1}`
            }
            className="w-full h-full object-contain absolute top-0 left-0 bottom-0 right-0"
            onError={() => {
              console.error("Failed to load image:", currentImageSrc);
              setCurrentImageSrc(null);
            }}
          />
        ) : (
          <div className="flex items-center justify-center h-full text-gray-500 text-center">
            <div>
              {viewMode === "images" ? (
                <>
                  <p className="text-xl mb-2">
                    {files.length > 0
                      ? "Loading image…"
                      : "No screenshots available"}
                  </p>
                  {files.length === 0 && (
                    <p>Select a date folder with screenshots to begin</p>
                  )}
                  {files.length > 0 && (
                    <p className="text-sm mt-2">
                      Trying to load: {files[currentImageIndex]}
                    </p>
                  )}
                </>
              ) : (
                <>
                  <p className="text-xl mb-2">
                    {extractionError
                      ? "Could not extract frames from this video"
                      : isExtractingFrames
                        ? "Extracting frames from video…"
                        : videoFiles.length > 0
                          ? "Loading frame…"
                          : videoCacheFolder
                            ? "Loading frames…"
                            : selectedVideo
                              ? "Loading video…"
                              : "No video selected"}
                  </p>
                  {extractionError && (
                    <p className="text-sm mt-2 text-red-400">
                      {extractionError}
                    </p>
                  )}
                  {!selectedVideo && videos.length > 0 && (
                    <p>Select a video to begin</p>
                  )}
                  {!selectedVideo && videos.length === 0 && (
                    <p>No videos found in the Timelapse directory</p>
                  )}
                  {selectedVideo && videoFiles.length > 0 && (
                    <p className="text-sm mt-2">
                      Trying to load: {videoFiles[currentImageIndex]}
                    </p>
                  )}
                </>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Bottom controls */}
      <div className="bg-gray-100 p-4">
        <div className="flex items-center gap-4">
          {/* Scrubber (works for both images and videos) */}
          <div className="flex-1 bg-gray-200 p-1 pt-0 rounded-full">
            <input
              type="range"
              min={0}
              max={Math.max(0, (viewMode === "images" ? files.length : videoFiles.length) - 1)}
              value={currentImageIndex}
              onChange={handleScrubberChange}
              disabled={viewMode === "images" ? files.length === 0 : videoFiles.length === 0}
              className="w-full h-2 bg-gray-300 rounded-lg appearance-none cursor-pointer
                         disabled:opacity-50 disabled:cursor-not-allowed"
            />
          </div>

          {/* Current time/frame display */}
          {viewMode === "images" && (
            <div className="text-sm text-gray-600 min-w-[60px] text-center">
              {files.length > 0 ? formatTime(currentImageIndex) : "--:--"}
            </div>
          )}

        </div>
      </div>
    </main>
  );
}

function localDateFolder(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}
