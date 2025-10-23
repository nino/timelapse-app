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
  const [currentVideoSrc, setCurrentVideoSrc] = React.useState<string | null>(
    null,
  );
  const [isTranscoding, setIsTranscoding] = React.useState(false);

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
      setSelectedVideo(videos[0]); // Already sorted with most recent first
    }
  }, [videos, selectedVideo, viewMode]);

  // Reset image index when folder changes
  React.useEffect(() => {
    if (viewMode === "images") {
      setCurrentImageIndex(Math.max(files.length - 1, 0));
    }
  }, [selectedFolder, files.length, viewMode]);

  // Load current video when video is selected
  React.useEffect(() => {
    let blobUrl: string | null = null;

    async function loadVideo(): Promise<void> {
      if (viewMode !== "videos" || !selectedVideo) {
        setCurrentVideoSrc(null);
        setIsTranscoding(false);
        return;
      }

      try {
        setIsTranscoding(true);
        console.log("Transcoding video:", selectedVideo);

        // Call Tauri command to transcode the video
        const videoData = await invoke<number[]>("transcode_video", {
          videoFilename: selectedVideo,
        });

        console.log("Transcoded video data received, size:", videoData.length);

        // Create a blob URL from the video data
        const blob = new Blob([new Uint8Array(videoData)], {
          type: "video/mp4",
        });
        blobUrl = URL.createObjectURL(blob);

        console.log("Created video blob URL:", blobUrl);
        setCurrentVideoSrc(blobUrl);
        setIsTranscoding(false);
      } catch (error) {
        console.error("Error transcoding video:", error);
        setCurrentVideoSrc(null);
        setIsTranscoding(false);
      }
    }

    loadVideo();

    // Clean up blob URL when effect re-runs or component unmounts
    return () => {
      if (blobUrl) {
        URL.revokeObjectURL(blobUrl);
      }
    };
  }, [selectedVideo, viewMode]);

  // Keyboard navigation
  React.useEffect(() => {
    const handleKeydown = (e: KeyboardEvent): void => {
      if (viewMode === "images" && (!selectedFolder || files.length === 0))
        return;
      if (viewMode === "videos" && videos.length === 0) return;

      if (viewMode === "images") {
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
      } else if (viewMode === "videos") {
        if (e.key === "ArrowLeft") {
          e.preventDefault();
          const currentIndex = videos.indexOf(selectedVideo || "");
          if (currentIndex > 0) {
            setSelectedVideo(videos[currentIndex - 1]);
          }
        } else if (e.key === "ArrowRight") {
          e.preventDefault();
          const currentIndex = videos.indexOf(selectedVideo || "");
          if (currentIndex < videos.length - 1) {
            setSelectedVideo(videos[currentIndex + 1]);
          }
        }
      }
    };

    window.addEventListener("keydown", handleKeydown);
    return (): void => window.removeEventListener("keydown", handleKeydown);
  }, [
    selectedFolder,
    files.length,
    currentImageIndex,
    viewMode,
    videos,
    selectedVideo,
  ]);

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
            </>
          )}
        </div>
      </header>

      {/* Main content area */}
      <div className="relative overflow-hidden object-contain">
        {viewMode === "images" ? (
          // Images view
          currentImageSrc ? (
            <img
              src={currentImageSrc}
              alt={`Screenshot ${currentImageIndex + 1}`}
              className="w-full h-full object-contain absolute top-0 left-0 bottom-0 right-0"
              onError={() => {
                console.error("Failed to load image:", currentImageSrc);
                setCurrentImageSrc(null);
              }}
            />
          ) : (
            <div className="flex items-center justify-center h-full text-gray-500 text-center">
              <div>
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
              </div>
            </div>
          )
        ) : // Videos view
        currentVideoSrc ? (
          <video
            key={currentVideoSrc}
            src={currentVideoSrc}
            controls
            autoPlay
            className="w-full h-full object-contain absolute top-0 left-0 bottom-0 right-0"
            onError={(e) => {
              const videoElement = e.currentTarget;
              console.error("Failed to load video:", currentVideoSrc);
              console.error("Video error code:", videoElement.error?.code);
              console.error("Video error message:", videoElement.error?.message);
              setCurrentVideoSrc(null);
            }}
            onLoadedMetadata={() => {
              console.log("Video metadata loaded successfully");
            }}
            onCanPlay={() => {
              console.log("Video can play");
            }}
          />
        ) : (
          <div className="flex items-center justify-center h-full text-gray-500 text-center">
            <div>
              <p className="text-xl mb-2">
                {isTranscoding
                  ? "Transcoding video…"
                  : selectedVideo
                    ? "Loading video…"
                    : "No video selected"}
              </p>
              {!selectedVideo && videos.length > 0 && (
                <p>Select a video to begin playback</p>
              )}
              {!selectedVideo && videos.length === 0 && (
                <p>No videos found in the Timelapse directory</p>
              )}
              {selectedVideo && (
                <p className="text-sm mt-2">
                  {isTranscoding ? "Transcoding: " : "Loading: "}
                  {selectedVideo}
                </p>
              )}
              {isTranscoding && (
                <p className="text-xs mt-2 text-gray-400">
                  This may take a moment for the first playback…
                </p>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Bottom controls */}
      <div className="bg-gray-100 p-4">
        <div className="flex items-center gap-4">
          {viewMode === "images" ? (
            <>
              {/* Image scrubber */}
              <div className="flex-1 bg-gray-200 p-1 pt-0 rounded-full">
                <input
                  type="range"
                  min={0}
                  max={Math.max(0, files.length - 1)}
                  value={currentImageIndex}
                  onChange={handleScrubberChange}
                  disabled={files.length === 0}
                  className="w-full h-2 bg-gray-300 rounded-lg appearance-none cursor-pointer 
                             disabled:opacity-50 disabled:cursor-not-allowed"
                />
              </div>

              {/* Current time display */}
              <div className="text-sm text-gray-600 min-w-[60px] text-center">
                {files.length > 0 ? formatTime(currentImageIndex) : "--:--"}
              </div>
            </>
          ) : (
            <>
              {/* Video navigation */}
              <div className="flex-1 flex items-center justify-center gap-4">
                <button
                  onClick={() => {
                    const currentIndex = videos.indexOf(selectedVideo || "");
                    if (currentIndex > 0) {
                      setSelectedVideo(videos[currentIndex - 1]);
                    }
                  }}
                  disabled={
                    !selectedVideo || videos.indexOf(selectedVideo) === 0
                  }
                  className="bg-white hover:bg-gray-50 disabled:bg-gray-200 disabled:text-gray-400 px-4 py-2 rounded-lg text-sm font-medium transition-colors border border-gray-300"
                >
                  ← Previous Video
                </button>
                <span className="text-sm text-gray-600">
                  {selectedVideo
                    ? `${videos.indexOf(selectedVideo) + 1} / ${videos.length}`
                    : "--"}
                </span>
                <button
                  onClick={() => {
                    const currentIndex = videos.indexOf(selectedVideo || "");
                    if (currentIndex < videos.length - 1) {
                      setSelectedVideo(videos[currentIndex + 1]);
                    }
                  }}
                  disabled={
                    !selectedVideo ||
                    videos.indexOf(selectedVideo) === videos.length - 1
                  }
                  className="bg-white hover:bg-gray-50 disabled:bg-gray-200 disabled:text-gray-400 px-4 py-2 rounded-lg text-sm font-medium transition-colors border border-gray-300"
                >
                  Next Video →
                </button>
              </div>
            </>
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
