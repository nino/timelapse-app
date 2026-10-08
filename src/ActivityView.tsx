import React from "react";

import "./App.css";
import {
  ago,
  count,
  encodeProgress,
  encodeSeconds,
  formatDuration,
  setBoostAllowBattery,
  startBoost,
  stopBoost,
  until,
  type Activity,
  type Encoding,
  type Failure,
} from "./activity";
import { useActivity } from "./hooks/useActivity";
import { focusRing, segmentButton } from "./ui";

/** The Activity window (Window → Activity): what capture, video conversion and OCR are doing. */
export function ActivityView(): React.ReactNode {
  const { activity, now, error, refresh } = useActivity();

  return (
    <main className="min-h-screen bg-page p-4 text-sm flex flex-col gap-3">
      {error && (
        <p role="alert" className="text-danger">
          Could not ask the app what it is doing: {error}
        </p>
      )}
      {activity ? (
        <>
          <Power onAcPower={activity.onAcPower} boost={activity.boost} />
          <Boost boost={activity.boost} onAcPower={activity.onAcPower} now={now} onChange={refresh} />
          <Capture capture={activity.capture} now={now} />
          <Conversion conversion={activity.conversion} now={now} />
          <Ocr ocr={activity.ocr} now={now} />
        </>
      ) : (
        !error && <p className="text-muted-fg">Loading…</p>
      )}
    </main>
  );
}

type Tone = "active" | "quiet" | "paused" | "failing";

const dotColour: Record<Tone, string> = {
  active: "bg-success",
  quiet: "bg-faint-fg",
  paused: "bg-playhead",
  failing: "bg-danger",
};

