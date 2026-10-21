import React, { useRef } from "react";

import { focusRing } from "./ui";

/**
 * A row of mutually exclusive choices: a grey track with the selected choice
 * on a white pill (a lighter grey in dark mode) inset from the track's edge,
 * and the other choices in grey text. The pill's edge is an unblurred ring,
 * not a shadow, because WebKit repaints blurred shadows on every frame of a
 * resize. Its corners are the track's radius less the track's padding, so
 * the two curves run parallel. The picked choice keeps the same font weight
 * as the rest, so switching never changes the control's width.
 *
 * The pill is one element behind the choices that slides to the selected one:
 * the selected choice's ref callback moves it there by writing its transform
 * and width directly, so moving it re-renders nothing. It only animates once
 * it has been placed, so it doesn't slide in from the left when the control
 * first appears.
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
  /** Classes for every choice, such as a fixed width; they replace the default `px-3`. */
  itemClassName?: string;
}): React.ReactNode {
  const pill = useRef<HTMLSpanElement>(null);

  const placePill = (button: HTMLButtonElement | null): void => {
    const element = pill.current;
    if (!button || !element) {
      return;
    }
    element.style.width = `${button.offsetWidth}px`;
    element.style.transform = `translateX(${button.offsetLeft}px)`;
    element.style.opacity = "1";
    if (!element.dataset.placed) {
      // Read layout so the first position applies before transitions start.
      void element.offsetWidth;
      element.dataset.placed = "";
    }
  };

  return (
    <div role="group" aria-label={label} className="relative flex h-7.5 w-fit gap-0.5 rounded-xl bg-segment p-0.5">
      <span
        ref={pill}
        aria-hidden
        className="pointer-events-none absolute top-0.5 bottom-0.5 left-0 rounded-[10px] bg-segment-pill opacity-0 ring-1 ring-border data-placed:transition-[transform,width] data-placed:duration-200 data-placed:ease-out motion-reduce:transition-none"
      />
      {options.map((option): React.ReactNode => {
        const selected = option.value === value;
        return (
          <button
            key={option.value}
            ref={selected ? placePill : undefined}
            type="button"
            aria-pressed={selected}
            onClick={(): void => onChange(option.value)}
            className={`relative inline-flex items-center justify-center rounded-[10px] transition-colors ${itemClassName} ${
              selected ? "text-fg" : "text-muted-fg hover:bg-segment-pill/50 hover:text-fg"
            } ${focusRing}`}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
