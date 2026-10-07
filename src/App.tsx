import React from "react";

import "./App.css";
import { DayPicker } from "./DayPicker";
import { frameUrl, getFrameTime, type FrameTime } from "./frames";
import { useDay, useDays } from "./hooks/useLibrary";
import {
  useDayMatches,
  useMatchCounts,
  useMatchLines,
  useOcrVersion,
} from "./hooks/useOcrSearch";
import { usePendingFrames } from "./hooks/usePendingFrames";
import { PendingStretches } from "./PendingStretches";
import { nextStop, previousStop, rangeAt, toStops, type DayMatch, type Stop } from "./search";
import { fieldFrame, focusRing, segmentButton } from "./ui";

// How many other days the find bar names before folding the rest away.
const OTHER_DAYS_SHOWN = 4;
// Frames are captured at this size; used until the first one has loaded.
const DEFAULT_FRAME_SIZE = { width: 1800, height: 1124 };

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

  // Find bar: what is typed, and the (debounced) query actually searched.
  const [typed, setTyped] = React.useState("");
  const query = useDebounced(typed.trim(), 150);
  const ocrVersion = useOcrVersion();
  const { matches, matchesError } = useDayMatches(selectedDay, query, ocrVersion);
  const stops = React.useMemo(() => toStops(matches ?? []), [matches]);
  const counts = useMatchCounts(query, ocrVersion);
  const [showAllDays, setShowAllDays] = React.useState(false);
  // A day picked from the "Also on" list opens on its first match once that
  // day's matches have loaded.
  const [jumpToFirstMatch, setJumpToFirstMatch] = React.useState<string | null>(null);
  const findInput = React.useRef<HTMLInputElement>(null);

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

  React.useEffect(() => {
    // Left for another day before its matches arrived: forget the jump.
    if (jumpToFirstMatch !== null && jumpToFirstMatch !== selectedDay) {
      setJumpToFirstMatch(null);
      return;
    }
    if (
      jumpToFirstMatch === null ||
      matches === null ||
      frameCount === 0
    ) {
      return;
    }
    setJumpToFirstMatch(null);
    if (stops.length > 0) goTo(stops[0].index);
  }, [jumpToFirstMatch, selectedDay, matches, stops, frameCount, goTo]);

  const stepToMatch = React.useCallback(
    (direction: 1 | -1): void => {
      const stop =
        direction === 1 ? nextStop(stops, currentIndex) : previousStop(stops, currentIndex);
      if (stop) goTo(stop.index);
    },
    [stops, currentIndex, goTo],
  );

  // ⌘F / Ctrl+F jumps to the find bar.
  React.useEffect(() => {
    const handleKeydown = (e: KeyboardEvent): void => {
      if (e.key.toLowerCase() !== "f" || !(e.metaKey || e.ctrlKey)) return;
      e.preventDefault();
      findInput.current?.focus();
      findInput.current?.select();
    };
    window.addEventListener("keydown", handleKeydown);
    return (): void => window.removeEventListener("keydown", handleKeydown);
  }, []);

  // Keyboard: ←/→ by 1, Shift by 10, Option by 100.
  React.useEffect(() => {
    const handleKeydown = (e: KeyboardEvent): void => {
      if (frameCount === 0) return;
      if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
      // Leave the day picker alone, open or closed.
      if (
        e.target instanceof Element &&
        e.target.closest('[role="listbox"], [aria-haspopup="listbox"]')
      ) {
        return;
      }
      // In the find bar they move the caret.
      if (e.target instanceof HTMLInputElement && e.target.type !== "range") return;
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
  const { src, frameFailed, loaded, onLoad, onError } = useGatedImage(wantedSrc);
  // Stretches still to be decoded from video are drawn paler on the scrubber.
  const pendingFrames = usePendingFrames(day, loaded);
  const [frameSize, setFrameSize] = React.useState(DEFAULT_FRAME_SIZE);
  const currentMatch: DayMatch | null = matches?.[rangeAt(matches, currentIndex)] ?? null;
  const matchLines = useMatchLines(selectedDay, currentMatch?.frame ?? null, query);
  // Outline the matching lines only once the frame on screen is the one the
  // match was read from, or stands for: until a new frame has loaded, the
  // browser keeps showing the old one.
  const highlights =
    currentMatch && loaded === wantedSrc && !frameFailed ? (matchLines ?? []) : [];

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
        <div className="text-danger">
          <p>Could not load the timelapse library: {error.message}</p>
        </div>
      </main>
    );
  }

  return (
    <main className="h-screen overflow-hidden grid grid-rows-[min-content_1fr_auto] bg-page text-fg">
      <header className="bg-card px-4 py-2.5 border-b border-border">
        <div className="flex items-center gap-4">
          <DayPicker days={days} today={today} value={selectedDay} onChange={setSelectedDay} />


          <div className="ml-auto flex items-center gap-1.5">
            <label className={`${fieldFrame} flex items-center gap-2 w-72 h-9 px-3`}>
              <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true" className="shrink-0 text-muted-fg">
                <circle cx="11" cy="11" r="7" />
                <path d="m20 20-3.5-3.5" />
              </svg>
              <input
                ref={findInput}
                type="search"
                aria-label="Find text on screen"
                placeholder="Find text on screen"
                value={typed}
                onChange={(e) => {
                  setTyped(e.target.value);
                  setShowAllDays(false);
                }}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    stepToMatch(e.shiftKey ? -1 : 1);
                  } else if (e.key === "Escape") {
                    setTyped("");
                    e.currentTarget.blur();
                  }
                }}
                className="flex-1 min-w-0 bg-transparent text-sm outline-none placeholder:text-muted-fg"
              />
              {query !== "" && (
                <span className="text-xs text-muted-fg whitespace-nowrap tabular-nums" aria-live="polite">
                  {matchLabel(stops, matches, matchesError, currentIndex)}
                </span>
              )}
            </label>
            <div className="flex h-9 shrink-0 rounded-xl border border-border bg-card">
              <button
                type="button"
                aria-label="Previous match"
                title="Previous match (Shift+Enter)"
                disabled={stops.length === 0}
                onClick={() => stepToMatch(-1)}
                className={`${segmentButton} rounded-l-[11px]`}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
                  <path d="m15 18-6-6 6-6" />
                </svg>
              </button>
              <button
                type="button"
                aria-label="Next match"
                title="Next match (Enter)"
                disabled={stops.length === 0}
                onClick={() => stepToMatch(1)}
                className={`${segmentButton} rounded-r-[11px] border-l border-border`}
              >
                <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true">
                  <path d="m9 18 6-6-6-6" />
                </svg>
              </button>
            </div>
          </div>
        </div>

        <OtherDays
          counts={counts.filter((c) => c.day !== selectedDay)}
          today={today}
          showAll={showAllDays}
          onShowAll={() => setShowAllDays(true)}
          onPick={(date) => {
            setSelectedDay(date);
            setJumpToFirstMatch(date);
          }}
        />
      </header>

      <div className="relative overflow-hidden">
        {src && (
          <img
            src={src}
            alt={`Frame ${currentIndex + 1}`}
            onLoad={(e) => {
              const { naturalWidth: width, naturalHeight: height } = e.currentTarget;
              if (width > 0 && height > 0) {
                setFrameSize((size) =>
                  size.width === width && size.height === height ? size : { width, height },
                );
              }
              onLoad();
            }}
            onError={onError}
            className={`w-full h-full object-contain absolute inset-0 ${
              frameFailed ? "invisible" : ""
            }`}
          />
        )}
        {highlights.length > 0 && (
          // Same box and aspect fitting as the <img>'s object-contain, so
          // normalized line boxes land on the text.
          <svg
            data-testid="match-highlights"
            viewBox={`0 0 ${frameSize.width} ${frameSize.height}`}
            preserveAspectRatio="xMidYMid meet"
            className="absolute inset-0 w-full h-full pointer-events-none"
            aria-hidden="true"
          >
            {highlights.map((line, i) => (
              <rect
                key={i}
                x={line.x * frameSize.width - 4}
                y={line.y * frameSize.height - 4}
                width={line.width * frameSize.width + 8}
                height={line.height * frameSize.height + 8}
                rx={4}
                fill="rgba(250, 204, 21, 0.2)"
                stroke="#facc15"
                strokeWidth={4}
              />
            ))}
          </svg>
        )}
        {(!src || frameFailed) && (
          <div className="flex items-center justify-center h-full text-muted-fg text-center">
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

      <div className="bg-card px-4 py-3 border-t border-border">
        <div className="flex items-center gap-4">
          <div className="relative flex-1 scrub-track p-1 pt-0 rounded-full">
            <PendingStretches pending={pendingFrames} />
            <MatchMarks stops={stops} frameCount={frameCount} currentIndex={currentIndex} />
            <input
              type="range"
              aria-label="Position in day"
              min={0}
              max={Math.max(0, frameCount - 1)}
              value={currentIndex}
              onChange={(e) => goTo(parseInt(e.target.value, 10))}
              disabled={frameCount === 0}
              className="relative block w-full h-2 rounded-full appearance-none cursor-pointer
                         disabled:opacity-50 disabled:cursor-not-allowed"
            />
          </div>
          <div
            className="text-sm font-medium text-muted-fg min-w-[60px] text-center tabular-nums"
            title={frameTime && !frameTime.exact ? "Estimated" : undefined}
          >
            {formatFrameTime(frameTime)}
          </div>
        </div>
      </div>
    </main>
  );
}

