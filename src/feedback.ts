import { invoke } from "@tauri-apps/api/core";

export type FeedbackKind = "bug" | "feature";

/** Sends a bug report or feature request to PostHog; rejects with the reason if it didn't arrive. */
export function sendFeedback(kind: FeedbackKind, message: string): Promise<void> {
  return invoke<void>("send_feedback", { kind, message });
}
