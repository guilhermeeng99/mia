import { invoke } from "@tauri-apps/api/core";

// Typed mirror of Rust `hotkey::{HotkeyConfig, ActivationMode}` (serde camelCase).

export type ActivationMode = "pushToHold" | "pressToToggle";

export interface HotkeyConfig {
  accelerator: string;
  mode: ActivationMode;
}

export interface HotkeyRecordSample {
  accelerator: string | null;
  released: boolean;
  cancelled: boolean;
}

/** The active PTT chord + mode. */
export function getHotkey(): Promise<HotkeyConfig> {
  return invoke<HotkeyConfig>("get_hotkey");
}

/** Runtime-only unregister + register. Settings UI persists hotkeys through updateSettings. */
export function updateHotkey(cfg: HotkeyConfig): Promise<void> {
  return invoke<void>("update_hotkey", { cfg });
}

/** Runtime-only suspension. Used while recording a replacement chord in the Hub. */
export function unregisterHotkey(): Promise<void> {
  return invoke<void>("unregister_hotkey");
}

/** Native key sampler used by the recorder so Win-key chords are visible to Rust. */
export function sampleHotkeyRecording(): Promise<HotkeyRecordSample> {
  return invoke<HotkeyRecordSample>("sample_hotkey_recording");
}

const MAC_KEY_LABELS: Record<string, string> = {
  Ctrl: "⌃ Control",
  Alt: "⌥ Option",
  Shift: "⇧ Shift",
  Super: "⌘ Cmd",
};

/**
 * Human label for a canonical accelerator. The engine stores Windows-style names
 * (`Alt`, `Super`); on macOS show the keys as printed on a Mac keyboard.
 */
export function formatAccelerator(accelerator: string): string {
  if (!/Mac/.test(navigator.userAgent)) return accelerator;
  return accelerator
    .split("+")
    .map((part) => MAC_KEY_LABELS[part] ?? part)
    .join(" + ");
}
