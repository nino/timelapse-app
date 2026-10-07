// Class strings from the Ninoes design system (github.com/nino/ninoes,
// app/components/ui/styles.ts), trimmed to what this app uses.

export const focusRing =
  "outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50";

/** A text field's frame. Goes on a wrapper `<label>` when the field has an icon. */
export const fieldFrame =
  "rounded-xl border border-input bg-field transition-[color,box-shadow] focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50";

/** One button in a joined group: the group draws the border, and each button rounds its outer corners. */
export const segmentButton = `inline-flex w-9 items-center justify-center transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50 [&_svg]:shrink-0 ${focusRing}`;
