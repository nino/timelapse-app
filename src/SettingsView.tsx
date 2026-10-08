import React, { useEffect, useRef, useState } from "react";

import "./App.css";
import { growSettingsWindow, getSettings, setUpdateAutomatically, type Settings } from "./settings";
import { focusRing } from "./ui";

/**
 * The Settings window (app menu → Settings…). As on macOS, every change
 * applies and is saved the moment it is made; there is no Save button.
 */
export function SettingsView(): React.ReactNode {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [error, setError] = useState<string | null>(null);
  const main = useRef<HTMLElement>(null);
  const loaded = settings !== null || error !== null;

  useEffect(() => {
    getSettings().then(setSettings, (e: unknown) => setError(String(e)));
  }, []);

  // The window is exactly as tall as its content, so the margin below the
  // card matches the margin above it. Refitted whenever the content's height
  // changes, which includes the fonts arriving and rewrapping the text.
  useEffect(() => {
    const content = main.current;
    if (!loaded || !content) return;
    const fit = (): void => {
      const by = Math.ceil(content.getBoundingClientRect().height) - window.innerHeight;
      growSettingsWindow(by).catch((e: unknown) => console.error("Could not size the Settings window:", e));
    };
    fit();
    // Again if the viewport turns out different once the window is shown.
    window.addEventListener("resize", fit);
    // happy-dom, in tests, has no ResizeObserver.
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(fit);
    observer?.observe(content);
    return (): void => {
      window.removeEventListener("resize", fit);
      observer?.disconnect();
    };
  }, [loaded]);

  const toggleUpdates = (enabled: boolean): void => {
    const previous = settings;
    setSettings((s) => (s ? { ...s, updateAutomatically: enabled } : s));
    setError(null);
    setUpdateAutomatically(enabled).then(setSettings, (e: unknown) => {
      setSettings(previous);
      setError(String(e));
    });
  };

  return (
    <main ref={main} className="bg-page p-4 text-sm flex flex-col gap-3">
      {error && (
        <p role="alert" className="text-danger">
          Could not save the setting: {error}
        </p>
      )}
      {settings ? (
        <section aria-label="Updates" className="rounded-xl border border-border bg-card px-4 py-3">
          <label className="flex items-start gap-3">
            <input
              type="checkbox"
              className={`mt-0.5 size-4 shrink-0 rounded accent-primary ${focusRing}`}
              checked={settings.updateAutomatically}
              onChange={(e) => toggleUpdates(e.currentTarget.checked)}
            />
            <span>
              <span className="font-medium">Update automatically</span>
              <span className="mt-0.5 block text-muted-fg">
                Installs new versions in the background and relaunches when the window isn't in use.
              </span>
            </span>
          </label>
        </section>
      ) : (
        !error && <p className="text-muted-fg">Loading…</p>
      )}
    </main>
  );
}
