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
