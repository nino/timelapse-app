import { getVersion } from "@tauri-apps/api/app";
import React, { useEffect, useState } from "react";

import icon from "../src-tauri/icons/128x128@2x.png";
import { byDay, formatDay, type ChangelogEntry } from "./about";
import "./App.css";

/**
 * The About window (app menu → About Timelapse App): the app's name and
 * version, and what changed in each version, newest first. The change log is
 * compiled in at build time, so it shows the changes up to this build.
 */
export function AboutView({ changelog }: { changelog: ChangelogEntry[] }): React.ReactNode {
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    getVersion().then(setVersion, (e: unknown) => console.error("Could not read the app version:", e));
  }, []);

  return (
    <main className="min-h-screen bg-page p-4 text-sm flex flex-col gap-4">
      <header className="flex flex-col items-center gap-1 pt-2 text-center">
        <img src={icon} alt="" className="size-16" />
        <h1 className="font-title text-xl">Timelapse App</h1>
        {version && <p className="text-muted-fg tabular-nums">Version {version}</p>}
      </header>
      <section aria-label="Change log" className="flex flex-col gap-3">
        <h2 className="text-xs font-semibold uppercase tracking-wide text-muted-fg">What's new</h2>
        {changelog.length === 0 ? (
          <p className="text-muted-fg">This build has no change log.</p>
        ) : (
          byDay(changelog).map((day) => (
            <section key={day.date} aria-label={formatDay(day.date)} className="rounded-xl border border-border bg-card px-4 py-3">
              <h3 className="font-semibold">{formatDay(day.date)}</h3>
              <ul className="mt-1 flex flex-col gap-1">
                {day.entries.map((entry) => (
                  <li key={entry.pr} className="grid grid-cols-[1fr_auto] items-baseline gap-x-3">
                    <span className="min-w-0 break-words">{entry.title}</span>
                    <span className="text-xs text-muted-fg tabular-nums">{entry.version}</span>
                  </li>
                ))}
              </ul>
            </section>
          ))
        )}
      </section>
    </main>
  );
}
