import React from "react";

import "./App.css";
import { frameUrl, getFrameTime, type FrameTime } from "./frames";
import { useDay, useDays } from "./hooks/useLibrary";
import { pendingTrackBackground, usePendingFrames } from "./hooks/usePendingFrames";

export function App(): React.ReactNode {
  const { days, daysError } = useDays();
  const [selectedDay, setSelectedDay] = React.useState<string | null>(null);
  const { day, dayError } = useDay(selectedDay);
  const frameCount = day?.frameCount ?? 0;
  // Where the viewer is in the selected day. `index: null` means "the newest
  // frame", so the view follows new captures for free and stops following
  // the moment the user scrubs back. A position recorded for another day
  // counts as null, which is what makes opening a day land on its last frame
  // without ever requesting frame 0 first.
  const [position, setPosition] = React.useState<{
    day: string | null;
    index: number | null;
  }>({ day: null, index: null });
  const followsLiveEdge = position.day !== selectedDay || position.index === null;
  const currentIndex = followsLiveEdge
    ? Math.max(frameCount - 1, 0)
    : Math.min(position.index ?? 0, Math.max(frameCount - 1, 0));
  const goTo = React.useCallback(
    (index: number): void => {
      setPosition({ day: selectedDay, index: index >= frameCount - 1 ? null : index });
    },
    [selectedDay, frameCount],
  );
  const [frameTime, setFrameTime] = React.useState<FrameTime | null>(null);

  // Today's day name. Recomputed whenever the day list changes so the
  // "(Today)" label moves over at midnight instead of sticking to launch day.
  // Local date, to match `create_day_dir_if_needed` in Rust.
  const today = React.useMemo(
    () => localDateFolder(new Date()),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [days],
  );
  const newestDay = days.length > 0 ? days[days.length - 1] : null;

  // Open the newest day (today, while capturing) when nothing is selected.
  React.useEffect(() => {
    if (!selectedDay && newestDay) {
      setSelectedDay(newestDay);
    }
  }, [selectedDay, newestDay]);

  // Roll over to the new day at midnight, but only for someone who was
  // watching the live edge of the previous newest day.
  const liveEdge = React.useRef({ selectedDay, followsLiveEdge });
  React.useEffect(() => {
    liveEdge.current = { selectedDay, followsLiveEdge };
  }, [selectedDay, followsLiveEdge]);
  const previousNewestDay = React.useRef(newestDay);
  React.useEffect(() => {
    const previous = previousNewestDay.current;
    previousNewestDay.current = newestDay;
    const viewer = liveEdge.current;
    if (
      previous &&
      newestDay &&
      newestDay !== previous &&
      viewer.selectedDay === previous &&
      viewer.followsLiveEdge
    ) {
      setSelectedDay(newestDay);
    }
  }, [newestDay]);

  // Keyboard: ←/→ by 1, Shift by 10, Option by 100.
  React.useEffect(() => {
    const handleKeydown = (e: KeyboardEvent): void => {
      if (frameCount === 0) return;
      if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
      // Arrows in the day picker change the day; leave those alone.
      if (e.target instanceof HTMLSelectElement) return;
      // Also stops a focused slider from taking its own 1-frame step on top.
      e.preventDefault();
      const step = e.altKey ? 100 : e.shiftKey ? 10 : 1;
      const direction = e.key === "ArrowLeft" ? -1 : 1;
      goTo(Math.min(frameCount - 1, Math.max(0, currentIndex + direction * step)));
    };
    window.addEventListener("keydown", handleKeydown);
    return (): void => window.removeEventListener("keydown", handleKeydown);
  }, [frameCount, currentIndex, goTo]);

  const wantedSrc =
    selectedDay && frameCount > 0 ? frameUrl(selectedDay, currentIndex) : null;
  const { src, frameFailed, loadedFrames, onLoad, onError } = useGatedImage(wantedSrc);
  // Stretches still to be decoded from video are drawn paler on the scrubber.
  const pendingFrames = usePendingFrames(day, loadedFrames);

  // Capture time of the frame on screen.
  React.useEffect(() => {
    if (!selectedDay || frameCount === 0) {
      setFrameTime(null);
      return;
    }
    let cancelled = false;
    getFrameTime(selectedDay, currentIndex).then(
      (time) => {
        if (!cancelled) setFrameTime(time);
      },
      (error: unknown) => {
        if (cancelled) return;
        console.error("Error fetching frame time:", error);
        setFrameTime(null);
      },
    );
    return (): void => {
      cancelled = true;
    };
  }, [selectedDay, currentIndex, frameCount]);

  const error = daysError ?? dayError;
  if (error) {
    return (
      <main className="flex items-center justify-center h-screen">
        <div className="text-red-500">
          <p>Could not load the timelapse library: {error.message}</p>
        </div>
      </main>
    );
  }

  return (
    <main className="h-screen overflow-hidden grid grid-rows-[min-content_1fr_56px] bg-gray-100 text-black">
      <header className="bg-gray-100 p-3 border-b border-gray-200">
        <div className="flex items-center gap-4">
          <h1 className="text-lg font-semibold">Timelapse Viewer</h1>

          <select
            aria-label="Day"
            value={selectedDay ?? ""}
            onChange={(e) => setSelectedDay(e.target.value || null)}
            className="bg-gray-700 text-white px-3 py-1 rounded border border-gray-600 focus:outline-none focus:ring-2 focus:ring-blue-500"
          >
            {selectedDay === null && <option value="">Select a day…</option>}
            {[...days].reverse().map((date) => (
              <option key={date} value={date}>
                {date} {date === today ? "(Today)" : ""}
              </option>
            ))}
          </select>

          {frameCount > 0 && (
            <span className="text-gray-600 text-sm tabular-nums">
              Frame {currentIndex + 1} / {frameCount}
            </span>
          )}
        </div>
      </header>

      <div className="relative overflow-hidden">
        {src && (
          <img
            src={src}
            alt={`Frame ${currentIndex + 1}`}
            onLoad={onLoad}
            onError={onError}
            className={`w-full h-full object-contain absolute inset-0 ${
              frameFailed ? "invisible" : ""
            }`}
          />
        )}
        {(!src || frameFailed) && (
          <div className="flex items-center justify-center h-full text-gray-500 text-center">
            <p className="text-xl">
              {frameFailed
                ? "Could not load this frame"
                : days.length === 0
                  ? "No screenshots yet"
                  : selectedDay && day && frameCount === 0
                    ? "Nothing was captured on this day"
                    : "Loading…"}
            </p>
          </div>
        )}
      </div>

      <div className="bg-gray-100 p-4">
        <div className="flex items-center gap-4">
          <div className="flex-1 bg-gray-200 p-1 pt-0 rounded-full">
            <input
              type="range"
              aria-label="Position in day"
              min={0}
              max={Math.max(0, frameCount - 1)}
              value={currentIndex}
              onChange={(e) => goTo(parseInt(e.target.value, 10))}
              disabled={frameCount === 0}
              style={{ background: pendingTrackBackground(pendingFrames, frameCount) }}
              className="w-full h-2 bg-gray-300 rounded-lg appearance-none cursor-pointer
                         disabled:opacity-50 disabled:cursor-not-allowed"
            />
          </div>
          <div
            className="text-sm text-gray-600 min-w-[60px] text-center tabular-nums"
            title={frameTime && !frameTime.exact ? "Estimated" : undefined}
          >
            {formatFrameTime(frameTime)}
          </div>
        </div>
      </div>
    </main>
  );
}

