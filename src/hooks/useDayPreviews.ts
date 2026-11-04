import React from "react";

import { hasVideo as hasVideoFrames, type Day } from "../frames";
import {
  getDayPreviews,
  imageUrl,
  nearest,
  previewPackUrl,
  readPack,
  releasePack,
  type LoadedPack,
} from "../previews";
import { useLatestLoad } from "./useDirectoryChanges";
import { useFramesDecoded } from "./useFramesDecoded";

/** Looks up the preview nearest a frame of the day. */
export type DayPreviewLookup = {
  /** Object URL of the preview nearest frame `index`, or null if its video has none. */
  at: (index: number) => string | null;
};

type PlacedPack = { firstIndex: number; frameCount: number; pack: LoadedPack };

/**
 * Packs fetched so far, by name. Only the open day's are kept: loading a day
 * releases the others. A pack's name changes when it does (a full pack
 * replaces a keyframe one), so a fetched pack is never stale.
 */
const packs = new Map<string, Promise<LoadedPack | null>>();

function fetchPack(name: string): Promise<LoadedPack | null> {
  let pack = packs.get(name);
  if (pack === undefined) {
    pack = fetch(previewPackUrl(name))
      .then(async (response) => (response.ok ? readPack(name, await response.blob()) : null))
      .catch((error: unknown) => {
        console.error(`Error loading previews ${name}:`, error);
        return null;
      })
      .then((loaded) => {
        // Try again next time rather than remembering the failure.
        if (loaded === null) packs.delete(name);
        return loaded;
      });
    packs.set(name, pack);
  }
  return pack;
}

/** Forget every pack not in `keep`, revoking its image URLs. */
function releaseExcept(keep: ReadonlySet<string>): void {
  for (const [name, pack] of packs) {
    if (keep.has(name)) continue;
    packs.delete(name);
    void pack.then((loaded) => loaded && releasePack(loaded));
  }
}

/**
 * The open day's preview images, loaded once into memory (a few MB per hour
 * of video), for showing while the scrubber is dragged without asking Rust
 * for anything per position. Days served only from screenshots have none.
 *
 * Re-asked when the day's frame count changes and when the frame source
 * reports building previews for it; packs already loaded are reused.
 */
export function useDayPreviews(day: Day | null): DayPreviewLookup | null {
  const [state, setState] = React.useState<{ date: string; packs: Array<PlacedPack> } | null>(null);
  const date = day?.date ?? null;
  const frameCount = day?.frameCount ?? 0;
  const hasVideo = hasVideoFrames(day);
  const built = useFramesDecoded(date, "previews");

  const load = React.useCallback(
    async (isCurrent: () => boolean): Promise<void> => {
      if (!date) return;
      if (!hasVideo) {
        // A day of screenshots only: nothing to keep in memory.
        releaseExcept(new Set());
        return;
      }
      try {
        const previews = await getDayPreviews(date);
        if (!isCurrent()) return;
        const placed = await Promise.all(
          previews.packs.map(async ({ name, firstIndex, frameCount: count }) => {
            const pack = await fetchPack(name);
            return pack && { firstIndex, frameCount: count, pack };
          }),
        );
        if (!isCurrent()) return;
        setState({ date, packs: placed.filter((p): p is PlacedPack => p !== null) });
        // Only now that the state no longer points at them: until then,
        // looking a preview up in a released pack would make it a URL that
        // is never revoked.
        releaseExcept(new Set(previews.packs.map((pack) => pack.name)));
      } catch (error) {
        console.error("Error fetching previews:", error);
      }
    },
    // `frameCount` and `built` aren't read, but each change means the
    // answer may have changed, so it is asked again.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [date, hasVideo, frameCount, built],
  );
  useLatestLoad(load);

  const current = hasVideo && state?.date === date ? state.packs : null;
  return React.useMemo(
    (): DayPreviewLookup | null =>
      current === null || current.length === 0
        ? null
        : {
            at: (index: number): string | null => {
              const placed = current.find(
                ({ firstIndex, frameCount: count }) => firstIndex <= index && index < firstIndex + count,
              );
              if (!placed || placed.pack.indices.length === 0) return null;
              const i = nearest(placed.pack.indices, index - placed.firstIndex);
              return imageUrl(placed.pack, i);
            },
          },
    [current],
  );
}
