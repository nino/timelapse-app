import React from "react";

/**
 * Calls `callback` once, `delayMs` after `delayMs` or `key` last changed, or
 * not at all while `delayMs` is null. A change to either before then starts
 * the wait over, so passing the watched value as `key` debounces it. The
 * latest `callback` is the one called, so an inline function is fine.
 */
export function useTimeout(
  callback: () => void,
  delayMs: number | null,
  key: string | number | boolean | null = null,
): void {
  const onTimeout = React.useEffectEvent(callback);

  React.useEffect(() => {
    if (delayMs === null) return;
    const timer = setTimeout(() => onTimeout(), delayMs);
    return (): void => clearTimeout(timer);
  }, [delayMs, key]);
}
