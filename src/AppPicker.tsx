import { Listbox, ListboxButton, ListboxOption, ListboxOptions } from "@headlessui/react";
import React from "react";

import { formatFrames, type DayApps } from "./apps";
import { Check, Chevron } from "./DayPicker";
import { focusRing } from "./ui";

/**
 * The app filter beside the find bar, built like the day picker: "All apps",
 * then the day's apps, most-used first, with how long each was in front.
 * A picked app stays picked on other days, even ones it isn't on.
 */
export function AppPicker({
  dayApps,
  value,
  onChange,
}: {
  /** Null while the day's apps load. */
  dayApps: DayApps | null;
  value: string | null;
  onChange: (app: string | null) => void;
}): React.ReactNode {
  const apps = dayApps?.apps ?? [];
  const missing = value !== null && dayApps !== null && !apps.some((app) => app.name === value);
  return (
    <Listbox value={value} onChange={onChange}>
      <ListboxButton
        aria-label="App"
        title="Only play the times an app was in front"
        className={`flex h-7.5 max-w-48 min-w-0 items-center gap-2 rounded-xl border border-input bg-field pr-2 pl-2.5 text-sm ${focusRing} ${
          value === null ? "text-muted-fg" : "font-medium text-fg"
        }`}
      >
        <svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" aria-hidden="true" className="shrink-0 text-muted-fg">
          <rect x="3" y="4" width="18" height="16" rx="3" />
          <path d="M3 9h18" />
        </svg>
        <span className={`truncate ${missing ? "line-through decoration-muted-fg" : ""}`}>
          {value ?? "All apps"}
        </span>
        <Chevron />
      </ListboxButton>
      <ListboxOptions
        anchor={{ to: "bottom end", gap: 4 }}
        className="z-10 max-h-96 w-64 overflow-y-auto rounded-2xl border border-border bg-card p-1 text-sm text-fg shadow-md outline-none"
      >
        <AppOption value={null} label="All apps" />
        {missing && <AppOption value={value} label={value} detail="Not on this day" />}
        {apps.map((app) => (
          <AppOption key={app.name} value={app.name} label={app.name} detail={formatFrames(app.frames)} />
        ))}
        {dayApps !== null && apps.length === 0 && (
          <p className="px-2.5 py-1.5 text-muted-fg">
            No app was recorded on this day.
          </p>
        )}
      </ListboxOptions>
    </Listbox>
  );
}

function AppOption({
  value,
  label,
  detail,
}: {
  value: string | null;
  label: string;
  detail?: string;
}): React.ReactNode {
  return (
    <ListboxOption
      value={value}
      className="flex h-7 cursor-default items-center gap-2 rounded-[11px] pr-2.5 pl-1.5 select-none data-focus:bg-muted data-selected:font-semibold"
    >
      <Check />
      <span className="min-w-0 flex-1 truncate">{label}</span>
      {detail !== undefined && (
        <span className="shrink-0 text-xs font-normal text-muted-fg tabular-nums">{detail}</span>
      )}
    </ListboxOption>
  );
}
