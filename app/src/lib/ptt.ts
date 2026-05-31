import { type UnlistenFn, listen } from "@tauri-apps/api/event";
import { type DictationEvent, cancelDictation, startDictation, stopDictation } from "./dictation";

type Intent = "start" | "stop" | "cancel";

/**
 * Wire the global push-to-talk hotkey: the Rust side emits `dictation://intent`
 * (driven by the tested reducer); we drive the orchestrator. push-to-hold:
 * down → start (open capture), release → stop (transcribe + inject); Esc → cancel.
 * The single active flag here mirrors the reducer's re-entry guard. A session ends
 * only on an explicit user action (release / 2nd toggle press), never on silence.
 */
export async function installPtt(onEvent: (e: DictationEvent) => void): Promise<UnlistenFn> {
  let active = false;
  const reportError = (e: unknown) => {
    onEvent({ kind: "error", message: String(e) });
  };
  const stop = () => {
    if (!active) return;
    active = false;
    void stopDictation(onEvent).catch(reportError);
  };
  const unIntent = await listen<Intent>("dictation://intent", ({ payload }) => {
    if (payload === "start") {
      if (active) return;
      active = true;
      void startDictation(onEvent).catch((e) => {
        active = false;
        reportError(e);
      });
    } else if (payload === "stop") {
      stop();
    } else {
      active = false;
      void cancelDictation(onEvent).catch(reportError);
    }
  });
  return unIntent;
}
