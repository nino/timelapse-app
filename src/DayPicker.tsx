import { Listbox, ListboxButton, ListboxOption, ListboxOptions } from "@headlessui/react";
import React from "react";

import { focusRing } from "./ui";

/**
 * The day picker: the Ninoes Select (github.com/nino/ninoes,
 * app/components/ui/Pager.tsx). A Listbox rather than a native select, so the
 * options open right under the button, lined up with it, instead of wherever
 * the OS puts its popup.
 */
export function DayPicker({
  days,
  today,
  value,
  onChange,
}: {
  /** Oldest first, as `list_days` returns them; shown newest first. */
  days: Array<string>;
  today: string;
  value: string | null;
  onChange: (date: string) => void;
}): React.ReactNode {
  const label = (date: string): string => (date === today ? `${date} (Today)` : date);
  return (
    <Listbox value={value} onChange={(date: string | null) => date && onChange(date)}>
      <ListboxButton
        aria-label="Day"
        className={`flex h-9 items-center gap-3 rounded-full border border-input bg-field pr-3.5 pl-4 text-sm font-medium tabular-nums ${focusRing}`}
      >
        {value === null ? <span className="text-muted-fg">Select a day…</span> : label(value)}
        <Chevron />
      </ListboxButton>
      <ListboxOptions
        anchor={{ to: "bottom start", gap: 4 }}
        className="z-10 max-h-96 min-w-(--button-width) overflow-y-auto rounded-2xl border border-border bg-card p-1 text-sm text-fg tabular-nums shadow-md outline-none"
      >
        {/* Options are concentric with the list: 16px minus the 1px border
            and 4px padding. */}
        {[...days].reverse().map((date) => (
          <ListboxOption
            key={date}
            value={date}
            className="flex h-8 cursor-default items-center justify-between gap-3 rounded-[11px] px-2.5 select-none data-focus:bg-muted data-selected:font-semibold"
          >
            {label(date)}
            <Check />
          </ListboxOption>
        ))}
      </ListboxOptions>
    </Listbox>
  );
}

function Chevron(): React.ReactNode {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className="size-4 text-muted-fg">
      <path
        d="m4 6 4 4 4-4"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

// Only shown on the selected option (ListboxOption sets data-selected).
function Check(): React.ReactNode {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className="invisible size-3.5 in-data-selected:visible">
      <path
        d="m3.5 8.5 3 3 6-7"
        fill="none"
        stroke="currentColor"
        strokeWidth="1.75"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}
