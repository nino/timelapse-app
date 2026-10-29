import { Listbox, ListboxButton, ListboxOption, ListboxOptions } from "@headlessui/react";
import React from "react";

import type { DayStatus, Stage } from "./frames";
import { useDayStatuses } from "./hooks/useLibrary";
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
      {({ open }): React.ReactElement => (
        <>
          <ListboxButton
            aria-label="Day"
            className={`flex h-7.5 min-w-0 items-center gap-3 rounded-full border border-input bg-field pr-3.5 pl-4 text-sm font-medium tabular-nums ${focusRing}`}
          >
            {/* One line: when the toolbar is tight the label ends in "…" rather than wrapping. */}
            {value === null ? (
              <span className="truncate text-muted-fg">Select a day…</span>
            ) : (
              <span className="truncate">{label(value)}</span>
            )}
            <Chevron />
          </ListboxButton>
          <DayOptions days={days} label={label} open={open} />
        </>
      )}
    </Listbox>
  );
}

function DayOptions({
  days,
  label,
  open,
}: {
  days: Array<string>;
  label: (date: string) => string;
  open: boolean;
}): React.ReactNode {
  // Asked for on every opening, so a closed picker costs nothing.
  const statuses = useDayStatuses(open);
  return (
    <ListboxOptions
      anchor={{ to: "bottom start", gap: 4 }}
      className="z-10 max-h-96 min-w-(--button-width) overflow-y-auto rounded-2xl border border-border bg-card p-1 text-sm text-fg tabular-nums shadow-md outline-none"
    >
      {/* Options are concentric with the list: 16px minus the 1px border
          and 4px padding. */}
      {monthsNewestFirst(days).map(({ month, dates }) => (
        <div key={month} role="group" aria-label={monthLabel(month)}>
          {/* Not an option, so Headless UI's arrow keys and typeahead skip it. */}
          <div
            aria-hidden="true"
            className="pt-2 pr-2.5 pb-1 pl-6.5 text-xs font-medium text-muted-fg select-none"
          >
            {monthLabel(month)}
          </div>
          {dates.map((date) => (
            <ListboxOption
              key={date}
              value={date}
              className="flex h-7 cursor-default items-center justify-between gap-3 rounded-[11px] pr-2.5 pl-1.5 select-none data-focus:bg-muted data-selected:font-semibold"
            >
              <span className="flex items-center gap-1.5">
                <Check />
                {label(date)}
              </span>
              <Status status={statuses.get(date)} />
            </ListboxOption>
          ))}
        </div>
      ))}
    </ListboxOptions>
  );
}

/**
 * Days newest first, in runs of one month (`YYYY-MM`). Takes `days` oldest
 * first, as `list_days` returns them.
 */
function monthsNewestFirst(days: Array<string>): Array<{ month: string; dates: Array<string> }> {
  const groups: Array<{ month: string; dates: Array<string> }> = [];
  for (let i = days.length - 1; i >= 0; i--) {
    const date = days[i];
    const month = date.slice(0, 7);
    const last = groups.at(-1);
    if (last?.month === month) last.dates.push(date);
    else groups.push({ month, dates: [date] });
  }
  return groups;
}

const MONTH_NAMES = [
  "January",
  "February",
  "March",
  "April",
  "May",
  "June",
  "July",
  "August",
  "September",
  "October",
  "November",
  "December",
];

/** `2026-10` → `October 2026`. */
function monthLabel(month: string): string {
  const [year, number] = month.split("-");
  return `${MONTH_NAMES[Number(number) - 1] ?? number} ${year}`;
}

const STAGE_WORDS: Record<Stage, string> = { todo: "not started", partial: "partly done", done: "done" };

/**
 * Two marks: the text glyph for OCR, the film glyph for video conversion,
 * each in the success colour once done, faint while still to do, and green
 * on its left half once started. Nothing until the status is known.
 */
function Status({ status }: { status: DayStatus | undefined }): React.ReactNode {
  if (!status) return <span className="w-7" />;
  const text = `OCR ${STAGE_WORDS[status.ocr]}, video ${STAGE_WORDS[status.video]}`;
  return (
    <span className="flex w-7 items-center gap-1" title={text}>
      <span className="sr-only">{text}</span>
      <Mark stage={status.ocr}>
        <path
          d="M2 3h8M2 6h8M2 9h5"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
        />
      </Mark>
      <Mark stage={status.video}>
        <rect x="1.5" y="2.5" width="9" height="7" rx="1.5" fill="none" stroke="currentColor" strokeWidth="1.5" />
        <path d="M5 4.5v3l2.5-1.5z" fill="currentColor" />
      </Mark>
    </span>
  );
}

const DONE = "text-success";
const TODO = "text-faint-fg opacity-50";

/**
 * A 12px glyph drawn in `currentColor`. A partial one is drawn twice: faint,
 * then green inside a nested 6px-wide viewport, which clips it to the left
 * half without needing a `clipPath` id.
 */
function Mark({ stage, children }: { stage: Stage; children: React.ReactNode }): React.ReactNode {
  if (stage !== "partial") {
    return (
      <svg viewBox="0 0 12 12" aria-hidden="true" className={`size-3 ${stage === "done" ? DONE : TODO}`}>
        {children}
      </svg>
    );
  }
  return (
    <svg viewBox="0 0 12 12" aria-hidden="true" className="size-3">
      <g className={TODO}>{children}</g>
      <svg width="6" height="12" viewBox="0 0 6 12" className={DONE}>
        {children}
      </svg>
    </svg>
  );
}

export function Chevron(): React.ReactNode {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className="size-4 shrink-0 text-muted-fg">
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

// Only shown on the selected option (ListboxOption sets data-selected). It
// leads the option, as in macOS menus, so anything on the right (the day
// picker's status marks) can sit at the edge.
export function Check(): React.ReactNode {
  return (
    <svg viewBox="0 0 16 16" aria-hidden="true" className="invisible size-3.5 shrink-0 in-data-selected:visible">
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
