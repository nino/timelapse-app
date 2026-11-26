import React from "react";

import { ensureError } from "../errors";
import { useLatestLoad } from "./useLatestLoad";

type Answer<T> = { key: string; value: T | null; error: Error | null };

/**
 * Run `ask` for `key`, and again whenever `version` changes. Returns null
 * while a new key loads, never the previous key's answer. A repeat that finds
 * nothing new keeps the old value, and one that fails keeps the last good
 * answer rather than wiping it. `onError` hears about every failed ask that
 * is still current.
 */
export function useAnswer<T>(
  key: string | null,
  version: string | null,
  ask: () => Promise<T>,
  onError?: (error: Error) => void,
): { value: T | null; error: Error | null } {
  const [answer, setAnswer] = React.useState<Answer<T>>({ key: "", value: null, error: null });

  const load = React.useCallback(
    async (isCurrent: () => boolean): Promise<void> => {
      if (key === null || version === null) return;
      try {
        const value = await ask();
        if (!isCurrent()) return;
        setAnswer((previous) =>
          previous.key === key && JSON.stringify(previous.value) === JSON.stringify(value)
            ? previous
            : { key, value, error: null },
        );
      } catch (error) {
        if (!isCurrent()) return;
        onError?.(ensureError(error));
        setAnswer((previous) =>
          previous.key === key && previous.value !== null
            ? previous
            : { key, value: null, error: ensureError(error) },
        );
      }
    },
    // `ask` and `onError` are rebuilt every render; `key` says everything
    // they depend on.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [key, version],
  );
  useLatestLoad(load);

  const current = key !== null && answer.key === key;
  return {
    value: current ? answer.value : null,
    error: current ? answer.error : null,
  };
}