/**
 * Show `wanted` in an `<img>`, but never start loading a frame while another
 * is still loading. Dragging the scrubber across a video day would otherwise
 * queue a decode for every chunk it passes; this way each load that finishes
 * jumps straight to wherever the scrubber is now.
 */
function useGatedImage(wanted: string | null): {
  src: string | null;
  frameFailed: boolean;
  /** Counts finished loads, so callers can re-check what a load changed. */
  loadedFrames: number;
  onLoad: () => void;
  onError: () => void;
} {
  const [src, setSrc] = React.useState<string | null>(null);
  const [frameFailed, setFrameFailed] = React.useState(false);
  const [loadedFrames, setLoadedFrames] = React.useState(0);
  const srcRef = React.useRef<string | null>(null);
  const wantedRef = React.useRef<string | null>(wanted);
  const inFlight = React.useRef(false);

  const show = React.useCallback((next: string | null): void => {
    srcRef.current = next;
    inFlight.current = next !== null;
    setSrc(next);
  }, []);

  React.useEffect(() => {
    wantedRef.current = wanted;
    if (!inFlight.current && wanted !== srcRef.current) {
      show(wanted);
    }
  }, [wanted, show]);

  const settle = React.useCallback(
    (failed: boolean): void => {
      inFlight.current = false;
      setFrameFailed(failed);
      setLoadedFrames((n) => n + 1);
      if (wantedRef.current !== srcRef.current) {
        show(wantedRef.current);
      }
    },
    [show],
  );

  return {
    src,
    frameFailed,
    loadedFrames,
    onLoad: React.useCallback((): void => settle(false), [settle]),
    onError: React.useCallback((): void => settle(true), [settle]),
  };
}

function formatFrameTime(time: FrameTime | null): string {
  if (!time) return "--:--";
  const date = new Date(time.localTime);
  if (Number.isNaN(date.getTime())) return "--:--";
  const hhmm = `${String(date.getHours()).padStart(2, "0")}:${String(
    date.getMinutes(),
  ).padStart(2, "0")}`;
  return time.exact ? hhmm : `~${hhmm}`;
}

function localDateFolder(date: Date): string {
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}
