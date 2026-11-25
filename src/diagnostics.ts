import { invoke } from "@tauri-apps/api/core";

function describe(value: unknown): { message: string; detail: string | null } {
  if (value instanceof Error) {
    return { message: value.message || value.name, detail: value.stack ?? null };
  }
  return { message: String(value), detail: null };
}

// Sends errors nothing else caught to the diagnostics log in the library
// (`diagnostics.db`), next to what the background work logs.
export function logUncaughtErrors(): void {
  window.addEventListener("error", (event: ErrorEvent): void => {
    const { message, detail } = describe(event.error ?? event.message);
    void invoke("log_frontend_error", { message, detail }).catch((): void => {});
  });
  window.addEventListener("unhandledrejection", (event: PromiseRejectionEvent): void => {
    const { message, detail } = describe(event.reason);
    void invoke("log_frontend_error", {
      message: `Unhandled rejection: ${message}`,
      detail,
    }).catch((): void => {});
  });
}

// How long frames took from being asked for to being on screen, sent to the
// scrub log (`scrub_log.rs`) in batches rather than one call per frame:
// `<img>` frames, and the beta video player's seeks.
let framesShown: Array<number> = [];
let videoSeeks: Array<number> = [];
let framesShownTimer: ReturnType<typeof setTimeout> | null = null;

/**
 * `performance.now()`, for timing a frame load. Only the scrub log reads it,
 * never what a component renders, so taking it while rendering is harmless.
 */
export function timestamp(): number {
  return performance.now();
}

export function noteFrameShown(milliseconds: number): void {
  framesShown.push(milliseconds);
  scheduleReport();
}

/** How long a seek of the beta video player took, from asking to showing its frame. */
export function noteVideoSeek(milliseconds: number): void {
  videoSeeks.push(milliseconds);
  scheduleReport();
}

function scheduleReport(): void {
  framesShownTimer ??= setTimeout((): void => {
    const batch = { milliseconds: framesShown, seeks: videoSeeks };
    framesShown = [];
    videoSeeks = [];
    framesShownTimer = null;
    void invoke("report_frames_shown", batch).catch((): void => {});
  }, 1000);
}
