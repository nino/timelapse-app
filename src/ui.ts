// Class strings from the Ninoes design system (github.com/nino/ninoes,
// app/components/ui/styles.ts), trimmed to what this app uses.

export const focusRing =
  "outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50";

/** A text field's frame. Goes on a wrapper `<label>` when the field has an icon. */
export const fieldFrame =
  "rounded-xl border border-input bg-field transition-[color,box-shadow] focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50";

/** One button in a joined group: the group draws the border, and each button rounds its outer corners. */
export const segmentButton = `inline-flex w-8 items-center justify-center transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50 [&_svg]:shrink-0 ${focusRing}`;

/**
 * A toolbar dropdown's button (day picker, app picker): a pill, like the
 * Ninoes Select. Add the left padding, which depends on whether it leads with
 * an icon.
 */
export const dropdownButton = `flex h-7.5 shrink-0 items-center gap-2 rounded-full border border-input bg-field pr-2.5 text-sm whitespace-nowrap ${focusRing}`;

/** The list a dropdown or popover opens. Add the width. */
export const menuPanel =
  "z-10 max-h-96 overflow-y-auto rounded-2xl border border-border bg-card p-1 text-sm text-fg tabular-nums shadow-md outline-none";

/**
 * One row in a `menuPanel`. Concentric with the panel: its 16px radius less
 * the 1px border and 4px padding.
 */
export const menuItem = "flex h-7 cursor-default items-center gap-1.5 rounded-[11px] select-none";