/** "3 of 31" when on a match, otherwise how many there are. */
function matchLabel(
  stops: Array<Stop>,
  matches: Array<DayMatch> | null,
  error: Error | null,
  currentIndex: number,
): string {
  if (error) return "Search failed";
  if (matches === null) return "…";
  if (stops.length === 0) return "No matches";
  const current = rangeAt(stops, currentIndex);
  if (current !== -1) return `${current + 1} of ${stops.length}`;
  return stops.length === 1 ? "1 match" : `${stops.length} matches`;
}

/** Where the searched text was on screen, drawn just above the scrubber. */
function MatchMarks({
  stops,
  frameCount,
  currentIndex,
}: {
  stops: Array<Stop>;
  frameCount: number;
  currentIndex: number;
}): React.ReactNode {
  if (stops.length === 0 || frameCount === 0) return null;
  const current = rangeAt(stops, currentIndex);
  // Where the slider puts its thumb's centre for frame `index`.
  const at = (index: number): number => (Math.min(index, frameCount - 1) / Math.max(frameCount - 1, 1)) * 100;
  return (
    <div
      data-testid="match-marks"
      // Inset by the track's padding (4px) plus half the 6px playhead, the
      // range the playhead's centre moves over.
      className="absolute left-[7px] right-[7px] -top-3 h-3 pointer-events-none"
      aria-hidden="true"
    >
      {stops.map((stop, i) => (
        <span
          key={stop.index}
          className={`absolute bottom-0 h-3 min-w-[3px] -ml-px rounded-sm ${
            i === current ? "bg-yellow-600" : "bg-yellow-400"
          }`}
          style={{
            left: `${at(stop.index)}%`,
            width: `${at(stop.endIndex - 1) - at(stop.index)}%`,
          }}
        />
      ))}
    </div>
  );
}

