import { ListboxOption } from "@headlessui/react";
import React from "react";

import { menuItem } from "./ui";

// Shared by the toolbar dropdowns (day, app and speed pickers), so they look
// and behave the same. Their class strings are in `ui.ts`.

/**
 * A row in a dropdown's list: the check mark (shown when selected), then
 * `children`.
 */
export function DropdownOption<T>({ value, children }: { value: T; children: React.ReactNode }): React.ReactNode {
  return (
    <ListboxOption
      value={value}
      className={`${menuItem} pr-2.5 pl-1.5 data-focus:bg-muted data-selected:font-semibold`}
    >
      <Check />
      {children}
    </ListboxOption>
  );
}

/**
 * For every dropdown's `onKeyPressCapture`. Headless UI's ListboxButton
 * cancels every keypress, and WebKit sends one for ⌘ shortcuts too and keeps
 * a cancelled one from the menu, so a focused dropdown blocked ⌘H, ⌘Q, ⌥⌘A
 * and the rest. Stopping ⌘ and Ctrl keypresses before the button sees them
 * lets them through.
 */
export function passShortcutsOn(e: React.KeyboardEvent): void {
  if (e.metaKey || e.ctrlKey) e.stopPropagation();
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
