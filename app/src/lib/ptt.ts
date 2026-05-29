import { type UnlistenFn, listen } from "@tauri-apps/api/event";
import { type DictationEvent, cancelDictation, startDictation, stopDictation } from "./dictation";

type Intent = "start" | "stop" | "cancel";

/**
 * Wire the global push-to-talk hotkey: the Rust side emits `dictation://intent`
 * (driven by the tested reducer); we drive the orchestrator. push-to-hold:
 * down → start (open capture), release → stop (transcribe + inject); Esc → cancel.
 * The single active flag here mirrors the reducer's re-entry guard.
 *
 * In toggle mode the engine may also emit `dictation://auto-endpoint` after sustained
 * silence (audio-capture.md §5) — we treat it exactly like a 2nd toggle press (stop),
 * sharing the same `active` guard so it can't double-stop.
 */
export async function installPtt(onEvent: (e: DictationEvent) => void): Promise<UnlistenFn> {
  let active = false;
  const stop = () => {
    if (!active) return;
    active = false;
    void stopDictation(onEvent);
  };
  const unIntent = await listen<Intent>("dictation://intent", ({ payload }) => {
    if (payload === "start") {
      if (active) return;
      active = true;
      void startDictation(onEvent);
    } else if (payload === "stop") {
      stop();
    } else {
      active = false;
      void cancelDictation(onEvent);
    }
  });
  const unAuto = await listen("dictation://auto-endpoint", stop);
  return () => {
    unIntent();
    unAuto();
  };
}
