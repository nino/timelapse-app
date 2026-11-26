import { Popover, PopoverButton, PopoverPanel } from "@headlessui/react";
import React from "react";

import { focusRing, menuItem, menuPanel } from "./ui";

// How many other days the find bar names before folding the rest away.
const OTHER_DAYS_SHOWN = 4;

/**
 * The other days the searched text appears on, as links to them. The first
 * few are chips in a row; the rest sit in a scrollable list behind "N more
 * days", so a word found on hundreds of days never pushes the frame away.
 */
export function OtherDays({
  counts,
  today,
  onPick,
}: {
  counts: Array<{ day: string; count: number }>;
  today: string;
  onPick: (date: string) => void;
}): React.ReactNode {
  if (counts.length === 0) return null;
  const shown = counts.slice(0, OTHER_DAYS_SHOWN);
  const hidden = counts.slice(OTHER_DAYS_SHOWN);
  const label = (day: string): string => (day === today ? "Today" : day);
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
          {label(day)}
          <span className="text-muted-fg tabular-nums">{count}</span>
        </button>
      ))}
      {hidden.length > 0 && (
        <Popover>
          <PopoverButton
            className={`flex items-center gap-1.5 rounded-full px-2.5 py-0.5 font-medium text-fg transition-colors hover:bg-muted data-open:bg-muted ${focusRing}`}
          >
            {hidden.length === 1 ? "1 more day" : `${hidden.length} more days`}
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none" stroke="currentColor" strokeWidth="1.5" aria-hidden="true">
              <path d="m2.5 4 2.5 2.5L7.5 4" />
            </svg>
          </PopoverButton>
          {/* Styled like the dropdowns' lists. The `!` beats the max-height
              Headless UI sets inline, which runs to the window's edge. */}
          <PopoverPanel
            anchor={{ to: "bottom start", gap: 4 }}
            className={`${menuPanel} max-h-[min(24rem,var(--anchor-max-height,100vh))]! min-w-44`}
          >
            {({ close }): React.ReactElement => (
              <>
                {hidden.map(({ day, count }) => (
                  <button
                    key={day}
                    type="button"
                    onClick={() => {
                      onPick(day);
                      close();
                    }}
                    className={`${menuItem} w-full justify-between px-2.5 outline-none hover:bg-muted focus-visible:bg-muted`}
                  >
                    {label(day)}
                    <span className="text-muted-fg">{count}</span>
                  </button>
                ))}
              </>
            )}
          </PopoverPanel>
        </Popover>
      )}
    </nav>
  );
}
