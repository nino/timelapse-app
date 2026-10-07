// Class strings from the Ninoes design system (github.com/nino/ninoes,
// app/components/ui/styles.ts), trimmed to what this app uses.

export const focusRing =
  "outline-none focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:shadow-glow";

export const outlineButton = `inline-flex shrink-0 items-center justify-center gap-2 rounded-md border border-border bg-card font-medium shadow-xs transition-colors hover:bg-muted disabled:pointer-events-none disabled:opacity-50 [&_svg]:shrink-0 ${focusRing}`;

/** A text field's frame. Goes on a wrapper `<label>` when the field has an icon. */
export const fieldFrame =
  "rounded-xl border border-input bg-field shadow-xs transition-[color,box-shadow] focus-within:border-ring focus-within:ring-[3px] focus-within:ring-ring/50 focus-within:shadow-glow";