/** The other days the searched text appears on, as links to them. */
function OtherDays({
  counts,
  today,
  showAll,
  onShowAll,
  onPick,
}: {
  counts: Array<{ day: string; count: number }>;
  today: string;
  showAll: boolean;
  onShowAll: () => void;
  onPick: (date: string) => void;
}): React.ReactNode {
  if (counts.length === 0) return null;
  const shown = showAll ? counts : counts.slice(0, OTHER_DAYS_SHOWN);
  const hidden = counts.length - shown.length;
  return (
    <nav aria-label="Other days with matches" className="flex flex-wrap items-center gap-2 mt-2 text-sm">
      <span className="text-muted-fg">Also on</span>
      {shown.map(({ day, count }) => (
        <button
          key={day}
          type="button"
          onClick={() => onPick(day)}
          className={`flex items-center gap-1.5 px-2.5 py-0.5 bg-card border border-border rounded-full transition-colors hover:bg-muted ${focusRing}`}
        >
          {day === today ? "Today" : day}
          <span className="text-muted-fg tabular-nums">{count}</span>
        </button>
      ))}
      {hidden > 0 && (
        <button type="button" onClick={onShowAll} className={`rounded-sm font-medium text-fg underline-offset-4 hover:underline ${focusRing}`}>
          {hidden === 1 ? "1 more day" : `${hidden} more days`}
        </button>
      )}
    </nav>
  );
}

/** `value`, once it has stopped changing for `delayMs`. */
function useDebounced<T>(value: T, delayMs: number): T {
  const [settled, setSettled] = React.useState(value);
  React.useEffect(() => {
    const timer = setTimeout(() => setSettled(value), delayMs);
    return (): void => clearTimeout(timer);
  }, [value, delayMs]);
  return settled;
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
  /** The last frame that finished loading successfully. */
  loaded: string | null;
  onLoad: () => void;
  onError: () => void;
} {
  const [src, setSrc] = React.useState<string | null>(null);
  const [frameFailed, setFrameFailed] = React.useState(false);
  const [loaded, setLoaded] = React.useState<string | null>(null);
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
      if (!failed) setLoaded(srcRef.current);
      if (wantedRef.current !== srcRef.current) {
        show(wantedRef.current);
      }
    },
    [show],
  );

  return {
    src,
    frameFailed,
    loaded,
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
