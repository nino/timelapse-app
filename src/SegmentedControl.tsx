import React from "react";

import { focusRing } from "./ui";

/**
 * A row of mutually exclusive choices: a grey track with the selected choice
 * on a white pill (a lighter grey in dark mode) inset from the track's edge,
 * and the other choices in grey text. The pill's edge is an unblurred ring,
 * not a shadow, because WebKit repaints blurred shadows on every frame of a
 * resize.
 */
export function SegmentedControl<T extends string | number>({
  label,
  options,
  value,
  onChange,
  itemClassName = "px-3",
}: {
  /** The group's accessible name. */
  label: string;
  options: { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
  /** Extra classes for every choice, such as a fixed width. */
  itemClassName?: string;
}): React.ReactNode {
  return (
    <div role="group" aria-label={label} className="flex h-7.5 w-fit gap-0.5 rounded-xl bg-track/70 p-0.5 dark:bg-page">
      {options.map((option) => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            type="button"
            aria-pressed={selected}
            onClick={() => onChange(option.value)}
            className={`inline-flex items-center justify-center rounded-[10px] transition-colors ${itemClassName} ${
              selected
                ? "bg-card font-semibold text-fg ring-1 ring-border dark:bg-muted"
                : "text-muted-fg hover:bg-card/50 hover:text-fg dark:hover:bg-muted/50"
            } ${focusRing}`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
