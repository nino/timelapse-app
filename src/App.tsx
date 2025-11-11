import { invoke } from "@tauri-apps/api/core";
import { BaseDirectory } from "@tauri-apps/api/path";
import { readFile } from "@tauri-apps/plugin-fs";
import React from "react";

import "./App.css";
import { useFiles, useFolders, useVideos } from "./hooks/useFolders";

type ViewMode = "images" | "videos";

export function App(): React.ReactNode {
  const { folders, foldersError, refreshFolders } = useFolders();
  const { videos, videosError, refreshVideos } = useVideos();
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
  const { files: videoFiles } = useFiles(
    videoCacheFolder ? `.cache/${videoCacheFolder}` : null,
  );

  // Get current date folder name (YYYY-MM-DD format)
  const currentDateFolder = React.useMemo(() => {
    const today = new Date();
    return today.toISOString().split("T")[0];
  }, []);

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

  // Reset image index when folder changes
  React.useEffect(() => {
    if (viewMode === "images") {
      setCurrentImageIndex(Math.max(files.length - 1, 0));
    }
  }, [selectedFolder, files.length, viewMode]);

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
        return;
      }

      try {
        setIsExtractingFrames(true);
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
        console.error("Error extracting frames:", error);
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

  // Load current image when folder or index changes (works for both images and videos)
  React.useEffect(() => {
    async function loadImage(): Promise<void> {
      if (viewMode === "images") {
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
      } else if (viewMode === "videos") {
        if (!videoCacheFolder || videoFiles.length === 0 || !videoFiles[currentImageIndex]) {
          setCurrentImageSrc(null);
          return;
        }

        try {
          const framePath = `Timelapse/.cache/${videoCacheFolder}/${videoFiles[currentImageIndex]}`;
          console.log("Loading video frame from path:", framePath);

          const frameData = await readFile(framePath, {
            baseDir: BaseDirectory.Home,
          });

          console.log("Frame data loaded, size:", frameData.length);

          // Create a blob URL from the binary data
          const blob = new Blob([frameData], { type: "image/jpeg" });
          const blobUrl = URL.createObjectURL(blob);

          console.log("Created blob URL:", blobUrl);
          setCurrentImageSrc(blobUrl);
        } catch (error) {
          console.error("Error loading frame:", error);
          setCurrentImageSrc(null);
        }
      }
    }

    loadImage();
  }, [selectedFolder, files, videoCacheFolder, videoFiles, currentImageIndex, viewMode]);

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

  const refreshContent = React.useCallback(() => {
    if (viewMode === "images") {
      refreshFolders();
      // Force refresh by changing the selected folder
      const currentFolder = selectedFolder;
      setSelectedFolder(null);
      setTimeout(() => setSelectedFolder(currentFolder), 10);
    } else {
      refreshVideos();
      // Force refresh by changing the selected video
      const currentVideo = selectedVideo;
      setSelectedVideo(null);
      setTimeout(() => setSelectedVideo(currentVideo), 10);
    }
  }, [refreshFolders, refreshVideos, selectedFolder, selectedVideo, viewMode]);

  const formatTime = React.useCallback((index: number) => {
    // Assume screenshots are taken every few seconds, estimate time
    const totalMinutes = Math.floor((index * 1) / 60); // Assuming 1 second between screenshots
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
                    ~{formatTime(currentImageIndex)}
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
                {videos.map((video) => (
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
                    {isExtractingFrames
                      ? "Extracting frames from video…"
                      : videoFiles.length > 0
                        ? "Loading frame…"
                        : selectedVideo
                          ? "Loading video…"
                          : "No video selected"}
                  </p>
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

          {/* Refresh button */}
          <button
            onClick={refreshContent}
            disabled={viewMode === "images" ? !selectedFolder : !selectedVideo}
            className="bg-gradient-to-b from-fuchsia-50 to-amber-50 hover:bg-blue-700 disabled:bg-gray-600 px-3 py-1 rounded-xl text-sm font-medium text-amber-800 transition-colors border-2 border-yellow-500 shadow-md shadow-amber-400/20"
            title={
              viewMode === "images" ? "Refresh screenshots" : "Refresh videos"
            }
          >
            ↻ Refresh
          </button>
        </div>
      </div>
    </main>
  );
}
