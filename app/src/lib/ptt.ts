import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { cancelDictation, type DictationEvent, startDictation, stopDictation } from "./dictation";

type Intent = "start" | "stop" | "cancel";

/**
 * Wire the global push-to-talk hotkey: the Rust side emits `dictation://intent`
 * (driven by the tested reducer); we drive the orchestrator. push-to-hold:
 * down → start (open capture), release → stop (transcribe + inject); Esc → cancel.
 * The single active flag here mirrors the reducer's re-entry guard.
 */
export function installPtt(onEvent: (e: DictationEvent) => void): Promise<UnlistenFn> {
  let active = false;
  return listen<Intent>("dictation://intent", ({ payload }) => {
    if (payload === "start") {
      if (active) return;
      active = true;
      void startDictation(onEvent);
    } else if (payload === "stop") {
      if (!active) return;
      active = false;
      void stopDictation(onEvent);
    } else {
      active = false;
      void cancelDictation(onEvent);
    }
  });
}
