import React from "react";

import "./App.css";
import { ago, count, formatDuration, until, type Activity, type Failure } from "./activity";
import { useActivity } from "./hooks/useActivity";

/** The Activity window (Window → Activity): what capture, video conversion and OCR are doing. */
export function ActivityView(): React.ReactNode {
  const { activity, now, error } = useActivity();

  return (
    <main className="min-h-screen bg-page p-4 text-sm flex flex-col gap-3">
      {error && (
        <p role="alert" className="text-danger">
          Could not ask the app what it is doing: {error}
        </p>
      )}
      {activity ? (
        <>
          <Power onAcPower={activity.onAcPower} />
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
}: {
  title: string;
  tone: Tone;
  headline: string;
  children?: React.ReactNode;
}): React.ReactNode {
  return (
    <section aria-label={title} className="rounded-xl border border-border bg-card px-4 py-3">
      <h2 className="text-xs font-semibold uppercase tracking-wide text-muted-fg">{title}</h2>
      <p className="mt-1 flex items-center gap-2 font-medium">
        <span aria-hidden className={`size-2 shrink-0 rounded-full ${dotColour[tone]}`} />
        <span data-testid={`${title}-headline`}>{headline}</span>
      </p>
      {children && <dl className="mt-2 grid grid-cols-[auto_1fr] gap-x-4 gap-y-1 tabular-nums">{children}</dl>}
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

function Power({ onAcPower }: { onAcPower: boolean | null }): React.ReactNode {
  if (onAcPower === null) return null;
  return (
    <Section
      title="Power"
      tone={onAcPower ? "active" : "paused"}
      headline={onAcPower ? "On AC power" : "On battery: conversion and OCR wait until the Mac is plugged in"}
    />
  );
}

function Capture({ capture, now }: { capture: Activity["capture"]; now: number }): React.ReactNode {
  const { running, lastFrame, framesSaved, lastBlackAt, lastError, failuresInARow, failuresBeforeBackoff } = capture;
  const lastSaved = lastFrame ? Date.parse(lastFrame.at) : 0;
  let tone: Tone = "active";
  let headline = "Capturing every second";
  if (!running) {
    tone = "quiet";
    headline = "Stopped";
  } else if (failuresInARow >= failuresBeforeBackoff) {
    tone = "failing";
    headline = `Capture failed ${failuresInARow} times in a row; trying again every minute`;
  } else if (failuresInARow > 0) {
    // A one-off failure, such as the focused window closing mid-capture.
    tone = "paused";
    headline = "Capture failed; trying again in a second";
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
      headline = `Paused on battery; next check ${next}`;
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
      {current && <Row label="Running for">{formatDuration((now - Date.parse(current.startedAt)) / 1000)}</Row>}
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

function Ocr({ ocr, now }: { ocr: Activity["ocr"]; now: number }): React.ReactNode {
  const { state, current, remaining, recognized, skipped, nextCheckAt, lastError } = ocr;
  const next = nextCheckAt ? until(nextCheckAt, now) : "soon";
  let tone: Tone = "quiet";
  let headline: string;
  switch (state) {
    case "working":
      tone = "active";
      headline = current
        ? `Reading ${current.day} #${String(current.number).padStart(5, "0")}`
        : "Reading";
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
      {state === "working" && <Row label="Left">{count(remaining, "frame")} in this pass</Row>}
      <Row label="Since launch">
        {count(recognized, "frame")} read, {skipped.toLocaleString("en-US")} unchanged
      </Row>
      <ErrorRow failure={lastError} now={now} />
    </Section>
  );
}
