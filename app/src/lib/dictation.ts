import { Channel, invoke } from "@tauri-apps/api/core";

// Typed mirror of Rust `dictation::{Phase, DictationEvent, DictationResult}`.
// The UI forwards trigger intent in and renders HUD state out — no dictation logic.

export type Phase = "idle" | "listening" | "transcribing" | "inserting" | "error";
export type CancelReason = "userEscape" | "emptySpeech" | "timeout";

/** HUD/orchestrator events streamed over the Channel; discriminated by `kind`. */
export type DictationEvent =
  | { kind: "stateChanged"; phase: Phase }
  | { kind: "level"; rms: number }
  | { kind: "injected"; chars: number; ms: number }
  | { kind: "cancelled"; reason: CancelReason }
  | { kind: "error"; message: string };

export interface DictationResult {
  injectedChars: number;
  detectedLanguage: string | null;
  totalMs: number;
  sttMs: number;
  backend: string;
}

/** Begin a session (open mic capture, HUD → listening); push-to-hold or toggle. */
export function startDictation(onEvent: (e: DictationEvent) => void): Promise<void> {
  const events = new Channel<DictationEvent>();
  events.onmessage = onEvent;
  return invoke<void>("start_dictation", { events });
}

/** End capture and run STT → cleanup → dictionary → snippets → inject. */
export function stopDictation(onEvent: (e: DictationEvent) => void): Promise<void> {
  const events = new Channel<DictationEvent>();
  return new Promise((resolve, reject) => {
    let settled = false;
    const settle = () => {
      if (!settled) {
        settled = true;
        resolve();
      }
    };
    events.onmessage = (event) => {
      onEvent(event);
      if (event.kind === "injected" || event.kind === "cancelled" || event.kind === "error") {
        settle();
      }
    };
    invoke<void>("stop_dictation", { events }).catch((error) => {
      if (!settled) {
        settled = true;
        reject(error);
      }
    });
  });
}

/** Abort the in-flight session; inject nothing, HUD → idle. */
export function cancelDictation(onEvent: (e: DictationEvent) => void): Promise<void> {
  const events = new Channel<DictationEvent>();
  events.onmessage = onEvent;
  return invoke<void>("cancel_dictation", { events });
}
