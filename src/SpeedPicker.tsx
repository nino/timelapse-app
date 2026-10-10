import { Listbox, ListboxButton, ListboxOption, ListboxOptions } from "@headlessui/react";
import React from "react";

import { Check, Chevron } from "./DayPicker";
import { focusRing } from "./ui";

/** The playback speeds offered, in frames a second. */
export const PLAYBACK_SPEEDS = [5, 10, 15, 30, 60] as const;
export type PlaybackSpeed = (typeof PLAYBACK_SPEEDS)[number];
/** The rate the old exported videos used: an hour of captures in four minutes. */
export const DEFAULT_PLAYBACK_SPEED: PlaybackSpeed = 15;

const STORAGE_KEY = "playbackSpeed";

/** The speed picked last time, or the default. */
export function savedPlaybackSpeed(): PlaybackSpeed {
  try {
    const saved = Number(localStorage.getItem(STORAGE_KEY));
    return PLAYBACK_SPEEDS.find((speed) => speed === saved) ?? DEFAULT_PLAYBACK_SPEED;
  } catch {
    return DEFAULT_PLAYBACK_SPEED;
  }
}

export function savePlaybackSpeed(speed: PlaybackSpeed): void {
  try {
    localStorage.setItem(STORAGE_KEY, String(speed));
  } catch {
    // Not remembering the speed is fine.
  }
}

/**
 * The fps picker on the right half of the play button's pill, built like the
 * day picker. `className` rounds its outer corners and draws the divider.
 */
export function SpeedPicker({
  value,
  onChange,
  className,
}: {
  value: PlaybackSpeed;
  onChange: (speed: PlaybackSpeed) => void;
  className: string;
}): React.ReactNode {
  return (
    <Listbox value={value} onChange={onChange}>
      <ListboxButton
        aria-label="Playback speed"
        title="Playback speed"
        className={`flex items-center gap-1 pr-2 pl-2.5 text-sm font-medium tabular-nums transition-colors hover:bg-muted ${focusRing} ${className}`}
      >
        {value} fps
        <Chevron />
      </ListboxButton>
      <ListboxOptions
        anchor={{ to: "top start", gap: 4 }}
        className="z-10 min-w-(--button-width) rounded-2xl border border-border bg-card p-1 text-sm text-fg tabular-nums shadow-md outline-none"
      >
        {PLAYBACK_SPEEDS.map((speed) => (
          <ListboxOption
            key={speed}
            value={speed}
            className="flex h-7 cursor-default items-center justify-between gap-3 rounded-[11px] px-2.5 select-none data-focus:bg-muted data-selected:font-semibold"
          >
            {speed} fps
            <Check />
          </ListboxOption>
        ))}
      </ListboxOptions>
    </Listbox>
  );
}
