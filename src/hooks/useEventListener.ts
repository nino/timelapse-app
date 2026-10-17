import React from "react";

/**
 * Listens for `type` on `window`, or on `document` when `target` says so,
 * for as long as the component is mounted. The latest `listener` is always
 * called, so an inline function is fine and the listener is never swapped.
 */
export function useEventListener<K extends keyof WindowEventMap>(
  type: K,
  listener: (event: WindowEventMap[K]) => void,
): void;
export function useEventListener<K extends keyof DocumentEventMap>(
  type: K,
  listener: (event: DocumentEventMap[K]) => void,
  target: "document",
): void;
export function useEventListener(
  type: string,
  listener: (event: Event) => void,
  target: "window" | "document" = "window",
): void {
  const onEvent = React.useEffectEvent(listener);

  React.useEffect(() => {
    const on = target === "document" ? document : window;
    const handle = (event: Event): void => onEvent(event);
    on.addEventListener(type, handle);
    return (): void => on.removeEventListener(type, handle);
  }, [type, target]);
}
