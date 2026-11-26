import React from "react";

import { useTimeout } from "./useTimeout";

/** `value`, once it has stopped changing for `delayMs`. */
export function useDebounced(value: string, delayMs: number): string {
  const [settled, setSettled] = React.useState(value);
  useTimeout(() => setSettled(value), value === settled ? null : delayMs, value);
  return settled;
}
