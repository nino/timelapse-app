import React from "react";

import { onFramesDecoded, onPreviewsBuilt } from "../frames";
import { useTauriEvent } from "./useTauriEvent";

/**
 * How many times since this mounted the frame source has reported, for
 * `date`, decoding part of it ahead of the viewer (`"decoded"`) or building
 * previews of one of its videos (`"previews"`). Counted per day, so another
 * day's reports don't change it; a hook that depends on it re-asks only
 * about the day it shows.
 */
export function useFramesDecoded(date: string | null, what: "decoded" | "previews"): number {
  const [counts, setCounts] = React.useState<ReadonlyMap<string, number>>(() => new Map());
  useTauriEvent(
    what === "decoded" ? onFramesDecoded : onPreviewsBuilt,
    (decoded: string) => {
      setCounts((current) => new Map(current).set(decoded, (current.get(decoded) ?? 0) + 1));
    },
    what === "decoded" ? "decoded frames" : "built previews",
  );
  return date === null ? 0 : (counts.get(date) ?? 0);
}