function Section({
  title,
  tone,
  headline,
  children,
  footer,
}: {
  title: string;
  tone: Tone;
  headline: string;
  children?: React.ReactNode;
  /** Shown below the rows. */
  footer?: React.ReactNode;
}): React.ReactNode {
  return (
    <section aria-label={title} className="rounded-xl border border-border bg-card px-4 py-3">
      <h2 className="text-xs font-semibold uppercase tracking-wide text-muted-fg">{title}</h2>
      <p className="mt-1 flex items-center gap-2 font-medium">
        <span aria-hidden className={`size-2 shrink-0 rounded-full ${dotColour[tone]}`} />
        <span data-testid={`${title}-headline`}>{headline}</span>
      </p>
      {children && <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 tabular-nums">{children}</dl>}
      {footer}
    </section>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }): React.ReactNode {
  return (
    <>
      <dt className="text-muted-fg">{label}</dt>
      <dd className="min-w-0 break-words">{children}</dd>
    </>
  );
}

function ErrorRow({ failure, now }: { failure: Failure | null; now: number }): React.ReactNode {
  if (!failure) return null;
  return (
    <Row label="Last error">
      <span className="text-danger">{failure.message}</span>{" "}
      <span className="whitespace-nowrap text-muted-fg">({ago(failure.at, now)})</span>
    </Row>
  );
}

function Power({ onAcPower, boost }: { onAcPower: boolean | null; boost: Activity["boost"] }): React.ReactNode {
  if (onAcPower === null) return null;
  let headline = "On AC power";
  if (!onAcPower) {
    headline = boost?.allowBattery
      ? "On battery: boosting anyway"
      : "On battery: conversion and OCR wait until the Mac is plugged in";
  }
  return <Section title="Power" tone={onAcPower || boost?.allowBattery ? "active" : "paused"} headline={headline} />;
}

/** Boost lengths on offer, in minutes. */
export const BOOST_MINUTES = [10, 20, 30, 60, 120];
const DEFAULT_BOOST_MINUTES = 20;

function boostLabel(minutes: number): string {
  return minutes < 60 ? `${minutes}m` : `${minutes / 60}h`;
}

const primaryButton = `rounded-xl bg-primary px-3 py-1 font-medium text-primary-fg transition-colors hover:bg-primary/90 disabled:opacity-50 ${focusRing}`;
const plainButton = `rounded-xl border border-border bg-card px-3 py-1 font-medium transition-colors hover:bg-muted disabled:opacity-50 ${focusRing}`;

/**
 * Lets conversion and OCR run at full speed for a while, and on battery too if
 * asked. Normally both run at background priority, a batch at most every ten
 * minutes, on AC power only.
 */
function Boost({
  boost,
  onAcPower,
  now,
  onChange,
}: {
  boost: Activity["boost"];
  onAcPower: boolean | null;
  now: number;
  onChange: () => void;
}): React.ReactNode {
  const [minutes, setMinutes] = React.useState(DEFAULT_BOOST_MINUTES);
  // Before a boost starts, the checkbox is the window's own choice; during
  // one it shows (and changes) the boost's.
  const [allowBattery, setAllowBattery] = React.useState(false);
  const [busy, setBusy] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  const on = boost !== null && Date.parse(boost.until) > now;

  const run = (action: () => Promise<unknown>): void => {
    setBusy(true);
    setError(null);
    action()
      .catch((e: unknown) => setError(String(e)))
      .finally(() => {
        setBusy(false);
        onChange();
      });
  };

  const toggleBattery = (allow: boolean): void => {
    setAllowBattery(allow);
    if (on) run(() => setBoostAllowBattery(allow));
  };

  const batteryChecked = on ? boost.allowBattery : allowBattery;
  const waitingForPower = on && onAcPower === false && !boost.allowBattery;
  let headline = "Off: conversion and OCR go easy on the CPU";
  if (on) {
    const left = formatDuration((Date.parse(boost.until) - now) / 1000);
    headline = waitingForPower
      ? `Waiting for AC power (${left} left); tick “Also on battery” to start now`
      : `Running at full speed for another ${left}`;
  }

  return (
    <Section
      title="Boost"
      tone={waitingForPower ? "paused" : on ? "active" : "quiet"}
      headline={headline}
      footer={
        <div className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2">
          {!on && (
            <div role="group" aria-label="Boost length" className="flex h-7.5 rounded-xl border border-border bg-card">
              {BOOST_MINUTES.map((m, i) => (
                <button
                  key={m}
                  type="button"
                  aria-pressed={m === minutes}
                  onClick={() => setMinutes(m)}
                  className={`${segmentButton} w-10 tabular-nums ${i === 0 ? "rounded-l-[11px]" : "border-l border-border"} ${
                    i === BOOST_MINUTES.length - 1 ? "rounded-r-[11px]" : ""
                  } ${m === minutes ? "bg-muted font-semibold" : "text-muted-fg"}`}
                >
                  {boostLabel(m)}
                </button>
              ))}
            </div>
          )}
          {on ? (
            <button type="button" disabled={busy} onClick={() => run(stopBoost)} className={plainButton}>
              Stop boost
            </button>
          ) : (
            <button type="button" disabled={busy} onClick={() => run(() => startBoost(minutes, allowBattery))} className={primaryButton}>
              Boost!
            </button>
          )}
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              className={`size-4 shrink-0 rounded accent-primary ${focusRing}`}
              checked={batteryChecked}
              disabled={busy}
              onChange={(e) => toggleBattery(e.currentTarget.checked)}
            />
            Also on battery
          </label>
          {error && (
            <p role="alert" className="basis-full text-danger">
              {error}
            </p>
          )}
        </div>
      }
    />
  );
}

function Capture({ capture, now }: { capture: Activity["capture"]; now: number }): React.ReactNode {
  const { running, lastFrame, framesSaved, lastBlackAt, lastError } = capture;
  const lastSaved = lastFrame ? Date.parse(lastFrame.at) : 0;
  let tone: Tone = "active";
  let headline = "Capturing every second";
  if (!running) {
    tone = "quiet";
    headline = "Stopped";
  } else if (lastError && Date.parse(lastError.at) > lastSaved) {
    tone = "failing";
    headline = "Capture failed; trying again a minute after each failure";
  } else if (lastBlackAt && Date.parse(lastBlackAt) > lastSaved) {
    tone = "paused";
    headline = "The screen is dark; checking again every 10s";
  } else if (!lastFrame) {
    tone = "quiet";
    headline = "Starting";
  }

  return (
    <Section title="Capture" tone={tone} headline={headline}>
      {lastFrame && (
        <Row label="Last frame">
          {lastFrame.day} #{String(lastFrame.number).padStart(5, "0")}{" "}
          <span className="whitespace-nowrap text-muted-fg">({ago(lastFrame.at, now)})</span>
        </Row>
      )}
      <Row label="Since launch">{count(framesSaved, "frame")} saved</Row>
      <ErrorRow failure={lastError} now={now} />
    </Section>
  );
}

function hourLabel(day: string, hour: number): string {
  return `${day} ${String(hour).padStart(2, "0")}:00`;
}

function Conversion({
  conversion,
  now,
}: {
  conversion: Activity["conversion"];
  now: number;
}): React.ReactNode {
  const { state, current, nextCheckAt, ready, waitingForOcr, last, videosMade, framesDeleted } = conversion;
  const next = nextCheckAt ? until(nextCheckAt, now) : "soon";
  let tone: Tone = "quiet";
  let headline: string;
  switch (state) {
    case "working":
      tone = "active";
      headline = current
        ? `Encoding ${hourLabel(current.day, current.hour)} (${count(current.frames, "frame")})`
        : "Encoding";
      break;
    case "resting":
      headline = ready > 0 ? `Next batch starts ${next}` : `Next check ${next}`;
      break;
    case "idle":
      headline = `Nothing to convert; next check ${next}`;
      break;
    case "onBattery":
      tone = "paused";
      headline = current
        ? `Encoding ${hourLabel(current.day, current.hour)} paused on battery`
        : `Paused on battery; next check ${next}`;
      break;
    case "unavailable":
      headline = "Not available";
      break;
    case "starting":
      headline = "Starting";
      break;
  }

  return (
    <Section title="Video conversion" tone={tone} headline={headline}>
      {current && <EncodeProgress encoding={current} now={now} />}
      {current && <Row label="Running for">{formatDuration(encodeSeconds(current, now))}</Row>}
      <Row label="Waiting">
        {count(ready, "hour")} ready, {waitingForOcr} waiting for OCR
      </Row>
      {last && (
        <Row label="Last batch">
          {last.error ? (
            <span className="text-danger">
              {last.video} failed: {last.error}
            </span>
          ) : (
            <>
              {last.video} in {formatDuration(last.tookSecs)}
            </>
          )}{" "}
          <span className="whitespace-nowrap text-muted-fg">({ago(last.finishedAt, now)})</span>
        </Row>
      )}
      <Row label="Since launch">
        {count(videosMade, "video")} made, {count(framesDeleted, "screenshot")} deleted
      </Row>
    </Section>
  );
}

function EncodeProgress({ encoding, now }: { encoding: Encoding; now: number }): React.ReactNode {
  const { percent, secondsLeft } = encodeProgress(encoding, now);
  return (
    <Row label="Progress">
      <span className="flex items-center gap-2">
        <span
          role="progressbar"
          aria-label="Encoding progress"
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
          className="h-1.5 w-24 shrink-0 overflow-hidden rounded-full bg-track"
        >
          <span className="block h-full bg-primary" style={{ width: `${percent}%` }} />
        </span>
        <span>
          {percent}%
          {secondsLeft !== null && (
            <span className="text-muted-fg">, about {formatDuration(secondsLeft)} left</span>
          )}
        </span>
      </span>
    </Row>
  );
}

function Ocr({ ocr, now }: { ocr: Activity["ocr"]; now: number }): React.ReactNode {
  const { state, current, remaining, recognized, skipped, nextCheckAt, lastError, readingVideo, videoDaysLeft } =
    ocr;
  const next = nextCheckAt ? until(nextCheckAt, now) : "soon";
  let tone: Tone = "quiet";
  let headline: string;
  switch (state) {
    case "working":
      tone = "active";
      if (readingVideo) {
        headline = current
          ? `Reading the ${current.day} video, frame ${current.number.toLocaleString("en-US")}`
          : "Reading old videos";
      } else {
        headline = current
          ? `Reading ${current.day} #${String(current.number).padStart(5, "0")}`
          : "Reading";
      }
      break;
    case "idle":
    case "resting":
      headline = `Caught up; next look ${next}`;
      break;
    case "onBattery":
      tone = "paused";
      headline = `Paused on battery; next check ${next}`;
      break;
    case "unavailable":
      headline = "Not available on this platform";
      break;
    case "starting":
      headline = "Starting";
      break;
  }

  return (
    <Section title="OCR" tone={tone} headline={headline}>
      {state === "working" && !readingVideo && <Row label="Left">{count(remaining, "frame")} in this pass</Row>}
      {videoDaysLeft !== null && (
        <Row label="Old videos">{videoDaysLeft === 0 ? "All read" : `${count(videoDaysLeft, "day")} left to read`}</Row>
      )}
      <Row label="Since launch">
        {count(recognized, "frame")} read, {skipped.toLocaleString("en-US")} unchanged
      </Row>
      <ErrorRow failure={lastError} now={now} />
    </Section>
  );
}
