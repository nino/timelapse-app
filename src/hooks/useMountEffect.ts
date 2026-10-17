import React from "react";

/**
 * Runs `effect` once after the component mounts, and its cleanup once on
 * unmount. This is the escape hatch for the `useEffect` ban: anything that
 * would need dependencies is derived state, an event handler, a ref
 * callback, or one of the other effect hooks in this folder.
 */
export function useMountEffect(effect: React.EffectCallback): void {
  // eslint-disable-next-line react-hooks/exhaustive-deps -- mount-only by design
  React.useEffect(effect, []);
}
