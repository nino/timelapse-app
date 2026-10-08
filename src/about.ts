/** Label of the About window. Mirrors `ABOUT_WINDOW` in Rust. */
export const ABOUT_WINDOW = "about";

/** One change in the About window's change log (`scripts/changelog.ts`). */
export type ChangelogEntry = {
  /** The version that first shipped the change, e.g. "0.1.182". */
  version: string;
  /** When it reached main, as YYYY-MM-DD. */
  date: string;
  /** The pull request's title. */
  title: string;
  /** The pull request's number. */
  pr: number;
};

/** The entries grouped by day, keeping their order (newest first). */
export function byDay(entries: ChangelogEntry[]): { date: string; entries: ChangelogEntry[] }[] {
  const days: { date: string; entries: ChangelogEntry[] }[] = [];
  for (const entry of entries) {
    const last = days.at(-1);
    if (last?.date === entry.date) last.entries.push(entry);
    else days.push({ date: entry.date, entries: [entry] });
  }
  return days;
}

/** "2026-10-08" as "8 October 2026", whatever the time zone. */
export function formatDay(date: string): string {
  const [year, month, day] = date.split("-").map(Number);
  return new Date(Date.UTC(year, month - 1, day)).toLocaleDateString("en-GB", {
    day: "numeric",
    month: "long",
    year: "numeric",
    timeZone: "UTC",
  });
}
